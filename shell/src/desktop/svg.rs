//! Icônes SVG de Ferrite (dossier `icons/SVG`), rasterisées avec resvg.
//!
//! Les icônes d'applications sont dessinées en couleur. Les icônes d'état, conçues en
//! couleur, sont aussi disponibles en version « symbolique » monochrome pour la barre
//! du haut et les menus : leurs tracés prennent une seule teinte, et les parties claires
//! et désaturées (barres Wi-Fi éteintes, rayons inactifs…) restent visibles en estompé.

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use ferrite_ui::theme::Color;
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg::{Options, Tree},
};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        /// Icônes embarquées dans le binaire : (nom, source SVG).
        const SOURCES: &[(&str, &str)] = &[
            $(($name, include_str!(concat!("../../../icons/SVG/", $name, ".svg")))),*
        ];
    };
}

icons![
    "app-calculator",
    "app-files",
    "app-search",
    "app-settings",
    "app-terminal",
    "app-text-editor",
    "file-generic",
    "status-battery-000",
    "status-battery-010",
    "status-battery-020",
    "status-battery-030",
    "status-battery-040",
    "status-battery-050",
    "status-battery-060",
    "status-battery-070",
    "status-battery-080",
    "status-battery-090",
    "status-battery-100",
    "status-battery-charging-000",
    "status-battery-charging-010",
    "status-battery-charging-020",
    "status-battery-charging-030",
    "status-battery-charging-040",
    "status-battery-charging-050",
    "status-battery-charging-060",
    "status-battery-charging-070",
    "status-battery-charging-080",
    "status-battery-charging-090",
    "status-battery-charging-100",
    "status-bluetooth-off",
    "status-bluetooth-on",
    "status-brightness-0",
    "status-brightness-1",
    "status-brightness-2",
    "status-brightness-3",
    "status-brightness-4",
    "status-brightness-5",
    "status-network",
    "status-power",
    "status-update",
    "status-volume-0",
    "status-volume-1",
    "status-volume-2",
    "status-volume-3",
    "status-volume-4",
    "status-volume-5",
    "status-wifi-0",
    "status-wifi-1",
    "status-wifi-2",
    "status-wifi-3",
    "status-wifi-4",
];

/// Rendu d'une icône : couleurs d'origine ou teinte unique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style {
    Color,
    Symbolic(Color),
}

/// Clé du cache : nom, côté en pixels et style.
type CacheKey = (&'static str, u32, Style);

/// Icônes déjà rasterisées.
#[derive(Debug, Default)]
pub struct IconCache {
    pixmaps: RefCell<HashMap<CacheKey, Option<Pixmap>>>,
    /// Icônes des applications installées, lues sur le disque.
    files: RefCell<HashMap<(PathBuf, u32), Option<Pixmap>>>,
}

impl IconCache {
    /// Dessine l'icône `name` dans un tampon BGRA prémultiplié (`Argb8888`), son coin
    /// haut-gauche en (`x`, `y`) et de côté `size`, en pixels physiques.
    #[allow(clippy::too_many_arguments)] // Tampon, nom, géométrie et style, sans état global.
    pub fn draw(
        &self,
        pixels: &mut [u8],
        width: u32,
        height: u32,
        name: &'static str,
        x: f32,
        y: f32,
        size: f32,
        style: Style,
    ) {
        let side = size.round().max(1.0) as u32;
        let mut pixmaps = self.pixmaps.borrow_mut();
        let pixmap = pixmaps
            .entry((name, side, style))
            .or_insert_with(|| rasterize(name, side, style));
        if let Some(pixmap) = pixmap {
            composite(pixels, width, height, pixmap, x.round() as i32, y.round() as i32);
        }
    }

    /// Comme [`IconCache::draw`], pour un fichier d'icône SVG ou PNG du système. Renvoie
    /// faux si le fichier est illisible (l'appelant dessine alors une icône par défaut).
    #[allow(clippy::too_many_arguments)] // Tampon, chemin et géométrie, sans état global.
    pub fn draw_file(
        &self,
        pixels: &mut [u8],
        width: u32,
        height: u32,
        path: &Path,
        x: f32,
        y: f32,
        size: f32,
    ) -> bool {
        let side = size.round().max(1.0) as u32;
        let mut files = self.files.borrow_mut();
        let pixmap = files
            .entry((path.to_path_buf(), side))
            .or_insert_with(|| rasterize_file(path, side));
        match pixmap {
            Some(pixmap) => {
                composite(pixels, width, height, pixmap, x.round() as i32, y.round() as i32);
                true
            }
            None => false,
        }
    }
}

/// Lit une icône du système (SVG ou PNG) et la met à `side` pixels de côté.
fn rasterize_file(path: &Path, side: u32) -> Option<Pixmap> {
    let data = fs::read(path).ok()?;
    let is_svg = path.extension().and_then(|ext| ext.to_str()) == Some("svg");
    let mut pixmap = Pixmap::new(side, side)?;
    if is_svg {
        let tree = Tree::from_data(&data, &Options::default()).ok()?;
        let view = tree.size();
        let factor = side as f32 / view.width().max(view.height());
        resvg::render(&tree, Transform::from_scale(factor, factor), &mut pixmap.as_mut());
        return Some(pixmap);
    }
    let (rgba, source_width, source_height) = decode_png(&data)?;
    // Réduction par moyenne de zone, en alpha prémultiplié (bords nets, sans halo).
    let target = pixmap.data_mut();
    for ty in 0..side {
        for tx in 0..side {
            let x0 = tx * source_width / side;
            let x1 = ((tx + 1) * source_width / side).max(x0 + 1).min(source_width);
            let y0 = ty * source_height / side;
            let y1 = ((ty + 1) * source_height / side).max(y0 + 1).min(source_height);
            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let offset = ((sy * source_width + sx) * 4) as usize;
                    let alpha = rgba[offset + 3] as u32;
                    for channel in 0..3 {
                        sum[channel] += rgba[offset + channel] as u32 * alpha / 255;
                    }
                    sum[3] += alpha;
                }
            }
            let count = (x1 - x0) * (y1 - y0);
            let offset = ((ty * side + tx) * 4) as usize;
            for channel in 0..4 {
                target[offset + channel] = (sum[channel] / count.max(1)) as u8;
            }
        }
    }
    Some(pixmap)
}

/// Décode un PNG en RVBA 8 bits (non prémultiplié).
fn decode_png(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let pixels = &buffer[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => pixels.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((rgba, info.width, info.height))
}

/// Rasterise l'icône `name` en un carré de `side` pixels.
fn rasterize(name: &str, side: u32, style: Style) -> Option<Pixmap> {
    let Some((_, source)) = SOURCES.iter().find(|(candidate, _)| *candidate == name) else {
        tracing::warn!(name, "icône inconnue");
        return None;
    };
    let source = match style {
        Style::Color => (*source).to_owned(),
        Style::Symbolic(ink) => ferrite_ui::icons::symbolic(source, ink),
    };
    let tree = Tree::from_str(&source, &Options::default())
        .inspect_err(|err| tracing::warn!(name, %err, "icône SVG illisible"))
        .ok()?;
    let mut pixmap = Pixmap::new(side, side)?;
    let view = tree.size();
    let factor = side as f32 / view.width().max(view.height());
    resvg::render(&tree, Transform::from_scale(factor, factor), &mut pixmap.as_mut());
    Some(pixmap)
}

/// Pose `pixmap` (RVBA prémultiplié) sur le tampon BGRA prémultiplié, par-dessus.
fn composite(pixels: &mut [u8], width: u32, height: u32, pixmap: &Pixmap, x: i32, y: i32) {
    let side_w = pixmap.width() as i32;
    let source = pixmap.data();
    for row in 0..pixmap.height() as i32 {
        let py = y + row;
        if py < 0 || py >= height as i32 {
            continue;
        }
        for column in 0..side_w {
            let px = x + column;
            if px < 0 || px >= width as i32 {
                continue;
            }
            let from = ((row * side_w + column) * 4) as usize;
            let alpha = source[from + 3] as u32;
            if alpha == 0 {
                continue;
            }
            let to = ((py as u32 * width + px as u32) * 4) as usize;
            let inverse = 255 - alpha;
            for (channel, offset) in [(2, 0), (1, 1), (0, 2), (3, 3)] {
                let below = pixels[to + offset] as u32;
                pixels[to + offset] = (source[from + channel] as u32 + below * inverse / 255).min(255) as u8;
            }
        }
    }
}

/// Nom de l'icône Wi-Fi : éteinte, ou pleine puissance quand la radio est active.
pub fn wifi_icon(enabled: bool) -> &'static str {
    if enabled { "status-wifi-4" } else { "status-wifi-0" }
}

/// Nom de l'icône du volume pour un niveau de 0 à 100.
pub fn volume_icon(level: u8) -> &'static str {
    const NAMES: [&str; 6] = [
        "status-volume-0",
        "status-volume-1",
        "status-volume-2",
        "status-volume-3",
        "status-volume-4",
        "status-volume-5",
    ];
    NAMES[step(level, 5)]
}

/// Nom de l'icône de luminosité pour un niveau de 0 à 100.
pub fn brightness_icon(level: u8) -> &'static str {
    const NAMES: [&str; 6] = [
        "status-brightness-0",
        "status-brightness-1",
        "status-brightness-2",
        "status-brightness-3",
        "status-brightness-4",
        "status-brightness-5",
    ];
    NAMES[step(level, 5)]
}

/// Nom de l'icône de batterie, arrondie à la dizaine.
pub fn battery_icon(percent: u8, charging: bool) -> &'static str {
    const NAMES: [&str; 11] = [
        "status-battery-000",
        "status-battery-010",
        "status-battery-020",
        "status-battery-030",
        "status-battery-040",
        "status-battery-050",
        "status-battery-060",
        "status-battery-070",
        "status-battery-080",
        "status-battery-090",
        "status-battery-100",
    ];
    const CHARGING: [&str; 11] = [
        "status-battery-charging-000",
        "status-battery-charging-010",
        "status-battery-charging-020",
        "status-battery-charging-030",
        "status-battery-charging-040",
        "status-battery-charging-050",
        "status-battery-charging-060",
        "status-battery-charging-070",
        "status-battery-charging-080",
        "status-battery-charging-090",
        "status-battery-charging-100",
    ];
    let index = ((percent.min(100) as usize) + 5) / 10;
    if charging { CHARGING[index] } else { NAMES[index] }
}

/// Palier de 0 à `steps` pour un niveau de 0 à 100 ; 0 est réservé au niveau nul.
fn step(level: u8, steps: usize) -> usize {
    if level == 0 {
        0
    } else {
        (level.min(100) as usize * steps).div_ceil(100).clamp(1, steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(name: &'static str, style: Style) -> Vec<u8> {
        let mut pixels = vec![0u8; 48 * 48 * 4];
        IconCache::default().draw(&mut pixels, 48, 48, name, 0.0, 0.0, 48.0, style);
        pixels
    }

    #[test]
    fn toutes_les_icones_embarquees_sont_lisibles() {
        // Une icône entièrement « éteinte » (Wi-Fi coupé) reste visible, estompée.
        for (name, _) in SOURCES {
            for style in [Style::Color, Style::Symbolic(Color::WHITE)] {
                let pixmap = rasterize(name, 32, style).unwrap_or_else(|| panic!("{name} illisible"));
                assert!(
                    pixmap.data().chunks(4).any(|pixel| pixel[3] > 60),
                    "{name} ({style:?}) est vide"
                );
            }
        }
    }

    #[test]
    fn une_icone_symbolique_n_a_qu_une_teinte() {
        let ink = Color::rgb(0x102030);
        let pixels = render("status-wifi-2", Style::Symbolic(ink));
        let mut alphas = Vec::new();
        for pixel in pixels.chunks(4).filter(|pixel| pixel[3] == 255) {
            assert_eq!(&pixel[..3], &[0x30, 0x20, 0x10], "BGRA attendu");
        }
        for pixel in pixels.chunks(4).filter(|pixel| pixel[3] > 0) {
            alphas.push(pixel[3]);
        }
        // Barres actives opaques et barre éteinte estompée.
        assert!(alphas.contains(&255));
        assert!(alphas.iter().any(|&alpha| (70..110).contains(&alpha)));
    }

    #[test]
    fn les_paliers_suivent_le_niveau() {
        assert_eq!(volume_icon(0), "status-volume-0");
        assert_eq!(volume_icon(1), "status-volume-1");
        assert_eq!(volume_icon(65), "status-volume-4");
        assert_eq!(volume_icon(100), "status-volume-5");
        assert_eq!(battery_icon(47, false), "status-battery-050");
        assert_eq!(battery_icon(3, true), "status-battery-charging-000");
        assert_eq!(wifi_icon(false), "status-wifi-0");
    }
}
