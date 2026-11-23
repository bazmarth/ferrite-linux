//! Fonds d'écran de Ferrite : ceux dessinés par Ferrite (thème « Fusion » : un dégradé
//! sombre ou clair et trois lueurs douces, comme sur la maquette), ou une image PNG ou
//! JPEG de l'utilisateur, recadrée pour couvrir l'écran. Partagé par le compositeur (fond
//! réel et écran de verrouillage) et Paramètres (vignettes).

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
};

use crate::theme::Color;

/// Fond d'écran choisi, tel qu'écrit dans les réglages (`fond_ecran`) : le nom d'un fond
/// de Ferrite, ou le chemin absolu d'une image.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    Builtin(Builtin),
    File(PathBuf),
}

impl Default for Source {
    fn default() -> Self {
        Source::Builtin(Builtin::DEFAULT)
    }
}

impl Source {
    pub fn parse(value: &str) -> Source {
        let value = value.trim();
        if value.starts_with('/') {
            return Source::File(PathBuf::from(value));
        }
        // Anciens fonds aux hexagones : remplacés par Fusion, sombre ou clair.
        let value = match value {
            "ferrite" | "nuit" | "ardoise" => "fusion",
            "clair" => "fusion-claire",
            other => other,
        };
        Builtin::ALL
            .into_iter()
            .find(|builtin| builtin.key() == value)
            .map(Source::Builtin)
            .unwrap_or_default()
    }

    pub fn key(&self) -> String {
        match self {
            Source::Builtin(builtin) => builtin.key(),
            Source::File(path) => path.display().to_string(),
        }
    }
}

/// Ambiance d'un fond dessiné : la teinte de la base et des trois lueurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mood {
    /// Celle de la maquette : lueur orange en haut à droite, prune en bas à gauche.
    Fusion,
    /// Plus rouge, comme des braises.
    Ember,
    /// Rose et violet.
    Amethyst,
    /// Orange sur un bleu-vert profond.
    Lagoon,
}

/// Teintes d'une ambiance : base en dégradé (haut gauche, milieu, bas droite), lueur
/// chaude (haut droite), lueur profonde (bas gauche), halo du centre, trait de l'anneau.
struct Tones {
    base: [u32; 3],
    warm: u32,
    deep: u32,
    glow: u32,
    line: u32,
}

impl Mood {
    pub const ALL: [Mood; 4] = [Mood::Fusion, Mood::Ember, Mood::Amethyst, Mood::Lagoon];

    fn key(self) -> &'static str {
        match self {
            Mood::Fusion => "fusion",
            Mood::Ember => "braise",
            Mood::Amethyst => "amethyste",
            Mood::Lagoon => "lagune",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mood::Fusion => "Fusion",
            Mood::Ember => "Braise",
            Mood::Amethyst => "Améthyste",
            Mood::Lagoon => "Lagune",
        }
    }

    /// Teintes en sombre ou en clair. Sur le bureau clair, la lueur chaude est adoucie
    /// (`soft`), comme sur la maquette ; l'écran de verrouillage la garde franche.
    fn tones(self, dark: bool, soft: bool) -> Tones {
        let (base, warm, soft_warm, deep, glow, line) = match (self, dark) {
            (Mood::Fusion, true) => (
                [0x1C1016, 0x0B0B0D, 0x140A12],
                0xFF7A45,
                0xFF7A45,
                0x7A2858,
                0xFFAA6E,
                0xFFB48C,
            ),
            (Mood::Fusion, false) => (
                [0xFDF0E8, 0xF4EFF3, 0xEFE3EC],
                0xFF7A45,
                0xFF9664,
                0xCD8CB9,
                0xFFAA6E,
                0xFFB48C,
            ),
            (Mood::Ember, true) => (
                [0x1A0E0A, 0x0C0A0A, 0x170B08],
                0xFF5A28,
                0xFF5A28,
                0x8C1E14,
                0xFFBE5A,
                0xFFAA78,
            ),
            (Mood::Ember, false) => (
                [0xFDF1E6, 0xF7EFEA, 0xF4E4DA],
                0xFF6E32,
                0xFF8C50,
                0xEB8C6E,
                0xFFC878,
                0xFFAA78,
            ),
            (Mood::Amethyst, true) => (
                [0x150F1E, 0x0B0B10, 0x120A18],
                0xFF6E96,
                0xFF6E96,
                0x6234A0,
                0xFFAA6E,
                0xFFB4C8,
            ),
            (Mood::Amethyst, false) => (
                [0xF8EFF8, 0xF2EFF6, 0xEBE4F3],
                0xFF7896,
                0xFF96AA,
                0xAF96E6,
                0xFFBE96,
                0xFFB4C8,
            ),
            (Mood::Lagoon, true) => (
                [0x0D1418, 0x0B0B0D, 0x0A1216],
                0xFF7A45,
                0xFF7A45,
                0x145A6E,
                0x78C8D2,
                0xFFB48C,
            ),
            (Mood::Lagoon, false) => (
                [0xFBF0E9, 0xEEF2F3, 0xE3EEF0],
                0xFF7A45,
                0xFF9664,
                0x82C3CD,
                0xFFBE8C,
                0xFFB48C,
            ),
        };
        Tones {
            base,
            warm: if soft { soft_warm } else { warm },
            deep,
            glow,
            line,
        }
    }

    /// Trois teintes `0xRRGGBB` pour une pastille de choix : de la lueur chaude à la
    /// lueur profonde, ce qui distingue le mieux les ambiances.
    pub fn swatch(self, dark: bool) -> [u32; 3] {
        let tones = self.tones(dark, false);
        [tones.warm, tones.deep, tones.deep]
    }
}

/// Fonds dessinés par Ferrite : une ambiance, en sombre ou en clair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Builtin {
    pub mood: Mood,
    pub dark: bool,
}

impl Builtin {
    /// Fond par défaut : Fusion sombre, celui de la maquette du bureau.
    pub const DEFAULT: Builtin = Builtin::new(Mood::Fusion, true);

    pub const ALL: [Builtin; 8] = [
        Builtin::new(Mood::Fusion, true),
        Builtin::new(Mood::Fusion, false),
        Builtin::new(Mood::Ember, true),
        Builtin::new(Mood::Ember, false),
        Builtin::new(Mood::Amethyst, true),
        Builtin::new(Mood::Amethyst, false),
        Builtin::new(Mood::Lagoon, true),
        Builtin::new(Mood::Lagoon, false),
    ];

    pub const fn new(mood: Mood, dark: bool) -> Builtin {
        Builtin { mood, dark }
    }

    /// Nom dans les réglages : `fusion`, `fusion-claire`…
    pub fn key(self) -> String {
        if self.dark {
            self.mood.key().to_owned()
        } else {
            format!("{}-claire", self.mood.key())
        }
    }

    /// Nom affiché : « Fusion », « Fusion claire »…
    pub fn label(self) -> String {
        if self.dark {
            self.mood.label().to_owned()
        } else {
            format!("{} claire", self.mood.label())
        }
    }
}

/// Lueur ronde, comme un `radial-gradient(circle r at cx cy, couleur, transparent reach)` :
/// centre en fractions de l'écran, rayon en fraction de son petit côté (un vrai cercle,
/// pas étiré sur un écran large), opacité au centre.
#[derive(Clone, Copy)]
struct Spot {
    center: (f32, f32),
    radius: f32,
    color: u32,
    alpha: f32,
    reach: f32,
}

/// Anneau centré : rayon intérieur et extérieur sur un écran de 900 pixels de haut.
#[derive(Clone, Copy)]
struct Ring {
    inner: f32,
    outer: f32,
    color: u32,
    alpha: f32,
}

/// Tout ce qu'il faut pour dessiner un fond : base, lueurs (de la plus basse à la plus
/// haute) et anneaux éventuels.
struct Scene {
    base: [u32; 3],
    spots: [Spot; 3],
    rings: Vec<Ring>,
}

impl Scene {
    /// Bureau : `Main.dc.html` de la maquette « Fusion », lueurs rondes.
    fn desktop(builtin: Builtin) -> Scene {
        let tones = builtin.mood.tones(builtin.dark, !builtin.dark);
        let (warm, deep, glow) = if builtin.dark {
            (0.50, 0.70, 0.14)
        } else {
            (0.40, 0.49, 0.14)
        };
        Scene {
            base: tones.base,
            spots: [
                spot((0.48, 0.60), 0.35, tones.glow, glow, 0.70),
                spot((0.10, 0.90), 0.60, tones.deep, deep, 0.62),
                spot((0.80, 0.20), 0.50, tones.warm, warm, 0.62),
            ],
            rings: Vec::new(),
        }
    }

    /// Écran de verrouillage : `Lock.dc.html`, lueurs plus franches et deux anneaux.
    fn lock(mood: Mood, dark: bool) -> Scene {
        let tones = mood.tones(dark, false);
        let deep = if dark { 0.80 } else { 0.56 };
        Scene {
            base: tones.base,
            spots: [
                spot((0.50, 0.50), 0.30, tones.glow, 0.18, 0.70),
                spot((0.12, 0.88), 0.60, tones.deep, deep, 0.62),
                spot((0.78, 0.22), 0.55, tones.warm, 0.60, 0.62),
            ],
            rings: vec![
                Ring {
                    inner: 460.0,
                    outer: 550.0,
                    color: tones.warm,
                    alpha: 0.08,
                },
                Ring {
                    inner: 409.0,
                    outer: 410.0,
                    color: tones.line,
                    alpha: 0.18,
                },
            ],
        }
    }
}

const fn spot(center: (f32, f32), radius: f32, color: u32, alpha: f32, reach: f32) -> Spot {
    Spot {
        center,
        radius,
        color,
        alpha,
        reach,
    }
}

/// Image RVBA 8 bits, non prémultipliée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// Vrai si la bande du haut, sur `rows` lignes, est sombre : la barre du haut, posée
    /// dessus, écrit alors en blanc.
    pub fn top_is_dark(&self, rows: u32) -> bool {
        let rows = rows.clamp(1, self.height.max(1));
        let pixels = &self.rgba[..(rows * self.width * 4) as usize];
        let count = (pixels.len() / 4).max(1) as f32;
        let luminance: f32 = pixels
            .chunks_exact(4)
            .map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32)
            .sum::<f32>()
            / count;
        luminance < 140.0
    }
}

/// Fond du bureau dessiné à `width` × `height` pixels. Dessiné par bandes, sur plusieurs
/// cœurs.
pub fn paint(builtin: Builtin, width: u32, height: u32) -> Image {
    paint_scene(&Scene::desktop(builtin), width, height, threads())
}

/// Fond de l'écran de verrouillage : la même ambiance, avec ses deux anneaux.
pub fn paint_lock(mood: Mood, dark: bool, width: u32, height: u32) -> Image {
    paint_scene(&Scene::lock(mood, dark), width, height, threads())
}

/// Seuils de tramage (matrice de Bayer 4 × 4) : évite les bandes dans les dégradés.
const BAYER: [f32; 16] = [
    0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0,
];

fn paint_scene(scene: &Scene, width: u32, height: u32, threads: usize) -> Image {
    let (width, height) = (width.max(1), height.max(1));
    let (w, h) = (width as f32, height as f32);
    // `linear-gradient(160deg, …)` : direction et longueur de la ligne du dégradé.
    let angle = 160f32.to_radians();
    let direction = (angle.sin(), -angle.cos());
    let length = (w * direction.0).abs() + (h * direction.1).abs();
    let base = scene.base.map(rgb);
    let spots = scene.spots.map(|spot| (spot, rgb(spot.color)));
    // Les anneaux ont la taille de la maquette (900 pixels de haut), à l'échelle.
    let scale = (w / 1440.0).min(h / 900.0);
    let rings: Vec<(Ring, [f32; 3])> = scene
        .rings
        .iter()
        .map(|ring| {
            let mut scaled = *ring;
            scaled.inner *= scale;
            scaled.outer = (ring.outer * scale).max(scaled.inner + 1.0);
            (scaled, rgb(ring.color))
        })
        .collect();
    let mut rgba = vec![0; (width * height * 4) as usize];
    in_bands(&mut rgba, width, threads, |first, _rows, band| {
        for (index, out) in band.chunks_exact_mut(4).enumerate() {
            let x = (index as u32 % width) as f32 + 0.5;
            let row = first + index as u32 / width;
            let y = row as f32 + 0.5;
            let t = 0.5 + ((x - w / 2.0) * direction.0 + (y - h / 2.0) * direction.1) / length;
            let mut pixel = if t < 0.55 {
                mix(base[0], base[1], (t / 0.55).clamp(0.0, 1.0))
            } else {
                mix(base[1], base[2], ((t - 0.55) / 0.45).clamp(0.0, 1.0))
            };
            for (spot, tone) in &spots {
                let radius = spot.radius * w.min(h);
                let distance = (x - spot.center.0 * w).hypot(y - spot.center.1 * h) / radius;
                if distance < spot.reach {
                    over(&mut pixel, *tone, spot.alpha * (1.0 - distance / spot.reach));
                }
            }
            if !rings.is_empty() {
                let distance = (x - w / 2.0).hypot(y - h / 2.0);
                for (ring, tone) in &rings {
                    let coverage = (distance - ring.inner + 0.5).clamp(0.0, 1.0)
                        * (ring.outer - distance + 0.5).clamp(0.0, 1.0);
                    if coverage > 0.0 {
                        over(&mut pixel, *tone, ring.alpha * coverage);
                    }
                }
            }
            let dither = (BAYER[((row % 4) * 4 + (index as u32 % width) % 4) as usize] + 0.5) / 16.0 - 0.5;
            let [r, g, b] = pixel.map(|channel| (channel + dither).round().clamp(0.0, 255.0) as u8);
            out.copy_from_slice(&[r, g, b, 255]);
        }
    });
    Image { width, height, rgba }
}

fn rgb(hex: u32) -> [f32; 3] {
    let color = Color::rgb(hex);
    [color.r, color.g, color.b].map(f32::from)
}

fn mix(from: [f32; 3], to: [f32; 3], amount: f32) -> [f32; 3] {
    std::array::from_fn(|channel| from[channel] + (to[channel] - from[channel]) * amount)
}

/// Pose `tone` à l'opacité `amount` sur `pixel`.
fn over(pixel: &mut [f32; 3], tone: [f32; 3], amount: f32) {
    *pixel = mix(*pixel, tone, amount);
}

/// Nombre de cœurs utilisés pour dessiner un fond (au plus 8).
fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, |count| count.get().min(8))
}

/// Bande de lignes à dessiner (première ligne et pixels), prise une seule fois.
type Band<'a> = Mutex<Option<(u32, &'a mut [u8])>>;

/// Partage `rgba` (lignes de `width` pixels RVBA) en bandes de lignes et appelle
/// `work(première ligne, nombre de lignes, bande)` sur chacune, en même temps, sur
/// `threads` fils. Une bande dont le fil ne démarre pas est faite sur place.
fn in_bands(rgba: &mut [u8], width: u32, threads: usize, work: impl Fn(u32, u32, &mut [u8]) + Sync) {
    let stride = width as usize * 4;
    let height = rgba.len() / stride.max(1);
    let per_band = height.div_ceil(threads.clamp(1, height.max(1))).max(1);
    // Chaque bande est prise une seule fois, par son fil ou, à défaut, par l'appelant.
    let bands: Vec<Band> = rgba
        .chunks_mut(per_band * stride)
        .enumerate()
        .map(|(index, band)| Mutex::new(Some(((index * per_band) as u32, band))))
        .collect();
    let run = |slot: &Band| {
        let taken = slot.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some((first, band)) = taken {
            work(first, (band.len() / stride) as u32, band);
        }
    };
    std::thread::scope(|scope| {
        for slot in bands.iter().skip(1) {
            let run = &run;
            if let Err(error) = std::thread::Builder::new()
                .name("fond d'écran".to_owned())
                .spawn_scoped(scope, move || run(slot))
            {
                eprintln!("fil de dessin du fond non démarré : {error}");
            }
        }
        // La première bande, puis celles dont le fil n'a pas démarré.
        for slot in &bands {
            run(slot);
        }
    });
}

/// Lit une image PNG ou JPEG (reconnue à son contenu, pas à son extension).
pub fn decode(path: &Path) -> Result<Image, String> {
    let data = fs::read(path).map_err(|error| format!("{} : {error}", path.display()))?;
    decode_bytes(data)
}

/// Comme [`decode`], depuis le contenu du fichier (pochette intégrée à un morceau, par exemple).
pub fn decode_bytes(data: Vec<u8>) -> Result<Image, String> {
    let image = if data.starts_with(b"\x89PNG") {
        decode_png(&data)
    } else if data.starts_with(&[0xFF, 0xD8]) {
        decode_jpeg(data)
    } else {
        Err("format non pris en charge (PNG ou JPEG seulement)".to_owned())
    }?;
    if image.width == 0 || image.height == 0 {
        return Err("image vide".to_owned());
    }
    Ok(image)
}

fn decode_png(data: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("PNG trop grand")?];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| error.to_string())?;
    let pixels = &buffer[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        png::ColorType::Indexed => return Err("PNG indexé non pris en charge".to_owned()),
    };
    Ok(Image {
        width: info.width,
        height: info.height,
        rgba,
    })
}

fn decode_jpeg(data: Vec<u8>) -> Result<Image, String> {
    let mut decoder = zune_jpeg::JpegDecoder::new(Cursor::new(data));
    let pixels = decoder
        .decode()
        .map_err(|error| format!("JPEG illisible : {error:?}"))?;
    let (width, height) = decoder.dimensions().ok_or("JPEG sans dimensions")?;
    let channels = pixels.len() / (width * height).max(1);
    let rgba = match channels {
        4 => pixels,
        3 => pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        1 => pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        _ => return Err("JPEG : espace de couleurs non pris en charge".to_owned()),
    };
    Ok(Image {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

/// Réduit ou agrandit `image` pour couvrir `width` × `height` sans déformation, en
/// coupant ce qui dépasse de part et d'autre. Chaque pixel est la moyenne de la zone de
/// l'image qu'il recouvre (net en réduction comme en agrandissement), et l'image finale
/// est opaque.
pub fn cover(image: &Image, width: u32, height: u32) -> Image {
    cover_with(image, width, height, threads())
}

fn cover_with(image: &Image, width: u32, height: u32, threads: usize) -> Image {
    let (width, height) = (width.max(1), height.max(1));
    let ratio = (width as f64 / image.width as f64).max(height as f64 / image.height as f64);
    // Zone de l'image retenue, en pixels de l'image.
    let (span_x, span_y) = (width as f64 / ratio, height as f64 / ratio);
    let (left, top) = (
        (image.width as f64 - span_x) / 2.0,
        (image.height as f64 - span_y) / 2.0,
    );
    let column = |x: u32| {
        let from = left + x as f64 / ratio;
        let start = (from.floor() as u32).min(image.width - 1);
        let end = ((from + 1.0 / ratio).ceil() as u32).clamp(start + 1, image.width);
        (start, end)
    };
    let columns: Vec<(u32, u32)> = (0..width).map(column).collect();
    let mut rgba = vec![0; (width * height * 4) as usize];
    // Chaque ligne ne dépend que de l'image source : bandes dessinées en même temps.
    in_bands(&mut rgba, width, threads, |first, _, band| {
        for (offset, line) in band.chunks_exact_mut(width as usize * 4).enumerate() {
            let y = first + offset as u32;
            let from = top + y as f64 / ratio;
            let start = (from.floor() as u32).min(image.height - 1);
            let end = ((from + 1.0 / ratio).ceil() as u32).clamp(start + 1, image.height);
            for (&(x0, x1), out) in columns.iter().zip(line.chunks_exact_mut(4)) {
                let mut sum = [0u32; 3];
                for sy in start..end {
                    let row = (sy * image.width) as usize * 4;
                    for sx in x0..x1 {
                        let p = &image.rgba[row + sx as usize * 4..][..4];
                        // Transparence de l'image : posée sur du noir.
                        let alpha = p[3] as u32;
                        for (total, channel) in sum.iter_mut().zip(p) {
                            *total += *channel as u32 * alpha / 255;
                        }
                    }
                }
                let count = (end - start) * (x1 - x0);
                let [r, g, b] = sum.map(|total| (total / count) as u8);
                out.copy_from_slice(&[r, g, b, 255]);
            }
        }
    });
    Image { width, height, rgba }
}

/// Flou de rayon `radius` pixels : trois passes de moyenne glissante, horizontale puis
/// verticale (proche d'un flou gaussien). Sert au verre dépoli des panneaux, posé sur
/// une petite image du fond d'écran.
pub fn blur(image: &Image, radius: u32) -> Image {
    let (width, height) = (image.width as usize, image.height as usize);
    let mut pixels: Vec<[f32; 4]> = image
        .rgba
        .chunks_exact(4)
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32])
        .collect();
    let radius = radius as isize;
    let pass = |pixels: &mut Vec<[f32; 4]>, horizontal: bool| {
        let (lines, length) = if horizontal {
            (height, width)
        } else {
            (width, height)
        };
        let at = |line: usize, i: isize| {
            let i = i.clamp(0, length as isize - 1) as usize;
            if horizontal {
                line * width + i
            } else {
                i * width + line
            }
        };
        let source = pixels.clone();
        let count = (2 * radius + 1) as f32;
        for line in 0..lines {
            let mut sum = [0.0f32; 4];
            for i in -radius..=radius {
                for (total, value) in sum.iter_mut().zip(source[at(line, i)]) {
                    *total += value;
                }
            }
            for i in 0..length as isize {
                pixels[at(line, i)] = sum.map(|total| total / count);
                let (outgoing, incoming) = (source[at(line, i - radius)], source[at(line, i + radius + 1)]);
                for channel in 0..4 {
                    sum[channel] += incoming[channel] - outgoing[channel];
                }
            }
        }
    };
    for _ in 0..3 {
        pass(&mut pixels, true);
        pass(&mut pixels, false);
    }
    let rgba = pixels
        .into_iter()
        .flat_map(|pixel| pixel.map(|value| value.round().clamp(0.0, 255.0) as u8))
        .collect();
    Image {
        width: image.width,
        height: image.height,
        rgba,
    }
}

/// Fond prêt à poser, à `width` × `height` pixels ; une image illisible donne le fond
/// par défaut, et l'erreur.
pub fn render(source: &Source, width: u32, height: u32) -> (Image, Option<String>) {
    match source {
        Source::Builtin(builtin) => (paint(*builtin, width, height), None),
        Source::File(path) => match decode(path) {
            Ok(image) => (cover(&image, width, height), None),
            Err(error) => (paint(Builtin::DEFAULT, width, height), Some(error)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fond_dessine_par_bandes_identique_a_une_seule_bande() {
        for scene in [
            Scene::desktop(Builtin::DEFAULT),
            Scene::desktop(Builtin::new(Mood::Lagoon, false)),
            Scene::lock(Mood::Fusion, true),
        ] {
            // Hauteur qui ne tombe pas juste : la dernière bande est plus courte.
            let alone = paint_scene(&scene, 401, 253, 1);
            for threads in [2, 7, 64] {
                assert!(
                    paint_scene(&scene, 401, 253, threads) == alone,
                    "{threads} bandes"
                );
            }
        }
    }

    #[test]
    fn image_recadree_par_bandes_identique_a_une_seule_bande() {
        let (width, height) = (97u32, 61u32);
        let rgba = (0..width * height)
            .flat_map(|i| [(i * 7) as u8, (i * 13) as u8, (i / 3) as u8, (i * 5 % 256) as u8])
            .collect();
        let image = Image { width, height, rgba };
        for (w, h) in [(50, 31), (300, 157), (1, 1)] {
            let alone = cover_with(&image, w, h, 1);
            for threads in [3, 8] {
                assert!(
                    cover_with(&image, w, h, threads) == alone,
                    "{w}×{h} en {threads} bandes"
                );
            }
        }
    }

    #[test]
    fn choix_du_fond_dans_les_reglages() {
        assert_eq!(Source::parse(""), Source::Builtin(Builtin::DEFAULT));
        assert_eq!(
            Source::parse("lagune-claire"),
            Source::Builtin(Builtin::new(Mood::Lagoon, false))
        );
        assert_eq!(
            Source::parse("/home/a/b.jpg"),
            Source::File("/home/a/b.jpg".into())
        );
        for builtin in Builtin::ALL {
            assert_eq!(
                Source::parse(&Source::Builtin(builtin).key()),
                Source::Builtin(builtin)
            );
        }
        assert_eq!(Builtin::new(Mood::Amethyst, false).label(), "Améthyste claire");
    }

    /// Couleur du pixel (x, y), sans le tramage (au plus un cran d'écart).
    fn near(image: &Image, x: u32, y: u32, expected: [u8; 3]) -> bool {
        let i = ((y * image.width + x) * 4) as usize;
        (0..3).all(|c| image.rgba[i + c].abs_diff(expected[c]) <= 1)
    }

    #[test]
    fn fusion_comme_sur_la_maquette() {
        let image = paint(Builtin::DEFAULT, 1440, 900);
        assert_eq!(image.rgba.len(), 1440 * 900 * 4);
        assert!(image.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
        // Coin haut gauche : la base #1C1016, hors des lueurs.
        assert!(near(&image, 0, 0, [0x1C, 0x10, 0x16]));
        // Centre de la lueur orange (80 %, 20 %) : la base mêlée à moitié de #FF7A45,
        // puis du halo central, déjà presque éteint là.
        let i = ((180 * 1440 + 1152) * 4) as usize;
        let [r, g, b] = [image.rgba[i], image.rgba[i + 1], image.rgba[i + 2]];
        assert!(r > 0x80 && g > 0x38 && g < 0x50 && b < 0x40, "{r} {g} {b}");
        assert!(image.top_is_dark(20));
        assert!(!paint(Builtin::new(Mood::Fusion, false), 160, 100).top_is_dark(5));
    }

    #[test]
    fn anneaux_de_l_ecran_de_verrouillage() {
        let lock = paint_lock(Mood::Fusion, true, 1440, 900);
        let plain = paint_scene(
            &Scene {
                rings: Vec::new(),
                ..Scene::lock(Mood::Fusion, true)
            },
            1440,
            900,
            1,
        );
        // Dans le grand anneau (rayon 460 à 550, ici à gauche du centre) : plus orange.
        let i = ((450 * 1440 + 720 - 500) * 4) as usize;
        assert!(lock.rgba[i] > plain.rgba[i]);
        // Au centre, hors des anneaux : identique.
        let i = ((450 * 1440 + 720) * 4) as usize;
        assert_eq!(lock.rgba[i..i + 4], plain.rgba[i..i + 4]);
    }

    #[test]
    fn un_ancien_fond_revient_a_fusion() {
        assert_eq!(Source::parse("crepuscule"), Source::Builtin(Builtin::DEFAULT));
        assert_eq!(Source::parse("ardoise"), Source::Builtin(Builtin::DEFAULT));
        assert_eq!(
            Source::parse("clair"),
            Source::Builtin(Builtin::new(Mood::Fusion, false))
        );
    }

    #[test]
    fn le_flou_adoucit_un_bord_net() {
        // Moitié gauche noire, moitié droite blanche.
        let rgba = (0..40 * 4)
            .flat_map(|i| {
                let value = if i % 40 < 20 { 0 } else { 255 };
                [value, value, value, 255]
            })
            .collect();
        let sharp = Image {
            width: 40,
            height: 4,
            rgba,
        };
        let soft = blur(&sharp, 3);
        let at = |x: usize| soft.rgba[(40 + x) * 4];
        assert!(at(0) < 10 && at(39) > 245);
        assert!(at(19) > 60 && at(20) < 195, "{} {}", at(19), at(20));
        assert!(soft.rgba.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn une_image_couvre_l_ecran_sans_deformation() {
        // Image 4×2 : moitié gauche rouge, moitié droite bleue.
        let mut rgba = Vec::new();
        for _ in 0..2 {
            rgba.extend([255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255]);
        }
        let image = Image {
            width: 4,
            height: 2,
            rgba,
        };
        // Écran carré : on garde le centre, une colonne rouge et une bleue.
        let square = cover(&image, 2, 2);
        assert_eq!(&square.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&square.rgba[4..8], &[0, 0, 255, 255]);
        // Agrandissement : chaque moitié reste de sa couleur.
        let large = cover(&image, 8, 4);
        assert_eq!(&large.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&large.rgba[7 * 4..8 * 4], &[0, 0, 255, 255]);
        assert!(
            !Image {
                width: 1,
                height: 1,
                rgba: vec![255; 4]
            }
            .top_is_dark(1)
        );
    }
}
