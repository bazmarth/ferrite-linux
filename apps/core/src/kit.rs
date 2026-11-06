//! Pièces communes des applications Ferrite : cartes, titres, boutons, interrupteurs,
//! curseurs, segments, chiffres stables. Elles suivent le thème clair ou sombre et la
//! couleur d'accent (voir `style`), et les mesures de `ferrite_ui::tokens`.
//!
//! Une nouvelle application les prend ici plutôt que de les redessiner : c'est ce qui
//! garde toutes les fenêtres semblables. L'application « galerie »
//! (`ferrite-apps galerie`) les montre toutes, en clair et en sombre.

use eframe::egui::{self, Color32, FontId, Frame, Margin, RichText, Sense, Stroke};
use ferrite_ui::{
    Easing,
    tokens::{control, motion, radius, typography},
};

use crate::style;

/// Valeur qui glisse vers `target` en [`motion::NORMAL`], avec la courbe commune au
/// bureau (`Easing::EaseOut` : départ rapide, arrivée douce). « Réduire les animations »
/// la rend immédiate.
pub fn glide(ui: &egui::Ui, id: egui::Id, target: f32) -> f32 {
    let now = ui.input(|input| input.time);
    let duration = if ui.style().animation_time == 0.0 {
        0.0
    } else {
        motion::NORMAL.as_secs_f64()
    };
    let at = |(from, to, start): (f32, f32, f64), now: f64| {
        let t = if duration == 0.0 {
            1.0
        } else {
            (now - start) / duration
        };
        from + (to - from) * Easing::EaseOut.apply(t as f32)
    };
    let course = ui.ctx().data_mut(|data| {
        let course = data.get_temp_mut_or_insert_with(id, || (target, target, now));
        if course.1 != target {
            *course = (at(*course, now), target, now);
        }
        *course
    });
    if now - course.2 < duration {
        ui.ctx().request_repaint();
    }
    at(course, now)
}

/// Rayon des cartes douces (fond gris, sans bord).
const SOFT_RADIUS: u8 = 14;

/// Carte bordée sur le fond de la fenêtre, pour une liste (réseaux, appareils, réglages).
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new()
        .fill(style::background())
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(radius::PANEL)
        .inner_margin(Margin::symmetric(20, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Carte douce : fond gris léger, sans bord (horloges du monde, tours du chronomètre).
pub fn soft_card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new()
        .fill(style::sidebar_color())
        .corner_radius(SOFT_RADIUS)
        .inner_margin(Margin::symmetric(16, 4))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Titre de partie, en gras.
pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(20.0);
    ui.label(RichText::new(text).size(17.0).family(style::strong_family()));
    ui.add_space(8.0);
}

/// Texte d'explication, en gris.
pub fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(typography::SMALL)
            .color(style::text_secondary()),
    );
}

/// Petit titre de section : capitales espacées, gris (« HEURE LOCALE »).
pub fn caption(painter: &egui::Painter, at: egui::Pos2, text: &str) {
    let mut x = at.x;
    for character in text.to_uppercase().chars() {
        let galley = painter.layout_no_wrap(
            character.to_string(),
            FontId::new(11.0, style::strong_family()),
            style::text_secondary(),
        );
        let width = galley.size().x;
        painter.galley(egui::pos2(x, at.y), galley, style::text_secondary());
        x += width + 0.9;
    }
}

/// Trait fin sur toute la largeur, entre deux lignes d'une carte.
pub fn separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, style::separator()),
    );
}

/// Bouton : gris, ou couleur d'accent pour l'action principale.
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
            .corner_radius(radius::CONTROL)
            .min_size(egui::vec2(0.0, control::HEIGHT)),
    )
}

/// Grand bouton de la largeur `width` : gris, accent, ou rouge pour arrêter.
pub fn wide_button(ui: &mut egui::Ui, width: f32, text: &str, tone: Tone, enabled: bool) -> egui::Response {
    let (fill, ink) = match tone {
        Tone::Plain => (style::control_background(), style::primary_text()),
        Tone::Accent => (style::accent(), style::accent_text()),
        Tone::Danger => (style::error(), Color32::WHITE),
    };
    let ink = if enabled { ink } else { ink.gamma_multiply(0.45) };
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).size(15.0).color(ink))
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(12)
            .min_size(egui::vec2(width, 44.0)),
    )
}

/// Teinte d'un bouton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Accent,
    Danger,
}

/// Côté d'un bouton d'outil (barre du haut d'une fenêtre).
pub const TOOL: f32 = 34.0;
/// Voile d'un bouton d'outil survolé.
pub const TOOL_HOVER: u8 = 12;

/// Bouton d'outil carré de `size` à la place courante : fond discret au survol, sélection
/// teintée quand il est actif. `paint` dessine le symbole dans le rectangle, à l'encre
/// donnée. Tous les boutons d'icône des applications passent par ici.
pub fn tool_button(
    ui: &mut egui::Ui,
    size: f32,
    active: bool,
    hint: &str,
    paint: impl FnOnce(&egui::Painter, egui::Rect, Color32),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
    paint_tool(ui, rect, &response, active, paint);
    response.on_hover_text(hint)
}

/// Comme [`tool_button`], posé sur `rect` (barre flottante, mise en page à la main).
pub fn tool_button_at(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    active: bool,
    hint: &str,
    paint: impl FnOnce(&egui::Painter, egui::Rect, Color32),
) -> egui::Response {
    let response = ui.interact(rect, ui.scope_id().with(("outil", hint)), Sense::click());
    paint_tool(ui, rect, &response, active, paint);
    response.on_hover_text(hint)
}

fn paint_tool(
    ui: &egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    active: bool,
    paint: impl FnOnce(&egui::Painter, egui::Rect, Color32),
) {
    let (fill, ink) = if active {
        (style::sidebar_selected(), style::sidebar_selected_text())
    } else if response.hovered() {
        (style::tint(TOOL_HOVER), style::primary_text())
    } else {
        (Color32::TRANSPARENT, style::text_secondary())
    };
    ui.painter().rect_filled(rect, radius::PANEL, fill);
    paint(ui.painter(), rect, ink);
}

/// Interrupteur, 44 × 26 px ; vrai s'il vient de changer.
pub fn switch(ui: &mut egui::Ui, on: &mut bool) -> bool {
    style::switch_sized(ui, on, true, egui::vec2(44.0, 26.0)).changed()
}

/// Curseur de 0 à 100 et sa valeur à gauche. La réponse dit si la valeur a changé et si
/// le glissement est fini.
pub fn slider(ui: &mut egui::Ui, value: &mut u8, text: &str) -> egui::Response {
    let (label, _) = ui.allocate_exact_size(egui::vec2(44.0, 20.0), Sense::hover());
    ui.painter().text(
        label.right_center(),
        egui::Align2::RIGHT_CENTER,
        text,
        FontId::proportional(typography::SMALL),
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
    painter.rect_filled(rail, 3.0, style::track());
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
        painter.circle_stroke(rect.center(), 9.0, Stroke::new(2.0, style::track()));
    }
    response
}

/// Mesures d'un contrôle à segments.
const SEGMENT_PAD: f32 = 24.0;

/// Contrôle à segments, à la place courante : renvoie le segment cliqué.
pub fn segments(ui: &mut egui::Ui, options: &[&str], selected: usize) -> Option<usize> {
    let font = FontId::proportional(13.0);
    let widths: Vec<f32> = options
        .iter()
        .map(|option| {
            ui.painter()
                .layout_no_wrap((*option).to_owned(), font.clone(), style::primary_text())
                .size()
                .x
                + SEGMENT_PAD
        })
        .collect();
    let width = widths.iter().sum::<f32>() + 6.0 + 2.0 * widths.len().saturating_sub(1) as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 32.0), Sense::hover());
    segments_in(ui, rect, options, &widths, selected)
}

/// Contrôle à segments de largeur égale `segment`, centré en `center` (barre d'une
/// fenêtre) : renvoie le segment cliqué.
pub fn segments_at(
    ui: &mut egui::Ui,
    center: egui::Pos2,
    segment: f32,
    options: &[&str],
    selected: usize,
) -> Option<usize> {
    let widths = vec![segment; options.len()];
    let width = segment * options.len() as f32 + 6.0 + 2.0 * options.len().saturating_sub(1) as f32;
    let rect = egui::Rect::from_center_size(center, egui::vec2(width, 32.0));
    segments_in(ui, rect, options, &widths, selected)
}

fn segments_in(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    options: &[&str],
    widths: &[f32],
    selected: usize,
) -> Option<usize> {
    ui.painter().rect_filled(rect, 9.0, style::control_background());
    let cells: Vec<egui::Rect> = widths
        .iter()
        .scan(rect.left() + 3.0, |x, segment| {
            let cell =
                egui::Rect::from_min_size(egui::pos2(*x, rect.top() + 3.0), egui::vec2(*segment, 26.0));
            *x += segment + 2.0;
            Some(cell)
        })
        .collect();
    // Le fond du segment choisi glisse d'un segment à l'autre.
    if let Some(target) = cells.get(selected) {
        let id = ui.scope_id().with("segment-choisi");
        let left = glide(ui, id.with("gauche"), target.left());
        let width = glide(ui, id.with("largeur"), target.width());
        let pill =
            egui::Rect::from_min_size(egui::pos2(left, target.top()), egui::vec2(width, target.height()));
        ui.painter().rect_filled(pill, 7.0, style::segment_on());
    }
    let mut clicked = None;
    for (index, (option, cell)) in options.iter().zip(&cells).enumerate() {
        let cell = *cell;
        let response = ui.interact(cell, ui.scope_id().with(("segment", index)), Sense::click());
        if response.clicked() {
            clicked = Some(index);
        }
        let on = index == selected;
        if !on && response.hovered() {
            ui.painter().rect_filled(cell, 7.0, style::control_hover());
        }
        let (font, ink) = if on {
            (FontId::new(13.0, style::strong_family()), style::primary_text())
        } else {
            (FontId::proportional(13.0), style::text_secondary())
        };
        ui.painter()
            .text(cell.center(), egui::Align2::CENTER_CENTER, option, font, ink);
    }
    clicked
}

/// État en cours, avec un cercle qui tourne.
pub fn status(ui: &mut egui::Ui, text: &str, running: bool) {
    ui.label(
        RichText::new(text)
            .size(typography::SMALL)
            .color(style::text_secondary()),
    );
    if running {
        ui.add(egui::Spinner::new().size(16.0));
    }
}

/// Écrit `text` en chiffres de largeur égale (comme des chiffres tabulaires) : un « 1 »
/// prend la place d'un « 0 », et un nombre centré ne bouge plus quand il change.
/// Renvoie le rectangle occupé.
pub fn tabular_text(
    painter: &egui::Painter,
    at: egui::Pos2,
    align: egui::Align2,
    text: &str,
    font: FontId,
    color: Color32,
) -> egui::Rect {
    let galley = |text: String| painter.layout_no_wrap(text, font.clone(), color);
    let cell = ('0'..='9')
        .map(|digit| galley(digit.to_string()).size().x)
        .fold(0.0_f32, f32::max);
    let parts: Vec<_> = text
        .chars()
        .map(|character| {
            let piece = galley(character.to_string());
            let width = if character.is_ascii_digit() {
                cell
            } else {
                piece.size().x
            };
            (piece, width)
        })
        .collect();
    let width: f32 = parts.iter().map(|(_, width)| width).sum();
    let height = parts
        .iter()
        .map(|(piece, _)| piece.size().y)
        .fold(0.0_f32, f32::max);
    let rect = align.anchor_size(at, egui::vec2(width, height));
    let mut x = rect.left();
    for (piece, advance) in parts {
        let offset = (advance - piece.size().x) / 2.0;
        painter.galley(egui::pos2(x + offset, rect.top()), piece, color);
        x += advance;
    }
    rect
}
