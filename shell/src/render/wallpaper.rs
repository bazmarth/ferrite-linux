//! Fond d'écran choisi dans Paramètres (voir `ferrite_ui::wallpaper`) : un fond dessiné
//! par Ferrite, ou une image de l'utilisateur recadrée pour couvrir l'écran. Dessiné à la
//! résolution réelle de l'écran (net sur un écran 2880×1800 comme en 1280×800).
use std::cell::RefCell;

use ferrite_ui::wallpaper::{self, Builtin, Image, Source};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::{Buffer, Logical, Rectangle, Transform},
};

use crate::system::WallpaperChoice;

/// Taille physique de l'écran et génération du fond dessiné.
type Key = ((i32, i32), u64);

/// Fond d'écran, redessiné à chaque nouvelle taille d'écran (en pixels physiques) ou
/// nouveau fond choisi.
#[derive(Debug, Default)]
pub struct Wallpaper {
    cached: RefCell<Option<(Key, MemoryRenderBuffer)>>,
}

impl Wallpaper {
    /// Texture à la taille physique de l'écran, et la zone à en dessiner.
    pub fn buffer(
        &self,
        choice: &WallpaperChoice,
        width: i32,
        height: i32,
    ) -> (MemoryRenderBuffer, Rectangle<f64, Logical>) {
        let size = (width.max(1), height.max(1));
        let key = (size, choice.generation);
        let mut cached = self.cached.borrow_mut();
        if cached.as_ref().is_none_or(|(known, _)| *known != key) {
            let image = pixels(choice, size.0 as u32, size.1 as u32);
            let opaque = vec![Rectangle::<i32, Buffer>::from_size(size.into())];
            let buffer = MemoryRenderBuffer::from_slice(
                &bgra(image),
                Fourcc::Argb8888,
                size,
                1,
                Transform::Normal,
                Some(opaque),
            );
            *cached = Some((key, buffer));
        }
        let buffer = cached
            .as_ref()
            .map(|(_, buffer)| buffer.clone())
            .expect("dessiné juste avant");
        (
            buffer,
            Rectangle::from_size((size.0 as f64, size.1 as f64).into()),
        )
    }
}

/// Fond choisi à `width` × `height` pixels ; une image illisible donne le fond Ferrite.
fn pixels(choice: &WallpaperChoice, width: u32, height: u32) -> Image {
    match (&choice.source, &choice.image) {
        (Source::Builtin(builtin), _) => wallpaper::paint(*builtin, width, height),
        (Source::File(_), Some(image)) => wallpaper::cover(image, width, height),
        (Source::File(_), None) => wallpaper::paint(Builtin::Ferrite, width, height),
    }
}

/// RVBA → BGRA (`Argb8888` en petit-boutiste), en place.
fn bgra(mut image: Image) -> Vec<u8> {
    for pixel in image.rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    image.rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fond_opaque_aux_dimensions_attendues() {
        // Écran du ThinkBook : texture à sa résolution réelle.
        let (_, source) = Wallpaper::default().buffer(&WallpaperChoice::default(), 2880, 1800);
        assert_eq!(source.size, (2880.0, 1800.0).into());
        let image = pixels(&WallpaperChoice::default(), 64, 40);
        let pixels = bgra(image.clone());
        assert_eq!(pixels.len(), 64 * 40 * 4);
        assert_eq!([pixels[0], pixels[2]], [image.rgba[2], image.rgba[0]]);
    }

    #[test]
    fn image_illisible_fond_ferrite() {
        let missing = WallpaperChoice {
            source: Source::File("/nulle/part.png".into()),
            ..WallpaperChoice::default()
        };
        assert_eq!(
            pixels(&missing, 32, 20),
            wallpaper::paint(Builtin::Ferrite, 32, 20)
        );
    }
}
