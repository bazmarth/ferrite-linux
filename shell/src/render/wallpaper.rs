//! Fond d'écran : pour l'instant le dégradé du bureau Ferrite d'origine. Les images
//! (`assets/wallpapers`) viendront avec les Réglages (phase 3).

use ferrite_ui::theme::{WALLPAPER_GRADIENT, gradient_at};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::{Buffer, Logical, Rectangle, Transform},
};

/// Hauteur de la texture du dégradé ; elle est étirée à la taille de l'écran avec un
/// filtrage linéaire, ce qui donne un dégradé continu.
const GRADIENT_STEPS: i32 = 256;

#[derive(Debug)]
pub struct Wallpaper {
    buffer: MemoryRenderBuffer,
}

impl Wallpaper {
    pub fn gradient() -> Wallpaper {
        let pixels = gradient_pixels(GRADIENT_STEPS as usize);
        let opaque = vec![Rectangle::<i32, Buffer>::from_size((1, GRADIENT_STEPS).into())];
        let buffer = MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Argb8888,
            (1, GRADIENT_STEPS),
            1,
            Transform::Normal,
            Some(opaque),
        );
        Wallpaper { buffer }
    }

    pub fn buffer(&self) -> &MemoryRenderBuffer {
        &self.buffer
    }

    /// Zone de la texture à dessiner : la texture entière.
    pub fn source(&self) -> Rectangle<f64, Logical> {
        Rectangle::from_size((1.0, GRADIENT_STEPS as f64).into())
    }
}

/// Colonne de `height` pixels `ARGB8888` (petit-boutiste : B, V, R, A en mémoire).
fn gradient_pixels(height: usize) -> Vec<u8> {
    let last = height.saturating_sub(1).max(1) as f32;
    (0..height)
        .flat_map(|y| {
            gradient_at(&WALLPAPER_GRADIENT, y as f32 / last)
                .to_argb8888_premultiplied()
                .to_le_bytes()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colonne_du_degrade() {
        let pixels = gradient_pixels(3);
        assert_eq!(pixels.len(), 12);
        // Haut : 0x1A2A6C opaque, stocké B, V, R, A.
        assert_eq!(&pixels[0..4], &[0x6C, 0x2A, 0x1A, 0xFF]);
        // Bas : 0xF29A7E.
        assert_eq!(&pixels[8..12], &[0x7E, 0x9A, 0xF2, 0xFF]);
    }
}
