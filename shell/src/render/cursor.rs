//! Curseur Ferrite : formes vectorielles noires bordées de blanc, dessinées à l'échelle de
//! l'écran (nettes en échelle 2 comme en 1) — la flèche, et les doubles flèches du
//! redimensionnement. Utilisé quand ferrite-shell affiche lui-même le pointeur (backend
//! DRM) ; en fenêtre, c'est le curseur du système hôte qui sert.

use std::{cell::RefCell, collections::HashMap};

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
/// Double flèche horizontale, centrée sur le point actif ; tournée pour les autres sens.
const DOUBLE_ARROW: [(f64, f64); 10] = [
    (-10.5, 0.0),
    (-5.5, -5.0),
    (-5.5, -1.6),
    (5.5, -1.6),
    (5.5, -5.0),
    (10.5, 0.0),
    (5.5, 5.0),
    (5.5, 1.6),
    (-5.5, 1.6),
    (-5.5, 5.0),
];
/// Épaisseur de la bordure blanche (vers l'intérieur) et débord vers l'extérieur.
const BORDER_INSIDE: f64 = 1.1;
const BORDER_OUTSIDE: f64 = 0.5;
/// Marge autour de la forme dans l'image, en pixels logiques.
const PADDING: f64 = 1.0;
/// Échantillons par pixel et par axe pour l'anticrénelage.
const SAMPLES: usize = 4;

/// Forme du curseur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CursorShape {
    #[default]
    Arrow,
    /// Redimensionnement gauche-droite.
    Horizontal,
    /// Redimensionnement haut-bas.
    Vertical,
    /// Coin haut-gauche ou bas-droite.
    DiagonalDown,
    /// Coin haut-droite ou bas-gauche.
    DiagonalUp,
}

impl CursorShape {
    /// Contour et rotation (en degrés) de la forme.
    fn outline(self) -> (&'static [(f64, f64)], f64) {
        match self {
            CursorShape::Arrow => (&ARROW, 0.0),
            CursorShape::Horizontal => (&DOUBLE_ARROW, 0.0),
            CursorShape::Vertical => (&DOUBLE_ARROW, 90.0),
            CursorShape::DiagonalDown => (&DOUBLE_ARROW, 45.0),
            CursorShape::DiagonalUp => (&DOUBLE_ARROW, -45.0),
        }
    }
}

/// Image du curseur à une échelle donnée.
#[derive(Debug, Clone)]
pub struct CursorImage {
    pub buffer: MemoryRenderBuffer,
    /// Taille en pixels physiques.
    pub size: (i32, i32),
    /// Position du point actif dans l'image, en pixels physiques.
    pub hotspot: (i32, i32),
}

#[derive(Debug, Default)]
pub struct Cursor {
    /// Images déjà dessinées, par forme et échelle (en millièmes).
    cache: RefCell<HashMap<(CursorShape, u32), CursorImage>>,
}

impl Cursor {
    /// Image de `shape` pour l'échelle `scale` (dessinée une seule fois).
    pub fn image(&self, shape: CursorShape, scale: f64) -> CursorImage {
        let key = (shape, (scale * 1000.0).round() as u32);
        self.cache
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let shape = Outline::new(shape);
                let (width, height, pixels) = shape.rasterize(scale);
                let buffer = MemoryRenderBuffer::from_slice(
                    &pixels,
                    Fourcc::Argb8888,
                    (width as i32, height as i32),
                    1,
                    Transform::Normal,
                    None,
                );
                let hotspot = shape.hotspot(scale);
                CursorImage {
                    buffer,
                    size: (width as i32, height as i32),
                    hotspot,
                }
            })
            .clone()
    }
}

/// Contour tourné, prêt à être dessiné.
struct Outline {
    points: Vec<(f64, f64)>,
    min: (f64, f64),
    max: (f64, f64),
}

impl Outline {
    fn new(shape: CursorShape) -> Self {
        let (points, degrees) = shape.outline();
        let (sin, cos) = degrees.to_radians().sin_cos();
        let points: Vec<(f64, f64)> = points
            .iter()
            .map(|&(x, y)| (x * cos - y * sin, x * sin + y * cos))
            .collect();
        let fold = |pick: fn(&(f64, f64)) -> f64, start: f64, f: fn(f64, f64) -> f64| {
            points.iter().map(pick).fold(start, f)
        };
        let min = (
            fold(|p| p.0, f64::MAX, f64::min),
            fold(|p| p.1, f64::MAX, f64::min),
        );
        let max = (
            fold(|p| p.0, f64::MIN, f64::max),
            fold(|p| p.1, f64::MIN, f64::max),
        );
        Self { points, min, max }
    }

    /// Point actif (origine des contours) dans l'image, en pixels physiques.
    fn hotspot(&self, scale: f64) -> (i32, i32) {
        (
            ((PADDING - self.min.0) * scale).round() as i32,
            ((PADDING - self.min.1) * scale).round() as i32,
        )
    }

    /// Dessine la forme : renvoie largeur, hauteur et pixels `ARGB8888` prémultipliés
    /// (petit-boutistes).
    fn rasterize(&self, scale: f64) -> (u32, u32, Vec<u8>) {
        let width = ((self.max.0 - self.min.0 + 2.0 * PADDING) * scale).ceil() as u32;
        let height = ((self.max.1 - self.min.1 + 2.0 * PADDING) * scale).ceil() as u32;
        let (hx, hy) = self.hotspot(scale);

        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for py in 0..height {
            for px in 0..width {
                // Moyenne des échantillons : (blanc, noir) pondérés par leur couverture.
                let (mut white, mut black) = (0.0, 0.0);
                for sy in 0..SAMPLES {
                    for sx in 0..SAMPLES {
                        let x = (px as f64 + (sx as f64 + 0.5) / SAMPLES as f64 - hx as f64) / scale;
                        let y = (py as f64 + (sy as f64 + 0.5) / SAMPLES as f64 - hy as f64) / scale;
                        let inside = self.contains(x, y);
                        let distance = self.edge_distance(x, y);
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

    /// Le point est-il dans la forme (règle pair-impair) ?
    fn contains(&self, x: f64, y: f64) -> bool {
        let mut inside = false;
        let count = self.points.len();
        for i in 0..count {
            let (xi, yi) = self.points[i];
            let (xj, yj) = self.points[(i + count - 1) % count];
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                inside = !inside;
            }
        }
        inside
    }

    /// Distance du point au contour.
    fn edge_distance(&self, x: f64, y: f64) -> f64 {
        let count = self.points.len();
        let mut best = f64::MAX;
        for i in 0..count {
            let (ax, ay) = self.points[i];
            let (bx, by) = self.points[(i + 1) % count];
            let (dx, dy) = (bx - ax, by - ay);
            let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            let (cx, cy) = (ax + t * dx - x, ay + t * dy - y);
            best = best.min((cx * cx + cy * cy).sqrt());
        }
        best
    }
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
        let (width, height, pixels) = Outline::new(CursorShape::Arrow).rasterize(1.0);
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
        let cursor = Cursor::default();
        let image = cursor.image(CursorShape::Arrow, 2.0);
        assert_eq!(image.hotspot, (2, 2));
        assert_eq!(image.size, (28, 41));
    }

    #[test]
    fn doubles_fleches_centrees_sur_le_point_actif() {
        for shape in [
            CursorShape::Horizontal,
            CursorShape::Vertical,
            CursorShape::DiagonalDown,
            CursorShape::DiagonalUp,
        ] {
            let outline = Outline::new(shape);
            let (width, height, pixels) = outline.rasterize(1.0);
            let (hx, hy) = outline.hotspot(1.0);
            // Le centre de la double flèche est noir, sous le point actif.
            assert_eq!(pixel(&pixels, width, hx as u32, hy as u32)[3], 255, "{shape:?}");
            assert!(
                (hx - width as i32 / 2).abs() <= 1 && (hy - height as i32 / 2).abs() <= 1,
                "{shape:?}"
            );
        }
        let (width, height, _) = Outline::new(CursorShape::Vertical).rasterize(1.0);
        assert!(height > width);
    }
}
