//! Icônes des fichiers, d'après la planche « Fichiers » : une page blanche au coin plié,
//! un motif selon la famille (fermeture éclair des archives, lecture des vidéos…) et une
//! étiquette de couleur où l'extension est écrite (ZIP, RAR, 7Z, DEB, PDF…), pour
//! reconnaître chaque type d'un coup d'œil. Les dossiers aussi sont dessinés : une
//! chemise de couleur, une feuille qui dépasse et un emblème gravé (maison, document,
//! téléchargement…). Couleur et emblème se changent dans le panneau d'aperçu.

use std::path::Path;

use eframe::egui::{self, Color32, FontId, Pos2, Rect, Shape, Stroke, vec2};

use crate::style;

/// Dessin posé sur la page, selon la famille du fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Motif {
    /// Lignes de texte grises.
    Lines,
    /// Fermeture éclair : archives et paquets.
    Zipper,
    /// Montagne et soleil.
    Picture,
    /// Note de musique.
    Note,
    /// Triangle de lecture.
    Play,
    /// Chevrons « < > » : code source.
    Code,
    /// Invite « >_ » : scripts et programmes.
    Prompt,
    /// Disque optique : images ISO.
    Disc,
}

/// Ce qu'on dessine pour un fichier : texte de l'étiquette, sa couleur, le motif.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Badge {
    pub label: String,
    pub color: Color32,
    pub motif: Motif,
}

const PDF: Color32 = Color32::from_rgb(0xE0, 0x31, 0x31);
const WORD: Color32 = Color32::from_rgb(0x2F, 0x6F, 0xDB);
const SHEET: Color32 = Color32::from_rgb(0x2F, 0x9E, 0x44);
const SLIDES: Color32 = Color32::from_rgb(0xE8, 0x59, 0x0C);
const TEXT: Color32 = Color32::from_rgb(0x5C, 0x63, 0x70);
const MARKDOWN: Color32 = Color32::from_rgb(0x3B, 0x5B, 0xDB);
const CODE: Color32 = Color32::from_rgb(0x0C, 0x85, 0x99);
const DATA: Color32 = Color32::from_rgb(0xB3, 0x5C, 0x00);
const SHELL: Color32 = Color32::from_rgb(0x34, 0x3A, 0x40);
const IMAGE: Color32 = Color32::from_rgb(0xC2, 0x25, 0x5C);
const AUDIO: Color32 = Color32::from_rgb(0x70, 0x48, 0xE8);
const VIDEO: Color32 = Color32::from_rgb(0xD9, 0x48, 0x0F);
const ZIP: Color32 = Color32::from_rgb(0xE6, 0x77, 0x00);
const RAR: Color32 = Color32::from_rgb(0x86, 0x2E, 0x9C);
const SEVEN_Z: Color32 = Color32::from_rgb(0x18, 0x64, 0xAB);
const TAR: Color32 = Color32::from_rgb(0x8D, 0x55, 0x24);
const DEB: Color32 = Color32::from_rgb(0xA6, 0x1E, 0x4D);
const RPM: Color32 = Color32::from_rgb(0xC9, 0x2A, 0x2A);
const DISC: Color32 = Color32::from_rgb(0x49, 0x50, 0x57);
const PROGRAM: Color32 = Color32::from_rgb(0x21, 0x25, 0x29);
const FONT: Color32 = Color32::from_rgb(0x08, 0x7F, 0x5B);

/// Page, pli et lignes, en clair et en sombre.
fn paper() -> (Color32, Color32, Color32, Color32) {
    if style::theme_is_dark() {
        (
            Color32::from_rgb(0xEE, 0xF0, 0xF3),
            Color32::from_rgb(0xC9, 0xCD, 0xD4),
            Color32::from_rgb(0xD3, 0xD7, 0xDD),
            Color32::from_rgba_unmultiplied(0, 0, 0, 90),
        )
    } else {
        (
            Color32::WHITE,
            Color32::from_rgb(0xE3, 0xE5, 0xEA),
            Color32::from_rgb(0xE3, 0xE5, 0xEA),
            Color32::from_rgba_unmultiplied(20, 24, 32, 56),
        )
    }
}

/// Extension à écrire sur l'étiquette : `tar.gz` et ses semblables gardent leurs deux
/// parties ; rien pour un fichier sans extension.
fn extension_label(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    for double in ["tar.gz", "tar.xz", "tar.bz2", "tar.zst"] {
        if name.len() > double.len() + 1 && name.ends_with(&format!(".{double}")) {
            return Some(double.to_ascii_uppercase());
        }
    }
    let extension = path.extension()?.to_str()?;
    if extension.is_empty() || name.starts_with('.') && name[1..].find('.').is_none() {
        return None;
    }
    let label: String = extension.to_ascii_uppercase().chars().take(8).collect();
    Some(label)
}

/// Étiquette et motif d'un fichier ; `None` pour un fichier sans extension (page seule).
pub(crate) fn badge(path: &Path) -> Option<Badge> {
    let label = extension_label(path)?;
    let extension = label.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    let (color, motif) = match extension.as_str() {
        "pdf" => (PDF, Motif::Lines),
        "doc" | "docx" | "odt" | "rtf" => (WORD, Motif::Lines),
        "xls" | "xlsx" | "ods" | "csv" | "tsv" => (SHEET, Motif::Lines),
        "ppt" | "pptx" | "odp" | "key" => (SLIDES, Motif::Lines),
        "md" | "markdown" => (MARKDOWN, Motif::Lines),
        "txt" | "log" | "nfo" => (TEXT, Motif::Lines),
        "json" | "jsonc" | "yaml" | "yml" | "toml" | "ini" | "conf" | "cfg" | "xml" | "lock" => {
            (DATA, Motif::Lines)
        }
        "rs" | "c" | "h" | "cpp" | "hpp" | "cc" | "py" | "js" | "mjs" | "ts" | "tsx" | "jsx" | "html"
        | "htm" | "css" | "scss" | "java" | "kt" | "go" | "rb" | "php" | "lua" | "swift" | "cs" | "vue"
        | "svelte" | "sql" | "tex" => (CODE, Motif::Code),
        "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" => (SHELL, Motif::Prompt),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "tif" | "tiff" | "heic" | "avif"
        | "ico" | "xcf" | "psd" => (IMAGE, Motif::Picture),
        "mp3" | "ogg" | "oga" | "flac" | "wav" | "m4a" | "opus" | "aac" | "wma" => (AUDIO, Motif::Note),
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "wmv" | "mpg" | "mpeg" => (VIDEO, Motif::Play),
        "zip" => (ZIP, Motif::Zipper),
        "rar" => (RAR, Motif::Zipper),
        "7z" => (SEVEN_Z, Motif::Zipper),
        "tar" | "gz" | "tgz" | "xz" | "bz2" | "zst" | "lz" | "lzma" | "cab" => (TAR, Motif::Zipper),
        "deb" => (DEB, Motif::Zipper),
        "rpm" => (RPM, Motif::Zipper),
        "flatpak" | "flatpakref" | "snap" | "jar" | "apk" => (TAR, Motif::Zipper),
        "iso" | "img" | "dmg" => (DISC, Motif::Disc),
        "appimage" | "exe" | "msi" | "run" | "bin" | "elf" | "out" => (PROGRAM, Motif::Prompt),
        "ttf" | "otf" | "woff" | "woff2" => (FONT, Motif::Lines),
        _ => (TEXT, Motif::Lines),
    };
    Some(Badge { label, color, motif })
}

/// Icône du fichier `path` dans le carré centré sur `rect`.
/// Icône du fichier `path` ; `color` remplace la couleur de l'étiquette choisie par
/// l'utilisateur.
pub(crate) fn paint(painter: &egui::Painter, rect: Rect, path: &Path, color: Option<Color32>) {
    let mut badge = badge(path);
    if let (Some(badge), Some(color)) = (badge.as_mut(), color) {
        badge.color = color;
    }
    paint_badge(painter, rect, badge.as_ref());
}

/// Page d'après la planche (repère de 96 sur 96), étiquette et motif par-dessus.
pub(crate) fn paint_badge(painter: &egui::Painter, rect: Rect, badge: Option<&Badge>) {
    let side = rect.width().min(rect.height());
    // La page (avec sa languette) occupe 64 sur 80 du repère : on la cadre sur 84 pour
    // qu'elle ait la même présence que les dossiers à côté.
    let unit = side / 84.0;
    let origin = rect.center() - vec2(44.0, 48.0) * unit;
    let at = |x: f32, y: f32| origin + vec2(x, y) * unit;
    let (page, fold, lines, edge) = paper();
    let detailed = side >= 30.0;

    // Contour : coin plié en haut à droite, trois coins arrondis.
    let mut outline = vec![at(20.0, 14.0)];
    arc(&mut outline, at(26.0, 14.0), 6.0 * unit, 180.0, 270.0);
    outline.push(at(56.0, 8.0));
    outline.push(at(76.0, 28.0));
    arc(&mut outline, at(70.0, 82.0), 6.0 * unit, 0.0, 90.0);
    arc(&mut outline, at(26.0, 82.0), 6.0 * unit, 90.0, 180.0);
    let shadow: Vec<Pos2> = outline
        .iter()
        .map(|p| *p + vec2(0.0, 1.5 * unit.max(0.5)))
        .collect();
    painter.add(Shape::convex_polygon(
        shadow,
        edge.gamma_multiply(0.5),
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        outline,
        page,
        Stroke::new((1.5 * unit).clamp(0.6, 1.5), edge),
    ));
    painter.add(Shape::convex_polygon(
        vec![at(56.0, 8.0), at(56.0, 22.0), at(62.0, 28.0), at(76.0, 28.0)],
        fold,
        Stroke::NONE,
    ));

    let Some(badge) = badge else {
        if detailed {
            paint_lines(painter, &at, unit, lines, 4);
        }
        return;
    };
    if detailed {
        paint_motif(painter, &at, unit, badge, lines);
    }

    // Étiquette : déborde à gauche de la page, comme une languette.
    let tag = Rect::from_min_max(at(12.0, 60.0), at(72.0, 80.0));
    painter.rect_filled(tag, 4.5 * unit, badge.color);
    if side < 26.0 {
        return;
    }
    let mut size = 13.0 * unit;
    let room = tag.width() - 6.0 * unit;
    let mut galley;
    loop {
        galley = painter.layout_no_wrap(
            badge.label.clone(),
            FontId::new(size, style::strong_family()),
            Color32::WHITE,
        );
        if galley.size().x <= room || size <= 5.0 {
            break;
        }
        size -= 0.5;
    }
    painter.galley(
        tag.center() - galley.size() / 2.0 + vec2(0.0, 0.5 * unit),
        galley,
        Color32::WHITE,
    );
}

/// Ajoute à `points` un arc de cercle (angles en degrés, sens des aiguilles d'une montre).
fn arc(points: &mut Vec<Pos2>, center: Pos2, radius: f32, from: f32, to: f32) {
    for step in 0..=4 {
        let angle = (from + (to - from) * step as f32 / 4.0).to_radians();
        points.push(center + vec2(angle.cos(), angle.sin()) * radius);
    }
}

fn paint_lines(
    painter: &egui::Painter,
    at: &impl Fn(f32, f32) -> Pos2,
    unit: f32,
    color: Color32,
    count: usize,
) {
    for (row, width) in [26.0, 40.0, 34.0, 38.0].into_iter().take(count).enumerate() {
        let top = 30.0 + row as f32 * 7.0;
        painter.rect_filled(
            Rect::from_min_max(at(28.0, top), at(28.0 + width, top + 3.0)),
            1.5 * unit,
            color,
        );
    }
}

fn paint_motif(
    painter: &egui::Painter,
    at: &impl Fn(f32, f32) -> Pos2,
    unit: f32,
    badge: &Badge,
    lines: Color32,
) {
    let ink = badge.color;
    let stroke = Stroke::new(3.2 * unit, ink);
    match badge.motif {
        Motif::Lines => paint_lines(painter, at, unit, lines, 3),
        Motif::Zipper => {
            // Dents alternées le long du bord du pli, puis la tirette.
            for tooth in 0..6 {
                let top = 8.0 + tooth as f32 * 6.0;
                let left = if tooth % 2 == 0 { 40.0 } else { 46.0 };
                painter.rect_filled(
                    Rect::from_min_max(at(left, top + 1.0), at(left + 6.0, top + 5.0)),
                    1.0 * unit,
                    ink.gamma_multiply(0.85),
                );
            }
            let pull = Rect::from_min_max(at(39.0, 45.0), at(53.0, 57.0));
            painter.rect_filled(pull, 3.0 * unit, ink);
            painter.rect_filled(
                Rect::from_min_max(at(43.5, 49.0), at(48.5, 53.0)),
                1.5 * unit,
                Color32::WHITE,
            );
        }
        Motif::Picture => {
            let frame = Rect::from_min_max(at(30.0, 26.0), at(66.0, 54.0));
            painter.rect_filled(frame, 3.0 * unit, ink.gamma_multiply(0.16));
            painter.circle_filled(at(57.0, 34.0), 4.0 * unit, ink.gamma_multiply(0.7));
            painter.add(Shape::convex_polygon(
                vec![at(31.0, 53.0), at(43.0, 37.0), at(55.0, 53.0)],
                ink,
                Stroke::NONE,
            ));
            painter.add(Shape::convex_polygon(
                vec![at(47.0, 53.0), at(56.0, 43.0), at(65.0, 53.0)],
                ink.gamma_multiply(0.75),
                Stroke::NONE,
            ));
        }
        Motif::Note => {
            painter.line_segment([at(55.0, 26.0), at(55.0, 47.0)], stroke);
            painter.line_segment([at(55.0, 26.0), at(64.0, 31.0)], stroke);
            painter.circle_filled(at(50.0, 48.0), 6.0 * unit, ink);
            painter.line_segment([at(37.0, 30.0), at(37.0, 51.0)], stroke);
            painter.line_segment([at(37.0, 30.0), at(55.0, 26.0)], stroke);
            painter.circle_filled(at(32.0, 52.0), 6.0 * unit, ink);
        }
        Motif::Play => {
            painter.circle_filled(at(48.0, 40.0), 15.0 * unit, ink.gamma_multiply(0.16));
            painter.add(Shape::convex_polygon(
                vec![at(43.0, 31.0), at(57.0, 40.0), at(43.0, 49.0)],
                ink,
                Stroke::NONE,
            ));
        }
        Motif::Code => {
            painter.add(Shape::line(
                vec![at(38.0, 30.0), at(29.0, 40.0), at(38.0, 50.0)],
                stroke,
            ));
            painter.add(Shape::line(
                vec![at(58.0, 30.0), at(67.0, 40.0), at(58.0, 50.0)],
                stroke,
            ));
            painter.line_segment([at(51.0, 28.0), at(45.0, 52.0)], stroke);
        }
        Motif::Prompt => {
            let screen = Rect::from_min_max(at(27.0, 26.0), at(69.0, 54.0));
            painter.rect_filled(screen, 4.0 * unit, ink);
            let light = Stroke::new(3.0 * unit, Color32::WHITE);
            painter.add(Shape::line(
                vec![at(34.0, 33.0), at(41.0, 40.0), at(34.0, 47.0)],
                light,
            ));
            painter.line_segment([at(45.0, 47.0), at(58.0, 47.0)], light);
        }
        Motif::Disc => {
            painter.circle_filled(at(48.0, 40.0), 16.0 * unit, ink.gamma_multiply(0.8));
            painter.circle_stroke(
                at(48.0, 40.0),
                10.0 * unit,
                Stroke::new(1.0 * unit, Color32::WHITE),
            );
            painter.circle_filled(at(48.0, 40.0), 4.0 * unit, Color32::WHITE);
        }
    }
}

// ----- Dossiers : couleur et emblème -----

/// Couleurs proposées pour un dossier ou l'étiquette d'un fichier : clé enregistrée, nom
/// affiché, teinte (celles de la planche).
pub(crate) const SWATCHES: [(&str, &str, Color32); 10] = [
    ("jaune", "Jaune", Color32::from_rgb(0xF2, 0xB0, 0x1E)),
    ("orange", "Orange", Color32::from_rgb(0xF7, 0x67, 0x07)),
    ("rouge", "Rouge", Color32::from_rgb(0xE0, 0x31, 0x31)),
    ("rose", "Rose", Color32::from_rgb(0xD6, 0x33, 0x6C)),
    ("violet", "Violet", Color32::from_rgb(0x70, 0x48, 0xE8)),
    ("bleu", "Bleu", Color32::from_rgb(0x2F, 0x6F, 0xDB)),
    ("sarcelle", "Sarcelle", Color32::from_rgb(0x0C, 0x85, 0x99)),
    ("vert", "Vert", Color32::from_rgb(0x2F, 0x9E, 0x44)),
    ("graphite", "Graphite", Color32::from_rgb(0x5C, 0x63, 0x70)),
    ("gris", "Gris", Color32::from_rgb(0xAD, 0xB5, 0xBD)),
];

/// Indice d'une couleur de `SWATCHES` par sa clé.
pub(crate) fn swatch(key: &str) -> Option<usize> {
    SWATCHES.iter().position(|(k, _, _)| *k == key)
}

/// Dessin gravé sur un dossier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Emblem {
    Aucun,
    Etoile,
    Coeur,
    Document,
    Image,
    Musique,
    Video,
    Code,
    Telechargement,
    Cadenas,
    Livre,
    Ecran,
    Personne,
    Nuage,
    Maison,
}

impl Emblem {
    pub(crate) const ALL: [Emblem; 15] = [
        Emblem::Aucun,
        Emblem::Etoile,
        Emblem::Coeur,
        Emblem::Document,
        Emblem::Image,
        Emblem::Musique,
        Emblem::Video,
        Emblem::Code,
        Emblem::Telechargement,
        Emblem::Cadenas,
        Emblem::Livre,
        Emblem::Ecran,
        Emblem::Personne,
        Emblem::Nuage,
        Emblem::Maison,
    ];

    /// Clé enregistrée dans le fichier de réglages.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Emblem::Aucun => "aucun",
            Emblem::Etoile => "etoile",
            Emblem::Coeur => "coeur",
            Emblem::Document => "document",
            Emblem::Image => "image",
            Emblem::Musique => "musique",
            Emblem::Video => "video",
            Emblem::Code => "code",
            Emblem::Telechargement => "telechargement",
            Emblem::Cadenas => "cadenas",
            Emblem::Livre => "livre",
            Emblem::Ecran => "ecran",
            Emblem::Personne => "personne",
            Emblem::Nuage => "nuage",
            Emblem::Maison => "maison",
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Emblem::Aucun => "Aucun",
            Emblem::Etoile => "Étoile",
            Emblem::Coeur => "Cœur",
            Emblem::Document => "Document",
            Emblem::Image => "Image",
            Emblem::Musique => "Musique",
            Emblem::Video => "Vidéo",
            Emblem::Code => "Code",
            Emblem::Telechargement => "Téléchargement",
            Emblem::Cadenas => "Cadenas",
            Emblem::Livre => "Livre",
            Emblem::Ecran => "Écran",
            Emblem::Personne => "Personne",
            Emblem::Nuage => "Nuage",
            Emblem::Maison => "Maison",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Emblem> {
        Emblem::ALL.into_iter().find(|emblem| emblem.key() == key)
    }
}

/// Couleur et emblème d'origine d'un dossier : les dossiers habituels du dossier perso
/// ont les leurs (comme sur la planche), les dossiers cachés sont gris.
pub(crate) fn folder_default(path: &Path, home: &Path) -> (usize, Emblem) {
    let index = |key| swatch(key).unwrap_or(5);
    if path == home {
        return (index("bleu"), Emblem::Maison);
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name.starts_with('.') {
        return (index("gris"), Emblem::Aucun);
    }
    if path.parent() != Some(home) {
        return (index("bleu"), Emblem::Aucun);
    }
    match name {
        "Bureau" | "Desktop" => (index("bleu"), Emblem::Ecran),
        "Documents" => (index("bleu"), Emblem::Document),
        "Téléchargements" | "Downloads" => (index("vert"), Emblem::Telechargement),
        "Images" | "Pictures" => (index("rose"), Emblem::Image),
        "Musique" | "Music" => (index("violet"), Emblem::Musique),
        "Vidéos" | "Videos" => (index("orange"), Emblem::Video),
        "Public" => (index("sarcelle"), Emblem::Personne),
        "Modèles" | "Templates" => (index("graphite"), Emblem::Document),
        _ => (index("bleu"), Emblem::Aucun),
    }
}

/// Dossier d'après la planche (repère de 96 sur 80, centré dans `rect`) : dos et onglet,
/// feuille qui dépasse, devant avec un reflet, emblème gravé.
pub(crate) fn paint_folder(painter: &egui::Painter, rect: Rect, color: Color32, emblem: Emblem) {
    let side = rect.width().min(rect.height());
    let unit = side / 96.0;
    let origin = rect.center() - vec2(48.0, 42.0) * unit;
    let at = |x: f32, y: f32| origin + vec2(x, y) * unit;
    let r = |v: f32| egui::CornerRadius::same((v * unit).round().clamp(1.0, 255.0) as u8);
    let box_at = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(at(x0, y0), at(x1, y1));
    let dark = Color32::from_rgba_unmultiplied(0, 0, 0, 56);

    // Ombre portée, dos, onglet.
    painter.rect_filled(
        box_at(6.0, 15.5, 90.0, 79.5),
        r(8.0),
        Color32::from_rgba_unmultiplied(0, 0, 0, 30),
    );
    painter.rect_filled(box_at(6.0, 6.0, 38.0, 24.0), r(8.0), color);
    painter.add(Shape::convex_polygon(
        vec![at(30.0, 6.0), at(34.0, 6.0), at(44.0, 16.0), at(30.0, 16.0)],
        color,
        Stroke::NONE,
    ));
    painter.rect_filled(box_at(6.0, 14.0, 90.0, 78.0), r(8.0), color);
    painter.rect_filled(box_at(6.0, 6.0, 38.0, 24.0), r(8.0), dark);
    painter.add(Shape::convex_polygon(
        vec![at(30.0, 6.0), at(34.0, 6.0), at(44.0, 16.0), at(30.0, 16.0)],
        dark,
        Stroke::NONE,
    ));
    painter.rect_filled(box_at(6.0, 14.0, 90.0, 78.0), r(8.0), dark);

    // Feuilles qui dépassent.
    painter.rect_filled(
        box_at(13.0, 13.0, 81.0, 55.0),
        r(3.0),
        Color32::from_rgb(0xDD, 0xE1, 0xE8),
    );
    painter.rect_filled(box_at(18.0, 19.0, 82.0, 61.0), r(3.0), Color32::WHITE);

    // Devant et son reflet.
    let front = box_at(6.0, 26.0, 90.0, 78.0);
    painter.rect_filled(front, r(8.0), color);
    let corner = (8.0 * unit).round().clamp(1.0, 255.0) as u8;
    painter.rect_filled(
        box_at(6.0, 26.0, 90.0, 34.0),
        egui::CornerRadius {
            nw: corner,
            ne: corner,
            sw: 0,
            se: 0,
        },
        Color32::from_rgba_unmultiplied(255, 255, 255, 56),
    );

    // Emblème gravé : un liseré clair sous un trait sombre.
    if emblem != Emblem::Aucun && side >= 20.0 {
        let glyph = Rect::from_min_size(at(34.0, 38.0), vec2(28.0, 28.0) * unit);
        let width = 2.4 * glyph.width() / 24.0;
        paint_emblem(
            painter,
            glyph.translate(vec2(0.0, 1.2 * unit)),
            emblem,
            Stroke::new(width, Color32::from_rgba_unmultiplied(255, 255, 255, 140)),
        );
        paint_emblem(
            painter,
            glyph,
            emblem,
            Stroke::new(width, Color32::from_rgba_unmultiplied(0, 0, 0, 110)),
        );
    }
}

/// Dossier d'un seul ton (charte « Forge ») : dos et devant gris, emblème en accent. Pour
/// tout dossier dont l'utilisateur n'a pas choisi la couleur.
pub(crate) fn paint_plain_folder(painter: &egui::Painter, rect: Rect, emblem: Emblem) {
    let side = rect.width().min(rect.height());
    let unit = side / 96.0;
    let origin = rect.center() - vec2(48.0, 42.0) * unit;
    let at = |x: f32, y: f32| origin + vec2(x, y) * unit;
    let r = |v: f32| egui::CornerRadius::same((v * unit).round().clamp(1.0, 255.0) as u8);
    let box_at = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(at(x0, y0), at(x1, y1));
    let back = crate::style::folder_back();
    painter.rect_filled(box_at(6.0, 8.0, 40.0, 26.0), r(7.0), back);
    painter.add(Shape::convex_polygon(
        vec![at(32.0, 8.0), at(36.0, 8.0), at(46.0, 18.0), at(32.0, 18.0)],
        back,
        Stroke::NONE,
    ));
    painter.rect_filled(box_at(6.0, 16.0, 90.0, 78.0), r(8.0), back);
    painter.rect_filled(
        box_at(6.0, 26.0, 90.0, 78.0),
        r(8.0),
        crate::style::folder_front(),
    );
    if emblem != Emblem::Aucun && side >= 20.0 {
        let glyph = Rect::from_min_size(at(34.0, 38.0), vec2(28.0, 28.0) * unit);
        let width = (2.2 * glyph.width() / 24.0).max(1.2);
        paint_emblem(painter, glyph, emblem, Stroke::new(width, crate::style::accent()));
    }
}

/// Emblème tracé au trait dans `rect` (repère de 24 sur 24, comme les icônes de la planche).
pub(crate) fn paint_emblem(painter: &egui::Painter, rect: Rect, emblem: Emblem, stroke: Stroke) {
    let unit = rect.width() / 24.0;
    let at = |x: f32, y: f32| rect.min + vec2(x, y) * unit;
    let line = |points: &[(f32, f32)]| {
        painter.add(Shape::line(
            points.iter().map(|&(x, y)| at(x, y)).collect(),
            stroke,
        ));
    };
    let closed = |points: Vec<Pos2>| {
        painter.add(Shape::closed_line(points, stroke));
    };
    let rounded = |x0: f32, y0: f32, x1: f32, y1: f32, radius: f32| {
        painter.rect_stroke(
            Rect::from_min_max(at(x0, y0), at(x1, y1)),
            radius * unit,
            stroke,
            egui::StrokeKind::Middle,
        );
    };
    let arc_points = |cx: f32, cy: f32, radius: f32, from: f32, to: f32| -> Vec<Pos2> {
        let mut points = Vec::new();
        for step in 0..=12 {
            let angle = (from + (to - from) * step as f32 / 12.0).to_radians();
            points.push(at(cx + radius * angle.cos(), cy + radius * angle.sin()));
        }
        points
    };
    match emblem {
        Emblem::Aucun => {
            line(&[(7.0, 7.0), (17.0, 17.0)]);
            line(&[(17.0, 7.0), (7.0, 17.0)]);
        }
        Emblem::Etoile => {
            let points = (0..10)
                .map(|i| {
                    let radius = if i % 2 == 0 { 8.6 } else { 3.9 };
                    let angle = (-90.0 + i as f32 * 36.0f32).to_radians();
                    at(12.0 + radius * angle.cos(), 12.6 + radius * angle.sin())
                })
                .collect();
            closed(points);
        }
        Emblem::Coeur => {
            // Deux lobes en arc qui se rejoignent en pointe, comme la planche.
            let mut points = vec![at(12.0, 20.0)];
            points.extend(arc_points(8.3, 9.6, 4.0, 145.0, 345.0));
            points.extend(arc_points(15.7, 9.6, 4.0, 195.0, 395.0));
            points.push(at(12.0, 20.0));
            painter.add(Shape::line(points, stroke));
        }
        Emblem::Document => {
            closed(vec![
                at(7.0, 3.5),
                at(14.0, 3.5),
                at(18.0, 7.5),
                at(18.0, 20.5),
                at(7.0, 20.5),
            ]);
            line(&[(14.0, 3.5), (14.0, 7.5), (18.0, 7.5)]);
            line(&[(9.5, 12.0), (14.5, 12.0)]);
            line(&[(9.5, 15.5), (14.5, 15.5)]);
        }
        Emblem::Image => {
            rounded(4.0, 5.0, 20.0, 19.0, 2.0);
            painter.circle_stroke(at(9.0, 10.0), 1.5 * unit, stroke);
            line(&[(4.5, 17.0), (9.0, 13.0), (12.5, 16.0), (15.5, 13.5), (19.5, 17.0)]);
        }
        Emblem::Musique => {
            line(&[(9.0, 17.5), (9.0, 6.0), (19.0, 4.0), (19.0, 15.5)]);
            painter.circle_stroke(at(6.5, 17.5), 2.5 * unit, stroke);
            painter.circle_stroke(at(16.5, 15.5), 2.5 * unit, stroke);
        }
        Emblem::Video => {
            rounded(3.5, 6.0, 15.5, 18.0, 2.0);
            line(&[(15.5, 10.5), (20.5, 7.5), (20.5, 16.5), (15.5, 13.5)]);
        }
        Emblem::Code => {
            line(&[(9.0, 7.0), (4.0, 12.0), (9.0, 17.0)]);
            line(&[(15.0, 7.0), (20.0, 12.0), (15.0, 17.0)]);
        }
        Emblem::Telechargement => {
            line(&[(12.0, 4.0), (12.0, 15.0)]);
            line(&[(7.5, 10.5), (12.0, 15.0), (16.5, 10.5)]);
            line(&[(5.0, 19.5), (19.0, 19.5)]);
        }
        Emblem::Cadenas => {
            rounded(5.0, 10.5, 19.0, 20.5, 2.5);
            let mut shackle = vec![at(8.0, 10.5)];
            shackle.extend(arc_points(12.0, 8.0, 4.0, 180.0, 360.0));
            shackle.push(at(16.0, 10.5));
            painter.add(Shape::line(shackle, stroke));
        }
        Emblem::Livre => {
            line(&[
                (4.5, 5.5),
                (8.0, 4.6),
                (12.0, 6.0),
                (16.0, 4.6),
                (19.5, 5.5),
                (19.5, 18.5),
                (16.0, 17.6),
                (12.0, 19.0),
                (8.0, 17.6),
                (4.5, 18.5),
                (4.5, 5.5),
            ]);
            line(&[(12.0, 6.0), (12.0, 19.0)]);
        }
        Emblem::Ecran => {
            rounded(3.0, 4.0, 21.0, 16.0, 2.0);
            line(&[(9.0, 20.0), (15.0, 20.0)]);
            line(&[(12.0, 16.0), (12.0, 20.0)]);
        }
        Emblem::Personne => {
            painter.circle_stroke(at(12.0, 8.0), 4.0 * unit, stroke);
            painter.add(Shape::line(arc_points(12.0, 20.5, 7.5, 180.0, 360.0), stroke));
        }
        Emblem::Nuage => {
            let mut points = arc_points(7.5, 14.0, 4.0, 90.0, 270.0);
            points.extend(arc_points(12.0, 10.0, 5.0, 205.0, 335.0));
            points.extend(arc_points(17.0, 13.5, 4.5, 270.0, 450.0));
            closed(points);
        }
        Emblem::Maison => {
            line(&[(4.0, 11.0), (12.0, 4.5), (20.0, 11.0)]);
            line(&[(6.0, 9.5), (6.0, 19.5), (18.0, 19.5), (18.0, 9.5)]);
            line(&[(10.0, 19.5), (10.0, 14.5), (14.0, 14.5), (14.0, 19.5)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(name: &str) -> Option<String> {
        badge(Path::new(name)).map(|badge| badge.label)
    }

    #[test]
    fn l_extension_est_ecrite_sur_l_etiquette() {
        assert_eq!(label("photos.zip").as_deref(), Some("ZIP"));
        assert_eq!(label("sauvegarde.RAR").as_deref(), Some("RAR"));
        assert_eq!(label("linux.tar.gz").as_deref(), Some("TAR.GZ"));
        assert_eq!(label("installeur.deb").as_deref(), Some("DEB"));
        assert_eq!(label("LISEZMOI"), None);
        assert_eq!(label(".bashrc"), None);
        assert_eq!(label(".config.json").as_deref(), Some("JSON"));
    }

    #[test]
    fn dossiers_habituels_et_cles() {
        let home = Path::new("/home/a");
        assert_eq!(folder_default(home, home).1, Emblem::Maison);
        assert_eq!(
            folder_default(&home.join("Téléchargements"), home),
            (swatch("vert").unwrap(), Emblem::Telechargement)
        );
        assert_eq!(
            folder_default(&home.join(".cache"), home),
            (swatch("gris").unwrap(), Emblem::Aucun)
        );
        assert_eq!(
            folder_default(&home.join("Documents/Images"), home).1,
            Emblem::Aucun
        );
        for emblem in Emblem::ALL {
            assert_eq!(Emblem::from_key(emblem.key()), Some(emblem));
        }
    }

    #[test]
    fn chaque_archive_a_sa_couleur() {
        let color = |name: &str| badge(Path::new(name)).map(|badge| badge.color);
        let archives = ["a.zip", "a.rar", "a.7z", "a.tar.gz", "a.deb", "a.rpm"];
        for (i, a) in archives.iter().enumerate() {
            assert_eq!(badge(Path::new(a)).map(|b| b.motif), Some(Motif::Zipper), "{a}");
            for b in &archives[i + 1..] {
                assert_ne!(color(a), color(b), "{a} et {b}");
            }
        }
    }
}
