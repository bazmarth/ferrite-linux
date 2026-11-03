//! Visionneuse d'images, d'après les planches « Visionneuse d'images » de la maquette :
//! l'image sur une scène sombre (claire en thème clair), les images du dossier en bandeau,
//! une barre flottante (zoom, rotation, corbeille) et un panneau d'informations.
//!
//! Les images (PNG et JPEG) sont décodées dans des fils à part : l'image affichée d'un
//! côté, les vignettes du bandeau de l'autre, l'une après l'autre.

use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant, SystemTime},
};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Key, Pos2, Rect, Sense, Stroke, TextureHandle,
    TextureOptions, Vec2, emath::Rot2, pos2, vec2,
};
use ferrite_ui::wallpaper::{self, Image};

use crate::{files, style};

/// Hauteur du bandeau des vignettes et largeur du panneau d'informations (maquette).
const STRIP_HEIGHT: f32 = 92.0;
const INFO_WIDTH: f32 = 300.0;
/// Largeur de scène en dessous de laquelle le panneau d'informations se cache.
const MIN_STAGE_WIDTH: f32 = 560.0;
/// Côté des vignettes décodées, et de l'échantillon des couleurs dominantes.
const THUMB_SIDE: u32 = 160;
const SAMPLE_SIDE: u32 = 64;
/// Pas du zoom et ses bornes (1 = un pixel de l'image par point).
const ZOOM_STEP: f32 = 1.25;
const ZOOM_MAX: f32 = 8.0;
/// Durée d'une image en diaporama.
const SLIDE: Duration = Duration::from_secs(4);

/// L'extension est-elle une image que la Visionneuse sait ouvrir ?
pub fn supported(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg")
    )
}

/// Images ouvrables d'un dossier, par nom (sans tenir compte de la casse).
fn images_in(folder: &Path) -> Vec<PathBuf> {
    let mut images: Vec<PathBuf> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && supported(path))
        .filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .collect();
    images.sort_by_key(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()));
    images
}

/// Dossier Images de l'utilisateur, ouvert quand la Visionneuse démarre sans fichier.
fn pictures_dir() -> Option<PathBuf> {
    let home = PathBuf::from(env::var_os("HOME")?);
    ["Images", "Pictures"]
        .into_iter()
        .map(|name| home.join(name))
        .find(|path| path.is_dir())
}

/// Réduit `image` pour que son plus grand côté tienne dans `side` (moyenne par blocs).
fn shrink(image: &Image, side: u32) -> Image {
    let factor = side as f32 / image.width.max(image.height) as f32;
    if factor >= 1.0 {
        return image.clone();
    }
    let width = ((image.width as f32 * factor).round() as u32).max(1);
    let height = ((image.height as f32 * factor).round() as u32).max(1);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let (y0, y1) = (
            y * image.height / height,
            ((y + 1) * image.height / height).max(y * image.height / height + 1),
        );
        for x in 0..width {
            let (x0, x1) = (
                x * image.width / width,
                ((x + 1) * image.width / width).max(x * image.width / width + 1),
            );
            let mut sum = [0u64; 4];
            // Au plus 4 × 4 échantillons par pixel : assez pour lisser, rapide sur une 4K.
            let (step_y, step_x) = (((y1 - y0) / 4).max(1), ((x1 - x0) / 4).max(1));
            let mut count = 0;
            for sy in (y0..y1).step_by(step_y as usize) {
                for sx in (x0..x1).step_by(step_x as usize) {
                    let p = &image.rgba[((sy * image.width + sx) * 4) as usize..][..4];
                    for channel in 0..4 {
                        sum[channel] += p[channel] as u64;
                    }
                    count += 1;
                }
            }
            rgba.extend(sum.iter().map(|value| (value / count) as u8));
        }
    }
    Image { width, height, rgba }
}

/// Cinq couleurs dominantes, de la plus présente à la moins présente, assez différentes
/// les unes des autres.
fn dominant_colors(sample: &Image) -> Vec<Color32> {
    let mut buckets: HashMap<(u8, u8, u8), (u64, [u64; 3])> = HashMap::new();
    for &[r, g, b, a] in sample.rgba.as_chunks::<4>().0 {
        if a <= 128 {
            continue;
        }
        let entry = buckets.entry((r >> 5, g >> 5, b >> 5)).or_default();
        entry.0 += 1;
        for (sum, value) in entry.1.iter_mut().zip([r, g, b]) {
            *sum += value as u64;
        }
    }
    let mut ranked: Vec<(u64, [u8; 3])> = buckets
        .into_values()
        .map(|(count, sum)| (count, sum.map(|value| (value / count) as u8)))
        .collect();
    ranked.sort_by_key(|(count, _)| std::cmp::Reverse(*count));
    let mut colors: Vec<[u8; 3]> = Vec::new();
    for (_, color) in ranked {
        let far = colors.iter().all(|kept| {
            kept.iter()
                .zip(color)
                .map(|(a, b)| (*a as i32 - b as i32).pow(2))
                .sum::<i32>()
                > 40 * 40
        });
        if far {
            colors.push(color);
        }
        if colors.len() == 5 {
            break;
        }
    }
    colors
        .into_iter()
        .map(|[r, g, b]| Color32::from_rgb(r, g, b))
        .collect()
}

fn color_image(image: &Image) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.rgba)
}

/// Image décodée dans le fil de chargement.
struct Decoded {
    image: egui::ColorImage,
    width: u32,
    height: u32,
    colors: Vec<Color32>,
}

/// Image affichée.
struct Shown {
    path: PathBuf,
    texture: TextureHandle,
    /// Dimensions d'origine (la texture peut être réduite).
    width: u32,
    height: u32,
    colors: Vec<Color32>,
    bytes: u64,
    format: &'static str,
    modified: Option<SystemTime>,
}

enum Thumb {
    Pending,
    Ready(TextureHandle),
    Failed,
}

/// Message en haut de la scène ; avec `undo`, l'image mise à la corbeille peut revenir.
struct Toast {
    text: String,
    until: Instant,
    undo: Option<(PathBuf, usize)>,
}

struct Slideshow {
    next_at: Instant,
    paused: bool,
}

pub struct ViewerState {
    images: Vec<PathBuf>,
    index: usize,
    folder: Option<PathBuf>,
    shown: Option<Shown>,
    loading: Option<PathBuf>,
    error: Option<String>,
    full_sender: Sender<(PathBuf, Result<Decoded, String>)>,
    full_receiver: Receiver<(PathBuf, Result<Decoded, String>)>,
    thumbs: HashMap<PathBuf, Thumb>,
    thumb_queue: Option<Sender<PathBuf>>,
    thumb_receiver: Receiver<(PathBuf, Option<egui::ColorImage>)>,
    thumb_sender: Sender<(PathBuf, Option<egui::ColorImage>)>,
    /// `None` : ajustée à la scène ; sinon, points d'écran par pixel de l'image.
    zoom: Option<f32>,
    offset: Vec2,
    /// Quarts de tour, dans le sens des aiguilles d'une montre.
    quarter_turns: i32,
    info: bool,
    toast: Option<Toast>,
    slideshow: Option<Slideshow>,
    /// Dernière échelle « ajustée », pour zoomer à partir de ce qu'on voit.
    fit_scale: f32,
}

impl ViewerState {
    pub fn new(file: Option<PathBuf>) -> Self {
        let (full_sender, full_receiver) = mpsc::channel();
        let (thumb_sender, thumb_receiver) = mpsc::channel();
        let mut state = Self {
            images: Vec::new(),
            index: 0,
            folder: None,
            shown: None,
            loading: None,
            error: None,
            full_sender,
            full_receiver,
            thumbs: HashMap::new(),
            thumb_queue: None,
            thumb_receiver,
            thumb_sender,
            zoom: None,
            offset: Vec2::ZERO,
            quarter_turns: 0,
            info: true,
            toast: None,
            slideshow: None,
            fit_scale: 1.0,
        };
        match file {
            Some(file) => state.open(file),
            None => {
                if let Some(folder) = pictures_dir() {
                    state.images = images_in(&folder);
                    state.folder = Some(folder);
                }
            }
        }
        state
    }

    /// Ouvre `path` (une image, ou un dossier : sa première image) avec les images de son dossier.
    pub fn open(&mut self, path: PathBuf) {
        let (folder, file) = if path.is_dir() {
            (path.clone(), None)
        } else {
            (
                path.parent().map(Path::to_path_buf).unwrap_or_default(),
                Some(path),
            )
        };
        self.images = images_in(&folder);
        if let Some(file) = &file
            && !self.images.contains(file)
        {
            self.images.insert(0, file.clone());
        }
        self.index = file
            .and_then(|file| self.images.iter().position(|image| *image == file))
            .unwrap_or(0);
        self.folder = Some(folder);
        self.thumbs.clear();
        self.thumb_queue = None;
        self.reset_view();
    }

    fn reset_view(&mut self) {
        self.zoom = None;
        self.offset = Vec2::ZERO;
        self.quarter_turns = 0;
    }

    fn go(&mut self, step: isize) {
        if self.images.is_empty() {
            return;
        }
        let count = self.images.len() as isize;
        self.index = (self.index as isize + step).rem_euclid(count) as usize;
        self.reset_view();
    }

    fn current(&self) -> Option<&PathBuf> {
        self.images.get(self.index)
    }

    /// Reçoit les images décodées et demande celle qui manque.
    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok((path, result)) = self.full_receiver.try_recv() {
            if self.loading.as_ref() == Some(&path) {
                self.loading = None;
            }
            if self.current() != Some(&path) {
                continue;
            }
            match result {
                Ok(decoded) => {
                    let metadata = fs::metadata(&path).ok();
                    let format = match fs::read(&path).ok().and_then(|data| data.first().copied()) {
                        Some(0x89) => "PNG",
                        Some(0xFF) => "JPEG",
                        _ => "Image",
                    };
                    self.shown = Some(Shown {
                        texture: ctx.load_texture(
                            path.to_string_lossy(),
                            decoded.image,
                            TextureOptions::LINEAR,
                        ),
                        width: decoded.width,
                        height: decoded.height,
                        colors: decoded.colors,
                        bytes: metadata.as_ref().map_or(0, fs::Metadata::len),
                        format,
                        modified: metadata.and_then(|metadata| metadata.modified().ok()),
                        path,
                    });
                    self.error = None;
                }
                Err(error) => {
                    self.shown = None;
                    self.error = Some(error);
                }
            }
        }
        while let Ok((path, image)) = self.thumb_receiver.try_recv() {
            let thumb = match image {
                Some(image) => Thumb::Ready(ctx.load_texture(
                    format!("vignette:{}", path.display()),
                    image,
                    TextureOptions::LINEAR,
                )),
                None => Thumb::Failed,
            };
            self.thumbs.insert(path, thumb);
        }
        let Some(current) = self.current().cloned() else {
            self.shown = None;
            return;
        };
        let up_to_date = self.shown.as_ref().is_some_and(|shown| shown.path == current);
        if up_to_date || self.loading.as_ref() == Some(&current) {
            return;
        }
        self.loading = Some(current.clone());
        let max_side = ctx.input(|input| input.max_texture_side).clamp(1024, 8192) as u32;
        let sender = self.full_sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = wallpaper::decode(&current).map(|image| Decoded {
                colors: dominant_colors(&shrink(&image, SAMPLE_SIDE)),
                width: image.width,
                height: image.height,
                image: color_image(&shrink(&image, max_side)),
            });
            let _ = sender.send((current, result));
            ctx.request_repaint();
        });
    }

    /// Vignette de `path` ; demandée au fil des vignettes la première fois.
    fn thumb(&mut self, ctx: &egui::Context, path: &Path) -> Option<TextureHandle> {
        match self.thumbs.get(path) {
            Some(Thumb::Ready(texture)) => return Some(texture.clone()),
            Some(_) => return None,
            None => {}
        }
        if self.thumb_queue.is_none() {
            let (queue, requests) = mpsc::channel::<PathBuf>();
            let results = self.thumb_sender.clone();
            let ctx = ctx.clone();
            thread::spawn(move || {
                for path in requests {
                    let image = wallpaper::decode(&path)
                        .ok()
                        .map(|image| color_image(&shrink(&image, THUMB_SIDE)));
                    if results.send((path, image)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
            self.thumb_queue = Some(queue);
        }
        if let Some(queue) = &self.thumb_queue {
            let _ = queue.send(path.to_path_buf());
        }
        self.thumbs.insert(path.to_path_buf(), Thumb::Pending);
        None
    }

    fn say(&mut self, text: impl Into<String>, undo: Option<(PathBuf, usize)>) {
        let duration = if undo.is_some() {
            Duration::from_millis(4500)
        } else {
            Duration::from_millis(1800)
        };
        self.toast = Some(Toast {
            text: text.into(),
            until: Instant::now() + duration,
            undo,
        });
    }

    fn trash_current(&mut self) {
        let Some(path) = self.current().cloned() else {
            return;
        };
        match files::trash::trash(&path) {
            Ok(trashed) => {
                let name = file_name(&path);
                self.images.remove(self.index);
                self.thumbs.remove(&path);
                let undo = Some((trashed, self.index));
                if self.index >= self.images.len() {
                    self.index = self.images.len().saturating_sub(1);
                }
                self.reset_view();
                self.say(format!("« {name} » mise à la corbeille"), undo);
            }
            Err(error) => self.say(format!("Corbeille impossible : {error}"), None),
        }
    }

    fn undo_trash(&mut self) {
        let Some((trashed, index)) = self.toast.take().and_then(|toast| toast.undo) else {
            return;
        };
        match files::trash::restore(&trashed) {
            Ok(restored) => {
                let index = index.min(self.images.len());
                self.images.insert(index, restored);
                self.index = index;
                self.reset_view();
            }
            Err(error) => self.say(error, None),
        }
    }

    fn set_wallpaper(&mut self) {
        let Some(path) = self.current().cloned() else {
            return;
        };
        let mut settings = ferrite_system::config::Settings::load();
        settings.set("fond_ecran", &path.to_string_lossy());
        match settings.save() {
            Ok(()) => self.say(format!("Fond d’écran défini : {}", file_name(&path)), None),
            Err(error) => self.say(format!("Fond d’écran non enregistré : {error}"), None),
        }
    }

    fn toggle_slideshow(&mut self, ctx: &egui::Context) {
        match &mut self.slideshow {
            Some(show) => show.paused = !show.paused,
            None => {
                self.slideshow = Some(Slideshow {
                    next_at: Instant::now() + SLIDE,
                    paused: false,
                });
                self.reset_view();
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            }
        }
    }

    fn stop_slideshow(&mut self, ctx: &egui::Context) {
        if self.slideshow.take().is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
    }

    fn zoom_by(&mut self, factor: f32) {
        let scale = self.zoom.unwrap_or(self.fit_scale) * factor;
        let lowest = self.fit_scale.min(1.0) * 0.5;
        if factor < 1.0 && scale <= self.fit_scale {
            // En dessous de l'image ajustée, on revient à l'image ajustée.
            self.zoom = None;
            self.offset = Vec2::ZERO;
        } else {
            self.zoom = Some(scale.clamp(lowest, ZOOM_MAX));
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        if typing {
            return;
        }
        let pressed = |key| ctx.input(|input| input.key_pressed(key));
        let shift = ctx.input(|input| input.modifiers.shift);
        if pressed(Key::ArrowRight) {
            self.go(1);
        }
        if pressed(Key::ArrowLeft) {
            self.go(-1);
        }
        if pressed(Key::Home) {
            self.index = 0;
            self.reset_view();
        }
        if pressed(Key::End) {
            self.index = self.images.len().saturating_sub(1);
            self.reset_view();
        }
        if pressed(Key::Plus) || pressed(Key::Equals) {
            self.zoom_by(ZOOM_STEP);
        }
        if pressed(Key::Minus) {
            self.zoom_by(1.0 / ZOOM_STEP);
        }
        if pressed(Key::Num0) {
            self.zoom = None;
            self.offset = Vec2::ZERO;
        }
        if pressed(Key::Num1) {
            self.zoom = Some(1.0);
        }
        if pressed(Key::R) {
            self.quarter_turns += if shift { -1 } else { 1 };
        }
        if pressed(Key::Delete) {
            self.trash_current();
        }
        if pressed(Key::I) {
            self.info = !self.info;
        }
        if pressed(Key::Space) {
            self.toggle_slideshow(ctx);
        }
        if pressed(Key::Escape) {
            self.stop_slideshow(ctx);
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.pump(&ctx);
        self.keys(&ctx);
        if let Some(show) = &mut self.slideshow
            && !show.paused
        {
            let now = Instant::now();
            if now >= show.next_at {
                show.next_at = now + SLIDE;
                self.go(1);
            }
            ctx.request_repaint_after(SLIDE / 8);
        }
        if let Some(toast) = &self.toast {
            let now = Instant::now();
            if now >= toast.until {
                self.toast = None;
            } else {
                ctx.request_repaint_after(toast.until - now);
            }
        }

        if self.slideshow.is_some() {
            egui::CentralPanel::no_frame().show(ui, |ui| self.stage(ui, Color32::BLACK));
            return;
        }
        style::toolbar_with(ui, true, bar(), line(), |ui| self.toolbar(ui));
        egui::Panel::bottom(ui.scope_id().with("visionneuse-bandeau"))
            .resizable(false)
            .exact_size(STRIP_HEIGHT)
            .frame(egui::Frame::new().fill(bar()))
            .show(ui, |ui| self.strip(ui));
        // Fenêtre étroite : la scène garde sa place, le panneau attend qu'on l'agrandisse.
        let roomy = ui.available_width() >= INFO_WIDTH + MIN_STAGE_WIDTH;
        if self.info && roomy && self.current().is_some() {
            egui::Panel::right(ui.scope_id().with("visionneuse-infos"))
                .resizable(false)
                .exact_size(INFO_WIDTH)
                .frame(
                    egui::Frame::new()
                        .fill(bar())
                        .inner_margin(egui::Margin::symmetric(20, 22)),
                )
                .show(ui, |ui| self.info_panel(ui));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.stage(ui, stage()));
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        ui.add_space(6.0);
        if icon_button(ui, Glyph::Previous, false, "Image précédente").clicked() {
            self.go(-1);
        }
        if icon_button(ui, Glyph::Next, false, "Image suivante").clicked() {
            self.go(1);
        }
        // Titre centré : nom du fichier, position et dossier.
        if let Some(current) = self.current() {
            let painter = ui.painter();
            let center = full.center();
            painter.text(
                center - vec2(0.0, 8.0),
                Align2::CENTER_CENTER,
                file_name(current),
                FontId::new(14.0, style::strong_family()),
                style::primary_text(),
            );
            let folder = self
                .folder
                .as_deref()
                .and_then(Path::file_name)
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            painter.text(
                center + vec2(0.0, 10.0),
                Align2::CENTER_CENTER,
                format!("{} sur {} · {folder}", self.index + 1, self.images.len()),
                FontId::proportional(12.0),
                style::text_secondary(),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, Glyph::Info, self.info, "Informations (I)").clicked() {
                self.info = !self.info;
            }
            if icon_button(ui, Glyph::Play, false, "Diaporama (Espace)").clicked() {
                let ctx = ui.ctx().clone();
                self.toggle_slideshow(&ctx);
            }
        });
    }

    fn strip(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        ui.painter()
            .hline(full.x_range(), full.top() + 0.5, Stroke::new(1.0, line()));
        if self.images.is_empty() {
            return;
        }
        // Les vignettes autour de l'image affichée, autant que la largeur en laisse voir.
        let (normal, current_w, gap) = (72.0, 108.0, 8.0);
        let room = ((full.width() - 32.0 - current_w) / (normal + gap)).max(0.0) as usize;
        let before = (room / 2).min(self.index);
        let after = (room - before).min(self.images.len() - 1 - self.index);
        let before = (room - after).min(self.index).max(before);
        let range = self.index - before..=self.index + after;
        let total: f32 = range.clone().count() as f32 * (normal + gap) - gap + (current_w - normal);
        let mut x = full.center().x - total / 2.0;
        let ctx = ui.ctx().clone();
        let mut pick = None;
        for index in range {
            let selected = index == self.index;
            let width = if selected { current_w } else { normal };
            let rect = Rect::from_min_size(pos2(x, full.center().y - 30.0), vec2(width, 60.0));
            x += width + gap;
            let path = self.images[index].clone();
            let response = ui
                .interact(rect, ui.scope_id().with(("vignette", index)), Sense::click())
                .on_hover_text(file_name(&path));
            if response.clicked() {
                pick = Some(index);
            }
            let painter = ui.painter();
            let radius = CornerRadius::same(8);
            match self.thumb(&ctx, &path) {
                Some(texture) => {
                    paint_cover(painter, rect, &texture, radius, if selected { 1.0 } else { 0.6 })
                }
                None => {
                    painter.rect_filled(rect, radius, style::tint(18));
                }
            }
            let ring = if selected {
                Stroke::new(2.0, style::accent())
            } else {
                Stroke::new(1.0, style::tint(20))
            };
            painter.rect_stroke(rect, radius, ring, egui::StrokeKind::Outside);
        }
        if let Some(index) = pick {
            self.index = index;
            self.reset_view();
        }
    }

    fn info_panel(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        ui.painter().vline(
            full.left() - 20.0 + 0.5,
            full.y_range().expand(22.0),
            Stroke::new(1.0, line()),
        );
        let Some(current) = self.current().cloned() else {
            return;
        };
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(
            egui::RichText::new(file_stem(&current))
                .font(FontId::new(17.0, style::strong_family()))
                .color(style::primary_text()),
        );
        ui.label(
            egui::RichText::new(file_name(&current))
                .size(13.0)
                .color(style::text_secondary()),
        );
        ui.add_space(12.0);
        let shown = self.shown.as_ref().filter(|shown| shown.path == current);
        let mut rows: Vec<(&str, String)> = Vec::new();
        if let Some(shown) = shown {
            rows.push(("Dimensions", format!("{} × {}", shown.width, shown.height)));
            rows.push(("Taille", files::format_size(shown.bytes)));
            rows.push(("Format", shown.format.to_owned()));
            rows.push(("Modifiée", files::format_date(shown.modified)));
        }
        rows.push(("Dossier", current.parent().map(home_relative).unwrap_or_default()));
        // Tableau entouré d'un filet, une ligne par propriété (maquette).
        let row_h = 42.0;
        let (table, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), row_h * rows.len() as f32),
            Sense::hover(),
        );
        let painter = ui.painter();
        painter.rect_stroke(table, 12.0, Stroke::new(1.0, line()), egui::StrokeKind::Inside);
        for (row, (key, value)) in rows.iter().enumerate() {
            let top = table.top() + row as f32 * row_h;
            if row > 0 {
                painter.hline(table.x_range(), top + 0.5, Stroke::new(1.0, line()));
            }
            let y = top + row_h / 2.0;
            painter.text(
                pos2(table.left() + 14.0, y),
                Align2::LEFT_CENTER,
                *key,
                FontId::proportional(13.0),
                style::text_secondary(),
            );
            painter.text(
                pos2(table.right() - 14.0, y),
                Align2::RIGHT_CENTER,
                value,
                FontId::proportional(13.0),
                style::primary_text(),
            );
        }
        if let Some(shown) = shown
            && !shown.colors.is_empty()
        {
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new("Couleurs dominantes")
                    .size(12.0)
                    .strong()
                    .color(style::text_secondary()),
            );
            let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
            let count = shown.colors.len() as f32;
            let width = (band.width() - 6.0 * (count - 1.0)) / count;
            for (index, color) in shown.colors.iter().enumerate() {
                let rect = Rect::from_min_size(
                    pos2(band.left() + index as f32 * (width + 6.0), band.top()),
                    vec2(width, 28.0),
                );
                ui.painter().rect_filled(rect, 7.0, *color);
                ui.painter().rect_stroke(
                    rect,
                    7.0,
                    Stroke::new(1.0, Color32::from_black_alpha(20)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        // Bouton bleu collé en bas du panneau.
        let button = Rect::from_min_max(pos2(full.left(), full.bottom() - 38.0), full.max);
        let response = ui.interact(button, ui.scope_id().with("fond"), Sense::click());
        let fill = if response.hovered() {
            style::accent().gamma_multiply(0.9)
        } else {
            style::accent()
        };
        ui.painter().rect_filled(button, 10.0, fill);
        ui.painter().text(
            button.center(),
            Align2::CENTER_CENTER,
            "Définir comme fond d’écran",
            FontId::new(14.0, style::strong_family()),
            Color32::WHITE,
        );
        if response.clicked() {
            self.set_wallpaper();
        }
    }

    fn stage(&mut self, ui: &mut egui::Ui, background: Color32) {
        let area = ui.max_rect();
        ui.painter().rect_filled(area, CornerRadius::ZERO, background);
        let response = ui.interact(area, ui.scope_id().with("scene"), Sense::click_and_drag());
        let painter = ui.painter_at(area);
        let slideshow = self.slideshow.is_some();

        if self.images.is_empty() {
            painter.text(
                area.center() - vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                "Aucune image",
                FontId::new(17.0, style::strong_family()),
                style::primary_text(),
            );
            painter.text(
                area.center() + vec2(0.0, 14.0),
                Align2::CENTER_CENTER,
                "Ouvrez une image PNG ou JPEG depuis Fichiers.",
                FontId::proportional(13.0),
                style::text_secondary(),
            );
            return;
        }
        let current = self.current().cloned();
        let shown = self
            .shown
            .as_ref()
            .filter(|shown| Some(&shown.path) == current.as_ref())
            .map(|shown| (shown.texture.id(), vec2(shown.width as f32, shown.height as f32)));
        match shown {
            None => {
                let text = match &self.error {
                    Some(error) if self.loading.is_none() => format!("Image illisible : {error}"),
                    _ => "Chargement…".to_owned(),
                };
                painter.text(
                    area.center(),
                    Align2::CENTER_CENTER,
                    text,
                    FontId::proportional(13.0),
                    style::text_secondary(),
                );
            }
            Some((texture, pixels)) => {
                let sideways = self.quarter_turns.rem_euclid(2) == 1;
                let turned = if sideways {
                    vec2(pixels.y, pixels.x)
                } else {
                    pixels
                };
                // Place libre : marges de la maquette, et la barre flottante en bas.
                let margin = if slideshow {
                    vec2(0.0, 0.0)
                } else {
                    vec2(112.0, 152.0)
                };
                let room = (area.size() - margin).max(vec2(40.0, 40.0));
                self.fit_scale = (room.x / turned.x).min(room.y / turned.y).min(1.0);
                // Molette : zoom autour du pointeur ; Ctrl+molette et pincement aussi.
                let (scroll, pinch, pointer) = ui.input(|input| {
                    (
                        input.smooth_scroll_delta.y,
                        input.zoom_delta(),
                        input.pointer.hover_pos(),
                    )
                });
                if response.hovered() && (scroll != 0.0 || pinch != 1.0) {
                    let before = self.zoom.unwrap_or(self.fit_scale);
                    self.zoom_by(pinch * (scroll / 400.0).exp());
                    let after = self.zoom.unwrap_or(self.fit_scale);
                    if let Some(pointer) = pointer
                        && self.zoom.is_some()
                    {
                        let from_center = pointer - area.center() - self.offset;
                        self.offset -= from_center * (after / before - 1.0);
                    }
                }
                if response.dragged() && self.zoom.is_some() {
                    self.offset += response.drag_delta();
                }
                if response.double_clicked() {
                    self.zoom = if self.zoom.is_some() { None } else { Some(1.0) };
                    self.offset = Vec2::ZERO;
                }
                let scale = self.zoom.unwrap_or(self.fit_scale);
                let lift = if slideshow { Vec2::ZERO } else { vec2(0.0, -20.0) };
                let center = area.center() + lift + self.offset;
                if !slideshow {
                    // Ombre portée sous l'image (maquette : 0 12 40, noir 35 %).
                    let shadow = Rect::from_center_size(center + vec2(0.0, 12.0), turned * scale);
                    painter.add(
                        egui::epaint::RectShape::filled(shadow, 4.0, Color32::from_black_alpha(90))
                            .with_blur_width(40.0),
                    );
                }
                let mut mesh = egui::Mesh::with_texture(texture);
                mesh.add_rect_with_uv(
                    Rect::from_center_size(center, pixels * scale),
                    Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                let angle = self.quarter_turns as f32 * std::f32::consts::FRAC_PI_2;
                mesh.rotate(Rot2::from_angle(angle), center);
                painter.add(egui::Shape::mesh(mesh));
            }
        }
        if slideshow {
            if response.clicked() {
                let ctx = ui.ctx().clone();
                self.stop_slideshow(&ctx);
            }
            return;
        }
        self.overlay(ui, area);
    }

    /// Boutons flottants de la scène : précédente et suivante sur les côtés, barre
    /// d'affichage en bas, message en haut.
    fn overlay(&mut self, ui: &mut egui::Ui, area: Rect) {
        let side = |x: f32| Rect::from_center_size(pos2(x, area.center().y), vec2(44.0, 44.0));
        if float_button(
            ui,
            side(area.left() + 42.0),
            22.0,
            Glyph::Previous,
            "Image précédente",
        )
        .clicked()
        {
            self.go(-1);
        }
        if float_button(ui, side(area.right() - 42.0), 22.0, Glyph::Next, "Image suivante").clicked() {
            self.go(1);
        }

        // Barre : zoom −, échelle, zoom +, filet, pivoter ×2, corbeille.
        let width = 5.0 * 2.0 + 36.0 * 2.0 + 72.0 + 9.0 + 36.0 * 3.0;
        let bar = Rect::from_center_size(
            pos2(area.center().x, area.bottom() - 18.0 - 23.0),
            vec2(width, 46.0),
        );
        let painter = ui.painter();
        painter.add(
            egui::epaint::RectShape::filled(
                bar.translate(vec2(0.0, 8.0)),
                16.0,
                Color32::from_black_alpha(50),
            )
            .with_blur_width(24.0),
        );
        painter.rect_filled(bar, 16.0, floating());
        painter.rect_stroke(
            bar,
            16.0,
            Stroke::new(1.0, floating_ring()),
            egui::StrokeKind::Inside,
        );
        let mut x = bar.left() + 5.0;
        let mut slot = |w: f32| {
            let rect = Rect::from_min_size(pos2(x, bar.top() + 5.0), vec2(w, 36.0));
            x += w;
            rect
        };
        if bar_button(ui, slot(36.0), Glyph::Minus, "Zoom arrière (−)").clicked() {
            self.zoom_by(1.0 / ZOOM_STEP);
        }
        let label = slot(72.0);
        let text = match self.zoom {
            None => "Ajusté".to_owned(),
            Some(zoom) => format!("{} %", (zoom * 100.0).round()),
        };
        let response = ui
            .interact(label, ui.scope_id().with("ajuster"), Sense::click())
            .on_hover_text("Ajuster à la fenêtre (0)");
        if response.hovered() {
            ui.painter().rect_filled(label, 10.0, style::tint(14));
        }
        ui.painter().text(
            label.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::new(13.0, style::strong_family()),
            floating_ink(),
        );
        if response.clicked() {
            self.zoom = None;
            self.offset = Vec2::ZERO;
        }
        if bar_button(ui, slot(36.0), Glyph::Plus, "Zoom avant (+)").clicked() {
            self.zoom_by(ZOOM_STEP);
        }
        let rule = slot(9.0);
        ui.painter().vline(
            rule.center().x,
            rule.center().y - 10.0..=rule.center().y + 10.0,
            Stroke::new(1.0, floating_ring()),
        );
        if bar_button(ui, slot(36.0), Glyph::RotateLeft, "Pivoter à gauche (Maj+R)").clicked() {
            self.quarter_turns -= 1;
        }
        if bar_button(ui, slot(36.0), Glyph::RotateRight, "Pivoter à droite (R)").clicked() {
            self.quarter_turns += 1;
        }
        if bar_button(ui, slot(36.0), Glyph::Trash, "Mettre à la corbeille (Suppr)").clicked() {
            self.trash_current();
        }

        let Some((text, undo)) = self
            .toast
            .as_ref()
            .map(|toast| (toast.text.clone(), toast.undo.is_some()))
        else {
            return;
        };
        let font = FontId::proportional(13.0);
        let text_w = ui
            .painter()
            .layout_no_wrap(text.clone(), font.clone(), Color32::WHITE)
            .size()
            .x;
        let width = text_w + 26.0 + if undo { 14.0 + 74.0 } else { 0.0 };
        let card = Rect::from_center_size(pos2(area.center().x, area.top() + 18.0 + 23.0), vec2(width, 46.0));
        let (fill, ink, action) = if style::theme_is_dark() {
            (
                Color32::from_rgb(0xE5, 0xE7, 0xEB),
                Color32::from_rgb(0x16, 0x18, 0x1C),
                Color32::from_rgb(0x17, 0x4F, 0x9E),
            )
        } else {
            (
                Color32::from_rgb(0x16, 0x18, 0x1C),
                Color32::WHITE,
                Color32::from_rgb(0x9C, 0xC7, 0xFF),
            )
        };
        let painter = ui.painter();
        painter.rect_filled(card, 12.0, fill);
        painter.text(
            pos2(card.left() + 16.0, card.center().y),
            Align2::LEFT_CENTER,
            text,
            font,
            ink,
        );
        if undo {
            let button = Rect::from_min_size(
                pos2(card.right() - 10.0 - 74.0, card.center().y - 14.0),
                vec2(74.0, 28.0),
            );
            let response = ui.interact(button, ui.scope_id().with("annuler"), Sense::click());
            ui.painter().rect_filled(
                button,
                8.0,
                Color32::from_gray(127).gamma_multiply(if response.hovered() { 0.3 } else { 0.18 }),
            );
            ui.painter().text(
                button.center(),
                Align2::CENTER_CENTER,
                "Annuler",
                FontId::new(13.0, style::strong_family()),
                action,
            );
            if response.clicked() {
                self.undo_trash();
            }
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// `~/Images` plutôt que `/home/nom/Images`.
fn home_relative(path: &Path) -> String {
    match env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) => {
            let rest = path.strip_prefix(&home).unwrap_or(path);
            if rest.as_os_str().is_empty() {
                "~".to_owned()
            } else {
                format!("~/{}", rest.display())
            }
        }
        _ => path.display().to_string(),
    }
}

/// Couleurs des planches (sombre, clair).
fn stage() -> Color32 {
    style::themed(
        Color32::from_rgb(0xED, 0xEF, 0xF2),
        Color32::from_rgb(0x12, 0x14, 0x18),
    )
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

fn floating() -> Color32 {
    style::themed(
        Color32::from_white_alpha(204),
        Color32::from_rgba_unmultiplied(28, 31, 37, 184),
    )
}

fn floating_ring() -> Color32 {
    style::themed(Color32::from_black_alpha(20), Color32::from_white_alpha(31))
}

fn floating_ink() -> Color32 {
    style::themed(
        Color32::from_rgb(0x1F, 0x29, 0x37),
        Color32::from_rgb(0xF3, 0xF4, 0xF6),
    )
}

/// Vignette recadrée pour couvrir `rect`.
fn paint_cover(
    painter: &egui::Painter,
    rect: Rect,
    texture: &TextureHandle,
    radius: CornerRadius,
    opacity: f32,
) {
    let [w, h] = texture.size().map(|side| side as f32);
    let (box_ratio, image_ratio) = (rect.width() / rect.height(), w / h);
    let (u, v) = if image_ratio > box_ratio {
        (box_ratio / image_ratio, 1.0)
    } else {
        (1.0, image_ratio / box_ratio)
    };
    let uv = Rect::from_center_size(pos2(0.5, 0.5), vec2(u, v));
    painter.add(
        egui::epaint::RectShape::filled(rect, radius, Color32::WHITE.gamma_multiply(opacity))
            .with_texture(texture.id(), uv),
    );
}

#[derive(Clone, Copy)]
enum Glyph {
    Previous,
    Next,
    Play,
    Info,
    Plus,
    Minus,
    RotateLeft,
    RotateRight,
    Trash,
}

/// Symbole au trait, centré sur `c` (dessins de la maquette, en points).
fn paint_glyph(painter: &egui::Painter, c: Pos2, glyph: Glyph, color: Color32) {
    let stroke = Stroke::new(1.8, color);
    let p = |x: f32, y: f32| pos2(c.x + x, c.y + y);
    match glyph {
        Glyph::Previous => {
            painter.line(
                vec![p(2.5, -5.5), p(-3.0, 0.0), p(2.5, 5.5)],
                Stroke::new(2.0, color),
            );
        }
        Glyph::Next => {
            painter.line(
                vec![p(-2.5, -5.5), p(3.0, 0.0), p(-2.5, 5.5)],
                Stroke::new(2.0, color),
            );
        }
        Glyph::Play => {
            painter.add(egui::Shape::closed_line(
                vec![p(-3.0, -4.5), p(4.5, 0.0), p(-3.0, 4.5)],
                Stroke::new(1.6, color),
            ));
        }
        Glyph::Info => {
            painter.circle_stroke(c, 7.5, Stroke::new(1.6, color));
            painter.line_segment([p(0.0, -0.8), p(0.0, 4.0)], stroke);
            painter.circle_filled(p(0.0, -3.6), 1.1, color);
        }
        Glyph::Plus => {
            painter.line_segment([p(-5.0, 0.0), p(5.0, 0.0)], stroke);
            painter.line_segment([p(0.0, -5.0), p(0.0, 5.0)], stroke);
        }
        Glyph::Minus => {
            painter.line_segment([p(-5.0, 0.0), p(5.0, 0.0)], stroke);
        }
        Glyph::RotateLeft | Glyph::RotateRight => {
            let mirror = if matches!(glyph, Glyph::RotateRight) {
                -1.0
            } else {
                1.0
            };
            // Arc de cercle presque fermé, ouvert en haut à gauche, et sa flèche.
            let arc: Vec<Pos2> = (0..=20)
                .map(|step| {
                    let angle = (-150.0 + step as f32 * 13.0).to_radians();
                    p(mirror * 6.5 * angle.cos(), 6.5 * angle.sin() + 1.0)
                })
                .collect();
            painter.line(arc, stroke);
            painter.line(
                vec![
                    p(mirror * -6.5, -6.0),
                    p(mirror * -6.5, -1.5),
                    p(mirror * -2.0, -1.5),
                ],
                stroke,
            );
        }
        Glyph::Trash => {
            painter.line_segment([p(-6.0, -4.0), p(6.0, -4.0)], stroke);
            painter.line(
                vec![p(-2.5, -4.0), p(-2.5, -6.5), p(2.5, -6.5), p(2.5, -4.0)],
                stroke,
            );
            painter.line(
                vec![p(-4.5, -4.0), p(-3.6, 6.5), p(3.6, 6.5), p(4.5, -4.0)],
                stroke,
            );
        }
    }
}

/// Bouton de la barre d'outils, teinté quand il est actif.
fn icon_button(ui: &mut egui::Ui, glyph: Glyph, active: bool, hint: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
    let (fill, ink) = if active {
        (style::sidebar_selected(), style::sidebar_selected_text())
    } else if response.hovered() {
        (style::tint(12), style::primary_text())
    } else {
        (Color32::TRANSPARENT, style::primary_text())
    };
    ui.painter().rect_filled(rect, 10.0, fill);
    paint_glyph(ui.painter(), rect.center(), glyph, ink);
    response.on_hover_text(hint)
}

/// Bouton rond flottant sur l'image (précédente, suivante).
fn float_button(ui: &mut egui::Ui, rect: Rect, radius: f32, glyph: Glyph, hint: &str) -> egui::Response {
    let response = ui
        .interact(rect, ui.scope_id().with(hint), Sense::click())
        .on_hover_text(hint);
    let painter = ui.painter();
    painter.circle_filled(rect.center(), radius, floating());
    painter.circle_stroke(rect.center(), radius, Stroke::new(1.0, floating_ring()));
    if response.hovered() {
        painter.circle_filled(rect.center(), radius, style::tint(14));
    }
    paint_glyph(painter, rect.center(), glyph, floating_ink());
    response
}

/// Bouton de la barre flottante d'affichage.
fn bar_button(ui: &mut egui::Ui, rect: Rect, glyph: Glyph, hint: &str) -> egui::Response {
    let response = ui
        .interact(rect, ui.scope_id().with(hint), Sense::click())
        .on_hover_text(hint);
    if response.hovered() {
        ui.painter().rect_filled(rect, 10.0, style::tint(14));
    }
    paint_glyph(ui.painter(), rect.center(), glyph, floating_ink());
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgb: [u8; 3]) -> Image {
        Image {
            width,
            height,
            rgba: (0..width * height)
                .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
                .collect(),
        }
    }

    #[test]
    fn images_ouvrables() {
        assert!(supported(Path::new("a/photo.JPG")));
        assert!(supported(Path::new("fond.png")));
        assert!(!supported(Path::new("dessin.svg")));
        assert!(!supported(Path::new("notes.txt")));
    }

    #[test]
    fn reduction_garde_les_proportions() {
        let small = shrink(&solid(400, 200, [10, 20, 30]), 100);
        assert_eq!((small.width, small.height), (100, 50));
        assert_eq!(&small.rgba[..4], &[10, 20, 30, 255]);
        // Déjà assez petite : inchangée.
        assert_eq!(shrink(&solid(40, 20, [1, 2, 3]), 100).width, 40);
    }

    #[test]
    fn couleurs_dominantes_distinctes() {
        let mut image = solid(10, 10, [200, 30, 30]);
        // Un tiers de bleu, et un rouge presque identique qui ne compte pas en plus.
        for pixel in image.rgba.chunks_exact_mut(4).take(33) {
            pixel.copy_from_slice(&[20, 40, 220, 255]);
        }
        image.rgba[396..400].copy_from_slice(&[205, 32, 30, 255]);
        let colors = dominant_colors(&image);
        assert_eq!(colors.len(), 2);
        assert_eq!(colors[0].r(), 200);
        assert_eq!(colors[1].b(), 220);
    }

    #[test]
    fn dossier_trie_et_filtre() {
        let dir = env::temp_dir().join(format!("ferrite-visionneuse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for name in ["b.png", "A.jpg", "c.txt", ".cachee.png"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        let names: Vec<String> = images_in(&dir).iter().map(|path| file_name(path)).collect();
        assert_eq!(names, ["A.jpg", "b.png"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
