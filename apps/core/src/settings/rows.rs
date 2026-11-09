//! Composants des pages de Paramètres, d'après les planches : bandeau, groupes de lignes et
//! commandes (bouton, liste, interrupteur, curseur, choix unique, segments).

use eframe::egui::{self, Color32, Frame, Margin, RichText, Sense, Stroke, text::LayoutJob};

use crate::style::{self, Icons};

const ROW_HEIGHT: f32 = 48.0;
const ROW_PADDING: f32 = 16.0;
/// Écart entre deux groupes d'une colonne.
pub const GROUP_GAP: f32 = 22.0;
/// Largeur maximale de la colonne de contenu, centrée dans la page.
pub const COLUMN: f32 = 680.0;
/// Marge minimale de part et d'autre de la colonne.
pub const COLUMN_MARGIN: f32 = 24.0;

/// Fond de la page, sous les cartes de groupe (un cran plus foncé qu'elles en clair, plus
/// clair que la barre latérale en sombre).
pub fn page_color() -> Color32 {
    style::themed(style::header_color(), style::sidebar_color())
}

/// Ce qui précède le libellé d'une ligne.
#[derive(Clone, Copy, Default)]
pub enum Lead {
    #[default]
    None,
    /// Pastille colorée de 30 px et son symbole blanc.
    Tile(&'static str, u32),
    /// Force du signal Wi-Fi, de 1 à 3 ondes.
    Bars(u8),
    /// Image ronde de 30 px (image de profil).
    Picture(egui::TextureId),
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
            // Séparateur décalé de 16 px, comme les réglages de la maquette.
            ui.painter().hline(
                egui::Rangef::new(rect.left() + ROW_PADDING, rect.right()),
                rect.top() + 0.5,
                Stroke::new(1.0, style::separator()),
            );
        }
        self.first = false;
        if row.clickable && row.enabled && response.hovered() {
            ui.painter().rect_filled(rect.shrink(1.0), 6.0, style::tint(8));
        }
        let inner = rect.shrink2(egui::vec2(ROW_PADDING, 6.0));
        // Identifiant propre à la ligne : deux lignes à segments (Position, Mode) ne se
        // confondent plus.
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("ligne", row.label))
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
                style::IconStyle::Tinted(style::tertiary_text()),
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
            Lead::Picture(texture) => {
                let tile = egui::Rect::from_center_size(
                    egui::pos2(x + 15.0, inner.center().y),
                    egui::vec2(30.0, 30.0),
                );
                egui::Image::from_texture(egui::load::SizedTexture::new(texture, tile.size()))
                    .corner_radius(15)
                    .paint_at(&child, tile);
                x += 42.0;
            }
            Lead::Bars(level) => {
                // Ondes Wi-Fi : allumées selon le niveau, les autres estompées.
                let icon = egui::Rect::from_center_size(
                    egui::pos2(x + 15.0, inner.center().y),
                    egui::vec2(26.0, 26.0),
                );
                let name = ["status-wifi-0", "status-wifi-1", "status-wifi-2", "status-wifi-3"]
                    [level.min(3) as usize];
                self.icons.paint(
                    &child,
                    name,
                    icon,
                    style::IconStyle::Tinted(style::primary_text()),
                );
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
            style::success()
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

/// Grande ligne de tête (Wi-Fi, Bluetooth) : pastille de 44 px, titre, état et
/// interrupteur principal, dans sa propre carte.
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
        .fill(style::background())
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(12)
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let (tile, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), Sense::hover());
                ui.painter().rect_filled(tile, 11.0, rgb(color));
                icons.paint(
                    ui,
                    glyph,
                    tile.shrink(10.0),
                    style::IconStyle::Tinted(Color32::WHITE),
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add_space(4.0);
                    ui.label(RichText::new(title).size(16.0).family(style::strong_family()));
                    ui.label(RichText::new(subtitle).size(12.0).color(style::text_secondary()));
                });
                if let Some((on, enabled)) = toggle {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        changed = style::switch_sized(ui, on, enabled, egui::vec2(44.0, 26.0)).changed();
                    });
                }
            });
        });
    ui.add_space(GROUP_GAP);
    changed
}

/// Titre de groupe : 13 px, semi-gras, couleur secondaire. Le même partout.
pub fn group_title(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(
            RichText::new(title)
                .size(13.0)
                .family(style::strong_family())
                .color(style::text_secondary()),
        );
    });
    ui.add_space(8.0);
}

/// Note de bas de groupe, en 12 px, juste sous la carte.
pub fn group_note(ui: &mut egui::Ui, text: &str) {
    ui.add_space(-GROUP_GAP + 8.0);
    ui.horizontal_wrapped(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(text).size(12.0).color(style::text_secondary()));
    });
    ui.add_space(GROUP_GAP);
}

/// Groupe libre (aperçus, vignettes) : titre puis carte, avec une marge intérieure.
pub fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    group_title(ui, title);
    let result = Frame::new()
        .fill(style::background())
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(12)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner;
    ui.add_space(GROUP_GAP);
    result
}

/// Groupe de lignes : titre puis carte aux coins arrondis.
pub fn group<R>(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    title: &str,
    add: impl FnOnce(&mut Group<'_>) -> R,
) -> R {
    if !title.is_empty() {
        group_title(ui, title);
    }
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

pub use crate::kit::{button, radio, segments, slider, status, switch};

/// Valeur affichée à droite d'une ligne, en 13 px gris.
pub fn value(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).color(style::text_secondary()));
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
            style::success()
        } else {
            style::warning()
        };
        ui.painter().rect_filled(cell, 2.0, color);
    }
}

fn rgb(hex: u32) -> Color32 {
    let [_, r, g, b] = hex.to_be_bytes();
    Color32::from_rgb(r, g, b)
}
