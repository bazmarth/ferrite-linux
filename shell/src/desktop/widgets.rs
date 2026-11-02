//! Données persistantes de la page des widgets : notes, tâches et météo.

use std::{
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ferrite_system::{
    config::Settings,
    weather::{self, Place, Weather},
};
use smithay::reexports::calloop::{LoopHandle, channel};
use tracing::warn;

use crate::state::Ferrite;

/// Délai entre deux relevés de la météo, et avant de réessayer après un échec.
const WEATHER_REFRESH: Duration = Duration::from_secs(30 * 60);
const WEATHER_RETRY: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardEdit {
    Note(usize),
    NewTask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority {
    High,
    #[default]
    Medium,
    Low,
}

impl Priority {
    /// Priorité suivante, au clic sur l'étiquette.
    pub fn next(self) -> Self {
        match self {
            Priority::High => Priority::Medium,
            Priority::Medium => Priority::Low,
            Priority::Low => Priority::High,
        }
    }

    fn token(self) -> &'static str {
        match self {
            Priority::High => "haute",
            Priority::Medium => "moyenne",
            Priority::Low => "basse",
        }
    }

    fn from_token(token: Option<&str>) -> Self {
        match token {
            Some("haute") => Priority::High,
            Some("basse") => Priority::Low,
            _ => Priority::Medium,
        }
    }
}

/// Note : la première ligne sert de titre, le reste d'aperçu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub text: String,
    /// Dernière modification, en secondes depuis 1970.
    pub updated: u64,
}

impl Note {
    pub fn title(&self) -> &str {
        self.text.lines().next().unwrap_or("").trim()
    }

    /// Texte après le titre, sur une ligne.
    pub fn preview(&self) -> String {
        self.text
            .lines()
            .skip(1)
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub text: String,
    pub done: bool,
    pub priority: Priority,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dashboard {
    pub notes: Vec<Note>,
    pub tasks: Vec<Task>,
    pub draft: String,
    pub month_offset: i32,
    /// Dernier relevé ; `None` tant qu'aucun n'a abouti.
    pub weather: Option<Weather>,
    pub place: String,
}

pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl Dashboard {
    pub fn load() -> Self {
        let settings = Settings::load();
        let notes = load_notes(&settings);
        let tasks = if let Some(count) = settings
            .get("tableau_taches")
            .and_then(|value| value.parse::<usize>().ok())
        {
            (0..count.min(100))
                .filter_map(|index| {
                    let text = settings.get(&format!("tableau_tache_{index}"))?;
                    Some(Task {
                        text: decode(text),
                        done: settings.get(&format!("tableau_tache_{index}_faite")) == Some("oui"),
                        priority: Priority::from_token(
                            settings.get(&format!("tableau_tache_{index}_priorite")),
                        ),
                    })
                })
                .collect()
        } else {
            sample_tasks()
        };
        Self {
            notes,
            tasks,
            place: Place::from_settings(&settings).name,
            ..Self::default()
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let mut settings = Settings::load();
        settings.set("tableau_notes_nombre", &self.notes.len().to_string());
        for (index, note) in self.notes.iter().enumerate() {
            settings.set(&format!("tableau_note_{index}"), &encode(&note.text));
            settings.set(&format!("tableau_note_{index}_date"), &note.updated.to_string());
        }
        settings.set("tableau_taches", &self.tasks.len().to_string());
        for (index, task) in self.tasks.iter().enumerate() {
            settings.set(&format!("tableau_tache_{index}"), &encode(&task.text));
            settings.set(
                &format!("tableau_tache_{index}_faite"),
                if task.done { "oui" } else { "non" },
            );
            settings.set(&format!("tableau_tache_{index}_priorite"), task.priority.token());
        }
        settings.save()
    }

    /// Commence une note, en tête de liste.
    pub fn add_note(&mut self, now: u64) -> Option<DashboardEdit> {
        if self.notes.len() >= 100 {
            return None;
        }
        self.notes.insert(
            0,
            Note {
                text: String::new(),
                updated: now,
            },
        );
        Some(DashboardEdit::Note(0))
    }

    /// Fin de la saisie : une note restée vide disparaît. Renvoie sa place.
    pub fn finish_editing(&mut self, editing: Option<DashboardEdit>) -> Option<usize> {
        let Some(DashboardEdit::Note(index)) = editing else {
            return None;
        };
        if self.notes.get(index)?.text.trim().is_empty() {
            self.notes.remove(index);
            return Some(index);
        }
        None
    }

    /// Ajoute `character` à la saisie en cours ; renvoie vrai s'il faut enregistrer.
    pub fn type_char(&mut self, editing: Option<DashboardEdit>, character: char, now: u64) -> bool {
        match editing {
            Some(DashboardEdit::Note(index)) => self.notes.get_mut(index).is_some_and(|note| {
                note.text.push(character);
                note.updated = now;
                true
            }),
            Some(DashboardEdit::NewTask) => {
                self.draft.push(character);
                false
            }
            None => false,
        }
    }

    /// Efface le dernier caractère ; renvoie vrai s'il faut enregistrer.
    pub fn erase_char(&mut self, editing: Option<DashboardEdit>, now: u64) -> bool {
        match editing {
            Some(DashboardEdit::Note(index)) => self.notes.get_mut(index).is_some_and(|note| {
                note.text.pop();
                note.updated = now;
                true
            }),
            Some(DashboardEdit::NewTask) => {
                self.draft.pop();
                false
            }
            None => false,
        }
    }

    pub fn commit_draft(&mut self) -> bool {
        if self.tasks.len() >= 100 {
            return false;
        }
        let text = self.draft.trim();
        if text.is_empty() {
            self.draft.clear();
            return false;
        }
        self.tasks.push(Task {
            text: text.to_owned(),
            done: false,
            priority: Priority::default(),
        });
        self.draft.clear();
        true
    }
}

/// Notes enregistrées, ou anciennes notes en un seul texte reprises comme une note.
fn load_notes(settings: &Settings) -> Vec<Note> {
    if let Some(count) = settings
        .get("tableau_notes_nombre")
        .and_then(|value| value.parse::<usize>().ok())
    {
        return (0..count.min(100))
            .filter_map(|index| {
                Some(Note {
                    text: decode(settings.get(&format!("tableau_note_{index}"))?),
                    updated: settings
                        .get(&format!("tableau_note_{index}_date"))
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                })
            })
            .collect();
    }
    let legacy = settings.get("tableau_notes").map(decode).unwrap_or_else(|| {
        let count = settings
            .get("widgets_nombre")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0)
            .min(100);
        (0..count)
            .filter_map(|index| settings.get(&format!("widget_{index}_texte")))
            .map(|text| text.replace("\\n", "\n").replace("\\s", "="))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    });
    if legacy.trim().is_empty() {
        return sample_notes();
    }
    vec![Note {
        text: legacy,
        updated: now_seconds(),
    }]
}

fn sample_notes() -> Vec<Note> {
    let now = now_seconds();
    [
        ("Projet web", "Finaliser l’interface d’administration…", now),
        (
            "Idées",
            "Application mobile (marketplace)…",
            now.saturating_sub(86_400),
        ),
        (
            "Personnel",
            "Préparer les documents pour le voyage",
            now.saturating_sub(2 * 86_400),
        ),
        (
            "Santé",
            "Prendre les médicaments",
            now.saturating_sub(20 * 86_400),
        ),
    ]
    .into_iter()
    .map(|(title, preview, updated)| Note {
        text: format!("{title}\n{preview}"),
        updated,
    })
    .collect()
}

fn sample_tasks() -> Vec<Task> {
    [
        ("Terminer le rapport", Priority::High, false),
        ("Répondre aux emails", Priority::Medium, false),
        ("Faire les courses", Priority::Low, false),
        ("Appeler la famille", Priority::Medium, false),
        ("Réviser le code", Priority::High, false),
        ("Nettoyer le bureau", Priority::Low, true),
    ]
    .into_iter()
    .map(|(text, priority, done)| Task {
        text: text.to_owned(),
        done,
        priority,
    })
    .collect()
}

/// Relève la météo en arrière-plan, sans jamais retenir le bureau.
pub fn start_weather(handle: &LoopHandle<'static, Ferrite>) -> Result<(), String> {
    let (sender, receiver) = channel::channel::<Weather>();
    handle
        .insert_source(receiver, |event, _, state| {
            if let channel::Event::Msg(weather) = event {
                state.dashboard.weather = Some(weather);
            }
        })
        .map_err(|err| err.to_string())?;
    let place = Place::from_settings(&Settings::load());
    thread::Builder::new()
        .name("ferrite-meteo".to_owned())
        .spawn(move || {
            loop {
                let delay = match weather::fetch(&place) {
                    Ok(weather) => {
                        if sender.send(weather).is_err() {
                            return;
                        }
                        WEATHER_REFRESH
                    }
                    Err(err) => {
                        warn!(%err, "météo indisponible");
                        WEATHER_RETRY
                    }
                };
                thread::sleep(delay);
            }
        })
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn encode(text: &str) -> String {
    let mut encoded = String::new();
    for character in text.chars() {
        match character {
            '\\' => encoded.push_str("\\\\"),
            '=' => encoded.push_str("\\e"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            ' ' => encoded.push_str("\\p"),
            character => encoded.push(character),
        }
    }
    encoded
}

fn decode(text: &str) -> String {
    let mut chars = text.chars();
    let mut decoded = String::new();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('\\') => decoded.push('\\'),
                Some('e') => decoded.push('='),
                Some('n') => decoded.push('\n'),
                Some('r') => decoded.push('\r'),
                Some('t') => decoded.push('\t'),
                Some('p') => decoded.push(' '),
                Some(other) => {
                    decoded.push('\\');
                    decoded.push(other);
                }
                None => decoded.push('\\'),
            }
        } else {
            decoded.push(character);
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_notes_encodent_les_separateurs_du_fichier_de_reglages() {
        let text = "  Ligne 1 = été\\n\n\tLigne 2  ";
        assert_eq!(decode(&encode(text)), text);
    }

    #[test]
    fn la_premiere_ligne_d_une_note_est_son_titre() {
        let note = Note {
            text: "Projet web\n\n  Finaliser\nl'interface ".to_owned(),
            updated: 0,
        };
        assert_eq!(note.title(), "Projet web");
        assert_eq!(note.preview(), "Finaliser l'interface");
    }

    #[test]
    fn une_note_restee_vide_disparait() {
        let mut dashboard = Dashboard::default();
        let editing = dashboard.add_note(5);
        assert_eq!(editing, Some(DashboardEdit::Note(0)));
        assert_eq!(dashboard.finish_editing(editing), Some(0));
        assert!(dashboard.notes.is_empty());

        let editing = dashboard.add_note(5);
        assert!(dashboard.type_char(editing, 'a', 9));
        assert_eq!(dashboard.notes[0].updated, 9);
        assert_eq!(dashboard.finish_editing(editing), None);
        assert_eq!(dashboard.notes[0].text, "a");
    }

    #[test]
    fn la_priorite_tourne_au_clic() {
        let priority = Priority::High;
        assert_eq!(priority.next().next().next(), priority);
        assert_eq!(Priority::from_token(Some(priority.token())), priority);
        assert_eq!(Priority::from_token(None), Priority::Medium);
    }

    #[test]
    fn une_tache_vide_n_est_pas_ajoutee() {
        let mut dashboard = Dashboard::default();
        dashboard.draft = "   ".to_owned();
        assert!(!dashboard.commit_draft());
        assert!(dashboard.tasks.is_empty());
    }
}
