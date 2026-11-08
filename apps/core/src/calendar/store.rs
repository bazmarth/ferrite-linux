//! Fichiers du Calendrier, dans `$XDG_DATA_HOME/ferrite/calendrier/` (par défaut
//! `~/.local/share/ferrite/calendrier/`) :
//!
//! - `agenda` : catégories, listes, événements et tâches, en blocs `[evenement]` de lignes
//!   `clé=valeur`, lisibles et en français comme `~/.config/ferrite/reglages` ;
//! - `rappels` : état des rappels (envoyé, reporté, acquitté), écrit aussi par le service
//!   de rappels (`ferrite-apps rappels`).
//!
//! Chaque écriture passe par un fichier temporaire renommé : un arrêt brutal laisse
//! l'ancien fichier ou le nouveau, jamais un fichier coupé.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use super::{
    date::{Date, DateTime, parse_time},
    model::{
        Agenda, Category, Event, Frequency, Priority, Recurrence, RecurrenceEnd, Task, TaskList,
        accent_from_key, color_key,
    },
};

/// Dossier des fichiers du Calendrier.
pub fn directory() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("ferrite/calendrier")
}

pub fn agenda_path() -> PathBuf {
    directory().join("agenda")
}

pub fn reminders_path() -> PathBuf {
    directory().join("rappels")
}

/// Date de dernière modification d'un fichier (pour relire après un changement extérieur).
pub fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

/// Écrit `text` dans `path` d'un seul coup (fichier temporaire puis renommage).
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path)
}

/// Lit l'agenda ; absent, l'agenda de départ (catégories et listes, sans événement).
pub fn load_agenda(path: &Path) -> io::Result<Agenda> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(parse_agenda(&text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Agenda::default()),
        Err(error) => Err(error),
    }
}

pub fn save_agenda(path: &Path, agenda: &Agenda) -> io::Result<()> {
    write_atomic(path, &format_agenda(agenda))
}

/// Échappe une valeur sur une ligne : `\` et retours à la ligne.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "")
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(character);
        }
    }
    out
}

fn yes_no(value: bool) -> &'static str {
    if value { "oui" } else { "non" }
}

fn numbers(values: &[u32]) -> String {
    values.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

fn parse_numbers(text: &str) -> Vec<u32> {
    let mut values: Vec<u32> = text
        .split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect();
    values.sort_unstable();
    values.dedup();
    values
}

/// Clés des jours de la semaine (bit 0 = lundi).
const DAY_KEYS: [&str; 7] = ["lun", "mar", "mer", "jeu", "ven", "sam", "dim"];

pub fn format_agenda(agenda: &Agenda) -> String {
    let mut out = String::from(
        "# Calendrier Ferrite : catégories, listes, événements et tâches.\n\
         # Fichier écrit par l'application ; dates en heure locale.\n",
    );
    out.push_str("version=1\n");
    out.push_str(&format!("prochain={}\n", agenda.next_id));
    let mut writer = Writer(out);
    for category in &agenda.categories {
        writer.section("[categorie]");
        writer.line("id", &category.id.to_string());
        writer.line("nom", &category.name);
        writer.line("couleur", color_key(category.color));
    }
    for list in &agenda.lists {
        writer.section("[liste]");
        writer.line("id", &list.id.to_string());
        writer.line("nom", &list.name);
        writer.line("couleur", color_key(list.color));
    }
    for event in &agenda.events {
        writer.section("[evenement]");
        writer.line("id", &event.id.to_string());
        writer.line("titre", &event.title);
        if !event.description.is_empty() {
            writer.line("description", &event.description);
        }
        if !event.location.is_empty() {
            writer.line("lieu", &event.location);
        }
        if let Some(category) = event.category {
            writer.line("categorie", &category.to_string());
        }
        writer.line("journee", yes_no(event.all_day));
        writer.line("debut", &event.start.iso());
        writer.line("fin", &event.end.iso());
        let rule = &event.recurrence;
        if rule.repeats() {
            writer.line("repetition", rule.frequency.key());
            writer.line("intervalle", &rule.interval.max(1).to_string());
            if rule.weekdays != 0 {
                let days: Vec<&str> = (0..7)
                    .filter(|day| rule.weekdays & (1 << day) != 0)
                    .map(|day| DAY_KEYS[day])
                    .collect();
                writer.line("jours", &days.join(","));
            }
            match rule.end {
                RecurrenceEnd::Never => {}
                RecurrenceEnd::Until(date) => writer.line("jusqua", &date.iso()),
                RecurrenceEnd::Count(count) => writer.line("nombre", &count.to_string()),
            }
            if !event.exceptions.is_empty() {
                let dates: Vec<String> = event.exceptions.iter().map(|date| date.iso()).collect();
                writer.line("exceptions", &dates.join(","));
            }
        }
        if !event.reminders.is_empty() {
            writer.line("rappels", &numbers(&event.reminders));
        }
    }
    for task in &agenda.tasks {
        writer.section("[tache]");
        writer.line("id", &task.id.to_string());
        writer.line("titre", &task.title);
        if !task.notes.is_empty() {
            writer.line("notes", &task.notes);
        }
        if let Some(list) = task.list {
            writer.line("liste", &list.to_string());
        }
        if let Some(due) = task.due {
            writer.line("echeance", &due.iso());
        }
        if let Some(time) = task.due_time {
            writer.line("heure", &super::date::clock(time));
        }
        writer.line("priorite", task.priority.key());
        writer.line("terminee", yes_no(task.done));
        if let Some(completed) = task.completed {
            writer.line("terminee_le", &completed.iso());
        }
        if let Some(event) = task.event {
            writer.line("evenement", &event.to_string());
        }
        if !task.reminders.is_empty() {
            writer.line("rappels", &numbers(&task.reminders));
        }
        writer.line("creee", &task.created.iso());
    }
    writer.0
}

/// Écriture des lignes `clé=valeur` et des titres de blocs.
struct Writer(String);

impl Writer {
    fn section(&mut self, name: &str) {
        self.0.push('\n');
        self.0.push_str(name);
        self.0.push('\n');
    }

    fn line(&mut self, key: &str, value: &str) {
        self.0.push_str(key);
        self.0.push('=');
        self.0.push_str(&escape(value));
        self.0.push('\n');
    }
}

/// Bloc en cours de lecture.
enum Block {
    None,
    Category(Category),
    List(TaskList),
    Event(Box<Event>),
    Task(Box<Task>),
}

fn blank_event() -> Event {
    let start = Date::from_days(0).at(0);
    Event {
        id: 0,
        title: String::new(),
        description: String::new(),
        location: String::new(),
        category: None,
        start,
        end: start,
        all_day: false,
        recurrence: Recurrence::default(),
        exceptions: Vec::new(),
        reminders: Vec::new(),
    }
}

fn blank_task() -> Task {
    Task {
        id: 0,
        title: String::new(),
        notes: String::new(),
        list: None,
        due: None,
        due_time: None,
        priority: Priority::Normal,
        done: false,
        completed: None,
        event: None,
        reminders: Vec::new(),
        created: Date::from_days(0).at(0),
    }
}

/// Lit l'agenda ; les lignes inconnues ou abîmées sont ignorées, les éléments sans
/// identifiant écartés.
pub fn parse_agenda(text: &str) -> Agenda {
    let mut agenda = Agenda::empty();
    let mut block = Block::None;
    let finish = |block: Block, agenda: &mut Agenda| match block {
        Block::Category(category) if category.id != 0 => agenda.categories.push(category),
        Block::List(list) if list.id != 0 => agenda.lists.push(list),
        Block::Event(event) if event.id != 0 => {
            let mut event = *event;
            if event.end < event.start {
                event.end = event.start;
            }
            agenda.events.push(event);
        }
        Block::Task(task) if task.id != 0 => agenda.tasks.push(*task),
        _ => {}
    };
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let next = match line.trim() {
            "[categorie]" => Some(Block::Category(Category {
                id: 0,
                name: String::new(),
                color: Default::default(),
            })),
            "[liste]" => Some(Block::List(TaskList {
                id: 0,
                name: String::new(),
                color: Default::default(),
            })),
            "[evenement]" => Some(Block::Event(Box::new(blank_event()))),
            "[tache]" => Some(Block::Task(Box::new(blank_task()))),
            _ => None,
        };
        if let Some(next) = next {
            finish(std::mem::replace(&mut block, next), &mut agenda);
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = unescape(value);
        let id = || value.trim().parse::<u64>().ok();
        match &mut block {
            Block::None => {
                if key == "prochain" {
                    agenda.next_id = id().unwrap_or(1);
                }
            }
            Block::Category(category) => match key {
                "id" => category.id = id().unwrap_or(0),
                "nom" => category.name = value,
                "couleur" => category.color = accent_from_key(&value),
                _ => {}
            },
            Block::List(list) => match key {
                "id" => list.id = id().unwrap_or(0),
                "nom" => list.name = value,
                "couleur" => list.color = accent_from_key(&value),
                _ => {}
            },
            Block::Event(event) => match key {
                "id" => event.id = id().unwrap_or(0),
                "titre" => event.title = value,
                "description" => event.description = value,
                "lieu" => event.location = value,
                "categorie" => event.category = id(),
                "journee" => event.all_day = value == "oui",
                "debut" => {
                    if let Some(start) = DateTime::parse_iso(&value) {
                        event.start = start;
                    }
                }
                "fin" => {
                    if let Some(end) = DateTime::parse_iso(&value) {
                        event.end = end;
                    }
                }
                "repetition" => event.recurrence.frequency = Frequency::parse(&value),
                "intervalle" => event.recurrence.interval = value.trim().parse().unwrap_or(1).max(1),
                "jours" => {
                    event.recurrence.weekdays = value
                        .split(',')
                        .filter_map(|day| DAY_KEYS.iter().position(|key| *key == day.trim()))
                        .fold(0, |mask, day| mask | (1 << day));
                }
                "jusqua" => {
                    if let Some(date) = Date::parse_iso(&value) {
                        event.recurrence.end = RecurrenceEnd::Until(date);
                    }
                }
                "nombre" => {
                    if let Ok(count) = value.trim().parse::<u32>() {
                        event.recurrence.end = RecurrenceEnd::Count(count.max(1));
                    }
                }
                "exceptions" => {
                    event.exceptions = value.split(',').filter_map(Date::parse_iso).collect();
                    event.exceptions.sort();
                }
                "rappels" => event.reminders = parse_numbers(&value),
                _ => {}
            },
            Block::Task(task) => match key {
                "id" => task.id = id().unwrap_or(0),
                "titre" => task.title = value,
                "notes" => task.notes = value,
                "liste" => task.list = id(),
                "echeance" => task.due = Date::parse_iso(&value),
                "heure" => task.due_time = parse_time(&value),
                "priorite" => task.priority = Priority::parse(value.trim()),
                "terminee" => task.done = value == "oui",
                "terminee_le" => task.completed = DateTime::parse_iso(&value),
                "evenement" => task.event = id(),
                "rappels" => task.reminders = parse_numbers(&value),
                "creee" => {
                    if let Some(created) = DateTime::parse_iso(&value) {
                        task.created = created;
                    }
                }
                _ => {}
            },
        }
    }
    finish(block, &mut agenda);
    // Le prochain identifiant dépasse toujours ceux déjà pris.
    let highest = agenda
        .categories
        .iter()
        .map(|item| item.id)
        .chain(agenda.lists.iter().map(|item| item.id))
        .chain(agenda.events.iter().map(|item| item.id))
        .chain(agenda.tasks.iter().map(|item| item.id))
        .max()
        .unwrap_or(0);
    agenda.next_id = agenda.next_id.max(highest + 1);
    agenda
}

#[cfg(test)]
mod tests {
    use ferrite_system::config::Accent;

    use super::*;

    fn sample() -> Agenda {
        let mut agenda = Agenda::default();
        let start = Date::new(2026, 11, 9).unwrap().at(9 * 60);
        let id = agenda.new_id();
        agenda.put_event(Event {
            id,
            title: "Cours = piano".to_owned(),
            description: "Ligne 1\nLigne 2 avec \\ barre".to_owned(),
            location: "Conservatoire".to_owned(),
            category: Some(1),
            start,
            end: start.add_minutes(60),
            all_day: false,
            recurrence: Recurrence {
                frequency: Frequency::Weekly,
                interval: 2,
                weekdays: 0b0010_0001,
                end: RecurrenceEnd::Until(Date::new(2027, 6, 30).unwrap()),
            },
            exceptions: vec![Date::new(2026, 12, 21).unwrap()],
            reminders: vec![15, 1440],
        });
        let task = agenda.new_id();
        agenda.put_task(Task {
            id: task,
            title: "Acheter la partition".to_owned(),
            notes: String::new(),
            list: Some(4),
            due: Some(Date::new(2026, 11, 8).unwrap()),
            due_time: Some(18 * 60),
            priority: Priority::High,
            done: true,
            completed: Some(start),
            event: Some(id),
            reminders: vec![0],
            created: start,
        });
        agenda
    }

    #[test]
    fn l_agenda_relu_est_identique() {
        let agenda = sample();
        let text = format_agenda(&agenda);
        assert!(text.contains("\n[evenement]\n"), "{text}");
        assert!(text.contains("jours=lun,sam\n"), "{text}");
        assert_eq!(parse_agenda(&text), agenda);
    }

    #[test]
    fn un_fichier_abime_ne_casse_rien() {
        let text = "prochain=2\n[evenement]\ntitre=Sans identifiant\n[tache]\nid=7\ntitre=Ok\n\
                    echeance=pas une date\npriorite=urgente\nligne sans signe\n[categorie]\nid=3\n\
                    nom=Sport\ncouleur=inconnue\n";
        let agenda = parse_agenda(text);
        assert!(agenda.events.is_empty());
        assert_eq!(agenda.tasks.len(), 1);
        assert_eq!(agenda.tasks[0].due, None);
        assert_eq!(agenda.tasks[0].priority, Priority::Normal);
        assert_eq!(agenda.categories[0].color, Accent::Blue);
        assert_eq!(agenda.next_id, 8, "au-delà du plus grand identifiant lu");
    }

    #[test]
    fn ecriture_atomique_dans_un_dossier_neuf() {
        let dir = std::env::temp_dir().join(format!("ferrite-calendrier-test-{}", std::process::id()));
        let path = dir.join("sous/agenda");
        let agenda = sample();
        save_agenda(&path, &agenda).unwrap();
        assert_eq!(load_agenda(&path).unwrap(), agenda);
        assert_eq!(load_agenda(&dir.join("absent")).unwrap(), Agenda::default());
        let _ = fs::remove_dir_all(dir);
    }
}
