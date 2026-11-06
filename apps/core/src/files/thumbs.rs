//! Vignettes des images (PNG, JPEG et SVG), décodées dans un fil à part puis gardées en texture :
//! la grille et le panneau d'aperçu les montrent à la place de l'icône du fichier.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use eframe::egui::{self, TextureHandle, TextureOptions};

/// Côté maximal d'une vignette, en pixels.
const SIDE: u32 = 360;
/// Au-delà, l'image n'est pas décodée (vignette trop longue à produire).
const MAX_BYTES: u64 = 40 * 1024 * 1024;

enum Slot {
    Pending,
    Ready(TextureHandle),
    Failed,
}

pub struct Thumbnails {
    slots: HashMap<PathBuf, Slot>,
    sender: Sender<(PathBuf, Option<egui::ColorImage>)>,
    receiver: Receiver<(PathBuf, Option<egui::ColorImage>)>,
}

impl Default for Thumbnails {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            slots: HashMap::new(),
            sender,
            receiver,
        }
    }
}

/// L'extension a-t-elle une vignette ?
pub fn supported(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "svg")
    )
}

impl Thumbnails {
    /// Vignette de `path`, ou rien tant qu'elle n'est pas prête (elle est alors demandée).
    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<TextureHandle> {
        while let Ok((done, image)) = self.receiver.try_recv() {
            let slot = match image {
                Some(image) => {
                    Slot::Ready(ctx.load_texture(done.to_string_lossy(), image, TextureOptions::LINEAR))
                }
                None => Slot::Failed,
            };
            self.slots.insert(done, slot);
        }
        if !supported(path) {
            return None;
        }
        match self.slots.get(path) {
            Some(Slot::Ready(texture)) => Some(texture.clone()),
            Some(_) => None,
            None => {
                self.slots.insert(path.to_path_buf(), Slot::Pending);
                let sender = self.sender.clone();
                let path = path.to_path_buf();
                let ctx = ctx.clone();
                thread::spawn(move || {
                    let image = decode(&path);
                    let _ = sender.send((path, image));
                    ctx.request_repaint();
                });
                None
            }
        }
    }

    /// Oublie les vignettes d'un dossier relu (fichiers modifiés entre-temps).
    pub fn forget(&mut self) {
        self.slots.retain(|_, slot| matches!(slot, Slot::Pending));
    }
}

fn decode(path: &Path) -> Option<egui::ColorImage> {
    if fs::metadata(path).ok()?.len() > MAX_BYTES {
        return None;
    }
    let data = fs::read(path).ok()?;
    let is_svg = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    if is_svg {
        decode_svg(&data)
    } else if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "jpg" | "jpeg"))
    {
        let image = ferrite_ui::wallpaper::decode_bytes(data).ok()?;
        let image = crate::viewer::shrink(&image, SIDE);
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [image.width as usize, image.height as usize],
            &image.rgba,
        ))
    } else {
        decode_png(&data)
    }
}

fn decode_svg(data: &[u8]) -> Option<egui::ColorImage> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let factor = SIDE as f32 / size.width().max(size.height());
    let (width, height) = (
        (size.width() * factor).round().max(1.0) as u32,
        (size.height() * factor).round().max(1.0) as u32,
    );
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(factor, factor),
        &mut pixmap.as_mut(),
    );
    Some(egui::ColorImage::from_rgba_premultiplied(
        [width as usize, height as usize],
        pixmap.data(),
    ))
}

fn decode_png(data: &[u8]) -> Option<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let (width, height) = (info.width as usize, info.height as usize);
    if width * height > 8192 * 8192 {
        return None;
    }
    let pixels = &buffer[..info.buffer_size()];
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    // Réduction par échantillonnage : la vignette tient dans `SIDE` × `SIDE`.
    let factor = (SIDE as f32 / width.max(height) as f32).min(1.0);
    let (out_w, out_h) = (
        ((width as f32 * factor).round() as usize).max(1),
        ((height as f32 * factor).round() as usize).max(1),
    );
    let mut rgba = Vec::with_capacity(out_w * out_h * 4);
    for y in 0..out_h {
        let source_y = (y * height / out_h).min(height - 1);
        for x in 0..out_w {
            let source_x = (x * width / out_w).min(width - 1);
            let p = &pixels[(source_y * width + source_x) * channels..][..channels];
            rgba.extend_from_slice(&match channels {
                4 => [p[0], p[1], p[2], p[3]],
                3 => [p[0], p[1], p[2], 255],
                2 => [p[0], p[0], p[0], p[1]],
                _ => [p[0], p[0], p[0], 255],
            });
        }
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([out_w, out_h], &rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vignettes_les_images_jpeg_sans_tenir_compte_de_la_casse() {
        assert!(supported(Path::new("photo.jpg")));
        assert!(supported(Path::new("photo.jpeg")));
        assert!(supported(Path::new("PHOTO.JPG")));
        assert!(supported(Path::new("photo.png")));
        assert!(supported(Path::new("dessin.svg")));
        assert!(!supported(Path::new("photo.gif")));
    }
}
