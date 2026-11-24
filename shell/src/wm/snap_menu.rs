//! Panneau « Ancrer la fenêtre », comme sous Windows 11 : le pointeur qui s'attarde sur le
//! bouton agrandir d'une fenêtre ouvre, sous lui, quatre dispositions (moitiés, deux tiers
//! et un tiers, trois tiers, quarts). Un clic sur une case range la fenêtre à cette place ;
//! la dernière ligne l'envoie sur l'espace de travail voisin.
//!
//! Les fenêtres décorées par ferrite-shell sont suivies ici ; les applications Ferrite, qui
//! dessinent leurs boutons, annoncent le survol sur la prise de ferrite-shell.

use std::time::Duration;

use smithay::{
    desktop::Window,
    reexports::calloop::timer::{TimeoutAction, Timer},
    utils::{IsAlive, Logical, Point},
};

use super::{
    Hit, Snap,
    decoration::{self, Part, WindowButton},
};
use crate::{desktop::panels::Popover, state::Ferrite};

/// Temps de survol du bouton agrandir avant l'ouverture du panneau.
const HOVER_DELAY: Duration = Duration::from_millis(450);
/// Le survol compte tant que le pointeur reste à cette distance du centre du bouton.
const HOVER_RADIUS: f64 = 16.0;

/// Les quatre dispositions, dans l'ordre du panneau (de gauche à droite, puis de haut en
/// bas) ; chacune liste ses cases.
pub const LAYOUTS: [&[Snap]; 4] = [
    &[Snap::Left, Snap::Right],
    &[Snap::TwoThirdsLeft, Snap::ThirdRight],
    &[Snap::Third(0), Snap::Third(1), Snap::Third(2)],
    &[Snap::TopLeft, Snap::TopRight, Snap::BottomLeft, Snap::BottomRight],
];

/// Panneau ouvert pour `window`, sous le bouton dont `anchor` est le centre (coordonnées
/// globales).
#[derive(Debug, Clone)]
pub struct SnapMenu {
    pub window: Window,
    pub anchor: Point<f64, Logical>,
    /// Ouvert au survol : il se referme quand le pointeur s'en va.
    pub hover: bool,
    /// Espace de travail où l'envoyer (page), s'il y en a un.
    pub target: Option<usize>,
}

/// Espace voisin où envoyer une fenêtre de la page `current` : le suivant, sinon le
/// précédent (la page 0, celle des widgets, n'en est pas un).
pub fn neighbor_space(current: usize, pages: usize) -> Option<usize> {
    if current + 1 < pages {
        Some(current + 1)
    } else if current >= 2 {
        Some(current - 1)
    } else {
        None
    }
}

impl Ferrite {
    /// Le pointeur est sur le bouton agrandir dont le centre est `center` : le panneau
    /// s'ouvrira s'il y est encore après le délai.
    pub fn schedule_snap_menu(&mut self, window: Window, center: Point<f64, Logical>) {
        self.snap_hover_serial = self.snap_hover_serial.wrapping_add(1);
        let serial = self.snap_hover_serial;
        let inserted =
            self.loop_handle
                .insert_source(Timer::from_duration(HOVER_DELAY), move |_, _, state| {
                    let still = state
                        .pointer_location()
                        .is_some_and(|at| (at.x - center.x).hypot(at.y - center.y) <= HOVER_RADIUS);
                    let pressed = state
                        .seat
                        .get_pointer()
                        .is_some_and(|pointer| pointer.is_grabbed());
                    if serial == state.snap_hover_serial && still && !pressed && state.popover.is_none() {
                        state.open_snap_menu(window.clone(), center, true);
                    }
                    TimeoutAction::Drop
                });
        if let Err(error) = inserted {
            tracing::warn!(%error, "panneau d'ancrage indisponible");
        }
    }

    /// Une application Ferrite signale le survol de son bouton agrandir, au point (`x`, `y`)
    /// de sa fenêtre (centre du bouton, en pixels logiques).
    pub fn app_zoom_hovered(&mut self, x: f64, y: f64) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let Some(window) = pointer
            .current_focus()
            .and_then(|surface| self.window_for_surface(&surface))
        else {
            return;
        };
        let Some(location) = self.space.element_location(&window) else {
            return;
        };
        let origin = (location - window.geometry().loc).to_f64();
        self.schedule_snap_menu(window, origin + Point::from((x, y)));
    }

    /// Ouvre le panneau d'ancrage de `window` sous le bouton centré en `anchor`.
    pub fn open_snap_menu(&mut self, window: Window, anchor: Point<f64, Logical>, hover: bool) {
        if self.lock.is_some() || self.overview || self.pending_power.is_some() || !window.alive() {
            return;
        }
        let target = neighbor_space(self.workspaces.current(), self.workspaces.page_count());
        self.dock_menu = None;
        self.window_menu = None;
        self.snap_menu = Some(SnapMenu {
            window,
            anchor,
            hover,
            target,
        });
        self.popover = Some(Popover::SnapLayouts);
        self.request_redraw();
    }

    pub fn close_snap_menu(&mut self) {
        if self.popover == Some(Popover::SnapLayouts) {
            self.popover = None;
        }
        self.snap_menu = None;
        self.request_redraw();
    }

    /// Le pointeur bouge : survol du bouton agrandir d'une fenêtre décorée par
    /// ferrite-shell, et fermeture du panneau ouvert au survol quand on le quitte.
    pub fn snap_menu_track(&mut self, location: Point<f64, Logical>) {
        if self.popover == Some(Popover::SnapLayouts) {
            let screen = self.primary_size();
            let leaves = self.snap_menu.as_ref().is_some_and(|menu| {
                menu.hover
                    && !crate::desktop::panels::snap_menu_keeps(
                        screen,
                        menu.anchor,
                        menu.target.is_some(),
                        location,
                    )
            });
            if leaves {
                self.close_snap_menu();
            }
            return;
        }
        let zoom = match self.hit_test(location) {
            Some(Hit::Decoration(window, Part::Button(WindowButton::Zoom))) => Some(window),
            _ => None,
        };
        let entered = zoom
            .as_ref()
            .is_some_and(|window| self.snap_hover.as_ref() != Some(window));
        self.snap_hover = zoom.clone();
        if entered
            && let Some(window) = zoom
            && let Some(content) = self.space.element_geometry(&window)
        {
            let center = decoration::button_center(content, WindowButton::Zoom);
            self.schedule_snap_menu(window, center);
        }
    }

    /// Taille de l'écran principal, où se dessinent les panneaux.
    fn primary_size(&self) -> (f64, f64) {
        self.space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))
            .map_or((f64::MAX, f64::MAX), |geo| (geo.size.w as f64, geo.size.h as f64))
    }

    /// Case `zone` de la disposition `layout` choisie : la fenêtre y est rangée.
    pub fn snap_menu_choose(&mut self, layout: usize, zone: usize) {
        let Some(menu) = self.snap_menu.take() else {
            return;
        };
        self.close_snap_menu();
        let Some(snap) = LAYOUTS.get(layout).and_then(|zones| zones.get(zone)).copied() else {
            return;
        };
        if !menu.window.alive() {
            return;
        }
        self.focus_window(Some(menu.window.clone()));
        self.last_snap = Some((menu.window.clone(), snap));
        self.snap_window(&menu.window, snap);
    }

    /// « Envoyer vers l'espace de travail N ».
    pub fn snap_menu_send(&mut self) {
        let Some(menu) = self.snap_menu.take() else {
            return;
        };
        self.close_snap_menu();
        if let Some(page) = menu.target.filter(|_| menu.window.alive()) {
            self.workspaces.ensure_page(page);
            self.move_window_to_page(&menu.window, page);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn espace_voisin() {
        // Bureau 1 et le bureau vide du bout : on envoie sur le 2.
        assert_eq!(neighbor_space(1, 3), Some(2));
        // Dernier bureau (nombre fixe) : on revient sur le précédent.
        assert_eq!(neighbor_space(3, 4), Some(2));
        // Un seul bureau, sans voisin.
        assert_eq!(neighbor_space(1, 2), None);
    }

    #[test]
    fn dispositions_du_panneau() {
        let counts: Vec<usize> = LAYOUTS.iter().map(|zones| zones.len()).collect();
        assert_eq!(counts, [2, 2, 3, 4]);
        // Chaque disposition couvre toute la largeur, sans trou.
        for zones in LAYOUTS {
            let top: u32 = zones
                .iter()
                .map(|snap| snap.part())
                .filter(|part| part[1] == 0)
                .map(|part| u32::from(part[2]))
                .sum();
            assert!((999..=1002).contains(&top), "{zones:?}");
        }
    }
}
