//! Lecteur PDF Ferrite. Toutes les pages défilent à la suite ; seules celles à l'écran sont
//! rendues par Poppler (`pdftoppm`), à la taille exacte où elles s'affichent, donc nettes à
//! tout zoom. Colonne de miniatures, recherche dans le texte, et liste des documents récents
//! qui rouvre chacun à la page où on l'avait laissé. Le texte se sélectionne à la souris, se
//! copie, se surligne, se souligne, se barre et se commente ; notes, signets et plan du
//! document sont dans la colonne de gauche (`annotate.rs`). Aucune commande bloquante : chaque appel
//! à Poppler part dans un fil et son résultat revient par un canal.

mod annotate;
mod annotations;
mod follow;
mod links;
mod poppler;
mod recents;
mod search;
mod selection;
mod tabs;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use eframe::egui::{
    self, Color32, CornerRadius, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2, vec2,
};

use self::{
    annotations::{Kind as MarkKind, Notes},
    poppler::{DocInfo, OutlineItem, Word},
    recents::Recent,
    search::Match,
    selection::Selection,
};
use crate::{
    files::{FilesState, PickerMode},
    kit, style,
};

/// Un point de PDF (1/72 de pouce) en points d'écran (1/96 de pouce) : zoom de 100 %.
const POINT: f32 = 96.0 / 72.0;
/// Marge autour des pages et écart entre deux pages, en points d'écran.
const MARGIN: f32 = 20.0;
const GAP: f32 = 16.0;
/// Place laissée à la barre de défilement quand on ajuste à la largeur.
const SCROLLBAR_ROOM: f32 = 12.0;
/// Paliers du zoom (+ / −) et bornes, en part de la taille réelle.
const ZOOM_STEPS: [f32; 13] = [
    0.25, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0,
];
const ZOOM_MIN: f32 = 0.25;
const ZOOM_MAX: f32 = 4.0;
/// Ajustement automatique : la largeur de la fenêtre, sans dépasser ce zoom.
const AUTO_MAX: f32 = 1.25;
/// Les rendus se font par paliers de largeur : un léger zoom ne relance pas Poppler.
const WIDTH_STEP: u32 = 128;
const MAX_RENDER_WIDTH: u32 = 6000;
/// Rendus de pages et de miniatures menés en même temps.
const MAX_JOBS: usize = 3;
const MAX_THUMB_JOBS: usize = 2;
/// Pages gardées en mémoire de part et d'autre de celles à l'écran.
const KEEP_AROUND: usize = 3;
/// Colonne des miniatures.
const SIDEBAR_WIDTH: f32 = 216.0;
const THUMB_WIDTH: f32 = 132.0;
const THUMB_LABEL: f32 = 24.0;
const THUMB_GAP: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Zoom {
    /// Largeur de la fenêtre, sans dépasser `AUTO_MAX` (réglage de départ).
    Auto,
    Width,
    Page,
    /// Part de la taille réelle (1,0 : 100 %).
    Factor(f32),
}

enum Loaded {
    Info(Result<DocInfo, String>),
    Page {
        page: usize,
        width: u32,
        image: Result<egui::ColorImage, String>,
    },
    Thumb {
        page: usize,
        image: Result<egui::ColorImage, String>,
    },
    Words(Result<Vec<Vec<Word>>, String>),
    Notes {
        key: Option<String>,
        notes: Notes,
    },
    Outline(Vec<OutlineItem>),
    Links(Vec<Vec<links::Link>>),
}

/// Ce que fait un clic sur la page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    /// Lire et sélectionner du texte.
    Read,
    /// Surligneur : chaque sélection est marquée aussitôt.
    Marker,
    /// Le prochain clic pose une note.
    Note,
}

/// Contenu de la colonne de gauche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Pages,
    Notes,
    Outline,
}

/// Annotation dont la bulle de commentaire est ouverte.
struct Editing {
    id: u64,
    /// Donner le clavier au champ de commentaire à la prochaine image.
    focus: bool,
    /// L'état d'avant est déjà gardé pour Ctrl+Z (une frappe ne le garde qu'une fois).
    undo_kept: bool,
}

struct Rendered {
    texture: TextureHandle,
    width: u32,
}

/// Où amener la vue dès que la mise en page est connue.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Jump {
    /// Haut de la page (numérotée depuis 0).
    Page(usize),
    /// Un résultat de recherche : page et hauteur en points PDF, centrée dans la vue.
    Point(usize, f32),
    /// Décalage brut, en points d'écran.
    Offset(f32),
    /// Cible d'un renvoi : page et hauteur en points PDF, placée près du haut de la vue.
    Top(usize, f32),
}

#[derive(Default)]
struct Search {
    open: bool,
    focus: bool,
    query: String,
    /// Requête des résultats affichés.
    searched: String,
    words: Option<Vec<Vec<Word>>>,
    loading: bool,
    error: Option<String>,
    matches: Vec<Match>,
    current: usize,
}

/// Mise en page de toutes les pages pour un zoom donné.
struct Layout {
    tops: Vec<f32>,
    sizes: Vec<egui::Vec2>,
    width: f32,
    height: f32,
}

impl Layout {
    fn new(pages: &[egui::Vec2], scale: f32) -> Self {
        let mut tops = Vec::with_capacity(pages.len());
        let mut sizes = Vec::with_capacity(pages.len());
        let mut y = MARGIN;
        let mut widest = 0.0f32;
        for page in pages {
            let size = *page * scale;
            tops.push(y);
            sizes.push(size);
            y += size.y + GAP;
            widest = widest.max(size.x);
        }
        Self {
            tops,
            sizes,
            width: widest + 2.0 * MARGIN,
            height: y - GAP + MARGIN,
        }
    }

    /// Page sous la hauteur `y` (la plus proche si `y` tombe entre deux pages).
    fn page_at(&self, y: f32) -> usize {
        self.tops.partition_point(|top| *top <= y).saturating_sub(1)
    }

    /// Pages qui touchent la bande `top..bottom`.
    fn pages_between(&self, top: f32, bottom: f32) -> std::ops::Range<usize> {
        let first = self.page_at(top);
        let last = self.tops.partition_point(|start| *start < bottom);
        first..last.max(first + 1).min(self.tops.len())
    }
}

/// Largeur de rendu (en pixels) d'une page affichée sur `width` pixels : palier au-dessus,
/// borné pour que l'image reste raisonnable en mémoire.
fn render_width(width: f32, page: egui::Vec2) -> u32 {
    let wanted = (width.ceil() as u32).div_ceil(WIDTH_STEP).max(1) * WIDTH_STEP;
    let ratio = (page.y / page.x.max(1.0)).max(0.01);
    let limit = ((poppler::MAX_PIXELS as f32 * 0.9) / ratio).sqrt() as u32;
    wanted.clamp(WIDTH_STEP, limit.clamp(WIDTH_STEP, MAX_RENDER_WIDTH))
}

pub struct PdfViewerState {
    /// Numéro unique de l'onglet (identifiants egui propres à chaque document).
    id: u64,
    path: PathBuf,
    /// Numéro du document ouvert : les réponses pour un document précédent sont ignorées.
    generation: u64,
    sender: Sender<(u64, Loaded)>,
    receiver: Receiver<(u64, Loaded)>,
    info: Option<DocInfo>,
    error: Option<String>,
    pages: HashMap<usize, Rendered>,
    pending: HashSet<usize>,
    failed: HashSet<usize>,
    thumbs: HashMap<usize, TextureHandle>,
    pending_thumbs: HashSet<usize>,
    zoom: Zoom,
    /// Échelle et décalage de la vue à l'image précédente.
    scale: f32,
    offset: egui::Vec2,
    view: egui::Vec2,
    /// Page et part de la page au centre de la vue, pour la garder en place quand l'échelle
    /// change (zoom, fenêtre redimensionnée).
    anchor: Option<(usize, f32)>,
    jump: Option<Jump>,
    /// Page affichée, numérotée depuis 0.
    current: usize,
    /// Page où rouvrir le document, numérotée depuis 1.
    start_page: usize,
    sidebar: bool,
    /// La colonne doit montrer la miniature de la page en cours.
    follow_thumb: bool,
    page_field: String,
    search: Search,
    title: String,
    keyboard_zoom_off: bool,
    info_requested: bool,
    /// Texte choisi à la souris, et vrai tant que le bouton est enfoncé.
    selection: Option<Selection>,
    selecting: bool,
    /// Annotations et signets du document, et le nom de leur fichier (son empreinte).
    notes: Notes,
    notes_key: Option<String>,
    undo: Vec<Notes>,
    redo: Vec<Notes>,
    tool: Tool,
    /// Encre et trait du surligneur : les derniers choisis.
    ink: usize,
    mark: MarkKind,
    editing: Option<Editing>,
    side: Side,
    outline: Vec<OutlineItem>,
    /// Liens de chaque page (renvois, adresses web).
    links: Vec<Vec<links::Link>>,
    /// Positions (décalage de la vue) quittées en suivant un renvoi, pour y revenir.
    back: Vec<(f32, usize)>,
    /// Message bref et l'heure de son apparition.
    message: Option<(String, f64)>,
    /// Page et point (en points de PDF) du dernier clic droit.
    menu_at: Option<(usize, Pos2)>,
    /// Pages à l'écran et leur rectangle, à cette image.
    page_rects: Vec<(usize, Rect)>,
    /// Demandes de la barre d'outils, traitées par `ViewerApp`.
    pub request_open: bool,
    pub request_recent: Option<PathBuf>,
    pub request_clear_recents: bool,
    /// Lien vers un autre document PDF, ouvert par `ViewerApp` dans un nouvel onglet.
    pub request_file: Option<follow::FileRequest>,
    pub request_tab: Option<tabs::TabAction>,
}

impl PdfViewerState {
    pub fn new(path: PathBuf, start_page: usize) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let (sender, receiver) = mpsc::channel();
        Self {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            path,
            generation: 0,
            sender,
            receiver,
            info: None,
            error: None,
            pages: HashMap::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
            thumbs: HashMap::new(),
            pending_thumbs: HashSet::new(),
            zoom: Zoom::Auto,
            scale: 0.0,
            offset: egui::Vec2::ZERO,
            view: egui::Vec2::ZERO,
            anchor: None,
            jump: None,
            current: 0,
            start_page: start_page.max(1),
            sidebar: true,
            follow_thumb: true,
            page_field: String::new(),
            search: Search::default(),
            title: String::new(),
            keyboard_zoom_off: false,
            info_requested: false,
            selection: None,
            selecting: false,
            notes: Notes::default(),
            notes_key: None,
            undo: Vec::new(),
            redo: Vec::new(),
            tool: Tool::Read,
            ink: 0,
            mark: MarkKind::Highlight,
            editing: None,
            side: Side::Pages,
            outline: Vec::new(),
            links: Vec::new(),
            back: Vec::new(),
            message: None,
            menu_at: None,
            page_rects: Vec::new(),
            request_open: false,
            request_recent: None,
            request_clear_recents: false,
            request_file: None,
            request_tab: None,
        }
    }

    /// Page lue, numérotée depuis 1 (0 tant que le document n'est pas chargé).
    pub fn page(&self) -> usize {
        if self.info.is_some() { self.current + 1 } else { 0 }
    }

    /// Autre document, dans un nouvel onglet : la colonne, le mode de zoom et l'encre du
    /// surligneur sont ceux choisis dans `other`.
    pub fn new_like(path: PathBuf, start_page: usize, other: &Self) -> Self {
        let mut state = Self::new(path, start_page);
        state.zoom = other.zoom;
        state.sidebar = other.sidebar;
        state.side = other.side;
        state.ink = other.ink;
        state.mark = other.mark;
        state.keyboard_zoom_off = other.keyboard_zoom_off;
        state
    }

    /// L'onglet redevient visible : le titre de la fenêtre doit suivre.
    pub fn shown_again(&mut self) {
        self.title.clear();
    }

    fn spawn(&self, ctx: &egui::Context, job: impl FnOnce(&Path) -> Loaded + Send + 'static) {
        let (path, generation, sender, ctx) = (
            self.path.clone(),
            self.generation,
            self.sender.clone(),
            ctx.clone(),
        );
        thread::spawn(move || {
            let _ = sender.send((generation, job(&path)));
            ctx.request_repaint();
        });
    }

    fn load_info(&mut self, ctx: &egui::Context) {
        self.error = None;
        self.spawn(ctx, |path| Loaded::Info(poppler::load_info(path)));
    }

    /// À l'ouverture : les annotations gardées pour ce document, son plan, et son texte
    /// (sélection et recherche).
    fn load_extras(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, |path| {
            let key = annotations::fingerprint(path);
            let notes = key.as_deref().map(annotations::load).unwrap_or_default();
            Loaded::Notes { key, notes }
        });
        self.spawn(ctx, |path| Loaded::Outline(poppler::load_outline(path)));
        self.spawn(ctx, |path| Loaded::Links(links::load(path)));
        self.search.loading = true;
        self.spawn(ctx, |path| Loaded::Words(poppler::load_words(path)));
    }

    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok((generation, loaded)) = self.receiver.try_recv() {
            if generation != self.generation {
                continue;
            }
            match loaded {
                Loaded::Info(Ok(info)) => {
                    let last = info.pages.len().saturating_sub(1);
                    self.current = self.start_page.saturating_sub(1).min(last);
                    self.jump = Some(Jump::Page(self.current));
                    self.follow_thumb = true;
                    self.info = Some(info);
                }
                Loaded::Info(Err(error)) => self.error = Some(error),
                Loaded::Page { page, width, image } => {
                    self.pending.remove(&page);
                    match image {
                        Ok(image) => {
                            let name = format!("pdf-page-{page}");
                            match self.pages.get_mut(&page) {
                                Some(rendered) => {
                                    rendered.texture.set(image, TextureOptions::LINEAR);
                                    rendered.width = width;
                                }
                                None => {
                                    let texture = ctx.load_texture(name, image, TextureOptions::LINEAR);
                                    self.pages.insert(page, Rendered { texture, width });
                                }
                            }
                        }
                        Err(_) => {
                            self.failed.insert(page);
                        }
                    }
                }
                Loaded::Thumb { page, image } => {
                    self.pending_thumbs.remove(&page);
                    if let Ok(image) = image {
                        let texture =
                            ctx.load_texture(format!("pdf-miniature-{page}"), image, TextureOptions::LINEAR);
                        self.thumbs.insert(page, texture);
                    }
                }
                Loaded::Words(result) => {
                    self.search.loading = false;
                    match result {
                        Ok(words) => {
                            self.search.words = Some(words);
                            self.search.searched.clear();
                            self.demo();
                        }
                        Err(error) => self.search.error = Some(error),
                    }
                }
                Loaded::Notes { key, notes } => {
                    self.notes_key = key;
                    // Une annotation posée avant la fin de la lecture n'est pas perdue.
                    if self.notes.is_empty() {
                        self.notes = notes;
                    }
                }
                Loaded::Outline(outline) => self.outline = outline,
                Loaded::Links(links) => {
                    self.links = links;
                    self.demo_link(ctx);
                }
            }
        }
    }

    fn page_count(&self) -> usize {
        self.info.as_ref().map_or(0, |info| info.pages.len())
    }

    fn go_to(&mut self, page: usize) {
        let count = self.page_count();
        if count == 0 {
            return;
        }
        let page = page.min(count - 1);
        self.jump = Some(Jump::Page(page));
        self.current = page;
        self.follow_thumb = true;
    }

    fn effective_zoom(&self) -> f32 {
        if self.scale > 0.0 { self.scale / POINT } else { 1.0 }
    }

    fn set_zoom(&mut self, zoom: Zoom) {
        self.zoom = match zoom {
            Zoom::Factor(factor) => Zoom::Factor(factor.clamp(ZOOM_MIN, ZOOM_MAX)),
            other => other,
        };
    }

    /// Palier de zoom suivant (`up`) ou précédent, depuis le zoom affiché.
    fn step_zoom(&mut self, up: bool) {
        let now = self.effective_zoom();
        let next = if up {
            ZOOM_STEPS
                .iter()
                .copied()
                .find(|step| *step > now * 1.01)
                .unwrap_or(ZOOM_MAX)
        } else {
            ZOOM_STEPS
                .iter()
                .rev()
                .copied()
                .find(|step| *step < now * 0.99)
                .unwrap_or(ZOOM_MIN)
        };
        self.set_zoom(Zoom::Factor(next));
    }

    fn scale_for(&self, view: egui::Vec2) -> f32 {
        let Some(info) = &self.info else {
            return 1.0;
        };
        let widest = info.pages.iter().map(|page| page.x).fold(1.0, f32::max);
        let tallest = info.pages.iter().map(|page| page.y).fold(1.0, f32::max);
        let width_fit = (view.x - 2.0 * MARGIN - SCROLLBAR_ROOM).max(80.0) / widest;
        let scale = match self.zoom {
            Zoom::Auto => width_fit.min(AUTO_MAX * POINT),
            Zoom::Width => width_fit,
            Zoom::Page => width_fit.min((view.y - 2.0 * MARGIN).max(80.0) / tallest),
            Zoom::Factor(factor) => factor * POINT,
        };
        scale.clamp(ZOOM_MIN * POINT, ZOOM_MAX * POINT)
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let (command, shift) = ctx.input(|input| (input.modifiers.command, input.modifiers.shift));
        let pressed = |key: Key| ctx.input(|input| input.key_pressed(key));
        if command && pressed(Key::F) {
            self.open_search(ctx);
        }
        if pressed(Key::Escape) {
            // Du plus proche au plus lointain : la bulle, la sélection, l'outil, la recherche.
            if self.editing.is_some() {
                self.close_editor();
            } else if self.selection.is_some() {
                self.selection = None;
            } else if self.tool != Tool::Read {
                self.tool = Tool::Read;
            } else {
                self.search.open = false;
            }
        }
        // Ctrl + molette, ou pincement du pavé tactile.
        let pinch = ctx.input(|input| input.zoom_delta());
        if (pinch - 1.0).abs() > 0.001 && self.info.is_some() {
            self.set_zoom(Zoom::Factor(self.effective_zoom() * pinch));
        }
        if typing || self.info.is_none() {
            return;
        }
        // Ctrl+C arrive comme un événement « copier », pas comme une touche.
        let copy = ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy))
        });
        if copy || (command && pressed(Key::C)) {
            self.copy_selection(ctx);
        }
        if command && pressed(Key::A) {
            self.select_page();
        }
        if command && pressed(Key::D) {
            self.toggle_bookmark(ctx, self.current);
        }
        if command && pressed(Key::Z) {
            if shift {
                self.redo(ctx);
            } else {
                self.undo(ctx);
            }
        }
        if command && pressed(Key::Y) {
            self.redo(ctx);
        }
        if !command && pressed(Key::H) {
            self.tool = if self.tool == Tool::Marker {
                Tool::Read
            } else {
                Tool::Marker
            };
        }
        if !command && pressed(Key::N) {
            self.tool = if self.tool == Tool::Note {
                Tool::Read
            } else {
                Tool::Note
            };
        }
        if pressed(Key::Plus) || pressed(Key::Equals) {
            self.step_zoom(true);
        }
        if pressed(Key::Minus) {
            self.step_zoom(false);
        }
        if pressed(Key::Num0) {
            self.set_zoom(if command { Zoom::Factor(1.0) } else { Zoom::Page });
        }
        if !command && pressed(Key::W) {
            self.set_zoom(Zoom::Width);
        }
        if pressed(Key::F9) {
            self.sidebar = !self.sidebar;
            self.follow_thumb = true;
        }
        let screen = (self.view.y - 60.0).max(80.0);
        if pressed(Key::PageDown) || (!shift && pressed(Key::Space)) {
            self.jump = Some(Jump::Offset(self.offset.y + screen));
        }
        if pressed(Key::PageUp) || (shift && pressed(Key::Space)) {
            self.jump = Some(Jump::Offset(self.offset.y - screen));
        }
        if pressed(Key::ArrowDown) {
            self.jump = Some(Jump::Offset(self.offset.y + 60.0));
        }
        if pressed(Key::ArrowUp) {
            self.jump = Some(Jump::Offset(self.offset.y - 60.0));
        }
        if pressed(Key::ArrowRight) {
            self.go_to(self.current + 1);
        }
        let alt = ctx.input(|input| input.modifiers.alt);
        if alt && pressed(Key::ArrowLeft) {
            self.go_back();
        } else if pressed(Key::ArrowLeft) {
            self.go_to(self.current.saturating_sub(1));
        }
        if pressed(Key::Home) {
            self.go_to(0);
        }
        if pressed(Key::End) {
            self.go_to(usize::MAX);
        }
    }

    fn open_search(&mut self, ctx: &egui::Context) {
        self.search.open = true;
        self.search.focus = true;
        if self.search.words.is_none() && !self.search.loading && self.info.is_some() {
            self.search.loading = true;
            self.search.error = None;
            self.spawn(ctx, |path| Loaded::Words(poppler::load_words(path)));
        }
    }

    /// Relance la recherche quand la requête change ; amène la vue au premier résultat
    /// à partir de la page lue.
    fn refresh_search(&mut self) {
        let Some(words) = &self.search.words else {
            return;
        };
        if self.search.query == self.search.searched {
            return;
        }
        self.search.searched = self.search.query.clone();
        self.search.matches = search::find(words, &self.search.query);
        self.search.current = self
            .search
            .matches
            .iter()
            .position(|found| found.page >= self.current)
            .unwrap_or(0);
        self.show_match();
    }

    fn show_match(&mut self) {
        if let Some(found) = self.search.matches.get(self.search.current) {
            let y = found.rects.first().map_or(0.0, |rect| rect.center().y);
            self.jump = Some(Jump::Point(found.page, y));
            self.current = found.page;
            self.follow_thumb = true;
        }
    }

    fn next_match(&mut self, forward: bool) {
        let count = self.search.matches.len();
        if count == 0 {
            return;
        }
        self.search.current = if forward {
            (self.search.current + 1) % count
        } else {
            (self.search.current + count - 1) % count
        };
        self.show_match();
    }

    /// Le titre de la fenêtre suit le nom du document, comme dans TextEdit.
    fn sync_title(&mut self, ctx: &egui::Context) {
        let title = file_name(&self.path);
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui, recents: &[Recent], tabs: Option<&tabs::Strip>) {
        let ctx = ui.ctx().clone();
        if !self.keyboard_zoom_off {
            // + et − zooment la page, pas toute l'interface.
            ctx.options_mut(|options| options.zoom_with_keyboard = false);
            self.keyboard_zoom_off = true;
        }
        if !self.info_requested {
            self.info_requested = true;
            self.load_info(&ctx);
            self.load_extras(&ctx);
        }
        self.pump(&ctx);
        self.keys(&ctx);
        self.sync_title(&ctx);
        self.refresh_search();

        style::toolbar_with(ui, true, style::header_color(), Color32::TRANSPARENT, |ui| {
            self.toolbar(ui, &ctx, recents)
        });
        if let Some(strip) = tabs {
            self.request_tab = tabs::strip(ui, strip, self.id);
        }
        if self.search.open {
            self.search_bar(ui, &ctx);
        }
        if self.sidebar && self.info.is_some() {
            self.side_panel(ui, &ctx);
        }
        // Thème « Fusion » : barre sur le chrome, contenu dans la carte encastrée.
        style::inset_panel(ui, true, false, |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(style::stage()))
                .show(ui, |ui| {
                    if let Some(error) = self.error.clone() {
                        self.error_screen(ui, &ctx, &error);
                    } else if self.info.is_none() {
                        ui.centered_and_justified(|ui| {
                            ui.horizontal_centered(|ui| {
                                ui.spinner();
                                ui.label("Ouverture du document…");
                            });
                        });
                    } else {
                        self.pages_view(ui, &ctx);
                    }
                });
        });
    }

    fn error_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, error: &str) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() / 2.0 - 70.0).max(0.0));
                ui.label(
                    egui::RichText::new("Impossible d’ouvrir ce document")
                        .size(18.0)
                        .strong()
                        .color(style::primary_text()),
                );
                ui.add_space(8.0);
                ui.label(egui::RichText::new(error).color(style::text_secondary()));
                ui.add_space(14.0);
                if kit::button(ui, "Réessayer", true).clicked() {
                    self.load_info(ctx);
                }
            });
        });
    }

    fn pages_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let view_rect = ui.max_rect();
        let view = view_rect.size();
        let scale = self.scale_for(view);
        let layout = Layout::new(
            &self
                .info
                .as_ref()
                .map(|info| info.pages.clone())
                .unwrap_or_default(),
            scale,
        );

        // Où placer la vue : saut demandé, sinon même endroit du document si l'échelle a changé.
        let mut offset = None;
        match self.jump.take() {
            Some(Jump::Page(page)) => offset = layout.tops.get(page).map(|top| top - GAP / 2.0),
            Some(Jump::Point(page, y)) => {
                offset = layout.tops.get(page).map(|top| top + y * scale - view.y / 2.0)
            }
            Some(Jump::Offset(y)) => offset = Some(y),
            Some(Jump::Top(page, y)) => offset = layout.tops.get(page).map(|top| top + y * scale - GAP),
            None => {
                if (scale - self.scale).abs() > 0.0001
                    && self.scale > 0.0
                    && let Some((page, part)) = self.anchor
                    && let (Some(top), Some(size)) = (layout.tops.get(page), layout.sizes.get(page))
                {
                    offset = Some(top + part * size.y - view.y / 2.0);
                }
            }
        }
        let bottom = (layout.height - view.y).max(0.0);
        let offset = offset.map(|y| y.clamp(0.0, bottom));
        let forced = offset.is_some();
        self.scale = scale;
        self.view = view;

        let mut area = egui::ScrollArea::both()
            .auto_shrink([false, false])
            .id_salt(("pages-pdf", self.id));
        if let Some(y) = offset {
            area = area.vertical_scroll_offset(y);
        }
        let pixels = ctx.pixels_per_point();
        let output = area.show_viewport(ui, |ui, viewport| {
            let width = layout.width.max(viewport.width());
            ui.set_min_size(vec2(width, layout.height));
            let origin = ui.min_rect().min;
            let visible = layout.pages_between(viewport.top(), viewport.bottom());
            let around = layout.pages_between(
                viewport.top() - viewport.height(),
                viewport.bottom() + viewport.height(),
            );
            self.page_rects.clear();
            for page in visible.clone() {
                let size = layout.sizes[page];
                let rect =
                    Rect::from_min_size(origin + vec2((width - size.x) / 2.0, layout.tops[page]), size);
                self.page_rects.push((page, rect));
                self.page_input(ui, ctx, page, rect);
                self.paint_page(ui, page, rect);
            }
            // Rendus à la taille d'affichage : d'abord les pages à l'écran, puis leurs voisines
            // (en laissant toujours une place libre pour une page qui arriverait à l'écran).
            let neighbours = around.clone().filter(|page| !visible.contains(page));
            for (page, nearby) in visible
                .clone()
                .map(|page| (page, false))
                .chain(neighbours.map(|page| (page, true)))
            {
                let limit = if nearby { MAX_JOBS - 1 } else { MAX_JOBS };
                if self.pending.len() >= limit {
                    continue;
                }
                let original = self
                    .info
                    .as_ref()
                    .map_or(layout.sizes[page], |info| info.pages[page]);
                let wanted = render_width(layout.sizes[page].x * pixels, original);
                let have = self.pages.get(&page).map(|rendered| rendered.width);
                if have != Some(wanted) && !self.pending.contains(&page) && !self.failed.contains(&page) {
                    self.pending.insert(page);
                    self.spawn(ctx, move |path| Loaded::Page {
                        page,
                        width: wanted,
                        image: poppler::render(path, page + 1, wanted),
                    });
                }
            }
            // Mémoire : les pages loin de la vue sont oubliées.
            let keep = around.start.saturating_sub(KEEP_AROUND)..around.end + KEEP_AROUND;
            self.pages.retain(|page, _| keep.contains(page));
        });
        let offset = output.state.clamped_offset();
        let moved = (offset.y - self.offset.y).abs() > 0.5;
        let jumped = forced;
        self.offset = offset;
        let center = offset.y + view.y / 2.0;
        let page = layout.page_at(center);
        if let (Some(top), Some(size)) = (layout.tops.get(page), layout.sizes.get(page)) {
            self.anchor = Some((page, (center - top) / size.y.max(1.0)));
        }
        // Page lue : celle qui occupe le haut de la vue, la dernière une fois en bas du
        // document. Elle ne suit que le défilement : un saut garde la page demandée.
        if moved && !jumped {
            let reading = if offset.y >= bottom - 1.0 {
                layout.page_at(offset.y + view.y - 1.0)
            } else {
                layout.page_at(offset.y + view.y / 3.0)
            };
            if reading != self.current {
                self.current = reading;
                self.follow_thumb = true;
            }
        }
        self.track_selection(ctx, view_rect);
        self.selection_bar(ctx, view_rect);
        self.editor(ctx, view_rect);
        self.back_button(ctx, view_rect);
        self.paint_message(ui, view_rect);
    }

    fn paint_page(&mut self, ui: &mut egui::Ui, page: usize, rect: Rect) {
        let painter = ui.painter();
        let shadow = style::elevation(ferrite_ui::tokens::elevation::WINDOW_INACTIVE);
        painter.add(shadow.as_shape(rect, CornerRadius::same(2)));
        painter.rect_filled(rect, CornerRadius::same(2), Color32::WHITE);
        if let Some(rendered) = self.pages.get(&page) {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(rendered.texture.id(), rect, uv, Color32::WHITE);
        } else if self.failed.contains(&page) {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Cette page n’a pas pu être affichée.",
                egui::FontId::proportional(13.0),
                style::text_secondary(),
            );
        } else {
            ui.put(
                Rect::from_center_size(rect.center(), vec2(22.0, 22.0)),
                egui::Spinner::new(),
            );
        }
        self.paint_marks(ui, page, rect);
        // Résultats de recherche : surlignés, le résultat en cours plus marqué.
        if self.search.open {
            let painter = ui.painter();
            let scale = self.scale;
            for (index, found) in self
                .search
                .matches
                .iter()
                .enumerate()
                .filter(|(_, found)| found.page == page)
            {
                let fill = if index == self.search.current {
                    style::warning().gamma_multiply(0.55)
                } else {
                    style::accent().gamma_multiply(0.28)
                };
                for part in &found.rects {
                    let area = Rect::from_min_max(
                        rect.min + part.min.to_vec2() * scale,
                        rect.min + part.max.to_vec2() * scale,
                    )
                    .expand(1.0);
                    painter.rect_filled(area, CornerRadius::same(2), fill);
                }
            }
        }
    }

    fn thumbnails(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(sizes) = self.info.as_ref().map(|info| info.pages.clone()) else {
            return;
        };
        let mut tops = Vec::with_capacity(sizes.len());
        let mut heights = Vec::with_capacity(sizes.len());
        let mut y = THUMB_GAP;
        for size in &sizes {
            let height = (THUMB_WIDTH * size.y / size.x.max(1.0)).min(THUMB_WIDTH * 1.6);
            tops.push(y);
            heights.push(height);
            y += height + THUMB_LABEL + THUMB_GAP;
        }
        let total = y;
        let mut pick = None;
        let pixels = (THUMB_WIDTH * ctx.pixels_per_point()).ceil() as u32;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("miniatures")
            .show_viewport(ui, |ui, viewport| {
                ui.set_min_size(vec2(SIDEBAR_WIDTH, total));
                let origin = ui.min_rect().min;
                let first = tops.partition_point(|top| *top + THUMB_WIDTH * 1.6 < viewport.top());
                let last = tops.partition_point(|top| *top < viewport.bottom());
                for page in first..last.min(sizes.len()) {
                    let image = Rect::from_min_size(
                        origin + vec2((SIDEBAR_WIDTH - THUMB_WIDTH) / 2.0, tops[page]),
                        vec2(THUMB_WIDTH, heights[page]),
                    );
                    let item = Rect::from_min_max(
                        image.min - vec2(10.0, 6.0),
                        pos2(image.max.x + 10.0, image.max.y + THUMB_LABEL),
                    );
                    let response = ui
                        .interact(item, ui.scope_id().with(("miniature", page)), Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if response.clicked() {
                        pick = Some(page);
                    }
                    let selected = page == self.current;
                    let painter = ui.painter();
                    if response.hovered() && !selected {
                        painter.rect_filled(item, CornerRadius::same(8), style::tint(10));
                    }
                    painter.rect_filled(image, CornerRadius::same(2), Color32::WHITE);
                    if let Some(texture) = self.thumbs.get(&page) {
                        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                        painter.image(texture.id(), image, uv, Color32::WHITE);
                    } else if !self.pending_thumbs.contains(&page)
                        && self.pending_thumbs.len() < MAX_THUMB_JOBS
                    {
                        self.pending_thumbs.insert(page);
                        self.spawn(ctx, move |path| Loaded::Thumb {
                            page,
                            image: poppler::render(path, page + 1, pixels),
                        });
                    }
                    let ring = if selected {
                        Stroke::new(2.5, style::accent())
                    } else {
                        Stroke::new(1.0, style::tint(30))
                    };
                    painter.rect_stroke(image, CornerRadius::same(2), ring, egui::StrokeKind::Outside);
                    let label = pos2(image.center().x, image.bottom() + THUMB_LABEL / 2.0 + 1.0);
                    let number = (page + 1).to_string();
                    if selected {
                        let galley = painter.layout_no_wrap(
                            number.clone(),
                            egui::FontId::proportional(12.0),
                            style::accent_text(),
                        );
                        let pill = Rect::from_center_size(label, vec2(galley.size().x + 14.0, 18.0));
                        painter.rect_filled(pill, CornerRadius::same(9), style::accent());
                        painter.galley(label - galley.size() / 2.0, galley, style::accent_text());
                    } else {
                        painter.text(
                            label,
                            egui::Align2::CENTER_CENTER,
                            number,
                            egui::FontId::proportional(12.0),
                            style::text_secondary(),
                        );
                    }
                    if selected && self.follow_thumb {
                        ui.scroll_to_rect(item, None);
                    }
                }
                if self.follow_thumb && !(first..last).contains(&self.current) {
                    if let Some(top) = tops.get(self.current) {
                        let target = Rect::from_min_size(
                            origin + vec2(0.0, *top),
                            vec2(SIDEBAR_WIDTH, heights[self.current] + THUMB_LABEL),
                        );
                        ui.scroll_to_rect(target, Some(egui::Align::Center));
                    }
                } else {
                    self.follow_thumb = false;
                }
                let keep = first.saturating_sub(30)..last + 30;
                self.thumbs.retain(|page, _| keep.contains(page));
            });
        if let Some(page) = pick {
            self.go_to(page);
            self.follow_thumb = false;
        }
    }

    fn search_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Panel::top(ui.scope_id().with("recherche-pdf"))
            .resizable(false)
            .exact_size(46.0)
            .frame(
                egui::Frame::new()
                    .fill(style::background())
                    .inner_margin(egui::Margin::symmetric(12, 0)),
            )
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter().hline(
                    full.x_range().expand(12.0),
                    full.bottom() - 0.5,
                    Stroke::new(1.0, style::line()),
                );
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.search.query)
                            .hint_text("Rechercher dans le document")
                            .desired_width(260.0),
                    );
                    if std::mem::take(&mut self.search.focus) {
                        field.request_focus();
                    }
                    if field.lost_focus() && ctx.input(|input| input.key_pressed(Key::Enter)) {
                        let back = ctx.input(|input| input.modifiers.shift);
                        self.next_match(!back);
                        field.request_focus();
                    }
                    let has = !self.search.matches.is_empty();
                    if icon_button(ui, Glyph::Up, false, "Résultat précédent (Maj+Entrée)", has).clicked()
                    {
                        self.next_match(false);
                    }
                    if icon_button(ui, Glyph::Down, false, "Résultat suivant (Entrée)", has).clicked() {
                        self.next_match(true);
                    }
                    ui.add_space(6.0);
                    let status = if self.search.loading {
                        "Lecture du texte…".to_owned()
                    } else if let Some(error) = &self.search.error {
                        error.clone()
                    } else if self.search.query.trim().is_empty() {
                        String::new()
                    } else if self
                        .search
                        .words
                        .as_ref()
                        .is_some_and(|pages| pages.iter().all(Vec::is_empty))
                    {
                        "Ce document ne contient pas de texte (page scannée ?)".to_owned()
                    } else if !has {
                        "Aucun résultat".to_owned()
                    } else {
                        format!("{} sur {}", self.search.current + 1, self.search.matches.len())
                    };
                    if self.search.loading {
                        ui.add(egui::Spinner::new().size(14.0));
                    }
                    ui.label(egui::RichText::new(status).color(style::text_secondary()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Glyph::Close, false, "Fermer la recherche (Échap)", true).clicked()
                        {
                            self.search.open = false;
                        }
                    });
                });
            });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, recents: &[Recent]) {
        let full = ui.max_rect();
        let ready = self.info.is_some();
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(6.0);
        if icon_button(
            ui,
            Glyph::Sidebar,
            self.sidebar && ready,
            "Miniatures (F9)",
            ready,
        )
        .clicked()
        {
            self.sidebar = !self.sidebar;
            self.follow_thumb = true;
        }
        ui.add_space(4.0);
        let count = self.page_count();
        if icon_button(
            ui,
            Glyph::Previous,
            false,
            "Page précédente (←)",
            ready && self.current > 0,
        )
        .clicked()
        {
            self.go_to(self.current.saturating_sub(1));
        }
        if icon_button(
            ui,
            Glyph::Next,
            false,
            "Page suivante (→)",
            ready && self.current + 1 < count,
        )
        .clicked()
        {
            self.go_to(self.current + 1);
        }
        ui.add_space(4.0);
        let field_id = ui.scope_id().with("numero-page");
        let editing = ui.memory(|memory| memory.has_focus(field_id));
        if !editing {
            self.page_field = if ready {
                (self.current + 1).to_string()
            } else {
                String::new()
            };
        }
        let field = ui
            .add_enabled(
                ready,
                egui::TextEdit::singleline(&mut self.page_field)
                    .id(field_id)
                    .desired_width(46.0)
                    .align(egui::Align2::CENTER_CENTER),
            )
            .on_hover_text("Aller à la page");
        if field.lost_focus()
            && ctx.input(|input| input.key_pressed(Key::Enter))
            && let Ok(page) = self.page_field.trim().parse::<usize>()
        {
            self.go_to(page.saturating_sub(1));
        }
        ui.label(
            egui::RichText::new(if ready {
                format!("sur {count}")
            } else {
                "sur …".to_owned()
            })
            .color(style::text_secondary()),
        );
        let left_end = ui.cursor().left();

        let right = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if icon_button(ui, Glyph::Search, self.search.open, "Rechercher (Ctrl+F)", ready).clicked() {
                if self.search.open {
                    self.search.open = false;
                } else {
                    self.open_search(ctx);
                }
            }
            ui.add_space(6.0);
            let marked = ready && self.notes.has_bookmark(self.current);
            if icon_button(
                ui,
                Glyph::Bookmark,
                marked,
                "Signet sur cette page (Ctrl+D)",
                ready,
            )
            .clicked()
            {
                self.toggle_bookmark(ctx, self.current);
            }
            if icon_button(
                ui,
                Glyph::Note,
                self.tool == Tool::Note,
                "Note : cliquez ensuite sur la page (N)",
                ready,
            )
            .clicked()
            {
                self.tool = if self.tool == Tool::Note {
                    Tool::Read
                } else {
                    Tool::Note
                };
            }
            // Encre du surligneur : une pastille qui ouvre le choix de la couleur et du trait.
            let (dot, ink) = ui.allocate_exact_size(vec2(18.0, kit::TOOL), Sense::click());
            let ink = ink.on_hover_text("Couleur et trait du surligneur");
            if ink.hovered() {
                ui.painter()
                    .rect_filled(dot, CornerRadius::same(6), style::tint(kit::TOOL_HOVER));
            }
            ui.painter()
                .circle_filled(dot.center(), 5.5, style::marker(self.ink));
            ui.painter()
                .circle_stroke(dot.center(), 5.5, Stroke::new(1.0, style::tint(40)));
            egui::Popup::menu(&ink).show(|ui| self.ink_menu(ui));
            if icon_button(
                ui,
                Glyph::Marker,
                self.tool == Tool::Marker,
                "Surligneur : marque chaque sélection (H)",
                ready,
            )
            .clicked()
            {
                self.tool = if self.tool == Tool::Marker {
                    Tool::Read
                } else {
                    Tool::Marker
                };
            }
            ui.add_space(6.0);
            if icon_button(ui, Glyph::Plus, false, "Zoom avant (+)", ready).clicked() {
                self.step_zoom(true);
            }
            let percent = format!("{:.0} %", self.effective_zoom() * 100.0);
            let zoom = ui
                .add_enabled(
                    ready,
                    egui::Button::new(egui::RichText::new(percent).size(13.0))
                        .min_size(vec2(62.0, 30.0))
                        .corner_radius(8.0),
                )
                .on_hover_text("Zoom");
            egui::Popup::menu(&zoom).show(|ui| self.zoom_menu(ui));
            if icon_button(ui, Glyph::Minus, false, "Zoom arrière (−)", ready).clicked() {
                self.step_zoom(false);
            }
            ui.add_space(6.0);
            let history = icon_button(ui, Glyph::Clock, false, "Documents récents", !recents.is_empty());
            egui::Popup::menu(&history).show(|ui| {
                style::menu_style(ui);
                for recent in recents {
                    let name = shorten(&file_name(&recent.path), 30);
                    let current = recent.path == self.path;
                    let detail = if current {
                        "ouvert".to_owned()
                    } else {
                        format!("p. {}", recent.page)
                    };
                    let row = style::menu_row(ui, &name, &detail, false, !current)
                        .on_hover_text(recent.path.display().to_string());
                    if row.clicked() {
                        self.request_recent = Some(recent.path.clone());
                    }
                }
                style::menu_separator(ui);
                if style::menu_row(ui, "Effacer la liste", "", false, true).clicked() {
                    self.request_clear_recents = true;
                }
            });
            if icon_button(ui, Glyph::Folder, false, "Ouvrir un document (Ctrl+O)", true).clicked() {
                self.request_open = true;
            }
        });
        // Nom du document au centre, s'il reste la place entre les deux groupes.
        let right_start = right.response.rect.left();
        let room = (right_start - left_end - 24.0).max(0.0);
        if room > 60.0 {
            let font = egui::FontId::new(14.0, style::strong_family());
            let name = file_name(&self.path);
            let mut text = name.clone();
            let painter = ui.painter();
            let mut galley = painter.layout_no_wrap(text.clone(), font.clone(), style::primary_text());
            while galley.size().x > room && text.chars().count() > 4 {
                text = shorten(&name, text.chars().count().saturating_sub(4));
                galley = painter.layout_no_wrap(text.clone(), font.clone(), style::primary_text());
            }
            let center_x = full.center().x.clamp(
                left_end + 12.0 + galley.size().x / 2.0,
                (right_start - 12.0 - galley.size().x / 2.0).max(left_end + 12.0 + galley.size().x / 2.0),
            );
            let at = pos2(center_x, full.center().y) - galley.size() / 2.0;
            let rect = Rect::from_min_size(at, galley.size());
            painter.galley(at, galley, style::primary_text());
            let hint = match self.info.as_ref().and_then(|info| info.title.clone()) {
                Some(title) => format!("{title}\n{}", self.path.display()),
                None => self.path.display().to_string(),
            };
            ui.interact(rect, ui.scope_id().with("nom-document"), Sense::hover())
                .on_hover_text(hint);
        }
    }

    fn zoom_menu(&mut self, ui: &mut egui::Ui) {
        style::menu_style(ui);
        let modes = [
            ("Ajustement automatique", Zoom::Auto),
            ("Largeur de la fenêtre (W)", Zoom::Width),
            ("Page entière (0)", Zoom::Page),
        ];
        for (label, mode) in modes {
            if style::radio_row(ui, label, self.zoom == mode).clicked() {
                self.set_zoom(mode);
            }
        }
        style::menu_separator(ui);
        for factor in [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0] {
            let selected = matches!(self.zoom, Zoom::Factor(now) if (now - factor).abs() < 0.001);
            let label = if factor == 1.0 {
                "100 % (taille réelle)".to_owned()
            } else {
                format!("{:.0} %", factor * 100.0)
            };
            if style::radio_row(ui, &label, selected).clicked() {
                self.set_zoom(Zoom::Factor(factor));
            }
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Lecteur PDF".to_owned())
}

/// Coupe un nom trop long au milieu, en gardant l'extension visible.
fn shorten(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max || max < 5 {
        return text.to_owned();
    }
    let tail = (max - 1) / 3;
    let head = max - 1 - tail;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!("{start}…{end}")
}

#[derive(Debug, Clone, Copy)]
enum Glyph {
    Sidebar,
    Previous,
    Next,
    Up,
    Down,
    Search,
    Plus,
    Minus,
    Clock,
    Folder,
    Close,
    Marker,
    Note,
    Bookmark,
    Underline,
    Strike,
    Comment,
    Copy,
    Trash,
    /// Petite croix (retirer une ligne d'une liste).
    Small,
}

/// Symbole au trait centré sur `c`, dans le style de ceux de la Visionneuse.
fn paint_glyph(painter: &egui::Painter, c: Pos2, glyph: Glyph, color: Color32) {
    let stroke = Stroke::new(1.7, color);
    let p = |x: f32, y: f32| pos2(c.x + x, c.y + y);
    match glyph {
        Glyph::Sidebar => {
            let frame = Rect::from_center_size(c, vec2(16.0, 13.0));
            painter.rect_stroke(
                frame,
                CornerRadius::same(3),
                Stroke::new(1.5, color),
                egui::StrokeKind::Middle,
            );
            painter.line_segment([p(-2.5, -6.5), p(-2.5, 6.5)], Stroke::new(1.5, color));
            for y in [-3.0, 0.0, 3.0] {
                painter.line_segment([p(-6.0, y), p(-4.5, y)], Stroke::new(1.3, color));
            }
        }
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
        Glyph::Up => {
            painter.line(
                vec![p(-5.5, 2.5), p(0.0, -3.0), p(5.5, 2.5)],
                Stroke::new(2.0, color),
            );
        }
        Glyph::Down => {
            painter.line(
                vec![p(-5.5, -2.5), p(0.0, 3.0), p(5.5, -2.5)],
                Stroke::new(2.0, color),
            );
        }
        Glyph::Search => {
            painter.circle_stroke(p(-1.5, -1.5), 5.0, stroke);
            painter.line_segment([p(2.2, 2.2), p(6.0, 6.0)], Stroke::new(2.0, color));
        }
        Glyph::Plus => {
            painter.line_segment([p(-5.5, 0.0), p(5.5, 0.0)], stroke);
            painter.line_segment([p(0.0, -5.5), p(0.0, 5.5)], stroke);
        }
        Glyph::Minus => {
            painter.line_segment([p(-5.5, 0.0), p(5.5, 0.0)], stroke);
        }
        Glyph::Clock => {
            painter.circle_stroke(c, 7.0, stroke);
            painter.line(vec![p(0.0, -4.0), p(0.0, 0.0), p(3.0, 2.0)], stroke);
        }
        Glyph::Folder => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(-7.5, -5.0),
                    p(-2.5, -5.0),
                    p(-1.0, -3.0),
                    p(7.5, -3.0),
                    p(7.5, 5.5),
                    p(-7.5, 5.5),
                ],
                Stroke::new(1.5, color),
            ));
        }
        Glyph::Close => {
            painter.line_segment([p(-4.5, -4.5), p(4.5, 4.5)], stroke);
            painter.line_segment([p(4.5, -4.5), p(-4.5, 4.5)], stroke);
        }
        Glyph::Small => {
            let thin = Stroke::new(1.5, color);
            painter.line_segment([p(-3.5, -3.5), p(3.5, 3.5)], thin);
            painter.line_segment([p(3.5, -3.5), p(-3.5, 3.5)], thin);
        }
        Glyph::Marker => {
            // Feutre en biais, et le trait qu'il laisse.
            painter.add(egui::Shape::closed_line(
                vec![p(2.5, -7.0), p(6.5, -3.0), p(-0.5, 4.0), p(-4.5, 0.0)],
                Stroke::new(1.5, color),
            ));
            painter.add(egui::Shape::closed_line(
                vec![p(-4.5, 0.0), p(-0.5, 4.0), p(-5.5, 5.0)],
                Stroke::new(1.5, color),
            ));
            painter.line_segment([p(-7.0, 7.5), p(7.0, 7.5)], Stroke::new(2.0, color));
        }
        Glyph::Note => {
            // Feuillet au coin replié.
            painter.add(egui::Shape::closed_line(
                vec![
                    p(-6.5, -6.5),
                    p(6.5, -6.5),
                    p(6.5, 2.0),
                    p(2.0, 6.5),
                    p(-6.5, 6.5),
                ],
                Stroke::new(1.5, color),
            ));
            painter.line(
                vec![p(6.5, 2.0), p(2.0, 2.0), p(2.0, 6.5)],
                Stroke::new(1.3, color),
            );
            painter.line_segment([p(-3.5, -3.0), p(3.5, -3.0)], Stroke::new(1.3, color));
            painter.line_segment([p(-3.5, 0.0), p(0.0, 0.0)], Stroke::new(1.3, color));
        }
        Glyph::Bookmark => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(-4.5, -7.0),
                    p(4.5, -7.0),
                    p(4.5, 7.0),
                    p(0.0, 3.0),
                    p(-4.5, 7.0),
                ],
                Stroke::new(1.6, color),
            ));
        }
        Glyph::Underline => {
            painter.line(
                vec![
                    p(-4.0, -6.5),
                    p(-4.0, -1.0),
                    p(-2.0, 2.0),
                    p(2.0, 2.0),
                    p(4.0, -1.0),
                    p(4.0, -6.5),
                ],
                stroke,
            );
            painter.line_segment([p(-5.5, 6.0), p(5.5, 6.0)], stroke);
        }
        Glyph::Strike => {
            painter.line(
                vec![
                    p(4.0, -4.5),
                    p(1.5, -6.5),
                    p(-2.0, -6.5),
                    p(-4.0, -4.0),
                    p(-2.0, -1.0),
                ],
                stroke,
            );
            painter.line(
                vec![p(2.0, 1.0), p(4.0, 4.0), p(2.0, 6.5), p(-1.5, 6.5), p(-4.0, 4.5)],
                stroke,
            );
            painter.line_segment([p(-7.0, 0.0), p(7.0, 0.0)], stroke);
        }
        Glyph::Comment => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(-7.0, -6.0),
                    p(7.0, -6.0),
                    p(7.0, 3.0),
                    p(0.0, 3.0),
                    p(-3.5, 6.5),
                    p(-3.5, 3.0),
                    p(-7.0, 3.0),
                ],
                Stroke::new(1.5, color),
            ));
        }
        Glyph::Copy => {
            let back = Rect::from_min_max(p(-6.5, -6.5), p(2.5, 2.5));
            let front = Rect::from_min_max(p(-2.5, -2.5), p(6.5, 6.5));
            painter.line(
                vec![back.left_bottom(), back.left_top(), back.right_top()],
                Stroke::new(1.5, color),
            );
            painter.rect_stroke(
                front,
                CornerRadius::same(2),
                Stroke::new(1.5, color),
                egui::StrokeKind::Middle,
            );
        }
        Glyph::Trash => {
            painter.line_segment([p(-6.5, -4.5), p(6.5, -4.5)], stroke);
            painter.line(
                vec![p(-2.0, -4.5), p(-2.0, -7.0), p(2.0, -7.0), p(2.0, -4.5)],
                Stroke::new(1.5, color),
            );
            painter.line(
                vec![p(-5.0, -4.5), p(-4.0, 7.0), p(4.0, 7.0), p(5.0, -4.5)],
                Stroke::new(1.5, color),
            );
        }
    }
}

/// Bouton d'outil ; grisé et sans effet quand `enabled` est faux.
fn icon_button(ui: &mut egui::Ui, glyph: Glyph, active: bool, hint: &str, enabled: bool) -> egui::Response {
    if !enabled {
        let (rect, response) = ui.allocate_exact_size(vec2(kit::TOOL, kit::TOOL), Sense::hover());
        paint_glyph(
            ui.painter(),
            rect.center(),
            glyph,
            style::text_secondary().gamma_multiply(0.4),
        );
        return response;
    }
    kit::tool_button(ui, kit::TOOL, active, hint, |painter, rect, ink| {
        paint_glyph(painter, rect.center(), glyph, ink);
    })
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
    recents: Vec<Recent>,
    /// Page déjà retenue pour le document ouvert (évite d'écrire à chaque image).
    saved_page: usize,
    /// Couvertures (première page) des documents récents.
    covers: HashMap<PathBuf, Option<TextureHandle>>,
    cover_sender: Sender<(PathBuf, Option<egui::ColorImage>)>,
    cover_receiver: Receiver<(PathBuf, Option<egui::ColorImage>)>,
}

enum Kind {
    Images(Box<crate::viewer::ViewerState>),
    /// Documents ouverts (un par onglet ; aucun : l'écran d'accueil) et celui affiché.
    Pdf(Vec<PdfViewerState>, usize),
}

impl ViewerApp {
    fn with_kind(kind: Kind) -> Self {
        let (cover_sender, cover_receiver) = mpsc::channel();
        Self {
            kind,
            picker: None,
            icons: Default::default(),
            recents: recents::load(),
            saved_page: 0,
            covers: HashMap::new(),
            cover_sender,
            cover_receiver,
        }
    }

    pub fn new(path: Option<PathBuf>) -> Self {
        match path {
            Some(path) if supported(&path) => Self::new_pdf(Some(path)),
            path => Self::with_kind(Kind::Images(Box::new(crate::viewer::ViewerState::new(path)))),
        }
    }

    pub fn new_pdf(path: Option<PathBuf>) -> Self {
        let mut app = Self::with_kind(Kind::Pdf(Vec::new(), 0));
        if let Some(path) = path {
            app.open_pdf(path);
        }
        app
    }

    pub fn open(&mut self, path: PathBuf) {
        if supported(&path) {
            self.open_pdf(path);
        } else {
            match &mut self.kind {
                Kind::Images(viewer) => viewer.open(path),
                Kind::Pdf(..) => *self = Self::new(Some(path)),
            }
        }
    }

    /// Ouvre un document à la page où on l'avait laissé, dans un nouvel onglet (ou revient
    /// à son onglet s'il est déjà ouvert), et le met en tête des récents.
    pub fn open_pdf(&mut self, path: PathBuf) {
        let path = recents::absolute(&path);
        let start = recents::load()
            .iter()
            .find(|recent| recent.path == path)
            .map_or(1, |recent| recent.page);
        self.open_pdf_at(path, start, false);
    }

    /// `forced` : aller à `start` même si le document est déjà ouvert (lien vers une page).
    fn open_pdf_at(&mut self, path: PathBuf, start: usize, forced: bool) {
        if !matches!(self.kind, Kind::Pdf(..)) {
            self.kind = Kind::Pdf(Vec::new(), 0);
        }
        let Kind::Pdf(documents, active) = &mut self.kind else {
            return;
        };
        let page = match documents.iter().position(|viewer| viewer.path == path) {
            // Même document ouvert à nouveau : la page et le zoom sont conservés.
            Some(index) => {
                *active = index;
                let viewer = &mut documents[index];
                viewer.shown_again();
                if forced {
                    viewer.go_to(start.saturating_sub(1));
                    start
                } else {
                    viewer.page().max(1)
                }
            }
            None => {
                let viewer = match documents.get(*active) {
                    Some(other) => PdfViewerState::new_like(path.clone(), start, other),
                    None => PdfViewerState::new(path.clone(), start),
                };
                documents.push(viewer);
                *active = documents.len() - 1;
                start
            }
        };
        self.saved_page = page;
        self.recents = recents::update(|list| recents::touch(list, &path, page));
    }

    fn active_pdf(&self) -> Option<&PdfViewerState> {
        match &self.kind {
            Kind::Pdf(documents, active) => documents.get(*active),
            Kind::Images(_) => None,
        }
    }

    /// Onglets : choisir, fermer, passer au suivant.
    fn tab_action(&mut self, action: tabs::TabAction) {
        let Kind::Pdf(documents, active) = &mut self.kind else {
            return;
        };
        match action {
            tabs::TabAction::Select(index) if index < documents.len() => *active = index,
            tabs::TabAction::Close(index) if index < documents.len() => {
                documents[index].close_editor();
                documents.remove(index);
                if *active > index || *active >= documents.len() {
                    *active = active.saturating_sub(1);
                }
            }
            tabs::TabAction::Open => {
                self.open_picker();
                return;
            }
            _ => return,
        }
        if let Some(viewer) = documents.get_mut(*active) {
            viewer.shown_again();
            self.saved_page = viewer.page();
        } else {
            self.saved_page = 0;
        }
    }

    /// Ctrl+W ferme l'onglet ; Ctrl+Tab et Ctrl+Pg. suiv. passent au suivant (avec Maj, ou
    /// Pg. préc., au précédent).
    fn tab_keys(&mut self, ctx: &egui::Context) {
        let Kind::Pdf(documents, active) = &self.kind else {
            return;
        };
        let (count, active) = (documents.len(), *active);
        if count == 0 || ctx.egui_wants_keyboard_input() {
            return;
        }
        let (command, shift) = ctx.input(|input| (input.modifiers.command, input.modifiers.shift));
        if !command {
            return;
        }
        let pressed = |key: Key| ctx.input_mut(|input| input.consume_key(input.modifiers, key));
        if pressed(Key::W) {
            self.tab_action(tabs::TabAction::Close(active));
        } else if count > 1 {
            let next = pressed(Key::PageDown) || (!shift && pressed(Key::Tab));
            let previous = pressed(Key::PageUp) || (shift && pressed(Key::Tab));
            if next {
                self.tab_action(tabs::TabAction::Select((active + 1) % count));
            } else if previous {
                self.tab_action(tabs::TabAction::Select((active + count - 1) % count));
            }
        }
    }

    fn open_picker(&mut self) {
        // Le sélecteur s'ouvre sur le dossier du document en cours, s'il y en a un.
        let start = match self.active_pdf() {
            Some(viewer) => viewer
                .path
                .parent()
                .filter(|path| path.is_dir())
                .map(Path::to_path_buf),
            None => self
                .recents
                .first()
                .and_then(|recent| recent.path.parent())
                .filter(|path| path.is_dir())
                .map(Path::to_path_buf),
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
        if self.picker.is_none() {
            self.tab_keys(&ctx);
        }
        let mut wants_open = false;
        let mut wanted_recent = None;
        let mut wanted_file = None;
        let mut tab_action = None;
        let mut clear = false;
        match &mut self.kind {
            Kind::Images(viewer) => viewer.update(ui),
            Kind::Pdf(documents, active) if *active < documents.len() => {
                let strip = (documents.len() > 1).then(|| tabs::Strip {
                    tabs: documents
                        .iter()
                        .map(|viewer| tabs::Tab {
                            name: file_name(&viewer.path),
                            path: viewer.path.display().to_string(),
                        })
                        .collect(),
                    active: *active,
                });
                let viewer = &mut documents[*active];
                viewer.update(ui, &self.recents, strip.as_ref());
                wants_open = std::mem::take(&mut viewer.request_open);
                wanted_recent = viewer.request_recent.take();
                wanted_file = viewer.request_file.take();
                tab_action = viewer.request_tab.take();
                clear = std::mem::take(&mut viewer.request_clear_recents);
                // La page lue est retenue dès qu'elle change.
                let page = viewer.page();
                if page > 0 && page != self.saved_page {
                    self.saved_page = page;
                    let path = viewer.path.clone();
                    self.recents = recents::update(|list| {
                        if !recents::set_page(list, &path, page)
                            && !list.iter().any(|recent| recent.path == path)
                        {
                            recents::touch(list, &path, page);
                        }
                    });
                }
            }
            Kind::Pdf(..) => match self.empty_state(ui) {
                Some(EmptyAction::Open) => wants_open = true,
                Some(EmptyAction::Recent(path)) => wanted_recent = Some(path),
                Some(EmptyAction::Forget(path)) => {
                    self.recents = recents::update(|list| list.retain(|recent| recent.path != path));
                }
                Some(EmptyAction::Clear) => clear = true,
                None => {}
            },
        }
        if clear {
            // Les documents ouverts restent dans la liste.
            let keep: Vec<PathBuf> = match &self.kind {
                Kind::Pdf(documents, _) => documents.iter().map(|viewer| viewer.path.clone()).collect(),
                Kind::Images(_) => Vec::new(),
            };
            self.recents = recents::update(|list| list.retain(|recent| keep.contains(&recent.path)));
        }
        if let Some(path) = wanted_recent {
            self.open_pdf(path);
        }
        if let Some((path, page)) = wanted_file {
            let path = recents::absolute(&path);
            self.open_pdf_at(path, page, true);
        }
        if let Some(action) = tab_action {
            self.tab_action(action);
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
        let size = vec2(
            (screen.width() - 48.0).clamp(420.0, 900.0),
            (screen.height() - 64.0).clamp(300.0, 560.0),
        );
        egui::Modal::new(egui::Id::unique("lecteur-pdf-selecteur"))
            .frame(
                egui::Frame::new()
                    .fill(style::background())
                    .corner_radius(12.0)
                    .shadow(style::elevation(ferrite_ui::tokens::elevation::MENU)),
            )
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
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

    fn pump_covers(&mut self, ctx: &egui::Context) {
        while let Ok((path, image)) = self.cover_receiver.try_recv() {
            let texture = image.map(|image| {
                ctx.load_texture(
                    format!("pdf-couverture:{}", path.display()),
                    image,
                    TextureOptions::LINEAR,
                )
            });
            self.covers.insert(path, texture);
        }
    }

    fn cover(&mut self, ctx: &egui::Context, path: &Path, width: u32) -> Option<TextureHandle> {
        if let Some(cover) = self.covers.get(path) {
            return cover.clone();
        }
        self.covers.insert(path.to_path_buf(), None);
        let (path, sender, ctx) = (path.to_path_buf(), self.cover_sender.clone(), ctx.clone());
        thread::spawn(move || {
            let image = poppler::render(&path, 1, width).ok();
            let _ = sender.send((path, image));
            ctx.request_repaint();
        });
        None
    }

    /// Fenêtre sans document : les documents récents, ou une invitation à en choisir un.
    fn empty_state(&mut self, ui: &mut egui::Ui) -> Option<EmptyAction> {
        let ctx = ui.ctx().clone();
        self.pump_covers(&ctx);
        let mut action = None;
        style::toolbar_with(ui, true, style::header_color(), Color32::TRANSPARENT, |ui| {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("Lecteur PDF")
                    .strong()
                    .color(style::primary_text()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Glyph::Folder, false, "Ouvrir un document (Ctrl+O)", true).clicked() {
                    action = Some(EmptyAction::Open);
                }
            });
        });
        let recents = self.recents.clone();
        // Thème « Fusion » : barre sur le chrome, contenu dans la carte encastrée.
        style::inset_panel(ui, true, false, |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(style::stage()))
                .show(ui, |ui| {
                    if recents.is_empty() {
                        if welcome(ui, &mut self.icons) {
                            action = Some(EmptyAction::Open);
                        }
                        return;
                    }
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let full = ui.max_rect();
                            let (card_w, cover_h, gap) = (150.0, 196.0, 22.0);
                            let columns =
                                (((full.width() - 64.0 + gap) / (card_w + gap)).floor() as usize).clamp(1, 6);
                            let grid_w = columns as f32 * (card_w + gap) - gap;
                            let left = full.left() + ((full.width() - grid_w) / 2.0).max(24.0);
                            ui.add_space(26.0);
                            ui.horizontal(|ui| {
                                ui.add_space(left - full.left());
                                ui.label(
                                    egui::RichText::new("Documents récents")
                                        .size(20.0)
                                        .strong()
                                        .color(style::heading_text()),
                                );
                            });
                            ui.add_space(16.0);
                            let top = ui.cursor().top();
                            let pixels = (card_w * ctx.pixels_per_point()).ceil() as u32;
                            let rows = recents.len().div_ceil(columns);
                            let row_h = cover_h + 58.0;
                            let (area, _) = ui.allocate_exact_size(
                                vec2(full.width(), rows as f32 * (row_h + gap)),
                                Sense::hover(),
                            );
                            let _ = area;
                            for (index, recent) in recents.iter().enumerate() {
                                let (row, column) = (index / columns, index % columns);
                                let card = Rect::from_min_size(
                                    pos2(
                                        left + column as f32 * (card_w + gap),
                                        top + row as f32 * (row_h + gap),
                                    ),
                                    vec2(card_w, row_h),
                                );
                                let response = ui
                                    .interact(card, ui.scope_id().with(("recent", index)), Sense::click())
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(recent.path.display().to_string());
                                if response.clicked() {
                                    action = Some(EmptyAction::Recent(recent.path.clone()));
                                }
                                response.context_menu(|ui| {
                                    style::menu_style(ui);
                                    if style::menu_row(ui, "Ouvrir", "", false, true).clicked() {
                                        action = Some(EmptyAction::Recent(recent.path.clone()));
                                    }
                                    if style::menu_row(ui, "Retirer de la liste", "", false, true).clicked() {
                                        action = Some(EmptyAction::Forget(recent.path.clone()));
                                    }
                                    style::menu_separator(ui);
                                    if style::menu_row(ui, "Effacer la liste", "", true, true).clicked() {
                                        action = Some(EmptyAction::Clear);
                                    }
                                });
                                let cover_area = Rect::from_min_size(card.min, vec2(card_w, cover_h));
                                let painter = ui.painter();
                                if response.hovered() {
                                    painter.rect_filled(
                                        card.expand(8.0),
                                        CornerRadius::same(12),
                                        style::tint(10),
                                    );
                                }
                                let cover = self.cover(&ctx, &recent.path, pixels);
                                let paper = match &cover {
                                    Some(texture) => {
                                        let size = texture.size_vec2();
                                        let fit = (card_w / size.x).min(cover_h / size.y);
                                        Rect::from_center_size(cover_area.center(), size * fit)
                                    }
                                    None => Rect::from_center_size(
                                        cover_area.center(),
                                        vec2(card_w * 0.76, cover_h),
                                    ),
                                };
                                let shadow = style::elevation(ferrite_ui::tokens::elevation::WINDOW_INACTIVE);
                                painter.add(shadow.as_shape(paper, CornerRadius::same(3)));
                                painter.rect_filled(paper, CornerRadius::same(3), Color32::WHITE);
                                if let Some(texture) = cover {
                                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                                    painter.image(texture.id(), paper, uv, Color32::WHITE);
                                }
                                painter.rect_stroke(
                                    paper,
                                    CornerRadius::same(3),
                                    Stroke::new(1.0, style::tint(24)),
                                    egui::StrokeKind::Outside,
                                );
                                let name = file_name(&recent.path);
                                let title = painter.layout(
                                    name,
                                    egui::FontId::new(13.0, style::strong_family()),
                                    style::primary_text(),
                                    card_w,
                                );
                                let title_rows = title.rows.len();
                                let title = if title_rows > 2 {
                                    painter.layout_no_wrap(
                                        shorten(&file_name(&recent.path), 20),
                                        egui::FontId::new(13.0, style::strong_family()),
                                        style::primary_text(),
                                    )
                                } else {
                                    title
                                };
                                let title_pos =
                                    pos2(card.center().x - title.size().x / 2.0, cover_area.bottom() + 10.0);
                                let title_h = title.size().y;
                                painter.galley(title_pos, title, style::primary_text());
                                painter.text(
                                    pos2(card.center().x, cover_area.bottom() + 12.0 + title_h),
                                    egui::Align2::CENTER_TOP,
                                    format!("Page {}", recent.page),
                                    egui::FontId::proportional(12.0),
                                    style::text_secondary(),
                                );
                            }
                            ui.add_space(8.0);
                            ui.vertical_centered(|ui| {
                                if kit::button(ui, "Ouvrir un autre document…", false).clicked() {
                                    action = Some(EmptyAction::Open);
                                }
                            });
                            ui.add_space(24.0);
                        });
                });
        });
        action
    }
}

enum EmptyAction {
    Open,
    Recent(PathBuf),
    Forget(PathBuf),
    Clear,
}

/// Aucun document récent : un bouton pour en choisir un (rend vrai si on l'a demandé).
fn welcome(ui: &mut egui::Ui, icons: &mut style::Icons) -> bool {
    let mut open = false;
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() / 2.0 - 130.0).max(0.0));
            let (rect, _) = ui.allocate_exact_size(vec2(84.0, 84.0), Sense::hover());
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
                egui::RichText::new("Les documents que vous lirez apparaîtront ici.")
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
    fn mise_en_page_continue() {
        let pages = [vec2(100.0, 200.0), vec2(200.0, 100.0), vec2(100.0, 200.0)];
        let layout = Layout::new(&pages, 2.0);
        assert_eq!(
            layout.tops,
            vec![MARGIN, MARGIN + 400.0 + GAP, MARGIN + 600.0 + 2.0 * GAP]
        );
        assert_eq!(layout.width, 400.0 + 2.0 * MARGIN);
        assert_eq!(layout.height, MARGIN * 2.0 + 1000.0 + 2.0 * GAP);
        assert_eq!(layout.page_at(0.0), 0);
        assert_eq!(layout.page_at(MARGIN + 401.0), 0);
        assert_eq!(layout.page_at(MARGIN + 400.0 + GAP + 1.0), 1);
        assert_eq!(layout.page_at(1.0e6), 2);
        assert_eq!(layout.pages_between(0.0, 50.0), 0..1);
        assert_eq!(layout.pages_between(MARGIN + 390.0, MARGIN + 450.0), 0..2);
    }

    #[test]
    fn rendu_par_paliers_et_borne() {
        let a4 = vec2(595.0, 842.0);
        assert_eq!(render_width(700.0, a4), 768);
        assert_eq!(render_width(768.0, a4), 768);
        assert_eq!(render_width(1.0, a4), WIDTH_STEP);
        let huge = render_width(20_000.0, a4);
        assert!(huge <= MAX_RENDER_WIDTH);
        assert!((huge as f32 * huge as f32 * 842.0 / 595.0) as usize <= poppler::MAX_PIXELS);
    }

    #[test]
    fn le_zoom_suit_les_paliers_et_reste_borne() {
        let mut etat = PdfViewerState::new(PathBuf::from("rapport.pdf"), 1);
        etat.scale = POINT;
        etat.step_zoom(true);
        assert_eq!(etat.zoom, Zoom::Factor(1.1));
        etat.scale = ZOOM_MAX * POINT;
        etat.step_zoom(true);
        assert_eq!(etat.zoom, Zoom::Factor(ZOOM_MAX));
        etat.scale = ZOOM_MIN * POINT;
        etat.step_zoom(false);
        assert_eq!(etat.zoom, Zoom::Factor(ZOOM_MIN));
        etat.set_zoom(Zoom::Factor(40.0));
        assert_eq!(etat.zoom, Zoom::Factor(ZOOM_MAX));
    }

    #[test]
    fn echelle_selon_le_mode() {
        let mut etat = PdfViewerState::new(PathBuf::from("rapport.pdf"), 1);
        etat.info = Some(DocInfo {
            pages: vec![vec2(600.0, 800.0)],
            title: None,
        });
        let view = vec2(600.0 + 2.0 * MARGIN + SCROLLBAR_ROOM, 400.0 + 2.0 * MARGIN);
        etat.zoom = Zoom::Width;
        assert!((etat.scale_for(view) - 1.0).abs() < 0.001);
        etat.zoom = Zoom::Page;
        assert!((etat.scale_for(view) - 0.5).abs() < 0.001);
        etat.zoom = Zoom::Factor(1.0);
        assert!((etat.scale_for(view) - POINT).abs() < 0.001);
        // Automatique : la largeur, sans dépasser 125 %.
        etat.zoom = Zoom::Auto;
        assert!((etat.scale_for(view * 4.0) - AUTO_MAX * POINT).abs() < 0.001);
    }

    #[test]
    fn coupe_les_noms_trop_longs() {
        assert_eq!(shorten("court.pdf", 20), "court.pdf");
        let short = shorten("un nom de document vraiment très long.pdf", 20);
        assert_eq!(short.chars().count(), 20);
        assert!(short.ends_with("g.pdf"));
    }

    #[test]
    fn le_lancement_pdf_reste_distinct_sans_document() {
        assert!(matches!(ViewerApp::new_pdf(None).kind, Kind::Pdf(ref documents, 0) if documents.is_empty()));
        assert!(matches!(
            ViewerApp::new(Some(PathBuf::from("Images/photo.png"))).kind,
            Kind::Images(_)
        ));
    }
}
