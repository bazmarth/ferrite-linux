//! Agent d'appairage Bluetooth du bureau (`org.bluez.Agent1`, bus système).
//!
//! Il tourne dans le service de réception (`ferrite-apps reception-bluetooth`), donc toute
//! la session. Inscrit comme agent par défaut de BlueZ, il reçoit les demandes de tous les
//! appairages, lancés d'ici (Paramètres, centre de contrôle) ou depuis le téléphone, et les
//! montre en notification : code à comparer (« Confirmer » / « Refuser »), code à taper
//! sur un clavier, appareil qui demande à s'appairer.
//!
//! Tant qu'il tourne, il tient un verrou (`AGENT_LOCK` dans `XDG_RUNTIME_DIR`) :
//! `ferrite_system::bluetooth::connect` le voit et lui laisse l'appairage au lieu
//! d'inscrire son propre agent sans code.

use std::{
    collections::HashMap,
    fs::File,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    task::{Context, Poll, Waker},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ferrite_system::bluetooth::AGENT_LOCK;
use zbus::{
    blocking::{Connection, MessageIterator, Proxy},
    zvariant::{ObjectPath, Value},
};

const AGENT_PATH: &str = "/org/ferrite/bluez/agent";
const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";
/// BlueZ attend la réponse de l'agent une demi-minute environ.
const ANSWER: Duration = Duration::from_secs(28);
/// Réinscription régulière : un redémarrage de `bluetoothd` efface l'agent.
const REGISTER: Duration = Duration::from_secs(30);

/// Réponse d'une méthode de l'agent, attendue sans bloquer le fil du bus.
#[derive(Default)]
struct Slot {
    answer: Option<bool>,
    waker: Option<Waker>,
}

type Reply = Arc<Mutex<Slot>>;

fn answer(reply: &Reply, accepted: bool) {
    if let Ok(mut slot) = reply.lock() {
        slot.answer = Some(accepted);
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

struct Answer(Reply);

impl Future for Answer {
    type Output = bool;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<bool> {
        let Ok(mut slot) = self.0.lock() else {
            return Poll::Ready(false);
        };
        match slot.answer.take() {
            Some(accepted) => Poll::Ready(accepted),
            None => {
                slot.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Titre (le code, bien lisible) et texte d'une notification.
type Message = (String, String);

/// Ce que l'agent demande au fil des notifications.
enum Event {
    /// Question « Confirmer / Refuser » : titre et texte, réponse.
    Ask(Message, Reply),
    /// Code à afficher (sans réponse) : appareil, titre et texte.
    Show(String, Message),
    Cancel,
    Action(u32, String),
    Closed(u32),
}

struct Agent {
    events: Mutex<Sender<Event>>,
    system: Connection,
}

impl Agent {
    fn send(&self, event: Event) {
        if let Ok(events) = self.events.lock() {
            let _ = events.send(event);
        }
    }

    fn device(&self, device: &ObjectPath<'_>) -> Device {
        Device::read(&self.system, device)
    }

    async fn ask(&self, message: Message) -> zbus::fdo::Result<()> {
        let reply = Reply::default();
        self.send(Event::Ask(message, reply.clone()));
        if Answer(reply).await {
            Ok(())
        } else {
            Err(zbus::fdo::Error::Failed("Refusé".into()))
        }
    }
}

/// Ce qu'on sait d'un appareil.
struct Device {
    name: String,
    keyboard: bool,
    paired: bool,
}

impl Device {
    fn read(connection: &Connection, path: &ObjectPath<'_>) -> Device {
        let proxy = Proxy::new(connection, "org.bluez", path.to_owned(), "org.bluez.Device1").ok();
        let text = |name: &str| {
            proxy
                .as_ref()
                .and_then(|proxy| proxy.get_property::<String>(name).ok())
                .filter(|value| !value.is_empty())
        };
        Device {
            name: text("Alias")
                .or_else(|| text("Address"))
                .unwrap_or_else(|| "Un appareil".to_owned()),
            keyboard: text("Icon").is_some_and(|icon| icon == "input-keyboard"),
            paired: proxy
                .as_ref()
                .and_then(|proxy| proxy.get_property::<bool>("Paired").ok())
                .unwrap_or(false),
        }
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    /// Ancien appairage par code PIN : un clavier tape le code affiché ; les autres
    /// appareils (casques) ont presque toujours « 0000 ».
    fn request_pin_code(&self, device: ObjectPath<'_>) -> String {
        let device = self.device(&device);
        if device.keyboard {
            let code = format!("{:06}", random_code());
            self.send(Event::Show(device.name.clone(), type_on(&device.name, &code)));
            code
        } else {
            "0000".to_owned()
        }
    }

    fn display_pin_code(&self, device: ObjectPath<'_>, pincode: String) {
        let device = self.device(&device);
        self.send(Event::Show(device.name.clone(), type_on(&device.name, &pincode)));
    }

    fn request_passkey(&self, device: ObjectPath<'_>) -> u32 {
        let device = self.device(&device);
        let passkey = random_code();
        self.send(Event::Show(
            device.name.clone(),
            type_on(&device.name, &spaced(passkey)),
        ));
        passkey
    }

    /// Appelée à chaque touche tapée sur le clavier : la même notification est remplacée.
    fn display_passkey(&self, device: ObjectPath<'_>, passkey: u32, _entered: u16) {
        let device = self.device(&device);
        self.send(Event::Show(
            device.name.clone(),
            type_on(&device.name, &spaced(passkey)),
        ));
    }

    /// Code à comparer : celui affiché sur l'autre appareil doit être le même.
    async fn request_confirmation(&self, device: ObjectPath<'_>, passkey: u32) -> zbus::fdo::Result<()> {
        let device = self.device(&device);
        let message = (
            format!("Code d’appairage : {}", spaced(passkey)),
            format!("Vérifiez que « {} » affiche le même code.", device.name),
        );
        self.ask(message).await
    }

    /// Appairage sans code proposé par l'autre appareil.
    async fn request_authorization(&self, device: ObjectPath<'_>) -> zbus::fdo::Result<()> {
        let device = self.device(&device);
        let message = (
            "Demande d’appairage".to_owned(),
            format!("« {} » veut s’appairer avec cet ordinateur.", device.name),
        );
        self.ask(message).await
    }

    /// Un appareil appairé peut utiliser ses services (son, fichiers) ; un inconnu non.
    fn authorize_service(&self, device: ObjectPath<'_>, _uuid: String) -> zbus::fdo::Result<()> {
        if self.device(&device).paired {
            Ok(())
        } else {
            Err(zbus::fdo::Error::Failed("Appareil non appairé".into()))
        }
    }

    fn cancel(&self) {
        self.send(Event::Cancel);
    }
}

/// Lance l'agent dans des fils à part. Rien ne se passe sans bus système ou sans BlueZ.
pub fn start() {
    let spawned = thread::Builder::new().name("appairage".to_owned()).spawn(|| {
        if let Err(error) = run() {
            eprintln!("appairage Bluetooth : {error}");
        }
    });
    if let Err(error) = spawned {
        eprintln!("appairage Bluetooth : {error}");
    }
}

fn run() -> Result<(), String> {
    let lock_path = std::env::var_os("XDG_RUNTIME_DIR")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
        .join(AGENT_LOCK);
    let lock = File::create(&lock_path).map_err(|error| error.to_string())?;
    lock.try_lock()
        .map_err(|_| "un autre agent tourne déjà".to_owned())?;
    // Numéro du processus, lu par `ferrite_system::bluetooth` pour savoir l'agent actif.
    std::fs::write(&lock_path, std::process::id().to_string()).map_err(|error| error.to_string())?;
    let system = Connection::system().map_err(|error| format!("bus système : {error}"))?;
    let session = Connection::session().map_err(|error| format!("bus de session : {error}"))?;
    let (events, receiver) = mpsc::channel();
    // Lectures des appareils sur une connexion à part : une lecture bloquante sur la
    // connexion qui sert l'agent attendrait une réponse que ce même fil doit lire.
    let lookups = Connection::system().map_err(|error| format!("bus système : {error}"))?;
    let agent = Agent {
        events: Mutex::new(events.clone()),
        system: lookups,
    };
    system
        .object_server()
        .at(AGENT_PATH, agent)
        .map_err(|error| error.to_string())?;
    follow_notifications(&session, events)?;

    // Questions en cours (identifiant de notification → réponse) et notification de code
    // affichée par appareil (remplacée plutôt qu'empilée).
    let mut questions: HashMap<u32, (Reply, Instant)> = HashMap::new();
    let mut shown: HashMap<String, u32> = HashMap::new();
    let mut registered: Option<Instant> = None;
    let mut warned = false;
    loop {
        if registered.is_none_or(|at| at.elapsed() >= REGISTER) {
            registered = Some(Instant::now());
            match register(&system) {
                Ok(()) => warned = false,
                Err(error) if !warned => {
                    eprintln!("appairage Bluetooth : agent non inscrit : {error}");
                    warned = true;
                }
                Err(_) => {}
            }
        }
        // Sans réponse à temps, BlueZ a abandonné : la question disparaît.
        questions.retain(|id, (reply, asked)| {
            let alive = asked.elapsed() < ANSWER;
            if !alive {
                answer(reply, false);
                close(&session, *id);
            }
            alive
        });
        let event = match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        match event {
            Event::Ask(message, reply) => {
                let actions = ["confirmer", "Confirmer", "refuser", "Refuser"];
                match notify(&session, 0, &message, &actions, 0) {
                    Ok(id) => {
                        questions.insert(id, (reply, Instant::now()));
                    }
                    Err(error) => {
                        eprintln!("appairage Bluetooth : question non affichée : {error}");
                        answer(&reply, false);
                    }
                }
            }
            Event::Show(device, message) => {
                let replaces = shown.get(&device).copied().unwrap_or(0);
                if let Ok(id) = notify(&session, replaces, &message, &[], 30_000) {
                    shown.insert(device, id);
                }
            }
            Event::Cancel => {
                for (id, (reply, _)) in questions.drain() {
                    answer(&reply, false);
                    close(&session, id);
                }
                for (_, id) in shown.drain() {
                    close(&session, id);
                }
            }
            Event::Action(id, action) => {
                if let Some((reply, _)) = questions.remove(&id) {
                    answer(&reply, action == "confirmer");
                }
            }
            Event::Closed(id) => {
                // Fermée sans réponse : refus.
                if let Some((reply, _)) = questions.remove(&id) {
                    answer(&reply, false);
                }
                shown.retain(|_, shown| *shown != id);
            }
        }
    }
}

/// Inscrit l'agent auprès de BlueZ et le rend agent par défaut.
fn register(system: &Connection) -> Result<(), String> {
    let manager = Proxy::new(system, "org.bluez", "/org/bluez", "org.bluez.AgentManager1")
        .map_err(|error| error.to_string())?;
    let path = ObjectPath::try_from(AGENT_PATH).map_err(|error| error.to_string())?;
    match manager.call::<_, _, ()>("RegisterAgent", &(&path, "KeyboardDisplay")) {
        Ok(()) => {}
        Err(error) if error.to_string().contains("AlreadyExists") => {}
        Err(error) => return Err(error.to_string()),
    }
    manager
        .call::<_, _, ()>("RequestDefaultAgent", &(&path,))
        .map_err(|error| error.to_string())
}

/// Code à 6 chiffres, différent à chaque appairage.
fn random_code() -> u32 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    (nanos ^ std::process::id().rotate_left(16)) % 1_000_000
}

/// « 123 456 » : plus facile à comparer.
fn spaced(code: u32) -> String {
    let code = format!("{code:06}");
    format!("{} {}", &code[..3], &code[3..])
}

fn type_on(device: &str, code: &str) -> Message {
    (
        format!("Code d’appairage : {code}"),
        format!("Tapez ce code sur « {device} », puis Entrée."),
    )
}

fn notify(
    session: &Connection,
    replaces: u32,
    (summary, body): &Message,
    actions: &[&str],
    timeout: i32,
) -> zbus::Result<u32> {
    let mut hints: HashMap<&str, Value> = HashMap::new();
    hints.insert("desktop-entry", Value::from("ferrite-settings"));
    // Question : ne disparaît pas seule.
    if !actions.is_empty() {
        hints.insert("urgency", Value::from(2u8));
    }
    let reply = session.call_method(
        Some(NOTIFICATIONS),
        NOTIFICATIONS_PATH,
        Some(NOTIFICATIONS),
        "Notify",
        &(
            "Bluetooth",
            replaces,
            "",
            summary.as_str(),
            body.as_str(),
            actions,
            hints,
            timeout,
        ),
    )?;
    reply.body().deserialize::<u32>()
}

fn close(session: &Connection, id: u32) {
    let _ = session.call_method(
        Some(NOTIFICATIONS),
        NOTIFICATIONS_PATH,
        Some(NOTIFICATIONS),
        "CloseNotification",
        &(id,),
    );
}

fn follow_notifications(session: &Connection, events: Sender<Event>) -> Result<(), String> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(NOTIFICATIONS)
        .and_then(|rule| rule.path(NOTIFICATIONS_PATH))
        .map_err(|error| error.to_string())?
        .build();
    let messages = MessageIterator::for_match_rule(rule, session, None).map_err(|error| error.to_string())?;
    thread::spawn(move || {
        for message in messages.flatten() {
            let member = message.header().member().map(|member| member.to_string());
            let event = match member.as_deref() {
                Some("ActionInvoked") => match message.body().deserialize::<(u32, String)>() {
                    Ok((id, action)) => Event::Action(id, action),
                    Err(_) => continue,
                },
                Some("NotificationClosed") => match message.body().deserialize::<(u32, u32)>() {
                    Ok((id, _)) => Event::Closed(id),
                    Err(_) => continue,
                },
                _ => continue,
            };
            if events.send(event).is_err() {
                break;
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_lisible() {
        assert_eq!(spaced(123_456), "123 456");
        assert_eq!(spaced(42), "000 042");
        assert!(random_code() < 1_000_000);
        let (summary, body) = type_on("Clavier", "123 456");
        assert_eq!(summary, "Code d’appairage : 123 456");
        assert_eq!(body, "Tapez ce code sur « Clavier », puis Entrée.");
    }
}
