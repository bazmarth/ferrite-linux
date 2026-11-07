//! Lecteurs multimédia vus par MPRIS (Musique, Brave, VLC…), pour l'îlot, l'écran
//! verrouillé et le widget Musique.
//!
//! Un fil à part relit tous les lecteurs toutes les deux secondes, et plus souvent juste
//! après un bouton, le temps que le lecteur change d'état. Le bureau en suit un seul (celui
//! qui joue, ou celui qu'on vient de toucher) : les boutons visent toujours ce lecteur-là,
//! même si un autre se met à jouer entre deux appuis.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use smithay::reexports::calloop::channel;

use crate::desktop::lock::Event;

/// Relecture des lecteurs au repos.
const REFRESH: Duration = Duration::from_secs(2);
/// Après un bouton : relecture rapprochée pendant [`FAST_FOR`], le temps que le lecteur
/// annonce son nouvel état.
const FAST_REFRESH: Duration = Duration::from_millis(100);
const FAST_FOR: Duration = Duration::from_millis(1500);
/// Après un bouton, le lecteur touché reste suivi tant de temps, même en pause.
pub const HOLD: Duration = Duration::from_secs(6);
/// Écart de position au-delà duquel c'est un saut (avance, retour au début) à annoncer.
const JUMP: Duration = Duration::from_millis(1500);

/// Un lecteur, tel que MPRIS le décrit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    pub detail: String,
    /// Part écoulée, de 0 à 100, au moment de la lecture.
    pub progress: u8,
    pub playing: bool,
    /// Pour le widget Musique : titre, artiste et pochette (`mpris:artUrl` local).
    pub title: String,
    pub artist: String,
    pub cover: Option<crate::clipboard::Thumbnail>,
    /// Fichier `.desktop` du lecteur (`DesktopEntry` de MPRIS), pour ouvrir sa fenêtre.
    pub entry: String,
    /// Nom du lecteur sur le bus (`org.mpris.MediaPlayer2.…`) : les boutons le visent.
    pub player: String,
    /// Position lue, son heure, et durée du morceau (zéro si inconnue : radio, direct).
    pub position: Duration,
    pub read_at: Instant,
    pub length: Duration,
}

impl Media {
    /// Position à `now` : celle lue, plus le temps passé s'il joue.
    pub fn position_at(&self, now: Instant) -> Duration {
        let position = if self.playing {
            self.position + now.saturating_duration_since(self.read_at)
        } else {
            self.position
        };
        if self.length.is_zero() {
            position
        } else {
            position.min(self.length)
        }
    }

    /// Rien de neuf à montrer : même état, et la position a suivi son cours sans saut.
    fn unchanged(&self, newer: &Media) -> bool {
        let expected = self.position_at(newer.read_at);
        let gap = expected.abs_diff(newer.position);
        self.detail == newer.detail
            && self.progress == newer.progress
            && self.playing == newer.playing
            && self.title == newer.title
            && self.artist == newer.artist
            && self.cover == newer.cover
            && self.entry == newer.entry
            && self.player == newer.player
            && self.length == newer.length
            && gap < JUMP
    }
}

/// Deux relevés successifs se lisent-ils pareil ?
fn unchanged(old: &[Media], new: &[Media]) -> bool {
    old.len() == new.len() && old.iter().zip(new).all(|(old, new)| old.unchanged(new))
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

/// Liaison avec le fil qui suit les lecteurs.
#[derive(Debug)]
pub struct MediaLink {
    pub(crate) watching: Arc<AtomicBool>,
    /// Bouton à transmettre, et le lecteur visé.
    commands: mpsc::Sender<(String, MediaCommand)>,
}

/// Lecteurs connus du bureau, et celui qu'il suit.
#[derive(Debug, Default)]
pub struct Players {
    /// En lecture ou en pause, triés par nom sur le bus (l'ordre ne saute pas d'un relevé
    /// à l'autre).
    pub list: Vec<Media>,
    /// Lecteur suivi (nom sur le bus).
    pub followed: Option<String>,
    /// Le lecteur suivi le reste jusque-là, même en pause (bouton touché, îlot déplié).
    pub hold_until: Option<Instant>,
    /// Une image par seconde pour la barre d'avancement de l'îlot déplié.
    pub ticking: bool,
}

impl Players {
    /// Lecteur suivi à `now`.
    pub fn current(&self, now: Instant) -> Option<&Media> {
        let hold = self.hold_until.is_some_and(|until| now < until);
        choose(&self.list, self.followed.as_deref(), hold).map(|index| &self.list[index])
    }

    /// Nouveau relevé : le lecteur suivi est choisi de nouveau.
    pub fn update(&mut self, list: Vec<Media>, now: Instant) {
        self.list = list;
        self.followed = self.current(now).map(|media| media.player.clone());
    }

    /// Le lecteur `player` devient le suivi, et le reste un moment même en pause.
    pub fn follow(&mut self, player: &str, now: Instant) {
        if self.list.iter().any(|media| media.player == player) {
            self.followed = Some(player.to_owned());
            self.hold(now);
        }
    }

    /// Le lecteur suivi le reste pendant [`HOLD`].
    pub fn hold(&mut self, now: Instant) {
        self.followed = self.current(now).map(|media| media.player.clone());
        self.hold_until = Some(now + HOLD);
    }

    /// Les autres lecteurs qui jouent, en plus du suivi.
    pub fn others_playing(&self, now: Instant) -> impl Iterator<Item = &Media> {
        let current = self.current(now).map(|media| media.player.clone());
        self.list
            .iter()
            .filter(move |media| media.playing && Some(&media.player) != current.as_ref())
    }

    /// Bouton envoyé au lecteur suivi : lecture et pause s'affichent aussitôt, sans
    /// attendre que le lecteur le confirme (le fil le relit dans la foulée).
    fn pressed(&mut self, command: MediaCommand, now: Instant) -> Option<String> {
        self.hold(now);
        let player = self.followed.clone()?;
        if command == MediaCommand::PlayPause
            && let Some(media) = self.list.iter_mut().find(|media| media.player == player)
        {
            media.position = media.position_at(now);
            media.read_at = now;
            media.playing = !media.playing;
        }
        Some(player)
    }
}

/// Lecteur à suivre parmi `players`. Celui déjà suivi le reste s'il joue, ou s'il est
/// retenu (`hold` : on vient d'appuyer sur un de ses boutons). Sinon le premier qui joue,
/// puis celui suivi même en pause, puis le premier en pause.
pub fn choose(players: &[Media], followed: Option<&str>, hold: bool) -> Option<usize> {
    let followed = followed.and_then(|name| players.iter().position(|media| media.player == name));
    if let Some(index) = followed
        && (hold || players[index].playing)
    {
        return Some(index);
    }
    players
        .iter()
        .position(|media| media.playing)
        .or(followed)
        .or((!players.is_empty()).then_some(0))
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

/// Durée MPRIS (microsecondes, parfois négative quand le lecteur ne sait pas).
fn micros(value: i64) -> Duration {
    Duration::from_micros(u64::try_from(value).unwrap_or(0))
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

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";

/// Lecteurs présents sur le bus, triés par nom.
fn mpris_players(connection: &zbus::blocking::Connection) -> zbus::Result<Vec<String>> {
    let names = zbus::blocking::fdo::DBusProxy::new(connection)?.list_names()?;
    let mut players: Vec<String> = names
        .into_iter()
        .map(|name| name.to_string())
        .filter(|name| name.starts_with(MPRIS_PREFIX))
        .collect();
    players.sort();
    Ok(players)
}

fn mpris_proxy<'a>(
    connection: &zbus::blocking::Connection,
    player: &'a str,
) -> zbus::Result<zbus::blocking::Proxy<'a>> {
    zbus::blocking::Proxy::new(connection, player, MPRIS_PATH, MPRIS_PLAYER)
}

/// Un lecteur en lecture ou en pause (rien s'il est à l'arrêt), et sa pochette locale.
fn read_player(
    connection: &zbus::blocking::Connection,
    player: &str,
) -> zbus::Result<Option<(Media, Option<std::path::PathBuf>)>> {
    let proxy = mpris_proxy(connection, player)?;
    let Ok(status) = proxy.get_property::<String>("PlaybackStatus") else {
        return Ok(None);
    };
    if status != "Playing" && status != "Paused" {
        return Ok(None);
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
    // `mpris:length` est un entier 64 bits, signé ou non selon les lecteurs.
    let length = metadata
        .get("mpris:length")
        .and_then(|value| {
            value
                .downcast_ref::<i64>()
                .ok()
                .or_else(|| value.downcast_ref::<u64>().ok().map(|value| value as i64))
        })
        .unwrap_or(0);
    let position = proxy.get_property::<i64>("Position").unwrap_or(0);
    let read_at = Instant::now();
    let art = metadata
        .get("mpris:artUrl")
        .and_then(|value| value.downcast_ref::<&str>().ok())
        .and_then(file_url_path);
    let short_name = player.trim_start_matches(MPRIS_PREFIX);
    let entry = zbus::blocking::Proxy::new(connection, player, MPRIS_PATH, "org.mpris.MediaPlayer2")
        .ok()
        .and_then(|root| root.get_property::<String>("DesktopEntry").ok())
        .unwrap_or_else(|| short_name.split('.').next().unwrap_or(short_name).to_owned());
    let media = Media {
        detail: media_detail(artist, title, short_name),
        progress: media_progress(position, length),
        playing: status == "Playing",
        title: title.unwrap_or(short_name).to_owned(),
        artist: artist.unwrap_or_default().to_owned(),
        // Remplie par `watch`, qui garde les pochettes décodées.
        cover: None,
        entry,
        player: player.to_owned(),
        position: micros(position),
        read_at,
        length: micros(length),
    };
    Ok(Some((media, art)))
}

/// Tous les lecteurs en lecture ou en pause, avec leur pochette.
fn read_all(
    connection: &zbus::blocking::Connection,
    covers: &mut HashMap<std::path::PathBuf, Option<crate::clipboard::Thumbnail>>,
) -> zbus::Result<Vec<Media>> {
    let mut found = Vec::new();
    let mut used = Vec::new();
    for player in mpris_players(connection)? {
        // Un lecteur qui part pendant la lecture ne gâche pas le relevé des autres.
        let Ok(Some((mut media, art))) = read_player(connection, &player) else {
            continue;
        };
        if let Some(art) = art {
            let cover = covers.entry(art.clone()).or_insert_with(|| load_cover(&art));
            media.cover = cover.clone();
            used.push(art);
        }
        found.push(media);
    }
    // Les pochettes des morceaux passés sont oubliées.
    covers.retain(|path, _| used.contains(path));
    Ok(found)
}

fn watch(
    events: &channel::Sender<Event>,
    watching: &AtomicBool,
    commands: &mpsc::Receiver<(String, MediaCommand)>,
) -> zbus::Result<()> {
    let connection = zbus::blocking::Connection::session()?;
    let mut last: Option<Vec<Media>> = None;
    let mut covers = HashMap::new();
    // Relecture rapprochée jusque-là, après un bouton.
    let mut fast_until: Option<Instant> = None;
    loop {
        let fast = fast_until.is_some_and(|until| Instant::now() < until);
        let wait = if fast { FAST_REFRESH } else { REFRESH };
        // Une commande réveille le fil : les lecteurs sont relus aussitôt.
        match commands.recv_timeout(wait) {
            Ok((player, command)) => {
                let reply = mpris_proxy(&connection, &player)
                    .and_then(|proxy| proxy.call_method(command.method(), &()).map(|_| ()));
                if let Err(error) = reply {
                    tracing::warn!(%error, "commande du lecteur refusée");
                }
                fast_until = Some(Instant::now() + FAST_FOR);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Fin de la relecture rapprochée : le relevé est renvoyé tel quel, au cas où
                // le lecteur aurait refusé le bouton que le bureau affiche déjà.
                if fast_until.is_some_and(|until| Instant::now() >= until) {
                    fast_until = None;
                    last = None;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
        if !watching.load(Ordering::Relaxed) {
            last = None;
            continue;
        }
        let found = read_all(&connection, &mut covers)?;
        if last.as_ref().is_none_or(|last| !unchanged(last, &found)) {
            last = Some(found.clone());
            if events.send(Event::Media(found)).is_err() {
                return Ok(());
            }
        }
    }
}

/// Lance le fil qui suit les lecteurs ; ses relevés arrivent par `events`.
pub fn start(events: channel::Sender<Event>) -> Result<MediaLink, String> {
    let watching = Arc::new(AtomicBool::new(false));
    let (commands, received) = mpsc::channel();
    let watched = watching.clone();
    std::thread::Builder::new()
        .name("lecteur-multimedia".to_owned())
        .spawn(move || {
            if let Err(err) = watch(&events, &watched, &received) {
                tracing::warn!(%err, "lecteur multimédia non suivi");
            }
        })
        .map_err(|err| err.to_string())?;
    Ok(MediaLink { watching, commands })
}

impl crate::state::Ferrite {
    /// Lecteur suivi : celui que montrent l'îlot, l'écran verrouillé et le widget.
    pub fn media(&self) -> Option<&Media> {
        self.media.current(Instant::now())
    }

    /// Nouveau relevé des lecteurs.
    pub fn on_media(&mut self, list: Vec<Media>) {
        self.media.update(list, Instant::now());
        self.sync_lock_media();
        self.island_media_tick();
    }

    /// Bouton (îlot, widget, écran verrouillé, touche du clavier) pour le lecteur suivi.
    pub fn press_media(&mut self, command: MediaCommand) {
        let Some(player) = self.media.pressed(command, Instant::now()) else {
            return;
        };
        if let Some(link) = &self.media_link {
            let _ = link.commands.send((player, command));
        }
        self.sync_lock_media();
        self.request_redraw();
    }

    /// L'écran verrouillé garde sa copie du lecteur suivi.
    fn sync_lock_media(&mut self) {
        let media = self.media().cloned();
        if let Some(lock) = self.lock.as_mut() {
            lock.media = media;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(player: &str, playing: bool) -> Media {
        Media {
            detail: player.to_owned(),
            progress: 0,
            playing,
            title: player.to_owned(),
            artist: String::new(),
            cover: None,
            entry: player.to_owned(),
            player: player.to_owned(),
            position: Duration::from_secs(30),
            read_at: Instant::now(),
            length: Duration::from_secs(200),
        }
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
        assert_eq!(micros(-1), Duration::ZERO);
    }

    #[test]
    fn la_position_avance_en_lecture_seulement() {
        let mut song = media("a", true);
        let later = song.read_at + Duration::from_secs(5);
        assert_eq!(song.position_at(later), Duration::from_secs(35));
        // Jamais au-delà de la fin.
        assert_eq!(
            song.position_at(song.read_at + Duration::from_secs(500)),
            Duration::from_secs(200)
        );
        song.playing = false;
        assert_eq!(song.position_at(later), Duration::from_secs(30));
    }

    #[test]
    fn un_relevé_qui_suit_son_cours_ne_change_rien() {
        let old = media("a", true);
        let mut new = old.clone();
        new.read_at = old.read_at + Duration::from_secs(2);
        new.position = old.position + Duration::from_secs(2);
        assert!(unchanged(std::slice::from_ref(&old), std::slice::from_ref(&new)));
        // Saut dans le morceau.
        new.position = Duration::from_secs(120);
        assert!(!unchanged(std::slice::from_ref(&old), std::slice::from_ref(&new)));
        // Pause.
        let mut paused = old.clone();
        paused.playing = false;
        assert!(!unchanged(std::slice::from_ref(&old), &[paused]));
        // Un deuxième lecteur arrive.
        assert!(!unchanged(
            std::slice::from_ref(&old),
            &[old.clone(), media("b", true)]
        ));
    }

    #[test]
    fn le_lecteur_suivi_reste_le_même() {
        let players = [media("a", true), media("b", true)];
        // Rien de suivi : le premier qui joue.
        assert_eq!(choose(&players, None, false), Some(0));
        // Le suivi qui joue reste suivi, même s'il n'est pas le premier.
        assert_eq!(choose(&players, Some("b"), false), Some(1));
        // Suivi en pause : un autre qui joue prend sa place…
        let players = [media("a", false), media("b", true)];
        assert_eq!(choose(&players, Some("a"), false), Some(1));
        // … sauf juste après un appui sur ses boutons.
        assert_eq!(choose(&players, Some("a"), true), Some(0));
        // Personne ne joue : celui suivi, sinon le premier.
        let players = [media("a", false), media("b", false)];
        assert_eq!(choose(&players, Some("b"), false), Some(1));
        assert_eq!(choose(&players, Some("parti"), false), Some(0));
        assert_eq!(choose(&[], Some("a"), true), None);
    }

    #[test]
    fn pause_puis_lecture_visent_le_même_lecteur() {
        let now = Instant::now();
        let mut players = Players::default();
        players.update(vec![media("a", true), media("b", true)], now);
        assert_eq!(players.followed.as_deref(), Some("a"));
        // Pause sur « a » : affichée aussitôt, et « a » reste suivi malgré « b ».
        assert_eq!(
            players.pressed(MediaCommand::PlayPause, now).as_deref(),
            Some("a")
        );
        let current = players.current(now).expect("suivi");
        assert_eq!((current.player.as_str(), current.playing), ("a", false));
        // Le lecteur confirme la pause : toujours « a ».
        players.update(vec![media("a", false), media("b", true)], now);
        assert_eq!(players.current(now).map(|m| m.player.as_str()), Some("a"));
        // Second appui : de nouveau « a », qui repart.
        assert_eq!(
            players.pressed(MediaCommand::PlayPause, now).as_deref(),
            Some("a")
        );
        assert!(players.current(now).is_some_and(|media| media.playing));
        // Une fois le délai passé, sans appui, celui qui joue reprend la main.
        players.update(vec![media("a", false), media("b", true)], now);
        let later = now + HOLD + Duration::from_secs(1);
        assert_eq!(players.current(later).map(|m| m.player.as_str()), Some("b"));
        assert_eq!(
            players
                .others_playing(now)
                .map(|media| media.player.as_str())
                .collect::<Vec<_>>(),
            vec!["b"]
        );
    }
}
