//! Fichiers : barre latérale d'emplacements, chemin cliquable et liste détaillée.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

use eframe::egui::{self, Frame, Margin, RichText, Sense};

use crate::style::{self, Icons};

#[derive(Debug)]
struct FileEntry {
    name: String,
    path: PathBuf,
    directory: bool,
    modified: Option<SystemTime>,
    size: Option<u64>,
}

#[derive(Debug)]
pub struct FilesState {
    current: PathBuf,
    entries: Vec<FileEntry>,
    selected: Option<PathBuf>,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    show_hidden: bool,
    message: String,
    /// Date de modification du dossier lors de sa dernière lecture.
    loaded: Option<SystemTime>,
    icons: Icons,
}

impl Default for FilesState {
    fn default() -> Self {
        let mut state = Self {
            current: home(),
            entries: Vec::new(),
            selected: None,
            back: Vec::new(),
            forward: Vec::new(),
            show_hidden: false,
            message: String::new(),
            loaded: None,
            icons: Icons::default(),
        };
        state.refresh();
        state
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from("/"))
}

impl FilesState {
    /// Ouvre `path` ; l'emplacement quitté va dans l'historique « précédent ».
    fn navigate(&mut self, path: PathBuf) {
        if path == self.current {
            return;
        }
        if path.is_dir() {
            let previous = std::mem::replace(&mut self.current, path);
            self.back.push(previous);
            self.forward.clear();
            self.refresh();
        } else {
            self.message = format!("Dossier introuvable : {}", path.display());
        }
    }

    fn go_back(&mut self) {
        if let Some(path) = self.back.pop() {
            let previous = std::mem::replace(&mut self.current, path);
            self.forward.push(previous);
            self.refresh();
        }
    }

    fn go_forward(&mut self) {
        if let Some(path) = self.forward.pop() {
            let previous = std::mem::replace(&mut self.current, path);
            self.back.push(previous);
            self.refresh();
        }
    }

    fn refresh(&mut self) {
        self.selected = None;
        self.message.clear();
        self.loaded = modified(&self.current);
        self.entries = match fs::read_dir(&self.current) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let metadata = entry.metadata().ok()?;
                    Some(FileEntry {
                        name: entry.file_name().to_string_lossy().into_owned(),
                        path: entry.path(),
                        directory: metadata.is_dir(),
                        modified: metadata.modified().ok(),
                        size: (!metadata.is_dir()).then_some(metadata.len()),
                    })
                })
                .collect(),
            Err(error) => {
                self.message = format!("Impossible de lire ce dossier : {error}");
                Vec::new()
            }
        };
        self.entries.sort_by(|left, right| {
            right
                .directory
                .cmp(&left.directory)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
    }

    fn open(&mut self, path: PathBuf, directory: bool) {
        if directory {
            self.navigate(path);
            return;
        }
        let result = if is_text_file(&path) {
            // TextEdit est le même exécutable que Fichiers.
            env::current_exe()
                .map(Command::new)
                .and_then(|mut command| command.arg("textedit").arg(&path).spawn())
        } else {
            Command::new("xdg-open").arg(&path).spawn()
        };
        if let Err(error) = result {
            self.message = format!("Ouverture impossible : {error}");
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        // Le dossier affiché suit les changements faits ailleurs (F5 force la relecture).
        if self.loaded != modified(&self.current) || ui.input(|input| input.key_pressed(egui::Key::F5)) {
            let selected = self.selected.take();
            self.refresh();
            self.selected = selected;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(2));
        if ui.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::H)) {
            self.show_hidden = !self.show_hidden;
        }
        if ui.input(|input| input.modifiers.alt && input.key_pressed(egui::Key::ArrowLeft)) {
            self.go_back();
        }

        style::header(ui, |ui| {
            if style::tool_button(ui, "←", !self.back.is_empty())
                .on_hover_text("Précédent")
                .clicked()
            {
                self.go_back();
            }
            if style::tool_button(ui, "→", !self.forward.is_empty())
                .on_hover_text("Suivant")
                .clicked()
            {
                self.go_forward();
            }
            ui.add_space(6.0);
            self.breadcrumb(ui);
        });

        let visible = self
            .entries
            .iter()
            .filter(|entry| self.show_hidden || !entry.name.starts_with('.'))
            .count();
        let message = self.message.clone();
        style::status_bar(ui, |ui| {
            if message.is_empty() {
                ui.label(match visible {
                    0 => "Dossier vide".to_owned(),
                    1 => "1 élément".to_owned(),
                    count => format!("{count} éléments"),
                });
            } else {
                ui.colored_label(style::ERROR, message);
            }
        });

        egui::Panel::left("emplacements")
            .resizable(false)
            .exact_size(190.0)
            .frame(
                Frame::new()
                    .fill(style::SIDEBAR)
                    .inner_margin(Margin::symmetric(8, 10)),
            )
            .show(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(style::BACKGROUND))
            .show(ui, |ui| self.list(ui));
    }

    /// Chemin courant, un bouton par dossier parent.
    fn breadcrumb(&mut self, ui: &mut egui::Ui) {
        let mut target = None;
        let mut parts: Vec<PathBuf> = self.current.ancestors().map(Path::to_path_buf).collect();
        parts.reverse();
        let last = parts.len().saturating_sub(1);
        for (index, part) in parts.iter().enumerate() {
            let name = match part.file_name() {
                Some(name) => name.to_string_lossy().into_owned(),
                None => "Système".to_owned(),
            };
            if index > 0 {
                ui.label(RichText::new("›").color(style::TEXT_SECONDARY));
            }
            let text = if index == last {
                RichText::new(name).family(style::strong_family())
            } else {
                RichText::new(name)
            };
            if ui
                .add(egui::Button::new(text).frame_when_inactive(false))
                .clicked()
                && index != last
            {
                target = Some(part.clone());
            }
        }
        if let Some(target) = target {
            self.navigate(target);
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let home = home();
        let mut favorites = vec![("Accueil", home.clone(), "status-user")];
        for (name, folder, icon) in [
            ("Bureau", "Bureau", "status-display"),
            ("Documents", "Documents", "file-text"),
            ("Téléchargements", "Téléchargements", "status-download"),
            ("Images", "Images", "app-photos"),
            ("Musique", "Musique", "app-music"),
            ("Vidéos", "Vidéos", "app-video"),
        ] {
            let path = home.join(folder);
            if path.is_dir() {
                favorites.push((name, path, icon));
            }
        }
        let devices = [("Ordinateur", PathBuf::from("/"), "status-storage")];

        let mut target = None;
        for (title, places) in [("Favoris", favorites.as_slice()), ("Appareils", &devices[..])] {
            style::sidebar_heading(ui, title);
            for (name, path, icon) in places {
                let selected = self.current == *path;
                let response = style::sidebar_item(
                    ui,
                    &mut self.icons,
                    icon,
                    style::IconStyle::Tinted(style::ACCENT),
                    name,
                    selected,
                );
                if response.clicked() {
                    target = Some(path.clone());
                }
            }
            ui.add_space(8.0);
        }
        if let Some(target) = target {
            self.navigate(target);
        }
    }

    fn list(&mut self, ui: &mut egui::Ui) {
        const ROW: f32 = 32.0;
        let width = ui.available_width();
        let columns = [width - 260.0, 100.0, 140.0];
        // En-tête des colonnes.
        let (header, _) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::hover());
        let painter = ui.painter();
        let small = egui::FontId::proportional(11.5);
        for (index, title) in ["Nom", "Taille", "Modifié"].into_iter().enumerate() {
            let x = header.left()
                + 16.0
                + columns[..index].iter().sum::<f32>()
                + if index == 0 { 30.0 } else { 8.0 * index as f32 };
            painter.text(
                egui::pos2(x, header.center().y),
                egui::Align2::LEFT_CENTER,
                title,
                small.clone(),
                style::TEXT_SECONDARY,
            );
        }
        painter.hline(
            header.x_range(),
            header.bottom(),
            egui::Stroke::new(1.0, style::SEPARATOR),
        );

        let mut open = None;
        let mut select = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let entries = self
                .entries
                .iter()
                .filter(|entry| self.show_hidden || !entry.name.starts_with('.'));
            for (index, entry) in entries.enumerate() {
                let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ROW), Sense::click());
                let selected = self.selected.as_ref() == Some(&entry.path);
                let fill = if selected {
                    Some(style::SELECTION)
                } else if response.hovered() {
                    Some(egui::Color32::from_rgb(0xF2, 0xF5, 0xF9))
                } else if index % 2 == 1 {
                    Some(egui::Color32::from_rgb(0xFA, 0xFA, 0xFB))
                } else {
                    None
                };
                if let Some(fill) = fill {
                    ui.painter()
                        .rect_filled(rect.shrink2(egui::vec2(6.0, 1.0)), 6.0, fill);
                }
                let mut row = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink2(egui::vec2(16.0, 0.0)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                self.icons.show(&mut row, entry_icon(entry), 22.0);
                let cells = [
                    (columns[0] - 30.0, RichText::new(&entry.name).color(style::TEXT)),
                    (
                        columns[1],
                        RichText::new(entry.size.map(format_size).unwrap_or_else(|| "—".to_owned()))
                            .color(style::TEXT_SECONDARY),
                    ),
                    (
                        columns[2],
                        RichText::new(format_time(entry.modified)).color(style::TEXT_SECONDARY),
                    ),
                ];
                for (cell_width, text) in cells {
                    row.allocate_ui_with_layout(
                        egui::vec2(cell_width, ROW),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(cell_width);
                            ui.add(egui::Label::new(text).truncate().selectable(false));
                        },
                    );
                }
                if response.clicked() {
                    select = Some(entry.path.clone());
                }
                if response.double_clicked() {
                    open = Some((entry.path.clone(), entry.directory));
                }
            }
        });
        if let Some(path) = select {
            self.selected = Some(path);
        }
        if let Some((path, directory)) = open {
            self.open(path, directory);
        }
    }
}

/// Icône du dossier `icons/SVG` selon la nature de l'entrée.
fn entry_icon(entry: &FileEntry) -> &'static str {
    if entry.directory {
        return "app-files";
    }
    let extension = entry
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => "app-photos",
        "mp3" | "ogg" | "flac" | "wav" | "m4a" | "opus" => "app-music",
        "mp4" | "mkv" | "webm" | "avi" | "mov" => "app-video",
        "rs" | "c" | "h" | "py" | "js" | "ts" | "toml" | "json" | "yaml" | "yml" | "html" | "css" => {
            "app-code"
        }
        "sh" => "app-terminal",
        _ if is_text_file(&entry.path) => "file-text",
        _ => "file-generic",
    }
}

pub fn is_text_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "txt"
                    | "md"
                    | "rs"
                    | "toml"
                    | "json"
                    | "yaml"
                    | "yml"
                    | "sh"
                    | "conf"
                    | "log"
                    | "ini"
                    | "csv"
            )
        })
}

fn format_size(size: u64) -> String {
    const UNITS: [&str; 4] = ["o", "Kio", "Mio", "Gio"];
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{size} o")
    } else {
        format!("{value:.1} {}", UNITS[unit]).replace('.', ",")
    }
}

fn format_time(time: Option<SystemTime>) -> String {
    let Some(age) = time.and_then(|time| SystemTime::now().duration_since(time).ok()) else {
        return String::new();
    };
    match age.as_secs() / 86_400 {
        0 => "Aujourd’hui".into(),
        1 => "Hier".into(),
        days => format!("Il y a {days} jours"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailles_lisibles() {
        assert_eq!(format_size(512), "512 o");
        assert_eq!(format_size(1536), "1,5 Kio");
    }

    #[test]
    fn historique_precedent_suivant() {
        let mut files = FilesState::default();
        let start = files.current.clone();
        files.navigate(PathBuf::from("/"));
        assert_eq!(files.current, PathBuf::from("/"));
        files.go_back();
        assert_eq!(files.current, start);
        files.go_forward();
        assert_eq!(files.current, PathBuf::from("/"));
    }
}
