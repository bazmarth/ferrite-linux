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
    // Rôles communs de `style` ; seule la sélection dérive de l'accentuation.
    let (win, selection) = if style::theme_is_dark() {
        // L'accent s'éclaircit sur fond sombre (#2779E0 donne #6CB8FF sur la planche).
        let light = mix(accent, Color32::WHITE, 0.33);
        (
            style::background(),
            (light, mix(light, Color32::WHITE, 0.4), 0.16, 0.26),
        )
    } else {
        // #2779E0 donne #DCEAFB et #174F9E sur la planche.
        (
            style::background(),
            (accent, mix(accent, Color32::BLACK, 0.4), 0.17, 0.28),
        )
    };
    let (accent, sel_fg, sel_amount, drop_amount) = selection;
    Look {
        fg: style::primary_text(),
        fg2: style::text_secondary(),
        fg3: style::tertiary_text(),
        icon: style::primary_text(),
        win,
        side: style::sidebar_color(),
        line: style::line(),
        chip: style::control_hover(),
        track: style::line(),
        preview: style::header_color(),
        stage: style::control_hover(),
        sel_bg: mix(win, accent, sel_amount),
        sel_fg,
        accent,
        head: style::text_secondary(),
        seg_on: style::segment_on(),
        drop: mix(win, accent, drop_amount),
    }
}
