//! Captures d'écran PNG : option `--capture` pour vérifier le rendu automatiquement, et
//! captures demandées au clavier (tout l'écran ou une zone), rangées dans le dossier Images.

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use smithay::utils::{Logical, Point, Rectangle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Screen,
    Window,
    Area,
}

/// Capture à faire à la prochaine image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub region: Option<Rectangle<i32, Logical>>,
    pub save: bool,
    pub clipboard: bool,
    pub cursor: bool,
}

/// Sélection de la zone à capturer : `start` est connu une fois le bouton enfoncé.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RegionSelect {
    pub start: Option<Point<f64, Logical>>,
}

/// Rectangle entre deux coins quelconques, en pixels entiers.
pub fn region_between(a: Point<f64, Logical>, b: Point<f64, Logical>) -> Rectangle<i32, Logical> {
    let (left, top) = (a.x.min(b.x).round() as i32, a.y.min(b.y).round() as i32);
    let (right, bottom) = (a.x.max(b.x).round() as i32, a.y.max(b.y).round() as i32);
    Rectangle::new((left, top).into(), (right - left, bottom - top).into())
}

/// Ce qu'on saisit d'une zone à ajuster : un de ses coins pour la redimensionner, ou son
/// intérieur pour la déplacer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionGrip {
    Move,
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

/// Zone sélectionnée, en cours d'ajustement avant la prise : on peut la déplacer ou la
/// redimensionner par les coins, puis valider (Entrée) ou annuler (Échap).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionAdjust {
    pub rect: Rectangle<i32, Logical>,
    /// Poignée tenue, et position du pointeur à l'appui.
    pub grip: Option<(RegionGrip, Point<f64, Logical>)>,
}

impl RegionAdjust {
    pub const fn new(rect: Rectangle<i32, Logical>) -> Self {
        Self { rect, grip: None }
    }
}

/// Taille des poignées de coin, en pixels logiques.
const GRIP: i32 = 14;

/// Poignée sous `pos` : un coin d'abord, puis l'intérieur (déplacement).
pub fn region_grip(rect: Rectangle<i32, Logical>, pos: Point<f64, Logical>) -> Option<RegionGrip> {
    let left = rect.loc.x;
    let top = rect.loc.y;
    let right = rect.loc.x + rect.size.w;
    let bottom = rect.loc.y + rect.size.h;
    for (corner, grip) in [
        ((left, top), RegionGrip::NorthWest),
        ((right, top), RegionGrip::NorthEast),
        ((left, bottom), RegionGrip::SouthWest),
        ((right, bottom), RegionGrip::SouthEast),
    ] {
        let zone = Rectangle::new(
            (corner.0 - GRIP / 2, corner.1 - GRIP / 2).into(),
            (GRIP, GRIP).into(),
        );
        if zone.to_f64().contains(pos) {
            return Some(grip);
        }
    }
    rect.to_f64().contains(pos).then_some(RegionGrip::Move)
}

/// Nouvelle zone après avoir tiré une poignée de `start` jusqu'à `current`, sans descendre
/// sous une taille minimale.
pub fn adjust_region(
    rect: Rectangle<i32, Logical>,
    grip: RegionGrip,
    start: Point<f64, Logical>,
    current: Point<f64, Logical>,
) -> Rectangle<i32, Logical> {
    const MIN: i32 = 4;
    let dx = current.x.round() as i32 - start.x.round() as i32;
    let dy = current.y.round() as i32 - start.y.round() as i32;
    let left = rect.loc.x;
    let top = rect.loc.y;
    let right = rect.loc.x + rect.size.w;
    let bottom = rect.loc.y + rect.size.h;
    match grip {
        RegionGrip::Move => Rectangle::new((left + dx, top + dy).into(), (right - left, bottom - top).into()),
        RegionGrip::NorthWest => Rectangle::new(
            ((left + dx).min(right - MIN), (top + dy).min(bottom - MIN)).into(),
            (
                right - (left + dx).min(right - MIN),
                bottom - (top + dy).min(bottom - MIN),
            )
                .into(),
        ),
        RegionGrip::NorthEast => Rectangle::new(
            (left, (top + dy).min(bottom - MIN)).into(),
            (
                (right + dx).max(left + MIN) - left,
                bottom - (top + dy).min(bottom - MIN),
            )
                .into(),
        ),
        RegionGrip::SouthWest => Rectangle::new(
            ((left + dx).min(right - MIN), top).into(),
            (
                right - (left + dx).min(right - MIN),
                (bottom + dy).max(top + MIN) - top,
            )
                .into(),
        ),
        RegionGrip::SouthEast => Rectangle::new(
            (left, top).into(),
            (
                (right + dx).max(left + MIN) - left,
                (bottom + dy).max(top + MIN) - top,
            )
                .into(),
        ),
    }
}

/// Découpe `(x, y, largeur, hauteur)` dans une image RVBA, de haut en bas et opaque (la
/// composition laisse parfois un alpha partiel, sans signification dans un fichier). La
/// zone est ramenée dans l'image ; renvoie les pixels, la largeur et la hauteur obtenues.
pub fn crop(
    rgba: &[u8],
    width: u32,
    height: u32,
    flip_y: bool,
    (x, y, w, h): (u32, u32, u32, u32),
) -> (Vec<u8>, u32, u32) {
    let (x, y) = (x.min(width), y.min(height));
    let (w, h) = (w.min(width - x), h.min(height - y));
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for line in y..y + h {
        let source = if flip_y { height - 1 - line } else { line } as usize;
        let start = (source * width as usize + x as usize) * 4;
        for pixel in rgba[start..start + w as usize * 4].chunks_exact(4) {
            out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
        }
    }
    (out, w, h)
}

/// Dossier Images de l'utilisateur (`XDG_PICTURES_DIR`, sinon `~/Images`).
pub fn pictures_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    let config = std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
    std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|text| parse_pictures_dir(&text, &home))
        .unwrap_or_else(|| home.join("Images"))
}

fn parse_pictures_dir(text: &str, home: &Path) -> Option<PathBuf> {
    let value = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("XDG_PICTURES_DIR="))?
        .trim()
        .trim_matches('"');
    match value.strip_prefix("$HOME") {
        Some(rest) => Some(home.join(rest.trim_start_matches('/'))),
        None => value.starts_with('/').then(|| PathBuf::from(value)),
    }
}

/// Chemin libre dans `dir` pour une capture prise à `stamp`.
fn free_path(dir: &Path, stamp: &str) -> PathBuf {
    let first = dir.join(format!("Capture du {stamp}.png"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|number| dir.join(format!("Capture du {stamp} ({number}).png")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

/// Encode en PNG sans bloquer le rendu.
pub fn capture_in_background(
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    save: bool,
    done: impl FnOnce(Result<(Option<PathBuf>, Vec<u8>), String>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let result = encode_png(width, height, &pixels).and_then(|png| {
            let path = if save {
                let stamp = crate::desktop::panels::file_timestamp(std::time::SystemTime::now());
                let dir = pictures_dir();
                let path = free_path(&dir, &stamp);
                std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
                std::fs::write(&path, &png).map_err(|err| format!("{}: {err}", path.display()))?;
                Some(path)
            } else {
                None
            };
            Ok((path, png))
        });
        done(result);
    });
}

pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let row = width as usize * 4;
    if rgba.len() < row * height as usize {
        return Err(format!(
            "tampon trop petit : {} octets pour {width}×{height}",
            rgba.len()
        ));
    }
    let mut data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut data, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
        writer
            .write_image_data(&rgba[..row * height as usize])
            .map_err(|err| err.to_string())?;
        writer.finish().map_err(|err| err.to_string())?;
    }
    Ok(data)
}

/// Écrit une image RVBA 8 bits en PNG. Si `flip_y`, les lignes sont prises de bas en haut
/// (tampons OpenGL, dont l'origine est en bas à gauche).
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8], flip_y: bool) -> Result<(), String> {
    let row = width as usize * 4;
    if rgba.len() < row * height as usize {
        return Err(format!(
            "tampon trop petit : {} octets pour {width}×{height}",
            rgba.len()
        ));
    }
    let mut data = Vec::with_capacity(row * height as usize);
    for y in 0..height as usize {
        let source = if flip_y { height as usize - 1 - y } else { y };
        data.extend_from_slice(&rgba[source * row..(source + 1) * row]);
    }

    let png = encode_png(width, height, &data)?;
    let mut file = BufWriter::new(File::create(path).map_err(|err| format!("{}: {err}", path.display()))?);
    file.write_all(&png)
        .map_err(|err| format!("{}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn decoupe_d_une_zone() {
        // Deux lignes de deux pixels, numérotés dans le rouge ; alpha partiel à corriger.
        let image = [1, 0, 0, 9, 2, 0, 0, 9, 3, 0, 0, 9, 4, 0, 0, 9];
        let (pixels, w, h) = crop(&image, 2, 2, false, (1, 0, 1, 2));
        assert_eq!((w, h), (1, 2));
        assert_eq!(pixels, [2, 0, 0, 255, 4, 0, 0, 255]);
        // Retournée : la première ligne vue est la dernière du tampon.
        let (pixels, ..) = crop(&image, 2, 2, true, (0, 0, 2, 1));
        assert_eq!(pixels, [3, 0, 0, 255, 4, 0, 0, 255]);
        // Une zone qui déborde est ramenée dans l'image.
        assert_eq!(crop(&image, 2, 2, false, (1, 1, 9, 9)).1, 1);
        assert_eq!(crop(&image, 2, 2, false, (5, 5, 1, 1)).1, 0);
    }

    #[test]
    fn zone_entre_deux_coins() {
        let region = region_between((30.0, 5.0).into(), (10.0, 25.0).into());
        assert_eq!(region, Rectangle::new((10, 5).into(), (20, 20).into()));
    }

    #[test]
    fn dossier_images() {
        let home = Path::new("/home/moi");
        assert_eq!(
            parse_pictures_dir("# x\nXDG_PICTURES_DIR=\"$HOME/Photos\"\n", home),
            Some(PathBuf::from("/home/moi/Photos"))
        );
        assert_eq!(
            parse_pictures_dir("XDG_PICTURES_DIR=\"/data/img\"", home),
            Some(PathBuf::from("/data/img"))
        );
        assert_eq!(parse_pictures_dir("XDG_MUSIC_DIR=\"$HOME/M\"", home), None);
    }

    #[test]
    fn png_retourne() {
        let dir = std::env::temp_dir().join(format!("ferrite-capture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.png");
        // Deux lignes d'un pixel : rouge puis bleu.
        let pixels = [255, 0, 0, 255, 0, 0, 255, 255];
        write_png(&path, 1, 2, &pixels, true).unwrap();

        let decoder = png::Decoder::new(std::io::BufReader::new(File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut out = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut out).unwrap();
        assert_eq!(out, [0, 0, 255, 255, 255, 0, 0, 255]);
        std::fs::remove_dir_all(dir).unwrap();

        assert!(write_png(Path::new("x.png"), 2, 2, &pixels, false).is_err());
    }

    #[test]
    fn poignees_d_une_zone() {
        let rect = Rectangle::new((100, 50).into(), (200, 100).into());
        let point = |x: f64, y: f64| Point::from((x, y));
        assert_eq!(region_grip(rect, point(100.0, 50.0)), Some(RegionGrip::NorthWest));
        assert_eq!(region_grip(rect, point(300.0, 50.0)), Some(RegionGrip::NorthEast));
        assert_eq!(
            region_grip(rect, point(100.0, 150.0)),
            Some(RegionGrip::SouthWest)
        );
        assert_eq!(
            region_grip(rect, point(300.0, 150.0)),
            Some(RegionGrip::SouthEast)
        );
        assert_eq!(region_grip(rect, point(200.0, 100.0)), Some(RegionGrip::Move));
        assert_eq!(region_grip(rect, point(400.0, 200.0)), None);
    }

    #[test]
    fn deplacement_et_redimensionnement_d_une_zone() {
        let rect = Rectangle::new((100, 50).into(), (200, 100).into());
        let point = |x: f64, y: f64| Point::from((x, y));
        // Déplacement de 30 px vers la droite et 10 vers le bas.
        let moved = adjust_region(rect, RegionGrip::Move, point(100.0, 50.0), point(130.0, 60.0));
        assert_eq!(moved, Rectangle::new((130, 60).into(), (200, 100).into()));
        // Coin sud-est tiré de 20 px à droite et 30 px vers le bas.
        let grown = adjust_region(
            rect,
            RegionGrip::SouthEast,
            point(300.0, 150.0),
            point(320.0, 180.0),
        );
        assert_eq!(grown, Rectangle::new((100, 50).into(), (220, 130).into()));
        // Le coin nord-ouest ne peut pas dépasser la taille minimale de 4 px.
        let clamped = adjust_region(
            rect,
            RegionGrip::NorthWest,
            point(100.0, 50.0),
            point(500.0, 500.0),
        );
        assert_eq!(clamped, Rectangle::new((296, 146).into(), (4, 4).into()));
    }
}
