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
    pub const ACCENT_LIGHT: u32 = 0x0A7AFF;
    pub const ACCENT_DARK: u32 = 0x0A84FF;
    pub const DANGER_LIGHT: u32 = 0xC6262E;
    pub const DANGER_DARK: u32 = 0xFF6961;
    pub const SUCCESS_LIGHT: u32 = 0x248A3D;
    pub const SUCCESS_DARK: u32 = 0x30D158;
    pub const WARNING_LIGHT: u32 = 0x9A6700;
    pub const WARNING_DARK: u32 = 0xFFD60A;
}

pub mod radius {
    pub const CONTROL: u8 = 7;
    pub const WINDOW: u32 = 11;
    pub const PANEL: u8 = 14;
    pub const POPOVER: u32 = 26;
}

pub mod border {
    pub const HAIRLINE: f32 = 1.0;
    pub const FOCUS: f32 = 2.0;
}

pub mod elevation {
    pub const WINDOW_MARGIN: i32 = 48;
    pub const WINDOW_OFFSET_Y: f32 = 10.0;
    pub const WINDOW_BLUR: f32 = 26.0;
    pub const WINDOW_ACTIVE_ALPHA: f32 = 0.42;
    pub const WINDOW_INACTIVE_ALPHA: f32 = 0.22;
}

pub mod typography {
    pub const SMALL: f32 = 11.5;
    pub const BODY: f32 = 13.5;
    pub const HEADING: f32 = 17.0;
    pub const MONOSPACE: f32 = 13.0;
    pub const WINDOW_TITLE: f32 = 13.0;
}

pub mod control {
    pub const HEIGHT: f32 = 28.0;
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
        assert_eq!(radius::WINDOW, 11);
        assert_eq!(border::HAIRLINE, 1.0);
        assert_eq!(control::HEIGHT, 28.0);
        assert_eq!(elevation::WINDOW_BLUR, 26.0);
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
