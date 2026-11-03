//! Fonds d'écran de Ferrite : ceux dessinés par Ferrite (couleur unie et hexagones au
//! trait fin, comme sur les planches du thème), ou une image PNG ou JPEG de l'utilisateur,
//! recadrée pour couvrir l'écran. Partagé par le compositeur (fond réel) et Paramètres (vignettes).

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
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
        Source::Builtin(Builtin::Ferrite)
    }
}

impl Source {
    pub fn parse(value: &str) -> Source {
        let value = value.trim();
        if value.starts_with('/') {
            return Source::File(PathBuf::from(value));
        }
        Builtin::ALL
            .into_iter()
            .find(|builtin| builtin.key() == value)
            .map(Source::Builtin)
            .unwrap_or_default()
    }

    pub fn key(&self) -> String {
        match self {
            Source::Builtin(builtin) => builtin.key().to_owned(),
            Source::File(path) => path.display().to_string(),
        }
    }
}

/// Fonds dessinés par Ferrite : mêmes hexagones, couleurs différentes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Builtin {
    Ferrite,
    Night,
    Slate,
    Light,
}

impl Builtin {
    pub const ALL: [Builtin; 4] = [Builtin::Ferrite, Builtin::Night, Builtin::Slate, Builtin::Light];

    pub fn key(self) -> &'static str {
        match self {
            Builtin::Ferrite => "ferrite",
            Builtin::Night => "nuit",
            Builtin::Slate => "ardoise",
            Builtin::Light => "clair",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Builtin::Ferrite => "Ferrite",
            Builtin::Night => "Nuit",
            Builtin::Slate => "Ardoise",
            Builtin::Light => "Clair",
        }
    }

    /// Couleur du fond et couleur du trait du cube.
    pub fn colors(self) -> (Color, Color) {
        match self {
            Builtin::Ferrite => (Color::rgb(0x16202D), Color::rgb(0x6CB8FF)),
            Builtin::Night => (Color::rgb(0x1B2433), Color::rgb(0x9A6BFF)),
            Builtin::Slate => (Color::rgb(0x2A3340), Color::rgb(0x2FD49A)),
            Builtin::Light => (Color::rgb(0xE7E9EE), Color::rgb(0x2779E0)),
        }
    }
}

type Segment = ((f32, f32), (f32, f32));

/// Contour d'un hexagone, à partir de ses sommets.
fn outline<const N: usize>(points: [(f32, f32); N]) -> [Segment; N] {
    std::array::from_fn(|index| (points[index], points[(index + 1) % N]))
}

/// Traits blancs du fond sur l'écran de référence de 1600 × 1000 : un grand hexagone à
/// droite et deux petits en bas (opacité, segments).
fn white_outlines() -> [(f32, Vec<Segment>); 3] {
    [
        (
            0.06,
            outline([
                (1180.0, 120.0),
                (1420.0, 258.0),
                (1420.0, 534.0),
                (1180.0, 672.0),
                (940.0, 534.0),
                (940.0, 258.0),
            ])
            .to_vec(),
        ),
        (
            0.05,
            outline([
                (300.0, 560.0),
                (470.0, 658.0),
                (470.0, 854.0),
                (300.0, 952.0),
                (130.0, 854.0),
                (130.0, 658.0),
            ])
            .to_vec(),
        ),
        (
            0.05,
            outline([
                (1460.0, 700.0),
                (1560.0, 758.0),
                (1560.0, 874.0),
                (1460.0, 932.0),
                (1360.0, 874.0),
                (1360.0, 758.0),
            ])
            .to_vec(),
        ),
    ]
}

/// Arêtes intérieures du grand hexagone, qui en font un cube, dans la couleur du fond.
const CUBE_EDGES: [Segment; 3] = [
    ((1180.0, 120.0), (1180.0, 396.0)),
    ((1180.0, 396.0), (1420.0, 534.0)),
    ((1180.0, 396.0), (940.0, 534.0)),
];
const CUBE_OPACITY: f32 = 0.16;

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

/// Fond dessiné à `width` × `height` pixels : les hexagones gardent leurs proportions et
/// couvrent l'écran, comme une image recadrée.
pub fn paint(builtin: Builtin, width: u32, height: u32) -> Image {
    let (width, height) = (width.max(1), height.max(1));
    let (background, stroke) = builtin.colors();
    let mut pixels = vec![
        [
            f32::from(background.r),
            f32::from(background.g),
            f32::from(background.b)
        ];
        (width * height) as usize
    ];
    let scale = (width as f32 / 1600.0).max(height as f32 / 1000.0);
    let origin = (
        (width as f32 - 1600.0 * scale) / 2.0,
        (height as f32 - 1000.0 * scale) / 2.0,
    );
    let place = |(x, y): (f32, f32)| (origin.0 + x * scale, origin.1 + y * scale);
    // Sur le fond clair, le blanc ne se verrait pas : tout prend la couleur du trait.
    let white = if builtin == Builtin::Light {
        stroke
    } else {
        Color::WHITE
    };
    let half_width = scale.max(0.5);
    for (opacity, segments) in white_outlines() {
        for (start, end) in segments {
            stroke_segment(
                &mut pixels,
                width,
                height,
                place(start),
                place(end),
                half_width,
                white,
                opacity,
            );
        }
    }
    for (start, end) in CUBE_EDGES {
        stroke_segment(
            &mut pixels,
            width,
            height,
            place(start),
            place(end),
            half_width,
            stroke,
            CUBE_OPACITY,
        );
    }
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        let [r, g, b] = pixel.map(|channel| channel.round().clamp(0.0, 255.0) as u8);
        rgba.extend([r, g, b, 255]);
    }
    Image { width, height, rgba }
}

/// Trait anticrénelé : seuls les pixels autour du segment sont parcourus.
#[allow(clippy::too_many_arguments)] // Image, segment et apparence.
fn stroke_segment(
    pixels: &mut [[f32; 3]],
    width: u32,
    height: u32,
    a: (f32, f32),
    b: (f32, f32),
    half_width: f32,
    color: Color,
    opacity: f32,
) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_squared = (dx * dx + dy * dy).max(f32::EPSILON);
    let steps = dx.abs().max(dy.abs()).ceil() as i32;
    let reach = half_width.ceil() as i32 + 1;
    // Le long de l'axe principal, chaque rangée ne touche que quelques pixels.
    let mut last = None;
    for step in 0..=steps {
        let t = step as f32 / steps.max(1) as f32;
        let (cx, cy) = ((a.0 + dx * t).floor() as i32, (a.1 + dy * t).floor() as i32);
        for y in (cy - reach)..=(cy + reach) {
            for x in (cx - reach)..=(cx + reach) {
                if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                    continue;
                }
                let index = y as usize * width as usize + x as usize;
                let visited = last
                    .is_some_and(|(px, py): (i32, i32)| (x - px).abs() <= reach && (y - py).abs() <= reach);
                if visited {
                    continue;
                }
                let (qx, qy) = (x as f32 + 0.5 - a.0, y as f32 + 0.5 - a.1);
                let along = ((qx * dx + qy * dy) / length_squared).clamp(0.0, 1.0);
                let distance = (qx - dx * along).hypot(qy - dy * along);
                let coverage = (half_width + 0.5 - distance).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    blend(&mut pixels[index], color, opacity * coverage);
                }
            }
        }
        last = Some((cx, cy));
    }
}

fn blend(color: &mut [f32; 3], over: Color, amount: f32) {
    for (channel, value) in color.iter_mut().zip([over.r, over.g, over.b]) {
        *channel += (f32::from(value) - *channel) * amount;
    }
}

/// Lit une image PNG ou JPEG (reconnue à son contenu, pas à son extension).
pub fn decode(path: &Path) -> Result<Image, String> {
    let data = fs::read(path).map_err(|error| format!("{} : {error}", path.display()))?;
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
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let from = top + y as f64 / ratio;
        let start = (from.floor() as u32).min(image.height - 1);
        let end = ((from + 1.0 / ratio).ceil() as u32).clamp(start + 1, image.height);
        for &(x0, x1) in &columns {
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
            rgba.extend(sum.map(|total| (total / count) as u8));
            rgba.push(255);
        }
    }
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
/// Ferrite, et l'erreur.
pub fn render(source: &Source, width: u32, height: u32) -> (Image, Option<String>) {
    match source {
        Source::Builtin(builtin) => (paint(*builtin, width, height), None),
        Source::File(path) => match decode(path) {
            Ok(image) => (cover(&image, width, height), None),
            Err(error) => (paint(Builtin::Ferrite, width, height), Some(error)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choix_du_fond_dans_les_reglages() {
        assert_eq!(Source::parse(""), Source::Builtin(Builtin::Ferrite));
        assert_eq!(Source::parse("ardoise"), Source::Builtin(Builtin::Slate));
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
    }

    #[test]
    fn fond_uni_et_hexagones() {
        let image = paint(Builtin::Ferrite, 1600, 1000);
        assert_eq!(image.rgba.len(), 1600 * 1000 * 4);
        assert!(image.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
        let at = |x: usize, y: usize| {
            let i = (y * 1600 + x) * 4;
            [image.rgba[i], image.rgba[i + 1], image.rgba[i + 2]]
        };
        // Loin des traits : la couleur du fond, #16202D.
        assert_eq!(at(10, 10), [0x16, 0x20, 0x2D]);
        // Sur le côté du grand hexagone : un peu plus clair.
        let side = at(1420, 400);
        assert!(side[0] > 0x16 && side[0] < 0x40);
        assert!(image.top_is_dark(20));
        assert!(!paint(Builtin::Light, 160, 100).top_is_dark(5));
    }

    #[test]
    fn un_ancien_fond_revient_au_fond_ferrite() {
        assert_eq!(Source::parse("crepuscule"), Source::Builtin(Builtin::Ferrite));
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
