//! Sélecteur de fenêtres (Alt+Tab) et raccourcis qui agissent sur la fenêtre active :
//! fermer, réduire, aimanter, tuiler, envoyer sur un autre espace, afficher le bureau.

use smithay::{desktop::Window, utils::IsAlive, wayland::seat::WaylandFocus};

use crate::state::Ferrite;

/// Sélecteur ouvert : fenêtres de la plus récente à la plus ancienne, puis les réduites.
#[derive(Debug)]
pub struct Switcher {
    pub windows: Vec<Window>,
    pub selected: usize,
}

/// Case atteinte depuis `selected` en avançant de `step`, en revenant au début après la fin.
fn cycle(selected: usize, len: usize, step: i32) -> usize {
    if len == 0 {
        return 0;
    }
    (selected as i64 + step as i64).rem_euclid(len as i64) as usize
}

impl Ferrite {
    /// Fenêtre qui a le clavier, s'il y en a une sur la page affichée.
    pub fn active_window(&self) -> Option<Window> {
        self.space
            .elements()
            .rev()
            .find(|window| crate::wm::is_activated(window))
            .cloned()
    }

    /// Alt+Tab : ouvre le sélecteur sur la fenêtre précédente, puis passe à la suivante
    /// (ou à celle d'avant avec Maj). Il n'y a rien à choisir sans fenêtre.
    pub fn switcher_step(&mut self, step: i32) {
        if let Some(switcher) = self.switcher.as_mut() {
            switcher.windows.retain(IsAlive::alive);
            switcher.selected = cycle(switcher.selected, switcher.windows.len(), step);
            return;
        }
        let windows: Vec<Window> = self
            .space
            .elements()
            .rev()
            .filter(|window| !window.x11_surface().is_some_and(|x11| x11.is_override_redirect()))
            .cloned()
            .chain(
                self.minimized
                    .iter()
                    .rev()
                    .map(|minimized| minimized.window.clone()),
            )
            .collect();
        if windows.is_empty() {
            return;
        }
        self.close_overview();
        self.popover = None;
        let selected = cycle(0, windows.len(), step);
        self.switcher = Some(Switcher { windows, selected });
    }

    /// Alt+` : comme Alt+Tab, entre les fenêtres de l'application active.
    pub fn switcher_step_same_app(&mut self, step: i32) {
        if self.switcher.is_some() {
            self.switcher_step(step);
            return;
        }
        let Some(active) = self.active_window() else {
            return;
        };
        let key = self.window_app(&active);
        self.switcher_step(step);
        let same: Vec<Window> = self
            .switcher
            .as_ref()
            .map(|switcher| switcher.windows.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|window| self.window_app(window) == key)
            .collect();
        if let Some(switcher) = self.switcher.as_mut() {
            switcher.selected = cycle(0, same.len(), step);
            switcher.windows = same;
        }
    }

    /// Alt relâchée : la fenêtre choisie passe au premier plan.
    pub fn switcher_commit(&mut self) {
        let Some(Switcher { windows, selected }) = self.switcher.take() else {
            return;
        };
        let Some(window) = windows.get(selected).filter(|window| window.alive()).cloned() else {
            return;
        };
        let surface = window.wl_surface().map(|surface| surface.into_owned());
        match surface.and_then(|surface| self.minimized_index(&surface)) {
            Some(index) => self.restore_minimized_at(index),
            None => self.focus_window(Some(window)),
        }
    }

    /// Fenêtres de la page affichée et fenêtres réduites de la même application que `window`.
    fn windows_of_same_app(&self, window: &Window) -> Vec<Window> {
        let Some(key) = self.window_app(window) else {
            return vec![window.clone()];
        };
        self.space
            .elements()
            .chain(self.minimized.iter().map(|minimized| &minimized.window))
            .chain(self.workspaces.all_parked().map(|parked| &parked.window))
            .filter(|other| self.window_app(other).as_ref() == Some(&key))
            .cloned()
            .collect()
    }

    /// Super+Q : ferme la fenêtre active.
    pub fn close_active_window(&mut self) {
        if let Some(window) = self.active_window() {
            crate::wm::close_window(&window);
        }
    }

    /// Super+M : réduit la fenêtre active.
    pub fn minimize_active_window(&mut self) {
        if let Some(window) = self.active_window() {
            self.minimize(&window);
        }
    }

    /// Super+↑ : agrandit la fenêtre active, même aimantée, ou lui rend sa taille.
    pub fn toggle_maximize_active(&mut self) {
        let Some(window) = self.active_window() else {
            return;
        };
        self.last_snap = None;
        if crate::wm::is_tiled(&window) {
            self.snap_window(&window, crate::wm::Snap::Maximize);
        } else {
            self.animate_frame_change(&window, |state| state.toggle_maximize(&window));
        }
    }

    /// Super+↓ : rend sa taille à une fenêtre agrandie ou aimantée ; sinon, la réduit.
    pub fn restore_or_minimize_active(&mut self) {
        let Some(window) = self.active_window() else {
            return;
        };
        if crate::wm::is_maximized(&window) || crate::wm::is_tiled(&window) {
            self.animate_frame_change(&window, |state| state.toggle_maximize(&window));
        } else {
            self.minimize(&window);
        }
    }

    /// Aimante la fenêtre active.
    pub fn snap_active(&mut self, snap: crate::wm::Snap) {
        if let Some(window) = self.active_window() {
            self.last_snap = Some((window.clone(), snap));
            self.snap_window(&window, snap);
        }
    }

    /// Super+← ou → : moitié, puis deux tiers et tiers si la touche est répétée.
    pub fn snap_active_side(&mut self, left: bool) {
        let Some(window) = self.active_window() else {
            return;
        };
        let snap = match &self.last_snap {
            Some((last, snap)) if *last == window => snap.next_on_repeat(left),
            _ if left => crate::wm::Snap::Left,
            _ => crate::wm::Snap::Right,
        };
        self.snap_active(snap);
    }

    /// Super+D : réduit toutes les fenêtres, ou les remet si le bureau est déjà affiché.
    pub fn toggle_show_desktop(&mut self) {
        if self.desktop_shown.is_empty() {
            let windows: Vec<Window> = self.space.elements().cloned().collect();
            for window in &windows {
                self.minimize(window);
            }
            self.desktop_shown = windows;
            return;
        }
        for window in std::mem::take(&mut self.desktop_shown) {
            let surface = window.wl_surface().map(|surface| surface.into_owned());
            if let Some(index) = surface.and_then(|surface| self.minimized_index(&surface)) {
                self.restore_minimized_at(index);
            }
        }
    }

    /// Super+Maj+N : envoie la fenêtre active sur la page `space` (l'espace N est la page N-1).
    pub fn send_active_to_space(&mut self, space: usize) {
        if let Some(window) = self.active_window() {
            self.move_window_to_page(&window, space);
        }
    }

    /// Super+T : tuilage automatique de l'espace affiché ; arrêté, les fenêtres reprennent
    /// leur taille d'avant.
    pub fn toggle_tiling(&mut self) {
        self.tiling = !self.tiling;
        // Paramètres montre le même état.
        let mut settings = ferrite_system::config::Settings::load();
        settings.set_flag("tuilage", self.tiling);
        if let Err(error) = settings.save() {
            tracing::warn!(%error, "tuilage non enregistré");
        }
        if self.tiling {
            self.tile_order.clear();
            self.retile();
        } else {
            for window in std::mem::take(&mut self.tile_order) {
                self.toggle_maximize(&window);
            }
        }
    }

    /// Fenêtres que le tuilage range : ni dialogues, ni petites fenêtres.
    fn tilable(&self) -> Vec<Window> {
        self.space
            .elements()
            .filter(|window| {
                let dialog = window
                    .toplevel()
                    .is_some_and(|toplevel| toplevel.parent().is_some());
                !dialog && window.geometry().size.w >= 360 && !crate::wm::is_fullscreen(window)
            })
            .cloned()
            .collect()
    }

    /// À chaque tour de boucle, en tuilage : range les fenêtres si la liste a changé.
    pub fn retile(&mut self) {
        if !self.tiling {
            return;
        }
        let windows = self.tilable();
        let mut order: Vec<Window> = self
            .tile_order
            .iter()
            .filter(|window| windows.contains(window))
            .cloned()
            .collect();
        for window in windows {
            if !order.contains(&window) {
                order.push(window);
            }
        }
        if order != self.tile_order {
            self.tile_order = order;
            self.apply_tiling();
        }
    }

    fn apply_tiling(&mut self) {
        let order = self.tile_order.clone();
        for (index, part) in tile_parts(order.len()).into_iter().enumerate() {
            self.snap_window(&order[index], crate::wm::Snap::Part(part));
        }
    }

    /// Super+Entrée en tuilage : échange la fenêtre active et la principale.
    pub fn swap_with_main(&mut self) {
        let Some(active) = self.active_window() else {
            return;
        };
        if let Some(index) = self.tile_order.iter().position(|window| *window == active)
            && index > 0
        {
            self.tile_order.swap(0, index);
            self.apply_tiling();
        }
    }

    /// Super+H : réduit toutes les fenêtres de l'application active sur la page affichée.
    pub fn hide_active_app(&mut self) {
        let Some(window) = self.active_window() else {
            return;
        };
        let shown: Vec<Window> = self
            .windows_of_same_app(&window)
            .into_iter()
            .filter(|other| self.space.elements().any(|mapped| mapped == other))
            .collect();
        for other in shown {
            self.minimize(&other);
        }
    }
}

/// Parts du tuilage : la première fenêtre prend 60 % à gauche, les suivantes s'empilent à
/// droite en parts égales (en millièmes : x, y, largeur, hauteur).
fn tile_parts(count: usize) -> Vec<[u16; 4]> {
    match count {
        0 => Vec::new(),
        1 => vec![[0, 0, 1000, 1000]],
        _ => {
            let stack = (count - 1) as u16;
            let height = 1000 / stack;
            std::iter::once([0, 0, 600, 1000])
                .chain((0..stack).map(|index| {
                    let last = index + 1 == stack;
                    let h = if last { 1000 - index * height } else { height };
                    [600, index * height, 400, h]
                }))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{cycle, tile_parts};

    #[test]
    fn parts_du_tuilage() {
        assert!(tile_parts(0).is_empty());
        assert_eq!(tile_parts(1), [[0, 0, 1000, 1000]]);
        assert_eq!(tile_parts(2), [[0, 0, 600, 1000], [600, 0, 400, 1000]]);
        assert_eq!(
            tile_parts(4),
            [
                [0, 0, 600, 1000],
                [600, 0, 400, 333],
                [600, 333, 400, 333],
                [600, 666, 400, 334]
            ]
        );
    }

    #[test]
    fn le_selecteur_tourne_dans_les_deux_sens() {
        assert_eq!(cycle(0, 3, 1), 1);
        assert_eq!(cycle(2, 3, 1), 0);
        assert_eq!(cycle(0, 3, -1), 2);
        assert_eq!(cycle(0, 1, 1), 0);
        assert_eq!(cycle(4, 0, 1), 0);
    }
}
