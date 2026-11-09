//! Menu d'une fenêtre : clic droit sur sa barre de titre (ou sur le haut d'une application
//! qui dessine le sien et le demande).

use smithay::{
    desktop::Window,
    utils::{IsAlive, Logical, Point},
};

use super::keep_above::is_kept_above;
use crate::{desktop::panels::Popover, state::Ferrite};

/// Choix du menu d'une fenêtre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMenuAction {
    KeepAbove,
    ReleaseAbove,
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl WindowMenuAction {
    pub fn label(self) -> &'static str {
        match self {
            WindowMenuAction::KeepAbove => "Toujours au-dessus",
            WindowMenuAction::ReleaseAbove => "Ne plus garder au-dessus",
            WindowMenuAction::Minimize => "Réduire",
            WindowMenuAction::Maximize => "Agrandir",
            WindowMenuAction::Restore => "Restaurer",
            WindowMenuAction::Close => "Fermer",
        }
    }
}

/// Entrées du menu, selon l'état de la fenêtre.
fn entries(kept_above: bool, maximized: bool) -> Vec<WindowMenuAction> {
    vec![
        if kept_above {
            WindowMenuAction::ReleaseAbove
        } else {
            WindowMenuAction::KeepAbove
        },
        WindowMenuAction::Minimize,
        if maximized {
            WindowMenuAction::Restore
        } else {
            WindowMenuAction::Maximize
        },
        WindowMenuAction::Close,
    ]
}

impl Ferrite {
    /// Ouvre le menu de `window` sous le pointeur, en `at`.
    pub fn open_window_menu(&mut self, window: Window, at: Point<f64, Logical>) {
        if self.lock.is_some() || self.overview || self.pending_power.is_some() {
            return;
        }
        let entries = entries(is_kept_above(&window), super::is_maximized(&window));
        self.dock_menu = None;
        self.dock_menu_pointer = None;
        self.window_menu = Some((window, entries, at));
        self.popover = Some(Popover::DockMenu);
        self.request_redraw();
    }

    /// Menu demandé par une application pour sa propre fenêtre : celle sous le pointeur,
    /// sinon la fenêtre active.
    pub fn open_window_menu_at_pointer(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let window = pointer
            .current_focus()
            .and_then(|surface| self.window_for_surface(&surface))
            .or_else(|| self.active_window());
        if let Some(window) = window {
            self.open_window_menu(window, pointer.current_location());
        }
    }

    /// Choix `index` dans le menu de fenêtre ouvert ; faux si ce n'est pas lui qui est ouvert.
    pub fn window_menu_action(&mut self, index: usize) -> bool {
        let Some((window, entries, _)) = self.window_menu.take() else {
            return false;
        };
        self.popover = None;
        let Some(action) = entries.get(index).filter(|_| window.alive()) else {
            return true;
        };
        match action {
            WindowMenuAction::KeepAbove => self.set_keep_above(&window, true),
            WindowMenuAction::ReleaseAbove => self.set_keep_above(&window, false),
            WindowMenuAction::Minimize => self.minimize(&window),
            WindowMenuAction::Maximize | WindowMenuAction::Restore => self.toggle_maximize(&window),
            WindowMenuAction::Close => super::close_window(&window),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{WindowMenuAction::*, entries};

    #[test]
    fn entrees_selon_l_etat() {
        assert_eq!(entries(false, false), [KeepAbove, Minimize, Maximize, Close]);
        assert_eq!(entries(true, true), [ReleaseAbove, Minimize, Restore, Close]);
    }
}
