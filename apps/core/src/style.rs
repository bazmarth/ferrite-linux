//! Apparence commune des applications Ferrite : couleurs de la charte, polices,
//! barre d'en-tête prolongeant la barre de titre du compositeur, et icônes SVG du
//! dossier `icons/SVG`.

use std::{collections::HashMap, sync::Arc};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin, Stroke,
    TextStyle, TextureHandle, TextureOptions,
};
use ferrite_ui::theme::{Color, Palette};

const PALETTE: Palette = Palette::LIGHT;

/// En-tête et barre d'état : même teinte que la barre de titre, pour ne former qu'un bloc.
pub const HEADER: Color32 = color(PALETTE.titlebar);
pub const BACKGROUND: Color32 = color(PALETTE.window_background);
pub const SIDEBAR: Color32 = Color32::from_rgb(0xEF, 0xEF, 0xF1);
pub const TEXT: Color32 = color(PALETTE.text);
pub const TEXT_SECONDARY: Color32 = color(PALETTE.text_secondary);
pub const SEPARATOR: Color32 = color(PALETTE.separator);
pub const ACCENT: Color32 = Color32::from_rgb(0x36, 0x89, 0xE6);
/// Ligne choisie d'une barre latérale : gris neutre, comme macOS et elementary OS.
pub const SIDEBAR_SELECTED: Color32 = Color32::from_rgb(0xDD, 0xDD, 0xE2);
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

/// Barre d'en-tête (boutons, chemin…) collée sous la barre de titre.
pub fn header(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Panel::top(ui.scope_id().with("ferrite-header"))
        .resizable(false)
        .frame(Frame::new().fill(HEADER).inner_margin(Margin::symmetric(10, 8)))
        .show(ui, |ui| ui.horizontal(add));
}

/// Barre d'état discrète en bas de la fenêtre.
pub fn status_bar(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Panel::bottom(ui.scope_id().with("ferrite-status"))
        .resizable(false)
        .frame(Frame::new().fill(HEADER).inner_margin(Margin::symmetric(12, 5)))
        .show(ui, |ui| {
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
            .stroke(Stroke::new(1.0, Color32::from_rgb(0x2A, 0x74, 0xC9))),
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
    let height = 32.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected {
        ui.painter().rect_filled(rect, 8.0, SIDEBAR_SELECTED);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 8.0, Color32::from_black_alpha(9));
    }
    let icon_size = 20.0;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 10.0 + icon_size / 2.0, rect.center().y),
        egui::vec2(icon_size, icon_size),
    );
    icons.paint(ui, icon, icon_rect, icon_style);
    let font = if selected {
        FontId::new(13.5, strong_family())
    } else {
        FontId::proportional(13.5)
    };
    ui.painter().text(
        egui::pos2(icon_rect.right() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font,
        TEXT,
    );
    response
}

/// Titre de groupe d'une barre latérale (« Favoris », « Appareils »…).
pub fn sidebar_heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(text)
            .size(11.5)
            .family(strong_family())
            .color(TEXT_SECONDARY),
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
