//! Notifications : serveur `org.freedesktop.Notifications` sur le bus de session, bulles en
//! haut à droite de l'écran et historique ouvert depuis l'horloge.
//!
//! Le bus vit sur son propre fil : les demandes arrivent par un canal dans la boucle du
//! compositeur, et les signaux aux clients repartent par un autre.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use smithay::reexports::calloop::{LoopHandle, channel};
use zbus::{interface, zvariant::OwnedValue};

use crate::state::Ferrite;

/// Durée d'une bulle quand le client laisse le choix.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// Bulles affichées en même temps, et notifications gardées dans l'historique.
const MAX_BUBBLES: usize = 3;
const MAX_HISTORY: usize = 50;
/// Tailles maximales acceptées d'un client : un texte géant ne doit pas gêner le rendu.
const MAX_SUMMARY: usize = 200;
const MAX_BODY: usize = 1000;
/// Boutons d'action affichés dans une bannière.
const MAX_ACTIONS: usize = 3;

const BUS_NAME: &str = "org.freedesktop.Notifications";
const BUS_PATH: &str = "/org/freedesktop/Notifications";

/// Raisons de fermeture de la spécification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    Expired = 1,
    Dismissed = 2,
    Requested = 3,
}

/// Notification reçue d'un client ou du bureau lui-même.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    /// Fichier `.desktop` indiqué par le client, ou nom de l'application.
    pub entry: String,
    /// Chemin absolu d'une icône fournie par le client.
    pub icon_file: Option<PathBuf>,
    /// Durée de la bulle ; `None` : elle reste jusqu'à ce qu'on la ferme.
    pub timeout: Option<Duration>,
    /// Urgente : traverse « Ne pas déranger ».
    pub critical: bool,
    /// Le client attend l'action « default » au clic.
    pub has_default: bool,
    /// Autres actions proposées : identifiant et libellé.
    pub actions: Vec<(String, String)>,
    /// Heure de réception.
    pub received: std::time::SystemTime,
    /// Fichier à ouvrir au clic (capture, enregistrement) ; sinon le clic ouvre l'application.
    pub file: Option<PathBuf>,
    /// Son d'alerte à l'arrivée (une application peut le refuser : `suppress-sound`).
    pub sound: bool,
}

/// Message vers la boucle du compositeur.
#[derive(Debug)]
pub enum Event {
    Notify(Notice),
    Close(u32),
    /// Minuteur de l'Horloge : phase, reste et durée totale.
    Timer(String, Duration, Duration),
}

/// Signal à émettre sur le bus.
#[derive(Debug)]
enum Signal {
    Closed(u32, Closed),
    Action(u32, String),
    /// Bouton de l'îlot pour le minuteur de l'Horloge.
    Timer(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    notice: Notice,
    /// Bulle affichée, et fin prévue si elle en a une.
    bubble: bool,
    until: Option<Instant>,
}

/// Notifications connues, de la plus ancienne à la plus récente.
#[derive(Debug, Default)]
pub struct Center {
    items: Vec<Item>,
    pub do_not_disturb: bool,
}

impl Center {
    /// Ajoute une notification, ou remplace à sa place celle de même identifiant. Renvoie
    /// vrai si elle mérite un son : nouvelle, montrée en bulle, et l'application le veut.
    pub fn notify(&mut self, notice: Notice, now: Instant) -> bool {
        let bubble = !self.do_not_disturb || notice.critical;
        let until = notice.timeout.filter(|_| bubble).map(|timeout| now + timeout);
        let item = Item {
            notice,
            bubble,
            until,
        };
        let sound = bubble && item.notice.sound;
        let new = match self
            .items
            .iter_mut()
            .find(|known| known.notice.id == item.notice.id)
        {
            Some(known) => {
                *known = item;
                false
            }
            None => {
                self.items.push(item);
                true
            }
        };
        if self.items.len() > MAX_HISTORY {
            self.items.remove(0);
        }
        new && sound
    }

    /// Retire une notification ; renvoie si elle existait.
    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.notice.id != id);
        self.items.len() != before
    }

    pub fn clear(&mut self) -> Vec<u32> {
        self.items.drain(..).map(|item| item.notice.id).collect()
    }

    /// Masque les bulles arrivées à leur fin (elles restent dans l'historique) et renvoie
    /// leurs identifiants.
    pub fn expire(&mut self, now: Instant) -> Vec<u32> {
        let mut expired = Vec::new();
        for item in &mut self.items {
            if item.bubble && item.until.is_some_and(|until| now >= until) {
                item.bubble = false;
                expired.push(item.notice.id);
            }
        }
        expired
    }

    pub fn has_default(&self, id: u32) -> bool {
        self.items
            .iter()
            .any(|item| item.notice.id == id && item.notice.has_default)
    }

    /// Masque toutes les bulles (elles restent dans l'historique) ; renvoie si une l'était.
    pub fn hide_bubbles(&mut self) -> bool {
        let mut hidden = false;
        for item in self.items.iter_mut().filter(|item| item.bubble) {
            item.bubble = false;
            hidden = true;
        }
        hidden
    }

    /// Bulles à afficher, la plus récente en haut.
    pub fn bubbles(&self) -> impl Iterator<Item = &Notice> {
        self.items
            .iter()
            .rev()
            .filter(|item| item.bubble)
            .take(MAX_BUBBLES)
            .map(|item| &item.notice)
    }

    /// Historique, la plus récente en premier.
    pub fn history(&self) -> impl Iterator<Item = &Notice> {
        self.items.iter().rev().map(|item| &item.notice)
    }
}

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

fn next_id() -> u32 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Durée de la bulle selon `expire_timeout` : -1 au choix du serveur, 0 sans fin.
fn timeout_from(expire_timeout: i32, critical: bool) -> Option<Duration> {
    match expire_timeout {
        0 => None,
        _ if critical => None,
        ms if ms < 0 => Some(DEFAULT_TIMEOUT),
        ms => Some(Duration::from_millis(ms as u64)),
    }
}

/// Texte d'un client ramené à une taille raisonnable.
fn limited(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Notification du bureau lui-même, avec la durée par défaut.
pub fn local_notice(summary: &str, body: &str) -> Notice {
    Notice {
        id: next_id(),
        app: "Ferrite".to_owned(),
        summary: summary.to_owned(),
        body: body.to_owned(),
        entry: String::new(),
        icon_file: None,
        timeout: Some(DEFAULT_TIMEOUT),
        critical: false,
        has_default: false,
        actions: Vec::new(),
        received: std::time::SystemTime::now(),
        file: None,
        // Le bureau a ses propres sons (capture, Corbeille) : pas d'alerte en plus.
        sound: false,
    }
}

/// Notification du bureau qui ouvre `file` au clic (capture d'écran, enregistrement).
pub fn file_notice(summary: &str, body: &str, file: PathBuf) -> Notice {
    Notice {
        file: Some(file),
        ..local_notice(summary, body)
    }
}

struct Server {
    events: channel::Sender<Event>,
}

const ISLAND_PATH: &str = "/org/ferrite/Ilot";
const ISLAND_INTERFACE: &str = "org.ferrite.Ilot";

/// Îlot du haut de l'écran, pour les applications du bureau.
struct Island {
    events: channel::Sender<Event>,
}

#[interface(name = "org.ferrite.Ilot")]
impl Island {
    /// Minuteur de l'Horloge : « marche », « pause », « fini » ou « arret », reste et durée
    /// totale en millisecondes.
    fn minuteur(&self, phase: String, restant_ms: u64, total_ms: u64) {
        let _ = self.events.send(Event::Timer(
            limited(&phase, 16),
            Duration::from_millis(restant_ms),
            Duration::from_millis(total_ms),
        ));
    }
}

#[interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<String> {
        vec!["actions".to_owned(), "body".to_owned()]
    }

    #[allow(clippy::too_many_arguments)] // Signature imposée par la spécification.
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id == 0 { next_id() } else { replaces_id };
        let critical = hints
            .get("urgency")
            .and_then(|value| value.downcast_ref::<u8>().ok())
            == Some(2);
        let entry = hints
            .get("desktop-entry")
            .and_then(|value| value.downcast_ref::<&str>().ok().map(str::to_owned))
            .unwrap_or_else(|| app_name.clone());
        let icon = app_icon.strip_prefix("file://").unwrap_or(&app_icon);
        let notice = Notice {
            id,
            app: limited(&app_name, MAX_SUMMARY),
            summary: limited(&summary, MAX_SUMMARY),
            body: limited(&body, MAX_BODY),
            entry: limited(&entry, MAX_SUMMARY),
            icon_file: icon.starts_with('/').then(|| PathBuf::from(icon)),
            timeout: timeout_from(expire_timeout, critical),
            critical,
            // La liste alterne identifiants et libellés.
            has_default: actions.iter().step_by(2).any(|action| action == "default"),
            actions: actions
                .chunks_exact(2)
                .filter(|pair| pair[0] != "default")
                .take(MAX_ACTIONS)
                .map(|pair| (pair[0].clone(), limited(&pair[1], 16)))
                .collect(),
            received: std::time::SystemTime::now(),
            file: None,
            sound: !hints
                .get("suppress-sound")
                .and_then(|value| value.downcast_ref::<bool>().ok())
                .unwrap_or(false),
        };
        if self.events.send(Event::Notify(notice)).is_err() {
            tracing::warn!("notification perdue : le bureau ne répond plus");
        }
        id
    }

    fn close_notification(&self, id: u32) {
        let _ = self.events.send(Event::Close(id));
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "Ferrite".to_owned(),
            "Ferrite OS".to_owned(),
            env!("CARGO_PKG_VERSION").to_owned(),
            "1.2".to_owned(),
        )
    }
}

/// Lien avec le fil du bus et le canal d'entrée du centre de notifications.
#[derive(Debug)]
pub struct Link {
    signals: mpsc::Sender<Signal>,
    pub events: channel::Sender<Event>,
}

/// Branche le canal sur la boucle et publie le service sur le bus de session. Sans bus, ou
/// si un autre bureau tient déjà le nom, seules les notifications internes s'affichent.
pub fn start(handle: &LoopHandle<'static, Ferrite>) -> Result<Link, String> {
    let (events, receiver) = channel::channel::<Event>();
    handle
        .insert_source(receiver, |event, _, state| {
            if let channel::Event::Msg(event) = event {
                state.on_notification_event(event);
                state.request_redraw();
            }
        })
        .map_err(|err| err.to_string())?;
    let (signals, outgoing) = mpsc::channel();
    let server = Server {
        events: events.clone(),
    };
    let island = Island {
        events: events.clone(),
    };
    std::thread::Builder::new()
        .name("notifications".to_owned())
        .spawn(move || {
            if let Err(err) = serve(server, island, outgoing) {
                tracing::warn!(%err, "service de notifications non publié");
            }
        })
        .map_err(|err| err.to_string())?;
    Ok(Link { signals, events })
}

fn serve(server: Server, island: Island, outgoing: mpsc::Receiver<Signal>) -> zbus::Result<()> {
    let connection = zbus::blocking::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(BUS_PATH, server)?
        .serve_at(ISLAND_PATH, island)?
        .build()?;
    tracing::info!("service de notifications publié");
    for signal in outgoing {
        let sent = match signal {
            Signal::Closed(id, reason) => connection.emit_signal(
                None::<()>,
                BUS_PATH,
                BUS_NAME,
                "NotificationClosed",
                &(id, reason as u32),
            ),
            Signal::Action(id, action) => {
                connection.emit_signal(None::<()>, BUS_PATH, BUS_NAME, "ActionInvoked", &(id, action))
            }
            Signal::Timer(command) => {
                connection.emit_signal(None::<()>, ISLAND_PATH, ISLAND_INTERFACE, "Commande", &(command,))
            }
        };
        if let Err(err) = sent {
            tracing::warn!(%err, "signal de notification non envoyé");
        }
    }
    Ok(())
}

/// Âge d'une notification : « maintenant », « il y a 5 min », ou son heure.
pub fn age_label(received: std::time::SystemTime, now: std::time::SystemTime) -> String {
    let minutes = now.duration_since(received).unwrap_or_default().as_secs() / 60;
    match minutes {
        0 => "maintenant".to_owned(),
        1..=59 => format!("il y a {minutes} min"),
        _ => crate::desktop::panels::hour_label(received),
    }
}

impl Ferrite {
    fn send_notification_signal(&self, signal: Signal) {
        if let Some(link) = &self.notification_link {
            let _ = link.signals.send(signal);
        }
    }

    pub fn on_notification_event(&mut self, event: Event) {
        match event {
            Event::Notify(notice) => {
                if self.notifications.notify(notice, Instant::now()) {
                    ferrite_system::sounds::play(ferrite_system::sounds::Sound::Alert(
                        ferrite_system::sounds::Alert::Drop,
                    ));
                }
            }
            Event::Close(id) => {
                if self.notifications.remove(id) {
                    self.send_notification_signal(Signal::Closed(id, Closed::Requested));
                }
            }
            Event::Timer(phase, remaining, total) => self.on_timer_report(&phase, remaining, total),
        }
    }

    /// Bouton de l'îlot pour le minuteur : « pause », « reprendre » ou « arreter ».
    pub fn send_timer_command(&self, command: &str) {
        self.send_notification_signal(Signal::Timer(command.to_owned()));
    }

    /// À chaque tour de boucle : les bulles arrivées à leur fin disparaissent.
    pub fn expire_notifications(&mut self) {
        for id in self.notifications.expire(Instant::now()) {
            self.send_notification_signal(Signal::Closed(id, Closed::Expired));
        }
    }

    /// Clic sur une notification : l'application est prévenue, puis elle se ferme.
    pub fn activate_notification(&mut self, id: u32) {
        if self.notifications.has_default(id) {
            self.send_notification_signal(Signal::Action(id, "default".to_owned()));
        }
        self.open_notification_target(id);
        self.dismiss_notification(id);
    }

    /// Ce qu'ouvre le clic sur une notification : son fichier (une capture dans la
    /// Visionneuse, un enregistrement dans le Lecteur vidéo, autre chose dans Fichiers),
    /// sinon l'application qui l'a envoyée, mise au premier plan ou lancée (une
    /// application Wayland ne peut pas se mettre au premier plan seule).
    fn open_notification_target(&mut self, id: u32) {
        use crate::desktop::apps::{App, AppKey};
        let Some(notice) = self.notifications.history().find(|notice| notice.id == id) else {
            return;
        };
        let (file, entry) = (notice.file.clone(), notice.entry.clone());
        if let Some(file) = file.filter(|file| file.exists()) {
            let app = App::for_file(&file);
            let env = self.client_environment();
            app.launch_with(&env, &[file]);
            return;
        }
        if entry.is_empty() {
            return;
        }
        let key = AppKey::for_window(&entry, &self.applications);
        if self.app_running(&key) || !matches!(key, AppKey::Other(_)) {
            self.show_app(&key);
        }
    }

    /// Bouton d'action d'une bannière : l'application est prévenue, puis elle se ferme.
    pub fn notification_action(&mut self, id: u32, index: usize) {
        let action = self
            .notifications
            .history()
            .find(|notice| notice.id == id)
            .and_then(|notice| notice.actions.get(index))
            .map(|(action, _)| action.clone());
        if let Some(action) = action {
            self.send_notification_signal(Signal::Action(id, action));
        }
        self.dismiss_notification(id);
    }

    pub fn dismiss_notification(&mut self, id: u32) {
        if self.notifications.remove(id) {
            self.send_notification_signal(Signal::Closed(id, Closed::Dismissed));
        }
    }

    pub fn clear_notifications(&mut self) {
        for id in self.notifications.clear() {
            self.send_notification_signal(Signal::Closed(id, Closed::Dismissed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(id: u32) -> Notice {
        Notice {
            id,
            ..local_notice("Titre", "Texte")
        }
    }

    #[test]
    fn un_clic_ailleurs_range_les_bulles_dans_l_historique() {
        let mut center = Center::default();
        center.notify(notice(1), Instant::now());
        center.notify(notice(2), Instant::now());
        assert!(center.hide_bubbles());
        assert_eq!(center.bubbles().count(), 0);
        assert_eq!(center.history().count(), 2);
        assert!(!center.hide_bubbles());
    }

    #[test]
    fn une_bulle_expire_et_reste_dans_l_historique() {
        let now = Instant::now();
        let mut center = Center::default();
        center.notify(notice(1), now);
        assert_eq!(center.bubbles().count(), 1);
        assert!(center.expire(now + Duration::from_secs(1)).is_empty());
        assert_eq!(center.expire(now + DEFAULT_TIMEOUT), [1]);
        assert_eq!(center.bubbles().count(), 0);
        assert_eq!(center.history().count(), 1);
        // Une seule annonce de fin par bulle.
        assert!(center.expire(now + DEFAULT_TIMEOUT).is_empty());
    }

    #[test]
    fn un_identifiant_connu_remplace_sa_notification() {
        let now = Instant::now();
        let mut center = Center::default();
        center.notify(notice(1), now);
        center.notify(notice(2), now);
        center.notify(
            Notice {
                summary: "Nouveau".to_owned(),
                ..notice(1)
            },
            now,
        );
        assert_eq!(center.history().count(), 2);
        assert_eq!(center.history().last().unwrap().summary, "Nouveau");
    }

    #[test]
    fn son_seulement_pour_une_nouvelle_bulle_qui_le_veut() {
        let now = Instant::now();
        let mut center = Center::default();
        let sonore = |id| Notice {
            sound: true,
            ..notice(id)
        };
        assert!(center.notify(sonore(1), now));
        // Mise à jour de la même notification : silencieuse.
        assert!(!center.notify(sonore(1), now));
        // Notification du bureau, ou `suppress-sound` : silencieuse.
        assert!(!center.notify(notice(2), now));
        // Ne pas déranger : silencieuse, sauf l'urgente.
        center.do_not_disturb = true;
        assert!(!center.notify(sonore(3), now));
        assert!(center.notify(
            Notice {
                critical: true,
                ..sonore(4)
            },
            now
        ));
    }

    #[test]
    fn ne_pas_deranger_laisse_passer_l_urgent() {
        let now = Instant::now();
        let mut center = Center {
            do_not_disturb: true,
            ..Center::default()
        };
        center.notify(notice(1), now);
        assert_eq!(center.bubbles().count(), 0);
        assert_eq!(center.history().count(), 1);
        center.notify(
            Notice {
                critical: true,
                ..notice(2)
            },
            now,
        );
        assert_eq!(center.bubbles().next().unwrap().id, 2);
    }

    #[test]
    fn bulles_recentes_d_abord_et_en_nombre_limite() {
        let now = Instant::now();
        let mut center = Center::default();
        for id in 1..=5 {
            center.notify(notice(id), now);
        }
        let ids: Vec<u32> = center.bubbles().map(|notice| notice.id).collect();
        assert_eq!(ids, [5, 4, 3]);
    }

    #[test]
    fn l_historique_est_borne() {
        let now = Instant::now();
        let mut center = Center::default();
        for id in 0..(MAX_HISTORY as u32 + 5) {
            center.notify(notice(id), now);
        }
        assert_eq!(center.history().count(), MAX_HISTORY);
        assert!(!center.remove(0));
        assert!(center.remove(10));
    }

    #[test]
    fn age_d_une_notification() {
        let now = std::time::SystemTime::now();
        assert_eq!(age_label(now, now), "maintenant");
        assert_eq!(age_label(now - Duration::from_secs(5 * 60), now), "il y a 5 min");
        assert!(age_label(now - Duration::from_secs(3600), now).contains(':'));
    }

    #[test]
    fn duree_des_bulles() {
        assert_eq!(timeout_from(-1, false), Some(DEFAULT_TIMEOUT));
        assert_eq!(timeout_from(0, false), None);
        assert_eq!(timeout_from(1500, false), Some(Duration::from_millis(1500)));
        assert_eq!(timeout_from(-1, true), None);
    }
}
