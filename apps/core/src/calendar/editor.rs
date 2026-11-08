//! Dialogues du Calendrier : événement, tâche, catégorie ou liste. Ils travaillent sur une
//! copie et rendent un résultat que `CalendarState` applique à l'agenda.

use eframe::egui::{self, Frame, Margin, RichText, Stroke};
use ferrite_system::config::Accent;

use super::{
    date::{DAY_MINUTES, Date, DateTime, WEEKDAYS_SHORT, duration_label},
    model::{
        Agenda, Event, Frequency, Occurrence, Priority, REMINDER_CHOICES, Recurrence, RecurrenceEnd, Task,
    },
    widgets::{self, DateInput, TimeInput},
};
use crate::{kit, style};

/// Largeur des dialogues.
const WIDTH: f32 = 470.0;
/// Largeur de la colonne des libellés.
const LABEL: f32 = 92.0;

/// Portée d'une modification d'un événement récurrent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Toute la série (ou l'événement unique).
    All,
    /// L'occurrence de ce jour seulement.
    Only(Date),
}

/// Ce que le dialogue demande à l'application.
#[derive(Debug, Clone)]
pub enum Outcome {
    Close,
    SaveEvent(Box<Event>, Scope),
    DeleteEvent(u64, Scope),
    SaveTask(Box<Task>),
    DeleteTask(u64),
    /// Nouvelle tâche liée à cet événement (enregistré d'abord).
    LinkedTask(Box<Event>),
    SaveCategory(u64, String, Accent),
    DeleteCategory(u64),
    SaveList(u64, String, Accent),
    DeleteList(u64),
}

/// Cadre commun des dialogues.
fn dialog<R>(ctx: &egui::Context, id: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> (R, bool) {
    let frame = Frame::new()
        .fill(style::background())
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(16)
        .inner_margin(Margin::symmetric(22, 18))
        .shadow(egui::Shadow {
            offset: [0, 18],
            blur: 60,
            spread: 0,
            color: style::shadow(),
        });
    let response = egui::Modal::new(egui::Id::unique(id))
        .frame(frame)
        .backdrop_color(style::scrim())
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            add(ui)
        });
    let close = response.should_close();
    (response.inner, close)
}

fn title_line(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(18.0).family(style::strong_family()));
    ui.add_space(12.0);
}

/// Ligne « libellé : contenu ».
fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        widgets::field_label(ui, label, LABEL);
        add(ui);
    });
    ui.add_space(6.0);
}

/// Liste des rappels : pastilles à retirer et menu pour en ajouter.
fn reminders_editor(ui: &mut egui::Ui, id: &str, reminders: &mut Vec<u32>, all_day: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        let mut removed = None;
        for (index, minutes) in reminders.iter().enumerate() {
            let text = format!("{}  ×", reminder_label(*minutes, all_day));
            if kit::button(ui, &text, false)
                .on_hover_text("Retirer ce rappel")
                .clicked()
            {
                removed = Some(index);
            }
        }
        if let Some(index) = removed {
            reminders.remove(index);
        }
        let choices: Vec<u32> = REMINDER_CHOICES
            .into_iter()
            .filter(|choice| !reminders.contains(choice))
            .collect();
        if !choices.is_empty() {
            egui::ComboBox::from_id_salt(("ajout-rappel", id))
                .selected_text(if reminders.is_empty() {
                    "Ajouter un rappel"
                } else {
                    "Ajouter"
                })
                .width(150.0)
                .show_ui(ui, |ui| {
                    for choice in choices {
                        if ui
                            .selectable_label(false, reminder_label(choice, all_day))
                            .clicked()
                        {
                            reminders.push(choice);
                            reminders.sort_unstable();
                        }
                    }
                });
        }
    });
}

/// « 15 min avant », « 1 jour avant », « À l'heure de début » (« le jour même à 9 h »
/// pour une journée entière).
pub fn reminder_label(minutes: u32, all_day: bool) -> String {
    match (minutes, all_day) {
        (0, true) => "Le jour même à 9 h".to_owned(),
        (0, false) => "À l’heure de début".to_owned(),
        (minutes, _) => format!("{} avant", duration_label(minutes)),
    }
}

/// Dialogue d'un événement.
#[derive(Debug, Clone)]
pub struct EventEditor {
    id: u64,
    is_new: bool,
    /// Occurrence ouverte d'un événement récurrent.
    occurrence: Option<Date>,
    /// Début de la série telle qu'enregistrée.
    series_start: DateTime,
    title: String,
    description: String,
    location: String,
    category: Option<u64>,
    all_day: bool,
    start_date: DateInput,
    start_time: TimeInput,
    end_date: DateInput,
    end_time: TimeInput,
    recurrence: Recurrence,
    until: DateInput,
    count: u32,
    exceptions: Vec<Date>,
    reminders: Vec<u32>,
    /// Question en cours sur un événement récurrent : enregistrer ou supprimer.
    asking: Option<Asking>,
    confirm_delete: bool,
    focus_title: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asking {
    Save,
    Delete,
}

impl EventEditor {
    /// Nouvel événement d'une heure (ou d'une journée).
    pub fn create(id: u64, start: DateTime, all_day: bool, category: Option<u64>) -> Self {
        let end = if all_day { start } else { start.add_minutes(60) };
        let event = Event {
            id,
            title: String::new(),
            description: String::new(),
            location: String::new(),
            category,
            start,
            end,
            all_day,
            recurrence: Recurrence::default(),
            exceptions: Vec::new(),
            reminders: if all_day { Vec::new() } else { vec![15] },
        };
        let mut editor = Self::from_event(&event, None);
        editor.is_new = true;
        editor
    }

    /// Événement existant ; `occurrence` : occurrence cliquée (montrée à ses dates).
    pub fn open(event: &Event, occurrence: Option<Occurrence>) -> Self {
        Self::from_event(event, occurrence)
    }

    fn from_event(event: &Event, occurrence: Option<Occurrence>) -> Self {
        let (start, end) = match occurrence {
            Some(occurrence) if event.recurrence.repeats() => {
                let end = if event.all_day {
                    occurrence.last_day().at(0)
                } else {
                    occurrence.end
                };
                (occurrence.start, end)
            }
            _ => (event.start, event.end),
        };
        let (until, count) = match event.recurrence.end {
            RecurrenceEnd::Until(date) => (date, 10),
            RecurrenceEnd::Count(count) => (start.date.add_months(3), count),
            RecurrenceEnd::Never => (start.date.add_months(3), 10),
        };
        Self {
            id: event.id,
            is_new: false,
            occurrence: occurrence
                .filter(|_| event.recurrence.repeats())
                .map(|occurrence| occurrence.start.date),
            series_start: event.start,
            title: event.title.clone(),
            description: event.description.clone(),
            location: event.location.clone(),
            category: event.category,
            all_day: event.all_day,
            start_date: DateInput::new(start.date),
            start_time: TimeInput::new(start.minute),
            end_date: DateInput::new(end.date),
            end_time: TimeInput::new(end.minute),
            recurrence: event.recurrence,
            until: DateInput::new(until),
            count,
            exceptions: event.exceptions.clone(),
            reminders: event.reminders.clone(),
            asking: None,
            confirm_delete: false,
            focus_title: true,
        }
    }

    fn start(&self) -> DateTime {
        self.start_date
            .value
            .at(if self.all_day { 0 } else { self.start_time.value })
    }

    fn end(&self) -> DateTime {
        self.end_date
            .value
            .at(if self.all_day { 0 } else { self.end_time.value })
    }

    /// Problème qui empêche d'enregistrer.
    fn problem(&self, today: Date) -> Option<&'static str> {
        if self.title.trim().is_empty() {
            return Some("Donnez un titre à l’événement.");
        }
        if !self.start_date.valid(today) || !self.end_date.valid(today) {
            return Some("Une date est illisible.");
        }
        if !self.all_day && (!self.start_time.valid() || !self.end_time.valid()) {
            return Some("Une heure est illisible.");
        }
        if self.end() < self.start() {
            return Some("La fin précède le début.");
        }
        if self.recurrence.repeats()
            && matches!(self.recurrence.end, RecurrenceEnd::Until(_))
            && self.until.value < self.start_date.value
        {
            return Some("La répétition s’arrête avant le début.");
        }
        None
    }

    /// Événement tel que saisi, pour la portée donnée.
    fn build(&self, scope: Scope) -> Event {
        let mut recurrence = self.recurrence;
        recurrence.end = match recurrence.end {
            RecurrenceEnd::Never => RecurrenceEnd::Never,
            RecurrenceEnd::Until(_) => RecurrenceEnd::Until(self.until.value),
            RecurrenceEnd::Count(_) => RecurrenceEnd::Count(self.count.max(1)),
        };
        let (start, end) = (self.start(), self.end());
        let (start, end, recurrence) = match (scope, self.occurrence) {
            (Scope::Only(_), _) => (start, end, Recurrence::default()),
            // Toute la série depuis une occurrence : le décalage saisi s'applique au début
            // de la série.
            (Scope::All, Some(occurrence)) => {
                let shift = start.date.days() - occurrence.days();
                let first = self.series_start.date.add_days(shift).at(start.minute);
                (first, first.add_minutes(end.stamp() - start.stamp()), recurrence)
            }
            (Scope::All, None) => (start, end, recurrence),
        };
        Event {
            id: self.id,
            title: self.title.trim().to_owned(),
            description: self.description.trim_end().to_owned(),
            location: self.location.trim().to_owned(),
            category: self.category,
            start,
            end,
            all_day: self.all_day,
            recurrence,
            exceptions: if recurrence.repeats() {
                self.exceptions.clone()
            } else {
                Vec::new()
            },
            reminders: self.reminders.clone(),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, agenda: &Agenda, today: Date) -> Option<Outcome> {
        let max_height = (ctx.content_rect().height() - 150.0).max(240.0);
        let (outcome, close) = dialog(ctx, "calendrier-evenement", |ui| {
            title_line(
                ui,
                if self.is_new {
                    "Nouvel événement"
                } else {
                    "Événement"
                },
            );
            egui::ScrollArea::vertical()
                .max_height(max_height)
                .auto_shrink([false, true])
                .show(ui, |ui| self.fields(ui, agenda, today));
            ui.add_space(12.0);
            if let Some(asking) = self.asking {
                self.ask_scope(ui, asking)
            } else {
                self.buttons(ui, agenda, today)
            }
        });
        if close && outcome.is_none() {
            return Some(Outcome::Close);
        }
        outcome
    }

    fn fields(&mut self, ui: &mut egui::Ui, agenda: &Agenda, today: Date) {
        let title = ui.add(
            egui::TextEdit::singleline(&mut self.title)
                .hint_text("Titre")
                .font(egui::FontId::proportional(16.0))
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(10, 8)),
        );
        if self.focus_title {
            title.request_focus();
            self.focus_title = false;
        }
        ui.add_space(8.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.location)
                .hint_text("Lieu")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(12.0);
        row(ui, "Journée", |ui| {
            if kit::switch(ui, &mut self.all_day) && !self.all_day && self.start_time.value == 0 {
                self.start_time.set(9 * 60);
                self.end_time.set(10 * 60);
            }
        });
        let before = self.start();
        row(ui, "Début", |ui| {
            let mut moved = self.start_date.show(ui, "debut", today);
            if !self.all_day {
                moved |= self.start_time.show(ui, "debut");
            }
            // La fin suit le début : même durée.
            if moved {
                let duration = self.end().stamp() - before.stamp();
                let end = self.start().add_minutes(duration.max(0));
                self.end_date.set(end.date);
                self.end_time.set(end.minute);
            }
        });
        row(ui, "Fin", |ui| {
            self.end_date.show(ui, "fin", today);
            if !self.all_day {
                self.end_time.show(ui, "fin");
            }
            let length = self.end().stamp() - self.start().stamp();
            if length > 0 && !self.all_day {
                kit::note(ui, &duration_label(length as u32));
            } else if self.all_day && length >= 0 {
                let days = (length / DAY_MINUTES + 1) as u32;
                kit::note(ui, &super::date::plural(days, "jour", "jours"));
            }
        });
        row(ui, "Catégorie", |ui| {
            let current = agenda.category(self.category);
            let text = current.map_or("Aucune".to_owned(), |category| category.name.clone());
            if let Some(category) = current {
                widgets::dot(ui, widgets::tint(Some(category.color)), 10.0);
            }
            egui::ComboBox::from_id_salt("categorie-evenement")
                .selected_text(text)
                .width(180.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(self.category.is_none(), "Aucune").clicked() {
                        self.category = None;
                    }
                    for category in &agenda.categories {
                        let selected = self.category == Some(category.id);
                        let response = ui.horizontal(|ui| {
                            widgets::dot(ui, widgets::tint(Some(category.color)), 10.0);
                            ui.selectable_label(selected, &category.name)
                        });
                        if response.inner.clicked() {
                            self.category = Some(category.id);
                        }
                    }
                });
        });
        self.recurrence_fields(ui, today);
        row(ui, "Rappels", |ui| {
            ui.vertical(|ui| reminders_editor(ui, "evenement", &mut self.reminders, self.all_day));
        });
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::multiline(&mut self.description)
                .hint_text("Description")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        if !self.is_new {
            let linked: Vec<&Task> = agenda.tasks_of_event(self.id).collect();
            if !linked.is_empty() {
                ui.add_space(10.0);
                kit::note(ui, "Tâches associées");
                for task in linked {
                    ui.horizontal(|ui| {
                        let color = widgets::tint(agenda.list(task.list).map(|list| list.color));
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        widgets::paint_check(ui.painter(), rect.center(), 7.0, task.done, color, false);
                        ui.label(&task.title);
                    });
                }
            }
        }
    }

    fn recurrence_fields(&mut self, ui: &mut egui::Ui, today: Date) {
        row(ui, "Répétition", |ui| {
            egui::ComboBox::from_id_salt("repetition")
                .selected_text(self.recurrence.frequency.label())
                .width(180.0)
                .show_ui(ui, |ui| {
                    for frequency in Frequency::ALL {
                        if ui
                            .selectable_label(self.recurrence.frequency == frequency, frequency.label())
                            .clicked()
                        {
                            self.recurrence.frequency = frequency;
                        }
                    }
                });
        });
        if !self.recurrence.repeats() {
            return;
        }
        row(ui, "Tous les", |ui| {
            ui.add(
                egui::DragValue::new(&mut self.recurrence.interval)
                    .range(1..=99)
                    .speed(0.1),
            );
            ui.label(self.recurrence.frequency.unit(self.recurrence.interval));
        });
        if self.recurrence.frequency == Frequency::Weekly {
            row(ui, "Jours", |ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                // Aucun jour coché : le jour du début.
                let mask = if self.recurrence.weekdays == 0 {
                    1 << self.start_date.value.weekday()
                } else {
                    self.recurrence.weekdays
                };
                for (day, name) in WEEKDAYS_SHORT.iter().enumerate() {
                    let on = mask & (1 << day) != 0;
                    let text = name.trim_end_matches('.');
                    if kit::button(ui, text, on).clicked() {
                        let next = mask ^ (1 << day);
                        // Au moins un jour.
                        if next != 0 {
                            self.recurrence.weekdays = next;
                        }
                    }
                }
            });
        }
        row(ui, "Fin", |ui| {
            let labels = ["Jamais", "Le", "Après"];
            let selected = match self.recurrence.end {
                RecurrenceEnd::Never => 0,
                RecurrenceEnd::Until(_) => 1,
                RecurrenceEnd::Count(_) => 2,
            };
            if let Some(choice) = kit::segments(ui, &labels, selected) {
                self.recurrence.end = match choice {
                    0 => RecurrenceEnd::Never,
                    1 => RecurrenceEnd::Until(self.until.value),
                    _ => RecurrenceEnd::Count(self.count),
                };
            }
        });
        match self.recurrence.end {
            RecurrenceEnd::Until(_) => row(ui, "", |ui| {
                self.until.show(ui, "jusqua", today);
            }),
            RecurrenceEnd::Count(_) => row(ui, "", |ui| {
                ui.add(egui::DragValue::new(&mut self.count).range(1..=999).speed(0.2));
                ui.label("fois");
            }),
            RecurrenceEnd::Never => {}
        }
        let mut preview = self.recurrence;
        preview.end = match preview.end {
            RecurrenceEnd::Until(_) => RecurrenceEnd::Until(self.until.value),
            RecurrenceEnd::Count(_) => RecurrenceEnd::Count(self.count),
            RecurrenceEnd::Never => RecurrenceEnd::Never,
        };
        row(ui, "", |ui| kit::note(ui, &preview.summary()));
    }

    fn buttons(&mut self, ui: &mut egui::Ui, agenda: &Agenda, today: Date) -> Option<Outcome> {
        let problem = self.problem(today);
        let mut outcome = None;
        ui.horizontal(|ui| {
            if !self.is_new {
                if self.confirm_delete {
                    if kit::wide_button(ui, 150.0, "Confirmer", kit::Tone::Danger, true).clicked() {
                        if self.occurrence.is_some() {
                            self.asking = Some(Asking::Delete);
                        } else {
                            outcome = Some(Outcome::DeleteEvent(self.id, Scope::All));
                        }
                    }
                } else if kit::button(ui, "Supprimer", false).clicked() {
                    self.confirm_delete = true;
                }
                if problem.is_none()
                    && kit::button(ui, "Tâche liée", false)
                        .on_hover_text("Enregistrer et créer une tâche associée à cet événement")
                        .clicked()
                {
                    outcome = Some(Outcome::LinkedTask(Box::new(self.build(Scope::All))));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let save = kit::wide_button(ui, 120.0, "Enregistrer", kit::Tone::Accent, problem.is_none());
                if save.clicked()
                    || (problem.is_none()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter) && input.modifiers.command))
                {
                    if self.occurrence.is_some() {
                        self.asking = Some(Asking::Save);
                    } else {
                        outcome = Some(Outcome::SaveEvent(Box::new(self.build(Scope::All)), Scope::All));
                    }
                }
                if kit::wide_button(ui, 100.0, "Annuler", kit::Tone::Plain, true).clicked() {
                    outcome = Some(Outcome::Close);
                }
                if let Some(problem) = problem {
                    kit::note(ui, problem);
                }
            });
        });
        let _ = agenda;
        outcome
    }

    /// « Cette occurrence » ou « Toute la série ».
    fn ask_scope(&mut self, ui: &mut egui::Ui, asking: Asking) -> Option<Outcome> {
        let occurrence = self.occurrence?;
        let mut outcome = None;
        let question = match asking {
            Asking::Save => "Modifier cet événement récurrent :",
            Asking::Delete => "Supprimer cet événement récurrent :",
        };
        ui.label(RichText::new(question).size(14.0));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if kit::wide_button(ui, 150.0, "Cette occurrence", kit::Tone::Plain, true).clicked() {
                let scope = Scope::Only(occurrence);
                outcome = Some(match asking {
                    Asking::Save => Outcome::SaveEvent(Box::new(self.build(scope)), scope),
                    Asking::Delete => Outcome::DeleteEvent(self.id, scope),
                });
            }
            let tone = if asking == Asking::Delete {
                kit::Tone::Danger
            } else {
                kit::Tone::Accent
            };
            if kit::wide_button(ui, 150.0, "Toute la série", tone, true).clicked() {
                outcome = Some(match asking {
                    Asking::Save => Outcome::SaveEvent(Box::new(self.build(Scope::All)), Scope::All),
                    Asking::Delete => Outcome::DeleteEvent(self.id, Scope::All),
                });
            }
            if kit::button(ui, "Annuler", false).clicked() {
                self.asking = None;
                self.confirm_delete = false;
            }
        });
        outcome
    }
}

/// Dialogue d'une tâche.
#[derive(Debug, Clone)]
pub struct TaskEditor {
    task: Task,
    is_new: bool,
    has_due: bool,
    has_time: bool,
    due: DateInput,
    time: TimeInput,
    confirm_delete: bool,
    focus_title: bool,
}

impl TaskEditor {
    pub fn create(task: Task) -> Self {
        let mut editor = Self::open(&task);
        editor.is_new = true;
        editor
    }

    pub fn open(task: &Task) -> Self {
        let due = task.due.unwrap_or(task.created.date);
        Self {
            task: task.clone(),
            is_new: false,
            has_due: task.due.is_some(),
            has_time: task.due_time.is_some(),
            due: DateInput::new(due),
            time: TimeInput::new(task.due_time.unwrap_or(18 * 60)),
            confirm_delete: false,
            focus_title: true,
        }
    }

    fn problem(&self, today: Date) -> Option<&'static str> {
        if self.task.title.trim().is_empty() {
            Some("Donnez un titre à la tâche.")
        } else if self.has_due && !self.due.valid(today) {
            Some("La date d’échéance est illisible.")
        } else if self.has_due && self.has_time && !self.time.valid() {
            Some("L’heure d’échéance est illisible.")
        } else {
            None
        }
    }

    fn build(&self, now: DateTime) -> Task {
        let mut task = self.task.clone();
        task.title = task.title.trim().to_owned();
        task.notes = task.notes.trim_end().to_owned();
        task.due = self.has_due.then_some(self.due.value);
        task.due_time = (self.has_due && self.has_time).then_some(self.time.value);
        if !self.has_due {
            task.reminders.clear();
        }
        if task.done && task.completed.is_none() {
            task.completed = Some(now);
        } else if !task.done {
            task.completed = None;
        }
        task
    }

    pub fn show(&mut self, ctx: &egui::Context, agenda: &Agenda, now: DateTime) -> Option<Outcome> {
        let today = now.date;
        let (outcome, close) = dialog(ctx, "calendrier-tache", |ui| {
            title_line(ui, if self.is_new { "Nouvelle tâche" } else { "Tâche" });
            let title = ui.add(
                egui::TextEdit::singleline(&mut self.task.title)
                    .hint_text("Titre")
                    .font(egui::FontId::proportional(16.0))
                    .desired_width(f32::INFINITY)
                    .margin(Margin::symmetric(10, 8)),
            );
            if self.focus_title {
                title.request_focus();
                self.focus_title = false;
            }
            ui.add_space(12.0);
            row(ui, "Liste", |ui| {
                let current = agenda.list(self.task.list);
                if let Some(list) = current {
                    widgets::dot(ui, widgets::tint(Some(list.color)), 10.0);
                }
                egui::ComboBox::from_id_salt("liste-tache")
                    .selected_text(current.map_or("Aucune".to_owned(), |list| list.name.clone()))
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(self.task.list.is_none(), "Aucune").clicked() {
                            self.task.list = None;
                        }
                        for list in &agenda.lists {
                            if ui
                                .selectable_label(self.task.list == Some(list.id), &list.name)
                                .clicked()
                            {
                                self.task.list = Some(list.id);
                            }
                        }
                    });
            });
            row(ui, "Priorité", |ui| {
                let labels: Vec<&str> = Priority::ALL.iter().map(|priority| priority.label()).collect();
                let selected = Priority::ALL
                    .iter()
                    .position(|priority| *priority == self.task.priority)
                    .unwrap_or(1);
                if let Some(choice) = kit::segments(ui, &labels, selected) {
                    self.task.priority = Priority::ALL[choice];
                }
            });
            row(ui, "Échéance", |ui| {
                kit::switch(ui, &mut self.has_due);
                if self.has_due {
                    self.due.show(ui, "echeance", today);
                }
            });
            if self.has_due {
                row(ui, "Heure", |ui| {
                    kit::switch(ui, &mut self.has_time);
                    if self.has_time {
                        self.time.show(ui, "echeance");
                    } else {
                        kit::note(ui, "Dans la journée");
                    }
                });
                row(ui, "Rappels", |ui| {
                    ui.vertical(|ui| reminders_editor(ui, "tache", &mut self.task.reminders, !self.has_time));
                });
            }
            row(ui, "Événement", |ui| {
                let current = self.task.event.and_then(|id| agenda.event(id));
                egui::ComboBox::from_id_salt("evenement-tache")
                    .selected_text(current.map_or("Aucun".to_owned(), |event| event.title.clone()))
                    .width(250.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(self.task.event.is_none(), "Aucun").clicked() {
                            self.task.event = None;
                        }
                        // Les événements à venir d'abord, puis les passés.
                        let mut events: Vec<&Event> = agenda.events.iter().collect();
                        events.sort_by_key(|event| {
                            (
                                event.start.date < today && !event.recurrence.repeats(),
                                event.start,
                            )
                        });
                        for event in events.into_iter().take(200) {
                            let text = format!("{} · {}", event.title, event.start.date.medium());
                            if ui
                                .selectable_label(self.task.event == Some(event.id), text)
                                .clicked()
                            {
                                self.task.event = Some(event.id);
                            }
                        }
                    });
            });
            row(ui, "Terminée", |ui| {
                kit::switch(ui, &mut self.task.done);
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.task.notes)
                    .hint_text("Notes")
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(14.0);
            let problem = self.problem(today);
            let mut outcome = None;
            ui.horizontal(|ui| {
                if !self.is_new {
                    if self.confirm_delete {
                        if kit::wide_button(ui, 150.0, "Confirmer", kit::Tone::Danger, true).clicked() {
                            outcome = Some(Outcome::DeleteTask(self.task.id));
                        }
                    } else if kit::button(ui, "Supprimer", false).clicked() {
                        self.confirm_delete = true;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save =
                        kit::wide_button(ui, 120.0, "Enregistrer", kit::Tone::Accent, problem.is_none());
                    let enter = problem.is_none()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter) && input.modifiers.command);
                    if save.clicked() || enter {
                        outcome = Some(Outcome::SaveTask(Box::new(self.build(now))));
                    }
                    if kit::wide_button(ui, 100.0, "Annuler", kit::Tone::Plain, true).clicked() {
                        outcome = Some(Outcome::Close);
                    }
                    if let Some(problem) = problem {
                        kit::note(ui, problem);
                    }
                });
            });
            outcome
        });
        if close && outcome.is_none() {
            return Some(Outcome::Close);
        }
        outcome
    }
}

/// Ce que gère le petit dialogue de nom et couleur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collection {
    Category,
    List,
}

/// Dialogue d'une catégorie ou d'une liste : nom et couleur.
#[derive(Debug, Clone)]
pub struct CollectionEditor {
    pub kind: Collection,
    id: u64,
    is_new: bool,
    name: String,
    color: Accent,
    confirm_delete: bool,
}

impl CollectionEditor {
    pub fn new(kind: Collection, id: u64, is_new: bool, name: &str, color: Accent) -> Self {
        Self {
            kind,
            id,
            is_new,
            name: name.to_owned(),
            color,
            confirm_delete: false,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let (outcome, close) = dialog(ctx, "calendrier-collection", |ui| {
            let title = match (self.kind, self.is_new) {
                (Collection::Category, true) => "Nouvelle catégorie",
                (Collection::Category, false) => "Catégorie",
                (Collection::List, true) => "Nouvelle liste",
                (Collection::List, false) => "Liste de tâches",
            };
            title_line(ui, title);
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.name)
                    .hint_text("Nom")
                    .desired_width(f32::INFINITY)
                    .margin(Margin::symmetric(10, 8)),
            );
            if self.is_new && self.name.is_empty() {
                field.request_focus();
            }
            ui.add_space(10.0);
            widgets::color_picker(ui, &mut self.color);
            ui.add_space(16.0);
            let valid = !self.name.trim().is_empty();
            let mut outcome = None;
            ui.horizontal(|ui| {
                if !self.is_new {
                    if self.confirm_delete {
                        if kit::wide_button(ui, 150.0, "Confirmer", kit::Tone::Danger, true)
                            .on_hover_text(match self.kind {
                                Collection::Category => "Les événements restent, sans catégorie",
                                Collection::List => "Les tâches restent, sans liste",
                            })
                            .clicked()
                        {
                            outcome = Some(match self.kind {
                                Collection::Category => Outcome::DeleteCategory(self.id),
                                Collection::List => Outcome::DeleteList(self.id),
                            });
                        }
                    } else if kit::button(ui, "Supprimer", false).clicked() {
                        self.confirm_delete = true;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = kit::wide_button(ui, 120.0, "Enregistrer", kit::Tone::Accent, valid);
                    if save.clicked()
                        || (valid
                            && field.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                    {
                        let name = self.name.trim().to_owned();
                        outcome = Some(match self.kind {
                            Collection::Category => Outcome::SaveCategory(self.id, name, self.color),
                            Collection::List => Outcome::SaveList(self.id, name, self.color),
                        });
                    }
                    if kit::wide_button(ui, 100.0, "Annuler", kit::Tone::Plain, true).clicked() {
                        outcome = Some(Outcome::Close);
                    }
                });
            });
            outcome
        });
        if close && outcome.is_none() {
            return Some(Outcome::Close);
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::new(y, m, d).unwrap()
    }

    fn weekly() -> Event {
        let start = date(2026, 11, 2).at(9 * 60);
        Event {
            id: 3,
            title: "Sport".to_owned(),
            description: String::new(),
            location: String::new(),
            category: None,
            start,
            end: start.add_minutes(60),
            all_day: false,
            recurrence: Recurrence {
                frequency: Frequency::Weekly,
                ..Recurrence::default()
            },
            exceptions: Vec::new(),
            reminders: vec![10],
        }
    }

    #[test]
    fn modifier_une_occurrence_ou_toute_la_serie() {
        let event = weekly();
        let occurrence = event.occurrences(date(2026, 11, 16), date(2026, 11, 17))[0];
        let mut editor = EventEditor::open(&event, Some(occurrence));
        assert_eq!(
            editor.start(),
            date(2026, 11, 16).at(9 * 60),
            "montrée à ses dates"
        );
        // Décalée d'un jour et d'une heure.
        editor.start_date.set(date(2026, 11, 17));
        editor.start_time.set(10 * 60);
        editor.end_date.set(date(2026, 11, 17));
        editor.end_time.set(11 * 60);
        let series = editor.build(Scope::All);
        assert_eq!(
            series.start,
            date(2026, 11, 3).at(10 * 60),
            "la série entière se décale"
        );
        assert_eq!(series.duration(), 60);
        assert!(series.recurrence.repeats());
        let single = editor.build(Scope::Only(date(2026, 11, 16)));
        assert_eq!(single.start, date(2026, 11, 17).at(10 * 60));
        assert!(!single.recurrence.repeats());
    }

    #[test]
    fn un_evenement_sans_titre_ou_a_l_envers_ne_s_enregistre_pas() {
        let today = date(2026, 11, 8);
        let mut editor = EventEditor::create(9, today.at(9 * 60), false, None);
        assert_eq!(editor.problem(today), Some("Donnez un titre à l’événement."));
        editor.title = "Réunion".to_owned();
        assert_eq!(editor.problem(today), None);
        editor.end_time.set(8 * 60);
        assert_eq!(editor.problem(today), Some("La fin précède le début."));
    }

    #[test]
    fn libelles_des_rappels() {
        assert_eq!(reminder_label(15, false), "15 min avant");
        assert_eq!(reminder_label(1440, true), "1 jour avant");
        assert_eq!(reminder_label(0, true), "Le jour même à 9 h");
    }
}
