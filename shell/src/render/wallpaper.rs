//! Fond d'écran choisi dans Paramètres (voir `ferrite_ui::wallpaper`) : un fond dessiné
//! par Ferrite, ou une image de l'utilisateur recadrée pour couvrir l'écran. Dessiné à la
//! résolution réelle de l'écran (net sur un écran 2880×1800 comme en 1280×800).
//!
//! Le dessin prend 80 à 200 ms : seul le tout premier se fait sur place. Ensuite (autre
//! taille d'écran, autre fond), il se fait dans un fil à part et l'ancien fond reste
//! affiché, étiré au besoin, jusqu'à ce que le nouveau soit prêt.
use std::{cell::RefCell, sync::mpsc};

use ferrite_ui::wallpaper::{self, Builtin, Image, Source};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    reexports::calloop::ping::Ping,
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
    /// Fond en cours de dessin dans un fil à part (pixels BGRA).
    pending: RefCell<Option<(Key, mpsc::Receiver<Vec<u8>>)>>,
}

impl Wallpaper {
    /// Texture du fond, et la zone à en dessiner (toute la texture, que l'appelant pose
    /// sur tout l'écran). `wake` relance le rendu quand un fond dessiné à part est prêt.
    pub fn buffer(
        &self,
        choice: &WallpaperChoice,
        width: i32,
        height: i32,
        wake: Option<&Ping>,
    ) -> (MemoryRenderBuffer, Rectangle<f64, Logical>) {
        let size = (width.max(1), height.max(1));
        let key = (size, choice.generation);
        self.collect();
        let mut cached = self.cached.borrow_mut();
        match cached.as_ref() {
            Some((known, _)) if *known == key => {}
            // Déjà un fond à l'écran : le nouveau est dessiné à part, sans arrêter l'image.
            Some(_) if self.start(choice, key, wake) => {}
            _ => {
                let pixels = bgra(pixels(choice, size.0 as u32, size.1 as u32));
                *cached = Some((key, texture(&pixels, size)));
            }
        }
        let ((known, _), buffer) = cached.as_ref().expect("dessiné juste avant");
        (
            buffer.clone(),
            Rectangle::from_size((known.0 as f64, known.1 as f64).into()),
        )
    }

    /// Range le fond dessiné à part s'il est prêt.
    fn collect(&self) {
        let mut pending = self.pending.borrow_mut();
        let Some((key, receiver)) = pending.as_ref() else {
            return;
        };
        match receiver.try_recv() {
            Ok(pixels) => {
                *self.cached.borrow_mut() = Some((*key, texture(&pixels, key.0)));
                *pending = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => *pending = None,
        }
    }

    /// Lance le dessin du fond `key` dans un fil à part (sauf s'il y est déjà). Faux si le
    /// fil n'a pas pu démarrer : l'appelant dessine alors sur place.
    fn start(&self, choice: &WallpaperChoice, key: Key, wake: Option<&Ping>) -> bool {
        let mut pending = self.pending.borrow_mut();
        if pending.as_ref().is_some_and(|(known, _)| *known == key) {
            return true;
        }
        let (sender, receiver) = mpsc::channel();
        let choice = choice.clone();
        let wake = wake.cloned();
        let started = std::thread::Builder::new()
            .name("fond d'écran".to_owned())
            .spawn(move || {
                let (width, height) = (key.0.0 as u32, key.0.1 as u32);
                let start = std::time::Instant::now();
                let pixels = bgra(pixels(&choice, width, height));
                tracing::debug!(
                    width,
                    height,
                    ms = start.elapsed().as_millis(),
                    "fond d'écran dessiné à part"
                );
                if sender.send(pixels).is_ok()
                    && let Some(wake) = wake
                {
                    wake.ping();
                }
            });
        match started {
            Ok(_) => {
                // Une demande plus ancienne est abandonnée : son résultat ne sera pas lu.
                *pending = Some((key, receiver));
                true
            }
            Err(err) => {
                tracing::warn!(%err, "fil du fond d'écran non démarré");
                false
            }
        }
    }
}

/// Texture opaque de `size` pixels à partir de pixels BGRA.
fn texture(pixels: &[u8], size: (i32, i32)) -> MemoryRenderBuffer {
    let opaque = vec![Rectangle::<i32, Buffer>::from_size(size.into())];
    MemoryRenderBuffer::from_slice(pixels, Fourcc::Argb8888, size, 1, Transform::Normal, Some(opaque))
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
        let (_, source) = Wallpaper::default().buffer(&WallpaperChoice::default(), 2880, 1800, None);
        assert_eq!(source.size, (2880.0, 1800.0).into());
        let image = pixels(&WallpaperChoice::default(), 64, 40);
        let pixels = bgra(image.clone());
        assert_eq!(pixels.len(), 64 * 40 * 4);
        assert_eq!([pixels[0], pixels[2]], [image.rgba[2], image.rgba[0]]);
    }

    #[test]
    fn nouvelle_taille_dessinee_a_part_l_ancien_fond_reste() {
        let wallpaper = Wallpaper::default();
        let choice = WallpaperChoice::default();
        // Premier fond : sur place.
        let (_, first) = wallpaper.buffer(&choice, 64, 40, None);
        assert_eq!(first.size, (64.0, 40.0).into());
        // Nouvelle taille : l'ancien fond reste affiché jusqu'à ce que le nouveau soit prêt.
        let start = std::time::Instant::now();
        loop {
            let (_, source) = wallpaper.buffer(&choice, 128, 80, None);
            if source.size == (128.0, 80.0).into() {
                break;
            }
            assert_eq!(source.size, (64.0, 40.0).into());
            assert!(
                start.elapsed() < std::time::Duration::from_secs(10),
                "fond jamais prêt"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(wallpaper.pending.borrow().is_none());
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
