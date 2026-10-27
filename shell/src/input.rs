//! Traitement des entrées (clavier, souris) communes à tous les backends.

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent, KeyState,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym},
        pointer::{AxisFrame, ButtonEvent, GrabStartData, MotionEvent, RelativeMotionEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER, Serial},
};
use tracing::info;

use crate::{
    desktop::{
        dock,
        panels::{self, DesktopAction, LAUNCHER_PAGE_SIZE, Popover, Slider},
    },
    state::Ferrite,
    system::Change,
    wm::{BTN_LEFT, Hit, decoration::Part},
};

/// Raccourcis gérés par le compositeur lui-même (non transmis aux applications).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Ctrl+Alt+Retour arrière : quitte ferrite-shell.
    Quit,
    /// Ctrl+Alt+M : remet à l'écran la dernière fenêtre réduite (en attendant le Dock).
    RestoreMinimized,
    /// Ctrl+Alt+F1…F12 : passe au terminal virtuel indiqué (backend DRM).
    SwitchVt(i32),
    /// Échap : ferme le menu ou la confirmation ouverte.
    CloseOverlay,
    /// Touche Super enfoncée, puis relâchée : seule, elle ouvre ou ferme le menu
    /// Applications (comme sous Windows).
    SuperDown,
    SuperUp,
    /// Super+V : historique du presse-papiers.
    ToggleClipboard,
    /// Panneau Super+V : ligne précédente ou suivante, coller, supprimer.
    ClipboardMove(isize),
    ClipboardPasteSelected,
    ClipboardDeleteSelected,
    /// Recherche dans le menu Applications ouvert.
    LauncherType(char),
    LauncherErase,
    LauncherLaunchFirst,
}

/// Symboles XKB `XF86Switch_VT_1` à `XF86Switch_VT_12`, produits par Ctrl+Alt+F1…F12.
const SWITCH_VT_FIRST: u32 = 0x1008_FE01;
const SWITCH_VT_LAST: u32 = 0x1008_FE0C;

impl Ferrite {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event } => self.on_key::<I>(event),
            InputEvent::PointerMotionAbsolute { event } => {
                // Tablette, ou souris d'une machine virtuelle (position absolue).
                let Some(output) = self.space.outputs().next() else {
                    return;
                };
                let Some(output_geo) = self.space.output_geometry(output) else {
                    return;
                };
                let location = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();
                self.move_pointer(location, None, event.time_msec());
            }
            InputEvent::PointerMotion { event } => {
                // Souris ou pavé tactile : déplacement relatif, borné à l'écran.
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                let Some(output) = self.space.outputs().next() else {
                    return;
                };
                let Some(geo) = self.space.output_geometry(output) else {
                    return;
                };
                let mut location = pointer.current_location() + event.delta();
                let (min_x, min_y) = (geo.loc.x as f64, geo.loc.y as f64);
                location.x = location.x.clamp(min_x, min_x + geo.size.w as f64 - 1.0);
                location.y = location.y.clamp(min_y, min_y + geo.size.h as f64 - 1.0);
                let relative = RelativeMotionEvent {
                    delta: event.delta(),
                    delta_unaccel: event.delta_unaccel(),
                    utime: event.time(),
                };
                self.move_pointer(location, Some(relative), event.time_msec());
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
            |state, modifiers, keysym| {
                let sym = keysym.modified_sym();
                if matches!(sym, Keysym::Super_L | Keysym::Super_R) {
                    return FilterResult::Intercept(if pressed {
                        Action::SuperDown
                    } else {
                        Action::SuperUp
                    });
                }
                if pressed && state.super_held {
                    // Super combinée à une autre touche : le menu ne s'ouvrira pas au relâcher.
                    state.super_used = true;
                    if matches!(sym, Keysym::v | Keysym::V) {
                        return FilterResult::Intercept(Action::ToggleClipboard);
                    }
                }
                // Panneau Super+V ouvert : flèches, Entrée et Suppr le pilotent.
                if pressed && state.popover == Some(Popover::Clipboard) {
                    match sym {
                        Keysym::Up => return FilterResult::Intercept(Action::ClipboardMove(-1)),
                        Keysym::Down => return FilterResult::Intercept(Action::ClipboardMove(1)),
                        Keysym::Return | Keysym::KP_Enter => {
                            return FilterResult::Intercept(Action::ClipboardPasteSelected);
                        }
                        Keysym::Delete => return FilterResult::Intercept(Action::ClipboardDeleteSelected),
                        _ => {}
                    }
                }
                if pressed && !modifiers.ctrl && !modifiers.alt {
                    if sym == Keysym::Escape && (state.popover.is_some() || state.pending_power.is_some()) {
                        return FilterResult::Intercept(Action::CloseOverlay);
                    }
                    // Menu Applications ouvert : le clavier sert à la recherche.
                    if state.popover == Some(Popover::Launcher) {
                        match keysym.modified_sym() {
                            Keysym::BackSpace => return FilterResult::Intercept(Action::LauncherErase),
                            Keysym::Return | Keysym::KP_Enter => {
                                return FilterResult::Intercept(Action::LauncherLaunchFirst);
                            }
                            sym => {
                                if let Some(character) = sym.key_char().filter(|c| !c.is_control()) {
                                    return FilterResult::Intercept(Action::LauncherType(character));
                                }
                            }
                        }
                    }
                }
                if !(pressed && modifiers.ctrl && modifiers.alt) {
                    return FilterResult::Forward;
                }
                let raw = keysym.modified_sym().raw();
                if (SWITCH_VT_FIRST..=SWITCH_VT_LAST).contains(&raw) {
                    return FilterResult::Intercept(Action::SwitchVt((raw - SWITCH_VT_FIRST + 1) as i32));
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
                self.request_exit();
                self.loop_signal.stop();
            }
            Some(Action::RestoreMinimized) => self.restore_minimized(),
            Some(Action::SwitchVt(vt)) => {
                if let Some(change_vt) = self.change_vt.as_mut() {
                    info!("passage au terminal virtuel {vt}");
                    change_vt(vt);
                }
            }
            Some(Action::CloseOverlay) => {
                self.popover = None;
                self.pending_power = None;
            }
            Some(Action::LauncherType(character)) => {
                self.launcher_query.push(character);
                self.launcher_page = 0;
            }
            Some(Action::LauncherErase) => {
                self.launcher_query.pop();
                self.launcher_page = 0;
            }
            Some(Action::LauncherLaunchFirst) => {
                let first = self.launcher_entries().into_iter().next();
                if let Some(entry) = first {
                    entry.launch(&self.client_environment());
                    self.popover = None;
                }
            }
            Some(Action::SuperDown) => {
                self.super_held = true;
                self.super_used = false;
            }
            Some(Action::SuperUp) => {
                if self.super_held && !self.super_used {
                    self.pending_power = None;
                    self.handle_desktop_action(DesktopAction::TogglePopover(Popover::Launcher));
                }
                self.super_held = false;
            }
            Some(Action::ToggleClipboard) => {
                self.pending_power = None;
                self.handle_desktop_action(DesktopAction::TogglePopover(Popover::Clipboard));
            }
            Some(Action::ClipboardMove(step)) => self.move_clipboard_selection(step),
            Some(Action::ClipboardPasteSelected) => self.paste_history(self.clipboard_selected),
            Some(Action::ClipboardDeleteSelected) => {
                self.handle_desktop_action(DesktopAction::ClipboardDelete(self.clipboard_selected));
            }
            None => {}
        }
    }

    /// Nouvelle valeur d'un curseur glissé, envoyée seulement si elle change.
    fn drag_slider(&mut self, slider: Slider, location: Point<f64, Logical>) {
        let Some(output_geo) = self
            .space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))
        else {
            return;
        };
        let view = crate::render::desktop_view(self);
        let value = panels::slider_value(output_geo, &view, slider, location);
        let (current, action) = match slider {
            Slider::Volume => (view.volume, DesktopAction::SetVolume(value)),
            Slider::Brightness => (view.brightness, DesktopAction::SetBrightness(value)),
        };
        if current != Some(value) {
            self.handle_desktop_action(action);
        }
    }

    /// Place le pointeur en `location` et prévient le client sous le pointeur.
    fn move_pointer(
        &mut self,
        location: Point<f64, Logical>,
        relative: Option<RelativeMotionEvent>,
        time: u32,
    ) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        // Glissement d'un curseur des réglages rapides : la valeur suit le pointeur.
        if let Some(slider) = self.slider_drag {
            self.drag_slider(slider, location);
        }
        let under = if self.slider_drag.is_some() {
            None
        } else {
            self.surface_under(location)
        };
        let motion = MotionEvent {
            location,
            serial: SERIAL_COUNTER.next_serial(),
            time,
        };
        pointer.motion(self, under.clone(), &motion);
        if let Some(relative) = relative {
            pointer.relative_motion(self, under, &relative);
        }
        pointer.frame(self);
    }

    /// Clic de souris : décorations (feux, barre de titre, bords) gérées ici, le reste
    /// transmis au client sous le pointeur.
    fn on_button(&mut self, button: ButtonEvent) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let location = pointer.current_location();

        if button.state == ButtonState::Released && self.slider_drag.take().is_some() {
            return;
        }
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
            let output_geo = self
                .space
                .outputs()
                .next()
                .and_then(|output| self.space.output_geometry(output));
            if button.button == BTN_LEFT
                && self.pending_power.is_none()
                && let Some(app) = output_geo
                    .filter(|geo| self.dock_shown(*geo))
                    .and_then(|geo| dock::app_at(geo, self.dock_config, location))
            {
                self.popover = None;
                app.launch(&self.client_environment());
                return;
            }
            let desktop_action = output_geo.and_then(|output_geo| {
                panels::action_at(output_geo, location, &crate::render::desktop_view(self))
            });
            if button.button == BTN_LEFT
                && let Some(action) = desktop_action
            {
                self.slider_drag = match action {
                    DesktopAction::SetVolume(_) => Some(Slider::Volume),
                    DesktopAction::SetBrightness(_) => Some(Slider::Brightness),
                    _ => None,
                };
                self.handle_desktop_action(action);
                return;
            }
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

    fn handle_desktop_action(&mut self, action: DesktopAction) {
        match action {
            DesktopAction::TogglePopover(popover) => {
                self.popover = (self.popover != Some(popover)).then_some(popover);
                self.launcher_query.clear();
                self.launcher_page = 0;
                self.clipboard_selected = 0;
                self.clipboard_first = 0;
            }
            DesktopAction::LaunchTile(index) => {
                let entry = self
                    .launcher_entries()
                    .into_iter()
                    .nth(self.launcher_page * LAUNCHER_PAGE_SIZE + index);
                if let Some(entry) = entry {
                    entry.launch(&self.client_environment());
                }
                self.popover = None;
            }
            DesktopAction::SetLauncherPage(page) => self.launcher_page = page,
            DesktopAction::ClosePopover => self.popover = None,
            DesktopAction::Launch(app) => {
                self.popover = None;
                app.launch(&self.client_environment());
            }
            DesktopAction::ToggleWifi => {
                if let Some(enabled) = self.system.wifi {
                    self.request_system_change(Change::Wifi(!enabled));
                }
            }
            DesktopAction::ToggleBluetooth => {
                if let Some(enabled) = self.system.bluetooth {
                    self.request_system_change(Change::Bluetooth(!enabled));
                }
            }
            DesktopAction::SetVolume(value) => self.request_system_change(Change::Volume(value)),
            DesktopAction::SetBrightness(value) => self.request_system_change(Change::Brightness(value)),
            DesktopAction::RequestPower(power) => {
                self.popover = None;
                self.pending_power = Some(power);
            }
            DesktopAction::CancelPower => self.pending_power = None,
            DesktopAction::ConfirmPower(power) => {
                // Le bureau se ferme quand systemd accepte l'arrêt (voir `system.rs`).
                self.pending_power = None;
                self.request_system_change(Change::Power(power));
            }
            DesktopAction::ClipboardPaste(index) => self.paste_history(index),
            DesktopAction::ClipboardPin(index) => self.clipboard.toggle_pin(index),
            DesktopAction::ClipboardDelete(index) => {
                self.clipboard.remove(index);
                self.clamp_clipboard_view();
            }
            DesktopAction::ClipboardClear => {
                self.clipboard.clear();
                self.clamp_clipboard_view();
            }
            DesktopAction::Consume => {}
        }
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
        // Panneau Super+V : la molette fait défiler la liste.
        if self.popover == Some(Popover::Clipboard) {
            let amount = event
                .amount(Axis::Vertical)
                .or_else(|| event.amount_v120(Axis::Vertical))
                .unwrap_or(0.0);
            if amount != 0.0 {
                self.scroll_clipboard(if amount > 0.0 { 1 } else { -1 });
            }
            return;
        }
        // Menu Applications ouvert : la molette tourne les pages.
        if self.popover == Some(Popover::Launcher) {
            let amount = event
                .amount(Axis::Vertical)
                .or_else(|| event.amount_v120(Axis::Vertical))
                .unwrap_or(0.0);
            let pages = self.launcher_entries().len().div_ceil(LAUNCHER_PAGE_SIZE).max(1);
            if amount > 0.0 {
                self.launcher_page = (self.launcher_page + 1).min(pages - 1);
            } else if amount < 0.0 {
                self.launcher_page = self.launcher_page.saturating_sub(1);
            }
            return;
        }
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
