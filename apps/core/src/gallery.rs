//! Galerie Ferrite (`ferrite-apps galerie`) : toutes les pièces communes de `kit` et les
//! rôles de couleur de `style`, sur une page. Elle sert à vérifier d'un coup d'œil que
//! tout va ensemble, en clair et en sombre, et de modèle à une nouvelle application :
//! un état, `update`, une barre du haut et des pièces de `kit`, aucune couleur écrite à
//! la main (voir `docs/NOUVELLE-APPLICATION.md`).

use eframe::egui::{self, Color32, FontId, Frame, Margin, RichText};

use crate::{kit, style};

/// Pages de la galerie.
const PAGES: [&str; 2] = ["Composants", "Couleurs"];

#[derive(Debug)]
pub struct GalleryState {
    page: usize,
    switch_on: bool,
    level: u8,
    choice: usize,
    segment: usize,
}

impl Default for GalleryState {
    fn default() -> Self {
        Self {
            page: usize::from(std::env::var_os("FERRITE_GALERIE_COULEURS").is_some()),
            switch_on: true,
            level: 60,
            choice: 0,
            segment: 1,
        }
    }
}

impl GalleryState {
    pub fn update(&mut self, ui: &mut egui::Ui) {
        // Barre du haut : boutons de fenêtre, puis les pages en segments au milieu.
        style::header(ui, |ui| {
            let center = ui.max_rect().center();
            if let Some(page) = kit::segments_at(ui, center, 120.0, &PAGES, self.page) {
                self.page = page;
            }
        });
        egui::CentralPanel::no_frame()
            .frame(
                Frame::new()
                    .fill(style::background())
                    .inner_margin(Margin::symmetric(28, 8)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| match self.page {
                    0 => self.components(ui),
                    _ => colors(ui),
                });
            });
    }

    fn components(&mut self, ui: &mut egui::Ui) {
        kit::heading(ui, "Réglages");
        kit::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Interrupteur").size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    kit::switch(ui, &mut self.switch_on);
                });
            });
            kit::separator(ui);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Curseur").size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let text = format!("{} %", self.level);
                    kit::slider(ui, &mut self.level, &text);
                });
            });
            kit::separator(ui);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (index, label) in ["Automatique", "Clair", "Sombre"].iter().enumerate() {
                    if kit::radio(ui, self.choice == index).clicked() {
                        self.choice = index;
                    }
                    ui.label(RichText::new(*label).size(14.0));
                    ui.add_space(12.0);
                }
            });
        });
        kit::note(ui, "Carte bordée : pour une liste de réglages ou d'appareils.");

        kit::heading(ui, "Boutons");
        ui.horizontal(|ui| {
            kit::button(ui, "Annuler", false);
            kit::button(ui, "Enregistrer", true);
            if let Some(segment) = kit::segments(ui, &["Grille", "Liste", "Colonnes"], self.segment) {
                self.segment = segment;
            }
            kit::status(ui, "Recherche…", true);
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let width = (ui.available_width() - 24.0) / 3.0;
            kit::wide_button(ui, width, "Réinitialiser", kit::Tone::Plain, true);
            kit::wide_button(ui, width, "Démarrer", kit::Tone::Accent, true);
            kit::wide_button(ui, width, "Arrêter", kit::Tone::Danger, true);
        });

        kit::heading(ui, "Textes");
        kit::soft_card(ui, |ui| {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 96.0), egui::Sense::hover());
            let painter = ui.painter();
            kit::caption(
                painter,
                rect.left_top() + egui::vec2(0.0, 12.0),
                "Petit titre de section",
            );
            kit::tabular_text(
                painter,
                rect.left_top() + egui::vec2(0.0, 32.0),
                egui::Align2::LEFT_TOP,
                "01:23:45.67",
                FontId::new(40.0, style::light_family()),
                style::primary_text(),
            );
        });
        kit::note(
            ui,
            "Chiffres de largeur égale : un compteur centré ne tremble pas.",
        );
        ui.add_space(20.0);
    }
}

/// Rôles de couleur communs, avec leur nom de fonction dans `style`.
fn colors(ui: &mut egui::Ui) {
    let roles: [(&str, Color32); 20] = [
        ("background", style::background()),
        ("header_color", style::header_color()),
        ("sidebar_color", style::sidebar_color()),
        ("content_gray", style::content_gray()),
        ("stage", style::stage()),
        ("control_background", style::control_background()),
        ("control_hover", style::control_hover()),
        ("segment_on", style::segment_on()),
        ("separator", style::separator()),
        ("line", style::line()),
        ("track", style::track()),
        ("primary_text", style::primary_text()),
        ("text_secondary", style::text_secondary()),
        ("tertiary_text", style::tertiary_text()),
        ("heading_text", style::heading_text()),
        ("inverse", style::inverse()),
        ("accent", style::accent()),
        ("success", style::success()),
        ("warning", style::warning()),
        ("error", style::error()),
    ];
    kit::heading(ui, "Rôles de couleur");
    kit::note(
        ui,
        "Chaque application prend ces rôles (style::nom) ; tools/check-colors.sh refuse les couleurs écrites à la main.",
    );
    ui.add_space(10.0);
    let columns = ((ui.available_width() / 200.0).floor() as usize).max(1);
    egui::Grid::new("roles")
        .num_columns(columns)
        .spacing(egui::vec2(12.0, 12.0))
        .show(ui, |ui| {
            for (index, (name, color)) in roles.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (swatch, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
                    ui.painter().rect_filled(swatch, 8.0, *color);
                    ui.painter().rect_stroke(
                        swatch,
                        8.0,
                        egui::Stroke::new(1.0, style::line()),
                        egui::StrokeKind::Inside,
                    );
                    ui.label(
                        RichText::new(*name)
                            .size(13.0)
                            .family(egui::FontFamily::Monospace),
                    );
                    ui.add_space(200.0 - 36.0 - ui.min_rect().width().min(200.0));
                });
                if (index + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });
    ui.add_space(20.0);
}
