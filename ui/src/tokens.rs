//! Valeurs visuelles indépendantes des moteurs de rendu.
//!
//! Les adaptateurs egui et ferrite-shell traduisent ces mesures dans leurs propres API.

pub mod spacing {
    pub const X_SMALL: f32 = 4.0;
    pub const SMALL: f32 = 6.0;
    pub const MEDIUM: f32 = 8.0;
    pub const LARGE: f32 = 12.0;
    pub const X_LARGE: f32 = 16.0;
    pub const CONTROL_HORIZONTAL: f32 = 12.0;
    pub const CONTROL_VERTICAL: f32 = 5.0;
}

pub mod colors {
    pub const ACCENT_LIGHT: u32 = 0x2779E0;
    pub const ACCENT_DARK: u32 = 0x6CB8FF;
    pub const DANGER_LIGHT: u32 = 0xE5372B;
    pub const DANGER_DARK: u32 = 0xE5372B;
    pub const SUCCESS_LIGHT: u32 = 0x1F9D63;
    pub const SUCCESS_DARK: u32 = 0x1F9D63;
    pub const WARNING_LIGHT: u32 = 0xFF9F0A;
    pub const WARNING_DARK: u32 = 0xFF9F0A;
}

pub mod radius {
    pub const CONTROL: u8 = 8;
    pub const WINDOW: u32 = 14;
    pub const PANEL: u8 = 10;
    pub const POPOVER: u32 = 10;
}

pub mod border {
    pub const HAIRLINE: f32 = 1.0;
    pub const FOCUS: f32 = 2.0;
}

pub mod elevation {
    /// Débord de l'ombre : assez pour le décalage et le flou de l'ombre active.
    pub const WINDOW_MARGIN: i32 = 100;
    /// Ombre active : 0 28 70, noir 38 %.
    pub const ACTIVE_OFFSET_Y: f32 = 28.0;
    pub const ACTIVE_BLUR: f32 = 70.0;
    pub const ACTIVE_ALPHA: f32 = 0.38;
    /// Ombre inactive : 0 6 14, noir 22 %. Aussi celle des fenêtres aimantées.
    pub const INACTIVE_OFFSET_Y: f32 = 6.0;
    pub const INACTIVE_BLUR: f32 = 14.0;
    pub const INACTIVE_ALPHA: f32 = 0.22;
}

pub mod typography {
    pub const SMALL: f32 = 12.0;
    pub const BODY: f32 = 14.0;
    pub const HEADING: f32 = 16.0;
    pub const MONOSPACE: f32 = 13.0;
    pub const WINDOW_TITLE: f32 = 14.0;
}

pub mod control {
    pub const HEIGHT: f32 = 30.0;
}

pub mod motion {
    use std::time::Duration;

    pub const SHORT: Duration = Duration::from_millis(200);
    pub const NORMAL: Duration = Duration::from_millis(280);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn les_mesures_et_durees_sont_positives() {
        assert_eq!(spacing::MEDIUM, 8.0);
        assert_eq!(radius::WINDOW, 14);
        assert_eq!(border::HAIRLINE, 1.0);
        assert_eq!(control::HEIGHT, 30.0);
        assert_eq!(elevation::ACTIVE_BLUR, 70.0);
        assert_eq!(motion::SHORT, Duration::from_millis(200));
        assert_eq!(motion::NORMAL, Duration::from_millis(280));
    }

    #[test]
    fn les_couleurs_semantiques_sont_rgb() {
        for color in [
            colors::ACCENT_LIGHT,
            colors::ACCENT_DARK,
            colors::DANGER_LIGHT,
            colors::DANGER_DARK,
            colors::SUCCESS_LIGHT,
            colors::SUCCESS_DARK,
            colors::WARNING_LIGHT,
            colors::WARNING_DARK,
        ] {
            assert!(color <= 0xFFFFFF);
        }
    }
}
