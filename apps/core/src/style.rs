//! Apparence commune des applications Ferrite, reprise du Ferrite d'origine
//! (`D:\Os\user\src\gui.rs`) : un panneau latéral gris flottant aux coins arrondis, avec
//! les trois boutons de fenêtre en haut ; sans panneau, une barre d'outils en dégradé ;
//! des groupes de boutons en pilule. Les applications dessinent leur haut elles-mêmes ;
//! ferrite-shell garde les coins arrondis, l'ombre et les bords.
//! Aussi : couleurs de la charte, polices, et icônes SVG du dossier `icons/SVG`.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU32, Ordering},
    },
};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Stroke,
    TextStyle, TextureHandle, TextureOptions,
};
use ferrite_system::config::{Accent, DockSize, ScrollBars, Settings};
use ferrite_ui::{
    theme::{Color, Palette},
    tokens::{border, control, radius, spacing, typography},
};

/// Marge autour du panneau latéral, et rayon de ses coins.
const PANEL_MARGIN: f32 = spacing::MEDIUM;
const PANEL_RADIUS: u8 = radius::PANEL;
/// Hauteur de la barre d'outils, et de la zone des boutons en haut de la colonne.
pub const TOOLBAR_HEIGHT: f32 = 64.0;
/// Préférences d'apparence en vigueur (voir [`apply_preferences`]) : accentuation en
/// `0xRRGGBB`, et taille des icônes des barres latérales en pour cent.
static THEME_DARK: AtomicU8 = AtomicU8::new(0);
static ACCENT_RGB: AtomicU32 = AtomicU32::new(0x000A7AFF);
static SIDEBAR_SCALE: AtomicU32 = AtomicU32::new(100);

fn palette() -> Palette {
    if THEME_DARK.load(Ordering::Relaxed) == 1 {
        Palette::DARK
    } else {
        Palette::LIGHT
    }
}

pub fn background() -> Color32 {
    color(palette().window_background)
}

pub fn theme_is_dark() -> bool {
    THEME_DARK.load(Ordering::Relaxed) == 1
}

pub fn header_color() -> Color32 {
    color(palette().header_background)
}

pub fn content_gray() -> Color32 {
    color(palette().content_background)
}

pub fn control_background() -> Color32 {
    color(palette().control_pressed)
}

pub fn control_hover() -> Color32 {
    color(palette().control_hover)
}

pub fn sidebar_color() -> Color32 {
    color(palette().sidebar_background)
}

pub fn primary_text() -> Color32 {
    color(palette().text)
}

pub fn text_secondary() -> Color32 {
    color(palette().text_secondary)
}

pub fn separator() -> Color32 {
    color(palette().control_separator)
}

pub fn sidebar_selected() -> Color32 {
    Color32::from_black_alpha(if THEME_DARK.load(Ordering::Relaxed) == 1 {
        48
    } else {
        22
    })
}

pub fn error() -> Color32 {
    color(palette().danger)
}

const fn color(color: Color) -> Color32 {
    Color32::from_rgb(color.r, color.g, color.b)
}

const fn rgb_of(color: Color32) -> u32 {
    u32::from_be_bytes([0, color.r(), color.g(), color.b()])
}

fn from_rgb(rgb: u32) -> Color32 {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

/// Couleur d'accentuation choisie dans Paramètres.
pub fn accent() -> Color32 {
    from_rgb(ACCENT_RGB.load(Ordering::Relaxed))
}

pub fn accent_text() -> Color32 {
    let accent = accent();
    let brightness =
        (299 * u32::from(accent.r()) + 587 * u32::from(accent.g()) + 114 * u32::from(accent.b())) / 1000;
    if brightness > 145 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

/// Couleur d'une accentuation de Paramètres (« Multicolore » : celle de Ferrite).
pub fn accent_color(accent: Accent) -> Color32 {
    accent.rgb().map_or(color(palette().accent), from_rgb)
}

/// Facteur de taille des lignes et icônes des barres latérales.
fn sidebar_scale() -> f32 {
    SIDEBAR_SCALE.load(Ordering::Relaxed) as f32 / 100.0
}

/// Applique les préférences d'apparence de Paramètres : accentuation, surlignage du
/// texte, taille des icônes des barres latérales et barres de défilement.
pub fn apply_preferences(context: &egui::Context, settings: &Settings) {
    THEME_DARK.store(u8::from(settings.appearance().is_dark()), Ordering::Relaxed);
    context.set_theme(if settings.appearance().is_dark() {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let accent = accent_color(settings.accent());
    ACCENT_RGB.store(rgb_of(accent), Ordering::Relaxed);
    let scale = match settings.sidebar_icons() {
        DockSize::Small => 85,
        DockSize::Medium => 100,
        DockSize::Large => 120,
    };
    SIDEBAR_SCALE.store(scale, Ordering::Relaxed);
    apply_theme(context);
    // Surlignage clair, comme sur macOS : le texte choisi reste lisible sur les deux thèmes.
    let highlight = settings.highlight().map_or(accent, accent_color).lerp_to_gamma(
        if THEME_DARK.load(Ordering::Relaxed) == 1 {
            color(palette().window_background)
        } else {
            Color32::WHITE
        },
        0.6,
    );
    let scroll = match settings.scroll_bars() {
        ScrollBars::Auto => egui::style::ScrollStyle::thin(),
        ScrollBars::WhenScrolling => egui::style::ScrollStyle::floating(),
        ScrollBars::Always => egui::style::ScrollStyle::solid(),
    };
    context.all_styles_mut(|style| {
        style.spacing.scroll = scroll;
        style.visuals.selection.bg_fill = highlight;
        style.visuals.selection.stroke = Stroke::new(border::HAIRLINE, primary_text());
        style.visuals.text_cursor.stroke = Stroke::new(2.0, accent);
    });
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

    apply_theme(context);
    apply_preferences(context, &Settings::load());
}

fn apply_theme(context: &egui::Context) {
    let palette = palette();
    let background = color(palette.window_background);
    let text = color(palette.text);
    context.all_styles_mut(|style| {
        style.visuals.dark_mode = THEME_DARK.load(Ordering::Relaxed) == 1;
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(typography::SMALL)),
            (TextStyle::Body, FontId::proportional(typography::BODY)),
            (TextStyle::Button, FontId::proportional(typography::BODY)),
            (
                TextStyle::Heading,
                FontId::new(typography::HEADING, strong_family()),
            ),
            (TextStyle::Monospace, FontId::monospace(typography::MONOSPACE)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(spacing::MEDIUM, spacing::SMALL);
        style.spacing.button_padding = egui::vec2(spacing::CONTROL_HORIZONTAL, spacing::CONTROL_VERTICAL);
        style.spacing.interact_size.y = control::HEIGHT;
        // Les libellés sont du texte d'interface, pas du contenu : ni sélection ni capture
        // du clic (le clic doit atteindre la ligne ou le bouton dessous).
        style.interaction.selectable_labels = false;

        let visuals = &mut style.visuals;
        visuals.panel_fill = background;
        visuals.window_fill = background;
        visuals.extreme_bg_color = background;
        visuals.faint_bg_color = color(palette.content_background);
        visuals.override_text_color = Some(text);

        let radius = CornerRadius::same(radius::CONTROL);
        let border = Stroke::new(border::HAIRLINE, color(palette.border));
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_stroke = Stroke::new(border::HAIRLINE, color(palette.control_separator));
        widgets.noninteractive.fg_stroke = Stroke::new(border::HAIRLINE, text);
        for (state, fill) in [
            (&mut widgets.inactive, background),
            (&mut widgets.hovered, color(palette.control_hover)),
            (&mut widgets.active, color(palette.control_pressed)),
            (&mut widgets.open, color(palette.control_pressed)),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = border;
            state.fg_stroke = Stroke::new(border::HAIRLINE, text);
            state.corner_radius = radius;
            state.expansion = 0.0;
        }
        widgets.hovered.bg_stroke = Stroke::new(border::HAIRLINE, color(palette.border_hover));
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
        .frame(Frame::new().fill(background()).inner_margin(Margin {
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
                vertical_gradient(
                    ui.painter(),
                    full,
                    color(palette().toolbar_top),
                    color(palette().toolbar_bottom),
                );
                ui.painter().hline(
                    full.x_range(),
                    full.bottom() - 0.5,
                    Stroke::new(border::HAIRLINE, color(palette().separator)),
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
        .frame(Frame::new().fill(background()))
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
                Stroke::new(border::HAIRLINE, Color32::from_black_alpha(22)),
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

/// Colonne pleine hauteur collée au bord gauche, comme celle des Réglages de macOS :
/// dégradé vertical (teinté par le fond d'écran), trait à droite, boutons de fenêtre en
/// haut ; la bande du haut déplace la fenêtre.
pub fn tinted_sidebar(
    ui: &mut egui::Ui,
    id: &str,
    width: f32,
    (top, bottom): (Color32, Color32),
    add: impl FnOnce(&mut egui::Ui),
) {
    egui::Panel::left(ui.scope_id().with(id))
        .resizable(false)
        .exact_size(width)
        .frame(Frame::new())
        .show(ui, |ui| {
            let full = ui.max_rect();
            vertical_gradient(ui.painter(), full, top, bottom);
            ui.painter().vline(
                full.right() - 0.5,
                full.y_range(),
                Stroke::new(border::HAIRLINE, Color32::from_black_alpha(26)),
            );
            let header = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), TOOLBAR_HEIGHT));
            drag_area(ui, header, "colonne");
            window_buttons(ui, egui::pos2(full.left() + 24.0, header.center().y));
            let body = egui::Rect::from_min_max(
                egui::pos2(full.left() + 14.0, header.bottom()),
                egui::pos2(full.right() - 14.0, full.bottom() - 8.0),
            );
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
            add(&mut child);
        });
}

/// Groupe de boutons en forme de pilule, avec contour et ombre légers. Les boutons de `add`
/// sont collés.
pub fn pill_group<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new()
        .fill(background())
        .stroke(Stroke::new(border::HAIRLINE, color(palette().border)))
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

/// Contrôle segmenté : rail contrasté dans lequel le segment choisi est en relief.
/// Les segments de `add` dessinent eux-mêmes leur état (voir [`segment_background`]).
pub fn segmented<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new()
        .fill(control_background())
        .corner_radius(8)
        .inner_margin(Margin::symmetric(2, 2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(add).inner
        })
        .inner
}

/// Fond d'un segment de [`segmented`] : surface de fenêtre en relief s'il est choisi.
pub fn segment_background(ui: &egui::Ui, rect: egui::Rect, selected: bool, hovered: bool) {
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect.translate(egui::vec2(0.0, 1.0)), 6.0, control_background());
        painter.rect_filled(rect, 6.0, background());
    } else if hovered {
        painter.rect_filled(rect, 6.0, control_hover());
    }
}

/// Zone vide qui déplace la fenêtre ; un double-clic l'agrandit ou la rétablit.
pub fn drag_area(ui: &mut egui::Ui, rect: egui::Rect, salt: &str) {
    let response = ui.interact(
        rect,
        ui.scope_id().with(("deplacer", salt)),
        egui::Sense::click_and_drag(),
    );
    if response.double_clicked() {
        // Basculer uniquement l'agrandissement : aucun mouvement de fenêtre
        // ne doit démarrer entre les deux clics.
        let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if response.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
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
    let active_palette = palette();
    let [close, minimize, zoom] = active_palette.traffic_lights;
    let (off_fill, off_edge) = active_palette.traffic_light_inactive;
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
        .frame(
            Frame::new()
                .fill(header_color())
                .inner_margin(Margin::symmetric(12, 7)),
        )
        .show(ui, |ui| {
            let full = ui.max_rect().expand2(egui::vec2(12.0, 5.0));
            ui.painter().hline(
                full.x_range(),
                full.top() + 0.5,
                Stroke::new(border::HAIRLINE, separator()),
            );
            ui.horizontal(|ui| {
                ui.style_mut().override_text_style = Some(TextStyle::Small);
                ui.visuals_mut().override_text_color = Some(text_secondary());
                add(ui);
            })
        });
}

/// Bouton mis en avant (action principale) : fond bleu, texte blanc.
pub fn suggested_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(accent_text()))
            .fill(accent())
            .stroke(Stroke::new(1.0, accent().lerp_to_gamma(Color32::BLACK, 0.15))),
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
        control_background()
    } else if *on {
        accent()
    } else {
        separator()
    };
    let painter = ui.painter();
    painter.rect_filled(rect, rect.height() / 2.0, track);
    let radius = rect.height() / 2.0 - 2.5;
    let x = egui::lerp(rect.left() + radius + 2.5..=rect.right() - radius - 2.5, progress);
    painter.circle_filled(egui::pos2(x, rect.center().y), radius, background());
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
    "settings-computer",
    "settings-wifi",
    "settings-bluetooth",
    "settings-sound",
    "settings-dock",
    "settings-desktop",
    "settings-keyboard",
    "settings-clock",
    "device-headphones",
    "device-mouse",
    "device-phone",
];

/// Rendu d'une icône : couleurs d'origine, ou teinte unique (symbolique).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconStyle {
    Color,
    Tinted(Color32),
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
                };
                Some(ctx.load_texture(name, image, TextureOptions::LINEAR))
            })
            .clone()
    }

    /// Dessine l'icône dans `rect`, sans créer de widget (donc sans capter la souris).
    pub fn paint(&mut self, ui: &egui::Ui, name: &'static str, rect: egui::Rect, style: IconStyle) {
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
    let height = 28.0 * sidebar_scale();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected {
        ui.painter().rect_filled(rect, 7.0, sidebar_selected());
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 7.0, Color32::from_black_alpha(9));
    }
    let icon_size = 18.0 * sidebar_scale();
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
        primary_text(),
    );
    response
}

/// Ligne de colonne des Paramètres, comme dans les Réglages de macOS : pastille colorée
/// ombrée avec un symbole blanc, puis le titre ; choisie, la ligne se voile de gris.
pub fn sidebar_tile_item(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    icon: &'static str,
    tile: Color32,
    text: &str,
    selected: bool,
) -> egui::Response {
    let scale = sidebar_scale();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 38.0 * scale),
        egui::Sense::click(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected {
        ui.painter().rect_filled(rect, 9.0, Color32::from_black_alpha(20));
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 9.0, Color32::from_black_alpha(9));
    }
    let side = 28.0 * scale;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + side / 2.0, rect.center().y),
        egui::Vec2::splat(side),
    );
    let corner = side * 0.24;
    ui.painter().rect_filled(
        icon_rect.translate(egui::vec2(0.0, 1.5)).expand(0.5),
        corner,
        Color32::from_black_alpha(30),
    );
    ui.painter().rect_filled(icon_rect, corner, tile);
    icons.paint(
        ui,
        icon,
        icon_rect.shrink(side * 0.2),
        IconStyle::Tinted(Color32::WHITE),
    );
    ui.painter().text(
        egui::pos2(icon_rect.right() + 14.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(14.0),
        primary_text(),
    );
    response
}

/// Titre de groupe d'une barre latérale (« Favoris », « Appareils »…).
pub fn sidebar_heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text).size(11.5).color(text_secondary()));
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
    fn adaptateur_egui_applique_les_tokens_partages() {
        let context = egui::Context::default();
        apply(&context);
        let style = context.style_of(egui::Theme::Light);

        assert_eq!(style.spacing.interact_size.y, control::HEIGHT);
        assert_eq!(
            style.spacing.button_padding,
            egui::vec2(spacing::CONTROL_HORIZONTAL, spacing::CONTROL_VERTICAL)
        );
        assert_eq!(style.visuals.window_fill, background());
        assert_eq!(error(), color(palette().danger));
    }

    #[test]
    fn changement_de_theme_met_a_jour_la_palette_egui() {
        let context = egui::Context::default();
        let dark = Settings::parse("apparence = sombre\naccent = jaune\n");
        apply_preferences(&context, &dark);
        assert_eq!(background(), color(Palette::DARK.window_background));
        assert_eq!(primary_text(), color(Palette::DARK.text));
        assert_eq!(accent_text(), Color32::BLACK);
        assert!(context.style_of(egui::Theme::Dark).visuals.dark_mode);

        let light = Settings::parse("apparence = clair\n");
        apply_preferences(&context, &light);
        assert_eq!(background(), color(Palette::LIGHT.window_background));
        assert_eq!(primary_text(), color(Palette::LIGHT.text));
    }

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
