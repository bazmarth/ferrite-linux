//! Image des fichiers glissés sous le pointeur, quand ferrite-shell a repris le glisser
//! (voir `desktop::file_drag`) : l'icône du premier fichier, et un badge rouge avec leur
//! nombre s'ils sont plusieurs.

use std::cell::RefCell;

use ferrite_ui::theme::Color;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{Kind, memory::MemoryRenderBuffer, memory::MemoryRenderBufferRenderElement},
            gles::GlesRenderer,
        },
    },
    utils::{Logical, Point, Rectangle},
};
use tracing::warn;

use super::{OutputElement, title::TitleFont};
use crate::desktop::svg::{self, IconCache, Style};

/// Côté de l'icône et décalage par rapport à la pointe du curseur, en points.
const ICON: f64 = 56.0;
const OFFSET: (f64, f64) = (6.0, 10.0);
/// Badge du nombre de fichiers : hauteur et rouge des pastilles de macOS.
const BADGE: f64 = 22.0;
const BADGE_RED: u32 = 0xFF3B30;

/// Image prête : icône, nombre, échelle ; tampon et sa taille en pixels.
type Cached = ((String, usize, u32), MemoryRenderBuffer, (i32, i32));

#[derive(Debug)]
pub struct Ghost {
    icons: IconCache,
    font: Option<TitleFont>,
    cached: RefCell<Option<Cached>>,
}

impl Default for Ghost {
    fn default() -> Self {
        Self {
            icons: IconCache::default(),
            font: TitleFont::new().ok(),
            cached: RefCell::new(None),
        }
    }
}

impl Ghost {
    fn image(&self, icon: &str, count: usize, scale: f64) -> (MemoryRenderBuffer, (i32, i32)) {
        let key = (icon.to_owned(), count, (scale * 1000.0).round() as u32);
        if let Some((known, buffer, size)) = self.cached.borrow().as_ref()
            && *known == key
        {
            return (buffer.clone(), *size);
        }
        let badge = (count > 1).then(|| count.to_string());
        let side = (ICON * scale).round() as u32;
        // Place à droite et en haut pour le badge qui déborde de l'icône.
        let margin = if badge.is_some() {
            (BADGE * 0.5 * scale).round() as u32
        } else {
            0
        };
        let (width, height) = (side + margin, side + margin);
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let name = svg::known(icon).unwrap_or("file-generic");
        self.icons.draw(
            &mut pixels,
            width,
            height,
            name,
            0.0,
            margin as f32,
            side as f32,
            Style::Color,
        );
        // Un peu transparente, comme sur macOS.
        for pixel in pixels.chunks_exact_mut(4) {
            for channel in pixel {
                *channel = (u32::from(*channel) * 217 / 255) as u8;
            }
        }
        if let Some(label) = badge {
            let h = (BADGE * scale).round();
            let text = self
                .font
                .as_ref()
                .and_then(|font| font.render(&label, (13.0 * scale) as f32, Color::WHITE, width));
            let text_w = text.as_ref().map_or(0.0, |text| f64::from(text.width));
            let w = (text_w + 12.0 * scale).max(h);
            let (x, y) = (f64::from(width) - w, 0.0);
            fill_pill(&mut pixels, width, height, (x, y, w, h), BADGE_RED);
            if let Some(text) = text {
                let tx = (x + (w - f64::from(text.width)) / 2.0).round() as i32;
                let ty = (y + (h - f64::from(text.height)) / 2.0).round() as i32;
                blit(
                    &mut pixels,
                    width,
                    height,
                    &text.pixels,
                    text.width,
                    text.height,
                    tx,
                    ty,
                );
            }
        }
        let buffer = MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Argb8888,
            (width as i32, height as i32),
            1,
            smithay::utils::Transform::Normal,
            None,
        );
        let size = (width as i32, height as i32);
        *self.cached.borrow_mut() = Some((key, buffer.clone(), size));
        (buffer, size)
    }

    /// Image sous le pointeur `pointer` (logique, relatif à l'écran).
    pub fn push(
        &self,
        renderer: &mut GlesRenderer,
        icon: &str,
        count: usize,
        pointer: Point<f64, Logical>,
        scale: f64,
        out: &mut Vec<OutputElement>,
    ) {
        let (buffer, (width, height)) = self.image(icon, count, scale);
        let position = Point::<f64, Logical>::from((
            pointer.x + OFFSET.0,
            pointer.y + OFFSET.1 - f64::from(height) / scale + ICON,
        ));
        let src = Rectangle::from_size((f64::from(width), f64::from(height)).into());
        let size = (
            (f64::from(width) / scale).round() as i32,
            (f64::from(height) / scale).round() as i32,
        );
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            position.to_physical(scale),
            &buffer,
            None,
            Some(src),
            Some(size.into()),
            Kind::Unspecified,
        ) {
            Ok(element) => out.push(OutputElement::Memory(element)),
            Err(err) => warn!(?err, "image du glisser non importée"),
        }
    }
}

/// Pilule pleine de couleur `rgb` dans `(x, y, w, h)`, bords adoucis.
fn fill_pill(pixels: &mut [u8], width: u32, height: u32, (x, y, w, h): (f64, f64, f64, f64), rgb: u32) {
    let radius = h / 2.0;
    let [r, g, b] = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
    for py in (y.floor().max(0.0) as u32)..((y + h).ceil() as u32).min(height) {
        for px in (x.floor().max(0.0) as u32)..((x + w).ceil() as u32).min(width) {
            let (cx, cy) = (f64::from(px) + 0.5, f64::from(py) + 0.5);
            let qx = (cx - (x + w / 2.0)).abs() - (w / 2.0 - radius);
            let qy = (cy - (y + h / 2.0)).abs() - (h / 2.0 - radius);
            let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
            let alpha = ((0.5 - distance).clamp(0.0, 1.0) * 255.0) as u32;
            if alpha == 0 {
                continue;
            }
            let at = ((py * width + px) * 4) as usize;
            let inverse = 255 - alpha;
            for (offset, value) in [(0, b), (1, g), (2, r), (3, 255)] {
                let below = u32::from(pixels[at + offset]);
                pixels[at + offset] = (u32::from(value) * alpha / 255 + below * inverse / 255).min(255) as u8;
            }
        }
    }
}

/// Pose une image `ARGB8888` prémultipliée à (`x`, `y`).
#[allow(clippy::too_many_arguments)] // Tampons et géométrie, sans état global.
fn blit(pixels: &mut [u8], width: u32, height: u32, source: &[u8], w: u32, h: u32, x: i32, y: i32) {
    for row in 0..h as i32 {
        let py = y + row;
        if py < 0 || py >= height as i32 {
            continue;
        }
        for column in 0..w as i32 {
            let px = x + column;
            if px < 0 || px >= width as i32 {
                continue;
            }
            let from = ((row * w as i32 + column) * 4) as usize;
            let alpha = u32::from(source[from + 3]);
            let to = ((py as u32 * width + px as u32) * 4) as usize;
            let inverse = 255 - alpha;
            for offset in 0..4 {
                let below = u32::from(pixels[to + offset]);
                pixels[to + offset] =
                    (u32::from(source[from + offset]) + below * inverse / 255).min(255) as u8;
            }
        }
    }
}
