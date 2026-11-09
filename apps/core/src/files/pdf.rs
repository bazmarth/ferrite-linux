//! Fichiers → clic droit : « Créer un PDF » à partir de photos (une page A4 par image,
//! dans l'ordre des noms) et « Assembler les PDF » (`pdfunite`, de poppler-utils).
//!
//! Le PDF est écrit ici, sans outil extérieur : une photo JPEG y entre telle quelle (sans
//! perte, rotation de l'appareil photo respectée), une image PNG y est recompressée sans
//! perte (zlib).

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use super::ops::{self, Job};

/// Page A4, en points (1/72 de pouce).
const A4: (f64, f64) = (595.28, 841.89);

/// L'image se met en page : JPEG ou PNG.
pub fn is_image(path: &Path) -> bool {
    matches!(extension(path).as_deref(), Some("jpg" | "jpeg" | "png"))
}

pub fn is_pdf(path: &Path) -> bool {
    extension(path).as_deref() == Some("pdf")
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
}

/// Fichiers dans l'ordre de leurs noms (« 2 » avant « 10 »).
fn in_name_order(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut sorted = paths.to_vec();
    sorted.sort_by(|a, b| {
        let name = |path: &PathBuf| path.file_name().map(|name| name.to_string_lossy().into_owned());
        super::natural_cmp(&name(a).unwrap_or_default(), &name(b).unwrap_or_default())
    });
    sorted
}

/// Nom du PDF : celui de la première image, ou « Document » pour plusieurs.
fn target_name(sorted: &[PathBuf], plural: &str) -> String {
    match sorted {
        [one] => one
            .file_stem()
            .map(|stem| format!("{}.pdf", stem.to_string_lossy()))
            .unwrap_or_else(|| format!("{plural}.pdf")),
        _ => format!("{plural}.pdf"),
    }
}

/// Crée un PDF des `images` dans `dir`.
pub fn images_to_pdf(images: &[PathBuf], dir: &Path, job: &Job) -> Result<(), String> {
    if images.is_empty() || !images.iter().all(|path| is_image(path)) {
        return Err("Choisissez des photos JPEG ou PNG".into());
    }
    let sorted = in_name_order(images);
    let target = ops::unique_destination(dir, &target_name(&sorted, "Document"));
    let mut pages = Vec::with_capacity(sorted.len());
    for (index, path) in sorted.iter().enumerate() {
        if job.cancelled() {
            return Err("Annulé".into());
        }
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
        job.update(|progress| {
            progress.current = name.unwrap_or_default();
            progress.done_bytes = index as u64;
            progress.total_bytes = sorted.len() as u64;
        });
        pages.push(picture(path)?);
    }
    let data = write_pdf(&pages);
    // Écrit à côté puis renommé : jamais de PDF à moitié écrit sous son vrai nom.
    let partial = target.with_extension("pdf.partiel");
    std::fs::write(&partial, data)
        .and_then(|()| std::fs::rename(&partial, &target))
        .map_err(|error| {
            let _ = std::fs::remove_file(&partial);
            format!("PDF non enregistré : {error}")
        })?;
    let name = target.file_name().map(|name| name.to_string_lossy().into_owned());
    job.update(|progress| progress.success = Some(format!("{} créé", name.unwrap_or_default())));
    Ok(())
}

/// Assemble les `pdfs` (au moins deux) en un seul, dans `dir`.
pub fn merge_pdfs(pdfs: &[PathBuf], dir: &Path, job: &Job) -> Result<(), String> {
    if pdfs.len() < 2 || !pdfs.iter().all(|path| is_pdf(path)) {
        return Err("Choisissez au moins deux PDF".into());
    }
    let sorted = in_name_order(pdfs);
    let target = ops::unique_destination(dir, "Assemblage.pdf");
    job.update(|progress| progress.current = "Assemblage.pdf".into());
    let mut command = Command::new("pdfunite");
    command.args(&sorted).arg(&target);
    ops::run_command(
        command,
        job,
        "Assemblage impossible (pdfunite, paquet poppler-utils)",
    )
    .inspect_err(|_| {
        let _ = std::fs::remove_file(&target);
    })?;
    job.update(|progress| progress.success = Some("Assemblage.pdf créé".into()));
    Ok(())
}

/// Image prête à entrer dans le PDF.
#[derive(Debug, Clone, PartialEq)]
struct Picture {
    width: u32,
    height: u32,
    /// Gris (1) ou couleur (3).
    components: u8,
    /// Données du flux et filtre PDF (`DCTDecode` ou `FlateDecode`).
    data: Vec<u8>,
    filter: &'static str,
    /// Orientation EXIF (1 à 8) : sens dans lequel l'appareil a pris la photo.
    orientation: u8,
}

impl Picture {
    /// Taille une fois tournée dans le bon sens.
    fn shown(&self) -> (f64, f64) {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        if self.orientation >= 5 { (h, w) } else { (w, h) }
    }
}

fn picture(path: &Path) -> Result<Picture, String> {
    let name = path.display();
    let bytes = std::fs::read(path).map_err(|error| format!("{name} : {error}"))?;
    if let Some(info) = jpeg_info(&bytes).filter(|info| matches!(info.components, 1 | 3)) {
        return Ok(Picture {
            width: info.width,
            height: info.height,
            components: info.components,
            data: bytes,
            filter: "DCTDecode",
            orientation: info.orientation,
        });
    }
    let image = ferrite_ui::wallpaper::decode_bytes(bytes).map_err(|error| format!("{name} : {error}"))?;
    // Transparence posée sur du blanc, comme sur une feuille.
    let rgb: Vec<u8> = image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let alpha = u16::from(p[3]);
            let over = |value: u8| ((u16::from(value) * alpha + 255 * (255 - alpha)) / 255) as u8;
            [over(p[0]), over(p[1]), over(p[2])]
        })
        .collect();
    Ok(Picture {
        width: image.width,
        height: image.height,
        components: 3,
        data: miniz_oxide::deflate::compress_to_vec_zlib(&rgb, 6),
        filter: "FlateDecode",
        orientation: 1,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JpegInfo {
    width: u32,
    height: u32,
    components: u8,
    orientation: u8,
}

/// Taille, nombre de composantes et orientation EXIF d'un JPEG (sans le décoder).
fn jpeg_info(bytes: &[u8]) -> Option<JpegInfo> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut orientation = 1;
    let mut at = 2;
    while at + 4 <= bytes.len() {
        if bytes[at] != 0xFF {
            return None;
        }
        let marker = bytes[at + 1];
        if marker == 0xFF {
            at += 1;
            continue;
        }
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        let segment = bytes.get(at + 4..at + 2 + length)?;
        match marker {
            0xE1 if segment.starts_with(b"Exif\0\0") => {
                orientation = exif_orientation(&segment[6..]).unwrap_or(1);
            }
            // Début d'image : SOF0 à SOF15, sauf DHT (C4), JPG (C8) et DAC (CC).
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                return Some(JpegInfo {
                    height: u32::from(u16::from_be_bytes([*segment.get(1)?, *segment.get(2)?])),
                    width: u32::from(u16::from_be_bytes([*segment.get(3)?, *segment.get(4)?])),
                    components: *segment.get(5)?,
                    orientation,
                });
            }
            _ => {}
        }
        at += 2 + length;
    }
    None
}

/// Orientation (balise 0x0112) dans un bloc TIFF d'EXIF.
fn exif_orientation(tiff: &[u8]) -> Option<u8> {
    let little = match tiff.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let pair = [*tiff.get(at)?, *tiff.get(at + 1)?];
        Some(if little {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let quad = [
            *tiff.get(at)?,
            *tiff.get(at + 1)?,
            *tiff.get(at + 2)?,
            *tiff.get(at + 3)?,
        ];
        Some(if little {
            u32::from_le_bytes(quad)
        } else {
            u32::from_be_bytes(quad)
        })
    };
    let directory = usize::try_from(u32_at(4)?).ok()?;
    let count = usize::from(u16_at(directory)?);
    (0..count).find_map(|index| {
        let entry = directory + 2 + index * 12;
        (u16_at(entry)? == 0x0112)
            .then(|| u16_at(entry + 8))
            .flatten()
            .and_then(|value| u8::try_from(value).ok())
            .filter(|value| (1..=8).contains(value))
    })
}

/// Matrice qui pose l'image (carré unité) dans la boîte `x, y, w, h` de la page, tournée
/// ou retournée selon son orientation EXIF.
fn placement(orientation: u8, (x, y, w, h): (f64, f64, f64, f64)) -> [f64; 6] {
    match orientation {
        2 => [-w, 0.0, 0.0, h, x + w, y],
        3 => [-w, 0.0, 0.0, -h, x + w, y + h],
        4 => [w, 0.0, 0.0, -h, x, y + h],
        5 => [0.0, -h, -w, 0.0, x + w, y + h],
        6 => [0.0, -h, w, 0.0, x, y + h],
        7 => [0.0, h, w, 0.0, x, y],
        8 => [0.0, h, -w, 0.0, x + w, y],
        _ => [w, 0.0, 0.0, h, x, y],
    }
}

/// Document PDF : une page A4 par image (à l'italienne si l'image est plus large que
/// haute), l'image centrée et aussi grande que possible.
fn write_pdf(pages: &[Picture]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    // 1 : catalogue ; 2 : arbre des pages ; puis page, contenu, image pour chaque page.
    let kids: String = (0..pages.len())
        .map(|index| format!("{} 0 R", 3 + index * 3))
        .collect::<Vec<_>>()
        .join(" ");
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(
        &mut out,
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()).as_bytes(),
    );
    for (index, page) in pages.iter().enumerate() {
        let (page_obj, content_obj, image_obj) = (3 + index * 3, 4 + index * 3, 5 + index * 3);
        let _ = page_obj;
        let (shown_w, shown_h) = page.shown();
        let (pw, ph) = if shown_w > shown_h { (A4.1, A4.0) } else { A4 };
        let scale = (pw / shown_w).min(ph / shown_h);
        let (w, h) = (shown_w * scale, shown_h * scale);
        let (x, y) = ((pw - w) / 2.0, (ph - h) / 2.0);
        let [a, b, c, d, e, f] = placement(page.orientation, (x, y, w, h));
        let mut content = String::new();
        let _ = write!(
            content,
            "q {a:.3} {b:.3} {c:.3} {d:.3} {e:.3} {f:.3} cm /Im0 Do Q"
        );
        object(
            &mut out,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] \
                 /Resources << /XObject << /Im0 {image_obj} 0 R >> >> /Contents {content_obj} 0 R >>"
            )
            .as_bytes(),
        );
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend_from_slice(content.as_bytes());
        stream.extend_from_slice(b"\nendstream");
        object(&mut out, &stream);
        let space = if page.components == 1 {
            "DeviceGray"
        } else {
            "DeviceRGB"
        };
        let mut stream = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /{space} \
             /BitsPerComponent 8 /Filter /{} /Length {} >>\nstream\n",
            page.width,
            page.height,
            page.filter,
            page.data.len()
        )
        .into_bytes();
        stream.extend_from_slice(&page.data);
        stream.extend_from_slice(b"\nendstream");
        object(&mut out, &stream);
    }
    let xref = out.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
    for offset in &offsets {
        let _ = writeln!(table, "{offset:010} 00000 n ");
    }
    let _ = write!(
        table,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        offsets.len() + 1
    );
    out.extend_from_slice(table.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JPEG minimal : SOI, APP1 EXIF (orientation 6, ordre Intel), SOF0 de 40 × 30 en
    /// couleur. Pas d'image dedans : seule l'en-tête compte ici.
    fn fake_jpeg() -> Vec<u8> {
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&6u16.to_le_bytes());
        tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&u16::try_from(app1.len() + 2).unwrap().to_be_bytes());
        jpeg.extend_from_slice(&app1);
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0, 17, 8, 0, 30, 0, 40, 3]);
        jpeg.extend_from_slice(&[0; 9]);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        jpeg
    }

    #[test]
    fn en_tete_jpeg_et_orientation() {
        assert_eq!(
            jpeg_info(&fake_jpeg()),
            Some(JpegInfo {
                width: 40,
                height: 30,
                components: 3,
                orientation: 6,
            })
        );
        assert_eq!(jpeg_info(b"\x89PNG"), None);
    }

    #[test]
    fn photo_tournee_couvre_la_boite() {
        // Coins du carré unité envoyés par chaque matrice : ils restent dans la boîte et
        // en touchent les quatre coins.
        let boxed = (10.0, 20.0, 100.0, 50.0);
        for orientation in 1..=8 {
            let [a, b, c, d, e, f] = placement(orientation, boxed);
            let mut corners: Vec<(i64, i64)> = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                .iter()
                .map(|(u, v)| ((a * u + c * v + e) as i64, (b * u + d * v + f) as i64))
                .collect();
            corners.sort_unstable();
            assert_eq!(
                corners,
                [(10, 20), (10, 70), (110, 20), (110, 70)],
                "orientation {orientation}"
            );
        }
        // Orientation 6 (téléphone tenu droit) : le haut de la photo va à droite.
        let [a, b, c, d, e, f] = placement(6, boxed);
        assert_eq!((a * 0.0 + c * 1.0 + e, b * 0.0 + d * 1.0 + f), (110.0, 70.0));
    }

    #[test]
    fn pdf_lisible() {
        let page = |width, height, orientation| Picture {
            width,
            height,
            components: 3,
            data: vec![1, 2, 3],
            filter: "DCTDecode",
            orientation,
        };
        let pdf = write_pdf(&[page(40, 30, 6), page(300, 100, 1)]);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/Count 2"));
        assert!(text.contains("/Kids [3 0 R 6 0 R]"));
        // Photo tournée : page en hauteur ; image large : page à l'italienne.
        assert!(text.contains("/MediaBox [0 0 595.28 841.89]"));
        assert!(text.contains("/MediaBox [0 0 841.89 595.28]"));
        // Chaque décalage de la table pointe bien sur son objet.
        let xref = text.find("xref\n").unwrap();
        for (index, line) in text[xref..].lines().skip(3).take(7).enumerate() {
            let offset: usize = line[..10].parse().unwrap();
            // Sur les octets : l'en-tête binaire ne survit pas à la lecture en texte.
            assert!(pdf[offset..].starts_with(format!("{} 0 obj", index + 1).as_bytes()));
        }
        assert!(text.ends_with("%%EOF\n"));
    }

    #[test]
    fn noms_et_ordre() {
        let paths = [PathBuf::from("/a/page 10.jpg"), PathBuf::from("/a/page 2.jpg")];
        let sorted = in_name_order(&paths);
        assert_eq!(sorted[0], PathBuf::from("/a/page 2.jpg"));
        assert_eq!(target_name(&sorted, "Document"), "Document.pdf");
        assert_eq!(target_name(&sorted[..1], "Document"), "page 2.pdf");
        assert!(is_image(Path::new("x.JPG")) && is_pdf(Path::new("x.PDF")));
    }
}
