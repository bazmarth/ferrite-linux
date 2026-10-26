//! Interface du bureau, dans l'esprit d'elementary OS : une barre fine en haut (menu
//! Applications à gauche, date et heure au centre, indicateurs à droite) et des menus
//! déroulants clairs qui s'ouvrent sous elle.
//!
//! La géométrie de chaque élément est calculée une seule fois par [`Layout`], qui sert
//! à la fois au dessin et à la détection des clics.

use std::{
    cell::RefCell,
    fs,
    path::PathBuf,
    process::Command,
    sync::OnceLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ferrite_ui::theme::{Color, MENUBAR_HEIGHT};
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
    utils::{Logical, Point, Rectangle},
};

use crate::desktop::{
    apps::App,
    svg::{self, IconCache, Style},
};
use crate::state::PowerAction;

const FONT_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Medium.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

const BAR: f64 = MENUBAR_HEIGHT as f64;
/// Écart entre la barre (ou le bord de l'écran) et un menu déroulant.
const POPOVER_MARGIN: f64 = 6.0;
const INDICATOR_WIDTH: f64 = 30.0;
const LAUNCHER_COLUMNS: usize = 4;
const LAUNCHER_CELL: (f64, f64) = (104.0, 100.0);
const CONTROLS_WIDTH: f64 = 300.0;
const SESSION_WIDTH: f64 = 230.0;
const DIALOG: (f64, f64) = (420.0, 168.0);
/// Fréquence de relecture de l'état de la batterie.
const BATTERY_REFRESH: Duration = Duration::from_secs(20);

const BAR_TINT: Color = Color::BLACK.with_alpha(112);
const BAR_INK: Color = Color::WHITE;
const CARD: Color = Color::rgb(0xFAFAFA);
const TEXT: Color = Color::rgb(0x333333);
const TEXT_SECONDARY: Color = Color::rgb(0x7E8087);
const SEPARATOR: Color = Color::rgb(0xE3E3E3);
const ACCENT: Color = Color::rgb(0x3689E6);
const DESTRUCTIVE: Color = Color::rgb(0xC6262E);

/// Menu déroulant ouvert depuis la barre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popover {
    Launcher,
    Controls,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAction {
    TogglePopover(Popover),
    ClosePopover,
    Launch(App),
    ToggleWifi,
    ToggleBluetooth,
    SetWifi(bool),
    SetBluetooth(bool),
    SetVolume(u8),
    SetBrightness(u8),
    RequestPower(PowerAction),
    ConfirmPower(PowerAction),
    CancelPower,
    /// Clic absorbé par un panneau, sans effet.
    Consume,
}

/// État du bureau à afficher, fourni par le compositeur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopView {
    pub popover: Option<Popover>,
    pub wifi: bool,
    pub bluetooth: bool,
    pub volume: u8,
    pub brightness: u8,
    pub power: Option<PowerAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VisualState {
    width: u32,
    height: u32,
    scale_milli: u32,
    view: DesktopView,
    battery: Option<Battery>,
    clock: String,
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
    cached: RefCell<Option<(VisualState, MemoryRenderBuffer)>>,
}

impl Panels {
    pub fn new() -> Result<Self, String> {
        let regular = Font::from_bytes(FONT_REGULAR, FontSettings::default()).map_err(|e| e.to_string())?;
        let bold = Font::from_bytes(FONT_BOLD, FontSettings::default()).map_err(|e| e.to_string())?;
        Ok(Self {
            regular,
            bold,
            icons: IconCache::default(),
            battery: RefCell::new((None, None)),
            cached: RefCell::new(None),
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
        let state = VisualState {
            width: output_geo.size.w.max(1) as u32,
            height: output_geo.size.h.max(1) as u32,
            scale_milli: (scale * 1000.0).round() as u32,
            view: view.clone(),
            battery: self.battery(),
            clock: clock_label(SystemTime::now(), local_offset()),
        };
        let mut cache = self.cached.borrow_mut();
        if !cache.as_ref().is_some_and(|(key, _)| key == &state) {
            let buffer = self.rasterize(&state, scale);
            *cache = Some((state, buffer));
        }
        let Some((_, buffer)) = cache.as_ref() else { return };
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

    /// État de la batterie, relu au plus toutes les [`BATTERY_REFRESH`].
    fn battery(&self) -> Option<Battery> {
        let mut cached = self.battery.borrow_mut();
        if cached.0.is_none_or(|read| read.elapsed() >= BATTERY_REFRESH) {
            *cached = (Some(Instant::now()), read_battery());
        }
        cached.1
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
        let layout = Layout::new(state.width as f64, state.height as f64);
        let view = &state.view;

        // Barre du haut.
        canvas.fill(Rect::new(0.0, 0.0, layout.w, BAR), 0.0, BAR_TINT);
        let applications = layout.applications_button();
        canvas.bar_label(&self.bold, "Applications", applications.x + 10.0, 11.5);
        let clock_width = canvas.text_width(&self.bold, &state.clock, 11.5);
        canvas.bar_label(&self.bold, &state.clock, (layout.w - clock_width) / 2.0, 11.5);
        for (indicator, rect) in layout.indicators() {
            let name = match indicator {
                Indicator::Bluetooth if view.bluetooth => "status-bluetooth-on",
                Indicator::Bluetooth => "status-bluetooth-off",
                Indicator::Wifi => svg::wifi_icon(view.wifi),
                Indicator::Volume => svg::volume_icon(view.volume),
                Indicator::Battery => match state.battery {
                    Some(battery) => svg::battery_icon(battery.percent, battery.charging),
                    None => continue,
                },
                Indicator::Power => "status-power",
            };
            canvas.icon(name, rect.center(), 26.0, Style::Symbolic(BAR_INK));
        }
        if let Some(battery) = state.battery {
            let rect = layout.indicator(Indicator::Battery);
            let text = format!("{} %", battery.percent);
            canvas.bar_label(&self.regular, &text, rect.right() + 2.0, 10.5);
        }
        // Souligne l'élément de la barre dont le menu est ouvert.
        if let Some(popover) = view.popover {
            let rect = match popover {
                Popover::Launcher => applications,
                Popover::Controls => layout.indicators_group(),
                Popover::Session => layout.indicator(Indicator::Power),
            };
            canvas.fill(
                Rect::new(rect.x + 6.0, BAR - 3.0, rect.w - 12.0, 2.0),
                1.0,
                BAR_INK,
            );
        }

        match view.popover {
            Some(Popover::Launcher) => self.draw_launcher(&mut canvas, &layout),
            Some(Popover::Controls) => self.draw_controls(&mut canvas, &layout, view),
            Some(Popover::Session) => self.draw_session(&mut canvas, &layout),
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

    fn draw_launcher(&self, canvas: &mut Canvas, layout: &Layout) {
        canvas.card(layout.launcher(), 12.0);
        for (index, app) in App::ALL.into_iter().enumerate() {
            let cell = layout.launcher_cell(index);
            canvas.icon(
                app.icon(),
                (cell.x + cell.w / 2.0, cell.y + 36.0),
                60.0,
                Style::Color,
            );
            let name_width = canvas.text_width(&self.regular, app.name(), 11.0);
            canvas.label(
                &self.regular,
                app.name(),
                cell.x + (cell.w - name_width) / 2.0,
                cell.y + 72.0,
                11.0,
                TEXT,
            );
        }
    }

    fn draw_controls(&self, canvas: &mut Canvas, layout: &Layout, view: &DesktopView) {
        let card = layout.controls();
        canvas.card(card, 12.0);
        let toggles = [
            (
                layout.control_row(Control::Wifi),
                svg::wifi_icon(true),
                "Wi-Fi",
                if view.wifi { "Activé" } else { "Désactivé" },
                view.wifi,
            ),
            (
                layout.control_row(Control::Bluetooth),
                "status-bluetooth-on",
                "Bluetooth",
                if view.bluetooth { "Activé" } else { "Désactivé" },
                view.bluetooth,
            ),
        ];
        for (row, icon, title, status, active) in toggles {
            canvas.icon(
                icon,
                (row.x + 22.0, row.y + row.h / 2.0),
                30.0,
                Style::Symbolic(TEXT),
            );
            canvas.label(&self.bold, title, row.x + 46.0, row.y + 8.0, 11.5, TEXT);
            canvas.label(
                &self.regular,
                status,
                row.x + 46.0,
                row.y + 26.0,
                10.0,
                TEXT_SECONDARY,
            );
            canvas.switch(Rect::new(row.right() - 50.0, row.y + 13.0, 40.0, 22.0), active);
        }
        canvas.fill(
            Rect::new(
                card.x + 12.0,
                layout.control_row(Control::Volume).y - 6.0,
                card.w - 24.0,
                1.0,
            ),
            0.0,
            SEPARATOR,
        );
        let sliders = [
            (Control::Volume, svg::volume_icon(view.volume), view.volume),
            (
                Control::Brightness,
                svg::brightness_icon(view.brightness),
                view.brightness,
            ),
        ];
        for (control, icon, value) in sliders {
            let row = layout.control_row(control);
            canvas.icon(
                icon,
                (row.x + 22.0, row.y + row.h / 2.0),
                30.0,
                Style::Symbolic(TEXT),
            );
            canvas.slider(layout.slider_track(control), value);
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
                Style::Symbolic(TEXT),
            );
            canvas.label(&self.regular, text, row.x + 40.0, row.y + 10.0, 11.5, TEXT);
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
            Style::Symbolic(TEXT),
        );
        let (title, confirm, color) = match power {
            PowerAction::PowerOff => ("Éteindre l’ordinateur ?", "Éteindre", DESTRUCTIVE),
            PowerAction::Restart => ("Redémarrer l’ordinateur ?", "Redémarrer", ACCENT),
        };
        canvas.label(&self.bold, title, dialog.x + 96.0, dialog.y + 30.0, 15.0, TEXT);
        canvas.label(
            &self.regular,
            "Les applications ouvertes seront fermées.",
            dialog.x + 96.0,
            dialog.y + 58.0,
            11.0,
            TEXT_SECONDARY,
        );
        canvas.button(layout.dialog_cancel(), "Annuler", Color::rgb(0xE8E8E8), TEXT);
        canvas.button(layout.dialog_confirm(), confirm, color, Color::WHITE);
    }
}

/// Hit-test du bureau ; les panneaux interceptent leurs propres surfaces pour que le clic
/// ne tombe jamais sur une fenêtre située derrière eux.
pub fn action_at(
    output: Rectangle<i32, Logical>,
    pos: Point<f64, Logical>,
    popover: Option<Popover>,
    pending_power: Option<PowerAction>,
) -> Option<DesktopAction> {
    let layout = Layout::new(output.size.w as f64, output.size.h as f64);
    let pos = Point::from((pos.x - output.loc.x as f64, pos.y - output.loc.y as f64));

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

    if pos.y < BAR {
        if layout.applications_button().contains(pos) {
            return Some(DesktopAction::TogglePopover(Popover::Launcher));
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
        Popover::Launcher if layout.launcher().contains(pos) => (0..App::ALL.len())
            .find(|&index| layout.launcher_cell(index).contains(pos))
            .map_or(DesktopAction::Consume, |index| {
                DesktopAction::Launch(App::ALL[index])
            }),
        Popover::Controls if layout.controls().contains(pos) => {
            if layout.control_row(Control::Wifi).contains(pos) {
                DesktopAction::ToggleWifi
            } else if layout.control_row(Control::Bluetooth).contains(pos) {
                DesktopAction::ToggleBluetooth
            } else if layout.control_row(Control::Volume).contains(pos) {
                DesktopAction::SetVolume(layout.slider_track(Control::Volume).percent_at(pos.x))
            } else if layout.control_row(Control::Brightness).contains(pos) {
                DesktopAction::SetBrightness(layout.slider_track(Control::Brightness).percent_at(pos.x))
            } else {
                DesktopAction::Consume
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

/// Indicateurs de la barre, de gauche à droite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Indicator {
    Bluetooth,
    Wifi,
    Volume,
    Battery,
    Power,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Wifi,
    Bluetooth,
    Volume,
    Brightness,
}

/// Géométrie du bureau, en pixels logiques relatifs à l'écran.
struct Layout {
    w: f64,
    h: f64,
    battery: bool,
}

impl Layout {
    fn new(w: f64, h: f64) -> Self {
        Self {
            w,
            h,
            battery: battery_dir().is_some(),
        }
    }

    fn applications_button(&self) -> Rect {
        Rect::new(4.0, 0.0, 102.0, BAR)
    }

    fn indicator(&self, indicator: Indicator) -> Rect {
        let power = Rect::new(self.w - 6.0 - INDICATOR_WIDTH, 0.0, INDICATOR_WIDTH, BAR);
        // Le pourcentage de batterie est écrit à droite de son icône.
        let battery_text = if self.battery { 34.0 } else { 0.0 };
        let slot = |index: f64| {
            Rect::new(
                power.x - 8.0 - battery_text - (index + 1.0) * INDICATOR_WIDTH,
                0.0,
                INDICATOR_WIDTH,
                BAR,
            )
        };
        let battery = if self.battery { 1.0 } else { 0.0 };
        match indicator {
            Indicator::Power => power,
            Indicator::Battery => slot(0.0),
            Indicator::Volume => slot(battery),
            Indicator::Wifi => slot(battery + 1.0),
            Indicator::Bluetooth => slot(battery + 2.0),
        }
    }

    fn indicators(&self) -> Vec<(Indicator, Rect)> {
        let mut list = vec![Indicator::Bluetooth, Indicator::Wifi, Indicator::Volume];
        if self.battery {
            list.push(Indicator::Battery);
        }
        list.push(Indicator::Power);
        list.into_iter()
            .map(|indicator| (indicator, self.indicator(indicator)))
            .collect()
    }

    /// Indicateurs qui ouvrent les réglages rapides (tous sauf l'alimentation).
    fn indicators_group(&self) -> Rect {
        let first = self.indicator(Indicator::Bluetooth);
        let end = self.indicator(Indicator::Power).x - 4.0;
        Rect::new(first.x, 0.0, end - first.x, BAR)
    }

    fn launcher(&self) -> Rect {
        let rows = App::ALL.len().div_ceil(LAUNCHER_COLUMNS) as f64;
        Rect::new(
            POPOVER_MARGIN,
            BAR + POPOVER_MARGIN,
            LAUNCHER_COLUMNS as f64 * LAUNCHER_CELL.0 + 16.0,
            rows * LAUNCHER_CELL.1 + 16.0,
        )
    }

    fn launcher_cell(&self, index: usize) -> Rect {
        let card = self.launcher();
        let (column, row) = (
            (index % LAUNCHER_COLUMNS) as f64,
            (index / LAUNCHER_COLUMNS) as f64,
        );
        Rect::new(
            card.x + 8.0 + column * LAUNCHER_CELL.0,
            card.y + 8.0 + row * LAUNCHER_CELL.1,
            LAUNCHER_CELL.0,
            LAUNCHER_CELL.1,
        )
    }

    fn controls(&self) -> Rect {
        let x = (self.w - POPOVER_MARGIN - CONTROLS_WIDTH).max(POPOVER_MARGIN);
        Rect::new(x, BAR + POPOVER_MARGIN, CONTROLS_WIDTH, 204.0)
    }

    fn control_row(&self, control: Control) -> Rect {
        let card = self.controls();
        let (y, h) = match control {
            Control::Wifi => (8.0, 48.0),
            Control::Bluetooth => (56.0, 48.0),
            Control::Volume => (116.0, 40.0),
            Control::Brightness => (156.0, 40.0),
        };
        Rect::new(card.x + 6.0, card.y + y, card.w - 12.0, h)
    }

    fn slider_track(&self, control: Control) -> Rect {
        let row = self.control_row(control);
        Rect::new(row.x + 48.0, row.y + row.h / 2.0 - 2.0, row.w - 64.0, 4.0)
    }

    fn session(&self) -> Rect {
        let x = (self.w - POPOVER_MARGIN - SESSION_WIDTH).max(POPOVER_MARGIN);
        Rect::new(x, BAR + POPOVER_MARGIN, SESSION_WIDTH, 84.0)
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
            ((self.h - DIALOG.1) * 0.4).max(BAR + 8.0),
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

    fn percent_at(self, x: f64) -> u8 {
        (((x - self.x) / self.w).clamp(0.0, 1.0) * 100.0).round() as u8
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
        for spread in (1..=10).rev() {
            let spread = spread as f64;
            self.fill(
                Rect::new(
                    rect.x - spread,
                    rect.y - spread + 4.0,
                    rect.w + 2.0 * spread,
                    rect.h + 2.0 * spread,
                ),
                radius + spread as f32,
                Color::BLACK.with_alpha(5),
            );
        }
        self.fill(
            Rect::new(rect.x - 1.0, rect.y - 1.0, rect.w + 2.0, rect.h + 2.0),
            radius + 1.0,
            Color::BLACK.with_alpha(36),
        );
        self.fill(rect, radius, CARD);
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

    fn switch(&mut self, rect: Rect, active: bool) {
        let track = if active { ACCENT } else { Color::rgb(0xCFCFCF) };
        self.fill(rect, rect.h as f32 / 2.0, track);
        let knob = rect.h - 4.0;
        let x = if active {
            rect.right() - 2.0 - knob
        } else {
            rect.x + 2.0
        };
        self.fill(
            Rect::new(x, rect.y + 2.0, knob, knob),
            knob as f32 / 2.0,
            Color::WHITE,
        );
    }

    fn slider(&mut self, track: Rect, value: u8) {
        self.fill(track, 2.0, Color::rgb(0xD6D6D6));
        let filled = track.w * value.min(100) as f64 / 100.0;
        self.fill(Rect::new(track.x, track.y, filled, track.h), 2.0, ACCENT);
        let knob = 16.0;
        let center = (track.x + filled, track.y + track.h / 2.0);
        self.fill(
            Rect::new(
                center.0 - knob / 2.0 - 1.0,
                center.1 - knob / 2.0 - 1.0,
                knob + 2.0,
                knob + 2.0,
            ),
            (knob / 2.0 + 1.0) as f32,
            Color::BLACK.with_alpha(45),
        );
        self.fill(
            Rect::new(center.0 - knob / 2.0, center.1 - knob / 2.0, knob, knob),
            (knob / 2.0) as f32,
            Color::WHITE,
        );
    }

    fn button(&mut self, rect: Rect, text: &str, background: Color, ink: Color) {
        self.fill(rect, 8.0, background);
        let font = self.bold;
        let width = self.text_width(font, text, 11.5);
        let y = rect.y + (rect.h - 11.5 * 1.22) / 2.0 - 1.0;
        self.label(font, text, rect.x + (rect.w - width) / 2.0, y, 11.5, ink);
    }

    /// Texte de la barre : blanc, avec une ombre légère pour rester lisible sur tout fond.
    fn bar_label(&mut self, font: &Font, text: &str, x: f64, size: f32) {
        let y = (BAR - size as f64 * 1.22) / 2.0 - 1.0;
        self.label(font, text, x, y + 1.0, size, Color::BLACK.with_alpha(90));
        self.label(font, text, x, y, size, BAR_INK);
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

pub fn run_system_action(action: DesktopAction) -> bool {
    let result = match action {
        DesktopAction::SetWifi(enabled) => Command::new("nmcli")
            .args(["radio", "wifi", if enabled { "on" } else { "off" }])
            .status(),
        DesktopAction::SetBluetooth(enabled) => Command::new("bluetoothctl")
            .args(["power", if enabled { "on" } else { "off" }])
            .status(),
        DesktopAction::SetVolume(value) => Command::new("wpctl")
            .args([
                "set-volume",
                "@DEFAULT_AUDIO_SINK@",
                &format!("{:.2}", value as f32 / 100.0),
            ])
            .status(),
        DesktopAction::SetBrightness(value) => Command::new("brightnessctl")
            .args(["set", &format!("{value}%")])
            .status(),
        DesktopAction::ConfirmPower(power) => Command::new("systemctl")
            .arg(match power {
                PowerAction::PowerOff => "poweroff",
                PowerAction::Restart => "reboot",
            })
            .status(),
        _ => return false,
    };
    match result {
        Ok(status) if status.success() => true,
        Ok(status) => {
            tracing::warn!(?action, ?status, "la commande du bureau a échoué");
            false
        }
        Err(err) => {
            tracing::warn!(?action, %err, "commande du bureau indisponible");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f64, y: f64) -> Point<f64, Logical> {
        Point::from((x, y))
    }

    fn output() -> Rectangle<i32, Logical> {
        Rectangle::new((0, 0).into(), (1280, 720).into())
    }

    fn layout() -> Layout {
        Layout::new(1280.0, 720.0)
    }

    fn click(pos: Point<f64, Logical>, popover: Option<Popover>) -> Option<DesktopAction> {
        action_at(output(), pos, popover, None)
    }

    #[test]
    fn la_barre_ouvre_chaque_menu() {
        assert_eq!(
            click(point(40.0, 15.0), None),
            Some(DesktopAction::TogglePopover(Popover::Launcher))
        );
        let wifi = layout().indicator(Indicator::Wifi).center();
        assert_eq!(
            click(point(wifi.0, wifi.1), None),
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
    fn le_menu_applications_lance_chaque_application() {
        for (index, app) in App::ALL.into_iter().enumerate() {
            let (x, y) = layout().launcher_cell(index).center();
            assert_eq!(
                click(point(x, y), Some(Popover::Launcher)),
                Some(DesktopAction::Launch(app))
            );
        }
        // Un clic hors du menu le ferme sans rien lancer.
        assert_eq!(
            click(point(700.0, 400.0), Some(Popover::Launcher)),
            Some(DesktopAction::ClosePopover)
        );
        // Sans menu ouvert, le bureau laisse passer le clic.
        assert_eq!(click(point(700.0, 400.0), None), None);
    }

    #[test]
    fn les_reglages_rapides_pilotent_wifi_bluetooth_son_et_luminosite() {
        let layout = layout();
        let open = Some(Popover::Controls);
        let wifi = layout.control_row(Control::Wifi).center();
        assert_eq!(
            click(point(wifi.0, wifi.1), open),
            Some(DesktopAction::ToggleWifi)
        );
        let bluetooth = layout.control_row(Control::Bluetooth).center();
        assert_eq!(
            click(point(bluetooth.0, bluetooth.1), open),
            Some(DesktopAction::ToggleBluetooth)
        );
        let volume = layout.slider_track(Control::Volume);
        assert_eq!(
            click(point(volume.x + volume.w / 4.0, volume.y), open),
            Some(DesktopAction::SetVolume(25))
        );
        let brightness = layout.slider_track(Control::Brightness);
        assert_eq!(
            click(point(brightness.right() + 5.0, brightness.y), open),
            Some(DesktopAction::SetBrightness(100))
        );
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

        let pending = Some(PowerAction::PowerOff);
        let confirm = layout.dialog_confirm().center();
        assert_eq!(
            action_at(output(), point(confirm.0, confirm.1), None, pending),
            Some(DesktopAction::ConfirmPower(PowerAction::PowerOff))
        );
        let cancel = layout.dialog_cancel().center();
        assert_eq!(
            action_at(output(), point(cancel.0, cancel.1), None, pending),
            Some(DesktopAction::CancelPower)
        );
        let inside = layout.dialog();
        assert_eq!(
            action_at(output(), point(inside.x + 10.0, inside.y + 10.0), None, pending),
            Some(DesktopAction::Consume)
        );
        assert_eq!(
            action_at(output(), point(5.0, 700.0), None, pending),
            Some(DesktopAction::CancelPower)
        );
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
