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
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
            XdgToplevelSurfaceData,
        },
    },
};
use tracing::debug;

use ferrite_ui::theme::TITLEBAR_HEIGHT;

use crate::{state::Ferrite, wm};

/// Taille supposée d'une nouvelle fenêtre pour le placement (sa vraie taille n'est connue
/// qu'après son premier tampon).
const NEW_WINDOW_SIZE: (i32, i32) = (740, 350);

/// Décalage de cascade d'une fenêtre qui attend encore son premier tampon : elle sera
/// recentrée selon sa vraie taille, en gardant ce décalage.
struct PendingPlacement(Cell<Option<Point<i32, Logical>>>);

/// Géométrie d'avant l'agrandissement, gardée dans les données de la surface.
#[derive(Default)]
struct RestoreGeometry(RefCell<Option<Rectangle<i32, Logical>>>);

impl XdgShellHandler for Ferrite {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        let area = self
            .usable_area()
            .unwrap_or_else(|| Rectangle::from_size(Size::from(NEW_WINDOW_SIZE)));
        // Le cadre est placé en cascade ; le contenu est posé sous la barre de titre (la
        // place est gardée même si le client choisit ensuite ses propres décorations).
        let estimate = Size::from(NEW_WINDOW_SIZE);
        let frame = self.placement.next_position(area, estimate);
        let offset = frame - wm::centered_position(area, estimate);
        window
            .user_data()
            .insert_if_missing(|| PendingPlacement(Cell::new(Some(offset))));
        let content = frame + Point::from((0, TITLEBAR_HEIGHT as i32));
        self.space.map_element(window.clone(), content, true);
        self.focus_window(Some(window));
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        self.minimized.retain(|m| {
            m.window
                .toplevel()
                .is_some_and(|t| t.wl_surface() != surface.wl_surface())
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

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        let Some(area) = self.usable_area() else { return };
        // Une fenêtre agrandie d'emblée n'est plus à recentrer.
        if let Some(pending) = window.user_data().get::<PendingPlacement>() {
            pending.0.set(None);
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
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        let previous = with_restore_geometry(surface.wl_surface(), |saved| saved.take());
        surface.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
            state.states.unset(xdg_toplevel::State::TiledLeft);
            state.states.unset(xdg_toplevel::State::TiledRight);
            state.size = previous.map(|rect| rect.size);
        });
        surface.send_pending_configure();
        if let Some(previous) = previous {
            self.space.map_element(window, previous.loc, true);
        }
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
            pending.0.set(None);
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
    /// Zone utile du premier écran (sous la barre de menus).
    pub fn usable_area(&self) -> Option<Rectangle<i32, Logical>> {
        let output = self.space.outputs().next()?;
        self.space
            .output_geometry(output)
            .map(|geometry| wm::usable_area(geometry, self.dock_config.reserved_height()))
    }

    /// Accroche une fenêtre sur une moitié d'écran et garde sa géométrie précédente pour
    /// que le double-clic sur la barre de titre puisse la restaurer.
    pub fn snap_window_to_side(&mut self, window: &Window, side: wm::SnapSide) {
        if window.x11_surface().is_some() {
            let Some(area) = self.usable_area() else { return };
            self.x11_save_geometry(window);
            self.place_x11(
                window,
                wm::snapped_content(area, side, wm::is_server_decorated(window)),
            );
            return;
        }
        let Some(toplevel) = window.toplevel().cloned() else {
            return;
        };
        let Some(area) = self.usable_area() else { return };
        let target = wm::snapped_content(area, side, wm::is_server_decorated(window));
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
            state.states.set(match side {
                wm::SnapSide::Left => xdg_toplevel::State::TiledLeft,
                wm::SnapSide::Right => xdg_toplevel::State::TiledRight,
            });
            state.size = Some(target.size);
        });
        toplevel.send_pending_configure();
        self.space.map_element(window.clone(), target.loc, true);
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
    /// dans la zone utile (en gardant son décalage de cascade).
    pub fn place_new_window(&mut self, surface: &WlSurface) {
        let Some(window) = self.window_for_surface(surface) else {
            return;
        };
        let size = window.geometry().size;
        if size.w <= 0 || size.h <= 0 {
            return;
        }
        let Some(offset) = window
            .user_data()
            .get::<PendingPlacement>()
            .and_then(|pending| pending.0.take())
        else {
            return;
        };
        let Some(area) = self.usable_area() else { return };
        let frame_size = Size::from((size.w, size.h + TITLEBAR_HEIGHT as i32));
        let frame = wm::clamp_frame(area, wm::centered_position(area, frame_size) + offset, frame_size);
        let content = frame + Point::from((0, TITLEBAR_HEIGHT as i32));
        self.space.map_element(window, content, true);
    }
}
