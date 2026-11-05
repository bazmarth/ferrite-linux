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
use std::time::Duration;

use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use tracing::info;

use crate::{
    desktop::panels::{self, DesktopAction, LAUNCHER_SCROLL_STEP, LockAction, Popover, Slider},
    state::Ferrite,
    system::Change,
    wm::{BTN_LEFT, BTN_RIGHT, Hit, decoration::Part},
};

/// Widget en train d'être déplacé ou redimensionné : ce qu'il était à l'appui.
#[derive(Debug, Clone, Copy)]
pub struct WidgetDrag {
    pub index: usize,
    pub edges: crate::desktop::widgets::WidgetEdges,
    pub pointer: Point<f64, smithay::utils::Logical>,
    pub start: crate::desktop::widgets::WidgetSlot,
}

/// Dialogue d'arrêt : l'action choisie se fait seule au bout de ce délai (maquette).
const POWER_COUNTDOWN: std::time::Duration = std::time::Duration::from_secs(60);

/// Molette à accumuler pour changer de mois ou faire défiler les tâches d'une ligne.
const WIDGET_SCROLL_STEP: f64 = 12.0;

/// Raccourcis gérés par le compositeur lui-même (non transmis aux applications).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    /// Ctrl+Alt+Retour arrière : quitte ferrite-shell.
    Quit,
    /// Ctrl+Alt+M : remet à l'écran la dernière fenêtre réduite (en attendant le Dock).
    RestoreMinimized,
    /// Ctrl+Alt+← ou → (Ctrl+Super sous GNOME, qui garde Ctrl+Alt) : page voisine.
    SwitchPage(i32),
    /// Ctrl+Alt+↑ ou ↓ (Ctrl+Super sous GNOME) : ouvre ou ferme la vue d'ensemble, comme le
    /// balayage à trois doigts vers le haut ou le bas.
    Overview(bool),
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
    /// Panneau Super+V : ligne précédente ou suivante, coller, supprimer ; recherche
    /// (caractère tapé, ou `None` pour effacer le dernier), filtre suivant, coller la
    /// n-ième ligne (chiffres 1 à 9).
    ClipboardMove(isize),
    ClipboardPasteSelected,
    ClipboardDeleteSelected,
    ClipboardType(Option<char>),
    ClipboardClearQuery,
    ClipboardNextFilter,
    ClipboardPasteNth(usize),
    /// Recherche dans le menu Applications ouvert.
    LauncherType(char),
    LauncherErase,
    LauncherLaunchFirst,
    DashboardType(char),
    DashboardErase,
    DashboardEnter,
    DashboardStopEditing,
    /// Alt+Tab, Alt+Maj+Tab ou flèches : avance ou recule dans le sélecteur de fenêtres.
    SwitcherStep(i32),
    SwitcherCancel,
    /// Alt+` : fenêtres de l'application active.
    SwitcherSameApp(i32),
    /// Super+Q, Super+M et Super+H : fenêtre ou application active.
    CloseWindow,
    MinimizeWindow,
    HideApp,
    /// Super+↑ et Super+↓.
    Maximize,
    RestoreOrMinimize,
    /// Super+← ou → : moitié, deux tiers, puis tiers.
    SnapSide(bool),
    /// Super+Alt+1, 2, 3 et Super+C.
    Snap(crate::wm::Snap),
    /// Super+T : tuilage automatique.
    ToggleTiling,
    /// Super+Entrée : échange avec la principale en tuilage, nouveau terminal sinon.
    SuperEnter,
    /// Ctrl+Alt+T : nouveau terminal.
    OpenTerminal,
    /// Super+D : afficher le bureau.
    ShowDesktop,
    /// Super+S : vue d'ensemble.
    ToggleOverview,
    /// Super+A : centre de contrôle.
    ToggleControls,
    /// Super+L : verrouiller.
    Lock,
    /// Super+Maj+1…9 : envoie la fenêtre active sur cet espace.
    SendToSpace(usize),
    /// Mot de passe Wi-Fi demandé dans le centre de contrôle.
    PasswordType(char),
    PasswordErase,
    PasswordSubmit,
    PasswordCancel,
    /// Écran de verrouillage : caractère, effacer, valider, ou autre touche.
    LockType(char),
    LockErase,
    /// Suppr, et déplacement du curseur (voir `lock_move_cursor`).
    LockDelete,
    LockMove(isize),
    LockSubmit,
    LockReveal,
    /// Touche relâchée pendant le verrouillage ou l'authentification, ou touche sans
    /// effet dans celle-ci : rien à faire, rien ne passe aux clients.
    LockIgnore,
    /// Fenêtre d'authentification (polkit) : caractère, effacer, valider, annuler.
    AuthType(char),
    AuthErase,
    AuthSubmit,
    AuthCancel,
    LockCancel,
    ToggleWidgets,
    WidgetsDismiss,
    /// Super+Espace : menu Applications et sa recherche.
    ToggleLauncher,
    /// Super+N : historique des notifications.
    ToggleNotifications,
    /// Impr. écran : tout l'écran.
    ScreenshotFull,
    /// Maj+Impr. écran : zone choisie à la souris.
    ScreenshotRegion,
    /// Super+Échap : dialogue d'arrêt ; dans le dialogue, Entrée et ← →.
    PowerDialog,
    PowerConfirm,
    PowerStep(i32),
    /// Touche de fonction : son, micro, luminosité, mode avion, calculatrice…
    Function(crate::desktop::function_keys::FunctionKey),
}

impl Action {
    /// Saisie et effacement dans les champs du bureau : maintenue, la touche se répète.
    fn repeats(self) -> bool {
        matches!(
            self,
            Action::LauncherType(_)
                | Action::LauncherErase
                | Action::DashboardType(_)
                | Action::DashboardErase
                | Action::PasswordType(_)
                | Action::PasswordErase
                | Action::LockType(_)
                | Action::LockErase
                | Action::AuthType(_)
                | Action::AuthErase
                | Action::LockDelete
                | Action::LockMove(_)
                | Action::ClipboardType(_)
                | Action::ClipboardMove(_)
        ) || matches!(self, Action::Function(key) if key.repeats())
    }
}

/// Codes de touche XKB (indépendants de la disposition) : rangée 1 à 9, et la touche à gauche
/// du 1 (`² ` en AZERTY, `` ` `` en QWERTY).
const KEY_DIGIT_1: u32 = 10;
const KEY_DIGIT_9: u32 = 18;
const KEY_GRAVE: u32 = 49;

/// Symboles XKB `XF86Switch_VT_1` à `XF86Switch_VT_12`, produits par Ctrl+Alt+F1…F12.
const SWITCH_VT_FIRST: u32 = 0x1008_FE01;
const SWITCH_VT_LAST: u32 = 0x1008_FE0C;

fn scroll_direction(source: AxisSource, inverted: bool) -> i32 {
    if inverted && matches!(source, AxisSource::Wheel | AxisSource::WheelTilt) {
        -1
    } else {
        1
    }
}

impl Ferrite {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        // Toute entrée est une activité (voir `protocols::idle`).
        self.idle_notifier_state.notify_activity(&self.seat);
        self.idle.activity();
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
                let code = keysym.raw_code().raw();
                // Super doit retomber même quand l'écran verrouillé garde ses touches (Super+L,
                // puis relâchée) : sinon, au retour, chaque touche compterait comme un
                // raccourci Super (L reverrouille, Q ferme la fenêtre…). Et une touche frappée
                // sans Super enfoncée prouve qu'elle n'est plus tenue (relâcher perdu).
                if (!pressed
                    && matches!(sym, Keysym::Super_L | Keysym::Super_R)
                    && (state.lock.is_some() || state.session_lock.is_some()))
                    || (pressed && !modifiers.logo && !matches!(sym, Keysym::Super_L | Keysym::Super_R))
                {
                    state.super_held = false;
                }
                // Verrouillé par un client : tout va à lui, sauf le changement de terminal.
                if state.session_lock.is_some() {
                    let raw = sym.raw();
                    if pressed
                        && modifiers.ctrl
                        && modifiers.alt
                        && (SWITCH_VT_FIRST..=SWITCH_VT_LAST).contains(&raw)
                    {
                        return FilterResult::Intercept(Action::SwitchVt((raw - SWITCH_VT_FIRST + 1) as i32));
                    }
                    return FilterResult::Forward;
                }
                // Touches de fonction : réglages même verrouillé, applications sinon.
                if pressed
                    && let Some(key) = crate::desktop::function_keys::FunctionKey::from_keysym(sym)
                    && (state.lock.is_none() || key.works_locked())
                {
                    return FilterResult::Intercept(Action::Function(key));
                }
                // Dialogue d'arrêt : Entrée confirme, Échap annule, ← et → changent d'action.
                if pressed && state.pending_power.is_some() {
                    match sym {
                        Keysym::Return | Keysym::KP_Enter => {
                            return FilterResult::Intercept(Action::PowerConfirm);
                        }
                        Keysym::Left => return FilterResult::Intercept(Action::PowerStep(-1)),
                        Keysym::Right => return FilterResult::Intercept(Action::PowerStep(1)),
                        Keysym::Escape => return FilterResult::Intercept(Action::CloseOverlay),
                        _ => {}
                    }
                }
                // Verrouillé : tout reste ici, sauf le changement de terminal virtuel.
                if state.lock.is_some() {
                    let raw = sym.raw();
                    if pressed
                        && modifiers.ctrl
                        && modifiers.alt
                        && (SWITCH_VT_FIRST..=SWITCH_VT_LAST).contains(&raw)
                    {
                        return FilterResult::Intercept(Action::SwitchVt((raw - SWITCH_VT_FIRST + 1) as i32));
                    }
                    if !pressed {
                        return FilterResult::Intercept(Action::LockIgnore);
                    }
                    return FilterResult::Intercept(match sym {
                        Keysym::Escape => Action::LockCancel,
                        Keysym::BackSpace => Action::LockErase,
                        Keysym::Delete | Keysym::KP_Delete => Action::LockDelete,
                        Keysym::Left | Keysym::KP_Left => Action::LockMove(-1),
                        Keysym::Right | Keysym::KP_Right => Action::LockMove(1),
                        Keysym::Home | Keysym::KP_Home => Action::LockMove(isize::MIN),
                        Keysym::End | Keysym::KP_End => Action::LockMove(isize::MAX),
                        Keysym::Return | Keysym::KP_Enter => Action::LockSubmit,
                        sym => match sym.key_char().filter(|c| !c.is_control()) {
                            Some(character) if !modifiers.ctrl && !modifiers.alt => {
                                Action::LockType(character)
                            }
                            _ => Action::LockReveal,
                        },
                    });
                }
                // Mot de passe administrateur demandé : la fenêtre garde le clavier.
                if !state.auth.is_empty() {
                    if !pressed {
                        return FilterResult::Intercept(Action::LockIgnore);
                    }
                    return FilterResult::Intercept(match sym {
                        Keysym::Escape => Action::AuthCancel,
                        Keysym::BackSpace => Action::AuthErase,
                        Keysym::Return | Keysym::KP_Enter => Action::AuthSubmit,
                        sym => match sym.key_char().filter(|c| !c.is_control()) {
                            Some(character) if !modifiers.ctrl && !modifiers.alt && !modifiers.logo => {
                                Action::AuthType(character)
                            }
                            _ => Action::LockIgnore,
                        },
                    });
                }
                if pressed && state.controls.password.is_some() {
                    match sym {
                        Keysym::Escape => return FilterResult::Intercept(Action::PasswordCancel),
                        Keysym::BackSpace => return FilterResult::Intercept(Action::PasswordErase),
                        Keysym::Return | Keysym::KP_Enter => {
                            return FilterResult::Intercept(Action::PasswordSubmit);
                        }
                        sym => {
                            if !modifiers.ctrl
                                && !modifiers.alt
                                && let Some(character) = sym.key_char().filter(|c| !c.is_control())
                            {
                                return FilterResult::Intercept(Action::PasswordType(character));
                            }
                        }
                    }
                }
                if matches!(sym, Keysym::Super_L | Keysym::Super_R) {
                    return FilterResult::Intercept(if pressed {
                        Action::SuperDown
                    } else {
                        Action::SuperUp
                    });
                }
                if pressed && state.switcher.is_some() {
                    match sym {
                        Keysym::Escape => return FilterResult::Intercept(Action::SwitcherCancel),
                        Keysym::Left => return FilterResult::Intercept(Action::SwitcherStep(-1)),
                        Keysym::Right => return FilterResult::Intercept(Action::SwitcherStep(1)),
                        _ => {}
                    }
                }
                if pressed && modifiers.alt && !modifiers.ctrl && code == KEY_GRAVE {
                    return FilterResult::Intercept(Action::SwitcherSameApp(if modifiers.shift {
                        -1
                    } else {
                        1
                    }));
                }
                if pressed && modifiers.ctrl && modifiers.alt && matches!(sym, Keysym::t | Keysym::T) {
                    return FilterResult::Intercept(Action::OpenTerminal);
                }
                if pressed && modifiers.alt && !modifiers.ctrl {
                    match sym {
                        Keysym::Tab if modifiers.shift => {
                            return FilterResult::Intercept(Action::SwitcherStep(-1));
                        }
                        Keysym::ISO_Left_Tab => return FilterResult::Intercept(Action::SwitcherStep(-1)),
                        Keysym::Tab => return FilterResult::Intercept(Action::SwitcherStep(1)),
                        _ => {}
                    }
                }
                // Touche de capture d'une zone (Lenovo) : comme Maj+Impr. écran.
                if pressed && sym == Keysym::XF86_SelectiveScreenshot {
                    return FilterResult::Intercept(Action::ScreenshotRegion);
                }
                if pressed && sym == Keysym::Print {
                    return FilterResult::Intercept(if modifiers.shift {
                        Action::ScreenshotRegion
                    } else {
                        Action::ScreenshotFull
                    });
                }
                if pressed && sym == Keysym::Escape && state.region_select.is_some() {
                    return FilterResult::Intercept(Action::CloseOverlay);
                }
                if pressed && state.super_held {
                    // Super combinée à une autre touche : le menu ne s'ouvrira pas au relâcher.
                    state.super_used = true;
                    // Super+Maj+1…9 et Super+Alt+1, 2, 3 : par code de touche, car le symbole
                    // dépend de la disposition et de Maj.
                    if (KEY_DIGIT_1..=KEY_DIGIT_9).contains(&code) {
                        let digit = (code - KEY_DIGIT_1) as usize;
                        if modifiers.shift {
                            return FilterResult::Intercept(Action::SendToSpace(digit));
                        }
                        if modifiers.alt && digit < 3 {
                            return FilterResult::Intercept(Action::Snap(crate::wm::Snap::Third(
                                digit as u8,
                            )));
                        }
                    }
                    match sym {
                        Keysym::v | Keysym::V => return FilterResult::Intercept(Action::ToggleClipboard),
                        Keysym::w | Keysym::W => return FilterResult::Intercept(Action::ToggleWidgets),
                        Keysym::q | Keysym::Q => {
                            return FilterResult::Intercept(Action::CloseWindow);
                        }
                        Keysym::m | Keysym::M => return FilterResult::Intercept(Action::MinimizeWindow),
                        Keysym::h | Keysym::H => return FilterResult::Intercept(Action::HideApp),
                        Keysym::space => return FilterResult::Intercept(Action::ToggleLauncher),
                        Keysym::Escape => return FilterResult::Intercept(Action::PowerDialog),
                        Keysym::n | Keysym::N => return FilterResult::Intercept(Action::ToggleNotifications),
                        Keysym::a | Keysym::A => return FilterResult::Intercept(Action::ToggleControls),
                        Keysym::s | Keysym::S => return FilterResult::Intercept(Action::ToggleOverview),
                        Keysym::d | Keysym::D => return FilterResult::Intercept(Action::ShowDesktop),
                        Keysym::l | Keysym::L => return FilterResult::Intercept(Action::Lock),
                        Keysym::t | Keysym::T => return FilterResult::Intercept(Action::ToggleTiling),
                        Keysym::c | Keysym::C => {
                            return FilterResult::Intercept(Action::Snap(crate::wm::Snap::Center));
                        }
                        Keysym::Return | Keysym::KP_Enter => {
                            return FilterResult::Intercept(Action::SuperEnter);
                        }
                        Keysym::Up if !modifiers.ctrl => return FilterResult::Intercept(Action::Maximize),
                        Keysym::Down if !modifiers.ctrl => {
                            return FilterResult::Intercept(Action::RestoreOrMinimize);
                        }
                        Keysym::Left if !modifiers.ctrl => {
                            return FilterResult::Intercept(Action::SnapSide(true));
                        }
                        Keysym::Right if !modifiers.ctrl => {
                            return FilterResult::Intercept(Action::SnapSide(false));
                        }
                        _ => {}
                    }
                    if modifiers.ctrl && sym == Keysym::Left {
                        return FilterResult::Intercept(Action::SwitchPage(-1));
                    }
                    if modifiers.ctrl && sym == Keysym::Right {
                        return FilterResult::Intercept(Action::SwitchPage(1));
                    }
                    if modifiers.ctrl && sym == Keysym::Up {
                        return FilterResult::Intercept(Action::Overview(true));
                    }
                    if modifiers.ctrl && sym == Keysym::Down {
                        return FilterResult::Intercept(Action::Overview(false));
                    }
                }
                if pressed
                    && sym == Keysym::Escape
                    && state.dashboard_editing.is_none()
                    && (state.widgets_front || state.widgets_edit)
                {
                    return FilterResult::Intercept(Action::WidgetsDismiss);
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
                // Panneau Super+V ouvert : flèches, Entrée et Suppr le pilotent, Tab change de
                // filtre, le reste s'écrit dans la recherche ; sans recherche, un chiffre colle
                // la ligne qui le porte.
                if pressed && state.popover == Some(Popover::Clipboard) {
                    let searching = !state.clipboard_query.is_empty();
                    match keysym.modified_sym() {
                        Keysym::Up => return FilterResult::Intercept(Action::ClipboardMove(-1)),
                        Keysym::Down => return FilterResult::Intercept(Action::ClipboardMove(1)),
                        Keysym::Return | Keysym::KP_Enter => {
                            return FilterResult::Intercept(Action::ClipboardPasteSelected);
                        }
                        Keysym::Delete => return FilterResult::Intercept(Action::ClipboardDeleteSelected),
                        Keysym::Tab | Keysym::ISO_Left_Tab => {
                            return FilterResult::Intercept(Action::ClipboardNextFilter);
                        }
                        Keysym::Escape if searching => {
                            return FilterResult::Intercept(Action::ClipboardClearQuery);
                        }
                        Keysym::BackSpace => return FilterResult::Intercept(Action::ClipboardType(None)),
                        sym if !modifiers.ctrl && !modifiers.alt && !modifiers.logo => {
                            if let Some(character) = sym.key_char().filter(|c| !c.is_control()) {
                                return FilterResult::Intercept(match character.to_digit(10) {
                                    Some(digit @ 1..=9) if !searching => {
                                        Action::ClipboardPasteNth(digit as usize - 1)
                                    }
                                    _ => Action::ClipboardType(Some(character)),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                if pressed && !modifiers.ctrl && !modifiers.alt {
                    if sym == Keysym::Escape
                        && (state.popover.is_some() || state.pending_power.is_some() || state.overview)
                    {
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
                    Keysym::Up => FilterResult::Intercept(Action::Overview(true)),
                    Keysym::Down => FilterResult::Intercept(Action::Overview(false)),
                    _ => FilterResult::Forward,
                }
            },
        );
        self.update_key_repeat(event.key_code().raw(), pressed, action);
        self.run_key_action(action);
        // Le sélecteur se referme au relâchement d'Alt, que le client voit aussi.
        if !pressed && self.switcher.is_some() && !keyboard.modifier_state().alt {
            self.switcher_commit();
        }
    }

    /// Une touche maintenue dans un champ du bureau (recherche, note, tâche, mot de passe)
    /// se répète comme dans les applications : après un délai, puis à cadence régulière.
    /// Les applications répètent elles-mêmes ; ces champs-là sont dessinés par le shell.
    fn update_key_repeat(&mut self, code: u32, pressed: bool, action: Option<Action>) {
        if !pressed {
            if self.key_repeat.is_some_and(|(held, _, _)| held == code) {
                self.key_repeat = None;
            }
            return;
        }
        self.key_repeat = None;
        let Some(action) = action.filter(|action| action.repeats()) else {
            return;
        };
        self.key_repeat_generation = self.key_repeat_generation.wrapping_add(1);
        let generation = self.key_repeat_generation;
        self.key_repeat = Some((code, action, generation));
        // Mêmes délai et cadence que ceux annoncés aux applications.
        let rate = u64::try_from(crate::state::KEY_REPEAT_RATE).unwrap_or(25).max(1);
        let delay = u64::try_from(crate::state::KEY_REPEAT_DELAY).unwrap_or(600);
        let interval = Duration::from_millis(1000 / rate);
        let timer = Timer::from_duration(Duration::from_millis(delay));
        let inserted = self
            .loop_handle
            .insert_source(timer, move |_, _, state| match state.key_repeat {
                Some((_, action, current)) if current == generation => {
                    state.run_key_action(Some(action));
                    state.request_redraw();
                    TimeoutAction::ToDuration(interval)
                }
                _ => TimeoutAction::Drop,
            });
        if let Err(error) = inserted {
            tracing::warn!(%error, "répétition des touches indisponible");
        }
    }

    fn run_key_action(&mut self, action: Option<Action>) {
        match action {
            Some(Action::Quit) => {
                info!("Ctrl+Alt+Retour arrière : arrêt de ferrite-shell");
                self.request_exit();
                self.loop_signal.stop();
            }
            Some(Action::RestoreMinimized) => self.restore_minimized(),
            Some(Action::SwitchPage(step)) => self.go_to_neighbor_page(step),
            Some(Action::Overview(true)) => self.open_overview(),
            Some(Action::Overview(false)) => self.close_overview(),
            Some(Action::SwitchVt(vt)) => {
                if let Some(change_vt) = self.change_vt.as_mut() {
                    info!("passage au terminal virtuel {vt}");
                    change_vt(vt);
                }
            }
            Some(Action::CloseOverlay) => {
                self.popover = None;
                self.pending_power = None;
                self.region_select = None;
                self.close_overview();
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
                Some(crate::desktop::widgets::DashboardEdit::ListName(_)) => {
                    self.stop_dashboard_editing();
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
            Some(Action::ClipboardPasteSelected) => {
                if let Some(index) = self.clipboard_selected_entry() {
                    self.paste_history(index);
                }
            }
            Some(Action::ClipboardDeleteSelected) => {
                if let Some(index) = self.clipboard_selected_entry() {
                    self.delete_clipboard_entry(index);
                }
            }
            Some(Action::ClipboardType(character)) => self.type_clipboard_query(character),
            Some(Action::ClipboardClearQuery) => {
                self.clipboard_query.clear();
                self.clamp_clipboard_view();
            }
            Some(Action::ClipboardNextFilter) => self.next_clipboard_filter(),
            Some(Action::ClipboardPasteNth(position)) => {
                if let Some(index) = self.clipboard_shown().get(position).copied() {
                    self.paste_history(index);
                }
            }
            Some(Action::SwitcherStep(step)) => self.switcher_step(step),
            Some(Action::SwitcherCancel) => self.switcher = None,
            Some(Action::SwitcherSameApp(step)) => self.switcher_step_same_app(step),
            Some(Action::CloseWindow) => self.close_active_window(),
            Some(Action::MinimizeWindow) => self.minimize_active_window(),
            Some(Action::HideApp) => self.hide_active_app(),
            Some(Action::Maximize) => self.toggle_maximize_active(),
            Some(Action::RestoreOrMinimize) => self.restore_or_minimize_active(),
            Some(Action::SnapSide(left)) => self.snap_active_side(left),
            Some(Action::Snap(snap)) => self.snap_active(snap),
            Some(Action::ToggleTiling) => self.toggle_tiling(),
            Some(Action::SuperEnter) => {
                if self.tiling {
                    self.swap_with_main();
                } else {
                    self.open_terminal();
                }
            }
            Some(Action::OpenTerminal) => self.open_terminal(),
            Some(Action::ShowDesktop) => self.toggle_show_desktop(),
            Some(Action::ToggleOverview) => {
                if self.overview {
                    self.close_overview();
                } else {
                    self.open_overview();
                }
            }
            Some(Action::ToggleControls) => {
                self.pending_power = None;
                self.handle_desktop_action(DesktopAction::TogglePopover(Popover::Controls));
            }
            Some(Action::Lock) => self.lock_session(),
            Some(Action::SendToSpace(space)) => self.send_active_to_space(space),
            Some(Action::PasswordType(character)) => self.password_type(character),
            Some(Action::PasswordErase) => self.password_erase(),
            Some(Action::PasswordSubmit) => self.password_submit(),
            Some(Action::PasswordCancel) => self.password_cancel(),
            Some(Action::LockType(character)) => self.lock_type(character),
            Some(Action::LockErase) => self.lock_erase(),
            Some(Action::LockDelete) => self.lock_delete(),
            Some(Action::LockMove(step)) => self.lock_move_cursor(step),
            Some(Action::LockSubmit) => self.lock_submit(),
            Some(Action::LockReveal) => self.lock_reveal(),
            Some(Action::LockIgnore) => {}
            Some(Action::AuthType(character)) => self.auth_type(character),
            Some(Action::AuthErase) => self.auth_erase(),
            Some(Action::AuthSubmit) => self.auth_submit(),
            Some(Action::AuthCancel) => self.auth_cancel(),
            Some(Action::LockCancel) => {
                if self.pending_power.take().is_none() {
                    self.lock_cancel();
                }
            }
            Some(Action::ToggleWidgets) => self.toggle_widgets_front(),
            Some(Action::WidgetsDismiss) => self.stop_widgets_overlay(),
            Some(Action::ToggleLauncher) => {
                self.pending_power = None;
                self.handle_desktop_action(DesktopAction::TogglePopover(Popover::Launcher));
            }
            Some(Action::ToggleNotifications) => {
                self.pending_power = None;
                self.handle_desktop_action(DesktopAction::TogglePopover(Popover::Notifications));
            }
            Some(Action::ScreenshotFull) => {
                self.screenshot_request = Some(crate::render::capture::Request { region: None });
            }
            Some(Action::ScreenshotRegion) => {
                self.popover = None;
                self.region_select = Some(crate::render::capture::RegionSelect::default());
            }
            Some(Action::Function(key)) => self.on_function_key(key),
            Some(Action::PowerDialog) => self.open_power_dialog(crate::state::PowerAction::PowerOff),
            Some(Action::PowerConfirm) => {
                if let Some(power) = self.pending_power {
                    self.handle_desktop_action(DesktopAction::ConfirmPower(power));
                }
            }
            Some(Action::PowerStep(step)) => {
                use crate::state::PowerAction;
                let choices = [PowerAction::PowerOff, PowerAction::Restart, PowerAction::Sleep];
                let current = self
                    .pending_power
                    .and_then(|power| choices.iter().position(|choice| *choice == power))
                    .unwrap_or(0) as i32;
                let next = (current + step).rem_euclid(choices.len() as i32) as usize;
                self.open_power_dialog(choices[next]);
            }
            None => {}
        }
    }

    /// Passe au thème opposé à celui affiché et l'écrit dans les réglages, qui le diffusent
    /// aux applications.
    fn toggle_dark_mode(&mut self) {
        let appearance = if self.appearance.is_dark() {
            ferrite_system::config::Appearance::Light
        } else {
            ferrite_system::config::Appearance::Dark
        };
        self.appearance = appearance;
        let mut settings = ferrite_system::config::Settings::load();
        settings.set("apparence", appearance.key());
        if let Err(error) = settings.save() {
            tracing::warn!(%error, "thème non enregistré");
        }
    }

    fn run_corner_action(&mut self, action: ferrite_system::config::CornerAction) {
        use ferrite_system::config::CornerAction;
        match action {
            CornerAction::None => {}
            CornerAction::Overview => self.open_overview(),
            CornerAction::ShowDesktop => self.toggle_show_desktop(),
            CornerAction::Launcher => {
                self.handle_desktop_action(DesktopAction::TogglePopover(Popover::Launcher));
            }
            CornerAction::ControlCenter => {
                self.handle_desktop_action(DesktopAction::TogglePopover(Popover::Controls));
            }
        }
    }

    fn open_terminal(&mut self) {
        self.launch_app(&crate::desktop::apps::AppKey::Builtin(
            crate::desktop::apps::App::Terminal,
        ));
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
        let width = crate::desktop::panels::CLIPBOARD_WIDTH;
        let height = crate::desktop::panels::clipboard_height(output.size.h as f64);
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
        let width = crate::desktop::panels::CLIPBOARD_WIDTH;
        let height = crate::desktop::panels::clipboard_height(output.size.h as f64);
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
        // Verrouillé par un client : seul son écran suit le pointeur.
        if self.session_lock.is_some() {
            let under = self.lock_surface_under(location);
            let motion = MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            };
            pointer.motion(self, under, &motion);
            pointer.frame(self);
            return;
        }
        let previous = pointer.current_location();
        let now = std::time::Instant::now();
        let elapsed = self
            .dock_pointer_motion_at
            .replace(Some(now))
            .map(|last| now.duration_since(last));
        if let Some(elapsed) = elapsed {
            let speed = crate::desktop::dock::descent_speed(
                self.dock_descent_speed.get(),
                location.y - previous.y,
                elapsed,
            );
            self.dock_descent_speed.set(speed);
            if let Some(output) = self.space.outputs().next()
                && let Some(output_geo) = self.space.output_geometry(output)
                && crate::desktop::dock::fast_approach(
                    speed,
                    location,
                    (output_geo.loc.y + output_geo.size.h) as f64,
                )
            {
                self.dock_fast_approach_until
                    .set(Some(now + crate::desktop::dock::REVEAL_APPROACH_GRACE));
            }
        }
        // Coins actifs : le pointeur qui arrive dans un angle de l'écran déclenche son action.
        if let Some(area) = self.screen_area()
            && self.lock.is_none()
        {
            let corner = crate::wm::screen_corner(area, location);
            if corner.is_some() && corner != crate::wm::screen_corner(area, previous) {
                self.run_corner_action(self.dock_config.corners[corner.unwrap_or_default()]);
            }
        }
        // Glissement d'un curseur des réglages rapides ou du panneau presse-papiers.
        if let Some(slider) = self.slider_drag {
            self.drag_slider(slider, location);
        }
        if self.clipboard_drag_offset.is_some() {
            self.drag_clipboard(location);
        }
        self.drag_widget(location);
        self.dock_drag_motion(location);
        self.file_drag_motion(location);
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
        // Glisser de fichiers repris par ferrite-shell : le relâchement est le dépôt.
        if self.file_drag.as_ref().is_some_and(|file| file.shown()) {
            pointer.button(self, &button);
            pointer.frame(self);
            return;
        }
        if button.state == ButtonState::Released {
            self.file_drag_released();
        }
        let location = pointer.current_location();

        // Verrouilleur externe, ou calque au-dessus du bureau (lanceur, barre d'un autre
        // bureau) : le clic lui revient directement.
        let layer = self.layer_surface_under(
            location,
            &[
                smithay::wayland::shell::wlr_layer::Layer::Overlay,
                smithay::wayland::shell::wlr_layer::Layer::Top,
            ],
        );
        if self.session_lock.is_some() || (self.lock.is_none() && layer.is_some()) {
            if let Some((surface, _)) = layer.filter(|_| button.state == ButtonState::Pressed) {
                self.focus_layer_on_click(&surface);
            }
            pointer.button(self, &button);
            pointer.frame(self);
            return;
        }
        if self.lock.is_some() {
            if button.button == BTN_LEFT {
                if button.state == ButtonState::Pressed {
                    self.on_lock_click(location);
                } else {
                    // L'œil se relâche où que soit le pointeur.
                    self.lock_peek(false);
                }
            }
            return;
        }
        // Fin d'un déplacement de widget.
        if button.button == BTN_LEFT
            && button.state == ButtonState::Released
            && self.widget_drag.take().is_some()
        {
            // Lâché, le widget s'aligne sur la grille, à une place libre.
            let last = self.dashboard.layout.len().saturating_sub(1);
            let bar = crate::render::desktop_view(self).bar_height as i32;
            if let Some(shown) = self.shown_widget(last) {
                self.dashboard.layout[last] = shown;
            }
            if let Some(area) = self.screen_area()
                && let Some(placed) = crate::desktop::widgets::place_on_grid(
                    &self.dashboard.layout,
                    last,
                    area.size.w,
                    area.size.h,
                    bar,
                )
            {
                self.dashboard.layout[last] = placed;
            }
            self.save_dashboard();
            return;
        }
        if let Some(select) = self.region_select.as_mut() {
            match (button.button, button.state) {
                (BTN_LEFT, ButtonState::Pressed) => select.start = Some(location),
                (BTN_LEFT, ButtonState::Released) => {
                    let start = select.start;
                    self.region_select = None;
                    self.screenshot_request = start
                        .map(|start| crate::render::capture::region_between(start, location))
                        .filter(|region| region.size.w >= 4 && region.size.h >= 4)
                        .map(|region| crate::render::capture::Request { region: Some(region) });
                }
                (_, ButtonState::Pressed) => self.region_select = None,
                _ => {}
            }
            return;
        }

        if button.state == ButtonState::Released && self.slider_drag.take().is_some() {
            return;
        }
        if button.state == ButtonState::Released && self.dock_drag.is_some() {
            self.dock_drag_release();
            return;
        }
        if button.state == ButtonState::Released && self.clipboard_drag_offset.take().is_some() {
            return;
        }
        if button.state == ButtonState::Released {
            // Un feu agit au relâchement, si le pointeur est toujours dessus.
            if let Some((window, light)) = self.pressed_light.take() {
                if let Some(Hit::Decoration(under, Part::Button(now))) = self.hit_test(location) {
                    if under == window && now == light {
                        self.activate_light(&window, light);
                    }
                }
                return;
            }
        } else if !pointer.is_grabbed() {
            if self.overview {
                let left = button.button == BTN_LEFT;
                if left {
                    let area = self
                        .space
                        .outputs()
                        .next()
                        .and_then(|output| self.space.output_geometry(output));
                    let space = area.and_then(|area| {
                        panels::overview_space_at(area, &crate::render::desktop_view(self), location)
                    });
                    if let Some(page) = space {
                        self.go_to_page(page);
                        self.overview_order = self.space.elements().rev().cloned().collect();
                        return;
                    }
                }
                if let Some((window, close)) = self.overview_hit(location) {
                    // La vue reste ouverte après une fermeture : les aperçus se réarrangent.
                    if left && close {
                        crate::wm::close_window(&window);
                    } else if left {
                        self.close_overview();
                        self.focus_window(Some(window));
                    }
                    return;
                }
                // Le Dock et la barre restent utilisables ; ailleurs, un clic referme la vue.
                let chrome = self.dock_item_at(location).is_some()
                    || location.y < f64::from(ferrite_ui::theme::MENUBAR_HEIGHT);
                if left {
                    self.close_overview();
                }
                if !chrome {
                    return;
                }
            }
            let output_geo = self
                .space
                .outputs()
                .next()
                .and_then(|output| self.space.output_geometry(output));
            // Dock : clic gauche pour ouvrir ou ramener l'application, clic droit pour son
            // menu.
            if self.pending_power.is_none()
                && let Some((index, item)) = self.dock_item_at(location)
            {
                match button.button {
                    BTN_LEFT => {
                        self.popover = None;
                        self.dock_menu = None;
                        // Une icône épinglée se clique ou se déplace : décidé au relâchement.
                        if item.pinned
                            && item.key
                                != crate::desktop::apps::AppKey::Builtin(crate::desktop::apps::App::Trash)
                        {
                            self.dock_drag = Some(crate::desktop::launch::DockDrag {
                                key: item.key,
                                start: location,
                                target: None,
                            });
                        } else {
                            self.activate_app(&item.key);
                        }
                    }
                    BTN_RIGHT => {
                        let entries = self.dock_menu_entries(&item);
                        if !entries.is_empty() {
                            // Le menu se pose au-dessus de l'icône telle qu'elle est agrandie.
                            let anchor = self
                                .dock_geometry()
                                .and_then(|(_, _, geometry)| geometry.icons.get(index).copied())
                                .map_or(location, |icon| {
                                    Point::from((icon.loc.x + icon.size.w / 2.0, icon.loc.y))
                                });
                            self.dock_menu_pointer = Some(location);
                            self.dock_menu = Some((item.key, entries, anchor));
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
            // Sous une fenêtre, les widgets ne reçoivent rien, sauf s'ils sont passés devant.
            let widgets_ahead = self.widgets_front || self.widgets_edit;
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
                        | DesktopAction::DashboardNoteStep(_)
                        | DesktopAction::DashboardApp(_)
                        | DesktopAction::DashboardMedia(_)
                        | DesktopAction::DashboardOpenMusic
                        | DesktopAction::DashboardShowNote(_)
                        | DesktopAction::DashboardToggleCheck(_, _)
                        | DesktopAction::DashboardDeleteNote(_)
                        | DesktopAction::DashboardList(_)
                        | DesktopAction::DashboardAddList
                        | DesktopAction::DashboardDeleteList(_)
                        | DesktopAction::DashboardConsume
                ) || widgets_ahead
                    || self.hit_test(location).is_none()
            });
            // Widgets ramenés devant : un clic à côté les renvoie derrière.
            if self.widgets_front && desktop_action.is_none() {
                if button.state == ButtonState::Pressed {
                    self.widgets_front = false;
                }
                return;
            }
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
            // Clic droit sur le bureau, widgets compris : son menu.
            if button.button == BTN_RIGHT
                && self.popover.is_none()
                && !self.overview
                && !self.widgets_edit
                && self.pending_power.is_none()
                && self.hit_test(location).is_none()
                && location.y >= f64::from(ferrite_ui::theme::MENUBAR_HEIGHT)
            {
                if button.state == ButtonState::Pressed {
                    self.desktop_menu = Some((location.x as i32, location.y as i32));
                    self.popover = Some(Popover::DesktopMenu);
                }
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

    /// Le lanceur revient en haut de sa liste.
    fn reset_launcher_view(&mut self) {
        self.launcher_scroll = 0;
    }

    pub(crate) fn handle_desktop_action(&mut self, action: DesktopAction) {
        match action {
            DesktopAction::TogglePopover(popover) => {
                let opening_clipboard =
                    popover == Popover::Clipboard && self.popover != Some(Popover::Clipboard);
                self.popover = (self.popover != Some(popover)).then_some(popover);
                if self.popover == Some(Popover::Controls) {
                    self.controls_opened();
                }
                self.controls.password = None;
                self.launcher_query.clear();
                self.reset_launcher_view();
                if opening_clipboard {
                    self.reset_clipboard_view();
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
                if let Some(task) = self.dashboard.tasks_mut().get_mut(index) {
                    task.done = !task.done;
                    if let Err(error) = self.dashboard.save() {
                        tracing::warn!(%error, "tâches du tableau de bord non enregistrées");
                    }
                }
            }
            DesktopAction::DashboardCyclePriority(index) => {
                if let Some(task) = self.dashboard.tasks_mut().get_mut(index) {
                    task.priority = task.priority.next();
                    if let Err(error) = self.dashboard.save() {
                        tracing::warn!(%error, "tâches du tableau de bord non enregistrées");
                    }
                }
            }
            DesktopAction::DashboardDeleteTask(index) => {
                self.stop_dashboard_editing();
                if self.dashboard.remove_task(index) {
                    self.save_dashboard();
                }
            }
            DesktopAction::DashboardShowNote(index) => {
                self.stop_dashboard_editing();
                self.dashboard.note_shown = index.min(self.dashboard.notes.len().saturating_sub(1));
                self.save_dashboard();
            }
            DesktopAction::DashboardToggleCheck(note, line) => {
                self.stop_dashboard_editing();
                let now = crate::desktop::widgets::now_seconds();
                if self.dashboard.toggle_check(note, line, now) {
                    self.save_dashboard();
                }
            }
            DesktopAction::DashboardDeleteNote(index) => {
                self.dashboard_editing = None;
                self.dashboard.remove_note(index);
                self.save_dashboard();
            }
            DesktopAction::DashboardList(index) => {
                self.stop_dashboard_editing();
                self.dashboard.show_list(index);
                self.save_dashboard();
            }
            DesktopAction::DashboardAddList => {
                self.stop_dashboard_editing();
                self.dashboard_editing = self.dashboard.add_list();
                self.save_dashboard();
            }
            DesktopAction::DashboardDeleteList(index) => {
                self.dashboard_editing = None;
                self.dashboard.remove_list(index);
                self.save_dashboard();
            }
            DesktopAction::DashboardMonth(delta) => {
                self.dashboard.month_offset = (self.dashboard.month_offset + delta).clamp(-120, 120);
            }
            DesktopAction::DashboardNoteStep(step) => {
                self.stop_dashboard_editing();
                let count = self.dashboard.notes.len() as i32;
                if count > 0 {
                    let shown = self.dashboard.note_shown.min(count as usize - 1) as i32;
                    self.dashboard.note_shown = (shown + step).rem_euclid(count) as usize;
                }
            }
            DesktopAction::WidgetsDone => self.stop_widgets_overlay(),
            DesktopAction::WidgetAdd(kind) => {
                self.dashboard.add_widget(kind);
                self.refresh_media_watching();
                self.save_dashboard();
            }
            DesktopAction::WidgetRemove(index) => {
                self.dashboard.remove_widget(index);
                self.refresh_media_watching();
                self.save_dashboard();
            }
            DesktopAction::WidgetsReset => {
                self.dashboard.layout = crate::desktop::widgets::default_layout();
                self.save_dashboard();
            }
            DesktopAction::WidgetDragStart(index, edges) => {
                let pointer = self.pointer_location().unwrap_or_default();
                let index = self.dashboard.raise_widget(index);
                if let Some(start) = self.shown_widget(index) {
                    self.dashboard.layout[index] = start;
                    self.widget_drag = Some(WidgetDrag {
                        index,
                        edges,
                        pointer,
                        start,
                    });
                }
            }
            DesktopAction::DesktopMenu(index) => {
                self.popover = None;
                match index {
                    0 => self.start_widgets_edit(),
                    _ => {
                        crate::desktop::apps::App::open_settings_page("apparence", &self.client_environment())
                    }
                }
            }
            // Un clic ailleurs sur les widgets termine la saisie.
            DesktopAction::DashboardConsume => {
                self.stop_dashboard_editing();
            }
            DesktopAction::DashboardApp(index) => {
                self.stop_dashboard_editing();
                self.open_recent_app(index);
            }
            DesktopAction::DashboardMedia(command) => {
                self.stop_dashboard_editing();
                self.lock_media(command);
            }
            DesktopAction::DashboardOpenMusic => {
                self.stop_dashboard_editing();
                self.activate_app(&crate::desktop::apps::AppKey::Builtin(
                    crate::desktop::apps::App::Music,
                ));
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
            DesktopAction::ClosePopover => self.popover = None,
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
            DesktopAction::SetVolume(value) => self.request_system_change(Change::Volume(value)),
            DesktopAction::SetBrightness(value) => self.request_system_change(Change::Brightness(value)),
            DesktopAction::RequestPower(power) => self.open_power_dialog(power),
            DesktopAction::PowerChoose(power) => self.open_power_dialog(power),
            DesktopAction::CancelPower => self.pending_power = None,
            DesktopAction::AuthSubmit => self.auth_submit(),
            DesktopAction::AuthCancel => self.auth_cancel(),
            DesktopAction::ConfirmPower(power) => {
                self.pending_power = None;
                if let Some(code) = power.exit_code() {
                    info!(code, ?power, "fin de la session demandée");
                    self.exit_code = code;
                    self.request_exit();
                    self.loop_signal.stop();
                } else {
                    // Le bureau se ferme quand systemd accepte l'arrêt (voir `system.rs`).
                    self.request_system_change(Change::Power(power));
                }
            }
            DesktopAction::ClipboardPaste(index) => self.paste_history(index),
            DesktopAction::ClipboardPin(index) => {
                self.clipboard.toggle_pin(index);
                self.clamp_clipboard_view();
            }
            DesktopAction::ClipboardDelete(index) => self.delete_clipboard_entry(index),
            DesktopAction::ClipboardClear => self.clear_clipboard(),
            DesktopAction::ClipboardFilter(filter) => self.set_clipboard_filter(filter),
            DesktopAction::ClipboardUndo => self.undo_clipboard(),
            DesktopAction::NotificationActivate(id) => self.activate_notification(id),
            DesktopAction::NotificationAction(id, index) => self.notification_action(id, index),
            DesktopAction::NotificationsClear => self.clear_notifications(),
            DesktopAction::ToggleDoNotDisturb => {
                self.notifications.do_not_disturb = !self.notifications.do_not_disturb;
            }
            DesktopAction::ToggleDarkMode => self.toggle_dark_mode(),
            DesktopAction::TogglePowerSaver => {
                self.request_system_change(Change::PowerSaver(!self.power_saver));
            }
            DesktopAction::OpenSettings => {
                self.popover = None;
                self.open_app(&crate::desktop::apps::AppKey::Builtin(
                    crate::desktop::apps::App::Settings,
                ));
            }
            DesktopAction::ToggleKeyboardLayout => self.toggle_keyboard_layout(),
            DesktopAction::ToggleNight => self.toggle_night(),
            DesktopAction::OpenNightSettings => {
                self.popover = None;
                crate::desktop::apps::App::open_settings_page("ecrans", &self.client_environment());
            }
            DesktopAction::ToggleAirplane => {
                let on = self.system.airplane != Some(true);
                self.request_system_change(Change::Airplane(on));
            }
            DesktopAction::CycleKeyboardLight => {
                if let Some((level, max)) = self.system.keyboard_light {
                    let next = if level >= max { 0 } else { level + 1 };
                    self.request_system_change(Change::KeyboardLight(next));
                }
            }
            DesktopAction::ControlsPage(page) => self.show_controls_page(page),
            DesktopAction::NetworkJoin(index) => self.join_network(index),
            DesktopAction::NetworkPassword(index) => self.show_network_password(index),
            DesktopAction::PasswordReveal => self.password_reveal(),
            DesktopAction::PasswordCancel => self.password_cancel(),
            DesktopAction::PasswordSubmit => self.password_submit(),
            DesktopAction::DeviceAction(index) => self.device_action(index),
            DesktopAction::OpenSettingsPage(page) => {
                self.popover = None;
                crate::desktop::apps::App::open_settings_page(page, &self.client_environment());
            }
            DesktopAction::Lock => self.lock_session(),
            DesktopAction::Consume => {}
        }
    }

    fn save_dashboard(&self) {
        if let Err(error) = self.dashboard.save() {
            tracing::warn!(%error, "widgets non enregistrés");
        }
    }

    /// Super+W : les widgets passent devant les fenêtres, ou y retournent.
    /// Ouvre (ou garde ouvert) le dialogue d'arrêt sur `power`, compte à rebours relancé.
    pub(crate) fn open_power_dialog(&mut self, power: crate::state::PowerAction) {
        self.popover = None;
        self.pending_power = Some(power);
        self.power_deadline = Some(std::time::Instant::now() + POWER_COUNTDOWN);
    }

    /// Compte à rebours écoulé : l'action choisie se fait seule.
    pub(crate) fn power_tick(&mut self) {
        if let (Some(power), Some(deadline)) = (self.pending_power, self.power_deadline)
            && std::time::Instant::now() >= deadline
        {
            self.power_deadline = None;
            self.handle_desktop_action(DesktopAction::ConfirmPower(power));
        }
    }

    /// Applications ouvertes (fenêtres affichées, réduites ou sur un autre bureau).
    pub(crate) fn open_app_count(&self) -> usize {
        let mut apps: Vec<crate::desktop::apps::AppKey> = self
            .space
            .elements()
            .chain(self.minimized.iter().map(|minimized| &minimized.window))
            .chain(self.workspaces.all_parked().map(|parked| &parked.window))
            .filter_map(|window| self.window_app(window))
            .collect();
        apps.sort_by_key(crate::desktop::apps::AppKey::token);
        apps.dedup();
        apps.len()
    }

    /// Super+W : va au bureau des widgets (le 0), puis revient au bureau quitté.
    fn toggle_widgets_front(&mut self) {
        if self.widgets_edit {
            return;
        }
        self.popover = None;
        let current = self.workspaces.current();
        if current == 0 {
            self.stop_dashboard_editing();
            let back = self.page_before_widgets.take().unwrap_or(1);
            if self.workspaces.page_count() > back {
                self.go_to_page(back);
            }
        } else {
            self.page_before_widgets = Some(current);
            self.go_to_page(0);
        }
    }

    fn start_widgets_edit(&mut self) {
        self.stop_dashboard_editing();
        self.widgets_front = false;
        self.widgets_edit = true;
    }

    /// Échap, Terminé ou verrouillage : les widgets retournent sous les fenêtres.
    pub fn stop_widgets_overlay(&mut self) {
        self.stop_dashboard_editing();
        self.widgets_front = false;
        self.widgets_edit = false;
        self.widget_drag = None;
        self.save_dashboard();
    }

    /// Le widget tel qu'il est à l'écran : ramené dedans si l'écran a rétréci.
    fn shown_widget(&self, index: usize) -> Option<crate::desktop::widgets::WidgetSlot> {
        let area = self.screen_area()?;
        let bar = crate::render::desktop_view(self).bar_height as i32;
        Some(
            self.dashboard
                .layout
                .get(index)?
                .clamped(area.size.w, area.size.h, bar),
        )
    }

    /// Le widget saisi suit le pointeur, librement.
    fn drag_widget(&mut self, location: Point<f64, smithay::utils::Logical>) {
        let Some(drag) = self.widget_drag else {
            return;
        };
        let (dx, dy) = (location.x - drag.pointer.x, location.y - drag.pointer.y);
        if let Some(slot) = self.dashboard.layout.get_mut(drag.index) {
            *slot = drag.edges.apply(drag.start, dx.round() as i32, dy.round() as i32);
        }
    }

    /// Clic sur l'écran de verrouillage. La confirmation d'arrêt, si elle est ouverte, passe avant.
    fn on_lock_click(&mut self, location: Point<f64, smithay::utils::Logical>) {
        let Some(area) = self
            .space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))
        else {
            return;
        };
        let view = crate::render::desktop_view(self);
        if self.pending_power.is_some() {
            if let Some(action) = panels::action_at(area, location, &view) {
                self.handle_desktop_action(action);
            }
            return;
        }
        match panels::lock_action_at(area, &view, location) {
            LockAction::Reveal => self.lock_reveal(),
            LockAction::Submit => self.lock_submit(),
            LockAction::Peek => self.lock_peek(true),
            LockAction::Cancel => self.lock_cancel(),
            LockAction::ToggleTheme => self.lock_toggle_theme(),
            LockAction::Wall(wall) => self.lock_set_wall(wall),
            LockAction::Keyboard => self.toggle_keyboard_layout(),
            LockAction::Power => self.open_power_dialog(crate::state::PowerAction::PowerOff),
            LockAction::Media(command) => self.lock_media(command),
            LockAction::Consume => {}
        }
    }

    /// Molette au-dessus d'un widget : change de mois ou fait défiler les tâches.
    fn scroll_widget(&mut self, location: Point<f64, smithay::utils::Logical>, amount: f64) -> bool {
        if self.popover.is_some() || self.overview || self.lock.is_some() || self.widgets_edit {
            return false;
        }
        if !self.widgets_front && self.hit_test(location).is_some() {
            return false;
        }
        let Some(area) = self
            .space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))
        else {
            return false;
        };
        let view = crate::render::desktop_view(self);
        let Some(index) = panels::widget_at(area, &view, location) else {
            return false;
        };
        self.widget_scroll += amount;
        if self.widget_scroll.abs() < WIDGET_SCROLL_STEP {
            return true;
        }
        let step = self.widget_scroll.signum() as i32;
        self.widget_scroll = 0.0;
        match self.dashboard.layout.get(index).map(|slot| slot.kind) {
            Some(crate::desktop::widgets::WidgetKind::Calendar) => {
                self.handle_desktop_action(DesktopAction::DashboardMonth(step));
            }
            Some(crate::desktop::widgets::WidgetKind::Tasks) => {
                let limit = panels::tasks_scroll_limit(self.dashboard.tasks().len()) as i32;
                let scroll = self.dashboard.tasks_scroll as i32 + step;
                self.dashboard.tasks_scroll = scroll.clamp(0, limit) as usize;
            }
            _ => {}
        }
        true
    }

    pub fn stop_dashboard_editing(&mut self) -> Option<usize> {
        let naming = matches!(
            self.dashboard_editing,
            Some(crate::desktop::widgets::DashboardEdit::ListName(_))
        );
        let removed = self.dashboard.finish_editing(self.dashboard_editing);
        self.dashboard_editing = None;
        if (removed.is_some() || naming)
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
            Part::Button(light) => {
                self.pressed_light = Some((window, light));
                true
            }
            Part::Titlebar => {
                if self.titlebar_click(&window) {
                    use ferrite_system::config::DoubleClick;
                    match self.dock_config.double_click {
                        DoubleClick::Maximize => self.toggle_maximize(&window),
                        DoubleClick::Minimize => self.minimize(&window),
                        DoubleClick::Nothing => {}
                    }
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
        let direction = scroll_direction(source, self.scroll_inverted) as f64;
        // Panneau Super+V : la molette fait défiler la liste.
        if self.popover == Some(Popover::Clipboard) {
            let amount = event
                .amount(Axis::Vertical)
                .or_else(|| event.amount_v120(Axis::Vertical))
                .unwrap_or(0.0)
                * direction;
            if amount != 0.0 {
                self.scroll_clipboard((amount * 2.5).clamp(-96.0, 96.0));
            }
            return;
        }
        // Lanceur ouvert : la molette (ou le pavé tactile) fait défiler les rubriques.
        if self.popover == Some(Popover::Launcher) {
            let amount = event.amount(Axis::Vertical).unwrap_or_else(|| {
                event.amount_v120(Axis::Vertical).unwrap_or(0.0) / 120.0 * LAUNCHER_SCROLL_STEP
            }) * direction;
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
        let amount = event
            .amount(Axis::Vertical)
            .or_else(|| event.amount_v120(Axis::Vertical).map(|value| value / 8.0))
            .unwrap_or(0.0)
            * direction;
        if let Some(location) = self.pointer_location()
            && amount != 0.0
            && self.scroll_widget(location, amount)
        {
            return;
        }
        let mut frame = AxisFrame::new(event.time_msec()).source(source);
        for axis in [Axis::Horizontal, Axis::Vertical] {
            // Molette sans valeur continue : 15 px par cran (120 unités v120).
            let amount = event
                .amount(axis)
                .unwrap_or_else(|| event.amount_v120(axis).unwrap_or(0.0) * 15.0 / 120.0)
                * direction;
            if amount != 0.0 {
                frame = frame.value(axis, amount);
                if let Some(discrete) = event.amount_v120(axis) {
                    frame = frame.v120(axis, discrete as i32 * direction as i32);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inversion_du_defilement_ne_touche_que_la_molette() {
        assert_eq!(scroll_direction(AxisSource::Wheel, true), -1);
        assert_eq!(scroll_direction(AxisSource::WheelTilt, true), -1);
        assert_eq!(scroll_direction(AxisSource::Wheel, false), 1);
        assert_eq!(scroll_direction(AxisSource::Finger, true), 1);
    }
}
