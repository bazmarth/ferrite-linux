//! Traitement des entrées (clavier, souris) communes à tous les backends.

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent, KeyState,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    utils::SERIAL_COUNTER,
};
use tracing::info;

use crate::state::Ferrite;

/// Raccourcis gérés par le compositeur lui-même (non transmis aux applications).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Ctrl+Alt+Retour arrière : quitte ferrite-shell.
    Quit,
}

impl Ferrite {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event } => self.on_key::<I>(event),
            InputEvent::PointerMotionAbsolute { event } => {
                let Some(output) = self.space.outputs().next() else {
                    return;
                };
                let Some(output_geo) = self.space.output_geometry(output) else {
                    return;
                };
                let location = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                let under = self.surface_under(location);
                let motion = MotionEvent {
                    location,
                    serial: SERIAL_COUNTER.next_serial(),
                    time: event.time_msec(),
                };
                pointer.motion(self, under, &motion);
                pointer.frame(self);
            }
            InputEvent::PointerButton { event } => {
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                let serial = SERIAL_COUNTER.next_serial();
                let state = event.state();
                if state == ButtonState::Pressed && !pointer.is_grabbed() {
                    // Clic : la fenêtre sous le pointeur passe au premier plan.
                    let window = self
                        .space
                        .element_under(pointer.current_location())
                        .map(|(w, _)| w.clone());
                    self.focus_window(window);
                }
                let button = ButtonEvent {
                    button: event.button_code(),
                    state,
                    serial,
                    time: event.time_msec(),
                };
                pointer.button(self, &button);
                pointer.frame(self);
            }
            InputEvent::PointerAxis { event } => self.on_axis::<I>(event),
            _ => {}
        }
    }

    fn on_key<I: InputBackend>(&mut self, event: I::KeyboardKeyEvent) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let pressed = event.state() == KeyState::Pressed;
        let action = keyboard.input::<Action, _>(
            self,
            event.key_code(),
            event.state(),
            SERIAL_COUNTER.next_serial(),
            event.time_msec(),
            |_, modifiers, keysym| {
                if pressed && modifiers.ctrl && modifiers.alt && keysym.modified_sym() == Keysym::BackSpace {
                    FilterResult::Intercept(Action::Quit)
                } else {
                    FilterResult::Forward
                }
            },
        );
        if action == Some(Action::Quit) {
            info!("Ctrl+Alt+Retour arrière : arrêt de ferrite-shell");
            self.loop_signal.stop();
        }
    }

    fn on_axis<I: InputBackend>(&mut self, event: I::PointerAxisEvent) {
        let source = event.source();
        let mut frame = AxisFrame::new(event.time_msec()).source(source);
        for axis in [Axis::Horizontal, Axis::Vertical] {
            // Molette sans valeur continue : 15 px par cran (120 unités v120).
            let amount = event
                .amount(axis)
                .unwrap_or_else(|| event.amount_v120(axis).unwrap_or(0.0) * 15.0 / 120.0);
            if amount != 0.0 {
                frame = frame.value(axis, amount);
                if let Some(discrete) = event.amount_v120(axis) {
                    frame = frame.v120(axis, discrete as i32);
                }
            }
            if source == AxisSource::Finger && event.amount(axis) == Some(0.0) {
                frame = frame.stop(axis);
            }
        }
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        pointer.axis(self, frame);
        pointer.frame(self);
    }
}
