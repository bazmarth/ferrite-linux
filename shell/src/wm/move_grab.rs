//! Déplacement d'une fenêtre à la souris (barre de titre, ou demande du client via
//! `xdg_toplevel.move`).

use smithay::{
    desktop::Window,
    input::pointer::{
        AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
        GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
        GestureSwipeUpdateEvent, GrabStartData as PointerGrabStartData, MotionEvent, PointerGrab,
        PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle},
};

use crate::state::Ferrite;

/// Bouton gauche (`BTN_LEFT` de `linux/input-event-codes.h`).
pub const BTN_LEFT: u32 = 0x110;
/// Bouton droit (`BTN_RIGHT`).
pub const BTN_RIGHT: u32 = 0x111;

pub struct MoveSurfaceGrab {
    pub start_data: PointerGrabStartData<Ferrite>,
    pub window: Window,
    pub initial_window_location: Point<i32, Logical>,
    /// Ordonnée minimale du contenu : la barre de titre ne passe pas sous la barre de menus.
    pub min_y: i32,
}

impl PointerGrab<Ferrite> for MoveSurfaceGrab {
    fn motion(
        &mut self,
        data: &mut Ferrite,
        handle: &mut PointerInnerHandle<'_, Ferrite>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        // Pendant le déplacement, aucun client n'a le focus du pointeur.
        handle.motion(data, None, event);
        let delta = event.location - self.start_data.location;
        let mut location: Point<i32, Logical> =
            (self.initial_window_location.to_f64() + delta).to_i32_round();
        location.y = location.y.max(self.min_y);
        // En tuilage, aperçu de la place échangée sous le pointeur ; sinon, de l'accrochage
        // tant que le pointeur est au bord ou dans un coin.
        data.snap_preview = if data.tiling && data.tile_order.contains(&self.window) {
            data.tile_at(event.location, &self.window).map(|(_, frame)| frame)
        } else {
            data.usable_area()
                .zip(data.screen_area())
                .and_then(|(area, screen)| {
                    super::snap_at(area, screen, event.location)
                        .filter(|_| data.dock_config.snapping)
                        .map(|snap| super::snap_zone(area, snap))
                })
        };
        if self.window.x11_surface().is_some() {
            let size = self.window.geometry().size;
            data.place_x11(&self.window, Rectangle::new(location, size));
        } else {
            data.space.map_element(self.window.clone(), location, true);
        }
    }

    fn relative_motion(
        &mut self,
        data: &mut Ferrite,
        handle: &mut PointerInnerHandle<'_, Ferrite>,
        focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }

    fn button(
        &mut self,
        data: &mut Ferrite,
        handle: &mut PointerInnerHandle<'_, Ferrite>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if !handle.current_pressed().contains(&BTN_LEFT) {
            // La position vient de `handle` : appeler `PointerHandle::current_location()` ici
            // reprendrait le verrou du pointeur, déjà tenu pendant le déplacement, et figerait
            // tout le compositeur.
            data.snap_preview = None;
            if data.drop_tiled(&self.window, handle.current_location()) {
                // Rangée par le tuilage : échangée, ou remise à sa place.
            } else if let Some((area, screen)) = data.usable_area().zip(data.screen_area()) {
                if let Some(snap) = super::snap_at(area, screen, handle.current_location())
                    .filter(|_| data.dock_config.snapping)
                {
                    data.snap_window(&self.window, snap);
                }
            }
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn axis(&mut self, data: &mut Ferrite, handle: &mut PointerInnerHandle<'_, Ferrite>, details: AxisFrame) {
        handle.axis(data, details)
    }

    fn frame(&mut self, data: &mut Ferrite, handle: &mut PointerInnerHandle<'_, Ferrite>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut Ferrite,
        handle: &mut PointerInnerHandle<'_, Ferrite>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event)
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut Ferrite,
        handle: &mut PointerInnerHandle<'_, Ferrite>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event)
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut Ferrite,
        handle: &mut PointerInnerHandle<'_, Ferrite>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event)
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut Ferrite,
        handle: &mut PointerInnerHandle<'_, Ferrite>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event)
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut Ferrite,
        handle: &mut PointerInnerHandle<'_, Ferrite>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event)
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut Ferrite,
        handle: &mut PointerInnerHandle<'_, Ferrite>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event)
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut Ferrite,
        handle: &mut PointerInnerHandle<'_, Ferrite>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event)
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut Ferrite,
        handle: &mut PointerInnerHandle<'_, Ferrite>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event)
    }

    fn start_data(&self) -> &PointerGrabStartData<Ferrite> {
        &self.start_data
    }

    fn unset(&mut self, data: &mut Ferrite) {
        data.snap_preview = None;
    }
}
