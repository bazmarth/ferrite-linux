//! Modèle du Calendrier : événements (récurrents ou non, sur plusieurs jours), tâches,
//! listes de tâches, catégories, et les questions qu'on leur pose (occurrences d'une
//! période, recherche, tâches en retard, progression).

use crate::config::Accent;

use super::date::{DAY_MINUTES, Date, DateTime};

/// Heure de référence des rappels d'un événement « toute la journée » ou d'une tâche sans
/// heure : 9 h.
pub const DEFAULT_REMINDER_MINUTE: u32 = 9 * 60;

/// Délais de rappel proposés, en minutes avant le début.
pub const REMINDER_CHOICES: [u32; 10] = [0, 5, 10, 15, 30, 60, 120, 1440, 2880, 10_080];

/// Couleur d'une catégorie ou d'une liste : une des teintes d'accentuation de Ferrite.
pub fn color_key(accent: Accent) -> &'static str {
    accent.key()
}

pub fn accent_from_key(key: &str) -> Accent {
    Accent::ALL
        .into_iter()
        .find(|accent| accent.key() == key)
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: u64,
    pub name: String,
    pub color: Accent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskList {
    pub id: u64,
    pub name: String,
    pub color: Accent,
}

/// Rythme d'une répétition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Frequency {
    #[default]
    Never,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Frequency {
    pub const ALL: [Frequency; 5] = [
        Frequency::Never,
        Frequency::Daily,
        Frequency::Weekly,
        Frequency::Monthly,
        Frequency::Yearly,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Frequency::Never => "jamais",
            Frequency::Daily => "quotidienne",
            Frequency::Weekly => "hebdomadaire",
            Frequency::Monthly => "mensuelle",
            Frequency::Yearly => "annuelle",
        }
    }

    pub fn parse(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|frequency| frequency.key() == key)
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Frequency::Never => "Aucune",
            Frequency::Daily => "Chaque jour",
            Frequency::Weekly => "Chaque semaine",
            Frequency::Monthly => "Chaque mois",
            Frequency::Yearly => "Chaque année",
        }
    }

    /// Unité de l'intervalle : « jours », « semaines »…
    pub fn unit(self, count: u32) -> &'static str {
        let many = count > 1;
        match (self, many) {
            (Frequency::Daily, false) => "jour",
            (Frequency::Daily, true) => "jours",
            (Frequency::Weekly, false) => "semaine",
            (Frequency::Weekly, true) => "semaines",
            (Frequency::Monthly, _) => "mois",
            (Frequency::Yearly, false) => "an",
            (Frequency::Yearly, true) => "ans",
            (Frequency::Never, _) => "",
        }
    }
}

/// Fin d'une répétition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecurrenceEnd {
    #[default]
    Never,
    /// Dernier jour possible (inclus).
    Until(Date),
    /// Nombre total d'occurrences.
    Count(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recurrence {
    pub frequency: Frequency,
    /// Tous les `interval` jours, semaines, mois ou ans (au moins 1).
    pub interval: u32,
    /// Jours de la semaine (bit 0 = lundi) d'une répétition hebdomadaire ; vide : le jour du
    /// début.
    pub weekdays: u8,
    pub end: RecurrenceEnd,
}

impl Default for Recurrence {
    fn default() -> Self {
        Self {
            frequency: Frequency::Never,
            interval: 1,
            weekdays: 0,
            end: RecurrenceEnd::Never,
        }
    }
}

impl Recurrence {
    pub fn repeats(&self) -> bool {
        self.frequency != Frequency::Never
    }

    /// Résumé : « Toutes les 2 semaines, lun., mer., jusqu'au 31 déc. 2026 ».
    pub fn summary(&self) -> String {
        if !self.repeats() {
            return "Ne se répète pas".to_owned();
        }
        let interval = self.interval.max(1);
        let mut text = if interval == 1 {
            self.frequency.label().to_owned()
        } else {
            let every = if self.frequency == Frequency::Weekly {
                "Toutes les"
            } else {
                "Tous les"
            };
            format!("{every} {interval} {}", self.frequency.unit(interval))
        };
        if self.frequency == Frequency::Weekly && self.weekdays != 0 {
            let days: Vec<&str> = (0..7)
                .filter(|day| self.weekdays & (1 << day) != 0)
                .map(|day| super::date::WEEKDAYS_SHORT[day])
                .collect();
            text.push_str(&format!(", {}", days.join(" ")));
        }
        match self.end {
            RecurrenceEnd::Never => {}
            RecurrenceEnd::Until(date) => text.push_str(&format!(", jusqu’au {}", date.medium())),
            RecurrenceEnd::Count(count) => text.push_str(&format!(", {count} fois")),
        }
        text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub location: String,
    pub category: Option<u64>,
    /// Début ; pour un événement « toute la journée », seule la date compte.
    pub start: DateTime,
    /// Fin ; pour un événement « toute la journée », le dernier jour (inclus).
    pub end: DateTime,
    pub all_day: bool,
    pub recurrence: Recurrence,
    /// Occurrences supprimées une à une (jour de leur début).
    pub exceptions: Vec<Date>,
    /// Rappels, en minutes avant le début.
    pub reminders: Vec<u32>,
}

impl Event {
    /// Durée en minutes ; en jours entiers × 1440 pour « toute la journée » (au moins un).
    pub fn duration(&self) -> i64 {
        if self.all_day {
            (self.end.date.days() - self.start.date.days()).max(0) * DAY_MINUTES + DAY_MINUTES
        } else {
            (self.end.stamp() - self.start.stamp()).max(0)
        }
    }

    /// Débuts des occurrences qui touchent la période [`from`, `to`[ (jours), dans l'ordre.
    pub fn occurrence_starts(&self, from: Date, to: Date) -> Vec<Date> {
        // Une occurrence qui commence avant `from` peut encore le couvrir.
        let span_days = (self.duration() - 1).max(0) / DAY_MINUTES + 1;
        let earliest = from.add_days(-span_days);
        let mut found = Vec::new();
        let first = self.start.date;
        let keep = |date: Date, found: &mut Vec<Date>| {
            if date >= earliest && date < to && !self.exceptions.contains(&date) {
                found.push(date);
            }
        };
        let rule = &self.recurrence;
        let interval = i64::from(rule.interval.max(1));
        let until = match rule.end {
            RecurrenceEnd::Until(date) => Some(date),
            _ => None,
        };
        let count = match rule.end {
            RecurrenceEnd::Count(count) => Some(count.max(1)),
            _ => None,
        };
        // Garde-fou : jamais plus de 50 000 candidats parcourus.
        const LIMIT: u32 = 50_000;
        let past = |date: Date| date >= to || until.is_some_and(|until| date > until);
        match rule.frequency {
            Frequency::Never => keep(first, &mut found),
            Frequency::Daily => {
                // Saut direct près de la période quand on ne compte pas les occurrences.
                let mut index = if count.is_none() {
                    ((earliest.days() - first.days()).max(0) / interval) as u32
                } else {
                    0
                };
                loop {
                    let date = first.add_days(i64::from(index) * interval);
                    if past(date) || count.is_some_and(|count| index >= count) || index > LIMIT * 10 {
                        break;
                    }
                    keep(date, &mut found);
                    index += 1;
                }
            }
            Frequency::Weekly => {
                let mask = if rule.weekdays == 0 {
                    1 << first.weekday()
                } else {
                    rule.weekdays
                };
                let monday = first.week_start();
                let mut produced = 0;
                let mut week = if count.is_none() {
                    ((earliest.days() - monday.days()).max(0) / (7 * interval)) as u32
                } else {
                    0
                };
                'weeks: loop {
                    let start = monday.add_days(i64::from(week) * 7 * interval);
                    if past(start) || week > LIMIT {
                        break;
                    }
                    for day in 0..7 {
                        if mask & (1 << day) == 0 {
                            continue;
                        }
                        let date = start.add_days(day);
                        if date < first {
                            continue;
                        }
                        if past(date) || count.is_some_and(|count| produced >= count) {
                            break 'weeks;
                        }
                        produced += 1;
                        keep(date, &mut found);
                    }
                    week += 1;
                }
            }
            Frequency::Monthly | Frequency::Yearly => {
                let step = if rule.frequency == Frequency::Monthly {
                    1
                } else {
                    12
                } * interval as i32;
                let mut produced = 0;
                let mut index = 0;
                loop {
                    let months = index * step;
                    index += 1;
                    if index as u32 > LIMIT {
                        break;
                    }
                    let base = first.first_of_month().add_months(months);
                    // Un 31 n'existe pas tous les mois : ces mois-là sont sautés.
                    let Some(date) = Date::new(base.year, base.month, first.day) else {
                        if base >= to {
                            break;
                        }
                        continue;
                    };
                    if past(date) || count.is_some_and(|count| produced >= count) {
                        break;
                    }
                    produced += 1;
                    keep(date, &mut found);
                }
            }
        }
        found
    }

    /// Occurrences qui touchent les jours [`from`, `to`[.
    pub fn occurrences(&self, from: Date, to: Date) -> Vec<Occurrence> {
        let duration = self.duration();
        self.occurrence_starts(from, to)
            .into_iter()
            .map(|date| {
                let start = date.at(if self.all_day { 0 } else { self.start.minute });
                Occurrence {
                    event: self.id,
                    start,
                    end: start.add_minutes(duration),
                    all_day: self.all_day,
                }
            })
            .filter(|occurrence| occurrence.touches_range(from, to))
            .collect()
    }

    /// Texte où chercher.
    fn matches(&self, query: &str) -> bool {
        [&self.title, &self.description, &self.location]
            .iter()
            .any(|field| field.to_lowercase().contains(query))
    }
}

/// Une occurrence d'un événement : ses vraies dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    pub event: u64,
    pub start: DateTime,
    /// Fin exclue (pour « toute la journée » : minuit après le dernier jour).
    pub end: DateTime,
    pub all_day: bool,
}

impl Occurrence {
    /// Touche le jour `date`.
    pub fn touches(&self, date: Date) -> bool {
        self.touches_range(date, date.add_days(1))
    }

    fn touches_range(&self, from: Date, to: Date) -> bool {
        let (from, to) = (from.at(0).stamp(), to.at(0).stamp());
        let (start, end) = (self.start.stamp(), self.end.stamp());
        if start == end {
            start >= from && start < to
        } else {
            start < to && end > from
        }
    }

    /// Dure plus d'une journée, ou passe minuit : dessiné en bandeau.
    pub fn spans_days(&self) -> bool {
        self.all_day || self.end.add_minutes(-1).date > self.start.date
    }

    /// Dernier jour touché.
    pub fn last_day(&self) -> Date {
        if self.end > self.start {
            self.end.add_minutes(-1).date
        } else {
            self.start.date
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
}

impl Priority {
    pub const ALL: [Priority; 3] = [Priority::Low, Priority::Normal, Priority::High];

    pub fn key(self) -> &'static str {
        match self {
            Priority::Low => "basse",
            Priority::Normal => "normale",
            Priority::High => "haute",
        }
    }

    pub fn parse(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|priority| priority.key() == key)
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Priority::Low => "Basse",
            Priority::Normal => "Normale",
            Priority::High => "Haute",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub notes: String,
    pub list: Option<u64>,
    pub due: Option<Date>,
    /// Heure d'échéance (minutes depuis minuit), si elle compte.
    pub due_time: Option<u32>,
    pub priority: Priority,
    pub done: bool,
    pub completed: Option<DateTime>,
    /// Événement associé.
    pub event: Option<u64>,
    /// Rappels, en minutes avant l'échéance.
    pub reminders: Vec<u32>,
    pub created: DateTime,
}

impl Task {
    /// Moment de l'échéance : son heure, ou la fin de la journée.
    pub fn deadline(&self) -> Option<DateTime> {
        self.due
            .map(|date| date.at(self.due_time.unwrap_or(DAY_MINUTES as u32 - 1)))
    }

    /// Repère des rappels : l'heure d'échéance, ou 9 h le jour dit.
    pub fn reminder_base(&self) -> Option<DateTime> {
        self.due
            .map(|date| date.at(self.due_time.unwrap_or(DEFAULT_REMINDER_MINUTE)))
    }

    pub fn is_overdue(&self, now: DateTime) -> bool {
        !self.done && self.deadline().is_some_and(|deadline| deadline < now)
    }

    fn matches(&self, query: &str) -> bool {
        [&self.title, &self.notes]
            .iter()
            .any(|field| field.to_lowercase().contains(query))
    }
}

/// Avancement : tâches terminées sur le total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

impl Progress {
    pub fn fraction(self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.done as f32 / self.total as f32
        }
    }

    fn of<'a>(tasks: impl Iterator<Item = &'a Task>) -> Self {
        tasks.fold(Self::default(), |progress, task| Self {
            done: progress.done + usize::from(task.done),
            total: progress.total + 1,
        })
    }
}

/// Résultat d'une recherche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    Event(Occurrence),
    Task(u64),
}

/// Tout le contenu du Calendrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agenda {
    pub next_id: u64,
    pub categories: Vec<Category>,
    pub lists: Vec<TaskList>,
    pub events: Vec<Event>,
    pub tasks: Vec<Task>,
}

impl Default for Agenda {
    /// Premier lancement : trois catégories et deux listes, aucun événement.
    fn default() -> Self {
        Self {
            next_id: 6,
            categories: vec![
                Category {
                    id: 1,
                    name: "Personnel".to_owned(),
                    color: Accent::Blue,
                },
                Category {
                    id: 2,
                    name: "Travail".to_owned(),
                    color: Accent::Orange,
                },
                Category {
                    id: 3,
                    name: "Famille".to_owned(),
                    color: Accent::Green,
                },
            ],
            lists: vec![
                TaskList {
                    id: 4,
                    name: "Personnel".to_owned(),
                    color: Accent::Purple,
                },
                TaskList {
                    id: 5,
                    name: "Travail".to_owned(),
                    color: Accent::Orange,
                },
            ],
            events: Vec::new(),
            tasks: Vec::new(),
        }
    }
}

impl Agenda {
    /// Agenda vide, sans catégorie ni liste.
    pub fn empty() -> Self {
        Self {
            next_id: 1,
            categories: Vec::new(),
            lists: Vec::new(),
            events: Vec::new(),
            tasks: Vec::new(),
        }
    }

    pub fn new_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn event(&self, id: u64) -> Option<&Event> {
        self.events.iter().find(|event| event.id == id)
    }

    pub fn event_mut(&mut self, id: u64) -> Option<&mut Event> {
        self.events.iter_mut().find(|event| event.id == id)
    }

    pub fn task(&self, id: u64) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    pub fn task_mut(&mut self, id: u64) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }

    pub fn category(&self, id: Option<u64>) -> Option<&Category> {
        id.and_then(|id| self.categories.iter().find(|category| category.id == id))
    }

    pub fn list(&self, id: Option<u64>) -> Option<&TaskList> {
        id.and_then(|id| self.lists.iter().find(|list| list.id == id))
    }

    /// Ajoute ou remplace l'événement (même identifiant).
    pub fn put_event(&mut self, event: Event) {
        match self.event_mut(event.id) {
            Some(slot) => *slot = event,
            None => self.events.push(event),
        }
    }

    pub fn put_task(&mut self, task: Task) {
        match self.task_mut(task.id) {
            Some(slot) => *slot = task,
            None => self.tasks.push(task),
        }
    }

    /// Supprime l'événement ; les tâches associées le perdent sans disparaître.
    pub fn remove_event(&mut self, id: u64) {
        self.events.retain(|event| event.id != id);
        for task in &mut self.tasks {
            if task.event == Some(id) {
                task.event = None;
            }
        }
    }

    /// Supprime une seule occurrence d'un événement récurrent.
    pub fn remove_occurrence(&mut self, id: u64, date: Date) {
        if let Some(event) = self.event_mut(id) {
            if event.recurrence.repeats() {
                if !event.exceptions.contains(&date) {
                    event.exceptions.push(date);
                    event.exceptions.sort();
                }
            } else {
                self.remove_event(id);
            }
        }
    }

    pub fn remove_task(&mut self, id: u64) {
        self.tasks.retain(|task| task.id != id);
    }

    pub fn remove_category(&mut self, id: u64) {
        self.categories.retain(|category| category.id != id);
        for event in &mut self.events {
            if event.category == Some(id) {
                event.category = None;
            }
        }
    }

    /// Supprime une liste ; ses tâches restent, sans liste.
    pub fn remove_list(&mut self, id: u64) {
        self.lists.retain(|list| list.id != id);
        for task in &mut self.tasks {
            if task.list == Some(id) {
                task.list = None;
            }
        }
    }

    pub fn set_done(&mut self, id: u64, done: bool, now: DateTime) {
        if let Some(task) = self.task_mut(id) {
            task.done = done;
            task.completed = done.then_some(now);
        }
    }

    /// Occurrences des jours [`from`, `to`[, triées : journées entières d'abord, puis par
    /// heure. `visible` écarte les catégories masquées.
    pub fn occurrences(&self, from: Date, to: Date, visible: impl Fn(&Event) -> bool) -> Vec<Occurrence> {
        let mut all: Vec<Occurrence> = self
            .events
            .iter()
            .filter(|event| visible(event))
            .flat_map(|event| event.occurrences(from, to))
            .collect();
        all.sort_by_key(|occurrence| {
            (
                occurrence.start.date,
                !occurrence.all_day,
                occurrence.start,
                occurrence.event,
            )
        });
        all
    }

    /// Tâches à faire dont l'échéance est le jour `date`.
    pub fn tasks_due_on(&self, date: Date) -> impl Iterator<Item = &Task> {
        self.tasks.iter().filter(move |task| task.due == Some(date))
    }

    /// Tâches en retard, la plus ancienne d'abord.
    pub fn overdue_tasks(&self, now: DateTime) -> Vec<&Task> {
        let mut tasks: Vec<&Task> = self.tasks.iter().filter(|task| task.is_overdue(now)).collect();
        tasks.sort_by_key(|task| task.deadline());
        tasks
    }

    /// Tâches à faire des `days` prochains jours (aujourd'hui compris, retard exclu).
    pub fn upcoming_tasks(&self, now: DateTime, days: i64) -> Vec<&Task> {
        let last = now.date.add_days(days);
        let mut tasks: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|task| !task.done && !task.is_overdue(now))
            .filter(|task| task.due.is_some_and(|due| due >= now.date && due < last))
            .collect();
        tasks.sort_by_key(|task| (task.deadline(), std::cmp::Reverse(task.priority)));
        tasks
    }

    /// Tâches liées à un événement.
    pub fn tasks_of_event(&self, id: u64) -> impl Iterator<Item = &Task> {
        self.tasks.iter().filter(move |task| task.event == Some(id))
    }

    /// Avancement d'une liste (toutes si `None`).
    pub fn list_progress(&self, list: Option<u64>) -> Progress {
        Progress::of(
            self.tasks
                .iter()
                .filter(|task| list.is_none() || task.list == list),
        )
    }

    /// Avancement du jour : tâches dues aujourd'hui (ou terminées aujourd'hui).
    pub fn day_progress(&self, today: Date) -> Progress {
        Progress::of(
            self.tasks.iter().filter(|task| {
                task.due == Some(today) || task.completed.is_some_and(|done| done.date == today)
            }),
        )
    }

    /// Recherche dans les titres, descriptions, lieux et notes. Les événements sont cherchés
    /// d'un an en arrière à deux ans en avant de `around`.
    pub fn search(&self, query: &str, around: Date) -> Vec<Found> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let from = around.add_months(-12);
        let to = around.add_months(24);
        let mut found: Vec<Found> = self
            .occurrences(from, to, |event| event.matches(&query))
            .into_iter()
            .map(Found::Event)
            .collect();
        found.extend(
            self.tasks
                .iter()
                .filter(|task| task.matches(&query))
                .map(|task| Found::Task(task.id)),
        );
        found
    }
}

/// Colonnes des événements qui se chevauchent dans une journée : pour chaque intervalle
/// (début, fin en minutes), sa colonne et le nombre de colonnes de son groupe.
pub fn layout_columns(spans: &[(i64, i64)]) -> Vec<(usize, usize)> {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|&index| (spans[index].0, std::cmp::Reverse(spans[index].1)));
    let mut result = vec![(0, 1); spans.len()];
    let mut group: Vec<usize> = Vec::new();
    // Fin de chaque colonne du groupe en cours.
    let mut columns: Vec<i64> = Vec::new();
    let mut group_end = i64::MIN;
    let close = |group: &mut Vec<usize>, columns: &mut Vec<i64>, result: &mut Vec<(usize, usize)>| {
        for &index in group.iter() {
            result[index].1 = columns.len().max(1);
        }
        group.clear();
        columns.clear();
    };
    for index in order {
        let (start, end) = spans[index];
        // Un événement sans durée occupe au moins 15 minutes à l'écran.
        let end = end.max(start + 15);
        if start >= group_end {
            close(&mut group, &mut columns, &mut result);
            group_end = i64::MIN;
        }
        let column = match columns.iter().position(|&last| last <= start) {
            Some(column) => {
                columns[column] = end;
                column
            }
            None => {
                columns.push(end);
                columns.len() - 1
            }
        };
        result[index].0 = column;
        group.push(index);
        group_end = group_end.max(end);
    }
    close(&mut group, &mut columns, &mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::new(y, m, d).unwrap()
    }

    fn event(start: DateTime, end: DateTime) -> Event {
        Event {
            id: 1,
            title: "Réunion".to_owned(),
            description: "Point d’équipe".to_owned(),
            location: "Salle 2".to_owned(),
            category: None,
            start,
            end,
            all_day: false,
            recurrence: Recurrence::default(),
            exceptions: Vec::new(),
            reminders: Vec::new(),
        }
    }

    fn starts(event: &Event, from: Date, to: Date) -> Vec<Date> {
        event.occurrences(from, to).iter().map(|o| o.start.date).collect()
    }

    #[test]
    fn un_evenement_simple_n_apparait_qu_une_fois() {
        let e = event(date(2026, 11, 9).at(540), date(2026, 11, 9).at(600));
        assert_eq!(
            starts(&e, date(2026, 11, 1), date(2026, 12, 1)),
            vec![date(2026, 11, 9)]
        );
        assert!(starts(&e, date(2026, 11, 10), date(2026, 12, 1)).is_empty());
    }

    #[test]
    fn repetition_quotidienne_avec_intervalle_et_nombre() {
        let mut e = event(date(2026, 11, 1).at(540), date(2026, 11, 1).at(600));
        e.recurrence = Recurrence {
            frequency: Frequency::Daily,
            interval: 3,
            weekdays: 0,
            end: RecurrenceEnd::Count(4),
        };
        assert_eq!(
            starts(&e, date(2026, 10, 1), date(2027, 1, 1)),
            vec![
                date(2026, 11, 1),
                date(2026, 11, 4),
                date(2026, 11, 7),
                date(2026, 11, 10)
            ]
        );
        // Sans fin : saut direct vers une période lointaine.
        e.recurrence.end = RecurrenceEnd::Never;
        assert_eq!(
            starts(&e, date(2036, 11, 1), date(2036, 11, 8)).len(),
            2,
            "tous les 3 jours, une semaine en compte 2 ou 3"
        );
    }

    #[test]
    fn repetition_hebdomadaire_sur_plusieurs_jours() {
        // Lundi 2 novembre 2026, lundi et mercredi, jusqu'au 11 novembre, sauf le 4.
        let mut e = event(date(2026, 11, 2).at(540), date(2026, 11, 2).at(600));
        e.recurrence = Recurrence {
            frequency: Frequency::Weekly,
            interval: 1,
            weekdays: 0b101,
            end: RecurrenceEnd::Until(date(2026, 11, 11)),
        };
        e.exceptions.push(date(2026, 11, 4));
        assert_eq!(
            starts(&e, date(2026, 10, 1), date(2026, 12, 31)),
            vec![date(2026, 11, 2), date(2026, 11, 9), date(2026, 11, 11)]
        );
        // Toutes les deux semaines, le jour du début, 3 fois.
        e.exceptions.clear();
        e.recurrence = Recurrence {
            frequency: Frequency::Weekly,
            interval: 2,
            weekdays: 0,
            end: RecurrenceEnd::Count(3),
        };
        assert_eq!(
            starts(&e, date(2026, 10, 1), date(2027, 12, 31)),
            vec![date(2026, 11, 2), date(2026, 11, 16), date(2026, 11, 30)]
        );
    }

    #[test]
    fn repetition_mensuelle_saute_les_mois_trop_courts() {
        let mut e = event(date(2026, 1, 31).at(600), date(2026, 1, 31).at(660));
        e.recurrence.frequency = Frequency::Monthly;
        assert_eq!(
            starts(&e, date(2026, 1, 1), date(2026, 6, 1)),
            vec![date(2026, 1, 31), date(2026, 3, 31), date(2026, 5, 31)]
        );
        let mut birthday = event(date(2024, 2, 29).at(0), date(2024, 2, 29).at(0));
        birthday.all_day = true;
        birthday.recurrence.frequency = Frequency::Yearly;
        assert_eq!(
            starts(&birthday, date(2024, 1, 1), date(2029, 1, 1)),
            vec![date(2024, 2, 29), date(2028, 2, 29)]
        );
    }

    #[test]
    fn un_evenement_sur_plusieurs_jours_touche_chaque_jour() {
        let mut e = event(date(2026, 11, 9).at(0), date(2026, 11, 11).at(0));
        e.all_day = true;
        let found = e.occurrences(date(2026, 11, 10), date(2026, 11, 11));
        assert_eq!(found.len(), 1, "commencé la veille, il couvre encore le 10");
        let occurrence = found[0];
        assert!(occurrence.spans_days());
        assert!(occurrence.touches(date(2026, 11, 11)));
        assert!(!occurrence.touches(date(2026, 11, 12)));
        assert_eq!(occurrence.last_day(), date(2026, 11, 11));
        // Une soirée qui passe minuit touche aussi le lendemain.
        let night = event(date(2026, 11, 9).at(22 * 60), date(2026, 11, 10).at(120));
        let found = night.occurrences(date(2026, 11, 10), date(2026, 11, 11));
        assert_eq!(found.len(), 1);
        assert!(found[0].spans_days());
    }

    #[test]
    fn taches_en_retard_a_venir_et_progression() {
        let now = date(2026, 11, 8).at(12 * 60);
        let mut agenda = Agenda::default();
        let mut task = |title: &str, due: Option<Date>, time: Option<u32>, done: bool| {
            let id = agenda.new_id();
            agenda.put_task(Task {
                id,
                title: title.to_owned(),
                notes: String::new(),
                list: Some(4),
                due,
                due_time: time,
                priority: Priority::Normal,
                done,
                completed: done.then_some(now),
                event: None,
                reminders: Vec::new(),
                created: now,
            });
            id
        };
        let late = task("Hier", Some(date(2026, 11, 7)), None, false);
        let morning = task("Ce matin", Some(date(2026, 11, 8)), Some(9 * 60), false);
        let tonight = task("Ce soir", Some(date(2026, 11, 8)), None, false);
        let _done = task("Fait", Some(date(2026, 11, 7)), None, true);
        let soon = task("Mardi", Some(date(2026, 11, 10)), None, false);
        let overdue: Vec<u64> = agenda.overdue_tasks(now).iter().map(|t| t.id).collect();
        assert_eq!(overdue, vec![late, morning]);
        let upcoming: Vec<u64> = agenda.upcoming_tasks(now, 7).iter().map(|t| t.id).collect();
        assert_eq!(upcoming, vec![tonight, soon]);
        assert_eq!(agenda.list_progress(Some(4)), Progress { done: 1, total: 5 });
        assert_eq!(agenda.day_progress(now.date), Progress { done: 1, total: 3 });
        agenda.set_done(tonight, true, now);
        assert_eq!(agenda.day_progress(now.date).done, 2);
        agenda.remove_list(4);
        assert!(agenda.tasks.iter().all(|task| task.list.is_none()));
    }

    #[test]
    fn recherche_et_suppression_d_occurrence() {
        let mut agenda = Agenda::empty();
        let mut e = event(date(2026, 11, 2).at(540), date(2026, 11, 2).at(600));
        e.recurrence.frequency = Frequency::Weekly;
        agenda.put_event(e);
        let found = agenda.search("salle", date(2026, 11, 8));
        assert!(
            found.len() > 50,
            "un an avant et deux ans après : {}",
            found.len()
        );
        assert!(agenda.search("absent", date(2026, 11, 8)).is_empty());
        agenda.remove_occurrence(1, date(2026, 11, 9));
        let week = agenda.occurrences(date(2026, 11, 9), date(2026, 11, 16), |_| true);
        assert!(week.is_empty());
        agenda.remove_occurrence(1, date(2026, 11, 9));
        assert_eq!(agenda.event(1).unwrap().exceptions.len(), 1);
    }

    #[test]
    fn colonnes_des_chevauchements() {
        // 9 h-10 h et 9 h 30-11 h se chevauchent ; 11 h-12 h est seul.
        let spans = [(540, 600), (570, 660), (660, 720)];
        assert_eq!(layout_columns(&spans), vec![(0, 2), (1, 2), (0, 1)]);
        // Trois en cascade : la troisième reprend la première colonne.
        let spans = [(0, 120), (60, 180), (130, 200)];
        assert_eq!(layout_columns(&spans), vec![(0, 2), (1, 2), (0, 2)]);
    }

    #[test]
    fn resume_des_repetitions() {
        let rule = Recurrence {
            frequency: Frequency::Weekly,
            interval: 2,
            weekdays: 0b101,
            end: RecurrenceEnd::Count(5),
        };
        assert_eq!(rule.summary(), "Toutes les 2 semaines, lun. mer., 5 fois");
        assert_eq!(Recurrence::default().summary(), "Ne se répète pas");
    }
}
