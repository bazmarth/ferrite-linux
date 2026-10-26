//! Curseur Ferrite : flèche vectorielle noire bordée de blanc, dessinée à l'échelle de
//! l'écran (nette en échelle 2 comme en 1). Utilisé quand ferrite-shell affiche lui-même le
//! pointeur (backend DRM) ; en fenêtre, c'est le curseur du système hôte qui sert.

use std::cell::RefCell;

use smithay::backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer};
use smithay::utils::Transform;

/// Contour de la flèche en pixels logiques ; la pointe (point actif) est en (0, 0).
const ARROW: [(f64, f64); 7] = [
    (0.0, 0.0),
    (0.0, 16.0),
    (3.8, 12.4),
    (6.4, 18.4),
    (8.9, 17.3),
    (6.4, 11.5),
    (11.6, 11.5),
];
/// Épaisseur de la bordure blanche (vers l'intérieur) et débord vers l'extérieur.
const BORDER_INSIDE: f64 = 1.1;
const BORDER_OUTSIDE: f64 = 0.5;
/// Marge autour de la flèche dans l'image, en pixels logiques.
const PADDING: f64 = 1.0;
/// Échantillons par pixel et par axe pour l'anticrénelage.
const SAMPLES: usize = 4;

/// Image du curseur à une échelle donnée.
#[derive(Debug, Clone)]
pub struct CursorImage {
    pub buffer: MemoryRenderBuffer,
    /// Taille en pixels physiques.
    pub size: (i32, i32),
    /// Position de la pointe dans l'image, en pixels physiques.
    pub hotspot: (i32, i32),
}

#[derive(Debug, Default)]
pub struct Cursor {
    cache: RefCell<Option<(f64, CursorImage)>>,
}

impl Cursor {
    /// Image du curseur pour l'échelle `scale` (recalculée seulement si elle change).
    pub fn image(&self, scale: f64) -> CursorImage {
        let mut cache = self.cache.borrow_mut();
        if let Some((cached_scale, image)) = cache.as_ref() {
            if *cached_scale == scale {
                return image.clone();
            }
        }
        let (width, height, pixels) = rasterize(scale);
        let buffer = MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Argb8888,
            (width as i32, height as i32),
            1,
            Transform::Normal,
            None,
        );
        let pad = (PADDING * scale).round() as i32;
        let image = CursorImage {
            buffer,
            size: (width as i32, height as i32),
            hotspot: (pad, pad),
        };
        *cache = Some((scale, image.clone()));
        image
    }
}

/// Dessine la flèche : renvoie largeur, hauteur et pixels `ARGB8888` prémultipliés
/// (petit-boutistes).
fn rasterize(scale: f64) -> (u32, u32, Vec<u8>) {
    let max_x = ARROW.iter().map(|p| p.0).fold(0.0, f64::max);
    let max_y = ARROW.iter().map(|p| p.1).fold(0.0, f64::max);
    let pad = (PADDING * scale).round();
    let width = ((max_x + 2.0 * PADDING) * scale).ceil() as u32;
    let height = ((max_y + 2.0 * PADDING) * scale).ceil() as u32;

    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for py in 0..height {
        for px in 0..width {
            // Moyenne des échantillons : (blanc, noir) pondérés par leur couverture.
            let (mut white, mut black) = (0.0, 0.0);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = (px as f64 + (sx as f64 + 0.5) / SAMPLES as f64 - pad) / scale;
                    let y = (py as f64 + (sy as f64 + 0.5) / SAMPLES as f64 - pad) / scale;
                    let inside = contains(x, y);
                    let distance = edge_distance(x, y);
                    if inside && distance >= BORDER_INSIDE {
                        black += 1.0;
                    } else if inside || distance < BORDER_OUTSIDE {
                        white += 1.0;
                    }
                }
            }
            let total = (SAMPLES * SAMPLES) as f64;
            let alpha = (white + black) / total;
            let luminance = white / total; // prémultiplié : blanc × couverture
            let a = (alpha * 255.0).round() as u8;
            let c = (luminance * 255.0).round() as u8;
            pixels.extend_from_slice(&[c, c, c, a]);
        }
    }
    (width, height, pixels)
}

/// Le point est-il dans la flèche (règle pair-impair) ?
fn contains(x: f64, y: f64) -> bool {
    let mut inside = false;
    // Chaque côté : du sommet précédent au sommet courant.
    let previous = ARROW.iter().cycle().skip(ARROW.len() - 1);
    for (&(xi, yi), &(xj, yj)) in ARROW.iter().zip(previous) {
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
    }
    inside
}

/// Distance du point au contour de la flèche.
fn edge_distance(x: f64, y: f64) -> f64 {
    let mut best = f64::MAX;
    for i in 0..ARROW.len() {
        let (ax, ay) = ARROW[i];
        let (bx, by) = ARROW[(i + 1) % ARROW.len()];
        let (dx, dy) = (bx - ax, by - ay);
        let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        let (cx, cy) = (ax + t * dx - x, ay + t * dy - y);
        best = best.min((cx * cx + cy * cy).sqrt());
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * width + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    #[test]
    fn fleche_noire_bordee_de_blanc() {
        let (width, height, pixels) = rasterize(1.0);
        assert_eq!((width, height), (14, 21));
        // Intérieur de la flèche : noir opaque.
        assert_eq!(pixel(&pixels, width, 4, 11), [0, 0, 0, 255]);
        // Bord gauche : blanc.
        let edge = pixel(&pixels, width, 1, 8);
        assert!(edge[0] > 200 && edge[3] > 200, "bord {edge:?}");
        // Loin de la flèche : transparent.
        assert_eq!(pixel(&pixels, width, 12, 2), [0, 0, 0, 0]);
    }

    #[test]
    fn echelle_deux() {
        let (width, height, _) = rasterize(2.0);
        assert_eq!((width, height), (28, 41));
        let cursor = Cursor::default();
        let image = cursor.image(2.0);
        assert_eq!(image.hotspot, (2, 2));
        assert_eq!(image.size, (28, 41));
    }
}
