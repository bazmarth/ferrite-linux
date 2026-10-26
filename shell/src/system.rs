//! Lien avec les services du système (réseau, Bluetooth, son, écran, alimentation).
//!
//! Les commandes système peuvent attendre plusieurs secondes (ou un démon absent) : elles
//! sont exécutées dans un fil d'exécution à part, jamais dans la boucle du compositeur.
//! Le fil renvoie l'état relu du système par un canal calloop, et relit cet état de
//! lui-même toutes les quelques secondes pour suivre les changements faits ailleurs.

use std::{
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::{Duration, Instant, SystemTime},
};

use ferrite_system::{
    Snapshot,
    applications::{self, DesktopEntry},
    audio, bluetooth, command,
    config::Settings,
    display, network,
};
use smithay::reexports::calloop::{
    LoopHandle,
    channel::{self, Channel},
};
use tracing::{info, warn};

use crate::{
    desktop::dock::DockConfig,
    state::{Ferrite, PowerAction},
};

/// Relecture périodique de l'état du système.
const REFRESH: Duration = Duration::from_secs(8);
/// Durée pendant laquelle une valeur réglée à la main prime sur la relecture du système.
const HOLD: Duration = Duration::from_secs(2);
/// Relecture du fichier de réglages (une simple date de modification).
const SETTINGS_POLL: Duration = Duration::from_secs(1);

/// Changement demandé depuis le bureau.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Wifi(bool),
    Bluetooth(bool),
    Volume(u8),
    Brightness(u8),
    Power(PowerAction),
}

impl Change {
    /// Deux changements du même réglage : seul le dernier compte.
    fn same_kind(self, other: Change) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }
}

/// Message du fil système vers le compositeur.
#[derive(Debug)]
pub enum Event {
    Snapshot(Snapshot),
    Failed(String),
    KeyboardLayout(String),
    Dock(DockConfig),
    Applications(Vec<DesktopEntry>),
    PowerAccepted,
}

/// Côté compositeur du lien : envoie les demandes au fil système.
#[derive(Debug)]
pub struct SystemLink {
    sender: mpsc::Sender<Change>,
}

impl SystemLink {
    pub fn request(&self, change: Change) {
        if self.sender.send(change).is_err() {
            warn!(?change, "fil système arrêté : demande ignorée");
        }
    }
}

/// Démarre le fil système et branche ses messages sur la boucle du compositeur.
pub fn start(handle: &LoopHandle<'static, Ferrite>) -> Result<SystemLink, String> {
    let (sender, receiver) = mpsc::channel::<Change>();
    let (events, channel): (channel::Sender<Event>, Channel<Event>) = channel::channel();
    handle
        .insert_source(channel, |event, _, state| {
            if let channel::Event::Msg(event) = event {
                state.on_system_event(event);
            }
        })
        .map_err(|err| format!("canal système : {err}"))?;
    thread::Builder::new()
        .name("ferrite-systeme".into())
        .spawn(move || worker(receiver, events))
        .map_err(|err| format!("fil système : {err}"))?;
    Ok(SystemLink { sender })
}

fn worker(receiver: mpsc::Receiver<Change>, events: channel::Sender<Event>) {
    let mut layout_stamp: Option<SystemTime> = None;
    let mut last_snapshot: Option<Instant> = None;
    let mut layout = String::new();
    let mut applications_stamp = None;
    loop {
        // Applications installées ou retirées (apt, Flatpak, .deb) : le menu suit.
        let stamp = Some(applications::stamp());
        if stamp != applications_stamp {
            applications_stamp = stamp;
            if events
                .send(Event::Applications(applications::installed()))
                .is_err()
            {
                return;
            }
        }
        // Réglages (clavier, Dock) : appliqués sans redémarrer, une seconde au plus après
        // leur changement dans Paramètres.
        let stamp = Settings::modified();
        if stamp != layout_stamp {
            layout_stamp = stamp;
            let settings = Settings::load();
            let current = settings.keyboard_layout().to_owned();
            if current != layout {
                layout.clone_from(&current);
                if events.send(Event::KeyboardLayout(current)).is_err() {
                    return;
                }
            }
            let dock = DockConfig {
                size: settings.dock_size(),
                hiding: settings.dock_hiding(),
            };
            if events.send(Event::Dock(dock)).is_err() {
                return;
            }
        }
        if last_snapshot.is_none_or(|at: Instant| at.elapsed() >= REFRESH) {
            last_snapshot = Some(Instant::now());
            if events.send(Event::Snapshot(Snapshot::read())).is_err() {
                return;
            }
        }

        let first = match receiver.recv_timeout(SETTINGS_POLL) {
            Ok(change) => change,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        // Un curseur glissé envoie beaucoup de valeurs : seule la dernière est appliquée.
        let mut changes = vec![first];
        while let Ok(change) = receiver.try_recv() {
            changes.retain(|known| !known.same_kind(change));
            changes.push(change);
        }
        last_snapshot = None;
        for change in changes {
            let result = apply(change);
            let event = match (change, result) {
                (Change::Power(_), Ok(())) => Event::PowerAccepted,
                (_, Ok(())) => continue,
                (_, Err(message)) => {
                    warn!(?change, %message, "réglage système refusé");
                    Event::Failed(message)
                }
            };
            if events.send(event).is_err() {
                return;
            }
        }
    }
}

fn apply(change: Change) -> Result<(), String> {
    match change {
        Change::Wifi(enabled) => network::set_wifi(enabled),
        Change::Bluetooth(enabled) => bluetooth::set_powered(enabled),
        Change::Volume(level) => audio::set_volume(level),
        Change::Brightness(level) => display::set_brightness(level),
        Change::Power(power) => command::run(
            "systemctl",
            &[match power {
                PowerAction::PowerOff => "poweroff",
                PowerAction::Restart => "reboot",
            }],
        )
        .map(drop),
    }
}

impl Ferrite {
    fn on_system_event(&mut self, event: Event) {
        match event {
            Event::Snapshot(mut snapshot) => {
                // Pendant un réglage (curseur glissé), la relecture du système peut dater
                // d'avant la dernière valeur envoyée : la valeur choisie reste affichée.
                let now = Instant::now();
                if self.volume_held_until.is_some_and(|until| now < until) {
                    snapshot.volume = self.system.volume;
                }
                if self.brightness_held_until.is_some_and(|until| now < until) {
                    snapshot.brightness = self.system.brightness;
                }
                self.system = snapshot;
            }
            Event::Failed(message) => self.system_error = Some(message),
            Event::KeyboardLayout(layout) => self.set_keyboard_layout(&layout),
            Event::Dock(config) => self.dock_config = config,
            Event::Applications(list) => {
                info!(nombre = list.len(), "applications installées");
                self.applications = list;
            }
            Event::PowerAccepted => {
                info!("arrêt du système accepté : fermeture du bureau");
                self.request_exit();
                self.loop_signal.stop();
            }
        }
    }

    /// Demande un changement au fil système et l'affiche tout de suite (l'état réel
    /// arrive ensuite et le corrige si la demande a échoué).
    pub fn request_system_change(&mut self, change: Change) {
        self.system_error = None;
        match change {
            Change::Wifi(enabled) => self.system.wifi = self.system.wifi.map(|_| enabled),
            Change::Bluetooth(enabled) => self.system.bluetooth = self.system.bluetooth.map(|_| enabled),
            Change::Volume(level) => {
                self.volume_held_until = Some(Instant::now() + HOLD);
                if let Some(volume) = self.system.volume.as_mut() {
                    volume.level = level;
                    volume.muted = level == 0;
                }
            }
            Change::Brightness(level) => {
                self.brightness_held_until = Some(Instant::now() + HOLD);
                self.system.brightness = self.system.brightness.map(|_| level);
            }
            Change::Power(_) => {}
        }
        match &self.system_link {
            Some(link) => link.request(change),
            None => warn!(?change, "services système indisponibles"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seul_le_dernier_reglage_d_un_meme_type_compte() {
        assert!(Change::Volume(10).same_kind(Change::Volume(80)));
        assert!(!Change::Volume(10).same_kind(Change::Brightness(10)));
    }
}
