//! Fonds d'écran de Ferrite : ceux dessinés par Ferrite (dégradé, halo et collines
//! ondulées translucides, repris du bureau d'origine `D:\Os\kernel\src\gui.rs`,
//! `paint_wallpaper`), ou une image PNG ou JPEG de l'utilisateur, recadrée pour couvrir
//! l'écran. Partagé par le compositeur (fond réel) et Paramètres (vignettes).

use std::{
    f32::consts::TAU,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use crate::theme::{Color, WALLPAPER_GRADIENT, gradient_at};

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

/// Fonds dessinés par Ferrite : même relief de collines, couleurs différentes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Builtin {
    Ferrite,
    Sunset,
    Ocean,
    Forest,
    Graphite,
}

/// Couleurs d'un fond dessiné : dégradé (haut, milieu, bas), halo, puis les collines
/// de la plus lointaine à la plus proche.
struct Palette {
    gradient: [Color; 3],
    glow: Color,
    hills: [Color; 4],
}

impl Builtin {
    pub const ALL: [Builtin; 5] = [
        Builtin::Ferrite,
        Builtin::Sunset,
        Builtin::Ocean,
        Builtin::Forest,
        Builtin::Graphite,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Builtin::Ferrite => "ferrite",
            Builtin::Sunset => "crepuscule",
            Builtin::Ocean => "ocean",
            Builtin::Forest => "foret",
            Builtin::Graphite => "graphite",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Builtin::Ferrite => "Ferrite",
            Builtin::Sunset => "Crépuscule",
            Builtin::Ocean => "Océan",
            Builtin::Forest => "Forêt",
            Builtin::Graphite => "Graphite",
        }
    }

    fn palette(self) -> Palette {
        let rgb = Color::rgb;
        match self {
            Builtin::Ferrite => Palette {
                gradient: WALLPAPER_GRADIENT,
                glow: rgb(0xFFC7A8),
                hills: [rgb(0xFFB49C), rgb(0xE66A8A), rgb(0x4A2F86), rgb(0x24185A)],
            },
            Builtin::Sunset => Palette {
                gradient: [rgb(0x3A1C71), rgb(0xD76D77), rgb(0xFFAF7B)],
                glow: rgb(0xFFE0B5),
                hills: [rgb(0xFFC59E), rgb(0xF0886A), rgb(0x8A3B6E), rgb(0x3A1C71)],
            },
            Builtin::Ocean => Palette {
                gradient: [rgb(0x0B2545), rgb(0x13678A), rgb(0x7FD1C7)],
                glow: rgb(0xD9F7F2),
                hills: [rgb(0x9BE3D9), rgb(0x2E9CCA), rgb(0x134074), rgb(0x0B2545)],
            },
            Builtin::Forest => Palette {
                gradient: [rgb(0x0F2A1D), rgb(0x2F6B3F), rgb(0xC9D88B)],
                glow: rgb(0xFFF2C2),
                hills: [rgb(0xB5D99C), rgb(0x5E9C5A), rgb(0x1E4D2B), rgb(0x0C2116)],
            },
            Builtin::Graphite => Palette {
                gradient: [rgb(0x1C1C1E), rgb(0x3A3A40), rgb(0x8E8E99)],
                glow: rgb(0xD0D0D8),
                hills: [rgb(0xA0A0AA), rgb(0x6A6A74), rgb(0x2E2E34), rgb(0x141416)],
            },
        }
    }
}

/// Relief d'une colline : opacité (0 à 1), hauteur de base, amplitude (fractions de la
/// hauteur de l'écran), nombre d'ondulations sur la largeur, et décalage (en 60es de tour).
struct Wave {
    opacity: f32,
    base: f32,
    amplitude: f32,
    frequency: f32,
    phase: f32,
}

const WAVES: [Wave; 4] = [
    Wave {
        opacity: 70.0 / 255.0,
        base: 0.58,
        amplitude: 0.06,
        frequency: 1.0,
        phase: 0.0,
    },
    Wave {
        opacity: 90.0 / 255.0,
        base: 0.69,
        amplitude: 0.045,
        frequency: 2.0,
        phase: 20.0,
    },
    Wave {
        opacity: 120.0 / 255.0,
        base: 0.82,
        amplitude: 0.03,
        frequency: 1.0,
        phase: 40.0,
    },
    Wave {
        opacity: 140.0 / 255.0,
        base: 0.92,
        amplitude: 0.02,
        frequency: 3.0,
        phase: 10.0,
    },
];

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

/// Fond dessiné à `width` × `height` pixels. Le bord de chaque colline est anticrénelé :
/// un pixel qu'il traverse n'en reçoit que la part qu'il couvre.
pub fn waves(builtin: Builtin, width: u32, height: u32) -> Image {
    let palette = builtin.palette();
    let (width, height) = (width.max(1), height.max(1));
    let (w, h) = (width as f32, height as f32);
    // Bord de chaque colline, colonne par colonne (en pixels depuis le haut).
    let edges: Vec<Vec<f32>> = WAVES
        .iter()
        .map(|wave| {
            (0..width)
                .map(|x| {
                    let turns = x as f32 * wave.frequency / w + wave.phase / 60.0;
                    (wave.base + wave.amplitude * (turns * TAU).sin()) * h
                })
                .collect()
        })
        .collect();
    let (glow_x, glow_y, glow_r) = (w * 0.72, h * 0.18, h * 0.60);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let base = gradient_at(&palette.gradient, y as f32 / (h - 1.0).max(1.0));
        for x in 0..width {
            let mut color = [base.r, base.g, base.b].map(f32::from);
            let (dx, dy) = (x as f32 - glow_x, y as f32 - glow_y);
            let d2 = (dx * dx + dy * dy) / (glow_r * glow_r);
            if d2 < 1.0 {
                let a = 60.0 * (1.0 - d2);
                blend(&mut color, palette.glow, a * a / 60.0 / 255.0);
            }
            for ((wave, edge), hill) in WAVES.iter().zip(&edges).zip(palette.hills) {
                let coverage = (y as f32 + 1.0 - edge[x as usize]).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    blend(&mut color, hill, wave.opacity * coverage);
                }
            }
            let [r, g, b] = color.map(|channel| channel.round().clamp(0.0, 255.0) as u8);
            rgba.extend([r, g, b, 255]);
        }
    }
    Image { width, height, rgba }
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
        Source::Builtin(builtin) => (waves(*builtin, width, height), None),
        Source::File(path) => match decode(path) {
            Ok(image) => (cover(&image, width, height), None),
            Err(error) => (waves(Builtin::Ferrite, width, height), Some(error)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choix_du_fond_dans_les_reglages() {
        assert_eq!(Source::parse(""), Source::Builtin(Builtin::Ferrite));
        assert_eq!(Source::parse("ocean"), Source::Builtin(Builtin::Ocean));
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
    fn degrade_puis_collines() {
        let image = waves(Builtin::Ferrite, 400, 300);
        assert_eq!(image.rgba.len(), 400 * 300 * 4);
        assert!(image.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
        let at = |x: usize, y: usize| {
            let i = (y * 400 + x) * 4;
            [image.rgba[i], image.rgba[i + 1], image.rgba[i + 2]]
        };
        // En haut à gauche : le bleu nuit du dégradé, loin du halo.
        let top = WALLPAPER_GRADIENT[0];
        let [r, g, b] = at(0, 0);
        assert!(r.abs_diff(top.r) < 6 && g.abs_diff(top.g) < 6 && b.abs_diff(top.b) < 6);
        // En bas : la dernière colline, sombre, recouvre le corail du dégradé.
        let [r, _, b] = at(200, 299);
        assert!(r < 0xC0 && b > 0x50);
        assert!(image.top_is_dark(20));
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
