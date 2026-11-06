//! `xdg_wm_base` : fenêtres (`xdg_toplevel`) et menus contextuels (`xdg_popup`).

use std::cell::{Cell, RefCell};

use smithay::{
    delegate_xdg_shell,
    desktop::{PopupKind, PopupManager, Space, Window, find_popup_root_surface, get_popup_toplevel_coords},
    input::{Seat, pointer::GrabStartData as PointerGrabStartData},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Resource,
            protocol::{wl_output, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Logical, Point, Rectangle, Serial, Size},
    wayland::{
        compositor::with_states,
        shell::xdg::{
            PopupSurface, PositionerState, SurfaceCachedState, ToplevelSurface, XdgShellHandler,
            XdgShellState, XdgToplevelSurfaceData,
        },
    },
};
use tracing::debug;

use ferrite_ui::theme::TITLEBAR_HEIGHT;

use crate::{state::Ferrite, wm};

/// Taille supposée d'une nouvelle fenêtre pour le placement (sa vraie taille n'est connue
/// qu'après son premier tampon).
const NEW_WINDOW_SIZE: (i32, i32) = (740, 350);

/// Fenêtre qui attend encore son premier tampon : elle sera centrée selon sa vraie taille,
/// sauf si elle a été agrandie entre-temps.
struct PendingPlacement {
    waiting: Cell<bool>,
    titlebar_height: i32,
    restore: RefCell<Option<crate::session::SavedWindow>>,
}

/// La fenêtre attend encore sa première configuration (et sa place).
pub(crate) fn awaiting_placement(window: &Window) -> bool {
    window
        .user_data()
        .get::<PendingPlacement>()
        .is_some_and(|pending| pending.waiting.get())
}

/// Posée au premier tampon d'une fenêtre : son apparition n'est animée qu'une fois.
#[derive(Default)]
struct Opened(Cell<bool>);

/// Géométrie d'avant l'agrandissement, gardée dans les données de la surface.
#[derive(Default)]
struct RestoreGeometry(RefCell<Option<Rectangle<i32, Logical>>>);

impl XdgShellHandler for Ferrite {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let titlebar_height = if wm::is_ferrite_app(&surface) {
            0
        } else {
            TITLEBAR_HEIGHT as i32
        };
        let area = self
            .usable_area()
            .unwrap_or_else(|| Rectangle::from_size(Size::from(NEW_WINDOW_SIZE)));
        // Taille utile annoncée à l'application, qui s'ouvre ainsi à une taille qui tient.
        surface.with_pending_state(|state| {
            state.bounds = Some(Size::from((area.size.w, (area.size.h - titlebar_height).max(1))));
        });
        let window = Window::new_wayland_window(surface);
        let restore = self.take_window_placement(&window);
        let frame_size = Size::from((NEW_WINDOW_SIZE.0, NEW_WINDOW_SIZE.1 + titlebar_height));
        let frame = wm::centered_position(area, frame_size);
        window.user_data().insert_if_missing(|| PendingPlacement {
            waiting: Cell::new(true),
            titlebar_height,
            restore: RefCell::new(restore.clone()),
        });
        let content = frame + Point::from((0, titlebar_height));
        if restore.is_none() {
            self.leave_widgets_page();
        }
        self.space.map_element(window.clone(), content, true);
        self.focus_window(Some(window));
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        self.minimized.retain(|m| {
            m.window
                .toplevel()
                .is_some_and(|t| t.wl_surface() != surface.wl_surface())
        });
        self.workspaces.retain(|parked| {
            parked
                .window
                .toplevel()
                .is_none_or(|t| t.wl_surface() != surface.wl_surface())
        });
        // Le clavier passe à la fenêtre du dessus restante.
        let next = self
            .space
            .elements()
            .rev()
            .find(|w| {
                w.toplevel()
                    .is_some_and(|t| t.wl_surface() != surface.wl_surface())
            })
            .cloned();
        self.focus_window(next);
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            debug!("menu contextuel ignoré : {err}");
        }
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some(seat) = Seat::<Ferrite>::from_resource(&seat) else {
            return;
        };
        let Some(start_data) = check_grab(&seat, surface.wl_surface(), serial) else {
            return;
        };
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        self.start_move(window, start_data, serial);
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let Some(seat) = Seat::<Ferrite>::from_resource(&seat) else {
            return;
        };
        let Some(start_data) = check_grab(&seat, surface.wl_surface(), serial) else {
            return;
        };
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        self.start_resize(window, edges.into(), start_data, serial);
    }

    /// Bouton jaune dessiné par l'application elle-même.
    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for_surface(surface.wl_surface()) {
            self.minimize(&window);
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        let target = window.clone();
        self.animate_frame_change(&window, move |state| {
            state.set_xdg_maximized(target, surface, true);
        });
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        let target = window.clone();
        self.animate_frame_change(&window, move |state| {
            state.set_xdg_maximized(target, surface, false);
        });
    }

    /// Plein écran (vidéo, jeu, présentation) : tout l'écran, sans décorations ; la barre
    /// du haut et le Dock s'effacent tant que la fenêtre est au premier plan.
    fn fullscreen_request(&mut self, surface: ToplevelSurface, _output: Option<wl_output::WlOutput>) {
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        let Some(screen) = self
            .space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))
        else {
            return;
        };
        if let Some(pending) = window.user_data().get::<PendingPlacement>() {
            pending.waiting.set(false);
        }
        if let Some(location) = self.space.element_location(&window) {
            let previous = Rectangle::new(location, window.geometry().size);
            with_restore_geometry(surface.wl_surface(), |saved| {
                if saved.is_none() {
                    *saved = Some(previous);
                }
            });
        }
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Fullscreen);
            state.size = Some(screen.size);
        });
        surface.send_pending_configure();
        self.space.map_element(window.clone(), screen.loc, true);
        self.focus_window(Some(window));
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        let previous = with_restore_geometry(surface.wl_surface(), |saved| saved.take());
        surface.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Fullscreen);
            state.size = previous.map(|rect| rect.size);
        });
        surface.send_pending_configure();
        if let Some(previous) = previous {
            self.space.map_element(window, previous.loc, true);
        }
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // Saisie exclusive des menus contextuels : à faire avec les menus (phase 3).
    }
}

delegate_xdg_shell!(Ferrite);

fn with_restore_geometry<T>(
    surface: &WlSurface,
    f: impl FnOnce(&mut Option<Rectangle<i32, Logical>>) -> T,
) -> T {
    with_states(surface, |states| {
        states.data_map.insert_if_missing(RestoreGeometry::default);
        let saved = states
            .data_map
            .get::<RestoreGeometry>()
            .expect("inséré juste avant");
        f(&mut saved.0.borrow_mut())
    })
}

/// Vérifie qu'une demande de déplacement ou de redimensionnement correspond bien à un
/// clic en cours sur ce client.
fn check_grab(
    seat: &Seat<Ferrite>,
    surface: &WlSurface,
    serial: Serial,
) -> Option<PointerGrabStartData<Ferrite>> {
    let pointer = seat.get_pointer()?;
    if !pointer.has_grab(serial) {
        return None;
    }
    let start_data = pointer.grab_start_data()?;
    let (focus, _) = start_data.focus.as_ref()?;
    if !focus.id().same_client_as(&surface.id()) {
        return None;
    }
    Some(start_data)
}

/// À appeler à chaque `wl_surface.commit` : envoie la configuration initiale attendue par
/// les fenêtres et les menus.
pub fn handle_commit(popups: &mut PopupManager, space: &Space<Window>, surface: &WlSurface) {
    if let Some(window) = space
        .elements()
        .find(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface))
    {
        let initial_configure_sent = with_states(surface, |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .is_some_and(|data| data.lock().is_ok_and(|data| data.initial_configure_sent))
        });
        if !initial_configure_sent {
            if let Some(toplevel) = window.toplevel() {
                toplevel.send_configure();
            }
        }
    }

    popups.commit(surface);
    if let Some(PopupKind::Xdg(xdg)) = popups.find_popup(surface) {
        if !xdg.is_initial_configure_sent() {
            // La configuration initiale est toujours permise.
            let _ = xdg.send_configure();
        }
    }
}

impl Ferrite {
    fn set_xdg_maximized(&mut self, window: Window, surface: ToplevelSurface, maximized: bool) {
        if maximized {
            let Some(area) = self.usable_area() else { return };
            if let Some(pending) = window.user_data().get::<PendingPlacement>() {
                pending.waiting.set(false);
            }
            let area = wm::maximized_content(area, wm::is_server_decorated(&window));
            if let Some(location) = self.space.element_location(&window) {
                let previous = Rectangle::new(location, window.geometry().size);
                with_restore_geometry(surface.wl_surface(), |saved| *saved = Some(previous));
            }
            surface.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Maximized);
                state.size = Some(area.size);
            });
            surface.send_pending_configure();
            self.space.map_element(window, area.loc, true);
        } else {
            let previous = with_restore_geometry(surface.wl_surface(), |saved| saved.take());
            surface.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Maximized);
                state.states.unset(xdg_toplevel::State::TiledLeft);
                state.states.unset(xdg_toplevel::State::TiledRight);
                state.states.unset(xdg_toplevel::State::TiledTop);
                state.states.unset(xdg_toplevel::State::TiledBottom);
                state.size = previous.map(|rect| rect.size);
            });
            surface.send_pending_configure();
            if let Some(previous) = previous {
                self.space.map_element(window, previous.loc, true);
            }
        }
    }

    /// Premier écran entier.
    pub fn screen_area(&self) -> Option<Rectangle<i32, Logical>> {
        let output = self.space.outputs().next()?;
        self.space.output_geometry(output)
    }

    /// Zone utile du premier écran (sous la barre de menus si elle ne se cache pas).
    pub fn usable_area(&self) -> Option<Rectangle<i32, Logical>> {
        let output = self.space.outputs().next()?;
        self.space.output_geometry(output).map(|geometry| {
            let area = wm::usable_area(
                geometry,
                self.dock_config.bar_reserved_height(),
                self.dock_config.reserved_height(),
            );
            // Une barre d'un autre bureau (calque à zone exclusive) prend aussi sa place. La
            // zone n'est recalculée que par `arrange` : sans cela, elle garde la taille de
            // l'écran au démarrage, et les fenêtres agrandies s'arrêtent avant le bord quand
            // l'écran (ou la fenêtre hôte) grandit ensuite.
            let zone = {
                let mut map = smithay::desktop::layer_map_for_output(output);
                map.arrange();
                map.non_exclusive_zone()
            };
            let zone = smithay::utils::Rectangle::new(zone.loc + geometry.loc, zone.size);
            area.intersection(zone).unwrap_or(area)
        })
    }

    /// Accroche une fenêtre (moitié, quart, ou tout l'écran utile) et garde sa géométrie
    /// précédente pour que le double-clic sur la barre de titre, ou un nouveau
    /// déplacement, puisse la restaurer.
    pub fn snap_window(&mut self, window: &Window, snap: wm::Snap) {
        self.animate_frame_change(window, |state| state.apply_snap(window, snap));
    }

    fn apply_snap(&mut self, window: &Window, snap: wm::Snap) {
        if snap == wm::Snap::Maximize {
            match window.toplevel().cloned() {
                Some(toplevel) => self.set_xdg_maximized(window.clone(), toplevel, true),
                None => self.x11_set_maximized(window, true),
            }
            return;
        }
        if window.x11_surface().is_some() {
            let Some(area) = self.usable_area() else { return };
            self.x11_save_geometry(window);
            self.place_x11(
                window,
                wm::snapped_content(area, snap, wm::is_server_decorated(window)),
            );
            return;
        }
        let Some(toplevel) = window.toplevel().cloned() else {
            return;
        };
        let Some(area) = self.usable_area() else { return };
        let target = wm::snapped_content(area, snap, wm::is_server_decorated(window));
        let Some(location) = self.space.element_location(window) else {
            return;
        };

        // La taille est lue avant : `window.geometry()` reprend le verrou des données de la
        // surface, déjà tenu dans `with_restore_geometry`, et figerait le compositeur.
        let previous = Rectangle::new(location, window.geometry().size);
        with_restore_geometry(toplevel.wl_surface(), |saved| {
            if saved.is_none() {
                *saved = Some(previous);
            }
        });
        toplevel.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
            state.states.unset(xdg_toplevel::State::TiledLeft);
            state.states.unset(xdg_toplevel::State::TiledRight);
            state.states.unset(xdg_toplevel::State::TiledTop);
            state.states.unset(xdg_toplevel::State::TiledBottom);
            // Côtés de la fenêtre collés au bord de l'écran ou à une voisine.
            use xdg_toplevel::State::{TiledBottom, TiledLeft, TiledRight, TiledTop};
            let [x, y, w, h] = snap.part();
            for (touches, side) in [
                (x == 0, TiledLeft),
                (x + w >= 1000, TiledRight),
                (y == 0, TiledTop),
                (y + h >= 1000, TiledBottom),
            ] {
                if touches {
                    state.states.set(side);
                }
            }
            state.size = Some(target.size);
        });
        toplevel.send_pending_configure();
        self.space.map_element(window.clone(), target.loc, true);
    }

    /// Fenêtre agrandie ou accrochée qu'on commence à déplacer : elle reprend sa taille
    /// d'avant, sous le pointeur au même endroit relatif de sa largeur, comme sous
    /// Windows. Renvoie sa nouvelle position, ou `None` si elle n'était pas accrochée.
    pub fn release_snapped(
        &mut self,
        window: &Window,
        pointer: Point<f64, Logical>,
    ) -> Option<Point<i32, Logical>> {
        let location = self.space.element_location(window)?;
        let size = window.geometry().size;
        let toplevel = match window.toplevel() {
            Some(toplevel) => toplevel.clone(),
            None => return self.x11_release_snapped(window, pointer),
        };
        use xdg_toplevel::State::{Maximized, TiledBottom, TiledLeft, TiledRight, TiledTop};
        let states = toplevel.current_state().states;
        if ![Maximized, TiledLeft, TiledRight, TiledTop, TiledBottom]
            .iter()
            .any(|state| states.contains(*state))
        {
            return None;
        }
        let previous = with_restore_geometry(toplevel.wl_surface(), |saved| saved.take())?;
        let target = wm::unsnapped_location(Rectangle::new(location, size), previous.size, pointer);
        toplevel.with_pending_state(|state| {
            state.states.unset(Maximized);
            state.states.unset(xdg_toplevel::State::TiledLeft);
            state.states.unset(xdg_toplevel::State::TiledRight);
            state.states.unset(xdg_toplevel::State::TiledTop);
            state.states.unset(xdg_toplevel::State::TiledBottom);
            state.size = Some(previous.size);
        });
        toplevel.send_pending_configure();
        self.space.map_element(window.clone(), target, true);
        Some(target)
    }

    /// Garde un menu contextuel entièrement visible à l'écran.
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self.window_for_surface(&root) else {
            return;
        };
        let Some(output) = self.space.outputs().next() else {
            return;
        };
        let Some(output_geo) = self.space.output_geometry(output) else {
            return;
        };
        let Some(window_geo) = self.space.element_geometry(&window) else {
            return;
        };

        // La géométrie cible est relative au parent du menu.
        let mut target = output_geo;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window_geo.loc;
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}

impl Ferrite {
    /// Au premier tampon d'une fenêtre, sa taille réelle est connue : elle est recentrée
    /// dans la zone utile, en conservant son placement centré, puis apparaît en
    /// s'animant, qu'elle ait été recentrée ou agrandie d'emblée.
    pub fn place_new_window(&mut self, surface: &WlSurface) {
        let Some(window) = self.window_for_surface(surface) else {
            return;
        };
        let size = window.geometry().size;
        if size.w <= 0 || size.h <= 0 {
            return;
        }
        window.user_data().insert_if_missing(Opened::default);
        if window
            .user_data()
            .get::<Opened>()
            .is_none_or(|opened| opened.0.replace(true))
        {
            return;
        }
        let placement = window.user_data().get::<PendingPlacement>().and_then(|pending| {
            pending
                .waiting
                .replace(false)
                .then(|| (pending.titlebar_height, pending.restore.borrow_mut().take()))
        });
        if let (Some((titlebar_height, restore)), Some(area)) = (placement, self.usable_area()) {
            if let Some(restore) = restore {
                self.place_restored_window(&window, surface, area, titlebar_height, restore);
            } else {
                // Plus grande que la zone utile (le bas passerait sous le Dock) : elle est
                // ramenée à une taille qui tient, comme les fenêtres X11.
                let min = with_states(surface, |states| {
                    states.cached_state.get::<SurfaceCachedState>().current().min_size
                });
                let size = match (
                    wm::fitted_size(area, size, titlebar_height, min),
                    window.toplevel(),
                ) {
                    (Some(fitted), Some(toplevel)) => {
                        toplevel.with_pending_state(|state| state.size = Some(fitted));
                        toplevel.send_pending_configure();
                        fitted
                    }
                    _ => size,
                };
                let frame_size = Size::from((size.w, size.h + titlebar_height));
                let frame = wm::clamp_frame(area, wm::centered_position(area, frame_size), frame_size);
                let content = frame + Point::from((0, titlebar_height));
                self.space.map_element(window.clone(), content, true);
            }
        }
        // En tuilage, elle apparaît directement à sa place dans la grille.
        self.retile();
        if self.space.element_location(&window).is_some() {
            self.animate_opening(&window);
        }
    }

    fn place_restored_window(
        &mut self,
        window: &Window,
        surface: &WlSurface,
        area: Rectangle<i32, Logical>,
        titlebar_height: i32,
        saved: crate::session::SavedWindow,
    ) {
        self.workspaces.ensure_page(saved.workspace);
        let content_size = Size::from((saved.width, (saved.height - titlebar_height).max(1)));
        let min = with_states(surface, |states| {
            states.cached_state.get::<SurfaceCachedState>().current().min_size
        });
        let size = match (
            wm::fitted_size(area, content_size, titlebar_height, min),
            window.toplevel(),
        ) {
            (Some(fitted), Some(toplevel)) => {
                toplevel.with_pending_state(|state| state.size = Some(fitted));
                toplevel.send_pending_configure();
                fitted
            }
            _ => content_size,
        };
        let frame_size = Size::from((size.w, size.h + titlebar_height));
        let origin = self.screen_area().map_or(area.loc, |screen| screen.loc);
        let requested = Point::from((origin.x + saved.x, origin.y + saved.y));
        let frame = wm::clamp_frame(area, requested, frame_size);
        let content = frame + Point::from((0, titlebar_height));
        self.space.map_element(window.clone(), content, true);
        if saved.minimized {
            self.space.unmap_elem(window);
            self.minimized.push(crate::wm::Minimized {
                window: window.clone(),
                location: content,
                workspace: saved.workspace,
            });
        } else if saved.workspace != self.workspaces.current() {
            self.move_window_to_page(window, saved.workspace);
        }
    }
}
