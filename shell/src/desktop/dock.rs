//! Dock Ferrite : raccourcis d'applications centrés en bas de l'écran, avec un point
//! sous les applications ouvertes.
//!
//! Sa taille et son masquage se règlent dans Paramètres (`DockConfig`). Masqué, il glisse
//! sous le bord de l'écran et revient quand la souris touche le bas, avec la touche Super,
//! ou dès que plus rien ne le recouvre.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    time::{Duration, Instant},
};

use crate::desktop::{
    apps::App,
    svg::{IconCache, Style},
};
use ferrite_system::config::{DockHiding, DockSize};
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

const ICON_GAP: i32 = 6;
const PAD_X: i32 = 10;
const PAD_Y: i32 = 8;
const BOTTOM_MARGIN: i32 = 10;
const CORNER_RADIUS: f32 = 18.0;
const RUNNING_DOT: f32 = 4.0;
/// Hauteur de la bande du bas qui fait revenir un Dock masqué.
const REVEAL_EDGE: f64 = 3.0;
/// Durée du glissement du Dock.
const SLIDE: Duration = Duration::from_millis(180);

/// Réglages du Dock, lus dans `~/.config/ferrite/reglages`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DockConfig {
    pub size: DockSize,
    pub hiding: DockHiding,
}

impl DockConfig {
    fn icon(self) -> i32 {
        self.size.icon()
    }

    pub fn width(self) -> i32 {
        let count = App::ALL.len() as i32;
        2 * PAD_X + count * self.icon() + (count - 1) * ICON_GAP
    }

    pub fn height(self) -> i32 {
        self.icon() + 2 * PAD_Y
    }

    /// Bande du bas que les fenêtres laissent libre : seulement si le Dock ne se cache jamais.
    pub fn reserved_height(self) -> i32 {
        match self.hiding {
            DockHiding::Never => self.height() + BOTTOM_MARGIN + 6,
            DockHiding::Smart | DockHiding::Always => 0,
        }
    }
}

/// Ce qui décide si le Dock est visible.
#[derive(Debug, Clone, Copy)]
pub struct Visibility<'a> {
    pub config: DockConfig,
    pub output: Rectangle<i32, Logical>,
    pub pointer: Option<Point<f64, Logical>>,
    /// Cadres des fenêtres affichées (barre de titre comprise).
    pub windows: &'a [Rectangle<i32, Logical>],
    /// La fenêtre du dessus est en plein écran.
    pub fullscreen: bool,
    /// Le menu Applications est ouvert (touche Super, par exemple).
    pub launcher_open: bool,
}

/// Le Dock doit-il être affiché ?
pub fn should_show(v: Visibility<'_>) -> bool {
    let shown_rect = dock_rect(v.output, v.config, 1.0).to_f64();
    let pointer_calls = v.pointer.is_some_and(|pointer| {
        let bottom = (v.output.loc.y + v.output.size.h) as f64;
        pointer.y >= bottom - REVEAL_EDGE || shown_rect.contains(pointer)
    });
    if pointer_calls || v.launcher_open {
        return true;
    }
    if v.fullscreen {
        return false;
    }
    match v.config.hiding {
        DockHiding::Never => true,
        DockHiding::Always => false,
        DockHiding::Smart => {
            let area = dock_rect(v.output, v.config, 1.0);
            !v.windows.iter().any(|frame| frame.overlaps(area))
        }
    }
}

#[derive(Debug)]
pub struct Dock {
    icons: IconCache,
    /// Tampons déjà rendus, par échelle (en millièmes), applications ouvertes et taille.
    buffers: RefCell<BTreeMap<(u32, u8, i32), MemoryRenderBuffer>>,
    /// Part visible (0 caché, 1 sorti) et instant du dernier calcul, pour l'animation.
    reveal: Cell<(f64, Option<Instant>)>,
}

impl Dock {
    pub fn new() -> Self {
        Self {
            icons: IconCache::default(),
            buffers: RefCell::new(BTreeMap::new()),
            reveal: Cell::new((1.0, None)),
        }
    }

    /// Fait avancer le glissement vers `shown` et renvoie la part visible.
    fn animate(&self, shown: bool) -> f64 {
        let (reveal, last) = self.reveal.get();
        let now = Instant::now();
        let step = last.map_or(1.0, |last| {
            now.duration_since(last).as_secs_f64() / SLIDE.as_secs_f64()
        });
        let target = if shown { 1.0 } else { 0.0 };
        let reveal = if reveal < target {
            (reveal + step).min(target)
        } else {
            (reveal - step).max(target)
        };
        self.reveal.set((reveal, Some(now)));
        reveal
    }

    #[allow(clippy::too_many_arguments)] // Rendu, écran et état du Dock, sans état global.
    pub fn push_element(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        output_geo: Rectangle<i32, Logical>,
        scale: f64,
        running: &[App],
        config: DockConfig,
        shown: bool,
        out: &mut Vec<super::super::render::OutputElement>,
    ) {
        let reveal = self.animate(shown);
        if reveal <= 0.0 {
            return;
        }
        let mask = App::ALL
            .iter()
            .enumerate()
            .filter(|(_, app)| running.contains(app))
            .fold(0u8, |mask, (index, _)| mask | 1 << index);
        let key = ((scale * 1000.0).round() as u32, mask, config.icon());
        let mut buffers = self.buffers.borrow_mut();
        let buffer = buffers
            .entry(key)
            .or_insert_with(|| self.rasterize(scale, mask, config));
        let logical_position = dock_rect(output_geo, config, ease(reveal)).loc - output_geo.loc;
        let position = logical_position.to_physical_precise_round(scale);
        let (width, height) = (config.width(), config.height());
        let physical_size = (width as f64 * scale, height as f64 * scale);
        let source = Rectangle::<f64, Logical>::from_size(physical_size.into());
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            position,
            buffer,
            None,
            Some(source),
            Some((width, height).into()),
            Kind::Unspecified,
        ) {
            Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
            Err(err) => tracing::warn!(?err, output = output.name(), "dock non importé"),
        }
    }

    fn rasterize(&self, scale: f64, running: u8, config: DockConfig) -> MemoryRenderBuffer {
        let icon = config.icon();
        let width = (config.width() as f64 * scale).round().max(1.0) as u32;
        let height = (config.height() as f64 * scale).round().max(1.0) as u32;
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
            let x = (PAD_X + index as i32 * (icon + ICON_GAP)) as f32 * factor;
            let size = icon as f32 * factor;
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

/// Courbe du glissement : rapide au départ, douce à l'arrivée.
fn ease(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Position du Dock, sorti à `reveal` (0 : entièrement sous le bord, 1 : à sa place).
pub fn dock_rect(
    output: Rectangle<i32, Logical>,
    config: DockConfig,
    reveal: f64,
) -> Rectangle<i32, Logical> {
    let (width, height) = (config.width(), config.height());
    let hidden_by = ((height + BOTTOM_MARGIN) as f64 * (1.0 - reveal)).round() as i32;
    Rectangle::new(
        (
            output.loc.x + (output.size.w - width) / 2,
            output.loc.y + output.size.h - height - BOTTOM_MARGIN + hidden_by,
        )
            .into(),
        (width, height).into(),
    )
}

/// Application sous `position`, si le Dock est sorti.
pub fn app_at(
    output: Rectangle<i32, Logical>,
    config: DockConfig,
    position: Point<f64, Logical>,
) -> Option<App> {
    let rect = dock_rect(output, config, 1.0);
    if !rect.to_f64().contains(position) {
        return None;
    }
    let x = position.x.floor() as i32 - rect.loc.x - PAD_X;
    if x < 0 {
        return None;
    }
    let stride = config.icon() + ICON_GAP;
    let index = (x / stride) as usize;
    (index < App::ALL.len() && x % stride < config.icon()).then_some(App::ALL[index])
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

    fn medium() -> DockConfig {
        DockConfig::default()
    }

    #[test]
    fn dock_centre_et_laisse_une_marge_en_bas() {
        let dock = dock_rect(output(), medium(), 1.0);
        assert_eq!(dock.size, (304, 68).into());
        assert_eq!(dock.loc, Point::from((488, 642)));
        // Caché : entièrement sous le bord de l'écran.
        assert!(dock_rect(output(), medium(), 0.0).loc.y >= 720);
        let large = DockConfig {
            size: DockSize::Large,
            ..medium()
        };
        assert!(large.width() > medium().width());
    }

    #[test]
    fn clics_sur_les_icones_identifient_les_applications() {
        let dock = dock_rect(output(), medium(), 1.0);
        let at = |dx: f64| app_at(output(), medium(), Point::from((dock.loc.x as f64 + dx, 660.0)));
        assert_eq!(at(20.0), Some(App::Files));
        assert_eq!(at(80.0), Some(App::TextEdit));
        assert_eq!(at(140.0), Some(App::Terminal));
        assert_eq!(at(200.0), Some(App::Calculator));
        assert_eq!(at(260.0), Some(App::Settings));
        // Entre deux icônes : rien.
        assert_eq!(at(64.0), None);
    }

    #[test]
    fn masquage_intelligent_plein_ecran_et_retour_par_le_bas() {
        let far = [Rectangle::new((100, 100).into(), (400, 300).into())];
        let over = [Rectangle::new((300, 300).into(), (700, 400).into())];
        let base = Visibility {
            config: medium(),
            output: output(),
            pointer: Some(Point::from((640.0, 300.0))),
            windows: &far,
            fullscreen: false,
            launcher_open: false,
        };
        assert!(should_show(base));
        assert!(!should_show(Visibility {
            windows: &over,
            ..base
        }));
        assert!(!should_show(Visibility {
            fullscreen: true,
            ..base
        }));
        // La souris en bas de l'écran le fait revenir, même en plein écran.
        let bottom = Some(Point::from((640.0, 719.0)));
        assert!(should_show(Visibility {
            windows: &over,
            fullscreen: true,
            pointer: bottom,
            ..base
        }));
        // La touche Super (menu Applications) aussi.
        assert!(should_show(Visibility {
            windows: &over,
            launcher_open: true,
            ..base
        }));
        // « Jamais » : toujours visible hors plein écran ; « Toujours » : caché.
        let never = DockConfig {
            hiding: DockHiding::Never,
            ..medium()
        };
        assert!(should_show(Visibility {
            config: never,
            windows: &over,
            ..base
        }));
        assert_eq!(never.reserved_height(), 84);
        assert_eq!(medium().reserved_height(), 0);
        let always = DockConfig {
            hiding: DockHiding::Always,
            ..medium()
        };
        assert!(!should_show(Visibility {
            config: always,
            ..base
        }));
    }
}
