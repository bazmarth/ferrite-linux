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
pub const SELECTION: Color32 = Color32::from_rgb(0xD9, 0xE8, 0xFA);
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
    egui::Panel::top("ferrite-header")
        .resizable(false)
        .frame(Frame::new().fill(HEADER).inner_margin(Margin::symmetric(10, 8)))
        .show(ui, |ui| ui.horizontal(add));
}

/// Barre d'état discrète en bas de la fenêtre.
pub fn status_bar(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Panel::bottom("ferrite-status")
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

/// Bouton d'outil carré, sans cadre au repos.
pub fn tool_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(text).size(16.0))
            .frame_when_inactive(false)
            .min_size(egui::vec2(32.0, 28.0)),
    )
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
    "app-video",
    "file-generic",
    "file-text",
    "status-storage",
    "status-user",
];

/// Icônes SVG rasterisées à la demande et gardées en texture.
#[derive(Default)]
pub struct Icons {
    textures: HashMap<(&'static str, u32), Option<TextureHandle>>,
}

impl std::fmt::Debug for Icons {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Icons")
            .field("textures", &self.textures.len())
            .finish()
    }
}

impl Icons {
    /// Affiche l'icône `name` en carré de `size` points.
    pub fn show(&mut self, ui: &mut egui::Ui, name: &'static str, size: f32) -> egui::Response {
        let pixels = (size * ui.ctx().pixels_per_point()).round().max(1.0) as u32;
        let texture = self
            .textures
            .entry((name, pixels))
            .or_insert_with(|| {
                let image = rasterize(name, pixels)?;
                Some(ui.ctx().load_texture(name, image, TextureOptions::LINEAR))
            })
            .clone();
        match texture {
            Some(texture) => ui.add(egui::Image::new((texture.id(), egui::vec2(size, size)))),
            None => ui.allocate_response(egui::vec2(size, size), egui::Sense::hover()),
        }
    }
}

fn rasterize(name: &str, side: u32) -> Option<egui::ColorImage> {
    use resvg::{tiny_skia, usvg};
    let (_, source) = SOURCES.iter().find(|(candidate, _)| *candidate == name)?;
    let tree = usvg::Tree::from_str(source, &usvg::Options::default()).ok()?;
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
            let image = rasterize(name, 24).unwrap_or_else(|| panic!("{name} illisible"));
            assert!(
                image.pixels.iter().any(|pixel| pixel.a() > 200),
                "{name} est vide"
            );
        }
    }
}
