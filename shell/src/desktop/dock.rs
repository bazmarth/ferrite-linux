//! Dock Ferrite, comme sur macOS : les applications épinglées, puis (après un trait) les
//! applications ouvertes qui ne le sont pas ; un point sous celles qui ont une fenêtre.
//! Sous la souris, les icônes grossissent un peu, sur place ; le fond garde sa taille.
//!
//! Sa taille, son masquage et l'agrandissement se règlent dans Paramètres (`DockConfig`).
//! Masqué, il glisse sous le bord de l'écran et revient quand la souris touche le bas,
//! avec la touche Super, ou dès que plus rien ne le recouvre.
//!
//! La géométrie (`layout`) sert à la fois au dessin et aux clics : une icône agrandie se
//! clique là où on la voit. Le fond est dessiné par un shader, chaque icône par sa propre
//! texture (elle change de taille à chaque image pendant l'agrandissement).

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    f64::consts::PI,
    time::{Duration, Instant},
};

use crate::desktop::{
    apps::{AppIcon, AppKey},
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
            gles::{GlesPixelProgram, GlesRenderer, Uniform, element::PixelShaderElement},
        },
    },
    output::Output,
    utils::{Logical, Point, Rectangle, Size},
};

const ICON_GAP: f64 = 6.0;
const PAD_X: f64 = 10.0;
const PAD_Y: f64 = 8.0;
const BOTTOM_MARGIN: i32 = 10;
const RUNNING_DOT: f64 = 4.0;
/// Largeur du trait entre applications épinglées et autres applications ouvertes.
const SEPARATOR: f64 = 13.0;
/// Hauteur de la bande du bas qui fait revenir un Dock masqué.
const REVEAL_EDGE: f64 = 3.0;
/// Durée du glissement du Dock.
const SLIDE: Duration = Duration::from_millis(180);
/// Agrandissement maximal d'une icône sous la souris, et portée de l'effet (en icônes).
const MAGNIFY: f64 = 1.22;
const MAGNIFY_RANGE: f64 = 1.6;
/// Durée de l'entrée et de la sortie de l'agrandissement.
const MAGNIFY_EASE: Duration = Duration::from_millis(140);

/// Réglages du Dock et de la barre du haut, lus dans `~/.config/ferrite/reglages`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockConfig {
    pub size: DockSize,
    pub hiding: DockHiding,
    /// Masquage de la barre du haut.
    pub bar: DockHiding,
    /// Agrandissement des icônes sous la souris.
    pub magnify: bool,
}

impl Default for DockConfig {
    fn default() -> Self {
        Self {
            size: DockSize::default(),
            hiding: DockHiding::default(),
            bar: DockHiding::Never,
            magnify: true,
        }
    }
}

impl DockConfig {
    /// Bande du haut que les fenêtres laissent libre : seulement si la barre ne se cache
    /// jamais (hors plein écran).
    pub fn bar_reserved_height(self) -> i32 {
        match self.bar {
            DockHiding::Never => ferrite_ui::theme::MENUBAR_HEIGHT as i32,
            DockHiding::Smart | DockHiding::Always => 0,
        }
    }

    fn icon(self) -> f64 {
        self.size.icon() as f64
    }

    pub fn height(self) -> i32 {
        (self.icon() + 2.0 * PAD_Y) as i32
    }

    /// Bande du bas que les fenêtres laissent libre : seulement si le Dock ne se cache jamais.
    pub fn reserved_height(self) -> i32 {
        match self.hiding {
            DockHiding::Never => self.height() + BOTTOM_MARGIN + 6,
            DockHiding::Smart | DockHiding::Always => 0,
        }
    }
}

/// Une case du Dock.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DockItem {
    pub key: AppKey,
    pub icon: AppIcon,
    pub pinned: bool,
    /// Au moins une fenêtre, visible ou réduite.
    pub running: bool,
}

/// Géométrie du Dock à un instant donné, en pixels logiques de l'écran.
#[derive(Debug, Clone, PartialEq)]
pub struct DockLayout {
    /// Fond du Dock.
    pub plate: Rectangle<f64, Logical>,
    /// Icônes, dans l'ordre des cases (carrés, posés sur le bas du fond).
    pub icons: Vec<Rectangle<f64, Logical>>,
    /// Trait entre applications épinglées et autres.
    pub separator: Option<f64>,
}

/// Place les icônes ; `zoom` (0 à 1) dose l'agrandissement autour de `pointer`.
pub fn layout(
    output: Rectangle<i32, Logical>,
    config: DockConfig,
    items: &[DockItem],
    reveal: f64,
    pointer: Option<Point<f64, Logical>>,
    zoom: f64,
) -> DockLayout {
    let icon = config.icon();
    let count = items.len();
    let pinned = items.iter().take_while(|item| item.pinned).count();
    let separator_after = (pinned > 0 && pinned < count).then_some(pinned);
    // Centres des icônes sans agrandissement, relatifs au bord gauche du fond.
    let mut base = Vec::with_capacity(count);
    let mut x = PAD_X;
    for index in 0..count {
        if separator_after == Some(index) {
            x += SEPARATOR;
        }
        base.push(x + icon / 2.0);
        x += icon + ICON_GAP;
    }
    let base_width = (x - ICON_GAP + PAD_X).max(2.0 * PAD_X + icon);
    let left = output.loc.x as f64 + (output.size.w as f64 - base_width) / 2.0;
    let height = icon + 2.0 * PAD_Y;
    let hidden_by = (height + BOTTOM_MARGIN as f64) * (1.0 - reveal);
    let top = output.loc.y as f64 + output.size.h as f64 - height - BOTTOM_MARGIN as f64 + hidden_by;

    // Agrandissement : chaque icône selon sa distance au pointeur (en largeurs d'icône).
    let scales: Vec<f64> = base
        .iter()
        .map(|center| match pointer {
            Some(pointer) if zoom > 0.0 => {
                let distance = (pointer.x - (left + center)).abs() / (icon + ICON_GAP);
                let weight = if distance < MAGNIFY_RANGE {
                    (1.0 + (PI * distance / MAGNIFY_RANGE).cos()) / 2.0
                } else {
                    0.0
                };
                1.0 + (MAGNIFY - 1.0) * zoom * weight
            }
            _ => 1.0,
        })
        .collect();
    // Le fond garde sa taille ; chaque icône grandit sur place, centrée sur sa case et
    // posée sur le bas du Dock (elle déborde vers le haut).
    let bottom = top + height - PAD_Y + 2.0;
    let icons = base
        .iter()
        .zip(&scales)
        .map(|(center, scale)| {
            let size = icon * scale;
            Rectangle::new(
                Point::from((left + center - size / 2.0, bottom - size)),
                Size::from((size, size)),
            )
        })
        .collect();
    let separator =
        separator_after.map(|index| left + base[index] - icon / 2.0 - SEPARATOR / 2.0 - ICON_GAP / 2.0);
    DockLayout {
        plate: Rectangle::new(Point::from((left, top)), Size::from((base_width, height))),
        icons,
        separator,
    }
}

impl DockLayout {
    /// Case sous `position` : l'icône, ou la colonne du fond sous elle.
    pub fn item_at(&self, position: Point<f64, Logical>) -> Option<usize> {
        let plate = self.plate;
        let in_plate = plate.contains(position);
        self.icons.iter().position(|icon| {
            let column = position.x >= icon.loc.x - ICON_GAP / 2.0
                && position.x < icon.loc.x + icon.size.w + ICON_GAP / 2.0;
            column && (in_plate || (position.y >= icon.loc.y && position.y < plate.loc.y + plate.size.h))
        })
    }

    /// Zone où la souris agrandit les icônes : le fond, et au-dessus la hauteur des
    /// icônes agrandies.
    fn hover_area(&self, icon: f64) -> Rectangle<f64, Logical> {
        let rise = icon * (MAGNIFY - 1.0);
        Rectangle::new(
            Point::from((self.plate.loc.x, self.plate.loc.y - rise)),
            Size::from((self.plate.size.w, self.plate.size.h + rise + BOTTOM_MARGIN as f64)),
        )
    }
}

/// Ce qui décide si le Dock est visible.
#[derive(Debug, Clone, Copy)]
pub struct Visibility<'a> {
    pub config: DockConfig,
    pub output: Rectangle<i32, Logical>,
    /// Fond du Dock sorti, sans agrandissement.
    pub dock: Rectangle<f64, Logical>,
    pub pointer: Option<Point<f64, Logical>>,
    /// Cadres des fenêtres affichées (barre de titre comprise).
    pub windows: &'a [Rectangle<i32, Logical>],
    /// La fenêtre du dessus est en plein écran.
    pub fullscreen: bool,
    /// Le menu Applications ou le menu d'une icône est ouvert.
    pub launcher_open: bool,
}

/// Le Dock doit-il être affiché ?
pub fn should_show(v: Visibility<'_>) -> bool {
    let pointer_calls = v.pointer.is_some_and(|pointer| {
        let bottom = (v.output.loc.y + v.output.size.h) as f64;
        pointer.y >= bottom - REVEAL_EDGE || v.dock.contains(pointer)
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
            let area = v.dock.to_i32_round();
            !v.windows.iter().any(|frame| frame.overlaps(area))
        }
    }
}

/// Glissement et agrandissement du Dock, partagés par le dessin et les clics.
#[derive(Debug)]
pub struct DockMotion {
    /// Part visible (0 caché, 1 sorti) et instant du dernier calcul.
    reveal: Cell<(f64, Option<Instant>)>,
    /// Agrandissement en cours (0 à 1) et instant du dernier calcul.
    zoom: Cell<(f64, Option<Instant>)>,
}

impl Default for DockMotion {
    fn default() -> Self {
        Self {
            reveal: Cell::new((1.0, None)),
            zoom: Cell::new((0.0, None)),
        }
    }
}

impl DockMotion {
    /// Fait avancer `value` vers `target` à la vitesse d'une course complète en `duration`.
    fn step(cell: &Cell<(f64, Option<Instant>)>, target: f64, duration: Duration) -> f64 {
        let (value, last) = cell.get();
        let now = Instant::now();
        let step = last.map_or(1.0, |last| {
            now.duration_since(last).as_secs_f64() / duration.as_secs_f64()
        });
        let value = if value < target {
            (value + step).min(target)
        } else {
            (value - step).max(target)
        };
        cell.set((value, Some(now)));
        value
    }

    /// Géométrie de cette image : glissement et agrandissement avancés d'un pas. Renvoie
    /// aussi la part visible.
    pub fn frame_layout(
        &self,
        output: Rectangle<i32, Logical>,
        config: DockConfig,
        items: &[DockItem],
        shown: bool,
        pointer: Option<Point<f64, Logical>>,
    ) -> (DockLayout, f64) {
        let reveal = ease(Self::step(&self.reveal, if shown { 1.0 } else { 0.0 }, SLIDE));
        let flat = layout(output, config, items, reveal, None, 0.0);
        let hovered = config.magnify
            && reveal > 0.99
            && pointer.is_some_and(|pointer| flat.hover_area(config.icon()).contains(pointer));
        let zoom = Self::step(&self.zoom, if hovered { 1.0 } else { 0.0 }, MAGNIFY_EASE);
        (layout(output, config, items, reveal, pointer, ease(zoom)), reveal)
    }

    /// Géométrie actuelle, sans faire avancer les animations (clics, cible d'une fenêtre
    /// réduite).
    pub fn current_layout(
        &self,
        output: Rectangle<i32, Logical>,
        config: DockConfig,
        items: &[DockItem],
        pointer: Option<Point<f64, Logical>>,
    ) -> DockLayout {
        let reveal = ease(self.reveal.get().0);
        layout(output, config, items, reveal, pointer, ease(self.zoom.get().0))
    }
}

/// Textures du Dock, liées au moteur de rendu.
#[derive(Debug)]
pub struct Dock {
    icons: IconCache,
    /// Textures des icônes, par icône et côté en pixels.
    textures: RefCell<HashMap<(AppIcon, u32), MemoryRenderBuffer>>,
    /// Point des applications ouvertes et trait de séparation, par échelle.
    marks: RefCell<HashMap<u32, (MemoryRenderBuffer, MemoryRenderBuffer)>>,
    plate: RefCell<Option<(Rectangle<i32, Logical>, PixelShaderElement)>>,
}

impl Dock {
    pub fn new() -> Self {
        Self {
            icons: IconCache::default(),
            textures: RefCell::new(HashMap::new()),
            marks: RefCell::new(HashMap::new()),
            plate: RefCell::new(None),
        }
    }

    #[allow(clippy::too_many_arguments)] // Rendu, écran et Dock, sans état global.
    pub fn push_elements(
        &self,
        renderer: &mut GlesRenderer,
        program: &GlesPixelProgram,
        output: &Output,
        output_geo: Rectangle<i32, Logical>,
        scale: f64,
        items: &[DockItem],
        config: DockConfig,
        geometry: &DockLayout,
        out: &mut Vec<super::super::render::OutputElement>,
    ) {
        let local =
            |rect: Rectangle<f64, Logical>| Rectangle::new(rect.loc - output_geo.loc.to_f64(), rect.size);
        let mut push = |buffer: &MemoryRenderBuffer, rect: Rectangle<f64, Logical>, source: (f64, f64)| {
            let rect = local(rect);
            let position = rect.loc.to_physical(scale);
            let size = Size::from((
                rect.size.w.round().max(1.0) as i32,
                rect.size.h.round().max(1.0) as i32,
            ));
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                (position.x.round(), position.y.round()),
                buffer,
                None,
                Some(Rectangle::from_size(source.into())),
                Some(size),
                Kind::Unspecified,
            ) {
                Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
                Err(err) => tracing::warn!(?err, output = output.name(), "icône du Dock non importée"),
            }
        };

        // Icônes nettes même agrandies : texture à la taille maximale.
        let side = (config.icon() * MAGNIFY * scale).round() as u32;
        let (dot, line) = self.marks(scale);
        for (item, rect) in items.iter().zip(&geometry.icons) {
            if item.running {
                let size = RUNNING_DOT;
                let center = rect.loc.x + rect.size.w / 2.0;
                let bottom = geometry.plate.loc.y + geometry.plate.size.h;
                let mark = Rectangle::new(
                    Point::from((center - size / 2.0, bottom - 3.0 - size)),
                    Size::from((size, size)),
                );
                let physical = (RUNNING_DOT * scale).round().max(1.0);
                push(&dot, mark, (physical, physical));
            }
            let texture = self.texture(&item.icon, side);
            push(&texture, *rect, (side as f64, side as f64));
        }
        if let Some(x) = geometry.separator {
            let plate = geometry.plate;
            let rect = Rectangle::new(
                Point::from((x, plate.loc.y + 12.0)),
                Size::from((1.0, plate.size.h - 24.0)),
            );
            push(&line, rect, (1.0, 1.0));
        }

        // Fond, sous les icônes.
        let plate = local(geometry.plate).to_i32_round();
        let mut cached = self.plate.borrow_mut();
        match cached.as_mut() {
            Some((area, element)) => {
                if *area != plate {
                    element.resize(plate, None);
                    *area = plate;
                }
                element.update_uniforms(vec![Uniform::new("scale", scale as f32)]);
            }
            None => {
                let element = PixelShaderElement::new(
                    program.clone(),
                    plate,
                    None,
                    1.0,
                    vec![Uniform::new("scale", scale as f32)],
                    Kind::Unspecified,
                );
                *cached = Some((plate, element));
            }
        }
        if let Some((_, element)) = cached.as_ref() {
            out.push(super::super::render::OutputElement::Shader(element.clone()));
        }
    }

    /// Texture d'une icône de `side` pixels de côté.
    fn texture(&self, icon: &AppIcon, side: u32) -> MemoryRenderBuffer {
        let mut textures = self.textures.borrow_mut();
        textures
            .entry((icon.clone(), side))
            .or_insert_with(|| {
                let mut pixels = vec![0u8; side as usize * side as usize * 4];
                let size = side as f32;
                let drawn = match icon {
                    AppIcon::Builtin(name) => {
                        self.icons
                            .draw(&mut pixels, side, side, name, 0.0, 0.0, size, Style::Color);
                        true
                    }
                    AppIcon::File(path) => {
                        self.icons
                            .draw_file(&mut pixels, side, side, path, 0.0, 0.0, size)
                    }
                    AppIcon::Missing => false,
                };
                if !drawn {
                    self.icons.draw(
                        &mut pixels,
                        side,
                        side,
                        "app-search",
                        0.0,
                        0.0,
                        size,
                        Style::Color,
                    );
                }
                MemoryRenderBuffer::from_slice(
                    &pixels,
                    Fourcc::Argb8888,
                    (side as i32, side as i32),
                    1,
                    smithay::utils::Transform::Normal,
                    None,
                )
            })
            .clone()
    }

    /// Point des applications ouvertes et trait de séparation, à l'échelle `scale`.
    fn marks(&self, scale: f64) -> (MemoryRenderBuffer, MemoryRenderBuffer) {
        let key = (scale * 1000.0).round() as u32;
        self.marks
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let side = (RUNNING_DOT * scale).round().max(1.0) as u32;
                let mut pixels = vec![0u8; side as usize * side as usize * 4];
                let s = side as f32;
                fill_rounded(
                    &mut pixels,
                    side,
                    side,
                    0.0,
                    0.0,
                    s,
                    s,
                    s / 2.0,
                    Color::rgb(0x1C1C1E).with_alpha(230),
                );
                let dot = MemoryRenderBuffer::from_slice(
                    &pixels,
                    Fourcc::Argb8888,
                    (side as i32, side as i32),
                    1,
                    smithay::utils::Transform::Normal,
                    None,
                );
                let gray = Color::BLACK.with_alpha(60);
                let a = gray.a;
                let line = MemoryRenderBuffer::from_slice(
                    &[0, 0, 0, a],
                    Fourcc::Argb8888,
                    (1, 1),
                    1,
                    smithay::utils::Transform::Normal,
                    None,
                );
                (dot, line)
            })
            .clone()
    }
}

/// Courbe des animations : rapide au départ, douce à l'arrivée.
fn ease(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
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
    use crate::desktop::apps::App;

    fn output() -> Rectangle<i32, Logical> {
        Rectangle::new((0, 0).into(), (1280, 720).into())
    }

    fn medium() -> DockConfig {
        DockConfig::default()
    }

    fn items(pinned: &[App], others: &[&str]) -> Vec<DockItem> {
        pinned
            .iter()
            .map(|app| DockItem {
                key: AppKey::Builtin(*app),
                icon: AppIcon::Builtin(app.icon()),
                pinned: true,
                running: false,
            })
            .chain(others.iter().map(|id| DockItem {
                key: AppKey::Other(id.to_string()),
                icon: AppIcon::Missing,
                pinned: false,
                running: true,
            }))
            .collect()
    }

    #[test]
    fn dock_centre_et_laisse_une_marge_en_bas() {
        let dock = layout(output(), medium(), &items(&App::DOCK, &[]), 1.0, None, 0.0);
        assert_eq!(dock.plate.size.w, 304.0);
        assert_eq!(dock.plate.size.h, 68.0);
        assert_eq!(dock.plate.loc, Point::from((488.0, 642.0)));
        assert_eq!(dock.separator, None);
        // Caché : entièrement sous le bord de l'écran.
        let hidden = layout(output(), medium(), &items(&App::DOCK, &[]), 0.0, None, 0.0);
        assert!(hidden.plate.loc.y >= 720.0);
    }

    #[test]
    fn applications_ouvertes_apres_un_trait() {
        let list = items(&App::DOCK, &["vlc", "brave"]);
        let dock = layout(output(), medium(), &list, 1.0, None, 0.0);
        assert_eq!(dock.icons.len(), 7);
        assert_eq!(dock.plate.size.w, 304.0 + 2.0 * (52.0 + ICON_GAP) + SEPARATOR);
        let separator = dock.separator.expect("trait entre les deux groupes");
        assert!(separator > dock.icons[4].loc.x + 52.0 && separator < dock.icons[5].loc.x);
    }

    #[test]
    fn clics_sur_les_icones_identifient_les_cases() {
        let list = items(&App::DOCK, &[]);
        let dock = layout(output(), medium(), &list, 1.0, None, 0.0);
        let x0 = dock.plate.loc.x;
        let at = |dx: f64| dock.item_at(Point::from((x0 + dx, 660.0)));
        assert_eq!(at(20.0), Some(0));
        assert_eq!(at(80.0), Some(1));
        assert_eq!(at(260.0), Some(4));
        assert_eq!(at(320.0), None);
    }

    #[test]
    fn agrandissement_sous_la_souris() {
        let list = items(&App::DOCK, &[]);
        let flat = layout(output(), medium(), &list, 1.0, None, 0.0);
        let pointer = Point::from((flat.icons[2].loc.x + 26.0, 670.0));
        let zoomed = layout(output(), medium(), &list, 1.0, Some(pointer), 1.0);
        // L'icône sous la souris est la plus grande, ses voisines un peu moins.
        let sizes: Vec<f64> = zoomed.icons.iter().map(|icon| icon.size.w).collect();
        assert!((sizes[2] - 52.0 * MAGNIFY).abs() < 0.5, "{sizes:?}");
        assert!(sizes[1] > 52.0 && sizes[1] < sizes[2]);
        assert!((sizes[1] - sizes[3]).abs() < 0.5);
        // Le fond ne bouge pas ; les icônes grandissent sur place, posées en bas.
        assert_eq!(zoomed.plate, flat.plate);
        let center = |icon: Rectangle<f64, Logical>| icon.loc.x + icon.size.w / 2.0;
        assert!((center(zoomed.icons[2]) - center(flat.icons[2])).abs() < 0.01);
        assert!((center(zoomed.icons[0]) - center(flat.icons[0])).abs() < 0.01);
        // Un peu seulement : deux voisines ne se chevauchent presque pas.
        let overlap = zoomed.icons[2].loc.x - (zoomed.icons[1].loc.x + zoomed.icons[1].size.w);
        assert!(overlap > -2.0, "chevauchement {overlap}");
        assert_eq!(
            zoomed.icons[2].loc.y + zoomed.icons[2].size.h,
            flat.icons[2].loc.y + flat.icons[2].size.h
        );
        // L'icône agrandie se clique là où on la voit, même au-dessus du fond.
        let above = Point::from((zoomed.icons[2].loc.x + 40.0, zoomed.icons[2].loc.y + 5.0));
        assert_eq!(zoomed.item_at(above), Some(2));
    }

    #[test]
    fn masquage_intelligent_plein_ecran_et_retour_par_le_bas() {
        let far = [Rectangle::new((100, 100).into(), (400, 300).into())];
        let over = [Rectangle::new((300, 300).into(), (700, 400).into())];
        let dock = layout(output(), medium(), &items(&App::DOCK, &[]), 1.0, None, 0.0).plate;
        let base = Visibility {
            config: medium(),
            output: output(),
            dock,
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
        assert!(should_show(Visibility {
            windows: &over,
            launcher_open: true,
            ..base
        }));
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
        let large = DockConfig {
            size: DockSize::Large,
            ..medium()
        };
        assert!(large.height() > medium().height());
    }
}
