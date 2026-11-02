//! Interface du bureau, avec l'aspect du Ferrite d'origine : une barre claire en haut (logo
//! et menu Applications à gauche, date et heure puis indicateurs à droite) et des menus
//! déroulants clairs qui s'ouvrent sous elle.
//!
//! La géométrie de chaque élément est calculée une seule fois par [`Layout`], qui sert
//! à la fois au dessin et à la détection des clics.

use std::{
    cell::{Cell, RefCell},
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ferrite_system::config::Accent;
use ferrite_ui::{
    theme::{Color, Palette},
    wallpaper::Image,
};
use fontdue::{Font, FontSettings};
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
    utils::{Logical, Point, Rectangle, Size},
};

use crate::clipboard::{Thumbnail, VISIBLE as CLIPBOARD_VISIBLE};
use crate::desktop::{
    apps::{App, Section},
    svg::{self, IconCache, Style},
};
use crate::state::PowerAction;

const FONT_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Medium.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

/// Écart entre la barre (ou le bord de l'écran) et un menu déroulant.
const POPOVER_MARGIN: f64 = 6.0;
const INDICATOR_WIDTH: f64 = 30.0;
/// Lanceur : icônes par ligne, case d'une application, marges et hauteurs.
const LAUNCHER_COLUMNS: usize = 5;
const LAUNCHER_CELL: (f64, f64) = (120.0, 122.0);
const LAUNCHER_PAD: f64 = 28.0;
/// Hauteur du champ de recherche, en haut du lanceur.
const LAUNCHER_HEADER: f64 = 70.0;
/// Hauteur du titre d'une rubrique, et espace entre deux rubriques.
const LAUNCHER_TITLE: f64 = 40.0;
const LAUNCHER_GAP: f64 = 14.0;
const LAUNCHER_MAX_HEIGHT: f64 = 620.0;
const LAUNCHER_RADIUS: f32 = 26.0;
/// Voile blanc posé sur le fond d'écran flouté (0 : fond seul, 1 : blanc).
const FROST_VEIL: f32 = 0.6;
/// Défilement d'un cran de molette, en pixels.
pub const LAUNCHER_SCROLL_STEP: f64 = 48.0;
const CONTROLS_WIDTH: f64 = 400.0;
/// Écart entre les modules des réglages rapides, et entre eux et le bord du panneau.
const CONTROLS_GAP: f64 = 12.0;
/// Hauteur du module Wi-Fi / Bluetooth / mode avion, et d'un module à curseur.
const CONTROLS_RADIOS: f64 = 156.0;
const CONTROLS_SLIDER: f64 = 74.0;
const CLIPBOARD_WIDTH: f64 = 380.0;
const CLIPBOARD_HEADER: f64 = 50.0;
const CLIPBOARD_ROW: f64 = 70.0;
const CLIPBOARD_GAP: f64 = 6.0;
const SESSION_WIDTH: f64 = 230.0;
const DIALOG: (f64, f64) = (420.0, 168.0);
/// Fréquence de relecture de l'état de la batterie.
const BATTERY_REFRESH: Duration = Duration::from_secs(20);

/// Verre des réglages rapides : gris très clair presque opaque (sans flou derrière, un
/// verre plus transparent laisse lire les fenêtres au travers), liseré blanc et contour
/// sombre fins.
static DARK_THEME: AtomicBool = AtomicBool::new(false);
static ACCENT_RGB: AtomicU32 = AtomicU32::new(0);

fn palette() -> Palette {
    if DARK_THEME.load(Ordering::Relaxed) {
        Palette::DARK
    } else {
        Palette::LIGHT
    }
}

fn glass() -> Color {
    palette().header_background.with_alpha(232)
}

fn glass_rim() -> Color {
    Color::WHITE.with_alpha(110)
}

fn glass_edge() -> Color {
    (if DARK_THEME.load(Ordering::Relaxed) {
        Color::WHITE
    } else {
        Color::BLACK
    })
    .with_alpha(28)
}

fn bar_ink() -> Color {
    palette().text
}
/// Barre du haut, presque transparente comme sur macOS : un voile très léger sur le fond
/// d'écran, et un texte blanc quand le fond est sombre derrière elle.
const BAR_VEIL_LIGHT: Color = Color::WHITE.with_alpha(60);
const BAR_VEIL_DARK: Color = Color::BLACK.with_alpha(22);
fn card() -> Color {
    palette().window_background
}

fn primary_text() -> Color {
    palette().text
}

fn text_secondary() -> Color {
    palette().text_secondary
}

fn accent() -> Color {
    let rgb = ACCENT_RGB.load(Ordering::Relaxed);
    accent_for(palette(), (rgb != 0).then_some(rgb))
}

fn accent_for(palette: Palette, rgb: Option<u32>) -> Color {
    rgb.map_or(palette.accent, Color::rgb)
}

fn destructive() -> Color {
    palette().danger
}

fn text_strong() -> Color {
    palette().text
}

fn text_blue() -> Color {
    palette().text
}

fn text_muted_blue() -> Color {
    palette().text_secondary
}

fn controls_module() -> Color {
    palette().header_background.with_alpha(170)
}

fn controls_track() -> Color {
    (if DARK_THEME.load(Ordering::Relaxed) {
        Color::WHITE
    } else {
        Color::BLACK
    })
    .with_alpha(16)
}

fn controls_on() -> Color {
    palette().accent
}

/// Menu déroulant ouvert depuis la barre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popover {
    Launcher,
    Controls,
    Session,
    /// Historique du presse-papiers (Super+V).
    Clipboard,
    /// Menu d'une icône du Dock (clic droit).
    DockMenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAction {
    TogglePopover(Popover),
    ClosePopover,
    Launch(App),
    /// Application du lanceur : position dans la liste (filtrée) des applications.
    LaunchTile(usize),
    /// « Plus » ou « Moins » à droite du titre d'une rubrique.
    LauncherLink(SectionLink),
    ToggleWifi,
    ToggleBluetooth,
    /// Coupe (ou rallume) toutes les radios présentes.
    ToggleAirplane,
    SetVolume(u8),
    SetBrightness(u8),
    RequestPower(PowerAction),
    ConfirmPower(PowerAction),
    CancelPower,
    /// Historique du presse-papiers : indices dans la liste complète.
    ClipboardPaste(usize),
    ClipboardPin(usize),
    ClipboardDelete(usize),
    ClipboardClear,
    /// Déplacement du panneau presse-papiers depuis sa barre ; décalage pointeur/carte.
    ClipboardDragStart(i32, i32),
    DashboardAddNote,
    DashboardEditNote(usize),
    DashboardAddTask,
    DashboardToggleTask(usize),
    DashboardCyclePriority(usize),
    DashboardDeleteTask(usize),
    DashboardMonth(i32),
    DashboardConsume,
    DashboardLaunch(App),
    DashboardShortcut(usize),
    /// Point de la barre : aller à cette page (widgets ou bureau).
    GoToPage(usize),
    /// Choix `index` du menu d'une icône du Dock.
    DockMenu(usize),
    /// Clic absorbé par un panneau, sans effet.
    Consume,
}

/// Aperçu d'un élément de l'historique du presse-papiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipPreview {
    /// Premières lignes du texte.
    Text(Vec<String>),
    Image(Thumbnail),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipItem {
    /// Position dans la liste complète.
    pub index: usize,
    pub preview: ClipPreview,
    pub pinned: bool,
}

/// Menu d'une icône du Dock, posé au-dessus d'elle.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DockMenuView {
    /// Centre de l'icône, et haut du Dock, en pixels logiques de l'écran.
    pub x: f64,
    pub bottom: f64,
    pub entries: Vec<String>,
}

impl Eq for DockMenuView {}

/// Lignes affichées du panneau Super+V.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClipboardView {
    pub items: Vec<ClipItem>,
    pub selected: usize,
    /// Éléments au-delà de ceux affichés.
    pub more: usize,
    /// Position personnalisée de la carte, en pixels logiques sur l'écran.
    pub position: Option<(i32, i32)>,
}

/// Icône d'une case du menu Applications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileIcon {
    /// Icône Ferrite embarquée.
    Builtin(&'static str),
    /// Fichier d'icône d'une application installée.
    File(PathBuf),
    /// Pas d'icône trouvée : icône générique.
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub name: String,
    pub icon: TileIcon,
    /// Position dans la liste (filtrée) des applications.
    pub index: usize,
}

/// Lien à droite du titre d'une rubrique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionLink {
    /// Affiche toute la rubrique.
    More(Section),
    /// Revient à la vue d'ensemble.
    Less,
}

/// Rubrique du lanceur : titre, applications, et lien éventuel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherSection {
    pub title: String,
    pub tiles: Vec<Tile>,
    pub link: Option<SectionLink>,
}

/// Contenu du lanceur : recherche, rubriques et défilement.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LauncherView {
    pub query: String,
    pub sections: Vec<LauncherSection>,
    /// Défilement demandé, en pixels logiques (borné à l'affichage).
    pub scroll: u32,
    /// Fond d'écran flouté, vu à travers le verre du lanceur.
    pub backdrop: Option<Arc<Image>>,
}

/// État du bureau à afficher, fourni par le compositeur. `None` : matériel ou service
/// absent, le réglage est alors grisé.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DashboardTaskView {
    pub text: String,
    pub done: bool,
    pub priority: crate::desktop::widgets::Priority,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DashboardNoteView {
    pub title: String,
    pub preview: String,
    pub updated: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DashboardView {
    pub notes: Vec<DashboardNoteView>,
    pub tasks: Vec<DashboardTaskView>,
    pub draft: String,
    pub editing: Option<crate::desktop::widgets::DashboardEdit>,
    pub month_offset: i32,
    pub weather: Option<ferrite_system::weather::Weather>,
    pub place: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DesktopView {
    pub dark_theme: bool,
    pub popover: Option<Popover>,
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
    pub volume: Option<u8>,
    pub brightness: Option<u8>,
    pub power: Option<PowerAction>,
    /// Dernier réglage refusé par le système.
    pub error: Option<String>,
    /// Barre du haut affichée (masquée quand une application est en plein écran).
    pub bar_visible: bool,
    /// Le fond d'écran est sombre sous la barre : elle écrit en blanc.
    pub bar_dark: bool,
    /// Hauteur configurable de la barre du haut.
    pub bar_height: u32,
    /// Page affichée (0 : widgets) et nombre de pages ; 0 page : pas de points dans la barre.
    pub page: usize,
    pub page_count: usize,
    /// Bande du bas occupée par le Dock, que les widgets évitent.
    pub dock_height: i32,
    pub launcher: LauncherView,
    pub clipboard: ClipboardView,
    pub dashboard: DashboardView,
    pub dock_menu: Option<DockMenuView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VisualState {
    width: u32,
    height: u32,
    scale_milli: u32,
    accent: Color,
    view: DesktopView,
    battery: Option<Battery>,
    clock: ClockSnapshot,
    system: SystemMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ClockSnapshot {
    year: i64,
    month: u32,
    day: u32,
    weekday: usize,
    hour: u32,
    minute: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct SystemMetrics {
    cpu: u8,
    memory: u8,
    disk: u8,
    uptime_minutes: u64,
    download_kib: u64,
    upload_kib: u64,
    network_history: Vec<(u64, u64)>,
}

#[derive(Debug, Default)]
struct SystemMonitor {
    read_at: Option<Instant>,
    previous_cpu: Option<(u64, u64)>,
    previous_network: Option<(u64, u64)>,
    network_history: Vec<(u64, u64)>,
    metrics: SystemMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Battery {
    percent: u8,
    charging: bool,
}

#[derive(Debug)]
pub struct Panels {
    regular: Font,
    bold: Font,
    icons: IconCache,
    battery: RefCell<(Option<Instant>, Option<Battery>)>,
    system: RefCell<SystemMonitor>,
    cached: RefCell<Option<(VisualState, MemoryRenderBuffer, Option<PopoverLayer>)>>,
    /// Page des widgets, dessinée à part : elle passe sous les fenêtres et glisse.
    dashboard: RefCell<Option<(VisualState, MemoryRenderBuffer)>>,
    /// Dernier panneau animé affiché (centre de contrôle, session), gardé pour s'effacer
    /// après sa fermeture, et sa part d'apparition (0 à 1).
    layer: RefCell<Option<PopoverLayer>>,
    layer_shown: Cell<(f64, Option<Instant>)>,
    /// Verre du lanceur, dernier calculé.
    frost: RefCell<Option<(FrostKey, Arc<Frost>)>>,
}

/// Durées de l'apparition et de la disparition d'un panneau, et sa taille de départ.
/// Module Raccourcis : marge sur les côtés et début des icônes, sous le titre.
const SHORTCUTS_PAD: f64 = 12.0;
const LAYER_IN: Duration = Duration::from_millis(200);
const LAYER_OUT: Duration = Duration::from_millis(150);
const LAYER_SCALE: f64 = 0.94;

/// Panneau dessiné à part pour pouvoir s'animer : son rectangle (ombre comprise, en
/// pixels logiques de l'écran) et ses pixels.
#[derive(Debug, Clone)]
struct PopoverLayer {
    rect: Rect,
    buffer: MemoryRenderBuffer,
}

/// Panneaux qui apparaissent et disparaissent en fondu, grandissant depuis leur coin
/// haut droit (sous leur bouton de la barre), comme sur macOS.
fn animated(popover: Option<Popover>) -> bool {
    matches!(popover, Some(Popover::Controls | Popover::Session))
}

/// Place (pixels physiques), échelle et fond d'écran d'un verre calculé.
type FrostKey = ([u32; 4], u32, Option<usize>);

/// Verre dépoli prêt à poser : pixels BGRA prémultipliés du rectangle `left`, `top`,
/// `width` × `height` (pixels physiques de l'écran).
#[derive(Debug)]
struct Frost {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Panels {
    pub fn set_theme(dark: bool, accent: Accent) {
        DARK_THEME.store(dark, Ordering::Relaxed);
        ACCENT_RGB.store(accent.rgb().unwrap_or_default(), Ordering::Relaxed);
    }

    pub fn new() -> Result<Self, String> {
        let regular = Font::from_bytes(FONT_REGULAR, FontSettings::default()).map_err(|e| e.to_string())?;
        let bold = Font::from_bytes(FONT_BOLD, FontSettings::default()).map_err(|e| e.to_string())?;
        Ok(Self {
            regular,
            bold,
            icons: IconCache::default(),
            battery: RefCell::new((None, None)),
            system: RefCell::new(SystemMonitor::default()),
            cached: RefCell::new(None),
            dashboard: RefCell::new(None),
            layer: RefCell::new(None),
            layer_shown: Cell::new((0.0, None)),
            frost: RefCell::new(None),
        })
    }

    pub fn push_element(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        output_geo: Rectangle<i32, Logical>,
        scale: f64,
        view: &DesktopView,
        out: &mut Vec<super::super::render::OutputElement>,
    ) {
        // Les widgets ont leur propre image : les retirer évite de tout redessiner pour eux.
        let state = VisualState {
            width: output_geo.size.w.max(1) as u32,
            height: output_geo.size.h.max(1) as u32,
            scale_milli: (scale * 1000.0).round() as u32,
            accent: accent(),
            view: DesktopView {
                dark_theme: view.dark_theme,
                dashboard: DashboardView::default(),
                ..view.clone()
            },
            battery: self.battery(),
            clock: clock_snapshot(SystemTime::now()),
            system: SystemMetrics::default(),
        };
        let mut cache = self.cached.borrow_mut();
        if !cache.as_ref().is_some_and(|(key, _, _)| key == &state) {
            let buffer = self.rasterize(&state, scale);
            let layer = self.rasterize_layer(&state, scale);
            *cache = Some((state, buffer, layer));
        }
        let Some((_, buffer, open)) = cache.as_ref() else {
            return;
        };

        // Panneau animé, au-dessus du reste.
        if let Some(open) = open {
            *self.layer.borrow_mut() = Some(open.clone());
        }
        let (target, duration) = if open.is_some() {
            (1.0, LAYER_IN)
        } else {
            (0.0, LAYER_OUT)
        };
        let shown = super::dock::ease(super::dock::DockMotion::step(&self.layer_shown, target, duration));
        if shown > 0.0
            && let Some(layer) = self.layer.borrow().as_ref()
        {
            let rect = layer.rect;
            let grow = LAYER_SCALE + (1.0 - LAYER_SCALE) * shown;
            let (w, h) = (rect.w * grow, rect.h * grow);
            let origin = Point::<f64, Logical>::from((rect.x + rect.w - w, rect.y));
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                origin.to_physical(scale),
                &layer.buffer,
                Some(shown as f32),
                // Comme pour le reste du bureau : pixels du tampon, posés sur `w` × `h`.
                Some(Rectangle::from_size((rect.w * scale, rect.h * scale).into())),
                Some(Size::from((w.round().max(1.0) as i32, h.round().max(1.0) as i32))),
                Kind::Unspecified,
            ) {
                Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
                Err(err) => tracing::warn!(?err, output = output.name(), "panneau animé non importé"),
            }
        }

        let physical_size = (output_geo.size.w as f64 * scale, output_geo.size.h as f64 * scale);
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::from((0.0, 0.0)),
            buffer,
            None,
            Some(Rectangle::<f64, Logical>::from_size(physical_size.into())),
            Some(output_geo.size),
            Kind::Unspecified,
        ) {
            Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
            Err(err) => tracing::warn!(?err, output = output.name(), "panneau du bureau non importé"),
        }
    }

    /// Page des widgets, décalée de `shift` pixels logiques quand elle glisse.
    #[allow(clippy::too_many_arguments)] // Rendu, écran, état et décalage.
    pub fn push_dashboard(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        output_geo: Rectangle<i32, Logical>,
        scale: f64,
        view: &DesktopView,
        shift: i32,
        out: &mut Vec<super::super::render::OutputElement>,
    ) {
        let state = VisualState {
            width: output_geo.size.w.max(1) as u32,
            height: output_geo.size.h.max(1) as u32,
            scale_milli: (scale * 1000.0).round() as u32,
            accent: accent(),
            view: DesktopView {
                dark_theme: view.dark_theme,
                dashboard: view.dashboard.clone(),
                bar_height: view.bar_height,
                dock_height: view.dock_height,
                ..DesktopView::default()
            },
            battery: self.battery(),
            clock: clock_snapshot(SystemTime::now()),
            system: self.system_metrics(),
        };
        let mut cache = self.dashboard.borrow_mut();
        if !cache.as_ref().is_some_and(|(key, _)| key == &state) {
            let buffer = self.rasterize_dashboard(&state, scale);
            *cache = Some((state, buffer));
        }
        let Some((_, buffer)) = cache.as_ref() else {
            return;
        };
        let physical_size = (output_geo.size.w as f64 * scale, output_geo.size.h as f64 * scale);
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::from((shift as f64 * scale, 0.0)),
            buffer,
            None,
            Some(Rectangle::<f64, Logical>::from_size(physical_size.into())),
            Some(output_geo.size),
            Kind::Unspecified,
        ) {
            Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
            Err(err) => tracing::warn!(?err, output = output.name(), "widgets non importés"),
        }
    }

    fn rasterize_dashboard(&self, state: &VisualState, scale: f64) -> MemoryRenderBuffer {
        let width = (state.width as f64 * scale).round().max(1.0) as u32;
        let height = (state.height as f64 * scale).round().max(1.0) as u32;
        let (layout, design_scale) = dashboard_layout(state.width as f64, state.height as f64, &state.view);
        let mut canvas = Canvas {
            pixels: vec![0; width as usize * height as usize * 4],
            width,
            height,
            scale: (scale * design_scale) as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        self.draw_dashboard(&mut canvas, &layout, state);
        MemoryRenderBuffer::from_slice(
            &canvas.pixels,
            Fourcc::Argb8888,
            (width as i32, height as i32),
            1,
            smithay::utils::Transform::Normal,
            None,
        )
    }

    /// État de la batterie, relu au plus toutes les [`BATTERY_REFRESH`].
    fn battery(&self) -> Option<Battery> {
        let mut cached = self.battery.borrow_mut();
        if cached.0.is_none_or(|read| read.elapsed() >= BATTERY_REFRESH) {
            *cached = (Some(Instant::now()), read_battery());
        }
        cached.1
    }

    fn system_metrics(&self) -> SystemMetrics {
        let mut monitor = self.system.borrow_mut();
        if monitor
            .read_at
            .is_none_or(|read| read.elapsed() >= Duration::from_secs(2))
        {
            let now = Instant::now();
            let elapsed = monitor
                .read_at
                .map_or(0.0, |read| now.duration_since(read).as_secs_f64());
            monitor.read_at = Some(Instant::now());
            if let Some((total, idle)) = cpu_counters() {
                if let Some((previous_total, previous_idle)) = monitor.previous_cpu {
                    let elapsed = total.saturating_sub(previous_total);
                    let busy = elapsed.saturating_sub(idle.saturating_sub(previous_idle));
                    monitor.metrics.cpu = if elapsed == 0 {
                        0
                    } else {
                        (busy.saturating_mul(100) / elapsed).min(100) as u8
                    };
                }
                monitor.previous_cpu = Some((total, idle));
            }
            if let Some(memory) = memory_usage() {
                monitor.metrics.memory = memory;
            }
            if let Some(disk) = disk_usage() {
                monitor.metrics.disk = disk;
            }
            monitor.metrics.uptime_minutes = fs::read_to_string("/proc/uptime")
                .ok()
                .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
                .map_or(0, |seconds| seconds as u64 / 60);
            if let Some((received, sent)) = network_counters() {
                if let Some((previous_received, previous_sent)) = monitor.previous_network
                    && elapsed > 0.0
                {
                    monitor.metrics.download_kib =
                        (received.saturating_sub(previous_received) as f64 / elapsed / 1024.0) as u64;
                    monitor.metrics.upload_kib =
                        (sent.saturating_sub(previous_sent) as f64 / elapsed / 1024.0) as u64;
                    let sample = (monitor.metrics.download_kib, monitor.metrics.upload_kib);
                    monitor.network_history.push(sample);
                    if monitor.network_history.len() > 32 {
                        monitor.network_history.remove(0);
                    }
                    monitor.metrics.network_history = monitor.network_history.clone();
                }
                monitor.previous_network = Some((received, sent));
            }
        }
        monitor.metrics.clone()
    }

    fn rasterize(&self, state: &VisualState, scale: f64) -> MemoryRenderBuffer {
        let width = (state.width as f64 * scale).round().max(1.0) as u32;
        let height = (state.height as f64 * scale).round().max(1.0) as u32;
        let mut canvas = Canvas {
            pixels: vec![0; width as usize * height as usize * 4],
            width,
            height,
            scale: scale as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        let view = &state.view;
        let layout = Layout::new(state.width as f64, state.height as f64, view);

        // Barre du haut (sauf en plein écran, tant que la souris ne la rappelle pas).
        if view.bar_visible {
            self.draw_bar(&mut canvas, &layout, state);
        }

        match view.popover {
            Some(Popover::Launcher) => self.draw_launcher(&mut canvas, &layout, &view.launcher),
            // Dessinés à part, par `rasterize_layer`.
            Some(Popover::Controls | Popover::Session) => {}
            Some(Popover::Clipboard) => self.draw_clipboard(&mut canvas, &layout, &view.clipboard),
            Some(Popover::DockMenu) => {
                if let Some(menu) = &view.dock_menu {
                    self.draw_dock_menu(&mut canvas, &layout, menu);
                }
            }
            None => {}
        }

        // Confirmation explicite, pour éviter un arrêt accidentel.
        if let Some(power) = view.power {
            self.draw_dialog(&mut canvas, &layout, power);
        }

        MemoryRenderBuffer::from_slice(
            &canvas.pixels,
            Fourcc::Argb8888,
            (width as i32, height as i32),
            1,
            smithay::utils::Transform::Normal,
            None,
        )
    }

    /// Panneau animé ouvert, seul, découpé à sa place (ombre comprise).
    fn rasterize_layer(&self, state: &VisualState, scale: f64) -> Option<PopoverLayer> {
        let view = &state.view;
        if !animated(view.popover) {
            return None;
        }
        let width = (state.width as f64 * scale).round().max(1.0) as u32;
        let height = (state.height as f64 * scale).round().max(1.0) as u32;
        let mut canvas = Canvas {
            pixels: vec![0; width as usize * height as usize * 4],
            width,
            height,
            scale: scale as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        let layout = Layout::new(state.width as f64, state.height as f64, view);
        let card = match view.popover {
            Some(Popover::Controls) => {
                self.draw_controls(&mut canvas, &layout, view);
                layout.controls()
            }
            _ => {
                self.draw_session(&mut canvas, &layout);
                layout.session()
            }
        };
        // Marge pour l'ombre, en pixels physiques entiers.
        let left = ((card.x - 16.0) * scale).floor().max(0.0) as u32;
        let top = ((card.y - 16.0) * scale).floor().max(0.0) as u32;
        let right = (((card.x + card.w + 16.0) * scale).ceil() as u32).min(width);
        let bottom = (((card.y + card.h + 20.0) * scale).ceil() as u32).min(height);
        if right <= left || bottom <= top {
            return None;
        }
        let (w, h) = (right - left, bottom - top);
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
        for row in top..bottom {
            let start = (row as usize * width as usize + left as usize) * 4;
            pixels.extend_from_slice(&canvas.pixels[start..start + w as usize * 4]);
        }
        Some(PopoverLayer {
            rect: Rect::new(
                left as f64 / scale,
                top as f64 / scale,
                w as f64 / scale,
                h as f64 / scale,
            ),
            buffer: MemoryRenderBuffer::from_slice(
                &pixels,
                Fourcc::Argb8888,
                (w as i32, h as i32),
                1,
                smithay::utils::Transform::Normal,
                None,
            ),
        })
    }

    fn draw_dashboard(&self, canvas: &mut Canvas, layout: &Layout, state: &VisualState) {
        let view = &state.view.dashboard;
        let [clock, weather, calendar, notes, tasks, system, shortcuts, quote] = layout.dashboard_cards();
        let cards = [clock, weather, calendar, notes, tasks, system, shortcuts, quote];
        for card in cards {
            canvas.card_shadow(card, 16.0);
            let alpha = if card == quote { 184 } else { 238 };
            canvas.fill(card, 16.0, Color::WHITE.with_alpha(alpha));
            canvas.fill(
                Rect::new(card.x + 16.0, card.y + 1.0, card.w - 32.0, 1.0),
                0.5,
                Color::WHITE.with_alpha(180),
            );
        }
        self.quote_landscape(canvas, quote);

        let months = [
            "janvier",
            "février",
            "mars",
            "avril",
            "mai",
            "juin",
            "juillet",
            "août",
            "septembre",
            "octobre",
            "novembre",
            "décembre",
        ];
        let weekdays = ["Dim.", "Lun.", "Mar.", "Mer.", "Jeu.", "Ven.", "Sam."];

        canvas.icon(
            "app-calendar",
            (clock.x + 30.0, clock.y + 28.0),
            20.0,
            Style::Color,
        );
        let date = format!(
            "{} {} {} {}",
            weekdays[state.clock.weekday],
            state.clock.day,
            months[state.clock.month.saturating_sub(1) as usize],
            state.clock.year
        );
        canvas.label(
            &self.regular,
            &date,
            clock.x + 51.0,
            clock.y + 20.0,
            13.0,
            text_blue(),
        );
        canvas.icon(
            "app-settings",
            (clock.right() - 26.0, clock.y + 27.0),
            18.0,
            Style::Color,
        );
        let hour = match state.clock.hour % 12 {
            0 => 12,
            hour => hour,
        };
        let meridiem = if state.clock.hour < 12 { "AM" } else { "PM" };
        canvas.label(
            &self.bold,
            &format!("{hour}:{:02}", state.clock.minute),
            clock.x + 20.0,
            clock.y + 53.0,
            62.0,
            text_blue(),
        );
        canvas.label(
            &self.bold,
            meridiem,
            clock.x + 194.0,
            clock.y + 109.0,
            15.0,
            text_muted_blue(),
        );
        canvas.icon(
            "status-location",
            (clock.x + 30.0, clock.y + 152.0),
            15.0,
            Style::Color,
        );
        canvas.label(
            &self.regular,
            &view.place,
            clock.x + 43.0,
            clock.y + 146.0,
            13.0,
            text_blue(),
        );
        self.sun(
            canvas,
            (clock.right() - 48.0, clock.y + 132.0),
            17.0,
            Color::rgb(0xFFC83D),
        );
        canvas.icon(
            "status-cloud",
            (clock.right() - 36.0, clock.y + 159.0),
            38.0,
            Style::Color,
        );
        canvas.fill(
            Rect::new(clock.x + 20.0, clock.y + clock.h - 51.0, clock.w - 40.0, 1.0),
            0.0,
            Color::BLACK.with_alpha(14),
        );
        canvas.label(
            &self.regular,
            "« Chaque petit pas compte. »",
            clock.x + 22.0,
            clock.bottom() - 36.0,
            13.0,
            text_muted_blue(),
        );

        self.sun_icon(canvas, (weather.x + 38.0, weather.y + 29.0), 12.0);
        canvas.label(
            &self.bold,
            &view.place,
            weather.x + 66.0,
            weather.y + 19.0,
            15.0,
            text_blue(),
        );
        canvas.icon(
            "status-location",
            (
                weather.x + 66.0 + canvas.text_width(&self.bold, &view.place, 15.0) + 10.0,
                weather.y + 28.0,
            ),
            12.0,
            Style::Color,
        );
        let weather_data = view.weather.as_ref();
        let temperature = weather_data.map_or_else(|| "--".to_owned(), |data| data.temperature.to_string());
        canvas.label(
            &self.bold,
            &format!("{temperature}°C"),
            weather.x + 20.0,
            weather.y + 47.0,
            48.0,
            text_blue(),
        );
        self.sun(
            canvas,
            (weather.right() - 56.0, weather.y + 94.0),
            25.0,
            Color::rgb(0xFFC83D),
        );
        let sky = weather_data.map_or("Chargement…", |data| data.sky.label());
        let feels_like =
            weather_data.map_or_else(String::new, |data| format!("Ressentie : {}°C", data.feels_like));
        canvas.label(
            &self.bold,
            sky,
            weather.x + 22.0,
            weather.y + 105.0,
            13.0,
            text_blue(),
        );
        canvas.label(
            &self.regular,
            &feels_like,
            weather.x + 22.0,
            weather.y + 124.0,
            12.0,
            text_muted_blue(),
        );
        let forecast = Rect::new(weather.x + 12.0, weather.bottom() - 87.0, weather.w - 24.0, 76.0);
        canvas.fill(forecast, 14.0, Color::rgb(0xEAF3FF).with_alpha(180));
        if let Some(data) = weather_data {
            for (index, day) in data.days.iter().take(4).enumerate() {
                let cell_width = forecast.w / 4.0;
                let center = forecast.x + cell_width * (index as f64 + 0.5);
                let weekday = weekday_for(day.date.0, day.date.1, day.date.2);
                canvas.label(
                    &self.regular,
                    weekday,
                    center - canvas.text_width(&self.regular, weekday, 10.0) / 2.0,
                    forecast.y + 8.0,
                    10.0,
                    text_blue(),
                );
                self.weather_symbol(canvas, (center, forecast.y + 37.0), day.sky);
                let range = format!("{}° / {}°", day.max, day.min);
                let width = canvas.text_width(&self.regular, &range, 10.0);
                canvas.label(
                    &self.regular,
                    &range,
                    center - width / 2.0,
                    forecast.y + 57.0,
                    10.0,
                    text_muted_blue(),
                );
                if index > 0 {
                    canvas.fill(
                        Rect::new(
                            forecast.x + cell_width * index as f64,
                            forecast.y + 12.0,
                            1.0,
                            forecast.h - 24.0,
                        ),
                        0.0,
                        Color::WHITE.with_alpha(180),
                    );
                }
            }
        }

        let (year, month) = shift_month(state.clock.year, state.clock.month, view.month_offset);
        canvas.label(
            &self.bold,
            &format!("{} {}", capitalize(months[month as usize - 1]), year),
            calendar.x + 20.0,
            calendar.y + 19.0,
            16.0,
            text_blue(),
        );
        canvas.label(
            &self.bold,
            "‹",
            calendar.right() - 58.0,
            calendar.y + 14.0,
            23.0,
            text_blue(),
        );
        canvas.label(
            &self.bold,
            "›",
            calendar.right() - 30.0,
            calendar.y + 14.0,
            23.0,
            text_blue(),
        );
        let grid_left = calendar.x + 16.0;
        let grid_width = calendar.w - 32.0;
        let cell_width = grid_width / 7.0;
        let week_y = calendar.y + 49.0;
        for (index, label) in ["Lun", "Mar", "Mer", "Jeu", "Ven", "Sam", "Dim"]
            .iter()
            .enumerate()
        {
            let x = grid_left + cell_width * (index as f64 + 0.5);
            let width = canvas.text_width(&self.regular, label, 10.0);
            canvas.label(&self.regular, label, x - width / 2.0, week_y, 10.0, text_blue());
        }
        let first_weekday = (days_from_civil(year, month, 1) + 3).rem_euclid(7) as usize;
        let days_in_month = days_in_month(year, month);
        let row_height = ((calendar.bottom() - week_y - 29.0) / 6.0).clamp(15.0, 25.0);
        for day in 1..=days_in_month {
            let cell = first_weekday + day as usize - 1;
            let row = cell / 7;
            let column = cell % 7;
            let center = (
                grid_left + cell_width * (column as f64 + 0.5),
                week_y + 25.0 + row_height * row as f64,
            );
            let today = view.month_offset == 0 && day == state.clock.day;
            if today {
                canvas.fill(
                    Rect::new(center.0 - 14.0, center.1 - 12.0, 28.0, 25.0),
                    12.0,
                    accent(),
                );
            }
            let text = day.to_string();
            let width = canvas.text_width(&self.regular, &text, 11.0);
            canvas.label(
                &self.regular,
                &text,
                center.0 - width / 2.0,
                center.1 - 7.0,
                11.0,
                if today { Color::WHITE } else { text_blue() },
            );
        }

        self.dashboard_title(canvas, notes, "Mes notes", "app-text-editor");
        let add_note = Rect::new(notes.right() - 44.0, notes.y + 10.0, 34.0, 34.0);
        canvas.label(
            &self.bold,
            "+",
            add_note.x + 7.0,
            add_note.y - 3.0,
            24.0,
            text_blue(),
        );
        let notes_top = notes.y + 56.0;
        let note_height = (notes.bottom() - notes_top - 8.0).max(0.0) / 4.0;
        let accents = [
            Color::rgb(0x348AF0),
            Color::rgb(0x22BA80),
            Color::rgb(0x973CF2),
            Color::rgb(0xF5A000),
        ];
        for index in 0..4 {
            let row = Rect::new(
                notes.x + 12.0,
                notes_top + index as f64 * note_height,
                notes.w - 24.0,
                note_height - 4.0,
            );
            let Some(note) = view.notes.get(index) else {
                if index == 0 && view.notes.is_empty() {
                    canvas.label(
                        &self.regular,
                        "Cliquez sur + pour créer une note",
                        row.x + 10.0,
                        row.y + 11.0,
                        11.0,
                        text_muted_blue(),
                    );
                }
                continue;
            };
            canvas.fill(row, 10.0, Color::rgb(0xF1F6FC).with_alpha(210));
            canvas.fill(
                Rect::new(row.x + 6.0, row.y + 6.0, 5.0, row.h - 12.0),
                3.0,
                accents[index % accents.len()],
            );
            let editing = view.editing == Some(crate::desktop::widgets::DashboardEdit::Note(index));
            let title = if editing && note.title.is_empty() {
                "Écrire une note…"
            } else {
                note.title.as_str()
            };
            let title = canvas.fit_text(&self.bold, title, 12.0, row.w - 75.0);
            canvas.label(&self.bold, &title, row.x + 21.0, row.y + 6.0, 12.0, text_blue());
            let preview = if editing && note.preview.is_empty() {
                "Saisir le contenu…"
            } else {
                note.preview.as_str()
            };
            let preview = canvas.fit_text(&self.regular, preview, 10.5, row.w - 35.0);
            canvas.label(
                &self.regular,
                &preview,
                row.x + 21.0,
                row.y + 23.0,
                10.5,
                text_muted_blue(),
            );
            let updated = note_timestamp(note.updated, state.clock);
            let width = canvas.text_width(&self.regular, &updated, 9.0);
            canvas.label(
                &self.regular,
                &updated,
                row.right() - width - 8.0,
                row.y + 8.0,
                9.0,
                text_muted_blue(),
            );
        }

        self.dashboard_title(canvas, tasks, "Ma To-Do Liste", "status-notifications");
        let add = Rect::new(tasks.right() - 44.0, tasks.y + 10.0, 34.0, 34.0);
        canvas.label(&self.bold, "+", add.x + 7.0, add.y - 3.0, 24.0, text_blue());
        let row_top = tasks.y + 53.0;
        let row_height = ((tasks.bottom() - row_top - 8.0) / 6.0).clamp(25.0, 34.0);
        let visible = ((tasks.bottom() - row_top - 5.0) / row_height).floor().max(0.0) as usize;
        for (index, task) in view.tasks.iter().take(visible).enumerate() {
            let y = row_top + index as f64 * row_height;
            let checkbox = Rect::new(tasks.x + 20.0, y + 5.0, 17.0, 17.0);
            canvas.fill(
                checkbox,
                4.0,
                if task.done {
                    Color::rgb(0x1767FF)
                } else {
                    Color::WHITE
                },
            );
            canvas.stroke(checkbox, 4.0, Color::rgb(0x183365), 1.5);
            if task.done {
                canvas.label(
                    &self.bold,
                    "✓",
                    checkbox.x + 3.0,
                    checkbox.y - 1.0,
                    13.0,
                    Color::WHITE,
                );
            }
            let label = canvas.fit_text(&self.regular, &task.text, 11.5, tasks.w - 125.0);
            canvas.label(
                &self.regular,
                &label,
                checkbox.right() + 14.0,
                y + 6.0,
                11.5,
                if task.done { text_muted_blue() } else { text_blue() },
            );
            let (priority_text, background, ink) = match task.priority {
                crate::desktop::widgets::Priority::High => {
                    ("Haute", Color::rgb(0xFFE0E2), Color::rgb(0xE6444C))
                }
                crate::desktop::widgets::Priority::Medium => {
                    ("Moyenne", Color::rgb(0xFFF0C8), Color::rgb(0xD78600))
                }
                crate::desktop::widgets::Priority::Low => {
                    ("Basse", Color::rgb(0xD7F3E6), Color::rgb(0x18996A))
                }
            };
            let priority_width = canvas.text_width(&self.regular, priority_text, 10.0);
            let pill = Rect::new(
                tasks.right() - priority_width - 30.0,
                y + 3.0,
                priority_width + 18.0,
                22.0,
            );
            canvas.fill(pill, 11.0, background);
            canvas.label(
                &self.regular,
                priority_text,
                pill.x + (pill.w - priority_width) / 2.0,
                pill.y + 4.0,
                10.0,
                ink,
            );
            canvas.fill(
                Rect::new(tasks.x + 18.0, y + row_height - 1.0, tasks.w - 36.0, 1.0),
                0.0,
                Color::rgb(0xDCE6F3),
            );
        }
        if view.tasks.is_empty() {
            canvas.label(
                &self.regular,
                "Aucune tâche. Cliquez sur + pour en ajouter.",
                tasks.x + 20.0,
                row_top + 12.0,
                11.0,
                text_muted_blue(),
            );
        }
        if view.editing == Some(crate::desktop::widgets::DashboardEdit::NewTask) {
            let draft = if view.draft.is_empty() {
                "Saisir une tâche puis Entrée".to_owned()
            } else {
                canvas.fit_text(&self.regular, &view.draft, 12.0, tasks.w - 60.0)
            };
            canvas.label(
                &self.regular,
                &format!("› {draft}"),
                tasks.x + 18.0,
                tasks.bottom() - 28.0,
                12.0,
                accent(),
            );
        }

        self.dashboard_title(canvas, system, "Système", "app-monitor");
        let ring_y = system.y + 91.0;
        let metrics = state.system.clone();
        let values = [
            ("CPU", metrics.cpu, Color::rgb(0x25B978)),
            ("RAM", metrics.memory, Color::rgb(0x2584F5)),
            ("Disque", metrics.disk, Color::rgb(0x8B3FF2)),
        ];
        for (index, (label, value, color)) in values.into_iter().enumerate() {
            let center = (system.x + system.w * (index as f64 + 0.5) / 3.0, ring_y);
            canvas.progress_ring(center, 28.0, 5.0, value, color);
            let text = format!("{value}%");
            let width = canvas.text_width(&self.bold, &text, 11.0);
            canvas.label(
                &self.bold,
                &text,
                center.0 - width / 2.0,
                center.1 - 5.0,
                11.0,
                text_blue(),
            );
            let width = canvas.text_width(&self.regular, label, 10.0);
            canvas.label(
                &self.regular,
                label,
                center.0 - width / 2.0,
                center.1 + 32.0,
                10.0,
                text_blue(),
            );
        }
        canvas.fill(
            Rect::new(system.x + 16.0, system.y + 150.0, system.w - 32.0, 1.0),
            0.0,
            Color::rgb(0xDFE8F4),
        );
        canvas.label(
            &self.bold,
            "Réseau",
            system.x + 18.0,
            system.y + 161.0,
            12.0,
            text_blue(),
        );
        let (download, download_unit) = network_rate(metrics.download_kib);
        let (upload, upload_unit) = network_rate(metrics.upload_kib);
        canvas.icon(
            "status-download",
            (system.x + 25.0, system.y + 190.0),
            16.0,
            Style::Color,
        );
        canvas.label(
            &self.regular,
            &format!("{download} {download_unit}/s"),
            system.x + 42.0,
            system.y + 181.0,
            10.0,
            text_blue(),
        );
        canvas.icon(
            "status-upload",
            (system.x + 25.0, system.y + 217.0),
            16.0,
            Style::Color,
        );
        canvas.label(
            &self.regular,
            &format!("{upload} {upload_unit}/s"),
            system.x + 42.0,
            system.y + 208.0,
            10.0,
            text_blue(),
        );
        let chart = Rect::new(
            system.x + system.w * 0.43,
            system.y + 176.0,
            system.w * 0.52,
            48.0,
        );
        canvas.network_chart(chart, &metrics.network_history);

        canvas.icon(
            "app-search",
            (shortcuts.x + 25.0, shortcuts.y + 26.0),
            19.0,
            Style::Color,
        );
        canvas.label(
            &self.bold,
            "Raccourcis",
            shortcuts.x + 42.0,
            shortcuts.y + 18.0,
            14.0,
            text_blue(),
        );
        let apps = [
            ("Fichiers", "app-files"),
            ("Terminal", "app-terminal"),
            ("Navigateur", "app-web"),
            ("Code", "app-code"),
            ("Paramètres", "app-settings"),
            ("Corbeille", "places-trash"),
        ];
        let item_width = (shortcuts.w - 2.0 * SHORTCUTS_PAD) / apps.len() as f64;
        let top = shortcuts.y + 42.0;
        for (index, (label, icon)) in apps.into_iter().enumerate() {
            let center_x = shortcuts.x + SHORTCUTS_PAD + item_width * (index as f64 + 0.5);
            let tile = Rect::new(center_x - 27.0, top, 54.0, 54.0);
            canvas.fill(tile, 15.0, Color::rgb(0xEAF3FF).with_alpha(230));
            canvas.icon(icon, (center_x, top + 27.0), 34.0, Style::Color);
            let label = canvas.fit_text(&self.regular, label, 10.0, item_width - 4.0);
            let width = canvas.text_width(&self.regular, &label, 10.0);
            canvas.label(
                &self.regular,
                &label,
                center_x - width / 2.0,
                top + 60.0,
                10.0,
                text_blue(),
            );
        }

        canvas.icon("app-clock", (quote.x + 25.0, quote.y + 24.0), 19.0, Style::Color);
        canvas.label(
            &self.bold,
            "Citation du jour",
            quote.x + 42.0,
            quote.y + 18.0,
            14.0,
            text_blue(),
        );
        let citation = "« Les grandes choses ne viennent pas du confort, mais de l’effort. »";
        let lines = wrap_dashboard_text(&self.regular, citation, 12.0, quote.w * 0.49);
        for (index, line) in lines.into_iter().take(3).enumerate() {
            canvas.label(
                &self.regular,
                &line,
                quote.x + 18.0,
                quote.y + 48.0 + index as f64 * 15.0,
                12.0,
                text_blue(),
            );
        }
        canvas.label(
            &self.regular,
            "— Inconnu",
            quote.x + 18.0,
            quote.bottom() - 20.0,
            9.0,
            text_muted_blue(),
        );
    }

    fn sun(&self, canvas: &mut Canvas, center: (f64, f64), radius: f64, color: Color) {
        canvas.fill(
            Rect::new(center.0 - radius, center.1 - radius, radius * 2.0, radius * 2.0),
            radius as f32,
            color,
        );
        for angle in 0..8 {
            let radians = std::f64::consts::TAU * angle as f64 / 8.0;
            let start = (
                center.0 + radians.cos() * (radius + 4.0),
                center.1 + radians.sin() * (radius + 4.0),
            );
            let end = (
                center.0 + radians.cos() * (radius + 9.0),
                center.1 + radians.sin() * (radius + 9.0),
            );
            canvas.line(start, end, 2.0, color);
        }
    }

    fn sun_icon(&self, canvas: &mut Canvas, center: (f64, f64), radius: f64) {
        self.sun(canvas, center, radius, Color::rgb(0xFFB91F));
    }

    fn weather_symbol(&self, canvas: &mut Canvas, center: (f64, f64), sky: ferrite_system::weather::Sky) {
        use ferrite_system::weather::Sky;
        match sky {
            Sky::Clear => self.sun_icon(canvas, center, 8.0),
            Sky::Partly => {
                self.sun(
                    canvas,
                    (center.0 - 4.0, center.1 - 2.0),
                    7.0,
                    Color::rgb(0xFFB91F),
                );
                canvas.icon(
                    "status-cloud",
                    (center.0 + 3.0, center.1 + 4.0),
                    21.0,
                    Style::Color,
                );
            }
            Sky::Cloudy | Sky::Fog => {
                canvas.icon("status-cloud", center, 22.0, Style::Color);
            }
            Sky::Rain | Sky::Storm | Sky::Snow => {
                canvas.icon("status-cloud", center, 21.0, Style::Color);
                let precipitation = if sky == Sky::Snow {
                    Color::rgb(0x74AFFF)
                } else {
                    Color::rgb(0x3089F2)
                };
                for offset in [-5.0, 3.0] {
                    canvas.line(
                        (center.0 + offset, center.1 + 6.0),
                        (center.0 + offset - 2.0, center.1 + 11.0),
                        1.8,
                        precipitation,
                    );
                }
            }
        }
    }

    fn quote_landscape(&self, canvas: &mut Canvas, quote: Rect) {
        let scene = Rect::new(
            quote.x + quote.w * 0.53,
            quote.y + 8.0,
            quote.w * 0.45,
            quote.h - 16.0,
        );
        canvas.fill(scene, 12.0, Color::rgb(0xA9DDF7));
        self.sun(
            canvas,
            (scene.x + scene.w * 0.78, scene.y + scene.h * 0.23),
            scene.h * 0.09,
            Color::rgb(0xFFE07A),
        );
        let left = scene.x;
        let right = scene.right();
        let horizon = scene.y + scene.h * 0.66;
        canvas.polygon(
            &[
                (left, horizon),
                (left + scene.w * 0.24, scene.y + scene.h * 0.24),
                (left + scene.w * 0.42, scene.y + scene.h * 0.57),
                (left + scene.w * 0.61, scene.y + scene.h * 0.31),
                (right, horizon),
                (right, scene.bottom()),
                (left, scene.bottom()),
            ],
            Color::rgb(0x8AB9D7),
        );
        canvas.polygon(
            &[
                (left + scene.w * 0.17, scene.bottom()),
                (left + scene.w * 0.48, scene.y + scene.h * 0.36),
                (left + scene.w * 0.67, scene.bottom()),
            ],
            Color::rgb(0x668EAE),
        );
        canvas.polygon(
            &[
                (left + scene.w * 0.47, scene.bottom()),
                (left + scene.w * 0.76, scene.y + scene.h * 0.33),
                (right, scene.bottom()),
            ],
            Color::rgb(0x729BB6),
        );
        canvas.polygon(
            &[
                (left, scene.y + scene.h * 0.78),
                (left + scene.w * 0.22, scene.y + scene.h * 0.68),
                (left + scene.w * 0.48, scene.y + scene.h * 0.82),
                (left + scene.w * 0.76, scene.y + scene.h * 0.69),
                (right, scene.y + scene.h * 0.77),
                (right, scene.bottom()),
                (left, scene.bottom()),
            ],
            Color::rgb(0x76B88D),
        );
        canvas.fill(
            Rect::new(scene.x + 14.0, scene.y + 14.0, scene.w * 0.2, 5.0),
            3.0,
            Color::WHITE.with_alpha(180),
        );
        canvas.fill(
            Rect::new(scene.x + 22.0, scene.y + 22.0, scene.w * 0.13, 4.0),
            2.0,
            Color::WHITE.with_alpha(150),
        );
    }

    fn dashboard_title(&self, canvas: &mut Canvas, card: Rect, title: &str, icon: &'static str) {
        canvas.icon(icon, (card.x + 27.0, card.y + 28.0), 21.0, Style::Color);
        canvas.label(&self.bold, title, card.x + 48.0, card.y + 19.0, 15.0, text_blue());
        canvas.fill(
            Rect::new(card.x + 16.0, card.y + 48.0, card.w - 32.0, 1.0),
            0.0,
            Color::BLACK.with_alpha(15),
        );
    }

    fn draw_bar(&self, canvas: &mut Canvas, layout: &Layout, state: &VisualState) {
        let view = &state.view;
        let (veil, ink, pill) = if view.bar_dark {
            (BAR_VEIL_DARK, Color::WHITE, Color::WHITE.with_alpha(45))
        } else {
            (BAR_VEIL_LIGHT, bar_ink(), Color::BLACK.with_alpha(30))
        };
        // Le fond d'écran passe sous la barre : seul un voile léger la distingue.
        canvas.fill(Rect::new(0.0, 0.0, layout.w, layout.bar), 0.0, veil);
        let applications = layout.applications_button();
        // Pastille sous l'élément de la barre dont le menu est ouvert.
        let open = view.popover.and_then(|popover| match popover {
            Popover::Launcher => Some(applications),
            Popover::Controls => Some(layout.indicators_group()),
            Popover::Session => Some(layout.indicator(Indicator::Power)),
            Popover::Clipboard | Popover::DockMenu => None,
        });
        if let Some(rect) = open {
            canvas.fill(
                Rect::new(rect.x + 2.0, 4.0, rect.w - 4.0, layout.bar - 8.0),
                6.0,
                pill,
            );
        }
        canvas.logo((applications.x + 16.0, layout.bar / 2.0), ink);
        canvas.bar_label(
            &self.bold,
            "Applications",
            applications.x + 32.0,
            11.5,
            ink,
            layout.bar,
        );
        // Une marque par page : carrée pour les widgets, ronde pour les bureaux.
        for page in 0..view.page_count {
            let dot = layout.page_dot(page);
            let (center_x, center_y) = dot.center();
            let color = if page == view.page {
                ink
            } else {
                ink.with_alpha(90)
            };
            let radius = if page == 0 { 1.5 } else { 3.5 };
            canvas.fill(Rect::new(center_x - 3.5, center_y - 3.5, 7.0, 7.0), radius, color);
        }
        let clock = clock_label(SystemTime::now(), local_offset());
        let clock_width = canvas.text_width(&self.regular, &clock, 11.5);
        let clock_right = layout.indicators_group().x - 10.0;
        canvas.bar_label(
            &self.regular,
            &clock,
            clock_right - clock_width,
            11.5,
            ink,
            layout.bar,
        );
        for (indicator, rect) in layout.indicators() {
            let name = match indicator {
                Indicator::Bluetooth if view.bluetooth == Some(true) => "status-bluetooth-on",
                Indicator::Bluetooth => "status-bluetooth-off",
                Indicator::Network => match view.wifi {
                    Some(enabled) => svg::wifi_icon(enabled),
                    None => "status-network",
                },
                Indicator::Volume => svg::volume_icon(view.volume.unwrap_or(0)),
                Indicator::Battery => match state.battery {
                    Some(battery) => svg::battery_icon(battery.percent, battery.charging),
                    None => continue,
                },
                Indicator::Power => "status-power",
            };
            // Les tracés Wi-Fi et Bluetooth remplissent leur dessin : plus petits, ils
            // gardent le même poids visuel que le son et la batterie.
            let size = match indicator {
                Indicator::Bluetooth => 19.0,
                Indicator::Network if view.wifi.is_some() => 21.0,
                _ => 26.0,
            };
            canvas.icon(name, rect.center(), size, Style::Symbolic(ink));
        }
        if let Some(battery) = state.battery {
            let rect = layout.indicator(Indicator::Battery);
            let text = format!("{} %", battery.percent);
            canvas.bar_label(&self.regular, &text, rect.right() + 2.0, 10.5, ink, layout.bar);
        }
    }

    /// Lanceur d'applications, au centre de l'écran comme celui de macOS : verre blanc
    /// dépoli, champ de recherche en haut, puis les rubriques (une ligne d'icônes chacune,
    /// « Plus » pour tout voir).
    fn draw_launcher(&self, canvas: &mut Canvas, layout: &Layout, launcher: &LauncherView) {
        let card = layout.launcher();
        let content = layout.launcher_content();
        // Le verre, opaque, recouvre tout le fond de la carte : seule l'ombre est utile.
        canvas.card_shadow(card, LAUNCHER_RADIUS);
        let frost = self.frost(canvas, card, launcher.backdrop.as_deref(), (layout.w, layout.h));
        canvas.blit_frost(&frost, None);

        // Ce qui défile peut déborder du panneau : l'écran autour est gardé puis remis.
        let margin = LAUNCHER_CELL.1 + LAUNCHER_TITLE + LAUNCHER_GAP;
        let above = canvas.save_rows(card.y - margin, card.y);
        let below = canvas.save_rows(card.bottom(), card.bottom() + margin);
        for item in layout.launcher_items(launcher) {
            match item {
                _ if !item.visible_in(content) => {}
                LauncherItem::Title { section, rect } => {
                    let title = &launcher.sections[section].title;
                    canvas.label(&self.bold, title, rect.x, rect.y + 13.0, 13.0, text_strong());
                }
                LauncherItem::Link { link, rect } => {
                    let text = match link {
                        SectionLink::More(_) => "Plus",
                        SectionLink::Less => "Moins",
                    };
                    let width = canvas.text_width(&self.bold, text, 12.5);
                    canvas.label(
                        &self.bold,
                        text,
                        rect.right() - width,
                        rect.y + 13.5,
                        12.5,
                        text_secondary(),
                    );
                }
                LauncherItem::Rule(rect) => canvas.fill(rect, 0.0, Color::BLACK.with_alpha(26)),
                LauncherItem::Tile { section, slot, rect } => {
                    self.draw_tile(canvas, &launcher.sections[section].tiles[slot], rect);
                }
            }
        }

        // Ce qui déborde du contenu (défilement) est recouvert par le verre, puis le champ
        // de recherche est dessiné par-dessus.
        canvas.restore_rows(above);
        canvas.restore_rows(below);
        canvas.blit_frost(&frost, Some((card.y, content.y)));
        canvas.blit_frost(&frost, Some((content.bottom(), card.bottom())));
        let header_y = card.y + LAUNCHER_HEADER / 2.0;
        canvas.icon(
            "app-search",
            (card.x + LAUNCHER_PAD + 13.0, header_y),
            36.0,
            Style::Symbolic(text_secondary()),
        );
        let text_x = card.x + LAUNCHER_PAD + 40.0;
        let text_y = header_y - 20.0 * 1.22 / 2.0 - 1.0;
        let caret = if launcher.query.is_empty() {
            canvas.label(
                &self.regular,
                "Applications",
                text_x + 4.0,
                text_y,
                20.0,
                text_secondary(),
            );
            text_x
        } else {
            canvas.label(
                &self.regular,
                &launcher.query,
                text_x,
                text_y,
                20.0,
                text_strong(),
            );
            text_x + 2.0 + canvas.text_width(&self.regular, &launcher.query, 20.0)
        };
        canvas.fill(Rect::new(caret, header_y - 15.0, 2.0, 30.0), 1.0, text_strong());
        canvas.fill(
            Rect::new(
                card.x + LAUNCHER_PAD,
                content.y - 1.0,
                card.w - 2.0 * LAUNCHER_PAD,
                1.0,
            ),
            0.0,
            Color::BLACK.with_alpha(30),
        );
    }

    /// Case d'une application : grande icône, nom sur deux lignes au plus, centré.
    fn draw_tile(&self, canvas: &mut Canvas, tile: &Tile, rect: Rect) {
        let center = (rect.x + rect.w / 2.0, rect.y + 42.0);
        let drawn = match &tile.icon {
            TileIcon::Builtin(name) => {
                canvas.icon(name, center, 70.0, Style::Color);
                true
            }
            TileIcon::File(path) => canvas.icon_file(path, center, 60.0),
            TileIcon::Missing => false,
        };
        if !drawn {
            canvas.icon("file-generic", center, 70.0, Style::Color);
        }
        let lines = wrap_two_lines(canvas, &self.regular, &tile.name, 12.0, rect.w - 8.0);
        for (index, line) in lines.iter().enumerate() {
            let width = canvas.text_width(&self.regular, line, 12.0);
            canvas.label(
                &self.regular,
                line,
                rect.x + (rect.w - width) / 2.0,
                rect.y + 84.0 + index as f64 * 16.0,
                12.0,
                text_strong(),
            );
        }
    }

    /// Verre du lanceur : le fond d'écran flouté sous un voile blanc, dans le rectangle
    /// arrondi `card`. Gardé tant que la place, l'échelle et le fond ne changent pas.
    fn frost(&self, canvas: &Canvas, card: Rect, backdrop: Option<&Image>, screen: (f64, f64)) -> Arc<Frost> {
        let s = canvas.scale as f64;
        let left = (card.x * s).floor().max(0.0) as u32;
        let top = (card.y * s).floor().max(0.0) as u32;
        let right = ((card.right() * s).ceil() as u32).min(canvas.width);
        let bottom = ((card.bottom() * s).ceil() as u32).min(canvas.height);
        let key = (
            [left, top, right, bottom],
            canvas.scale.to_bits(),
            backdrop.map(|image| image as *const Image as usize),
        );
        let mut cache = self.frost.borrow_mut();
        if let Some((known, frost)) = cache.as_ref()
            && *known == key
        {
            return frost.clone();
        }
        let (width, height) = (right.saturating_sub(left), bottom.saturating_sub(top));
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let radius = LAUNCHER_RADIUS * canvas.scale;
        let (x, y, w, h) = (
            card.x as f32 * canvas.scale,
            card.y as f32 * canvas.scale,
            card.w as f32 * canvas.scale,
            card.h as f32 * canvas.scale,
        );
        for py in 0..height {
            for px in 0..width {
                let (sx, sy) = ((left + px) as f32 + 0.5, (top + py) as f32 + 0.5);
                let qx = (sx - (x + w * 0.5)).abs() - (w * 0.5 - radius);
                let qy = (sy - (y + h * 0.5)).abs() - (h * 0.5 - radius);
                let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                let coverage = (0.5 - distance).clamp(0.0, 1.0);
                if coverage == 0.0 {
                    continue;
                }
                let behind = backdrop.map_or([236.0, 236.0, 240.0], |image| {
                    sample(image, sx as f64 / s / screen.0, sy as f64 / s / screen.1)
                });
                // Liseré clair d'un pixel le long du bord, comme le verre de macOS.
                let rim = if distance > -1.5 * canvas.scale { 0.35 } else { 0.0 };
                let veil = FROST_VEIL + (1.0 - FROST_VEIL) * rim;
                let offset = (py as usize * width as usize + px as usize) * 4;
                let [r, g, b] = behind.map(|channel| (channel + (255.0 - channel) * veil) * coverage);
                pixels[offset..offset + 4].copy_from_slice(&[
                    b.round() as u8,
                    g.round() as u8,
                    r.round() as u8,
                    (255.0 * coverage).round() as u8,
                ]);
            }
        }
        let frost = Arc::new(Frost {
            left,
            top,
            width,
            height,
            pixels,
        });
        *cache = Some((key, frost.clone()));
        frost
    }

    /// Réglages rapides, à la manière du Centre de contrôle de macOS : modules clairs
    /// posés sur un panneau gris, interrupteurs ronds et curseurs épais.
    fn draw_controls(&self, canvas: &mut Canvas, layout: &Layout, view: &DesktopView) {
        let card = layout.controls();
        canvas.glass(card, 22.0);

        let status = |state: Option<bool>| match state {
            Some(true) => "Activé",
            Some(false) => "Désactivé",
            None => "Aucun adaptateur",
        };
        canvas.module(layout.controls_radios());
        let radios = [
            (Control::Wifi, "glyph-wifi", "Wi-Fi", view.wifi),
            (Control::Bluetooth, "glyph-bluetooth", "Bluetooth", view.bluetooth),
            (Control::Airplane, "glyph-airplane", "Mode avion", airplane(view)),
        ];
        for (control, glyph, title, state) in radios {
            let row = layout.control_row(control);
            let center = (row.x + 30.0, row.y + row.h / 2.0);
            canvas.toggle_circle(center, glyph, state);
            let ink = if state.is_some() {
                text_strong()
            } else {
                text_secondary()
            };
            canvas.label(&self.bold, title, row.x + 56.0, center.1 - 18.0, 14.0, ink);
            canvas.label(
                &self.regular,
                status(state),
                row.x + 56.0,
                center.1 + 1.5,
                11.5,
                text_secondary(),
            );
        }

        for (control, glyph, title) in [
            (Control::Clipboard, "glyph-clipboard", "Presse-papiers"),
            (Control::Settings, "glyph-gear", "Paramètres"),
        ] {
            let tile = layout.control_row(control);
            canvas.module(tile);
            let center = (tile.x + 28.0, tile.y + tile.h / 2.0);
            canvas.toggle_circle(center, glyph, Some(false));
            // Le titre garde une marge à droite : un peu plus petit s'il le faut.
            let (x, room) = (tile.x + 52.0, tile.w - 52.0 - 14.0);
            let mut size = 13.5;
            while size > 11.0 && canvas.text_width(&self.bold, title, size) > room {
                size -= 0.5;
            }
            canvas.label(
                &self.bold,
                title,
                x,
                center.1 - size as f64 * 0.66,
                size,
                text_strong(),
            );
        }

        let sliders = [
            (Control::Brightness, "Moniteur", "glyph-sun", view.brightness),
            (Control::Volume, "Son", "glyph-speaker", view.volume),
        ];
        for (control, title, glyph, value) in sliders {
            // Pas de module pour un réglage que la machine n'offre pas.
            let Some(value) = value else { continue };
            let module = layout.control_row(control);
            canvas.module(module);
            canvas.label(
                &self.bold,
                title,
                module.x + 15.0,
                module.y + 10.0,
                14.0,
                text_strong(),
            );
            canvas.slider(layout.slider_track(control), value, glyph);
        }

        if let Some(error) = &view.error {
            let line = layout.controls_error();
            canvas.label(
                &self.regular,
                &ellipsize(error, 46),
                line.x,
                line.y + 2.0,
                10.0,
                destructive(),
            );
        }
    }

    fn draw_session(&self, canvas: &mut Canvas, layout: &Layout) {
        canvas.card(layout.session(), 12.0);
        let rows = [
            (PowerAction::Restart, "status-update", "Redémarrer…"),
            (PowerAction::PowerOff, "status-power", "Éteindre…"),
        ];
        for (index, (_, icon, text)) in rows.into_iter().enumerate() {
            let row = layout.session_row(index);
            canvas.icon(
                icon,
                (row.x + 20.0, row.y + row.h / 2.0),
                26.0,
                Style::Symbolic(primary_text()),
            );
            canvas.label(
                &self.regular,
                text,
                row.x + 40.0,
                row.y + 10.0,
                11.5,
                primary_text(),
            );
        }
    }

    fn draw_dock_menu(&self, canvas: &mut Canvas, layout: &Layout, menu: &DockMenuView) {
        canvas.card(layout.dock_menu(menu), 10.0);
        for (index, entry) in menu.entries.iter().enumerate() {
            let row = layout.dock_menu_row(menu, index);
            canvas.label(
                &self.regular,
                entry,
                row.x + 12.0,
                row.y + 8.0,
                11.5,
                primary_text(),
            );
        }
    }

    /// Historique du presse-papiers, comme Windows + V.
    fn draw_clipboard(&self, canvas: &mut Canvas, layout: &Layout, view: &ClipboardView) {
        let card = layout.clipboard();
        canvas.card(card, 14.0);
        canvas.label(
            &self.bold,
            "Presse-papiers",
            card.x + 16.0,
            card.y + 16.0,
            13.0,
            primary_text(),
        );
        if view.items.is_empty() {
            let lines = [
                (&self.bold, "Rien n’a encore été copié", primary_text(), 12.0),
                (
                    &self.regular,
                    "Le texte et les images copiés apparaîtront ici.",
                    text_secondary(),
                    11.0,
                ),
            ];
            for (index, (font, text, color, size)) in lines.into_iter().enumerate() {
                let width = canvas.text_width(font, text, size);
                let y = card.y + card.h / 2.0 - 24.0 + index as f64 * 24.0;
                canvas.label(font, text, card.x + (card.w - width) / 2.0, y, size, color);
            }
            return;
        }
        let clear = layout.clipboard_clear();
        let width = canvas.text_width(&self.regular, "Tout effacer", 11.0);
        canvas.label(
            &self.regular,
            "Tout effacer",
            clear.right() - width - 6.0,
            clear.y + 6.0,
            11.0,
            accent(),
        );
        for (slot, item) in view.items.iter().enumerate() {
            let row = layout.clipboard_row(slot);
            let selected = item.index == view.selected;
            canvas.fill(
                row,
                9.0,
                if selected {
                    Color::rgb(0xDCE8F8)
                } else {
                    Color::rgb(0xF0F0F2)
                },
            );
            if selected {
                canvas.fill(Rect::new(row.x, row.y + 12.0, 3.0, row.h - 24.0), 1.5, accent());
            }
            match &item.preview {
                ClipPreview::Text(lines) => {
                    for (number, line) in lines.iter().take(2).enumerate() {
                        let text = ellipsize_to(canvas, &self.regular, line, 11.5, row.w - 96.0);
                        canvas.label(
                            &self.regular,
                            &text,
                            row.x + 14.0,
                            row.y + 14.0 + number as f64 * 21.0,
                            11.5,
                            primary_text(),
                        );
                    }
                }
                ClipPreview::Image(thumbnail) => {
                    canvas.image(
                        Rect::new(row.x + 12.0, row.y + 7.0, row.w - 96.0, row.h - 14.0),
                        thumbnail,
                    );
                }
            }
            // Épingle (bleue si l'élément est épinglé) et croix de suppression.
            let pin = layout.clipboard_pin(slot);
            let ink = if item.pinned { accent() } else { text_secondary() };
            let (cx, cy) = pin.center();
            canvas.fill(Rect::new(cx - 5.0, cy - 8.0, 10.0, 10.0), 5.0, ink);
            canvas.fill(Rect::new(cx - 6.0, cy + 1.0, 12.0, 2.0), 1.0, ink);
            canvas.fill(Rect::new(cx - 0.9, cy + 2.0, 1.8, 7.0), 0.9, ink);
            let delete = layout.clipboard_delete(slot);
            let width = canvas.text_width(&self.regular, "×", 16.0);
            canvas.label(
                &self.regular,
                "×",
                delete.center().0 - width / 2.0,
                delete.y + 3.0,
                16.0,
                text_secondary(),
            );
        }
        if view.more > 0 {
            let text = if view.more == 1 {
                "1 autre élément plus bas".to_owned()
            } else {
                format!("{} autres éléments plus bas", view.more)
            };
            let width = canvas.text_width(&self.regular, &text, 10.0);
            canvas.label(
                &self.regular,
                &text,
                card.x + (card.w - width) / 2.0,
                card.bottom() - 22.0,
                10.0,
                text_secondary(),
            );
        }
    }

    fn draw_dialog(&self, canvas: &mut Canvas, layout: &Layout, power: PowerAction) {
        canvas.fill(
            Rect::new(0.0, 0.0, layout.w, layout.h),
            0.0,
            Color::BLACK.with_alpha(70),
        );
        let dialog = layout.dialog();
        canvas.card(dialog, 14.0);
        canvas.icon(
            "status-power",
            (dialog.x + 52.0, dialog.y + 54.0),
            64.0,
            Style::Symbolic(primary_text()),
        );
        let (title, confirm, color) = match power {
            PowerAction::PowerOff => ("Éteindre l’ordinateur ?", "Éteindre", destructive()),
            PowerAction::Restart => ("Redémarrer l’ordinateur ?", "Redémarrer", accent()),
        };
        canvas.label(
            &self.bold,
            title,
            dialog.x + 96.0,
            dialog.y + 30.0,
            15.0,
            primary_text(),
        );
        canvas.label(
            &self.regular,
            "Les applications ouvertes seront fermées.",
            dialog.x + 96.0,
            dialog.y + 58.0,
            11.0,
            text_secondary(),
        );
        canvas.button(
            layout.dialog_cancel(),
            "Annuler",
            Color::rgb(0xE8E8E8),
            primary_text(),
        );
        canvas.button(layout.dialog_confirm(), confirm, color, Color::WHITE);
    }
}

/// Hit-test du bureau ; les panneaux interceptent leurs propres surfaces pour que le clic
/// ne tombe jamais sur une fenêtre située derrière eux.
pub fn action_at(
    output: Rectangle<i32, Logical>,
    pos: Point<f64, Logical>,
    view: &DesktopView,
) -> Option<DesktopAction> {
    let layout = Layout::new(output.size.w as f64, output.size.h as f64, view);
    let (popover, pending_power) = (view.popover, view.power);
    if !view.bar_visible && pending_power.is_none() && popover.is_none() && pos.y < view.bar_height as f64 {
        // Barre masquée (plein écran) : le clic va à l'application.
        return None;
    }
    let pos = Point::from((pos.x - output.loc.x as f64, pos.y - output.loc.y as f64));

    if popover.is_none()
        && pending_power.is_none()
        && view.page == 0
        && let Some(action) = {
            let (dashboard_layout, scale) =
                dashboard_layout(output.size.w as f64, output.size.h as f64, view);
            let dashboard_pos = Point::from((pos.x / scale, pos.y / scale));
            dashboard_action_at(&dashboard_layout, view, dashboard_pos)
        }
    {
        return Some(action);
    }

    if let Some(power) = pending_power {
        return Some(if layout.dialog_cancel().contains(pos) {
            DesktopAction::CancelPower
        } else if layout.dialog_confirm().contains(pos) {
            DesktopAction::ConfirmPower(power)
        } else if layout.dialog().contains(pos) {
            DesktopAction::Consume
        } else {
            DesktopAction::CancelPower
        });
    }

    if pos.y < view.bar_height as f64 {
        if layout.applications_button().contains(pos) {
            return Some(DesktopAction::TogglePopover(Popover::Launcher));
        }
        if let Some(page) = (0..view.page_count).find(|&page| layout.page_dot(page).contains(pos)) {
            return Some(DesktopAction::GoToPage(page));
        }
        if layout.indicator(Indicator::Power).contains(pos) {
            return Some(DesktopAction::TogglePopover(Popover::Session));
        }
        if layout.indicators_group().contains(pos) {
            return Some(DesktopAction::TogglePopover(Popover::Controls));
        }
        return Some(if popover.is_some() {
            DesktopAction::ClosePopover
        } else {
            DesktopAction::Consume
        });
    }

    let popover = popover?;
    let action = match popover {
        Popover::DockMenu => match &view.dock_menu {
            Some(menu) if layout.dock_menu(menu).contains(pos) => (0..menu.entries.len())
                .find(|&index| layout.dock_menu_row(menu, index).contains(pos))
                .map_or(DesktopAction::Consume, DesktopAction::DockMenu),
            _ => DesktopAction::ClosePopover,
        },
        Popover::Launcher if layout.launcher().contains(pos) => {
            let launcher = &view.launcher;
            if !layout.launcher_content().contains(pos) {
                DesktopAction::Consume
            } else {
                layout
                    .launcher_items(launcher)
                    .into_iter()
                    .find_map(|item| match item {
                        LauncherItem::Tile { section, slot, rect } if rect.contains(pos) => Some(
                            DesktopAction::LaunchTile(launcher.sections[section].tiles[slot].index),
                        ),
                        LauncherItem::Link { link, rect } if rect.contains(pos) => {
                            Some(DesktopAction::LauncherLink(link))
                        }
                        _ => None,
                    })
                    .unwrap_or(DesktopAction::Consume)
            }
        }
        Popover::Controls if layout.controls().contains(pos) => {
            let on = |control| layout.control_row(control).contains(pos);
            let percent = |control| slider_percent(layout.slider_track(control), pos.x);
            if on(Control::Wifi) && view.wifi.is_some() {
                DesktopAction::ToggleWifi
            } else if on(Control::Bluetooth) && view.bluetooth.is_some() {
                DesktopAction::ToggleBluetooth
            } else if on(Control::Airplane) && airplane(view).is_some() {
                DesktopAction::ToggleAirplane
            } else if on(Control::Clipboard) {
                DesktopAction::TogglePopover(Popover::Clipboard)
            } else if on(Control::Settings) {
                DesktopAction::Launch(App::Settings)
            } else if on(Control::Volume) && view.volume.is_some() {
                DesktopAction::SetVolume(percent(Control::Volume))
            } else if on(Control::Brightness) && view.brightness.is_some() {
                DesktopAction::SetBrightness(percent(Control::Brightness))
            } else {
                DesktopAction::Consume
            }
        }
        Popover::Clipboard if layout.clipboard().contains(pos) => {
            let clipboard = &view.clipboard;
            if layout.clipboard_header().contains(pos) {
                let card = layout.clipboard();
                DesktopAction::ClipboardDragStart(
                    (pos.x - card.x).round() as i32,
                    (pos.y - card.y).round() as i32,
                )
            } else {
                let row = clipboard
                    .items
                    .iter()
                    .enumerate()
                    .find(|(slot, _)| layout.clipboard_row(*slot).contains(pos));
                if !clipboard.items.is_empty() && layout.clipboard_clear().contains(pos) {
                    DesktopAction::ClipboardClear
                } else if let Some((slot, item)) = row {
                    if layout.clipboard_pin(slot).contains(pos) {
                        DesktopAction::ClipboardPin(item.index)
                    } else if layout.clipboard_delete(slot).contains(pos) {
                        DesktopAction::ClipboardDelete(item.index)
                    } else {
                        DesktopAction::ClipboardPaste(item.index)
                    }
                } else {
                    DesktopAction::Consume
                }
            }
        }
        Popover::Session if layout.session().contains(pos) => {
            if layout.session_row(0).contains(pos) {
                DesktopAction::RequestPower(PowerAction::Restart)
            } else if layout.session_row(1).contains(pos) {
                DesktopAction::RequestPower(PowerAction::PowerOff)
            } else {
                DesktopAction::Consume
            }
        }
        _ => DesktopAction::ClosePopover,
    };
    Some(action)
}

fn dashboard_layout(width: f64, height: f64, view: &DesktopView) -> (Layout, f64) {
    let scale = dashboard_scale(width, height);
    let mut scaled_view = view.clone();
    scaled_view.bar_height = (view.bar_height as f64 / scale).round() as u32;
    scaled_view.dock_height = (view.dock_height as f64 / scale).round() as i32;
    (Layout::new(width / scale, height / scale, &scaled_view), scale)
}

fn dashboard_scale(width: f64, height: f64) -> f64 {
    (width / 1152.0).min(height / 768.0).clamp(0.75, 2.0)
}

fn dashboard_action_at(
    layout: &Layout,
    view: &DesktopView,
    pos: Point<f64, Logical>,
) -> Option<DesktopAction> {
    let [clock, weather, calendar, notes, tasks, system, shortcuts, quote] = layout.dashboard_cards();
    if clock.contains(pos) {
        if Rect::new(clock.right() - 48.0, clock.y + 8.0, 40.0, 40.0).contains(pos) {
            return Some(DesktopAction::DashboardLaunch(App::Settings));
        }
        return Some(DesktopAction::DashboardConsume);
    }
    if weather.contains(pos) {
        return Some(DesktopAction::DashboardConsume);
    }
    if calendar.contains(pos) {
        if Rect::new(calendar.right() - 66.0, calendar.y + 8.0, 24.0, 38.0).contains(pos) {
            return Some(DesktopAction::DashboardMonth(-1));
        }
        if Rect::new(calendar.right() - 38.0, calendar.y + 8.0, 24.0, 38.0).contains(pos) {
            return Some(DesktopAction::DashboardMonth(1));
        }
        return Some(DesktopAction::DashboardConsume);
    }
    if notes.contains(pos) {
        if Rect::new(notes.right() - 48.0, notes.y + 7.0, 42.0, 42.0).contains(pos) {
            return Some(DesktopAction::DashboardAddNote);
        }
        let row_height = (notes.bottom() - (notes.y + 56.0) - 8.0).max(0.0) / 4.0;
        if row_height > 0.0 && pos.y >= notes.y + 56.0 {
            let index = ((pos.y - notes.y - 56.0) / row_height).floor() as usize;
            if index < view.dashboard.notes.len().min(4) {
                return Some(DesktopAction::DashboardEditNote(index));
            }
        }
        return Some(DesktopAction::DashboardConsume);
    }
    if tasks.contains(pos) {
        let add = Rect::new(tasks.right() - 48.0, tasks.y + 7.0, 42.0, 42.0);
        if add.contains(pos) {
            return Some(DesktopAction::DashboardAddTask);
        }
        let row_top = tasks.y + 53.0;
        let row_height = ((tasks.bottom() - row_top - 8.0) / 6.0).clamp(25.0, 34.0);
        if pos.y >= row_top {
            let index = ((pos.y - row_top) / row_height).floor() as usize;
            if let Some(task) = view.dashboard.tasks.get(index) {
                if pos.x >= tasks.right() - 100.0 {
                    return Some(DesktopAction::DashboardCyclePriority(index));
                }
                if !task.text.is_empty() {
                    return Some(DesktopAction::DashboardToggleTask(index));
                }
            }
        }
        return Some(DesktopAction::DashboardConsume);
    }
    if system.contains(pos) {
        return Some(DesktopAction::DashboardConsume);
    }
    if shortcuts.contains(pos) {
        let item_width = (shortcuts.w - 2.0 * SHORTCUTS_PAD) / 6.0;
        let index = ((pos.x - shortcuts.x - SHORTCUTS_PAD) / item_width).floor();
        if index < 0.0 || pos.y < shortcuts.y + 42.0 {
            return Some(DesktopAction::DashboardConsume);
        }
        return (index < 6.0)
            .then_some(DesktopAction::DashboardShortcut(index as usize))
            .or(Some(DesktopAction::DashboardConsume));
    }
    if quote.contains(pos) {
        return Some(DesktopAction::DashboardConsume);
    }
    None
}

/// Élément du lanceur, placé par [`launcher_flow`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum LauncherItem {
    Title {
        section: usize,
        rect: Rect,
    },
    Link {
        link: SectionLink,
        rect: Rect,
    },
    /// Case `slot` de la rubrique `section`.
    Tile {
        section: usize,
        slot: usize,
        rect: Rect,
    },
    Rule(Rect),
}

impl LauncherItem {
    /// Au moins en partie dans la zone visible `content` (seuls ceux-là sont dessinés).
    fn visible_in(&self, content: Rect) -> bool {
        let (LauncherItem::Title { rect, .. }
        | LauncherItem::Link { rect, .. }
        | LauncherItem::Tile { rect, .. }
        | LauncherItem::Rule(rect)) = *self;
        rect.bottom() > content.y && rect.y < content.bottom()
    }
}

/// Rubriques les unes sous les autres, relatives au coin du contenu (marge comprise) ;
/// renvoie aussi la hauteur totale.
fn launcher_flow(launcher: &LauncherView) -> (Vec<LauncherItem>, f64) {
    let width = LAUNCHER_COLUMNS as f64 * LAUNCHER_CELL.0;
    let mut items = Vec::new();
    let mut y = 4.0;
    for (index, section) in launcher.sections.iter().enumerate() {
        if index > 0 {
            items.push(LauncherItem::Rule(Rect::new(
                0.0,
                y + LAUNCHER_GAP / 2.0,
                width,
                1.0,
            )));
            y += LAUNCHER_GAP;
        }
        items.push(LauncherItem::Title {
            section: index,
            rect: Rect::new(0.0, y, width - 80.0, LAUNCHER_TITLE),
        });
        if let Some(link) = section.link {
            items.push(LauncherItem::Link {
                link,
                rect: Rect::new(width - 80.0, y, 80.0, LAUNCHER_TITLE),
            });
        }
        y += LAUNCHER_TITLE;
        for slot in 0..section.tiles.len() {
            let (column, row) = (slot % LAUNCHER_COLUMNS, slot / LAUNCHER_COLUMNS);
            items.push(LauncherItem::Tile {
                section: index,
                slot,
                rect: Rect::new(
                    column as f64 * LAUNCHER_CELL.0,
                    y + row as f64 * LAUNCHER_CELL.1,
                    LAUNCHER_CELL.0,
                    LAUNCHER_CELL.1,
                ),
            });
        }
        y += section.tiles.len().div_ceil(LAUNCHER_COLUMNS) as f64 * LAUNCHER_CELL.1;
    }
    (items, y + 8.0)
}

/// Défilement maximal du lanceur, en pixels logiques (pour borner la molette).
pub fn launcher_scroll_limit(output: Rectangle<i32, Logical>, view: &DesktopView) -> u32 {
    let layout = Layout::new(output.size.w as f64, output.size.h as f64, view);
    let (_, height) = launcher_flow(&view.launcher);
    (height - layout.launcher_content().h).max(0.0).ceil() as u32
}

/// Couleur du fond flouté au point (`u`, `v`) de l'écran (0 à 1), interpolée.
fn sample(image: &Image, u: f64, v: f64) -> [f32; 3] {
    let x = (u * image.width as f64 - 0.5).clamp(0.0, image.width as f64 - 1.0);
    let y = (v * image.height as f64 - 0.5).clamp(0.0, image.height as f64 - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(image.width - 1), (y0 + 1).min(image.height - 1));
    let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
    let at =
        |x: u32, y: u32, channel: usize| image.rgba[((y * image.width + x) * 4) as usize + channel] as f32;
    [0, 1, 2].map(|c| {
        let top = at(x0, y0, c) + (at(x1, y0, c) - at(x0, y0, c)) * fx;
        let bottom = at(x0, y1, c) + (at(x1, y1, c) - at(x0, y1, c)) * fx;
        top + (bottom - top) * fy
    })
}

/// Nom d'une application sur deux lignes au plus, coupé entre les mots ; la seconde
/// ligne se termine par « … » si elle déborde.
fn wrap_two_lines(canvas: &Canvas, font: &Font, text: &str, size: f32, width: f64) -> Vec<String> {
    if canvas.text_width(font, text, size) <= width {
        return vec![text.to_owned()];
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut first = String::new();
    let mut rest = 0;
    for (index, word) in words.iter().enumerate() {
        let candidate = if first.is_empty() {
            (*word).to_owned()
        } else {
            format!("{first} {word}")
        };
        if canvas.text_width(font, &candidate, size) > width && !first.is_empty() {
            break;
        }
        first = candidate;
        rest = index + 1;
    }
    let first = ellipsize_to(canvas, font, &first, size, width);
    let second = words[rest..].join(" ");
    if second.is_empty() {
        vec![first]
    } else {
        vec![first, ellipsize_to(canvas, font, &second, size, width)]
    }
}

/// Curseur des réglages rapides que l'on peut faire glisser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slider {
    Volume,
    Brightness,
}

/// Valeur (0 à 100) d'un curseur des réglages rapides pour le pointeur en `pos`, même
/// hors du curseur pendant un glissement.
pub fn slider_value(
    output: Rectangle<i32, Logical>,
    view: &DesktopView,
    slider: Slider,
    pos: Point<f64, Logical>,
) -> u8 {
    let layout = Layout::new(output.size.w as f64, output.size.h as f64, view);
    let control = match slider {
        Slider::Volume => Control::Volume,
        Slider::Brightness => Control::Brightness,
    };
    slider_percent(layout.slider_track(control), pos.x - output.loc.x as f64)
}

/// Valeur d'un curseur pour l'abscisse `x` : le bouton reste entier dans la piste, d'où
/// la course réduite d'un demi-bouton de chaque côté.
fn slider_percent(track: Rect, x: f64) -> u8 {
    let knob = track.h;
    (((x - track.x - knob / 2.0) / (track.w - knob)).clamp(0.0, 1.0) * 100.0).round() as u8
}

/// Mode avion : actif quand toutes les radios présentes sont coupées ; `None` sans radio.
fn airplane(view: &DesktopView) -> Option<bool> {
    let radios = [view.wifi, view.bluetooth];
    radios
        .iter()
        .any(Option::is_some)
        .then(|| !radios.contains(&Some(true)))
}

/// Indicateurs de la barre, de gauche à droite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Indicator {
    Bluetooth,
    /// Wi-Fi s'il y a une carte, réseau filaire sinon : toujours présent.
    Network,
    Volume,
    Battery,
    Power,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Wifi,
    Bluetooth,
    Airplane,
    Clipboard,
    Settings,
    Brightness,
    Volume,
}

/// Géométrie du bureau, en pixels logiques relatifs à l'écran.
struct Layout {
    w: f64,
    h: f64,
    bar: f64,
    dock: f64,
    clipboard_position: Option<(i32, i32)>,
    battery: bool,
    bluetooth: bool,
    volume: bool,
    brightness: bool,
    error: bool,
}

impl Layout {
    fn new(w: f64, h: f64, view: &DesktopView) -> Self {
        Self {
            w,
            h,
            bar: view.bar_height as f64,
            dock: view.dock_height as f64,
            clipboard_position: view.clipboard.position,
            battery: battery_dir().is_some(),
            bluetooth: view.bluetooth.is_some(),
            volume: view.volume.is_some(),
            brightness: view.brightness.is_some(),
            error: view.error.is_some(),
        }
    }

    fn dashboard_cards(&self) -> [Rect; 8] {
        let margin = 24.0;
        let gap = 12.0;
        let width = (self.w - margin * 2.0).max(1.0);
        let top = self.bar + 16.0;
        // Le Dock reste entièrement visible sous les widgets.
        let height = (self.h - top - margin.max(self.dock + gap)).max(1.0);
        let rows_height = (height - gap * 2.0).max(3.0);
        let row_one = rows_height * 0.38;
        let row_two = rows_height * 0.41;
        let row_three = rows_height - row_one - row_two;
        let third = (width - 2.0 * gap) / 3.0;
        let shortcuts_width = (width - gap) * 0.58;
        let quote_width = width - gap - shortcuts_width;
        [
            Rect::new(margin, top, third, row_one),
            Rect::new(margin + third + gap, top, third, row_one),
            Rect::new(margin + 2.0 * (third + gap), top, third, row_one),
            Rect::new(margin, top + row_one + gap, third, row_two),
            Rect::new(margin + third + gap, top + row_one + gap, third, row_two),
            Rect::new(margin + 2.0 * (third + gap), top + row_one + gap, third, row_two),
            Rect::new(
                margin,
                top + row_one + gap + row_two + gap,
                shortcuts_width,
                row_three,
            ),
            Rect::new(
                margin + shortcuts_width + gap,
                top + row_one + gap + row_two + gap,
                quote_width,
                row_three,
            ),
        ]
    }

    /// Marque d'une page dans la barre, juste après « Applications ».
    fn page_dot(&self, page: usize) -> Rect {
        Rect::new(140.0 + page as f64 * 16.0, 0.0, 16.0, self.bar)
    }

    fn applications_button(&self) -> Rect {
        // Logo, « Applications » en gras, et une marge de chaque côté.
        Rect::new(4.0, 0.0, 132.0, self.bar)
    }

    /// Indicateurs présents, de gauche à droite (l'alimentation à part, tout à droite).
    fn present(&self) -> Vec<Indicator> {
        let mut list = Vec::new();
        if self.bluetooth {
            list.push(Indicator::Bluetooth);
        }
        list.push(Indicator::Network);
        if self.volume {
            list.push(Indicator::Volume);
        }
        if self.battery {
            list.push(Indicator::Battery);
        }
        list
    }

    /// Case d'un indicateur ; vide (largeur nulle) s'il est absent.
    fn indicator(&self, indicator: Indicator) -> Rect {
        let power = Rect::new(self.w - 6.0 - INDICATOR_WIDTH, 0.0, INDICATOR_WIDTH, self.bar);
        if indicator == Indicator::Power {
            return power;
        }
        // Le pourcentage de batterie est écrit à droite de son icône.
        let battery_text = if self.battery { 46.0 } else { 0.0 };
        let present = self.present();
        let Some(index) = present.iter().position(|known| *known == indicator) else {
            return Rect::new(power.x, 0.0, 0.0, 0.0);
        };
        let from_right = (present.len() - index) as f64;
        Rect::new(
            power.x - 8.0 - battery_text - from_right * INDICATOR_WIDTH,
            0.0,
            INDICATOR_WIDTH,
            self.bar,
        )
    }

    fn indicators(&self) -> Vec<(Indicator, Rect)> {
        let mut list = self.present();
        list.push(Indicator::Power);
        list.into_iter()
            .map(|indicator| (indicator, self.indicator(indicator)))
            .collect()
    }

    /// Indicateurs qui ouvrent les réglages rapides (tous sauf l'alimentation).
    fn indicators_group(&self) -> Rect {
        let first = self.indicator(self.present()[0]);
        let end = self.indicator(Indicator::Power).x - 4.0;
        Rect::new(first.x, 0.0, end - first.x, self.bar)
    }

    /// Lanceur, centré dans l'écran (un peu au-dessus du milieu, comme sur macOS).
    fn launcher(&self) -> Rect {
        let w = 2.0 * LAUNCHER_PAD + LAUNCHER_COLUMNS as f64 * LAUNCHER_CELL.0;
        let h = LAUNCHER_MAX_HEIGHT
            .min(self.h - self.bar - 120.0)
            .max(LAUNCHER_HEADER + 160.0);
        Rect::new(
            ((self.w - w) / 2.0).max(POPOVER_MARGIN),
            self.bar + ((self.h - self.bar - h) * 0.42).max(POPOVER_MARGIN),
            w,
            h,
        )
    }

    /// Partie qui défile, sous le champ de recherche.
    fn launcher_content(&self) -> Rect {
        let card = self.launcher();
        Rect::new(
            card.x,
            card.y + LAUNCHER_HEADER,
            card.w,
            card.h - LAUNCHER_HEADER - 10.0,
        )
    }

    /// Titres, liens, traits et cases des rubriques, à leur place (défilement compris).
    fn launcher_items(&self, launcher: &LauncherView) -> Vec<LauncherItem> {
        let (mut items, height) = launcher_flow(launcher);
        let content = self.launcher_content();
        let scroll = (launcher.scroll as f64).min((height - content.h).max(0.0));
        let (dx, dy) = (content.x + LAUNCHER_PAD, content.y - scroll);
        for item in &mut items {
            let rect = match item {
                LauncherItem::Title { rect, .. }
                | LauncherItem::Link { rect, .. }
                | LauncherItem::Tile { rect, .. }
                | LauncherItem::Rule(rect) => rect,
            };
            rect.x += dx;
            rect.y += dy;
        }
        items
    }

    fn controls(&self) -> Rect {
        let x = (self.w - POPOVER_MARGIN - CONTROLS_WIDTH).max(POPOVER_MARGIN);
        let sliders = [self.brightness, self.volume]
            .into_iter()
            .filter(|&present| present)
            .count();
        let error = if self.error { 22.0 } else { 0.0 };
        let h = CONTROLS_GAP
            + CONTROLS_RADIOS
            + CONTROLS_GAP
            + sliders as f64 * (CONTROLS_SLIDER + CONTROLS_GAP)
            + error;
        Rect::new(x, self.bar + POPOVER_MARGIN, CONTROLS_WIDTH, h)
    }

    /// Module de gauche : Wi-Fi, Bluetooth et mode avion.
    fn controls_radios(&self) -> Rect {
        let card = self.controls();
        let half = (card.w - 3.0 * CONTROLS_GAP) / 2.0;
        Rect::new(
            card.x + CONTROLS_GAP,
            card.y + CONTROLS_GAP,
            half,
            CONTROLS_RADIOS,
        )
    }

    /// Ligne, case ou module d'un réglage ; vide si le module est absent.
    fn control_row(&self, control: Control) -> Rect {
        let card = self.controls();
        let radios = self.controls_radios();
        let tile_h = (CONTROLS_RADIOS - CONTROLS_GAP) / 2.0;
        let right = radios.right() + CONTROLS_GAP;
        let radio = |index: f64| {
            let h = (radios.h - 14.0) / 3.0;
            Rect::new(radios.x, radios.y + 7.0 + index * h, radios.w, h)
        };
        let slider = |index: f64| {
            Rect::new(
                card.x + CONTROLS_GAP,
                radios.bottom() + CONTROLS_GAP + index * (CONTROLS_SLIDER + CONTROLS_GAP),
                card.w - 2.0 * CONTROLS_GAP,
                CONTROLS_SLIDER,
            )
        };
        match control {
            Control::Wifi => radio(0.0),
            Control::Bluetooth => radio(1.0),
            Control::Airplane => radio(2.0),
            Control::Clipboard => Rect::new(right, radios.y, radios.w, tile_h),
            Control::Settings => Rect::new(right, radios.bottom() - tile_h, radios.w, tile_h),
            Control::Brightness if self.brightness => slider(0.0),
            Control::Volume if self.volume => slider(if self.brightness { 1.0 } else { 0.0 }),
            Control::Brightness | Control::Volume => Rect::new(card.x, card.y, 0.0, 0.0),
        }
    }

    /// Ligne du message d'erreur, sous le dernier module.
    fn controls_error(&self) -> Rect {
        let card = self.controls();
        Rect::new(card.x + 14.0, card.bottom() - 24.0, card.w - 28.0, 20.0)
    }

    /// Piste d'un curseur, sous le titre de son module.
    fn slider_track(&self, control: Control) -> Rect {
        let module = self.control_row(control);
        Rect::new(module.x + 12.0, module.y + 36.0, module.w - 24.0, 26.0)
    }

    /// Menu d'une icône du Dock, au-dessus d'elle (sans sortir de l'écran).
    fn dock_menu(&self, menu: &DockMenuView) -> Rect {
        let (w, h) = (210.0, 12.0 + menu.entries.len() as f64 * 32.0);
        let x = (menu.x - w / 2.0).clamp(POPOVER_MARGIN, (self.w - w - POPOVER_MARGIN).max(POPOVER_MARGIN));
        Rect::new(x, (menu.bottom - h - 10.0).max(self.bar + POPOVER_MARGIN), w, h)
    }

    fn dock_menu_row(&self, menu: &DockMenuView, index: usize) -> Rect {
        let card = self.dock_menu(menu);
        Rect::new(
            card.x + 6.0,
            card.y + 6.0 + index as f64 * 32.0,
            card.w - 12.0,
            32.0,
        )
    }

    /// Panneau Super+V : position personnalisée ou emplacement par défaut en bas à droite.
    fn clipboard(&self) -> Rect {
        let h = CLIPBOARD_HEADER + CLIPBOARD_VISIBLE as f64 * (CLIPBOARD_ROW + CLIPBOARD_GAP) + 26.0;
        if let Some((x, y)) = self.clipboard_position {
            return Rect::new(x as f64, y as f64, CLIPBOARD_WIDTH, h);
        }
        Rect::new(
            (self.w - POPOVER_MARGIN - 10.0 - CLIPBOARD_WIDTH).max(POPOVER_MARGIN),
            (self.h - h - 96.0).max(self.bar + POPOVER_MARGIN),
            CLIPBOARD_WIDTH,
            h,
        )
    }

    fn clipboard_header(&self) -> Rect {
        let card = self.clipboard();
        Rect::new(card.x, card.y, card.w, CLIPBOARD_HEADER)
    }

    fn clipboard_row(&self, slot: usize) -> Rect {
        let card = self.clipboard();
        Rect::new(
            card.x + 10.0,
            card.y + CLIPBOARD_HEADER + slot as f64 * (CLIPBOARD_ROW + CLIPBOARD_GAP),
            card.w - 20.0,
            CLIPBOARD_ROW,
        )
    }

    fn clipboard_pin(&self, slot: usize) -> Rect {
        let row = self.clipboard_row(slot);
        Rect::new(row.right() - 68.0, row.y + 6.0, 30.0, 30.0)
    }

    fn clipboard_delete(&self, slot: usize) -> Rect {
        let row = self.clipboard_row(slot);
        Rect::new(row.right() - 36.0, row.y + 6.0, 30.0, 30.0)
    }

    fn clipboard_clear(&self) -> Rect {
        let card = self.clipboard();
        Rect::new(card.right() - 120.0, card.y + 10.0, 110.0, 30.0)
    }

    fn session(&self) -> Rect {
        let x = (self.w - POPOVER_MARGIN - SESSION_WIDTH).max(POPOVER_MARGIN);
        Rect::new(x, self.bar + POPOVER_MARGIN, SESSION_WIDTH, 84.0)
    }

    fn session_row(&self, index: usize) -> Rect {
        let card = self.session();
        Rect::new(
            card.x + 6.0,
            card.y + 6.0 + index as f64 * 36.0,
            card.w - 12.0,
            36.0,
        )
    }

    fn dialog(&self) -> Rect {
        Rect::new(
            ((self.w - DIALOG.0) / 2.0).max(8.0),
            ((self.h - DIALOG.1) * 0.4).max(self.bar + 8.0),
            DIALOG.0,
            DIALOG.1,
        )
    }

    fn dialog_confirm(&self) -> Rect {
        let dialog = self.dialog();
        Rect::new(
            dialog.right() - 20.0 - 120.0,
            dialog.bottom() - 20.0 - 34.0,
            120.0,
            34.0,
        )
    }

    fn dialog_cancel(&self) -> Rect {
        let confirm = self.dialog_confirm();
        Rect::new(confirm.x - 10.0 - 104.0, confirm.y, 104.0, confirm.h)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    fn right(self) -> f64 {
        self.x + self.w
    }

    fn bottom(self) -> f64 {
        self.y + self.h
    }

    fn center(self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    fn contains(self, p: Point<f64, Logical>) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }
}

/// Tampon de dessin BGRA prémultiplié ; les coordonnées des méthodes sont logiques.
struct Canvas<'a> {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    scale: f32,
    bold: &'a Font,
    icons: &'a IconCache,
}

impl Canvas<'_> {
    fn fill(&mut self, rect: Rect, radius: f32, color: Color) {
        let s = self.scale;
        let (x, y, w, h) = (
            rect.x as f32 * s,
            rect.y as f32 * s,
            rect.w as f32 * s,
            rect.h as f32 * s,
        );
        let radius = (radius * s).clamp(0.0, w.min(h) * 0.5);
        let left = (x - 1.0).floor().max(0.0) as u32;
        let top = (y - 1.0).floor().max(0.0) as u32;
        let right = (x + w + 1.0).ceil().clamp(0.0, self.width as f32) as u32;
        let bottom = (y + h + 1.0).ceil().clamp(0.0, self.height as f32) as u32;
        for py in top..bottom {
            for px in left..right {
                let qx = (px as f32 + 0.5 - (x + w * 0.5)).abs() - (w * 0.5 - radius);
                let qy = (py as f32 + 0.5 - (y + h * 0.5)).abs() - (h * 0.5 - radius);
                let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                self.blend(px, py, color, (0.5 - distance).clamp(0.0, 1.0));
            }
        }
    }

    fn stroke(&mut self, rect: Rect, radius: f32, color: Color, width: f64) {
        let s = self.scale as f64;
        let half = width * 0.5;
        let left = ((rect.x - half) * s).floor().max(0.0) as u32;
        let top = ((rect.y - half) * s).floor().max(0.0) as u32;
        let right = ((rect.right() + half) * s).ceil().min(self.width as f64) as u32;
        let bottom = ((rect.bottom() + half) * s).ceil().min(self.height as f64) as u32;
        let radius = (radius as f64 * s).clamp(0.0, rect.w.min(rect.h) * s * 0.5);
        for y in top..bottom {
            for x in left..right {
                let px = x as f64 + 0.5;
                let py = y as f64 + 0.5;
                let qx = (px - (rect.x + rect.w * 0.5) * s).abs() - (rect.w * s * 0.5 - radius);
                let qy = (py - (rect.y + rect.h * 0.5) * s).abs() - (rect.h * s * 0.5 - radius);
                let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                let coverage = (half * s + 0.5 - distance.abs()).clamp(0.0, 1.0) as f32;
                if coverage > 0.0 {
                    self.blend(x, y, color, coverage);
                }
            }
        }
    }

    fn line(&mut self, start: (f64, f64), end: (f64, f64), width: f64, color: Color) {
        let s = self.scale as f64;
        let (x1, y1) = (start.0 * s, start.1 * s);
        let (x2, y2) = (end.0 * s, end.1 * s);
        let radius = width * s * 0.5;
        let left = (x1.min(x2) - radius - 1.0).floor().max(0.0) as u32;
        let top = (y1.min(y2) - radius - 1.0).floor().max(0.0) as u32;
        let right = (x1.max(x2) + radius + 1.0).ceil().min(self.width as f64) as u32;
        let bottom = (y1.max(y2) + radius + 1.0).ceil().min(self.height as f64) as u32;
        let (dx, dy) = (x2 - x1, y2 - y1);
        let length_squared = dx * dx + dy * dy;
        for y in top..bottom {
            for x in left..right {
                let px = x as f64 + 0.5 - x1;
                let py = y as f64 + 0.5 - y1;
                let t = if length_squared == 0.0 {
                    0.0
                } else {
                    ((px * dx + py * dy) / length_squared).clamp(0.0, 1.0)
                };
                let distance = (px - dx * t).hypot(py - dy * t);
                let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0) as f32;
                if coverage > 0.0 {
                    self.blend(x, y, color, coverage);
                }
            }
        }
    }

    fn polygon(&mut self, points: &[(f64, f64)], color: Color) {
        if points.len() < 3 {
            return;
        }
        let s = self.scale as f64;
        let min_y = points.iter().map(|point| point.1).fold(f64::INFINITY, f64::min);
        let max_y = points
            .iter()
            .map(|point| point.1)
            .fold(f64::NEG_INFINITY, f64::max);
        let top = (min_y * s).floor().max(0.0) as u32;
        let bottom = (max_y * s).ceil().min(self.height as f64) as u32;
        for y in top..bottom {
            let scan = y as f64 + 0.5;
            let mut intersections = Vec::new();
            for index in 0..points.len() {
                let (x1, y1) = points[index];
                let (x2, y2) = points[(index + 1) % points.len()];
                if (y1 <= scan / s && y2 > scan / s) || (y2 <= scan / s && y1 > scan / s) {
                    let y_logical = scan / s;
                    intersections.push((x1 + (y_logical - y1) * (x2 - x1) / (y2 - y1)) * s);
                }
            }
            intersections.sort_by(f64::total_cmp);
            for pair in intersections.chunks_exact(2) {
                let left = pair[0].ceil().max(0.0) as u32;
                let right = pair[1].ceil().min(self.width as f64) as u32;
                for x in left..right {
                    self.blend(x, y, color, 1.0);
                }
            }
        }
    }

    fn network_chart(&mut self, rect: Rect, samples: &[(u64, u64)]) {
        if samples.is_empty() {
            return;
        }
        let max = samples
            .iter()
            .flat_map(|(download, upload)| [*download, *upload])
            .max()
            .unwrap_or(1)
            .max(1) as f64;
        let point = |index: usize, value: u64| {
            let x = rect.x + rect.w * index as f64 / (samples.len().saturating_sub(1).max(1) as f64);
            let y = rect.bottom() - (value as f64 / max * (rect.h - 4.0)).clamp(2.0, rect.h - 2.0);
            (x, y)
        };
        for index in 1..samples.len() {
            self.line(
                point(index - 1, samples[index - 1].0),
                point(index, samples[index].0),
                1.8,
                Color::rgb(0x2389FF),
            );
            self.line(
                point(index - 1, samples[index - 1].1),
                point(index, samples[index].1),
                1.8,
                Color::rgb(0x1DBC82),
            );
        }
    }

    fn progress_ring(&mut self, center: (f64, f64), radius: f64, thickness: f64, value: u8, color: Color) {
        let scale = self.scale as f64;
        let outer = radius * scale;
        let inner = (radius - thickness) * scale;
        let left = ((center.0 - radius) * scale).floor().max(0.0) as u32;
        let top = ((center.1 - radius) * scale).floor().max(0.0) as u32;
        let right = ((center.0 + radius) * scale).ceil().min(self.width as f64) as u32;
        let bottom = ((center.1 + radius) * scale).ceil().min(self.height as f64) as u32;
        let progress = value.min(100) as f64 / 100.0;
        for y in top..bottom {
            for x in left..right {
                let dx = x as f64 + 0.5 - center.0 * scale;
                let dy = y as f64 + 0.5 - center.1 * scale;
                let distance = dx.hypot(dy);
                if distance < inner || distance > outer {
                    continue;
                }
                let angle = (dy.atan2(dx) + std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU)
                    / std::f64::consts::TAU;
                self.blend(
                    x,
                    y,
                    if angle <= progress {
                        color
                    } else {
                        Color::rgb(0xDCE5EF)
                    },
                    1.0,
                );
            }
        }
    }

    fn fit_text(&self, font: &Font, text: &str, size: f32, max_width: f64) -> String {
        if self.text_width(font, text, size) <= max_width {
            return text.to_owned();
        }
        let mut output = String::new();
        for character in text.chars() {
            let mut candidate = output.clone();
            candidate.push(character);
            candidate.push('…');
            if self.text_width(font, &candidate, size) > max_width {
                break;
            }
            output.push(character);
        }
        output.push('…');
        output
    }

    fn blend(&mut self, x: u32, y: u32, color: Color, coverage: f32) {
        let alpha = (color.a as f32 * coverage).round().clamp(0.0, 255.0) as u32;
        if alpha == 0 {
            return;
        }
        let inverse = 255 - alpha;
        let offset = ((y * self.width + x) * 4) as usize;
        let source = [color.b, color.g, color.r].map(|channel| channel as u32 * alpha / 255);
        let pixel = &mut self.pixels[offset..offset + 4];
        for (index, channel) in source.into_iter().enumerate() {
            pixel[index] = (channel + pixel[index] as u32 * inverse / 255).min(255) as u8;
        }
        pixel[3] = (alpha + pixel[3] as u32 * inverse / 255).min(255) as u8;
    }

    /// Carte claire avec ombre douce et contour discret.
    fn card(&mut self, rect: Rect, radius: f32) {
        self.card_shadow(rect, radius);
        self.fill(rect, radius, card());
    }

    /// Ombre douce et contour discret d'une carte, sans son fond.
    ///
    /// L'ombre empile dix voiles noirs à 5/255, chacun la carte (décalée de 4 vers le bas)
    /// agrandie de 1 à 10 points : leur distance au pixel se déduit d'un seul calcul. Le
    /// dedans, caché par la carte, n'est pas peint ; tout repeindre à chaque image rendait
    /// le défilement du lanceur très lent.
    fn card_shadow(&mut self, rect: Rect, radius: f32) {
        const SPREAD: f32 = 10.0;
        let s = self.scale;
        let (x, y, w, h) = (
            rect.x as f32 * s,
            rect.y as f32 * s,
            rect.w as f32 * s,
            rect.h as f32 * s,
        );
        let radius = (radius * s).min(w.min(h) * 0.5);
        // Distance signée au bord de la carte décalée de `dy`, en pixels physiques.
        let distance = |px: u32, py: u32, dy: f32| {
            let qx = (px as f32 + 0.5 - (x + w * 0.5)).abs() - (w * 0.5 - radius);
            let qy = (py as f32 + 0.5 - (y + dy + h * 0.5)).abs() - (h * 0.5 - radius);
            qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
        };
        let reach = (SPREAD + 1.0) * s;
        let left = (x - reach).floor().max(0.0) as u32;
        let top = (y - reach).floor().max(0.0) as u32;
        let right = (x + w + reach).ceil().clamp(0.0, self.width as f32) as u32;
        let bottom = (y + h + 4.0 * s + reach).ceil().clamp(0.0, self.height as f32) as u32;
        // Dedans sûr : plus loin du bord que l'arrondi des coins (r × (1 − 1/√2)).
        let inset = radius * 0.3 + 1.0;
        let hole_x = ((x + inset).ceil().max(0.0) as u32).clamp(left, right)
            ..((x + w - inset).floor().max(0.0) as u32).clamp(left, right);
        let hole_y = (y + inset).ceil().max(0.0) as u32..(y + h - inset).floor().max(0.0) as u32;
        for py in top..bottom {
            let (skip_from, skip_to) = match hole_y.contains(&py) && !hole_x.is_empty() {
                true => (hole_x.start, hole_x.end),
                false => (right, right),
            };
            for px in (left..skip_from).chain(skip_to..right) {
                let shadow = distance(px, py, 4.0 * s);
                if shadow < reach {
                    let clear: f32 = (1..=SPREAD as u32)
                        .map(|spread| 1.0 - 5.0 / 255.0 * (0.5 - shadow + spread as f32 * s).clamp(0.0, 1.0))
                        .product();
                    self.blend(px, py, Color::BLACK, 1.0 - clear);
                }
                let contour = distance(px, py, 0.0) - s;
                self.blend(
                    px,
                    py,
                    Color::BLACK.with_alpha(36),
                    (0.5 - contour).clamp(0.0, 1.0),
                );
            }
        }
    }

    /// Copie des lignes de pixels entre les ordonnées logiques `top` et `bottom`, à
    /// remettre avec [`Canvas::restore_rows`].
    fn save_rows(&self, top: f64, bottom: f64) -> (usize, Vec<u8>) {
        let s = self.scale as f64;
        let row = self.width as usize * 4;
        let first = ((top * s).floor().max(0.0) as usize).min(self.height as usize);
        let last = ((bottom * s).ceil().max(0.0) as usize)
            .min(self.height as usize)
            .max(first);
        (first * row, self.pixels[first * row..last * row].to_vec())
    }

    fn restore_rows(&mut self, (offset, saved): (usize, Vec<u8>)) {
        self.pixels[offset..offset + saved.len()].copy_from_slice(&saved);
    }

    /// Pose le verre `frost` : en entier par-dessus ce qui est dessous (`rows` vide), ou
    /// seulement entre les ordonnées logiques `rows`, en remplaçant ce qui y était dessiné
    /// (sauf sur le bord arrondi, mélangé).
    fn blit_frost(&mut self, frost: &Frost, rows: Option<(f64, f64)>) {
        let s = self.scale as f64;
        let (from, to) = match rows {
            Some((top, bottom)) => (
                ((top * s).floor().max(frost.top as f64) as u32).min(frost.top + frost.height),
                ((bottom * s).ceil().max(0.0) as u32).min(frost.top + frost.height),
            ),
            None => (frost.top, frost.top + frost.height),
        };
        for y in from..to {
            for x in 0..frost.width {
                let source = ((y - frost.top) * frost.width + x) as usize * 4;
                let pixel = &frost.pixels[source..source + 4];
                let alpha = pixel[3] as u32;
                if alpha == 0 {
                    continue;
                }
                let target = ((y * self.width + frost.left + x) * 4) as usize;
                let below = &mut self.pixels[target..target + 4];
                if alpha == 255 {
                    below.copy_from_slice(pixel);
                } else {
                    for channel in 0..4 {
                        below[channel] = (pixel[channel] as u32 + below[channel] as u32 * (255 - alpha) / 255)
                            .min(255) as u8;
                    }
                }
            }
        }
    }

    fn icon(&mut self, name: &'static str, center: (f64, f64), size: f64, style: Style) {
        let s = self.scale;
        let side = size as f32 * s;
        self.icons.draw(
            &mut self.pixels,
            self.width,
            self.height,
            name,
            center.0 as f32 * s - side / 2.0,
            center.1 as f32 * s - side / 2.0,
            side,
            style,
        );
    }

    /// Interrupteur ; `None` : grisé, indisponible.
    /// Icône d'une application installée ; faux si le fichier est illisible.
    fn icon_file(&mut self, path: &std::path::Path, center: (f64, f64), size: f64) -> bool {
        let s = self.scale;
        let side = size as f32 * s;
        self.icons.draw_file(
            &mut self.pixels,
            self.width,
            self.height,
            path,
            center.0 as f32 * s - side / 2.0,
            center.1 as f32 * s - side / 2.0,
            side,
        )
    }

    /// Vignette d'image, réduite pour tenir dans `rect` (alignée à gauche, centrée en hauteur).
    fn image(&mut self, rect: Rect, thumbnail: &Thumbnail) {
        let s = self.scale as f64;
        let ratio = (rect.w * s / thumbnail.width as f64).min(rect.h * s / thumbnail.height as f64);
        let (w, h) = (
            (thumbnail.width as f64 * ratio).round().max(1.0) as u32,
            (thumbnail.height as f64 * ratio).round().max(1.0) as u32,
        );
        let x0 = (rect.x * s).round() as i64;
        let y0 = (rect.y * s + (rect.h * s - h as f64) / 2.0).round() as i64;
        for y in 0..h {
            for x in 0..w {
                let (px, py) = (x0 + x as i64, y0 + y as i64);
                if px < 0 || py < 0 || px >= self.width as i64 || py >= self.height as i64 {
                    continue;
                }
                let sx = ((x as f64 + 0.5) / ratio) as u32;
                let sy = ((y as f64 + 0.5) / ratio) as u32;
                let i = ((sy.min(thumbnail.height - 1) * thumbnail.width + sx.min(thumbnail.width - 1)) * 4)
                    as usize;
                let source = &thumbnail.pixels[i..i + 4];
                let offset = ((py as u32 * self.width + px as u32) * 4) as usize;
                let inverse = 255 - source[3] as u32;
                let pixel = &mut self.pixels[offset..offset + 4];
                for channel in 0..4 {
                    pixel[channel] =
                        (source[channel] as u32 + pixel[channel] as u32 * inverse / 255).min(255) as u8;
                }
            }
        }
    }

    /// Module des réglages rapides : verre un peu plus clair que le panneau.
    fn module(&mut self, rect: Rect) {
        self.fill(rect, 17.0, controls_module());
    }

    /// Panneau de verre comme le Dock : intérieur blanc translucide, liseré blanc, contour
    /// sombre fin et ombre douce tout autour, sans assombrir l'intérieur.
    fn glass(&mut self, rect: Rect, radius: f32) {
        const SHADOW: f32 = 12.0;
        let s = self.scale;
        let (x, y, w, h) = (
            rect.x as f32 * s,
            rect.y as f32 * s,
            rect.w as f32 * s,
            rect.h as f32 * s,
        );
        let radius = (radius * s).min(w.min(h) * 0.5);
        let reach = (SHADOW + 2.0) * s;
        let left = (x - reach).floor().max(0.0) as u32;
        let top = (y - reach).floor().max(0.0) as u32;
        let right = (x + w + reach).ceil().clamp(0.0, self.width as f32) as u32;
        let bottom = (y + h + reach).ceil().clamp(0.0, self.height as f32) as u32;
        for py in top..bottom {
            for px in left..right {
                let qx = (px as f32 + 0.5 - (x + w * 0.5)).abs() - (w * 0.5 - radius);
                let qy = (py as f32 + 0.5 - (y + h * 0.5)).abs() - (h * 0.5 - radius);
                let d = (qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius) / s;
                let inside = (0.5 - d * s).clamp(0.0, 1.0);
                if inside < 1.0 {
                    let fade = (1.0 - d.max(0.0) / SHADOW).clamp(0.0, 1.0);
                    let shadow = Color::BLACK.with_alpha((fade * fade * 22.0) as u8);
                    self.blend(px, py, shadow, 1.0 - inside);
                    let edge = (1.0 - (d - 0.5).abs()).clamp(0.0, 1.0);
                    self.blend(px, py, glass_edge(), edge);
                }
                if inside > 0.0 {
                    self.blend(px, py, glass(), inside);
                    let rim = (1.5 + d).clamp(0.0, 1.0) * inside;
                    self.blend(px, py, glass_rim(), rim);
                }
            }
        }
    }

    /// Pastille ronde d'un interrupteur : bleue s'il est actif, grise sinon, estompée
    /// s'il est indisponible (`None`).
    fn toggle_circle(&mut self, center: (f64, f64), glyph: &'static str, state: Option<bool>) {
        let (disc, ink) = match state {
            Some(true) => (controls_on(), Color::WHITE),
            Some(false) => (Color::BLACK.with_alpha(22), text_strong()),
            None => (Color::BLACK.with_alpha(12), text_secondary()),
        };
        let radius = 16.0;
        self.fill(
            Rect::new(center.0 - radius, center.1 - radius, 2.0 * radius, 2.0 * radius),
            radius as f32,
            disc,
        );
        self.icon(glyph, center, 18.0, Style::Symbolic(ink));
    }

    /// Curseur épais : partie réglée blanche, bouton rond à son extrémité et pictogramme
    /// au début de la piste.
    fn slider(&mut self, track: Rect, value: u8, glyph: &'static str) {
        let radius = (track.h / 2.0) as f32;
        self.fill(
            Rect::new(track.x - 1.0, track.y - 1.0, track.w + 2.0, track.h + 2.0),
            radius + 1.0,
            Color::BLACK.with_alpha(26),
        );
        self.fill(track, radius, controls_track());
        let knob = track.h;
        let filled = knob + (track.w - knob) * value.min(100) as f64 / 100.0;
        self.fill(Rect::new(track.x, track.y, filled, track.h), radius, Color::WHITE);
        self.icon(
            glyph,
            (track.x + knob / 2.0 + 2.0, track.y + track.h / 2.0),
            14.0,
            Style::Symbolic(text_secondary()),
        );
        let x = track.x + filled - knob;
        for (spread, alpha) in [(2.0, 10), (1.0, 34)] {
            self.fill(
                Rect::new(
                    x - spread,
                    track.y - spread + 0.5,
                    knob + 2.0 * spread,
                    knob + 2.0 * spread,
                ),
                radius + spread as f32,
                Color::BLACK.with_alpha(alpha),
            );
        }
        self.fill(Rect::new(x, track.y, knob, knob), radius, Color::WHITE);
    }

    fn button(&mut self, rect: Rect, text: &str, background: Color, ink: Color) {
        self.fill(rect, 8.0, background);
        let font = self.bold;
        let width = self.text_width(font, text, 11.5);
        let y = rect.y + (rect.h - 11.5 * 1.22) / 2.0 - 1.0;
        self.label(font, text, rect.x + (rect.w - width) / 2.0, y, 11.5, ink);
    }

    /// Texte de la barre, de la couleur `ink`.
    fn bar_label(&mut self, font: &Font, text: &str, x: f64, size: f32, ink: Color, bar: f64) {
        let y = (bar - size as f64 * 1.22) / 2.0 - 1.0;
        self.label(font, text, x, y, size, ink);
    }

    /// Logo Ferrite du Ferrite d'origine : un losange plein, traversé d'un trait clair.
    fn logo(&mut self, center: (f64, f64), color: Color) {
        let (cx, cy) = center;
        for dy in -7i32..=7 {
            let half = 6.0 - f64::from(dy.abs()) * 6.0 / 7.0;
            self.fill(
                Rect::new(cx - half - 0.5, cy + f64::from(dy) - 0.5, 2.0 * half + 1.0, 1.0),
                0.0,
                color,
            );
        }
        self.fill(
            Rect::new(cx - 6.5, cy - 0.5, 13.0, 1.0),
            0.0,
            color.mix(Color::WHITE, 90.0 / 255.0),
        );
    }

    fn text_width(&self, font: &Font, text: &str, size: f32) -> f64 {
        text.chars()
            .map(|character| font.metrics(character, size * 1.22).advance_width as f64)
            .sum()
    }

    /// Écrit `text` dont la boîte commence en (`x`, `y`) ; `size` est la taille de base.
    fn label(&mut self, font: &Font, text: &str, x: f64, y: f64, size: f32, color: Color) {
        let s = self.scale;
        let mut cursor = x as f32 * s;
        let font_size = size * s * 1.22;
        let baseline = ((y as f32 + size * 1.22) * s).round() as i32;
        for character in text.chars() {
            let (metrics, coverage) = font.rasterize(character, font_size);
            for cy in 0..metrics.height {
                for cx in 0..metrics.width {
                    let px = cursor.round() as i32 + metrics.xmin + cx as i32;
                    let py = baseline - metrics.ymin - metrics.height as i32 + cy as i32;
                    if px >= 0 && py >= 0 && px < self.width as i32 && py < self.height as i32 {
                        let value = coverage[cy * metrics.width + cx] as f32 / 255.0;
                        self.blend(px as u32, py as u32, color, value);
                    }
                }
            }
            cursor += metrics.advance_width;
        }
    }
}

fn wrap_dashboard_text(font: &Font, text: &str, size: f32, max_width: f64) -> Vec<String> {
    let width = |text: &str| {
        text.chars()
            .map(|character| font.metrics(character, size * 1.22).advance_width as f64)
            .sum::<f64>()
    };
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if width(&candidate) <= max_width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            for character in word.chars() {
                let mut candidate = line.clone();
                candidate.push(character);
                if !line.is_empty() && width(&candidate) > max_width {
                    lines.push(std::mem::take(&mut line));
                }
                line.push(character);
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

/// Date et heure locales, à la française : « lun. 26 oct.  18:05 ».
fn clock_label(now: SystemTime, offset: i64) -> String {
    const DAYS: [&str; 7] = ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."];
    const MONTHS: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc.",
    ];
    let seconds = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64 + offset;
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (_, month, day) = civil_from_days(days);
    let weekday = (days + 4).rem_euclid(7) as usize;
    format!(
        "{} {} {}   {:02}:{:02}",
        DAYS[weekday],
        day,
        MONTHS[month as usize - 1],
        time / 3600,
        (time / 60) % 60
    )
}

fn clock_snapshot(now: SystemTime) -> ClockSnapshot {
    let seconds = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64 + local_offset();
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let time = seconds.rem_euclid(86_400);
    ClockSnapshot {
        year,
        month,
        day,
        weekday: (days + 4).rem_euclid(7) as usize,
        hour: (time / 3600) as u32,
        minute: ((time / 60) % 60) as u32,
    }
}

fn shift_month(year: i64, month: u32, offset: i32) -> (i64, u32) {
    let index = year * 12 + i64::from(month) - 1 + i64::from(offset);
    (index.div_euclid(12), index.rem_euclid(12) as u32 + 1)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn weekday_for(year: i64, month: u32, day: u32) -> &'static str {
    ["Dim.", "Lun.", "Mar.", "Mer.", "Jeu.", "Ven.", "Sam."]
        [(days_from_civil(year, month, day) + 4).rem_euclid(7) as usize]
}

fn note_timestamp(updated: u64, now: ClockSnapshot) -> String {
    if updated == 0 {
        return "—".to_owned();
    }
    let local_seconds = updated as i64 + local_offset();
    let note_days = local_seconds.div_euclid(86_400);
    let now_days = days_from_civil(now.year, now.month, now.day);
    let (year, month, day) = civil_from_days(note_days);
    match now_days - note_days {
        0 => format!(
            "{:02}:{:02}",
            local_seconds.rem_euclid(86_400) / 3600,
            (local_seconds / 60).rem_euclid(60)
        ),
        1 => "Hier".to_owned(),
        2..=6 => weekday_for(year, month, day).to_owned(),
        _ => {
            let months = [
                "jan.", "fév.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
                "déc.",
            ];
            format!("{day} {}", months[month.saturating_sub(1) as usize])
        }
    }
}

fn network_rate(kib_per_second: u64) -> (String, &'static str) {
    if kib_per_second >= 1024 {
        (format!("{:.1}", kib_per_second as f64 / 1024.0), "MB")
    } else {
        (kib_per_second.to_string(), "KB")
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

fn cpu_counters() -> Option<(u64, u64)> {
    let line = fs::read_to_string("/proc/stat").ok()?.lines().next()?.to_owned();
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let values: Vec<u64> = fields.map(str::parse).collect::<Result<_, _>>().ok()?;
    let total = values.iter().sum();
    let idle = values.get(3)?.saturating_add(*values.get(4).unwrap_or(&0));
    Some((total, idle))
}

fn network_counters() -> Option<(u64, u64)> {
    let text = fs::read_to_string("/proc/net/dev").ok()?;
    text.lines()
        .skip(2)
        .filter_map(|line| {
            let (interface, counters) = line.split_once(':')?;
            if interface.trim() == "lo" {
                return None;
            }
            let counters: Vec<u64> = counters
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            Some((*counters.first()?, *counters.get(8)?))
        })
        .fold(None, |totals, (received, sent)| {
            Some(totals.map_or((received, sent), |(rx, tx)| (rx + received, tx + sent)))
        })
}

fn memory_usage() -> Option<u8> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    let value = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
    };
    let total = value("MemTotal:")?;
    let available = value("MemAvailable:").or_else(|| value("MemFree:"))?;
    Some((total.saturating_sub(available).saturating_mul(100) / total.max(1)).min(100) as u8)
}

#[allow(unsafe_code)]
fn disk_usage() -> Option<u8> {
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `stats` points to writable storage for a `statvfs` result and `c"/"`
    // is a valid, NUL-terminated path.
    if unsafe { libc::statvfs(c"/".as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: `statvfs` returned success and initialized the whole output structure.
    let stats = unsafe { stats.assume_init() };
    let total = (stats.f_blocks as u64).checked_mul(stats.f_frsize as u64)?;
    let available = (stats.f_bavail as u64).checked_mul(stats.f_frsize as u64)?;
    if total == 0 {
        return None;
    }
    Some((total.saturating_sub(available).saturating_mul(100) / total).min(100) as u8)
}

/// Date (année, mois, jour) du jour numéro `days` depuis le 1er janvier 1970.
/// Algorithme de H. Hinnant (calendrier grégorien proleptique).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Décalage du fuseau local en secondes, lu une fois au démarrage avec `date +%z`
/// (un passage à l'heure d'été n'est pris en compte qu'au prochain démarrage).
fn local_offset() -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|output| parse_offset(String::from_utf8_lossy(&output.stdout).trim()))
            .unwrap_or(0)
    })
}

/// « +0130 » → 5400 secondes.
fn parse_offset(text: &str) -> Option<i64> {
    let (sign, digits) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = digits[..2].parse().ok()?;
    let minutes: i64 = digits[2..].parse().ok()?;
    Some(sign * (hours * 3600 + minutes * 60))
}

/// Dossier sysfs de la première batterie, s'il y en a une.
fn battery_dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let mut batteries: Vec<_> = fs::read_dir("/sys/class/power_supply")
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("BAT"))
            })
            .collect();
        batteries.sort();
        batteries.into_iter().next()
    })
    .as_ref()
}

fn read_battery() -> Option<Battery> {
    let dir = battery_dir()?;
    let percent = fs::read_to_string(dir.join("capacity"))
        .ok()?
        .trim()
        .parse::<u8>()
        .ok()?;
    let status = fs::read_to_string(dir.join("status")).unwrap_or_default();
    Some(Battery {
        percent: percent.min(100),
        charging: matches!(status.trim(), "Charging" | "Full"),
    })
}

/// Coupe `text` avec « … » pour qu'il tienne dans `width` points.
fn ellipsize_to(canvas: &Canvas, font: &Font, text: &str, size: f32, width: f64) -> String {
    if canvas.text_width(font, text, size) <= width {
        return text.to_owned();
    }
    let mut short: String = text.to_owned();
    while !short.is_empty() && canvas.text_width(font, &format!("{short}…"), size) > width {
        short.pop();
    }
    format!("{}…", short.trim_end())
}

/// Coupe un texte trop long avec « … ».
fn ellipsize(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut short: String = text.chars().take(max.saturating_sub(1)).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accentuation_personnalisee_remplace_l_accent_de_la_palette_du_shell() {
        assert_eq!(
            accent_for(Palette::DARK, Accent::Purple.rgb()),
            Color::rgb(0x953D96)
        );
        assert_eq!(accent_for(Palette::DARK, None), Palette::DARK.accent);
    }

    #[test]
    fn les_notes_du_tableau_de_bord_reviennent_a_la_ligne() {
        let font = Font::from_bytes(FONT_REGULAR, FontSettings::default()).expect("police régulière");
        let lines = wrap_dashboard_text(
            &font,
            "Mes notes\n\nClique ici pour écrire… Soiuyuyfchdxthjgvklbjnk;ojjgufvtszeusduyk",
            16.0,
            180.0,
        );
        assert!(lines.len() > 3);
        assert_eq!(lines[0], "Mes notes");
        assert_eq!(lines[1], "");
        assert!(lines.iter().all(|line| {
            line.chars()
                .map(|character| font.metrics(character, 16.0 * 1.22).advance_width as f64)
                .sum::<f64>()
                <= 180.0
        }));
    }

    #[test]
    fn les_mois_du_calendrier_gere_le_changement_d_annee_et_bissextiles() {
        assert_eq!(shift_month(2026, 1, -1), (2025, 12));
        assert_eq!(shift_month(2026, 12, 1), (2027, 1));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(civil_from_days(days_from_civil(2026, 10, 30)), (2026, 10, 30));
    }

    #[test]
    fn le_tableau_de_bord_repond_aux_clics_de_ses_modules() {
        let output = output();
        let mut desktop = view(None);
        desktop.bar_height = 34;
        desktop.dashboard.tasks = vec![DashboardTaskView {
            text: "Rapport".to_owned(),
            done: false,
            priority: crate::desktop::widgets::Priority::High,
        }];
        desktop.dashboard.notes = vec![DashboardNoteView {
            title: "Projet web".to_owned(),
            preview: "Finir l'interface".to_owned(),
            updated: 0,
        }];
        let (layout, scale) = dashboard_layout(output.size.w as f64, output.size.h as f64, &desktop);
        let [_, _, calendar, notes, tasks, _, shortcuts, _] = layout.dashboard_cards();
        assert_eq!(
            action_at(
                output,
                point((notes.x + 30.0) * scale, (notes.y + 70.0) * scale),
                &desktop
            ),
            Some(DesktopAction::DashboardEditNote(0))
        );
        assert_eq!(
            action_at(
                output,
                point((notes.right() - 28.0) * scale, (notes.y + 25.0) * scale),
                &desktop
            ),
            Some(DesktopAction::DashboardAddNote)
        );
        assert_eq!(
            action_at(
                output,
                point((calendar.right() - 55.0) * scale, (calendar.y + 24.0) * scale),
                &desktop
            ),
            Some(DesktopAction::DashboardMonth(-1))
        );
        assert_eq!(
            action_at(
                output,
                point((tasks.x + 25.0) * scale, (tasks.y + 63.0) * scale),
                &desktop
            ),
            Some(DesktopAction::DashboardToggleTask(0))
        );
        assert_eq!(
            action_at(
                output,
                point((tasks.right() - 30.0) * scale, (tasks.y + 63.0) * scale),
                &desktop
            ),
            Some(DesktopAction::DashboardCyclePriority(0))
        );
        assert_eq!(
            action_at(
                output,
                point((shortcuts.x + 45.0) * scale, (shortcuts.y + 70.0) * scale),
                &desktop
            ),
            Some(DesktopAction::DashboardShortcut(0))
        );
    }

    fn point(x: f64, y: f64) -> Point<f64, Logical> {
        Point::from((x, y))
    }

    fn output() -> Rectangle<i32, Logical> {
        Rectangle::new((0, 0).into(), (1280, 720).into())
    }

    /// Ordinateur complet : Wi-Fi, Bluetooth, son et rétroéclairage présents.
    fn view(popover: Option<Popover>) -> DesktopView {
        DesktopView {
            popover,
            wifi: Some(true),
            bluetooth: Some(false),
            volume: Some(60),
            brightness: Some(70),
            bar_visible: true,
            bar_height: 34,
            ..DesktopView::default()
        }
    }

    fn layout() -> Layout {
        Layout::new(1280.0, 720.0, &view(None))
    }

    fn click(pos: Point<f64, Logical>, popover: Option<Popover>) -> Option<DesktopAction> {
        action_at(output(), pos, &view(popover))
    }

    #[test]
    fn les_widgets_ne_captent_les_clics_que_sur_leur_page() {
        let mut desktop = view(None);
        let (layout, scale) = dashboard_layout(1280.0, 720.0, &desktop);
        let notes = layout.dashboard_cards()[3];
        let inside = point((notes.x + 30.0) * scale, (notes.y + 70.0) * scale);
        assert_eq!(
            action_at(output(), inside, &desktop),
            Some(DesktopAction::DashboardConsume)
        );
        desktop.page = 1;
        assert_eq!(action_at(output(), inside, &desktop), None);
    }

    #[test]
    fn les_marques_de_la_barre_menent_a_leur_page() {
        let mut desktop = view(None);
        desktop.page = 1;
        desktop.page_count = 3;
        let layout = Layout::new(1280.0, 720.0, &desktop);
        for page in 0..3 {
            let (x, y) = layout.page_dot(page).center();
            assert_eq!(
                action_at(output(), point(x, y), &desktop),
                Some(DesktopAction::GoToPage(page))
            );
            assert!(!layout.applications_button().contains(point(x, y)));
        }
    }

    #[test]
    fn les_widgets_laissent_la_place_au_dock() {
        let mut desktop = view(None);
        desktop.dock_height = 76;
        let cards = Layout::new(1280.0, 720.0, &desktop).dashboard_cards();
        for card in cards {
            assert!(card.bottom() <= 720.0 - 76.0);
        }
    }

    #[test]
    fn les_cartes_reproduisent_la_grille_de_la_maquette() {
        let cards = Layout::new(1280.0, 720.0, &view(None)).dashboard_cards();
        for row in [&cards[0..3], &cards[3..6]] {
            assert!((row[0].w - row[1].w).abs() < 0.01);
            assert!((row[1].w - row[2].w).abs() < 0.01);
            assert_eq!(row[0].y, row[1].y);
            assert_eq!(row[1].y, row[2].y);
        }
        assert!(cards[0].x < cards[1].x && cards[1].x < cards[2].x);
        assert!(cards[3].y > cards[0].y);
        assert!(cards[6].y > cards[3].y);
        assert!(cards[6].w > cards[7].w);
    }

    #[test]
    fn la_barre_ouvre_chaque_menu() {
        assert_eq!(
            click(point(40.0, 15.0), None),
            Some(DesktopAction::TogglePopover(Popover::Launcher))
        );
        let network = layout().indicator(Indicator::Network).center();
        assert_eq!(
            click(point(network.0, network.1), None),
            Some(DesktopAction::TogglePopover(Popover::Controls))
        );
        assert_eq!(
            click(point(1265.0, 15.0), None),
            Some(DesktopAction::TogglePopover(Popover::Session))
        );
        // Le reste de la barre absorbe le clic, ou ferme le menu ouvert.
        assert_eq!(click(point(640.0, 15.0), None), Some(DesktopAction::Consume));
        assert_eq!(
            click(point(640.0, 15.0), Some(Popover::Launcher)),
            Some(DesktopAction::ClosePopover)
        );
    }

    #[test]
    fn sans_materiel_les_indicateurs_absents_disparaissent() {
        // Machine virtuelle : ni Wi-Fi, ni Bluetooth, ni son.
        let bare = DesktopView {
            bar_visible: true,
            bar_height: 34,
            ..DesktopView::default()
        };
        let layout = Layout::new(1280.0, 720.0, &bare);
        assert_eq!(layout.indicator(Indicator::Bluetooth).w, 0.0);
        assert_eq!(layout.indicator(Indicator::Volume).w, 0.0);
        // L'indicateur réseau reste : les réglages rapides restent accessibles.
        let network = layout.indicator(Indicator::Network).center();
        assert_eq!(
            action_at(output(), point(network.0, network.1), &bare),
            Some(DesktopAction::TogglePopover(Popover::Controls))
        );
        // Les réglages indisponibles ne réagissent pas.
        let open = DesktopView {
            popover: Some(Popover::Controls),
            ..bare
        };
        let bluetooth = layout.control_row(Control::Bluetooth).center();
        assert_eq!(
            action_at(output(), point(bluetooth.0, bluetooth.1), &open),
            Some(DesktopAction::Consume)
        );
    }

    fn launcher(sections: Vec<(usize, Option<SectionLink>)>, scroll: u32) -> DesktopView {
        let mut next = 0;
        let sections = sections
            .into_iter()
            .map(|(count, link)| LauncherSection {
                title: "Rubrique".to_owned(),
                tiles: (0..count)
                    .map(|_| {
                        next += 1;
                        Tile {
                            name: format!("App {next}"),
                            icon: TileIcon::Missing,
                            index: next - 1,
                        }
                    })
                    .collect(),
                link,
            })
            .collect();
        DesktopView {
            launcher: LauncherView {
                sections,
                scroll,
                ..LauncherView::default()
            },
            ..view(Some(Popover::Launcher))
        }
    }

    fn tile_rect(view: &DesktopView, index: usize) -> Rect {
        layout()
            .launcher_items(&view.launcher)
            .into_iter()
            .find_map(|item| match item {
                LauncherItem::Tile { section, slot, rect }
                    if view.launcher.sections[section].tiles[slot].index == index =>
                {
                    Some(rect)
                }
                _ => None,
            })
            .expect("case présente")
    }

    #[test]
    fn le_lanceur_est_centre_et_lance_chaque_application() {
        let card = layout().launcher();
        assert!((card.x + card.w / 2.0 - 640.0).abs() < 1.0);
        let open = launcher(
            vec![(5, None), (5, Some(SectionLink::More(Section::Utilities)))],
            0,
        );
        let at = |x: f64, y: f64| action_at(output(), point(x, y), &open);
        for index in [0, 4, 5, 9] {
            let (x, y) = tile_rect(&open, index).center();
            assert_eq!(at(x, y), Some(DesktopAction::LaunchTile(index)));
        }
        // Cinq applications par ligne : la sixième commence la rubrique suivante.
        assert!(tile_rect(&open, 5).y > tile_rect(&open, 4).bottom());
        assert_eq!(tile_rect(&open, 5).x, tile_rect(&open, 0).x);
        // « Plus » ouvre la rubrique.
        let link = layout()
            .launcher_items(&open.launcher)
            .into_iter()
            .find_map(|item| match item {
                LauncherItem::Link { rect, .. } => Some(rect),
                _ => None,
            })
            .expect("lien Plus");
        assert_eq!(
            at(link.center().0, link.center().1),
            Some(DesktopAction::LauncherLink(SectionLink::More(Section::Utilities)))
        );
        // Le champ de recherche absorbe le clic ; dehors, le lanceur se ferme.
        assert_eq!(at(card.center().0, card.y + 20.0), Some(DesktopAction::Consume));
        assert_eq!(at(20.0, 600.0), Some(DesktopAction::ClosePopover));
        assert_eq!(
            click(point(700.0, 400.0), None),
            Some(DesktopAction::DashboardConsume)
        );
    }

    #[test]
    fn le_lanceur_defile_sans_depasser_la_fin() {
        let long = launcher(vec![(40, Some(SectionLink::Less))], 0);
        let limit = launcher_scroll_limit(output(), &long);
        assert!(limit > 0);
        // Défilé jusqu'au bout : la dernière case est visible, juste au-dessus du bas.
        let end = launcher(vec![(40, Some(SectionLink::Less))], limit + 500);
        let content = layout().launcher_content();
        let last = tile_rect(&end, 39);
        assert!(last.bottom() <= content.bottom() + 0.5 && last.bottom() > content.bottom() - 20.0);
        // Une case passée sous le champ de recherche ne se clique plus.
        let first = tile_rect(&end, 0);
        assert!(first.bottom() < content.y);
        // Peu d'applications : rien à faire défiler.
        assert_eq!(launcher_scroll_limit(output(), &launcher(vec![(5, None)], 0)), 0);
    }

    #[test]
    fn les_reglages_rapides_pilotent_wifi_bluetooth_son_et_luminosite() {
        let layout = layout();
        let open = Some(Popover::Controls);
        let at = |control| {
            let (x, y) = layout.control_row(control).center();
            click(point(x, y), open)
        };
        assert_eq!(at(Control::Wifi), Some(DesktopAction::ToggleWifi));
        assert_eq!(at(Control::Bluetooth), Some(DesktopAction::ToggleBluetooth));
        assert_eq!(at(Control::Airplane), Some(DesktopAction::ToggleAirplane));
        assert_eq!(
            at(Control::Clipboard),
            Some(DesktopAction::TogglePopover(Popover::Clipboard))
        );
        assert_eq!(at(Control::Settings), Some(DesktopAction::Launch(App::Settings)));
        // Le bouton reste dans la piste : sa course s'arrête à un demi-bouton des bords.
        let volume = layout.slider_track(Control::Volume);
        let quarter = volume.x + volume.h / 2.0 + (volume.w - volume.h) / 4.0;
        assert_eq!(
            click(point(quarter, volume.y + 5.0), open),
            Some(DesktopAction::SetVolume(25))
        );
        let brightness = layout.slider_track(Control::Brightness);
        assert_eq!(
            click(point(brightness.right() - 2.0, brightness.y + 5.0), open),
            Some(DesktopAction::SetBrightness(100))
        );
        assert_eq!(
            click(point(brightness.x + 2.0, brightness.y + 5.0), open),
            Some(DesktopAction::SetBrightness(0))
        );
    }

    #[test]
    fn le_mode_avion_suit_les_radios_presentes() {
        let mut view = view(None);
        assert_eq!(airplane(&view), Some(false));
        view.wifi = Some(false);
        assert_eq!(airplane(&view), Some(true));
        view.bluetooth = None;
        assert_eq!(airplane(&view), Some(true));
        view.wifi = None;
        assert_eq!(airplane(&view), None);
    }

    #[test]
    fn sans_retroeclairage_le_module_moniteur_disparait() {
        let desktop = DesktopView {
            brightness: None,
            ..view(Some(Popover::Controls))
        };
        let layout = Layout::new(1280.0, 720.0, &desktop);
        assert_eq!(layout.control_row(Control::Brightness).w, 0.0);
        // Le module Son prend sa place, juste sous Wi-Fi et Bluetooth.
        let volume = layout.control_row(Control::Volume);
        assert_eq!(volume.y, layout.controls_radios().bottom() + CONTROLS_GAP);
        assert!(layout.controls().bottom() < self::layout().controls().bottom());
    }

    #[test]
    fn l_arret_demande_toujours_une_confirmation() {
        let layout = layout();
        let restart = layout.session_row(0).center();
        assert_eq!(
            click(point(restart.0, restart.1), Some(Popover::Session)),
            Some(DesktopAction::RequestPower(PowerAction::Restart))
        );
        let off = layout.session_row(1).center();
        assert_eq!(
            click(point(off.0, off.1), Some(Popover::Session)),
            Some(DesktopAction::RequestPower(PowerAction::PowerOff))
        );

        let pending = DesktopView {
            power: Some(PowerAction::PowerOff),
            ..view(None)
        };
        let at = |x: f64, y: f64| action_at(output(), point(x, y), &pending);
        let confirm = layout.dialog_confirm().center();
        assert_eq!(
            at(confirm.0, confirm.1),
            Some(DesktopAction::ConfirmPower(PowerAction::PowerOff))
        );
        let cancel = layout.dialog_cancel().center();
        assert_eq!(at(cancel.0, cancel.1), Some(DesktopAction::CancelPower));
        let inside = layout.dialog();
        assert_eq!(at(inside.x + 10.0, inside.y + 10.0), Some(DesktopAction::Consume));
        assert_eq!(at(5.0, 700.0), Some(DesktopAction::CancelPower));
    }

    #[test]
    fn horloge_en_francais_et_fuseau() {
        // Lundi 26 octobre 2026, 17:57 UTC.
        let time = UNIX_EPOCH + Duration::from_secs(20_752 * 86_400 + 17 * 3600 + 57 * 60);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_752), (2026, 10, 26));
        assert_eq!(clock_label(time, 0), "lun. 26 oct.   17:57");
        assert_eq!(clock_label(time, 3600), "lun. 26 oct.   18:57");
        assert_eq!(clock_label(time, 7 * 3600), "mar. 27 oct.   00:57");
        assert_eq!(parse_offset("+0100"), Some(3600));
        assert_eq!(parse_offset("-0230"), Some(-9000));
        assert_eq!(parse_offset("UTC"), None);
    }
}
