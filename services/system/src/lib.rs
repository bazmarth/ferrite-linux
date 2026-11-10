//! Services du système pour le bureau Ferrite.
//!
//! Chaque fonction lance un outil standard de Debian (`nmcli`, `bluetoothctl`, `wpctl`,
//! `brightnessctl`, `timedatectl`) avec un **délai maximal** : un outil qui attend un
//! démon absent (Bluetooth sans adaptateur, par exemple) est arrêté au lieu de figer
//! l'appelant. Les fonctions restent toutefois bloquantes le temps de ce délai ; le
//! compositeur et les applications les appellent depuis un fil d'exécution à part.
//!
//! Les analyses de sortie sont des fonctions pures, testées sans le matériel.

pub mod about;
pub mod agenda;
pub mod applications;
pub mod audio;
pub mod bluetooth;
pub mod catalog;
pub mod command;
pub mod config;
pub mod cursors;
pub mod defaults;
pub mod display;
pub mod drag;
pub mod formats;
pub mod idle;
pub mod network;
pub mod night;
pub mod power;
pub mod printers;
pub mod rfkill;
pub mod screens;
pub mod shortcuts;
pub mod sounds;
pub mod time;
pub mod weather;

/// État rapide du système, affiché dans la barre et les réglages rapides. `None` :
/// matériel ou service absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub wifi: Option<bool>,
    /// Force du signal (0 à 100) du réseau Wi-Fi connecté.
    pub wifi_signal: Option<u8>,
    pub bluetooth: Option<bool>,
    pub volume: Option<audio::Volume>,
    pub brightness: Option<u8>,
    /// Micro par défaut coupé.
    pub mic_muted: Option<bool>,
    pub airplane: Option<bool>,
    /// Rétroéclairage du clavier : niveau et maximum.
    pub keyboard_light: Option<(u8, u8)>,
    /// Profil « économie d'énergie » actif (`power-profiles-daemon`).
    pub power_saver: Option<bool>,
}

impl Snapshot {
    /// Lit l'état de chaque service (quelques secondes au pire, jamais indéfiniment).
    pub fn read() -> Snapshot {
        let wifi = network::wifi_enabled();
        Snapshot {
            wifi,
            wifi_signal: if wifi == Some(true) {
                network::wifi_signal()
            } else {
                None
            },
            bluetooth: bluetooth::powered(),
            volume: audio::volume(),
            brightness: display::brightness(),
            mic_muted: audio::input_muted(),
            airplane: rfkill::airplane(),
            keyboard_light: display::keyboard_light(),
            power_saver: power::power_saver(),
        }
    }
}
