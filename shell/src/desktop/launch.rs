//! Applications et fenêtres, comme sur macOS : le Dock montre les applications épinglées
//! et celles qui sont ouvertes ; cliquer une application ouverte ramène sa fenêtre (même
//! réduite) au lieu d'en ouvrir une autre. Seules les applications choisies dans
//! Paramètres (Fichiers par défaut) peuvent avoir plusieurs fenêtres.

use ferrite_system::config::Settings;
use smithay::{
    desktop::Window,
    utils::{Logical, Point, Rectangle},
};
use tracing::warn;

use crate::{
    desktop::{
        apps::{App, AppKey},
        dock::{Bounce, DockItem, DockLayout},
    },
    state::Ferrite,
};

/// Choix du menu d'une icône du Dock (clic droit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockMenuAction {
    NewWindow,
    Pin,
    Unpin,
    Quit,
}

impl DockMenuAction {
    pub fn label(self) -> &'static str {
        match self {
            DockMenuAction::NewWindow => "Nouvelle fenêtre",
            DockMenuAction::Pin => "Garder dans le Dock",
            DockMenuAction::Unpin => "Retirer du Dock",
            DockMenuAction::Quit => "Quitter",
        }
    }
}

/// Distance (px) au-delà de laquelle une icône tenue est déplacée au lieu d'être cliquée.
const DOCK_DRAG_THRESHOLD: f64 = 6.0;

/// Icône épinglée tenue au clic gauche : relâchée sur place, c'est un clic ; déplacée,
/// elle change de place parmi les épinglées, en direct, et l'ordre est enregistré au
/// relâchement.
#[derive(Debug, Clone)]
pub struct DockDrag {
    pub key: AppKey,
    pub start: Point<f64, Logical>,
    /// Place visée parmi les épinglées, une fois le seuil dépassé.
    pub target: Option<usize>,
}

/// Place d'une icône déplacée : le nombre des autres icônes épinglées dont le centre est
/// à gauche du pointeur.
fn drop_index(centers: &[f64], dragged: usize, x: f64) -> usize {
    centers
        .iter()
        .enumerate()
        .filter(|&(index, center)| index != dragged && *center < x)
        .count()
}

/// Déplace `key` à la place `target` (bornée à la fin de la liste).
fn move_to<T: PartialEq>(list: &mut Vec<T>, key: &T, target: usize) {
    if let Some(from) = list.iter().position(|item| item == key) {
        let item = list.remove(from);
        list.insert(target.min(list.len()), item);
    }
}

impl Ferrite {
    /// Application d'une fenêtre ; `None` pour les menus et bulles X11.
    pub fn window_app(&self, window: &Window) -> Option<AppKey> {
        if window.x11_surface().is_some_and(|x11| x11.is_override_redirect()) {
            return None;
        }
        crate::render::app_id(window).map(|id| AppKey::for_window(&id, &self.applications))
    }

    /// Applications épinglées (le Dock par défaut si rien n'est réglé), avec l'icône en
    /// cours de déplacement à sa place visée.
    pub fn pinned_apps(&self) -> Vec<AppKey> {
        let mut pinned: Vec<AppKey> = match &self.dock_pinned {
            Some(tokens) => tokens
                .iter()
                .map(|token| AppKey::from_token(token, &self.applications))
                .collect(),
            None => App::DOCK.into_iter().map(AppKey::Builtin).collect(),
        };
        if let Some(drag) = &self.dock_drag
            && let Some(target) = drag.target
        {
            move_to(&mut pinned, &drag.key, target);
        }
        pinned
    }

    /// Pointeur déplacé pendant qu'une icône épinglée est tenue.
    pub fn dock_drag_motion(&mut self, location: Point<f64, Logical>) {
        let Some(drag) = &self.dock_drag else {
            return;
        };
        let moved = (location.x - drag.start.x).hypot(location.y - drag.start.y);
        if drag.target.is_none() && moved < DOCK_DRAG_THRESHOLD {
            return;
        }
        let pinned = self.pinned_apps();
        let Some(dragged) = pinned.iter().position(|key| *key == drag.key) else {
            self.dock_drag = None;
            return;
        };
        let Some((_, _, geometry)) = self.dock_geometry() else {
            return;
        };
        // Les épinglées occupent les premières cases du Dock.
        let centers: Vec<f64> = geometry
            .icons
            .iter()
            .take(pinned.len())
            .map(|icon| icon.loc.x + icon.size.w / 2.0)
            .collect();
        let target = drop_index(&centers, dragged, location.x);
        if let Some(drag) = &mut self.dock_drag {
            drag.target = Some(target);
        }
    }

    /// Relâchement d'une icône tenue : enregistre le nouvel ordre, ou ouvre l'application
    /// si l'icône n'a pas bougé.
    pub fn dock_drag_release(&mut self) {
        let order = self.pinned_apps();
        let Some(drag) = self.dock_drag.take() else {
            return;
        };
        if drag.target.is_some() {
            self.save_dock_pinned(order.iter().map(AppKey::token).collect());
        } else {
            self.activate_app(&drag.key);
        }
    }

    fn save_dock_pinned(&mut self, pinned: Vec<String>) {
        let mut settings = Settings::load();
        settings.set_dock_pinned(&pinned);
        if let Err(err) = settings.save() {
            warn!(%err, "Dock non enregistré");
        }
        self.dock_pinned = Some(pinned);
    }

    /// Cases du Dock : épinglées, puis ouvertes non épinglées dans l'ordre d'ouverture.
    pub fn dock_items(&self) -> Vec<DockItem> {
        let pinned = self.pinned_apps();
        let windows = self
            .space
            .elements()
            .chain(self.minimized.iter().map(|minimized| &minimized.window))
            .chain(self.workspaces.all_parked().map(|parked| &parked.window));
        let mut running: Vec<AppKey> = Vec::new();
        let mut several: Vec<AppKey> = Vec::new();
        for key in windows.filter_map(|window| self.window_app(window)) {
            if running.contains(&key) {
                several.push(key);
            } else {
                running.push(key);
            }
        }
        // Une application qui démarre a déjà sa case et son point ; son rebond s'achève
        // quand sa fenêtre arrive.
        let now = std::time::Instant::now();
        let mut bounces = self.dock_bounces.borrow_mut();
        for (key, bounce) in bounces.iter_mut() {
            if running.contains(key) {
                bounce.settle(now);
            }
        }
        bounces.retain(|_, bounce| !bounce.over(now));
        for key in bounces.keys() {
            if !running.contains(key) {
                running.push(key.clone());
            }
        }
        drop(bounces);
        // Ordre stable : une application garde sa place tant qu'elle reste ouverte.
        let mut order = self.dock_order.borrow_mut();
        order.retain(|key| running.contains(key));
        for key in &running {
            if !order.contains(key) {
                order.push(key.clone());
            }
        }
        pinned
            .iter()
            .map(|key| (key.clone(), true))
            .chain(
                order
                    .iter()
                    .filter(|key| self.dock_config.recents && !pinned.contains(key))
                    .map(|key| (key.clone(), false)),
            )
            // La corbeille ferme toujours la rangée, après le trait.
            .chain(std::iter::once((AppKey::Builtin(App::Trash), true)))
            .map(|(key, pinned)| DockItem {
                icon: key.icon(&self.applications),
                running: running.contains(&key),
                several: several.contains(&key),
                key,
                pinned,
            })
            .collect()
    }

    /// Géométrie actuelle du Dock sur le premier écran.
    pub fn dock_geometry(&self) -> Option<(Rectangle<i32, Logical>, Vec<DockItem>, DockLayout)> {
        let output = self.screen_area()?;
        let items = self.dock_items();
        let geometry = self
            .dock_motion
            .current_layout(output, self.dock_config, &items, self.dock_pointer());
        Some((output, items, geometry))
    }

    /// Case du Dock sous `position`, si le Dock est affiché.
    pub fn dock_item_at(&self, position: Point<f64, Logical>) -> Option<(usize, DockItem)> {
        let (output, items, geometry) = self.dock_geometry()?;
        if !self.dock_shown(output) {
            return None;
        }
        let index = geometry.item_at(position)?;
        Some((index, items.get(index)?.clone()))
    }

    /// Icône de l'application `key` dans le Dock (cible d'une fenêtre réduite).
    pub fn dock_icon_rect(&self, key: Option<&AppKey>) -> Option<Rectangle<f64, Logical>> {
        let (output, items, geometry) = self.dock_geometry()?;
        let index = key.and_then(|key| items.iter().position(|item| item.key == *key));
        match index.and_then(|index| geometry.icons.get(index)) {
            Some(icon) => Some(*icon),
            // Pas d'icône : vers le milieu du bas de l'écran.
            None => Some(Rectangle::new(
                Point::from((
                    output.loc.x as f64 + output.size.w as f64 / 2.0 - 24.0,
                    (output.loc.y + output.size.h) as f64 - 60.0,
                )),
                (48.0, 48.0).into(),
            )),
        }
    }

    /// Fenêtres de l'application : visibles (de la plus haute à la plus basse) et indices
    /// des réduites.
    fn app_windows(&self, key: &AppKey) -> (Vec<Window>, Vec<usize>) {
        let mapped = self
            .space
            .elements()
            .rev()
            .filter(|window| self.window_app(window).as_ref() == Some(key))
            .cloned()
            .collect();
        let minimized = self
            .minimized
            .iter()
            .enumerate()
            .filter(|(_, minimized)| self.window_app(&minimized.window).as_ref() == Some(key))
            .map(|(index, _)| index)
            .collect();
        (mapped, minimized)
    }

    /// Bureau, autre que celui affiché, où l'application a une fenêtre.
    fn app_parked_page(&self, key: &AppKey) -> Option<usize> {
        self.workspaces
            .page_of(|parked| self.window_app(&parked.window).as_ref() == Some(key))
    }

    /// L'application peut-elle ouvrir plusieurs fenêtres ? Le Terminal toujours, les autres
    /// selon Paramètres.
    pub fn allows_multiple_windows(&self, key: &AppKey) -> bool {
        *key == AppKey::Builtin(crate::desktop::apps::App::Terminal)
            || self.multi_window.iter().any(|token| *token == key.token())
    }

    /// Ouvre une application depuis le menu Applications : si elle est déjà ouverte et
    /// n'a droit qu'à une fenêtre, sa fenêtre revient au premier plan.
    pub fn open_app(&mut self, key: &AppKey) {
        let (mapped, minimized) = self.app_windows(key);
        let running = !mapped.is_empty() || !minimized.is_empty() || self.app_parked_page(key).is_some();
        if running && !self.allows_multiple_windows(key) {
            self.activate_app(key);
        } else {
            self.launch_app(key);
        }
    }

    pub fn launch_app(&mut self, key: &AppKey) {
        let env = self.client_environment();
        if key.launch(&self.applications, &env) {
            self.start_bounce(key);
            self.note_recent_app(key);
        } else {
            warn!(?key, "application sans commande de lancement");
        }
    }

    /// L'icône de l'application rebondit jusqu'à ce que sa fenêtre apparaisse.
    pub fn start_bounce(&mut self, key: &AppKey) {
        self.dock_bounces
            .borrow_mut()
            .entry(key.clone())
            .or_insert_with(|| Bounce::new(std::time::Instant::now()));
    }

    /// Fait venir l'application au premier plan (fenêtre devant, ou réduites restaurées),
    /// sans jamais la réduire : survol de son icône pendant un glisser de fichiers.
    pub fn bring_app_forward(&mut self, key: &AppKey) {
        let (mapped, minimized) = self.app_windows(key);
        if let Some(top) = mapped.first() {
            self.focus_window(Some(top.clone()));
        } else if let Some(&newest) = minimized.last() {
            let last = self.minimized[newest].window.clone();
            for &index in minimized.iter().rev() {
                self.restore_minimized_at(index);
            }
            self.focus_window(Some(last));
        } else if let Some(page) = self.app_parked_page(key) {
            self.go_to_page(page);
        }
    }

    /// Hauteur de rebond de chaque case du Dock (voir [`Bounce`]).
    pub fn dock_bounce_heights(&self, items: &[DockItem]) -> Vec<f64> {
        let now = std::time::Instant::now();
        let bounces = self.dock_bounces.borrow();
        items
            .iter()
            .map(|item| bounces.get(&item.key).map_or(0.0, |bounce| bounce.height(now)))
            .collect()
    }

    /// Clic sur une icône du Dock : lance l'application absente, ramène son application
    /// au premier plan si elle est ouverte ailleurs, restaure toutes ses fenêtres si elles
    /// sont réduites, ou réduit sa fenêtre active si elle est déjà au premier plan.
    pub fn activate_app(&mut self, key: &AppKey) {
        let (mapped, minimized) = self.app_windows(key);
        // Sa fenêtre est sur un autre bureau : on y va, comme sur macOS.
        if mapped.is_empty()
            && let Some(page) = self.app_parked_page(key)
        {
            self.go_to_page(page);
            let (mapped, _) = self.app_windows(key);
            if let Some(top) = mapped.first() {
                self.focus_window(Some(top.clone()));
            }
            return;
        }
        if mapped.is_empty() && minimized.is_empty() {
            self.launch_app(key);
            return;
        }

        if let Some(top) = self.top_window()
            && self.window_app(&top).as_ref() == Some(key)
        {
            // Comme sur macOS, un deuxième appui sur l'application active la réduit.
            self.minimize(&top);
            return;
        }

        if let Some(top) = mapped.first() {
            self.focus_window(Some(top.clone()));
            return;
        }

        // Toutes ses fenêtres reviennent, la dernière réduite au premier plan. Des
        // indices les plus grands aux plus petits, pour que chaque retrait laisse les
        // suivants valides.
        let Some(last) = minimized
            .last()
            .map(|&index| self.minimized[index].window.clone())
        else {
            return;
        };
        for &index in minimized.iter().rev() {
            self.restore_minimized_at(index);
        }
        self.focus_window(Some(last));
    }

    /// Entrées du menu d'une case du Dock.
    pub fn dock_menu_entries(&self, item: &DockItem) -> Vec<DockMenuAction> {
        let mut entries = Vec::new();
        if item.key == AppKey::Builtin(App::Trash) {
            return entries;
        }
        let launchable = !matches!(item.key, AppKey::Other(_));
        if launchable && item.running && self.allows_multiple_windows(&item.key) {
            entries.push(DockMenuAction::NewWindow);
        }
        if launchable {
            entries.push(if item.pinned {
                DockMenuAction::Unpin
            } else {
                DockMenuAction::Pin
            });
        }
        if item.running {
            entries.push(DockMenuAction::Quit);
        }
        entries
    }

    /// Choix dans le menu d'une case du Dock.
    pub fn dock_menu_action(&mut self, key: &AppKey, action: DockMenuAction) {
        self.popover = None;
        self.dock_menu = None;
        match action {
            DockMenuAction::NewWindow => self.launch_app(key),
            DockMenuAction::Pin | DockMenuAction::Unpin => {
                let mut pinned: Vec<String> = self.pinned_apps().iter().map(AppKey::token).collect();
                let token = key.token();
                if action == DockMenuAction::Pin {
                    if !pinned.contains(&token) {
                        pinned.push(token);
                    }
                } else {
                    pinned.retain(|known| *known != token);
                }
                self.save_dock_pinned(pinned);
            }
            DockMenuAction::Quit => {
                let (mapped, minimized) = self.app_windows(key);
                let minimized = minimized
                    .iter()
                    .map(|&index| self.minimized[index].window.clone());
                let parked = self
                    .workspaces
                    .all_parked()
                    .filter(|parked| self.window_app(&parked.window).as_ref() == Some(key))
                    .map(|parked| parked.window.clone());
                for window in mapped
                    .into_iter()
                    .chain(minimized)
                    .chain(parked)
                    .collect::<Vec<_>>()
                {
                    crate::wm::close_window(&window);
                }
            }
        }
    }
}

/// Applications récemment utilisées, gardées pour le mini-Dock des widgets : une par
/// ligne, la plus récente d'abord.
fn recent_apps_path() -> std::path::PathBuf {
    ferrite_system::config::Settings::path().with_file_name("applications-recentes")
}

/// Nombre d'applications retenues.
const RECENT_APPS: usize = 8;

pub fn load_recent_apps() -> Vec<String> {
    std::fs::read_to_string(recent_apps_path())
        .map(|text| {
            text.lines()
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .take(RECENT_APPS)
                .collect()
        })
        .unwrap_or_default()
}

impl Ferrite {
    /// L'application vient d'être utilisée (fenêtre au premier plan ou lancement).
    pub fn note_recent_app(&mut self, key: &AppKey) {
        if matches!(key, AppKey::Other(_) | AppKey::Builtin(App::Trash)) {
            return;
        }
        let token = key.token();
        if self.recent_apps.first() == Some(&token) {
            return;
        }
        self.recent_apps.retain(|known| *known != token);
        self.recent_apps.insert(0, token);
        self.recent_apps.truncate(RECENT_APPS);
        let path = recent_apps_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(err) = std::fs::write(&path, self.recent_apps.join("\n")) {
            warn!(%err, "applications récentes non enregistrées");
        }
    }

    /// Applications du mini-Dock, encore installées.
    pub fn recent_app_keys(&self) -> Vec<AppKey> {
        self.recent_apps
            .iter()
            .map(|token| AppKey::from_token(token, &self.applications))
            .filter(|key| !matches!(key, AppKey::Other(_)))
            .collect()
    }

    /// Clic sur une application du mini-Dock : elle vient au premier plan, ou démarre.
    pub fn open_recent_app(&mut self, index: usize) {
        if let Some(key) = self.recent_app_keys().get(index).cloned() {
            let (mapped, minimized) = self.app_windows(&key);
            if mapped.is_empty() && minimized.is_empty() && self.app_parked_page(&key).is_none() {
                self.launch_app(&key);
            } else {
                self.bring_app_forward(&key);
            }
        }
    }

    /// L'application a-t-elle une fenêtre, visible ou réduite ?
    pub fn app_running(&self, key: &AppKey) -> bool {
        let (mapped, minimized) = self.app_windows(key);
        !mapped.is_empty() || !minimized.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_icone_deplacee_prend_la_place_sous_le_pointeur() {
        // Cinq icônes de 60 px, centres à 30, 90, 150, 210, 270 ; la deuxième est tenue.
        let centers = [30.0, 90.0, 150.0, 210.0, 270.0];
        assert_eq!(drop_index(&centers, 1, 10.0), 0);
        assert_eq!(drop_index(&centers, 1, 95.0), 1);
        assert_eq!(drop_index(&centers, 1, 160.0), 2);
        assert_eq!(drop_index(&centers, 1, 400.0), 4);

        let mut list = vec!['a', 'b', 'c', 'd', 'e'];
        move_to(&mut list, &'b', drop_index(&centers, 1, 160.0));
        assert_eq!(list, ['a', 'c', 'b', 'd', 'e']);
        move_to(&mut list, &'e', 0);
        assert_eq!(list, ['e', 'a', 'c', 'b', 'd']);
        // Place hors de la liste : à la fin ; clé absente : rien ne change.
        move_to(&mut list, &'e', 99);
        assert_eq!(list, ['a', 'c', 'b', 'd', 'e']);
        move_to(&mut list, &'z', 0);
        assert_eq!(list, ['a', 'c', 'b', 'd', 'e']);
    }
}
