//! Charte visuelle de Ferrite (voir `docs/PLAN.md`, §3.4).
//!
//! Toutes les mesures sont en **pixels logiques**. L'écran du ThinkBook (2880×1800) est
//! utilisé à l'échelle 2, soit 1440×900 pixels logiques. Les valeurs sont celles de la
//! « Spécification des fenêtres et dispositions » et de ses planches.

use std::time::Duration;

use crate::tokens::{colors, motion};

/// Hauteur de la barre du haut.
pub const MENUBAR_HEIGHT: u32 = 34;
/// Hauteur de la barre de titre des fenêtres.
pub const TITLEBAR_HEIGHT: u32 = 46;
/// Rayon des coins des fenêtres.
pub const WINDOW_RADIUS: u32 = crate::tokens::radius::WINDOW;
/// Côté des icônes du Dock.
pub const DOCK_ICON: u32 = 58;
/// Espace entre deux icônes du Dock.
pub const DOCK_GAP: u32 = 10;
/// Marge intérieure du Dock.
pub const DOCK_PADDING: u32 = 12;
/// Largeur d'un menu déroulant.
pub const MENU_WIDTH: u32 = 220;
/// Hauteur d'une ligne de menu.
pub const MENU_ITEM_HEIGHT: u32 = 32;

/// Boutons de fenêtre : pastille de 22 px, centre à 23 px du bord (fermer à gauche,
/// agrandir à droite).
pub const WINDOW_BUTTON_RADIUS: u32 = 11;
pub const WINDOW_BUTTON_INSET: u32 = 23;

/// Polices de l'interface (fichiers dans `assets/fonts`).
pub const FONT_UI: &str = "Inter";
pub const FONT_DISPLAY: &str = "InterDisplay";
pub const FONT_MONO: &str = "JetBrains Mono";
pub const FONT_SERIF: &str = "Source Serif 4";

/// Durées d'animation : courte (survol, appui) et normale (ouverture, réduction).
pub const ANIMATION_SHORT: Duration = motion::SHORT;
pub const ANIMATION_NORMAL: Duration = motion::NORMAL;

/// Couleur RVBA non prémultipliée, 8 bits par canal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const WHITE: Color = Color::rgb(0xFFFFFF);
    pub const BLACK: Color = Color::rgb(0x000000);
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// Couleur opaque à partir d'une valeur `0xRRGGBB`.
    pub const fn rgb(hex: u32) -> Color {
        Color {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
            a: 255,
        }
    }

    pub const fn with_alpha(self, a: u8) -> Color {
        Color { a, ..self }
    }

    /// Mélange linéaire : `t = 0` donne `self`, `t = 1` donne `other`.
    pub fn mix(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Color {
            r: lerp(self.r, other.r),
            g: lerp(self.g, other.g),
            b: lerp(self.b, other.b),
            a: lerp(self.a, other.a),
        }
    }

    /// Pixel `ARGB8888` prémultiplié, le format `wl_shm` utilisé par les clients Wayland.
    pub fn to_argb8888_premultiplied(self) -> u32 {
        let premul = |c: u8| ((c as u32 * self.a as u32 + 127) / 255) & 0xFF;
        (self.a as u32) << 24 | premul(self.r) << 16 | premul(self.g) << 8 | premul(self.b)
    }
}

/// Thème clair ou sombre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

/// Couleurs de l'interface pour un thème donné.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub accent: Color,
    pub window_background: Color,
    pub content_background: Color,
    pub header_background: Color,
    pub sidebar_background: Color,
    pub toolbar_top: Color,
    pub toolbar_bottom: Color,
    pub titlebar: Color,
    pub titlebar_inactive: Color,
    /// Filet sous la barre de titre, fenêtre active ou inactive.
    pub titlebar_line: Color,
    pub titlebar_line_inactive: Color,
    pub window_title_active: Color,
    pub window_title_inactive: Color,
    pub titlebar_action_active: Color,
    pub titlebar_action_inactive: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub separator: Color,
    pub control_separator: Color,
    pub border: Color,
    pub border_hover: Color,
    pub control_hover: Color,
    pub control_pressed: Color,
    pub danger: Color,
    pub success: Color,
    pub warning: Color,
    /// Pastille des boutons de fenêtre (active, inactive) et leur symbole.
    pub window_button: Color,
    pub window_button_inactive: Color,
    pub window_glyph: Color,
    /// Bordure d'une fenêtre : 1 px noir 8 % en clair, blanc 6 % en sombre.
    pub window_ring: Color,
    /// Sélection dans une barre latérale : fond et texte.
    pub selection: Color,
    pub selection_text: Color,
    pub shadow: Color,
    /// Rail d'un curseur ou d'une jauge, sous la part remplie.
    pub track: Color,
    /// Titre de partie, entre le texte et le texte secondaire.
    pub heading: Color,
    /// Texte de réussite (« Connecté », « Enregistré ») : plus lisible que `success`.
    pub success_text: Color,
    /// Scène d'une image, d'une page ou d'une vidéo, un peu plus sombre que la fenêtre.
    pub stage: Color,
    /// Contraste fort (bouton lecture, bandeau « Annuler ») : son fond, son texte, son action.
    pub inverse: Color,
    pub inverse_text: Color,
    pub inverse_action: Color,
}

impl Palette {
    pub const LIGHT: Palette = Palette {
        accent: Color::rgb(colors::ACCENT_LIGHT),
        window_background: Color::rgb(0xFFFFFF),
        content_background: Color::rgb(0xFFFFFF),
        header_background: Color::rgb(0xF7F8FA),
        sidebar_background: Color::rgb(0xF2F3F6),
        toolbar_top: Color::rgb(0xF7F8FA),
        toolbar_bottom: Color::rgb(0xF7F8FA),
        titlebar: Color::rgb(0xF7F8FA),
        titlebar_inactive: Color::rgb(0xFBFBFC),
        titlebar_line: Color::rgb(0xE3E6EB),
        titlebar_line_inactive: Color::rgb(0xEDEFF2),
        window_title_active: Color::rgb(0x1F2937),
        window_title_inactive: Color::rgb(0x6B7280),
        titlebar_action_active: Color::rgb(0x4B5563),
        titlebar_action_inactive: Color::rgb(0x6B7280),
        text: Color::rgb(0x16181C),
        text_secondary: Color::rgb(0x4B5563),
        separator: Color::rgb(0xE3E6EB),
        control_separator: Color::rgb(0xEDEFF2),
        border: Color::rgb(0xD5D9E0),
        border_hover: Color::rgb(0xAEB6C2),
        control_hover: Color::rgb(0xEEF0F4),
        control_pressed: Color::rgb(0xE7E9EE),
        danger: Color::rgb(colors::DANGER_LIGHT),
        success: Color::rgb(colors::SUCCESS_LIGHT),
        warning: Color::rgb(colors::WARNING_LIGHT),
        window_button: Color::rgb(0xD5D9E0),
        window_button_inactive: Color::rgb(0xE7E9EE),
        window_glyph: Color::rgb(0x4B5563),
        window_ring: Color::BLACK.with_alpha(20),
        selection: Color::rgb(0xDCEAFB),
        selection_text: Color::rgb(0x174F9E),
        shadow: Color::BLACK.with_alpha(97),
        track: Color::rgb(0xD5D9E0),
        heading: Color::rgb(0x374151),
        success_text: Color::rgb(0x1F7A4D),
        stage: Color::rgb(0xEDEFF2),
        inverse: Color::rgb(0x16181C),
        inverse_text: Color::WHITE,
        inverse_action: Color::rgb(0x9CC7FF),
    };

    pub const DARK: Palette = Palette {
        accent: Color::rgb(colors::ACCENT_DARK),
        window_background: Color::rgb(0x22262D),
        content_background: Color::rgb(0x22262D),
        header_background: Color::rgb(0x2A2E35),
        sidebar_background: Color::rgb(0x1E2127),
        toolbar_top: Color::rgb(0x2A2E35),
        toolbar_bottom: Color::rgb(0x2A2E35),
        titlebar: Color::rgb(0x2A2E35),
        titlebar_inactive: Color::rgb(0x25282E),
        titlebar_line: Color::rgb(0x353A42),
        titlebar_line_inactive: Color::rgb(0x2E333A),
        window_title_active: Color::rgb(0xE5E7EB),
        window_title_inactive: Color::rgb(0x9CA3AF),
        titlebar_action_active: Color::rgb(0xC9CED6),
        titlebar_action_inactive: Color::rgb(0x9CA3AF),
        text: Color::rgb(0xE5E7EB),
        text_secondary: Color::rgb(0x9CA3AF),
        separator: Color::rgb(0x353A42),
        control_separator: Color::rgb(0x2E333A),
        border: Color::rgb(0x353A42),
        border_hover: Color::rgb(0x4B5360),
        control_hover: Color::rgb(0x30343B),
        control_pressed: Color::rgb(0x3A3F47),
        danger: Color::rgb(colors::DANGER_DARK),
        success: Color::rgb(colors::SUCCESS_DARK),
        warning: Color::rgb(colors::WARNING_DARK),
        window_button: Color::rgb(0x4B5360),
        window_button_inactive: Color::rgb(0x3A3F47),
        window_glyph: Color::rgb(0xE5E7EB),
        window_ring: Color::WHITE.with_alpha(15),
        selection: Color::rgb(0x23395B),
        selection_text: Color::rgb(0xCFE3FB),
        shadow: Color::BLACK.with_alpha(115),
        track: Color::rgb(0x4B5360),
        heading: Color::rgb(0xD1D5DB),
        success_text: Color::rgb(0x4ADE80),
        stage: Color::rgb(0x121418),
        inverse: Color::rgb(0xE5E7EB),
        inverse_text: Color::rgb(0x16181C),
        inverse_action: Color::rgb(0x174F9E),
    };

    pub const fn for_mode(mode: ThemeMode) -> Palette {
        match mode {
            ThemeMode::Light => Palette::LIGHT,
            ThemeMode::Dark => Palette::DARK,
        }
    }
}

/// Facteur d'échelle, stocké en 120es comme le protocole Wayland `wp_fractional_scale_v1`
/// (120 = 1, 210 = 1,75, 240 = 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scale(u32);

impl Scale {
    pub const ONE: Scale = Scale(120);
    /// Échelle par défaut du ThinkBook (2880×1800 → 1440×900 logiques).
    pub const DEFAULT: Scale = Scale(240);

    /// Échelle à partir de la valeur en 120es. Renvoie `None` pour 0.
    pub const fn from_120ths(value: u32) -> Option<Scale> {
        if value == 0 { None } else { Some(Scale(value)) }
    }

    /// Échelle à partir d'un facteur décimal, arrondie au 120e le plus proche.
    pub fn from_factor(factor: f64) -> Option<Scale> {
        if !factor.is_finite() || factor <= 0.0 {
            return None;
        }
        Scale::from_120ths((factor * 120.0).round() as u32)
    }

    pub const fn as_120ths(self) -> u32 {
        self.0
    }

    pub fn factor(self) -> f64 {
        self.0 as f64 / 120.0
    }

    /// Échelle entière pour les tampons (`wl_surface.set_buffer_scale`) : arrondie au-dessus.
    pub const fn ceil(self) -> u32 {
        self.0.div_ceil(120)
    }

    /// Longueur logique → pixels physiques (arrondi au plus proche).
    pub const fn to_physical(self, logical: u32) -> u32 {
        (logical * self.0 + 60) / 120
    }

    /// Pixels physiques → longueur logique (arrondi au plus proche).
    pub const fn to_logical(self, physical: u32) -> u32 {
        (physical * 120 + self.0 / 2) / self.0
    }
}

impl Default for Scale {
    fn default() -> Self {
        Scale::DEFAULT
    }
}

/// Courbes d'accélération des animations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Easing {
    Linear,
    /// Départ rapide, arrivée douce : apparitions, déplacements.
    EaseOut,
    /// Douce aux deux extrémités : transitions entre deux états.
    EaseInOut,
}

impl Easing {
    /// Progression animée pour un temps `t` entre 0 et 1.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseOut => 1.0 - (1.0 - t).powi(3),
            Easing::EaseInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
        }
    }

    /// Progression après `elapsed` sur une animation de durée `duration`. Si l'utilisateur
    /// a choisi « réduire les animations », l'état final est atteint immédiatement.
    pub fn progress(self, elapsed: Duration, duration: Duration, reduce_motion: bool) -> f32 {
        if reduce_motion || duration.is_zero() {
            return 1.0;
        }
        self.apply(elapsed.as_secs_f32() / duration.as_secs_f32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echelle_du_thinkbook() {
        let scale = Scale::default();
        assert_eq!(scale.to_physical(1440), 2880);
        assert_eq!(scale.to_physical(900), 1800);
        assert_eq!(scale.to_logical(2880), 1440);
        assert_eq!(scale.to_physical(MENUBAR_HEIGHT), 68);
        assert_eq!(scale.ceil(), 2);
    }

    #[test]
    fn echelle_fractionnaire() {
        let scale = Scale::from_factor(1.75).unwrap();
        assert_eq!(scale.as_120ths(), 210);
        assert_eq!(scale.ceil(), 2);
        assert_eq!(scale.to_physical(TITLEBAR_HEIGHT), 81);
        assert_eq!(scale.to_logical(2880), 1646);
        assert_eq!(Scale::from_factor(0.0), None);
        assert_eq!(Scale::from_factor(f64::NAN), None);
    }

    #[test]
    fn couleurs() {
        let accent = Palette::LIGHT.accent;
        assert_eq!((accent.r, accent.g, accent.b, accent.a), (0x27, 0x79, 0xE0, 255));
        assert_eq!(Color::WHITE.to_argb8888_premultiplied(), 0xFFFF_FFFF);
        assert_eq!(
            Color::WHITE.with_alpha(128).to_argb8888_premultiplied(),
            0x8080_8080
        );
        assert_eq!(Color::TRANSPARENT.to_argb8888_premultiplied(), 0);
        assert_eq!(Color::BLACK.mix(Color::WHITE, 0.5), Color::rgb(0x808080));
        assert_eq!(Palette::for_mode(ThemeMode::Dark), Palette::DARK);
        assert_ne!(Palette::LIGHT.window_background, Palette::DARK.window_background);
        assert_ne!(Palette::LIGHT.text, Palette::DARK.text);
        assert_eq!(Palette::LIGHT.danger, Palette::DARK.danger);
        assert_eq!((Palette::LIGHT.border.a, Palette::DARK.border.a), (255, 255));
    }

    #[test]
    fn courbes_d_animation() {
        for easing in [Easing::Linear, Easing::EaseOut, Easing::EaseInOut] {
            assert_eq!(easing.apply(0.0), 0.0);
            assert_eq!(easing.apply(1.0), 1.0);
            assert_eq!(easing.apply(2.0), 1.0);
        }
        assert!(Easing::EaseOut.apply(0.5) > 0.5);
        assert_eq!(Easing::EaseInOut.apply(0.5), 0.5);
        let half = ANIMATION_NORMAL / 2;
        assert_eq!(Easing::EaseOut.progress(half, ANIMATION_NORMAL, true), 1.0);
        assert!((Easing::Linear.progress(half, ANIMATION_NORMAL, false) - 0.5).abs() < 1e-6);
    }
}
