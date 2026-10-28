//! Fond d'écran Ferrite, repris du bureau d'origine (`D:\Os\kernel\src\gui.rs`,
//! `paint_wallpaper`) : dégradé du bleu nuit au corail, halo en haut à droite et
//! collines ondulées translucides. Dessiné à la résolution réelle de l'écran (net sur
//! un écran 2880×1800 comme en 1280×800).
use std::{cell::RefCell, f32::consts::TAU};

use ferrite_ui::theme::{Color, WALLPAPER_GRADIENT, gradient_at};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::{Buffer, Logical, Rectangle, Transform},
};

/// Halo lumineux du haut à droite.
const GLOW: Color = Color::rgb(0xFFC7A8);

/// Une colline : couleur, opacité (0 à 1), hauteur de base, amplitude (fractions de la
/// hauteur de l'écran), nombre d'ondulations sur la largeur, et décalage (en 60es de tour).
struct Wave {
    color: Color,
    opacity: f32,
    base: f32,
    amplitude: f32,
    frequency: f32,
    phase: f32,
}

const WAVES: [Wave; 4] = [
    Wave {
        color: Color::rgb(0xFFB49C),
        opacity: 70.0 / 255.0,
        base: 0.58,
        amplitude: 0.06,
        frequency: 1.0,
        phase: 0.0,
    },
    Wave {
        color: Color::rgb(0xE66A8A),
        opacity: 90.0 / 255.0,
        base: 0.69,
        amplitude: 0.045,
        frequency: 2.0,
        phase: 20.0,
    },
    Wave {
        color: Color::rgb(0x4A2F86),
        opacity: 120.0 / 255.0,
        base: 0.82,
        amplitude: 0.03,
        frequency: 1.0,
        phase: 40.0,
    },
    Wave {
        color: Color::rgb(0x24185A),
        opacity: 140.0 / 255.0,
        base: 0.92,
        amplitude: 0.02,
        frequency: 3.0,
        phase: 10.0,
    },
];

/// Fond d'écran, dessiné une fois par taille d'écran (en pixels physiques).
#[derive(Debug, Default)]
pub struct Wallpaper {
    cached: RefCell<Option<((i32, i32), MemoryRenderBuffer)>>,
}

impl Wallpaper {
    pub fn waves() -> Wallpaper {
        Wallpaper::default()
    }

    /// Texture à la taille physique de l'écran, et la zone à en dessiner.
    pub fn buffer(&self, width: i32, height: i32) -> (MemoryRenderBuffer, Rectangle<f64, Logical>) {
        let size = (width.max(1), height.max(1));
        let mut cached = self.cached.borrow_mut();
        if cached.as_ref().is_none_or(|(known, _)| *known != size) {
            let pixels = waves_pixels(size.0, size.1);
            let opaque = vec![Rectangle::<i32, Buffer>::from_size(size.into())];
            let buffer = MemoryRenderBuffer::from_slice(
                &pixels,
                Fourcc::Argb8888,
                size,
                1,
                Transform::Normal,
                Some(opaque),
            );
            *cached = Some((size, buffer));
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

/// Fond à `width` × `height` pixels. Le bord de chaque colline est anticrénelé : un
/// pixel qu'il traverse n'en reçoit que la part qu'il couvre.
fn waves_pixels(width: i32, height: i32) -> Vec<u8> {
    let (w, h) = (width as f32, height as f32);
    // Bord de chaque colline, colonne par colonne (en pixels depuis le haut).
    let edges: Vec<Vec<f32>> = WAVES
        .iter()
        .map(|wave| {
            (0..width)
                .map(|x| {
                    let turns = x as f32 * wave.frequency / w + wave.phase / 60.0;
                    (wave.base + wave.amplitude * (turns * TAU).sin()) * h
                })
                .collect()
        })
        .collect();
    let (glow_x, glow_y, glow_r) = (w * 0.72, h * 0.18, h * 0.60);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let base = gradient_at(&WALLPAPER_GRADIENT, y as f32 / (h - 1.0).max(1.0));
        for x in 0..width {
            let mut color = [base.r, base.g, base.b].map(f32::from);
            let (dx, dy) = (x as f32 - glow_x, y as f32 - glow_y);
            let d2 = (dx * dx + dy * dy) / (glow_r * glow_r);
            if d2 < 1.0 {
                let a = 60.0 * (1.0 - d2);
                blend(&mut color, GLOW, a * a / 60.0 / 255.0);
            }
            for (wave, edge) in WAVES.iter().zip(&edges) {
                let coverage = (y as f32 + 1.0 - edge[x as usize]).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    blend(&mut color, wave.color, wave.opacity * coverage);
                }
            }
            let [r, g, b] = color.map(|channel| channel.round().clamp(0.0, 255.0) as u8);
            pixels.extend([b, g, r, 255]);
        }
    }
    pixels
}

fn blend(color: &mut [f32; 3], over: Color, amount: f32) {
    for (channel, value) in color.iter_mut().zip([over.r, over.g, over.b]) {
        *channel += (f32::from(value) - *channel) * amount;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fond_opaque_aux_dimensions_attendues() {
        let pixels = waves_pixels(1280, 800);
        assert_eq!(pixels.len(), 1280 * 800 * 4);
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        // Écran du ThinkBook : texture à sa résolution réelle.
        let (_, source) = Wallpaper::waves().buffer(2880, 1800);
        assert_eq!(source.size, (2880.0, 1800.0).into());
    }

    #[test]
    fn degrade_puis_collines() {
        let pixels = waves_pixels(400, 300);
        let at = |x: usize, y: usize| {
            let i = (y * 400 + x) * 4;
            [pixels[i + 2], pixels[i + 1], pixels[i]]
        };
        // En haut à gauche : le bleu nuit du dégradé, loin du halo.
        let top = WALLPAPER_GRADIENT[0];
        let [r, g, b] = at(0, 0);
        assert!(r.abs_diff(top.r) < 6 && g.abs_diff(top.g) < 6 && b.abs_diff(top.b) < 6);
        // En bas : la dernière colline, sombre, recouvre le corail du dégradé.
        let [r, _, b] = at(200, 299);
        assert!(r < 0xC0 && b > 0x50);
    }
}
