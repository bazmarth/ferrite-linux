//! Lecteur visible des autres programmes (MPRIS sur le bus de session) : le widget Musique,
//! l'écran de verrouillage et les touches multimédia de ferrite-shell le pilotent.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
};

use eframe::egui;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

const BUS_NAME: &str = "org.mpris.MediaPlayer2.ferrite_musique";
const PATH: &str = "/org/mpris/MediaPlayer2";
const TRACK_ID: &str = "/org/ferrite/musique/morceau";

/// Ce que le lecteur montre aux autres.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Published {
    pub has_track: bool,
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: Option<PathBuf>,
    /// Secondes.
    pub length: f64,
    pub position: f64,
}

/// Demande reçue d'un autre programme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Request {
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    /// Décalage relatif, en secondes.
    SeekBy(f64),
    /// Position absolue, en secondes.
    SetPosition(f64),
}

pub struct Mpris {
    shared: Arc<Mutex<Published>>,
    pub requests: Receiver<Request>,
    /// Gardée ouverte tant que l'application vit.
    _connection: Option<zbus::blocking::Connection>,
}

impl Mpris {
    pub fn start(ctx: egui::Context) -> Self {
        let shared = Arc::new(Mutex::new(Published::default()));
        let (sender, requests) = mpsc::channel();
        let connection = connect(shared.clone(), sender, ctx)
            .map_err(|error| eprintln!("MPRIS indisponible : {error}"))
            .ok();
        Self {
            shared,
            requests,
            _connection: connection,
        }
    }

    pub fn publish(&self, published: Published) {
        if let Ok(mut shared) = self.shared.lock() {
            *shared = published;
        }
    }
}

fn connect(
    shared: Arc<Mutex<Published>>,
    sender: Sender<Request>,
    ctx: egui::Context,
) -> zbus::Result<zbus::blocking::Connection> {
    zbus::blocking::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(PATH, Root)?
        .serve_at(PATH, Player { shared, sender, ctx })?
        .build()
}

struct Root;

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {}
    fn quit(&self) {}
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> String {
        "Musique".to_owned()
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        "ferrite-music".to_owned()
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec!["file".to_owned()]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    shared: Arc<Mutex<Published>>,
    sender: Sender<Request>,
    ctx: egui::Context,
}

impl Player {
    fn published(&self) -> Published {
        self.shared
            .lock()
            .map(|shared| shared.clone())
            .unwrap_or_default()
    }

    fn ask(&self, request: Request) {
        let _ = self.sender.send(request);
        self.ctx.request_repaint();
    }
}

/// Texte sans octet nul : D-Bus coupe la connexion d'un programme qui en envoie un.
fn clean(text: &str) -> String {
    text.replace('\0', "")
}

fn owned<'a>(value: impl Into<Value<'a>>) -> Option<OwnedValue> {
    OwnedValue::try_from(value.into()).ok()
}

/// Métadonnées MPRIS d'un morceau (durée en microsecondes).
fn metadata(published: &Published) -> HashMap<String, OwnedValue> {
    let mut map = HashMap::new();
    if !published.has_track {
        return map;
    }
    let mut put = |key: &str, value: Option<OwnedValue>| {
        if let Some(value) = value {
            map.insert(key.to_owned(), value);
        }
    };
    put(
        "mpris:trackid",
        owned(ObjectPath::from_static_str_unchecked(TRACK_ID)),
    );
    put("xesam:title", owned(clean(&published.title)));
    put("xesam:artist", owned(vec![clean(&published.artist)]));
    put("xesam:album", owned(clean(&published.album)));
    put("mpris:length", owned((published.length * 1e6) as i64));
    if let Some(art) = &published.art {
        put("mpris:artUrl", owned(format!("file://{}", art.display())));
    }
    map
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play_pause(&self) {
        self.ask(Request::PlayPause);
    }
    fn play(&self) {
        self.ask(Request::Play);
    }
    fn pause(&self) {
        self.ask(Request::Pause);
    }
    fn stop(&self) {
        self.ask(Request::Pause);
    }
    fn next(&self) {
        self.ask(Request::Next);
    }
    fn previous(&self) {
        self.ask(Request::Previous);
    }
    fn seek(&self, offset: i64) {
        self.ask(Request::SeekBy(offset as f64 / 1e6));
    }
    fn set_position(&self, _track: OwnedObjectPath, position: i64) {
        self.ask(Request::SetPosition(position as f64 / 1e6));
    }
    fn open_uri(&self, _uri: String) {}

    #[zbus(property)]
    fn playback_status(&self) -> String {
        let published = self.published();
        match (published.has_track, published.playing) {
            (false, _) => "Stopped",
            (true, true) => "Playing",
            (true, false) => "Paused",
        }
        .to_owned()
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(&self.published())
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        (self.published().position * 1e6) as i64
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.published().has_track
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.published().has_track
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.published().has_track
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadonnees_d_un_morceau() {
        let published = Published {
            has_track: true,
            title: "Baobab".into(),
            artist: "Collectif Sahel".into(),
            album: "Savane".into(),
            length: 242.5,
            ..Published::default()
        };
        let map = metadata(&published);
        assert_eq!(map["xesam:title"].downcast_ref::<&str>().unwrap(), "Baobab");
        assert_eq!(map["mpris:length"].downcast_ref::<i64>().unwrap(), 242_500_000);
        let artists = map["xesam:artist"]
            .downcast_ref::<&zbus::zvariant::Array>()
            .unwrap();
        assert_eq!(artists.get::<&str>(0).unwrap(), Some("Collectif Sahel"));
        assert!(!map.contains_key("mpris:artUrl"));
        assert!(metadata(&Published::default()).is_empty());
        // Octet nul d'une étiquette WAV : retiré.
        let nul = Published {
            title: "Baobab\0".into(),
            ..published
        };
        assert_eq!(
            metadata(&nul)["xesam:title"].downcast_ref::<&str>().unwrap(),
            "Baobab"
        );
    }
}
