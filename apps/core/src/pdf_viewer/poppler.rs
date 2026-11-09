//! Appels à Poppler (`pdfinfo`, `pdftoppm`, `pdftotext`, `pdftohtml`), toujours depuis un fil
//! d'arrière-plan : taille de chaque page, rendu d'une page à une largeur donnée, et texte
//! avec la position de chaque mot (recherche, sélection), et plan du document.

use std::{io::Cursor, path::Path, process::Command};

use eframe::egui;

/// Nombre de pixels au-delà duquel un rendu est refusé (protège la mémoire).
pub const MAX_PIXELS: usize = 24_000_000;

/// Ce qu'il faut savoir d'un document avant d'en dessiner la moindre page.
#[derive(Debug, Clone, PartialEq)]
pub struct DocInfo {
    /// Taille de chaque page en points, rotation déjà appliquée.
    pub pages: Vec<egui::Vec2>,
    pub title: Option<String>,
}

/// Un mot d'une page et son rectangle, en points depuis le coin haut gauche.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub rect: egui::Rect,
}

fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<Vec<u8>, String> {
    let output = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("Poppler n’est pas installé (commande {program} introuvable).")
            } else {
                format!("Lecture du PDF impossible : {error}")
            }
        })?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if message.is_empty() {
            "Le document PDF est illisible.".to_owned()
        } else {
            format!("Le document PDF est illisible : {message}")
        });
    }
    Ok(output.stdout)
}

pub fn load_info(path: &Path) -> Result<DocInfo, String> {
    let stdout = run(
        "pdfinfo",
        &[
            "-f".as_ref(),
            "1".as_ref(),
            "-l".as_ref(),
            "1000000".as_ref(),
            path.as_os_str(),
        ],
    )?;
    parse_info(&String::from_utf8_lossy(&stdout))
        .ok_or_else(|| "Le document PDF ne contient aucune page lisible.".to_owned())
}

/// Lit la sortie de `pdfinfo -f 1 -l N` : nombre de pages, taille et rotation de chacune.
pub fn parse_info(output: &str) -> Option<DocInfo> {
    let mut count = 0usize;
    let mut title = None;
    let mut default_size = None;
    let mut sizes: Vec<Option<egui::Vec2>> = Vec::new();
    let mut rotations: Vec<u32> = Vec::new();
    for line in output.lines() {
        if let Some(value) = line.strip_prefix("Pages:") {
            count = value.trim().parse().ok()?;
            sizes.resize(count, None);
            rotations.resize(count, 0);
        } else if let Some(value) = line.strip_prefix("Title:") {
            let value = value.trim();
            if !value.is_empty() {
                title = Some(value.to_owned());
            }
        } else if let Some(value) = line.strip_prefix("Page size:") {
            default_size = parse_size(value);
        } else if let Some(rest) = line.strip_prefix("Page ") {
            // « Page    3 size:  595 x 842 pts (A4) » ou « Page    3 rot:   90 »
            let rest = rest.trim_start();
            let (number, rest) = rest.split_once(' ')?;
            let Ok(number) = number.parse::<usize>() else {
                continue;
            };
            let Some(index) = number.checked_sub(1).filter(|index| *index < count) else {
                continue;
            };
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix("size:") {
                sizes[index] = parse_size(value);
            } else if let Some(value) = rest.strip_prefix("rot:") {
                rotations[index] = value.trim().parse().unwrap_or(0);
            }
        }
    }
    if count == 0 {
        return None;
    }
    let fallback = default_size.unwrap_or(egui::vec2(595.0, 842.0));
    let pages = sizes
        .into_iter()
        .zip(rotations)
        .map(|(size, rotation)| {
            let size = size.unwrap_or(fallback);
            if rotation % 180 == 90 {
                egui::vec2(size.y, size.x)
            } else {
                size
            }
        })
        .collect();
    Some(DocInfo { pages, title })
}

fn parse_size(value: &str) -> Option<egui::Vec2> {
    let mut parts = value.split_whitespace();
    let width: f32 = parts.next()?.parse().ok()?;
    if parts.next()? != "x" {
        return None;
    }
    let height: f32 = parts.next()?.parse().ok()?;
    (width > 0.0 && height > 0.0).then(|| egui::vec2(width, height))
}

/// Rend la page `page` (numérotée depuis 1) à `width` pixels de large.
pub fn render(path: &Path, page: usize, width: u32) -> Result<egui::ColorImage, String> {
    let page = page.to_string();
    let width = width.max(16).to_string();
    let png = run(
        "pdftoppm",
        &[
            "-f".as_ref(),
            page.as_ref(),
            "-l".as_ref(),
            page.as_ref(),
            "-scale-to-x".as_ref(),
            width.as_ref(),
            "-scale-to-y".as_ref(),
            "-1".as_ref(),
            "-singlefile".as_ref(),
            "-png".as_ref(),
            "-aa".as_ref(),
            "yes".as_ref(),
            "-aaVector".as_ref(),
            "yes".as_ref(),
            path.as_os_str(),
        ],
    )?;
    decode_png(&png)
}

/// Texte de tout le document, mot par mot, page par page.
pub fn load_words(path: &Path) -> Result<Vec<Vec<Word>>, String> {
    let stdout = run(
        "pdftotext",
        &[
            "-bbox".as_ref(),
            "-enc".as_ref(),
            "UTF-8".as_ref(),
            path.as_os_str(),
            "-".as_ref(),
        ],
    )?;
    Ok(parse_words(&String::from_utf8_lossy(&stdout)))
}

/// Lit la sortie HTML de `pdftotext -bbox`.
pub fn parse_words(output: &str) -> Vec<Vec<Word>> {
    let mut pages: Vec<Vec<Word>> = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("<page ") {
            pages.push(Vec::new());
        } else if let Some(rest) = line.strip_prefix("<word ") {
            let Some(page) = pages.last_mut() else {
                continue;
            };
            let Some((attributes, text)) = rest.split_once('>') else {
                continue;
            };
            let text = text.strip_suffix("</word>").unwrap_or(text);
            let number = |name: &str| -> Option<f32> {
                let start = attributes.find(&format!("{name}=\""))? + name.len() + 2;
                let end = attributes[start..].find('"')? + start;
                attributes[start..end].parse().ok()
            };
            let (Some(x0), Some(y0), Some(x1), Some(y1)) =
                (number("xMin"), number("yMin"), number("xMax"), number("yMax"))
            else {
                continue;
            };
            page.push(Word {
                text: unescape(text),
                rect: egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1)),
            });
        }
    }
    pages
}

/// Une entrée du plan du document (ses « signets » d'origine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    /// Profondeur : 0 pour un chapitre, 1 pour une section…
    pub level: usize,
    /// Page visée, numérotée depuis 0.
    pub page: usize,
    pub title: String,
}

/// Plan du document, vide s'il n'en a pas. `pdftohtml -xml` le donne en entier même quand
/// on ne lui demande que la première page.
pub fn load_outline(path: &Path) -> Vec<OutlineItem> {
    run(
        "pdftohtml",
        &[
            "-xml".as_ref(),
            "-i".as_ref(),
            "-stdout".as_ref(),
            "-f".as_ref(),
            "1".as_ref(),
            "-l".as_ref(),
            "1".as_ref(),
            path.as_os_str(),
        ],
    )
    .map(|stdout| parse_outline(&String::from_utf8_lossy(&stdout)))
    .unwrap_or_default()
}

/// Lit les balises `<outline>` et `<item page="N">` de `pdftohtml -xml`.
pub fn parse_outline(output: &str) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    for line in output.lines() {
        let line = line.trim();
        if line == "<outline>" {
            depth += 1;
        } else if line == "</outline>" {
            depth = depth.saturating_sub(1);
        } else if let Some(rest) = line.strip_prefix("<item") {
            let Some((attributes, title)) = rest.split_once('>') else {
                continue;
            };
            let title = unescape(title.strip_suffix("</item>").unwrap_or(title).trim());
            let page = attributes
                .split_once("page=\"")
                .and_then(|(_, rest)| rest.split_once('"'))
                .and_then(|(page, _)| page.parse::<usize>().ok())
                .and_then(|page| page.checked_sub(1));
            if let Some(page) = page
                && !title.is_empty()
                && depth > 0
            {
                items.push(OutlineItem {
                    level: depth - 1,
                    page,
                    title,
                });
            }
        }
    }
    items
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

pub fn decode_png(data: &[u8]) -> Result<egui::ColorImage, String> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|error| format!("Image PDF invalide : {error}"))?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("Image PDF trop grande")?];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| format!("Image PDF invalide : {error}"))?;
    if info.width as usize * info.height as usize > MAX_PIXELS {
        return Err("La page rendue dépasse la taille maximale autorisée.".to_owned());
    }
    let pixels = &buffer[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .chunks(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .chunks(2)
            .flat_map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
            .collect(),
        png::ColorType::Grayscale => pixels
            .iter()
            .flat_map(|&value| [value, value, value, 255])
            .collect(),
        png::ColorType::Indexed => return Err("Format d’image PDF non pris en charge.".to_owned()),
    };
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [info.width as usize, info.height as usize],
        &rgba,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_la_taille_et_la_rotation_de_chaque_page() {
        let output = "Title:           Rapport annuel\nPages:           3\n\
            Page    1 size:  595.276 x 841.89 pts (A4)\nPage    1 rot:   0\n\
            Page    2 size:  595.276 x 841.89 pts (A4)\nPage    2 rot:   90\n\
            Page    3 size:  612 x 792 pts (letter)\nPage    3 rot:   0\n";
        let info = parse_info(output).unwrap();
        assert_eq!(info.title.as_deref(), Some("Rapport annuel"));
        assert_eq!(info.pages.len(), 3);
        assert_eq!(info.pages[0], egui::vec2(595.276, 841.89));
        assert_eq!(info.pages[1], egui::vec2(841.89, 595.276));
        assert_eq!(info.pages[2], egui::vec2(612.0, 792.0));
    }

    #[test]
    fn taille_commune_quand_les_pages_ne_sont_pas_detaillees() {
        let info = parse_info("Pages: 2\nPage size: 612 x 792 pts (letter)\n").unwrap();
        assert_eq!(info.pages, vec![egui::vec2(612.0, 792.0); 2]);
        assert!(parse_info("Pages: 0\n").is_none());
        assert!(parse_info("Title: vide\n").is_none());
    }

    #[test]
    fn lit_les_mots_et_leur_position() {
        let output = r#"<doc>
  <page width="595" height="842">
    <word xMin="10.5" yMin="20" xMax="40" yMax="32">L&apos;été</word>
    <word xMin="44" yMin="20" xMax="60" yMax="32">&amp;</word>
  </page>
  <page width="595" height="842">
  </page>
</doc>"#;
        let pages = parse_words(output);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0][0].text, "L'été");
        assert_eq!(pages[0][1].text, "&");
        assert_eq!(pages[0][0].rect.min, egui::pos2(10.5, 20.0));
        assert!(pages[1].is_empty());
    }

    #[test]
    fn lit_le_plan_du_document() {
        let output = "<pdf2xml>\n<page number=\"1\">\n<text>1. Faux titre</text>\n</page>\n\
            <outline>\n<item page=\"1\">1. Introduction</item>\n<outline>\n\
            <item page=\"3\">1.1. Pourquoi &amp; comment</item>\n</outline>\n\
            <item page=\"9\">2. Suite</item>\n<item>Sans page</item>\n</outline>\n</pdf2xml>";
        let items = parse_outline(output);
        assert_eq!(items.len(), 3);
        assert_eq!((items[0].level, items[0].page), (0, 0));
        assert_eq!(items[1].title, "1.1. Pourquoi & comment");
        assert_eq!((items[1].level, items[1].page), (1, 2));
        assert_eq!((items[2].level, items[2].page), (0, 8));
        assert!(parse_outline("<pdf2xml>\n</pdf2xml>").is_empty());
    }

    #[test]
    fn decode_une_page_png_rendue() {
        let mut data = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut data, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[12, 34, 56, 255]).unwrap();
        }
        let image = decode_png(&data).unwrap();
        assert_eq!(image.size, [1, 1]);
        assert_eq!(image.pixels, [egui::Color32::from_rgb(12, 34, 56)]);
    }
}
