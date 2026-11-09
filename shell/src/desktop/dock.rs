//! Dock Ferrite, comme sur macOS : les applications épinglées, puis (après un trait) les
//! applications ouvertes qui ne le sont pas, puis (après un autre trait) la corbeille ;
//! un point sous celles qui ont une fenêtre.
//! Sous la souris, les icônes grossissent comme sur macOS : la plus proche double
//! presque, ses voisines grandissent en vague, toutes s'écartent et le fond s'élargit,
//! sans que le point sous la souris ne bouge.
//!
//! Une application qu'on lance rebondit dans le Dock, son point déjà allumé, jusqu'à
//! l'arrivée de sa première fenêtre ([`Bounce`]).
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
    apps::{App, AppIcon, AppKey},
    svg::{IconCache, Style},
};
use ferrite_system::config::{ClockStyle, CornerAction, DockHiding, DoubleClick, Settings};
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

const ICON_GAP: f64 = 10.0;
const PAD_X: f64 = 12.0;
/// Marge du haut et du bas du fond, identique : les icônes sont centrées, et le point des
/// applications ouvertes se pose dans la marge du bas.
const PAD_Y: f64 = 14.0;
const DOT_BOTTOM: f64 = 4.0;
const BOTTOM_MARGIN: i32 = 18;
const RUNNING_DOT: f64 = 5.0;
/// Contour sombre du point, qui le garde lisible sur un fond d'écran clair.
const DOT_HALO: f64 = 1.0;
/// Place ajoutée pour chaque trait (applications ouvertes non épinglées, corbeille).
const SEPARATOR: f64 = 9.0;
const SEPARATOR_HEIGHT: f64 = 54.0;
/// Débord du fond du Dock pour son ombre (0 12 32).
pub const PLATE_SHADOW_MARGIN: i32 = 44;
/// Zone du bas où un mouvement rapide vers le bas rappelle un Dock masqué : il ne sort
/// que si on y glisse vite la souris, pas quand elle passe simplement par là.
const REVEAL_APPROACH_ZONE: f64 = 20.0;
/// Vitesse de descente (pixels logiques par seconde) qui rappelle le Dock.
const REVEAL_APPROACH_SPEED: f64 = 650.0;
/// Temps de lissage de la vitesse, et pause au-delà de laquelle elle repart de zéro.
const DESCENT_SMOOTHING: f64 = 0.04;
const DESCENT_PAUSE: Duration = Duration::from_millis(100);
/// Temps laissé pour rejoindre le Dock après une approche rapide.
pub(crate) const REVEAL_APPROACH_GRACE: Duration = Duration::from_millis(600);
/// Plus grand pas d'une image à la suivante pour les glissements et fondus (30 images
/// par seconde).
const MAX_STEP: Duration = Duration::from_millis(33);
/// Durée du glissement du Dock.
const SLIDE: Duration = ferrite_ui::tokens::motion::SHORT;
/// Agrandissement maximal d'une icône sous la souris, et portée de l'effet (en icônes,
/// de chaque côté du pointeur).
const MAGNIFY: f64 = 1.9;
const MAGNIFY_RANGE: f64 = 2.5;
/// Durée de l'entrée et de la sortie de l'agrandissement.
const MAGNIFY_EASE: Duration = ferrite_ui::tokens::motion::QUICK;

/// Rebond d'une application qui démarre : durée d'un saut, hauteur (en icônes), et
/// délai au-delà duquel on renonce à attendre sa fenêtre.
const BOUNCE_PERIOD: Duration = Duration::from_millis(640);
const BOUNCE_HEIGHT: f64 = 0.5;
const BOUNCE_TIMEOUT: Duration = Duration::from_secs(12);

/// Rebond d'une icône pendant le démarrage de son application, comme sur macOS : elle
/// saute tant que l'application n'a pas de fenêtre, puis finit son saut en cours.
#[derive(Debug, Clone, Copy)]
pub struct Bounce {
    start: Instant,
    /// Fin du dernier saut, fixée quand la fenêtre arrive.
    end: Option<Instant>,
}

impl Bounce {
    pub fn new(start: Instant) -> Self {
        Self { start, end: None }
    }

    /// L'application a sa fenêtre : le saut en cours (au moins un) sera le dernier.
    pub fn settle(&mut self, now: Instant) {
        if self.end.is_none() {
            let elapsed = now.duration_since(self.start).as_secs_f64();
            let jumps = (elapsed / BOUNCE_PERIOD.as_secs_f64()).ceil().max(1.0);
            self.end = Some(self.start + BOUNCE_PERIOD.mul_f64(jumps));
        }
    }

    pub fn over(&self, now: Instant) -> bool {
        self.end.is_some_and(|end| now >= end) || now.duration_since(self.start) >= BOUNCE_TIMEOUT
    }

    /// Hauteur de l'icône au-dessus de sa place, en icônes : une parabole par saut,
    /// rapide au départ et ralentie en haut.
    pub fn height(&self, now: Instant) -> f64 {
        if self.over(now) {
            return 0.0;
        }
        let t = (now.duration_since(self.start).as_secs_f64() / BOUNCE_PERIOD.as_secs_f64()).fract();
        4.0 * t * (1.0 - t) * BOUNCE_HEIGHT
    }
}

/// Réglages du Dock et de la barre du haut, lus dans `~/.config/ferrite/reglages`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockConfig {
    /// Côté des icônes, en pixels logiques.
    pub icon_px: u32,
    pub hiding: DockHiding,
    /// Masquage de la barre du haut.
    pub bar: DockHiding,
    /// Hauteur de la barre du haut, en pixels logiques.
    pub bar_height: u32,
    /// Agrandissement des icônes sous la souris.
    pub magnify: bool,
    /// Point sous les applications ouvertes, et applications ouvertes non épinglées.
    pub dots: bool,
    pub recents: bool,
    /// Barre du haut : translucide, pourcentage de batterie, icône Bluetooth, horloge.
    pub bar_translucent: bool,
    pub battery_percent: bool,
    pub bluetooth_icon: bool,
    pub clock: ClockStyle,
    /// Coins actifs : haut gauche, haut droit, bas gauche, bas droit.
    pub corners: [CornerAction; 4],
    /// Fenêtres : aimantation, tuilage, double-clic sur la barre de titre, animations.
    pub snapping: bool,
    pub tiling: bool,
    pub double_click: DoubleClick,
    pub reduce_motion: bool,
}

impl Default for DockConfig {
    fn default() -> Self {
        Self {
            icon_px: 58,
            hiding: DockHiding::default(),
            bar: DockHiding::Never,
            bar_height: 34,
            magnify: true,
            dots: true,
            recents: true,
            bar_translucent: true,
            battery_percent: false,
            bluetooth_icon: true,
            clock: ClockStyle::Full,
            corners: [
                CornerAction::Overview,
                CornerAction::None,
                CornerAction::None,
                CornerAction::ShowDesktop,
            ],
            snapping: true,
            tiling: false,
            double_click: DoubleClick::Maximize,
            reduce_motion: false,
        }
    }
}

impl DockConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            icon_px: settings.dock_icon(),
            hiding: settings.dock_hiding(),
            bar: settings.bar_hiding(),
            bar_height: settings.bar_height(),
            magnify: settings.dock_magnify(),
            dots: settings.flag("dock_points", true),
            recents: settings.flag("dock_recentes", true),
            bar_translucent: settings.flag("barre_translucide", true),
            battery_percent: settings.flag("barre_pourcentage", false),
            bluetooth_icon: settings.flag("barre_bluetooth", true),
            clock: settings.clock_style(),
            corners: ["hg", "hd", "bg", "bd"].map(|corner| settings.hot_corner(corner)),
            snapping: settings.flag("aimantation", true),
            tiling: settings.flag("tuilage", false),
            double_click: settings.double_click(),
            reduce_motion: settings.flag("reduire_animations", false),
        }
    }

    /// Bande du haut que les fenêtres laissent libre : seulement si la barre ne se cache
    /// jamais (hors plein écran).
    pub fn bar_reserved_height(self) -> i32 {
        match self.bar {
            DockHiding::Never => self.bar_height as i32,
            DockHiding::Smart | DockHiding::Always => 0,
        }
    }

    pub(crate) fn icon(self) -> f64 {
        f64::from(self.icon_px)
    }

    pub fn height(self) -> i32 {
        (self.icon() + 2.0 * PAD_Y) as i32
    }

    /// Bande du bas que les fenêtres laissent libre : seulement si le Dock ne se cache jamais.
    /// Bande occupée par le Dock quand il est sorti, qu'il se cache ou non.
    pub fn shown_height(self) -> i32 {
        self.height() + BOTTOM_MARGIN
    }

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
    /// Au moins une fenêtre, visible ou réduite, ou en train de démarrer.
    pub running: bool,
    /// Plusieurs fenêtres : deux points au lieu d'un.
    pub several: bool,
}

/// Géométrie du Dock à un instant donné, en pixels logiques de l'écran.
#[derive(Debug, Clone, PartialEq)]
pub struct DockLayout {
    /// Fond du Dock.
    pub plate: Rectangle<f64, Logical>,
    /// Icônes, dans l'ordre des cases (carrés, posés sur le bas du fond).
    pub icons: Vec<Rectangle<f64, Logical>>,
    /// Traits de séparation : devant les applications ouvertes non épinglées et devant
    /// la corbeille.
    pub separators: Vec<f64>,
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
    // Un trait sépare les applications épinglées des applications ouvertes qui ne le
    // sont pas, un autre précède la corbeille, dernière case du Dock.
    let mut separator_before: Vec<usize> = [
        items.iter().position(|item| !item.pinned),
        items
            .iter()
            .position(|item| item.key == AppKey::Builtin(App::TRASH)),
    ]
    .into_iter()
    .flatten()
    .filter(|index| *index > 0)
    .collect();
    separator_before.dedup();
    // Centres des icônes sans agrandissement, relatifs au bord gauche du fond.
    let mut base = Vec::with_capacity(count);
    let mut x = PAD_X;
    for index in 0..count {
        if separator_before.contains(&index) {
            x += SEPARATOR;
        }
        base.push(x + icon / 2.0);
        x += icon + ICON_GAP;
    }
    let base_width = (x - ICON_GAP + PAD_X).max(2.0 * PAD_X + icon);
    let left = output.loc.x as f64 + (output.size.w as f64 - base_width) / 2.0;
    let height = config.height() as f64;
    let hidden_by = (height + BOTTOM_MARGIN as f64) * (1.0 - reveal);
    let top = output.loc.y as f64 + output.size.h as f64 - height - BOTTOM_MARGIN as f64 + hidden_by;

    // Agrandissement : chaque icône selon la distance de sa case au pointeur (en
    // largeurs d'icône), en vague de cosinus.
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
    // Les icônes agrandies s'écartent : centres recalculés de proche en proche.
    let mut centers = Vec::with_capacity(count);
    let mut x = 0.0;
    for (index, scale) in scales.iter().enumerate() {
        if separator_before.contains(&index) {
            x += SEPARATOR;
        }
        centers.push(x + icon * scale / 2.0);
        x += icon * scale + ICON_GAP;
    }
    // Recalage : le point sous la souris reste sous la souris (le Dock grandit des deux
    // côtés), sinon le Dock reste centré.
    let shift = match pointer.filter(|_| zoom > 0.0 && count > 0) {
        Some(pointer) => {
            let flat: Vec<f64> = base.iter().map(|center| left + center).collect();
            pointer.x - follow(&flat, &centers, pointer.x)
        }
        None => left + PAD_X - centers.first().map_or(0.0, |first| first - icon / 2.0),
    };
    let bottom = top + height - PAD_Y;
    let icons: Vec<Rectangle<f64, Logical>> = centers
        .iter()
        .zip(&scales)
        .map(|(center, scale)| {
            let size = icon * scale;
            Rectangle::new(
                Point::from((shift + center - size / 2.0, bottom - size)),
                Size::from((size, size)),
            )
        })
        .collect();
    // Le fond suit les icônes, sans jamais être plus étroit qu'au repos.
    let (plate_left, plate_right) = match (icons.first(), icons.last()) {
        (Some(first), Some(last)) => (
            (first.loc.x - PAD_X).min(left),
            (last.loc.x + last.size.w + PAD_X).max(left + base_width),
        ),
        _ => (left, left + base_width),
    };
    let separators = separator_before
        .iter()
        .map(|&index| {
            let before = icons[index - 1];
            (before.loc.x + before.size.w + icons[index].loc.x) / 2.0
        })
        .collect();
    DockLayout {
        plate: Rectangle::new(
            Point::from((plate_left, top)),
            Size::from((plate_right - plate_left, height)),
        ),
        icons,
        separators,
    }
}

/// Position, dans la disposition agrandie, du point `x` de la disposition au repos :
/// interpolée entre les centres d'icônes qui l'encadrent (`flat` : centres au repos,
/// `zoomed` : centres agrandis, relatifs au début du Dock).
fn follow(flat: &[f64], zoomed: &[f64], x: f64) -> f64 {
    let last = flat.len() - 1;
    if x <= flat[0] {
        return zoomed[0] - (flat[0] - x);
    }
    if x >= flat[last] {
        return zoomed[last] + (x - flat[last]);
    }
    let index = flat.windows(2).position(|pair| x <= pair[1]).unwrap_or(last - 1);
    let t = (x - flat[index]) / (flat[index + 1] - flat[index]);
    zoomed[index] + (zoomed[index + 1] - zoomed[index]) * t
}

impl DockLayout {
    /// Case sous `position` : l'icône, ou la colonne du fond sous elle.
    pub fn item_at(&self, position: Point<f64, Logical>) -> Option<usize> {
        let plate = self.plate;
        let in_plate = plate.contains(position);
        self.icons.iter().position(|icon| {
            let column = position.x >= icon.loc.x - ICON_GAP / 2.0
                && position.x < icon.loc.x + icon.size.w + ICON_GAP / 2.0;
            column
                && (in_plate
                    || (position.y >= icon.loc.y
                        && position.y < plate.loc.y + plate.size.h + BOTTOM_MARGIN as f64))
        })
    }

    /// Zone où la souris agrandit les icônes : le fond, et au-dessus la hauteur des
    /// icônes agrandies.
    fn hover_area(&self, icon: f64) -> Rectangle<f64, Logical> {
        dock_hover_area(self.plate, icon)
    }
}

pub(crate) fn dock_hover_area(plate: Rectangle<f64, Logical>, icon: f64) -> Rectangle<f64, Logical> {
    let rise = icon * (MAGNIFY - 1.0);
    // Le Dock agrandi déborde de son fond au repos : la zone aussi, sur les côtés.
    let side = icon * (MAGNIFY - 1.0);
    Rectangle::new(
        Point::from((plate.loc.x - side, plate.loc.y - rise)),
        Size::from((
            plate.size.w + 2.0 * side,
            plate.size.h + rise + BOTTOM_MARGIN as f64,
        )),
    )
}

/// Vitesse de descente lissée après un déplacement de `dy` en `elapsed` : mesurée sur la
/// course et non d'un événement à l'autre, car une souris à 1000 Hz ne bouge que d'un ou
/// deux pixels par événement.
pub(crate) fn descent_speed(previous: f64, dy: f64, elapsed: Duration) -> f64 {
    let dt = elapsed.as_secs_f64();
    if elapsed >= DESCENT_PAUSE || dt <= 0.0 {
        return if dt > 0.0 { (dy / dt).max(0.0) } else { 0.0 };
    }
    let weight = 1.0 - (-dt / DESCENT_SMOOTHING).exp();
    previous + ((dy / dt).max(0.0) - previous) * weight
}

/// La souris descend-elle assez vite, près du bas de l'écran, pour rappeler le Dock ?
pub(crate) fn fast_approach(speed: f64, to: Point<f64, Logical>, bottom: f64) -> bool {
    bottom - to.y <= REVEAL_APPROACH_ZONE && speed >= REVEAL_APPROACH_SPEED
}

/// La souris monte-t-elle assez vite, près du haut de l'écran, pour rappeler la barre
/// masquée ? Même règle que le Dock, vers l'autre bord : `speed` est la vitesse de
/// montée, mesurée par [`descent_speed`] sur le déplacement inversé.
pub(crate) fn fast_approach_top(speed: f64, to: Point<f64, Logical>, top: f64) -> bool {
    to.y - top <= REVEAL_APPROACH_ZONE && speed >= REVEAL_APPROACH_SPEED
}

/// Ce qui décide si le Dock est visible.
#[derive(Debug, Clone, Copy)]
pub struct Visibility<'a> {
    pub config: DockConfig,
    /// Fond du Dock sorti, sans agrandissement.
    pub dock: Rectangle<f64, Logical>,
    /// Le Dock est appelé : approche rapide vers son emplacement, ou souris dessus depuis
    /// qu'il est sorti.
    pub called: bool,
    /// Cadres des fenêtres affichées (barre de titre comprise).
    pub windows: &'a [Rectangle<i32, Logical>],
    /// La fenêtre du dessus est en plein écran.
    pub fullscreen: bool,
    /// Le menu Applications ou le menu d'une icône est ouvert.
    pub launcher_open: bool,
}

/// Le Dock doit-il être affiché ?
pub fn should_show(v: Visibility<'_>) -> bool {
    if v.called || v.launcher_open {
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
    /// Un pas ne dépasse jamais `MAX_STEP` : après une image lente, ou une longue pause
    /// sans dessin, le mouvement reprend où il en était au lieu de sauter à sa fin.
    pub(crate) fn step(cell: &Cell<(f64, Option<Instant>)>, target: f64, duration: Duration) -> f64 {
        let (value, last) = cell.get();
        let now = Instant::now();
        let step = last.map_or(1.0, |last| {
            now.duration_since(last).min(MAX_STEP).as_secs_f64() / duration.as_secs_f64()
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
/// Fond du Dock gardé : place, échelle et élément.
type Plate = (Rectangle<i32, Logical>, f64, PixelShaderElement);

#[derive(Debug)]
pub struct Dock {
    icons: IconCache,
    /// Textures des icônes, par icône et côté en pixels.
    textures: RefCell<HashMap<(AppIcon, u32), MemoryRenderBuffer>>,
    /// Point des applications ouvertes et trait de séparation, par échelle et par
    /// exemplaire : deux éléments tirés du même tampon portent le même identifiant, et
    /// l'affichage n'en garde qu'un.
    marks: RefCell<HashMap<(u32, usize), (MemoryRenderBuffer, MemoryRenderBuffer)>>,
    /// Fond du Dock : place, échelle et élément. Ses paramètres ne sont réécrits que s'ils
    /// changent : chaque réécriture compte comme un changement et fait redessiner le Dock.
    plate: RefCell<Option<Plate>>,
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
        bounces: &[f64],
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
        let mut dots = 0;
        for (index, (item, rect)) in items.iter().zip(&geometry.icons).enumerate() {
            if item.running && config.dots {
                let size = RUNNING_DOT + 2.0 * DOT_HALO;
                let center = rect.loc.x + rect.size.w / 2.0;
                let bottom = geometry.plate.loc.y + geometry.plate.size.h;
                let offsets: &[f64] = if item.several { &[-4.5, 4.5] } else { &[0.0] };
                for offset in offsets {
                    let mark = Rectangle::new(
                        Point::from((
                            center + offset - size / 2.0,
                            bottom - DOT_BOTTOM - RUNNING_DOT - DOT_HALO,
                        )),
                        Size::from((size, size)),
                    );
                    let physical = (size * scale).round().max(1.0);
                    let (dot, _) = self.marks(scale, dots);
                    dots += 1;
                    push(&dot, mark, (physical, physical));
                }
            }
            let mut rect = *rect;
            rect.loc.y -= bounces.get(index).copied().unwrap_or(0.0) * rect.size.h;
            let texture = self.texture(&item.icon, side);
            push(&texture, rect, (side as f64, side as f64));
        }
        for (index, &x) in geometry.separators.iter().enumerate() {
            let (_, line) = self.marks(scale, index);
            let plate = geometry.plate;
            let rect = Rectangle::new(
                Point::from((x, plate.loc.y + (plate.size.h - SEPARATOR_HEIGHT) / 2.0)),
                Size::from((1.0, SEPARATOR_HEIGHT)),
            );
            push(&line, rect, (1.0, 1.0));
        }

        // Fond, sous les icônes.
        let margin = PLATE_SHADOW_MARGIN;
        let plate = local(geometry.plate).to_i32_round();
        let plate = Rectangle::new(
            plate.loc - Point::from((margin, margin)),
            Size::from((plate.size.w + 2 * margin, plate.size.h + 2 * margin)),
        );
        let mut cached = self.plate.borrow_mut();
        match cached.as_mut() {
            Some((area, known_scale, element)) => {
                if *area != plate {
                    element.resize(plate, None);
                    *area = plate;
                }
                if *known_scale != scale {
                    element.update_uniforms(vec![Uniform::new("scale", scale as f32)]);
                    *known_scale = scale;
                }
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
                *cached = Some((plate, scale, element));
            }
        }
        if let Some((_, _, element)) = cached.as_ref() {
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

    /// Point des applications ouvertes et trait de séparation, à l'échelle `scale`
    /// (exemplaire `copy` : un par point ou trait dessiné).
    fn marks(&self, scale: f64, copy: usize) -> (MemoryRenderBuffer, MemoryRenderBuffer) {
        let key = ((scale * 1000.0).round() as u32, copy);
        self.marks
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                // Point blanc cerclé de noir : visible sur un fond clair comme sombre.
                let side = ((RUNNING_DOT + 2.0 * DOT_HALO) * scale).round().max(1.0) as u32;
                let mut pixels = vec![0u8; side as usize * side as usize * 4];
                let s = side as f32;
                let halo = (DOT_HALO * scale) as f32;
                fill_rounded(
                    &mut pixels,
                    side,
                    side,
                    0.0,
                    0.0,
                    s,
                    s,
                    s / 2.0,
                    Color::BLACK.with_alpha(115),
                );
                let inner = s - 2.0 * halo;
                fill_rounded(
                    &mut pixels,
                    side,
                    side,
                    halo,
                    halo,
                    inner,
                    inner,
                    inner / 2.0,
                    Color::WHITE.with_alpha(240),
                );
                let dot = MemoryRenderBuffer::from_slice(
                    &pixels,
                    Fourcc::Argb8888,
                    (side as i32, side as i32),
                    1,
                    smithay::utils::Transform::Normal,
                    None,
                );
                // Blanc à 30 %, prémultiplié.
                let a = 77;
                let line = MemoryRenderBuffer::from_slice(
                    &[a, a, a, a],
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
pub(crate) fn ease(t: f64) -> f64 {
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
                several: false,
            })
            .chain(others.iter().map(|id| DockItem {
                key: AppKey::Other(id.to_string()),
                icon: AppIcon::Missing,
                pinned: false,
                running: true,
                several: false,
            }))
            .collect()
    }

    #[test]
    fn dock_centre_et_laisse_une_marge_en_bas() {
        let dock = layout(output(), medium(), &items(&App::dock(), &[]), 1.0, None, 0.0);
        assert_eq!(dock.plate.size.w, 354.0);
        assert_eq!(dock.plate.size.h, 86.0);
        assert_eq!(dock.plate.loc, Point::from((463.0, 616.0)));
        // Icônes au milieu du fond : autant de marge en haut qu'en bas.
        let icon = dock.icons[0];
        let above = icon.loc.y - dock.plate.loc.y;
        let below = dock.plate.loc.y + dock.plate.size.h - icon.loc.y - icon.size.h;
        assert_eq!(above, below);
        assert!(dock.separators.is_empty());
        // Caché : entièrement sous le bord de l'écran.
        let hidden = layout(output(), medium(), &items(&App::dock(), &[]), 0.0, None, 0.0);
        assert!(hidden.plate.loc.y >= 720.0);
    }

    #[test]
    fn des_traits_separent_les_ouvertes_et_la_corbeille() {
        let mut list = items(&App::dock(), &["vlc", "brave"]);
        list.push(DockItem {
            key: AppKey::Builtin(App::TRASH),
            icon: AppIcon::Builtin(App::TRASH.icon()),
            pinned: true,
            running: false,
            several: false,
        });
        let dock = layout(output(), medium(), &list, 1.0, None, 0.0);
        assert_eq!(dock.icons.len(), 8);
        assert_eq!(
            dock.plate.size.w,
            354.0 + 3.0 * (58.0 + ICON_GAP) + 2.0 * SEPARATOR
        );
        // Un trait entre épinglées et ouvertes, un autre devant la corbeille.
        let [opened, trash] = dock.separators[..] else {
            panic!("deux traits attendus : {:?}", dock.separators);
        };
        assert!(opened > dock.icons[4].loc.x + 58.0 && opened < dock.icons[5].loc.x);
        assert!(trash > dock.icons[6].loc.x + 58.0 && trash < dock.icons[7].loc.x);
        // Sans application ouverte non épinglée, seul le trait de la corbeille reste.
        let mut only_pinned = items(&App::dock(), &[]);
        only_pinned.push(list[7].clone());
        let dock = layout(output(), medium(), &only_pinned, 1.0, None, 0.0);
        assert_eq!(dock.separators.len(), 1);
    }

    #[test]
    fn clics_sur_les_icones_identifient_les_cases() {
        let list = items(&App::dock(), &[]);
        let dock = layout(output(), medium(), &list, 1.0, None, 0.0);
        let x0 = dock.plate.loc.x;
        let at = |dx: f64| dock.item_at(Point::from((x0 + dx, 660.0)));
        assert_eq!(at(20.0), Some(0));
        assert_eq!(at(80.0), Some(1));
        assert_eq!(at(330.0), Some(4));
        assert_eq!(at(360.0), None);
    }

    #[test]
    fn agrandissement_sous_la_souris() {
        let list = items(&App::dock(), &[]);
        let flat = layout(output(), medium(), &list, 1.0, None, 0.0);
        let pointer = Point::from((flat.icons[2].loc.x + 29.0, 670.0));
        let zoomed = layout(output(), medium(), &list, 1.0, Some(pointer), 1.0);
        // L'icône sous la souris est la plus grande, ses voisines en vague décroissante.
        let sizes: Vec<f64> = zoomed.icons.iter().map(|icon| icon.size.w).collect();
        assert!((sizes[2] - 58.0 * MAGNIFY).abs() < 0.5, "{sizes:?}");
        assert!(sizes[1] > 58.0 && sizes[1] < sizes[2] && sizes[0] < sizes[1]);
        assert!((sizes[1] - sizes[3]).abs() < 0.5);
        // Les icônes s'écartent sans se chevaucher, et le fond s'élargit pour les contenir.
        for pair in zoomed.icons.windows(2) {
            let gap = pair[1].loc.x - (pair[0].loc.x + pair[0].size.w);
            assert!((gap - ICON_GAP).abs() < 0.01, "écart {gap}");
        }
        assert!(zoomed.plate.size.w > flat.plate.size.w + 58.0);
        assert!(zoomed.plate.loc.x <= zoomed.icons[0].loc.x - PAD_X + 0.01);
        assert_eq!(zoomed.plate.size.h, flat.plate.size.h);
        // Le point sous la souris ne bouge pas : l'icône du milieu reste centrée dessous.
        let center = |icon: Rectangle<f64, Logical>| icon.loc.x + icon.size.w / 2.0;
        assert!((center(zoomed.icons[2]) - pointer.x).abs() < 0.01);
        // Les icônes restent posées en bas et débordent vers le haut.
        assert_eq!(
            zoomed.icons[2].loc.y + zoomed.icons[2].size.h,
            flat.icons[2].loc.y + flat.icons[2].size.h
        );
        // L'icône agrandie se clique là où on la voit, même au-dessus du fond.
        let above = Point::from((center(zoomed.icons[2]), zoomed.icons[2].loc.y + 5.0));
        assert_eq!(zoomed.item_at(above), Some(2));
        // Sans souris, retour exact à la disposition au repos.
        assert_eq!(layout(output(), medium(), &list, 1.0, Some(pointer), 0.0), flat);
    }

    #[test]
    fn le_point_sous_la_souris_reste_fixe_entre_deux_icones() {
        let list = items(&App::dock(), &["vlc"]);
        let flat = layout(output(), medium(), &list, 1.0, None, 0.0);
        let x = flat.icons[1].loc.x + 58.0 + ICON_GAP / 2.0;
        let zoomed = layout(output(), medium(), &list, 1.0, Some(Point::from((x, 670.0))), 1.0);
        // À mi-chemin entre deux icônes, elles sont de même taille, de part et d'autre.
        assert!((zoomed.icons[1].size.w - zoomed.icons[2].size.w).abs() < 0.5);
        let middle = (zoomed.icons[1].loc.x + zoomed.icons[1].size.w + zoomed.icons[2].loc.x) / 2.0;
        assert!((middle - x).abs() < 0.01);
    }

    #[test]
    fn masquage_intelligent_plein_ecran_et_retour_par_le_bas() {
        let far = [Rectangle::new((100, 100).into(), (400, 300).into())];
        let over = [Rectangle::new((300, 300).into(), (700, 400).into())];
        let dock = layout(output(), medium(), &items(&App::dock(), &[]), 1.0, None, 0.0).plate;
        let base = Visibility {
            config: medium(),
            dock,
            called: false,
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
        // Appelé (approche rapide, ou souris dessus), il revient, même en plein écran.
        assert!(should_show(Visibility {
            windows: &over,
            fullscreen: true,
            called: true,
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
        assert_eq!(never.reserved_height(), 110);
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
            icon_px: 72,
            ..medium()
        };
        assert!(large.height() > medium().height());
    }

    #[test]
    fn une_approche_rapide_pres_de_la_zone_rappelle_le_dock() {
        let bottom = 720.0;
        let near = Point::from((640.0, 705.0));
        // Souris à 1000 Hz : deux pixels par milliseconde (2000 px/s) pendant 100 ms.
        let mut speed = 0.0;
        for _ in 0..100 {
            speed = descent_speed(speed, 2.0, Duration::from_millis(1));
        }
        assert!(speed > 1500.0);
        assert!(fast_approach(speed, near, bottom));
        // Trop loin du bas, ou trop lent.
        assert!(!fast_approach(speed, Point::from((640.0, 620.0)), bottom));
        let mut slow = 0.0;
        for _ in 0..40 {
            slow = descent_speed(slow, 0.2, Duration::from_millis(1));
        }
        assert!(!fast_approach(slow, near, bottom));
        // Remonter ou s'arrêter fait retomber la vitesse.
        assert_eq!(descent_speed(speed, -5.0, Duration::from_millis(200)), 0.0);
        assert!(descent_speed(speed, -2.0, Duration::from_millis(1)) < speed);
    }

    #[test]
    fn approche_rapide_vers_le_haut() {
        let mut speed = 0.0;
        for _ in 0..50 {
            speed = descent_speed(speed, 2.0, Duration::from_millis(1));
        }
        assert!(fast_approach_top(speed, Point::from((640.0, 5.0)), 0.0));
        assert!(!fast_approach_top(speed, Point::from((640.0, 100.0)), 0.0));
        let mut slow = 0.0;
        for _ in 0..50 {
            slow = descent_speed(slow, 0.2, Duration::from_millis(1));
        }
        assert!(!fast_approach_top(slow, Point::from((640.0, 5.0)), 0.0));
    }

    #[test]
    fn les_icones_restent_cliquables_jusqu_au_bord_bas() {
        let list = items(&App::dock(), &["vlc"]);
        let dock = layout(output(), medium(), &list, 1.0, None, 0.0);
        let x = dock.icons[0].loc.x + dock.icons[0].size.w / 2.0;
        assert_eq!(dock.item_at(Point::from((x, 719.0))), Some(0));
    }

    #[test]
    fn l_icone_rebondit_jusqu_a_la_fenetre_puis_finit_son_saut() {
        let start = Instant::now();
        let mut bounce = Bounce::new(start);
        assert_eq!(bounce.height(start), 0.0);
        // En haut au milieu du saut, de retour en bas à la fin.
        assert!((bounce.height(start + BOUNCE_PERIOD / 2) - BOUNCE_HEIGHT).abs() < 1e-9);
        assert!(bounce.height(start + BOUNCE_PERIOD * 3 + BOUNCE_PERIOD / 4) > 0.0);
        // La fenêtre arrive pendant le quatrième saut : il va jusqu'au bout.
        let window = start + BOUNCE_PERIOD * 3 + BOUNCE_PERIOD / 4;
        bounce.settle(window);
        assert!(!bounce.over(window + BOUNCE_PERIOD / 2));
        assert!(bounce.over(start + BOUNCE_PERIOD * 4));
        // Sans fenêtre, on finit par abandonner.
        assert!(Bounce::new(start).over(start + BOUNCE_TIMEOUT));
    }
}
