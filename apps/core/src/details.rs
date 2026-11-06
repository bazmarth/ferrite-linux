//! Dialogues de détails, d'après la planche « Dialogues de détails » : réseau Wi-Fi,
//! appareil Bluetooth et propriétés d'un fichier partagent leur cadre (600 points, coins
//! de 18, voile derrière), leur en-tête, leurs tuiles, leurs sections encadrées et leur
//! pied de page.

use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Frame, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, vec2,
};

use crate::style;

/// Largeur des dialogues et marge de leurs bords.
pub const WIDTH: f32 = 600.0;
const PAD: f32 = 22.0;
/// Temps pendant lequel un bouton « Copier » montre une coche.
const COPIED: Duration = Duration::from_millis(1500);

/// Couleurs de la planche, claires ou sombres.
pub struct Tokens {
    pub fg: Color32,
    pub fg2: Color32,
    pub fg3: Color32,
    pub panel: Color32,
    pub line: Color32,
    pub chip: Color32,
    pub soft: Color32,
    pub field: Color32,
    pub field_ring: Color32,
    pub ring: Color32,
    pub ok: Color32,
    pub danger: Color32,
    seg_on: Color32,
    scrim: Color32,
    shadow: Color32,
}

pub fn tokens() -> Tokens {
    Tokens {
        fg: style::primary_text(),
        fg2: style::text_secondary(),
        fg3: style::tertiary_text(),
        panel: style::background(),
        line: style::line(),
        chip: style::control_hover(),
        soft: style::sidebar_color(),
        field: style::themed(style::background(), style::sidebar_color()),
        field_ring: style::track(),
        ring: style::themed(Color32::from_black_alpha(20), Color32::from_white_alpha(15)),
        ok: style::success(),
        danger: style::error(),
        seg_on: style::segment_on(),
        scrim: style::scrim(),
        shadow: style::shadow(),
    }
}

pub fn font(size: f32, strong: bool) -> FontId {
    if strong {
        FontId::new(size, style::strong_family())
    } else {
        FontId::new(size, FontFamily::Proportional)
    }
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Texte sur une ligne, coupé par « … » au-delà de `max_width` ; rend sa boîte.
pub fn text(
    ui: &Ui,
    pos: Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> Rect {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(1.0));
    let galley = ui.painter().layout_job(job);
    let rect = align.anchor_size(pos, galley.size());
    ui.painter().galley(rect.min, galley, color);
    rect
}

/// Titre de section : capitales espacées.
fn caption(ui: &Ui, pos: Pos2, title: &str, color: Color32) {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &title.to_uppercase(),
        0.0,
        egui::TextFormat {
            font_id: font(12.0, true),
            extra_letter_spacing: 0.72,
            color,
            ..Default::default()
        },
    );
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(pos, galley, color);
}

/// Ouvre le dialogue `id`, rempli par `add`. Vrai s'il doit se fermer (Échap ou clic sur
/// le voile).
pub fn show(ctx: &egui::Context, id: &str, add: impl FnOnce(&mut Ui, &Tokens)) -> bool {
    let t = tokens();
    let frame = Frame::new()
        .fill(t.panel)
        .corner_radius(18)
        .stroke(Stroke::new(1.0, t.ring))
        .shadow(egui::Shadow {
            offset: [0, 30],
            blur: 80,
            spread: 0,
            color: t.shadow,
        });
    let modal = egui::Modal::new(egui::Id::unique(id))
        .frame(frame)
        .backdrop_color(t.scrim)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            add(ui, &t);
        });
    modal.should_close()
}

/// En-tête : `lead` (carré de `lead_size`) à gauche, `body` (haut de `body_height`) au
/// milieu, croix de fermeture à droite. Vrai si la croix est cliquée.
pub fn header(
    ui: &mut Ui,
    t: &Tokens,
    lead_size: f32,
    body_height: f32,
    lead: impl FnOnce(&mut Ui, Rect),
    body: impl FnOnce(&mut Ui),
) -> bool {
    let height = lead_size.max(body_height);
    let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, PAD + height + 18.0), Sense::hover());
    let row = Rect::from_min_size(rect.min + vec2(PAD, PAD), vec2(WIDTH - 2.0 * PAD, height));
    let lead_rect = Rect::from_min_size(
        Pos2::new(row.left(), row.center().y - lead_size / 2.0),
        egui::Vec2::splat(lead_size),
    );
    lead(ui, lead_rect);
    let close = Rect::from_min_size(Pos2::new(row.right() - 32.0, row.top()), egui::Vec2::splat(32.0));
    let body_rect = Rect::from_min_max(
        Pos2::new(lead_rect.right() + 14.0, row.center().y - body_height / 2.0),
        Pos2::new(close.left() - 14.0, row.center().y + body_height / 2.0),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    body(&mut child);
    round_close(ui, close, t)
}

/// Croix ronde de fermeture.
fn round_close(ui: &mut Ui, rect: Rect, t: &Tokens) -> bool {
    let response = ui
        .interact(rect, ui.scope_id().with("fermer"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if response.hovered() {
        t.chip.lerp_to_gamma(t.fg, 0.08)
    } else {
        t.chip
    };
    ui.painter().circle_filled(rect.center(), 16.0, fill);
    let c = rect.center();
    let stroke = Stroke::new(1.8, t.fg2);
    for (from, to) in [
        (vec2(-3.5, -3.5), vec2(3.5, 3.5)),
        (vec2(3.5, -3.5), vec2(-3.5, 3.5)),
    ] {
        ui.painter().line_segment([c + from, c + to], stroke);
    }
    response.clicked()
}

/// Titre et état (point coloré, « Connecté »), pour le corps de l'en-tête : 43 points.
pub fn title_and_state(ui: &mut Ui, t: &Tokens, title: &str, state: &str, color: Color32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 43.0), Sense::hover());
    text(
        ui,
        rect.left_top(),
        Align2::LEFT_TOP,
        title,
        font(19.0, true),
        t.fg,
        width,
    );
    let y = rect.top() + 27.0;
    ui.painter()
        .circle_filled(Pos2::new(rect.left() + 3.5, y + 8.0), 3.5, color);
    text(
        ui,
        Pos2::new(rect.left() + 13.0, y),
        Align2::LEFT_TOP,
        state,
        font(13.0, false),
        color,
        width - 13.0,
    );
}

/// Grande icône sur carré coloré, pour l'en-tête.
pub fn lead_tile(ui: &Ui, rect: Rect, fill: Color32, paint: impl FnOnce(&egui::Painter, Rect)) {
    ui.painter().rect_filled(rect, 14.0, fill);
    paint(ui.painter(), rect.shrink(12.0));
}

/// Tuiles côte à côte : intitulé et valeur.
pub fn tiles(ui: &mut Ui, t: &Tokens, items: &[(&str, String)]) {
    let count = items.len().max(1) as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, 58.0), Sense::hover());
    let width = (WIDTH - 2.0 * PAD - 10.0 * (count - 1.0)) / count;
    for (slot, (key, value)) in items.iter().enumerate() {
        let tile = Rect::from_min_size(
            Pos2::new(rect.left() + PAD + slot as f32 * (width + 10.0), rect.top()),
            vec2(width, 58.0),
        );
        ui.painter().rect_filled(tile, 12.0, t.soft);
        text(
            ui,
            tile.min + vec2(14.0, 11.0),
            Align2::LEFT_TOP,
            key,
            font(12.0, false),
            t.fg3,
            width - 28.0,
        );
        text(
            ui,
            tile.min + vec2(14.0, 29.0),
            Align2::LEFT_TOP,
            value,
            font(15.0, true),
            t.fg,
            width - 28.0,
        );
    }
}

/// Section encadrée : titre (et `aside` à droite), puis ses lignes. `top` : espace au-dessus.
pub fn section<R>(
    ui: &mut Ui,
    t: &Tokens,
    top: f32,
    title: &str,
    aside: Option<&str>,
    add: impl FnOnce(&mut Rows) -> R,
) -> R {
    ui.add_space(top);
    let (head, _) = ui.allocate_exact_size(vec2(WIDTH, 23.0), Sense::hover());
    caption(ui, head.min + vec2(PAD, 0.0), title, t.fg3);
    if let Some(aside) = aside {
        text(
            ui,
            Pos2::new(head.right() - PAD, head.top()),
            Align2::RIGHT_TOP,
            aside,
            mono(12.0),
            t.fg3,
            200.0,
        );
    }
    let start = ui.cursor().top();
    let mut rows = Rows { ui, t, count: 0 };
    let result = add(&mut rows);
    let bottom = rows.ui.cursor().top();
    let group = Rect::from_min_max(
        Pos2::new(head.left() + PAD, start),
        Pos2::new(head.right() - PAD, bottom),
    );
    rows.ui
        .painter()
        .rect_stroke(group, 12.0, Stroke::new(1.0, t.line), StrokeKind::Inside);
    result
}

/// Lignes d'une section, séparées par un filet.
pub struct Rows<'a> {
    ui: &'a mut Ui,
    t: &'a Tokens,
    count: usize,
}

impl Rows<'_> {
    /// Ligne haute de `height` : sa boîte, filet du haut posé (sauf la première).
    fn line(&mut self, height: f32) -> Rect {
        let (rect, _) = self.ui.allocate_exact_size(vec2(WIDTH, height), Sense::hover());
        let rect = rect.shrink2(vec2(PAD, 0.0));
        if self.count > 0 {
            self.ui
                .painter()
                .hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, self.t.line));
        }
        self.count += 1;
        rect
    }

    /// Intitulé et valeur (à chasse fixe si `monospace`), sous-titre éventuel, bouton
    /// d'action éventuel. Vrai si le bouton est cliqué.
    pub fn value(
        &mut self,
        key: &str,
        value: &str,
        monospace: bool,
        sub: Option<&str>,
        action: Option<&str>,
    ) -> bool {
        let t = self.t;
        let rect = self.line(if sub.is_some() { 56.0 } else { 48.0 });
        let ui = &mut *self.ui;
        text(
            ui,
            Pos2::new(rect.left() + 16.0, rect.center().y),
            Align2::LEFT_CENTER,
            key,
            font(14.0, false),
            t.fg2,
            120.0,
        );
        let mut clicked = false;
        let mut right = rect.right() - 16.0;
        if let Some(action) = action {
            let width = ui
                .painter()
                .layout_no_wrap(action.to_owned(), font(13.0, false), t.fg)
                .size()
                .x
                + 20.0;
            let button = Rect::from_min_size(
                Pos2::new(right - width, rect.center().y - 15.0),
                vec2(width, 30.0),
            );
            clicked = chip_button(ui, button, action, font(13.0, false), 8.0, t);
            right = button.left() - 12.0;
        }
        let x = rect.left() + 16.0 + 120.0 + 12.0;
        let value_font = if monospace { mono(14.0) } else { font(14.0, false) };
        let width = right - x;
        match sub {
            Some(sub) => {
                text(
                    ui,
                    Pos2::new(x, rect.center().y - 1.0),
                    Align2::LEFT_BOTTOM,
                    value,
                    value_font,
                    t.fg,
                    width,
                );
                text(
                    ui,
                    Pos2::new(x, rect.center().y + 1.0),
                    Align2::LEFT_TOP,
                    sub,
                    font(12.0, false),
                    t.fg3,
                    width,
                );
            }
            None => {
                text(
                    ui,
                    Pos2::new(x, rect.center().y),
                    Align2::LEFT_CENTER,
                    value,
                    value_font,
                    t.fg,
                    width,
                );
            }
        }
        clicked
    }

    /// Intitulé, valeur à chasse fixe alignée à droite et bouton « Copier » (qui la copie).
    pub fn copy(&mut self, key: &str, value: &str) {
        let t = self.t;
        let rect = self.line(48.0);
        let ui = &mut *self.ui;
        text(
            ui,
            Pos2::new(rect.left() + 16.0, rect.center().y),
            Align2::LEFT_CENTER,
            key,
            font(14.0, false),
            t.fg2,
            120.0,
        );
        let button = Rect::from_min_size(
            Pos2::new(rect.right() - 8.0 - 32.0, rect.center().y - 16.0),
            vec2(32.0, 32.0),
        );
        text(
            ui,
            Pos2::new(button.left() - 4.0, rect.center().y),
            Align2::RIGHT_CENTER,
            value,
            mono(14.0),
            t.fg,
            button.left() - rect.left() - 160.0,
        );
        let id = ui.scope_id().with(("copier", key));
        let response = ui
            .interact(button, id, Sense::click())
            .on_hover_text(format!("Copier {}", key.to_lowercase()));
        if response.hovered() {
            ui.painter().rect_filled(button, 8.0, t.chip);
        }
        if response.clicked() {
            ui.ctx().copy_text(value.to_owned());
            ui.ctx()
                .memory_mut(|memory| memory.data.insert_temp(id, Instant::now()));
        }
        let copied = ui
            .ctx()
            .memory(|memory| memory.data.get_temp::<Instant>(id))
            .is_some_and(|at| at.elapsed() < COPIED);
        let c = button.center();
        if copied {
            ui.ctx().request_repaint_after(COPIED);
            let stroke = Stroke::new(1.8, t.ok);
            ui.painter()
                .line_segment([c + vec2(-4.5, 0.0), c + vec2(-1.5, 3.5)], stroke);
            ui.painter()
                .line_segment([c + vec2(-1.5, 3.5), c + vec2(4.5, -3.5)], stroke);
        } else {
            // Deux feuilles décalées, comme l'icône de la planche.
            let stroke = Stroke::new(1.5, t.fg3);
            let front = Rect::from_min_max(c + vec2(-2.0, -2.0), c + vec2(5.0, 5.0));
            ui.painter().rect_stroke(front, 1.0, stroke, StrokeKind::Middle);
            ui.painter()
                .line_segment([c + vec2(-4.5, 2.5), c + vec2(-4.5, -4.5)], stroke);
            ui.painter()
                .line_segment([c + vec2(-4.5, -4.5), c + vec2(2.5, -4.5)], stroke);
        }
    }

    /// Option à interrupteur, avec son explication. Vrai si elle change.
    pub fn switch(&mut self, label: &str, hint: &str, on: &mut bool) -> bool {
        let t = self.t;
        let rect = self.line(54.0);
        let ui = &mut *self.ui;
        let width = rect.width() - 32.0 - 56.0;
        text(
            ui,
            Pos2::new(rect.left() + 16.0, rect.center().y - 1.0),
            Align2::LEFT_BOTTOM,
            label,
            font(14.0, false),
            t.fg,
            width,
        );
        text(
            ui,
            Pos2::new(rect.left() + 16.0, rect.center().y + 1.0),
            Align2::LEFT_TOP,
            hint,
            font(12.0, false),
            t.fg3,
            width,
        );
        let place = Rect::from_min_size(
            Pos2::new(rect.right() - 16.0 - 44.0, rect.center().y - 13.0),
            vec2(44.0, 26.0),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(place)
                .id_salt(("interrupteur", label)),
        );
        style::switch(&mut child, on, true).changed()
    }

    /// Choix exclusif (« Aucun · Lecture · Lecture et écriture »). Vrai s'il change.
    pub fn choice(&mut self, who: &str, sub: &str, options: &[&str], selected: &mut usize) -> bool {
        let t = self.t;
        let rect = self.line(50.0);
        let ui = &mut *self.ui;
        let widths: Vec<f32> = options
            .iter()
            .map(|option| {
                ui.painter()
                    .layout_no_wrap((*option).to_owned(), font(12.0, true), t.fg)
                    .size()
                    .x
                    + 20.0
            })
            .collect();
        let total = widths.iter().sum::<f32>() + 2.0 * (widths.len() as f32 - 1.0) + 6.0;
        let group = Rect::from_min_size(
            Pos2::new(rect.right() - 10.0 - total, rect.center().y - 17.0),
            vec2(total, 34.0),
        );
        let width = group.left() - rect.left() - 28.0;
        text(
            ui,
            Pos2::new(rect.left() + 16.0, rect.center().y - 1.0),
            Align2::LEFT_BOTTOM,
            who,
            font(14.0, false),
            t.fg,
            width,
        );
        text(
            ui,
            Pos2::new(rect.left() + 16.0, rect.center().y + 1.0),
            Align2::LEFT_TOP,
            sub,
            font(12.0, false),
            t.fg3,
            width,
        );
        ui.painter().rect_filled(group, 9.0, t.chip);
        let mut changed = false;
        let mut x = group.left() + 3.0;
        for (slot, (option, width)) in options.iter().zip(&widths).enumerate() {
            let button = Rect::from_min_size(Pos2::new(x, group.top() + 3.0), vec2(*width, 28.0));
            x += width + 2.0;
            let on = *selected == slot;
            let response = ui
                .interact(button, ui.scope_id().with(("choix", who, slot)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if on {
                let shadow = egui::Shadow {
                    offset: [0, 1],
                    blur: 3,
                    spread: 0,
                    color: Color32::from_black_alpha(38),
                };
                ui.painter().add(shadow.as_shape(button, 7.0));
                ui.painter().rect_filled(button, 7.0, t.seg_on);
            }
            let ink = if on { t.fg } else { t.fg2 };
            text(
                ui,
                button.center(),
                Align2::CENTER_CENTER,
                option,
                font(12.0, on),
                ink,
                *width,
            );
            if response.clicked() && !on {
                *selected = slot;
                changed = true;
            }
        }
        changed
    }
}

/// Bouton gris arrondi. Vrai s'il est cliqué.
fn chip_button(ui: &mut Ui, rect: Rect, label: &str, font: FontId, radius: f32, t: &Tokens) -> bool {
    let response = ui
        .interact(rect, ui.scope_id().with(("bouton", label)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if response.hovered() {
        t.chip.lerp_to_gamma(t.fg, 0.08)
    } else {
        t.chip
    };
    ui.painter().rect_filled(rect, radius, fill);
    text(
        ui,
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        t.fg,
        rect.width(),
    );
    response.clicked()
}

/// À gauche du pied : lien rouge (« Oublier ce réseau ») ou simple remarque.
pub enum Left<'a> {
    Danger(&'a str),
    Note(&'a str),
}

/// Ce qui a été cliqué dans le pied.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Footer {
    pub left: bool,
    /// Bouton de droite, dans l'ordre donné ; le dernier est le bouton principal.
    pub button: Option<usize>,
}

/// Pied du dialogue : filet, action ou remarque à gauche, boutons à droite (le dernier en
/// couleur d'accent).
pub fn footer(ui: &mut Ui, t: &Tokens, left: Left, buttons: &[&str]) -> Footer {
    ui.add_space(20.0);
    let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, 64.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, t.line));
    let row = Rect::from_min_max(rect.min + vec2(PAD, 14.0), rect.max - vec2(PAD, 14.0));
    let mut footer = Footer::default();
    let mut right = row.right();
    for (slot, label) in buttons.iter().enumerate().rev() {
        let primary = slot == buttons.len() - 1;
        let label_font = font(14.0, primary);
        let width = ui
            .painter()
            .layout_no_wrap((*label).to_owned(), label_font.clone(), t.fg)
            .size()
            .x
            + if primary { 36.0 } else { 32.0 };
        let button = Rect::from_min_size(Pos2::new(right - width, row.top()), vec2(width, 36.0));
        right = button.left() - 8.0;
        let clicked = if primary {
            let response = ui
                .interact(button, ui.scope_id().with(("principal", label)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let fill = if response.hovered() {
                style::accent().lerp_to_gamma(Color32::BLACK, 0.1)
            } else {
                style::accent()
            };
            ui.painter().rect_filled(button, 10.0, fill);
            text(
                ui,
                button.center(),
                Align2::CENTER_CENTER,
                label,
                label_font,
                style::accent_text(),
                width,
            );
            response.clicked()
        } else {
            chip_button(ui, button, label, label_font, 10.0, t)
        };
        if clicked {
            footer.button = Some(slot);
        }
    }
    match left {
        Left::Danger(label) => {
            let galley_width = ui
                .painter()
                .layout_no_wrap(label.to_owned(), font(14.0, true), t.danger)
                .size()
                .x;
            let link = Rect::from_min_size(Pos2::new(row.left(), row.top()), vec2(galley_width + 8.0, 36.0));
            let response = ui
                .interact(link, ui.scope_id().with("lien"), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let color = if response.hovered() {
                t.danger.lerp_to_gamma(Color32::BLACK, 0.15)
            } else {
                t.danger
            };
            text(
                ui,
                Pos2::new(link.left() + 4.0, link.center().y),
                Align2::LEFT_CENTER,
                label,
                font(14.0, true),
                color,
                galley_width + 4.0,
            );
            footer.left = response.clicked();
        }
        Left::Note(note) => {
            text(
                ui,
                Pos2::new(row.left(), row.center().y),
                Align2::LEFT_CENTER,
                note,
                font(12.0, false),
                t.fg3,
                right - row.left(),
            );
        }
    }
    footer
}
