//! Listes du centre de contrôle : le chevron de Wi-Fi ou de Bluetooth les ouvre dans le
//! panneau. Tout ce qui interroge le système passe par des fils à part.

use std::time::{Duration, Instant};

use ferrite_system::{
    bluetooth::{self, Device},
    network::{self, WifiNetwork},
};

use crate::{
    desktop::panels::{ControlsPage, DeviceRow, NetworkRow, PasswordRow},
    state::Ferrite,
    system::Event,
};

/// Analyse automatique, tant que le panneau est ouvert.
const RESCAN: Duration = Duration::from_secs(10);
/// Lignes affichées au plus, dans chaque liste.
const MAX_ROWS: usize = 8;

/// Mot de passe en cours de saisie, sur la ligne de son réseau.
#[derive(Debug)]
pub struct PasswordEntry {
    pub ssid: String,
    pub text: String,
}

/// État des listes du centre de contrôle.
#[derive(Debug, Default)]
pub struct Controls {
    pub page: ControlsPage,
    pub networks: Vec<WifiNetwork>,
    /// Réseaux enregistrés : ils se rejoignent sans mot de passe.
    pub known: Vec<String>,
    pub devices: Vec<Device>,
    pub password: Option<PasswordEntry>,
    pub scanning: bool,
    scanned: Option<Instant>,
}

impl Controls {
    /// Réseaux : le connecté d'abord, puis les autres du plus fort au plus faible.
    pub fn network_rows(&self) -> Vec<NetworkRow> {
        self.networks
            .iter()
            .take(MAX_ROWS)
            .enumerate()
            .map(|(index, network)| NetworkRow {
                index,
                ssid: network.ssid.clone(),
                level: network.level(),
                secured: network.secured,
                connected: network.connected,
                hint: self.network_hint(network),
            })
            .collect()
    }

    fn network_hint(&self, network: &WifiNetwork) -> String {
        if network.connected {
            let summary = network.summary();
            // « Connecté · 5 GHz » : la sécurité se voit au cadenas, le débit dans Paramètres.
            let band = summary.split(" · ").find(|part| part.ends_with("GHz"));
            return band.map_or("Connecté".to_owned(), |band| format!("Connecté · {band}"));
        }
        if self.known.contains(&network.ssid) {
            "Réseau connu".to_owned()
        } else if network.secured {
            "Sécurisé".to_owned()
        } else {
            "Ouvert · non sécurisé".to_owned()
        }
    }

    pub fn password_row(&self) -> Option<PasswordRow> {
        self.password.as_ref().map(|entry| PasswordRow {
            ssid: entry.ssid.clone(),
            length: entry.text.chars().count(),
        })
    }

    pub fn device_rows(&self) -> Vec<DeviceRow> {
        self.devices
            .iter()
            .enumerate()
            .take(MAX_ROWS)
            .map(|(index, device)| DeviceRow {
                index,
                name: device.name.clone(),
                paired: device.paired,
                connected: device.connected,
                hint: device_hint(device),
                glyph: device_glyph(device),
            })
            .collect()
    }

    /// Nom du réseau connecté.
    pub fn current_network(&self) -> Option<&str> {
        self.networks
            .iter()
            .find(|network| network.connected)
            .map(|network| network.ssid.as_str())
    }

    pub fn connected_devices(&self) -> usize {
        self.devices.iter().filter(|device| device.connected).count()
    }
}

fn device_hint(device: &Device) -> String {
    match (device.connected, device.paired, device.battery) {
        (true, _, Some(battery)) => format!("Connecté · batterie {battery} %"),
        (true, _, None) => "Connecté".to_owned(),
        (false, true, _) => "Non connecté".to_owned(),
        (false, false, _) => "Prêt à associer".to_owned(),
    }
}

/// Symbole d'un appareil, selon le genre qu'il annonce ou son nom.
fn device_glyph(device: &Device) -> &'static str {
    let name = device.name.to_lowercase();
    let is = |known: &str, word: &str| device.icon.contains(known) || name.contains(word);
    if is("audio-head", "casque") {
        "cc-head"
    } else if is("mouse", "souris") {
        "cc-mouse"
    } else if is("keyboard", "clavier") {
        "cc-keyboard"
    } else if is("phone", "téléphone") {
        "cc-phone"
    } else if is("watch", "montre") {
        "cc-watch"
    } else if is("audio", "enceinte") {
        "cc-speaker"
    } else {
        "cc-bluetooth"
    }
}

/// Réseaux à portée et réseaux enregistrés.
fn wifi_lists() -> Vec<Event> {
    let known = network::known_wifi()
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    vec![Event::WifiList(
        network::wifi_networks().unwrap_or_default(),
        known,
    )]
}

fn bluetooth_list() -> Event {
    Event::BluetoothList(bluetooth::devices().unwrap_or_default())
}

impl Ferrite {
    /// Ouverture du centre de contrôle : retour à ses tuiles, listes relues.
    pub fn controls_opened(&mut self) {
        self.controls.page = ControlsPage::Main;
        self.controls.password = None;
        self.controls.scanned = None;
    }

    /// À chaque tour de boucle, panneau ouvert : relance l'analyse toutes les 10 s.
    pub fn refresh_controls(&mut self) {
        use crate::desktop::panels::Popover;
        if self.popover != Some(Popover::Controls)
            || self.controls.scanned.is_some_and(|at| at.elapsed() < RESCAN)
        {
            return;
        }
        self.controls.scanned = Some(Instant::now());
        let Some(link) = &self.system_link else {
            return;
        };
        if self.system.wifi == Some(true) {
            link.run(wifi_lists);
        }
        if self.system.bluetooth == Some(true) {
            let scan = self.controls.page == ControlsPage::Bluetooth;
            if scan {
                self.controls.scanning = true;
            }
            link.run(move || {
                let mut events = Vec::new();
                if scan {
                    let _ = bluetooth::scan();
                    events.push(Event::BluetoothScanDone);
                }
                events.push(bluetooth_list());
                events
            });
        }
    }

    pub fn show_controls_page(&mut self, page: ControlsPage) {
        self.controls.page = page;
        self.controls.password = None;
        self.controls.scanned = None;
    }

    /// Clic sur un réseau : connu ou ouvert, il est rejoint ; sinon le mot de passe se
    /// demande sur place. Le réseau connecté n'a rien à faire.
    pub fn join_network(&mut self, index: usize) {
        let Some(network) = self.controls.networks.get(index).cloned() else {
            return;
        };
        if network.connected {
            return;
        }
        if network.secured && !self.controls.known.contains(&network.ssid) {
            self.controls.password = Some(PasswordEntry {
                ssid: network.ssid,
                text: String::new(),
            });
            return;
        }
        self.connect_network(network.ssid, None);
    }

    fn connect_network(&mut self, ssid: String, password: Option<String>) {
        self.system_error = None;
        if let Some(link) = &self.system_link {
            link.run(move || {
                let result = network::connect_wifi(&ssid, password.as_deref());
                let mut events = wifi_lists();
                if let Err(error) = result {
                    events.push(Event::Failed(error));
                }
                events
            });
        }
    }

    pub fn password_type(&mut self, character: char) {
        if let Some(entry) = self.controls.password.as_mut() {
            entry.text.push(character);
        }
    }

    pub fn password_erase(&mut self) {
        if let Some(entry) = self.controls.password.as_mut() {
            entry.text.pop();
        }
    }

    pub fn password_cancel(&mut self) {
        self.controls.password = None;
    }

    pub fn password_submit(&mut self) {
        if let Some(entry) = self.controls.password.take() {
            self.connect_network(entry.ssid, Some(entry.text));
        }
    }

    /// Bouton d'un appareil : connecter, associer ou déconnecter selon son état.
    pub fn device_action(&mut self, index: usize) {
        let Some(device) = self.controls.devices.get(index).cloned() else {
            return;
        };
        self.system_error = None;
        if let Some(link) = &self.system_link {
            link.run(move || {
                let result = if device.connected {
                    bluetooth::disconnect(&device.address)
                } else {
                    bluetooth::connect(&device.address)
                };
                let mut events = vec![bluetooth_list()];
                if let Err(error) = result {
                    events.push(Event::Failed(error));
                }
                events
            });
        }
    }

    /// Tuile Clavier : passe directement à l'autre disposition.
    pub fn toggle_keyboard_layout(&mut self) {
        let mut settings = ferrite_system::config::Settings::load();
        let next = next_layout(settings.keyboard_layout());
        settings.set("clavier", next);
        if let Err(error) = settings.save() {
            tracing::warn!(%error, "disposition du clavier non enregistrée");
        }
        self.set_keyboard_layout(next);
    }
}

/// Disposition suivante : on bascule entre français et américain.
fn next_layout(current: &str) -> &'static str {
    if current == "fr" { "us" } else { "fr" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(ssid: &str, secured: bool, connected: bool) -> WifiNetwork {
        WifiNetwork {
            ssid: ssid.to_owned(),
            signal: 80,
            secured,
            connected,
            security: String::new(),
            frequency: "5180 MHz".to_owned(),
            rate: String::new(),
        }
    }

    #[test]
    fn indications_des_reseaux() {
        let controls = Controls {
            networks: vec![
                network("Maison", true, true),
                network("Atelier", true, false),
                network("Invités", true, false),
                network("Café", false, false),
            ],
            known: vec!["Atelier".to_owned()],
            ..Controls::default()
        };
        let hints: Vec<String> = controls.network_rows().into_iter().map(|row| row.hint).collect();
        assert_eq!(
            hints,
            [
                "Connecté · 5 GHz",
                "Réseau connu",
                "Sécurisé",
                "Ouvert · non sécurisé"
            ]
        );
        assert_eq!(controls.current_network(), Some("Maison"));
    }

    #[test]
    fn la_tuile_clavier_bascule() {
        assert_eq!(next_layout("fr"), "us");
        assert_eq!(next_layout("us"), "fr");
        assert_eq!(next_layout("de"), "fr");
    }

    #[test]
    fn indications_des_appareils() {
        let mut device = Device {
            address: "AA".to_owned(),
            name: "Mon casque".to_owned(),
            paired: true,
            connected: true,
            icon: "audio-headset".to_owned(),
            battery: Some(80),
        };
        assert_eq!(device_hint(&device), "Connecté · batterie 80 %");
        assert_eq!(device_glyph(&device), "cc-head");
        device.connected = false;
        assert_eq!(device_hint(&device), "Non connecté");
        device.paired = false;
        assert_eq!(device_hint(&device), "Prêt à associer");
    }
}
