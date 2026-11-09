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
use ferrite_system::config::{Accent, DockSize, DoubleClick, ScrollBars, Settings};
use ferrite_ui::{
    theme::{Color, Palette},
    tokens::{border, control, radius, spacing, typography},
};

/// Hauteur de la barre d'outils, et de la zone des boutons en haut de la colonne.
pub const TOOLBAR_HEIGHT: f32 = 42.0;
/// Préférences d'apparence en vigueur (voir [`apply_preferences`]) : accentuation en
/// `0xRRGGBB`, et taille des icônes des barres latérales en pour cent.
static THEME_DARK: AtomicU8 = AtomicU8::new(0);
/// Application toujours sombre, quel que soit le thème du bureau (le Terminal).
static ALWAYS_DARK: AtomicU8 = AtomicU8::new(0);
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

// ---- Rôles de couleur communs ----------------------------------------------------------
// Les applications prennent ces rôles plutôt que d'écrire leurs propres couleurs : une
// même chose a la même teinte partout, dans les deux thèmes (`tools/check-colors.sh`).

/// Rail d'un curseur ou d'une jauge, sous la part remplie.
pub fn track() -> Color32 {
    color(palette().track)
}

/// Filet sous une barre ou entre deux zones de la fenêtre (plus marqué que
/// [`separator`], qui sépare les lignes d'une carte).
pub fn line() -> Color32 {
    color(palette().separator)
}

/// Voile sous une fenêtre de dialogue.
pub fn scrim() -> Color32 {
    themed(
        Color32::from_rgba_unmultiplied(15, 23, 42, 71),
        Color32::from_black_alpha(115),
    )
}

/// Ombre d'un panneau qui flotte au-dessus du contenu.
pub fn shadow() -> Color32 {
    themed(
        Color32::from_rgba_unmultiplied(15, 23, 42, 64),
        Color32::from_black_alpha(140),
    )
}

/// Segment choisi d'un contrôle à segments, sur [`control_background`].
pub fn segment_on() -> Color32 {
    themed(Color32::WHITE, color(palette().border_hover))
}

/// Bord d'un champ ou d'une case à cocher.
pub fn border() -> Color32 {
    color(palette().border_hover)
}

/// Scène d'une image, d'une page ou d'une vidéo : un peu plus sombre que la fenêtre,
/// pour que le document s'en détache.
pub fn stage() -> Color32 {
    color(palette().stage)
}

/// Commandes flottantes sur une scène (zoom, page suivante) : verre clair ou sombre.
pub fn floating() -> Color32 {
    themed(
        Color32::from_white_alpha(204),
        Color32::from_rgba_unmultiplied(28, 31, 37, 184),
    )
}

pub fn floating_ring() -> Color32 {
    tint_ring()
}

fn tint_ring() -> Color32 {
    themed(Color32::from_black_alpha(20), Color32::from_white_alpha(31))
}

/// Contraste fort : fond foncé en thème clair, clair en thème sombre (bouton lecture,
/// bandeau « Annuler »).
pub fn inverse() -> Color32 {
    color(palette().inverse)
}

/// Texte sur [`inverse`].
pub fn inverse_text() -> Color32 {
    color(palette().inverse_text)
}

/// Action sur [`inverse`] (« Annuler » d'un bandeau).
pub fn inverse_action() -> Color32 {
    color(palette().inverse_action)
}

/// Texte de titre de partie, entre le texte et le texte secondaire.
pub fn heading_text() -> Color32 {
    color(palette().heading)
}

/// Texte discret (aide, indications), plus pâle que le texte secondaire.
pub fn tertiary_text() -> Color32 {
    color(palette().window_title_inactive)
}

/// Réussite, enregistré, meilleur tour.
pub fn success() -> Color32 {
    color(palette().success_text)
}

/// Attention, modifié, niveau élevé.
pub fn warning() -> Color32 {
    color(palette().warning)
}

pub fn control_hover() -> Color32 {
    color(palette().control_hover)
}

pub fn sidebar_color() -> Color32 {
    color(palette().sidebar_background)
}

/// Ligne de menu contextuel : 32 px, raccourci à droite, surlignée au survol, rouge si
/// l'action détruit quelque chose.
pub fn menu_row(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: &str,
    destructive: bool,
    enabled: bool,
) -> egui::Response {
    let width = MENU_WIDTH - 12.0;
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), sense);
    let hovered = enabled && response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, CornerRadius::same(6), accent());
    }
    let (ink, secondary) = if hovered {
        (accent_text(), accent_text())
    } else if !enabled {
        (
            text_secondary().gamma_multiply(0.6),
            text_secondary().gamma_multiply(0.6),
        )
    } else if destructive {
        (Color32::from_rgb(0xC4, 0x2B, 0x20), text_secondary())
    } else {
        (primary_text(), text_secondary())
    };
    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        ink,
    );
    if shortcut == "›" {
        let (x, y) = (rect.right() - 13.0, rect.center().y);
        ui.painter().line_segment(
            [egui::pos2(x - 2.0, y - 4.0), egui::pos2(x + 2.0, y)],
            egui::Stroke::new(1.4, secondary),
        );
        ui.painter().line_segment(
            [egui::pos2(x + 2.0, y), egui::pos2(x - 2.0, y + 4.0)],
            egui::Stroke::new(1.4, secondary),
        );
    } else if !shortcut.is_empty() {
        ui.painter().text(
            egui::pos2(rect.right() - 10.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            shortcut,
            egui::FontId::proportional(12.0),
            secondary,
        );
    }
    response
}

/// Séparateur d'un menu contextuel.
pub fn menu_separator(ui: &mut egui::Ui) {
    let width = MENU_WIDTH - 12.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 9.0), egui::Sense::hover());
    ui.painter().hline(
        (rect.left() + 6.0)..=(rect.right() - 6.0),
        rect.center().y,
        egui::Stroke::new(1.0, separator()),
    );
}

/// Espacement d'un menu contextuel : les lignes se touchent.
pub fn menu_style(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.set_width(MENU_WIDTH - 12.0);
}

/// Bouton de liste à choix unique, dans un popover : 32 px, rond plein quand il est choisi.
pub fn radio_row(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), separator().gamma_multiply(0.5));
    }
    let center = egui::pos2(rect.left() + 14.0, rect.center().y);
    if selected {
        ui.painter()
            .circle_stroke(center, 6.5, egui::Stroke::new(1.5, accent()));
        ui.painter().circle_filled(center, 3.5, accent());
    } else {
        ui.painter()
            .circle_stroke(center, 6.5, egui::Stroke::new(1.2, text_secondary()));
    }
    ui.painter().text(
        egui::pos2(rect.left() + 30.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        primary_text(),
    );
    response
}

/// Largeur des menus contextuels.
pub const MENU_WIDTH: f32 = 260.0;

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
    color(palette().selection)
}

/// Texte d'un élément choisi dans une barre latérale.
pub fn sidebar_selected_text() -> Color32 {
    color(palette().selection_text)
}

/// Valeur selon le thème en vigueur.
pub fn themed<T>(light: T, dark: T) -> T {
    if theme_is_dark() { dark } else { light }
}

/// Voile qui contraste avec le fond : noir en thème clair, blanc en thème sombre. Pour les
/// filets, les survols et les pastilles ; les ombres, elles, restent noires.
pub fn tint(alpha: u8) -> Color32 {
    themed(Color32::from_black_alpha(alpha), Color32::from_white_alpha(alpha))
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

/// Couleur d'une accentuation de Paramètres (le bleu Ferrite dépend du thème).
pub fn accent_color(accent: Accent) -> Color32 {
    accent.rgb().map_or(color(palette().accent), from_rgb)
}

/// Facteur de taille des lignes et icônes des barres latérales.
fn sidebar_scale() -> f32 {
    SIDEBAR_SCALE.load(Ordering::Relaxed) as f32 / 100.0
}

/// Garde cette application en thème sombre.
pub fn set_always_dark() {
    ALWAYS_DARK.store(1, Ordering::Relaxed);
}

/// Thème à afficher selon les réglages.
pub fn wants_dark(settings: &Settings) -> bool {
    ALWAYS_DARK.load(Ordering::Relaxed) == 1 || settings.appearance().is_dark()
}

/// Dernière taille du texte appliquée, en pour cent (0 : aucune encore).
static TEXT_SIZE: AtomicU32 = AtomicU32::new(0);

/// Applique les préférences d'apparence de Paramètres : accentuation, surlignage du
/// texte, taille des icônes des barres latérales et barres de défilement.
pub fn apply_preferences(context: &egui::Context, settings: &Settings) {
    let dark = wants_dark(settings);
    THEME_DARK.store(u8::from(dark), Ordering::Relaxed);
    context.set_theme(if dark {
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
    // Taille du texte (Paramètres → Apparence) : tout le contenu de la fenêtre grandit,
    // texte et commandes ensemble, pour que rien ne déborde. Appliquée seulement quand le
    // réglage change, pour ne pas défaire un zoom au clavier propre à l'application.
    let text_size = settings.text_size();
    if TEXT_SIZE.swap(text_size, Ordering::Relaxed) != text_size {
        context.set_zoom_factor(text_size as f32 / 100.0);
    }
    let reduce_motion = settings.flag("reduire_animations", false);
    context.all_styles_mut(|style| {
        style.animation_time = if reduce_motion { 0.0 } else { 1.0 / 12.0 };
        style.spacing.scroll = scroll;
        style.visuals.selection.bg_fill = highlight;
        style.visuals.selection.stroke = Stroke::new(border::HAIRLINE, primary_text());
        style.visuals.text_cursor.stroke = Stroke::new(2.0, accent);
    });
}

/// Polices (Inter et JetBrains Mono) et réglages visuels partagés.
pub fn apply(context: &egui::Context) {
    context.set_fonts(font_definitions());

    apply_theme(context);
    apply_preferences(context, &Settings::load());
}

/// Ajoute des polices de secours à la chasse fixe, pour les caractères absents de JetBrains
/// Mono (écritures d'Asie, symboles). Un fichier illisible est ignoré.
pub fn add_mono_fallbacks(context: &egui::Context, files: &[std::path::PathBuf]) {
    let mut fonts = font_definitions();
    let mut added = false;
    for file in files {
        let Ok(bytes) = std::fs::read(file) else {
            continue;
        };
        let name = format!("secours:{}", file.display());
        fonts
            .font_data
            .insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Monospace, mono_bold_family()] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
        added = true;
    }
    if added {
        context.set_fonts(fonts);
    }
}

pub(crate) fn font_definitions() -> FontDefinitions {
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
    // Emoji en couleur, juste après la police principale : avant les emoji en noir et
    // blanc fournis par egui.
    if let Some(emoji) = color_emoji() {
        fonts.font_data.insert("ferrite-emoji".into(), emoji);
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(1, "ferrite-emoji".into());
        }
    }
    fonts.font_data.insert(
        "ferrite-mono-bold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/JetBrainsMono-Bold.ttf"
        ))),
    );
    let mut mono_bold = vec!["ferrite-mono-bold".to_owned()];
    mono_bold.extend(fonts.families[&FontFamily::Monospace].iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name("mono-bold".into()), mono_bold);
    fonts.font_data.insert(
        "ferrite-inter-light".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/InterDisplay-Light.ttf"
        ))),
    );
    let mut light = vec!["ferrite-inter-light".to_owned()];
    light.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name("light".into()), light);
    let mut strong = vec!["ferrite-inter-semibold".to_owned()];
    strong.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name("strong".into()), strong);
    // Caractère suivi de U+FE0F (« ⚠️ », « ❤️ ») : emoji en couleur d'abord, même si Inter
    // a une version en noir de ce caractère. Sans police d'emoji, même liste que le texte.
    let mut emoji: Vec<String> = fonts
        .font_data
        .contains_key("ferrite-emoji")
        .then(|| "ferrite-emoji".to_owned())
        .into_iter()
        .collect();
    emoji.extend(
        fonts.families[&FontFamily::Proportional]
            .iter()
            .filter(|name| *name != "ferrite-emoji")
            .cloned(),
    );
    fonts.families.insert(emoji_family(), emoji);
    fonts
}

/// Famille des caractères en présentation emoji (suivis de U+FE0F).
pub fn emoji_family() -> FontFamily {
    FontFamily::Name("emoji".into())
}

/// Sélecteur de variante « emoji » : le caractère qui le précède veut être en couleur.
const EMOJI_SELECTOR: char = '\u{FE0F}';

/// Met chaque caractère suivi de U+FE0F dans la famille [`emoji_family`] : egui choisit la
/// police caractère par caractère, sans regarder le sélecteur, et prendrait sinon la version
/// en noir de « ⚠ » ou « ❤ » qu'ont Inter et JetBrains Mono.
pub fn emoji_presentation(job: &mut egui::text::LayoutJob) {
    if !job.text.contains(EMOJI_SELECTOR) {
        return;
    }
    let text = &job.text;
    let mut sections = Vec::with_capacity(job.sections.len() + 2);
    for section in job.sections.drain(..) {
        let range = section.byte_range.start.0..section.byte_range.end.0;
        let mut pieces: Vec<(std::ops::Range<usize>, bool)> = Vec::new();
        let mut start = range.start;
        let mut chars = text[range.clone()].char_indices().peekable();
        while let Some((at, ch)) = chars.next() {
            if ch == EMOJI_SELECTOR {
                continue;
            }
            if let Some(&(selector, EMOJI_SELECTOR)) = chars.peek() {
                let (at, end) = (
                    range.start + at,
                    range.start + selector + EMOJI_SELECTOR.len_utf8(),
                );
                pieces.push((start..at, false));
                pieces.push((at..end, true));
                start = end;
                chars.next();
            }
        }
        pieces.push((start..range.end, false));
        let mut first = true;
        for (byte_range, emoji) in pieces {
            if byte_range.is_empty() {
                continue;
            }
            let mut piece = section.clone();
            piece.byte_range = egui::text::ByteIndex(byte_range.start)..egui::text::ByteIndex(byte_range.end);
            if !first {
                piece.leading_space = 0.0;
            }
            if emoji {
                piece.format.font_id.family = emoji_family();
            }
            sections.push(piece);
            first = false;
        }
    }
    job.sections = sections;
}

/// Police d'emoji en couleur du système (Noto Color Emoji, paquet
/// `fonts-noto-color-emoji`), lue une fois par programme. Rien si elle manque.
fn color_emoji() -> Option<Arc<FontData>> {
    static EMOJI: std::sync::OnceLock<Option<Arc<FontData>>> = std::sync::OnceLock::new();
    EMOJI
        .get_or_init(|| {
            [
                "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
                "/usr/share/fonts/noto/NotoColorEmoji.ttf",
                "/usr/share/fonts/google-noto-color-emoji-fonts/NotoColorEmoji.ttf",
            ]
            .iter()
            .find_map(|path| std::fs::read(path).ok())
            .map(|bytes| Arc::new(FontData::from_owned(bytes)))
        })
        .clone()
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
        // Menus et popovers : coins de 10, ombre flottante.
        visuals.menu_corner_radius = CornerRadius::same(10);
        visuals.popup_shadow = egui::Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(77),
        };
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

/// Police à chasse fixe en gras, pour le terminal.
pub fn mono_bold_family() -> FontFamily {
    FontFamily::Name("mono-bold".into())
}

/// Inter Display Light : grands chiffres fins (Horloge).
pub fn light_family() -> FontFamily {
    FontFamily::Name("light".into())
}

pub fn strong_family() -> FontFamily {
    FontFamily::Name("strong".into())
}

/// Barre d'outils d'une fenêtre sans colonne : boutons de fenêtre à gauche, puis `add`.
pub fn header(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    toolbar(ui, true, add);
}

/// Barre d'outils en haut du contenu, sur le fond des barres de titre avec son filet. Elle
/// porte le bouton agrandir à droite ; avec `buttons` (fenêtre sans barre latérale), aussi le
/// bouton fermer à gauche. Le vide sert à déplacer la fenêtre.
pub fn toolbar(ui: &mut egui::Ui, buttons: bool, add: impl FnOnce(&mut egui::Ui)) {
    toolbar_with(ui, buttons, header_color(), color(palette().titlebar_line), add);
}

/// Comme [`toolbar`], sur le fond `fill` et avec le filet `line` (Fichiers reprend les
/// teintes de sa planche).
pub fn toolbar_with(
    ui: &mut egui::Ui,
    buttons: bool,
    fill: Color32,
    line: Color32,
    add: impl FnOnce(&mut egui::Ui),
) {
    // Hors de Ferrite, les trois boutons (réduire, agrandir, fermer) sont à droite.
    let native = inside_ferrite();
    let left: i8 = if buttons && native { 46 } else { 14 };
    let right: i8 = if native { 46 } else { 106 };
    egui::Panel::top(ui.scope_id().with("ferrite-header"))
        .resizable(false)
        .exact_size(TOOLBAR_HEIGHT)
        .frame(Frame::new().fill(fill).inner_margin(Margin {
            left,
            right,
            top: 0,
            bottom: 0,
        }))
        .show(ui, |ui| {
            let inner = ui.max_rect();
            let full = egui::Rect::from_min_max(
                egui::pos2(inner.left() - left as f32, inner.top()),
                egui::pos2(inner.right() + right as f32, inner.bottom()),
            );
            ui.painter().hline(
                full.x_range(),
                full.bottom() - 0.5,
                Stroke::new(border::HAIRLINE, line),
            );
            // Fond déplaçable, enregistré avant les boutons pour qu'ils restent prioritaires.
            drag_area(ui, full, "barre");
            let inset = ferrite_ui::theme::WINDOW_BUTTON_INSET as f32;
            if !native {
                let at =
                    |rank: f32| egui::pos2(full.right() - inset - rank * WINDOW_BUTTON_GAP, full.center().y);
                close_button(ui, at(0.0));
                zoom_button(ui, at(1.0));
                minimize_button(ui, at(2.0));
            } else {
                if buttons {
                    close_button(ui, egui::pos2(full.left() + inset, full.center().y));
                }
                zoom_button(ui, egui::pos2(full.right() - inset, full.center().y));
            }
            // Centré en hauteur, sans étirer les éléments (pastilles, onglets).
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), add);
        });
}

/// Barre latérale pleine hauteur, collée au bord gauche, avec son filet à droite et le bouton
/// fermer en haut (sauf `buttons` faux : sélecteur de fichiers dans une feuille).
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
        .frame(Frame::new().fill(fill))
        .show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter().vline(
                full.right() - 0.5,
                full.y_range(),
                Stroke::new(border::HAIRLINE, color(palette().titlebar_line)),
            );
            let top = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), TOOLBAR_HEIGHT));
            if buttons {
                drag_area(ui, top, "colonne");
            }
            // Hors de Ferrite, le bouton fermer est à droite, dans la barre d'outils.
            if buttons && inside_ferrite() {
                close_button(
                    ui,
                    egui::pos2(
                        full.left() + ferrite_ui::theme::WINDOW_BUTTON_INSET as f32,
                        top.center().y,
                    ),
                );
            }
            let body = egui::Rect::from_min_max(
                egui::pos2(
                    full.left() + 10.0,
                    if buttons { top.bottom() } else { full.top() + 10.0 },
                ),
                egui::pos2(full.right() - 10.0, full.bottom() - 10.0),
            );
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
            add(&mut child);
        });
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
        // Basculer uniquement l'état choisi : aucun mouvement de fenêtre ne doit démarrer
        // entre les deux clics.
        match Settings::load().double_click() {
            DoubleClick::Maximize => {
                let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            DoubleClick::Minimize => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            DoubleClick::Nothing => {}
        }
    } else if response.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

/// Écart entre les centres des boutons de fenêtre côte à côte (hors de Ferrite).
const WINDOW_BUTTON_GAP: f32 = 30.0;

/// Vrai dans la session Ferrite : ferrite-shell y donne bords, ombre et redimensionnement,
/// et le Dock remplace le bouton réduire. Ailleurs (GNOME, KDE, Xfce…), la fenêtre garde sa
/// propre barre, sans barre de titre imposée, avec réduire, agrandir et fermer à droite.
pub fn inside_ferrite() -> bool {
    static INSIDE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *INSIDE.get_or_init(|| {
        std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| {
            desktop
                .split(':')
                .any(|name| name.eq_ignore_ascii_case("ferrite"))
        })
    })
}

/// Rôle d'un bouton de fenêtre.
#[derive(Clone, Copy, PartialEq)]
enum WindowButton {
    Close,
    Zoom,
    Minimize,
}

/// Bouton de fenêtre : pastille de 18 px et symbole, rouge (fermer) ou couleur d'accent
/// (agrandir, réduire) au survol, estompé si la fenêtre n'a pas le focus.
fn window_button(ui: &mut egui::Ui, center: egui::Pos2, kind: WindowButton) -> egui::Response {
    let close = kind == WindowButton::Close;
    use ferrite_ui::theme::WINDOW_BUTTON_RADIUS;
    let radius = WINDOW_BUTTON_RADIUS as f32;
    let rect = egui::Rect::from_center_size(center, egui::vec2(2.0 * radius + 2.0, 2.0 * radius + 2.0));
    let salt = match kind {
        WindowButton::Close => "fermer",
        WindowButton::Zoom => "agrandir",
        WindowButton::Minimize => "reduire",
    };
    let response = ui.interact(rect, ui.scope_id().with(salt), egui::Sense::click());
    let focused = ui.input(|input| input.viewport().focused.unwrap_or(true));
    let active_palette = palette();
    let (fill, ink) = if response.hovered() {
        let fill = if close {
            color(active_palette.danger)
        } else {
            accent()
        };
        (fill, Color32::WHITE)
    } else if focused {
        (
            color(active_palette.window_button),
            color(active_palette.window_glyph),
        )
    } else {
        (
            color(active_palette.window_button_inactive),
            color(active_palette.window_glyph),
        )
    };
    let painter = ui.painter();
    painter.circle_filled(center, radius, fill);
    let at = |x: f32, y: f32| center + egui::vec2(x, y);
    if kind == WindowButton::Minimize {
        painter.line_segment([at(-3.5, 0.0), at(3.5, 0.0)], Stroke::new(1.6, ink));
    } else if close {
        let stroke = Stroke::new(1.6, ink);
        painter.line_segment([at(-3.0, -3.0), at(3.0, 3.0)], stroke);
        painter.line_segment([at(3.0, -3.0), at(-3.0, 3.0)], stroke);
    } else {
        let stroke = Stroke::new(1.5, ink);
        for segment in [
            [at(-3.0, 1.0), at(-3.0, 3.0)],
            [at(-3.0, 3.0), at(-1.0, 3.0)],
            [at(1.0, -3.0), at(3.0, -3.0)],
            [at(3.0, -3.0), at(3.0, -1.0)],
            [at(-3.0, 3.0), at(-0.5, 0.5)],
            [at(3.0, -3.0), at(0.5, -0.5)],
        ] {
            painter.line_segment(segment, stroke);
        }
    }
    response
}

/// Bouton « fermer », à gauche de la barre de la fenêtre.
pub fn close_button(ui: &mut egui::Ui, center: egui::Pos2) {
    if window_button(ui, center, WindowButton::Close).clicked() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// Bouton « agrandir », à droite de la barre de la fenêtre.
pub fn zoom_button(ui: &mut egui::Ui, center: egui::Pos2) {
    if window_button(ui, center, WindowButton::Zoom).clicked() {
        let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
}

/// Bouton « réduire » (hors de Ferrite seulement : dans Ferrite, le Dock s'en charge).
pub fn minimize_button(ui: &mut egui::Ui, center: egui::Pos2) {
    if window_button(ui, center, WindowButton::Minimize).clicked() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
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
    switch_sized(ui, on, enabled, egui::vec2(44.0, 26.0))
}

/// Interrupteur de la taille voulue : bouton blanc ombré, piste couleur d'accent s'il est mis.
pub fn switch_sized(ui: &mut egui::Ui, on: &mut bool, enabled: bool, size: egui::Vec2) -> egui::Response {
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
    let off = themed(rgb_color(0xD5D9E0), rgb_color(0x4B5360));
    let track = if !enabled {
        control_background()
    } else {
        off.lerp_to_gamma(accent(), progress)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, rect.height() / 2.0, track);
    let radius = rect.height() / 2.0 - 2.0;
    let x = egui::lerp(rect.left() + radius + 2.0..=rect.right() - radius - 2.0, progress);
    painter.circle_filled(
        egui::pos2(x, rect.center().y + 1.0),
        radius,
        Color32::from_black_alpha(46),
    );
    painter.circle_filled(egui::pos2(x, rect.center().y), radius, Color32::WHITE);
    response
}

fn rgb_color(hex: u32) -> Color32 {
    from_rgb(hex)
}

macro_rules! icons {
    ($($name:literal),* $(,)?; $($theme_name:literal => $theme_path:literal),* $(,)?) => {
        const SOURCES: &[(&str, &str)] = &[
            $(($name, include_str!(concat!("../../../icons/SVG/", $name, ".svg")))),*,
            $((
                $theme_name,
                include_str!(concat!("../../../ferrite-theme/icons/Ferrite/scalable/", $theme_path, ".svg")),
            )),*
        ];
    };
}

icons![
    "app-code",
    "app-text-editor",
    "ferrite-logo",
    "ferrite-logo-light",
    "places-usb",
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
    "file-document",
    "file-spreadsheet",
    "file-presentation",
    ;
    "app-files" => "apps/system-file-manager",
    "app-video" => "apps/multimedia-video-player",
    "app-music" => "apps/multimedia-audio-player",
    "app-photos" => "apps/image-viewer",
    "app-pdf" => "mimetypes/application-pdf",
    "app-terminal" => "apps/utilities-terminal",
    "app-store" => "apps/system-software-install",
    "app-calendar" => "apps/x-office-calendar",
    "app-clock" => "apps/preferences-system-time",
    "app-settings" => "apps/preferences-system",
    "file-generic" => "mimetypes/text-x-generic",
    "file-text" => "mimetypes/text-x-generic",
    "file-image" => "mimetypes/image-x-generic",
    "file-audio" => "mimetypes/audio-x-generic",
    "file-video" => "mimetypes/video-x-generic",
    "file-pdf" => "mimetypes/application-pdf",
    "file-archive" => "mimetypes/package-x-generic",
    "file-script" => "mimetypes/text-x-script",
    "file-executable" => "mimetypes/application-x-executable",
    "places-folder" => "places/folder",
    "places-computer" => "devices/drive-harddisk",
    "places-desktop" => "places/user-desktop",
    "places-documents" => "places/folder-documents",
    "places-downloads" => "places/folder-download",
    "places-home" => "places/user-home",
    "places-music" => "places/folder-music",
    "places-pictures" => "places/folder-pictures",
    "places-public" => "places/folder-publicshare",
    "places-templates" => "places/folder-templates",
    "places-trash" => "places/user-trash",
    "places-trash-full" => "places/user-trash-full",
    "places-videos" => "places/folder-videos",
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

    /// Texture de l'icône `name` à `size` points (pour la poser soi-même).
    pub fn texture_for(
        &mut self,
        ctx: &egui::Context,
        name: &'static str,
        size: f32,
    ) -> Option<TextureHandle> {
        self.texture(ctx, name, size, IconStyle::Color)
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

/// Ligne de colonne des Paramètres, comme dans les Réglages de macOS : pastille colorée
/// avec un symbole blanc, puis le titre ; choisie, la ligne prend le fond de sélection.
pub fn sidebar_tile_item(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    icon: &'static str,
    tile: Color32,
    text: &str,
    selected: bool,
) -> egui::Response {
    tile_item(ui, icons, icon, tile, text, selected, false)
}

/// Comme [`sidebar_tile_item`], l'élément choisi en pleine couleur d'accent, texte
/// contrasté (Paramètres).
pub fn sidebar_tile_item_accent(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    icon: &'static str,
    tile: Color32,
    text: &str,
    selected: bool,
) -> egui::Response {
    tile_item(ui, icons, icon, tile, text, selected, true)
}

fn tile_item(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    icon: &'static str,
    tile: Color32,
    text: &str,
    selected: bool,
    accent_fill: bool,
) -> egui::Response {
    let scale = sidebar_scale();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 34.0 * scale),
        egui::Sense::click(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected {
        let fill = if accent_fill { accent() } else { sidebar_selected() };
        ui.painter().rect_filled(rect, 8.0, fill);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 8.0, tint(9));
    }
    let side = 24.0 * scale;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + side / 2.0, rect.center().y),
        egui::Vec2::splat(side),
    );
    ui.painter().rect_filled(icon_rect, 7.0 * scale, tile);
    icons.paint(
        ui,
        icon,
        icon_rect.shrink(side * 0.2),
        IconStyle::Tinted(Color32::WHITE),
    );
    ui.painter().text(
        egui::pos2(icon_rect.right() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        if selected && accent_fill {
            FontId::new(14.0, strong_family())
        } else {
            FontId::proportional(14.0)
        },
        match (selected, accent_fill) {
            (true, true) => accent_text(),
            (true, false) => sidebar_selected_text(),
            _ => primary_text(),
        },
    );
    response
}

/// Titre de groupe d'une barre latérale (« Favoris », « Appareils »…).
pub fn sidebar_heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .extra_letter_spacing(0.66)
            .color(text_secondary()),
    );
    ui.add_space(2.0);
}

/// Symboles au trait des pages de Paramètres, repris tels quels des planches.
const GLYPHS: &[(&str, &str)] = &[
    // Calendrier : colonne latérale.
    (
        "glyph-calendar",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 4 H18 a2 2 0 0 1 2 2 V19 a2 2 0 0 1 -2 2 H6 a2 2 0 0 1 -2 -2 V6 a2 2 0 0 1 2 -2 Z M4 9.5 H20 M8 2.5 V5.5 M16 2.5 V5.5" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-today",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 8 a4 4 0 1 0 0.01 0 M12 2 V4 M12 20 V22 M2 12 H4 M20 12 H22 M4.9 4.9 L6.3 6.3 M17.7 17.7 L19.1 19.1 M4.9 19.1 L6.3 17.7 M17.7 6.3 L19.1 4.9" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-upcoming",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 12 H16 M11 6 L17 12 L11 18 M21 5 V19" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-overdue",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3.5 L21.5 20 H2.5 Z M12 10 V14 M12 17 V17.1" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-tasks",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3.5 6 L5 7.5 L7.5 5 M3.5 12 L5 13.5 L7.5 11 M3.5 18 L5 19.5 L7.5 17 M11 6.5 H20.5 M11 12.5 H20.5 M11 18.5 H20.5" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-wifi",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M2 9 a15 15 0 0 1 20 0 M5.5 12.5 a10 10 0 0 1 13 0 M9 16 a5 5 0 0 1 6 0 M12 19.5 V19.6" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-bluetooth",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M7 7 L17 17 L12 21 V3 L17 7 L7 17" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-network",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3 a9 9 0 1 0 0.01 0 M3 12 H21 M12 3 a14 14 0 0 1 0 18 M12 3 a14 14 0 0 0 0 18" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-appearance",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3 a9 9 0 1 0 0.01 0 M12 3 V21" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-dock",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 5 H20 V15 H4 Z M8 19 H16" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-windows",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 5 H20 V19 H4 Z M4 9 H20" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-bell",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 16 V11 a6 6 0 0 1 12 0 V16 L19.5 18 H4.5 Z M10 21 H14" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-sound",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 9 H8 L13 5 V19 L8 15 H4 Z M16.5 9 a4 4 0 0 1 0 6" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-sound-full",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 9 H8 L13 5 V19 L8 15 H4 Z M16.5 9 a4 4 0 0 1 0 6 M19 6.5 a8 8 0 0 1 0 11" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-energy",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 8 H18 V16 H3 Z M20 11 V13" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-screen",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 5 H21 V16 H3 Z M9 20 H15 M12 16 V20" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-keyboard",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 7 H21 V17 H3 Z M7 11 H7.01 M11 11 H11.01 M15 11 H15.01 M8 14 H16" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-mouse",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3 a6 6 0 0 1 6 6 V15 a6 6 0 0 1 -12 0 V9 a6 6 0 0 1 6 -6 Z M12 3 V9" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-printer",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M7 9 V3 H17 V9 M5 9 H19 V16 H5 Z M8 14 H16 V21 H8 Z" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-account",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 4 a4 4 0 1 0 0.01 0 M4 21 a8 8 0 0 1 16 0" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-lock",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 11 H18 V20 H6 Z M8.5 11 V8 a3.5 3.5 0 0 1 7 0 V11" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-access",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3.5 V3.6 M5 8 H19 M12 8 V14 L8 21 M12 14 L16 21" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-update",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M20 12 a8 8 0 1 1 -2.3 -5.7 M20 4 V9 H15" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-about",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3 a9 9 0 1 0 0.01 0 M12 11 V17 M12 7.5 V7.6" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-clock",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3 a9 9 0 1 0 0.01 0 M12 7 V12 L15.5 14" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-head",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 15 V12 a8 8 0 0 1 16 0 V15 M4 14 H7 V20 H4 Z M17 14 H20 V20 H17 Z" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-phone",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M8 3 H16 V21 H8 Z M11 18 H13" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-speaker",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M7 3 H17 V21 H7 Z M12 15 a3 3 0 1 0 0.01 0 M12 7 V7.1" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-watch",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M8 7 H16 V17 H8 Z M9.5 7 L10 3 H14 L14.5 7 M9.5 17 L10 21 H14 L14.5 17" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-laptop",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M5 6 H19 V16 H5 Z M3 19 H21" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-mic",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3 a3 3 0 0 1 3 3 V12 a3 3 0 0 1 -6 0 V6 a3 3 0 0 1 3 -3 Z M6 11 a6 6 0 0 0 12 0 M12 17 V21" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-app",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M5 5 H19 V19 H5 Z" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
];

fn rasterize(name: &str, side: u32, ink: Option<Color32>) -> Option<egui::ColorImage> {
    use resvg::{tiny_skia, usvg};
    let (_, source) = SOURCES
        .iter()
        .chain(GLYPHS)
        .find(|(candidate, _)| *candidate == name)?;
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

    /// Le thème est global : les tests qui le changent passent l'un après l'autre, sinon
    /// l'un lit le thème que l'autre vient de poser.
    static THEME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn theme_lock() -> std::sync::MutexGuard<'static, ()> {
        THEME.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn adaptateur_egui_applique_les_tokens_partages() {
        let _theme = theme_lock();
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
        let _theme = theme_lock();
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
    fn les_icones_du_theme_n_ont_pas_de_texte() {
        // resvg est compilé sans le texte : une balise <text> disparaîtrait au rendu.
        for (name, source) in SOURCES {
            assert!(!source.contains("<text"), "{name} contient du texte");
        }
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

    #[test]
    fn caractere_suivi_de_fe0f_en_emoji() {
        let mut job = egui::text::LayoutJob::default();
        let format = egui::TextFormat::simple(FontId::proportional(14.0), Color32::WHITE);
        job.append("Attention ⚠\u{FE0F} ici ⚠ !", 2.0, format);
        emoji_presentation(&mut job);
        let pieces: Vec<(&str, bool, f32)> = job
            .sections
            .iter()
            .map(|section| {
                (
                    &job.text[section.byte_range.start.0..section.byte_range.end.0],
                    section.format.font_id.family == emoji_family(),
                    section.leading_space,
                )
            })
            .collect();
        assert_eq!(
            pieces,
            [
                ("Attention ", false, 2.0),
                ("⚠\u{FE0F}", true, 0.0),
                (" ici ⚠ !", false, 0.0),
            ]
        );
        // La famille existe toujours, police d'emoji présente ou non.
        assert!(font_definitions().families.contains_key(&emoji_family()));
    }
}
