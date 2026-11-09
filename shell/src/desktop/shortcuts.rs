//! Raccourcis du bureau : applications, dossiers et fichiers posés sur le bureau des
//! widgets (bureau 0), en colonnes depuis le coin haut droit.
//!
//! La liste vit dans `ferrite_system::shortcuts` ; le fil système la relit quand son
//! fichier change (Fichiers y ajoute des dossiers). On ajoute un raccourci depuis le menu
//! d'une icône du Dock ou du menu Applications (clic droit), depuis Fichiers (« Ajouter au
//! bureau »), ou depuis le menu du bureau (« Ajouter un dossier au bureau… »).

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

use ferrite_system::shortcuts::{self, Shortcut};
use tracing::{info, warn};

use crate::{
    desktop::apps::{App, AppIcon, AppKey},
    state::Ferrite,
};

/// Choix du menu du bureau (clic droit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopMenuItem {
    EditWidgets,
    AddFolder,
    Wallpaper,
    /// Raccourci visé par le clic droit.
    Open(usize),
    Remove(usize),
}

impl DesktopMenuItem {
    pub fn label(self) -> &'static str {
        match self {
            DesktopMenuItem::EditWidgets => "Modifier les widgets",
            DesktopMenuItem::AddFolder => "Ajouter un dossier au bureau…",
            DesktopMenuItem::Wallpaper => "Changer le fond d’écran…",
            DesktopMenuItem::Open(_) => "Ouvrir",
            DesktopMenuItem::Remove(_) => "Retirer du bureau",
        }
    }
}

/// Raccourci tel que le bureau le dessine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutView {
    pub icon: AppIcon,
    pub name: String,
    /// Application désinstallée, dossier ou fichier disparu : nom estompé, rien à ouvrir.
    pub missing: bool,
}

/// Icône d'un dossier ou d'un fichier.
fn path_icon(path: &Path) -> &'static str {
    if !path.is_dir() {
        return crate::render::file_icon(path);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if home.as_deref() == Some(path) {
        return "places-home";
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let in_home = home.is_some() && path.parent() == home.as_deref();
    match name {
        "Bureau" | "Desktop" if in_home => "places-desktop",
        "Documents" if in_home => "places-documents",
        "Téléchargements" | "Downloads" if in_home => "places-downloads",
        "Musique" | "Music" if in_home => "places-music",
        "Images" | "Pictures" if in_home => "places-pictures",
        "Vidéos" | "Videos" if in_home => "places-videos",
        _ => "places-folder",
    }
}

impl Ferrite {
    /// Raccourcis à dessiner, dans l'ordre de la liste.
    pub fn shortcut_views(&self) -> Vec<ShortcutView> {
        self.desktop_shortcuts
            .iter()
            .map(|shortcut| match shortcut {
                Shortcut::App(token) => {
                    let key = AppKey::from_token(token, &self.applications);
                    ShortcutView {
                        missing: matches!(key, AppKey::Other(_)),
                        icon: key.icon(&self.applications),
                        name: key.name(&self.applications),
                    }
                }
                Shortcut::Path(path) => ShortcutView {
                    icon: AppIcon::Builtin(path_icon(path)),
                    name: Shortcut::file_name(path),
                    missing: !path.exists(),
                },
            })
            .collect()
    }

    /// L'application a-t-elle un raccourci sur le bureau.
    pub fn app_on_desktop(&self, key: &AppKey) -> bool {
        self.desktop_shortcuts.contains(&Shortcut::App(key.token()))
    }

    /// Ajoute ou retire le raccourci d'une application.
    pub fn set_app_on_desktop(&mut self, key: &AppKey, present: bool) {
        let shortcut = Shortcut::App(key.token());
        if present {
            self.add_shortcuts(&[shortcut]);
        } else {
            self.remove_shortcut(&shortcut);
        }
    }

    /// Ajoute des raccourcis ; la liste affichée suit tout de suite, sans attendre le fil
    /// système.
    pub fn add_shortcuts(&mut self, list: &[Shortcut]) {
        match shortcuts::add(list) {
            Ok(_) => self.desktop_shortcuts = shortcuts::load(),
            Err(err) => warn!(%err, "raccourci du bureau non enregistré"),
        }
    }

    fn remove_shortcut(&mut self, shortcut: &Shortcut) {
        match shortcuts::remove(shortcut) {
            Ok(()) => self.desktop_shortcuts.retain(|known| known != shortcut),
            Err(err) => warn!(%err, "raccourci du bureau non retiré"),
        }
    }

    /// Ouvre le raccourci `index` : l'application, ou le dossier dans Fichiers, ou le
    /// fichier dans l'application choisie pour son type.
    pub fn open_shortcut(&mut self, index: usize) {
        let Some(shortcut) = self.desktop_shortcuts.get(index).cloned() else {
            return;
        };
        match shortcut {
            Shortcut::App(token) => {
                let key = AppKey::from_token(&token, &self.applications);
                if matches!(key, AppKey::Other(_)) {
                    warn!(%token, "raccourci d'une application absente");
                    return;
                }
                self.activate_app(&key);
            }
            Shortcut::Path(path) => {
                if !path.exists() {
                    warn!(path = %path.display(), "raccourci vers un chemin disparu");
                    return;
                }
                let env = self.client_environment();
                let files = [path.clone()];
                let foreign = (!path.is_dir())
                    .then(|| ferrite_system::defaults::foreign_default(&path))
                    .flatten()
                    .map(|id| AppKey::Installed(id.trim_end_matches(".desktop").to_owned()))
                    .filter(|key| key.launch_with(&self.applications, &env, &files));
                let key = match foreign {
                    Some(key) => key,
                    None => {
                        let app = if path.is_dir() {
                            App::FILES
                        } else {
                            App::for_file(&path)
                        };
                        app.launch_with(&env, &files);
                        AppKey::Builtin(app)
                    }
                };
                self.start_bounce(&key);
            }
        }
    }

    /// Choix du menu du bureau, selon ce qui est sous le clic droit.
    pub fn desktop_menu_items(&self) -> Vec<DesktopMenuItem> {
        match self.desktop_menu_target {
            Some(index) => {
                let mut items = Vec::new();
                if self.shortcut_views().get(index).is_some_and(|view| !view.missing) {
                    items.push(DesktopMenuItem::Open(index));
                }
                items.push(DesktopMenuItem::Remove(index));
                items
            }
            None => vec![
                DesktopMenuItem::EditWidgets,
                DesktopMenuItem::AddFolder,
                DesktopMenuItem::Wallpaper,
            ],
        }
    }

    /// Choix `index` du menu du bureau.
    pub fn desktop_menu_choose(&mut self, index: usize) {
        let item = self.desktop_menu_items().get(index).copied();
        self.popover = None;
        self.desktop_menu_target = None;
        match item {
            Some(DesktopMenuItem::EditWidgets) => self.start_widgets_edit(),
            Some(DesktopMenuItem::AddFolder) => self.choose_folders_for_desktop(),
            Some(DesktopMenuItem::Wallpaper) => {
                App::open_settings_page("apparence", &self.client_environment())
            }
            Some(DesktopMenuItem::Open(shortcut)) => self.open_shortcut(shortcut),
            Some(DesktopMenuItem::Remove(shortcut)) => {
                if let Some(shortcut) = self.desktop_shortcuts.get(shortcut).cloned() {
                    self.remove_shortcut(&shortcut);
                }
            }
            None => {}
        }
    }

    /// Ouvre le sélecteur de dossiers ; ceux choisis rejoignent le bureau (le fil système
    /// voit le fichier changer). Le sélecteur attend dans un fil à part.
    fn choose_folders_for_desktop(&self) {
        let mut command = crate::desktop::apps::ferrite_command("ferrite-apps");
        command
            .args(["choisir", "--dossier", "--multiple"])
            .args(["--titre", "Ajouter au bureau", "--bouton", "Ajouter"])
            .envs(self.client_environment())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        thread::spawn(move || choose_and_add(command));
    }
}

fn choose_and_add(mut command: Command) {
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            warn!(%err, "sélecteur de dossiers indisponible");
            return;
        }
    };
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut output);
    }
    let _ = child.wait();
    let chosen: Vec<Shortcut> = output
        .lines()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(Shortcut::Path)
        .collect();
    if chosen.is_empty() {
        return;
    }
    match shortcuts::add(&chosen) {
        Ok(added) => info!(added, "dossiers ajoutés au bureau"),
        Err(err) => warn!(%err, "dossiers non ajoutés au bureau"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icone_d_un_dossier_ou_d_un_fichier() {
        let dir = std::env::temp_dir();
        assert_eq!(path_icon(&dir), "places-folder");
        assert_eq!(path_icon(Path::new("/nulle/part/rapport.pdf")), "file-pdf");
    }
}
