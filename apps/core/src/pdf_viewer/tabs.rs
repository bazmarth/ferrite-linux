//! Plusieurs documents ouverts dans la même fenêtre : un onglet par document, sous la
//! barre d'outils, dès qu'il y en a deux. Même dessin que les onglets de TextEdit.

use eframe::egui::{self, Rect, Sense, Stroke, pos2, vec2};

use super::{Glyph, paint_glyph};
use crate::style;

const TAB_HEIGHT: f32 = 36.0;

/// Ce qu'on a fait dans la bande d'onglets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    Select(usize),
    Close(usize),
    /// Bouton « + » : ouvrir un autre document.
    Open,
}

/// Un onglet : nom affiché et chemin complet (info-bulle).
pub struct Tab {
    pub name: String,
    pub path: String,
}

/// Onglets à montrer : rien tant qu'un seul document est ouvert.
pub struct Strip {
    pub tabs: Vec<Tab>,
    pub active: usize,
}

pub fn strip(ui: &mut egui::Ui, strip: &Strip, id: u64) -> Option<TabAction> {
    let mut action = None;
    egui::Panel::top(ui.scope_id().with(("pdf-onglets", id)))
        .resizable(false)
        .show_separator_line(false)
        .exact_size(TAB_HEIGHT)
        .frame(egui::Frame::new().fill(style::content_gray()))
        .show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter().hline(
                full.x_range(),
                full.bottom() - 0.5,
                Stroke::new(1.0, style::border()),
            );
            egui::ScrollArea::horizontal()
                .id_salt("pdf-onglets-defilement")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (index, tab) in strip.tabs.iter().enumerate() {
                            if let Some(chosen) = one_tab(ui, index, tab, index == strip.active) {
                                action = Some(chosen);
                            }
                        }
                        ui.add_space(6.0);
                        let (rect, response) = ui.allocate_exact_size(vec2(34.0, TAB_HEIGHT), Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(
                                Rect::from_center_size(rect.center(), vec2(30.0, 30.0)),
                                7.0,
                                style::tint(10),
                            );
                        }
                        paint_glyph(ui.painter(), rect.center(), Glyph::Plus, style::text_secondary());
                        if response
                            .on_hover_text("Ouvrir un autre document (Ctrl+O)")
                            .clicked()
                        {
                            action = Some(TabAction::Open);
                        }
                    });
                });
        });
    action
}

fn one_tab(ui: &mut egui::Ui, index: usize, tab: &Tab, active: bool) -> Option<TabAction> {
    let font = egui::FontId::proportional(13.0);
    let text_width = ui
        .painter()
        .layout_no_wrap(tab.name.clone(), font.clone(), style::primary_text())
        .size()
        .x;
    let width = (text_width + 56.0).clamp(120.0, 240.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, TAB_HEIGHT), Sense::click());
    let painter = ui.painter();
    if active {
        // Le trait du bas s'interrompt sous l'onglet actif.
        painter.rect_filled(rect.expand2(vec2(0.0, 0.5)), 0.0, style::background());
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, style::tint(6));
    }
    painter.vline(
        rect.right() - 0.5,
        rect.top() + 10.0..=rect.bottom() - 10.0,
        Stroke::new(1.0, style::border()),
    );
    let color = if active {
        style::primary_text()
    } else {
        style::text_secondary()
    };
    let mut job = egui::text::LayoutJob::simple_singleline(tab.name.clone(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width - 52.0);
    let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
    painter.galley(
        pos2(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    let close = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(20.0, 20.0));
    let close_response = ui.interact(close, response.id.with("fermer"), Sense::click());
    if active || response.hovered() || close_response.hovered() {
        if close_response.hovered() {
            ui.painter().rect_filled(close, 5.0, style::tint(14));
        }
        paint_glyph(ui.painter(), close.center(), Glyph::Close, color);
    }
    let closed = close_response.on_hover_text("Fermer (Ctrl+W)").clicked();
    if closed || response.middle_clicked() {
        Some(TabAction::Close(index))
    } else if response.on_hover_text(&tab.path).clicked() {
        Some(TabAction::Select(index))
    } else {
        None
    }
}
