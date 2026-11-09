//! Interface de Fichiers, d'après la planche « Fichiers » : barre latérale (Favoris,
//! Appareils, Étiquettes), barre d'outils (chemin, Grille / Liste, recherche, thème,
//! aperçu), pastilles de filtre et tri, fichiers récents, grille ou liste, bande
//! d'informations et panneau d'aperçu ; glisser-déposer vers les
//! dossiers, la barre latérale, le chemin et la Corbeille, puis, hors de la fenêtre, vers
//! le Dock et les autres applications (voir `ferrite_system::drag`).

use std::{
    io::Read,
    os::unix::{fs::MetadataExt, net::UnixListener},
};

use eframe::egui::{FontFamily, FontId};
use ferrite_system::drag::{self, Deposit};

use crate::details;

use super::look::{Look, look};
use super::*;

/// Barre latérale, panneau d'aperçu et cases de la grille, en points.
const SIDEBAR_WIDTH: f32 = 250.0;
const PREVIEW_WIDTH: f32 = 300.0;
const CELL_MIN: f32 = 124.0;
const CELL_HEIGHT: f32 = 152.0;
const CELL_GAP: f32 = 10.0;
const LIST_ROW: f32 = 40.0;
/// Bande d'informations du bas.
const STATUS_HEIGHT: f32 = 52.0;
/// Rangée des filtres, bande des récents et largeur minimale d'une carte récente.
const FILTER_BAR: f32 = 42.0;
const RECENT_PANEL: f32 = 104.0;
const RECENT_CARD_MIN: f32 = 190.0;
/// Clés du menu « Trier ».
const SORT_KEYS: [(&str, SortKey); 4] = [
    ("Nom", SortKey::Name),
    ("Date de modification", SortKey::Modified),
    ("Taille", SortKey::Size),
    ("Type", SortKey::Kind),
];
/// Colonnes Taille, Modifié et Type de la liste.
const TAB_BAR_HEIGHT: f32 = 40.0;
const LIST_COLUMNS: [f32; 3] = [120.0, 190.0, 150.0];

/// Où l'on peut lâcher des fichiers glissés.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    Folder(PathBuf),
    Trash,
}

/// Ce que l'utilisateur a demandé dans le dialogue des propriétés.
#[derive(Default)]
struct PropertiesOutcome {
    close: bool,
    rename: bool,
    open: bool,
    change_opener: bool,
    reveal: bool,
    /// Nouveaux droits choisis.
    mode: Option<u32>,
}

/// Sélection au rectangle : point de départ, dans le contenu défilé, et sélection gardée.
pub(super) struct Lasso {
    origin: egui::Pos2,
    base: BTreeSet<PathBuf>,
}

/// Largeur de la barre de défilement (où un clic ne tire pas de rectangle) et bande des
/// bords où le rectangle fait défiler.
const LASSO_SCROLLBAR: f32 = 14.0;
const LASSO_EDGE: f32 = 28.0;

/// Fichiers en train d'être glissés dans la fenêtre.
#[derive(Debug, Clone)]
pub(super) struct Dragging {
    pub paths: Vec<PathBuf>,
    pub icon: &'static str,
}

/// Dépôts confiés par ferrite-shell, reçus sur la prise propre à ce processus (chaque
/// fenêtre de Fichiers en est un) ; la prise disparaît avec la fenêtre.
pub(super) struct Deposits {
    receiver: Receiver<Deposit>,
    path: PathBuf,
}

impl Deposits {
    fn listen(ctx: &egui::Context) -> Self {
        let path = drag::deposit_socket_path(std::process::id());
        let (sender, receiver) = mpsc::channel();
        let _ = fs::remove_file(&path);
        match UnixListener::bind(&path) {
            Ok(listener) => {
                let ctx = ctx.clone();
                thread::spawn(move || {
                    for stream in listener.incoming().filter_map(Result::ok) {
                        let mut text = String::new();
                        if stream.take(1 << 20).read_to_string(&mut text).is_ok()
                            && let Some(deposit) = Deposit::decode(&text)
                        {
                            if sender.send(deposit).is_err() {
                                return;
                            }
                            ctx.request_repaint();
                        }
                    }
                });
            }
            Err(error) => eprintln!("fichiers : dépôts depuis les autres fenêtres indisponibles ({error})"),
        }
        Self { receiver, path }
    }
}

impl Drop for Deposits {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Cibles de dépôt relevées pendant le dessin de l'image.
type Targets = Vec<(egui::Rect, Target)>;

fn font(size: f32, strong: bool) -> FontId {
    if strong {
        FontId::new(size, style::strong_family())
    } else {
        FontId::new(size, FontFamily::Proportional)
    }
}

/// Texte d'une ligne coupé avec « … » au-delà de `max_width`, posé à `anchor`.
fn text(
    ui: &egui::Ui,
    value: &str,
    anchor: egui::Pos2,
    align: egui::Align2,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> egui::Rect {
    let painter = ui.painter();
    let mut shown = value.to_owned();
    let mut galley = painter.layout_no_wrap(shown.clone(), font.clone(), color);
    while galley.size().x > max_width && shown.chars().count() > 1 {
        shown.pop();
        galley = painter.layout_no_wrap(format!("{}…", shown.trim_end()), font.clone(), color);
    }
    let rect = align.anchor_size(anchor, galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

/// Bouton carré sans texte : fond `fill` (ou effet de survol), dessin par `paint`.
fn icon_button(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    radius: u8,
    fill: Option<Color32>,
    hint: &str,
    paint: impl FnOnce(&egui::Painter, egui::Rect),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_text(hint);
    match fill {
        Some(fill) => {
            ui.painter().rect_filled(rect, radius, fill);
        }
        None if response.hovered() => {
            ui.painter()
                .rect_filled(rect, radius, style::tint(crate::kit::TOOL_HOVER));
        }
        None => {}
    }
    paint(ui.painter(), rect);
    response
}

/// Pastille de filtre ou d'étiquette : fond accentué quand elle est choisie, point de
/// couleur éventuel devant le texte.
fn chip(ui: &mut egui::Ui, look: &Look, label: &str, dot: Option<Color32>, on: bool) -> egui::Response {
    let ink = if on { look.sel_fg } else { look.fg2 };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font(12.0, on), ink);
    let lead = if dot.is_some() { 16.0 } else { 0.0 };
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(galley.size().x + 24.0 + lead, 26.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if on {
        look.sel_bg
    } else if response.hovered() {
        look.stage
    } else {
        look.chip
    };
    ui.painter().rect_filled(rect, 13.0, fill);
    if let Some(dot) = dot {
        ui.painter()
            .circle_filled(egui::pos2(rect.left() + 15.0, rect.center().y), 4.0, dot);
    }
    ui.painter().galley(
        egui::pos2(rect.left() + 12.0 + lead, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response
}

/// Couleur d'une étiquette : son accentuation, comme dans Paramètres.
pub(super) fn tag_color(tags: &places::Tags, tag: &str) -> Color32 {
    style::accent_color(tags.color_of(tag))
}

fn chevron(painter: &egui::Painter, rect: egui::Rect, left: bool, color: Color32, width: f32) {
    let c = rect.center();
    let (x, y) = (if left { 2.5 } else { -2.5 }, 5.0);
    painter.line(
        vec![
            c + egui::vec2(x, -y),
            c + egui::vec2(-x, 0.0),
            c + egui::vec2(x, y),
        ],
        Stroke::new(width, color),
    );
}

/// Barre de remplissage d'un disque.
fn usage_bar(painter: &egui::Painter, rect: egui::Rect, used: f32, look: &Look) {
    painter.rect_filled(rect, 2.0, look.track);
    let fill = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width() * used.clamp(0.0, 1.0), rect.height()),
    );
    painter.rect_filled(fill, 2.0, look.accent);
}

fn usage_text((free, total): (u64, u64)) -> String {
    format!("{} libres sur {}", format_size(free), format_size(total))
}

fn used_part((free, total): (u64, u64)) -> f32 {
    if total == 0 {
        0.0
    } else {
        1.0 - free as f32 / total as f32
    }
}

/// Nature d'un élément, comme la colonne « Type ».
pub(super) fn kind_label(entry: &FileEntry) -> String {
    if entry.directory {
        return "Dossier".into();
    }
    let extension = extension(&entry.name).to_ascii_lowercase();
    match extension.as_str() {
        "pdf" => "Document PDF".into(),
        "doc" | "docx" => "Document Word".into(),
        "odt" => "Document OpenDocument".into(),
        "rtf" => "Document RTF".into(),
        "xls" | "xlsx" => "Classeur Excel".into(),
        "ods" => "Classeur OpenDocument".into(),
        "ppt" | "pptx" => "Présentation PowerPoint".into(),
        "odp" => "Présentation OpenDocument".into(),
        "txt" | "md" | "log" => "Document texte".into(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => {
            format!("Image {}", extension.to_uppercase())
        }
        "mp3" | "ogg" | "flac" | "wav" | "m4a" | "opus" => format!("Audio {}", extension.to_uppercase()),
        "mp4" | "mkv" | "webm" | "avi" | "mov" => format!("Vidéo {}", extension.to_uppercase()),
        "zip" | "tar" | "gz" | "xz" | "bz2" | "zst" | "7z" | "rar" => {
            format!("Archive {}", extension.to_uppercase())
        }
        "sh" => "Script shell".into(),
        "rs" | "py" | "js" | "ts" | "c" | "h" | "toml" | "json" | "yaml" | "yml" | "html" | "css" => {
            format!("Code {}", extension.to_uppercase())
        }
        "appimage" | "run" | "bin" | "exe" => "Programme".into(),
        "" => "Document".into(),
        other => format!("Fichier {}", other.to_uppercase()),
    }
}

/// Application de Ferrite qui ouvre ce genre de fichier (liste commune
/// `ferrite_system::formats`) ; les archives s'ouvrent dans Fichiers.
pub(super) fn opener_label(entry: &FileEntry) -> &'static str {
    if entry.directory || super::ops::is_archive(&entry.path) {
        "Fichiers"
    } else {
        ferrite_system::formats::builtin_for(&entry.path).map_or("—", |app| app.name())
    }
}

impl FilesState {
    pub fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.sync_deep_search(&ctx);
        let look = look();
        let mut actions = Vec::new();
        let mut targets = Targets::new();
        self.keyboard(ui, &mut actions);

        let picker = self.picker.is_some();
        style::sidebar(ui, "emplacements", SIDEBAR_WIDTH, look.side, !picker, |ui| {
            self.sidebar(ui, &look, &mut actions, &mut targets)
        });
        style::toolbar_with(ui, !picker, look.win, look.line, |ui| {
            self.toolbar(ui, &look, &mut actions, &mut targets)
        });
        if !picker && self.tabs.len() > 1 {
            self.tab_bar(ui, &look, &mut actions);
        }
        if picker {
            self.picker_bar(ui, &mut actions);
        } else {
            if self.preview {
                self.preview_panel(ui, &look, &mut actions);
            }
            self.status_panel(ui, &look, &mut actions);
        }
        let analyzing = matches!(self.location, Location::Analysis { .. });
        if analyzing {
            self.analysis_bar(ui, &look, &mut actions);
        } else {
            self.filter_bar(ui, &look, &mut actions);
        }
        if !self.recent_strip.is_empty() && self.search.trim().is_empty() && self.filter == Filter::All {
            self.recent_panel(ui, &look, &mut actions);
        }
        self.item_rects.clear();
        let (layer, (area, offset)) = egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(look.win))
            .show(ui, |ui| {
                // L'analyse se lit en liste : tailles, parts et barres alignées.
                let shown = match self.view {
                    _ if analyzing => self.list(ui, &look, &mut actions, &mut targets),
                    View::List => self.list(ui, &look, &mut actions, &mut targets),
                    View::Grid => self.grid(ui, &look, &mut actions, &mut targets),
                };
                (ui.layer_id(), shown)
            })
            .inner;
        self.lasso_frame(&ctx, &look, layer, area, offset);
        self.drag_frame(&ctx, &look, &targets, &mut actions);
        self.deposits(&ctx, &targets, &mut actions);
        self.quick_look(&ctx, &look, &mut actions);
        self.dialogs(&ctx, &mut actions);

        for action in actions {
            self.apply(action, &ctx);
        }
    }

    // ----- Barre latérale -----

    fn sidebar(&mut self, ui: &mut egui::Ui, look: &Look, actions: &mut Vec<Action>, targets: &mut Targets) {
        // Titre de la fenêtre, en haut de la colonne (au-dessus de la zone de `ui`).
        if self.picker.is_none() {
            let top = ui.max_rect().top() - style::TOOLBAR_HEIGHT / 2.0;
            ui.ctx().layer_painter(ui.layer_id()).text(
                egui::pos2(ui.max_rect().left() + 4.0, top),
                egui::Align2::LEFT_CENTER,
                "Fichiers",
                font(15.0, true),
                look.fg,
            );
        }
        let home = home();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            self.heading(ui, look, "Favoris");
            let recent_on = self.location == Location::Recent;
            if self
                .place(ui, look, "app-clock", "Récents", None, recent_on)
                .clicked()
            {
                actions.push(Action::Show(Location::Recent));
            }
            let mut favorites = vec![("Dossier perso", home.clone(), "places-home")];
            for (name, folder, icon) in [
                ("Bureau", "Bureau", "places-desktop"),
                ("Documents", "Documents", "places-documents"),
                ("Téléchargements", "Téléchargements", "places-downloads"),
                ("Images", "Images", "places-pictures"),
                ("Musique", "Musique", "places-music"),
                ("Vidéos", "Vidéos", "places-videos"),
            ] {
                let path = home.join(folder);
                if path.is_dir() {
                    favorites.push((name, path, icon));
                }
            }
            for (name, path, icon) in favorites {
                let count =
                    (icon == "places-downloads" && self.downloads > 0).then(|| self.downloads.to_string());
                let selected = self.current_dir() == Some(path.as_path());
                let response = self.place(ui, look, icon, name, count.as_deref(), selected);
                targets.push((response.rect, Target::Folder(path.clone())));
                if response.clicked() {
                    actions.push(Action::Navigate(path));
                }
            }
            if self.picker.is_none() {
                let icon = if trash::is_empty() {
                    "places-trash"
                } else {
                    "places-trash-full"
                };
                let response = self.place(
                    ui,
                    look,
                    icon,
                    "Corbeille",
                    None,
                    self.location == Location::Trash,
                );
                targets.push((response.rect, Target::Trash));
                if response.clicked() {
                    actions.push(Action::ShowTrash);
                }
                let analyzing = matches!(self.location, Location::Analysis { .. });
                if self
                    .place(ui, look, "status-storage", "Analyse de l’espace", None, analyzing)
                    .on_hover_text("Retrouver les dossiers et fichiers les plus lourds")
                    .clicked()
                {
                    actions.push(Action::Analyze(home.clone()));
                }
            }

            self.heading(ui, look, "Appareils");
            let system = self.system_usage;
            let selected = self.current_dir() == Some(Path::new("/"));
            if self.device(ui, look, "Système", system, None, selected).clicked() {
                actions.push(Action::Navigate(PathBuf::from("/")));
            }
            for device in self.devices.clone() {
                let selected = device
                    .mount
                    .as_deref()
                    .is_some_and(|mount| self.current_dir() == Some(mount));
                let mut eject = false;
                let button = device.removable.then_some(&mut eject);
                let response = self.device(ui, look, &device.label, device.usage, button, selected);
                if let Some(mount) = &device.mount {
                    targets.push((response.rect, Target::Folder(mount.clone())));
                }
                if let Some(mount) = device.mount.clone() {
                    response.context_menu(|ui| {
                        style::menu_style(ui);
                        if style::menu_row(ui, "Analyser l’espace", "", false, true).clicked() {
                            actions.push(Action::Analyze(mount));
                            ui.close();
                        }
                    });
                }
                if eject {
                    actions.push(Action::Eject(device));
                } else if response.clicked() {
                    actions.push(match &device.mount {
                        Some(mount) => Action::Navigate(mount.clone()),
                        None => Action::Mount(device),
                    });
                }
            }

            if self.picker.is_none() {
                if self.heading_with_add(ui, look, "Étiquettes", "Nouvelle étiquette") {
                    actions.push(Action::EditTag(None, None));
                }
                let names: Vec<String> = self.tags.list().iter().map(|def| def.name.clone()).collect();
                for tag in names {
                    let selected = self.location == Location::Tag(tag.clone());
                    let response = self.tag_row(ui, look, &tag, selected);
                    if response.clicked() {
                        actions.push(Action::Show(Location::Tag(tag.clone())));
                    }
                    response.context_menu(|ui| {
                        style::menu_style(ui);
                        if style::menu_row(ui, "Modifier…", "", false, true).clicked() {
                            actions.push(Action::EditTag(Some(tag.clone()), None));
                            ui.close();
                        }
                        if style::menu_row(ui, "Supprimer…", "", true, true).clicked() {
                            actions.push(Action::AskDeleteTag(tag.clone()));
                            ui.close();
                        }
                    });
                }
            }
        });
    }

    /// Titre de rubrique avec un bouton « + » à droite ; vrai si on l'a cliqué.
    fn heading_with_add(&self, ui: &mut egui::Ui, look: &Look, title: &str, hint: &str) -> bool {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), Sense::hover());
        ui.painter().text(
            egui::pos2(rect.left() + 10.0, rect.bottom() - 5.0),
            egui::Align2::LEFT_BOTTOM,
            title.to_uppercase(),
            font(11.0, true),
            look.head,
        );
        let button = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 16.0, rect.bottom() - 11.0),
            egui::Vec2::splat(24.0),
        );
        let response = ui
            .interact(button, ui.scope_id().with(("ajouter", title)), Sense::click())
            .on_hover_text(hint)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.hovered() {
            ui.painter().rect_filled(button, 7.0, style::tint(10));
        }
        let c = button.center();
        let stroke = Stroke::new(1.6, look.fg2);
        ui.painter()
            .line_segment([c - egui::vec2(5.0, 0.0), c + egui::vec2(5.0, 0.0)], stroke);
        ui.painter()
            .line_segment([c - egui::vec2(0.0, 5.0), c + egui::vec2(0.0, 5.0)], stroke);
        response.clicked()
    }

    fn heading(&self, ui: &mut egui::Ui, look: &Look, title: &str) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), Sense::hover());
        ui.painter().text(
            egui::pos2(rect.left() + 10.0, rect.bottom() - 5.0),
            egui::Align2::LEFT_BOTTOM,
            title.to_uppercase(),
            font(11.0, true),
            look.head,
        );
    }

    /// Ligne d'emplacement : icône de 22, nom de 14, compteur éventuel à droite.
    fn place(
        &mut self,
        ui: &mut egui::Ui,
        look: &Look,
        icon: &'static str,
        name: &str,
        count: Option<&str>,
        selected: bool,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if selected {
            ui.painter().rect_filled(rect, 9.0, look.sel_bg);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 9.0, style::tint(8));
        }
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 21.0, rect.center().y),
            egui::Vec2::splat(22.0),
        );
        self.icons.paint(ui, icon, icon_rect, IconStyle::Color);
        let right = match count {
            Some(count) => {
                text(
                    ui,
                    count,
                    egui::pos2(rect.right() - 10.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    font(12.0, false),
                    look.fg3,
                    40.0,
                )
                .left()
                    - 6.0
            }
            None => rect.right() - 10.0,
        };
        text(
            ui,
            name,
            egui::pos2(icon_rect.right() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            font(14.0, selected),
            if selected { look.sel_fg } else { look.fg },
            right - icon_rect.right() - 10.0,
        );
        response
    }

    /// Appareil : nom, bouton d'éjection (clés), remplissage et espace libre.
    fn device(
        &mut self,
        ui: &mut egui::Ui,
        look: &Look,
        name: &str,
        usage: Option<(u64, u64)>,
        eject: Option<&mut bool>,
        selected: bool,
    ) -> egui::Response {
        let height = if usage.is_some() { 58.0 } else { 34.0 };
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if selected {
            ui.painter().rect_filled(rect, 9.0, look.sel_bg);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 9.0, style::tint(8));
        }
        let line = rect.top() + 17.0;
        let icon = if eject.is_some() {
            "places-usb"
        } else {
            "places-computer"
        };
        let icon_rect =
            egui::Rect::from_center_size(egui::pos2(rect.left() + 21.0, line), egui::Vec2::splat(22.0));
        self.icons.paint(ui, icon, icon_rect, IconStyle::Color);
        text(
            ui,
            name,
            egui::pos2(icon_rect.right() + 10.0, line),
            egui::Align2::LEFT_CENTER,
            font(14.0, selected),
            if selected { look.sel_fg } else { look.fg },
            rect.right() - icon_rect.right() - 44.0,
        );
        if let Some(eject) = eject {
            let button =
                egui::Rect::from_center_size(egui::pos2(rect.right() - 18.0, line), egui::Vec2::splat(24.0));
            let clicked = ui
                .interact(button, response.id.with("ejecter"), Sense::click())
                .on_hover_text("Éjecter");
            if clicked.hovered() {
                ui.painter().rect_filled(button, 12.0, style::tint(12));
            }
            paint_glyph(ui, Glyph::Eject, button.shrink(3.0), look.fg3);
            *eject = clicked.clicked();
        }
        if let Some(usage) = usage {
            let left = icon_rect.right() + 10.0;
            let bar = egui::Rect::from_min_size(
                egui::pos2(left, line + 15.0),
                egui::vec2(rect.right() - 12.0 - left, 4.0),
            );
            usage_bar(ui.painter(), bar, used_part(usage), look);
            ui.painter().text(
                egui::pos2(left, bar.bottom() + 4.0),
                egui::Align2::LEFT_TOP,
                usage_text(usage),
                font(11.0, false),
                look.fg3,
            );
        }
        response
    }

    fn tag_row(&self, ui: &mut egui::Ui, look: &Look, tag: &str, selected: bool) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if selected {
            ui.painter().rect_filled(rect, 9.0, look.sel_bg);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 9.0, style::tint(8));
        }
        let dot = egui::pos2(rect.left() + 17.0, rect.center().y);
        ui.painter().circle_filled(dot, 5.0, tag_color(&self.tags, tag));
        ui.painter().text(
            egui::pos2(dot.x + 17.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            tag,
            font(14.0, selected),
            if selected { look.sel_fg } else { look.fg },
        );
        response
    }

    // ----- Barre d'outils -----

    fn toolbar(&mut self, ui: &mut egui::Ui, look: &Look, actions: &mut Vec<Action>, targets: &mut Targets) {
        ui.spacing_mut().item_spacing.x = 2.0;
        let back = !self.back.is_empty();
        let forward = !self.forward.is_empty();
        let size = egui::vec2(28.0, 28.0);
        if icon_button(ui, size, 9, None, "Précédent (Alt+←)", |p, r| {
            chevron(p, r, true, if back { look.icon } else { look.fg3 }, 2.0)
        })
        .clicked()
            && back
        {
            actions.push(Action::Back);
        }
        if icon_button(ui, size, 9, None, "Suivant (Alt+→)", |p, r| {
            chevron(p, r, false, if forward { look.icon } else { look.fg3 }, 2.0)
        })
        .clicked()
            && forward
        {
            actions.push(Action::Forward);
        }
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            // Panneau d'aperçu.
            let on = self.preview;
            if icon_button(
                ui,
                egui::vec2(28.0, 28.0),
                8,
                Some(if on { look.sel_bg } else { look.chip }),
                "Panneau d’aperçu",
                |p, r| {
                    let ink = if on { look.sel_fg } else { look.icon };
                    let frame = egui::Rect::from_center_size(r.center(), egui::vec2(12.0, 10.0));
                    p.rect_stroke(frame, 1.0, Stroke::new(1.5, ink), egui::StrokeKind::Middle);
                    p.vline(frame.right() - 3.0, frame.y_range(), Stroke::new(1.5, ink));
                },
            )
            .clicked()
            {
                actions.push(Action::TogglePreview);
            }
            // Thème clair ou sombre.
            let dark = style::theme_is_dark();
            if icon_button(
                ui,
                egui::vec2(28.0, 28.0),
                8,
                Some(look.chip),
                if dark {
                    "Passer en thème clair"
                } else {
                    "Passer en thème sombre"
                },
                |p, r| {
                    let c = r.center();
                    let stroke = Stroke::new(1.5, look.icon);
                    if dark {
                        p.circle_stroke(c, 3.4, stroke);
                        for ray in 0..8 {
                            let angle = std::f32::consts::TAU * ray as f32 / 8.0;
                            let (sin, cos) = angle.sin_cos();
                            p.line_segment(
                                [c + egui::vec2(cos, sin) * 5.6, c + egui::vec2(cos, sin) * 7.2],
                                stroke,
                            );
                        }
                    } else {
                        // Croissant de lune.
                        let points: Vec<egui::Pos2> = (0..=24)
                            .map(|i| {
                                let t = std::f32::consts::PI * (0.15 + 1.55 * i as f32 / 24.0);
                                c + egui::vec2(t.cos(), t.sin()) * 6.0
                            })
                            .chain((0..=24).map(|i| {
                                let t = std::f32::consts::PI * (1.7 - 1.55 * i as f32 / 24.0);
                                c + egui::vec2(2.4, -2.4) + egui::vec2(t.cos(), t.sin()) * 4.6
                            }))
                            .collect();
                        p.add(egui::Shape::closed_line(points, stroke));
                    }
                },
            )
            .clicked()
            {
                actions.push(Action::ToggleTheme);
            }
            // Nouvel onglet (pas dans le sélecteur).
            if self.picker.is_none()
                && icon_button(
                    ui,
                    egui::vec2(28.0, 28.0),
                    8,
                    Some(look.chip),
                    "Nouvel onglet (Ctrl+T)",
                    |p, r| {
                        let c = r.center();
                        let stroke = Stroke::new(1.6, look.icon);
                        p.line_segment([c - egui::vec2(6.0, 0.0), c + egui::vec2(6.0, 0.0)], stroke);
                        p.line_segment([c - egui::vec2(0.0, 6.0), c + egui::vec2(0.0, 6.0)], stroke);
                    },
                )
                .clicked()
            {
                actions.push(Action::NewTab(None));
            }
            // Recherche : 220 × 28, moins quand la fenêtre est étroite (le chemin d'abord).
            let search_width = (ui.available_width() * 0.3 - 40.0).clamp(150.0, 220.0);
            let (field, _) = ui.allocate_exact_size(egui::vec2(search_width, 28.0), Sense::hover());
            ui.painter().rect_filled(field, 8.0, look.chip);
            let c = egui::pos2(field.left() + 16.0, field.center().y - 1.0);
            ui.painter().circle_stroke(c, 4.2, Stroke::new(1.6, look.fg3));
            ui.painter().line_segment(
                [c + egui::vec2(3.2, 3.2), c + egui::vec2(6.3, 6.3)],
                Stroke::new(1.6, look.fg3),
            );
            let input = egui::Rect::from_min_max(
                egui::pos2(field.left() + 30.0, field.top()),
                egui::pos2(field.right() - 10.0, field.bottom()),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(input)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.add(
                egui::TextEdit::singleline(&mut self.search)
                    .id(egui::Id::unique(super::SEARCH_FIELD))
                    // Même police que le texte tapé : sinon l'invitation, plus grande, remonte.
                    .hint_text(RichText::new("Rechercher").font(font(14.0, false)))
                    .font(font(14.0, false))
                    // Centré dans les 28 points du champ, comme la loupe (egui cale en haut).
                    .min_size(egui::vec2(0.0, input.height()))
                    .align(egui::Align2::LEFT_CENTER)
                    .text_color(look.fg)
                    .frame(egui::Frame::NONE)
                    .desired_width(input.width()),
            );
            // Grille ou liste : rail de 2 autour de deux segments de 30 × 24.
            let (rail, _) =
                ui.allocate_exact_size(egui::vec2(2.0 + 30.0 + 2.0 + 30.0 + 2.0, 28.0), Sense::hover());
            ui.painter().rect_filled(rail, 8.0, look.chip);
            for (slot, (view, hint)) in [(View::Grid, "Grille"), (View::List, "Liste")]
                .into_iter()
                .enumerate()
            {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(rail.left() + 2.0 + slot as f32 * 32.0, rail.top() + 2.0),
                    egui::vec2(30.0, 24.0),
                );
                let response = ui.interact(rect, ui.scope_id().with(("vue", slot)), Sense::click());
                let on = self.view == view;
                if on {
                    ui.painter()
                        .rect_filled(rect.translate(egui::vec2(0.0, 1.0)), 8.0, style::tint(30));
                    ui.painter().rect_filled(rect, 8.0, look.seg_on);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 8.0, style::tint(8));
                }
                paint_glyph(
                    ui,
                    if view == View::Grid {
                        Glyph::Grid
                    } else {
                        Glyph::List
                    },
                    rect,
                    look.icon,
                );
                if response.on_hover_text(hint).clicked() {
                    actions.push(Action::SetView(view));
                }
            }
            // Le chemin dans la place restante.
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                self.crumbs(ui, look, actions, targets);
            });
        });
    }

    /// Rangée d'onglets sous la barre d'outils, affichée dès qu'il y en a deux.
    fn tab_bar(&mut self, ui: &mut egui::Ui, look: &Look, actions: &mut Vec<Action>) {
        let titles: Vec<String> = (0..self.tabs.len()).map(|index| self.tab_title(index)).collect();
        let active = self.active_tab;
        egui::Panel::top(ui.scope_id().with("onglets"))
            .resizable(false)
            .exact_size(TAB_BAR_HEIGHT)
            .frame(
                Frame::new()
                    .fill(look.side)
                    .inner_margin(Margin::symmetric(10, 0)),
            )
            .show(ui, |ui| {
                let bar = ui.max_rect();
                let full = bar.expand2(egui::vec2(10.0, 0.0));
                ui.painter()
                    .hline(full.x_range(), full.bottom() - 0.5, Stroke::new(1.0, look.line));
                let count = titles.len() as f32;
                let gap = 4.0;
                let width = ((bar.width() - gap * (count - 1.0)) / count).min(240.0);
                for (index, title) in titles.iter().enumerate() {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(bar.left() + index as f32 * (width + gap), bar.top() + 5.0),
                        egui::vec2(width, bar.height() - 10.0),
                    );
                    let id = ui.scope_id().with(("onglet", index));
                    let response = ui.interact(rect, id, Sense::click());
                    let on = index == active;
                    if on {
                        ui.painter()
                            .rect_filled(rect.translate(egui::vec2(0.0, 1.0)), 8.0, style::tint(20));
                        ui.painter().rect_filled(rect, 8.0, look.win);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 8.0, style::tint(8));
                    }
                    // Croix de fermeture, à droite, sur l'onglet actif ou survolé.
                    let close = egui::Rect::from_center_size(
                        egui::pos2(rect.right() - 14.0, rect.center().y),
                        egui::vec2(20.0, 20.0),
                    );
                    let close_response = ui
                        .interact(close, id.with("fermer"), Sense::click())
                        .on_hover_text("Fermer l’onglet (Ctrl+W)");
                    let show_close = on || response.hovered() || close_response.hovered();
                    if show_close {
                        if close_response.hovered() {
                            ui.painter().rect_filled(close, 6.0, style::tint(14));
                        }
                        let c = close.center();
                        let stroke = Stroke::new(1.4, look.fg2);
                        ui.painter()
                            .line_segment([c - egui::vec2(4.0, 4.0), c + egui::vec2(4.0, 4.0)], stroke);
                        ui.painter()
                            .line_segment([c + egui::vec2(-4.0, 4.0), c + egui::vec2(4.0, -4.0)], stroke);
                    }
                    text(
                        ui,
                        title,
                        rect.center() - egui::vec2(6.0, 0.0),
                        egui::Align2::CENTER_CENTER,
                        font(13.0, on),
                        if on { look.fg } else { look.fg2 },
                        width - 44.0,
                    );
                    // La croix ou un clic du milieu ferment l'onglet.
                    if close_response.clicked() || response.middle_clicked() {
                        actions.push(Action::CloseTab(index));
                    } else if response.clicked() {
                        actions.push(Action::SwitchTab(index));
                    }
                }
            });
    }

    fn crumbs(&mut self, ui: &mut egui::Ui, look: &Look, actions: &mut Vec<Action>, targets: &mut Targets) {
        if let Some(edit) = &mut self.path_edit {
            let response = ui.add(egui::TextEdit::singleline(edit).desired_width(ui.available_width() - 8.0));
            if !response.has_focus() {
                response.request_focus();
                select_all(ui, response.id, edit);
            }
            if ui.input(|input| input.key_pressed(Key::Enter)) {
                actions.push(Action::CommitPath);
            } else if ui.input(|input| input.key_pressed(Key::Escape)) || response.lost_focus() {
                self.path_edit = None;
            }
            return;
        }
        let parts: Vec<(String, Option<Location>)> = match &self.location {
            Location::Trash => vec![("Corbeille".into(), None)],
            Location::Recent => vec![("Récents".into(), None)],
            Location::Tag(tag) => vec![(tag.clone(), None)],
            Location::Analysis { dir, largest } => {
                let root = self
                    .scan
                    .as_ref()
                    .map_or_else(|| dir.clone(), |scan| scan.root.clone());
                let mut parts: Vec<(String, Option<Location>)> = dir
                    .ancestors()
                    .take_while(|part| part.starts_with(&root))
                    .map(|part| {
                        let name = if part == home() {
                            "Dossier perso".to_owned()
                        } else if part.as_os_str() == "/" {
                            "Système".to_owned()
                        } else {
                            file_name(part)
                        };
                        (
                            name,
                            Some(Location::Analysis {
                                dir: part.to_path_buf(),
                                largest: false,
                            }),
                        )
                    })
                    .collect();
                parts.push(("Analyse".into(), None));
                parts.reverse();
                if *largest {
                    parts.push(("Plus gros fichiers".into(), None));
                }
                parts
            }
            Location::Archive { archive, directory } => {
                let parent = archive.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
                let parent_name = if parent == home() {
                    "Dossier perso".to_owned()
                } else if parent.as_os_str() == "/" {
                    "Système".to_owned()
                } else {
                    file_name(&parent)
                };
                let mut parts = vec![
                    (parent_name, Some(Location::Folder(parent))),
                    (
                        file_name(archive),
                        Some(Location::Archive {
                            archive: archive.clone(),
                            directory: String::new(),
                        }),
                    ),
                ];
                let mut current = String::new();
                for component in directory.split('/').filter(|part| !part.is_empty()) {
                    if !current.is_empty() {
                        current.push('/');
                    }
                    current.push_str(component);
                    parts.push((
                        component.to_owned(),
                        Some(Location::Archive {
                            archive: archive.clone(),
                            directory: current.clone(),
                        }),
                    ));
                }
                parts
            }
            Location::Folder(dir) => {
                let home = home();
                let mut parts: Vec<PathBuf> = if dir.starts_with(&home) {
                    dir.ancestors()
                        .take_while(|p| *p != home)
                        .map(Path::to_path_buf)
                        .collect()
                } else {
                    dir.ancestors().map(Path::to_path_buf).collect()
                };
                if dir.starts_with(&home) {
                    parts.push(home.clone());
                }
                parts.reverse();
                parts
                    .into_iter()
                    .map(|part| {
                        let name = if part == home {
                            "Dossier perso".to_owned()
                        } else if part.as_os_str() == "/" {
                            "Système".to_owned()
                        } else {
                            file_name(&part)
                        };
                        (name, Some(Location::Folder(part)))
                    })
                    .collect()
            }
        };
        let last = parts.len().saturating_sub(1);
        // Les premiers éléments cèdent la place quand le chemin est long.
        let budget = (ui.available_width() - 30.0).max(60.0);
        let widths: Vec<f32> = parts
            .iter()
            .map(|(name, _)| {
                ui.painter()
                    .layout_no_wrap(name.clone(), font(14.0, true), look.fg)
                    .size()
                    .x
                    + 20.0
                    + 14.0
            })
            .collect();
        let mut first = 0;
        while first < last && widths[first..].iter().sum::<f32>() > budget {
            first += 1;
        }
        for (index, (name, path)) in parts.into_iter().enumerate().skip(first) {
            if index > first {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 28.0), Sense::hover());
                chevron(ui.painter(), rect, false, look.fg3, 1.8);
            }
            let current = index == last;
            let galley = ui
                .painter()
                .layout_no_wrap(name.clone(), font(14.0, current), look.fg);
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2((galley.size().x + 20.0).min(budget), 28.0),
                Sense::click(),
            );
            if current {
                ui.painter().rect_filled(rect, 8.0, look.chip);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 8.0, style::tint(8));
            }
            text(
                ui,
                &name,
                rect.center(),
                egui::Align2::CENTER_CENTER,
                font(14.0, current),
                if current { look.fg } else { look.fg2 },
                rect.width() - 16.0,
            );
            if let Some(Location::Folder(path)) = &path {
                targets.push((rect, Target::Folder(path.clone())));
            }
            if response.clicked() {
                match (current, path) {
                    (true, Some(Location::Folder(_))) => actions.push(Action::EditPath),
                    (true, Some(Location::Archive { .. })) => {}
                    (false, Some(location)) => actions.push(Action::Show(location)),
                    _ => {}
                }
            }
        }
        // Un clic dans le vide du chemin permet de le saisir (Ctrl+L).
        let rest = ui.allocate_response(ui.available_size().max(egui::Vec2::ZERO), Sense::click());
        if rest.clicked() {
            actions.push(Action::EditPath);
        }
    }

    // ----- Analyse de l'espace -----

    /// Total de référence des parts : le dossier analysé (ou la somme de ses éléments).
    fn analysis_total(&self) -> Option<u64> {
        let Location::Analysis { dir, .. } = &self.location else {
            return None;
        };
        let counted = self.scan.as_ref().and_then(|scan| scan.size_of(dir));
        Some(counted.unwrap_or_else(|| self.entries.iter().filter_map(|entry| entry.size).sum()))
    }

    /// Bandeau de l'analyse : Contenu ou Plus gros fichiers, avancement, Actualiser.
    fn analysis_bar(&mut self, ui: &mut egui::Ui, look: &Look, actions: &mut Vec<Action>) {
        let largest = matches!(self.location, Location::Analysis { largest: true, .. });
        let progress = self.scan.as_ref().map(analysis::Scan::progress);
        let total = self.analysis_total().unwrap_or(0);
        egui::Panel::top(ui.scope_id().with("analyse"))
            .resizable(false)
            .show_separator_line(false)
            .exact_size(FILTER_BAR)
            .frame(Frame::new().fill(look.win).inner_margin(Margin::symmetric(22, 0)))
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if chip(ui, look, "Contenu", None, !largest).clicked() {
                        actions.push(Action::AnalysisMode(false));
                    }
                    if chip(ui, look, "Plus gros fichiers", None, largest).clicked() {
                        actions.push(Action::AnalysisMode(true));
                    }
                    ui.add_space(12.0);
                    let summary = match progress {
                        Some(progress) if !progress.done => format!(
                            "Analyse en cours… {} fichier{} · {}",
                            progress.files,
                            plural(progress.files as usize),
                            format_size(progress.bytes)
                        ),
                        Some(progress) => {
                            let mut text = format!(
                                "{} ici · {} fichier{} analysé{}",
                                format_size(total),
                                progress.files,
                                plural(progress.files as usize),
                                plural(progress.files as usize)
                            );
                            if progress.unreadable > 0 {
                                text.push_str(&format!(
                                    " · {} dossier{} illisible{}",
                                    progress.unreadable,
                                    plural(progress.unreadable as usize),
                                    plural(progress.unreadable as usize)
                                ));
                            }
                            text
                        }
                        None => String::new(),
                    };
                    ui.label(RichText::new(summary).font(font(12.0, false)).color(look.fg2));
                    if progress.is_some_and(|progress| !progress.done) {
                        ui.add(egui::Spinner::new().size(14.0).color(look.fg3));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if chip(ui, look, "Actualiser", None, false)
                            .on_hover_text("Tout recompter")
                            .clicked()
                        {
                            actions.push(Action::Rescan);
                        }
                        ui.label(
                            RichText::new("Suppr : mettre à la Corbeille")
                                .font(font(11.0, false))
                                .color(look.fg3),
                        );
                    });
                });
            });
    }

    // ----- Filtres et tri -----

    /// Pastilles des natures présentes dans le dossier, et menu « Trier » à droite.
    fn filter_bar(&mut self, ui: &mut egui::Ui, look: &Look, actions: &mut Vec<Action>) {
        let present: Vec<Filter> = Filter::ALL
            .into_iter()
            .filter(|filter| {
                *filter == Filter::All
                    || *filter == self.filter
                    || self.entries.iter().any(|entry| {
                        (self.show_hidden || !entry.name.starts_with('.')) && filter.matches(entry)
                    })
            })
            .collect();
        egui::Panel::top(ui.scope_id().with("filtres"))
            .resizable(false)
            .show_separator_line(false)
            .exact_size(FILTER_BAR)
            .frame(Frame::new().fill(look.win).inner_margin(Margin::symmetric(22, 0)))
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    // Une seule pastille « Tout » n'apporte rien.
                    if present.len() > 2 {
                        for filter in present {
                            if chip(ui, look, filter.label(), None, self.filter == filter).clicked() {
                                actions.push(Action::SetFilter(filter));
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.sort_menu(ui, look, actions);
                    });
                });
            });
    }

    fn sort_menu(&mut self, ui: &mut egui::Ui, look: &Look, actions: &mut Vec<Action>) {
        let (key_label, _) = SORT_KEYS
            .iter()
            .find(|(_, key)| *key == self.sort)
            .copied()
            .unwrap_or(SORT_KEYS[0]);
        let label = format!("Trier : {}", key_label.to_lowercase());
        let galley = ui.painter().layout_no_wrap(label, font(12.0, false), look.fg2);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(galley.size().x + 34.0, 28.0), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.hovered() {
            ui.painter().rect_filled(rect, 8.0, style::tint(8));
        }
        ui.painter().galley(
            egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
            galley,
            look.fg2,
        );
        let c = egui::pos2(rect.right() - 14.0, rect.center().y);
        ui.painter().line(
            vec![
                c + egui::vec2(-3.5, -1.5),
                c + egui::vec2(0.0, 2.0),
                c + egui::vec2(3.5, -1.5),
            ],
            Stroke::new(1.5, look.fg3),
        );
        egui::Popup::menu(&response).show(|ui| {
            style::menu_style(ui);
            for (label, key) in SORT_KEYS {
                if style::radio_row(ui, label, self.sort == key).clicked() {
                    let ascending = if self.sort == key {
                        self.ascending
                    } else {
                        matches!(key, SortKey::Name | SortKey::Kind)
                    };
                    actions.push(Action::SortBy(key, ascending));
                    ui.close();
                }
            }
            ui.separator();
            for (label, ascending) in [("Croissant", true), ("Décroissant", false)] {
                if style::radio_row(ui, label, self.ascending == ascending).clicked() {
                    actions.push(Action::SortBy(self.sort, ascending));
                    ui.close();
                }
            }
        });
    }

    /// Fichiers ouverts récemment, en cartes, en tête du dossier personnel.
    fn recent_panel(&mut self, ui: &mut egui::Ui, look: &Look, actions: &mut Vec<Action>) {
        let recent = self.recent_strip.clone();
        let ctx = ui.ctx().clone();
        egui::Panel::top(ui.scope_id().with("recents"))
            .resizable(false)
            .show_separator_line(false)
            .exact_size(RECENT_PANEL)
            .frame(Frame::new().fill(look.win).inner_margin(Margin::symmetric(22, 0)))
            .show(ui, |ui| {
                let area = ui.max_rect();
                ui.painter().text(
                    egui::pos2(area.left(), area.top() + 12.0),
                    egui::Align2::LEFT_CENTER,
                    "Récemment ouverts",
                    font(13.0, true),
                    look.fg2,
                );
                let top = area.top() + 26.0;
                let columns = ((area.width() + 10.0) / (RECENT_CARD_MIN + 10.0))
                    .floor()
                    .max(1.0) as usize;
                let shown = recent.len().min(columns);
                let width = (area.width() - 10.0 * (columns - 1) as f32) / columns as f32;
                for (slot, entry) in recent.iter().take(shown).enumerate() {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(area.left() + slot as f32 * (width + 10.0), top),
                        egui::vec2(width, 52.0),
                    );
                    let response = ui
                        .interact(rect, ui.scope_id().with(("recent", slot)), Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(display_location(&entry.path));
                    ui.painter().rect_filled(
                        rect,
                        12.0,
                        if response.hovered() { look.stage } else { look.chip },
                    );
                    let art = egui::Rect::from_center_size(
                        egui::pos2(rect.left() + 28.0, rect.center().y),
                        egui::Vec2::splat(34.0),
                    );
                    match self.thumbs.get(&ctx, &entry.path) {
                        Some(texture) => paint_cover(ui, &texture, art, 6.0),
                        None => self.icons.paint(ui, entry_icon(entry), art, IconStyle::Color),
                    }
                    let left = art.right() + 10.0;
                    let room = rect.right() - left - 10.0;
                    text(
                        ui,
                        &entry.name,
                        egui::pos2(left, rect.center().y - 9.0),
                        egui::Align2::LEFT_CENTER,
                        font(13.0, true),
                        look.fg,
                        room,
                    );
                    text(
                        ui,
                        &format_time(entry.modified),
                        egui::pos2(left, rect.center().y + 9.0),
                        egui::Align2::LEFT_CENTER,
                        font(11.0, false),
                        look.fg3,
                        room,
                    );
                    if response.clicked() {
                        actions.push(Action::Open(entry.path.clone(), false));
                    }
                }
                ui.painter().text(
                    egui::pos2(area.left(), area.bottom() - 12.0),
                    egui::Align2::LEFT_CENTER,
                    "Tous les fichiers",
                    font(13.0, true),
                    look.fg2,
                );
            });
    }

    // ----- Barre d'état -----

    /// Bande d'informations du bas : l'élément choisi (icône, nom, nature, taille, date)
    /// avec Partager et Ouvrir ; sinon le résumé du dossier et l'espace libre ; une copie
    /// en cours prend sa place.
    fn status_panel(&mut self, ui: &mut egui::Ui, look: &Look, actions: &mut Vec<Action>) {
        let visible = self.visible().len();
        let selected = self.selected.len();
        let detail = self.selection_detail();
        let job = self.jobs.first().map(|job| (job.title.clone(), job.snapshot()));
        let message = self.message.clone();
        let usage = self.usage;
        let in_trash = self.location == Location::Trash;
        let in_archive = self.in_archive();
        let query = self.search.trim().to_owned();
        let deep = self.deep.as_ref().map(|deep| (deep.done, deep.truncated));
        let single = (selected == 1)
            .then(|| {
                self.entries
                    .iter()
                    .find(|entry| self.selected.contains(&entry.path))
            })
            .flatten()
            .cloned();
        let paths = self.selection();
        let ctx = ui.ctx().clone();
        egui::Panel::bottom(ui.scope_id().with("etat"))
            .resizable(false)
            .exact_size(STATUS_HEIGHT)
            .frame(
                Frame::new()
                    .fill(look.preview)
                    .inner_margin(Margin::symmetric(18, 0)),
            )
            .show(ui, |ui| {
                let full = ui.max_rect().expand2(egui::vec2(18.0, 0.0));
                ui.painter()
                    .hline(full.x_range(), full.top() + 0.5, Stroke::new(1.0, look.line));
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.visuals_mut().override_text_color = Some(look.fg2);
                    ui.style_mut().override_font_id = Some(font(12.0, false));
                    if let Some((title, progress)) = &job {
                        let fraction = if progress.total_bytes > 0 {
                            progress.done_bytes as f32 / progress.total_bytes as f32
                        } else {
                            0.0
                        };
                        ui.label(format!("{title} : {}", progress.current));
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .desired_width(140.0)
                                .desired_height(5.0),
                        );
                        if ui.small_button("Annuler").clicked() {
                            actions.push(Action::CancelJob(0));
                        }
                        return;
                    }
                    let mut room = ui.available_width() - 240.0;
                    if let Some(entry) = &single {
                        // Icône (ou vignette), nom, puis nature · taille · date.
                        let (art, _) = ui.allocate_exact_size(egui::Vec2::splat(32.0), Sense::hover());
                        match self.thumbs.get(&ctx, &entry.path) {
                            Some(texture) => paint_cover(ui, &texture, art, 6.0),
                            None => self.icons.paint(ui, entry_icon(entry), art, IconStyle::Color),
                        }
                        ui.add_space(6.0);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(room.max(80.0), 36.0), Sense::hover());
                        text(
                            ui,
                            &entry.name,
                            egui::pos2(rect.left(), rect.top() + 9.0),
                            egui::Align2::LEFT_CENTER,
                            font(13.0, true),
                            look.fg,
                            rect.width(),
                        );
                        let mut facts = vec![kind_label(entry)];
                        if let Some(size) = self.cell_detail(entry) {
                            facts.push(size);
                        }
                        let date = format_time(entry.modified);
                        if !date.is_empty() {
                            facts.push(format!("modifié {}", date.to_lowercase()));
                        }
                        text(
                            ui,
                            &facts.join(" · "),
                            egui::pos2(rect.left(), rect.bottom() - 8.0),
                            egui::Align2::LEFT_CENTER,
                            font(11.0, false),
                            look.fg3,
                            rect.width(),
                        );
                        room = 0.0;
                    } else {
                        let mut summary = format!("{visible} élément{}", plural(visible));
                        if selected > 0 {
                            summary.push_str(&format!(" · {selected} sélectionné{}", plural(selected)));
                            if let Some(detail) = &detail {
                                summary.push_str(&format!(" ({detail})"));
                            }
                        }
                        if !query.is_empty() {
                            summary.push_str(&format!(" · recherche « {query} »"));
                        }
                        summary.push_str(match deep {
                            Some((false, _)) => " · sous-dossiers en cours…",
                            Some((true, true)) => " · sous-dossiers : 2 000 premiers résultats",
                            Some((true, false)) => " · sous-dossiers compris",
                            None => "",
                        });
                        ui.label(summary);
                    }
                    if let Some((text, error)) = &message
                        && room > 0.0
                    {
                        ui.label("·");
                        if *error {
                            ui.colored_label(style::error(), text);
                        } else {
                            ui.label(text);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if let Some(entry) = &single
                            && !in_trash
                        {
                            let open = egui::Button::new(
                                RichText::new("Ouvrir")
                                    .font(font(13.0, true))
                                    .color(style::accent_text()),
                            )
                            // Texte noir ou blanc selon la clarté de l'accent (clair en thème sombre).
                            .fill(style::accent())
                            .corner_radius(9)
                            .min_size(egui::vec2(88.0, 32.0));
                            if ui.add(open).clicked() {
                                actions.push(Action::Open(entry.path.clone(), entry.directory));
                            }
                        }
                        if selected > 0 && !in_trash && !in_archive {
                            let share = egui::Button::new(
                                RichText::new("Partager").font(font(13.0, false)).color(look.fg),
                            )
                            .fill(look.chip)
                            .corner_radius(9)
                            .min_size(egui::vec2(88.0, 32.0));
                            if ui
                                .add(share)
                                .on_hover_text("Copie l’adresse, à coller dans un message")
                                .clicked()
                            {
                                actions.push(Action::Share(paths.clone()));
                            }
                        } else if in_trash {
                            if ui.small_button("Vider la Corbeille").clicked() {
                                actions.push(Action::AskEmptyTrash);
                            }
                        } else if selected == 0
                            && let Some(usage) = usage
                        {
                            let (bar, _) = ui.allocate_exact_size(egui::vec2(90.0, 4.0), Sense::hover());
                            usage_bar(ui.painter(), bar, used_part(usage), look);
                            ui.label(usage_text(usage));
                        }
                    });
                });
            });
    }

    // ----- Contenu -----

    fn empty(&mut self, ui: &mut egui::Ui, look: &Look) {
        let searching = !self.search.trim().is_empty();
        let (title, hint) = if self.denied {
            (
                "Dossier réservé à l’administrateur",
                "Son contenu n’est lisible qu’avec le mot de passe administrateur.",
            )
        } else if searching && self.deep.as_ref().is_some_and(|deep| !deep.done) {
            ("Recherche en cours…", "Les sous-dossiers sont parcourus.")
        } else if searching {
            ("Aucun résultat", "Essayez un autre nom.")
        } else if matches!(self.location, Location::Analysis { .. })
            && self.scan.as_ref().is_some_and(|scan| !scan.progress().done)
        {
            (
                "Analyse en cours…",
                "Les tailles apparaissent à mesure qu’elles sont comptées.",
            )
        } else if matches!(self.location, Location::Analysis { .. }) {
            ("Rien à montrer ici", "Ce dossier ne contient aucun fichier.")
        } else if self.location == Location::Trash {
            ("La Corbeille est vide", "Les éléments supprimés arrivent ici.")
        } else if self.location == Location::Recent {
            (
                "Aucun fichier récent",
                "Les fichiers ouverts récemment apparaissent ici.",
            )
        } else if matches!(self.location, Location::Tag(_)) {
            (
                "Aucun fichier étiqueté",
                "Ajoutez cette étiquette depuis le panneau d’aperçu.",
            )
        } else {
            (
                "Ce dossier est vide",
                "Glissez des fichiers ici pour les ajouter.",
            )
        };
        let center = ui.max_rect().center();
        let icon = egui::Rect::from_center_size(center - egui::vec2(0.0, 40.0), egui::Vec2::splat(96.0));
        self.icons.paint(ui, "places-folder", icon, IconStyle::Color);
        // Voile du fond par-dessus : l'icône paraît estompée (opacité 0,55).
        ui.painter().rect_filled(icon, 0.0, look.win.gamma_multiply(0.45));
        ui.painter().text(
            egui::pos2(center.x, icon.bottom() + 12.0),
            egui::Align2::CENTER_TOP,
            title,
            font(17.0, true),
            look.fg,
        );
        ui.painter().text(
            egui::pos2(center.x, icon.bottom() + 40.0),
            egui::Align2::CENTER_TOP,
            hint,
            font(14.0, false),
            look.fg2,
        );
        if self.denied {
            let button = egui::Rect::from_center_size(
                egui::pos2(center.x, icon.bottom() + 92.0),
                egui::vec2(230.0, 36.0),
            );
            let clicked = ui
                .put(
                    button,
                    egui::Button::new(
                        egui::RichText::new("Ouvrir en administrateur")
                            .font(font(14.0, true))
                            .color(egui::Color32::WHITE),
                    )
                    .fill(look.accent)
                    .corner_radius(9.0),
                )
                .clicked();
            if clicked {
                self.open_as_admin();
            }
        }
    }

    /// Grille ; rend la zone visible des éléments et son défilement.
    fn grid(
        &mut self,
        ui: &mut egui::Ui,
        look: &Look,
        actions: &mut Vec<Action>,
        targets: &mut Targets,
    ) -> (egui::Rect, egui::Vec2) {
        let visible = self.visible();
        if visible.is_empty() {
            self.empty(ui, look);
        }
        let inner = (ui.available_width() - 44.0).max(CELL_MIN);
        let columns = (((inner + CELL_GAP) / (CELL_MIN + CELL_GAP)).floor() as usize).max(1);
        let cell_width = (inner - CELL_GAP * (columns - 1) as f32) / columns as f32;
        let ctx = ui.ctx().clone();
        let scrolled = egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            self.lasso_autoscroll(ui);
            // Sous les récents, leur titre « Tous les fichiers » sert déjà d'espace.
            ui.add_space(if self.recent_strip.is_empty() { 12.0 } else { 4.0 });
            ui.spacing_mut().item_spacing = egui::vec2(CELL_GAP, CELL_GAP);
            for row in visible.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.add_space(22.0 - CELL_GAP);
                    for &index in row {
                        let entry = self.entries[index].clone();
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(cell_width, CELL_HEIGHT),
                            Sense::click_and_drag(),
                        );
                        self.item_rects.push((rect, entry.path.clone()));
                        let selected = self.selected.contains(&entry.path);
                        if selected {
                            ui.painter().rect_filled(rect, 14.0, look.sel_bg);
                            ui.painter().rect_stroke(
                                rect,
                                14.0,
                                Stroke::new(2.0, look.accent),
                                egui::StrokeKind::Inside,
                            );
                        } else if response.hovered() && self.drag.is_none() {
                            ui.painter().rect_filled(rect, 14.0, style::tint(6));
                        }
                        if entry.directory && !self.in_archive() {
                            targets.push((rect, Target::Folder(entry.path.clone())));
                        }
                        // Image : sa vignette ; sinon l'icône de 72.
                        let art = egui::Rect::from_center_size(
                            egui::pos2(rect.center().x, rect.top() + 10.0 + 38.0),
                            egui::vec2(96.0, 76.0),
                        );
                        match self.thumbs.get(&ctx, &entry.path) {
                            Some(texture) => {
                                paint_thumbnail(ui, &texture, art.shrink2(egui::vec2(0.0, 8.0)), 8.0)
                            }
                            None => self.icons.paint(
                                ui,
                                entry_icon(&entry),
                                egui::Rect::from_center_size(art.center(), egui::Vec2::splat(72.0)),
                                IconStyle::Color,
                            ),
                        }
                        let label_center = egui::pos2(rect.center().x, rect.bottom() - 28.0);
                        let mut child = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(egui::Rect::from_center_size(
                                    label_center,
                                    egui::vec2(cell_width - 8.0, 22.0),
                                ))
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        if !self.rename_field(&mut child, &entry.path, cell_width - 12.0, actions) {
                            let name_font = font(13.0, selected);
                            let ink = if selected { Color32::WHITE } else { look.fg };
                            let galley_width = ui
                                .painter()
                                .layout_no_wrap(entry.name.clone(), name_font.clone(), ink)
                                .size()
                                .x
                                .min(112.0 - 14.0);
                            if selected {
                                let pill = egui::Rect::from_center_size(
                                    label_center,
                                    egui::vec2(galley_width + 14.0, 20.0),
                                );
                                ui.painter().rect_filled(pill, 7.0, look.accent);
                            }
                            text(
                                ui,
                                &entry.name,
                                label_center,
                                egui::Align2::CENTER_CENTER,
                                name_font,
                                ink,
                                112.0 - 14.0,
                            );
                            // Sous le nom : taille du fichier ou contenu du dossier, ou le
                            // sous-dossier où la recherche l'a trouvé.
                            let detail = self
                                .found_in(&entry)
                                .map(|place| format!("dans {place}"))
                                .or_else(|| self.cell_detail(&entry));
                            if let Some(detail) = detail {
                                text(
                                    ui,
                                    &detail,
                                    egui::pos2(rect.center().x, rect.bottom() - 10.0),
                                    egui::Align2::CENTER_CENTER,
                                    font(11.0, false),
                                    if selected { look.sel_fg } else { look.fg3 },
                                    cell_width - 12.0,
                                );
                            }
                            // Étiquettes : un point de couleur chacune, en haut à droite.
                            for (slot, tag) in self.tags.of(&entry.path).iter().enumerate() {
                                ui.painter().circle_filled(
                                    egui::pos2(rect.right() - 14.0 - slot as f32 * 12.0, rect.top() + 14.0),
                                    4.5,
                                    tag_color(&self.tags, tag),
                                );
                            }
                        }
                        let response = response.on_hover_text_at_pointer(&entry.name);
                        self.item_response(&response, index, ui, actions);
                        response.context_menu(|ui| self.item_menu(ui, &entry, actions));
                    }
                });
            }
            let rest = ui.allocate_response(ui.available_size().max(egui::vec2(inner, 80.0)), Sense::click());
            if rest.clicked() {
                actions.push(Action::ClearSelection);
            }
            rest.context_menu(|ui| self.folder_menu(ui, actions));
        });
        // Le dossier affiché reçoit ce qu'on lâche dans le vide.
        if let Some(dir) = self.current_dir() {
            targets.insert(0, (ui.max_rect(), Target::Folder(dir.to_path_buf())));
        }
        (scrolled.inner_rect, scrolled.state.unclamped_offset())
    }

    /// Liste ; rend la zone visible des lignes et son défilement.
    fn list(
        &mut self,
        ui: &mut egui::Ui,
        look: &Look,
        actions: &mut Vec<Action>,
        targets: &mut Targets,
    ) -> (egui::Rect, egui::Vec2) {
        let visible = self.visible();
        let in_trash = self.location == Location::Trash;
        let area = ui.max_rect().shrink2(egui::vec2(14.0, 0.0));
        let width = area.width();
        let name_width = (width - 24.0 - LIST_COLUMNS.iter().sum::<f32>() - 3.0 * 12.0).max(80.0);
        let x = |column: usize| {
            area.left()
                + 12.0
                + name_width
                + 12.0
                + LIST_COLUMNS[..column].iter().map(|w| w + 12.0).sum::<f32>()
        };
        // En-têtes : un clic trie.
        ui.add_space(8.0);
        let (header, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), Sense::hover());
        ui.painter()
            .hline(area.x_range(), header.bottom() - 0.5, Stroke::new(1.0, look.line));
        let largest = matches!(self.location, Location::Analysis { largest: true, .. });
        let third = if in_trash {
            "Supprimé le"
        } else if largest {
            "Dossier"
        } else {
            "Modifié"
        };
        let analysis_total = self.analysis_total();
        let scan_root = self.scan.as_ref().map(|scan| scan.root.clone());
        let analyzing = analysis_total.is_some();
        let fourth = if analyzing { "Part" } else { "Type" };
        for (index, (title, key, left)) in [
            ("Nom", SortKey::Name, area.left() + 12.0),
            ("Taille", SortKey::Size, x(0)),
            (third, SortKey::Modified, x(1)),
            (fourth, SortKey::Kind, x(2)),
        ]
        .into_iter()
        .enumerate()
        {
            let width = if index == 0 {
                name_width
            } else {
                LIST_COLUMNS[index - 1]
            };
            let rect =
                egui::Rect::from_min_size(egui::pos2(left, header.top()), egui::vec2(width, header.height()));
            let response = ui.interact(rect, ui.scope_id().with(("tri", index)), Sense::click());
            // L'analyse est toujours triée du plus lourd au plus léger.
            let arrow = match (self.sort == key, self.ascending) {
                _ if analyzing => "",
                (true, true) => " ↑",
                (true, false) => " ↓",
                _ => "",
            };
            ui.painter().text(
                egui::pos2(rect.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{title}{arrow}"),
                font(12.0, true),
                if self.sort == key { look.fg2 } else { look.fg3 },
            );
            if !analyzing && response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                actions.push(Action::Sort(key));
            }
        }
        if visible.is_empty() {
            self.empty(ui, look);
        }
        let scrolled = egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            self.lasso_autoscroll(ui);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(2.0);
            for &index in &visible {
                let entry = self.entries[index].clone();
                let (row, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), LIST_ROW),
                    Sense::click_and_drag(),
                );
                let rect = egui::Rect::from_x_y_ranges(area.x_range(), row.y_range());
                self.item_rects.push((rect, entry.path.clone()));
                let selected = self.selected.contains(&entry.path);
                if selected {
                    ui.painter().rect_filled(rect, 9.0, look.sel_bg);
                } else if response.hovered() && self.drag.is_none() {
                    ui.painter().rect_filled(rect, 9.0, style::tint(6));
                }
                if entry.directory && !self.in_archive() {
                    targets.push((rect, Target::Folder(entry.path.clone())));
                }
                let (ink, sub) = if selected {
                    (look.sel_fg, look.sel_fg)
                } else {
                    (look.fg, look.fg2)
                };
                let icon = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 24.0, rect.center().y),
                    egui::Vec2::splat(24.0),
                );
                self.icons.paint(ui, entry_icon(&entry), icon, IconStyle::Color);
                let name_left = icon.right() + 10.0;
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(egui::Rect::from_min_size(
                            egui::pos2(name_left, rect.top() + 6.0),
                            egui::vec2(name_width - 40.0, LIST_ROW - 12.0),
                        ))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                if !self.rename_field(&mut child, &entry.path, name_width - 44.0, actions) {
                    let name = if entry.symlink {
                        format!("{} (lien)", entry.name)
                    } else {
                        entry.name.clone()
                    };
                    text(
                        ui,
                        &name,
                        egui::pos2(name_left, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        font(14.0, selected),
                        ink,
                        name_width - 40.0,
                    );
                }
                let size = match (entry.directory, entry.size) {
                    (true, Some(size)) if analyzing => format_size(size),
                    (true, None) if analyzing => "Calcul…".to_owned(),
                    (true, _) => "—".to_owned(),
                    (false, Some(size)) => format_size(size),
                    (false, None) => String::new(),
                };
                let date = match &entry.trash {
                    Some(item) => item.deleted.replace('T', " à ").chars().take(18).collect(),
                    // Plus gros fichiers : où il se trouve, depuis le dossier analysé.
                    None if largest => entry
                        .path
                        .parent()
                        .zip(scan_root.as_deref())
                        .and_then(|(parent, root)| parent.strip_prefix(root).ok())
                        .map(|relative| relative.display().to_string())
                        .filter(|relative| !relative.is_empty())
                        .unwrap_or_else(|| "—".into()),
                    None => format_time(entry.modified),
                };
                // Analyse : la part du dossier analysé, en barre et en pourcentage.
                let fourth = match analysis_total {
                    Some(total) => {
                        let part = match entry.size {
                            Some(size) if total > 0 => size as f32 / total as f32,
                            _ => 0.0,
                        };
                        let bar = egui::Rect::from_min_size(
                            egui::pos2(x(2), rect.center().y - 3.0),
                            egui::vec2(LIST_COLUMNS[2] - 52.0, 6.0),
                        );
                        usage_bar(ui.painter(), bar, part, look);
                        if entry.size.is_some() {
                            format!("{:>3} %", (part * 100.0).round() as u32)
                        } else {
                            String::new()
                        }
                    }
                    // Trouvé plus bas par la recherche : le sous-dossier plutôt que la nature.
                    None => self
                        .found_in(&entry)
                        .map_or_else(|| kind_label(&entry), |place| format!("dans {place}")),
                };
                for (column, value) in [size, date, fourth].into_iter().enumerate() {
                    let left = if column == 2 && analyzing {
                        x(2) + LIST_COLUMNS[2] - 46.0
                    } else {
                        x(column)
                    };
                    text(
                        ui,
                        &value,
                        egui::pos2(left, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        font(14.0, column == 0 && analyzing),
                        sub,
                        LIST_COLUMNS[column] - 6.0,
                    );
                }
                let response = response.on_hover_text_at_pointer(match &entry.trash {
                    Some(item) => format!("Origine : {}", item.original.display()),
                    None if largest || self.found_in(&entry).is_some() => {
                        format!("{} — dans {}", entry.name, display_location(&entry.path))
                    }
                    None => entry.name.clone(),
                });
                self.item_response(&response, index, ui, actions);
                response.context_menu(|ui| self.item_menu(ui, &entry, actions));
            }
            let rest = ui.allocate_response(ui.available_size().max(egui::vec2(width, 80.0)), Sense::click());
            if rest.clicked() {
                actions.push(Action::ClearSelection);
            }
            rest.context_menu(|ui| self.folder_menu(ui, actions));
        });
        if let Some(dir) = self.current_dir() {
            targets.insert(0, (ui.max_rect(), Target::Folder(dir.to_path_buf())));
        }
        (scrolled.inner_rect, scrolled.state.unclamped_offset())
    }

    // ----- Panneau d'aperçu -----

    fn preview_panel(&mut self, ui: &mut egui::Ui, look: &Look, actions: &mut Vec<Action>) {
        let selection: Vec<FileEntry> = self
            .entries
            .iter()
            .filter(|entry| self.selected.contains(&entry.path))
            .cloned()
            .collect();
        let ctx = ui.ctx().clone();
        egui::Panel::right(ui.scope_id().with("apercu"))
            .resizable(false)
            .exact_size(PREVIEW_WIDTH)
            .frame(
                Frame::new()
                    .fill(look.preview)
                    .inner_margin(Margin::symmetric(22, 24)),
            )
            .show(ui, |ui| {
                let full = ui.max_rect().expand2(egui::vec2(22.0, 24.0));
                ui.painter()
                    .vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0, look.line));
                ui.spacing_mut().item_spacing.y = 18.0;
                let width = ui.available_width();

                // Scène : vignette (recadrée) ou grande icône.
                let (stage, _) = ui.allocate_exact_size(egui::vec2(width, 170.0), Sense::hover());
                ui.painter().rect_filled(stage, 16.0, look.stage);
                let single = (selection.len() == 1).then(|| selection[0].clone());
                let (icon, name, detail): (&'static str, String, String) = match (&single, selection.len()) {
                    (Some(entry), _) => {
                        let size = if entry.directory {
                            let count = fs::read_dir(&entry.path).map(|dir| dir.count()).unwrap_or(0);
                            format!("{count} élément{}", plural(count))
                        } else {
                            entry.size.map(format_size).unwrap_or_default()
                        };
                        (
                            entry_icon(entry),
                            entry.name.clone(),
                            format!("{} · {size}", kind_label(entry)),
                        )
                    }
                    (None, 0) => {
                        let count = self.visible().len();
                        let (icon, name) = match &self.location {
                            Location::Folder(dir) => (
                                folder_icon(dir, &home()),
                                if *dir == home() {
                                    "Dossier perso".into()
                                } else {
                                    file_name(dir)
                                },
                            ),
                            Location::Archive { archive, directory } => (
                                "places-folder",
                                if directory.is_empty() {
                                    file_name(archive)
                                } else {
                                    directory.rsplit('/').next().unwrap_or_default().to_owned()
                                },
                            ),
                            Location::Trash => ("places-trash", "Corbeille".into()),
                            Location::Recent => ("app-clock", "Récents".into()),
                            Location::Tag(tag) => ("places-folder", tag.clone()),
                            Location::Analysis { dir, .. } => ("status-storage", file_name(dir)),
                        };
                        (icon, name, format!("Dossier · {count} élément{}", plural(count)))
                    }
                    (None, count) => {
                        let total: u64 = selection.iter().filter_map(|entry| entry.size).sum();
                        (
                            "file-generic",
                            format!("{count} éléments"),
                            format!("Sélection · {}", format_size(total)),
                        )
                    }
                };
                match single
                    .as_ref()
                    .and_then(|entry| self.thumbs.get(&ctx, &entry.path))
                {
                    Some(texture) => paint_cover(ui, &texture, stage, 16.0),
                    None => self.icons.paint(
                        ui,
                        icon,
                        egui::Rect::from_center_size(stage.center(), egui::Vec2::splat(112.0)),
                        IconStyle::Color,
                    ),
                }

                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.add(
                        egui::Label::new(RichText::new(&name).font(font(17.0, true)).color(look.fg)).wrap(),
                    );
                    ui.label(RichText::new(detail).font(font(13.0, false)).color(look.fg2));
                });

                // Ouvrir, Partager, autres actions.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let flex = (width - 36.0 - 16.0) / 2.0;
                    let open = egui::Button::new(
                        RichText::new("Ouvrir")
                            .font(font(13.0, true))
                            .color(Color32::WHITE),
                    )
                    .fill(look.accent)
                    .corner_radius(10)
                    .min_size(egui::vec2(flex, 36.0));
                    if ui.add_enabled(single.is_some(), open).clicked()
                        && let Some(entry) = &single
                    {
                        actions.push(Action::Open(entry.path.clone(), entry.directory));
                    }
                    let share =
                        egui::Button::new(RichText::new("Partager").font(font(13.0, false)).color(look.fg))
                            .fill(look.chip)
                            .corner_radius(10)
                            .min_size(egui::vec2(flex, 36.0));
                    if ui
                        .add_enabled(!selection.is_empty() && !self.in_archive(), share)
                        .on_hover_text("Copie l’adresse du fichier, à coller dans un message")
                        .clicked()
                    {
                        actions.push(Action::Share(
                            selection.iter().map(|entry| entry.path.clone()).collect(),
                        ));
                    }
                    let more = icon_button(
                        ui,
                        egui::vec2(36.0, 36.0),
                        10,
                        Some(look.chip),
                        "Plus d’actions",
                        |p, r| {
                            for dx in [-4.5, 0.0, 4.5] {
                                p.circle_filled(r.center() + egui::vec2(dx, 0.0), 1.4, look.icon);
                            }
                        },
                    );
                    if let Some(entry) = &single
                        && !self.in_archive()
                    {
                        egui::Popup::menu(&more).show(|ui| self.item_menu(ui, entry, actions));
                    }
                });

                // Informations.
                let rows: Vec<(&str, String)> = match &single {
                    Some(entry) if entry.directory => {
                        let count = fs::read_dir(&entry.path).map(|dir| dir.count()).unwrap_or(0);
                        vec![
                            ("Contenu", format!("{count} élément{}", plural(count))),
                            ("Emplacement", display_location(&entry.path)),
                            ("Partage", "Privé".into()),
                        ]
                    }
                    Some(entry) => {
                        let created = entry.path.metadata().ok().and_then(|m| m.created().ok());
                        vec![
                            ("Modifié", format_date(entry.modified)),
                            ("Créé", format_date(created)),
                            ("Emplacement", display_location(&entry.path)),
                            ("Ouvrir avec", opener_label(entry).into()),
                        ]
                    }
                    None => match self.current_dir() {
                        Some(dir) => vec![
                            (
                                "Contenu",
                                format!("{} élément{}", self.visible().len(), plural(self.visible().len())),
                            ),
                            ("Emplacement", display_location(&dir.join("x"))),
                            ("Partage", "Privé".into()),
                        ],
                        None => Vec::new(),
                    },
                };
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let line = Stroke::new(1.0, look.line);
                    let top = ui.cursor().top();
                    ui.painter().hline(ui.max_rect().x_range(), top, line);
                    for (label, value) in rows {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 38.0), Sense::hover());
                        ui.painter().text(
                            egui::pos2(rect.left(), rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            label,
                            font(13.0, false),
                            look.fg3,
                        );
                        text(
                            ui,
                            &value,
                            egui::pos2(rect.right(), rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            font(13.0, false),
                            look.fg,
                            width - 100.0,
                        );
                        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, line);
                    }
                });

                // Étiquettes du fichier.
                if let Some(entry) = &single
                    && !self.in_archive()
                {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        ui.label(RichText::new("Étiquettes").font(font(12.0, true)).color(look.fg3));
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                            for tag in self.tags.of(&entry.path).to_vec() {
                                let galley =
                                    ui.painter()
                                        .layout_no_wrap(tag.clone(), font(12.0, false), look.fg);
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(galley.size().x + 34.0, 26.0),
                                    Sense::click(),
                                );
                                ui.painter().rect_filled(rect, 13.0, look.chip);
                                ui.painter().circle_filled(
                                    egui::pos2(rect.left() + 14.0, rect.center().y),
                                    4.0,
                                    tag_color(&self.tags, &tag),
                                );
                                ui.painter().galley(
                                    egui::pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0),
                                    galley,
                                    look.fg,
                                );
                                if response.on_hover_text("Retirer l’étiquette").clicked() {
                                    actions.push(Action::ToggleTag(entry.path.clone(), tag));
                                }
                            }
                            let galley =
                                ui.painter()
                                    .layout_no_wrap("+ Ajouter".into(), font(12.0, false), look.fg2);
                            let (rect, add) = ui.allocate_exact_size(
                                egui::vec2(galley.size().x + 20.0, 26.0),
                                Sense::click(),
                            );
                            dashed_pill(ui.painter(), rect, look.fg3);
                            ui.painter()
                                .galley(rect.center() - galley.size() / 2.0, galley, look.fg2);
                            egui::Popup::menu(&add).show(|ui| {
                                style::menu_style(ui);
                                self.tag_choices(ui, &entry.path, actions);
                            });
                        });
                    });
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(
                        RichText::new("Espace : aperçu rapide · Entrée : renommer")
                            .font(font(12.0, false))
                            .color(look.fg3),
                    );
                });
            });
    }

    // ----- Aperçu rapide (Espace) -----

    fn quick_look(&mut self, ctx: &egui::Context, look: &Look, actions: &mut Vec<Action>) {
        let Some(path) = self.quick.clone() else { return };
        let Some(entry) = self.entries.iter().find(|entry| entry.path == path).cloned() else {
            self.quick = None;
            return;
        };
        let modal = egui::Modal::new(egui::Id::unique("fichiers-apercu-rapide"))
            .frame(
                Frame::popup(&ctx.global_style())
                    .fill(look.win)
                    .corner_radius(16)
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                let screen = ctx.content_rect();
                let width = (screen.width() * 0.7).clamp(360.0, 900.0);
                let height = (screen.height() * 0.62).clamp(240.0, 640.0);
                ui.set_width(width);
                let (stage, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
                ui.painter().rect_filled(stage, 12.0, look.stage);
                if let Some(texture) = self.thumbs.get(ctx, &entry.path) {
                    paint_contained(ui, &texture, stage.shrink(12.0));
                } else if !entry.directory && is_text_file(&entry.path) {
                    let content: String = fs::read(&entry.path)
                        .map(|bytes| {
                            String::from_utf8_lossy(&bytes[..bytes.len().min(16 * 1024)]).into_owned()
                        })
                        .unwrap_or_default();
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(stage.shrink(16.0)));
                    egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(&mut child, |ui| {
                            ui.label(RichText::new(content).monospace().size(13.0).color(look.fg));
                        });
                } else {
                    self.icons.paint(
                        ui,
                        entry_icon(&entry),
                        egui::Rect::from_center_size(stage.center(), egui::Vec2::splat(160.0)),
                        IconStyle::Color,
                    );
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&entry.name).font(font(17.0, true)).color(look.fg));
                        let size = entry.size.map(format_size).unwrap_or_default();
                        ui.label(
                            RichText::new(format!("{} · {size}", kind_label(&entry)))
                                .font(font(13.0, false))
                                .color(look.fg2),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let open = egui::Button::new(
                            RichText::new("Ouvrir")
                                .font(font(13.0, true))
                                .color(Color32::WHITE),
                        )
                        .fill(look.accent)
                        .corner_radius(10)
                        .min_size(egui::vec2(96.0, 36.0));
                        if ui.add(open).clicked() {
                            actions.push(Action::Open(entry.path.clone(), entry.directory));
                            actions.push(Action::QuickLook);
                        }
                    });
                });
            });
        let close = ctx.input(|input| input.key_pressed(Key::Space) || input.key_pressed(Key::Escape));
        if modal.should_close() || close {
            self.quick = None;
        }
    }

    /// Ligne sous le nom d'une case : « 240 Ko », « 12 éléments ».
    fn cell_detail(&self, entry: &FileEntry) -> Option<String> {
        if entry.directory {
            let count = *self.child_counts.get(&entry.path)?;
            Some(if count == 0 {
                "Vide".to_owned()
            } else {
                format!("{count} élément{}", plural(count))
            })
        } else {
            entry.size.map(format_size)
        }
    }

    /// Détail d'une sélection de plusieurs éléments : dossiers, fichiers et leur taille
    /// (« 8 dossiers, 7 fichiers · 916,8 Ko »).
    fn selection_detail(&self) -> Option<String> {
        if self.selected.len() < 2 {
            return None;
        }
        let chosen = self
            .entries
            .iter()
            .filter(|entry| self.selected.contains(&entry.path));
        let (mut folders, mut files, mut bytes) = (0usize, 0usize, 0u64);
        let analyzing = matches!(self.location, Location::Analysis { .. });
        for entry in chosen {
            if entry.directory {
                folders += 1;
                if analyzing {
                    bytes += entry.size.unwrap_or(0);
                }
            } else {
                files += 1;
                bytes += entry.size.unwrap_or(0);
            }
        }
        let mut parts = Vec::new();
        if folders > 0 {
            parts.push(format!("{folders} dossier{}", plural(folders)));
        }
        if files > 0 {
            parts.push(format!("{files} fichier{}", plural(files)));
        }
        let mut detail = parts.join(", ");
        if files > 0 || (analyzing && bytes > 0) {
            detail.push_str(&format!(" · {}", format_size(bytes)));
        }
        Some(detail)
    }

    // ----- Propriétés -----

    /// Propriétés d'un élément (planche « Dialogues de détails ») : nom modifiable, type,
    /// emplacement, taille et date, droits réglables appliqués tout de suite.
    pub(super) fn properties_dialog(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let in_trash = self.location == Location::Trash;
        let Some(Dialog::Properties(properties)) = &mut self.dialog else {
            return;
        };
        let (icons, thumbs) = (&mut self.icons, &mut self.thumbs);
        let directory = properties.path.is_dir();
        let size = match properties.size {
            Some((bytes, files)) if directory => format!(
                "{} · {files} élément{}",
                format_size(bytes),
                plural(files as usize)
            ),
            Some((bytes, _)) => format_size(bytes),
            None => "Calcul en cours…".to_owned(),
        };
        let on_disk = properties
            .on_disk
            .map(|bytes| format!("Sur le disque : {}", format_size(bytes)));
        let parent = properties
            .path
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default();
        let opener = (properties.opener != "—").then(|| format!("Ouvert avec {}", properties.opener));
        let me = env::var("USER").ok();
        let owner = if me.as_deref() == Some(properties.owner.as_str()) {
            format!("vous ({})", properties.owner)
        } else {
            properties.owner.clone()
        };
        let buttons: &[&str] = if in_trash { &["OK"] } else { &["Ouvrir", "OK"] };
        let mut outcome = PropertiesOutcome::default();
        let escape = details::show(ctx, "fichiers-proprietes", |ui, t| {
            let subtitle = format!("{} · {size}", properties.kind);
            outcome.close |= details::header(
                ui,
                t,
                64.0,
                77.0,
                |ui, rect| match thumbs.get(ui.ctx(), &properties.path) {
                    Some(texture) => paint_thumbnail(ui, &texture, rect, 8.0),
                    None => icons.paint(ui, properties.icon, rect, IconStyle::Color),
                },
                |ui| {
                    let width = ui.available_width();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 77.0), Sense::hover());
                    details::text(
                        ui,
                        rect.left_top(),
                        egui::Align2::LEFT_TOP,
                        "Nom",
                        details::font(12.0, false),
                        t.fg3,
                        width,
                    );
                    let field =
                        egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 19.0), egui::vec2(width, 36.0));
                    ui.painter().rect_filled(field, 9.0, t.field);
                    ui.painter().rect_stroke(
                        field,
                        9.0,
                        Stroke::new(1.0, t.field_ring),
                        egui::StrokeKind::Inside,
                    );
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(field.shrink2(egui::vec2(12.0, 0.0)))
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    let response = child.add_enabled(
                        !in_trash,
                        egui::TextEdit::singleline(&mut properties.name)
                            .font(details::font(16.0, true))
                            .text_color(t.fg)
                            .frame(Frame::NONE)
                            .min_size(egui::vec2(0.0, field.height()))
                            .align(egui::Align2::LEFT_CENTER)
                            .desired_width(field.width() - 24.0),
                    );
                    if response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                        outcome.rename = true;
                    }
                    details::text(
                        ui,
                        egui::pos2(rect.left(), field.bottom() + 6.0),
                        egui::Align2::LEFT_TOP,
                        &subtitle,
                        details::font(13.0, false),
                        t.fg2,
                        width,
                    );
                },
            );
            details::section(ui, t, 0.0, "Général", None, |rows| {
                let change = (!in_trash && !directory).then_some("Changer…");
                outcome.change_opener =
                    rows.value("Type", &properties.kind, false, opener.as_deref(), change);
                let reveal = (!in_trash).then_some("Afficher");
                outcome.reveal = rows.value("Emplacement", &parent, true, None, reveal);
                rows.value("Taille", &size, false, on_disk.as_deref(), None);
                let (when, date) = (format_time(properties.modified), format_date(properties.modified));
                let date = (date != when).then_some(date.as_str());
                rows.value("Modifié", &when, false, date, None);
            });
            let mode = mode_text(properties.mode);
            details::section(ui, t, 18.0, "Autorisations", Some(&mode), |rows| {
                let group = properties.group.clone();
                for (who, sub, shift) in [
                    ("Propriétaire", owner.as_str(), 6),
                    ("Groupe", group.as_str(), 3),
                    ("Autres", "tous les utilisateurs", 0),
                ] {
                    let mut level = access_level(properties.mode, shift);
                    if rows.choice(who, sub, &["Aucun", "Lecture", "Lecture et écriture"], &mut level) {
                        outcome.mode = Some(with_access(properties.mode, shift, level));
                    }
                }
            });
            let footer = details::footer(
                ui,
                t,
                details::Left::Note("Les changements s’appliquent immédiatement."),
                buttons,
            );
            match footer.button {
                Some(slot) if slot == buttons.len() - 1 => {
                    outcome.rename = true;
                    outcome.close = true;
                }
                Some(_) => outcome.open = true,
                None => {}
            }
        });

        let path = properties.path.clone();
        let mut error = None;
        if let Some(mode) = outcome.mode {
            match fs::set_permissions(&path, fs::Permissions::from_mode(mode)) {
                Ok(()) => properties.mode = mode,
                Err(err) => error = Some(format!("Droits non modifiés : {err}")),
            }
        }
        let renamed = (outcome.rename && !in_trash && properties.name.trim() != file_name(&path))
            .then(|| properties.name.trim().to_owned());
        if let Some(error) = error {
            self.error(error);
        }
        if let Some(name) = renamed {
            match ops::rename(&path, &name) {
                Ok(target) => {
                    self.refresh();
                    self.selected = BTreeSet::from([target.clone()]);
                    if let Some(Dialog::Properties(properties)) = &mut self.dialog {
                        properties.path = target;
                    }
                }
                Err(error) => {
                    self.error(error);
                    outcome.close = false;
                }
            }
        }
        let path = match &self.dialog {
            Some(Dialog::Properties(properties)) => properties.path.clone(),
            _ => path,
        };
        if outcome.reveal {
            if let Some(parent) = path.parent() {
                self.go(Location::Folder(parent.to_path_buf()));
                self.selected = BTreeSet::from([path.clone()]);
            }
            self.dialog = None;
        } else if outcome.change_opener {
            actions.push(Action::OpenWith(path));
        } else if outcome.open {
            self.dialog = None;
            actions.push(Action::Open(path, directory));
        } else if outcome.close || escape {
            self.dialog = None;
        }
    }

    // ----- Sélection au rectangle -----

    /// Rectangle tiré depuis le vide de la grille ou de la liste : tout élément qu'il touche
    /// est sélectionné, en plus de la sélection d'avant si Ctrl ou Maj était tenu.
    fn lasso_frame(
        &mut self,
        ctx: &egui::Context,
        look: &Look,
        layer: egui::LayerId,
        area: egui::Rect,
        offset: egui::Vec2,
    ) {
        if self.picker.is_some() {
            return;
        }
        let (pressed, down, pointer, extend) = ctx.input(|input| {
            (
                input.pointer.primary_pressed(),
                input.pointer.primary_down(),
                input.pointer.interact_pos(),
                input.modifiers.command || input.modifiers.shift,
            )
        });
        if self.lasso.is_none()
            && pressed
            && self.drag.is_none()
            && let Some(at) = pointer
            // Pas sur la barre de défilement, un menu ou une fenêtre de dialogue.
            && area.contains(at)
            && at.x < area.right() - LASSO_SCROLLBAR
            && ctx.layer_id_at(at) == Some(layer)
            && !self.item_rects.iter().any(|(rect, _)| rect.contains(at))
        {
            self.lasso = Some(Lasso {
                origin: at + offset,
                base: if extend {
                    self.selected.clone()
                } else {
                    BTreeSet::new()
                },
            });
        }
        let Some(lasso) = &self.lasso else { return };
        let Some(at) = pointer.filter(|_| down) else {
            self.lasso = None;
            return;
        };
        let rect = egui::Rect::from_two_pos(lasso.origin - offset, at);
        let mut selected = lasso.base.clone();
        selected.extend(
            self.item_rects
                .iter()
                .filter(|(item, _)| item.intersects(rect))
                .map(|(_, path)| path.clone()),
        );
        if selected != self.selected {
            self.selected = selected;
            self.anchor = None;
        }
        let shown = rect.intersect(area);
        if shown.is_positive() {
            let painter = ctx.layer_painter(layer).with_clip_rect(area);
            painter.rect_filled(shown, 4.0, look.accent.gamma_multiply(0.14));
            painter.rect_stroke(
                shown,
                4.0,
                Stroke::new(1.0, look.accent.gamma_multiply(0.8)),
                egui::StrokeKind::Inside,
            );
        }
        ctx.request_repaint();
    }

    /// Le rectangle qui touche le haut ou le bas de la zone la fait défiler, d'autant plus
    /// vite qu'il la dépasse.
    fn lasso_autoscroll(&self, ui: &mut egui::Ui) {
        if self.lasso.is_none() {
            return;
        }
        let Some(at) = ui.ctx().input(|input| input.pointer.interact_pos()) else {
            return;
        };
        let clip = ui.clip_rect();
        let speed = if at.y < clip.top() + LASSO_EDGE {
            (clip.top() + LASSO_EDGE - at.y).min(80.0) * 0.25
        } else if at.y > clip.bottom() - LASSO_EDGE {
            -(at.y - (clip.bottom() - LASSO_EDGE)).min(80.0) * 0.25
        } else {
            0.0
        };
        if speed != 0.0 {
            ui.scroll_with_delta(egui::vec2(0.0, speed));
        }
    }

    // ----- Glisser-déposer -----

    /// Pendant un glisser : cible survolée, fantôme sous le pointeur, dépôt au relâchement.
    /// Si ferrite-shell reprend le glisser (pointeur hors de la fenêtre ou sur le Dock), la
    /// fenêtre perd le pointeur : le glisser s'achève ailleurs.
    fn drag_frame(&mut self, ctx: &egui::Context, look: &Look, targets: &Targets, actions: &mut Vec<Action>) {
        let Some(drag) = self.drag.clone() else { return };
        let (pointer, down, released, copy) = ctx.input(|input| {
            (
                input.pointer.hover_pos(),
                input.pointer.primary_down(),
                input.pointer.primary_released(),
                input.modifiers.command || input.modifiers.alt,
            )
        });
        let Some(pointer) = pointer else {
            self.drag = None;
            return;
        };
        let target = self.target_at(targets, pointer, &drag.paths);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::unique("fichiers-glisser"),
        ));
        if let Some((rect, target)) = &target {
            // Le dossier affiché lui-même n'est pas surligné : on dépose « ici ».
            let here = matches!(target, Target::Folder(dir) if Some(dir.as_path()) == self.current_dir());
            if !here {
                painter.rect_filled(*rect, 10.0, look.drop.gamma_multiply(0.55));
                painter.rect_stroke(
                    *rect,
                    10.0,
                    Stroke::new(2.0, look.accent),
                    egui::StrokeKind::Inside,
                );
            }
        }
        let icon = egui::Rect::from_center_size(pointer + egui::vec2(10.0, 14.0), egui::Vec2::splat(56.0));
        if let Some(texture) = self.icons.texture_for(ctx, drag.icon, icon.width()) {
            painter.image(
                texture.id(),
                icon,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE.gamma_multiply(0.85),
            );
        }
        if drag.paths.len() > 1 {
            let badge = egui::pos2(icon.right() - 4.0, icon.top() + 6.0);
            let label = drag.paths.len().to_string();
            let galley = painter.layout_no_wrap(label, font(12.0, true), Color32::WHITE);
            let pill =
                egui::Rect::from_center_size(badge, egui::vec2((galley.size().x + 12.0).max(22.0), 22.0));
            painter.rect_filled(pill, 11.0, style::error());
            painter.galley(pill.center() - galley.size() / 2.0, galley, Color32::WHITE);
        }
        ctx.set_cursor_icon(if copy {
            egui::CursorIcon::Copy
        } else {
            egui::CursorIcon::Grabbing
        });
        if released || !down {
            if let Some((_, target)) = target {
                actions.push(Action::Drop {
                    target,
                    paths: drag.paths,
                    copy,
                });
            }
            self.drag = None;
        } else {
            ctx.request_repaint();
        }
    }

    /// Cible sous `pointer`, la plus précise d'abord ; jamais un des éléments `paths`.
    fn target_at(
        &self,
        targets: &Targets,
        pointer: egui::Pos2,
        paths: &[PathBuf],
    ) -> Option<(egui::Rect, Target)> {
        targets
            .iter()
            .rev()
            .find(|(rect, target)| {
                rect.contains(pointer)
                    && match target {
                        Target::Folder(dir) => !paths.iter().any(|path| dir.starts_with(path)),
                        Target::Trash => self.location != Location::Trash,
                    }
            })
            .cloned()
    }

    /// Fichiers lâchés sur la fenêtre depuis une autre (le glisser était repris par
    /// ferrite-shell) : rangés comme un dépôt fait ici, sur ce qui est sous le pointeur, ou
    /// à défaut dans le dossier affiché.
    fn deposits(&mut self, ctx: &egui::Context, targets: &Targets, actions: &mut Vec<Action>) {
        if self.picker.is_some() {
            return;
        }
        let deposits = self.deposits.get_or_insert_with(|| Deposits::listen(ctx));
        let received: Vec<Deposit> = deposits.receiver.try_iter().collect();
        for deposit in received {
            let pointer = egui::pos2(deposit.x, deposit.y);
            let target = self
                .target_at(targets, pointer, &deposit.paths)
                .map(|(_, target)| target)
                .or_else(|| self.current_dir().map(|dir| Target::Folder(dir.to_path_buf())));
            if let Some(target) = target {
                actions.push(Action::Drop {
                    target,
                    paths: deposit.paths,
                    copy: deposit.copy,
                });
            }
        }
    }

    /// Début d'un glisser depuis l'élément `index` : la sélection part avec lui.
    pub(super) fn start_drag(&mut self, index: usize) {
        let entry = &self.entries[index];
        if !self.selected.contains(&entry.path) {
            self.selected = BTreeSet::from([entry.path.clone()]);
            self.anchor = Some(index);
        }
        let paths = self.selection();
        let icon = entry_icon(entry);
        // ferrite-shell reprendra le glisser hors de la fenêtre.
        ferrite_system::drag::Drag {
            icon: icon.to_owned(),
            paths: paths.clone(),
        }
        .announce();
        self.drag = Some(Dragging { paths, icon });
    }

    /// Fichiers lâchés sur `target` : déplacés (copiés avec Ctrl ou Alt, ou vers un autre
    /// disque), ou mis à la Corbeille.
    pub(super) fn drop_on(&mut self, target: Target, paths: Vec<PathBuf>, copy: bool) {
        match target {
            Target::Trash => {
                for path in &paths {
                    if let Err(error) = trash::trash(path) {
                        self.error(error);
                    }
                }
                self.refresh();
            }
            Target::Folder(dir) => {
                let paths: Vec<PathBuf> = paths
                    .into_iter()
                    .filter(|path| path.parent() != Some(dir.as_path()) && !dir.starts_with(path))
                    .collect();
                if paths.is_empty() {
                    return;
                }
                let device = |path: &Path| fs::metadata(path).map(|m| m.dev()).ok();
                let other_disk = paths.iter().any(|path| device(path) != device(&dir));
                if copy || other_disk {
                    self.run_job("Copie", move |job| ops::copy_into(&paths, &dir, job));
                } else {
                    for path in &paths {
                        if let Some(name) = path.file_name() {
                            self.tags.moved(path, &dir.join(name));
                        }
                    }
                    self.run_job("Déplacement", move |job| ops::move_into(&paths, &dir, job));
                }
            }
        }
    }
}

/// Emplacement lisible du dossier de `path` (« ~/Documents »).
fn display_location(path: &Path) -> String {
    let parent = path.parent().unwrap_or(path);
    let home = home();
    match parent.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => parent.display().to_string(),
    }
}

/// Vignette dans `rect`, entière et centrée, avec coins et ombre légère.
fn paint_thumbnail(ui: &egui::Ui, texture: &egui::TextureHandle, rect: egui::Rect, radius: f32) {
    let size = texture.size_vec2();
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let shown = egui::Rect::from_center_size(rect.center(), size * scale);
    ui.painter().add(egui::Shape::rect_filled(
        shown.translate(egui::vec2(0.0, 2.0)).expand(1.0),
        radius,
        style::tint(40),
    ));
    ui.painter().add(egui::Shape::Rect(
        egui::epaint::RectShape::filled(shown, radius, Color32::WHITE).with_texture(
            texture.id(),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        ),
    ));
}

/// Vignette qui couvre `rect` (recadrée), comme la scène du panneau d'aperçu.
fn paint_cover(ui: &egui::Ui, texture: &egui::TextureHandle, rect: egui::Rect, radius: f32) {
    let size = texture.size_vec2();
    let scale = (rect.width() / size.x).max(rect.height() / size.y);
    let shown = size * scale;
    let (u, v) = (rect.width() / shown.x, rect.height() / shown.y);
    let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(u, v));
    ui.painter().add(egui::Shape::Rect(
        egui::epaint::RectShape::filled(rect, radius, Color32::WHITE).with_texture(texture.id(), uv),
    ));
}

/// Image entière dans `rect`, centrée.
fn paint_contained(ui: &egui::Ui, texture: &egui::TextureHandle, rect: egui::Rect) {
    let size = texture.size_vec2();
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let shown = egui::Rect::from_center_size(rect.center(), size * scale);
    ui.painter().image(
        texture.id(),
        shown,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// Pilule au contour en tirets (« + Ajouter »).
fn dashed_pill(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let radius = rect.height() / 2.0;
    let mut points = Vec::new();
    let steps = 48;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        // Contour : demi-cercle droit, bas, demi-cercle gauche, haut.
        let perimeter = 2.0 * (rect.width() - 2.0 * radius) + std::f32::consts::TAU * radius;
        let d = t * perimeter;
        let straight = rect.width() - 2.0 * radius;
        let arc = std::f32::consts::PI * radius;
        let point = if d < straight {
            egui::pos2(rect.left() + radius + d, rect.top())
        } else if d < straight + arc {
            let a = -std::f32::consts::FRAC_PI_2 + (d - straight) / radius;
            egui::pos2(
                rect.right() - radius + radius * a.cos(),
                rect.center().y + radius * a.sin(),
            )
        } else if d < 2.0 * straight + arc {
            egui::pos2(rect.right() - radius - (d - straight - arc), rect.bottom())
        } else {
            let a = std::f32::consts::FRAC_PI_2 + (d - 2.0 * straight - arc) / radius;
            egui::pos2(
                rect.left() + radius + radius * a.cos(),
                rect.center().y + radius * a.sin(),
            )
        };
        points.push(point);
    }
    painter.extend(egui::Shape::dashed_line(
        &points,
        Stroke::new(1.0, color),
        4.0,
        3.0,
    ));
}
