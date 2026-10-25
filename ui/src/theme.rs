//! Charte visuelle de Ferrite (voir `docs/PLAN.md`, §3.4).
//!
//! Toutes les mesures sont en **pixels logiques**. L'écran du ThinkBook (2880×1800) est
//! utilisé à l'échelle 2, soit 1440×900 pixels logiques. Les valeurs reprennent celles du
//! bureau Ferrite d'origine (`D:\Os\kernel\src\gui.rs`) pour garder le même rendu.

use std::time::Duration;

/// Hauteur de la barre de menus.
pub const MENUBAR_HEIGHT: u32 = 25;
/// Hauteur de la barre de titre des fenêtres.
pub const TITLEBAR_HEIGHT: u32 = 30;
/// Rayon des coins des fenêtres.
pub const WINDOW_RADIUS: u32 = 11;
/// Côté des icônes du Dock.
pub const DOCK_ICON: u32 = 54;
/// Espace entre deux icônes du Dock.
pub const DOCK_GAP: u32 = 10;
/// Marge intérieure du Dock.
pub const DOCK_PADDING: u32 = 8;
/// Largeur d'un menu déroulant.
pub const MENU_WIDTH: u32 = 220;
/// Hauteur d'une ligne de menu.
pub const MENU_ITEM_HEIGHT: u32 = 24;

/// Feux tricolores : rayon, centre du premier feu depuis le bord gauche, et pas entre feux.
pub const TRAFFIC_LIGHT_RADIUS: u32 = 6;
pub const TRAFFIC_LIGHT_FIRST_X: u32 = 20;
pub const TRAFFIC_LIGHT_STEP: u32 = 20;

/// Polices de l'interface (fichiers dans `assets/fonts`).
pub const FONT_UI: &str = "Inter";
pub const FONT_DISPLAY: &str = "InterDisplay";
pub const FONT_MONO: &str = "JetBrains Mono";
pub const FONT_SERIF: &str = "Source Serif 4";

/// Durées d'animation : courte (survol, appui) et normale (ouverture, réduction).
pub const ANIMATION_SHORT: Duration = Duration::from_millis(200);
pub const ANIMATION_NORMAL: Duration = Duration::from_millis(280);

/// Fond d'écran par défaut : dégradé vertical du bureau Ferrite d'origine (haut, milieu, bas).
pub const WALLPAPER_GRADIENT: [Color; 3] = [Color::rgb(0x1A2A6C), Color::rgb(0x6A3FA6), Color::rgb(0xF29A7E)];

/// Couleur d'un dégradé vertical à la hauteur relative `t` (0 en haut, 1 en bas), les
/// couleurs étant réparties à intervalles égaux.
pub fn gradient_at(stops: &[Color], t: f32) -> Color {
    match stops {
        [] => Color::TRANSPARENT,
        [only] => *only,
        _ => {
            let position = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
            let index = (position.floor() as usize).min(stops.len() - 2);
            stops[index].mix(stops[index + 1], position - index as f32)
        }
    }
}

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
    pub titlebar: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub separator: Color,
    /// Teinte posée sur le flou de la barre de menus.
    pub menubar_tint: Color,
    /// Teinte posée sur le flou du Dock.
    pub dock_tint: Color,
    pub shadow: Color,
    /// Feux tricolores (remplissage, contour) : fermer, réduire, plein écran.
    pub traffic_lights: [(Color, Color); 3],
    /// Feux d'une fenêtre inactive (remplissage, contour).
    pub traffic_light_inactive: (Color, Color),
}

const TRAFFIC_LIGHTS: [(Color, Color); 3] = [
    (Color::rgb(0xFF5F57), Color::rgb(0xE0443E)),
    (Color::rgb(0xFEBC2E), Color::rgb(0xDEA123)),
    (Color::rgb(0x28C840), Color::rgb(0x1AAB29)),
];

impl Palette {
    pub const LIGHT: Palette = Palette {
        accent: Color::rgb(0x0A7AFF),
        window_background: Color::rgb(0xFFFFFF),
        titlebar: Color::rgb(0xF6F6F6),
        text: Color::rgb(0x1C1C1E),
        text_secondary: Color::rgb(0x6E6E73),
        separator: Color::rgb(0xD8D8DC),
        menubar_tint: Color::rgb(0xF5F5F7).with_alpha(170),
        dock_tint: Color::rgb(0xF5F5F7).with_alpha(110),
        shadow: Color::BLACK.with_alpha(90),
        traffic_lights: TRAFFIC_LIGHTS,
        traffic_light_inactive: (Color::rgb(0xDCDCDE), Color::rgb(0xC8C8CC)),
    };

    pub const DARK: Palette = Palette {
        accent: Color::rgb(0x0A84FF),
        window_background: Color::rgb(0x1E1E1E),
        titlebar: Color::rgb(0x2C2C2E),
        text: Color::rgb(0xF2F2F7),
        text_secondary: Color::rgb(0x98989D),
        separator: Color::rgb(0x3A3A3C),
        menubar_tint: Color::rgb(0x1C1C1E).with_alpha(170),
        dock_tint: Color::rgb(0x2C2C2E).with_alpha(120),
        shadow: Color::BLACK.with_alpha(140),
        traffic_lights: TRAFFIC_LIGHTS,
        traffic_light_inactive: (Color::rgb(0x4A4A4D), Color::rgb(0x3A3A3C)),
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
        assert_eq!(scale.to_physical(MENUBAR_HEIGHT), 50);
        assert_eq!(scale.ceil(), 2);
    }

    #[test]
    fn echelle_fractionnaire() {
        let scale = Scale::from_factor(1.75).unwrap();
        assert_eq!(scale.as_120ths(), 210);
        assert_eq!(scale.ceil(), 2);
        assert_eq!(scale.to_physical(TITLEBAR_HEIGHT), 53);
        assert_eq!(scale.to_logical(2880), 1646);
        assert_eq!(Scale::from_factor(0.0), None);
        assert_eq!(Scale::from_factor(f64::NAN), None);
    }

    #[test]
    fn couleurs() {
        let accent = Palette::LIGHT.accent;
        assert_eq!((accent.r, accent.g, accent.b, accent.a), (0x0A, 0x7A, 0xFF, 255));
        assert_eq!(Color::WHITE.to_argb8888_premultiplied(), 0xFFFF_FFFF);
        assert_eq!(
            Color::WHITE.with_alpha(128).to_argb8888_premultiplied(),
            0x8080_8080
        );
        assert_eq!(Color::TRANSPARENT.to_argb8888_premultiplied(), 0);
        assert_eq!(Color::BLACK.mix(Color::WHITE, 0.5), Color::rgb(0x808080));
        assert_eq!(Palette::for_mode(ThemeMode::Dark), Palette::DARK);
    }

    #[test]
    fn degrade_du_fond() {
        let [top, middle, bottom] = WALLPAPER_GRADIENT;
        assert_eq!(gradient_at(&WALLPAPER_GRADIENT, 0.0), top);
        assert_eq!(gradient_at(&WALLPAPER_GRADIENT, 0.5), middle);
        assert_eq!(gradient_at(&WALLPAPER_GRADIENT, 1.0), bottom);
        assert_eq!(gradient_at(&WALLPAPER_GRADIENT, 0.25), top.mix(middle, 0.5));
        assert_eq!(gradient_at(&[], 0.5), Color::TRANSPARENT);
        assert_eq!(gradient_at(&[top], 0.9), top);
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
