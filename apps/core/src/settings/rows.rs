//! Composants des pages de Paramètres, d'après les planches : bandeau, groupes de lignes et
//! commandes (bouton, liste, interrupteur, curseur, choix unique, segments).

use eframe::egui::{self, Color32, Frame, Margin, RichText, Sense, Stroke, text::LayoutJob};

use crate::style::{self, Icons};

const ROW_HEIGHT: f32 = 48.0;
const ROW_PADDING: f32 = 14.0;
/// Écart entre deux groupes d'une colonne.
pub const GROUP_GAP: f32 = 18.0;

/// Ce qui précède le libellé d'une ligne.
#[derive(Clone, Copy, Default)]
pub enum Lead {
    #[default]
    None,
    /// Pastille colorée de 30 px et son symbole blanc.
    Tile(&'static str, u32),
    /// Barres de signal, de 1 à 4.
    Bars(u8),
}

/// Une ligne de groupe.
#[derive(Clone, Copy)]
pub struct Row<'a> {
    pub label: &'a str,
    pub hint: &'a str,
    /// L'indication est une bonne nouvelle (« Connecté ») : elle s'écrit en vert.
    pub ok: bool,
    pub bold: bool,
    pub lead: Lead,
    /// Cadenas des réseaux protégés.
    pub lock: bool,
    /// Grisée : la commande existe sur la maquette, mais pas encore dans le système.
    pub enabled: bool,
    /// Toute la ligne se clique.
    pub clickable: bool,
}

impl<'a> Row<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            hint: "",
            ok: false,
            bold: false,
            lead: Lead::None,
            lock: false,
            enabled: true,
            clickable: false,
        }
    }

    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    pub fn ok(mut self, ok: bool) -> Self {
        self.ok = ok;
        self
    }

    pub fn bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }

    pub fn lead(mut self, lead: Lead) -> Self {
        self.lead = lead;
        self
    }

    pub fn lock(mut self, lock: bool) -> Self {
        self.lock = lock;
        self
    }

    pub fn clickable(mut self) -> Self {
        self.clickable = true;
        self
    }

    /// Ligne de la maquette sans fonction réelle pour l'instant.
    pub fn unavailable(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// Groupe en cours de dessin : son titre est posé, ses lignes s'ajoutent.
pub struct Group<'a> {
    ui: &'a mut egui::Ui,
    icons: &'a mut Icons,
    first: bool,
}

impl Group<'_> {
    /// Ajoute une ligne : `right` y pose ses commandes, de droite à gauche. Renvoie son
    /// résultat et la réponse de la ligne (clic, menu contextuel).
    pub fn row<R>(&mut self, row: Row<'_>, right: impl FnOnce(&mut egui::Ui) -> R) -> (R, egui::Response) {
        let ui = &mut *self.ui;
        let sense = if row.clickable && row.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), sense);
        if !self.first {
            ui.painter().hline(
                rect.x_range(),
                rect.top() + 0.5,
                Stroke::new(1.0, style::separator()),
            );
        }
        self.first = false;
        if row.clickable && row.enabled && response.hovered() {
            ui.painter().rect_filled(rect.shrink(1.0), 6.0, style::tint(8));
        }
        let inner = rect.shrink2(egui::vec2(ROW_PADDING, 6.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        if !row.enabled {
            child.disable();
        }
        child.spacing_mut().item_spacing.x = 10.0;
        let result = right(&mut child);
        if row.lock {
            let (lock, _) = child.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
            self.icons.paint(
                &child,
                "glyph-lock",
                lock,
                style::IconStyle::Tinted(Color32::from_rgb(0x6B, 0x72, 0x80)),
            );
        }
        // Reste de la ligne, à gauche des commandes.
        let room = child.available_width();
        let mut x = inner.left();
        match row.lead {
            Lead::None => {}
            Lead::Tile(glyph, color) => {
                let tile = egui::Rect::from_center_size(
                    egui::pos2(x + 15.0, inner.center().y),
                    egui::vec2(30.0, 30.0),
                );
                child.painter().rect_filled(tile, 8.0, rgb(color));
                self.icons.paint(
                    &child,
                    glyph,
                    tile.shrink(6.5),
                    style::IconStyle::Tinted(Color32::WHITE),
                );
                x += 42.0;
            }
            Lead::Bars(level) => {
                let bottom = inner.center().y + 8.0;
                for (index, height) in [4.0, 7.0, 10.0, 13.0].into_iter().enumerate() {
                    let bar = egui::Rect::from_min_size(
                        egui::pos2(x + 4.0 + index as f32 * 6.0, bottom - height),
                        egui::vec2(4.0, height),
                    );
                    let color = if (index as u8) < level {
                        style::primary_text()
                    } else {
                        style::themed(rgb(0xD5D9E0), rgb(0x4B5360))
                    };
                    child.painter().rect_filled(bar, 1.0, color);
                }
                x += 42.0;
            }
        }
        let width = (room - (x - inner.left()) - 6.0).max(10.0);
        let font = if row.bold {
            egui::FontId::new(14.0, style::strong_family())
        } else {
            egui::FontId::proportional(14.0)
        };
        let title = one_line(&child, row.label, font, style::primary_text(), width);
        let hint_color = if row.ok {
            style::themed(rgb(0x1F7A4D), rgb(0x4ADE80))
        } else {
            style::text_secondary()
        };
        if row.hint.is_empty() {
            let y = inner.center().y - title.size().y / 2.0;
            child
                .painter()
                .galley(egui::pos2(x, y), title, style::primary_text());
        } else {
            let hint = one_line(
                &child,
                row.hint,
                egui::FontId::proportional(12.0),
                hint_color,
                width,
            );
            let total = title.size().y + 1.0 + hint.size().y;
            let top = inner.center().y - total / 2.0;
            let below = top + title.size().y + 1.0;
            child
                .painter()
                .galley(egui::pos2(x, top), title, style::primary_text());
            child.painter().galley(egui::pos2(x, below), hint, hint_color);
        }
        let response = if row.enabled {
            response
        } else {
            response.on_hover_text("Pas encore disponible")
        };
        (result, response)
    }

    /// Ligne sans commande.
    pub fn text(&mut self, row: Row<'_>) -> egui::Response {
        self.row(row, |_| ()).1
    }
}

/// Texte sur une seule ligne, coupé avec « … » s'il dépasse.
fn one_line(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ui.painter().layout_job(job)
}

/// Bandeau de la page : pastille de 48 px, titre, sous-titre et interrupteur.
pub fn hero(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    (glyph, color): (&'static str, u32),
    title: &str,
    subtitle: &str,
    toggle: Option<(&mut bool, bool)>,
) -> bool {
    let mut changed = false;
    Frame::new()
        .fill(style::header_color())
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(14)
        .inner_margin(Margin::symmetric(18, 16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let (tile, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), Sense::hover());
                ui.painter().rect_filled(tile, 13.0, rgb(color));
                icons.paint(
                    ui,
                    glyph,
                    tile.shrink(11.0),
                    style::IconStyle::Tinted(Color32::WHITE),
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add_space(3.0);
                    ui.label(RichText::new(title).size(18.0).family(style::strong_family()));
                    ui.label(RichText::new(subtitle).size(13.0).color(style::text_secondary()));
                });
                if let Some((on, enabled)) = toggle {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        changed = style::switch_sized(ui, on, enabled, egui::vec2(52.0, 30.0)).changed();
                    });
                }
            });
        });
    changed
}

/// Groupe de lignes : titre puis carte aux coins arrondis.
pub fn group<R>(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    title: &str,
    add: impl FnOnce(&mut Group<'_>) -> R,
) -> R {
    ui.label(
        RichText::new(title)
            .size(13.0)
            .family(style::strong_family())
            .color(style::themed(rgb(0x374151), rgb(0xD1D5DB))),
    );
    ui.add_space(8.0);
    let result = Frame::new()
        .fill(style::background())
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(&mut Group {
                ui,
                icons,
                first: true,
            })
        })
        .inner;
    ui.add_space(GROUP_GAP);
    result
}

/// Bouton de ligne : gris, ou couleur d'accent pour l'action principale.
pub fn button(ui: &mut egui::Ui, text: &str, primary: bool) -> egui::Response {
    let (fill, ink) = if primary {
        (style::accent(), style::accent_text())
    } else {
        (style::control_background(), style::primary_text())
    };
    ui.add(
        egui::Button::new(RichText::new(text).size(13.0).color(ink))
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(8)
            .min_size(egui::vec2(0.0, 30.0)),
    )
}

/// Liste déroulante : renvoie vrai si le choix a changé.
pub fn pick<T: PartialEq + Copy>(ui: &mut egui::Ui, id: &str, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    let selected = options
        .iter()
        .find(|(option, _)| option == value)
        .map_or("", |(_, label)| *label);
    egui::ComboBox::from_id_salt(id)
        .selected_text(RichText::new(selected).size(13.0))
        .show_ui(ui, |ui| {
            for (option, label) in options {
                changed |= ui.selectable_value(value, *option, *label).changed();
            }
        });
    changed
}

/// Interrupteur de ligne, 44 × 26 px.
pub fn switch(ui: &mut egui::Ui, on: &mut bool) -> bool {
    style::switch_sized(ui, on, true, egui::vec2(44.0, 26.0)).changed()
}

/// Curseur de 200 px et sa valeur à droite. La réponse dit si la valeur a changé et si le
/// glissement est fini.
pub fn slider(ui: &mut egui::Ui, value: &mut u8, text: &str) -> egui::Response {
    let (label, _) = ui.allocate_exact_size(egui::vec2(44.0, 20.0), Sense::hover());
    ui.painter().text(
        label.right_center(),
        egui::Align2::RIGHT_CENTER,
        text,
        egui::FontId::proportional(12.0),
        style::text_secondary(),
    );
    // Le curseur se raccourcit dans une colonne étroite, pour que le libellé reste lisible.
    let track = (ui.available_width() - 130.0).clamp(80.0, 146.0);
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(track, 20.0), Sense::click_and_drag());
    if let Some(pointer) = response.interact_pointer_pos() {
        let span = (rect.width() - 20.0).max(1.0);
        let next = (((pointer.x - rect.left() - 10.0) / span).clamp(0.0, 1.0) * 100.0).round() as u8;
        if next != *value {
            *value = next;
            response.mark_changed();
        }
    }
    let center = rect.center().y;
    let x = rect.left() + 10.0 + (rect.width() - 20.0) * f32::from(*value) / 100.0;
    let painter = ui.painter();
    let rail = egui::Rect::from_min_max(
        egui::pos2(rect.left(), center - 3.0),
        egui::pos2(rect.right(), center + 3.0),
    );
    painter.rect_filled(rail, 3.0, style::themed(rgb(0xD5D9E0), rgb(0x4B5360)));
    let done = egui::Rect::from_min_max(rail.min, egui::pos2(x, rail.bottom()));
    painter.rect_filled(done, 3.0, style::accent());
    painter.circle_filled(egui::pos2(x, center + 1.0), 10.0, Color32::from_black_alpha(50));
    painter.circle_filled(egui::pos2(x, center), 10.0, Color32::WHITE);
    response
}

/// Choix unique dans une liste.
pub fn radio(ui: &mut egui::Ui, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
    let painter = ui.painter();
    if on {
        painter.circle_filled(rect.center(), 10.0, style::accent());
        painter.circle_filled(rect.center(), 4.0, style::background());
    } else {
        painter.circle_filled(rect.center(), 10.0, style::background());
        painter.circle_stroke(rect.center(), 9.0, Stroke::new(2.0, rgb(0xAEB6C2)));
    }
    response
}

/// Vu-mètre à 16 cases.
pub fn meter(ui: &mut egui::Ui, level: usize) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0 * 11.0 - 3.0, 14.0), Sense::hover());
    for index in 0..16 {
        let cell = egui::Rect::from_min_size(
            rect.min + egui::vec2(index as f32 * 11.0, 0.0),
            egui::vec2(8.0, 14.0),
        );
        let color = if index >= level {
            style::separator()
        } else if index < 11 {
            rgb(0x1F9D63)
        } else {
            rgb(0xFF9F0A)
        };
        ui.painter().rect_filled(cell, 2.0, color);
    }
}

/// Contrôle à segments : renvoie le segment cliqué.
pub fn segments(ui: &mut egui::Ui, options: &[&str], selected: usize) -> Option<usize> {
    let font = egui::FontId::proportional(13.0);
    let ink = style::primary_text();
    let widths: Vec<f32> = options
        .iter()
        .map(|option| {
            ui.painter()
                .layout_no_wrap((*option).to_owned(), font.clone(), ink)
                .size()
                .x
                + 24.0
        })
        .collect();
    let width = widths.iter().sum::<f32>() + 6.0 + 2.0 * widths.len().saturating_sub(1) as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 32.0), Sense::hover());
    ui.painter().rect_filled(rect, 9.0, style::control_background());
    let mut clicked = None;
    let mut x = rect.left() + 3.0;
    for (index, (option, segment)) in options.iter().zip(&widths).enumerate() {
        let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 3.0), egui::vec2(*segment, 26.0));
        let response = ui.interact(cell, ui.scope_id().with(("segment", index)), Sense::click());
        if response.clicked() {
            clicked = Some(index);
        }
        let on = index == selected;
        if on {
            ui.painter().rect_filled(cell, 7.0, style::background());
        }
        let font = if on {
            egui::FontId::new(13.0, style::strong_family())
        } else {
            font.clone()
        };
        ui.painter()
            .text(cell.center(), egui::Align2::CENTER_CENTER, option, font, ink);
        x += segment + 2.0;
    }
    clicked
}

/// État en cours, avec un cercle qui tourne.
pub fn status(ui: &mut egui::Ui, text: &str, running: bool) {
    ui.label(RichText::new(text).size(12.0).color(style::text_secondary()));
    if running {
        ui.add(egui::Spinner::new().size(16.0));
    }
}

fn rgb(hex: u32) -> Color32 {
    let [_, r, g, b] = hex.to_be_bytes();
    Color32::from_rgb(r, g, b)
}
