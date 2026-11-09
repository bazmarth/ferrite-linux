//! Données persistantes des widgets du bureau : disposition, notes, tâches et météo.
//!
//! Les tâches sont celles du Calendrier (`ferrite_system::agenda`) : le widget Tâches les
//! lit et les écrit dans le même fichier que l'application, sans copie à part.

use std::{
    cell::RefCell,
    path::PathBuf,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ferrite_system::{
    agenda::{
        date::{Date, DateTime},
        model::{self as agenda, Agenda},
        store,
    },
    config::{Accent, Settings},
    weather::{self, Place, Weather},
};
use smithay::reexports::calloop::{
    LoopHandle, channel,
    timer::{TimeoutAction, Timer},
};
use tracing::warn;

use crate::state::Ferrite;

/// Délai entre deux relevés de la météo, et avant de réessayer après un échec.
const WEATHER_REFRESH: Duration = Duration::from_secs(30 * 60);
const WEATHER_RETRY: Duration = Duration::from_secs(5 * 60);

/// Disposition d'origine : colonnes de 170 px, écart de 16, à gauche sous la barre.
const CELL: i32 = 170;
const GAP: i32 = 16;
const LEFT: i32 = 32;
const TOP: i32 = 58;
/// Marge gardée autour d'un widget, pour qu'il ne sorte jamais de l'écran.
const SCREEN_MARGIN: i32 = 8;
/// Pas de la grille des widgets : une demi-case. Places et tailles s'y alignent, les
/// bords des widgets tombent donc en face les uns des autres, écart de 16 compris.
pub const GRID: i32 = (CELL + GAP) / 2;

/// Widgets disponibles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetKind {
    Clock,
    Calendar,
    System,
    Tasks,
    Notes,
    Quote,
    /// Mini-Dock des applications récemment utilisées.
    Apps,
    /// Lecture en cours (tout lecteur MPRIS : Musique, navigateur…).
    Music,
    /// Widgets Notes de la maquette : écrire une note, dernières notes, liste à cocher.
    NoteQuick,
    NotesRecent,
    NotePinned,
}

impl WidgetKind {
    pub const ALL: [WidgetKind; 11] = [
        WidgetKind::Clock,
        WidgetKind::Calendar,
        WidgetKind::System,
        WidgetKind::Tasks,
        WidgetKind::Notes,
        WidgetKind::Quote,
        WidgetKind::Apps,
        WidgetKind::Music,
        WidgetKind::NoteQuick,
        WidgetKind::NotesRecent,
        WidgetKind::NotePinned,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WidgetKind::Clock => "Horloge",
            WidgetKind::Calendar => "Calendrier",
            WidgetKind::System => "Système",
            WidgetKind::Tasks => "Tâches",
            WidgetKind::Notes => "Notes",
            WidgetKind::Quote => "Citation",
            WidgetKind::Apps => "Applications",
            WidgetKind::Music => "Musique",
            WidgetKind::NoteQuick => "Note rapide",
            WidgetKind::NotesRecent => "Notes récentes",
            WidgetKind::NotePinned => "Note épinglée",
        }
    }

    fn token(self) -> &'static str {
        match self {
            WidgetKind::Clock => "horloge",
            WidgetKind::Calendar => "calendrier",
            WidgetKind::System => "systeme",
            WidgetKind::Tasks => "taches",
            WidgetKind::Notes => "notes",
            WidgetKind::Quote => "citation",
            WidgetKind::Apps => "applications",
            WidgetKind::Music => "musique",
            WidgetKind::NoteQuick => "note-rapide",
            WidgetKind::NotesRecent => "notes-recentes",
            WidgetKind::NotePinned => "note-epinglee",
        }
    }

    /// Taille en dessous de laquelle le contenu ne tiendrait plus.
    fn min_size(self) -> (i32, i32) {
        match self {
            WidgetKind::Clock => (170, 130),
            WidgetKind::Calendar => (170, 170),
            WidgetKind::System => (170, 170),
            WidgetKind::Tasks => (220, 100),
            WidgetKind::Notes => (160, 100),
            WidgetKind::Quote => (200, 80),
            WidgetKind::Apps => (170, 110),
            WidgetKind::Music => (300, 150),
            WidgetKind::NoteQuick => (170, 150),
            WidgetKind::NotesRecent => (300, 150),
            WidgetKind::NotePinned => (300, 300),
        }
    }

    /// Place d'origine (celle de la maquette).
    fn default_slot(self) -> WidgetSlot {
        let wide = 2 * CELL + GAP;
        let second_row = TOP + CELL + GAP;
        let (x, y, w, h) = match self {
            WidgetKind::Clock => (LEFT, TOP, wide, CELL),
            WidgetKind::Calendar => (LEFT + wide + GAP, TOP, CELL, CELL),
            WidgetKind::System => (LEFT + wide + 2 * GAP + CELL, TOP, CELL, CELL),
            WidgetKind::Tasks => (LEFT, second_row, wide, CELL),
            WidgetKind::Notes => (LEFT + wide + GAP, second_row, wide, CELL),
            WidgetKind::Quote => (LEFT, second_row + CELL + GAP, 2 * wide + GAP, CELL),
            WidgetKind::Apps => (LEFT + 2 * (wide + GAP), second_row + CELL + GAP, wide, CELL),
            WidgetKind::Music => (LEFT, second_row + 2 * (CELL + GAP), wide, CELL),
            WidgetKind::NoteQuick => (LEFT + wide + GAP, second_row + 2 * (CELL + GAP), CELL, CELL),
            WidgetKind::NotesRecent => (LEFT + 2 * (wide + GAP), TOP, wide, CELL),
            WidgetKind::NotePinned => (LEFT + 2 * (wide + GAP), TOP + CELL + GAP, wide, wide),
        };
        WidgetSlot {
            kind: self,
            x,
            y,
            w,
            h,
        }
    }
}

/// Un widget posé sur le bureau : place et taille libres, en points de l'écran. Le dernier
/// de la liste passe devant les autres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetSlot {
    pub kind: WidgetKind,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// `value` sur la ligne de la grille la plus proche, en partant de `origin`.
fn snap(value: i32, origin: i32) -> i32 {
    origin + ((value - origin) as f64 / f64::from(GRID)).round() as i32 * GRID
}

/// Taille d'un nombre entier de pas, écart compris, jamais sous `min`.
fn snap_size(size: i32, min: i32) -> i32 {
    let steps = (f64::from(size + GAP) / f64::from(GRID)).round().max(1.0) as i32;
    let fitting = (f64::from(min + GAP) / f64::from(GRID)).ceil() as i32;
    steps.max(fitting) * GRID - GAP
}

impl WidgetSlot {
    /// Aligné sur la grille : place et taille.
    pub fn snapped(self) -> Self {
        let (min_w, min_h) = self.kind.min_size();
        Self {
            x: snap(self.x, LEFT),
            y: snap(self.y, TOP),
            w: snap_size(self.w, min_w),
            h: snap_size(self.h, min_h),
            ..self
        }
    }

    fn overlaps(&self, other: &WidgetSlot) -> bool {
        self.x < other.x + other.w + GAP
            && other.x < self.x + self.w + GAP
            && self.y < other.y + other.h + GAP
            && other.y < self.y + self.h + GAP
    }

    /// Ramené dans l'écran de `width` × `height`, sous la barre de hauteur `bar`.
    pub fn clamped(self, width: i32, height: i32, bar: i32) -> Self {
        let (min_w, min_h) = self.kind.min_size();
        let w = self.w.max(min_w).min((width - 2 * SCREEN_MARGIN).max(min_w));
        let h = self
            .h
            .max(min_h)
            .min((height - bar - 2 * SCREEN_MARGIN).max(min_h));
        Self {
            w,
            h,
            x: self.x.min(width - w - SCREEN_MARGIN).max(SCREEN_MARGIN),
            y: self.y.min(height - h - SCREEN_MARGIN).max(bar + SCREEN_MARGIN),
            ..self
        }
    }
}

/// Bords saisis pour redimensionner ; aucun : le widget se déplace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WidgetEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl WidgetEdges {
    /// Le widget `start` après un glissement de (`dx`, `dy`). Le bord opposé ne bouge pas,
    /// même quand la taille minimale arrête le geste.
    pub fn apply(self, start: WidgetSlot, dx: i32, dy: i32) -> WidgetSlot {
        if self == WidgetEdges::default() {
            return WidgetSlot {
                x: start.x + dx,
                y: start.y + dy,
                ..start
            };
        }
        let (min_w, min_h) = start.kind.min_size();
        let mut slot = start;
        if self.right {
            slot.w = (start.w + dx).max(min_w);
        }
        if self.left {
            slot.w = (start.w - dx).max(min_w);
            slot.x = start.x + start.w - slot.w;
        }
        if self.bottom {
            slot.h = (start.h + dy).max(min_h);
        }
        if self.top {
            slot.h = (start.h - dy).max(min_h);
            slot.y = start.y + start.h - slot.h;
        }
        slot
    }
}

/// Place sur la grille du widget `index`, lâché à sa position actuelle : la case la plus
/// proche, ou la plus proche qui ne chevauche aucun autre widget, dans l'écran de
/// `width` × `height` sous la barre de hauteur `bar`.
pub fn place_on_grid(
    layout: &[WidgetSlot],
    index: usize,
    width: i32,
    height: i32,
    bar: i32,
) -> Option<WidgetSlot> {
    let wanted = layout.get(index)?.snapped();
    let others: Vec<WidgetSlot> = layout
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != index)
        .map(|(_, slot)| slot.clamped(width, height, bar))
        .collect();
    let fits = |slot: &WidgetSlot| {
        slot.x >= SCREEN_MARGIN
            && slot.y >= bar + SCREEN_MARGIN
            && slot.x + slot.w <= width - SCREEN_MARGIN
            && slot.y + slot.h <= height - SCREEN_MARGIN
            && others.iter().all(|other| !slot.overlaps(other))
    };
    if fits(&wanted) {
        return Some(wanted);
    }
    // Cases de la grille, de la plus proche à la plus lointaine.
    let columns = (width - LEFT) / GRID + 1;
    let rows = (height - TOP) / GRID + 1;
    let mut candidates: Vec<WidgetSlot> = (-1..columns)
        .flat_map(|column| (-1..rows).map(move |row| (column, row)))
        .map(|(column, row)| WidgetSlot {
            x: LEFT + column * GRID,
            y: TOP + row * GRID,
            ..wanted
        })
        .filter(fits)
        .collect();
    candidates.sort_by_key(|slot| (slot.x - wanted.x).pow(2) + (slot.y - wanted.y).pow(2));
    Some(candidates.first().copied().unwrap_or(wanted))
}

/// Disposition d'origine : celle de la maquette. Les widgets venus après (Musique) s'ajoutent
/// par « Modifier les widgets ».
pub fn default_layout() -> Vec<WidgetSlot> {
    WidgetKind::ALL
        .into_iter()
        .filter(|kind| {
            !matches!(
                kind,
                WidgetKind::Music | WidgetKind::NoteQuick | WidgetKind::NotesRecent | WidgetKind::NotePinned
            )
        })
        .map(WidgetKind::default_slot)
        .collect()
}

/// Lit `horloge:32:58:356:170,…` ; ce qui n'existe plus est ignoré, sans doublon.
fn parse_layout(text: &str) -> Vec<WidgetSlot> {
    let mut layout: Vec<WidgetSlot> = Vec::new();
    for item in text.split(',') {
        let mut parts = item.split(':');
        let token = parts.next().unwrap_or_default();
        let kind = WidgetKind::ALL.into_iter().find(|kind| kind.token() == token);
        let numbers: Vec<i32> = parts.filter_map(|part| part.parse().ok()).collect();
        if let (Some(kind), [x, y, w, h]) = (kind, numbers.as_slice())
            && layout.iter().all(|slot| slot.kind != kind)
        {
            // Une disposition d'avant la grille s'y aligne.
            layout.push(
                WidgetSlot {
                    kind,
                    x: *x,
                    y: *y,
                    w: *w,
                    h: *h,
                }
                .snapped(),
            );
        }
    }
    layout
}

fn layout_text(layout: &[WidgetSlot]) -> String {
    layout
        .iter()
        .map(|slot| {
            format!(
                "{}:{}:{}:{}:{}",
                slot.kind.token(),
                slot.x,
                slot.y,
                slot.w,
                slot.h
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardEdit {
    Note(usize),
    NewTask,
    /// Nom d'une liste de tâches, juste créée.
    ListName(usize),
}

/// Plus long nom de liste de tâches.
const LIST_NAME_MAX: usize = 32;
/// Listes de tâches au plus.
const LISTS_MAX: usize = 20;

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

    fn to_agenda(self) -> agenda::Priority {
        match self {
            Priority::High => agenda::Priority::High,
            Priority::Medium => agenda::Priority::Normal,
            Priority::Low => agenda::Priority::Low,
        }
    }

    fn from_agenda(priority: agenda::Priority) -> Self {
        match priority {
            agenda::Priority::High => Priority::High,
            agenda::Priority::Normal => Priority::Medium,
            agenda::Priority::Low => Priority::Low,
        }
    }

    #[cfg(test)]
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

/// Ligne à cocher d'une note (`- [ ] texte`, `- [x] texte`, `[ ] texte`) : numéro de la
/// ligne dans la note, cochée ou non, texte.
pub fn checklist(lines: &[String]) -> Vec<(usize, bool, String)> {
    lines
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(|(index, line)| {
            let rest = line.trim_start();
            let rest = rest
                .strip_prefix("- ")
                .or_else(|| rest.strip_prefix("* "))
                .unwrap_or(rest);
            let (done, text) = match rest.strip_prefix("[ ]") {
                Some(text) => (false, text),
                None => (
                    true,
                    rest.strip_prefix("[x]").or_else(|| rest.strip_prefix("[X]"))?,
                ),
            };
            Some((index, done, text.trim().to_owned()))
        })
        .collect()
}

/// Tâche du Calendrier, telle que le widget la montre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Numéro de la tâche dans l'agenda.
    pub id: u64,
    pub text: String,
    pub done: bool,
    pub priority: Priority,
}

/// Une feuille du widget Tâches : une liste du Calendrier, ou (`id` vide) les tâches
/// rangées dans aucune liste.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskList {
    pub id: Option<u64>,
    pub name: String,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dashboard {
    pub notes: Vec<Note>,
    /// Feuilles du widget Tâches, tirées de `agenda` ; `list_shown` est celle affichée.
    pub lists: Vec<TaskList>,
    /// Agenda du Calendrier : listes et tâches du widget Tâches, jours occupés des
    /// calendriers du bureau.
    pub agenda: Agenda,
    /// Fichier de l'agenda ; aucun dans les essais (tout reste en mémoire).
    agenda_file: Option<PathBuf>,
    agenda_stamp: Option<SystemTime>,
    /// Jours occupés déjà calculés, par mois ; vidé quand l'agenda change.
    busy: RefCell<Vec<((i32, u32), u32)>>,
    pub list_shown: usize,
    pub draft: String,
    pub month_offset: i32,
    /// Widgets posés, du dessous vers le dessus.
    pub layout: Vec<WidgetSlot>,
    /// Note affichée par le widget Notes.
    pub note_shown: usize,
    /// Première tâche affichée, quand elles ne tiennent pas toutes.
    pub tasks_scroll: usize,
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
        let agenda_file = store::agenda_path();
        let agenda = store::load_agenda(&agenda_file).unwrap_or_else(|error| {
            warn!(%error, "agenda du Calendrier illisible");
            Agenda::default()
        });
        let mut layout = settings
            .get("widgets_disposition")
            .map_or_else(default_layout, parse_layout);
        // Le mini-Dock des applications récentes rejoint une fois les dispositions d'avant.
        if settings.get("widgets_applications").is_none()
            && layout.iter().all(|slot| slot.kind != WidgetKind::Apps)
        {
            let mut slot = WidgetKind::Apps.default_slot();
            let bottom = layout.iter().map(|slot| slot.y + slot.h).max().unwrap_or(TOP);
            if layout.iter().any(|other| other.overlaps(&slot)) {
                slot.x = LEFT;
                slot.y = snap(bottom + GAP, TOP);
            }
            layout.push(slot);
        }
        let mut dashboard = Self {
            notes,
            list_shown: settings
                .get("tableau_liste_affichee")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            layout,
            place: Place::from_settings(&settings).name,
            agenda_stamp: store::modified(&agenda_file),
            agenda,
            agenda_file: Some(agenda_file),
            ..Self::default()
        };
        dashboard.import_old_tasks(settings);
        dashboard.rebuild_lists();
        dashboard
    }

    /// Tableau de bord sur un agenda en mémoire seulement (essais).
    #[cfg(test)]
    pub fn with_agenda(agenda: Agenda) -> Self {
        let mut dashboard = Self {
            agenda,
            ..Self::default()
        };
        dashboard.rebuild_lists();
        dashboard
    }

    /// Les tâches d'avant le Calendrier, gardées dans les réglages, rejoignent l'agenda
    /// une seule fois : chaque feuille dans la liste du même nom, créée s'il le faut.
    fn import_old_tasks(&mut self, mut settings: Settings) {
        const IMPORTED: &str = "tableau_taches_calendrier";
        if settings.get(IMPORTED).is_some()
            || (settings.get("tableau_listes").is_none() && settings.get("tableau_taches").is_none())
        {
            return;
        }
        let old = load_lists(&settings);
        let now = local_now();
        let imported = self.edit_agenda(|agenda| {
            for list in old {
                let id = match agenda
                    .lists
                    .iter()
                    .find(|known| known.name.to_lowercase() == list.name.to_lowercase())
                {
                    Some(known) => known.id,
                    None => new_list(agenda, list.name.clone()),
                };
                for task in list.tasks {
                    let id = new_task(agenda, Some(id), task.text, now);
                    if let Some(added) = agenda.task_mut(id) {
                        added.priority = task.priority.to_agenda();
                        added.done = task.done;
                        added.completed = task.done.then_some(now);
                    }
                }
            }
            true
        });
        if imported {
            settings.set(IMPORTED, "oui");
            if let Err(error) = settings.save() {
                warn!(%error, "reprise des tâches non notée");
            }
        }
    }

    /// Feuilles du widget d'après l'agenda : ses listes, dans l'ordre, puis les tâches
    /// sans liste s'il y en a (ou s'il n'y a aucune liste).
    fn rebuild_lists(&mut self) {
        let shown = |task: &agenda::Task| Task {
            id: task.id,
            text: task.title.clone(),
            done: task.done,
            priority: Priority::from_agenda(task.priority),
        };
        let mut lists: Vec<TaskList> = self
            .agenda
            .lists
            .iter()
            .map(|list| TaskList {
                id: Some(list.id),
                name: list.name.clone(),
                tasks: self
                    .agenda
                    .tasks
                    .iter()
                    .filter(|task| task.list == Some(list.id))
                    .map(shown)
                    .collect(),
            })
            .collect();
        let loose: Vec<Task> = self
            .agenda
            .tasks
            .iter()
            .filter(|task| task.list.is_none_or(|id| self.agenda.list(Some(id)).is_none()))
            .map(shown)
            .collect();
        if !loose.is_empty() || lists.is_empty() {
            lists.push(TaskList {
                id: None,
                name: LOOSE_LIST.to_owned(),
                tasks: loose,
            });
        }
        self.lists = lists;
        self.list_shown = self.list_shown.min(self.lists.len() - 1);
        self.busy.borrow_mut().clear();
    }

    /// Change l'agenda : relu juste avant (l'application a pu l'écrire), changé par
    /// `change`, puis enregistré s'il a changé (`change` renvoie vrai).
    fn edit_agenda(&mut self, change: impl FnOnce(&mut Agenda) -> bool) -> bool {
        if let Some(file) = &self.agenda_file {
            match store::load_agenda(file) {
                Ok(agenda) => self.agenda = agenda,
                Err(error) => {
                    warn!(%error, "agenda du Calendrier illisible, rien n'est changé");
                    return false;
                }
            }
        }
        let changed = change(&mut self.agenda);
        if changed && let Some(file) = &self.agenda_file {
            if let Err(error) = store::save_agenda(file, &self.agenda) {
                warn!(%error, "agenda du Calendrier non enregistré");
            }
            self.agenda_stamp = store::modified(file);
        }
        self.rebuild_lists();
        changed
    }

    /// Relit l'agenda s'il a changé ailleurs (l'application Calendrier) ; vrai s'il faut
    /// redessiner.
    pub fn refresh_agenda(&mut self) -> bool {
        let Some(file) = &self.agenda_file else {
            return false;
        };
        let stamp = store::modified(file);
        if stamp == self.agenda_stamp {
            return false;
        }
        self.agenda_stamp = stamp;
        match store::load_agenda(file) {
            Ok(agenda) => {
                self.agenda = agenda;
                self.rebuild_lists();
                true
            }
            Err(error) => {
                warn!(%error, "agenda du Calendrier illisible");
                false
            }
        }
    }

    /// Jours du mois qui ont un événement ou une tâche à faire : un bit par jour (bit 0
    /// pour le 1er).
    pub fn busy_days(&self, year: i32, month: u32) -> u32 {
        if let Some((_, bits)) = self.busy.borrow().iter().find(|(key, _)| *key == (year, month)) {
            return *bits;
        }
        let bits = busy_days(&self.agenda, year, month);
        let mut cache = self.busy.borrow_mut();
        if cache.len() >= 4 {
            cache.remove(0);
        }
        cache.push(((year, month), bits));
        bits
    }

    /// La feuille affichée est celle des tâches sans liste (qu'on ne supprime pas).
    pub fn loose_list_shown(&self) -> bool {
        self.lists
            .get(self.list_shown)
            .is_some_and(|list| list.id.is_none())
    }

    pub fn save(&self) -> std::io::Result<()> {
        let mut settings = Settings::load();
        settings.set("widgets_disposition", &layout_text(&self.layout));
        settings.set("widgets_applications", "oui");
        settings.set("tableau_notes_nombre", &self.notes.len().to_string());
        for (index, note) in self.notes.iter().enumerate() {
            settings.set(&format!("tableau_note_{index}"), &encode(&note.text));
            settings.set(&format!("tableau_note_{index}_date"), &note.updated.to_string());
        }
        // Les tâches, elles, sont dans l'agenda du Calendrier, enregistré à chaque changement.
        settings.set("tableau_liste_affichee", &self.list_shown.to_string());
        settings.save()
    }

    /// Widgets qu'on peut encore ajouter.
    pub fn absent_widgets(layout: &[WidgetSlot]) -> Vec<WidgetKind> {
        WidgetKind::ALL
            .into_iter()
            .filter(|kind| layout.iter().all(|slot| slot.kind != *kind))
            .collect()
    }

    pub fn add_widget(&mut self, kind: WidgetKind) {
        if self.layout.iter().all(|slot| slot.kind != kind) {
            self.layout.push(kind.default_slot());
        }
    }

    pub fn remove_widget(&mut self, index: usize) {
        if index < self.layout.len() {
            self.layout.remove(index);
        }
    }

    /// Passe le widget devant les autres ; renvoie sa nouvelle place dans la liste.
    pub fn raise_widget(&mut self, index: usize) -> usize {
        if index < self.layout.len() {
            let slot = self.layout.remove(index);
            self.layout.push(slot);
        }
        self.layout.len().saturating_sub(1)
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
        self.note_shown = 0;
        Some(DashboardEdit::Note(0))
    }

    /// Tâches de la feuille affichée.
    pub fn tasks(&self) -> &[Task] {
        self.lists
            .get(self.list_shown.min(self.lists.len().saturating_sub(1)))
            .map_or(&[], |list| list.tasks.as_slice())
    }

    /// Tâche `index` de la feuille affichée : son numéro dans l'agenda.
    fn task_id(&self, index: usize) -> Option<u64> {
        self.tasks().get(index).map(|task| task.id)
    }

    /// Coche ou décoche la tâche `index` de la feuille affichée.
    pub fn toggle_task(&mut self, index: usize) -> bool {
        let Some(id) = self.task_id(index) else {
            return false;
        };
        let now = local_now();
        self.edit_agenda(|agenda| {
            let Some(done) = agenda.task(id).map(|task| !task.done) else {
                return false;
            };
            agenda.set_done(id, done, now);
            true
        })
    }

    /// Priorité suivante pour la tâche `index` de la feuille affichée.
    pub fn cycle_priority(&mut self, index: usize) -> bool {
        let Some(id) = self.task_id(index) else {
            return false;
        };
        self.edit_agenda(|agenda| {
            agenda.task_mut(id).is_some_and(|task| {
                task.priority = Priority::from_agenda(task.priority).next().to_agenda();
                true
            })
        })
    }

    /// Affiche la feuille `index`.
    pub fn show_list(&mut self, index: usize) {
        if index < self.lists.len() {
            self.list_shown = index;
            self.tasks_scroll = 0;
        }
    }

    /// Nouvelle feuille de tâches, affichée, son nom à saisir.
    pub fn add_list(&mut self) -> Option<DashboardEdit> {
        if self.agenda.lists.len() >= LISTS_MAX {
            return None;
        }
        let mut created = None;
        self.edit_agenda(|agenda| {
            created = Some(new_list(agenda, String::new()));
            true
        });
        let created = created?;
        let index = self.lists.iter().position(|list| list.id == Some(created))?;
        self.show_list(index);
        Some(DashboardEdit::ListName(index))
    }

    /// Renomme la feuille `index` (une liste du Calendrier).
    fn rename_list(&mut self, index: usize, name: String) -> bool {
        let Some(id) = self.lists.get(index).and_then(|list| list.id) else {
            return false;
        };
        self.edit_agenda(|agenda| {
            agenda
                .lists
                .iter_mut()
                .find(|list| list.id == id)
                .is_some_and(|list| {
                    list.name = name;
                    true
                })
        })
    }

    /// Supprime une liste, comme le Calendrier : ses tâches restent, sans liste. La
    /// feuille des tâches sans liste ne se supprime pas.
    pub fn remove_list(&mut self, index: usize) {
        let Some(id) = self.lists.get(index).and_then(|list| list.id) else {
            return;
        };
        if self.list_shown > index {
            self.list_shown -= 1;
        }
        self.edit_agenda(|agenda| {
            agenda.remove_list(id);
            true
        });
        self.tasks_scroll = 0;
    }

    /// Supprime une note ; la suivante s'affiche.
    /// Coche ou décoche la ligne `line` de la note `note` (widget Note épinglée).
    pub fn toggle_check(&mut self, note: usize, line: usize, now: u64) -> bool {
        let Some(note) = self.notes.get_mut(note) else {
            return false;
        };
        let mut lines: Vec<String> = note.text.lines().map(str::to_owned).collect();
        let Some(text) = lines.get_mut(line) else {
            return false;
        };
        let toggled = if let Some(at) = text.find("[ ]") {
            text.replace_range(at..at + 3, "[x]");
            true
        } else if let Some(at) = text.find("[x]").or_else(|| text.find("[X]")) {
            text.replace_range(at..at + 3, "[ ]");
            true
        } else {
            false
        };
        if toggled {
            note.text = lines.join("\n");
            note.updated = now;
        }
        toggled
    }

    pub fn remove_note(&mut self, index: usize) {
        if index < self.notes.len() {
            self.notes.remove(index);
            self.note_shown = self.note_shown.min(self.notes.len().saturating_sub(1));
        }
    }

    pub fn remove_task(&mut self, index: usize) -> bool {
        let Some(id) = self.task_id(index) else {
            return false;
        };
        self.edit_agenda(|agenda| {
            agenda.remove_task(id);
            true
        })
    }

    /// Fin de la saisie : une note restée vide disparaît (renvoie sa place) ; une feuille
    /// restée sans nom en reçoit un.
    pub fn finish_editing(&mut self, editing: Option<DashboardEdit>) -> Option<usize> {
        if let Some(DashboardEdit::ListName(index)) = editing {
            if self
                .lists
                .get(index)
                .is_some_and(|list| list.name.trim().is_empty())
            {
                self.rename_list(index, format!("Liste {}", index + 1));
            }
            return None;
        }
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
            // La liste est enregistrée dans l'agenda : rien de plus à enregistrer.
            Some(DashboardEdit::ListName(index)) => {
                if let Some(list) = self.lists.get(index)
                    && list.name.chars().count() < LIST_NAME_MAX
                {
                    let name = format!("{}{character}", list.name);
                    self.rename_list(index, name);
                }
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
            Some(DashboardEdit::ListName(index)) => {
                if let Some(list) = self.lists.get(index) {
                    let mut name = list.name.clone();
                    name.pop();
                    self.rename_list(index, name);
                }
                false
            }
            None => false,
        }
    }

    pub fn commit_draft(&mut self) -> bool {
        if self.tasks().len() >= 100 {
            return false;
        }
        let text = self.draft.trim();
        if text.is_empty() {
            self.draft.clear();
            return false;
        }
        let text = text.to_owned();
        let list = self.lists.get(self.list_shown).and_then(|list| list.id);
        let now = local_now();
        self.draft.clear();
        self.edit_agenda(|agenda| {
            new_task(agenda, list, text, now);
            true
        })
    }
}

/// Anciennes feuilles de tâches des réglages (avant le Calendrier), pour les reprendre ;
/// avant les feuilles, une seule liste « Tâches ».
fn load_lists(settings: &Settings) -> Vec<TaskList> {
    let task = |key: &str| {
        Some(Task {
            id: 0,
            text: decode(settings.get(key)?),
            done: settings.get(&format!("{key}_faite")) == Some("oui"),
            priority: Priority::from_token(settings.get(&format!("{key}_priorite"))),
        })
    };
    let count = |key: &str| {
        settings
            .get(key)
            .and_then(|value| value.parse::<usize>().ok())
            .map(|count| count.min(100))
    };
    if let Some(lists) = count("tableau_listes") {
        let lists: Vec<TaskList> = (0..lists.min(LISTS_MAX))
            .map(|list| {
                let key = format!("tableau_liste_{list}");
                TaskList {
                    id: None,
                    name: settings
                        .get(&format!("{key}_nom"))
                        .map_or_else(|| format!("Liste {}", list + 1), decode),
                    tasks: (0..count(&format!("{key}_taches")).unwrap_or(0))
                        .filter_map(|index| task(&format!("{key}_tache_{index}")))
                        .collect(),
                }
            })
            .collect();
        if !lists.is_empty() {
            return lists;
        }
    }
    let tasks = match count("tableau_taches") {
        Some(tasks) => (0..tasks)
            .filter_map(|index| task(&format!("tableau_tache_{index}")))
            .collect(),
        None => Vec::new(),
    };
    vec![TaskList {
        id: None,
        name: "Tâches".to_owned(),
        tasks,
    }]
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
    vec![Note {
        text: "Idées pour Ferrite\nCentre de contrôle : chevrons Wi-Fi et Bluetooth. Widgets en grille \
               4 colonnes, verre sombre. Tester le scanout direct…"
            .to_owned(),
        updated: now_seconds(),
    }]
}

/// Nom de la feuille des tâches rangées dans aucune liste du Calendrier.
pub const LOOSE_LIST: &str = "Sans liste";

/// Maintenant, en heure locale, pour l'agenda (sans lancer de commande : le décalage
/// horaire est celui que l'horloge de la barre a déjà lu).
fn local_now() -> DateTime {
    crate::desktop::panels::local_minutes().map_or_else(|| DateTime::from_stamp(0), DateTime::from_stamp)
}

/// Nouvelle liste dans l'agenda, d'une couleur différente de la précédente.
fn new_list(agenda: &mut Agenda, name: String) -> u64 {
    let id = agenda.new_id();
    let color = Accent::ALL[(agenda.lists.len() + 3) % Accent::ALL.len()];
    agenda.lists.push(agenda::TaskList { id, name, color });
    id
}

/// Nouvelle tâche à faire dans l'agenda, sans échéance.
fn new_task(agenda: &mut Agenda, list: Option<u64>, title: String, now: DateTime) -> u64 {
    let id = agenda.new_id();
    agenda.tasks.push(agenda::Task {
        id,
        title,
        notes: String::new(),
        list,
        due: None,
        due_time: None,
        priority: agenda::Priority::Normal,
        done: false,
        completed: None,
        event: None,
        reminders: Vec::new(),
        created: now,
    });
    id
}

/// Jours du mois `month` qui ont un événement ou une tâche à faire (un bit par jour).
fn busy_days(agenda: &Agenda, year: i32, month: u32) -> u32 {
    let Some(first) = Date::new(year, month, 1) else {
        return 0;
    };
    let next = first.add_months(1);
    let mut bits = 0u32;
    let mut mark = |date: Date| {
        if date >= first && date < next {
            bits |= 1 << (date.day - 1);
        }
    };
    for occurrence in agenda.occurrences(first, next, |_| true) {
        let mut day = occurrence.start.date.max(first);
        let last = occurrence.last_day().min(next.add_days(-1));
        while day <= last {
            mark(day);
            day = day.add_days(1);
        }
    }
    for task in agenda.tasks.iter().filter(|task| !task.done) {
        if let Some(due) = task.due {
            mark(due);
        }
    }
    bits
}

/// Relit l'agenda du Calendrier toutes les deux secondes s'il a changé, pour que les
/// widgets et le calendrier de la barre suivent l'application.
pub fn start_agenda_watch(handle: &LoopHandle<'static, Ferrite>) -> Result<(), String> {
    const CHECK: Duration = Duration::from_secs(2);
    handle
        .insert_source(Timer::from_duration(CHECK), |_, _, state| {
            if state.dashboard.refresh_agenda() {
                state.request_redraw();
            }
            TimeoutAction::ToDuration(CHECK)
        })
        .map(drop)
        .map_err(|error| error.to_string())
}

/// Relève la météo en arrière-plan, sans jamais retenir le bureau.
pub fn start_weather(handle: &LoopHandle<'static, Ferrite>) -> Result<(), String> {
    let (sender, receiver) = channel::channel::<Weather>();
    handle
        .insert_source(receiver, |event, _, state| {
            if let channel::Event::Msg(weather) = event {
                state.dashboard.weather = Some(weather);
                state.request_redraw();
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

    fn slot(kind: WidgetKind) -> WidgetSlot {
        kind.default_slot()
    }

    #[test]
    fn la_disposition_d_origine_reproduit_la_maquette() {
        let places: Vec<_> = default_layout()
            .iter()
            .map(|slot| (slot.x, slot.y, slot.w, slot.h))
            .collect();
        assert_eq!(
            places,
            vec![
                (32, 58, 356, 170),
                (404, 58, 170, 170),
                (590, 58, 170, 170),
                (32, 244, 356, 170),
                (404, 244, 356, 170),
                (32, 430, 728, 170),
                (776, 430, 356, 170),
            ]
        );
        // Toutes sur la grille.
        assert!(default_layout().iter().all(|slot| slot.snapped() == *slot));
    }

    #[test]
    fn un_widget_lache_s_aligne_sur_la_grille() {
        let loose = WidgetSlot {
            x: 900,
            y: 300,
            w: 400,
            h: 260,
            ..slot(WidgetKind::Notes)
        };
        let snapped = loose.snapped();
        assert_eq!((snapped.x, snapped.y, snapped.w, snapped.h), (869, 337, 356, 263));
        // Jamais sous la taille minimale : la citation garde deux pas de haut.
        let thin = WidgetSlot {
            h: 40,
            ..slot(WidgetKind::Quote)
        };
        assert_eq!(thin.snapped().h, 170);
    }

    #[test]
    fn un_widget_lache_sur_un_autre_prend_la_place_libre_la_plus_proche() {
        let mut layout = default_layout();
        // Le calendrier lâché presque sur l'horloge.
        layout[1].x = 60;
        layout[1].y = 70;
        let placed = place_on_grid(&layout, 1, 1600, 1000, 34).expect("widget");
        let others: Vec<WidgetSlot> = layout
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != 1)
            .map(|(_, slot)| *slot)
            .collect();
        assert!(others.iter().all(|other| !placed.overlaps(other)));
        assert_eq!(placed.snapped(), placed);
        // Lâché sur une place libre, il y reste.
        layout[1].x = 1200;
        layout[1].y = 60;
        let free = place_on_grid(&layout, 1, 1600, 1000, 34).expect("widget");
        assert_eq!((free.x, free.y), (1241, 58));
    }

    #[test]
    fn la_disposition_se_relit_et_ignore_l_inconnu() {
        let layout = default_layout();
        assert_eq!(parse_layout(&layout_text(&layout)), layout);
        let read = parse_layout("notes:900:300:400:260,inconnu:1:1:1:1,notes:0:0:9:9,taches:2");
        assert_eq!(
            read,
            vec![WidgetSlot {
                kind: WidgetKind::Notes,
                x: 869,
                y: 337,
                w: 356,
                h: 263
            }]
        );
    }

    #[test]
    fn un_widget_se_deplace_librement() {
        let moved = WidgetEdges::default().apply(slot(WidgetKind::Notes), 300, 123);
        assert_eq!((moved.x, moved.y, moved.w, moved.h), (704, 367, 356, 170));
    }

    #[test]
    fn un_widget_se_redimensionne_par_ses_bords_sans_passer_sous_son_minimum() {
        let start = slot(WidgetKind::Calendar);
        let corner = WidgetEdges {
            right: true,
            bottom: true,
            ..WidgetEdges::default()
        };
        let bigger = corner.apply(start, 200, 90);
        assert_eq!((bigger.x, bigger.y, bigger.w, bigger.h), (404, 58, 370, 260));
        // Le bord opposé reste en place, même arrêté par la taille minimale.
        let left = WidgetEdges {
            left: true,
            top: true,
            ..WidgetEdges::default()
        };
        let smaller = left.apply(bigger, 500, 500);
        assert_eq!((smaller.w, smaller.h), (170, 170));
        assert_eq!(smaller.x + smaller.w, bigger.x + bigger.w);
        assert_eq!(smaller.y + smaller.h, bigger.y + bigger.h);
    }

    #[test]
    fn un_widget_reste_dans_l_ecran() {
        let far = WidgetSlot {
            x: 5000,
            y: -300,
            w: 9000,
            h: 400,
            ..slot(WidgetKind::Tasks)
        };
        let shown = far.clamped(1280, 800, 34);
        assert_eq!(shown.w, 1264);
        assert_eq!(shown.x, 8);
        assert_eq!(shown.y, 42);
        assert!(shown.y + shown.h <= 792);
    }

    #[test]
    fn un_widget_saisi_passe_devant_et_un_retire_peut_revenir() {
        let mut dashboard = Dashboard {
            layout: default_layout(),
            ..Dashboard::default()
        };
        assert_eq!(dashboard.raise_widget(1), 6);
        assert_eq!(dashboard.layout[6].kind, WidgetKind::Calendar);
        dashboard.remove_widget(6);
        assert_eq!(
            Dashboard::absent_widgets(&dashboard.layout),
            vec![
                WidgetKind::Calendar,
                WidgetKind::Music,
                WidgetKind::NoteQuick,
                WidgetKind::NotesRecent,
                WidgetKind::NotePinned,
            ]
        );
        dashboard.add_widget(WidgetKind::Calendar);
        dashboard.add_widget(WidgetKind::Calendar);
        assert_eq!(dashboard.layout.len(), 7);
    }

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
        let mut dashboard = Dashboard {
            draft: "   ".to_owned(),
            ..Dashboard::default()
        };
        assert!(!dashboard.commit_draft());
        assert!(dashboard.tasks().is_empty());
    }

    #[test]
    fn feuilles_de_taches_et_notes_s_ajoutent_et_se_suppriment() {
        let mut dashboard = Dashboard::with_agenda(Agenda::empty());
        // Sans liste au Calendrier, une seule feuille : les tâches sans liste.
        assert_eq!(dashboard.lists.len(), 1);
        assert!(dashboard.loose_list_shown());
        dashboard.draft = "Pain".to_owned();
        assert!(dashboard.commit_draft());
        assert_eq!(dashboard.tasks()[0].text, "Pain");
        assert_eq!(dashboard.agenda.tasks[0].list, None);

        // Nouvelle feuille = nouvelle liste du Calendrier, nommée à la saisie ; sans nom,
        // elle en reçoit un. Les listes passent avant les tâches sans liste.
        let editing = dashboard.add_list();
        assert_eq!(editing, Some(DashboardEdit::ListName(0)));
        assert!(dashboard.tasks().is_empty());
        for character in "Courses".chars() {
            dashboard.type_char(editing, character, 0);
        }
        dashboard.erase_char(editing, 0);
        dashboard.finish_editing(editing);
        assert_eq!(dashboard.agenda.lists[0].name, "Course");
        let unnamed = dashboard.add_list();
        assert_eq!(unnamed, Some(DashboardEdit::ListName(1)));
        dashboard.finish_editing(unnamed);
        assert_eq!(dashboard.lists[1].name, "Liste 2");
        assert_eq!(dashboard.lists[2].name, LOOSE_LIST);

        // Chaque feuille garde ses tâches, rangées dans sa liste du Calendrier.
        dashboard.show_list(0);
        dashboard.draft = "Lait".to_owned();
        dashboard.commit_draft();
        let course = dashboard.lists[0].id;
        assert_eq!(dashboard.agenda.tasks[1].list, course);
        assert!(dashboard.toggle_task(0));
        assert!(dashboard.agenda.tasks[1].done);
        assert!(dashboard.cycle_priority(0));
        assert_eq!(dashboard.agenda.tasks[1].priority, agenda::Priority::Low);
        dashboard.show_list(2);
        assert_eq!(dashboard.tasks()[0].text, "Pain");
        assert!(dashboard.remove_task(0));
        // Plus de tâche sans liste : la feuille disparaît.
        assert_eq!(dashboard.lists.len(), 2);

        // Supprimer une liste garde ses tâches, sans liste (comme le Calendrier).
        dashboard.show_list(1);
        dashboard.remove_list(0);
        assert_eq!(dashboard.lists[dashboard.list_shown].name, "Liste 2");
        assert_eq!(dashboard.lists[1].name, LOOSE_LIST);
        assert_eq!(dashboard.lists[1].tasks[0].text, "Lait");
        // La feuille des tâches sans liste ne se supprime pas.
        dashboard.remove_list(1);
        assert_eq!(dashboard.lists.len(), 2);

        dashboard.add_note(1);
        dashboard.add_note(2);
        dashboard.note_shown = 1;
        dashboard.remove_note(1);
        assert_eq!((dashboard.notes.len(), dashboard.note_shown), (1, 0));
    }

    #[test]
    fn une_note_se_coche_depuis_le_widget() {
        let lines: Vec<String> = ["Idées", "- [x] Chevrons", "texte", "[ ] Scanout"]
            .iter()
            .map(|line| (*line).to_owned())
            .collect();
        assert_eq!(
            checklist(&lines),
            [(1, true, "Chevrons".to_owned()), (3, false, "Scanout".to_owned())]
        );
        let mut dashboard = Dashboard {
            notes: vec![Note {
                text: lines.join("\n"),
                updated: 0,
            }],
            ..Dashboard::default()
        };
        assert!(dashboard.toggle_check(0, 3, 42));
        assert!(dashboard.toggle_check(0, 1, 42));
        assert!(!dashboard.toggle_check(0, 2, 42));
        assert_eq!(
            dashboard.notes[0].text,
            "Idées\n- [ ] Chevrons\ntexte\n[x] Scanout"
        );
        assert_eq!(dashboard.notes[0].updated, 42);
    }

    #[test]
    fn les_jours_occupes_viennent_du_calendrier() {
        let mut agenda = Agenda::empty();
        let start = Date::new(2026, 11, 9).unwrap();
        agenda.events.push(agenda::Event {
            id: 1,
            title: "Salon".to_owned(),
            description: String::new(),
            location: String::new(),
            category: None,
            start: start.at(9 * 60),
            end: start.add_days(1).at(18 * 60),
            all_day: false,
            recurrence: agenda::Recurrence::default(),
            exceptions: Vec::new(),
            reminders: Vec::new(),
        });
        let now = DateTime::from_stamp(0);
        let id = new_task(&mut agenda, None, "Rapport".to_owned(), now);
        agenda.task_mut(id).unwrap().due = Date::new(2026, 11, 20);
        let done = new_task(&mut agenda, None, "Fait".to_owned(), now);
        agenda.set_done(done, true, now);
        agenda.task_mut(done).unwrap().due = Date::new(2026, 11, 1);
        let dashboard = Dashboard::with_agenda(agenda);
        assert_eq!(dashboard.busy_days(2026, 11), (1 << 8) | (1 << 9) | (1 << 19));
        assert_eq!(dashboard.busy_days(2026, 12), 0);
    }
}
