//! Couleurs de Fichiers, reprises de la planche « Fichiers » (clair) et « FichiersSombre ».
//! L'accentuation suit le réglage de Paramètres ; les teintes de sélection en dérivent.

use eframe::egui::Color32;

use crate::style;

#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub fg: Color32,
    pub fg2: Color32,
    pub fg3: Color32,
    pub icon: Color32,
    /// Fond du contenu, de la barre latérale, filets et pastilles.
    pub win: Color32,
    pub side: Color32,
    pub line: Color32,
    pub chip: Color32,
    pub track: Color32,
    /// Panneau d'aperçu et sa scène.
    pub preview: Color32,
    pub stage: Color32,
    /// Sélection : fond, texte, anneau et pastille du nom.
    pub sel_bg: Color32,
    pub sel_fg: Color32,
    pub accent: Color32,
    pub head: Color32,
    /// Segment choisi du contrôle Grille / Liste.
    pub seg_on: Color32,
    /// Cible survolée pendant un glisser-déposer.
    pub drop: Color32,
}

fn hex(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

fn mix(from: Color32, to: Color32, amount: f32) -> Color32 {
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * amount).round() as u8;
    Color32::from_rgb(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}

pub fn look() -> Look {
    let accent = style::accent();
    if style::theme_is_dark() {
        let win = hex(0x22262D);
        // L'accent s'éclaircit sur fond sombre (#2779E0 donne #6CB8FF sur la planche).
        let light = mix(accent, Color32::WHITE, 0.33);
        Look {
            fg: hex(0xE5E7EB),
            fg2: hex(0x9CA3AF),
            fg3: hex(0x8B93A0),
            icon: hex(0xE5E7EB),
            win,
            side: hex(0x1E2127),
            line: hex(0x353A42),
            chip: hex(0x2A2E35),
            track: hex(0x353A42),
            preview: hex(0x1E2127),
            stage: hex(0x2A2E35),
            sel_bg: mix(win, light, 0.16),
            sel_fg: mix(light, Color32::WHITE, 0.4),
            accent: light,
            head: hex(0x9CA3AF),
            seg_on: hex(0x3A3F47),
            drop: mix(win, light, 0.26),
        }
    } else {
        Look {
            fg: hex(0x111827),
            fg2: hex(0x4B5563),
            fg3: hex(0x6B7280),
            icon: hex(0x1F2937),
            win: Color32::WHITE,
            side: hex(0xF2F3F6),
            line: hex(0xE3E6EB),
            chip: hex(0xEEF0F4),
            track: hex(0xE3E6EB),
            preview: hex(0xF7F8FA),
            stage: hex(0xEEF0F4),
            // #2779E0 donne #DCEAFB et #174F9E sur la planche.
            sel_bg: mix(Color32::WHITE, accent, 0.17),
            sel_fg: mix(accent, Color32::BLACK, 0.4),
            accent,
            head: hex(0x4B5563),
            seg_on: Color32::WHITE,
            drop: mix(Color32::WHITE, accent, 0.28),
        }
    }
}
