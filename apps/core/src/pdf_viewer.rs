//! Visionneuse PDF Ferrite ; les pages sont rendues en arrière-plan par Poppler
//! (`pdftoppm`), puis affichées et zoomées par la carte graphique. Aucune commande
//! bloquante : chaque rendu part dans un fil et son résultat arrive par un canal.

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use eframe::egui::{self, Color32, CornerRadius, Key, TextureHandle, TextureOptions};

use crate::{
    files::{FilesState, PickerMode},
    style,
};

/// Côté de la page rendue pour l'ajustement, puis pour la lecture de près (zoom, largeur).
const RENDER_SIDE: u32 = 2400;
const RENDER_SIDE_SHARP: u32 = 3600;
/// Au-delà de ce zoom, une page rendue à `RENDER_SIDE` serait floue.
const SHARP_ZOOM: f32 = 1.5;
/// Nombre de pixels au-delà duquel un rendu est refusé (protège la mémoire).
const MAX_PIXELS: usize = 24_000_000;
/// Place laissée à la barre de défilement quand on ajuste à la largeur, en points.
const SCROLLBAR_ROOM: f32 = 14.0;
const ZOOM_STEP: f32 = 1.2;
const ZOOM_MAX: f32 = 4.0;
const ZOOM_MIN: f32 = 0.5;

struct PageImage {
    path: PathBuf,
    page: usize,
    page_count: usize,
    /// Côté du rendu (voir `RENDER_SIDE`), pour savoir s'il faut le refaire.
    side: u32,
    image: egui::ColorImage,
}

pub struct PdfViewerState {
    path: PathBuf,
    page_count: usize,
    page: usize,
    texture: Option<TextureHandle>,
    /// Page et côté du rendu affiché (0 : aucun).
    texture_page: usize,
    rendered_side: u32,
    loading: bool,
    /// Numéro de la demande en cours : la réponse d'une demande abandonnée est ignorée.
    request: u64,
    error: Option<String>,
    zoom: Option<f32>,
    fit_width: bool,
    /// Titre déjà envoyé à la fenêtre.
    title: String,
    /// Vrai quand la barre d'outils demande à ouvrir un document (traité par `ViewerApp`).
    pub request_open: bool,
    sender: Sender<(u64, Result<PageImage, String>)>,
    receiver: Receiver<(u64, Result<PageImage, String>)>,
}

impl PdfViewerState {
    pub fn new(path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            path,
            page_count: 0,
            page: 1,
            texture: None,
            texture_page: 0,
            rendered_side: 0,
            loading: false,
            request: 0,
            error: None,
            zoom: None,
            fit_width: false,
            title: String::new(),
            request_open: false,
            sender,
            receiver,
        }
    }

    /// Nouveau document : la page affichée et le rendu en cours sont oubliés.
    pub fn open(&mut self, path: PathBuf) {
        if path == self.path {
            return;
        }
        self.path = path;
        self.page_count = 0;
        self.page = 1;
        self.texture = None;
        self.texture_page = 0;
        self.rendered_side = 0;
        self.loading = false;
        self.request = self.request.wrapping_add(1);
        self.error = None;
        self.zoom = None;
        self.fit_width = false;
        self.title.clear();
    }

    fn request_page(&mut self, page: usize, side: u32, ctx: &egui::Context) {
        if page == 0 || (self.page_count > 0 && page > self.page_count) {
            return;
        }
        self.page = page;
        self.loading = true;
        self.error = None;
        self.request = self.request.wrapping_add(1);
        let serial = self.request;
        let path = self.path.clone();
        let known_page_count = (self.page_count > 0).then_some(self.page_count);
        let sender = self.sender.clone();
        let context = ctx.clone();
        thread::spawn(move || {
            let result = render_page(&path, page, side, known_page_count);
            let _ = sender.send((serial, result));
            context.request_repaint();
        });
    }

    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok((serial, result)) = self.receiver.try_recv() {
            // Une demande plus récente a pris la place : ce résultat ne sert plus.
            if serial != self.request {
                continue;
            }
            self.loading = false;
            match result {
                Ok(loaded) if loaded.path == self.path && loaded.page == self.page => {
                    self.page_count = loaded.page_count;
                    self.texture_page = loaded.page;
                    self.rendered_side = loaded.side;
                    self.show(ctx, loaded.image);
                    self.error = None;
                }
                Ok(_) => {}
                Err(error) => {
                    self.texture = None;
                    self.texture_page = 0;
                    self.rendered_side = 0;
                    self.error = Some(error);
                }
            }
        }
        if self.page_count == 0 && !self.loading && self.error.is_none() {
            self.request_page(self.page, self.wanted_side(), ctx);
        }
    }

    /// Envoie l'image à la carte graphique, en réutilisant la texture déjà en place.
    fn show(&mut self, ctx: &egui::Context, image: egui::ColorImage) {
        self.texture = Some(match self.texture.take() {
            Some(mut texture) => {
                texture.set(image, TextureOptions::LINEAR);
                texture
            }
            None => ctx.load_texture(
                format!("pdf:{}", self.path.display()),
                image,
                TextureOptions::LINEAR,
            ),
        });
    }

    /// Côté du rendu voulu : plus fin quand on lit de près (zoom, ajustement à la largeur).
    fn wanted_side(&self) -> u32 {
        if self.fit_width || self.zoom.is_some_and(|zoom| zoom > SHARP_ZOOM) {
            RENDER_SIDE_SHARP
        } else {
            RENDER_SIDE
        }
    }

    fn set_page(&mut self, page: usize, ctx: &egui::Context) {
        if page == 0 || page == self.page || (self.page_count > 0 && page > self.page_count) {
            return;
        }
        self.request_page(page, self.wanted_side(), ctx);
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input(|input| input.key_pressed(Key::PageDown) || input.key_pressed(Key::ArrowRight)) {
            self.set_page(self.page.saturating_add(1), ctx);
        }
        if ctx.input(|input| input.key_pressed(Key::PageUp) || input.key_pressed(Key::ArrowLeft)) {
            self.set_page(self.page.saturating_sub(1), ctx);
        }
        if ctx.input(|input| input.key_pressed(Key::Home)) && self.page != 1 {
            self.request_page(1, self.wanted_side(), ctx);
        }
        if ctx.input(|input| input.key_pressed(Key::End)) && self.page_count > 0 {
            self.set_page(self.page_count, ctx);
        }
        if ctx.input(|input| input.key_pressed(Key::Plus) || input.key_pressed(Key::Equals)) {
            self.change_zoom(ZOOM_STEP);
        }
        if ctx.input(|input| input.key_pressed(Key::Minus)) {
            self.change_zoom(1.0 / ZOOM_STEP);
        }
        if ctx.input(|input| input.key_pressed(Key::Num0)) {
            self.zoom = None;
            self.fit_width = false;
        }
        if ctx.input(|input| input.key_pressed(Key::W)) {
            self.toggle_width();
        }
        // Ctrl + molette : le zoom, comme dans les navigateurs.
        let wheel = ctx.input(|input| {
            if input.modifiers.command {
                input.smooth_scroll_delta.y
            } else {
                0.0
            }
        });
        if wheel > 0.5 {
            self.change_zoom(ZOOM_STEP);
        } else if wheel < -0.5 {
            self.change_zoom(1.0 / ZOOM_STEP);
        }
    }

    fn change_zoom(&mut self, factor: f32) {
        let zoom = self.zoom.unwrap_or(1.0) * factor;
        self.zoom = Some(zoom.clamp(ZOOM_MIN, ZOOM_MAX));
        self.fit_width = false;
    }

    fn toggle_width(&mut self) {
        self.fit_width = !self.fit_width;
        self.zoom = None;
    }

    /// Le titre de la fenêtre suit le nom du document, comme dans TextEdit.
    fn sync_title(&mut self, ctx: &egui::Context) {
        let title = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Lecteur PDF".to_owned());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.pump(&ctx);
        self.keys(&ctx);
        self.sync_title(&ctx);

        style::toolbar_with(ui, true, bar(), line(), |ui| self.toolbar(ui, &ctx));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(stage()))
            .show(ui, |ui| {
                if let Some(error) = self.error.clone() {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("Impossible d’ouvrir ce PDF")
                                    .size(18.0)
                                    .strong(),
                            );
                            ui.add_space(8.0);
                            ui.label(error);
                            ui.add_space(8.0);
                            ui.label("Vérifiez que le document est lisible et que Poppler est installé.");
                            if ui.button("Réessayer").clicked() {
                                self.error = None;
                                self.request_page(self.page, self.wanted_side(), &ctx);
                            }
                        });
                    });
                    return;
                }
                // Aucune page encore rendue : on patiente. Sinon la page précédente reste à
                // l'écran pendant le rendu de la suivante (la barre montre l'attente).
                if self.texture.is_none() {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                        ui.label("Chargement de la page…");
                    });
                    return;
                }
                // Rendu plus fin nécessaire (on lit de près) : demandé une seule fois.
                if !self.loading && self.rendered_side != self.wanted_side() {
                    self.request_page(self.page, self.wanted_side(), &ctx);
                }
                let Some(texture) = self.texture.clone() else {
                    return;
                };
                // Taille de la zone utile : le rectangle du panneau (celui de la Visionneuse),
                // et non `available_size`, qui ignore la barre posée au-dessus.
                let available = ui.max_rect().size();
                let page_size = texture.size_vec2();
                // Les marges au-dessus et au-dessous de la feuille restent visibles.
                let place = (available - egui::vec2(0.0, 28.0)).max(egui::vec2(120.0, 120.0));
                let fit = (place.x / page_size.x).min(place.y / page_size.y).max(0.05);
                let scale = match self.zoom {
                    Some(zoom) => fit * zoom,
                    // Ajusté à la largeur : la place de la barre de défilement est réservée.
                    None if self.fit_width => (available.x - SCROLLBAR_ROOM).max(120.0) / page_size.x,
                    None => fit,
                };
                let size = page_size * scale;
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_size(available);
                        ui.vertical_centered(|ui| {
                            ui.add_space(12.0);
                            // La feuille garde le format de la page : un cadre dans un
                            // agencement horizontal ne s'étire pas à toute la largeur.
                            ui.horizontal(|ui| {
                                let marge = ((available.x - size.x) / 2.0).max(0.0);
                                ui.add_space(marge);
                                egui::Frame::new()
                                    .fill(Color32::WHITE)
                                    .stroke(egui::Stroke::new(1.0, style::tint(24)))
                                    .corner_radius(CornerRadius::same(3))
                                    .shadow(egui::Shadow {
                                        color: Color32::from_black_alpha(50),
                                        offset: [0, 3],
                                        blur: 12,
                                        spread: 0,
                                    })
                                    .show(ui, |ui| {
                                        ui.add(egui::Image::from_texture(&texture).fit_to_exact_size(size));
                                    });
                            });
                            ui.add_space(16.0);
                        });
                    });
            });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(6.0);
        if toolbar_button(ui, "‹", "Page précédente (←)").clicked() {
            self.set_page(self.page.saturating_sub(1), ctx);
        }
        if toolbar_button(ui, "›", "Page suivante (→)").clicked() {
            self.set_page(self.page.saturating_add(1), ctx);
        }
        ui.add_space(8.0);
        ui.label("Page");
        let mut page = self.page as u32;
        let response = ui.add(
            egui::DragValue::new(&mut page)
                .range(1..=self.page_count.max(1) as u32)
                .speed(0.0)
                .fixed_decimals(0),
        );
        if response.changed() {
            self.set_page(page as usize, ctx);
        }
        ui.label(format!(
            "sur {}",
            if self.page_count == 0 {
                "…".into()
            } else {
                self.page_count.to_string()
            }
        ));
        ui.add_space(10.0);
        let fit_page = self.zoom.is_none() && !self.fit_width;
        if toolbar_toggle(ui, "Page", fit_page, "Ajuster la page entière (0)").clicked() {
            self.zoom = None;
            self.fit_width = false;
        }
        if toolbar_toggle(ui, "Largeur", self.fit_width, "Ajuster à la largeur (W)").clicked() {
            self.toggle_width();
        }
        ui.add_space(10.0);
        if ui
            .button("Ouvrir…")
            .on_hover_text("Ouvrir un document PDF (Ctrl+O)")
            .clicked()
        {
            self.request_open = true;
        }
        // Un nouveau rendu (plus fin) est en cours : la page affichée reste à l'écran.
        if self.loading {
            ui.add(egui::Spinner::new().size(14.0));
        }
        let title = self
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if toolbar_button(ui, "＋", "Zoom avant (+)").clicked() {
                self.change_zoom(ZOOM_STEP);
            }
            if toolbar_button(ui, "−", "Zoom arrière (−)").clicked() {
                self.change_zoom(1.0 / ZOOM_STEP);
            }
            ui.add_space(12.0);
            ui.label(egui::RichText::new(title).color(style::primary_text()));
            ui.add_space(20.0);
        });
    }
}

/// Rend une page en PNG avec Poppler, puis la décode (fil d'arrière-plan).
fn render_page(path: &Path, page: usize, side: u32, known_pages: Option<usize>) -> Result<PageImage, String> {
    let pages = match known_pages {
        Some(pages) => pages,
        None => page_count(path)?,
    };
    if page == 0 || page > pages {
        return Err(format!("La page {page} n’existe pas."));
    }
    let output_prefix = std::env::temp_dir().join(format!(
        "ferrite-pdf-{}-{}-{}",
        std::process::id(),
        page,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let output = Command::new("pdftoppm")
        .args([
            "-f",
            &page.to_string(),
            "-l",
            &page.to_string(),
            "-scale-to",
            &side.to_string(),
            "-singlefile",
            "-png",
        ])
        .arg(path)
        .arg(&output_prefix)
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "Poppler n’est pas installé (commande pdftoppm introuvable).".to_owned()
            } else {
                format!("Rendu PDF impossible : {error}")
            }
        })?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let _ = fs::remove_file(output_prefix.with_extension("png"));
        return Err(format!("Rendu PDF impossible : {message}"));
    }
    let rendered_path = output_prefix.with_extension("png");
    let image = match fs::read(&rendered_path) {
        Ok(bytes) => match decode_png(&bytes) {
            Ok(image) => image,
            Err(error) => {
                let _ = fs::remove_file(&rendered_path);
                return Err(error);
            }
        },
        Err(error) => {
            let _ = fs::remove_file(&rendered_path);
            return Err(format!("Lecture du PNG rendu impossible : {error}"));
        }
    };
    let _ = fs::remove_file(&rendered_path);
    Ok(PageImage {
        path: path.to_path_buf(),
        page,
        page_count: pages,
        side,
        image,
    })
}

fn page_count(path: &Path) -> Result<usize, String> {
    let output = Command::new("pdfinfo")
        .arg(path)
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "Poppler n’est pas installé (commande pdfinfo introuvable).".to_owned()
            } else {
                format!("Lecture des informations PDF impossible : {error}")
            }
        })?;
    if !output.status.success() {
        return Err(format!(
            "PDF illisible : {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_page_count(&String::from_utf8_lossy(&output.stdout))
        .filter(|pages| *pages > 0)
        .ok_or_else(|| "Le document PDF ne contient aucune page lisible.".to_owned())
}

fn parse_page_count(output: &str) -> Option<usize> {
    output.lines().find_map(|line| {
        line.strip_prefix("Pages:")
            .and_then(|value| value.trim().parse().ok())
    })
}

fn decode_png(data: &[u8]) -> Result<egui::ColorImage, String> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|error| format!("Image PDF invalide : {error}"))?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("Image PDF trop grande")?];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| format!("Image PDF invalide : {error}"))?;
    if info.width as usize * info.height as usize > MAX_PIXELS {
        return Err("La page rendue dépasse la taille maximale autorisée.".to_owned());
    }
    let pixels = &buffer[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .chunks(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .chunks(2)
            .flat_map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
            .collect(),
        png::ColorType::Grayscale => pixels
            .iter()
            .flat_map(|&value| [value, value, value, 255])
            .collect(),
        png::ColorType::Indexed => return Err("Format d’image PDF non pris en charge.".to_owned()),
    };
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [info.width as usize, info.height as usize],
        &rgba,
    ))
}

fn toolbar_button(ui: &mut egui::Ui, label: &str, hint: &str) -> egui::Response {
    ui.add_sized([34.0, 34.0], egui::Button::new(label).corner_radius(10.0))
        .on_hover_text(hint)
}

/// Bouton qui reste enfoncé pour montrer le mode d'affichage en cours.
fn toolbar_toggle(ui: &mut egui::Ui, label: &str, selected: bool, hint: &str) -> egui::Response {
    ui.add_sized(
        [66.0, 34.0],
        egui::Button::new(label).corner_radius(10.0).selected(selected),
    )
    .on_hover_text(hint)
}

fn bar() -> Color32 {
    style::themed(Color32::WHITE, Color32::from_rgb(0x1C, 0x1F, 0x25))
}

/// Scène autour de la page, comme celle de la Visionneuse : la page blanche s'y détache.
fn stage() -> Color32 {
    style::themed(
        Color32::from_rgb(0xED, 0xEF, 0xF2),
        Color32::from_rgb(0x12, 0x14, 0x18),
    )
}

fn line() -> Color32 {
    style::themed(
        Color32::from_rgb(0xE3, 0xE6, 0xEB),
        Color32::from_rgb(0x2C, 0x30, 0x38),
    )
}

pub fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

/// Visionneuse d'images ou lecteur PDF, avec le sélecteur de fichiers de Fichiers.
pub struct ViewerApp {
    kind: Kind,
    picker: Option<FilesState>,
    icons: style::Icons,
}

enum Kind {
    Images(Box<crate::viewer::ViewerState>),
    Pdf(Option<PdfViewerState>),
}

impl ViewerApp {
    pub fn new(path: Option<PathBuf>) -> Self {
        let kind = match path {
            Some(path) if supported(&path) => Kind::Pdf(Some(PdfViewerState::new(path))),
            path => Kind::Images(Box::new(crate::viewer::ViewerState::new(path))),
        };
        Self {
            kind,
            picker: None,
            icons: Default::default(),
        }
    }

    pub fn new_pdf(path: Option<PathBuf>) -> Self {
        Self {
            kind: Kind::Pdf(path.map(PdfViewerState::new)),
            picker: None,
            icons: Default::default(),
        }
    }

    pub fn open(&mut self, path: PathBuf) {
        if supported(&path) {
            self.open_pdf(path);
        } else {
            match &mut self.kind {
                Kind::Images(viewer) => viewer.open(path),
                Kind::Pdf(_) => *self = Self::new(Some(path)),
            }
        }
    }

    pub fn open_pdf(&mut self, path: PathBuf) {
        match &mut self.kind {
            // Même document ouvert à nouveau : la page et le zoom sont conservés.
            Kind::Pdf(Some(viewer)) => viewer.open(path),
            _ => *self = Self::new_pdf(Some(path)),
        }
    }

    fn open_picker(&mut self) {
        // Le sélecteur s'ouvre sur le dossier du document en cours, s'il y en a un.
        let start = match &self.kind {
            Kind::Pdf(Some(viewer)) => viewer
                .path
                .parent()
                .filter(|path| path.is_dir())
                .map(Path::to_path_buf),
            _ => None,
        };
        self.picker = Some(FilesState::picker(PickerMode::Open, start.as_deref(), ""));
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.picker.is_none()
            && !ctx.egui_wants_keyboard_input()
            && ctx.input(|input| input.modifiers.command && input.key_pressed(Key::O))
        {
            self.open_picker();
        }
        let mut wants_open = false;
        match &mut self.kind {
            Kind::Images(viewer) => viewer.update(ui),
            Kind::Pdf(Some(viewer)) => {
                viewer.update(ui);
                wants_open = std::mem::take(&mut viewer.request_open);
            }
            Kind::Pdf(None) => wants_open = empty_state(ui, &mut self.icons),
        }
        if wants_open {
            self.open_picker();
        }
        self.picker_sheet(&ctx);
    }

    /// Sélecteur de fichier dans une feuille au-dessus de la fenêtre (comme le lecteur vidéo).
    fn picker_sheet(&mut self, ctx: &egui::Context) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        let screen = ctx.content_rect();
        let size = egui::vec2(
            (screen.width() - 48.0).clamp(420.0, 900.0),
            (screen.height() - 64.0).clamp(300.0, 560.0),
        );
        egui::Modal::new(egui::Id::unique("lecteur-pdf-selecteur"))
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
}

/// Fenêtre sans document : un bouton pour en choisir un (rend vrai si on l'a demandé).
fn empty_state(ui: &mut egui::Ui, icons: &mut style::Icons) -> bool {
    let mut open = false;
    style::toolbar_with(ui, true, bar(), line(), |ui| {
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Lecteur PDF").strong());
    });
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(stage()))
        .show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(84.0, 84.0), egui::Sense::hover());
                    icons.paint(ui, "app-pdf", rect, style::IconStyle::Color);
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new("Aucun document ouvert")
                            .size(18.0)
                            .strong()
                            .color(style::primary_text()),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Choisissez un document PDF à lire.")
                            .color(style::text_secondary()),
                    );
                    ui.add_space(16.0);
                    if ui
                        .add_sized([196.0, 40.0], egui::Button::new("Choisir un document"))
                        .on_hover_text("Ouvrir un document PDF (Ctrl+O)")
                        .clicked()
                    {
                        open = true;
                    }
                });
            });
        });
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifie_les_pdf_sans_tenir_compte_de_la_casse() {
        assert!(supported(Path::new("Documents/rapport.PDF")));
        assert!(!supported(Path::new("Documents/rapport.png")));
        assert!(!supported(Path::new("Documents/rapport.pdf.txt")));
    }

    #[test]
    fn le_lancement_pdf_reste_distinct_sans_document() {
        assert!(matches!(ViewerApp::new_pdf(None).kind, Kind::Pdf(None)));
        assert!(matches!(
            ViewerApp::new(Some(PathBuf::from("Documents/rapport.pdf"))).kind,
            Kind::Pdf(Some(_))
        ));
        assert!(matches!(
            ViewerApp::new(Some(PathBuf::from("Images/photo.png"))).kind,
            Kind::Images(_)
        ));
    }

    #[test]
    fn demande_un_rendu_plus_fin_quand_on_lit_de_pres() {
        let mut etat = PdfViewerState::new(PathBuf::from("Documents/rapport.pdf"));
        assert_eq!(etat.wanted_side(), RENDER_SIDE);
        etat.change_zoom(ZOOM_STEP);
        assert_eq!(etat.wanted_side(), RENDER_SIDE);
        etat.change_zoom(2.0);
        assert_eq!(etat.wanted_side(), RENDER_SIDE_SHARP);
        etat.toggle_width();
        assert_eq!(etat.zoom, None);
        assert_eq!(etat.wanted_side(), RENDER_SIDE_SHARP);
        etat.toggle_width();
        assert_eq!(etat.wanted_side(), RENDER_SIDE);
        etat.change_zoom(ZOOM_STEP);
        assert_eq!(etat.wanted_side(), RENDER_SIDE);
    }

    #[test]
    fn un_zoom_reste_dans_les_bornes() {
        let mut etat = PdfViewerState::new(PathBuf::from("Documents/rapport.pdf"));
        for _ in 0..20 {
            etat.change_zoom(ZOOM_STEP);
        }
        assert_eq!(etat.zoom, Some(ZOOM_MAX));
        for _ in 0..40 {
            etat.change_zoom(1.0 / ZOOM_STEP);
        }
        assert_eq!(etat.zoom, Some(ZOOM_MIN));
    }

    #[test]
    fn compte_les_pages_dans_les_metadonnees() {
        let output = "Title: document\nPages:           12\nPage size: 612 x 792 pts\n";
        assert_eq!(parse_page_count(output), Some(12));
        assert_eq!(parse_page_count("Title: document\nPages: -1\n"), None);
    }

    #[test]
    fn decode_une_page_png_rendue() {
        let mut data = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut data, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[12, 34, 56, 255]).unwrap();
        }
        let image = decode_png(&data).unwrap();
        assert_eq!(image.size, [1, 1]);
        assert_eq!(image.pixels, [Color32::from_rgb(12, 34, 56)]);
    }
}
