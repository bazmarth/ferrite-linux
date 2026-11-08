//! Rappels du Calendrier.
//!
//! Chaque rappel d'un événement (par occurrence) ou d'une tâche a une clé stable, et un
//! état gardé dans le fichier `rappels` : envoyé, reporté jusqu'à telle heure, ou acquitté.
//! Le service `ferrite-apps rappels`, lancé par le bureau à l'ouverture de session (et par
//! le Calendrier s'il ne tourne pas), regarde chaque minute les rappels arrivés à échéance
//! et les annonce par le centre de notifications de Ferrite (`org.freedesktop.Notifications`),
//! avec les boutons « Reporter » et « OK ». L'état survit au redémarrage : un rappel
//! manqué pendant que l'ordinateur était éteint est annoncé au retour (s'il date de moins
//! d'un jour), et jamais deux fois.

use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use super::{
    date::{self, DAY_MINUTES, DateTime, clock, duration_label},
    model::Agenda,
    store,
};

/// Retard au-delà duquel un rappel manqué n'est plus annoncé.
pub const CATCH_UP: i64 = DAY_MINUTES;
/// Durée d'un report (« Reporter »).
pub const SNOOZE_MINUTES: i64 = 10;
/// Délai de rappel le plus long proposé (une semaine).
const LONGEST: i64 = 7 * DAY_MINUTES;
/// États gardés au plus 30 jours après leur échéance.
const KEEP: i64 = 30 * DAY_MINUTES;

/// Ce que vise un rappel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Événement, et début de l'occurrence concernée.
    Event(u64, DateTime),
    /// Tâche, et son échéance.
    Task(u64, DateTime),
}

/// Un rappel précis : sa cible, son délai et son heure d'annonce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub target: Target,
    /// Minutes avant le début ou l'échéance.
    pub offset: u32,
    pub trigger: DateTime,
}

impl Reminder {
    /// Clé stable : « e:12:2026-11-09T09:00:15 » ou « t:7:2026-11-10T18:00:0 ».
    pub fn key(&self) -> String {
        let (kind, id, at) = match self.target {
            Target::Event(id, at) => ('e', id, at),
            Target::Task(id, at) => ('t', id, at),
        };
        format!("{kind}:{id}:{}:{}", at.iso(), self.offset)
    }

    pub fn parse_key(key: &str) -> Option<Self> {
        let mut parts = key.splitn(3, ':');
        let kind = parts.next()?;
        let id = parts.next()?.parse().ok()?;
        let rest = parts.next()?;
        let (at, offset) = rest.rsplit_once(':')?;
        let at = DateTime::parse_iso(at)?;
        let offset: u32 = offset.parse().ok()?;
        let target = match kind {
            "e" => Target::Event(id, at),
            "t" => Target::Task(id, at),
            _ => return None,
        };
        Some(Self {
            target,
            offset,
            trigger: at.add_minutes(-i64::from(offset)),
        })
    }

    /// Titre et texte de la notification, ou rien si l'élément n'existe plus.
    pub fn describe(&self, agenda: &Agenda, now: DateTime) -> Option<(String, String)> {
        match self.target {
            Target::Event(id, at) => {
                let event = agenda.event(id)?;
                let mut parts = Vec::new();
                parts.push(when(at, now, event.all_day));
                if !event.all_day {
                    let end = at.add_minutes(event.duration());
                    parts.push(format!("{} – {}", clock(at.minute), clock(end.minute)));
                }
                if !event.location.is_empty() {
                    parts.push(event.location.clone());
                }
                Some((title_or_default(&event.title, "Événement"), parts.join(" · ")))
            }
            Target::Task(id, at) => {
                let task = agenda.task(id)?;
                if task.done {
                    return None;
                }
                let deadline = if task.due_time.is_some() {
                    format!("Échéance {} à {}", day_word(at, now), clock(at.minute))
                } else {
                    format!("Échéance {}", day_word(at, now))
                };
                Some((title_or_default(&task.title, "Tâche"), deadline))
            }
        }
    }
}

fn title_or_default(title: &str, fallback: &str) -> String {
    if title.trim().is_empty() {
        fallback.to_owned()
    } else {
        title.to_owned()
    }
}

/// « aujourd'hui », « demain », « le 12 nov. 2026 ».
fn day_word(at: DateTime, now: DateTime) -> String {
    match at.date.days() - now.date.days() {
        0 => "aujourd’hui".to_owned(),
        1 => "demain".to_owned(),
        -1 => "hier".to_owned(),
        _ => format!("le {}", at.date.medium()),
    }
}

/// « Dans 15 min », « Maintenant », « Commencé depuis 1 h », « Demain ».
fn when(at: DateTime, now: DateTime, all_day: bool) -> String {
    if all_day {
        return date::capitalize(&day_word(at, now));
    }
    let delta = at.stamp() - now.stamp();
    match delta {
        d if d > 0 && d < DAY_MINUTES => format!("Dans {}", duration_label(d as u32)),
        d if d > 0 => date::capitalize(&day_word(at, now)),
        d if d > -2 => "Maintenant".to_owned(),
        d => format!("Commencé depuis {}", duration_label((-d) as u32)),
    }
}

/// Rappels dont l'heure d'annonce tombe dans [`from`, `to`].
pub fn reminders_between(agenda: &Agenda, from: DateTime, to: DateTime) -> Vec<Reminder> {
    let mut found = Vec::new();
    // Une occurrence annoncée dans la fenêtre commence au plus une semaine après.
    let first = from.date;
    let last = to.add_minutes(LONGEST).date.add_days(1);
    for event in agenda.events.iter().filter(|event| !event.reminders.is_empty()) {
        for occurrence in event.occurrences(first, last) {
            let base = if event.all_day {
                occurrence.start.date.at(super::model::DEFAULT_REMINDER_MINUTE)
            } else {
                occurrence.start
            };
            for &offset in &event.reminders {
                let trigger = base.add_minutes(-i64::from(offset));
                if trigger >= from && trigger <= to {
                    found.push(Reminder {
                        target: Target::Event(event.id, base),
                        offset,
                        trigger,
                    });
                }
            }
        }
    }
    for task in agenda.tasks.iter().filter(|task| !task.done) {
        let Some(base) = task.reminder_base() else {
            continue;
        };
        for &offset in &task.reminders {
            let trigger = base.add_minutes(-i64::from(offset));
            if trigger >= from && trigger <= to {
                found.push(Reminder {
                    target: Target::Task(task.id, base),
                    offset,
                    trigger,
                });
            }
        }
    }
    found.sort_by_key(|reminder| reminder.trigger);
    found
}

/// État d'un rappel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Annoncé à cette heure, pas encore acquitté.
    Sent(DateTime),
    /// Reporté : annoncé de nouveau à cette heure.
    Snoozed(DateTime),
    /// Acquitté (« OK ») à cette heure.
    Acknowledged(DateTime),
}

/// États de tous les rappels connus, par clé.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Book {
    pub entries: BTreeMap<String, Status>,
}

impl Book {
    pub fn parse(text: &str) -> Self {
        let entries = text
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let (key, value) = line.split_once('=')?;
                let (kind, at) = value.split_once(' ')?;
                let at = DateTime::parse_iso(at)?;
                let status = match kind {
                    "envoye" => Status::Sent(at),
                    "reporte" => Status::Snoozed(at),
                    "acquitte" => Status::Acknowledged(at),
                    _ => return None,
                };
                Some((key.to_owned(), status))
            })
            .collect();
        Self { entries }
    }

    pub fn format(&self) -> String {
        let mut out = String::from("# Rappels du Calendrier Ferrite : envoyé, reporté ou acquitté.\n");
        for (key, status) in &self.entries {
            let (kind, at) = match status {
                Status::Sent(at) => ("envoye", at),
                Status::Snoozed(at) => ("reporte", at),
                Status::Acknowledged(at) => ("acquitte", at),
            };
            out.push_str(&format!("{key}={kind} {}\n", at.iso()));
        }
        out
    }

    /// Rappels à annoncer maintenant : arrivés à échéance depuis moins d'un jour et jamais
    /// annoncés, ou reportés jusqu'à maintenant.
    pub fn due(&self, agenda: &Agenda, now: DateTime) -> Vec<Reminder> {
        let mut due: Vec<Reminder> = reminders_between(agenda, now.add_minutes(-CATCH_UP), now)
            .into_iter()
            .filter(|reminder| !self.entries.contains_key(&reminder.key()))
            .collect();
        for (key, status) in &self.entries {
            if let Status::Snoozed(until) = status
                && *until <= now
                && let Some(reminder) = Reminder::parse_key(key)
            {
                due.push(reminder);
            }
        }
        // L'élément a pu disparaître, ou la tâche être terminée.
        due.retain(|reminder| reminder.describe(agenda, now).is_some());
        due
    }

    /// Rappels annoncés et pas encore acquittés ni reportés (bandeau du Calendrier).
    pub fn waiting(&self, agenda: &Agenda, now: DateTime) -> Vec<Reminder> {
        self.entries
            .iter()
            .filter(|(_, status)| matches!(status, Status::Sent(_)))
            .filter_map(|(key, _)| Reminder::parse_key(key))
            .filter(|reminder| reminder.trigger.stamp() >= now.stamp() - CATCH_UP)
            .filter(|reminder| reminder.describe(agenda, now).is_some())
            .collect()
    }

    pub fn mark_sent(&mut self, key: &str, now: DateTime) {
        self.entries.insert(key.to_owned(), Status::Sent(now));
    }

    pub fn snooze(&mut self, key: &str, now: DateTime) {
        self.entries
            .insert(key.to_owned(), Status::Snoozed(now.add_minutes(SNOOZE_MINUTES)));
    }

    pub fn acknowledge(&mut self, key: &str, now: DateTime) {
        self.entries.insert(key.to_owned(), Status::Acknowledged(now));
    }

    /// Oublie les rappels échus depuis plus de 30 jours.
    pub fn prune(&mut self, now: DateTime) {
        self.entries.retain(|key, _| {
            Reminder::parse_key(key).is_some_and(|reminder| reminder.trigger.stamp() > now.stamp() - KEEP)
        });
    }
}

/// Fichier verrou des écritures de `rappels` (le Calendrier et le service y écrivent).
fn lock_path(path: &Path) -> PathBuf {
    path.with_extension("verrou")
}

/// Lit l'état des rappels.
pub fn load(path: &Path) -> Book {
    fs::read_to_string(path)
        .map(|text| Book::parse(&text))
        .unwrap_or_default()
}

/// Lit, modifie et réécrit l'état des rappels sous verrou : le Calendrier et le service ne
/// s'écrasent pas l'un l'autre.
pub fn update<R>(path: &Path, change: impl FnOnce(&mut Book) -> R) -> io::Result<R> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock = File::create(lock_path(path))?;
    lock.lock()?;
    let mut book = load(path);
    let before = book.clone();
    let result = change(&mut book);
    if book != before {
        store::write_atomic(path, &book.format())?;
    }
    drop(lock);
    Ok(result)
}

// --- Service de rappels -------------------------------------------------------------

const BUS_NAME: &str = "org.freedesktop.Notifications";
const BUS_PATH: &str = "/org/freedesktop/Notifications";

/// Garde le service unique pour un dossier de données : verrou tenu tant que le processus
/// vit (un bureau d'essai, avec son propre `XDG_DATA_HOME`, a le sien).
fn single_instance() -> Option<File> {
    let directory = store::directory();
    fs::create_dir_all(&directory).ok()?;
    let file = File::create(directory.join("service.verrou")).ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// Annonce un rappel ; renvoie l'identifiant de la notification.
fn notify(connection: &zbus::blocking::Connection, summary: &str, body: &str) -> zbus::Result<u32> {
    use zbus::zvariant::Value;
    let actions = [
        "default",
        "Ouvrir",
        "reporter",
        "Reporter 10 min",
        "acquitter",
        "OK",
    ];
    let mut hints: HashMap<&str, Value> = HashMap::new();
    hints.insert("desktop-entry", Value::from("ferrite-calendar"));
    hints.insert("urgency", Value::from(1u8));
    let reply = connection.call_method(
        Some(BUS_NAME),
        BUS_PATH,
        Some(BUS_NAME),
        "Notify",
        &("Calendrier", 0u32, "", summary, body, &actions[..], hints, 0i32),
    )?;
    reply.body().deserialize::<u32>()
}

/// Ouvre le Calendrier (clic sur la notification).
fn open_calendar() {
    let program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ferrite-apps"));
    let mut command = std::process::Command::new(program);
    command.args(["calendar", "aujourdhui"]);
    if let Err(error) = ferrite_system::command::spawn_detached(command) {
        eprintln!("Calendrier non ouvert : {error}");
    }
}

/// Suit les boutons des notifications : « Reporter », « OK », clic, fermeture.
fn listen(connection: zbus::blocking::Connection, sent: Arc<Mutex<HashMap<u32, String>>>, path: PathBuf) {
    let rule = match zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(BUS_NAME)
        .and_then(|rule| rule.path(BUS_PATH))
    {
        Ok(rule) => rule.build(),
        Err(error) => {
            eprintln!("rappels : boutons non suivis : {error}");
            return;
        }
    };
    let messages = match zbus::blocking::MessageIterator::for_match_rule(rule, &connection, None) {
        Ok(messages) => messages,
        Err(error) => {
            eprintln!("rappels : boutons non suivis : {error}");
            return;
        }
    };
    for message in messages.flatten() {
        let header = message.header();
        let Some(member) = header.member().map(|member| member.to_string()) else {
            continue;
        };
        let (id, action) = match member.as_str() {
            "ActionInvoked" => match message.body().deserialize::<(u32, String)>() {
                Ok(pair) => pair,
                Err(_) => continue,
            },
            // Fermée par l'utilisateur (raison 2) : vaut un « OK ».
            "NotificationClosed" => match message.body().deserialize::<(u32, u32)>() {
                Ok((id, 2)) => (id, "acquitter".to_owned()),
                Ok((id, _)) => {
                    if let Ok(mut sent) = sent.lock() {
                        sent.remove(&id);
                    }
                    continue;
                }
                Err(_) => continue,
            },
            _ => continue,
        };
        let key = sent.lock().ok().and_then(|mut sent| sent.remove(&id));
        let Some(key) = key else {
            continue;
        };
        let now = date::now();
        let result = match action.as_str() {
            "reporter" => update(&path, |book| book.snooze(&key, now)),
            "acquitter" => update(&path, |book| book.acknowledge(&key, now)),
            "default" => {
                open_calendar();
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            eprintln!("rappels : état non enregistré : {error}");
        }
    }
}

/// Service de rappels (`ferrite-apps rappels`) : tourne jusqu'à la fin de la session.
/// Renvoie le code de sortie.
pub fn run_service() -> i32 {
    let Some(_instance) = single_instance() else {
        // Un autre service tourne déjà.
        return 0;
    };
    let connection = match zbus::blocking::Connection::session() {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("rappels : pas de bus de session : {error}");
            return 1;
        }
    };
    let agenda_path = store::agenda_path();
    let book_path = store::reminders_path();
    let sent: Arc<Mutex<HashMap<u32, String>>> = Arc::default();
    {
        let connection = connection.clone();
        let sent = sent.clone();
        let path = book_path.clone();
        let spawned = std::thread::Builder::new()
            .name("rappels-boutons".to_owned())
            .spawn(move || listen(connection, sent, path));
        if let Err(error) = spawned {
            eprintln!("rappels : boutons non suivis : {error}");
        }
    }
    let mut agenda = Agenda::default();
    let mut stamp = None;
    loop {
        let modified = store::modified(&agenda_path);
        if modified != stamp {
            match store::load_agenda(&agenda_path) {
                Ok(loaded) => {
                    agenda = loaded;
                    stamp = modified;
                }
                Err(error) => eprintln!("rappels : agenda illisible : {error}"),
            }
        }
        let now = date::now();
        let result = update(&book_path, |book| {
            let due = book.due(&agenda, now);
            for reminder in due {
                let key = reminder.key();
                let Some((summary, body)) = reminder.describe(&agenda, now) else {
                    continue;
                };
                match notify(&connection, &summary, &body) {
                    Ok(id) => {
                        if let Ok(mut sent) = sent.lock() {
                            sent.insert(id, key.clone());
                        }
                    }
                    Err(error) => eprintln!("rappels : notification non envoyée : {error}"),
                }
                // Annoncé ou non (pas de centre de notifications), il ne revient pas en
                // boucle chaque minute.
                book.mark_sent(&key, now);
            }
            book.prune(now);
        });
        if let Err(error) = result {
            eprintln!("rappels : état non enregistré : {error}");
        }
        // Réveil juste après la minute suivante.
        let wait = 61 - date::seconds_in_minute();
        std::thread::sleep(Duration::from_secs(wait));
    }
}

/// Lance le service de rappels s'il ne tourne pas (il se retire seul s'il tourne déjà).
pub fn ensure_service() {
    let Ok(program) = std::env::current_exe() else {
        return;
    };
    let mut command = std::process::Command::new(program);
    command
        .arg("rappels")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Err(error) = ferrite_system::command::spawn_detached(command) {
        eprintln!("service de rappels non lancé : {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{
        date::Date,
        model::{Event, Frequency, Priority, Recurrence, Task},
    };

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::new(y, m, d).unwrap()
    }

    fn agenda() -> Agenda {
        let mut agenda = Agenda::empty();
        let start = date(2026, 11, 9).at(9 * 60);
        agenda.put_event(Event {
            id: 1,
            title: "Dentiste".to_owned(),
            description: String::new(),
            location: "Cabinet".to_owned(),
            category: None,
            start,
            end: start.add_minutes(30),
            all_day: false,
            recurrence: Recurrence {
                frequency: Frequency::Daily,
                ..Recurrence::default()
            },
            exceptions: Vec::new(),
            reminders: vec![15, 60],
        });
        agenda.put_task(Task {
            id: 2,
            title: "Rapport".to_owned(),
            notes: String::new(),
            list: None,
            due: Some(date(2026, 11, 9)),
            due_time: None,
            priority: Priority::High,
            done: false,
            completed: None,
            event: None,
            reminders: vec![0],
            created: start,
        });
        agenda
    }

    #[test]
    fn plusieurs_rappels_par_evenement_et_par_occurrence() {
        let agenda = agenda();
        let found = reminders_between(&agenda, date(2026, 11, 9).at(0), date(2026, 11, 10).at(9 * 60));
        let triggers: Vec<String> = found.iter().map(|r| r.trigger.iso()).collect();
        assert_eq!(
            triggers,
            vec![
                "2026-11-09T08:00",
                "2026-11-09T08:45",
                "2026-11-09T09:00", // tâche : 9 h le jour de l'échéance
                "2026-11-10T08:00",
                "2026-11-10T08:45",
            ]
        );
        let key = found[0].key();
        assert_eq!(key, "e:1:2026-11-09T09:00:60");
        assert_eq!(Reminder::parse_key(&key), Some(found[0].clone()));
    }

    #[test]
    fn annonce_une_seule_fois_puis_report_et_acquittement() {
        let agenda = agenda();
        let mut book = Book::default();
        let now = date(2026, 11, 9).at(8 * 60 + 50);
        let due = book.due(&agenda, now);
        assert_eq!(due.len(), 2, "8 h et 8 h 45 sont passés");
        for reminder in &due {
            book.mark_sent(&reminder.key(), now);
        }
        assert!(book.due(&agenda, now).is_empty(), "jamais deux fois");
        assert_eq!(book.waiting(&agenda, now).len(), 2);
        let key = due[1].key();
        book.snooze(&key, now);
        assert!(book.due(&agenda, now.add_minutes(9)).is_empty());
        let again = book.due(&agenda, now.add_minutes(10));
        assert_eq!(again.len(), 2, "le report revient, avec la tâche de 9 h");
        book.acknowledge(&key, now);
        assert_eq!(book.waiting(&agenda, now).len(), 1);
    }

    #[test]
    fn l_etat_survit_a_un_redemarrage_et_rattrape_les_rappels_manques() {
        let agenda = agenda();
        let mut book = Book::default();
        let morning = date(2026, 11, 9).at(8 * 60);
        book.mark_sent(&reminders_between(&agenda, morning, morning)[0].key(), morning);
        let reread = Book::parse(&book.format());
        assert_eq!(reread, book);
        // Éteint de 8 h à midi : 8 h 45 et la tâche de 9 h sont rattrapés, 8 h non.
        let noon = date(2026, 11, 9).at(12 * 60);
        assert_eq!(reread.due(&agenda, noon).len(), 2);
        // Au-delà d'un jour, plus rien n'est rattrapé de la veille.
        let later = date(2026, 11, 11).at(7 * 60);
        assert!(
            reread
                .due(&agenda, later)
                .iter()
                .all(|reminder| reminder.trigger.date >= date(2026, 11, 10))
        );
    }

    #[test]
    fn une_tache_terminee_ne_rappelle_plus() {
        let mut agenda = agenda();
        let now = date(2026, 11, 9).at(9 * 60 + 1);
        agenda.set_done(2, true, now);
        let book = Book::default();
        assert!(
            book.due(&agenda, now)
                .iter()
                .all(|r| matches!(r.target, Target::Event(..)))
        );
    }

    #[test]
    fn textes_des_notifications() {
        let agenda = agenda();
        let now = date(2026, 11, 9).at(8 * 60 + 45);
        let reminder = &reminders_between(&agenda, now, now)[0];
        let (title, body) = reminder.describe(&agenda, now).unwrap();
        assert_eq!(title, "Dentiste");
        assert_eq!(body, "Dans 15 min · 09:00 – 09:30 · Cabinet");
        let task = Reminder::parse_key("t:2:2026-11-09T09:00:0").unwrap();
        assert_eq!(task.describe(&agenda, now).unwrap().1, "Échéance aujourd’hui");
        assert!(
            Reminder::parse_key("t:99:2026-11-09T09:00:0")
                .unwrap()
                .describe(&agenda, now)
                .is_none()
        );
    }

    #[test]
    fn les_vieux_etats_sont_oublies() {
        let mut book = Book::default();
        let old = date(2026, 9, 1).at(0);
        book.mark_sent("e:1:2026-09-01T09:00:15", old);
        book.mark_sent("e:1:2026-11-09T09:00:15", old);
        book.prune(date(2026, 11, 9).at(0));
        assert_eq!(book.entries.len(), 1);
    }

    #[test]
    fn mise_a_jour_sous_verrou() {
        let dir = std::env::temp_dir().join(format!("ferrite-rappels-test-{}", std::process::id()));
        let path = dir.join("rappels");
        let now = date(2026, 11, 9).at(0);
        update(&path, |book| book.mark_sent("e:1:2026-11-09T09:00:15", now)).unwrap();
        update(&path, |book| book.acknowledge("e:1:2026-11-09T09:00:15", now)).unwrap();
        assert_eq!(
            load(&path).entries.values().next(),
            Some(&Status::Acknowledged(now))
        );
        let _ = fs::remove_dir_all(dir);
    }
}
