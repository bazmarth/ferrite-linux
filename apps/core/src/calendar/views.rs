//! Vues du calendrier : mois, semaine, jour, agenda et résultats de recherche. Elles ne
//! modifient rien : elles posent des [`Action`] que `CalendarState` applique ensuite.

use std::collections::HashSet;

use eframe::egui::{self, Color32, FontId, RichText, Sense, Stroke};

use super::{
    date::{self, DAY_MINUTES, Date, DateTime, WEEKDAYS_SHORT, clock},
    model::{Agenda, Event, Found, Occurrence, Priority, Task, layout_columns},
    reminders::Reminder,
    widgets,
};
use crate::{kit, style};

/// Hauteur d'une heure dans les vues Semaine et Jour.
pub const HOUR: f32 = 48.0;
/// Colonne des heures, à gauche de la grille.
const GUTTER: f32 = 56.0;
/// Hauteur d'une pastille d'événement (vue Mois, bandeau des journées).
const CHIP: f32 = 19.0;

/// Demande d'une vue à l'application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    NewEvent(DateTime, bool),
    EditOccurrence(Occurrence),
    NewTask(Option<Date>),
    EditTask(u64),
    ToggleTask(u64),
    /// Choisir ce jour (sans changer de vue).
    Select(Date),
    /// Ouvrir ce jour dans la vue Jour.
    OpenDay(Date),
    /// Remettre une tâche en retard à aujourd'hui.
    DueToday(u64),
    Snooze(String),
    Acknowledge(String),
}

/// Ce que les vues lisent, et les actions qu'elles posent.
#[derive(Debug)]
pub struct Painter<'a> {
    pub agenda: &'a Agenda,
    pub now: DateTime,
    pub hidden: &'a HashSet<u64>,
    pub actions: Vec<Action>,
}

impl Painter<'_> {
    pub fn visible(&self, event: &Event) -> bool {
        event
            .category
            .is_none_or(|category| !self.hidden.contains(&category))
    }

    pub fn occurrences(&self, from: Date, to: Date) -> Vec<Occurrence> {
        self.agenda.occurrences(from, to, |event| {
            event
                .category
                .is_none_or(|category| !self.hidden.contains(&category))
        })
    }

    pub fn event_color(&self, id: u64) -> Color32 {
        let category = self
            .agenda
            .event(id)
            .and_then(|event| self.agenda.category(event.category));
        widgets::tint(category.map(|category| category.color))
    }

    pub fn task_color(&self, task: &Task) -> Color32 {
        widgets::tint(self.agenda.list(task.list).map(|list| list.color))
    }

    fn title(&self, id: u64) -> String {
        self.agenda
            .event(id)
            .map(|event| event.title.clone())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "Sans titre".to_owned())
    }

    fn today(&self) -> Date {
        self.now.date
    }
}

/// Texte coupé à la largeur `width`, avec « … ».
fn fit(painter: &egui::Painter, text: &str, font: &FontId, width: f32) -> String {
    let measure = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
    };
    if measure(text) <= width {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if measure(&candidate) <= width {
            return candidate;
        }
    }
    String::new()
}

/// Pastille d'un événement : fond teinté, filet de couleur à gauche, texte.
fn paint_chip(ui: &egui::Ui, rect: egui::Rect, color: Color32, text: &str, hovered: bool, strong: bool) {
    let painter = ui.painter();
    let fill = color.gamma_multiply(if hovered { 0.34 } else { 0.22 });
    painter.rect_filled(rect, 4.0, fill);
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
    painter.rect_filled(
        bar,
        egui::CornerRadius {
            nw: 4,
            sw: 4,
            ne: 0,
            se: 0,
        },
        color,
    );
    let font = if strong {
        FontId::new(11.5, style::strong_family())
    } else {
        FontId::proportional(ferrite_ui::tokens::typography::CAPTION)
    };
    let text = fit(painter, text, &font, rect.width() - 10.0);
    painter.text(
        egui::pos2(rect.left() + 7.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font,
        style::primary_text(),
    );
}

/// Ligne d'une tâche dans une case (vue Mois, bandeau) : rond à cocher et titre.
fn paint_task_chip(ui: &egui::Ui, rect: egui::Rect, color: Color32, task: &Task, hovered: bool) {
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, 4.0, style::tint(10));
    }
    widgets::paint_check(
        painter,
        egui::pos2(rect.left() + 8.0, rect.center().y),
        5.5,
        task.done,
        color,
        false,
    );
    let font = FontId::proportional(ferrite_ui::tokens::typography::CAPTION);
    let text = fit(painter, &task.title, &font, rect.width() - 20.0);
    let ink = if task.done {
        style::text_secondary()
    } else {
        style::primary_text()
    };
    let galley = painter.layout_no_wrap(text, font, ink);
    let at = egui::pos2(rect.left() + 17.0, rect.center().y - galley.size().y / 2.0);
    let width = galley.size().x;
    painter.galley(at, galley, ink);
    if task.done {
        painter.hline(
            at.x..=at.x + width,
            rect.center().y,
            Stroke::new(1.0, style::text_secondary()),
        );
    }
}

/// Infobulle d'une occurrence : titre, heures, lieu, répétition.
fn occurrence_tooltip(view: &Painter, occurrence: &Occurrence) -> String {
    let Some(event) = view.agenda.event(occurrence.event) else {
        return String::new();
    };
    let mut lines = vec![view.title(occurrence.event)];
    lines.push(occurrence_time(occurrence));
    if !event.location.is_empty() {
        lines.push(event.location.clone());
    }
    if event.recurrence.repeats() {
        lines.push(event.recurrence.summary());
    }
    lines.join("\n")
}

/// « 09:00 – 10:30 », « Toute la journée », « 9 nov. 22:00 → 10 nov. 02:00 ».
pub fn occurrence_time(occurrence: &Occurrence) -> String {
    if occurrence.all_day {
        let last = occurrence.last_day();
        if last == occurrence.start.date {
            "Toute la journée".to_owned()
        } else {
            format!("Du {} au {}", occurrence.start.date.short(), last.short())
        }
    } else if occurrence.spans_days() {
        format!("{} → {}", occurrence.start.short(), occurrence.end.short())
    } else {
        format!(
            "{} – {}",
            clock(occurrence.start.minute),
            clock(occurrence.end.minute)
        )
    }
}

// --- Vue Mois ------------------------------------------------------------------------

pub fn month(ui: &mut egui::Ui, view: &mut Painter, cursor: Date) {
    let first = cursor.first_of_month().week_start();
    let last = cursor.first_of_month().add_months(1).add_days(-1);
    let weeks = ((last.days() - first.days()) / 7 + 1) as usize;
    let area = ui.available_rect_before_wrap();
    let head = 30.0;
    let cell_w = area.width() / 7.0;
    let cell_h = ((area.height() - head) / weeks as f32).max(60.0);
    let narrow = cell_w < 92.0;
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter().clone();
    for (index, name) in WEEKDAYS_SHORT.iter().enumerate() {
        let x = area.left() + cell_w * (index as f32 + 0.5);
        let label = if narrow {
            date::WEEKDAYS_LETTER[index]
        } else {
            name
        };
        painter.text(
            egui::pos2(x, area.top() + head / 2.0),
            egui::Align2::CENTER_CENTER,
            date::capitalize(label.trim_end_matches('.')),
            FontId::new(12.0, style::strong_family()),
            style::text_secondary(),
        );
    }
    let occurrences = view.occurrences(first, first.add_days(weeks as i64 * 7));
    let today = view.today();
    for index in 0..weeks * 7 {
        let day = first.add_days(index as i64);
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                area.left() + cell_w * (index % 7) as f32,
                area.top() + head + cell_h * (index / 7) as f32,
            ),
            egui::vec2(cell_w, cell_h),
        );
        let in_month = day.month == cursor.month;
        if !in_month {
            painter.rect_filled(rect, 0.0, style::tint(6));
        } else if day.weekday() >= 5 {
            painter.rect_filled(rect, 0.0, style::tint(3));
        }
        let response = ui.interact(rect, ui.scope_id().with(("mois", day.days())), Sense::click());
        if day == cursor {
            painter.rect_stroke(
                rect.shrink(1.0),
                4.0,
                Stroke::new(1.5, style::accent()),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_filled(rect, 0.0, style::tint(5));
        }
        painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, style::separator()));
        if index % 7 != 0 {
            painter.vline(rect.left(), rect.y_range(), Stroke::new(1.0, style::separator()));
        }
        // Numéro du jour, rond d'accent aujourd'hui.
        let number = egui::pos2(rect.left() + 16.0, rect.top() + 14.0);
        let label = if day.day == 1 && !narrow {
            format!("{} {}", day.day, date::MONTHS_SHORT[day.month as usize - 1])
        } else {
            day.day.to_string()
        };
        if day == today {
            painter.circle_filled(number, 11.0, style::accent());
        }
        painter.text(
            if day == today {
                number
            } else {
                egui::pos2(rect.left() + 7.0, number.y)
            },
            if day == today {
                egui::Align2::CENTER_CENTER
            } else {
                egui::Align2::LEFT_CENTER
            },
            label,
            FontId::new(12.5, style::strong_family()),
            if day == today {
                style::accent_text()
            } else if in_month {
                style::primary_text()
            } else {
                style::tertiary_text()
            },
        );
        if response.double_clicked() {
            view.actions.push(Action::NewEvent(day.at(9 * 60), false));
        } else if response.clicked() {
            view.actions.push(Action::Select(day));
        }
        // Contenu : événements du jour, puis tâches.
        let items: Vec<&Occurrence> = occurrences.iter().filter(|o| o.touches(day)).collect();
        let tasks: Vec<&Task> = view.agenda.tasks_due_on(day).collect();
        let total = items.len() + tasks.len();
        let capacity = (((rect.height() - 30.0) / (CHIP + 2.0)).floor() as usize).max(1);
        let shown = if total > capacity { capacity - 1 } else { total };
        let mut y = rect.top() + 27.0;
        let chip_rect = |y: f32| {
            egui::Rect::from_min_size(
                egui::pos2(rect.left() + 3.0, y),
                egui::vec2(rect.width() - 6.0, CHIP),
            )
        };
        for occurrence in items.iter().take(shown) {
            let chip = chip_rect(y);
            let id = ui
                .scope_id()
                .with(("puce", occurrence.event, occurrence.start.stamp(), day.days()));
            let chip_response = ui.interact(chip, id, Sense::click());
            let color = view.event_color(occurrence.event);
            let title = view.title(occurrence.event);
            let text = if occurrence.spans_days() {
                // Le titre au premier jour et à chaque début de semaine.
                if occurrence.start.date == day || day.weekday() == 0 {
                    title
                } else {
                    String::new()
                }
            } else if narrow {
                title
            } else {
                format!("{} {}", clock(occurrence.start.minute), title)
            };
            if occurrence.spans_days() {
                paint_chip(ui, chip, color, &text, chip_response.hovered(), true);
            } else {
                // Événement de quelques heures : pastille ronde et texte, sans fond.
                if chip_response.hovered() {
                    painter.rect_filled(chip, 4.0, style::tint(10));
                }
                painter.circle_filled(egui::pos2(chip.left() + 6.0, chip.center().y), 3.5, color);
                let font = FontId::proportional(ferrite_ui::tokens::typography::CAPTION);
                let text = fit(&painter, &text, &font, chip.width() - 16.0);
                painter.text(
                    egui::pos2(chip.left() + 13.0, chip.center().y),
                    egui::Align2::LEFT_CENTER,
                    text,
                    font,
                    style::primary_text(),
                );
            }
            if chip_response
                .on_hover_text(occurrence_tooltip(view, occurrence))
                .clicked()
            {
                view.actions.push(Action::EditOccurrence(**occurrence));
            }
            y += CHIP + 2.0;
        }
        for task in tasks.iter().take(shown.saturating_sub(items.len())) {
            let chip = chip_rect(y);
            let chip_response =
                ui.interact(chip, ui.scope_id().with(("tache-mois", task.id)), Sense::click());
            paint_task_chip(ui, chip, view.task_color(task), task, chip_response.hovered());
            if chip_response.clicked() {
                view.actions.push(Action::EditTask(task.id));
            }
            y += CHIP + 2.0;
        }
        if total > shown {
            let more = chip_rect(y);
            let more_response = ui
                .interact(more, ui.scope_id().with(("plus", day.days())), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            painter.text(
                egui::pos2(more.left() + 6.0, more.center().y),
                egui::Align2::LEFT_CENTER,
                format!(
                    "+ {} autre{}",
                    total - shown,
                    if total - shown > 1 { "s" } else { "" }
                ),
                FontId::new(11.0, style::strong_family()),
                if more_response.hovered() {
                    style::accent()
                } else {
                    style::text_secondary()
                },
            );
            if more_response.clicked() {
                view.actions.push(Action::OpenDay(day));
            }
        }
    }
}

// --- Vues Semaine et Jour ------------------------------------------------------------

/// Grille horaire de `days` jours à partir de `first`. `scroll_to` : heure (minutes) à
/// montrer en haut au premier affichage.
pub fn time_grid(
    ui: &mut egui::Ui,
    view: &mut Painter,
    first: Date,
    days: usize,
    cursor: Date,
    scroll_to: &mut Option<u32>,
) {
    let area = ui.available_rect_before_wrap();
    let column = ((area.width() - GUTTER) / days as f32).max(40.0);
    let today = view.today();
    let last = first.add_days(days as i64);
    let occurrences = view.occurrences(first, last);
    // En-tête des jours.
    let head = 46.0;
    let (header, _) = ui.allocate_exact_size(egui::vec2(area.width(), head), Sense::hover());
    let painter = ui.painter().clone();
    for index in 0..days {
        let day = first.add_days(index as i64);
        let rect = egui::Rect::from_min_size(
            egui::pos2(header.left() + GUTTER + column * index as f32, header.top()),
            egui::vec2(column, head),
        );
        let response = ui
            .interact(rect, ui.scope_id().with(("entete", day.days())), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Ouvrir ce jour");
        if response.clicked() && days > 1 {
            view.actions.push(Action::OpenDay(day));
        }
        let name = date::capitalize(WEEKDAYS_SHORT[day.weekday()].trim_end_matches('.'));
        let compact = column < 70.0;
        let number_center = egui::pos2(
            rect.center().x + if compact { 0.0 } else { 16.0 },
            rect.center().y + if compact { 7.0 } else { 0.0 },
        );
        if day == today {
            painter.circle_filled(number_center, 13.0, style::accent());
        } else if day == cursor && days > 1 {
            painter.circle_filled(number_center, 13.0, style::sidebar_selected());
        }
        painter.text(
            egui::pos2(
                if compact {
                    rect.center().x
                } else {
                    number_center.x - 17.0
                },
                if compact {
                    rect.center().y - 10.0
                } else {
                    rect.center().y
                },
            ),
            if compact {
                egui::Align2::CENTER_CENTER
            } else {
                egui::Align2::RIGHT_CENTER
            },
            name,
            FontId::proportional(ferrite_ui::tokens::typography::SMALL),
            style::text_secondary(),
        );
        painter.text(
            number_center,
            egui::Align2::CENTER_CENTER,
            day.day.to_string(),
            FontId::new(15.0, style::strong_family()),
            if day == today {
                style::accent_text()
            } else {
                style::primary_text()
            },
        );
    }
    // Bandeau des journées entières, des événements sur plusieurs jours et des tâches.
    let banner: Vec<Vec<BannerItem>> = (0..days)
        .map(|index| {
            let day = first.add_days(index as i64);
            let mut items: Vec<BannerItem> = occurrences
                .iter()
                .filter(|o| o.spans_days() && o.touches(day))
                .map(|o| BannerItem::Event(*o))
                .collect();
            items.extend(
                view.agenda
                    .tasks_due_on(day)
                    .map(|task| BannerItem::Task(task.id)),
            );
            items
        })
        .collect();
    let rows = banner.iter().map(Vec::len).max().unwrap_or(0).min(4);
    let banner_height = if rows == 0 {
        8.0
    } else {
        rows as f32 * (CHIP + 2.0) + 8.0
    };
    let (strip, _) = ui.allocate_exact_size(egui::vec2(area.width(), banner_height), Sense::hover());
    painter.hline(strip.x_range(), strip.top(), Stroke::new(1.0, style::separator()));
    if rows > 0 {
        painter.text(
            egui::pos2(strip.left() + GUTTER - 8.0, strip.top() + 4.0 + CHIP / 2.0),
            egui::Align2::RIGHT_CENTER,
            "Journée",
            FontId::proportional(ferrite_ui::tokens::typography::CAPTION),
            style::text_secondary(),
        );
    }
    for (index, items) in banner.iter().enumerate() {
        let day = first.add_days(index as i64);
        let x = strip.left() + GUTTER + column * index as f32;
        let overflow = items.len() > rows;
        for (row, item) in items
            .iter()
            .enumerate()
            .take(if overflow { rows - 1 } else { rows })
        {
            let chip = egui::Rect::from_min_size(
                egui::pos2(x + 2.0, strip.top() + 4.0 + row as f32 * (CHIP + 2.0)),
                egui::vec2(column - 4.0, CHIP),
            );
            match item {
                BannerItem::Event(occurrence) => {
                    let id = ui.scope_id().with((
                        "bandeau",
                        occurrence.event,
                        occurrence.start.stamp(),
                        day.days(),
                    ));
                    let response = ui.interact(chip, id, Sense::click());
                    let text = if occurrence.start.date == day || index == 0 {
                        view.title(occurrence.event)
                    } else {
                        String::new()
                    };
                    paint_chip(
                        ui,
                        chip,
                        view.event_color(occurrence.event),
                        &text,
                        response.hovered(),
                        true,
                    );
                    if response
                        .on_hover_text(occurrence_tooltip(view, occurrence))
                        .clicked()
                    {
                        view.actions.push(Action::EditOccurrence(*occurrence));
                    }
                }
                BannerItem::Task(id) => {
                    let Some(task) = view.agenda.task(*id) else {
                        continue;
                    };
                    let response =
                        ui.interact(chip, ui.scope_id().with(("bandeau-tache", id)), Sense::click());
                    paint_task_chip(ui, chip, view.task_color(task), task, response.hovered());
                    if response.clicked() {
                        view.actions.push(Action::EditTask(*id));
                    }
                }
            }
        }
        if overflow {
            let chip = egui::Rect::from_min_size(
                egui::pos2(x + 2.0, strip.top() + 4.0 + (rows - 1) as f32 * (CHIP + 2.0)),
                egui::vec2(column - 4.0, CHIP),
            );
            let response = ui.interact(
                chip,
                ui.scope_id().with(("bandeau-plus", day.days())),
                Sense::click(),
            );
            painter.text(
                egui::pos2(chip.left() + 6.0, chip.center().y),
                egui::Align2::LEFT_CENTER,
                format!("+ {}", items.len() - rows + 1),
                FontId::new(11.0, style::strong_family()),
                if response.hovered() {
                    style::accent()
                } else {
                    style::text_secondary()
                },
            );
            if response.clicked() {
                view.actions.push(Action::OpenDay(day));
            }
        }
    }
    let (_, line) = ui.allocate_exact_size(egui::vec2(area.width(), 1.0), Sense::hover());
    painter.hline(
        line.rect.x_range(),
        line.rect.top(),
        Stroke::new(1.0, style::separator()),
    );
    // Grille des heures, qui défile.
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(("grille", days))
        .auto_shrink(false);
    if let Some(minute) = scroll_to.take() {
        scroll = scroll.vertical_scroll_offset(minute as f32 / 60.0 * HOUR);
    }
    scroll.show(ui, |ui| {
        let (grid, _) = ui.allocate_exact_size(egui::vec2(area.width(), HOUR * 24.0 + 8.0), Sense::hover());
        let grid = egui::Rect::from_min_size(
            grid.min + egui::vec2(0.0, 6.0),
            egui::vec2(grid.width(), HOUR * 24.0),
        );
        let painter = ui.painter().clone();
        for index in 0..days {
            let day = first.add_days(index as i64);
            let x = grid.left() + GUTTER + column * index as f32;
            let rect =
                egui::Rect::from_min_size(egui::pos2(x, grid.top()), egui::vec2(column, grid.height()));
            if day.weekday() >= 5 {
                painter.rect_filled(rect, 0.0, style::tint(3));
            }
            painter.vline(x, grid.y_range(), Stroke::new(1.0, style::separator()));
            // Clic dans le vide : nouvel événement à la demi-heure.
            let response = ui.interact(rect, ui.scope_id().with(("colonne", day.days())), Sense::click());
            if let Some(pointer) = response.interact_pointer_pos()
                && response.double_clicked()
            {
                let minute =
                    (((pointer.y - grid.top()) / HOUR * 60.0) as u32 / 30 * 30).min(DAY_MINUTES as u32 - 30);
                view.actions.push(Action::NewEvent(day.at(minute), false));
            } else if response.clicked() {
                view.actions.push(Action::Select(day));
            }
        }
        for hour in 0..24 {
            let y = grid.top() + hour as f32 * HOUR;
            painter.hline(
                grid.left() + GUTTER - 6.0..=grid.right(),
                y,
                Stroke::new(1.0, style::separator()),
            );
            // L'étiquette de l'heure actuelle remplace celle d'une heure trop proche.
            let near_now = first.days() <= today.days()
                && today.days() < last.days()
                && (i64::from(view.now.minute) - i64::from(hour) * 60).abs() < 15;
            if hour > 0 && !near_now {
                painter.text(
                    egui::pos2(grid.left() + GUTTER - 10.0, y),
                    egui::Align2::RIGHT_CENTER,
                    format!("{hour:02}:00"),
                    FontId::proportional(ferrite_ui::tokens::typography::CAPTION),
                    style::text_secondary(),
                );
            }
        }
        // Événements de la journée, en colonnes s'ils se chevauchent.
        for index in 0..days {
            let day = first.add_days(index as i64);
            let x = grid.left() + GUTTER + column * index as f32;
            let items: Vec<&Occurrence> = occurrences
                .iter()
                .filter(|o| !o.spans_days() && o.start.date == day)
                .collect();
            let spans: Vec<(i64, i64)> = items
                .iter()
                .map(|o| (i64::from(o.start.minute), o.end.stamp() - day.at(0).stamp()))
                .collect();
            for (occurrence, (span, (col, cols))) in
                items.iter().zip(spans.iter().zip(layout_columns(&spans)))
            {
                let top = grid.top() + span.0 as f32 / 60.0 * HOUR;
                let height = ((span.1 - span.0) as f32 / 60.0 * HOUR).max(18.0);
                let width = (column - 6.0) / cols as f32;
                let rect = egui::Rect::from_min_size(
                    egui::pos2(x + 2.0 + width * col as f32, top + 1.0),
                    egui::vec2(width - 2.0, height - 2.0),
                );
                let id = ui
                    .scope_id()
                    .with(("bloc", occurrence.event, occurrence.start.stamp()));
                let response = ui.interact(rect, id, Sense::click());
                let color = view.event_color(occurrence.event);
                let past = occurrence.end < view.now;
                let fill = color.gamma_multiply(if response.hovered() {
                    0.36
                } else if past {
                    0.14
                } else {
                    0.24
                });
                painter.rect_filled(rect, 6.0, fill);
                painter.rect_filled(
                    egui::Rect::from_min_size(rect.min, egui::vec2(3.5, rect.height())),
                    egui::CornerRadius {
                        nw: 6,
                        sw: 6,
                        ne: 0,
                        se: 0,
                    },
                    color,
                );
                let clip =
                    painter.with_clip_rect(rect.shrink2(egui::vec2(2.0, 1.0)).intersect(painter.clip_rect()));
                let title_font = FontId::new(12.0, style::strong_family());
                let ink = if past {
                    style::text_secondary()
                } else {
                    style::primary_text()
                };
                let title = fit(
                    &clip,
                    &view.title(occurrence.event),
                    &title_font,
                    rect.width() - 12.0,
                );
                clip.text(
                    egui::pos2(rect.left() + 8.0, rect.top() + 3.0),
                    egui::Align2::LEFT_TOP,
                    title,
                    title_font,
                    ink,
                );
                if height >= 36.0 {
                    let mut detail = occurrence_time(occurrence);
                    if let Some(event) = view.agenda.event(occurrence.event)
                        && !event.location.is_empty()
                        && height >= 52.0
                    {
                        detail = format!("{detail}\n{}", event.location);
                    }
                    clip.text(
                        egui::pos2(rect.left() + 8.0, rect.top() + 19.0),
                        egui::Align2::LEFT_TOP,
                        detail,
                        FontId::proportional(ferrite_ui::tokens::typography::CAPTION),
                        style::text_secondary(),
                    );
                }
                if response
                    .on_hover_text(occurrence_tooltip(view, occurrence))
                    .clicked()
                {
                    view.actions.push(Action::EditOccurrence(**occurrence));
                }
            }
            // Heure actuelle : trait rouge sur la colonne d'aujourd'hui.
            if day == today {
                let y = grid.top() + view.now.minute as f32 / 60.0 * HOUR;
                painter.hline(x..=x + column, y, Stroke::new(1.5, style::error()));
                painter.circle_filled(egui::pos2(x, y), 4.0, style::error());
                painter.text(
                    egui::pos2(grid.left() + GUTTER - 10.0, y),
                    egui::Align2::RIGHT_CENTER,
                    clock(view.now.minute),
                    FontId::new(11.0, style::strong_family()),
                    style::error(),
                );
            }
        }
    });
}

#[derive(Debug, Clone, Copy)]
enum BannerItem {
    Event(Occurrence),
    Task(u64),
}

// --- Listes : agenda, recherche, à venir -----------------------------------------------

/// Titre d'un jour dans une liste (« Aujourd'hui », « Mardi 10 novembre 2026 »).
pub fn day_heading(ui: &mut egui::Ui, day: Date, today: Date) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        let ink = if day == today {
            style::accent()
        } else {
            style::heading_text()
        };
        ui.label(
            RichText::new(day.relative(today))
                .size(14.5)
                .family(style::strong_family())
                .color(ink),
        );
        if day != today && (day.days() - today.days()).abs() <= 1 {
            ui.label(
                RichText::new(day.long())
                    .size(12.0)
                    .color(style::text_secondary()),
            );
        }
    });
    ui.add_space(4.0);
}

/// Ligne d'un événement dans une liste : heure, pastille, titre, lieu.
pub fn occurrence_row(ui: &mut egui::Ui, view: &mut Painter, occurrence: &Occurrence) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 44.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, 8.0, style::tint(8));
    }
    let now = view.now;
    let running = occurrence.start <= now && now < occurrence.end;
    let time_width = if width < 420.0 { 0.0 } else { 118.0 };
    if time_width > 0.0 {
        let (first, second) = if occurrence.all_day {
            ("Journée".to_owned(), String::new())
        } else if occurrence.spans_days() {
            (occurrence.start.short(), format!("→ {}", occurrence.end.short()))
        } else {
            (clock(occurrence.start.minute), clock(occurrence.end.minute))
        };
        painter.text(
            egui::pos2(
                rect.left() + 10.0,
                rect.center().y - if second.is_empty() { 0.0 } else { 8.0 },
            ),
            egui::Align2::LEFT_CENTER,
            first,
            FontId::new(12.5, style::strong_family()),
            style::primary_text(),
        );
        if !second.is_empty() {
            painter.text(
                egui::pos2(rect.left() + 10.0, rect.center().y + 8.0),
                egui::Align2::LEFT_CENTER,
                second,
                FontId::proportional(ferrite_ui::tokens::typography::CAPTION),
                style::text_secondary(),
            );
        }
    }
    let color = view.event_color(occurrence.event);
    let bar = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 10.0 + time_width, rect.top() + 8.0),
        egui::vec2(4.0, rect.height() - 16.0),
    );
    painter.rect_filled(bar, 2.0, color);
    let text_left = bar.right() + 12.0;
    let event = view.agenda.event(occurrence.event);
    let title_font = FontId::new(14.0, style::strong_family());
    let mut right_room = 16.0;
    if running {
        let badge = "En cours";
        let galley = painter.layout_no_wrap(
            badge.to_owned(),
            FontId::new(11.0, style::strong_family()),
            style::accent_text(),
        );
        let badge_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - galley.size().x - 26.0, rect.center().y - 10.0),
            egui::vec2(galley.size().x + 16.0, 20.0),
        );
        painter.rect_filled(badge_rect, 10.0, style::accent());
        painter.galley(
            badge_rect.min + egui::vec2(8.0, (20.0 - galley.size().y) / 2.0),
            galley,
            style::accent_text(),
        );
        right_room += badge_rect.width() + 8.0;
    }
    let title = fit(
        painter,
        &view.title(occurrence.event),
        &title_font,
        rect.right() - text_left - right_room,
    );
    let mut subtitle = Vec::new();
    if time_width == 0.0 {
        subtitle.push(occurrence_time(occurrence));
    }
    if let Some(event) = event {
        if !event.location.is_empty() {
            subtitle.push(event.location.clone());
        }
        if let Some(category) = view.agenda.category(event.category) {
            subtitle.push(category.name.clone());
        }
        if event.recurrence.repeats() {
            subtitle.push("↻".to_owned());
        }
    }
    let has_subtitle = !subtitle.is_empty();
    painter.text(
        egui::pos2(text_left, rect.center().y - if has_subtitle { 8.0 } else { 0.0 }),
        egui::Align2::LEFT_CENTER,
        title,
        title_font,
        style::primary_text(),
    );
    if has_subtitle {
        let font = FontId::proportional(12.0);
        let text = fit(
            painter,
            &subtitle.join(" · "),
            &font,
            rect.right() - text_left - right_room,
        );
        painter.text(
            egui::pos2(text_left, rect.center().y + 9.0),
            egui::Align2::LEFT_CENTER,
            text,
            font,
            style::text_secondary(),
        );
    }
    if response.clicked() {
        view.actions.push(Action::EditOccurrence(*occurrence));
    }
}

/// Ligne d'une tâche : rond à cocher, titre, priorité, échéance, liste, événement lié.
pub fn task_row(ui: &mut egui::Ui, view: &mut Painter, task: &Task) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 44.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().rect_filled(rect, 8.0, style::tint(8));
    }
    let color = view.task_color(task);
    let check = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 22.0, rect.center().y),
        egui::vec2(24.0, 24.0),
    );
    let check_response = ui
        .interact(check, ui.scope_id().with(("coche", task.id)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if task.done {
            "Marquer à faire"
        } else {
            "Marquer comme terminée"
        });
    let painter = ui.painter();
    widgets::paint_check(
        painter,
        check.center(),
        9.0,
        task.done,
        color,
        check_response.hovered(),
    );
    let overdue = task.is_overdue(view.now);
    let left = rect.left() + 42.0;
    let mut right = rect.right() - 12.0;
    // À droite : échéance (rouge si en retard).
    if let Some(due) = task.due {
        let mut text = due.relative(view.today());
        if (due.days() - view.today().days()).abs() > 1 {
            text = due.short();
        }
        if let Some(time) = task.due_time {
            text = format!("{text} · {}", clock(time));
        }
        let ink = if overdue {
            style::error()
        } else {
            style::text_secondary()
        };
        let galley = painter.layout_no_wrap(text, FontId::proportional(12.0), ink);
        let at = egui::pos2(right - galley.size().x, rect.center().y - galley.size().y / 2.0);
        right = at.x - 12.0;
        painter.galley(at, galley, ink);
    }
    let mut title = task.title.clone();
    if task.priority == Priority::High {
        title = format!("!! {title}");
    }
    let font = FontId::new(
        14.0,
        if task.done {
            egui::FontFamily::Proportional
        } else {
            style::strong_family()
        },
    );
    let title = fit(painter, &title, &font, right - left);
    let mut subtitle = Vec::new();
    if let Some(list) = view.agenda.list(task.list) {
        subtitle.push(list.name.clone());
    }
    if let Some(event) = task.event.and_then(|id| view.agenda.event(id)) {
        subtitle.push(format!("Pour « {} »", event.title));
    }
    if !task.notes.is_empty() {
        subtitle.push(task.notes.lines().next().unwrap_or_default().to_owned());
    }
    let ink = if task.done {
        style::text_secondary()
    } else {
        style::primary_text()
    };
    let y = rect.center().y - if subtitle.is_empty() { 0.0 } else { 8.0 };
    let galley = painter.layout_no_wrap(title, font, ink);
    let title_width = galley.size().x;
    let at = egui::pos2(left, y - galley.size().y / 2.0);
    painter.galley(at, galley, ink);
    if task.priority == Priority::High && !task.done {
        let mark = painter.layout_no_wrap(
            "!!".to_owned(),
            FontId::new(14.0, style::strong_family()),
            style::error(),
        );
        painter.galley(at, mark, style::error());
    }
    if task.done {
        painter.hline(
            left..=left + title_width,
            y,
            Stroke::new(1.0, style::text_secondary()),
        );
    }
    if !subtitle.is_empty() {
        let font = FontId::proportional(12.0);
        let text = fit(painter, &subtitle.join(" · "), &font, right - left);
        painter.text(
            egui::pos2(left, rect.center().y + 9.0),
            egui::Align2::LEFT_CENTER,
            text,
            font,
            style::text_secondary(),
        );
    }
    if check_response.clicked() {
        view.actions.push(Action::ToggleTask(task.id));
    } else if response.clicked() {
        view.actions.push(Action::EditTask(task.id));
    }
}

/// Message d'une liste vide.
pub fn empty(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(title)
                .size(16.0)
                .family(style::strong_family())
                .color(style::heading_text()),
        );
        ui.add_space(4.0);
        kit::note(ui, detail);
    });
}

/// Vue Agenda : les jours à venir à partir de `from`, avec événements et tâches.
pub fn agenda(ui: &mut egui::Ui, view: &mut Painter, from: Date, days: i64) {
    let to = from.add_days(days);
    let occurrences = view.occurrences(from, to);
    let mut any = false;
    for offset in 0..days {
        let day = from.add_days(offset);
        let items: Vec<Occurrence> = occurrences
            .iter()
            .filter(|o| o.touches(day) && (o.start.date == day || day == from || o.all_day))
            .copied()
            .collect();
        let tasks: Vec<Task> = view
            .agenda
            .tasks_due_on(day)
            .filter(|task| !task.done)
            .cloned()
            .collect();
        if items.is_empty() && tasks.is_empty() {
            continue;
        }
        any = true;
        day_heading(ui, day, view.today());
        for occurrence in &items {
            occurrence_row(ui, view, occurrence);
        }
        for task in &tasks {
            task_row(ui, view, task);
        }
    }
    if !any {
        empty(
            ui,
            "Rien de prévu",
            &format!(
                "Aucun événement du {} au {}.",
                from.medium(),
                to.add_days(-1).medium()
            ),
        );
    }
}

/// Résultats d'une recherche.
pub fn search_results(ui: &mut egui::Ui, view: &mut Painter, query: &str, around: Date) {
    let found = view.agenda.search(query, around);
    let events: Vec<Occurrence> = found
        .iter()
        .filter_map(|found| match found {
            Found::Event(occurrence) => Some(*occurrence),
            Found::Task(_) => None,
        })
        .filter(|occurrence| {
            view.agenda
                .event(occurrence.event)
                .is_some_and(|event| view.visible(event))
        })
        .collect();
    let tasks: Vec<Task> = found
        .iter()
        .filter_map(|found| match found {
            Found::Task(id) => view.agenda.task(*id).cloned(),
            Found::Event(_) => None,
        })
        .collect();
    if events.is_empty() && tasks.is_empty() {
        empty(
            ui,
            "Aucun résultat",
            &format!("Rien ne correspond à « {} ».", query.trim()),
        );
        return;
    }
    kit::note(
        ui,
        &format!(
            "{} · {}",
            date::plural(events.len() as u32, "occurrence", "occurrences"),
            date::plural(tasks.len() as u32, "tâche", "tâches")
        ),
    );
    // Les prochaines d'abord, puis les passées.
    let (upcoming, past): (Vec<Occurrence>, Vec<Occurrence>) = events
        .into_iter()
        .partition(|occurrence| occurrence.end >= view.now);
    let mut last_day = None;
    for occurrence in upcoming.iter().take(150) {
        if last_day != Some(occurrence.start.date) {
            day_heading(ui, occurrence.start.date, view.today());
            last_day = Some(occurrence.start.date);
        }
        occurrence_row(ui, view, occurrence);
    }
    if !tasks.is_empty() {
        kit::heading(ui, "Tâches");
        for task in &tasks {
            task_row(ui, view, task);
        }
    }
    if !past.is_empty() {
        kit::heading(ui, "Passés");
        for occurrence in past.iter().rev().take(50) {
            ui.horizontal(|ui| {
                kit::note(ui, &occurrence.start.date.medium());
            });
            occurrence_row(ui, view, occurrence);
        }
    }
}

/// Bandeau des rappels annoncés et pas encore acquittés.
pub fn waiting_reminders(ui: &mut egui::Ui, view: &mut Painter, waiting: &[Reminder]) {
    if waiting.is_empty() {
        return;
    }
    kit::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Rappels").size(14.0).family(style::strong_family()));
            kit::note(ui, &format!("{} en attente", waiting.len()));
        });
        ui.add_space(4.0);
        for (index, reminder) in waiting.iter().enumerate() {
            let Some((title, body)) = reminder.describe(view.agenda, view.now) else {
                continue;
            };
            if index > 0 {
                kit::separator(ui);
            }
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(13.5).family(style::strong_family()));
                    kit::note(ui, &body);
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let key = reminder.key();
                    if kit::button(ui, "OK", true).clicked() {
                        view.actions.push(Action::Acknowledge(key.clone()));
                    }
                    if kit::button(ui, "Reporter 10 min", false).clicked() {
                        view.actions.push(Action::Snooze(key));
                    }
                });
            });
        }
    });
    ui.add_space(8.0);
}
