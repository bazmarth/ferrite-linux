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
    collections::HashSet,
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
    /// Fait partie de la sélection (clic, Ctrl+clic ou rectangle).
    pub selected: bool,
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

/// Case libre la plus proche de `want` dans une grille de `columns` × `rows` cases.
fn nearest_free(want: Cell, taken: &HashSet<Cell>, (columns, rows): (u16, u16)) -> Option<Cell> {
    (0..columns)
        .flat_map(|column| (0..rows).map(move |row| Cell { column, row }))
        .filter(|cell| !taken.contains(cell))
        .min_by_key(|cell| {
            let column = i32::from(cell.column) - i32::from(want.column);
            let row = i32::from(cell.row) - i32::from(want.row);
            (column * column + row * row, cell.column, cell.row)
        })
}

/// Nouvelles cases après le dépôt du groupe `moving` décalé de `delta` cases (colonnes
/// vers la gauche, lignes vers le bas), dans une grille de `size` cases. Toutes les icônes
/// gardent la case où on les voit (`shown`). Le groupe garde sa forme : il s'arrête au
/// bord de la grille. Une icône recouverte va dans une case laissée libre par le groupe
/// (la plus proche), sinon dans la case libre la plus proche : une icône seule lâchée sur
/// une autre échange donc sa place avec elle.
pub fn drop_group(
    shown: &[Option<Cell>],
    moving: &[usize],
    delta: (i32, i32),
    size: (u16, u16),
) -> Vec<Option<Cell>> {
    let mut cells = shown.to_vec();
    let group: Vec<(usize, Cell)> = moving
        .iter()
        .filter_map(|&index| Some((index, shown.get(index).copied()??)))
        .collect();
    let (columns, rows) = (i32::from(size.0), i32::from(size.1));
    if group.is_empty() || columns == 0 || rows == 0 {
        return cells;
    }
    let bound = |pick: fn(&Cell) -> u16| {
        let values = group.iter().map(|(_, cell)| i32::from(pick(cell)));
        (values.clone().min().unwrap_or(0), values.max().unwrap_or(0))
    };
    let (first_column, last_column) = bound(|cell| cell.column);
    let (first_row, last_row) = bound(|cell| cell.row);
    let delta = (
        delta
            .0
            .clamp(-first_column, (columns - 1 - last_column).max(-first_column)),
        delta.1.clamp(-first_row, (rows - 1 - last_row).max(-first_row)),
    );
    let mut taken = HashSet::new();
    for &(index, from) in &group {
        let to = Cell {
            column: (i32::from(from.column) + delta.0) as u16,
            row: (i32::from(from.row) + delta.1) as u16,
        };
        taken.insert(to);
        cells[index] = Some(to);
    }
    let staying: Vec<usize> = (0..cells.len())
        .filter(|index| !group.iter().any(|(moved, _)| moved == index))
        .collect();
    let mut covered = Vec::new();
    for &index in &staying {
        if let Some(cell) = cells[index]
            && !taken.insert(cell)
        {
            covered.push((index, cell));
        }
    }
    let mut vacated: Vec<Cell> = group
        .iter()
        .map(|(_, from)| *from)
        .filter(|cell| !taken.contains(cell))
        .collect();
    for (index, cell) in covered {
        let distance = |other: &Cell| {
            let column = i32::from(other.column) - i32::from(cell.column);
            let row = i32::from(other.row) - i32::from(cell.row);
            column * column + row * row
        };
        let place = match (0..vacated.len()).min_by_key(|&slot| distance(&vacated[slot])) {
            Some(slot) => Some(vacated.swap_remove(slot)),
            None => nearest_free(cell, &taken, size),
        };
        if let Some(place) = place {
            taken.insert(place);
        }
        cells[index] = place;
    }
    cells
}

/// Position d'une case dans la grille, de 0 à 1 dans chaque sens (son centre).
pub fn grid_fraction(cell: Cell, (columns, rows): (u16, u16)) -> (f64, f64) {
    (
        (f64::from(cell.column) + 0.5) / f64::from(columns.max(1)),
        (f64::from(cell.row) + 0.5) / f64::from(rows.max(1)),
    )
}

/// Cases d'une grille de `size` cases pour des icônes vues aux positions `fractions`
/// (voir `grid_fraction`) : chacune va à la même place relative de l'écran, ou à la case
/// libre la plus proche. Celles sans position vont dans la première case libre ; aucune
/// quand la grille est pleine.
pub fn place_fractions(fractions: &[Option<(f64, f64)>], size: (u16, u16)) -> Vec<Option<Cell>> {
    let (columns, rows) = size;
    let mut taken = HashSet::new();
    if columns == 0 || rows == 0 {
        return vec![None; fractions.len()];
    }
    let mut cells: Vec<Option<Cell>> = fractions
        .iter()
        .map(|fraction| {
            let (x, y) = (*fraction)?;
            let want = Cell {
                column: ((x * f64::from(columns)).floor().max(0.0) as u16).min(columns - 1),
                row: ((y * f64::from(rows)).floor().max(0.0) as u16).min(rows - 1),
            };
            let cell = if taken.contains(&want) {
                nearest_free(want, &taken, size)?
            } else {
                want
            };
            taken.insert(cell);
            Some(cell)
        })
        .collect();
    let mut free = (0..columns)
        .flat_map(|column| (0..rows).map(move |row| Cell { column, row }))
        .filter(|cell| !taken.contains(cell));
    for (cell, fraction) in cells.iter_mut().zip(fractions) {
        if cell.is_none() && fraction.is_none() {
            *cell = free.next();
        }
    }
    cells
}

/// Positions des icônes au premier d'une suite de changements de taille : toutes les
/// tailles se calculent depuis elles, si bien qu'agrandir puis réduire ramène chaque icône
/// à sa case. Oubliées dès que la liste ou les cases changent autrement.
#[derive(Debug, Clone, PartialEq)]
pub struct IconAnchor {
    shortcuts: Vec<Shortcut>,
    cells: Vec<Option<Cell>>,
    fractions: Vec<Option<(f64, f64)>>,
}

/// Rectangle de sélection tiré depuis le bureau nu.
#[derive(Debug, Clone, PartialEq)]
pub struct ShortcutLasso {
    pub origin: Point<f64, Logical>,
    pub pointer: Point<f64, Logical>,
    /// Sélection d'avant, gardée si Ctrl ou Maj était tenu.
    base: HashSet<Shortcut>,
}

impl ShortcutLasso {
    /// Rectangle tiré : coin haut gauche, largeur et hauteur.
    pub fn rect(&self) -> (f64, f64, f64, f64) {
        let (x0, x1) = (
            self.origin.x.min(self.pointer.x),
            self.origin.x.max(self.pointer.x),
        );
        let (y0, y1) = (
            self.origin.y.min(self.pointer.y),
            self.origin.y.max(self.pointer.y),
        );
        (x0, y0, x1 - x0, y1 - y0)
    }
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
                        selected: self.desktop_selection.contains(&item.shortcut),
                    }
                }
                Shortcut::Path(path) => ShortcutView {
                    icon: AppIcon::Builtin(path_icon(path)),
                    name: Shortcut::file_name(path),
                    missing: !path.exists(),
                    cell: item.cell,
                    selected: self.desktop_selection.contains(&item.shortcut),
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
            Some(DesktopMenuItem::Open(shortcut)) => {
                for index in self.selection_with(shortcut) {
                    self.open_shortcut(index);
                }
            }
            Some(DesktopMenuItem::Remove(shortcut)) => {
                let chosen: Vec<Shortcut> = self
                    .selection_with(shortcut)
                    .into_iter()
                    .filter_map(|index| self.desktop_shortcuts.items.get(index))
                    .map(|item| item.shortcut.clone())
                    .collect();
                for shortcut in &chosen {
                    self.remove_shortcut(shortcut);
                    self.desktop_selection.remove(shortcut);
                }
            }
            None => {}
        }
    }

    /// Raccourcis visés par une action sur `index` : toute la sélection s'il en fait
    /// partie, sinon lui seul.
    fn selection_with(&self, index: usize) -> Vec<usize> {
        let selected = |index: usize| {
            self.desktop_shortcuts
                .items
                .get(index)
                .is_some_and(|item| self.desktop_selection.contains(&item.shortcut))
        };
        if !selected(index) {
            return vec![index];
        }
        (0..self.desktop_shortcuts.items.len())
            .filter(|&other| selected(other))
            .collect()
    }

    /// Le raccourci `index` devient la sélection, sauf s'il en fait déjà partie (clic sur
    /// une icône d'un groupe : tout le groupe reste choisi).
    pub fn select_shortcut(&mut self, index: usize) {
        if let Some(item) = self.desktop_shortcuts.items.get(index)
            && !self.desktop_selection.contains(&item.shortcut)
        {
            self.desktop_selection = HashSet::from([item.shortcut.clone()]);
            self.request_redraw();
        }
    }

    /// Ctrl+clic : ajoute le raccourci `index` à la sélection, ou l'en retire.
    pub fn toggle_shortcut(&mut self, index: usize) {
        if let Some(item) = self.desktop_shortcuts.items.get(index) {
            if !self.desktop_selection.remove(&item.shortcut) {
                self.desktop_selection.insert(item.shortcut.clone());
            }
            self.request_redraw();
        }
    }

    /// Appui sur le bureau nu : début d'un rectangle de sélection. Sans Ctrl ni Maj
    /// (`extend`), la sélection d'avant est oubliée, comme après un simple clic à côté.
    pub fn start_lasso(&mut self, pointer: Point<f64, Logical>, extend: bool) {
        let base = if extend {
            self.desktop_selection.clone()
        } else {
            HashSet::new()
        };
        if self.desktop_selection != base {
            self.desktop_selection = base.clone();
            self.request_redraw();
        }
        self.shortcut_lasso = Some(ShortcutLasso {
            origin: pointer,
            pointer,
            base,
        });
    }

    /// Le rectangle suit le pointeur ; toute icône qu'il touche est sélectionnée.
    pub fn drag_lasso(&mut self, pointer: Point<f64, Logical>) {
        let Some(lasso) = self.shortcut_lasso.as_mut() else {
            return;
        };
        lasso.pointer = pointer;
        let (rect, mut selection) = (lasso.rect(), lasso.base.clone());
        if let Some(area) = self.screen_area() {
            let view = crate::render::desktop_view(self);
            for index in crate::desktop::panels::shortcuts_in_rect(area, &view, rect) {
                if let Some(item) = self.desktop_shortcuts.items.get(index) {
                    selection.insert(item.shortcut.clone());
                }
            }
        }
        self.desktop_selection = selection;
        self.request_redraw();
    }

    /// Fin du rectangle de sélection ; faux s'il n'y en avait pas.
    pub fn end_lasso(&mut self) -> bool {
        let ended = self.shortcut_lasso.take().is_some();
        if ended {
            self.request_redraw();
        }
        ended
    }

    /// Taille des icônes ; `save` : l'écrire aussi (pas à chaque pas d'un pincement). Les
    /// icônes gardent leur place relative sur l'écran (voir `keep_places`).
    pub fn set_icon_size(&mut self, size: u32, save: bool) {
        let size = Desktop::clamp_size(size);
        let changed = size != self.desktop_shortcuts.icon_size;
        if changed {
            self.keep_places(size);
        }
        self.desktop_shortcuts.icon_size = size;
        if changed || save {
            self.request_redraw();
        }
        if save {
            self.save_desktop();
        }
    }

    /// Nouvelles cases pour des icônes de `size` pixels : chacune reste à la même place
    /// relative de l'écran (en haut à droite, au milieu, en bas à gauche…) au lieu de
    /// garder son numéro de colonne et de ligne, que la grille n'a plus forcément. Calculé
    /// depuis les positions d'avant le premier changement de taille (`IconAnchor`).
    fn keep_places(&mut self, size: u32) {
        let Some(area) = self.screen_area() else {
            return;
        };
        let mut view = crate::render::desktop_view(self);
        view.dashboard.shortcut_drag = None;
        let shortcuts: Vec<Shortcut> = self
            .desktop_shortcuts
            .items
            .iter()
            .map(|item| item.shortcut.clone())
            .collect();
        let current: Vec<Option<Cell>> = self
            .desktop_shortcuts
            .items
            .iter()
            .map(|item| item.cell)
            .collect();
        let anchor = match self.icon_anchor.take() {
            Some(anchor) if anchor.shortcuts == shortcuts && anchor.cells == current => anchor,
            _ => {
                let grid = crate::desktop::panels::shortcut_grid_size(area, &view);
                IconAnchor {
                    fractions: crate::desktop::panels::shortcut_cells(area, &view)
                        .into_iter()
                        .map(|cell| cell.map(|cell| grid_fraction(cell, grid)))
                        .collect(),
                    shortcuts,
                    cells: current,
                }
            }
        };
        view.dashboard.icon_size = size;
        let grid = crate::desktop::panels::shortcut_grid_size(area, &view);
        let cells = place_fractions(&anchor.fractions, grid);
        for (item, cell) in self.desktop_shortcuts.items.iter_mut().zip(&cells) {
            item.cell = *cell;
        }
        self.icon_anchor = Some(IconAnchor { cells, ..anchor });
    }

    fn save_desktop(&self) {
        if let Err(err) = shortcuts::save(&self.desktop_shortcuts) {
            warn!(%err, "bureau non enregistré");
        }
    }

    /// Appui sur le raccourci `index` : rien ne s'ouvre avant le relâchement.
    pub fn press_shortcut(&mut self, index: usize, pointer: Point<f64, Logical>) {
        self.select_shortcut(index);
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

    /// Relâchement : ouvre le raccourci, ou le pose dans la case visée avec le reste de la
    /// sélection. Faux s'il n'y avait pas de raccourci saisi.
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
        let Some(cells) = drag
            .offset()
            .and_then(|offset| crate::desktop::panels::shortcut_drop_cells(area, &view, drag.index, offset))
        else {
            return true;
        };
        for (item, cell) in self.desktop_shortcuts.items.iter_mut().zip(cells) {
            item.cell = cell;
        }
        self.save_desktop();
        true
    }

    /// Sélection figée pour les captures (`--demo-selection`) : `groupe`, les deux
    /// premières icônes saisies et déplacées ; `rectangle`, un rectangle tiré sur le haut
    /// droit de l'écran.
    pub fn demo_selection(&mut self, mode: &str) {
        let (right, top) = self.screen_area().map_or((1280.0, 0.0), |area| {
            (f64::from(area.loc.x + area.size.w), f64::from(area.loc.y))
        });
        match mode {
            "groupe" => {
                self.toggle_shortcut(0);
                self.toggle_shortcut(1);
                self.shortcut_drag = Some(ShortcutDrag {
                    index: 0,
                    press: Point::from((0.0, 0.0)),
                    pointer: Point::from((-230.0, 140.0)),
                    moved: true,
                });
            }
            _ => {
                self.start_lasso(Point::from((right - 330.0, top + 90.0)), false);
                self.drag_lasso(Point::from((right - 40.0, top + 260.0)));
            }
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

    fn cell(column: u16, row: u16) -> Option<Cell> {
        Some(Cell { column, row })
    }

    #[test]
    fn deposer_sur_une_icone_echange_les_places() {
        let shown = [cell(0, 0), cell(0, 1), cell(0, 2)];
        // Sur une case libre : seule l'icône déplacée change.
        assert_eq!(
            drop_group(&shown, &[0], (4, 3), (8, 6)),
            vec![cell(4, 3), cell(0, 1), cell(0, 2)]
        );
        // Sur une autre icône : elles échangent leurs places.
        assert_eq!(
            drop_group(&shown, &[0], (0, 2), (8, 6)),
            vec![cell(0, 2), cell(0, 1), cell(0, 0)]
        );
    }

    #[test]
    fn un_groupe_se_deplace_ensemble_et_garde_sa_forme() {
        let shown = [cell(0, 0), cell(0, 1), cell(1, 0), cell(3, 0)];
        // Les deux premières glissent d'une colonne : la quatrième reste où elle est,
        // la troisième, recouverte, prend une place laissée libre.
        assert_eq!(
            drop_group(&shown, &[0, 1], (1, 0), (8, 6)),
            vec![cell(1, 0), cell(1, 1), cell(0, 0), cell(3, 0)]
        );
        // Contre le bord bas, le groupe s'arrête sans se déformer.
        assert_eq!(
            drop_group(&shown, &[0, 1], (0, 9), (8, 6)),
            vec![cell(0, 4), cell(0, 5), cell(1, 0), cell(3, 0)]
        );
    }

    #[test]
    fn changer_la_taille_garde_les_places() {
        let small = (12, 8);
        let shown = [cell(0, 0), cell(11, 7), cell(6, 0), None];
        let fractions: Vec<_> = shown.iter().map(|c| c.map(|c| grid_fraction(c, small))).collect();
        // Plus grandes : coin haut droit, coin bas gauche, milieu du haut.
        let big = place_fractions(&fractions, (6, 4));
        assert_eq!(big, vec![cell(0, 0), cell(5, 3), cell(3, 0), cell(0, 1)]);
        // Revenir à la taille de départ rend chaque case.
        assert_eq!(&place_fractions(&fractions, small)[..3], &shown[..3]);
        // Deux icônes voisines qui tomberaient dans la même case : la seconde à côté.
        let near = [cell(0, 0), cell(1, 0)].map(|c| c.map(|c| grid_fraction(c, small)));
        assert_eq!(place_fractions(&near, (3, 2)), vec![cell(0, 0), cell(0, 1)]);
    }
}
