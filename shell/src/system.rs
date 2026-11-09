//! Lien avec les services du système (réseau, Bluetooth, son, écran, alimentation).
//!
//! Les commandes système peuvent attendre plusieurs secondes (ou un démon absent) : elles
//! sont exécutées dans un fil d'exécution à part, jamais dans la boucle du compositeur.
//! Le fil renvoie l'état relu du système par un canal calloop, et relit cet état de
//! lui-même toutes les quelques secondes pour suivre les changements faits ailleurs.

use std::{
    sync::Arc,
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::{Duration, Instant, SystemTime},
};

use ferrite_system::{
    Snapshot,
    applications::{self, DesktopEntry},
    audio, bluetooth, command,
    config::{Accent, Appearance, Settings},
    display, network, power,
    rfkill::{self, Radio},
};
use ferrite_ui::wallpaper::{self, Builtin, Image, Source};
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
    Muted(bool),
    MicMuted(bool),
    Brightness(u8),
    /// Toutes les radios bloquées (`true`) ou débloquées.
    Airplane(bool),
    /// Niveau du rétroéclairage du clavier.
    KeyboardLight(u8),
    /// Profil d'énergie du système.
    PowerSaver(bool),
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
    KeyboardSettings(ferrite_system::config::Settings),
    /// Listes du centre de contrôle, relues à la demande.
    WifiList(Vec<network::WifiNetwork>, Vec<String>),
    BluetoothList(Vec<bluetooth::Device>),
    BluetoothScanDone,
    /// Fin d'une connexion ou d'une déconnexion demandée depuis le panneau.
    BluetoothDone,
    /// Mot de passe enregistré d'un réseau, demandé par l'œil de sa ligne.
    WifiPassword(String, Result<String, String>),
    Dock(DockConfig),
    Theme(Appearance, Accent),
    ScrollInverted(bool),
    /// Notifications : ne pas déranger, son, aperçu dans la bannière.
    Notifications(crate::desktop::notifications::Preferences),
    /// Vitesses de défilement (molette, pavé tactile), en pour cent.
    ScrollSpeed(u32, u32),
    /// Taille du curseur, et thème de curseurs s'il a changé (déjà lu ; `Some(None)` :
    /// formes vectorielles).
    CursorSize(u32, Option<Option<Arc<crate::render::ThemeCursors>>>),
    Wallpaper(WallpaperChoice),
    /// Applications épinglées dans le Dock, et applications à plusieurs fenêtres.
    DockApps(Option<Vec<String>>, Vec<String>),
    Applications(Vec<DesktopEntry>),
    /// Raccourcis du bureau, relus quand leur fichier change.
    Shortcuts(ferrite_system::shortcuts::Desktop),
    PowerAccepted,
    /// La commande de veille a échoué : le voile noir ne doit pas rester affiché.
    SleepFailed(String),
    /// Photo de l'écran de verrouillage, décodée (ou retirée).
    LockPicture(Option<crate::desktop::panels::LockPicture>),
    /// Image de profil du compte, relue quand elle change.
    LockAvatar(Option<crate::clipboard::Thumbnail>),
    /// Délais de verrouillage et de mise en veille automatiques.
    Idle(ferrite_system::idle::IdleSettings),
    /// Réglages du mode nuit et ville (pour le lever et le coucher du soleil).
    Night(
        ferrite_system::night::NightSettings,
        ferrite_system::weather::Place,
    ),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallpaperChoice {
    pub source: Source,
    pub image: Option<Image>,
    pub generation: u64,
    pub bar_dark: bool,
    pub backdrop: Option<Arc<Image>>,
    /// Fond à peine flouté, vu au travers du verre liquide des widgets.
    pub glass: Option<Arc<Image>>,
}

impl WallpaperChoice {
    pub fn from_settings(settings: &Settings, generation: u64) -> Self {
        let source = Source::parse(settings.wallpaper());
        let image = match &source {
            Source::Builtin(_) => None,
            Source::File(path) => match wallpaper::decode(path) {
                Ok(image) => Some(image),
                Err(error) => {
                    warn!(%error, "fond d'écran illisible ; fond Ferrite utilisé");
                    None
                }
            },
        };
        Self::from_parts(source, image, generation)
    }

    fn from_parts(source: Source, image: Option<Image>, generation: u64) -> Self {
        let preview = match (&source, &image) {
            (Source::Builtin(builtin), _) => wallpaper::paint(*builtin, 320, 200),
            (Source::File(_), Some(image)) => wallpaper::cover(image, 320, 200),
            (Source::File(_), None) => wallpaper::paint(Builtin::Ferrite, 320, 200),
        };
        let bar_dark = preview.top_is_dark(24);
        let backdrop = Some(Arc::new(wallpaper::blur(&preview, 10)));
        // Flou du verre des widgets : assez large pour ne garder que des teintes douces.
        let glass = Some(Arc::new(wallpaper::blur(&preview, 6)));
        Self {
            source,
            image,
            generation,
            bar_dark,
            backdrop,
            glass,
        }
    }
}

impl Default for WallpaperChoice {
    fn default() -> Self {
        Self::from_parts(Source::default(), None, 0)
    }
}

/// Côté compositeur du lien : envoie les demandes au fil système.
#[derive(Debug)]
pub struct SystemLink {
    sender: mpsc::Sender<Change>,
    events: channel::Sender<Event>,
}

impl SystemLink {
    /// Lance une opération lente (liste des réseaux, connexion) sur son propre fil ; ses
    /// résultats reviennent à la boucle.
    pub fn run(&self, job: impl FnOnce() -> Vec<Event> + Send + 'static) {
        let events = self.events.clone();
        std::thread::spawn(move || {
            for event in job() {
                if events.send(event).is_err() {
                    return;
                }
            }
        });
    }

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
                // Les essais de rendu s'espacent au repos : on prévient tout de suite.
                state.request_redraw();
            }
        })
        .map_err(|err| format!("canal système : {err}"))?;
    // Dans une fenêtre d'un autre bureau, ses réglages ne sont pas les nôtres à changer.
    let nested = std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some();
    thread::Builder::new()
        .name("ferrite-systeme".into())
        .spawn({
            let events = events.clone();
            move || worker(receiver, events, nested)
        })
        .map_err(|err| format!("fil système : {err}"))?;
    Ok(SystemLink { sender, events })
}

/// Annonce le thème aux applications tierces : GTK, et par le portail Firefox ou
/// Chromium, suivent cette clé en direct.
fn publish_color_scheme(dark: bool) {
    let scheme = if dark { "prefer-dark" } else { "default" };
    let args = ["set", "org.gnome.desktop.interface", "color-scheme", scheme];
    if let Err(err) = command::run("gsettings", &args) {
        warn!("thème des applications tierces non transmis : {err}");
    }
}

/// Boutons de fenêtre des applications GTK (et de Firefox, Chromium, Brave, qui suivent
/// GTK) : réduire, agrandir et fermer à droite, comme les fenêtres Ferrite.
const WINDOW_BUTTONS: &str = ":minimize,maximize,close";

/// Annonce l'ordre des boutons de fenêtre aux applications tierces, par la clé GNOME que
/// lisent GTK et le portail des réglages.
fn publish_window_buttons() {
    let args = [
        "set",
        "org.gnome.desktop.wm.preferences",
        "button-layout",
        WINDOW_BUTTONS,
    ];
    if let Err(err) = command::run("gsettings", &args) {
        warn!("ordre des boutons de fenêtre non transmis : {err}");
    }
}

/// Taille du texte des applications GTK (et de celles qui suivent GTK), comme la clé
/// `text-scaling-factor` de GNOME ; les applications Ferrite lisent le réglage elles-mêmes.
fn publish_text_size(percent: u32) {
    let factor = format!("{:.2}", f64::from(percent) / 100.0);
    let args = [
        "set",
        "org.gnome.desktop.interface",
        "text-scaling-factor",
        &factor,
    ];
    if let Err(err) = command::run("gsettings", &args) {
        warn!("taille du texte des applications tierces non transmise : {err}");
    }
}

fn worker(receiver: mpsc::Receiver<Change>, events: channel::Sender<Event>, nested: bool) {
    if !nested {
        publish_window_buttons();
    }
    let mut layout_stamp: Option<SystemTime> = None;
    let mut settings_initialized = false;
    let mut known_theme = None;
    let mut known_dark = None;
    let mut known_scroll_inverted = None;
    let mut known_notifications = None;
    let mut notification_settings: Option<Settings> = None;
    let mut known_scroll_speed = None;
    let mut known_cursor: Option<(String, u32)> = None;
    let mut known_text_size = None;
    let mut known_wallpaper = None;
    let mut known_lock_picture: Option<String> = None;
    let mut known_avatar: Option<Option<(std::path::PathBuf, Option<SystemTime>)>> = None;
    let mut wallpaper_generation = 0u64;
    let mut last_snapshot: Option<Instant> = None;
    let mut applications_stamp = None;
    let mut shortcuts_stamp = None;
    let mut shortcuts_initialized = false;
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
        // Image de profil (Paramètres → Comptes) : l'écran de verrouillage suit dans la
        // seconde. Décodée ici, seulement quand le fichier change.
        let avatar = crate::desktop::panels::avatar_path().map(|path| {
            let modified = std::fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok();
            (path, modified)
        });
        if known_avatar.as_ref() != Some(&avatar) {
            let picture = avatar
                .as_ref()
                .and_then(|(path, _)| crate::desktop::panels::load_avatar(path));
            known_avatar = Some(avatar);
            if events.send(Event::LockAvatar(picture)).is_err() {
                return;
            }
        }
        // Raccourcis du bureau, ajoutés aussi par Fichiers : affichés dans la seconde.
        let stamp = ferrite_system::shortcuts::modified();
        if !shortcuts_initialized || stamp != shortcuts_stamp {
            shortcuts_initialized = true;
            shortcuts_stamp = stamp;
            if events
                .send(Event::Shortcuts(ferrite_system::shortcuts::load()))
                .is_err()
            {
                return;
            }
        }
        // Réglages (clavier, Dock) : appliqués sans redémarrer, une seconde au plus après
        // leur changement dans Paramètres.
        let stamp = Settings::modified();
        if !settings_initialized || stamp != layout_stamp {
            settings_initialized = true;
            layout_stamp = stamp;
            let settings = Settings::load();
            if events.send(Event::KeyboardSettings(settings.clone())).is_err() {
                return;
            }
            let dock = DockConfig::from_settings(&settings);
            if events.send(Event::Dock(dock)).is_err() {
                return;
            }
            let theme = (settings.appearance(), settings.accent());
            if known_theme != Some(theme) {
                known_theme = Some(theme);
                if events.send(Event::Theme(theme.0, theme.1)).is_err() {
                    return;
                }
            }
            let dark = settings.appearance().is_dark();
            if known_dark != Some(dark) {
                known_dark = Some(dark);
                if !nested {
                    publish_color_scheme(dark);
                }
            }
            // Photo de l'écran de verrouillage : décodée ici, une fois par choix.
            let lock_path = settings
                .get("verrouillage_fond")
                .filter(|value| value.starts_with('/'))
                .map(str::to_owned);
            if lock_path != known_lock_picture {
                known_lock_picture.clone_from(&lock_path);
                let picture = lock_path
                    .and_then(|path| crate::desktop::panels::LockPicture::load(std::path::Path::new(&path)));
                if events.send(Event::LockPicture(picture)).is_err() {
                    return;
                }
            }
            if events
                .send(Event::Idle(ferrite_system::idle::IdleSettings::load(&settings)))
                .is_err()
            {
                return;
            }
            let night = ferrite_system::night::NightSettings::load(&settings);
            let place = ferrite_system::weather::Place::from_settings(&settings);
            if events.send(Event::Night(night, place)).is_err() {
                return;
            }
            notification_settings = Some(settings.clone());
            let scroll_inverted = settings.scroll_inverted();
            if known_scroll_inverted != Some(scroll_inverted) {
                known_scroll_inverted = Some(scroll_inverted);
                if events.send(Event::ScrollInverted(scroll_inverted)).is_err() {
                    return;
                }
            }
            let speed = (settings.scroll_speed_wheel(), settings.scroll_speed_touchpad());
            if known_scroll_speed != Some(speed) {
                known_scroll_speed = Some(speed);
                if events.send(Event::ScrollSpeed(speed.0, speed.1)).is_err() {
                    return;
                }
            }
            let cursor = (settings.cursor_theme().to_owned(), settings.cursor_size());
            if known_cursor.as_ref() != Some(&cursor) {
                // Le thème n'est relu que s'il change (pas pour une nouvelle taille).
                let reload = known_cursor.as_ref().is_none_or(|(theme, _)| *theme != cursor.0);
                let theme = if reload {
                    let theme = crate::render::ThemeCursors::load(&cursor.0).map(Arc::new);
                    if theme.is_none() && cursor.0 != ferrite_system::cursors::BUILTIN {
                        warn!(theme = %cursor.0, "thème de curseurs illisible : formes Ferrite");
                    }
                    Some(theme)
                } else {
                    None
                };
                known_cursor = Some(cursor.clone());
                if events.send(Event::CursorSize(cursor.1, theme)).is_err() {
                    return;
                }
            }
            let text_size = settings.text_size();
            if known_text_size != Some(text_size) {
                known_text_size = Some(text_size);
                if !nested {
                    publish_text_size(text_size);
                }
            }
            let wallpaper_key = Source::parse(settings.wallpaper()).key();
            if known_wallpaper.as_deref() != Some(wallpaper_key.as_str()) {
                known_wallpaper = Some(wallpaper_key);
                wallpaper_generation = wallpaper_generation.wrapping_add(1);
                if events
                    .send(Event::Wallpaper(WallpaperChoice::from_settings(
                        &settings,
                        wallpaper_generation,
                    )))
                    .is_err()
                {
                    return;
                }
            }
            let apps = Event::DockApps(settings.dock_pinned(), settings.multi_window_apps());
            if events.send(apps).is_err() {
                return;
            }
        }
        // Notifications : recalculées à chaque tour pour « Ne pas déranger » programmé, qui
        // change avec l'heure ; envoyées seulement si quelque chose a changé.
        if let Some(settings) = &notification_settings {
            let minute = crate::desktop::notifications::local_minute();
            let notifications = crate::desktop::notifications::Preferences::from_settings(settings, minute);
            if known_notifications.as_ref() != Some(&notifications) {
                known_notifications = Some(notifications.clone());
                if events.send(Event::Notifications(notifications)).is_err() {
                    return;
                }
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
                // La veille laisse le bureau ouvert ; l'arrêt et le redémarrage le ferment.
                (Change::Power(PowerAction::PowerOff | PowerAction::Restart), Ok(())) => Event::PowerAccepted,
                (_, Ok(())) => continue,
                (Change::Power(PowerAction::Sleep), Err(message)) => Event::SleepFailed(message),
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
        Change::Wifi(enabled) => {
            // Un blocage resté d'un mode avion ferait échouer NetworkManager.
            if enabled {
                let _ = rfkill::set_blocked(Radio::Wlan, false);
            }
            network::set_wifi(enabled)
        }
        Change::Bluetooth(enabled) => {
            if enabled {
                let _ = rfkill::set_blocked(Radio::Bluetooth, false);
            }
            bluetooth::set_powered(enabled)
        }
        Change::Volume(level) => audio::set_volume(level),
        Change::Muted(muted) => audio::set_muted(muted),
        Change::MicMuted(muted) => audio::set_input_muted(muted),
        Change::Brightness(level) => display::set_brightness(level),
        Change::Airplane(on) => rfkill::set_airplane(on),
        Change::KeyboardLight(level) => display::set_keyboard_light(level),
        Change::PowerSaver(enabled) => power::set_power_saver(enabled),
        Change::Power(power) => command::run(
            "systemctl",
            &[match power {
                PowerAction::PowerOff => "poweroff",
                PowerAction::Restart => "reboot",
                PowerAction::Sleep => "suspend",
                // Gérées par le bureau lui-même (voir `PowerAction::exit_code`).
                PowerAction::LogOut | PowerAction::RelaunchDesktop => return Ok(()),
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
            Event::SleepFailed(message) => {
                warn!(%message, "mise en veille refusée");
                self.reveal_screen();
                self.system_error = Some(message);
            }
            Event::KeyboardSettings(settings) => {
                if self.keyboard_shortcuts.keyboard_layout() != settings.keyboard_layout() {
                    self.set_keyboard_layout(settings.keyboard_layout());
                }
                self.keyboard_shortcuts = settings;
            }
            Event::WifiList(networks, known) => {
                self.controls.networks = networks;
                self.controls.known = known;
            }
            Event::BluetoothList(devices) => self.controls.devices = devices,
            Event::BluetoothScanDone => self.controls.scanning = false,
            Event::BluetoothDone => self.controls.busy = None,
            Event::WifiPassword(ssid, result) => match result {
                Ok(password) => self.controls.saved_password = Some((ssid, password)),
                Err(message) => self.system_error = Some(message),
            },
            Event::Dock(config) => {
                ferrite_ui::theme::set_high_contrast(config.high_contrast);
                if !config.sticky_keys {
                    self.release_sticky_keys();
                }
                self.dock_config = config;
                if config.tiling != self.tiling {
                    self.toggle_tiling();
                }
            }
            Event::Theme(appearance, accent) => {
                self.appearance = appearance;
                self.accent = accent;
            }
            Event::ScrollInverted(inverted) => self.scroll_inverted = inverted,
            Event::Notifications(preferences) => self.notifications.set_preferences(preferences),
            Event::ScrollSpeed(wheel, touchpad) => self.scroll_speed = (wheel, touchpad),
            Event::CursorSize(size, theme) => {
                self.cursor_size = size;
                if let Some(theme) = theme {
                    self.cursor_theme = theme;
                }
            }
            Event::LockAvatar(picture) => self.lock_avatar = picture,
            Event::LockPicture(picture) => {
                self.lock_picture = picture;
                // Choisie pendant que l'écran est verrouillé : elle s'affiche tout de suite.
                if let Some(lock) = self.lock.as_mut()
                    && self.lock_picture.is_some()
                    && ferrite_system::config::Settings::load()
                        .get("verrouillage_fond")
                        .is_some_and(|value| value.starts_with('/'))
                {
                    lock.wall = crate::desktop::panels::LOCK_WALLS.len();
                }
            }
            Event::Idle(settings) => self.idle.settings = settings,
            Event::Night(night, place) => {
                if self.night.settings != night || self.night.place.as_ref() != Some(&place) {
                    self.night.settings = night;
                    self.night.place = Some(place);
                    self.night.checked = None;
                }
            }
            Event::Wallpaper(wallpaper) => self.wallpaper = wallpaper,
            Event::DockApps(pinned, multi) => {
                self.dock_pinned = pinned;
                self.multi_window = multi;
            }
            Event::Applications(list) => {
                info!(nombre = list.len(), "applications installées");
                self.applications = list;
                self.restore_session_after_applications();
            }
            Event::Shortcuts(list) => self.desktop_shortcuts = list,
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
            Change::Muted(muted) => {
                self.volume_held_until = Some(Instant::now() + HOLD);
                if let Some(volume) = self.system.volume.as_mut() {
                    volume.muted = muted;
                }
            }
            Change::MicMuted(muted) => self.system.mic_muted = Some(muted),
            Change::Airplane(on) => self.system.airplane = Some(on),
            Change::KeyboardLight(level) => {
                self.system.keyboard_light = self.system.keyboard_light.map(|(_, max)| (level, max));
            }
            Change::PowerSaver(enabled) => self.system.power_saver = self.system.power_saver.map(|_| enabled),
            Change::Brightness(level) => {
                self.brightness_held_until = Some(Instant::now() + HOLD);
                self.system.brightness = self.system.brightness.map(|_| level);
            }
            Change::Power(_) => {}
        }
        match &self.system_link {
            Some(link) => link.request(change),
            None => {
                warn!(?change, "services système indisponibles");
                if change == Change::Power(PowerAction::Sleep) {
                    self.reveal_screen();
                    self.system_error = Some("Services système indisponibles".to_owned());
                }
            }
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
