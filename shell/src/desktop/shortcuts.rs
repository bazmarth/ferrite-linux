//! Raccourcis du bureau : applications, dossiers et fichiers posés sur une grille, sur
//! tous les bureaux sauf celui des widgets. Rangés en colonnes depuis le coin haut droit,
//! ils se déplacent à la souris vers n'importe quelle case ; Ctrl+molette, le pincement ou
//! le menu du bureau changent la taille des icônes.
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

use ferrite_system::shortcuts::{self, Cell, Desktop, Shortcut};
use smithay::utils::{Logical, Point};
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
    IconsBigger,
    IconsSmaller,
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
            DesktopMenuItem::IconsBigger => "Agrandir les icônes",
            DesktopMenuItem::IconsSmaller => "Réduire les icônes",
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
    /// Case choisie à la souris ; aucune : rangé à la suite des autres.
    pub cell: Option<Cell>,
}

/// Écart du pointeur, en pixels logiques, au-delà duquel un appui devient un glisser.
const DRAG_THRESHOLD: f64 = 6.0;
/// Pas de la taille des icônes (molette, menu).
pub const ICON_STEP: u32 = 8;

/// Raccourci saisi : relâché sans avoir bougé, il s'ouvre ; sinon il va dans la case visée.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShortcutDrag {
    pub index: usize,
    pub press: Point<f64, Logical>,
    pub pointer: Point<f64, Logical>,
    pub moved: bool,
}

impl ShortcutDrag {
    /// Déplacement depuis l'appui, une fois le glisser commencé.
    pub fn offset(&self) -> Option<(f64, f64)> {
        self.moved
            .then_some((self.pointer.x - self.press.x, self.pointer.y - self.press.y))
    }
}

/// Nouvelles cases après le dépôt du raccourci `index` sur `target` : toutes les icônes
/// gardent la case où on les voit (`shown`), et celle qui occupait la case visée prend
/// l'ancienne place de l'icône déplacée.
pub fn drop_on(shown: &[Option<Cell>], index: usize, target: Cell) -> Vec<Option<Cell>> {
    let mut cells = shown.to_vec();
    let Some(from) = cells.get(index).copied() else {
        return cells;
    };
    if let Some(other) = cells.iter().position(|cell| *cell == Some(target)) {
        cells[other] = from;
    }
    cells[index] = Some(target);
    cells
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
            .items
            .iter()
            .map(|item| match &item.shortcut {
                Shortcut::App(token) => {
                    let key = AppKey::from_token(token, &self.applications);
                    ShortcutView {
                        missing: matches!(key, AppKey::Other(_)),
                        icon: key.icon(&self.applications),
                        name: key.name(&self.applications),
                        cell: item.cell,
                    }
                }
                Shortcut::Path(path) => ShortcutView {
                    icon: AppIcon::Builtin(path_icon(path)),
                    name: Shortcut::file_name(path),
                    missing: !path.exists(),
                    cell: item.cell,
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
            Ok(()) => self
                .desktop_shortcuts
                .items
                .retain(|item| item.shortcut != *shortcut),
            Err(err) => warn!(%err, "raccourci du bureau non retiré"),
        }
    }

    /// Ouvre le raccourci `index` : l'application, ou le dossier dans Fichiers, ou le
    /// fichier dans l'application choisie pour son type.
    pub fn open_shortcut(&mut self, index: usize) {
        let Some(shortcut) = self
            .desktop_shortcuts
            .items
            .get(index)
            .map(|item| item.shortcut.clone())
        else {
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
            None => {
                let mut items = vec![
                    DesktopMenuItem::EditWidgets,
                    DesktopMenuItem::AddFolder,
                    DesktopMenuItem::Wallpaper,
                ];
                // Les icônes ne sont pas sur le bureau des widgets.
                let size = self.desktop_shortcuts.icon_size;
                if self.workspaces.current() != 0 && !self.desktop_shortcuts.items.is_empty() {
                    if size < shortcuts::ICON_SIZE_MAX {
                        items.push(DesktopMenuItem::IconsBigger);
                    }
                    if size > shortcuts::ICON_SIZE_MIN {
                        items.push(DesktopMenuItem::IconsSmaller);
                    }
                }
                items
            }
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
            Some(DesktopMenuItem::IconsBigger) => {
                self.set_icon_size(self.desktop_shortcuts.icon_size + ICON_STEP, true)
            }
            Some(DesktopMenuItem::IconsSmaller) => {
                self.set_icon_size(self.desktop_shortcuts.icon_size.saturating_sub(ICON_STEP), true)
            }
            Some(DesktopMenuItem::Open(shortcut)) => self.open_shortcut(shortcut),
            Some(DesktopMenuItem::Remove(shortcut)) => {
                if let Some(item) = self.desktop_shortcuts.items.get(shortcut).cloned() {
                    self.remove_shortcut(&item.shortcut);
                }
            }
            None => {}
        }
    }

    /// Taille des icônes ; `save` : l'écrire aussi (pas à chaque pas d'un pincement).
    pub fn set_icon_size(&mut self, size: u32, save: bool) {
        let size = Desktop::clamp_size(size);
        let changed = size != self.desktop_shortcuts.icon_size;
        self.desktop_shortcuts.icon_size = size;
        if changed || save {
            self.request_redraw();
        }
        if save {
            self.save_desktop();
        }
    }

    fn save_desktop(&self) {
        if let Err(err) = shortcuts::save(&self.desktop_shortcuts) {
            warn!(%err, "bureau non enregistré");
        }
    }

    /// Appui sur le raccourci `index` : rien ne s'ouvre avant le relâchement.
    pub fn press_shortcut(&mut self, index: usize, pointer: Point<f64, Logical>) {
        self.shortcut_drag = Some(ShortcutDrag {
            index,
            press: pointer,
            pointer,
            moved: false,
        });
    }

    /// Le raccourci saisi suit le pointeur, une fois passé le petit seuil.
    pub fn drag_shortcut(&mut self, pointer: Point<f64, Logical>) {
        let Some(drag) = self.shortcut_drag.as_mut() else {
            return;
        };
        drag.pointer = pointer;
        let (dx, dy) = (pointer.x - drag.press.x, pointer.y - drag.press.y);
        if dx.hypot(dy) > DRAG_THRESHOLD {
            drag.moved = true;
        }
    }

    /// Relâchement : ouvre le raccourci, ou le pose dans la case visée. Faux s'il n'y avait
    /// pas de raccourci saisi.
    pub fn release_shortcut(&mut self) -> bool {
        let Some(drag) = self.shortcut_drag.take() else {
            return false;
        };
        if !drag.moved {
            self.stop_dashboard_editing();
            self.open_shortcut(drag.index);
            return true;
        }
        let Some(area) = self.screen_area() else {
            return true;
        };
        let mut view = crate::render::desktop_view(self);
        view.dashboard.shortcut_drag = None;
        let shown = crate::desktop::panels::shortcut_cells(area, &view);
        let Some(target) = crate::desktop::panels::shortcut_drop_cell(area, &view, drag.index, drag.offset())
        else {
            return true;
        };
        for (item, cell) in self
            .desktop_shortcuts
            .items
            .iter_mut()
            .zip(drop_on(&shown, drag.index, target))
        {
            item.cell = cell;
        }
        self.save_desktop();
        true
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

    #[test]
    fn deposer_sur_une_icone_echange_les_places() {
        let cell = |column, row| Some(Cell { column, row });
        let shown = [cell(0, 0), cell(0, 1), cell(0, 2)];
        // Sur une case libre : seule l'icône déplacée change.
        assert_eq!(
            drop_on(&shown, 0, Cell { column: 4, row: 3 }),
            vec![cell(4, 3), cell(0, 1), cell(0, 2)]
        );
        // Sur une autre icône : elles échangent leurs places.
        assert_eq!(
            drop_on(&shown, 0, Cell { column: 0, row: 2 }),
            vec![cell(0, 2), cell(0, 1), cell(0, 0)]
        );
    }
}
