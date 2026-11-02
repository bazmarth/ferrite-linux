//! Traitement des entrées (clavier, souris) communes à tous les backends.

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, GestureBeginEvent, GestureEndEvent,
        GestureSwipeUpdateEvent, InputBackend, InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent,
        PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym},
        pointer::{AxisFrame, ButtonEvent, GrabStartData, MotionEvent, RelativeMotionEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER, Serial},
};
use tracing::info;

use crate::{
    desktop::panels::{self, DesktopAction, LAUNCHER_SCROLL_STEP, Popover, SectionLink, Slider},
    state::Ferrite,
    system::Change,
    wm::{BTN_LEFT, BTN_RIGHT, Hit, decoration::Part},
};

/// Raccourcis gérés par le compositeur lui-même (non transmis aux applications).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Ctrl+Alt+Retour arrière : quitte ferrite-shell.
    Quit,
    /// Ctrl+Alt+M : remet à l'écran la dernière fenêtre réduite (en attendant le Dock).
    RestoreMinimized,
    /// Ctrl+Alt+← ou → (Ctrl+Super sous GNOME, qui garde Ctrl+Alt) : page voisine.
    SwitchPage(i32),
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
    DashboardType(char),
    DashboardErase,
    DashboardEnter,
    DashboardStopEditing,
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
            InputEvent::GestureSwipeBegin { event } => self.swipe_begin(event.fingers()),
            InputEvent::GestureSwipeUpdate { event } => self.swipe_update(event.delta_x(), event.delta_y()),
            InputEvent::GestureSwipeEnd { event } => self.swipe_end(event.cancelled()),
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
                    if modifiers.ctrl && sym == Keysym::Left {
                        return FilterResult::Intercept(Action::SwitchPage(-1));
                    }
                    if modifiers.ctrl && sym == Keysym::Right {
                        return FilterResult::Intercept(Action::SwitchPage(1));
                    }
                }
                if pressed && state.dashboard_editing.is_some() {
                    match sym {
                        Keysym::Escape => return FilterResult::Intercept(Action::DashboardStopEditing),
                        Keysym::BackSpace => return FilterResult::Intercept(Action::DashboardErase),
                        Keysym::Return | Keysym::KP_Enter => {
                            return FilterResult::Intercept(Action::DashboardEnter);
                        }
                        sym => {
                            if !modifiers.ctrl && !modifiers.alt {
                                if let Some(character) =
                                    sym.key_char().filter(|character| !character.is_control())
                                {
                                    return FilterResult::Intercept(Action::DashboardType(character));
                                }
                            }
                        }
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
                    Keysym::Left => FilterResult::Intercept(Action::SwitchPage(-1)),
                    Keysym::Right => FilterResult::Intercept(Action::SwitchPage(1)),
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
            Some(Action::SwitchPage(step)) => self.go_to_neighbor_page(step),
            Some(Action::SwitchVt(vt)) => {
                if let Some(change_vt) = self.change_vt.as_mut() {
                    info!("passage au terminal virtuel {vt}");
                    change_vt(vt);
                }
            }
            Some(Action::CloseOverlay) => {
                self.popover = None;
                self.pending_power = None;
                self.overview = false;
            }
            Some(Action::LauncherType(character)) => {
                self.launcher_query.push(character);
                self.reset_launcher_view();
            }
            Some(Action::LauncherErase) => {
                self.launcher_query.pop();
                self.reset_launcher_view();
            }
            Some(Action::DashboardType(character)) => {
                if self.dashboard.type_char(
                    self.dashboard_editing,
                    character,
                    crate::desktop::widgets::now_seconds(),
                ) && let Err(error) = self.dashboard.save()
                {
                    tracing::warn!(%error, "notes du tableau de bord non enregistrées");
                }
            }
            Some(Action::DashboardErase) => {
                if self
                    .dashboard
                    .erase_char(self.dashboard_editing, crate::desktop::widgets::now_seconds())
                    && let Err(error) = self.dashboard.save()
                {
                    tracing::warn!(%error, "notes du tableau de bord non enregistrées");
                }
            }
            Some(Action::DashboardEnter) => match self.dashboard_editing {
                Some(crate::desktop::widgets::DashboardEdit::Note(_)) => {
                    if self.dashboard.type_char(
                        self.dashboard_editing,
                        '\n',
                        crate::desktop::widgets::now_seconds(),
                    ) && let Err(error) = self.dashboard.save()
                    {
                        tracing::warn!(%error, "notes du tableau de bord non enregistrées");
                    }
                }
                Some(crate::desktop::widgets::DashboardEdit::NewTask) => {
                    if self.dashboard.commit_draft()
                        && let Err(error) = self.dashboard.save()
                    {
                        tracing::warn!(%error, "tâches du tableau de bord non enregistrées");
                    }
                }
                None => {}
            },
            Some(Action::DashboardStopEditing) => {
                self.stop_dashboard_editing();
            }
            Some(Action::LauncherLaunchFirst) => {
                let first = self
                    .launcher_entries()
                    .into_iter()
                    .next()
                    .map(|entry| entry.key());
                if let Some(key) = first {
                    self.popover = None;
                    self.open_app(&key);
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

    fn place_clipboard_near_pointer(&mut self) {
        let Some(output) = self
            .space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))
        else {
            return;
        };
        let pointer = self
            .seat
            .get_pointer()
            .map(|pointer| pointer.current_location())
            .unwrap_or(Point::from((
                output.size.w as f64 / 2.0,
                output.size.h as f64 / 2.0,
            )));
        let width = 380.0_f64;
        let height = 456.0_f64;
        let x = (pointer.x - width / 2.0)
            .round()
            .clamp(8.0, (output.size.w as f64 - width - 8.0).max(8.0));
        let y = (pointer.y + 18.0)
            .round()
            .clamp(40.0, (output.size.h as f64 - height - 8.0).max(40.0));
        self.clipboard_position = Some((output.loc.x + x as i32, output.loc.y + y as i32));
    }

    fn drag_clipboard(&mut self, location: Point<f64, Logical>) {
        let Some((offset_x, offset_y)) = self.clipboard_drag_offset else {
            return;
        };
        let Some(output) = self
            .space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))
        else {
            return;
        };
        let width = 380.0_f64;
        let height = 456.0_f64;
        let x = (location.x - offset_x as f64).clamp(
            output.loc.x as f64 + 8.0,
            output.loc.x as f64 + (output.size.w as f64 - width - 8.0).max(8.0),
        );
        let y = (location.y - offset_y as f64).clamp(
            output.loc.y as f64 + 40.0,
            output.loc.y as f64 + (output.size.h as f64 - height - 8.0).max(40.0),
        );
        self.clipboard_position = Some((x.round() as i32, y.round() as i32));
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
        let previous = pointer.current_location();
        let now = std::time::Instant::now();
        let elapsed = self
            .dock_pointer_motion_at
            .replace(Some(now))
            .map(|last| now.duration_since(last));
        if let (Some(elapsed), Some(output)) = (elapsed, self.space.outputs().next())
            && let Some(output_geo) = self.space.output_geometry(output)
            && crate::desktop::dock::fast_approach(
                previous,
                location,
                (output_geo.loc.y + output_geo.size.h) as f64,
                elapsed,
            )
        {
            self.dock_fast_approach_until
                .set(Some(now + crate::desktop::dock::REVEAL_APPROACH_GRACE));
        }
        // Glissement d'un curseur des réglages rapides ou du panneau presse-papiers.
        if let Some(slider) = self.slider_drag {
            self.drag_slider(slider, location);
        }
        if self.clipboard_drag_offset.is_some() {
            self.drag_clipboard(location);
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
        if button.state == ButtonState::Released && self.clipboard_drag_offset.take().is_some() {
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
            if self.overview && button.button == BTN_LEFT {
                self.overview = false;
                let selected = self
                    .space
                    .outputs()
                    .next()
                    .and_then(|output| self.space.output_geometry(output))
                    .and_then(|output_geo| {
                        let windows: Vec<_> = self.space.elements().rev().collect();
                        let tiles = crate::wm::workspaces::overview_tiles(output_geo, windows.len());
                        windows
                            .into_iter()
                            .zip(tiles)
                            .find(|(_, tile)| tile.to_f64().contains(location))
                            .map(|(window, _)| (*window).clone())
                    });
                if let Some(window) = selected {
                    self.focus_window(Some(window));
                }
                return;
            }
            let output_geo = self
                .space
                .outputs()
                .next()
                .and_then(|output| self.space.output_geometry(output));
            // Dock : clic gauche pour ouvrir ou ramener l'application, clic droit pour son
            // menu.
            if self.pending_power.is_none()
                && let Some((_, item)) = self.dock_item_at(location)
            {
                match button.button {
                    BTN_LEFT => {
                        self.popover = None;
                        self.dock_menu = None;
                        self.activate_app(&item.key);
                    }
                    BTN_RIGHT => {
                        let entries = self.dock_menu_entries(&item);
                        if !entries.is_empty() {
                            self.dock_menu = Some((item.key, entries, location));
                            self.popover = Some(Popover::DockMenu);
                        }
                    }
                    _ => {}
                }
                return;
            }
            let desktop_action = output_geo.and_then(|output_geo| {
                panels::action_at(output_geo, location, &crate::render::desktop_view(self))
            });
            let desktop_action = desktop_action.filter(|action| {
                !matches!(
                    action,
                    DesktopAction::DashboardAddNote
                        | DesktopAction::DashboardEditNote(_)
                        | DesktopAction::DashboardAddTask
                        | DesktopAction::DashboardToggleTask(_)
                        | DesktopAction::DashboardCyclePriority(_)
                        | DesktopAction::DashboardDeleteTask(_)
                        | DesktopAction::DashboardMonth(_)
                        | DesktopAction::DashboardConsume
                        | DesktopAction::DashboardLaunch(_)
                        | DesktopAction::DashboardShortcut(_)
                ) || self.hit_test(location).is_none()
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
            if button.button == BTN_RIGHT
                && let Some(DesktopAction::DashboardCyclePriority(index)) = desktop_action
            {
                self.handle_desktop_action(DesktopAction::DashboardDeleteTask(index));
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

    /// Le lanceur revient à sa vue d'ensemble, en haut.
    fn reset_launcher_view(&mut self) {
        self.launcher_section = None;
        self.launcher_scroll = 0;
    }

    fn handle_desktop_action(&mut self, action: DesktopAction) {
        match action {
            DesktopAction::TogglePopover(popover) => {
                let opening_clipboard =
                    popover == Popover::Clipboard && self.popover != Some(Popover::Clipboard);
                self.popover = (self.popover != Some(popover)).then_some(popover);
                self.launcher_query.clear();
                self.reset_launcher_view();
                self.clipboard_selected = 0;
                self.clipboard_first = 0;
                if opening_clipboard {
                    self.place_clipboard_near_pointer();
                }
            }
            DesktopAction::ClipboardDragStart(offset_x, offset_y) => {
                self.clipboard_drag_offset = Some((offset_x, offset_y));
            }
            DesktopAction::DashboardAddNote => {
                self.stop_dashboard_editing();
                self.dashboard_editing = self.dashboard.add_note(crate::desktop::widgets::now_seconds());
                if self.dashboard_editing.is_some()
                    && let Err(error) = self.dashboard.save()
                {
                    tracing::warn!(%error, "note du tableau de bord non enregistrée");
                }
                if self.dashboard_editing.is_none() {
                    tracing::warn!("limite de notes du tableau de bord atteinte");
                }
            }
            DesktopAction::DashboardEditNote(index) => {
                let removed = self.stop_dashboard_editing();
                let index = if removed.is_some_and(|removed| removed < index) {
                    index - 1
                } else {
                    index
                };
                self.dashboard_editing = Some(crate::desktop::widgets::DashboardEdit::Note(index));
            }
            DesktopAction::DashboardAddTask => {
                self.dashboard.draft.clear();
                self.dashboard_editing = Some(crate::desktop::widgets::DashboardEdit::NewTask);
            }
            DesktopAction::DashboardToggleTask(index) => {
                if let Some(task) = self.dashboard.tasks.get_mut(index) {
                    task.done = !task.done;
                    if let Err(error) = self.dashboard.save() {
                        tracing::warn!(%error, "tâches du tableau de bord non enregistrées");
                    }
                }
            }
            DesktopAction::DashboardCyclePriority(index) => {
                if let Some(task) = self.dashboard.tasks.get_mut(index) {
                    task.priority = task.priority.next();
                    if let Err(error) = self.dashboard.save() {
                        tracing::warn!(%error, "tâches du tableau de bord non enregistrées");
                    }
                }
            }
            DesktopAction::DashboardDeleteTask(index) => {
                if index < self.dashboard.tasks.len() {
                    self.dashboard.tasks.remove(index);
                    if let Err(error) = self.dashboard.save() {
                        tracing::warn!(%error, "tâche du tableau de bord non supprimée");
                    }
                }
            }
            DesktopAction::DashboardMonth(delta) => {
                self.dashboard.month_offset = (self.dashboard.month_offset + delta).clamp(-120, 120);
            }
            DesktopAction::DashboardLaunch(app) => {
                self.open_app(&crate::desktop::apps::AppKey::Builtin(app));
            }
            DesktopAction::DashboardShortcut(index) => match index {
                0 => self.open_app(&crate::desktop::apps::AppKey::Builtin(
                    crate::desktop::apps::App::Files,
                )),
                1 => self.open_app(&crate::desktop::apps::AppKey::Builtin(
                    crate::desktop::apps::App::Terminal,
                )),
                2 | 3 => {
                    let query = if index == 2 {
                        ["firefox", "chromium", "browser", "navigateur", "web"]
                    } else {
                        ["codium", "visual studio code", "vscode", "code", "éditeur"]
                    };
                    let entry = self.launcher_entries().into_iter().find(|entry| {
                        let name = entry.name().to_lowercase();
                        query.iter().any(|term| name.contains(term))
                    });
                    if let Some(entry) = entry {
                        let key = entry.key();
                        self.open_app(&key);
                    } else {
                        tracing::warn!(shortcut = index, "application du raccourci indisponible");
                    }
                }
                4 => self.open_app(&crate::desktop::apps::AppKey::Builtin(
                    crate::desktop::apps::App::Settings,
                )),
                5 => self.open_app(&crate::desktop::apps::AppKey::Builtin(
                    crate::desktop::apps::App::Files,
                )),
                _ => {}
            },
            // Un clic ailleurs sur les widgets termine la saisie.
            DesktopAction::DashboardConsume => {
                self.stop_dashboard_editing();
            }
            DesktopAction::GoToPage(page) => self.go_to_page(page),
            DesktopAction::LaunchTile(index) => {
                let key = self
                    .launcher_entries()
                    .into_iter()
                    .nth(index)
                    .map(|entry| entry.key());
                self.popover = None;
                if let Some(key) = key {
                    self.open_app(&key);
                }
            }

            DesktopAction::LauncherLink(link) => {
                self.launcher_section = match link {
                    SectionLink::More(section) => Some(section),
                    SectionLink::Less => None,
                };
                self.launcher_scroll = 0;
            }
            DesktopAction::ClosePopover => self.popover = None,
            DesktopAction::Launch(app) => {
                self.popover = None;
                self.open_app(&crate::desktop::apps::AppKey::Builtin(app));
            }
            DesktopAction::DockMenu(index) => {
                if let Some((key, actions, _)) = self.dock_menu.clone()
                    && let Some(action) = actions.get(index)
                {
                    self.dock_menu_action(&key, *action);
                }
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
            DesktopAction::ToggleAirplane => {
                // Mode avion : toutes les radios présentes éteintes. Le quitter les rallume.
                let (wifi, bluetooth) = (self.system.wifi, self.system.bluetooth);
                let enable = wifi != Some(true) && bluetooth != Some(true);
                if wifi.is_some() {
                    self.request_system_change(Change::Wifi(enable));
                }
                if bluetooth.is_some() {
                    self.request_system_change(Change::Bluetooth(enable));
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

    fn stop_dashboard_editing(&mut self) -> Option<usize> {
        let removed = self.dashboard.finish_editing(self.dashboard_editing);
        self.dashboard_editing = None;
        if removed.is_some()
            && let Err(error) = self.dashboard.save()
        {
            tracing::warn!(%error, "note du tableau de bord non supprimée");
        }
        removed
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
            Part::Plus => {
                if crate::render::app_id(&window).is_some_and(|id| {
                    let id = id.to_lowercase();
                    id == "foot" || id == "footclient" || id.contains("terminal")
                }) {
                    self.new_terminal_tmux_window();
                }
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
        // Lanceur ouvert : la molette (ou le pavé tactile) fait défiler les rubriques.
        if self.popover == Some(Popover::Launcher) {
            let amount = event.amount(Axis::Vertical).unwrap_or_else(|| {
                event.amount_v120(Axis::Vertical).unwrap_or(0.0) / 120.0 * LAUNCHER_SCROLL_STEP
            });
            let output_geo = self
                .space
                .outputs()
                .next()
                .and_then(|output| self.space.output_geometry(output));
            if let Some(output_geo) = output_geo {
                let limit = panels::launcher_scroll_limit(output_geo, &crate::render::desktop_view(self));
                self.launcher_scroll = (self.launcher_scroll as f64 + amount).clamp(0.0, limit as f64) as u32;
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
