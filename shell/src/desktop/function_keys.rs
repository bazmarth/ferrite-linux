//! Touches de fonction du clavier (rangée F1…F12 avec Fn, ou touches dédiées) : son,
//! micro, luminosité, mode avion, calculatrice, recherche, lecteur multimédia. Chaque
//! réglage s'affiche un instant dans une bulle au-dessus du Dock.

use std::time::{Duration, Instant};

use ferrite_system::rfkill;
use smithay::{
    input::keyboard::Keysym,
    reexports::calloop::timer::{TimeoutAction, Timer},
};

use crate::{
    desktop::{
        apps::{App, AppKey},
        lock::MediaCommand,
        panels::{Osd, Popover},
        svg,
    },
    state::Ferrite,
    system::Change,
};

/// Pas du volume et de la luminosité, en points de pourcentage.
const STEP: u8 = 5;
/// Durée d'affichage de la bulle.
const OSD_DURATION: Duration = Duration::from_millis(1500);
/// Délai laissé au noyau pour basculer lui-même les radios sur la touche mode avion.
const RFKILL_SETTLE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKey {
    VolumeUp,
    VolumeDown,
    Mute,
    MicMute,
    BrightnessUp,
    BrightnessDown,
    Airplane,
    Calculator,
    Search,
    Media(MediaCommand),
}

impl FunctionKey {
    pub fn from_keysym(sym: Keysym) -> Option<FunctionKey> {
        Some(match sym {
            Keysym::XF86_AudioRaiseVolume => FunctionKey::VolumeUp,
            Keysym::XF86_AudioLowerVolume => FunctionKey::VolumeDown,
            Keysym::XF86_AudioMute => FunctionKey::Mute,
            Keysym::XF86_AudioMicMute => FunctionKey::MicMute,
            Keysym::XF86_MonBrightnessUp => FunctionKey::BrightnessUp,
            Keysym::XF86_MonBrightnessDown => FunctionKey::BrightnessDown,
            Keysym::XF86_RFKill => FunctionKey::Airplane,
            Keysym::XF86_Calculator => FunctionKey::Calculator,
            Keysym::XF86_Search => FunctionKey::Search,
            Keysym::XF86_AudioPlay | Keysym::XF86_AudioPause => FunctionKey::Media(MediaCommand::PlayPause),
            Keysym::XF86_AudioNext => FunctionKey::Media(MediaCommand::Next),
            Keysym::XF86_AudioPrev => FunctionKey::Media(MediaCommand::Previous),
            _ => return None,
        })
    }

    /// Maintenue, la touche se répète (volume et luminosité).
    pub fn repeats(self) -> bool {
        matches!(
            self,
            FunctionKey::VolumeUp
                | FunctionKey::VolumeDown
                | FunctionKey::BrightnessUp
                | FunctionKey::BrightnessDown
        )
    }

    /// Utilisable sur l'écran de verrouillage : les réglages, pas les applications.
    pub fn works_locked(self) -> bool {
        !matches!(self, FunctionKey::Calculator | FunctionKey::Search)
    }
}

/// Bulle d'un niveau de son.
fn volume_osd(level: u8, muted: bool) -> Osd {
    if muted {
        Osd {
            icon: svg::volume_icon(0),
            label: "Son coupé",
            level: None,
        }
    } else {
        Osd {
            // Les ondes allumées suivent le niveau, comme dans la barre du haut.
            icon: svg::volume_icon(level),
            label: "Volume",
            level: Some(level),
        }
    }
}

fn airplane_osd(on: bool) -> Osd {
    Osd {
        icon: "cc-airplane",
        label: if on {
            "Mode avion activé"
        } else {
            "Mode avion désactivé"
        },
        level: None,
    }
}

impl Ferrite {
    pub fn on_function_key(&mut self, key: FunctionKey) {
        match key {
            FunctionKey::VolumeUp | FunctionKey::VolumeDown => {
                let Some(volume) = self.system.volume else {
                    return;
                };
                let level = if key == FunctionKey::VolumeUp {
                    volume.level.saturating_add(STEP).min(100)
                } else {
                    volume.level.saturating_sub(STEP)
                };
                self.request_system_change(Change::Volume(level));
                self.show_osd(volume_osd(level, level == 0));
            }
            FunctionKey::Mute => {
                let Some(volume) = self.system.volume else {
                    return;
                };
                let muted = !volume.muted;
                self.request_system_change(Change::Muted(muted));
                self.show_osd(volume_osd(volume.level, muted));
            }
            FunctionKey::MicMute => {
                let muted = !self.system.mic_muted.unwrap_or(false);
                self.request_system_change(Change::MicMuted(muted));
                self.show_osd(Osd {
                    icon: if muted { "cc-mic-off" } else { "cc-mic" },
                    label: if muted { "Micro coupé" } else { "Micro activé" },
                    level: None,
                });
            }
            FunctionKey::BrightnessUp | FunctionKey::BrightnessDown => {
                let Some(brightness) = self.system.brightness else {
                    return;
                };
                let level = if key == FunctionKey::BrightnessUp {
                    brightness.saturating_add(STEP).min(100)
                } else {
                    // Jamais d'écran noir : 5 % au moins, comme le curseur des réglages.
                    brightness.saturating_sub(STEP).max(STEP)
                };
                self.request_system_change(Change::Brightness(level));
                self.show_osd(Osd {
                    icon: "cc-brightness",
                    label: "Luminosité",
                    level: Some(level),
                });
            }
            FunctionKey::Airplane => self.toggle_airplane(),
            FunctionKey::Calculator => self.activate_app(&AppKey::Builtin(App::CALCULATOR)),
            FunctionKey::Search => {
                self.handle_desktop_action(crate::desktop::panels::DesktopAction::TogglePopover(
                    Popover::Launcher,
                ));
            }
            FunctionKey::Media(command) => self.press_media(command),
        }
    }

    /// Le noyau bascule parfois lui-même les radios sur cette touche (`rfkill-input`), mais
    /// chaque sorte à part : un Wi-Fi allumé et un Bluetooth éteint restaient décalés. Un
    /// instant après, on impose donc à toutes les radios l'état voulu, calculé avant l'appui.
    fn toggle_airplane(&mut self) {
        let Some(before) = rfkill::airplane() else {
            return;
        };
        let _ = self
            .loop_handle
            .insert_source(Timer::from_duration(RFKILL_SETTLE), move |_, _, state| {
                state.request_system_change(Change::Airplane(!before));
                state.show_osd(airplane_osd(!before));
                TimeoutAction::Drop
            });
    }

    /// Affiche `osd` un instant ; une nouvelle bulle remplace la précédente.
    pub fn show_osd(&mut self, osd: Osd) {
        self.osd = Some((osd, Instant::now() + OSD_DURATION));
        self.request_redraw();
        // La bulle disparaît seule : une image de plus à son échéance.
        let _ = self
            .loop_handle
            .insert_source(Timer::from_duration(OSD_DURATION), |_, _, state| {
                state.request_redraw();
                TimeoutAction::Drop
            });
    }

    /// Bulle à montrer, tant qu'elle n'a pas expiré.
    pub fn osd_view(&self) -> Option<Osd> {
        self.osd
            .filter(|(_, until)| Instant::now() < *until)
            .map(|(osd, _)| osd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touches_de_fonction_du_thinkbook() {
        assert_eq!(
            FunctionKey::from_keysym(Keysym::XF86_AudioRaiseVolume),
            Some(FunctionKey::VolumeUp)
        );
        assert_eq!(
            FunctionKey::from_keysym(Keysym::XF86_AudioMicMute),
            Some(FunctionKey::MicMute)
        );
        assert_eq!(
            FunctionKey::from_keysym(Keysym::XF86_MonBrightnessDown),
            Some(FunctionKey::BrightnessDown)
        );
        assert_eq!(
            FunctionKey::from_keysym(Keysym::XF86_RFKill),
            Some(FunctionKey::Airplane)
        );
        assert_eq!(
            FunctionKey::from_keysym(Keysym::XF86_Calculator),
            Some(FunctionKey::Calculator)
        );
        assert_eq!(FunctionKey::from_keysym(Keysym::a), None);
        assert!(FunctionKey::VolumeUp.repeats() && !FunctionKey::Mute.repeats());
        assert!(FunctionKey::Airplane.works_locked() && !FunctionKey::Calculator.works_locked());
    }

    #[test]
    fn bulle_du_son() {
        assert_eq!(volume_osd(40, false).level, Some(40));
        assert_eq!(volume_osd(40, true).icon, "status-volume-0");
        assert_eq!(volume_osd(90, false).icon, "status-volume-3");
        assert_eq!(volume_osd(0, true).level, None);
    }
}
