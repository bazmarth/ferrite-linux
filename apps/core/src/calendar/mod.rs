//! Calendrier : événements (récurrents, sur plusieurs jours), tâches et listes, rappels.
//!
//! - `date` : jours, heures, saisies et mise en forme en français ;
//! - `model` : événements, récurrences, tâches, catégories, listes et requêtes ;
//! - `store` : fichiers de `~/.local/share/ferrite/calendrier/` ;
//! - `reminders` : rappels, leur état, et le service `ferrite-apps rappels` qui les annonce
//!   par les notifications de Ferrite ;
//! - `views`, `lists`, `editor`, `widgets` : l'interface.

pub mod date;
mod editor;
mod lists;
pub mod model;
pub mod reminders;
pub mod store;
mod views;
mod widgets;

use std::{
    collections::HashSet,
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use eframe::egui::{self, FontId, Frame, Margin, RichText, Sense};
use ferrite_system::config::Accent;

use self::{
    date::{Date, DateTime},
    editor::{Collection, CollectionEditor, EventEditor, Outcome, Scope, TaskEditor},
    lists::TaskFilter,
    model::{Agenda, Category, Task, TaskList},
    reminders::{Book, Reminder},
    views::{Action, Painter},
};
use crate::{kit, style};

/// Largeur de la colonne latérale.
const SIDEBAR: f32 = 248.0;
/// En dessous de cette largeur de fenêtre, la colonne se replie d'elle-même.
const NARROW: f32 = 780.0;

/// Rubrique affichée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Calendar,
    Today,
    Upcoming,
    Overdue,
    Tasks,
}

/// Vue du calendrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Day,
    Week,
    Month,
    Agenda,
}

impl Mode {
    const ALL: [Mode; 4] = [Mode::Day, Mode::Week, Mode::Month, Mode::Agenda];

    fn label(self) -> &'static str {
        match self {
            Mode::Day => "Jour",
            Mode::Week => "Semaine",
            Mode::Month => "Mois",
            Mode::Agenda => "Agenda",
        }
    }
}

/// Dialogue ouvert.
#[derive(Debug)]
enum Dialog {
    Event(Box<EventEditor>),
    Task(Box<TaskEditor>),
    Collection(CollectionEditor),
}

#[derive(Debug)]
pub struct CalendarState {
    agenda: Agenda,
    agenda_path: PathBuf,
    book_path: PathBuf,
    /// Date du fichier lu, pour relire un agenda changé ailleurs.
    stamp: Option<SystemTime>,
    checked: Instant,
    book: Book,
    book_checked: Instant,
    page: Page,
    mode: Mode,
    cursor: Date,
    /// Mois montré par le petit calendrier.
    shown: Date,
    search: String,
    focus_search: bool,
    hidden: HashSet<u64>,
    filter: TaskFilter,
    dialog: Option<Dialog>,
    quick_today: String,
    quick_tasks: String,
    /// Colonne affichée : choix de l'utilisateur, sinon selon la largeur.
    sidebar: Option<bool>,
    error: Option<String>,
    /// Heure à montrer en haut de la grille au prochain affichage.
    scroll_to: Option<u32>,
    icons: style::Icons,
}

impl Default for CalendarState {
    fn default() -> Self {
        Self::new(store::agenda_path(), store::reminders_path())
    }
}

impl CalendarState {
    fn new(agenda_path: PathBuf, book_path: PathBuf) -> Self {
        let (agenda, error) = match store::load_agenda(&agenda_path) {
            Ok(agenda) => (agenda, None),
            Err(error) => (Agenda::default(), Some(format!("Agenda illisible : {error}"))),
        };
        let today = date::now().date;
        // Les rappels sont annoncés même Calendrier fermé : le service est lancé ici s'il
        // ne l'a pas été à l'ouverture de session.
        reminders::ensure_service();
        Self {
            stamp: store::modified(&agenda_path),
            book: reminders::load(&book_path),
            agenda,
            agenda_path,
            book_path,
            checked: Instant::now(),
            book_checked: Instant::now(),
            page: Page::Calendar,
            mode: Mode::Month,
            cursor: today,
            shown: today,
            search: String::new(),
            focus_search: false,
            hidden: HashSet::new(),
            filter: TaskFilter::default(),
            dialog: None,
            quick_today: String::new(),
            quick_tasks: String::new(),
            sidebar: None,
            error,
            scroll_to: Some(7 * 60),
            icons: style::Icons::default(),
        }
    }

    /// Rubrique demandée au lancement ou par un autre lancement : « aujourdhui », « taches »,
    /// « a-venir », « en-retard », « jour », « semaine », « mois », « agenda », ou une date
    /// « 2026-11-09 ».
    pub fn open(&mut self, target: &str) {
        let today = date::now().date;
        match target {
            "aujourdhui" | "aujourd'hui" => self.page = Page::Today,
            "taches" | "tâches" => self.page = Page::Tasks,
            "a-venir" => self.page = Page::Upcoming,
            "en-retard" => self.page = Page::Overdue,
            "jour" => self.show_mode(Mode::Day),
            "semaine" => self.show_mode(Mode::Week),
            "mois" => self.show_mode(Mode::Month),
            "agenda" => self.show_mode(Mode::Agenda),
            other => {
                if let Some(day) = Date::parse_input(other, today) {
                    self.cursor = day;
                    self.shown = day;
                    self.show_mode(Mode::Day);
                }
            }
        }
    }

    fn show_mode(&mut self, mode: Mode) {
        self.page = Page::Calendar;
        if mode != self.mode && matches!(mode, Mode::Day | Mode::Week) {
            self.scroll_to = Some(7 * 60);
        }
        self.mode = mode;
    }

    // --- Données ------------------------------------------------------------------------

    fn save(&mut self) {
        match store::save_agenda(&self.agenda_path, &self.agenda) {
            Ok(()) => {
                self.stamp = store::modified(&self.agenda_path);
                self.error = None;
            }
            Err(error) => self.error = Some(format!("Enregistrement impossible : {error}")),
        }
    }

    /// Relit l'agenda s'il a changé ailleurs, et l'état des rappels.
    fn refresh(&mut self) {
        if self.dialog.is_none() && self.checked.elapsed() >= Duration::from_secs(2) {
            self.checked = Instant::now();
            let stamp = store::modified(&self.agenda_path);
            if stamp != self.stamp {
                if let Ok(agenda) = store::load_agenda(&self.agenda_path) {
                    self.agenda = agenda;
                }
                self.stamp = stamp;
            }
        }
        if self.book_checked.elapsed() >= Duration::from_secs(5) {
            self.book_checked = Instant::now();
            self.book = reminders::load(&self.book_path);
        }
    }

    fn change_reminder(&mut self, key: &str, snooze: bool) {
        let now = date::now();
        let result = reminders::update(&self.book_path, |book| {
            if snooze {
                book.snooze(key, now);
            } else {
                book.acknowledge(key, now);
            }
            book.clone()
        });
        match result {
            Ok(book) => self.book = book,
            Err(error) => self.error = Some(format!("Rappel non enregistré : {error}")),
        }
    }

    fn new_task(&mut self, title: String, due: Option<Date>) -> Task {
        let now = date::now();
        Task {
            id: self.agenda.new_id(),
            title,
            notes: String::new(),
            list: self
                .filter
                .list
                .or_else(|| self.agenda.lists.first().map(|list| list.id)),
            due,
            due_time: None,
            priority: model::Priority::Normal,
            done: false,
            completed: None,
            event: None,
            reminders: Vec::new(),
            created: now,
        }
    }

    fn apply(&mut self, action: Action) {
        let now = date::now();
        match action {
            Action::NewEvent(start, all_day) => {
                let id = self.agenda.new_id();
                let category = self.agenda.categories.first().map(|category| category.id);
                self.dialog = Some(Dialog::Event(Box::new(EventEditor::create(
                    id, start, all_day, category,
                ))));
            }
            Action::EditOccurrence(occurrence) => {
                if let Some(event) = self.agenda.event(occurrence.event) {
                    self.dialog = Some(Dialog::Event(Box::new(EventEditor::open(
                        event,
                        Some(occurrence),
                    ))));
                }
            }
            Action::NewTask(due) => {
                let task = self.new_task(String::new(), due);
                self.dialog = Some(Dialog::Task(Box::new(TaskEditor::create(task))));
            }
            Action::EditTask(id) => {
                if let Some(task) = self.agenda.task(id) {
                    self.dialog = Some(Dialog::Task(Box::new(TaskEditor::open(task))));
                }
            }
            Action::ToggleTask(id) => {
                let done = self.agenda.task(id).is_some_and(|task| !task.done);
                self.agenda.set_done(id, done, now);
                self.save();
            }
            Action::Select(day) => {
                self.cursor = day;
                self.shown = day;
            }
            Action::OpenDay(day) => {
                self.cursor = day;
                self.shown = day;
                self.search.clear();
                self.show_mode(Mode::Day);
            }
            Action::DueToday(id) => {
                if let Some(task) = self.agenda.task_mut(id) {
                    task.due = Some(now.date);
                    if task.due_time.is_some_and(|time| time < now.minute) {
                        task.due_time = None;
                    }
                }
                self.save();
            }
            Action::Snooze(key) => self.change_reminder(&key, true),
            Action::Acknowledge(key) => self.change_reminder(&key, false),
        }
    }

    fn finish(&mut self, outcome: Outcome) {
        self.dialog = None;
        match outcome {
            Outcome::Close => return,
            Outcome::SaveEvent(event, Scope::All) => self.agenda.put_event(*event),
            Outcome::SaveEvent(event, Scope::Only(day)) => {
                // L'occurrence quitte la série et devient un événement à part.
                self.agenda.remove_occurrence(event.id, day);
                let mut single = *event;
                single.id = self.agenda.new_id();
                self.agenda.put_event(single);
            }
            Outcome::DeleteEvent(id, Scope::All) => self.agenda.remove_event(id),
            Outcome::DeleteEvent(id, Scope::Only(day)) => self.agenda.remove_occurrence(id, day),
            Outcome::LinkedTask(event) => {
                let mut task = self.new_task(String::new(), Some(event.start.date));
                task.event = Some(event.id);
                self.agenda.put_event(*event);
                self.dialog = Some(Dialog::Task(Box::new(TaskEditor::create(task))));
            }
            Outcome::SaveTask(task) => self.agenda.put_task(*task),
            Outcome::DeleteTask(id) => self.agenda.remove_task(id),
            Outcome::SaveCategory(id, name, color) => {
                match self
                    .agenda
                    .categories
                    .iter_mut()
                    .find(|category| category.id == id)
                {
                    Some(category) => {
                        category.name = name;
                        category.color = color;
                    }
                    None => self.agenda.categories.push(Category { id, name, color }),
                }
            }
            Outcome::DeleteCategory(id) => {
                self.agenda.remove_category(id);
                self.hidden.remove(&id);
            }
            Outcome::SaveList(id, name, color) => {
                match self.agenda.lists.iter_mut().find(|list| list.id == id) {
                    Some(list) => {
                        list.name = name;
                        list.color = color;
                    }
                    None => self.agenda.lists.push(TaskList { id, name, color }),
                }
            }
            Outcome::DeleteList(id) => {
                self.agenda.remove_list(id);
                if self.filter.list == Some(id) {
                    self.filter.list = None;
                }
            }
        }
        self.save();
    }

    // --- Navigation ---------------------------------------------------------------------

    fn step(&mut self, direction: i64) {
        self.cursor = match self.mode {
            Mode::Day => self.cursor.add_days(direction),
            Mode::Week => self.cursor.add_days(7 * direction),
            Mode::Month => self.cursor.add_months(direction as i32),
            Mode::Agenda => self.cursor.add_days(14 * direction),
        };
        self.shown = self.cursor;
    }

    fn go_today(&mut self) {
        self.cursor = date::now().date;
        self.shown = self.cursor;
        self.scroll_to = Some(date::now().minute.saturating_sub(90));
    }

    /// Titre de la barre selon la vue.
    fn title(&self) -> String {
        match self.page {
            Page::Today => "Aujourd’hui".to_owned(),
            Page::Upcoming => "À venir".to_owned(),
            Page::Overdue => "En retard".to_owned(),
            Page::Tasks => "Tâches".to_owned(),
            Page::Calendar => match self.mode {
                Mode::Month => date::capitalize(&self.cursor.month_title()),
                Mode::Day => date::capitalize(&self.cursor.long()),
                Mode::Agenda => format!("À partir du {}", self.cursor.medium()),
                Mode::Week => {
                    let first = self.cursor.week_start();
                    let last = first.add_days(6);
                    if first.month == last.month {
                        format!("{} – {} {}", first.day, last.day, last.month_title())
                    } else if first.year == last.year {
                        format!(
                            "{} – {} {}",
                            first.short().split_once(' ').map_or("", |(_, rest)| rest),
                            last.short().split_once(' ').map_or("", |(_, rest)| rest),
                            last.year
                        )
                    } else {
                        format!("{} – {}", first.medium(), last.medium())
                    }
                }
            },
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() {
            return;
        }
        let typing = ctx.memory(|memory| memory.focused().is_some());
        let (command, keys) = ctx.input(|input| {
            let pressed = |key| input.key_pressed(key);
            (
                input.modifiers.command,
                [
                    pressed(egui::Key::N),
                    pressed(egui::Key::T),
                    pressed(egui::Key::F),
                    pressed(egui::Key::Num1),
                    pressed(egui::Key::Num2),
                    pressed(egui::Key::Num3),
                    pressed(egui::Key::Num4),
                    pressed(egui::Key::ArrowLeft),
                    pressed(egui::Key::ArrowRight),
                    pressed(egui::Key::Escape),
                ],
            )
        });
        let [n, t, f, one, two, three, four, left, right, escape] = keys;
        if command {
            if n {
                let start = self.cursor.at(9 * 60);
                self.apply(Action::NewEvent(start, false));
            } else if t {
                self.apply(Action::NewTask(None));
            } else if f {
                self.focus_search = true;
            } else if one || two || three || four {
                let index = [one, two, three, four]
                    .iter()
                    .position(|pressed| *pressed)
                    .unwrap_or(0);
                self.show_mode(Mode::ALL[index]);
            }
            return;
        }
        if escape && !self.search.is_empty() {
            self.search.clear();
        }
        if typing || self.page != Page::Calendar {
            return;
        }
        if left {
            self.step(-1);
        } else if right {
            self.step(1);
        } else if t {
            self.go_today();
        }
    }

    // --- Interface ----------------------------------------------------------------------

    pub fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.refresh();
        self.keyboard(&ctx);
        // L'heure avance : trait de l'heure actuelle, retards, rappels.
        ctx.request_repaint_after(Duration::from_secs(30));
        let now = date::now();
        let width = ui.max_rect().width();
        let sidebar = self.sidebar.unwrap_or(width >= NARROW);

        if let Some(error) = self.error.clone() {
            style::status_bar(ui, |ui| {
                ui.colored_label(style::error(), error);
            });
        }
        if sidebar {
            style::sidebar(
                ui,
                "calendrier-colonne",
                SIDEBAR,
                style::sidebar_color(),
                true,
                |ui| {
                    self.sidebar_body(ui, now);
                },
            );
        }
        let mut toggle = false;
        style::toolbar(ui, !sidebar, |ui| {
            toggle = self.toolbar(ui, sidebar);
        });
        if toggle {
            self.sidebar = Some(!sidebar);
        }
        let waiting: Vec<Reminder> = self.book.waiting(&self.agenda, now);
        let mut quick_added: Option<(String, Option<Date>)> = None;
        let actions = {
            let mut view = Painter {
                agenda: &self.agenda,
                now,
                hidden: &self.hidden,
                actions: Vec::new(),
            };
            egui::CentralPanel::no_frame()
                .frame(
                    Frame::new()
                        .fill(style::background())
                        .inner_margin(Margin::symmetric(14, 10)),
                )
                .show(ui, |ui| {
                    if !self.search.trim().is_empty() {
                        lists::column(ui, |ui| {
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("Recherche")
                                    .size(24.0)
                                    .family(style::strong_family()),
                            );
                            views::search_results(ui, &mut view, &self.search, now.date);
                        });
                        return;
                    }
                    match self.page {
                        Page::Calendar => {
                            if !waiting.is_empty() && self.mode != Mode::Agenda {
                                views::waiting_reminders(ui, &mut view, &waiting);
                            }
                            match self.mode {
                                Mode::Month => views::month(ui, &mut view, self.cursor),
                                Mode::Week => views::time_grid(
                                    ui,
                                    &mut view,
                                    self.cursor.week_start(),
                                    7,
                                    self.cursor,
                                    &mut self.scroll_to,
                                ),
                                Mode::Day => views::time_grid(
                                    ui,
                                    &mut view,
                                    self.cursor,
                                    1,
                                    self.cursor,
                                    &mut self.scroll_to,
                                ),
                                Mode::Agenda => lists::column(ui, |ui| {
                                    views::waiting_reminders(ui, &mut view, &waiting);
                                    views::agenda(ui, &mut view, self.cursor, 60);
                                }),
                            }
                        }
                        Page::Today => {
                            if let Some(title) = lists::today(ui, &mut view, &waiting, &mut self.quick_today)
                            {
                                quick_added = Some((title, Some(now.date)));
                            }
                        }
                        Page::Upcoming => lists::upcoming(ui, &mut view),
                        Page::Overdue => lists::overdue(ui, &mut view),
                        Page::Tasks => {
                            if let Some(title) =
                                lists::tasks(ui, &mut view, &mut self.filter, &mut self.quick_tasks)
                            {
                                quick_added = Some((title, None));
                            }
                        }
                    }
                });
            view.actions
        };
        for action in actions {
            self.apply(action);
        }
        if let Some((title, due)) = quick_added {
            let task = self.new_task(title, due);
            self.agenda.put_task(task);
            self.save();
        }
        self.dialogs(&ctx, now);
    }

    fn dialogs(&mut self, ctx: &egui::Context, now: DateTime) {
        let outcome = match &mut self.dialog {
            None => None,
            Some(Dialog::Event(editor)) => editor.show(ctx, &self.agenda, now.date),
            Some(Dialog::Task(editor)) => editor.show(ctx, &self.agenda, now),
            Some(Dialog::Collection(editor)) => editor.show(ctx),
        };
        if let Some(outcome) = outcome {
            self.finish(outcome);
        }
    }

    /// Barre du haut ; vrai si le bouton de la colonne est cliqué.
    fn toolbar(&mut self, ui: &mut egui::Ui, sidebar: bool) -> bool {
        let mut toggle = false;
        let width = ui.available_width();
        if kit::tool_button(
            ui,
            kit::TOOL,
            sidebar,
            if sidebar {
                "Masquer la colonne"
            } else {
                "Afficher la colonne"
            },
            widgets::paint_sidebar_symbol,
        )
        .clicked()
        {
            toggle = true;
        }
        if self.page == Page::Calendar && self.search.trim().is_empty() {
            ui.add_space(4.0);
            if kit::tool_button(ui, kit::TOOL, false, "Précédent (←)", |painter, rect, ink| {
                widgets::paint_chevron(painter, rect, ink, -1.0)
            })
            .clicked()
            {
                self.step(-1);
            }
            if kit::button(ui, "Aujourd’hui", false)
                .on_hover_text("Revenir à aujourd’hui (T)")
                .clicked()
            {
                self.go_today();
            }
            if kit::tool_button(ui, kit::TOOL, false, "Suivant (→)", |painter, rect, ink| {
                widgets::paint_chevron(painter, rect, ink, 1.0)
            })
            .clicked()
            {
                self.step(1);
            }
        }
        ui.add_space(8.0);
        let compact = width < 620.0;
        let title_room = (width - if self.page == Page::Calendar { 560.0 } else { 300.0 }).max(80.0);
        let title = self.title();
        let font = FontId::new(if compact { 14.0 } else { 16.0 }, style::strong_family());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(title_room.min(320.0), 30.0), Sense::hover());
        let shown = {
            let painter = ui.painter();
            let mut text = title.clone();
            while painter
                .layout_no_wrap(text.clone(), font.clone(), style::primary_text())
                .size()
                .x
                > rect.width()
                && text.chars().count() > 2
            {
                text.pop();
                text = text.trim_end_matches('…').to_owned();
                text.pop();
                text.push('…');
            }
            text
        };
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            shown,
            font,
            style::primary_text(),
        );
        if self.page == Page::Calendar && self.mode == Mode::Week && !compact {
            kit::note(ui, &format!("Sem. {}", self.cursor.iso_week()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let field_width = if compact { 120.0 } else { 190.0 };
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Rechercher")
                    .desired_width(field_width)
                    .margin(Margin::symmetric(10, 6)),
            );
            if self.focus_search {
                field.request_focus();
                self.focus_search = false;
            }
            field.on_hover_text("Titres, lieux, descriptions et notes (Ctrl+F)");
            if self.page == Page::Calendar && self.search.trim().is_empty() {
                ui.add_space(8.0);
                let selected = Mode::ALL.iter().position(|mode| *mode == self.mode).unwrap_or(2);
                if compact {
                    egui::ComboBox::from_id_salt("vue-calendrier")
                        .selected_text(self.mode.label())
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for mode in Mode::ALL {
                                if ui.selectable_label(self.mode == mode, mode.label()).clicked() {
                                    self.show_mode(mode);
                                }
                            }
                        });
                } else {
                    let labels: Vec<&str> = Mode::ALL.iter().map(|mode| mode.label()).collect();
                    // Les segments se posent de droite à gauche : on les place à la main.
                    let segment = 70.0;
                    let total = segment * 4.0 + 12.0;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, 32.0), Sense::hover());
                    if let Some(choice) = kit::segments_at(ui, rect.center(), segment, &labels, selected) {
                        self.show_mode(Mode::ALL[choice]);
                    }
                }
            }
        });
        toggle
    }

    fn sidebar_body(&mut self, ui: &mut egui::Ui, now: DateTime) {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let half = (ui.available_width() - 6.0) / 2.0;
                if kit::wide_button(ui, half, "+ Événement", kit::Tone::Accent, true)
                    .on_hover_text("Nouvel événement (Ctrl+N)")
                    .clicked()
                {
                    self.apply(Action::NewEvent(self.cursor.at(9 * 60), false));
                }
                if kit::wide_button(ui, half, "+ Tâche", kit::Tone::Plain, true)
                    .on_hover_text("Nouvelle tâche (Ctrl+T)")
                    .clicked()
                {
                    self.apply(Action::NewTask(None));
                }
            });
            ui.add_space(10.0);
            let month_start = self.shown.first_of_month().week_start();
            let marked: HashSet<Date> = self
                .agenda
                .occurrences(month_start, month_start.add_days(42), |event| {
                    event
                        .category
                        .is_none_or(|category| !self.hidden.contains(&category))
                })
                .iter()
                .flat_map(|occurrence| {
                    let first = occurrence.start.date.max(month_start);
                    let last = occurrence.last_day().min(month_start.add_days(41));
                    (0..=(last.days() - first.days()).max(0)).map(move |offset| first.add_days(offset))
                })
                .collect();
            if let Some(day) = widgets::mini_month(ui, &mut self.shown, self.cursor, now.date, |day| {
                marked.contains(&day)
            }) {
                self.cursor = day;
                self.search.clear();
                if self.page != Page::Calendar {
                    self.show_mode(Mode::Day);
                }
            }
            ui.add_space(8.0);
            let overdue = self.agenda.overdue_tasks(now).len();
            let open = self.agenda.tasks.iter().filter(|task| !task.done).count();
            let today_count = self
                .agenda
                .occurrences(now.date, now.date.add_days(1), |_| true)
                .len()
                + self
                    .agenda
                    .tasks_due_on(now.date)
                    .filter(|task| !task.done)
                    .count();
            let upcoming = self.agenda.upcoming_tasks(now, 14).len();
            let items: [(Page, &'static str, Accent, &str, usize); 5] = [
                (Page::Calendar, "glyph-calendar", Accent::Red, "Calendrier", 0),
                (
                    Page::Today,
                    "glyph-today",
                    Accent::Blue,
                    "Aujourd’hui",
                    today_count,
                ),
                (
                    Page::Upcoming,
                    "glyph-upcoming",
                    Accent::Purple,
                    "À venir",
                    upcoming,
                ),
                (
                    Page::Overdue,
                    "glyph-overdue",
                    Accent::Orange,
                    "En retard",
                    overdue,
                ),
                (Page::Tasks, "glyph-tasks", Accent::Green, "Tâches", open),
            ];
            for (page, icon, color, label, count) in items {
                let selected = self.page == page
                    && self.search.trim().is_empty()
                    && (page != Page::Tasks || self.filter.list.is_none());
                let response = style::sidebar_tile_item(
                    ui,
                    &mut self.icons,
                    icon,
                    style::accent_color(color),
                    label,
                    selected,
                );
                if count > 0 {
                    let ink = if page == Page::Overdue {
                        style::error()
                    } else {
                        style::text_secondary()
                    };
                    kit::tabular_text(
                        ui.painter(),
                        response.rect.right_center() - egui::vec2(10.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        &count.to_string(),
                        FontId::new(12.5, style::strong_family()),
                        ink,
                    );
                }
                if response.clicked() {
                    self.search.clear();
                    self.page = page;
                    if page == Page::Tasks {
                        self.filter.list = None;
                    }
                }
            }
            self.categories(ui);
            self.lists(ui);
        });
    }

    /// Titre de groupe avec un bouton « + ».
    fn group_heading(ui: &mut egui::Ui, title: &str, hint: &str) -> bool {
        ui.add_space(12.0);
        let mut clicked = false;
        ui.horizontal(|ui| {
            style::sidebar_heading(ui, title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = kit::tool_button(ui, 22.0, false, hint, widgets::paint_plus).clicked();
            });
        });
        clicked
    }

    fn categories(&mut self, ui: &mut egui::Ui) {
        if Self::group_heading(ui, "Catégories", "Nouvelle catégorie") {
            let id = self.agenda.new_id();
            let color = Accent::ALL[self.agenda.categories.len() % Accent::ALL.len()];
            self.dialog = Some(Dialog::Collection(CollectionEditor::new(
                Collection::Category,
                id,
                true,
                "",
                color,
            )));
        }
        let categories = self.agenda.categories.clone();
        for category in &categories {
            let visible = !self.hidden.contains(&category.id);
            let response = collection_row(
                ui,
                &category.name,
                widgets::tint(Some(category.color)),
                visible,
                false,
                None,
            );
            let response = response.on_hover_text(if visible {
                "Masquer ses événements"
            } else {
                "Afficher ses événements"
            });
            if response.clicked() {
                if visible {
                    self.hidden.insert(category.id);
                } else {
                    self.hidden.remove(&category.id);
                }
            }
            response.context_menu(|ui| {
                if ui.button("Modifier…").clicked() {
                    self.dialog = Some(Dialog::Collection(CollectionEditor::new(
                        Collection::Category,
                        category.id,
                        false,
                        &category.name,
                        category.color,
                    )));
                }
                if ui.button("Afficher seulement celle-ci").clicked() {
                    self.hidden = categories
                        .iter()
                        .map(|other| other.id)
                        .filter(|id| *id != category.id)
                        .collect();
                }
                if ui.button("Tout afficher").clicked() {
                    self.hidden.clear();
                }
            });
        }
    }

    fn lists(&mut self, ui: &mut egui::Ui) {
        if Self::group_heading(ui, "Listes", "Nouvelle liste de tâches") {
            let id = self.agenda.new_id();
            let color = Accent::ALL[(self.agenda.lists.len() + 4) % Accent::ALL.len()];
            self.dialog = Some(Dialog::Collection(CollectionEditor::new(
                Collection::List,
                id,
                true,
                "",
                color,
            )));
        }
        let lists = self.agenda.lists.clone();
        for list in &lists {
            let progress = self.agenda.list_progress(Some(list.id));
            let selected = self.page == Page::Tasks
                && self.filter.list == Some(list.id)
                && self.search.trim().is_empty();
            let open = progress.total - progress.done;
            let response = collection_row(
                ui,
                &list.name,
                widgets::tint(Some(list.color)),
                true,
                selected,
                Some(open),
            );
            let response =
                response.on_hover_text(format!("{} terminées sur {}", progress.done, progress.total));
            if response.clicked() {
                self.search.clear();
                self.page = Page::Tasks;
                self.filter.list = Some(list.id);
            }
            response.context_menu(|ui| {
                if ui.button("Modifier…").clicked() {
                    self.dialog = Some(Dialog::Collection(CollectionEditor::new(
                        Collection::List,
                        list.id,
                        false,
                        &list.name,
                        list.color,
                    )));
                }
            });
        }
    }
}

/// Ligne d'une catégorie ou d'une liste : pastille (pleine si visible), nom, compte.
fn collection_row(
    ui: &mut egui::Ui,
    name: &str,
    color: egui::Color32,
    visible: bool,
    selected: bool,
    count: Option<usize>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 8.0, style::sidebar_selected());
    } else if response.hovered() {
        painter.rect_filled(rect, 8.0, style::tint(9));
    }
    let center = egui::pos2(rect.left() + 18.0, rect.center().y);
    if visible {
        painter.circle_filled(center, 6.0, color);
    } else {
        painter.circle_stroke(center, 5.5, egui::Stroke::new(1.5, color));
    }
    let ink = if !visible {
        style::text_secondary()
    } else if selected {
        style::sidebar_selected_text()
    } else {
        style::primary_text()
    };
    painter.text(
        egui::pos2(rect.left() + 34.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        FontId::proportional(13.5),
        ink,
    );
    if let Some(count) = count.filter(|count| *count > 0) {
        painter.text(
            rect.right_center() - egui::vec2(10.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            count.to_string(),
            FontId::proportional(12.5),
            style::text_secondary(),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> (CalendarState, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "ferrite-calendrier-etat-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        // Sans `ensure_service` : on construit l'état à la main.
        let state = CalendarState {
            agenda: Agenda::default(),
            agenda_path: dir.join("agenda"),
            book_path: dir.join("rappels"),
            stamp: None,
            checked: Instant::now(),
            book: Book::default(),
            book_checked: Instant::now(),
            page: Page::Calendar,
            mode: Mode::Month,
            cursor: Date::new(2026, 11, 8).unwrap(),
            shown: Date::new(2026, 11, 8).unwrap(),
            search: String::new(),
            focus_search: false,
            hidden: HashSet::new(),
            filter: TaskFilter::default(),
            dialog: None,
            quick_today: String::new(),
            quick_tasks: String::new(),
            sidebar: None,
            error: None,
            scroll_to: None,
            icons: style::Icons::default(),
        };
        (state, dir)
    }

    #[test]
    fn navigation_et_titres() {
        let (mut state, _) = state();
        assert_eq!(state.title(), "Novembre 2026");
        state.step(1);
        assert_eq!(state.cursor, Date::new(2026, 12, 8).unwrap());
        state.show_mode(Mode::Week);
        assert_eq!(state.title(), "7 – 13 décembre 2026");
        state.step(-1);
        assert_eq!(state.title(), "30 nov. – 6 déc. 2026");
        state.show_mode(Mode::Day);
        state.step(1);
        assert_eq!(state.title(), "Mercredi 2 décembre 2026");
        state.open("2026-11-09");
        assert_eq!(
            (state.page, state.mode, state.cursor),
            (Page::Calendar, Mode::Day, Date::new(2026, 11, 9).unwrap())
        );
        state.open("taches");
        assert_eq!(state.page, Page::Tasks);
    }

    #[test]
    fn une_occurrence_modifiee_quitte_la_serie_et_tout_est_enregistre() {
        let (mut state, dir) = state();
        let start = Date::new(2026, 11, 2).unwrap().at(9 * 60);
        let mut event = model::Event {
            id: state.agenda.new_id(),
            title: "Yoga".to_owned(),
            description: String::new(),
            location: String::new(),
            category: Some(1),
            start,
            end: start.add_minutes(60),
            all_day: false,
            recurrence: model::Recurrence {
                frequency: model::Frequency::Weekly,
                ..model::Recurrence::default()
            },
            exceptions: Vec::new(),
            reminders: vec![15],
        };
        state.finish(Outcome::SaveEvent(Box::new(event.clone()), Scope::All));
        let day = Date::new(2026, 11, 16).unwrap();
        event.start = day.at(18 * 60);
        event.end = day.at(19 * 60);
        event.recurrence = model::Recurrence::default();
        state.finish(Outcome::SaveEvent(Box::new(event.clone()), Scope::Only(day)));
        assert_eq!(state.agenda.events.len(), 2);
        let week = state.agenda.occurrences(day, day.add_days(1), |_| true);
        assert_eq!(week.len(), 1);
        assert_eq!(week[0].start, day.at(18 * 60));
        assert_ne!(week[0].event, event.id, "un nouvel événement à part");
        // Relu depuis le disque : identique.
        let saved = store::load_agenda(&state.agenda_path).unwrap();
        assert_eq!(saved, state.agenda);
        // Tâche ajoutée, cochée, puis reportée à aujourd'hui.
        let task = state.new_task(
            "Préparer le tapis".to_owned(),
            Some(Date::new(2026, 1, 1).unwrap()),
        );
        let id = task.id;
        state.agenda.put_task(task);
        state.apply(Action::ToggleTask(id));
        assert!(state.agenda.task(id).unwrap().done);
        state.apply(Action::DueToday(id));
        assert_eq!(state.agenda.task(id).unwrap().due, Some(date::now().date));
        // Catégorie supprimée : les événements la perdent.
        state.finish(Outcome::DeleteCategory(1));
        assert!(state.agenda.events.iter().all(|event| event.category.is_none()));
        let _ = std::fs::remove_dir_all(dir);
    }
}
