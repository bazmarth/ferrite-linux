//! Pièces propres au Calendrier : petit calendrier du mois, champs de date et d'heure,
//! pastilles de couleur, barre et anneau de progression. Couleurs : rôles de `style` et
//! teintes d'accentuation de Ferrite seulement.

use eframe::egui::{self, Color32, FontId, Sense, Stroke};
use ferrite_system::config::Accent;

use super::date::{self, Date, WEEKDAYS_LETTER, clock, parse_time};
use crate::{kit, style};

/// Couleur d'une catégorie ou d'une liste.
pub fn tint(accent: Option<Accent>) -> Color32 {
    style::accent_color(accent.unwrap_or(Accent::Graphite))
}

/// Pastille ronde de couleur.
pub fn dot(ui: &mut egui::Ui, color: Color32, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
    response
}

/// Chevron vers la gauche (`-1.0`) ou la droite (`1.0`), dans `rect`.
pub fn paint_chevron(painter: &egui::Painter, rect: egui::Rect, ink: Color32, direction: f32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.16;
    let stroke = Stroke::new(1.8, ink);
    painter.line_segment(
        [
            c + egui::vec2(direction * -s * 0.5, -s),
            c + egui::vec2(direction * s * 0.5, 0.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            c + egui::vec2(direction * s * 0.5, 0.0),
            c + egui::vec2(direction * -s * 0.5, s),
        ],
        stroke,
    );
}

/// Signe « + » dans `rect`.
pub fn paint_plus(painter: &egui::Painter, rect: egui::Rect, ink: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.2;
    let stroke = Stroke::new(1.8, ink);
    painter.line_segment([c - egui::vec2(s, 0.0), c + egui::vec2(s, 0.0)], stroke);
    painter.line_segment([c - egui::vec2(0.0, s), c + egui::vec2(0.0, s)], stroke);
}

/// Trois traits (afficher ou masquer la colonne).
pub fn paint_sidebar_symbol(painter: &egui::Painter, rect: egui::Rect, ink: Color32) {
    let r = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 13.0));
    let stroke = Stroke::new(1.5, ink);
    painter.rect_stroke(r, 3.0, stroke, egui::StrokeKind::Middle);
    painter.line_segment(
        [
            egui::pos2(r.left() + 5.5, r.top()),
            egui::pos2(r.left() + 5.5, r.bottom()),
        ],
        stroke,
    );
}

pub fn paint_check(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    done: bool,
    color: Color32,
    hovered: bool,
) {
    if done {
        painter.circle_filled(center, radius, color);
        let stroke = Stroke::new(1.8, style::background());
        let a = center + egui::vec2(-radius * 0.45, 0.0);
        let b = center + egui::vec2(-radius * 0.1, radius * 0.38);
        let c = center + egui::vec2(radius * 0.5, -radius * 0.35);
        painter.line_segment([a, b], stroke);
        painter.line_segment([b, c], stroke);
    } else {
        if hovered {
            painter.circle_filled(center, radius, color.gamma_multiply(0.15));
        }
        painter.circle_stroke(center, radius - 0.75, Stroke::new(1.5, color));
    }
}

/// Barre de progression horizontale.
pub fn progress_bar(ui: &mut egui::Ui, fraction: f32, width: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, style::track());
    let fraction = kit::glide(
        ui,
        ui.scope_id()
            .with(("progression", rect.left() as i32, rect.top() as i32)),
        fraction.clamp(0.0, 1.0),
    );
    if fraction > 0.0 {
        let done = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction, rect.height()));
        ui.painter().rect_filled(done, 3.0, color);
    }
}

/// Anneau de progression, le pourcentage au centre.
pub fn progress_ring(ui: &mut egui::Ui, fraction: f32, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    let center = rect.center();
    let radius = size / 2.0 - 4.0;
    let painter = ui.painter();
    painter.circle_stroke(center, radius, Stroke::new(6.0, style::track()));
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        let steps = (64.0 * fraction).ceil().max(2.0) as usize;
        let points: Vec<egui::Pos2> = (0..=steps)
            .map(|step| {
                let angle = -std::f32::consts::FRAC_PI_2
                    + std::f32::consts::TAU * fraction * step as f32 / steps as f32;
                center + radius * egui::vec2(angle.cos(), angle.sin())
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(6.0, color)));
    }
    kit::tabular_text(
        painter,
        center,
        egui::Align2::CENTER_CENTER,
        &format!("{} %", (fraction * 100.0).round() as u32),
        FontId::new(15.0, style::strong_family()),
        style::primary_text(),
    );
}

/// Petit calendrier d'un mois : renvoie le jour cliqué. `shown` est le mois affiché (il
/// change avec les flèches), `marked` dit si un jour a des événements.
pub fn mini_month(
    ui: &mut egui::Ui,
    shown: &mut Date,
    selected: Date,
    today: Date,
    marked: impl Fn(Date) -> bool,
) -> Option<Date> {
    let width = ui.available_width().min(260.0);
    let cell = (width / 7.0).floor();
    let mut clicked = None;
    // Titre et flèches.
    let (head, _) = ui.allocate_exact_size(egui::vec2(cell * 7.0, 28.0), Sense::hover());
    ui.painter().text(
        egui::pos2(head.left() + 4.0, head.center().y),
        egui::Align2::LEFT_CENTER,
        date::capitalize(&shown.month_title()),
        FontId::new(13.5, style::strong_family()),
        style::primary_text(),
    );
    for (rank, direction) in [(1.0, -1.0_f32), (0.0, 1.0)] {
        let rect = egui::Rect::from_center_size(
            egui::pos2(head.right() - 12.0 - rank * 26.0, head.center().y),
            egui::vec2(24.0, 24.0),
        );
        let hint = if direction < 0.0 {
            "Mois précédent"
        } else {
            "Mois suivant"
        };
        if kit::tool_button_at(ui, rect, false, hint, |painter, rect, ink| {
            paint_chevron(painter, rect, ink, direction);
        })
        .clicked()
        {
            *shown = shown.first_of_month().add_months(direction as i32);
        }
    }
    let (letters, _) = ui.allocate_exact_size(egui::vec2(cell * 7.0, 20.0), Sense::hover());
    for (index, letter) in WEEKDAYS_LETTER.iter().enumerate() {
        ui.painter().text(
            egui::pos2(letters.left() + cell * (index as f32 + 0.5), letters.center().y),
            egui::Align2::CENTER_CENTER,
            *letter,
            FontId::proportional(10.5),
            style::text_secondary(),
        );
    }
    let first = shown.first_of_month().week_start();
    let (grid, _) = ui.allocate_exact_size(egui::vec2(cell * 7.0, cell * 6.0), Sense::hover());
    for index in 0..42 {
        let day = first.add_days(index);
        let rect = egui::Rect::from_min_size(
            grid.min + egui::vec2(cell * (index % 7) as f32, cell * (index / 7) as f32),
            egui::vec2(cell, cell),
        );
        let response = ui
            .interact(
                rect,
                ui.scope_id().with(("mini-jour", day.days())),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let disc = rect.shrink(3.0);
        let in_month = day.month == shown.month;
        let ink = if day == today {
            style::accent_text()
        } else if !in_month {
            style::tertiary_text()
        } else {
            style::primary_text()
        };
        if day == today {
            ui.painter()
                .circle_filled(disc.center(), disc.width() / 2.0, style::accent());
        } else if day == selected {
            ui.painter()
                .circle_filled(disc.center(), disc.width() / 2.0, style::sidebar_selected());
        } else if response.hovered() {
            ui.painter()
                .circle_filled(disc.center(), disc.width() / 2.0, style::tint(12));
        }
        ui.painter().text(
            disc.center() - egui::vec2(0.0, 1.0),
            egui::Align2::CENTER_CENTER,
            day.day.to_string(),
            if day == today || day == selected {
                FontId::new(12.0, style::strong_family())
            } else {
                FontId::proportional(12.0)
            },
            ink,
        );
        if in_month && day != today && marked(day) {
            ui.painter().circle_filled(
                egui::pos2(disc.center().x, disc.bottom() - 2.5),
                1.6,
                style::text_secondary(),
            );
        }
        if response.clicked() {
            clicked = Some(day);
        }
    }
    clicked
}

/// Champ de date : saisie libre (« 9/11 », « demain »…) et petit calendrier en menu.
#[derive(Debug, Clone)]
pub struct DateInput {
    pub value: Date,
    text: String,
    shown: Date,
}

impl DateInput {
    pub fn new(value: Date) -> Self {
        Self {
            value,
            text: value.input(),
            shown: value,
        }
    }

    pub fn set(&mut self, value: Date) {
        *self = Self::new(value);
    }

    /// Vrai si la saisie est lisible.
    pub fn valid(&self, today: Date) -> bool {
        Date::parse_input(&self.text, today).is_some()
    }

    /// Dessine le champ ; vrai si la date a changé.
    pub fn show(&mut self, ui: &mut egui::Ui, id: &str, today: Date) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let valid = self.valid(today);
            if !valid {
                ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.0, style::error());
                ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(1.0, style::error());
            }
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.text)
                    .id_salt(("date", id))
                    .desired_width(96.0)
                    .hint_text("jj/mm/aaaa"),
            );
            let field = if valid {
                field.on_hover_text(self.value.long())
            } else {
                field.on_hover_text("Date illisible : « 9/11/2026 », « 9/11 », « demain »…")
            };
            if field.changed()
                && let Some(date) = Date::parse_input(&self.text, today)
                && date != self.value
            {
                self.value = date;
                self.shown = date;
                changed = true;
            }
            if field.lost_focus() && valid {
                self.text = self.value.input();
            }
            let button = kit::tool_button(
                ui,
                28.0,
                false,
                "Choisir dans le calendrier",
                |painter, rect, ink| {
                    paint_calendar_symbol(painter, rect, ink);
                },
            );
            egui::Popup::menu(&button)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.set_width(230.0);
                    if let Some(date) = mini_month(ui, &mut self.shown, self.value, today, |_| false) {
                        self.value = date;
                        self.text = date.input();
                        changed = true;
                        ui.close();
                    }
                });
        });
        changed
    }
}

/// Petit calendrier dessiné au trait.
pub fn paint_calendar_symbol(painter: &egui::Painter, rect: egui::Rect, ink: Color32) {
    let r = egui::Rect::from_center_size(rect.center() + egui::vec2(0.0, 1.0), egui::vec2(14.0, 13.0));
    let stroke = Stroke::new(1.4, ink);
    painter.rect_stroke(r, 2.5, stroke, egui::StrokeKind::Middle);
    painter.line_segment(
        [
            egui::pos2(r.left(), r.top() + 4.0),
            egui::pos2(r.right(), r.top() + 4.0),
        ],
        stroke,
    );
    for x in [r.left() + 4.0, r.right() - 4.0] {
        painter.line_segment(
            [egui::pos2(x, r.top() - 2.0), egui::pos2(x, r.top() + 1.5)],
            stroke,
        );
    }
}

/// Champ d'heure : « 9h30 », « 14:00 »…
#[derive(Debug, Clone)]
pub struct TimeInput {
    pub value: u32,
    text: String,
}

impl TimeInput {
    pub fn new(value: u32) -> Self {
        Self {
            value,
            text: clock(value),
        }
    }

    pub fn set(&mut self, value: u32) {
        *self = Self::new(value);
    }

    pub fn valid(&self) -> bool {
        parse_time(&self.text).is_some()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, id: &str) -> bool {
        let valid = self.valid();
        let mut changed = false;
        ui.scope(|ui| {
            if !valid {
                ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.0, style::error());
                ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(1.0, style::error());
            }
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.text)
                    .id_salt(("heure", id))
                    .desired_width(52.0)
                    .hint_text("hh:mm"),
            );
            if field.changed()
                && let Some(value) = parse_time(&self.text)
                && value != self.value
            {
                self.value = value;
                changed = true;
            }
            if field.lost_focus() && valid {
                self.text = clock(self.value);
            }
            field.on_hover_text("Heure : « 9h30 », « 14:00 »…");
        });
        changed
    }
}

/// Choix d'une teinte d'accentuation parmi les neuf de Ferrite.
pub fn color_picker(ui: &mut egui::Ui, selected: &mut Accent) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for accent in Accent::ALL {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::click());
            let response = response.on_hover_text(accent.label());
            ui.painter()
                .circle_filled(rect.center(), 9.0, style::accent_color(accent));
            if *selected == accent {
                ui.painter()
                    .circle_stroke(rect.center(), 10.5, Stroke::new(2.0, style::primary_text()));
            }
            if response.clicked() && *selected != accent {
                *selected = accent;
                changed = true;
            }
        }
    });
    changed
}

/// Libellé de champ, aligné à gauche, de largeur fixe.
pub fn field_label(ui: &mut egui::Ui, text: &str, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(13.0),
        style::text_secondary(),
    );
}
