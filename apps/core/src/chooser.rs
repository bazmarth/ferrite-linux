//! Sélecteur de fichiers Ferrite (`ferrite-apps choisir`), ouvert par le portail
//! `xdg-desktop-portal-ferrite` quand une application demande où ouvrir ou enregistrer.
//!
//! ```text
//! ferrite-apps choisir [--enregistrer | --dossier] [--multiple] [--titre T] [--bouton B]
//!                      [--nom N] [--dans DOSSIER] [--filtre "Nom|*.pdf;*.png"]… [--filtre-actuel I]
//! ```
//!
//! Le choix sort sur la sortie standard, un chemin absolu par ligne ; rien n'en sort si
//! l'on annule. Pièces et couleurs communes (`kit`, `style`), comme toute application.

use std::{
    io::Write,
    path::{Path, PathBuf},
    time::SystemTime,
};

use eframe::egui::{self, Frame, Margin, RichText, Sense};

use crate::{
    files::{format_date, format_size, paint_path_icon, path_icon},
    kit,
    style::{self, Icons},
};

/// Ce que l'application demande.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Open,
    Save,
    Folder,
}

/// Filtre proposé : son nom et ses motifs (`*.pdf`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub name: String,
    pub patterns: Vec<String>,
}

impl Filter {
    fn accepts(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        self.patterns
            .iter()
            .any(|pattern| glob(&pattern.to_lowercase(), &name))
    }
}

/// Motif simple : `*` remplace n'importe quelle suite, le reste doit correspondre.
fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((before, after)) => {
            let Some(rest) = name.strip_prefix(before) else {
                return false;
            };
            if after.is_empty() {
                return true;
            }
            (0..=rest.len())
                .filter(|&start| rest.is_char_boundary(start))
                .any(|start| glob(after, &rest[start..]))
        }
    }
}

/// Demande lue sur la ligne de commande.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub mode: Mode,
    pub multiple: bool,
    pub title: String,
    pub button: String,
    pub name: String,
    pub folder: Option<PathBuf>,
    pub filters: Vec<Filter>,
    pub filter: usize,
}

impl Request {
    pub fn parse(args: &[String]) -> Self {
        let mut request = Request {
            mode: Mode::Open,
            multiple: false,
            title: String::new(),
            button: String::new(),
            name: String::new(),
            folder: None,
            filters: Vec::new(),
            filter: 0,
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--enregistrer" => request.mode = Mode::Save,
                "--dossier" => request.mode = Mode::Folder,
                "--multiple" => request.multiple = true,
                "--titre" => request.title = args.next().cloned().unwrap_or_default(),
                "--bouton" => request.button = args.next().cloned().unwrap_or_default(),
                "--nom" => request.name = args.next().cloned().unwrap_or_default(),
                "--dans" => request.folder = args.next().map(PathBuf::from),
                "--filtre" => {
                    if let Some((name, patterns)) = args.next().and_then(|text| text.split_once('|')) {
                        request.filters.push(Filter {
                            name: name.to_owned(),
                            patterns: patterns.split(';').map(str::to_owned).collect(),
                        });
                    }
                }
                "--filtre-actuel" => {
                    request.filter = args.next().and_then(|text| text.parse().ok()).unwrap_or(0);
                }
                _ => {}
            }
        }
        request
    }
}

#[derive(Debug, Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    directory: bool,
    size: u64,
    modified: Option<SystemTime>,
}

/// Dossiers habituels de la colonne : nom affiché et chemin.
fn places() -> Vec<(&'static str, PathBuf)> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/".into());
    let mut places = vec![("Dossier personnel", home.clone())];
    for (label, names) in [
        ("Bureau", ["Bureau", "Desktop"]),
        ("Documents", ["Documents", "Documents"]),
        ("Téléchargements", ["Téléchargements", "Downloads"]),
        ("Images", ["Images", "Pictures"]),
        ("Musique", ["Musique", "Music"]),
        ("Vidéos", ["Vidéos", "Videos"]),
    ] {
        if let Some(path) = names
            .iter()
            .map(|name| home.join(name))
            .find(|path| path.is_dir())
        {
            places.push((label, path));
        }
    }
    places
}

pub struct ChooserState {
    request: Request,
    folder: PathBuf,
    history: Vec<PathBuf>,
    entries: Vec<Entry>,
    selected: Vec<usize>,
    name: String,
    filter: usize,
    icons: Icons,
    /// Fichier existant : un second appui le remplace.
    confirm_replace: Option<PathBuf>,
    error: Option<String>,
    new_folder: Option<String>,
}

impl std::fmt::Debug for ChooserState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChooserState")
            .field("folder", &self.folder)
            .finish()
    }
}

impl ChooserState {
    pub fn new(request: Request) -> Self {
        let folder = request
            .folder
            .clone()
            .filter(|folder| folder.is_dir())
            .unwrap_or_else(|| places()[0].1.clone());
        let mut state = Self {
            name: request.name.clone(),
            filter: request.filter.min(request.filters.len().saturating_sub(1)),
            request,
            folder: folder.clone(),
            history: Vec::new(),
            entries: Vec::new(),
            selected: Vec::new(),
            icons: Icons::default(),
            confirm_replace: None,
            error: None,
            new_folder: None,
        };
        state.load(folder);
        state
    }

    fn load(&mut self, folder: PathBuf) {
        self.folder = folder;
        self.selected.clear();
        self.confirm_replace = None;
        self.error = None;
        let filter = self.request.filters.get(self.filter).cloned();
        let only_folders = self.request.mode == Mode::Folder;
        let mut entries: Vec<Entry> = std::fs::read_dir(&self.folder)
            .map(|read| {
                read.filter_map(Result::ok)
                    .filter_map(|item| {
                        let name = item.file_name().to_string_lossy().into_owned();
                        if name.starts_with('.') {
                            return None;
                        }
                        let path = item.path();
                        let meta = std::fs::metadata(&path).ok()?;
                        let directory = meta.is_dir();
                        if !directory
                            && (only_folders || filter.as_ref().is_some_and(|filter| !filter.accepts(&name)))
                        {
                            return None;
                        }
                        Some(Entry {
                            path,
                            name,
                            directory,
                            size: meta.len(),
                            modified: meta.modified().ok(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_else(|error| {
                self.error = Some(format!("Dossier illisible : {error}"));
                Vec::new()
            });
        entries.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        self.entries = entries;
    }

    fn enter(&mut self, folder: PathBuf) {
        self.history.push(self.folder.clone());
        self.load(folder);
    }

    fn button_label(&self) -> String {
        if !self.request.button.is_empty() && self.request.button != "Save" && self.request.button != "Open" {
            return self.request.button.replace('_', "");
        }
        match self.request.mode {
            Mode::Open => "Ouvrir",
            Mode::Save => "Enregistrer",
            Mode::Folder => "Choisir",
        }
        .to_owned()
    }

    fn title(&self) -> String {
        if !self.request.title.is_empty() {
            return self.request.title.clone();
        }
        match self.request.mode {
            Mode::Open if self.request.multiple => "Ouvrir des fichiers",
            Mode::Open => "Ouvrir un fichier",
            Mode::Save => "Enregistrer",
            Mode::Folder => "Choisir un dossier",
        }
        .to_owned()
    }

    /// Chemins choisis, s'il y en a.
    fn choice(&self) -> Vec<PathBuf> {
        match self.request.mode {
            Mode::Save => {
                let name = self.name.trim();
                if name.is_empty() {
                    return Vec::new();
                }
                vec![self.folder.join(name)]
            }
            Mode::Folder => match self.selected.first().and_then(|&index| self.entries.get(index)) {
                Some(entry) if entry.directory => vec![entry.path.clone()],
                _ => vec![self.folder.clone()],
            },
            Mode::Open => self
                .selected
                .iter()
                .filter_map(|&index| self.entries.get(index))
                .filter(|entry| !entry.directory)
                .map(|entry| entry.path.clone())
                .collect(),
        }
    }

    /// Bouton principal ou Entrée : rend le choix et ferme.
    fn accept(&mut self) {
        // Ouvrir sur un dossier sélectionné : on y entre.
        if self.request.mode == Mode::Open
            && let Some(entry) = self.selected.first().and_then(|&index| self.entries.get(index))
            && entry.directory
        {
            let path = entry.path.clone();
            self.enter(path);
            return;
        }
        let choice = self.choice();
        if choice.is_empty() {
            return;
        }
        if self.request.mode == Mode::Save
            && choice[0].exists()
            && self.confirm_replace.as_ref() != Some(&choice[0])
        {
            self.confirm_replace = Some(choice[0].clone());
            return;
        }
        let mut out = std::io::stdout().lock();
        for path in &choice {
            let _ = writeln!(out, "{}", path.display());
        }
        let _ = out.flush();
        std::process::exit(0);
    }

    fn cancel() -> ! {
        std::process::exit(1);
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            Self::cancel();
        }
        if ui.input(|input| input.key_pressed(egui::Key::Enter)) && self.new_folder.is_none() {
            self.accept();
        }
        let title = self.title();
        style::header(ui, |ui| {
            let bar = ui.max_rect();
            ui.painter().text(
                bar.center(),
                egui::Align2::CENTER_CENTER,
                &title,
                egui::FontId::new(14.0, style::strong_family()),
                style::primary_text(),
            );
        });
        self.footer(ui);
        style::sidebar(ui, "lieux", 200.0, style::sidebar_color(), false, |ui| {
            ui.add_space(8.0);
            style::sidebar_heading(ui, "Emplacements");
            for (label, path) in places() {
                let selected = self.folder == path;
                let icon = path_icon(&path, true);
                if style::sidebar_tile_item(ui, &mut self.icons, icon, style::accent(), label, selected)
                    .clicked()
                {
                    self.enter(path);
                }
            }
        });
        egui::CentralPanel::no_frame()
            .frame(
                Frame::new()
                    .fill(style::background())
                    .inner_margin(Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| {
                self.path_bar(ui);
                ui.add_space(8.0);
                self.list(ui);
            });
    }

    fn path_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let back = kit::tool_button(ui, 30.0, false, "Précédent", |painter, rect, ink| {
                let c = rect.center();
                painter.line(
                    vec![
                        c + egui::vec2(3.0, -6.0),
                        c + egui::vec2(-3.0, 0.0),
                        c + egui::vec2(3.0, 6.0),
                    ],
                    egui::Stroke::new(2.0, ink),
                );
            });
            if back.clicked() {
                if let Some(previous) = self.history.pop() {
                    self.load(previous);
                } else if let Some(parent) = self.folder.parent().map(Path::to_path_buf) {
                    self.load(parent);
                }
            }
            // Fil d'Ariane : chaque dossier parent est cliquable.
            let mut crumbs: Vec<PathBuf> = self.folder.ancestors().map(Path::to_path_buf).collect();
            crumbs.reverse();
            let mut clicked = None;
            for crumb in crumbs.iter().skip(crumbs.len().saturating_sub(4)) {
                let name = crumb
                    .file_name()
                    .map_or_else(|| "/".to_owned(), |name| name.to_string_lossy().into_owned());
                let last = crumb == &self.folder;
                let text = RichText::new(name).size(13.0).color(if last {
                    style::primary_text()
                } else {
                    style::text_secondary()
                });
                if ui.add(egui::Button::new(text).frame(false)).clicked() && !last {
                    clicked = Some(crumb.clone());
                }
                if !last {
                    ui.label(RichText::new("›").color(style::tertiary_text()));
                }
            }
            if let Some(crumb) = clicked {
                self.enter(crumb);
            }
            if self.request.mode != Mode::Open {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::button(ui, "Nouveau dossier", false).clicked() {
                        self.new_folder = Some(String::new());
                    }
                    if let Some(name) = &mut self.new_folder {
                        let field = ui.add(
                            egui::TextEdit::singleline(name)
                                .hint_text("Nom du dossier")
                                .desired_width(180.0),
                        );
                        field.request_focus();
                        if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                            let path = self.folder.join(name.trim());
                            let created = std::fs::create_dir(&path);
                            self.new_folder = None;
                            match created {
                                Ok(()) => self.enter(path),
                                Err(error) => self.error = Some(format!("Dossier non créé : {error}")),
                            }
                        }
                    }
                });
            }
        });
    }

    fn list(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.colored_label(style::error(), error);
        }
        if self.entries.is_empty() && self.error.is_none() {
            kit::note(ui, "Ce dossier est vide.");
            return;
        }
        let row_h = 34.0;
        let mut open = None;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            row_h,
            self.entries.len(),
            |ui, rows| {
                for index in rows {
                    let entry = self.entries[index].clone();
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
                    let selected = self.selected.contains(&index);
                    if selected {
                        ui.painter().rect_filled(rect, 8.0, style::sidebar_selected());
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 8.0, style::control_hover());
                    }
                    let icon = egui::Rect::from_center_size(
                        rect.left_center() + egui::vec2(20.0, 0.0),
                        egui::vec2(22.0, 22.0),
                    );
                    paint_path_icon(&mut self.icons, ui, &entry.path, entry.directory, icon);
                    let ink = if selected {
                        style::sidebar_selected_text()
                    } else {
                        style::primary_text()
                    };
                    ui.painter().text(
                        rect.left_center() + egui::vec2(42.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        &entry.name,
                        egui::FontId::proportional(13.5),
                        ink,
                    );
                    let detail = if entry.directory {
                        format_date(entry.modified)
                    } else {
                        format!("{}   {}", format_size(entry.size), format_date(entry.modified))
                    };
                    ui.painter().text(
                        rect.right_center() - egui::vec2(12.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        detail,
                        egui::FontId::proportional(12.0),
                        style::text_secondary(),
                    );
                    if response.clicked() {
                        let adding = self.request.multiple && ui.input(|input| input.modifiers.command);
                        if adding {
                            if let Some(at) = self.selected.iter().position(|&known| known == index) {
                                self.selected.remove(at);
                            } else {
                                self.selected.push(index);
                            }
                        } else {
                            self.selected = vec![index];
                        }
                        if self.request.mode == Mode::Save && !entry.directory {
                            self.name = entry.name.clone();
                        }
                        self.confirm_replace = None;
                    }
                    if response.double_clicked() {
                        open = Some(entry);
                    }
                }
            },
        );
        if let Some(entry) = open {
            if entry.directory {
                self.enter(entry.path);
            } else {
                self.accept();
            }
        }
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom(ui.scope_id().with("choix"))
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(style::header_color())
                    .inner_margin(Margin::symmetric(16, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.request.mode == Mode::Save {
                        ui.label(RichText::new("Nom").size(13.0));
                        let field = ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(280.0));
                        if field.changed() {
                            self.confirm_replace = None;
                        }
                    }
                    if self.request.filters.len() > 1 {
                        let mut filter = self.filter;
                        egui::ComboBox::from_id_salt("filtre")
                            .selected_text(&self.request.filters[filter].name)
                            .show_ui(ui, |ui| {
                                for (index, item) in self.request.filters.iter().enumerate() {
                                    ui.selectable_value(&mut filter, index, &item.name);
                                }
                            });
                        if filter != self.filter {
                            self.filter = filter;
                            let folder = self.folder.clone();
                            self.load(folder);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if self.confirm_replace.is_some() {
                            "Remplacer".to_owned()
                        } else {
                            self.button_label()
                        };
                        let ready = !self.choice().is_empty();
                        if ui
                            .add_enabled_ui(ready, |ui| kit::button(ui, &label, true))
                            .inner
                            .clicked()
                        {
                            self.accept();
                        }
                        if kit::button(ui, "Annuler", false).clicked() {
                            Self::cancel();
                        }
                        if self.confirm_replace.is_some() {
                            ui.label(
                                RichText::new("Ce fichier existe déjà.")
                                    .size(12.0)
                                    .color(style::warning()),
                            );
                        }
                    });
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motifs_des_filtres() {
        assert!(glob("*.pdf", "guide.pdf"));
        assert!(!glob("*.pdf", "guide.pdf.txt"));
        assert!(glob("*", "tout"));
        assert!(glob("photo*.jp*g", "photo-1.jpeg"));
        let filter = Filter {
            name: "Images".into(),
            patterns: vec!["*.png".into(), "*.JPG".into()],
        };
        assert!(filter.accepts("Vacances.jpg"));
        assert!(!filter.accepts("notes.txt"));
    }

    #[test]
    fn lit_la_demande() {
        let args: Vec<String> = [
            "--enregistrer",
            "--titre",
            "Enregistrer le PDF",
            "--nom",
            "guide.pdf",
            "--filtre",
            "PDF|*.pdf",
            "--filtre",
            "Tout|*",
            "--filtre-actuel",
            "1",
        ]
        .map(str::to_owned)
        .to_vec();
        let request = Request::parse(&args);
        assert_eq!(request.mode, Mode::Save);
        assert_eq!(request.name, "guide.pdf");
        assert_eq!(request.filters.len(), 2);
        assert_eq!(request.filter, 1);
    }
}
