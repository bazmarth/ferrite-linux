//! Capture d'écran PNG du compositeur (option `--capture`), pour vérifier le rendu
//! automatiquement. Les captures demandées par l'utilisateur passeront par screencopy.

use std::{fs::File, io::BufWriter, path::Path};

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

    let file = File::create(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    writer.write_image_data(&data).map_err(|err| err.to_string())?;
    writer.finish().map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
