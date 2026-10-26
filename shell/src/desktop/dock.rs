//! Dock Ferrite : raccourcis d'applications centrés en bas de l'écran, avec un point
//! sous les applications ouvertes.

use std::{cell::RefCell, collections::BTreeMap};

use crate::desktop::{
    apps::App,
    svg::{IconCache, Style},
};
use ferrite_ui::theme::Color;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            },
            gles::GlesRenderer,
        },
    },
    output::Output,
    utils::{Logical, Point, Rectangle},
};

const ICON_SIZE: i32 = 52;
const ICON_GAP: i32 = 6;
const PAD_X: i32 = 10;
const PAD_Y: i32 = 8;
const DOCK_HEIGHT: i32 = ICON_SIZE + 2 * PAD_Y;
const DOCK_WIDTH: i32 =
    2 * PAD_X + App::ALL.len() as i32 * ICON_SIZE + (App::ALL.len() as i32 - 1) * ICON_GAP;
const BOTTOM_MARGIN: i32 = 10;
/// Bande du bas réservée au Dock, que les fenêtres placées ou agrandies laissent libre.
pub const RESERVED_HEIGHT: i32 = DOCK_HEIGHT + BOTTOM_MARGIN + 6;
const CORNER_RADIUS: f32 = 18.0;
const RUNNING_DOT: f32 = 4.0;

#[derive(Debug)]
pub struct Dock {
    icons: IconCache,
    /// Tampons déjà rendus, par échelle (en millièmes) et applications ouvertes.
    buffers: RefCell<BTreeMap<(u32, u8), MemoryRenderBuffer>>,
}

impl Dock {
    pub fn new() -> Self {
        Self {
            icons: IconCache::default(),
            buffers: RefCell::new(BTreeMap::new()),
        }
    }

    pub fn push_element(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        output_geo: Rectangle<i32, Logical>,
        scale: f64,
        running: &[App],
        out: &mut Vec<super::super::render::OutputElement>,
    ) {
        let mask = App::ALL
            .iter()
            .enumerate()
            .filter(|(_, app)| running.contains(app))
            .fold(0u8, |mask, (index, _)| mask | 1 << index);
        let key = ((scale * 1000.0).round() as u32, mask);
        let mut buffers = self.buffers.borrow_mut();
        let buffer = buffers.entry(key).or_insert_with(|| self.rasterize(scale, mask));
        let logical_position = dock_rect(output_geo).loc - output_geo.loc;
        let position = logical_position.to_physical_precise_round(scale);
        let physical_size = (DOCK_WIDTH as f64 * scale, DOCK_HEIGHT as f64 * scale);
        let source = Rectangle::<f64, Logical>::from_size(physical_size.into());
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            position,
            buffer,
            None,
            Some(source),
            Some((DOCK_WIDTH, DOCK_HEIGHT).into()),
            Kind::Unspecified,
        ) {
            Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
            Err(err) => tracing::warn!(?err, output = output.name(), "dock non importé"),
        }
    }

    fn rasterize(&self, scale: f64, running: u8) -> MemoryRenderBuffer {
        let width = (DOCK_WIDTH as f64 * scale).round().max(1.0) as u32;
        let height = (DOCK_HEIGHT as f64 * scale).round().max(1.0) as u32;
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let factor = scale as f32;
        let (w, h) = (width as f32, height as f32);
        // Verre clair : contour discret, fond translucide et reflet sur le bord haut.
        let radius = CORNER_RADIUS * factor;
        fill_rounded(
            &mut pixels,
            width,
            height,
            0.0,
            0.0,
            w,
            h,
            radius,
            Color::BLACK.with_alpha(40),
        );
        fill_rounded(
            &mut pixels,
            width,
            height,
            factor,
            factor,
            w - 2.0 * factor,
            h - 2.0 * factor,
            radius - factor,
            Color::rgb(0xF4F6FA).with_alpha(150),
        );
        fill_rounded(
            &mut pixels,
            width,
            height,
            radius,
            factor,
            w - 2.0 * radius,
            factor,
            0.0,
            Color::WHITE.with_alpha(150),
        );
        for (index, app) in App::ALL.into_iter().enumerate() {
            let x = (PAD_X + index as i32 * (ICON_SIZE + ICON_GAP)) as f32 * factor;
            let size = ICON_SIZE as f32 * factor;
            self.icons.draw(
                &mut pixels,
                width,
                height,
                app.icon(),
                x,
                PAD_Y as f32 * factor - 2.0 * factor,
                size,
                Style::Color,
            );
            if running & (1 << index) != 0 {
                let dot = RUNNING_DOT * factor;
                fill_rounded(
                    &mut pixels,
                    width,
                    height,
                    x + (size - dot) / 2.0,
                    h - 3.0 * factor - dot,
                    dot,
                    dot,
                    dot / 2.0,
                    Color::rgb(0x333A45).with_alpha(220),
                );
            }
        }
        MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Argb8888,
            (width as i32, height as i32),
            1,
            smithay::utils::Transform::Normal,
            None,
        )
    }
}

/// Zone interactive du Dock, exprimée dans l'espace global de l'écran.
pub fn dock_rect(output: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    Rectangle::new(
        (
            output.loc.x + (output.size.w - DOCK_WIDTH) / 2,
            output.loc.y + output.size.h - DOCK_HEIGHT - BOTTOM_MARGIN,
        )
            .into(),
        (DOCK_WIDTH, DOCK_HEIGHT).into(),
    )
}

pub fn app_at(output: Rectangle<i32, Logical>, position: Point<f64, Logical>) -> Option<App> {
    let rect = dock_rect(output);
    if !rect.to_f64().contains(position) {
        return None;
    }
    let x = position.x.floor() as i32 - rect.loc.x - PAD_X;
    if x < 0 {
        return None;
    }
    let stride = ICON_SIZE + ICON_GAP;
    let index = (x / stride) as usize;
    (index < App::ALL.len() && x % stride < ICON_SIZE).then_some(App::ALL[index])
}

#[allow(clippy::too_many_arguments)] // Rectangle, rayon et pixels sont transmis sans état global.
fn fill_rounded(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    rect_width: f32,
    rect_height: f32,
    radius: f32,
    color: Color,
) {
    let left = (x - 1.0).floor().max(0.0) as u32;
    let top = (y - 1.0).floor().max(0.0) as u32;
    let right = (x + rect_width + 1.0).ceil().min(width as f32) as u32;
    let bottom = (y + rect_height + 1.0).ceil().min(height as f32) as u32;
    let radius = radius.clamp(0.0, rect_width.min(rect_height) * 0.5);
    for py in top..bottom {
        for px in left..right {
            let qx = (px as f32 + 0.5 - (x + rect_width * 0.5)).abs() - (rect_width * 0.5 - radius);
            let qy = (py as f32 + 0.5 - (y + rect_height * 0.5)).abs() - (rect_height * 0.5 - radius);
            let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
            put_pixel_coverage(pixels, width, px, py, color, (0.5 - distance).clamp(0.0, 1.0));
        }
    }
}

fn put_pixel_coverage(pixels: &mut [u8], width: u32, x: u32, y: u32, color: Color, coverage: f32) {
    let alpha = (color.a as f32 * coverage).round().clamp(0.0, 255.0) as u32;
    let inverse = 255 - alpha;
    let offset = ((y * width + x) * 4) as usize;
    let source = [color.b, color.g, color.r].map(|channel| channel as u32 * alpha / 255);
    for (index, channel) in source.into_iter().enumerate() {
        pixels[offset + index] = (channel + pixels[offset + index] as u32 * inverse / 255).min(255) as u8;
    }
    pixels[offset + 3] = (alpha + pixels[offset + 3] as u32 * inverse / 255).min(255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output() -> Rectangle<i32, Logical> {
        Rectangle::new((0, 0).into(), (1280, 720).into())
    }

    #[test]
    fn dock_centre_et_laisse_une_marge_en_bas() {
        let dock = dock_rect(output());
        assert_eq!(dock.size, (246, 68).into());
        assert_eq!(dock.loc, Point::from((517, 642)));
    }

    #[test]
    fn clics_sur_les_icones_identifient_les_applications() {
        let dock = dock_rect(output());
        let at = |dx: f64| app_at(output(), Point::from((dock.loc.x as f64 + dx, 660.0)));
        assert_eq!(at(20.0), Some(App::Files));
        assert_eq!(at(80.0), Some(App::TextEdit));
        assert_eq!(at(140.0), Some(App::Terminal));
        assert_eq!(at(200.0), Some(App::Calculator));
        // Entre deux icônes : rien.
        assert_eq!(at(64.0), None);
    }
}
