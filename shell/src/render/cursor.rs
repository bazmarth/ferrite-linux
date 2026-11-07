//! Curseur Ferrite : formes vectorielles noires bordées de blanc, dessinées à l'échelle de
//! l'écran (nettes en échelle 2 comme en 1) — la flèche, les doubles flèches du
//! redimensionnement, et les formes que les applications demandent par leur nom (texte,
//! main, déplacement, viseur). Utilisé quand ferrite-shell affiche lui-même le pointeur (backend
//! DRM) ; en fenêtre, c'est le curseur du système hôte qui sert.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

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
/// Barre de texte, centrée sur le point actif.
const TEXT_BEAM: [(f64, f64); 12] = [
    (-4.5, -10.0),
    (4.5, -10.0),
    (4.5, -7.2),
    (1.5, -7.2),
    (1.5, 7.2),
    (4.5, 7.2),
    (4.5, 10.0),
    (-4.5, 10.0),
    (-4.5, 7.2),
    (-1.5, 7.2),
    (-1.5, -7.2),
    (-4.5, -7.2),
];
/// Main qui montre un lien, le bout de l'index sur le point actif.
const HAND: [(f64, f64); 21] = [
    (-2.3, 1.6),
    (-1.6, 0.3),
    (0.0, -0.2),
    (1.6, 0.3),
    (2.3, 1.6),
    (2.3, 7.0),
    (4.6, 6.6),
    (6.6, 7.2),
    (8.6, 8.0),
    (9.7, 9.6),
    (9.7, 15.0),
    (8.2, 19.0),
    (-0.6, 19.0),
    (-3.6, 15.6),
    (-7.1, 11.6),
    (-7.3, 10.2),
    (-6.1, 9.4),
    (-4.4, 10.0),
    (-2.3, 12.0),
    (-2.3, 6.0),
    (-2.3, 3.0),
];
/// Quatre flèches : déplacer ; centrées sur le point actif.
const MOVE: [(f64, f64); 24] = [
    (1.6, -1.6),
    (5.5, -1.6),
    (5.5, -5.0),
    (10.5, 0.0),
    (5.5, 5.0),
    (5.5, 1.6),
    (1.6, 1.6),
    (1.6, 5.5),
    (5.0, 5.5),
    (0.0, 10.5),
    (-5.0, 5.5),
    (-1.6, 5.5),
    (-1.6, 1.6),
    (-5.5, 1.6),
    (-5.5, 5.0),
    (-10.5, 0.0),
    (-5.5, -5.0),
    (-5.5, -1.6),
    (-1.6, -1.6),
    (-1.6, -5.5),
    (-5.0, -5.5),
    (0.0, -10.5),
    (5.0, -5.5),
    (1.6, -5.5),
];
/// Croix de visée fine.
const CROSS: [(f64, f64); 12] = [
    (1.5, -1.5),
    (9.0, -1.5),
    (9.0, 1.5),
    (1.5, 1.5),
    (1.5, 9.0),
    (-1.5, 9.0),
    (-1.5, 1.5),
    (-9.0, 1.5),
    (-9.0, -1.5),
    (-1.5, -1.5),
    (-1.5, -9.0),
    (1.5, -9.0),
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
    /// Texte à sélectionner.
    Text,
    /// Lien ou bouton.
    Pointer,
    /// Déplacer.
    Move,
    /// Viser un point précis.
    Crosshair,
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
            CursorShape::Text => (&TEXT_BEAM, 0.0),
            CursorShape::Pointer => (&HAND, 0.0),
            CursorShape::Move => (&MOVE, 0.0),
            CursorShape::Crosshair => (&CROSS, 0.0),
        }
    }
}

/// Image du curseur à une échelle donnée.
#[derive(Debug, Clone)]
pub struct CursorImage {
    pub buffer: MemoryRenderBuffer,
    /// Taille de l'image, en pixels.
    pub size: (i32, i32),
    /// Taille affichée, en pixels logiques (l'image d'un thème est agrandie ou réduite à la
    /// taille demandée).
    pub logical: (f64, f64),
    /// Position du point actif dans l'image affichée, en pixels physiques de l'écran.
    pub hotspot: (i32, i32),
}

/// Taille de référence des formes vectorielles (la taille « Normale »).
const BUILTIN_SIZE: f64 = 24.0;

/// Noms cherchés dans un thème pour chaque forme : celui de la spécification CSS, puis les
/// anciens noms X11.
fn xcursor_names(shape: CursorShape) -> &'static [&'static str] {
    match shape {
        CursorShape::Arrow => &["default", "left_ptr"],
        CursorShape::Horizontal => &["ew-resize", "sb_h_double_arrow", "h_double_arrow"],
        CursorShape::Vertical => &["ns-resize", "sb_v_double_arrow", "v_double_arrow"],
        CursorShape::DiagonalDown => &["nwse-resize", "bd_double_arrow", "size_fdiag"],
        CursorShape::DiagonalUp => &["nesw-resize", "fd_double_arrow", "size_bdiag"],
        CursorShape::Text => &["text", "xterm"],
        CursorShape::Pointer => &["pointer", "hand2", "hand1"],
        // « move » est une simple flèche dans certains thèmes (Adwaita) : les quatre
        // flèches d'abord.
        CursorShape::Move => &["all-scroll", "fleur", "move"],
        CursorShape::Crosshair => &["crosshair", "cross", "tcross"],
    }
}

/// Pack `Ferrite-Cursors` de `ferrite-theme`, embarqué : rien à installer.
const FERRITE_PACK: [(CursorShape, &[u8]); 9] = [
    (
        CursorShape::Arrow,
        include_bytes!("../../../ferrite-theme/cursors/Ferrite-Cursors/cursors/default"),
    ),
    (
        CursorShape::Horizontal,
        include_bytes!("../../../ferrite-theme/cursors/Ferrite-Cursors/cursors/ew-resize"),
    ),
    (
        CursorShape::Vertical,
        include_bytes!("../../../ferrite-theme/cursors/Ferrite-Cursors/cursors/ns-resize"),
    ),
    (
        CursorShape::DiagonalDown,
        include_bytes!("../../../ferrite-theme/cursors/Ferrite-Cursors/cursors/nwse-resize"),
    ),
    (
        CursorShape::DiagonalUp,
        include_bytes!("../../../ferrite-theme/cursors/Ferrite-Cursors/cursors/nesw-resize"),
    ),
    (
        CursorShape::Text,
        include_bytes!("../../../ferrite-theme/cursors/Ferrite-Cursors/cursors/text"),
    ),
    (
        CursorShape::Pointer,
        include_bytes!("../../../ferrite-theme/cursors/Ferrite-Cursors/cursors/pointer"),
    ),
    (
        CursorShape::Move,
        include_bytes!("../../../ferrite-theme/cursors/Ferrite-Cursors/cursors/move"),
    ),
    (
        CursorShape::Crosshair,
        include_bytes!("../../../ferrite-theme/cursors/Ferrite-Cursors/cursors/crosshair"),
    ),
];

/// Une image d'un fichier Xcursor, à une taille nominale.
#[derive(Debug)]
struct ThemeImage {
    nominal: u32,
    width: u32,
    height: u32,
    hotspot: (u32, u32),
    /// `ARGB8888` prémultiplié petit-boutiste, tel que dans le fichier.
    pixels: Vec<u8>,
}

/// Formes d'un thème de curseurs, lues une fois (dans le fil système, jamais au rendu).
#[derive(Debug)]
pub struct ThemeCursors {
    /// Numéro unique de ce chargement : change le cache des images.
    generation: u64,
    shapes: HashMap<CursorShape, Vec<ThemeImage>>,
}

const ALL_SHAPES: [CursorShape; 9] = [
    CursorShape::Arrow,
    CursorShape::Horizontal,
    CursorShape::Vertical,
    CursorShape::DiagonalDown,
    CursorShape::DiagonalUp,
    CursorShape::Text,
    CursorShape::Pointer,
    CursorShape::Move,
    CursorShape::Crosshair,
];

impl ThemeCursors {
    /// Thème `id` : le pack Ferrite embarqué, ou un thème installé. `None` pour les formes
    /// vectorielles, ou si le thème ne donne aucune forme lisible.
    pub fn load(id: &str) -> Option<ThemeCursors> {
        use ferrite_system::cursors;
        let mut shapes = HashMap::new();
        if id == cursors::BUILTIN {
            return None;
        } else if id == cursors::FERRITE_PACK {
            for (shape, bytes) in FERRITE_PACK {
                if let Some(images) = parse(bytes) {
                    shapes.insert(shape, images);
                }
            }
        } else {
            let theme = xcursor::CursorTheme::load(id);
            for shape in ALL_SHAPES {
                let images = xcursor_names(shape).iter().find_map(|name| {
                    let path = theme.load_icon(name)?;
                    parse(&std::fs::read(path).ok()?)
                });
                if let Some(images) = images {
                    shapes.insert(shape, images);
                }
            }
        }
        if shapes.is_empty() {
            return None;
        }
        static GENERATION: AtomicU64 = AtomicU64::new(1);
        Some(ThemeCursors {
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            shapes,
        })
    }

    /// Image la plus proche au-dessus de `pixels` (sinon la plus grande) : réduire garde le
    /// dessin net, agrandir le floute.
    fn pick(&self, shape: CursorShape, pixels: f64) -> Option<&ThemeImage> {
        let images = self.shapes.get(&shape)?;
        images
            .iter()
            .filter(|image| f64::from(image.nominal) >= pixels - 0.5)
            .min_by_key(|image| image.nominal)
            .or_else(|| images.iter().max_by_key(|image| image.nominal))
    }
}

/// Images d'un fichier Xcursor : la première de chaque taille (un curseur animé n'est pas
/// animé ici).
fn parse(bytes: &[u8]) -> Option<Vec<ThemeImage>> {
    let mut images: Vec<ThemeImage> = Vec::new();
    for image in xcursor::parser::parse_xcursor(bytes)? {
        let expected = (image.width * image.height * 4) as usize;
        if image.width == 0
            || image.height == 0
            || image.pixels_rgba.len() != expected
            || images.iter().any(|known| known.nominal == image.size)
        {
            continue;
        }
        images.push(ThemeImage {
            nominal: image.size,
            width: image.width,
            height: image.height,
            hotspot: (image.xhot.min(image.width), image.yhot.min(image.height)),
            pixels: image.pixels_rgba,
        });
    }
    (!images.is_empty()).then_some(images)
}

/// Clé du cache : forme, échelle et taille (en millièmes), chargement du thème (0 : formes
/// vectorielles).
type CacheKey = (CursorShape, u32, u32, u64);

#[derive(Debug, Default)]
pub struct Cursor {
    /// Images déjà préparées.
    cache: RefCell<HashMap<CacheKey, CursorImage>>,
    /// Thème des images du cache : un autre thème vide le cache.
    generation: Cell<u64>,
}

impl Cursor {
    /// Image de `shape` pour l'échelle `scale`, à la taille `size` (pixels logiques), prise
    /// dans `theme` s'il a cette forme, sinon dessinée (une seule fois dans les deux cas).
    pub fn image(
        &self,
        shape: CursorShape,
        scale: f64,
        size: u32,
        theme: Option<&ThemeCursors>,
    ) -> CursorImage {
        let theme = theme.filter(|theme| theme.shapes.contains_key(&shape));
        let generation = theme.map_or(0, |theme| theme.generation);
        if theme.is_some() && self.generation.replace(generation) != generation {
            self.cache.borrow_mut().clear();
        }
        let key = (shape, (scale * 1000.0).round() as u32, size, generation);
        self.cache
            .borrow_mut()
            .entry(key)
            .or_insert_with(
                || match theme.and_then(|theme| theme.pick(shape, f64::from(size) * scale)) {
                    Some(image) => themed_image(image, scale, size),
                    None => vector_image(shape, scale, size),
                },
            )
            .clone()
    }
}

fn vector_image(shape: CursorShape, scale: f64, size: u32) -> CursorImage {
    let factor = scale * f64::from(size) / BUILTIN_SIZE;
    let shape = Outline::new(shape);
    let (width, height, pixels) = shape.rasterize(factor);
    CursorImage {
        buffer: buffer(&pixels, width, height),
        size: (width as i32, height as i32),
        logical: (f64::from(width) / scale, f64::from(height) / scale),
        hotspot: shape.hotspot(factor),
    }
}

/// Image d'un thème ramenée à `size` : sa taille nominale correspond à `size` pixels logiques.
fn themed_image(image: &ThemeImage, scale: f64, size: u32) -> CursorImage {
    let factor = f64::from(size) / f64::from(image.nominal.max(1));
    CursorImage {
        buffer: buffer(&image.pixels, image.width, image.height),
        size: (image.width as i32, image.height as i32),
        logical: (f64::from(image.width) * factor, f64::from(image.height) * factor),
        hotspot: (
            (f64::from(image.hotspot.0) * factor * scale).round() as i32,
            (f64::from(image.hotspot.1) * factor * scale).round() as i32,
        ),
    }
}

fn buffer(pixels: &[u8], width: u32, height: u32) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(
        pixels,
        Fourcc::Argb8888,
        (width as i32, height as i32),
        1,
        Transform::Normal,
        None,
    )
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
    fn formes_nommees_dessinees_autour_du_point_actif() {
        for shape in [
            CursorShape::Text,
            CursorShape::Pointer,
            CursorShape::Move,
            CursorShape::Crosshair,
        ] {
            let image = Cursor::default().image(shape, 2.0, 24, None);
            let outline = Outline::new(shape);
            let (width, height, pixels) = outline.rasterize(2.0);
            assert!(pixels.chunks(4).any(|pixel| pixel[3] == 255), "{shape:?} vide");
            let (x, y) = image.hotspot;
            assert!(
                x >= 0 && y >= 0 && (x as u32) < width && (y as u32) < height,
                "{shape:?}"
            );
        }
    }

    #[test]
    fn echelle_deux() {
        let cursor = Cursor::default();
        let image = cursor.image(CursorShape::Arrow, 2.0, 24, None);
        assert_eq!(image.hotspot, (2, 2));
        assert_eq!(image.size, (28, 41));
        assert_eq!(image.logical, (14.0, 20.5));
    }

    #[test]
    fn grande_taille_vectorielle() {
        let image = Cursor::default().image(CursorShape::Arrow, 2.0, 48, None);
        assert_eq!(image.hotspot, (4, 4));
        // Deux fois la taille Normale : point actif et hauteur doublés.
        assert_eq!(image.size, (55, 82));
        assert_eq!(image.logical, (27.5, 41.0));
    }

    #[test]
    fn pack_ferrite_embarque() {
        let theme = ThemeCursors::load(ferrite_system::cursors::FERRITE_PACK).expect("pack lisible");
        for shape in ALL_SHAPES {
            assert!(theme.shapes.contains_key(&shape), "{shape:?} absent du pack");
        }
        // Écran ×2, taille Normale : l'image de 48 px, affichée sur 24 points.
        let image = Cursor::default().image(CursorShape::Arrow, 2.0, 24, Some(&theme));
        assert_eq!(image.size.0 as f64, image.logical.0 * 2.0);
        // Couleurs prémultipliées, comme le veut `Argb8888` ici : jamais plus que l'opacité.
        let pixels = &theme.shapes[&CursorShape::Arrow][0].pixels;
        assert!(
            pixels
                .chunks(4)
                .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3])
        );
        assert!(pixels.chunks(4).any(|p| p[3] == 255));
        // Très grande sur ×2 : 96 px, l'image la plus grande du pack.
        let image = Cursor::default().image(CursorShape::Arrow, 2.0, 48, Some(&theme));
        assert_eq!(image.size.0 as f64, image.logical.0 * 2.0);
        assert!(ThemeCursors::load(ferrite_system::cursors::BUILTIN).is_none());
    }

    #[test]
    fn choix_de_l_image_du_theme() {
        let theme = ThemeCursors {
            generation: 99,
            shapes: HashMap::from([(
                CursorShape::Arrow,
                [24, 48, 96]
                    .map(|nominal| ThemeImage {
                        nominal,
                        width: nominal,
                        height: nominal,
                        hotspot: (nominal / 8, nominal / 8),
                        pixels: vec![0; (nominal * nominal * 4) as usize],
                    })
                    .into(),
            )]),
        };
        assert_eq!(
            theme.pick(CursorShape::Arrow, 30.0).map(|image| image.nominal),
            Some(48)
        );
        assert_eq!(
            theme.pick(CursorShape::Arrow, 48.0).map(|image| image.nominal),
            Some(48)
        );
        assert_eq!(
            theme.pick(CursorShape::Arrow, 200.0).map(|image| image.nominal),
            Some(96)
        );
        assert!(theme.pick(CursorShape::Text, 24.0).is_none());
        // Forme absente du thème : la forme vectorielle, à la même taille.
        let cursor = Cursor::default();
        let text = cursor.image(CursorShape::Text, 1.0, 24, Some(&theme));
        let vector = cursor.image(CursorShape::Text, 1.0, 24, None);
        assert_eq!(text.size, vector.size);
        // Échelle 1, taille Grande (32) : image de 48 réduite à 32 points, point actif suivi.
        let arrow = cursor.image(CursorShape::Arrow, 1.0, 32, Some(&theme));
        assert_eq!(arrow.size, (48, 48));
        assert_eq!(arrow.logical, (32.0, 32.0));
        assert_eq!(arrow.hotspot, (4, 4));
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
