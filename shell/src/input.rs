//! Traitement des entrées (clavier, souris) communes à tous les backends.

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent, KeyState,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym},
        pointer::{AxisFrame, ButtonEvent, GrabStartData, MotionEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER, Serial},
};
use tracing::info;

use crate::{
    state::Ferrite,
    wm::{BTN_LEFT, Hit, decoration::Part},
};

/// Raccourcis gérés par le compositeur lui-même (non transmis aux applications).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Ctrl+Alt+Retour arrière : quitte ferrite-shell.
    Quit,
    /// Ctrl+Alt+M : remet à l'écran la dernière fenêtre réduite (en attendant le Dock).
    RestoreMinimized,
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
                let button = ButtonEvent {
                    button: event.button_code(),
                    state: event.state(),
                    serial: SERIAL_COUNTER.next_serial(),
                    time: event.time_msec(),
                };
                self.on_button(button);
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
                if !(pressed && modifiers.ctrl && modifiers.alt) {
                    return FilterResult::Forward;
                }
                match keysym.modified_sym() {
                    Keysym::BackSpace => FilterResult::Intercept(Action::Quit),
                    Keysym::m | Keysym::M => FilterResult::Intercept(Action::RestoreMinimized),
                    _ => FilterResult::Forward,
                }
            },
        );
        match action {
            Some(Action::Quit) => {
                info!("Ctrl+Alt+Retour arrière : arrêt de ferrite-shell");
                self.loop_signal.stop();
            }
            Some(Action::RestoreMinimized) => self.restore_minimized(),
            None => {}
        }
    }

    /// Clic de souris : décorations (feux, barre de titre, bords) gérées ici, le reste
    /// transmis au client sous le pointeur.
    fn on_button(&mut self, button: ButtonEvent) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let location = pointer.current_location();

        if button.state == ButtonState::Released {
            // Un feu agit au relâchement, si le pointeur est toujours dessus.
            if let Some((window, light)) = self.pressed_light.take() {
                if let Some(Hit::Decoration(under, Part::Light(now))) = self.hit_test(location) {
                    if under == window && now == light {
                        self.activate_light(&window, light);
                    }
                }
                return;
            }
        } else if !pointer.is_grabbed() {
            match self.hit_test(location) {
                Some(Hit::Decoration(window, part)) => {
                    self.focus_window(Some(window.clone()));
                    if button.button == BTN_LEFT
                        && self.on_decoration_press(window, part, location, button.serial)
                    {
                        // Le clic est entièrement géré par ferrite-shell.
                        return;
                    }
                }
                Some(Hit::Surface(window, ..)) => self.focus_window(Some(window)),
                None => self.focus_window(None),
            }
        }

        pointer.button(self, &button);
        pointer.frame(self);
    }

    /// Appui du bouton gauche sur une décoration. Renvoie vrai si l'événement ne doit pas
    /// être transmis plus loin.
    fn on_decoration_press(
        &mut self,
        window: smithay::desktop::Window,
        part: Part,
        location: Point<f64, Logical>,
        serial: Serial,
    ) -> bool {
        let start_data = GrabStartData {
            focus: None,
            button: BTN_LEFT,
            location,
        };
        match part {
            Part::Light(light) => {
                self.pressed_light = Some((window, light));
                true
            }
            Part::Titlebar => {
                if self.titlebar_click(&window) {
                    self.toggle_maximize(&window);
                    return true;
                }
                // Le bouton est ensuite transmis à la saisie, qui finit au relâchement.
                self.start_move(window, start_data, serial);
                false
            }
            Part::Border(edges) => {
                self.start_resize(window, edges, start_data, serial);
                false
            }
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
