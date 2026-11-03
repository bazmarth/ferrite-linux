//! Couleurs du terminal : celles de la charte, sauf ce que le programme a redéfini.

use alacritty_terminal::{
    term::color::Colors,
    vte::ansi::{Color, NamedColor, Rgb},
};
use eframe::egui::Color32;

use crate::style;

/// Les 16 couleurs ANSI, lisibles sur le fond de chaque thème.
const ANSI_LIGHT: [u32; 16] = [
    0x3A3A3C, 0xC4332B, 0x237A33, 0x8A5F00, 0x0A63D0, 0x8B3FD9, 0x0F7B8A, 0xC7C7CC, 0x6E6E73, 0xE5534B,
    0x2E9A45, 0xA9780C, 0x2F81EF, 0xA860E8, 0x1C9DB0, 0xE5E5EA,
];
const ANSI_DARK: [u32; 16] = [
    0x4A4F59, 0xE5534B, 0x6BC46D, 0xE0B354, 0x5A9CF0, 0xB47FE6, 0x4CB8C4, 0xD7DAE0, 0x7A8290, 0xF07178,
    0x8BD98D, 0xF2CD6E, 0x7DB8F5, 0xC99BF0, 0x6FD0DA, 0xFFFFFF,
];

fn from_hex(hex: u32) -> Rgb {
    let [_, r, g, b] = hex.to_be_bytes();
    Rgb { r, g, b }
}

fn from_color32(color: Color32) -> Rgb {
    Rgb {
        r: color.r(),
        g: color.g(),
        b: color.b(),
    }
}

/// Couleur 16 à 255 de la table de xterm : cube 6 × 6 × 6, puis 24 gris.
fn extended(index: usize) -> Rgb {
    if index >= 232 {
        let gray = (8 + (index - 232) * 10) as u8;
        return Rgb {
            r: gray,
            g: gray,
            b: gray,
        };
    }
    let level = |value: usize| if value == 0 { 0 } else { (55 + value * 40) as u8 };
    let cube = index - 16;
    Rgb {
        r: level(cube / 36),
        g: level(cube / 6 % 6),
        b: level(cube % 6),
    }
}

pub fn foreground() -> Color32 {
    style::primary_text()
}

pub fn background() -> Color32 {
    Color32::from_rgb(0x1B, 0x1E, 0x23)
}

/// Couleur d'un index de la table du terminal : 0 à 255, puis les couleurs nommées.
pub fn indexed(index: usize, colors: &Colors) -> Rgb {
    if let Some(defined) = colors[index] {
        return defined;
    }
    let ansi = style::themed(ANSI_LIGHT, ANSI_DARK);
    let dim = |rgb: Rgb| Rgb {
        r: (u16::from(rgb.r) * 2 / 3) as u8,
        g: (u16::from(rgb.g) * 2 / 3) as u8,
        b: (u16::from(rgb.b) * 2 / 3) as u8,
    };
    const FOREGROUND: usize = NamedColor::Foreground as usize;
    const BACKGROUND: usize = NamedColor::Background as usize;
    const CURSOR: usize = NamedColor::Cursor as usize;
    const DIM_BLACK: usize = NamedColor::DimBlack as usize;
    const DIM_WHITE: usize = NamedColor::DimWhite as usize;
    const BRIGHT_FOREGROUND: usize = NamedColor::BrightForeground as usize;
    const DIM_FOREGROUND: usize = NamedColor::DimForeground as usize;
    match index {
        0..=15 => from_hex(ansi[index]),
        16..=255 => extended(index),
        BACKGROUND => from_color32(background()),
        DIM_BLACK..=DIM_WHITE => dim(from_hex(ansi[index - DIM_BLACK])),
        DIM_FOREGROUND => dim(from_color32(foreground())),
        FOREGROUND | CURSOR | BRIGHT_FOREGROUND => from_color32(foreground()),
        _ => from_color32(foreground()),
    }
}

pub fn resolve(color: Color, colors: &Colors) -> Color32 {
    let rgb = match color {
        Color::Spec(rgb) => rgb,
        Color::Named(named) => indexed(named as usize, colors),
        Color::Indexed(index) => indexed(usize::from(index), colors),
    };
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_de_xterm() {
        assert_eq!(extended(16), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(extended(196), Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(
            extended(231),
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(extended(232), Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(
            extended(255),
            Rgb {
                r: 238,
                g: 238,
                b: 238
            }
        );
    }

    #[test]
    fn une_couleur_redefinie_par_le_programme_passe_avant() {
        let mut colors = Colors::default();
        let pink = Rgb { r: 255, g: 0, b: 128 };
        colors[1] = Some(pink);
        assert_eq!(indexed(1, &colors), pink);
        assert_eq!(
            resolve(Color::Named(NamedColor::Red), &colors),
            Color32::from_rgb(255, 0, 128)
        );
        assert_eq!(
            resolve(Color::Spec(Rgb { r: 1, g: 2, b: 3 }), &colors),
            Color32::from_rgb(1, 2, 3)
        );
    }
}
