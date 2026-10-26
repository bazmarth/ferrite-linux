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
    utils::{Logical, Point},
};

use crate::state::Ferrite;

/// Bouton gauche (`BTN_LEFT` de `linux/input-event-codes.h`).
pub const BTN_LEFT: u32 = 0x110;

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
        data.space.map_element(self.window.clone(), location, true);
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
            if let (Some(pointer), Some(area)) = (data.seat.get_pointer(), data.usable_area()) {
                if let Some(side) = super::snap_side_at(area, pointer.current_location()) {
                    data.snap_window_to_side(&self.window, side);
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

    fn unset(&mut self, _data: &mut Ferrite) {}
}
