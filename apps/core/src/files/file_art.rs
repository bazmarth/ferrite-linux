//! Icônes des fichiers, d'après la planche « Fichiers » : une page blanche au coin plié,
//! un motif selon la famille (fermeture éclair des archives, lecture des vidéos…) et une
//! étiquette de couleur où l'extension est écrite (ZIP, RAR, 7Z, DEB, PDF…), pour
//! reconnaître chaque type d'un coup d'œil. Les dossiers gardent les icônes du thème.

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
pub(crate) fn paint(painter: &egui::Painter, rect: Rect, path: &Path) {
    paint_badge(painter, rect, badge(path).as_ref());
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
