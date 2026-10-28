//! Apparence commune des applications Ferrite, reprise du Ferrite d'origine
//! (`D:\Os\user\src\gui.rs`) : un panneau latéral gris flottant aux coins arrondis, avec
//! les trois boutons de fenêtre en haut ; sans panneau, une barre d'outils en dégradé ;
//! des groupes de boutons en pilule. Les applications dessinent leur haut elles-mêmes ;
//! ferrite-shell garde les coins arrondis, l'ombre et les bords.
//! Aussi : couleurs de la charte, polices, et icônes SVG du dossier `icons/SVG`.

use std::{collections::HashMap, sync::Arc};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Stroke,
    TextStyle, TextureHandle, TextureOptions,
};
use ferrite_ui::theme::{Color, Palette};

const PALETTE: Palette = Palette::LIGHT;

/// Barre d'état et fonds secondaires, à peine teintés.
pub const HEADER: Color32 = Color32::from_rgb(0xF6, 0xF6, 0xF8);
/// Contenu et barre d'outils : la partie claire de la fenêtre.
pub const BACKGROUND: Color32 = Color32::WHITE;
/// Fond du contenu des Paramètres, où les réglages sont posés sur des cartes blanches.
pub const CONTENT_GRAY: Color32 = Color32::from_rgb(0xF5, 0xF5, 0xF7);
/// Panneau latéral flottant : gris clair (milieu du dégradé F1F1F4 → E8E8EC d'origine).
pub const SIDEBAR: Color32 = Color32::from_rgb(0xED, 0xED, 0xF0);
/// Barre d'outils d'une fenêtre sans panneau : dégradé de haut en bas, trait dessous.
const TOOLBAR_TOP: Color32 = Color32::from_rgb(0xFB, 0xFB, 0xFC);
const TOOLBAR_BOTTOM: Color32 = Color32::from_rgb(0xF1, 0xF1, 0xF4);
const TOOLBAR_LINE: Color32 = Color32::from_rgb(0xD9, 0xD9, 0xDE);
/// Marge autour du panneau latéral, et rayon de ses coins.
const PANEL_MARGIN: f32 = 8.0;
const PANEL_RADIUS: u8 = 14;
/// Hauteur de la barre d'outils, et de la zone des boutons en haut de la colonne.
pub const TOOLBAR_HEIGHT: f32 = 52.0;
pub const TEXT: Color32 = Color32::from_rgb(0x1D, 0x1D, 0x1F);
pub const TEXT_SECONDARY: Color32 = color(PALETTE.text_secondary);
pub const SEPARATOR: Color32 = Color32::from_rgb(0xE3, 0xE3, 0xE8);
pub const ACCENT: Color32 = color(PALETTE.accent);
/// Ligne choisie d'une barre latérale : voile sombre léger, comme dans l'original.
pub const SIDEBAR_SELECTED: Color32 = Color32::from_black_alpha(22);
pub const ERROR: Color32 = Color32::from_rgb(0xC6, 0x26, 0x2E);

const fn color(color: Color) -> Color32 {
    Color32::from_rgb(color.r, color.g, color.b)
}

/// Polices (Inter et JetBrains Mono) et réglages visuels partagés.
pub fn apply(context: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "ferrite-inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "ferrite-inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    fonts.font_data.insert(
        "ferrite-mono".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "ferrite-inter".into());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "ferrite-mono".into());
    let mut strong = vec!["ferrite-inter-semibold".to_owned()];
    strong.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name("strong".into()), strong);
    context.set_fonts(fonts);

    context.set_theme(egui::Theme::Light);
    context.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Body, FontId::proportional(13.5)),
            (TextStyle::Button, FontId::proportional(13.5)),
            (TextStyle::Heading, FontId::new(17.0, strong_family())),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 5.0);
        style.spacing.interact_size.y = 28.0;
        // Les libellés sont du texte d'interface, pas du contenu : ni sélection ni capture
        // du clic (le clic doit atteindre la ligne ou le bouton dessous).
        style.interaction.selectable_labels = false;

        let visuals = &mut style.visuals;
        visuals.panel_fill = BACKGROUND;
        visuals.window_fill = BACKGROUND;
        visuals.extreme_bg_color = Color32::WHITE;
        visuals.faint_bg_color = Color32::from_rgb(0xF7, 0xF7, 0xF8);
        visuals.override_text_color = Some(TEXT);
        visuals.selection.bg_fill = ACCENT;
        visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.text_cursor.stroke = Stroke::new(2.0, ACCENT);

        let radius = CornerRadius::same(7);
        let border = Stroke::new(1.0, Color32::from_rgb(0xCF, 0xCF, 0xD3));
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        for (state, fill) in [
            (&mut widgets.inactive, Color32::WHITE),
            (&mut widgets.hovered, Color32::from_rgb(0xF4, 0xF4, 0xF6)),
            (&mut widgets.active, Color32::from_rgb(0xE6, 0xE6, 0xEA)),
            (&mut widgets.open, Color32::from_rgb(0xE6, 0xE6, 0xEA)),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = border;
            state.fg_stroke = Stroke::new(1.0, TEXT);
            state.corner_radius = radius;
            state.expansion = 0.0;
        }
        widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xB8, 0xB8, 0xBE));
    });
}

pub fn strong_family() -> FontFamily {
    FontFamily::Name("strong".into())
}

/// Barre d'outils d'une fenêtre sans colonne : boutons de fenêtre à gauche, puis `add`.
pub fn header(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    toolbar(ui, true, add);
}

/// Barre d'outils en haut du contenu. Avec `buttons` (fenêtre sans panneau latéral),
/// elle porte les boutons de fenêtre sur un dégradé clair, avec un trait dessous ; à côté
/// d'un panneau, elle reste blanche et sans trait, ses boutons flottant sur le contenu.
/// Le vide sert à déplacer la fenêtre.
pub fn toolbar(ui: &mut egui::Ui, buttons: bool, add: impl FnOnce(&mut egui::Ui)) {
    let left = if buttons { 96 } else { 14 };
    egui::Panel::top(ui.scope_id().with("ferrite-header"))
        .resizable(false)
        .exact_size(TOOLBAR_HEIGHT)
        .frame(Frame::new().fill(BACKGROUND).inner_margin(Margin {
            left,
            right: 12,
            top: 0,
            bottom: 0,
        }))
        .show(ui, |ui| {
            let inner = ui.max_rect();
            let full = egui::Rect::from_min_max(
                egui::pos2(inner.left() - left as f32, inner.top()),
                egui::pos2(inner.right() + 12.0, inner.bottom()),
            );
            if buttons {
                vertical_gradient(ui.painter(), full, TOOLBAR_TOP, TOOLBAR_BOTTOM);
                ui.painter().hline(
                    full.x_range(),
                    full.bottom() - 0.5,
                    Stroke::new(1.0, TOOLBAR_LINE),
                );
            }
            // Fond déplaçable, enregistré avant les boutons pour qu'ils restent prioritaires.
            drag_area(ui, full, "barre");
            if buttons {
                window_buttons(ui, egui::pos2(full.left() + 22.0, full.center().y));
            }
            // Centré en hauteur, sans étirer les éléments (pastilles, onglets).
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), add);
        });
}

/// Rectangle peint d'un dégradé vertical, de `top` à `bottom`.
fn vertical_gradient(painter: &egui::Painter, rect: egui::Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
}

/// Panneau latéral flottant, de couleur `fill`, détaché des bords de la fenêtre par une
/// marge blanche, avec les boutons de fenêtre dans son coin (sauf `buttons` faux :
/// sélecteur de fichiers dans une feuille).
pub fn sidebar(
    ui: &mut egui::Ui,
    id: &str,
    width: f32,
    fill: Color32,
    buttons: bool,
    add: impl FnOnce(&mut egui::Ui),
) {
    egui::Panel::left(ui.scope_id().with(id))
        .resizable(false)
        .exact_size(width)
        .frame(Frame::new().fill(BACKGROUND))
        .show(ui, |ui| {
            let full = ui.max_rect();
            let panel = full.shrink(PANEL_MARGIN);
            let painter = ui.painter();
            painter.rect_filled(
                panel.expand(1.0).translate(egui::vec2(0.0, 1.0)),
                PANEL_RADIUS + 1,
                Color32::from_black_alpha(12),
            );
            painter.rect(
                panel,
                PANEL_RADIUS,
                fill,
                Stroke::new(1.0, Color32::from_black_alpha(22)),
                egui::StrokeKind::Inside,
            );
            let top = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), TOOLBAR_HEIGHT));
            if buttons {
                drag_area(ui, top, "colonne");
                window_buttons(ui, egui::pos2(panel.left() + 21.0, top.center().y));
            }
            let body = egui::Rect::from_min_max(
                egui::pos2(
                    panel.left() + 6.0,
                    if buttons { top.bottom() } else { panel.top() + 10.0 },
                ),
                egui::pos2(panel.right() - 6.0, panel.bottom() - 6.0),
            );
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
            add(&mut child);
        });
}

/// Groupe de boutons en forme de pilule, comme dans les barres d'outils du Ferrite
/// d'origine : fond blanc, contour et ombre légers. Les boutons de `add` sont collés.
pub fn pill_group<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, Color32::from_black_alpha(26)))
        .corner_radius(16)
        .shadow(egui::Shadow {
            offset: [0, 1],
            blur: 0,
            spread: 0,
            color: Color32::from_black_alpha(14),
        })
        .inner_margin(Margin::symmetric(2, 2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(add).inner
        })
        .inner
}

/// Contrôle segmenté : rail gris dans lequel le segment choisi est blanc et en relief.
/// Les segments de `add` dessinent eux-mêmes leur état (voir [`segment_background`]).
pub fn segmented<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new()
        .fill(Color32::from_rgb(0xE3, 0xE3, 0xE8))
        .corner_radius(8)
        .inner_margin(Margin::symmetric(2, 2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(add).inner
        })
        .inner
}

/// Fond d'un segment de [`segmented`] : blanc en relief s'il est choisi.
pub fn segment_background(ui: &egui::Ui, rect: egui::Rect, selected: bool, hovered: bool) {
    let painter = ui.painter();
    if selected {
        painter.rect_filled(
            rect.translate(egui::vec2(0.0, 1.0)),
            6.0,
            Color32::from_black_alpha(20),
        );
        painter.rect_filled(rect, 6.0, Color32::WHITE);
    } else if hovered {
        painter.rect_filled(rect, 6.0, Color32::from_black_alpha(10));
    }
}

/// Zone vide qui déplace la fenêtre ; un double-clic l'agrandit ou la rétablit.
fn drag_area(ui: &mut egui::Ui, rect: egui::Rect, salt: &str) {
    let response = ui.interact(
        rect,
        ui.scope_id().with(("deplacer", salt)),
        egui::Sense::click_and_drag(),
    );
    if response.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if response.double_clicked() {
        let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
}

/// Les trois boutons de fenêtre (fermer, réduire, agrandir), comme ceux que ferrite-shell
/// dessine pour les autres applications : même taille, mêmes couleurs, symboles au survol.
fn window_buttons(ui: &mut egui::Ui, first: egui::Pos2) {
    use ferrite_ui::theme::{TRAFFIC_LIGHT_RADIUS, TRAFFIC_LIGHT_STEP};
    let radius = TRAFFIC_LIGHT_RADIUS as f32;
    let step = TRAFFIC_LIGHT_STEP as f32;
    let group = egui::Rect::from_min_max(
        egui::pos2(first.x - radius - 3.0, first.y - radius - 3.0),
        egui::pos2(first.x + 2.0 * step + radius + 3.0, first.y + radius + 3.0),
    );
    let hovered = ui.rect_contains_pointer(group);
    let focused = ui.input(|input| input.viewport().focused.unwrap_or(true));
    let [close, minimize, zoom] = PALETTE.traffic_lights;
    let (off_fill, off_edge) = PALETTE.traffic_light_inactive;
    for (index, (fill, edge)) in [close, minimize, zoom].into_iter().enumerate() {
        let center = egui::pos2(first.x + index as f32 * step, first.y);
        let rect = egui::Rect::from_center_size(center, egui::vec2(2.0 * radius + 4.0, 2.0 * radius + 4.0));
        let response = ui.interact(rect, ui.scope_id().with(("feu", index)), egui::Sense::click());
        let (fill, edge) = if focused || hovered {
            (fill, edge)
        } else {
            (off_fill, off_edge)
        };
        let painter = ui.painter();
        painter.circle(center, radius, color(fill), Stroke::new(0.8, color(edge)));
        if hovered {
            let ink = color(edge).gamma_multiply(0.55);
            let stroke = Stroke::new(1.3, ink);
            let d = radius * 0.42;
            match index {
                0 => {
                    painter.line_segment([center + egui::vec2(-d, -d), center + egui::vec2(d, d)], stroke);
                    painter.line_segment([center + egui::vec2(-d, d), center + egui::vec2(d, -d)], stroke);
                }
                1 => {
                    painter.line_segment(
                        [
                            center + egui::vec2(-d - 0.6, 0.0),
                            center + egui::vec2(d + 0.6, 0.0),
                        ],
                        stroke,
                    );
                }
                _ => {
                    painter.line_segment(
                        [
                            center + egui::vec2(-d - 0.6, 0.0),
                            center + egui::vec2(d + 0.6, 0.0),
                        ],
                        stroke,
                    );
                    painter.line_segment(
                        [
                            center + egui::vec2(0.0, -d - 0.6),
                            center + egui::vec2(0.0, d + 0.6),
                        ],
                        stroke,
                    );
                }
            }
        }
        if response.clicked() {
            let ctx = ui.ctx();
            match index {
                0 => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                1 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                _ => {
                    let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
            }
        }
    }
}

/// Barre d'état discrète en bas de la fenêtre.
pub fn status_bar(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Panel::bottom(ui.scope_id().with("ferrite-status"))
        .resizable(false)
        .frame(Frame::new().fill(HEADER).inner_margin(Margin::symmetric(12, 5)))
        .show(ui, |ui| {
            let full = ui.max_rect().expand2(egui::vec2(12.0, 5.0));
            ui.painter()
                .hline(full.x_range(), full.top() + 0.5, Stroke::new(1.0, SEPARATOR));
            ui.horizontal(|ui| {
                ui.style_mut().override_text_style = Some(TextStyle::Small);
                ui.visuals_mut().override_text_color = Some(TEXT_SECONDARY);
                add(ui);
            })
        });
}

/// Bouton mis en avant (action principale) : fond bleu, texte blanc.
pub fn suggested_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(Color32::WHITE))
            .fill(ACCENT)
            .stroke(Stroke::new(1.0, Color32::from_rgb(0x08, 0x66, 0xD6))),
    )
}

/// Interrupteur à glissière, comme dans les réglages rapides du bureau.
pub fn switch(ui: &mut egui::Ui, on: &mut bool, enabled: bool) -> egui::Response {
    let size = egui::vec2(42.0, 24.0);
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(size, sense);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let progress = ui.ctx().animate_bool_responsive(response.id, *on);
    let track = if !enabled {
        egui::Color32::from_rgb(0xE6, 0xE6, 0xE8)
    } else if *on {
        ACCENT
    } else {
        egui::Color32::from_rgb(0xCF, 0xCF, 0xD3)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, rect.height() / 2.0, track);
    let radius = rect.height() / 2.0 - 2.5;
    let x = egui::lerp(rect.left() + radius + 2.5..=rect.right() - radius - 2.5, progress);
    painter.circle_filled(egui::pos2(x, rect.center().y), radius, egui::Color32::WHITE);
    response
}

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const SOURCES: &[(&str, &str)] = &[
            $(($name, include_str!(concat!("../../../icons/SVG/", $name, ".svg")))),*
        ];
    };
}

icons![
    "app-code",
    "app-files",
    "app-music",
    "app-photos",
    "app-terminal",
    "app-store",
    "app-video",
    "file-generic",
    "file-text",
    "places-computer",
    "places-desktop",
    "places-documents",
    "places-downloads",
    "places-home",
    "places-music",
    "places-pictures",
    "places-trash",
    "places-trash-full",
    "places-usb",
    "places-videos",
    "app-calendar",
    "app-clock",
    "app-settings",
    "status-bluetooth-on",
    "status-display",
    "status-download",
    "status-keyboard",
    "status-lock",
    "status-storage",
    "status-usb",
    "status-user",
    "status-volume-4",
    "status-wifi-4",
];

/// Rendu d'une icône : couleurs d'origine, teinte unique (symbolique), ou symbole blanc
/// sur une pastille colorée arrondie (comme les Réglages de macOS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconStyle {
    Color,
    Tinted(Color32),
    Tile(Color32),
}

/// Icônes SVG rasterisées à la demande et gardées en texture.
#[derive(Default)]
pub struct Icons {
    textures: HashMap<(&'static str, u32, IconStyle), Option<TextureHandle>>,
}

impl std::fmt::Debug for Icons {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Icons")
            .field("textures", &self.textures.len())
            .finish()
    }
}

impl Icons {
    fn texture(
        &mut self,
        ctx: &egui::Context,
        name: &'static str,
        size: f32,
        style: IconStyle,
    ) -> Option<TextureHandle> {
        let pixels = (size * ctx.pixels_per_point()).round().max(1.0) as u32;
        self.textures
            .entry((name, pixels, style))
            .or_insert_with(|| {
                let image = match style {
                    IconStyle::Color => rasterize(name, pixels, None)?,
                    IconStyle::Tinted(ink) => rasterize(name, pixels, Some(ink))?,
                    IconStyle::Tile(_) => rasterize(name, pixels, Some(Color32::WHITE))?,
                };
                Some(ctx.load_texture(name, image, TextureOptions::LINEAR))
            })
            .clone()
    }

    /// Dessine l'icône dans `rect`, sans créer de widget (donc sans capter la souris).
    pub fn paint(&mut self, ui: &egui::Ui, name: &'static str, rect: egui::Rect, style: IconStyle) {
        if let IconStyle::Tile(color) = style {
            ui.painter().rect_filled(rect, rect.width() * 0.24, color);
            let inner = rect.shrink(rect.width() * 0.1);
            if let Some(texture) = self.texture(ui.ctx(), name, inner.width(), style) {
                paint_texture(ui, &texture, inner);
            }
            return;
        }
        if let Some(texture) = self.texture(ui.ctx(), name, rect.width(), style) {
            paint_texture(ui, &texture, rect);
        }
    }
}

fn paint_texture(ui: &egui::Ui, texture: &TextureHandle, rect: egui::Rect) {
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
}

/// Ligne de barre latérale : icône et texte dessinés, toute la ligne est cliquable.
pub fn sidebar_item(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    icon: &'static str,
    icon_style: IconStyle,
    text: &str,
    selected: bool,
) -> egui::Response {
    let height = 28.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected {
        ui.painter().rect_filled(rect, 7.0, SIDEBAR_SELECTED);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 7.0, Color32::from_black_alpha(9));
    }
    let icon_size = 18.0;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + icon_size / 2.0, rect.center().y),
        egui::vec2(icon_size, icon_size),
    );
    icons.paint(ui, icon, icon_rect, icon_style);
    let font = FontId::proportional(13.5);
    ui.painter().text(
        egui::pos2(icon_rect.right() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font,
        TEXT,
    );
    response
}

/// Ligne de colonne des Paramètres : choisie, elle se remplit de bleu, texte blanc
/// (comme les Réglages de macOS).
pub fn sidebar_item_filled(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    icon: &'static str,
    icon_style: IconStyle,
    text: &str,
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected {
        ui.painter().rect_filled(rect, 8.0, ACCENT);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 8.0, Color32::from_black_alpha(10));
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 20.0, rect.center().y),
        egui::vec2(22.0, 22.0),
    );
    icons.paint(ui, icon, icon_rect, icon_style);
    ui.painter().text(
        egui::pos2(icon_rect.right() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(13.5),
        if selected { Color32::WHITE } else { TEXT },
    );
    response
}

/// Carte de l'utilisateur en haut de la colonne des Paramètres : avatar à l'initiale,
/// nom et identifiant.
pub fn account_card(ui: &mut egui::Ui, name: &str, login: &str, selected: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter();
    let fill = if selected {
        ACCENT
    } else if response.hovered() {
        Color32::from_black_alpha(10)
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 10.0, fill);
    let avatar = egui::pos2(rect.left() + 28.0, rect.center().y);
    painter.circle_filled(avatar, 19.0, Color32::from_rgb(0x8E, 0x93, 0x9E));
    let initial: String = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    painter.text(
        avatar,
        egui::Align2::CENTER_CENTER,
        initial,
        FontId::new(17.0, strong_family()),
        Color32::WHITE,
    );
    let (ink, secondary) = if selected {
        (Color32::WHITE, Color32::from_white_alpha(210))
    } else {
        (TEXT, TEXT_SECONDARY)
    };
    painter.text(
        egui::pos2(rect.left() + 56.0, rect.center().y - 8.0),
        egui::Align2::LEFT_CENTER,
        name,
        FontId::new(13.5, strong_family()),
        ink,
    );
    painter.text(
        egui::pos2(rect.left() + 56.0, rect.center().y + 9.0),
        egui::Align2::LEFT_CENTER,
        format!("Compte Ferrite · {login}"),
        FontId::proportional(11.5),
        secondary,
    );
    response
}

/// Flèche précédent (`forward` faux) ou suivant d'une barre d'outils.
pub fn arrow_button(ui: &mut egui::Ui, forward: bool, enabled: bool) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(30.0, 26.0), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 6.0, Color32::from_black_alpha(12));
    }
    let color = if enabled {
        TEXT
    } else {
        Color32::from_rgb(0xBB, 0xBB, 0xC2)
    };
    let c = rect.center();
    let s = if forward { -1.0 } else { 1.0 };
    ui.painter().line(
        vec![
            c + egui::vec2(3.0 * s, -6.0),
            c + egui::vec2(-3.0 * s, 0.0),
            c + egui::vec2(3.0 * s, 6.0),
        ],
        Stroke::new(1.8, color),
    );
    response
}

/// Titre de groupe d'une barre latérale (« Favoris », « Appareils »…).
pub fn sidebar_heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(text)
            .size(11.5)
            .color(Color32::from_rgb(0x8E, 0x8E, 0x93)),
    );
    ui.add_space(2.0);
}

fn rasterize(name: &str, side: u32, ink: Option<Color32>) -> Option<egui::ColorImage> {
    use resvg::{tiny_skia, usvg};
    let (_, source) = SOURCES.iter().find(|(candidate, _)| *candidate == name)?;
    let source = match ink {
        Some(ink) => ferrite_ui::icons::symbolic(
            source,
            Color::rgb(u32::from_be_bytes([0, ink.r(), ink.g(), ink.b()])),
        ),
        None => (*source).to_owned(),
    };
    let tree = usvg::Tree::from_str(&source, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(side, side)?;
    let factor = side as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(factor, factor),
        &mut pixmap.as_mut(),
    );
    Some(egui::ColorImage::from_rgba_premultiplied(
        [side as usize, side as usize],
        pixmap.data(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_icones_des_applications_sont_lisibles() {
        for (name, _) in SOURCES {
            let image = rasterize(name, 24, None).unwrap_or_else(|| panic!("{name} illisible"));
            assert!(
                image.pixels.iter().any(|pixel| pixel.a() > 200),
                "{name} est vide"
            );
        }
    }
}
