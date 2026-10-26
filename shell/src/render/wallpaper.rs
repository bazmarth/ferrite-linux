//! Fond d'écran Ferrite : montagnes et lac au coucher du soleil.
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::{Buffer, Logical, Rectangle, Transform},
};

const WIDTH: i32 = 1280;
const HEIGHT: i32 = 800;
const SUPER_SAMPLE: f32 = 2.0;

#[derive(Debug)]
pub struct Wallpaper {
    buffer: MemoryRenderBuffer,
}

impl Wallpaper {
    pub fn landscape() -> Wallpaper {
        let pixels = landscape_pixels();
        let opaque = vec![Rectangle::<i32, Buffer>::from_size((WIDTH, HEIGHT).into())];
        let buffer = MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Argb8888,
            (WIDTH, HEIGHT),
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
        Rectangle::from_size((WIDTH as f64, HEIGHT as f64).into())
    }
}

/// Fond 2× pour les écrans QEMU et les panneaux HiDPI. Les polygones sont
/// évalués en quatre sous-pixels pour garder les crêtes et le soleil nets.
fn landscape_pixels() -> Vec<u8> {
    let far = [
        (0, 250),
        (55, 205),
        (104, 222),
        (205, 128),
        (277, 203),
        (356, 160),
        (433, 218),
        (510, 174),
        (640, 227),
        (640, 286),
        (0, 286),
    ];
    let near = [
        (0, 268),
        (64, 218),
        (130, 259),
        (225, 190),
        (296, 253),
        (372, 221),
        (450, 266),
        (520, 204),
        (585, 250),
        (640, 218),
        (640, 305),
        (0, 305),
    ];
    let snow = [
        [(158, 195), (205, 128), (239, 183)],
        [(331, 178), (356, 160), (382, 192)],
        [(493, 219), (520, 204), (544, 235)],
    ];
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let mut accumulated = [0u32; 3];
            for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let scene_x = (x as f32 + sx) / SUPER_SAMPLE;
                let scene_y = (y as f32 + sy) / SUPER_SAMPLE;
                let color = landscape_color(scene_x, scene_y, &far, &near, &snow);
                for channel in 0..3 {
                    accumulated[channel] += color[channel] as u32;
                }
            }
            let color = accumulated.map(|channel| ((channel + 2) / 4) as u8);
            pixels.extend([color[2], color[1], color[0], 255]);
        }
    }
    pixels
}

fn landscape_color(
    x: f32,
    y: f32,
    far: &[(i32, i32)],
    near: &[(i32, i32)],
    snow: &[[(i32, i32); 3]],
) -> [u8; 3] {
    let mut color = if y < 252.0 {
        mix([105, 151, 196], [255, 218, 192], y / 252.0)
    } else {
        mix([112, 146, 177], [24, 49, 82], (y - 252.0) / (400.0 - 252.0))
    };
    let sun = (((x - 472.0) / 62.0).powi(2) + ((y - 245.0) / 48.0).powi(2)).sqrt();
    if sun < 1.0 {
        color = mix(color, [255, 245, 221], (1.0 - sun) * 0.92);
    } else if sun < 3.0 {
        color = mix(color, [255, 204, 175], (3.0 - sun) * 0.08);
    }
    if inside(x, y, far) {
        color = [101, 130, 158];
    }
    if inside(x, y, near) {
        color = [55, 83, 112];
    }
    if snow.iter().any(|triangle| inside(x, y, triangle)) {
        color = [206, 220, 230];
    }
    if y >= 252.0 {
        let distance = (x - 472.0).abs();
        let reflection = (1.0 - distance / (20.0 + (y - 252.0) * 0.45)).clamp(0.0, 1.0);
        let shimmer = ((y * 0.22 + x * 0.02).sin() * 0.5 + 0.5) * 0.08;
        color = mix(color, [255, 213, 178], reflection * (0.13 + shimmer));
        if ((y as i32 + (x as i32 / 8) % 4) % 19 == 0) && distance < 90.0 {
            color = mix(color, [194, 208, 220], 0.16);
        }
    }
    color
}

fn inside(x: f32, y: f32, polygon: &[(i32, i32)]) -> bool {
    let mut hit = false;
    let mut previous = polygon.len() - 1;
    for current in 0..polygon.len() {
        let (xi, yi) = polygon[current];
        let (xj, yj) = polygon[previous];
        if (yi as f32 > y) != (yj as f32 > y)
            && x < (xj - xi) as f32 * (y - yi as f32) / (yj - yi) as f32 + xi as f32
        {
            hit = !hit;
        }
        previous = current;
    }
    hit
}

fn mix(a: [u8; 3], b: [u8; 3], amount: f32) -> [u8; 3] {
    let amount = amount.clamp(0.0, 1.0);
    std::array::from_fn(|index| {
        (a[index] as f32 + (b[index] as f32 - a[index] as f32) * amount).round() as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paysage_opaque_aux_dimensions_attendues() {
        let pixels = landscape_pixels();
        assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert!(inside(205.0, 150.0, &[(0, 250), (205, 128), (300, 250)]));
        assert!(!inside(5.0, 5.0, &[(0, 250), (205, 128), (300, 250)]));
    }
}
