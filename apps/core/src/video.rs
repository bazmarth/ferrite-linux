//! Lecteur vidéo basé sur GStreamer ; la vidéo est intégrée à la fenêtre Ferrite et le son
//! passe par la sortie audio système.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use eframe::egui::{self, Color32, Key, TextureHandle, TextureOptions};
use gst::prelude::*;
use gstreamer as gst;

use crate::{
    files::{FilesState, PickerMode},
    style,
};

const SEEK_STEP: f64 = 5.0;
const REFRESH: Duration = Duration::from_millis(100);

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
        pipeline.set_property("volume", 1.0_f64);
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
        let Some(pipeline) = &self.pipeline else {
            return;
        };
        let seconds = seconds.clamp(0.0, self.duration.max(0.0));
        let position = gst::ClockTime::from_nseconds((seconds * 1_000_000_000.0) as u64);
        match pipeline.seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, position) {
            Ok(()) => {
                self.position = seconds;
                self.seeking = None;
            }
            Err(error) => self.error = Some(format!("Déplacement dans la vidéo impossible : {error}")),
        }
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
                    _ => {}
                }
            }
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
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input(|input| input.key_pressed(Key::Space)) {
            self.toggle_play();
        }
        if ctx.input(|input| input.key_pressed(Key::ArrowRight)) {
            self.seek(self.position + SEEK_STEP);
        }
        if ctx.input(|input| input.key_pressed(Key::ArrowLeft)) {
            self.seek(self.position - SEEK_STEP);
        }
        if ctx.input(|input| input.key_pressed(Key::M)) {
            self.toggle_mute();
        }
        if ctx.input(|input| input.modifiers.command && input.key_pressed(Key::O)) {
            self.open_picker();
        }
        if ctx.input(|input| input.key_pressed(Key::F)) {
            let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
        }
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if let Some(pipeline) = &self.pipeline {
            pipeline.set_property("mute", self.muted);
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        self.update_status();
        self.keys(ui.ctx());
        if self.playing {
            ui.ctx().request_repaint_after(REFRESH);
        }
        style::toolbar_with(ui, true, bar(), line(), |ui| self.toolbar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(7, 12, 20)))
            .show(ui, |ui| {
                let player_width = (ui.available_width() - 268.0).max(280.0);
                let height = ui.available_height();
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(player_width, height),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| self.player_surface(ui),
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(252.0, height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| self.playlist(ui),
                    );
                });
            });
        egui::Panel::bottom(ui.scope_id().with("lecteur-video"))
            .resizable(false)
            .exact_size(76.0)
            .frame(egui::Frame::new().fill(bar()))
            .show(ui, |ui| self.controls(ui));
        self.picker_sheet(&ui.ctx().clone());
    }

    fn player_surface(&mut self, ui: &mut egui::Ui) {
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 12.0, Color32::from_rgb(5, 11, 19));
        ui.painter().circle_filled(
            area.center(),
            area.width().min(area.height()) * 0.34,
            Color32::from_rgba_unmultiplied(20, 121, 145, 24),
        );
        if let Some(texture) = &self.texture {
            let size = texture.size_vec2();
            let scale = (area.width() / size.x).min(area.height() / size.y);
            let displayed = size * scale;
            let rect = egui::Rect::from_center_size(area.center(), displayed);
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else if let Some(error) = &self.error {
            ui.painter().text(
                area.center() - egui::vec2(0.0, 20.0),
                egui::Align2::CENTER_CENTER,
                "Lecture impossible",
                egui::FontId::proportional(22.0),
                Color32::WHITE,
            );
            ui.painter().text(
                area.center() + egui::vec2(0.0, 15.0),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(13.0),
                Color32::from_gray(180),
            );
        } else if self.path.as_os_str().is_empty() {
            let icon =
                egui::Rect::from_center_size(area.center() - egui::vec2(0.0, 95.0), egui::vec2(84.0, 84.0));
            self.icons.paint(ui, "app-video", icon, style::IconStyle::Color);
            ui.painter().text(
                area.center() - egui::vec2(0.0, 16.0),
                egui::Align2::CENTER_CENTER,
                "Votre cinéma, ici.",
                egui::FontId::proportional(24.0),
                Color32::WHITE,
            );
            ui.painter().text(
                area.center() + egui::vec2(0.0, 14.0),
                egui::Align2::CENTER_CENTER,
                "Ouvrez une vidéo pour commencer la lecture.",
                egui::FontId::proportional(13.0),
                Color32::from_gray(170),
            );
            let button =
                egui::Rect::from_center_size(area.center() + egui::vec2(0.0, 68.0), egui::vec2(190.0, 42.0));
            if ui.put(button, egui::Button::new("Choisir une vidéo")).clicked() {
                self.open_picker();
            }
        } else {
            ui.spinner();
            ui.painter().text(
                area.center() + egui::vec2(0.0, 28.0),
                egui::Align2::CENTER_CENTER,
                "Préparation de la lecture…",
                egui::FontId::proportional(14.0),
                Color32::WHITE,
            );
        }
        ui.allocate_rect(area, egui::Sense::hover());
    }

    fn playlist(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(
            egui::RichText::new("À SUIVRE")
                .size(12.0)
                .strong()
                .color(Color32::from_gray(180)),
        );
        ui.label(
            egui::RichText::new(format!("{} vidéo(s)", self.queue.len()))
                .size(11.0)
                .color(Color32::from_gray(130)),
        );
        ui.add_space(6.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for path in self.queue.clone() {
                let selected = path == self.path;
                let name: String = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .chars()
                    .take(22)
                    .collect();
                let (row, response) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 66.0), egui::Sense::click());
                let painter = ui.painter();
                painter.rect_filled(
                    row,
                    9.0,
                    if selected {
                        Color32::from_rgb(30, 47, 62)
                    } else if response.hovered() {
                        Color32::from_rgb(23, 31, 42)
                    } else {
                        Color32::from_rgb(14, 21, 30)
                    },
                );
                let thumb = egui::Rect::from_min_size(row.min + egui::vec2(7.0, 7.0), egui::vec2(78.0, 52.0));
                painter.rect_filled(thumb, 6.0, Color32::from_rgb(18, 83, 91));
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(thumb.left(), thumb.center().y), thumb.max),
                    6.0,
                    Color32::from_rgb(24, 45, 70),
                );
                painter.circle_filled(thumb.center(), 10.0, Color32::from_black_alpha(140));
                painter.text(
                    thumb.center(),
                    egui::Align2::CENTER_CENTER,
                    "▶",
                    egui::FontId::proportional(11.0),
                    Color32::WHITE,
                );
                let title_color = if selected {
                    Color32::WHITE
                } else {
                    Color32::from_gray(205)
                };
                painter.text(
                    row.min + egui::vec2(94.0, 17.0),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::proportional(12.0),
                    title_color,
                );
                let subtitle = if selected {
                    "En lecture".to_owned()
                } else {
                    path.extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                };
                painter.text(
                    row.min + egui::vec2(94.0, 43.0),
                    egui::Align2::LEFT_CENTER,
                    subtitle,
                    egui::FontId::proportional(10.0),
                    Color32::from_gray(145),
                );
                if response.clicked() && !selected {
                    self.open(path);
                }
            }
            if self.queue.is_empty() {
                ui.label(
                    egui::RichText::new("Les vidéos du dossier apparaîtront ici.")
                        .color(Color32::from_gray(140)),
                );
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        let title = self
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        ui.horizontal(|ui| {
            let (icon, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
            self.icons.paint(ui, "app-video", icon, style::IconStyle::Color);
            ui.label(egui::RichText::new(title).strong().color(style::primary_text()));
            if ui
                .button("Ouvrir…")
                .on_hover_text("Ouvrir une vidéo (Ctrl+O)")
                .clicked()
            {
                self.open_picker();
            }
        });
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
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_black_alpha(60),
                    }),
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

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            if ui
                .add_sized(
                    [48.0, 36.0],
                    egui::Button::new(if self.playing { "Ⅱ" } else { "▶" }),
                )
                .on_hover_text(if self.playing {
                    "Pause (Espace)"
                } else {
                    "Lecture (Espace)"
                })
                .clicked()
            {
                self.toggle_play();
            }
            let mut position = self.seeking.unwrap_or(self.position);
            let duration = self.duration.max(0.0);
            let slider_width = (ui.available_width() - 150.0).max(80.0);
            let response = ui.add_sized(
                [slider_width, 30.0],
                egui::Slider::new(&mut position, 0.0..=duration.max(0.1))
                    .show_value(false)
                    .trailing_fill(true),
            );
            if response.changed() {
                self.seeking = Some(position);
            }
            if response.drag_stopped()
                && let Some(position) = self.seeking.take()
            {
                self.seek(position);
            }
            ui.label(format_time(self.seeking.unwrap_or(self.position)));
            ui.label("/");
            ui.label(format_time(self.duration));
            if ui
                .add_sized(
                    [44.0, 34.0],
                    egui::Button::new(if self.muted { "Muet" } else { "Son" }),
                )
                .on_hover_text("Activer/désactiver le son (M)")
                .clicked()
            {
                self.toggle_mute();
            }
        });
        ui.add_space(6.0);
    }
}

impl Drop for VideoState {
    fn drop(&mut self) {
        self.stop();
    }
}

fn file_uri(path: &Path) -> Result<String, String> {
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

fn bar() -> Color32 {
    style::themed(Color32::WHITE, Color32::from_rgb(0x1C, 0x1F, 0x25))
}

fn line() -> Color32 {
    style::themed(
        Color32::from_rgb(0xE3, 0xE6, 0xEB),
        Color32::from_rgb(0x2C, 0x30, 0x38),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn encode_les_chemins_en_uri_file() {
        assert_eq!(
            encode_file_uri(Path::new("/tmp/a b.mp4")),
            "file:///tmp/a%20b.mp4"
        );
    }
}
