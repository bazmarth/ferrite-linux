//! Vues de productivité : Aujourd'hui, À venir, En retard et Tâches.

use eframe::egui::{self, FontId, Margin, RichText};

use super::{
    date::{self, Date},
    model::{Occurrence, Priority, Progress, Task},
    reminders::Reminder,
    views::{self, Action, Painter},
    widgets,
};
use crate::{kit, style};

/// Filtres de la vue Tâches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskFilter {
    /// Liste montrée (toutes si `None`).
    pub list: Option<u64>,
    pub status: Status,
    /// Priorité minimale.
    pub priority: Option<Priority>,
    pub sort: Sort,
}

impl Default for TaskFilter {
    fn default() -> Self {
        Self {
            list: None,
            status: Status::Open,
            priority: None,
            sort: Sort::Due,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Open,
    Done,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Due,
    Priority,
    Created,
}

impl TaskFilter {
    pub fn keep(&self, task: &Task) -> bool {
        (self.list.is_none() || task.list == self.list)
            && match self.status {
                Status::Open => !task.done,
                Status::Done => task.done,
                Status::All => true,
            }
            && self.priority.is_none_or(|minimum| task.priority >= minimum)
    }

    /// Tâches retenues, dans l'ordre choisi (sans échéance à la fin pour l'échéance).
    pub fn apply<'a>(&self, tasks: &'a [Task]) -> Vec<&'a Task> {
        let mut kept: Vec<&Task> = tasks.iter().filter(|task| self.keep(task)).collect();
        match self.sort {
            Sort::Due => kept.sort_by_key(|task| {
                (
                    task.done,
                    task.deadline().is_none(),
                    task.deadline(),
                    std::cmp::Reverse(task.priority),
                )
            }),
            Sort::Priority => {
                kept.sort_by_key(|task| (task.done, std::cmp::Reverse(task.priority), task.deadline()))
            }
            Sort::Created => kept.sort_by_key(|task| std::cmp::Reverse(task.created)),
        }
        kept
    }
}

/// Colonne centrée de largeur lisible.
pub fn column(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let width = (ui.available_width() - 40.0).clamp(240.0, 760.0);
        let side = ((ui.available_width() - width) / 2.0).max(0.0);
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            ui.add_space(side);
            ui.vertical(|ui| {
                ui.set_width(width);
                add(ui);
                ui.add_space(32.0);
            });
        });
    });
}

/// Champ d'ajout rapide d'une tâche ; renvoie le titre validé par Entrée.
pub fn quick_add(ui: &mut egui::Ui, text: &mut String, hint: &str) -> Option<String> {
    let mut added = None;
    ui.horizontal(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(text)
                .hint_text(hint)
                .desired_width(ui.available_width() - 90.0)
                .margin(Margin::symmetric(10, 7)),
        );
        let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        let valid = !text.trim().is_empty();
        let button = ui.add_enabled(valid, egui::Button::new("Ajouter"));
        if valid && (enter || button.clicked()) {
            added = Some(text.trim().to_owned());
            text.clear();
            if enter {
                field.request_focus();
            }
        }
    });
    added
}

/// En-tête d'une vue : grand titre et ligne d'explication.
fn header(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.add_space(10.0);
    ui.label(
        RichText::new(title)
            .size(24.0)
            .family(style::strong_family())
            .color(style::heading_text()),
    );
    if !detail.is_empty() {
        kit::note(ui, detail);
    }
    ui.add_space(8.0);
}

/// Carte de progression : anneau, texte, et barre.
fn progress_card(ui: &mut egui::Ui, label: &str, progress: Progress) {
    kit::card(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::progress_ring(ui, progress.fraction(), 64.0, style::success());
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new(label).size(14.0).family(style::strong_family()));
                let text = if progress.total == 0 {
                    "Aucune tâche prévue".to_owned()
                } else if progress.done == progress.total {
                    format!("Tout est fait : {} sur {}", progress.done, progress.total)
                } else {
                    format!(
                        "{} terminée{} sur {}",
                        progress.done,
                        if progress.done > 1 { "s" } else { "" },
                        progress.total
                    )
                };
                kit::note(ui, &text);
            });
        });
    });
}

/// Vue Aujourd'hui.
/// Renvoie le titre d'une tâche ajoutée par le champ rapide.
pub fn today(
    ui: &mut egui::Ui,
    view: &mut Painter,
    waiting: &[Reminder],
    quick: &mut String,
) -> Option<String> {
    let today = view.now.date;
    let mut added = None;
    column(ui, |ui| {
        header(
            ui,
            &date::capitalize(&today.long()),
            &format!("Semaine {}", today.iso_week()),
        );
        views::waiting_reminders(ui, view, waiting);
        progress_card(ui, "Tâches du jour", view.agenda.day_progress(today));
        let overdue = view.agenda.overdue_tasks(view.now).len();
        if overdue > 0 {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(date::plural(
                        overdue as u32,
                        "tâche en retard",
                        "tâches en retard",
                    ))
                    .color(style::error())
                    .family(style::strong_family()),
                );
            });
        }
        kit::heading(ui, "Événements");
        let occurrences: Vec<Occurrence> = view.occurrences(today, today.add_days(1));
        if occurrences.is_empty() {
            kit::note(ui, "Aucun événement aujourd’hui.");
        }
        for occurrence in &occurrences {
            views::occurrence_row(ui, view, occurrence);
        }
        kit::heading(ui, "Tâches");
        let mut tasks: Vec<Task> = view
            .agenda
            .tasks
            .iter()
            .filter(|task| {
                task.due == Some(today) || (task.done && task.completed.is_some_and(|at| at.date == today))
            })
            .cloned()
            .collect();
        tasks.sort_by_key(|task| (task.done, task.deadline(), std::cmp::Reverse(task.priority)));
        for task in &tasks {
            views::task_row(ui, view, task);
        }
        if tasks.is_empty() {
            kit::note(ui, "Aucune tâche pour aujourd’hui.");
        }
        ui.add_space(8.0);
        added = quick_add(ui, quick, "Nouvelle tâche pour aujourd’hui");
    });
    added
}

/// Vue À venir : quatorze jours, événements et tâches.
pub fn upcoming(ui: &mut egui::Ui, view: &mut Painter) {
    let today = view.now.date;
    column(ui, |ui| {
        header(ui, "À venir", "Les quatorze prochains jours");
        let tasks = view.agenda.upcoming_tasks(view.now, 14).len();
        let events = view.occurrences(today, today.add_days(14)).len();
        kit::note(
            ui,
            &format!(
                "{} · {}",
                date::plural(events as u32, "événement", "événements"),
                date::plural(tasks as u32, "tâche à faire", "tâches à faire")
            ),
        );
        views::agenda(ui, view, today, 14);
        let undated: Vec<Task> = view
            .agenda
            .tasks
            .iter()
            .filter(|task| !task.done && task.due.is_none())
            .cloned()
            .collect();
        if !undated.is_empty() {
            kit::heading(ui, "Sans échéance");
            for task in &undated {
                views::task_row(ui, view, task);
            }
        }
    });
}

/// Vue En retard.
pub fn overdue(ui: &mut egui::Ui, view: &mut Painter) {
    column(ui, |ui| {
        let tasks: Vec<Task> = view.agenda.overdue_tasks(view.now).into_iter().cloned().collect();
        header(
            ui,
            "En retard",
            &if tasks.is_empty() {
                String::new()
            } else {
                date::plural(
                    tasks.len() as u32,
                    "tâche dont l’échéance est passée",
                    "tâches dont l’échéance est passée",
                )
            },
        );
        if tasks.is_empty() {
            views::empty(ui, "Rien en retard", "Toutes les tâches échues sont terminées.");
            return;
        }
        if kit::button(ui, "Tout reporter à aujourd’hui", false).clicked() {
            for task in &tasks {
                view.actions.push(Action::DueToday(task.id));
            }
        }
        ui.add_space(6.0);
        let mut last: Option<Date> = None;
        for task in &tasks {
            if last != task.due {
                if let Some(due) = task.due {
                    views::day_heading(ui, due, view.now.date);
                }
                last = task.due;
            }
            ui.horizontal(|ui| {
                let width = ui.available_width();
                ui.allocate_ui(egui::vec2(width - 120.0, 44.0), |ui| {
                    views::task_row(ui, view, task)
                });
                if kit::button(ui, "Aujourd’hui", false)
                    .on_hover_text("Reporter l’échéance à aujourd’hui")
                    .clicked()
                {
                    view.actions.push(Action::DueToday(task.id));
                }
            });
        }
    });
}

/// Vue Tâches : listes, filtres, progression.
pub fn tasks(
    ui: &mut egui::Ui,
    view: &mut Painter,
    filter: &mut TaskFilter,
    quick: &mut String,
) -> Option<String> {
    let mut added = None;
    column(ui, |ui| {
        let list = view.agenda.list(filter.list);
        let title = list.map_or("Toutes les tâches".to_owned(), |list| list.name.clone());
        header(ui, &title, "");
        let progress = view.agenda.list_progress(filter.list);
        ui.horizontal(|ui| {
            let color = widgets::tint(list.map(|list| list.color));
            widgets::progress_bar(
                ui,
                progress.fraction(),
                (ui.available_width() - 160.0).max(80.0),
                color,
            );
            ui.label(
                RichText::new(format!("{} / {} terminées", progress.done, progress.total))
                    .size(12.0)
                    .color(style::text_secondary()),
            );
        });
        ui.add_space(10.0);
        // Filtres.
        ui.horizontal_wrapped(|ui| {
            let statuses = ["À faire", "Terminées", "Toutes"];
            let selected = match filter.status {
                Status::Open => 0,
                Status::Done => 1,
                Status::All => 2,
            };
            if let Some(choice) = kit::segments(ui, &statuses, selected) {
                filter.status = [Status::Open, Status::Done, Status::All][choice];
            }
            ui.add_space(8.0);
            let priority = filter
                .priority
                .map_or("Toutes priorités", |priority| match priority {
                    Priority::High => "Haute",
                    Priority::Normal => "Normale ou plus",
                    Priority::Low => "Toutes priorités",
                });
            egui::ComboBox::from_id_salt("filtre-priorite")
                .selected_text(priority)
                .width(140.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(filter.priority.is_none(), "Toutes priorités")
                        .clicked()
                    {
                        filter.priority = None;
                    }
                    if ui
                        .selectable_label(filter.priority == Some(Priority::Normal), "Normale ou plus")
                        .clicked()
                    {
                        filter.priority = Some(Priority::Normal);
                    }
                    if ui
                        .selectable_label(filter.priority == Some(Priority::High), "Haute")
                        .clicked()
                    {
                        filter.priority = Some(Priority::High);
                    }
                });
            let sort = match filter.sort {
                Sort::Due => "Par échéance",
                Sort::Priority => "Par priorité",
                Sort::Created => "Plus récentes",
            };
            egui::ComboBox::from_id_salt("tri-taches")
                .selected_text(sort)
                .width(130.0)
                .show_ui(ui, |ui| {
                    for (choice, label) in [
                        (Sort::Due, "Par échéance"),
                        (Sort::Priority, "Par priorité"),
                        (Sort::Created, "Plus récentes"),
                    ] {
                        if ui.selectable_label(filter.sort == choice, label).clicked() {
                            filter.sort = choice;
                        }
                    }
                });
        });
        ui.add_space(10.0);
        let hint = match list {
            Some(list) => format!("Nouvelle tâche dans « {} »", list.name),
            None => "Nouvelle tâche".to_owned(),
        };
        added = quick_add(ui, quick, &hint);
        ui.add_space(6.0);
        let kept: Vec<Task> = filter.apply(&view.agenda.tasks).into_iter().cloned().collect();
        if kept.is_empty() {
            let (title, detail) = match filter.status {
                Status::Done => ("Aucune tâche terminée", "Les tâches cochées apparaîtront ici."),
                _ => ("Aucune tâche", "Ajoutez-en une ci-dessus, ou avec Ctrl+T."),
            };
            views::empty(ui, title, detail);
        }
        let now = view.now;
        let mut section: Option<&str> = None;
        for task in &kept {
            // Groupes par échéance quand on trie par échéance.
            if filter.sort == Sort::Due {
                let name = if task.done {
                    "Terminées"
                } else if task.is_overdue(now) {
                    "En retard"
                } else if task.due == Some(now.date) {
                    "Aujourd’hui"
                } else if task.due.is_some_and(|due| due.days() - now.date.days() < 7) {
                    "Cette semaine"
                } else if task.due.is_some() {
                    "Plus tard"
                } else {
                    "Sans échéance"
                };
                if section != Some(name) {
                    ui.add_space(10.0);
                    let ink = if name == "En retard" {
                        style::error()
                    } else {
                        style::text_secondary()
                    };
                    ui.label(
                        RichText::new(name.to_uppercase())
                            .font(FontId::new(11.0, style::strong_family()))
                            .color(ink),
                    );
                    section = Some(name);
                }
            }
            views::task_row(ui, view, task);
        }
    });
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{date::Date, model::Agenda};

    #[test]
    fn filtres_et_tri_des_taches() {
        let now = Date::new(2026, 11, 8).unwrap().at(600);
        let mut agenda = Agenda::default();
        for (title, list, priority, done, due) in [
            ("a", 4, Priority::Low, false, Some(10)),
            ("b", 4, Priority::High, false, Some(12)),
            ("c", 5, Priority::Normal, true, Some(9)),
            ("d", 4, Priority::Normal, false, None),
        ] {
            let id = agenda.new_id();
            agenda.put_task(Task {
                id,
                title: title.to_owned(),
                notes: String::new(),
                list: Some(list),
                due: due.map(|day| Date::new(2026, 11, day).unwrap()),
                due_time: None,
                priority,
                done,
                completed: None,
                event: None,
                reminders: Vec::new(),
                created: now.add_minutes(id as i64),
            });
        }
        let titles = |filter: &TaskFilter| -> Vec<String> {
            filter
                .apply(&agenda.tasks)
                .iter()
                .map(|task| task.title.clone())
                .collect()
        };
        let mut filter = TaskFilter::default();
        assert_eq!(titles(&filter), ["a", "b", "d"]);
        filter.sort = Sort::Priority;
        assert_eq!(titles(&filter), ["b", "d", "a"]);
        filter.priority = Some(Priority::Normal);
        assert_eq!(titles(&filter), ["b", "d"]);
        filter = TaskFilter {
            list: Some(5),
            status: Status::All,
            ..TaskFilter::default()
        };
        assert_eq!(titles(&filter), ["c"]);
        filter = TaskFilter {
            status: Status::All,
            sort: Sort::Created,
            ..TaskFilter::default()
        };
        assert_eq!(titles(&filter), ["d", "c", "b", "a"]);
    }
}
