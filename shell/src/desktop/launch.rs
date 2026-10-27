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
        dock::{DockItem, DockLayout},
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

impl Ferrite {
    /// Application d'une fenêtre ; `None` pour les menus et bulles X11.
    pub fn window_app(&self, window: &Window) -> Option<AppKey> {
        if window.x11_surface().is_some_and(|x11| x11.is_override_redirect()) {
            return None;
        }
        crate::render::app_id(window).map(|id| AppKey::for_window(&id, &self.applications))
    }

    /// Applications épinglées (le Dock par défaut si rien n'est réglé).
    pub fn pinned_apps(&self) -> Vec<AppKey> {
        match &self.dock_pinned {
            Some(tokens) => tokens
                .iter()
                .map(|token| AppKey::from_token(token, &self.applications))
                .collect(),
            None => App::DOCK.into_iter().map(AppKey::Builtin).collect(),
        }
    }

    /// Cases du Dock : épinglées, puis ouvertes non épinglées dans l'ordre d'ouverture.
    pub fn dock_items(&self) -> Vec<DockItem> {
        let pinned = self.pinned_apps();
        let windows = self
            .space
            .elements()
            .chain(self.minimized.iter().map(|minimized| &minimized.window));
        let mut running: Vec<AppKey> = Vec::new();
        for key in windows.filter_map(|window| self.window_app(window)) {
            if !running.contains(&key) {
                running.push(key);
            }
        }
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
                    .filter(|key| !pinned.contains(key))
                    .map(|key| (key.clone(), false)),
            )
            .map(|(key, pinned)| DockItem {
                icon: key.icon(&self.applications),
                running: running.contains(&key),
                key,
                pinned,
            })
            .collect()
    }

    /// Géométrie actuelle du Dock sur le premier écran.
    pub fn dock_geometry(&self) -> Option<(Rectangle<i32, Logical>, Vec<DockItem>, DockLayout)> {
        let output = self.screen_area()?;
        let items = self.dock_items();
        let pointer = self.seat.get_pointer().map(|pointer| pointer.current_location());
        let geometry = self
            .dock_motion
            .current_layout(output, self.dock_config, &items, pointer);
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

    /// L'application peut-elle ouvrir plusieurs fenêtres (Paramètres) ?
    pub fn allows_multiple_windows(&self, key: &AppKey) -> bool {
        self.multi_window.iter().any(|token| *token == key.token())
    }

    /// Ouvre une application depuis le menu Applications : si elle est déjà ouverte et
    /// n'a droit qu'à une fenêtre, sa fenêtre revient au premier plan.
    pub fn open_app(&mut self, key: &AppKey) {
        let (mapped, minimized) = self.app_windows(key);
        let running = !mapped.is_empty() || !minimized.is_empty();
        if running && !self.allows_multiple_windows(key) {
            self.activate_app(key);
        } else {
            self.launch_app(key);
        }
    }

    fn launch_app(&mut self, key: &AppKey) {
        let env = self.client_environment();
        if !key.launch(&self.applications, &env) {
            warn!(?key, "application sans commande de lancement");
        }
    }

    /// Clic sur une icône du Dock : lance l'application fermée ; sinon ramène sa fenêtre,
    /// réduite comprise ; cliquée alors qu'elle est déjà devant, elle passe à sa fenêtre
    /// suivante.
    pub fn activate_app(&mut self, key: &AppKey) {
        let (mapped, minimized) = self.app_windows(key);
        if mapped.is_empty() && minimized.is_empty() {
            self.launch_app(key);
            return;
        }
        let in_front = self
            .top_window()
            .is_some_and(|window| self.window_app(&window).as_ref() == Some(key));
        match (mapped.first(), minimized.last()) {
            // Rien de visible : la dernière fenêtre réduite revient.
            (None, Some(&index)) => self.restore_minimized_at(index),
            (Some(_), Some(&index)) if in_front => self.restore_minimized_at(index),
            (Some(_), _) if in_front && mapped.len() > 1 => {
                let last = mapped.last().cloned();
                self.focus_window(last);
            }
            (Some(top), _) => {
                let top = top.clone();
                self.focus_window(Some(top));
            }
            (None, None) => {}
        }
    }

    /// Entrées du menu d'une case du Dock.
    pub fn dock_menu_entries(&self, item: &DockItem) -> Vec<DockMenuAction> {
        let mut entries = Vec::new();
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
                let mut settings = Settings::load();
                settings.set_dock_pinned(&pinned);
                if let Err(err) = settings.save() {
                    warn!(%err, "Dock non enregistré");
                }
                self.dock_pinned = Some(pinned);
            }
            DockMenuAction::Quit => {
                let (mapped, minimized) = self.app_windows(key);
                let minimized = minimized
                    .iter()
                    .map(|&index| self.minimized[index].window.clone());
                for window in mapped.into_iter().chain(minimized).collect::<Vec<_>>() {
                    crate::wm::close_window(&window);
                }
            }
        }
    }
}
