//! Lecteur vidéo basé sur GStreamer ; la vidéo est intégrée à la fenêtre Ferrite et le son
//! passe par la sortie audio système.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use eframe::egui::{self, Color32, Key, TextureHandle, TextureOptions};
use gst::prelude::*;
use gstreamer as gst;

use crate::{
    files::{FilesState, PickerMode},
    style,
};

/// Saut des boutons « 10 s » et des flèches.
const SEEK_STEP: f64 = 10.0;
const REFRESH: Duration = Duration::from_millis(250);
/// Commandes masquées après ce délai sans mouvement de souris, pendant la lecture.
const CONTROLS_IDLE: Duration = Duration::from_millis(2500);
/// Durée d'affichage de la bulle centrale (« + 10 s », « Volume 70 % »…).
const OSD_TIME: Duration = Duration::from_millis(900);
/// Vitesses proposées, dans l'ordre du bouton.
const SPEEDS: [f64; 5] = [1.0, 1.25, 1.5, 2.0, 0.75];
const SIDEBAR: f32 = 320.0;

// Palette de la maquette « Lecteur vidéo » (toujours sombre, comme un écran de cinéma).
const SIDEBAR_FILL: Color32 = Color32::from_rgb(0x15, 0x18, 0x1D);
const SIDEBAR_LINE: Color32 = Color32::from_rgb(0x2A, 0x2E, 0x35);
const MUTED: Color32 = Color32::from_rgb(0x8B, 0x93, 0xA0);
const CURRENT: Color32 = Color32::from_rgb(0x9C, 0xC7, 0xFF);
const SWITCH_ON: Color32 = Color32::from_rgb(0x27, 0x79, 0xE0);
const SWITCH_OFF: Color32 = Color32::from_rgb(0x3A, 0x3F, 0x47);
const INK: Color32 = Color32::from_rgb(0x11, 0x18, 0x27);

struct Frame {
    image: egui::ColorImage,
}

pub struct VideoState {
    ctx: egui::Context,
    path: PathBuf,
    pipeline: Option<gst::Element>,
    bus: Option<gst::Bus>,
    frame: Arc<Mutex<Option<Frame>>>,
    texture: Option<TextureHandle>,
    icons: style::Icons,
    queue: Vec<PathBuf>,
    position: f64,
    duration: f64,
    seeking: Option<f64>,
    error: Option<String>,
    picker: Option<FilesState>,
    playing: bool,
    muted: bool,
    /// Volume linéaire de 0 à 1.
    volume: f64,
    speed: f64,
    show_list: bool,
    autoplay: bool,
    osd: Option<(String, Instant)>,
    /// Dernier mouvement de la souris : les commandes restent affichées un moment après.
    last_motion: Instant,
    last_pointer: Option<egui::Pos2>,
    /// Ouverture de la vidéo en cours : les commandes s'affichent un moment au début.
    opened: Instant,
}

impl VideoState {
    fn refresh_queue(&mut self) {
        self.queue.clear();
        let Some(parent) = self.path.parent() else {
            return;
        };
        match std::fs::read_dir(parent) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) if supported(&entry.path()) => self.queue.push(entry.path()),
                        Ok(_) => {}
                        Err(error) => {
                            tracing::warn!(path = %parent.display(), %error, "fichier vidéo ignoré")
                        }
                    }
                }
                self.queue.sort();
                if !self.queue.contains(&self.path) {
                    self.queue.push(self.path.clone());
                    self.queue.sort();
                }
            }
            Err(error) => {
                tracing::warn!(path = %parent.display(), %error, "liste vidéo indisponible");
                self.queue.push(self.path.clone());
            }
        }
    }

    pub fn new(path: Option<PathBuf>, ctx: egui::Context) -> Self {
        let frame = Arc::new(Mutex::new(None));
        let mut state = Self {
            ctx,
            path: path.unwrap_or_default(),
            pipeline: None,
            bus: None,
            frame,
            texture: None,
            icons: Default::default(),
            queue: Vec::new(),
            position: 0.0,
            duration: 0.0,
            seeking: None,
            error: None,
            picker: None,
            playing: false,
            muted: false,
            volume: 1.0,
            speed: 1.0,
            show_list: true,
            autoplay: true,
            osd: None,
            last_motion: Instant::now(),
            last_pointer: None,
            opened: Instant::now(),
        };
        if !state.path.as_os_str().is_empty() {
            let path = state.path.clone();
            state.open(path);
        }
        state
    }

    pub fn open(&mut self, path: PathBuf) {
        self.stop();
        self.path = path;
        self.refresh_queue();
        self.position = 0.0;
        self.duration = 0.0;
        self.seeking = None;
        self.texture = None;
        self.error = None;
        self.playing = false;
        self.speed = 1.0;
        self.opened = Instant::now();
        match self.frame.lock() {
            Ok(mut frame) => *frame = None,
            Err(error) => {
                self.error = Some(format!("Mémoire vidéo indisponible : {error}"));
                return;
            }
        }
        if let Err(error) = self.create_pipeline() {
            self.error = Some(error);
        }
    }

    fn create_pipeline(&mut self) -> Result<(), String> {
        gst::init().map_err(|error| format!("Initialisation vidéo impossible : {error}"))?;

        let video_sink = gstreamer_app::AppSink::builder()
            .caps(&gst::Caps::builder("video/x-raw").field("format", "RGBA").build())
            .max_buffers(1)
            .drop(true)
            .build();

        let frame = self.frame.clone();
        let context = self.ctx.clone();
        video_sink.set_callbacks(
            gstreamer_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let caps = sample.caps().ok_or(gst::FlowError::NotNegotiated)?;
                    let info = gstreamer_video::VideoInfo::from_caps(caps)
                        .map_err(|_| gst::FlowError::NotNegotiated)?;
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let mapped = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                    let (width, height) = (info.width() as usize, info.height() as usize);
                    let stride = usize::try_from(info.stride()[0]).map_err(|_| gst::FlowError::Error)?;
                    let row_bytes = width.checked_mul(4).ok_or(gst::FlowError::Error)?;
                    let total_bytes = row_bytes.checked_mul(height).ok_or(gst::FlowError::Error)?;
                    let mut rgba = Vec::with_capacity(total_bytes);
                    for row in 0..height {
                        let start = row.checked_mul(stride).ok_or(gst::FlowError::Error)?;
                        let end = start.checked_add(row_bytes).ok_or(gst::FlowError::Error)?;
                        rgba.extend_from_slice(
                            mapped.as_slice().get(start..end).ok_or(gst::FlowError::Error)?,
                        );
                    }
                    let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                    *frame.lock().map_err(|_| gst::FlowError::Error)? = Some(Frame { image });
                    context.request_repaint();
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        let pipeline = gst::ElementFactory::make("playbin")
            .build()
            .map_err(|error| format!("Lecteur GStreamer indisponible : {error}"))?;
        pipeline.set_property("uri", file_uri(&self.path)?);
        pipeline.set_property("video-sink", &video_sink);
        pipeline.set_property("volume", self.volume);
        pipeline.set_property("mute", self.muted);
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| format!("Ouverture vidéo impossible : {error}"))?;
        self.bus = Some(pipeline.bus().ok_or("Bus vidéo indisponible")?);
        self.pipeline = Some(pipeline);
        self.playing = true;
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(pipeline) = self.pipeline.take()
            && let Err(error) = pipeline.set_state(gst::State::Null)
        {
            tracing::warn!(%error, "arrêt du lecteur vidéo impossible");
        }
        self.bus = None;
    }

    fn toggle_play(&mut self) {
        let Some(pipeline) = &self.pipeline else {
            return;
        };
        let next = if self.playing {
            gst::State::Paused
        } else {
            gst::State::Playing
        };
        match pipeline.set_state(next) {
            Ok(_) => self.playing = !self.playing,
            Err(error) => self.error = Some(format!("Commande de lecture impossible : {error}")),
        }
    }

    fn seek(&mut self, seconds: f64) {
        let seconds = seconds.clamp(0.0, self.duration.max(0.0));
        if self.apply_rate(seconds, self.speed) {
            self.position = seconds;
            self.seeking = None;
        }
    }

    /// Déplacement à `seconds` à la vitesse `rate` (le même appel change l'un ou l'autre).
    fn apply_rate(&mut self, seconds: f64, rate: f64) -> bool {
        let Some(pipeline) = &self.pipeline else {
            return false;
        };
        let position = gst::ClockTime::from_nseconds((seconds.max(0.0) * 1_000_000_000.0) as u64);
        match pipeline.seek(
            rate,
            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            gst::SeekType::Set,
            position,
            gst::SeekType::None,
            gst::ClockTime::NONE,
        ) {
            Ok(()) => true,
            Err(error) => {
                self.error = Some(format!("Déplacement dans la vidéo impossible : {error}"));
                false
            }
        }
    }

    fn step(&mut self, delta: f64) {
        self.seek(self.position + delta);
        self.flash(if delta < 0.0 { "− 10 s" } else { "+ 10 s" });
    }

    fn cycle_speed(&mut self) {
        let index = SPEEDS.iter().position(|speed| *speed == self.speed).unwrap_or(0);
        let next = SPEEDS[(index + 1) % SPEEDS.len()];
        if self.apply_rate(self.position, next) {
            self.speed = next;
            self.flash(&format!("Vitesse {}", speed_label(next)));
        }
    }

    fn set_volume(&mut self, volume: f64) {
        self.volume = volume.clamp(0.0, 1.0);
        self.muted = false;
        if let Some(pipeline) = &self.pipeline {
            pipeline.set_property("volume", self.volume);
            pipeline.set_property("mute", false);
        }
        self.flash(&format!("Volume {} %", (self.volume * 100.0).round()));
    }

    fn next(&self) -> Option<PathBuf> {
        let index = self.queue.iter().position(|path| *path == self.path)?;
        self.queue.get(index + 1).cloned()
    }

    fn play_next(&mut self) {
        if let Some(path) = self.next() {
            self.open(path);
        }
    }

    fn flash(&mut self, text: &str) {
        self.osd = Some((text.to_owned(), Instant::now()));
    }

    fn update_status(&mut self) {
        let mut ended = false;
        if let Some(bus) = self.bus.clone() {
            while let Some(message) = bus.timed_pop(gst::ClockTime::ZERO) {
                match message.view() {
                    gst::MessageView::Error(error) => {
                        self.error = Some(format!(
                            "{} : {}",
                            error.error(),
                            error.debug().unwrap_or_default()
                        ));
                        self.playing = false;
                    }
                    gst::MessageView::Eos(..) => {
                        self.playing = false;
                        ended = true;
                    }
                    // Décodeur absent : GStreamer continue avec le son seul et ne signale
                    // pas d'erreur ; sans ce message, l'image resterait en « Préparation ».
                    gst::MessageView::Element(element) => {
                        if let Some(format) = element
                            .structure()
                            .filter(|structure| structure.name() == "missing-plugin")
                            .and_then(|structure| structure.get::<gst::Caps>("detail").ok())
                            .and_then(|caps| caps.structure(0).and_then(|s| missing_video_format(s.name())))
                        {
                            tracing::warn!(%format, "décodeur vidéo absent");
                            self.error = Some(format!(
                                "Le décodeur du format {format} n'est pas installé sur ce PC."
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        if ended && self.autoplay && self.next().is_some() {
            self.play_next();
            return;
        }
        if ended {
            if let Some(pipeline) = &self.pipeline
                && let Err(error) = pipeline.set_state(gst::State::Paused)
            {
                self.error = Some(format!("Pause vidéo impossible : {error}"));
            }
            self.seek(0.0);
        }
        if let Some(pipeline) = &self.pipeline {
            self.position = pipeline
                .query_position::<gst::ClockTime>()
                .map_or(self.position, |position| {
                    position.nseconds() as f64 / 1_000_000_000.0
                });
            self.duration = pipeline
                .query_duration::<gst::ClockTime>()
                .map_or(self.duration, |duration| {
                    duration.nseconds() as f64 / 1_000_000_000.0
                });
        }

        if let Ok(mut latest) = self.frame.lock()
            && let Some(frame) = latest.take()
        {
            self.texture = Some(match self.texture.take() {
                Some(mut texture) => {
                    texture.set(frame.image, TextureOptions::LINEAR);
                    texture
                }
                None => self.ctx.load_texture(
                    format!("vidéo:{}", self.path.display()),
                    frame.image,
                    TextureOptions::LINEAR,
                ),
            });
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || self.picker.is_some() {
            return;
        }
        let pressed = |key| ctx.input(|input| input.key_pressed(key));
        if pressed(Key::Space) || pressed(Key::K) {
            self.toggle_play_flash();
        }
        if pressed(Key::ArrowRight) || pressed(Key::L) {
            self.step(SEEK_STEP);
        }
        if pressed(Key::ArrowLeft) || pressed(Key::J) {
            self.step(-SEEK_STEP);
        }
        if pressed(Key::ArrowUp) {
            self.set_volume(self.volume + 0.05);
        }
        if pressed(Key::ArrowDown) {
            self.set_volume(self.volume - 0.05);
        }
        if pressed(Key::M) {
            self.toggle_mute();
        }
        if pressed(Key::N) {
            self.play_next();
        }
        if ctx.input(|input| input.modifiers.command && input.key_pressed(Key::O)) {
            self.open_picker();
        }
        if pressed(Key::F) {
            toggle_fullscreen(ctx);
        }
        if pressed(Key::Escape) && ctx.input(|input| input.viewport().fullscreen.unwrap_or(false)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
    }

    fn toggle_play_flash(&mut self) {
        self.toggle_play();
        self.flash(if self.playing { "Lecture" } else { "Pause" });
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if let Some(pipeline) = &self.pipeline {
            pipeline.set_property("mute", self.muted);
        }
        self.flash(if self.muted { "Son coupé" } else { "Son rétabli" });
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        self.update_status();
        self.keys(ui.ctx());
        let ctx = ui.ctx().clone();
        // Une image arrivée réveille déjà l'interface ; ce rappel fait avancer le temps
        // affiché et masque les commandes à l'heure.
        if self.playing || self.osd.is_some() {
            ctx.request_repaint_after(REFRESH);
        }
        if self.osd.as_ref().is_some_and(|(_, at)| at.elapsed() > OSD_TIME) {
            self.osd = None;
        }
        let wide = ui.available_width() > 760.0;
        if self.show_list && wide {
            egui::Panel::right(ui.scope_id().with("video-a-suivre"))
                .resizable(false)
                .exact_size(SIDEBAR)
                .frame(
                    egui::Frame::new()
                        .fill(SIDEBAR_FILL)
                        .stroke(egui::Stroke::new(1.0, SIDEBAR_LINE)),
                )
                .show(ui, |ui| self.sidebar(ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::BLACK))
            .show(ui, |ui| self.stage(ui));
        self.picker_sheet(&ctx);
    }

    /// Commandes affichées : à l'arrêt, au survol récent, ou pendant un réglage.
    fn controls_visible(&mut self, ui: &egui::Ui, area: egui::Rect) -> bool {
        let pointer = ui.input(|input| input.pointer.hover_pos());
        if pointer != self.last_pointer {
            self.last_pointer = pointer;
            self.last_motion = Instant::now();
        }
        let idle = self.last_motion.elapsed();
        let inside = pointer.is_some_and(|pointer| area.contains(pointer));
        let since_open = self.opened.elapsed();
        let visible = !self.playing
            || self.texture.is_none()
            || since_open < CONTROLS_IDLE
            || (inside && idle < CONTROLS_IDLE);
        if self.playing && visible {
            let left = CONTROLS_IDLE.saturating_sub(idle.min(since_open));
            ui.ctx()
                .request_repaint_after(left.max(Duration::from_millis(50)));
        }
        visible
    }

    /// La vidéo, et par-dessus le titre, le bouton de lecture et les commandes.
    fn stage(&mut self, ui: &mut egui::Ui) {
        let area = ui.max_rect();
        let painter = ui.painter().clone();
        if let Some(texture) = &self.texture {
            let size = texture.size_vec2();
            let scale = (area.width() / size.x).min(area.height() / size.y);
            let rect = egui::Rect::from_center_size(area.center(), size * scale);
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                if self.playing {
                    Color32::WHITE
                } else {
                    Color32::from_gray(205)
                },
            );
        } else if let Some(error) = self.error.clone() {
            message(&painter, area, "Lecture impossible", &error);
        } else if self.path.as_os_str().is_empty() {
            self.empty_state(ui, area);
            return;
        } else {
            message(&painter, area, "Préparation de la lecture…", "");
        }

        let visible = self.controls_visible(ui, area);
        // Clic sur l'image : lecture ou pause ; double-clic : plein écran.
        let surface = egui::Rect::from_min_max(
            area.min + egui::vec2(0.0, 60.0),
            area.max - egui::vec2(0.0, 140.0),
        );
        let response = ui.interact(surface, ui.scope_id().with("surface"), egui::Sense::click());
        if response.double_clicked() {
            toggle_fullscreen(ui.ctx());
        } else if response.clicked() {
            self.toggle_play_flash();
        }

        if !self.playing && self.texture.is_some() {
            let center = area.center() - egui::vec2(0.0, area.height() * 0.05);
            painter.circle_filled(center, 46.0, Color32::from_rgba_unmultiplied(20, 22, 26, 150));
            painter.circle_stroke(
                center,
                46.0,
                egui::Stroke::new(1.0, Color32::from_white_alpha(40)),
            );
            play_triangle(&painter, center + egui::vec2(3.0, 0.0), 15.0, Color32::WHITE);
        }
        if visible {
            self.top_bar(ui, area);
            self.control_panel(ui, area);
        }
        if let Some((text, _)) = &self.osd {
            let galley =
                painter.layout_no_wrap(text.clone(), egui::FontId::proportional(16.0), Color32::WHITE);
            let pill = egui::Rect::from_center_size(
                area.center() - egui::vec2(0.0, area.height() * 0.05),
                galley.size() + egui::vec2(40.0, 24.0),
            );
            painter.rect_filled(pill, 14.0, Color32::from_rgba_unmultiplied(20, 22, 26, 190));
            painter.galley(pill.center() - galley.size() / 2.0, galley, Color32::WHITE);
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        let painter = ui.painter();
        let icon =
            egui::Rect::from_center_size(area.center() - egui::vec2(0.0, 95.0), egui::vec2(84.0, 84.0));
        self.icons.paint(ui, "app-video", icon, style::IconStyle::Color);
        painter.text(
            area.center() - egui::vec2(0.0, 16.0),
            egui::Align2::CENTER_CENTER,
            "Votre cinéma, ici.",
            egui::FontId::proportional(24.0),
            Color32::WHITE,
        );
        painter.text(
            area.center() + egui::vec2(0.0, 14.0),
            egui::Align2::CENTER_CENTER,
            "Ouvrez une vidéo pour commencer la lecture.",
            egui::FontId::proportional(13.0),
            MUTED,
        );
        let button =
            egui::Rect::from_center_size(area.center() + egui::vec2(0.0, 68.0), egui::vec2(190.0, 40.0));
        if pill_button(ui, button, "Choisir une vidéo", true).clicked() {
            self.open_picker();
        }
    }

    /// Haut de l'image : dégradé, bouton Ouvrir, titre et fichier, liste « À suivre ».
    fn top_bar(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        let painter = ui.painter().clone();
        let band = egui::Rect::from_min_size(area.min, egui::vec2(area.width(), 120.0));
        gradient(
            &painter,
            band,
            Color32::from_black_alpha(150),
            Color32::TRANSPARENT,
        );
        let row = egui::Rect::from_min_size(area.min, egui::vec2(area.width(), 60.0));
        let title = self
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let file = self
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let room = area.width() - 300.0;
        painter.text(
            egui::pos2(row.center().x, row.center().y - 8.0),
            egui::Align2::CENTER_CENTER,
            truncate(&title, room, 15.0),
            egui::FontId::new(15.0, style::strong_family()),
            Color32::WHITE,
        );
        painter.text(
            egui::pos2(row.center().x, row.center().y + 11.0),
            egui::Align2::CENTER_CENTER,
            truncate(&file, room, 12.0),
            egui::FontId::proportional(12.0),
            Color32::from_white_alpha(180),
        );
        // Fond déplaçable d'abord, pour que les boutons restent prioritaires.
        style::drag_area(ui, row, "video-haut");
        style::window_buttons(ui, row);
        let open = egui::Rect::from_min_size(row.min + egui::vec2(14.0, 14.0), egui::vec2(78.0, 32.0));
        if pill_button(ui, open, "Ouvrir…", false)
            .on_hover_text("Ouvrir une vidéo (Ctrl+O)")
            .clicked()
        {
            self.open_picker();
        }
        if ui.available_width() > 760.0 || self.show_list {
            let list = egui::Rect::from_min_size(
                egui::pos2(row.right() - 144.0, row.top() + 13.0),
                egui::vec2(34.0, 34.0),
            );
            let response = icon_button(ui, list, self.show_list).on_hover_text("Liste « À suivre »");
            let painter = ui.painter();
            let (x, y) = (list.left() + 8.0, list.top() + 9.0);
            let stroke = egui::Stroke::new(1.8, Color32::WHITE);
            for (index, width) in [12.0, 12.0, 7.0].into_iter().enumerate() {
                let line_y = y + index as f32 * 6.0;
                painter.line_segment([egui::pos2(x, line_y), egui::pos2(x + width, line_y)], stroke);
            }
            play_triangle(painter, egui::pos2(x + 15.0, y + 13.0), 4.0, Color32::WHITE);
            if response.clicked() {
                self.show_list = !self.show_list;
            }
        }
    }

    /// Panneau de verre en bas : progression, puis boutons de lecture et réglages.
    fn control_panel(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        let painter = ui.painter().clone();
        let fade = egui::Rect::from_min_max(egui::pos2(area.left(), area.bottom() - 200.0), area.max);
        gradient(
            &painter,
            fade,
            Color32::TRANSPARENT,
            Color32::from_black_alpha(190),
        );
        let panel = egui::Rect::from_min_max(
            egui::pos2(area.left() + 24.0, area.bottom() - 20.0 - 104.0),
            egui::pos2(area.right() - 24.0, area.bottom() - 20.0),
        );
        painter.rect(
            panel,
            18.0,
            Color32::from_rgba_unmultiplied(22, 24, 28, 200),
            egui::Stroke::new(1.0, Color32::from_white_alpha(26)),
            egui::StrokeKind::Inside,
        );
        // Ligne 1 : temps, barre de progression, durée.
        let line_y = panel.top() + 24.0;
        let time_font = egui::FontId::monospace(12.0);
        let shown = self.seeking.unwrap_or(self.position);
        painter.text(
            egui::pos2(panel.left() + 16.0, line_y),
            egui::Align2::LEFT_CENTER,
            format_time(shown),
            time_font.clone(),
            Color32::from_white_alpha(217),
        );
        painter.text(
            egui::pos2(panel.right() - 16.0, line_y),
            egui::Align2::RIGHT_CENTER,
            format_time(self.duration),
            time_font,
            Color32::from_white_alpha(217),
        );
        let track = egui::Rect::from_min_max(
            egui::pos2(panel.left() + 72.0, line_y - 10.0),
            egui::pos2(panel.right() - 72.0, line_y + 10.0),
        );
        let response = ui.interact(
            track,
            ui.scope_id().with("progression"),
            egui::Sense::click_and_drag(),
        );
        if let Some(pointer) = response.interact_pointer_pos()
            && (response.dragged() || response.clicked())
            && self.duration > 0.0
        {
            let fraction = ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
            self.seeking = Some(fraction * self.duration);
            self.last_motion = Instant::now();
        }
        if (response.drag_stopped() || response.clicked())
            && let Some(position) = self.seeking.take()
        {
            self.seek(position);
        }
        let fraction = if self.duration > 0.0 {
            (self.seeking.unwrap_or(self.position) / self.duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        slider_track(&painter, track, fraction, Color32::from_white_alpha(51));
        if response.hovered() || response.dragged() || self.duration > 0.0 {
            let knob = egui::pos2(track.left() + track.width() * fraction, track.center().y);
            style::knob_shadow(&painter, knob, 7.0);
            painter.circle_filled(knob, 7.0, Color32::WHITE);
        }

        // Ligne 2 : boutons.
        let center_y = panel.bottom() - 34.0;
        let mut x = panel.left() + 12.0;
        let back = egui::Rect::from_center_size(egui::pos2(x + 20.0, center_y), egui::vec2(40.0, 40.0));
        if icon_button(ui, back, false)
            .on_hover_text("Reculer de 10 s (←)")
            .clicked()
        {
            self.step(-SEEK_STEP);
        }
        skip_icon(ui.painter(), back.center(), false);
        x += 44.0;
        let play = egui::Rect::from_center_size(egui::pos2(x + 24.0, center_y), egui::vec2(48.0, 48.0));
        let play_response = ui.interact(play, ui.scope_id().with("lecture"), egui::Sense::click());
        ui.painter().circle_filled(
            play.center(),
            24.0,
            if play_response.hovered() {
                Color32::from_gray(230)
            } else {
                Color32::WHITE
            },
        );
        if self.playing {
            for offset in [-4.0, 4.0] {
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(
                        play.center() + egui::vec2(offset, 0.0),
                        egui::vec2(4.0, 14.0),
                    ),
                    1.2,
                    INK,
                );
            }
        } else {
            play_triangle(ui.painter(), play.center() + egui::vec2(2.0, 0.0), 8.0, INK);
        }
        if play_response
            .on_hover_text(if self.playing {
                "Pause (Espace)"
            } else {
                "Lecture (Espace)"
            })
            .clicked()
        {
            self.toggle_play_flash();
        }
        x += 52.0;
        let forward = egui::Rect::from_center_size(egui::pos2(x + 20.0, center_y), egui::vec2(40.0, 40.0));
        if icon_button(ui, forward, false)
            .on_hover_text("Avancer de 10 s (→)")
            .clicked()
        {
            self.step(SEEK_STEP);
        }
        skip_icon(ui.painter(), forward.center(), true);
        x += 44.0;
        let has_next = self.next().is_some();
        let next = egui::Rect::from_center_size(egui::pos2(x + 20.0, center_y), egui::vec2(40.0, 40.0));
        if has_next {
            if icon_button(ui, next, false)
                .on_hover_text("Vidéo suivante (N)")
                .clicked()
            {
                self.play_next();
            }
            let c = next.center();
            play_triangle(ui.painter(), c + egui::vec2(-2.0, 0.0), 6.0, Color32::WHITE);
            ui.painter().line_segment(
                [c + egui::vec2(6.0, -6.0), c + egui::vec2(6.0, 6.0)],
                egui::Stroke::new(1.8, Color32::WHITE),
            );
            x += 44.0;
        }

        // Son : bouton et réglette.
        x += 10.0;
        let mute = egui::Rect::from_center_size(egui::pos2(x + 18.0, center_y), egui::vec2(36.0, 36.0));
        let silent = self.muted || self.volume <= 0.0;
        if icon_button(ui, mute, false)
            .on_hover_text(if silent {
                "Rétablir le son (M)"
            } else {
                "Couper le son (M)"
            })
            .clicked()
        {
            self.toggle_mute();
        }
        speaker_icon(ui.painter(), mute.center(), silent);
        x += 42.0;
        let volume_track = egui::Rect::from_min_size(egui::pos2(x, center_y - 10.0), egui::vec2(96.0, 20.0));
        let volume_response = ui.interact(
            volume_track,
            ui.scope_id().with("volume"),
            egui::Sense::click_and_drag(),
        );
        if let Some(pointer) = volume_response.interact_pointer_pos()
            && (volume_response.dragged() || volume_response.clicked())
        {
            self.set_volume(((pointer.x - volume_track.left()) / volume_track.width()) as f64);
        }
        let shown_volume = if self.muted { 0.0 } else { self.volume as f32 };
        slider_track(
            ui.painter(),
            volume_track,
            shown_volume,
            Color32::from_white_alpha(64),
        );

        // À droite : vitesse et plein écran.
        let full =
            egui::Rect::from_center_size(egui::pos2(panel.right() - 30.0, center_y), egui::vec2(36.0, 36.0));
        if icon_button(ui, full, false)
            .on_hover_text("Plein écran (F)")
            .clicked()
        {
            toggle_fullscreen(ui.ctx());
        }
        fullscreen_icon(ui.painter(), full.center());
        let speed =
            egui::Rect::from_center_size(egui::pos2(full.left() - 34.0, center_y), egui::vec2(52.0, 32.0));
        let speed_response = ui.interact(speed, ui.scope_id().with("vitesse"), egui::Sense::click());
        ui.painter().rect_filled(
            speed,
            8.0,
            Color32::from_white_alpha(if speed_response.hovered() { 46 } else { 31 }),
        );
        ui.painter().text(
            speed.center(),
            egui::Align2::CENTER_CENTER,
            speed_label(self.speed),
            egui::FontId::proportional(13.0),
            Color32::WHITE,
        );
        if speed_response.on_hover_text("Vitesse de lecture").clicked() {
            self.cycle_speed();
        }
    }

    /// Colonne « À suivre » : vidéos du dossier, puis la lecture automatique.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let area = ui.max_rect();
        let painter = ui.painter().clone();
        painter.text(
            area.left_top() + egui::vec2(18.0, 30.0),
            egui::Align2::LEFT_CENTER,
            "À suivre",
            egui::FontId::new(16.0, style::strong_family()),
            Color32::from_rgb(0xF3, 0xF4, 0xF6),
        );
        let left = self
            .queue
            .iter()
            .position(|path| *path == self.path)
            .map_or(self.queue.len(), |index| self.queue.len() - index - 1);
        painter.text(
            area.right_top() + egui::vec2(-18.0, 31.0),
            egui::Align2::RIGHT_CENTER,
            match left {
                0 => "Dernière vidéo".to_owned(),
                1 => "1 vidéo".to_owned(),
                count => format!("{count} vidéos"),
            },
            egui::FontId::proportional(12.0),
            MUTED,
        );
        let footer = egui::Rect::from_min_max(egui::pos2(area.left(), area.bottom() - 54.0), area.max);
        painter.line_segment(
            [footer.left_top(), footer.right_top()],
            egui::Stroke::new(1.0, SIDEBAR_LINE),
        );
        painter.text(
            egui::pos2(footer.left() + 18.0, footer.center().y),
            egui::Align2::LEFT_CENTER,
            "Lecture automatique",
            egui::FontId::proportional(13.0),
            Color32::from_rgb(0xF3, 0xF4, 0xF6),
        );
        let switch = egui::Rect::from_center_size(
            egui::pos2(footer.right() - 40.0, footer.center().y),
            egui::vec2(44.0, 26.0),
        );
        let response = ui.interact(switch, ui.scope_id().with("auto"), egui::Sense::click());
        painter.rect_filled(switch, 13.0, if self.autoplay { SWITCH_ON } else { SWITCH_OFF });
        let knob_x = if self.autoplay {
            switch.right() - 13.0
        } else {
            switch.left() + 13.0
        };
        painter.circle_filled(egui::pos2(knob_x, switch.center().y), 11.0, Color32::WHITE);
        if response.clicked() {
            self.autoplay = !self.autoplay;
        }

        let list = egui::Rect::from_min_max(
            egui::pos2(area.left() + 10.0, area.top() + 56.0),
            egui::pos2(area.right() - 10.0, footer.top() - 6.0),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list).id_salt("liste"));
        egui::ScrollArea::vertical().show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if self.queue.is_empty() {
                ui.label(egui::RichText::new("Les vidéos du dossier apparaîtront ici.").color(MUTED));
            }
            for path in self.queue.clone() {
                if self.queue_row(ui, &path) {
                    self.open(path);
                }
            }
        });
    }

    /// Une ligne de la liste ; vrai si on l'a choisie.
    fn queue_row(&self, ui: &mut egui::Ui, path: &Path) -> bool {
        let current = path == self.path;
        let (row, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 84.0), egui::Sense::click());
        let painter = ui.painter();
        if current {
            painter.rect_filled(row, 12.0, Color32::from_rgba_unmultiplied(108, 184, 255, 31));
        } else if response.hovered() {
            painter.rect_filled(row, 12.0, Color32::from_white_alpha(10));
        }
        let thumb = egui::Rect::from_min_size(row.min + egui::vec2(8.0, 8.0), egui::vec2(120.0, 68.0));
        match (&self.texture, current) {
            (Some(texture), true) => {
                // Image en cours, recadrée pour remplir la vignette.
                let size = texture.size_vec2();
                let ratio = thumb.width() / thumb.height();
                let uv = if size.x / size.y > ratio {
                    let w = ratio * size.y / size.x;
                    egui::Rect::from_min_max(
                        egui::pos2((1.0 - w) / 2.0, 0.0),
                        egui::pos2((1.0 + w) / 2.0, 1.0),
                    )
                } else {
                    let h = size.x / ratio / size.y;
                    egui::Rect::from_min_max(
                        egui::pos2(0.0, (1.0 - h) / 2.0),
                        egui::pos2(1.0, (1.0 + h) / 2.0),
                    )
                };
                egui::Image::new((texture.id(), thumb.size()))
                    .uv(uv)
                    .corner_radius(8.0)
                    .paint_at(ui, thumb);
            }
            _ => {
                painter.rect_filled(thumb, 8.0, Color32::from_rgb(0x22, 0x27, 0x30));
                painter.circle_filled(thumb.center(), 14.0, Color32::from_black_alpha(110));
                play_triangle(
                    painter,
                    thumb.center() + egui::vec2(1.5, 0.0),
                    6.0,
                    Color32::from_white_alpha(220),
                );
            }
        }
        if current && self.duration > 0.0 {
            let progress = (self.position / self.duration).clamp(0.0, 1.0) as f32;
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(thumb.left(), thumb.bottom() - 3.0),
                    egui::vec2(thumb.width() * progress, 3.0),
                ),
                0.0,
                Color32::from_rgb(0x6C, 0xB8, 0xFF),
            );
        }
        let text_x = thumb.right() + 12.0;
        let width = row.right() - text_x - 8.0;
        let title = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let galley = painter.layout(
            title,
            egui::FontId::proportional(13.0),
            Color32::from_rgb(0xF3, 0xF4, 0xF6),
            width,
        );
        let galley = if galley.rows.len() > 2 {
            let mut job = egui::text::LayoutJob::simple(
                galley.job.text.clone(),
                egui::FontId::proportional(13.0),
                Color32::from_rgb(0xF3, 0xF4, 0xF6),
                width,
            );
            job.wrap.max_rows = 2;
            painter.layout_job(job)
        } else {
            galley
        };
        let title_height = galley.size().y;
        painter.galley(egui::pos2(text_x, thumb.top() + 2.0), galley, Color32::WHITE);
        let subtitle = if current {
            if self.playing { "En lecture" } else { "En pause" }.to_owned()
        } else {
            path.extension()
                .map(|extension| extension.to_string_lossy().to_uppercase())
                .unwrap_or_default()
        };
        painter.text(
            egui::pos2(text_x, thumb.top() + title_height + 8.0),
            egui::Align2::LEFT_TOP,
            subtitle,
            egui::FontId::proportional(12.0),
            if current { CURRENT } else { MUTED },
        );
        response.clicked() && !current
    }

    fn open_picker(&mut self) {
        let start = self.path.parent().filter(|path| path.is_dir());
        self.picker = Some(FilesState::picker(PickerMode::Open, start, ""));
    }

    fn picker_sheet(&mut self, ctx: &egui::Context) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        let screen = ctx.content_rect();
        let size = egui::vec2(
            (screen.width() - 48.0).clamp(420.0, 900.0),
            (screen.height() - 64.0).clamp(300.0, 560.0),
        );
        egui::Modal::new(egui::Id::unique("lecteur-video-selecteur"))
            .frame(
                egui::Frame::new()
                    .fill(style::background())
                    .corner_radius(12.0)
                    .shadow(style::elevation(ferrite_ui::tokens::elevation::MENU)),
            )
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("selecteur"));
                child.set_clip_rect(rect);
                picker.update(&mut child);
            });
        let choice = self.picker.as_mut().and_then(FilesState::take_choice);
        if let Some(choice) = choice {
            self.picker = None;
            if let Some(path) = choice {
                self.open(path);
            }
        }
    }
}

fn toggle_fullscreen(ctx: &egui::Context) {
    let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
}

/// Nom lisible du format vidéo dont le décodeur manque (`None` pour le son et le reste :
/// une piste audio illisible n'empêche pas de voir la vidéo).
fn missing_video_format(media: &str) -> Option<String> {
    let kind = media.strip_prefix("video/")?;
    Some(match kind {
        "x-av1" => "AV1".to_owned(),
        "x-h264" => "H.264".to_owned(),
        "x-h265" => "H.265 (HEVC)".to_owned(),
        "x-vp8" => "VP8".to_owned(),
        "x-vp9" => "VP9".to_owned(),
        "mpeg" => "MPEG".to_owned(),
        other => other.trim_start_matches("x-").to_uppercase(),
    })
}

/// Texte centré sur l'image (chargement, erreur).
fn message(painter: &egui::Painter, area: egui::Rect, title: &str, detail: &str) {
    painter.text(
        area.center() - egui::vec2(0.0, 14.0),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(20.0),
        Color32::WHITE,
    );
    if !detail.is_empty() {
        painter.text(
            area.center() + egui::vec2(0.0, 16.0),
            egui::Align2::CENTER_CENTER,
            detail,
            egui::FontId::proportional(13.0),
            MUTED,
        );
    }
}

/// Dégradé vertical de `top` à `bottom`.
fn gradient(painter: &egui::Painter, rect: egui::Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(egui::Shape::mesh(mesh));
}

/// Piste de 4 px : fond `rest`, partie remplie en blanc jusqu'à `fraction`.
fn slider_track(painter: &egui::Painter, area: egui::Rect, fraction: f32, rest: Color32) {
    let track = egui::Rect::from_center_size(area.center(), egui::vec2(area.width(), 4.0));
    painter.rect_filled(track, 2.0, rest);
    let filled = egui::Rect::from_min_size(track.min, egui::vec2(track.width() * fraction, 4.0));
    painter.rect_filled(filled, 2.0, Color32::WHITE);
}

fn play_triangle(painter: &egui::Painter, center: egui::Pos2, size: f32, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            center + egui::vec2(-size * 0.75, -size),
            center + egui::vec2(size, 0.0),
            center + egui::vec2(-size * 0.75, size),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

/// Bouton rond-carré transparent (fond léger au survol, ou s'il est actif).
fn icon_button(ui: &mut egui::Ui, rect: egui::Rect, active: bool) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.scope_id()
            .with(("bouton", rect.min.x as i32, rect.min.y as i32)),
        egui::Sense::click(),
    );
    let alpha = match (active, response.hovered()) {
        (true, _) => 51,
        (false, true) => 26,
        _ => 0,
    };
    if alpha > 0 {
        ui.painter()
            .rect_filled(rect, 10.0, Color32::from_white_alpha(alpha));
    }
    response
}

/// Bouton texte en pastille : bleu (`primary`) ou blanc translucide.
fn pill_button(ui: &mut egui::Ui, rect: egui::Rect, label: &str, primary: bool) -> egui::Response {
    let response = ui.interact(rect, ui.scope_id().with(label), egui::Sense::click());
    let fill = match (primary, response.hovered()) {
        (true, false) => SWITCH_ON,
        (true, true) => Color32::from_rgb(0x3A, 0x8A, 0xEE),
        (false, false) => Color32::from_white_alpha(46),
        (false, true) => Color32::from_white_alpha(66),
    };
    ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(13.0),
        Color32::WHITE,
    );
    response
}

/// Flèche circulaire et « 10 » : reculer (`forward` faux) ou avancer de 10 s.
fn skip_icon(painter: &egui::Painter, center: egui::Pos2, forward: bool) {
    let stroke = egui::Stroke::new(1.8, Color32::WHITE);
    let direction = if forward { 1.0 } else { -1.0 };
    let radius = 8.5;
    let points: Vec<egui::Pos2> = (0..=20)
        .map(|step| {
            // Arc de -60° à 240°, ouvert en haut du côté de la flèche.
            let angle = (-60.0 + step as f32 * 15.0).to_radians();
            center + egui::vec2(direction * -angle.cos() * radius, -angle.sin() * radius)
        })
        .collect();
    let tip = points[0];
    painter.add(egui::Shape::line(points, stroke));
    painter.line_segment([tip, tip + egui::vec2(0.0, -5.0)], stroke);
    painter.line_segment([tip, tip + egui::vec2(direction * -5.0, 0.0)], stroke);
    painter.text(
        center + egui::vec2(0.0, 1.0),
        egui::Align2::CENTER_CENTER,
        "10",
        egui::FontId::proportional(7.5),
        Color32::WHITE,
    );
}

fn speaker_icon(painter: &egui::Painter, center: egui::Pos2, silent: bool) {
    let c = center + egui::vec2(-3.0, 0.0);
    painter.add(egui::Shape::convex_polygon(
        vec![
            c + egui::vec2(-6.0, -3.0),
            c + egui::vec2(-2.0, -3.0),
            c + egui::vec2(3.0, -7.0),
            c + egui::vec2(3.0, 7.0),
            c + egui::vec2(-2.0, 3.0),
            c + egui::vec2(-6.0, 3.0),
        ],
        Color32::WHITE,
        egui::Stroke::NONE,
    ));
    let stroke = egui::Stroke::new(1.8, Color32::WHITE);
    if silent {
        painter.line_segment([c + egui::vec2(6.5, -3.0), c + egui::vec2(11.0, 2.0)], stroke);
        painter.line_segment([c + egui::vec2(11.0, -3.0), c + egui::vec2(6.5, 2.0)], stroke);
    } else {
        for radius in [5.0_f32, 9.0] {
            let points: Vec<egui::Pos2> = (-3..=3)
                .map(|step| {
                    let angle = (step as f32 * 15.0).to_radians();
                    c + egui::vec2(3.0 + angle.cos() * radius, angle.sin() * radius)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
        }
    }
}

fn fullscreen_icon(painter: &egui::Painter, center: egui::Pos2) {
    let stroke = egui::Stroke::new(1.9, Color32::WHITE);
    let d = 7.0;
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let corner = center + egui::vec2(sx * d, sy * d);
        painter.line_segment([corner, corner + egui::vec2(-sx * 4.5, 0.0)], stroke);
        painter.line_segment([corner, corner + egui::vec2(0.0, -sy * 4.5)], stroke);
    }
}

/// Coupe `text` pour tenir à peu près dans `width` (estimation par la taille de police).
fn truncate(text: &str, width: f32, size: f32) -> String {
    let max = (width / (size * 0.55)).max(4.0) as usize;
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let kept: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{kept}…")
    }
}

impl Drop for VideoState {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn file_uri(path: &Path) -> Result<String, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("Fichier vidéo inaccessible : {error}"))?;
    Ok(encode_file_uri(&path))
}

fn encode_file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            uri.push(*byte as char);
        } else {
            use std::fmt::Write as _;
            write!(uri, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    uri
}

pub fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp4"
                    | "m4v"
                    | "mkv"
                    | "webm"
                    | "avi"
                    | "mov"
                    | "ogv"
                    | "mpeg"
                    | "mpg"
                    | "wmv"
                    | "flv"
                    | "3gp"
                    | "ts"
            )
        })
}

fn format_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    let hours = seconds / 3600;
    let minutes = (seconds / 60) % 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn speed_label(speed: f64) -> String {
    format!("{}×", format!("{speed}").replace('.', ","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nomme_le_format_video_sans_decodeur() {
        assert_eq!(missing_video_format("video/x-av1").as_deref(), Some("AV1"));
        assert_eq!(
            missing_video_format("video/x-h265").as_deref(),
            Some("H.265 (HEVC)")
        );
        assert_eq!(missing_video_format("video/x-theora").as_deref(), Some("THEORA"));
        assert_eq!(missing_video_format("audio/x-opus"), None);
    }

    #[test]
    fn reconnait_les_formats_video_sans_tenir_compte_de_la_casse() {
        for path in [
            "film.MP4",
            "film.mkv",
            "film.webm",
            "film.avi",
            "film.mov",
            "film.ogv",
            "film.mpeg",
            "film.mpg",
            "film.wmv",
            "film.flv",
            "film.3gp",
            "film.ts",
        ] {
            assert!(supported(Path::new(path)), "{path}");
        }
        assert!(!supported(Path::new("film.mp3")));
        assert!(!supported(Path::new("film.mp4.txt")));
    }

    #[test]
    fn formate_le_temps() {
        assert_eq!(format_time(65.0), "1:05");
        assert_eq!(format_time(3661.0), "1:01:01");
        assert_eq!(speed_label(1.0), "1×");
        assert_eq!(speed_label(1.25), "1,25×");
    }

    #[test]
    fn encode_les_chemins_en_uri_file() {
        assert_eq!(
            encode_file_uri(Path::new("/tmp/a b.mp4")),
            "file:///tmp/a%20b.mp4"
        );
    }
}
