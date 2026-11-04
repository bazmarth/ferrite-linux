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
    desktop::panels::{LOCK_BLUR_MAX, LOCK_WALLS, LockMessage},
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
    /// Place du curseur dans le mot de passe, en caractères.
    cursor: usize,
    /// L'œil est tenu enfoncé : le mot de passe s'affiche en clair.
    peek: bool,
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
    /// Flou de la photo, de 0 (aucun, par défaut) à [`LOCK_BLUR_MAX`].
    pub blur: u8,
    pub media: Option<Media>,
}

impl Lock {
    fn new() -> Self {
        let settings = ferrite_system::config::Settings::load();
        // Clair par défaut, quel que soit le thème du bureau.
        let dark = settings
            .get("verrouillage_theme")
            .is_some_and(|theme| theme == "sombre");
        let blur = settings
            .get("verrouillage_flou")
            .and_then(|level| level.parse::<u8>().ok())
            .unwrap_or(0)
            .min(LOCK_BLUR_MAX);
        // Un chemin : la photo choisie dans la Visionneuse (après les fonds dessinés).
        let wall = settings
            .get("verrouillage_fond")
            .and_then(|name| {
                if name.starts_with('/') {
                    Some(LOCK_WALLS.len())
                } else {
                    LOCK_WALLS.iter().position(|wall| wall.0 == name)
                }
            })
            .unwrap_or(0);
        Self {
            password: String::new(),
            cursor: 0,
            peek: false,
            field_visible: false,
            field_changed: None,
            checking: false,
            welcome_until: None,
            message: LockMessage::Hint,
            refused_at: None,
            opened_at: Instant::now(),
            dark,
            wall,
            blur,
            media: None,
        }
    }

    pub fn password_length(&self) -> usize {
        self.password.chars().count()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Le mot de passe en clair, seulement tant que l'œil est tenu.
    pub fn peeked(&self) -> Option<String> {
        self.peek.then(|| self.password.clone())
    }

    /// Position en octets du caractère `index` (la fin s'il n'existe pas).
    fn byte_at(&self, index: usize) -> usize {
        self.password
            .char_indices()
            .nth(index)
            .map_or(self.password.len(), |(at, _)| at)
    }

    fn forget_password(&mut self) {
        wipe(&mut self.password);
        self.cursor = 0;
        self.peek = false;
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
    /// Pour le widget Musique : titre, artiste et pochette (`mpris:artUrl` local).
    pub title: String,
    pub artist: String,
    pub cover: Option<crate::clipboard::Thumbnail>,
}

/// Côté de la pochette réduite pour le widget Musique.
const COVER_SIDE: u32 = 160;

/// Chemin d'une adresse `file://` (les `%20` et autres décodés).
fn file_url_path(url: &str) -> Option<std::path::PathBuf> {
    let encoded = url.strip_prefix("file://")?;
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok());
        match (
            bytes[index],
            hex.and_then(|pair| u8::from_str_radix(pair, 16).ok()),
        ) {
            (b'%', Some(value)) => {
                decoded.push(value);
                index += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    Some(std::path::PathBuf::from(
        String::from_utf8_lossy(&decoded).into_owned(),
    ))
}

/// Pochette réduite, en BGRA prémultiplié comme les vignettes du presse-papiers.
fn load_cover(path: &std::path::Path) -> Option<crate::clipboard::Thumbnail> {
    let image = ferrite_ui::wallpaper::decode(path).ok()?;
    let small = ferrite_ui::wallpaper::cover(&image, COVER_SIDE, COVER_SIDE);
    let pixels = small
        .rgba
        .chunks_exact(4)
        .flat_map(|p| {
            let alpha = u32::from(p[3]);
            let premultiply = |value: u8| (u32::from(value) * alpha / 255) as u8;
            [premultiply(p[2]), premultiply(p[1]), premultiply(p[0]), p[3]]
        })
        .collect();
    Some(crate::clipboard::Thumbnail {
        width: small.width,
        height: small.height,
        pixels: Arc::new(pixels),
    })
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
/// Lecteur trouvé : son nom de bus, ce qu'il joue et sa pochette.
type Found = (String, Media, Option<std::path::PathBuf>);

fn read_media(connection: &zbus::blocking::Connection) -> zbus::Result<Option<Found>> {
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
        let art = metadata
            .get("mpris:artUrl")
            .and_then(|value| value.downcast_ref::<&str>().ok())
            .and_then(file_url_path);
        let short_name = player.trim_start_matches(MPRIS_PREFIX);
        let media = Media {
            detail: media_detail(artist, title, short_name),
            progress: media_progress(position, length),
            playing: status == "Playing",
            title: title.unwrap_or(short_name).to_owned(),
            artist: artist.unwrap_or_default().to_owned(),
            // Remplie par `watch_media`, qui garde la dernière pochette décodée.
            cover: None,
        };
        drop(proxy);
        if media.playing {
            return Ok(Some((player.clone(), media, art)));
        }
        paused.get_or_insert((player.clone(), media, art));
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
    // Dernière pochette décodée, gardée tant que le morceau ne change pas.
    let mut cover: Option<(std::path::PathBuf, Option<crate::clipboard::Thumbnail>)> = None;
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
        current = found.as_ref().map(|(player, _, _)| player.clone());
        let media = found.map(|(_, mut media, art)| {
            if let Some(art) = art {
                if cover.as_ref().is_none_or(|(path, _)| *path != art) {
                    cover = Some((art.clone(), load_cover(&art)));
                }
                media.cover = cover.as_ref().and_then(|(_, thumbnail)| thumbnail.clone());
            }
            media
        });
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

/// Ouvre le trousseau par défaut de gnome-keyring avec le mot de passe de la session.
///
/// La connexion sur tty1 est automatique : PAM n'a jamais le mot de passe, et Brave
/// demandait le trousseau à chaque démarrage. Le mot de passe que l'écran de verrouillage
/// vient d'accepter sert donc aussi ici, quand le trousseau a le même (sinon gnome-keyring
/// le refuse et l'application le redemande, comme avant).
fn unlock_keyring(password: &str) -> zbus::Result<()> {
    use zbus::{
        blocking::Proxy,
        zvariant::{OwnedObjectPath, OwnedValue, Value},
    };

    const SERVICE: &str = "org.freedesktop.secrets";
    const PATH: &str = "/org/freedesktop/secrets";
    let connection = zbus::blocking::Connection::session()?;
    let service = Proxy::new(&connection, SERVICE, PATH, "org.freedesktop.Secret.Service")?;
    let collection: OwnedObjectPath = service.call("ReadAlias", &("default",))?;
    if collection.as_str() == "/" {
        return Ok(());
    }
    let locked: bool = Proxy::new(
        &connection,
        SERVICE,
        collection.as_str(),
        "org.freedesktop.Secret.Collection",
    )?
    .get_property("Locked")?;
    if !locked {
        return Ok(());
    }
    // Session « plain » : le secret ne passe que par le bus de l'utilisateur.
    let (_, session): (OwnedValue, OwnedObjectPath) =
        service.call("OpenSession", &("plain", Value::from("")))?;
    let unlocked = Proxy::new(
        &connection,
        SERVICE,
        PATH,
        "org.gnome.keyring.InternalUnsupportedGuiltRiddenInterface",
    )?
    .call::<_, _, ()>(
        "UnlockWithMasterPassword",
        &(
            &collection,
            (&session, Vec::<u8>::new(), password.as_bytes(), "text/plain"),
        ),
    );
    let _ = Proxy::new(
        &connection,
        SERVICE,
        session.as_str(),
        "org.freedesktop.Secret.Session",
    )?
    .call::<_, _, ()>("Close", &());
    unlocked
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
                state.request_redraw();
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
        self.lock = Some(Lock::new());
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
                lock.forget_password();
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
                    lock.media = media.clone();
                }
                self.media = media;
            }
        }
    }

    /// Caractère tapé. Au repos, la touche ne fait qu'afficher le champ : elle n'est pas
    /// écrite.
    pub fn lock_type(&mut self, character: char) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            if !lock.field_visible {
                lock.show_field();
                return;
            }
            lock.message = LockMessage::Hint;
            let at = lock.byte_at(lock.cursor);
            lock.password.insert(at, character);
            lock.cursor += 1;
        }
    }

    /// Retour arrière : le caractère avant le curseur.
    pub fn lock_erase(&mut self) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            lock.show_field();
            if lock.cursor > 0 {
                lock.cursor -= 1;
                let at = lock.byte_at(lock.cursor);
                lock.password.remove(at);
            }
        }
    }

    /// Suppr : le caractère après le curseur.
    pub fn lock_delete(&mut self) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            lock.show_field();
            if lock.cursor < lock.password_length() {
                let at = lock.byte_at(lock.cursor);
                lock.password.remove(at);
            }
        }
    }

    /// ← et → (`step` de -1 ou 1), Début et Fin (`step` de `isize::MIN` ou `isize::MAX`).
    pub fn lock_move_cursor(&mut self, step: isize) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            lock.show_field();
            let length = lock.password_length();
            lock.cursor = lock.cursor.saturating_add_signed(step).min(length);
        }
    }

    /// L'œil du champ, tenu enfoncé (`true`) puis relâché.
    pub fn lock_peek(&mut self, held: bool) {
        if let Some(lock) = self.lock.as_mut() {
            lock.peek = held && !lock.checking;
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
            lock.forget_password();
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
        let name = match LOCK_WALLS.get(wall) {
            Some((name, _, _)) => Some((*name).to_owned()),
            None => self
                .lock_picture
                .as_ref()
                .filter(|_| wall == LOCK_WALLS.len())
                .map(|picture| picture.path.to_string_lossy().into_owned()),
        };
        if let Some(lock) = self.lock.as_mut()
            && let Some(name) = name
        {
            lock.wall = wall;
            let mut settings = ferrite_system::config::Settings::load();
            settings.set("verrouillage_fond", &name);
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

    /// Le lecteur est suivi pendant le verrouillage, et tant que le widget Musique est posé.
    pub fn refresh_media_watching(&self) {
        let widget = self
            .dashboard
            .layout
            .iter()
            .any(|slot| slot.kind == crate::desktop::widgets::WidgetKind::Music);
        self.set_media_watching(self.lock.is_some() || widget);
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
            self.refresh_media_watching();
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
        lock.cursor = 0;
        lock.peek = false;
        std::thread::spawn(move || {
            let result = check(&user_name(), &password);
            if result == Ok(true)
                && let Err(err) = unlock_keyring(&password)
            {
                tracing::info!(%err, "trousseau non ouvert");
            }
            wipe(&mut password);
            let _ = events.send(Event::Checked(result));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_curseur_compte_en_caracteres() {
        let mut lock = Lock::new();
        lock.password = "éa€".to_owned();
        assert_eq!(lock.byte_at(0), 0);
        assert_eq!(lock.byte_at(1), 2);
        assert_eq!(lock.byte_at(2), 3);
        assert_eq!(lock.byte_at(3), 6);
        lock.forget_password();
        assert!(lock.password.is_empty() && lock.cursor == 0 && lock.peeked().is_none());
    }

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
