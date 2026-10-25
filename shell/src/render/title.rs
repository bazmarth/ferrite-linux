//! Rendu du titre des fenêtres avec la police Inter (fontdue).

use ferrite_ui::theme::Color;
use fontdue::{Font, FontSettings};

/// Police des titres : Inter demi-gras, comme les barres de titre de macOS.
const TITLE_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");
/// Taille du titre en pixels logiques.
pub const TITLE_SIZE: f32 = 13.0;
const ELLIPSIS: char = '…';

/// Image du titre : pixels `ARGB8888` prémultipliés, petit-boutistes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleBitmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug)]
pub struct TitleFont {
    font: Font,
}

impl TitleFont {
    pub fn new() -> Result<TitleFont, String> {
        let font = Font::from_bytes(TITLE_FONT, FontSettings::default()).map_err(|err| err.to_string())?;
        Ok(TitleFont { font })
    }

    /// Largeur du texte à la taille `px`, crénage compris.
    fn width(&self, chars: &[char], px: f32) -> f32 {
        let mut width = 0.0;
        for (i, &ch) in chars.iter().enumerate() {
            width += self.font.metrics(ch, px).advance_width;
            if let Some(&next) = chars.get(i + 1) {
                width += self.font.horizontal_kern(ch, next, px).unwrap_or(0.0);
            }
        }
        width
    }

    /// Dessine `text` à la taille `px` (pixels physiques), raccourci par « … » s'il dépasse
    /// `max_width`. Renvoie `None` si le texte est vide ou si rien ne tient.
    pub fn render(&self, text: &str, px: f32, color: Color, max_width: u32) -> Option<TitleBitmap> {
        let mut chars: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
        if chars.is_empty() || max_width == 0 {
            return None;
        }
        if self.width(&chars, px) > max_width as f32 {
            loop {
                chars.pop();
                while chars.last().is_some_and(|c| c.is_whitespace()) {
                    chars.pop();
                }
                if chars.is_empty() {
                    return None;
                }
                let mut candidate = chars.clone();
                candidate.push(ELLIPSIS);
                if self.width(&candidate, px) <= max_width as f32 {
                    chars = candidate;
                    break;
                }
            }
        }

        let line = self.font.horizontal_line_metrics(px)?;
        let width = self.width(&chars, px).ceil().max(1.0) as u32;
        let height = (line.ascent - line.descent).ceil().max(1.0) as u32;
        let mut coverage = vec![0u8; (width * height) as usize];

        let mut pen = 0.0f32;
        for (i, &ch) in chars.iter().enumerate() {
            let (metrics, glyph) = self.font.rasterize(ch, px);
            let left = (pen + metrics.xmin as f32).round() as i32;
            // `ymin` est la distance du bas du glyphe à la ligne de base.
            let top = (line.ascent - metrics.height as f32 - metrics.ymin as f32).round() as i32;
            for gy in 0..metrics.height {
                for gx in 0..metrics.width {
                    let (x, y) = (left + gx as i32, top + gy as i32);
                    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                        continue;
                    }
                    let dst = &mut coverage[(y as u32 * width + x as u32) as usize];
                    *dst = (*dst).max(glyph[gy * metrics.width + gx]);
                }
            }
            pen += metrics.advance_width;
            if let Some(&next) = chars.get(i + 1) {
                pen += self.font.horizontal_kern(ch, next, px).unwrap_or(0.0);
            }
        }

        let pixels = coverage
            .iter()
            .flat_map(|&c| {
                let alpha = (c as u32 * color.a as u32 / 255) as u8;
                color.with_alpha(alpha).to_argb8888_premultiplied().to_le_bytes()
            })
            .collect();
        Some(TitleBitmap {
            width,
            height,
            pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titre_dessine() {
        let font = TitleFont::new().unwrap();
        let bitmap = font.render("Terminal", 26.0, Color::BLACK, 1000).unwrap();
        assert!(
            bitmap.width > 60 && bitmap.width < 140,
            "largeur {}",
            bitmap.width
        );
        assert!(
            bitmap.height >= 26 && bitmap.height <= 36,
            "hauteur {}",
            bitmap.height
        );
        assert_eq!(bitmap.pixels.len(), (bitmap.width * bitmap.height * 4) as usize);
        // Du texte noir opaque quelque part.
        assert!(bitmap.pixels.chunks(4).any(|p| p == [0, 0, 0, 255]));
    }

    #[test]
    fn titre_raccourci() {
        let font = TitleFont::new().unwrap();
        let long = "Un titre de fenêtre beaucoup trop long pour la place disponible";
        let bitmap = font.render(long, 26.0, Color::BLACK, 200).unwrap();
        assert!(bitmap.width <= 200);
        assert_eq!(font.render("", 26.0, Color::BLACK, 200), None);
        assert_eq!(font.render("Titre", 26.0, Color::BLACK, 0), None);
    }
}
