//! Réception de fichiers par Bluetooth (`ferrite-apps reception-bluetooth`) : service sans
//! fenêtre, lancé par le bureau à l'ouverture de session.
//!
//! Il s'inscrit comme agent auprès d'`obexd` (BlueZ, bus de session). Quand un appareil
//! envoie un fichier, une notification demande « Accepter » ou « Refuser » ; le fichier
//! accepté arrive d'abord dans le cache d'`obexd`, puis rejoint le dossier choisi dans
//! Paramètres › Bluetooth (Téléchargements par défaut) une fois complet, et une
//! notification l'annonce (clic : Fichiers sur ce dossier). Réception coupée dans les
//! réglages : tout fichier proposé est refusé sans rien demander.

use std::{
    collections::HashMap,
    env,
    fs::{self, File},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process::Command,
    sync::{
        Arc, Mutex,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    task::{Context, Poll, Waker},
    thread,
    time::{Duration, Instant},
};

use ferrite_system::{bluetooth, config::Settings};
use zbus::{
    blocking::{Connection, MessageIterator, Proxy},
    zvariant::{OwnedObjectPath, Value},
};

use super::bluetooth::{Change, OBEX, explain, watch};

const AGENT_PATH: &str = "/org/ferrite/obex/agent";
const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";
/// `obexd` attend la réponse de l'agent une minute au plus.
const ANSWER: Duration = Duration::from_secs(55);
/// Fréquence de la réinscription (si `obexd` a redémarré, l'agent est perdu).
const REGISTER: Duration = Duration::from_secs(30);

/// Réglage oui/non : accepter qu'on propose des fichiers (oui par défaut).
pub const SETTING_ENABLED: &str = "bluetooth_reception";
/// Réglage du dossier où ranger les fichiers reçus (une valeur de [`FOLDERS`]).
pub const SETTING_FOLDER: &str = "bluetooth_dossier";
/// Dossiers proposés : valeur du réglage, nom français, nom anglais (si seul présent).
pub const FOLDERS: [(&str, &str, &str); 4] = [
    ("telechargements", "Téléchargements", "Downloads"),
    ("bureau", "Bureau", "Desktop"),
    ("documents", "Documents", "Documents"),
    ("images", "Images", "Pictures"),
];

/// Réponse attendue par `AuthorizePush` : chemin où écrire le fichier, ou refus.
type Reply = Arc<Mutex<Slot>>;

#[derive(Default)]
struct Slot {
    answer: Option<Option<String>>,
    waker: Option<Waker>,
}

fn answer(reply: &Reply, path: Option<String>) {
    if let Ok(mut slot) = reply.lock() {
        slot.answer = Some(path);
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

/// Attente de la réponse sans bloquer le fil du bus.
struct Answer(Reply);

impl Future for Answer {
    type Output = Option<String>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let Ok(mut slot) = self.0.lock() else {
            return Poll::Ready(None);
        };
        match slot.answer.take() {
            Some(answer) => Poll::Ready(answer),
            None => {
                slot.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Ce qui arrive au fil principal du service.
enum Event {
    Push(String, Reply),
    Cancel,
    Action(u32, String),
    Closed(u32, u32),
    Transfer(Change),
}

struct Agent {
    events: Mutex<Sender<Event>>,
}

impl Agent {
    fn send(&self, event: Event) -> bool {
        self.events.lock().is_ok_and(|events| events.send(event).is_ok())
    }
}

#[zbus::interface(name = "org.bluez.obex.Agent1")]
impl Agent {
    async fn authorize_push(&self, transfer: OwnedObjectPath) -> zbus::fdo::Result<String> {
        let reply = Reply::default();
        if !self.send(Event::Push(transfer.to_string(), reply.clone())) {
            return Err(zbus::fdo::Error::Failed("service arrêté".into()));
        }
        Answer(reply)
            .await
            .ok_or_else(|| zbus::fdo::Error::Failed("Refusé".into()))
    }

    fn cancel(&self) {
        self.send(Event::Cancel);
    }

    fn release(&self) {}
}

/// Fichier proposé, en attente de réponse.
struct Pending {
    notification: u32,
    reply: Reply,
    transfer: String,
    incoming: Incoming,
    deadline: Instant,
}

/// Fichier accepté, en cours de réception.
#[derive(Clone)]
struct Incoming {
    name: String,
    device: String,
    size: Option<u64>,
    temporary: PathBuf,
}

/// Service de réception ; renvoie le code de sortie.
pub fn run_service() -> i32 {
    let Some(_instance) = single_instance() else {
        return 0;
    };
    // Agent d'appairage (codes à confirmer), actif même réception coupée.
    crate::pairing::start();
    repair_obexd();
    let (events, receiver) = mpsc::channel();
    let agent = Agent {
        events: Mutex::new(events.clone()),
    };
    // Deux connexions : celle de l'agent (qui attend les réponses) et celle des
    // notifications et des transferts.
    let served = zbus::blocking::connection::Builder::session()
        .and_then(|builder| builder.serve_at(AGENT_PATH, agent))
        .and_then(|builder| builder.build());
    let (agent_bus, bus) = match (served, Connection::session()) {
        (Ok(agent_bus), Ok(bus)) => (agent_bus, bus),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("réception Bluetooth : pas de bus de session : {error}");
            return 1;
        }
    };
    if let Err(error) = follow_notifications(&bus, events.clone()) {
        eprintln!("réception Bluetooth : notifications non suivies : {error}");
    }
    match watch(&bus, "/org/bluez/obex") {
        Ok(changes) => {
            let events = events.clone();
            thread::spawn(move || {
                for change in changes {
                    if events.send(Event::Transfer(change)).is_err() {
                        break;
                    }
                }
            });
        }
        Err(error) => eprintln!("réception Bluetooth : transferts non suivis : {error}"),
    }
    drop(events);

    let mut pending: Option<Pending> = None;
    let mut incoming: HashMap<String, Incoming> = HashMap::new();
    let mut received: HashMap<u32, PathBuf> = HashMap::new();
    let mut registered = Instant::now() - REGISTER;
    let mut warned = false;
    loop {
        if registered.elapsed() >= REGISTER {
            registered = Instant::now();
            if let Err(error) = register(&agent_bus)
                && !warned
            {
                eprintln!("réception Bluetooth : agent non inscrit : {error}");
                warned = true;
            }
        }
        if let Some(waiting) = &pending
            && Instant::now() > waiting.deadline
        {
            let waiting = pending.take().expect("demande en attente");
            answer(&waiting.reply, None);
            close(&bus, waiting.notification);
        }
        let event = match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return 0,
        };
        match event {
            Event::Push(transfer, reply) => {
                if pending.is_some() || !Settings::load().flag(SETTING_ENABLED, true) {
                    // Une demande à la fois ; réception coupée : refus sans question.
                    answer(&reply, None);
                    continue;
                }
                let offer = describe(&bus, &transfer);
                match ask(&bus, &offer) {
                    Ok(notification) => {
                        pending = Some(Pending {
                            notification,
                            reply,
                            transfer,
                            incoming: offer,
                            deadline: Instant::now() + ANSWER,
                        });
                    }
                    Err(error) => {
                        eprintln!("réception Bluetooth : demande non affichée : {error}");
                        answer(&reply, None);
                    }
                }
            }
            Event::Cancel => {
                if let Some(waiting) = pending.take() {
                    answer(&waiting.reply, None);
                    close(&bus, waiting.notification);
                }
            }
            Event::Action(id, action) => {
                if pending.as_ref().is_some_and(|waiting| waiting.notification == id) {
                    let waiting = pending.take().expect("demande en attente");
                    if action == "accepter" {
                        let temporary = waiting.incoming.temporary.clone();
                        if let Some(parent) = temporary.parent() {
                            let _ = fs::create_dir_all(parent);
                        }
                        incoming.insert(waiting.transfer, waiting.incoming);
                        answer(&waiting.reply, Some(temporary.display().to_string()));
                    } else {
                        answer(&waiting.reply, None);
                    }
                } else if let Some(path) = received.remove(&id) {
                    show_in_files(&path);
                }
            }
            Event::Closed(id, reason) => {
                // Fermée par l'utilisateur (raison 2) : vaut un refus.
                if reason == 2 && pending.as_ref().is_some_and(|waiting| waiting.notification == id) {
                    let waiting = pending.take().expect("demande en attente");
                    answer(&waiting.reply, None);
                }
                received.remove(&id);
            }
            Event::Transfer(change) => {
                let Some(file) = incoming.get(&change.path) else {
                    continue;
                };
                match change.status.as_deref() {
                    Some("complete") => {
                        let file = file.clone();
                        incoming.remove(&change.path);
                        let (dir, label) = destination(&Settings::load());
                        match store(&file.temporary, &file.name, &dir) {
                            Ok(path) => {
                                let body = format!("« {} » de {}, dans {label}", file.name, file.device);
                                if let Ok(id) =
                                    notify(&bus, "Fichier reçu", &body, &["default", "Afficher"], 1)
                                {
                                    received.insert(id, path);
                                }
                            }
                            Err(error) => {
                                let _ = notify(&bus, "Fichier reçu non rangé", &error, &[], 1);
                            }
                        }
                    }
                    Some("error") => {
                        let file = file.clone();
                        incoming.remove(&change.path);
                        let _ = fs::remove_file(&file.temporary);
                        let body = format!("« {} » de {} n’est pas arrivé entier", file.name, file.device);
                        let _ = notify(&bus, "Réception interrompue", &body, &[], 1);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Réglage d'`obexd` livré par l'image (`/etc/systemd/user/obex.service.d/`).
const OBEX_DROP_IN: &str =
    include_str!("../../../../image/config/includes.chroot/etc/systemd/user/obex.service.d/ferrite.conf");
const OBEX_DROP_IN_NAME: &str = "obex.service.d/ferrite.conf";

/// Sans ce réglage, `obexd` n'annonce pas la réception de fichiers (voir le fichier).
/// Un système installé avant qu'il existe le reçoit dans `~/.config/systemd/user/`,
/// puis `obexd` est relancé ; ensuite, plus rien à faire.
fn repair_obexd() {
    let system = Path::new("/etc/systemd/user").join(OBEX_DROP_IN_NAME);
    let Some(user) = user_units_dir().map(|dir| dir.join(OBEX_DROP_IN_NAME)) else {
        return;
    };
    if !needs_drop_in(&system, &user) {
        return;
    }
    let written = user
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&user, OBEX_DROP_IN));
    if let Err(error) = written {
        eprintln!("réception Bluetooth : réglage d'obexd non écrit : {error}");
        return;
    }
    for args in [
        &["--user", "daemon-reload"][..],
        &["--user", "try-restart", "obex.service"],
    ] {
        if let Err(error) = ferrite_system::command::run("systemctl", args) {
            eprintln!("réception Bluetooth : {error}");
        }
    }
}

fn user_units_dir() -> Option<PathBuf> {
    let config = env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("systemd/user"))
}

/// Le réglage manque s'il n'est ni dans l'image ni, à jour, chez l'utilisateur.
fn needs_drop_in(system: &Path, user: &Path) -> bool {
    !system.exists() && fs::read_to_string(user).ok().as_deref() != Some(OBEX_DROP_IN)
}

/// Garde le service unique pour la session.
fn single_instance() -> Option<File> {
    let directory = env::var_os("XDG_RUNTIME_DIR").map_or_else(env::temp_dir, PathBuf::from);
    let file = File::create(directory.join("ferrite-reception-bluetooth.verrou")).ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// Inscrit l'agent ; déjà inscrit, ce n'est pas une erreur.
fn register(connection: &Connection) -> Result<(), String> {
    let manager = Proxy::new(
        connection,
        OBEX,
        "/org/bluez/obex",
        "org.bluez.obex.AgentManager1",
    )
    .map_err(|error| explain(&error))?;
    let path = zbus::zvariant::ObjectPath::try_from(AGENT_PATH).map_err(|error| error.to_string())?;
    match manager.call::<_, _, ()>("RegisterAgent", &(path,)) {
        Ok(()) => Ok(()),
        Err(error) if error.to_string().contains("AlreadyExists") => Ok(()),
        Err(error) => Err(explain(&error)),
    }
}

/// Nom, appareil et emplacement provisoire du fichier proposé.
fn describe(connection: &Connection, transfer: &str) -> Incoming {
    let property = |path: &str, interface: &str, name: &str| -> Option<zbus::zvariant::OwnedValue> {
        Proxy::new(connection, OBEX, path.to_owned(), interface.to_owned())
            .ok()?
            .get_property(name)
            .ok()
    };
    let name = property(transfer, "org.bluez.obex.Transfer1", "Name")
        .and_then(|value| String::try_from(value).ok())
        .map(|name| safe_name(&name))
        .unwrap_or_else(|| "fichier".to_owned());
    let session = property(transfer, "org.bluez.obex.Transfer1", "Session")
        .and_then(|value| OwnedObjectPath::try_from(value).ok());
    let address = session
        .and_then(|session| property(session.as_str(), "org.bluez.obex.Session1", "Destination"))
        .and_then(|value| String::try_from(value).ok());
    let device = address
        .as_deref()
        .and_then(bluetooth::device_name)
        .or(address)
        .unwrap_or_else(|| "un appareil".to_owned());
    let size =
        property(transfer, "org.bluez.obex.Transfer1", "Size").and_then(|value| u64::try_from(value).ok());
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(env::temp_dir);
    let temporary = unique(&cache.join("obexd"), &name);
    Incoming {
        name,
        device,
        size,
        temporary,
    }
}

/// Nom de fichier sans dossier ni caractère gênant (un appareil peut envoyer n'importe quoi).
fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    let clean: String = base.chars().filter(|c| !c.is_control()).collect();
    if clean.is_empty() || clean == "." || clean == ".." {
        "fichier".to_owned()
    } else {
        clean
    }
}

/// Chemin libre dans `dir` : « nom.ext », sinon « nom (2).ext », « nom (3).ext »…
fn unique(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){extension}")))
        .find(|path| !path.exists())
        .expect("un nom libre")
}

/// Dossier choisi dans les réglages et son nom affiché (Téléchargements si le réglage est
/// absent ou inconnu).
fn destination(settings: &Settings) -> (PathBuf, &'static str) {
    let home = env::var_os("HOME").map_or_else(env::temp_dir, PathBuf::from);
    folder(&home, settings.get(SETTING_FOLDER))
}

/// Dossier d'après la valeur du réglage : nom français, ou anglais s'il est seul présent.
fn folder(home: &Path, value: Option<&str>) -> (PathBuf, &'static str) {
    let (_, french, english) = FOLDERS
        .iter()
        .find(|(key, _, _)| Some(*key) == value)
        .unwrap_or(&FOLDERS[0]);
    let french_dir = home.join(french);
    let english_dir = home.join(english);
    if !french_dir.exists() && english_dir.is_dir() {
        (english_dir, french)
    } else {
        (french_dir, french)
    }
}

/// Range le fichier reçu dans `dir` ; renvoie son chemin final.
fn store(temporary: &Path, name: &str, dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|error| format!("{} : {error}", dir.display()))?;
    let target = unique(dir, name);
    if fs::rename(temporary, &target).is_err() {
        // Autre disque : copie puis effacement.
        fs::copy(temporary, &target).map_err(|error| format!("{name} : {error}"))?;
        let _ = fs::remove_file(temporary);
    }
    Ok(target)
}

/// Demande « Accepter / Refuser » ; renvoie l'identifiant de la notification.
fn ask(connection: &Connection, offer: &Incoming) -> zbus::Result<u32> {
    let size = offer
        .size
        .filter(|size| *size > 0)
        .map(|size| format!(" ({})", super::format_size(size)))
        .unwrap_or_default();
    let body = format!("{} veut vous envoyer « {} »{size}", offer.device, offer.name);
    notify(
        connection,
        "Fichier reçu par Bluetooth",
        &body,
        &["accepter", "Accepter", "refuser", "Refuser"],
        0,
    )
}

/// Notification de Fichiers ; `timeout` en millisecondes (0 : sans fin, -1 : au choix du
/// bureau quand il vaut 1).
fn notify(
    connection: &Connection,
    summary: &str,
    body: &str,
    actions: &[&str],
    timeout: i32,
) -> zbus::Result<u32> {
    let mut hints: HashMap<&str, Value> = HashMap::new();
    hints.insert("desktop-entry", Value::from("ferrite-files"));
    let timeout = if timeout == 1 { -1 } else { timeout };
    let reply = connection.call_method(
        Some(NOTIFICATIONS),
        NOTIFICATIONS_PATH,
        Some(NOTIFICATIONS),
        "Notify",
        &("Fichiers", 0u32, "", summary, body, actions, hints, timeout),
    )?;
    reply.body().deserialize::<u32>()
}

fn close(connection: &Connection, id: u32) {
    let _ = connection.call_method(
        Some(NOTIFICATIONS),
        NOTIFICATIONS_PATH,
        Some(NOTIFICATIONS),
        "CloseNotification",
        &(id,),
    );
}

/// Relaie les boutons et fermetures des notifications.
fn follow_notifications(connection: &Connection, events: Sender<Event>) -> Result<(), String> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(NOTIFICATIONS)
        .and_then(|rule| rule.path(NOTIFICATIONS_PATH))
        .map_err(|error| error.to_string())?
        .build();
    let messages =
        MessageIterator::for_match_rule(rule, connection, None).map_err(|error| error.to_string())?;
    thread::spawn(move || {
        for message in messages.flatten() {
            let member = message.header().member().map(|member| member.to_string());
            let event = match member.as_deref() {
                Some("ActionInvoked") => match message.body().deserialize::<(u32, String)>() {
                    Ok((id, action)) => Event::Action(id, action),
                    Err(_) => continue,
                },
                Some("NotificationClosed") => match message.body().deserialize::<(u32, u32)>() {
                    Ok((id, reason)) => Event::Closed(id, reason),
                    Err(_) => continue,
                },
                _ => continue,
            };
            if events.send(event).is_err() {
                break;
            }
        }
        // Bus fermé : la session est finie.
        std::process::exit(0);
    });
    Ok(())
}

/// Ouvre Fichiers sur le dossier du fichier reçu.
fn show_in_files(path: &Path) {
    let Some(dir) = path.parent() else { return };
    let program = env::current_exe().unwrap_or_else(|_| PathBuf::from("ferrite-apps"));
    let mut command = Command::new(program);
    command.arg("files").arg(dir);
    if let Err(error) = ferrite_system::command::spawn_detached(command) {
        eprintln!("Fichiers non ouvert : {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nom_sur_d_un_fichier_recu() {
        assert_eq!(safe_name("photo.jpg"), "photo.jpg");
        assert_eq!(safe_name("../../.bashrc"), ".bashrc");
        assert_eq!(safe_name("C:\\dossier\\note.txt"), "note.txt");
        assert_eq!(safe_name(".."), "fichier");
        assert_eq!(safe_name("a\u{7}b"), "ab");
    }

    #[test]
    fn dossier_de_reception_choisi() {
        let home = env::temp_dir().join(format!("ferrite-reception-dossier-{}", std::process::id()));
        fs::create_dir_all(home.join("Pictures")).unwrap();
        assert_eq!(
            folder(&home, None),
            (home.join("Téléchargements"), "Téléchargements")
        );
        assert_eq!(folder(&home, Some("inconnu")).1, "Téléchargements");
        assert_eq!(folder(&home, Some("bureau")), (home.join("Bureau"), "Bureau"));
        // Seul « Pictures » existe : il sert de dossier Images.
        assert_eq!(folder(&home, Some("images")), (home.join("Pictures"), "Images"));
        fs::create_dir_all(home.join("Images")).unwrap();
        assert_eq!(folder(&home, Some("images")), (home.join("Images"), "Images"));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn reglage_obexd_ecrit_une_seule_fois() {
        let dir = env::temp_dir().join(format!("ferrite-obexd-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let system = dir.join("systeme.conf");
        let user = dir.join("utilisateur.conf");
        assert!(OBEX_DROP_IN.contains("-P pbap,irmc"));
        assert!(needs_drop_in(&system, &user));
        fs::write(&user, "ancien").unwrap();
        assert!(needs_drop_in(&system, &user));
        fs::write(&user, OBEX_DROP_IN).unwrap();
        assert!(!needs_drop_in(&system, &user));
        fs::remove_file(&user).unwrap();
        fs::write(&system, OBEX_DROP_IN).unwrap();
        assert!(!needs_drop_in(&system, &user));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nom_libre_dans_un_dossier() {
        let dir = env::temp_dir().join(format!("ferrite-reception-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique(&dir, "photo.jpg"), dir.join("photo.jpg"));
        fs::write(dir.join("photo.jpg"), b"x").unwrap();
        fs::write(dir.join("photo (2).jpg"), b"x").unwrap();
        assert_eq!(unique(&dir, "photo.jpg"), dir.join("photo (3).jpg"));
        fs::write(dir.join("LISEZMOI"), b"x").unwrap();
        assert_eq!(unique(&dir, "LISEZMOI"), dir.join("LISEZMOI (2)"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
