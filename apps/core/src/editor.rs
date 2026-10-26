//! TextEdit : éditeur de texte UTF-8.

use std::{
    fs,
    path::{Path, PathBuf},
};

use eframe::egui::{self, Frame, Margin, RichText};

use crate::style;

#[derive(Debug, Default)]
pub struct EditorState {
    pub path: String,
    pub contents: String,
    message: String,
    error: bool,
    dirty: bool,
    title: String,
}

impl EditorState {
    pub fn open(&mut self, path: PathBuf) {
        match fs::read_to_string(&path) {
            Ok(contents) => {
                self.path = path.display().to_string();
                self.contents = contents;
                self.report("Ouvert", false);
                self.dirty = false;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.path = path.display().to_string();
                self.contents.clear();
                self.report("Nouveau fichier", false);
                self.dirty = false;
            }
            Err(error) => self.report(format!("Lecture impossible : {error}"), true),
        }
    }

    pub fn save(&mut self) {
        if self.path.trim().is_empty() {
            self.report("Indique un chemin avant d’enregistrer", true);
            return;
        }
        match fs::write(Path::new(&self.path), &self.contents) {
            Ok(()) => {
                self.report("Enregistré", false);
                self.dirty = false;
            }
            Err(error) => self.report(format!("Enregistrement impossible : {error}"), true),
        }
    }

    fn report(&mut self, message: impl Into<String>, error: bool) {
        self.message = message.into();
        self.error = error;
    }

    /// Titre de la fenêtre : nom du document, marqué s'il a des modifications.
    fn window_title(&self) -> String {
        let name = Path::new(&self.path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Sans titre".to_owned());
        let mark = if self.dirty { "• " } else { "" };
        format!("{mark}{name} — TextEdit")
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        if ui.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::S)) {
            self.save();
        }
        let title = self.window_title();
        if title != self.title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        style::header(ui, |ui| {
            let field = egui::TextEdit::singleline(&mut self.path)
                .hint_text("Chemin du fichier, par exemple ~/notes.txt")
                .desired_width((ui.available_width() - 230.0).max(120.0));
            let response = ui.add(field);
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                self.open(PathBuf::from(expand_home(&self.path)));
            }
            if ui.button("Ouvrir").clicked() {
                self.open(PathBuf::from(expand_home(&self.path)));
            }
            if style::suggested_button(ui, "Enregistrer").clicked() {
                self.path = expand_home(&self.path);
                self.save();
            }
        });

        let lines = self.contents.lines().count().max(1);
        let characters = self.contents.chars().count();
        let (message, error, dirty) = (self.message.clone(), self.error, self.dirty);
        style::status_bar(ui, |ui| {
            ui.label(format!(
                "{lines} ligne{} · {characters} caractère{}",
                plural(lines),
                plural(characters)
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if error {
                    ui.colored_label(style::ERROR, message);
                } else if dirty {
                    ui.label("Modifications non enregistrées");
                } else if !message.is_empty() {
                    ui.label(message);
                }
            });
        });

        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(style::BACKGROUND))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut self.contents)
                            .font(egui::TextStyle::Monospace)
                            .frame(Frame::new().inner_margin(Margin::symmetric(20, 16)))
                            .desired_width(f32::INFINITY)
                            .desired_rows(30)
                            .hint_text(RichText::new("Commence à écrire…").color(style::TEXT_SECONDARY))
                            .lock_focus(true),
                    );
                    self.dirty |= response.changed();
                });
            });
    }
}

fn plural(count: usize) -> &'static str {
    if count > 1 { "s" } else { "" }
}

/// Remplace un `~` initial par le dossier personnel.
fn expand_home(path: &str) -> String {
    match (path.strip_prefix('~'), std::env::var("HOME")) {
        (Some(rest), Ok(home)) if rest.is_empty() || rest.starts_with('/') => format!("{home}{rest}"),
        _ => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::{env, time::SystemTime};

    use super::*;

    #[test]
    fn textedit_ouvre_et_enregistre_un_fichier_utf8() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = env::temp_dir().join(format!("ferrite-textedit-{}-{unique}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("note.txt");
        fs::write(&path, "Bonjour, ferrite !").unwrap();

        let mut editor = EditorState::default();
        editor.open(path.clone());
        assert_eq!(editor.contents, "Bonjour, ferrite !");
        assert_eq!(editor.window_title(), "note.txt — TextEdit");
        editor.contents = "Texte modifié : été".into();
        editor.save();
        assert_eq!(fs::read_to_string(path).unwrap(), "Texte modifié : été");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn le_tilde_designe_le_dossier_personnel() {
        let home = env::var("HOME").unwrap();
        assert_eq!(expand_home("~/a.txt"), format!("{home}/a.txt"));
        assert_eq!(expand_home("/tmp/~a"), "/tmp/~a");
    }
}
