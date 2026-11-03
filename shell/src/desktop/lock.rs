//! Écran de verrouillage : Super+L, bouton du centre de contrôle, ou avant une mise en veille.
//!
//! Tant qu'il est là, aucune fenêtre n'est dessinée ni ne reçoit d'entrée. Le mot de passe
//! est vérifié sur un autre fil par `unix_chkpwd`, l'auxiliaire de PAM prévu pour ça.

use std::{
    collections::HashMap,
    io::Write,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{
    desktop::panels::{LOCK_WALLS, LockMessage},
    state::Ferrite,
};

/// Auxiliaire de `pam_unix` : il ne vérifie que le compte qui l'appelle.
const HELPERS: [&str; 2] = ["/usr/sbin/unix_chkpwd", "/sbin/unix_chkpwd"];
/// Temps du passage du repos à la saisie, et de la secousse d'un refus.
const TRANSITION: Duration = Duration::from_millis(320);
const SHAKE: Duration = Duration::from_millis(380);
/// Message d'accueil, une fois le mot de passe accepté.
const WELCOME: Duration = Duration::from_millis(600);
/// Période de l'invitation qui pulse.
const PULSE: Duration = Duration::from_millis(2400);
/// Relecture du lecteur multimédia, tant que l'écran est verrouillé.
const MEDIA_REFRESH: Duration = Duration::from_secs(2);

/// État de l'écran de verrouillage.
#[derive(Debug)]
pub struct Lock {
    password: String,
    /// Le champ n'apparaît qu'après une touche ou un clic.
    pub field_visible: bool,
    /// Dernier passage entre le repos et la saisie.
    field_changed: Option<Instant>,
    pub checking: bool,
    /// Mot de passe accepté : l'écran part à cette heure.
    welcome_until: Option<Instant>,
    pub message: LockMessage,
    refused_at: Option<Instant>,
    opened_at: Instant,
    pub dark: bool,
    pub wall: usize,
    pub media: Option<Media>,
}

impl Lock {
    fn new(session_dark: bool) -> Self {
        let settings = ferrite_system::config::Settings::load();
        let dark = settings
            .get("verrouillage_theme")
            .map_or(session_dark, |theme| theme != "clair");
        let wall = settings
            .get("verrouillage_fond")
            .and_then(|name| LOCK_WALLS.iter().position(|wall| wall.0 == name))
            .unwrap_or(0);
        Self {
            password: String::new(),
            field_visible: false,
            field_changed: None,
            checking: false,
            welcome_until: None,
            message: LockMessage::Hint,
            refused_at: None,
            opened_at: Instant::now(),
            dark,
            wall,
            media: None,
        }
    }

    pub fn password_length(&self) -> usize {
        self.password.chars().count()
    }

    pub fn is_welcoming(&self) -> bool {
        self.welcome_until.is_some()
    }

    /// Part du passage à la saisie, de 0 à 1000 (`instant` : sans animation).
    pub fn progress(&self, instant: bool) -> u16 {
        let shown = self.field_changed.filter(|_| !instant).map_or(1.0, |changed| {
            (changed.elapsed().as_secs_f64() / TRANSITION.as_secs_f64()).min(1.0)
        });
        // Démarre vite et se pose en douceur.
        let eased = 1.0 - (1.0 - shown).powi(3);
        let part = if self.field_visible { eased } else { 1.0 - eased };
        (part * 1000.0).round() as u16
    }

    /// Décalage du champ après un refus, en dixièmes de point.
    pub fn shake(&self) -> i16 {
        let Some(refused) = self.refused_at else {
            return 0;
        };
        shake_offset(refused.elapsed().as_secs_f64() / SHAKE.as_secs_f64())
    }

    /// Niveau de l'invitation qui pulse, de 0 à 8.
    pub fn pulse(&self) -> u8 {
        let cycle = self.opened_at.elapsed().as_secs_f64() / PULSE.as_secs_f64();
        let wave = (cycle.fract() * 2.0 - 1.0).abs();
        (wave * 8.0).round() as u8
    }

    /// Angle de l'indicateur d'attente, en 36 pas.
    pub fn spin(&self) -> u8 {
        if self.checking || self.is_welcoming() {
            (self.opened_at.elapsed().as_millis() / 25 % 36) as u8
        } else {
            0
        }
    }
}

/// Position de la secousse à l'instant `t` (de 0 à 1), en dixièmes de point.
fn shake_offset(t: f64) -> i16 {
    const KEYS: [f64; 6] = [0.0, -100.0, 90.0, -60.0, 40.0, 0.0];
    if !(0.0..1.0).contains(&t) {
        return 0;
    }
    let at = t * 5.0;
    let index = at.floor() as usize;
    (KEYS[index] + (KEYS[index + 1] - KEYS[index]) * at.fract()).round() as i16
}

/// Lecteur multimédia en cours, vu par MPRIS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    pub detail: String,
    pub progress: u8,
    pub playing: bool,
}

/// Bouton du lecteur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    Previous,
    PlayPause,
    Next,
}

impl MediaCommand {
    fn method(self) -> &'static str {
        match self {
            MediaCommand::Previous => "Previous",
            MediaCommand::PlayPause => "PlayPause",
            MediaCommand::Next => "Next",
        }
    }
}

/// Liaison avec le fil qui suit le lecteur multimédia.
#[derive(Debug)]
pub struct MediaLink {
    /// Vrai tant que l'écran est verrouillé : le fil n'interroge personne le reste du temps.
    watching: Arc<AtomicBool>,
    commands: mpsc::Sender<MediaCommand>,
}

/// Texte d'un morceau : « artiste · titre », ou ce qu'on en sait.
fn media_detail(artist: Option<&str>, title: Option<&str>, player: &str) -> String {
    match (
        artist.filter(|text| !text.is_empty()),
        title.filter(|text| !text.is_empty()),
    ) {
        (Some(artist), Some(title)) => format!("{artist} · {title}"),
        (None, Some(title)) => title.to_owned(),
        _ => player.to_owned(),
    }
}

/// Part écoulée, de 0 à 100.
fn media_progress(position: i64, length: i64) -> u8 {
    if length <= 0 || position <= 0 {
        return 0;
    }
    (position.saturating_mul(100) / length).min(100) as u8
}

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";

fn mpris_players(connection: &zbus::blocking::Connection) -> zbus::Result<Vec<String>> {
    let names = zbus::blocking::fdo::DBusProxy::new(connection)?.list_names()?;
    Ok(names
        .into_iter()
        .map(|name| name.to_string())
        .filter(|name| name.starts_with(MPRIS_PREFIX))
        .collect())
}

fn mpris_proxy<'a>(
    connection: &zbus::blocking::Connection,
    player: &'a str,
) -> zbus::Result<zbus::blocking::Proxy<'a>> {
    zbus::blocking::Proxy::new(connection, player, MPRIS_PATH, MPRIS_PLAYER)
}

/// Lecteur à montrer : celui qui joue, sinon le premier en pause. Rien s'il est à l'arrêt.
fn read_media(connection: &zbus::blocking::Connection) -> zbus::Result<Option<(String, Media)>> {
    let mut paused = None;
    for player in mpris_players(connection)? {
        let proxy = mpris_proxy(connection, &player)?;
        let Ok(status) = proxy.get_property::<String>("PlaybackStatus") else {
            continue;
        };
        if status != "Playing" && status != "Paused" {
            continue;
        }
        let metadata: HashMap<String, zbus::zvariant::OwnedValue> =
            proxy.get_property("Metadata").unwrap_or_default();
        let title = metadata
            .get("xesam:title")
            .and_then(|value| value.downcast_ref::<&str>().ok());
        let artist = metadata
            .get("xesam:artist")
            .and_then(|value| value.downcast_ref::<&zbus::zvariant::Array>().ok())
            .and_then(|artists| artists.get::<&str>(0).ok().flatten());
        let length = metadata
            .get("mpris:length")
            .and_then(|value| value.downcast_ref::<i64>().ok())
            .unwrap_or(0);
        let position = proxy.get_property::<i64>("Position").unwrap_or(0);
        let short_name = player.trim_start_matches(MPRIS_PREFIX);
        let media = Media {
            detail: media_detail(artist, title, short_name),
            progress: media_progress(position, length),
            playing: status == "Playing",
        };
        drop(proxy);
        if media.playing {
            return Ok(Some((player.clone(), media)));
        }
        paused.get_or_insert((player.clone(), media));
    }
    Ok(paused)
}

fn watch_media(
    events: &channel::Sender<Event>,
    watching: &AtomicBool,
    commands: &mpsc::Receiver<MediaCommand>,
) -> zbus::Result<()> {
    let connection = zbus::blocking::Connection::session()?;
    let mut last = None;
    let mut current: Option<String> = None;
    loop {
        // Une commande réveille le fil : le lecteur est relu aussitôt.
        match commands.recv_timeout(MEDIA_REFRESH) {
            Ok(command) => {
                if let Some(player) = &current {
                    let reply = mpris_proxy(&connection, player)
                        .and_then(|proxy| proxy.call_method(command.method(), &()).map(|_| ()));
                    if let Err(error) = reply {
                        tracing::warn!(%error, "commande du lecteur refusée");
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
        if !watching.load(Ordering::Relaxed) {
            last = None;
            current = None;
            continue;
        }
        let found = read_media(&connection)?;
        current = found.as_ref().map(|(player, _)| player.clone());
        let media = found.map(|(_, media)| media);
        if media != last {
            last.clone_from(&media);
            if events.send(Event::Media(media)).is_err() {
                return Ok(());
            }
        }
    }
}

/// Efface un mot de passe de la mémoire avant de le rendre.
fn wipe(password: &mut String) {
    // Même longueur : les octets sont écrasés sur place, sans nouvelle allocation.
    let zeros = "\0".repeat(password.len());
    password.replace_range(.., &zeros);
    password.clear();
}

/// Message vers la boucle du compositeur.
#[derive(Debug)]
pub enum Event {
    Lock,
    Checked(Result<bool, String>),
    Media(Option<Media>),
}

/// Vérifie le mot de passe du compte courant avec `helper`.
fn check_with(helper: &str, user: &str, password: &str) -> Result<bool, String> {
    let mut child = Command::new(helper)
        .args([user, "nonull"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        // L'auxiliaire attend le mot de passe terminé par un zéro.
        stdin
            .write_all(password.as_bytes())
            .and_then(|()| stdin.write_all(&[0]))
            .map_err(|err| err.to_string())?;
    }
    let status = child.wait().map_err(|err| err.to_string())?;
    Ok(status.success())
}

fn check(user: &str, password: &str) -> Result<bool, String> {
    let helper = HELPERS
        .into_iter()
        .find(|path| std::path::Path::new(path).exists())
        .ok_or("unix_chkpwd introuvable")?;
    check_with(helper, user, password)
}

/// Nom du compte courant.
pub fn user_name() -> String {
    std::env::var("USER").unwrap_or_default()
}

/// Branche le canal sur la boucle et verrouille avant chaque mise en veille (logind).
pub fn start(handle: &LoopHandle<'static, Ferrite>) -> Result<(channel::Sender<Event>, MediaLink), String> {
    let (events, receiver) = channel::channel::<Event>();
    handle
        .insert_source(receiver, |event, _, state| {
            if let channel::Event::Msg(event) = event {
                state.on_lock_event(event);
            }
        })
        .map_err(|err| err.to_string())?;
    let sleep_events = events.clone();
    std::thread::Builder::new()
        .name("mise-en-veille".to_owned())
        .spawn(move || {
            if let Err(err) = watch_sleep(&sleep_events) {
                tracing::warn!(%err, "mise en veille non surveillée");
            }
        })
        .map_err(|err| err.to_string())?;
    let watching = Arc::new(AtomicBool::new(false));
    let (commands, received) = mpsc::channel();
    let media_events = events.clone();
    let watched = watching.clone();
    std::thread::Builder::new()
        .name("lecteur-multimedia".to_owned())
        .spawn(move || {
            if let Err(err) = watch_media(&media_events, &watched, &received) {
                tracing::warn!(%err, "lecteur multimédia non suivi");
            }
        })
        .map_err(|err| err.to_string())?;
    Ok((events, MediaLink { watching, commands }))
}

/// Attend le signal `PrepareForSleep` de logind.
fn watch_sleep(events: &channel::Sender<Event>) -> zbus::Result<()> {
    let connection = zbus::blocking::Connection::system()?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.login1")?
        .interface("org.freedesktop.login1.Manager")?
        .member("PrepareForSleep")?
        .build();
    for message in zbus::blocking::MessageIterator::for_match_rule(rule, &connection, None)? {
        let going_to_sleep: bool = message?.body().deserialize()?;
        if going_to_sleep && events.send(Event::Lock).is_err() {
            break;
        }
    }
    Ok(())
}

impl Lock {
    fn show_field(&mut self) {
        if !self.field_visible {
            self.field_visible = true;
            self.field_changed = Some(Instant::now());
        }
    }
}

impl Ferrite {
    /// Verrouille la session : tout ce qui est ouvert au-dessus du bureau se referme.
    pub fn lock_session(&mut self) {
        if let Some(lock) = self.lock.as_mut() {
            // Mise en veille pendant l'accueil : on repart verrouillé.
            lock.welcome_until = None;
            lock.checking = false;
            return;
        }
        self.popover = None;
        self.pending_power = None;
        self.switcher = None;
        self.region_select = None;
        self.close_overview();
        self.stop_widgets_overlay();
        self.lock = Some(Lock::new(self.appearance.is_dark()));
        self.set_media_watching(true);
        self.focus_window(None);
        self.refresh_pointer_focus();
    }

    pub fn on_lock_event(&mut self, event: Event) {
        match event {
            Event::Lock => self.lock_session(),
            Event::Checked(result) => {
                let accepted = match result {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        tracing::warn!(%error, "mot de passe non vérifié");
                        false
                    }
                };
                let Some(lock) = self.lock.as_mut() else {
                    return;
                };
                wipe(&mut lock.password);
                if accepted {
                    lock.welcome_until = Some(Instant::now() + WELCOME);
                } else {
                    lock.checking = false;
                    lock.message = LockMessage::Wrong;
                    lock.refused_at = Some(Instant::now());
                }
            }
            Event::Media(media) => {
                if let Some(lock) = self.lock.as_mut() {
                    lock.media = media;
                }
            }
        }
    }

    pub fn lock_type(&mut self, character: char) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            lock.show_field();
            lock.message = LockMessage::Hint;
            lock.password.push(character);
        }
    }

    pub fn lock_erase(&mut self) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            lock.show_field();
            lock.password.pop();
        }
    }

    /// Toute autre touche, ou un clic, fait apparaître le champ.
    pub fn lock_reveal(&mut self) {
        if let Some(lock) = self.lock.as_mut() {
            lock.show_field();
        }
    }

    /// Échap ou Annuler : retour au repos, le mot de passe commencé est oublié.
    pub fn lock_cancel(&mut self) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            wipe(&mut lock.password);
            lock.message = LockMessage::Hint;
            lock.refused_at = None;
            if lock.field_visible {
                lock.field_visible = false;
                lock.field_changed = Some(Instant::now());
            }
        }
    }

    /// Thème de cet écran, indépendant de celui de la session.
    pub fn lock_toggle_theme(&mut self) {
        if let Some(lock) = self.lock.as_mut() {
            lock.dark = !lock.dark;
            let mut settings = ferrite_system::config::Settings::load();
            settings.set("verrouillage_theme", if lock.dark { "sombre" } else { "clair" });
            if let Err(error) = settings.save() {
                tracing::warn!(%error, "thème de l'écran de verrouillage non enregistré");
            }
        }
    }

    pub fn lock_set_wall(&mut self, wall: usize) {
        if let Some(lock) = self.lock.as_mut()
            && let Some((name, _, _)) = LOCK_WALLS.get(wall)
        {
            lock.wall = wall;
            let mut settings = ferrite_system::config::Settings::load();
            settings.set("verrouillage_fond", name);
            if let Err(error) = settings.save() {
                tracing::warn!(%error, "fond de l'écran de verrouillage non enregistré");
            }
        }
    }

    pub fn lock_media(&mut self, command: MediaCommand) {
        if let Some(link) = &self.media_link {
            let _ = link.commands.send(command);
        }
    }

    pub fn set_media_watching(&self, watching: bool) {
        if let Some(link) = &self.media_link {
            link.watching.store(watching, Ordering::Relaxed);
        }
    }

    /// À chaque tour de boucle : la session s'ouvre une fois l'accueil passé.
    pub fn lock_tick(&mut self) {
        let ready = self
            .lock
            .as_ref()
            .and_then(|lock| lock.welcome_until)
            .is_some_and(|until| Instant::now() >= until);
        if ready {
            self.lock = None;
            self.set_media_watching(false);
            let top = self.top_window();
            self.focus_window(top);
            self.refresh_pointer_focus();
        }
    }

    /// Entrée : la vérification se fait à côté, sans figer le bureau.
    pub fn lock_submit(&mut self) {
        let Some(events) = self.lock_events.clone() else {
            return;
        };
        let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) else {
            return;
        };
        lock.show_field();
        if lock.password.is_empty() {
            lock.message = LockMessage::Empty;
            lock.refused_at = Some(Instant::now());
            return;
        }
        lock.message = LockMessage::Hint;
        lock.checking = true;
        let mut password = std::mem::take(&mut lock.password);
        std::thread::spawn(move || {
            let result = check(&user_name(), &password);
            wipe(&mut password);
            let _ = events.send(Event::Checked(result));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_secousse_part_et_revient_au_centre() {
        assert_eq!(shake_offset(0.0), 0);
        assert_eq!(shake_offset(0.2), -100);
        assert_eq!(shake_offset(0.4), 90);
        assert_eq!(shake_offset(1.0), 0);
        assert_eq!(shake_offset(3.0), 0);
    }

    #[test]
    fn le_morceau_s_affiche_avec_ce_qu_on_en_sait() {
        assert_eq!(
            media_detail(Some("Nina"), Some("Sinnerman"), "vlc"),
            "Nina · Sinnerman"
        );
        assert_eq!(media_detail(None, Some("Sinnerman"), "vlc"), "Sinnerman");
        assert_eq!(media_detail(Some(""), Some(""), "vlc"), "vlc");
    }

    #[test]
    fn la_progression_du_morceau_reste_entre_0_et_100() {
        assert_eq!(media_progress(50, 200), 25);
        assert_eq!(media_progress(500, 200), 100);
        assert_eq!(media_progress(50, 0), 0);
        assert_eq!(media_progress(-1, 200), 0);
    }

    #[test]
    fn un_auxiliaire_absent_ne_deverrouille_pas() {
        assert!(check_with("/nexiste/pas", "moi", "secret").is_err());
    }

    #[test]
    fn la_reponse_de_l_auxiliaire_decide() {
        assert_eq!(check_with("/bin/true", "moi", "secret"), Ok(true));
        assert_eq!(check_with("/bin/false", "moi", "secret"), Ok(false));
    }

    #[test]
    fn le_mot_de_passe_est_efface() {
        let mut password = "secret".to_owned();
        wipe(&mut password);
        assert!(password.is_empty());
    }
}
