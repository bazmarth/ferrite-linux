//! Gestion des fenêtres : placement, focus, décorations, déplacement, redimensionnement,
//! agrandissement et réduction.

pub mod animation;
pub mod decoration;
pub mod keep_above;
mod move_grab;
mod resize_grab;
pub mod switcher;
pub mod window_menu;
pub mod workspaces;

pub use move_grab::{BTN_LEFT, BTN_RIGHT, MoveSurfaceGrab};
pub use resize_grab::{ResizeEdge, ResizeSurfaceGrab, handle_commit as handle_resize_commit};

use std::time::{Duration, Instant};

use ferrite_ui::theme::TITLEBAR_HEIGHT;
use smithay::{
    desktop::{Window, WindowSurfaceType},
    input::pointer::{Focus, GrabStartData},
    reexports::{
        wayland_protocols::xdg::{
            decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
            shell::server::xdg_toplevel,
        },
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Serial, Size},
    wayland::{seat::WaylandFocus, shell::xdg::XdgShellHandler},
};
use tracing::info;

use self::decoration::{Part, WindowButton};
use crate::state::Ferrite;

/// Marge entre la première fenêtre et le coin de la zone utile.
const WINDOW_MARGIN: i32 = 40;
/// Délai maximal entre deux clics pour un double-clic.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Distance du bord de l'écran qui déclenche l'accrochage d'une fenêtre.
const SNAP_EDGE_MARGIN: i32 = 24;
/// Longueur des zones de coin, le long des bords, qui donnent un quart d'écran.
const SNAP_CORNER: i32 = 140;

/// Place d'une fenêtre accrochée, comme sous Windows : moitiés, quarts, plein écran utile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snap {
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Maximize,
    /// Deux tiers ou tiers d'écran, à gauche ou à droite (Super+← ou → répété).
    TwoThirdsLeft,
    TwoThirdsRight,
    ThirdLeft,
    ThirdRight,
    /// Tiers de l'écran, de 0 à 2 (Super+Alt+1, 2, 3).
    Third(u8),
    /// Centrée sur 64 % de la largeur (Super+C).
    Center,
    /// Part de la zone utile, en millièmes : x, y, largeur, hauteur (tuilage automatique).
    Part([u16; 4]),
}

impl Snap {
    /// Part de la zone utile qu'occupe la fenêtre, en millièmes : x, y, largeur, hauteur.
    pub fn part(self) -> [u16; 4] {
        match self {
            Snap::Maximize => [0, 0, 1000, 1000],
            Snap::Left => [0, 0, 500, 1000],
            Snap::Right => [500, 0, 500, 1000],
            Snap::TopLeft => [0, 0, 500, 500],
            Snap::TopRight => [500, 0, 500, 500],
            Snap::BottomLeft => [0, 500, 500, 500],
            Snap::BottomRight => [500, 500, 500, 500],
            Snap::TwoThirdsLeft => [0, 0, 660, 1000],
            Snap::TwoThirdsRight => [340, 0, 660, 1000],
            Snap::ThirdLeft => [0, 0, 340, 1000],
            Snap::ThirdRight => [660, 0, 340, 1000],
            Snap::Third(index) => [u16::from(index.min(2)) * 333, 0, 334, 1000],
            Snap::Center => [180, 0, 640, 1000],
            Snap::Part(part) => part,
        }
    }

    /// Accrochage suivant quand Super+← ou → est répété : moitié, deux tiers, puis tiers.
    pub fn next_on_repeat(self, left: bool) -> Snap {
        match (self, left) {
            (Snap::Left, true) => Snap::TwoThirdsLeft,
            (Snap::TwoThirdsLeft, true) => Snap::ThirdLeft,
            (Snap::Right, false) => Snap::TwoThirdsRight,
            (Snap::TwoThirdsRight, false) => Snap::ThirdRight,
            (_, true) => Snap::Left,
            (_, false) => Snap::Right,
        }
    }
}

/// Angle de l'écran où se trouve `at` : 0 haut gauche, 1 haut droit, 2 bas gauche, 3 bas droit.
pub fn screen_corner(area: Rectangle<i32, Logical>, at: Point<f64, Logical>) -> Option<usize> {
    let (left, top) = (f64::from(area.loc.x), f64::from(area.loc.y));
    let (right, bottom) = (
        left + f64::from(area.size.w) - 1.0,
        top + f64::from(area.size.h) - 1.0,
    );
    match (at.x <= left, at.x >= right, at.y <= top, at.y >= bottom) {
        (true, _, true, _) => Some(0),
        (_, true, true, _) => Some(1),
        (true, _, _, true) => Some(2),
        (_, true, _, true) => Some(3),
        _ => None,
    }
}

/// Marge autour de la zone de travail, et espacement entre deux fenêtres aimantées.
pub const SNAP_GAP: i32 = 12;

/// Cadre d'une part (millièmes) de `area` : marges de 12 px et écart de 12 px entre voisines.
pub fn part_zone(area: Rectangle<i32, Logical>, part: [u16; 4]) -> Rectangle<i32, Logical> {
    let gap = SNAP_GAP as f64;
    let (aw, ah) = (area.size.w as f64 - 2.0 * gap, area.size.h as f64 - 2.0 * gap);
    let [fx, fy, fw, fh] = part.map(|value| f64::from(value) / 1000.0);
    let split = |start: f64, length: f64, total: f64| {
        let before = if start > 0.0 { gap / 2.0 } else { 0.0 };
        let after = if start + length < 0.999 { gap / 2.0 } else { 0.0 };
        (start * total + before, length * total - before - after)
    };
    let (x, w) = split(fx, fw, aw);
    let (y, h) = split(fy, fh, ah);
    Rectangle::new(
        Point::from((
            area.loc.x + SNAP_GAP + x.round() as i32,
            area.loc.y + SNAP_GAP + y.round() as i32,
        )),
        Size::from((w.round() as i32, h.round() as i32)),
    )
}

/// Accrochage proposé quand on lâche une fenêtre avec le pointeur en `pointer` : bord
/// gauche ou droit pour une moitié, coin pour un quart, bord du haut pour tout l'écran.
/// `screen` est l'écran entier (le bord du haut est celui de l'écran, pas de la zone utile).
pub fn snap_at(
    area: Rectangle<i32, Logical>,
    screen: Rectangle<i32, Logical>,
    pointer: Point<f64, Logical>,
) -> Option<Snap> {
    let margin = SNAP_EDGE_MARGIN as f64;
    let corner = SNAP_CORNER as f64;
    let (left, right) = (area.loc.x as f64, (area.loc.x + area.size.w) as f64);
    let (top, bottom) = (area.loc.y as f64, (area.loc.y + area.size.h) as f64);
    let at_left = pointer.x < left + margin;
    let at_right = pointer.x >= right - margin;
    let at_top = pointer.y < screen.loc.y as f64 + margin;
    let near_top = pointer.y < top + corner;
    let near_bottom = pointer.y >= bottom - corner;
    if at_left || at_right {
        return Some(match (at_left, near_top, near_bottom) {
            (true, true, _) => Snap::TopLeft,
            (true, _, true) => Snap::BottomLeft,
            (true, _, _) => Snap::Left,
            (false, true, _) => Snap::TopRight,
            (false, _, true) => Snap::BottomRight,
            (false, _, _) => Snap::Right,
        });
    }
    if at_top {
        return Some(if pointer.x < left + corner {
            Snap::TopLeft
        } else if pointer.x >= right - corner {
            Snap::TopRight
        } else {
            Snap::Maximize
        });
    }
    None
}

/// Cadre (barre de titre comprise) d'une fenêtre accrochée en `snap` dans `area`.
pub fn snap_zone(area: Rectangle<i32, Logical>, snap: Snap) -> Rectangle<i32, Logical> {
    match snap {
        Snap::Maximize => area,
        other => part_zone(area, other.part()),
    }
}

/// Position d'une fenêtre décrochée (contenu `current`) qui reprend la largeur `size` :
/// le pointeur reste au même endroit relatif de sa largeur, et à la même hauteur.
pub fn unsnapped_location(
    current: Rectangle<i32, Logical>,
    size: Size<i32, Logical>,
    pointer: Point<f64, Logical>,
) -> Point<i32, Logical> {
    let ratio = ((pointer.x - current.loc.x as f64) / current.size.w.max(1) as f64).clamp(0.0, 1.0);
    let x = pointer.x - ratio * size.w as f64;
    Point::from((x.round() as i32, current.loc.y))
}

/// Contenu d'une fenêtre accrochée, la barre de titre Ferrite comprise dans la zone.
pub fn snapped_content(
    area: Rectangle<i32, Logical>,
    snap: Snap,
    decorated: bool,
) -> Rectangle<i32, Logical> {
    maximized_content(snap_zone(area, snap), decorated)
}

/// Cadre de taille `window` centré dans `area` : toute fenêtre s'ouvre ainsi, en gardant la
/// marge si elle est trop grande.
pub fn centered_position(area: Rectangle<i32, Logical>, window: Size<i32, Logical>) -> Point<i32, Logical> {
    let centered =
        |total: i32, used: i32| (WINDOW_MARGIN + (total - 2 * WINDOW_MARGIN - used).max(0) / 2).max(0);
    area.loc + Point::from((centered(area.size.w, window.w), centered(area.size.h, window.h)))
}

/// Ramène un cadre dans `area` autant que possible (le haut reste toujours visible).
pub fn clamp_frame(
    area: Rectangle<i32, Logical>,
    frame: Point<i32, Logical>,
    size: Size<i32, Logical>,
) -> Point<i32, Logical> {
    let clamp =
        |position: i32, start: i32, total: i32, used: i32| position.min(start + total - used).max(start);
    Point::from((
        clamp(frame.x, area.loc.x, area.size.w, size.w),
        clamp(frame.y, area.loc.y, area.size.h, size.h),
    ))
}

/// Taille du contenu d'une fenêtre trop grande pour `area` (avec sa barre de titre de
/// `titlebar`), ramenée dans la zone sans descendre sous `min` ; `None` si elle y tient.
pub fn fitted_size(
    area: Rectangle<i32, Logical>,
    size: Size<i32, Logical>,
    titlebar: i32,
    min: Size<i32, Logical>,
) -> Option<Size<i32, Logical>> {
    let max_w = area.size.w.max(min.w);
    let max_h = (area.size.h - titlebar).max(min.h).max(1);
    let fitted = Size::from((size.w.min(max_w), size.h.min(max_h)));
    (fitted != size).then_some(fitted)
}

/// Zone utile d'un écran de taille `output` : tout sauf les bandes réservées à la barre du
/// haut (`bar_reserved`) et au Dock (`dock_reserved`), nulles quand ils se cachent.
pub fn usable_area(
    output: Rectangle<i32, Logical>,
    bar_reserved: i32,
    dock_reserved: i32,
) -> Rectangle<i32, Logical> {
    Rectangle::new(
        output.loc + Point::from((0, bar_reserved)),
        Size::from((
            output.size.w,
            (output.size.h - bar_reserved - dock_reserved).max(0),
        )),
    )
}

/// La fenêtre est-elle en plein écran ?
pub fn is_fullscreen(window: &Window) -> bool {
    if let Some(x11) = window.x11_surface() {
        return x11.is_fullscreen();
    }
    window.toplevel().is_some_and(|toplevel| {
        toplevel
            .current_state()
            .states
            .contains(xdg_toplevel::State::Fullscreen)
    })
}

/// La fenêtre est-elle agrandie (coins droits, sans ombre) ?
pub fn is_maximized(window: &Window) -> bool {
    if let Some(x11) = window.x11_surface() {
        return x11.is_maximized();
    }
    window.toplevel().is_some_and(|toplevel| {
        toplevel
            .current_state()
            .states
            .contains(xdg_toplevel::State::Maximized)
    })
}

/// La fenêtre est-elle aimantée à un bord (ombre réduite) ?
pub fn is_tiled(window: &Window) -> bool {
    window.toplevel().is_some_and(|toplevel| {
        let states = toplevel.current_state().states;
        // Une fenêtre agrandie depuis une moitié peut garder ces états : elle n'est plus aimantée.
        !states.contains(xdg_toplevel::State::Maximized)
            && (states.contains(xdg_toplevel::State::TiledLeft)
                || states.contains(xdg_toplevel::State::TiledRight)
                || states.contains(xdg_toplevel::State::TiledTop)
                || states.contains(xdg_toplevel::State::TiledBottom))
    })
}

/// Contenu d'une fenêtre agrandie dans `area` : sous la barre de titre si elle est décorée.
pub fn maximized_content(area: Rectangle<i32, Logical>, decorated: bool) -> Rectangle<i32, Logical> {
    if !decorated {
        return area;
    }
    let bar = TITLEBAR_HEIGHT as i32;
    Rectangle::new(
        area.loc + Point::from((0, bar)),
        Size::from((area.size.w, (area.size.h - bar).max(1))),
    )
}

/// Une application Ferrite dessine elle-même sa barre de titre.
pub fn is_ferrite_app(surface: &smithay::wayland::shell::xdg::ToplevelSurface) -> bool {
    smithay::wayland::compositor::with_states(surface.wl_surface(), |states| {
        states
            .data_map
            .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
            .and_then(|data| data.lock().ok()?.app_id.clone())
    })
    .is_some_and(|id| id.starts_with("ferrite-"))
}

/// La fenêtre est-elle décorée par ferrite-shell (barre de titre, ombre, coins) ?
/// Les clients qui dessinent leurs propres décorations (mode client) ne le sont pas, ni les
/// fenêtres en plein écran.
/// Fenêtre d'une application Ferrite qui dessine elle-même son haut (colonne et barre
/// d'outils, feux compris) : ferrite-shell garde les coins arrondis, l'ombre et les bords
/// de redimensionnement, sans barre de titre.
pub fn is_unified(window: &Window) -> bool {
    window.toplevel().is_some_and(|toplevel| {
        let state = toplevel.current_state();
        is_ferrite_app(toplevel)
            && state.decoration_mode == Some(DecorationMode::ClientSide)
            && !state.states.contains(xdg_toplevel::State::Fullscreen)
    })
}

pub fn is_server_decorated(window: &Window) -> bool {
    if let Some(x11) = window.x11_surface() {
        // `is_decorated` signale une fenêtre X11 qui dessine son propre cadre (Brave,
        // Chromium) ; les menus et infobulles n'en ont jamais.
        return !x11.is_override_redirect() && !x11.is_decorated() && !x11.is_fullscreen();
    }
    window.toplevel().is_some_and(|toplevel| {
        let state = toplevel.current_state();
        state.decoration_mode == Some(DecorationMode::ServerSide)
            && !state.states.contains(xdg_toplevel::State::Fullscreen)
    })
}

/// Comme `is_server_decorated`, mais d'après le mode déjà décidé par ferrite-shell, même si
/// le client ne l'a pas encore confirmé. Une application qui demande à être agrandie dès son
/// ouverture (WordCraft) le fait avant d'avoir reçu sa première configuration : l'état
/// confirmé ne dit pas encore qu'elle aura la barre de Ferrite, et la fenêtre prenait tout
/// l'écran, barre de titre (et ses boutons) hors de l'écran.
pub fn will_be_server_decorated(window: &Window) -> bool {
    if window.x11_surface().is_some() {
        return is_server_decorated(window);
    }
    window.toplevel().is_some_and(|toplevel| {
        let pending = toplevel.with_pending_state(|state| {
            (
                state.decoration_mode,
                state.states.contains(xdg_toplevel::State::Fullscreen),
            )
        });
        let mode = pending.0.or(toplevel.current_state().decoration_mode);
        mode == Some(DecorationMode::ServerSide) && !pending.1
    })
}

/// Cadre complet utilisé par les animations, barre de titre serveur comprise.
pub fn animation_frame(window: &Window, content: Rectangle<i32, Logical>) -> Rectangle<f64, Logical> {
    let frame = if is_server_decorated(window) {
        decoration::frame(content)
    } else {
        content
    };
    frame.to_f64()
}

/// Demande à une fenêtre de se fermer (Wayland ou X11).
pub fn close_window(window: &Window) {
    if let Some(toplevel) = window.toplevel() {
        toplevel.send_close();
    } else if let Some(x11) = window.x11_surface() {
        if let Err(err) = x11.close() {
            tracing::warn!(?err, "fenêtre X11 non fermée");
        }
    }
}

/// La fenêtre est-elle active (au premier plan, avec le clavier) ?
pub fn is_activated(window: &Window) -> bool {
    if let Some(x11) = window.x11_surface() {
        return x11.is_activated();
    }
    window.toplevel().is_some_and(|toplevel| {
        toplevel
            .current_state()
            .states
            .contains(xdg_toplevel::State::Activated)
    })
}

/// Ce qui se trouve sous un point de l'écran.
#[derive(Debug, Clone)]
pub enum Hit {
    /// Décoration d'une fenêtre (barre de titre, feu, bord).
    Decoration(Window, Part),
    /// Surface d'un client, avec la position de son origine.
    Surface(Window, WlSurface, Point<f64, Logical>),
    /// Dans le cadre d'une fenêtre, mais sur aucune de ses surfaces (contenu plus petit que
    /// le cadre, trou dans sa zone cliquable) : le point reste à elle et ne traverse pas.
    Body(Window),
}

/// Fenêtre réduite, avec la position où la remettre.
#[derive(Debug)]
pub struct Minimized {
    pub window: Window,
    pub location: Point<i32, Logical>,
    pub workspace: usize,
}

impl Ferrite {
    /// Contenu d'une fenêtre à sa place, à la taille qu'on vient de lui demander : elle
    /// n'a peut-être pas encore redessiné, et les animations visent sa taille d'arrivée.
    pub fn target_content(&self, window: &Window) -> Option<Rectangle<i32, Logical>> {
        let content = self.space.element_geometry(window)?;
        let pending = window
            .toplevel()
            .and_then(|toplevel| toplevel.with_pending_state(|state| state.size));
        Some(match pending {
            Some(size) if size.w > 0 && size.h > 0 => Rectangle::new(content.loc, size),
            _ => content,
        })
    }

    /// Met `window` au premier plan et lui donne le clavier (ou retire le focus si `None`).
    pub fn focus_window(&mut self, window: Option<Window>) {
        let serial = SERIAL_COUNTER.next_serial();
        // Verrouillé : aucune fenêtre, même nouvelle, ne reçoit le clavier.
        let window = window.filter(|_| self.lock.is_none());
        // Une fenêtre qui prend la main renvoie les widgets derrière et clôt leur saisie.
        if window.is_some() {
            self.widgets_front = false;
            self.stop_dashboard_editing();
        }
        // Les menus X11 ne prennent jamais le clavier.
        let window = window.filter(|w| !w.x11_surface().is_some_and(|x11| x11.is_override_redirect()));
        let surface: Option<WlSurface> = window
            .as_ref()
            .and_then(|w| w.wl_surface())
            .map(|surface| surface.into_owned());
        match &window {
            Some(window) => {
                if let Some(key) = self.window_app(window) {
                    self.note_recent_app(&key);
                }
                self.space.raise_element(window, true);
                if let (Some(x11), Some(xwm)) = (window.x11_surface(), self.xwm.as_mut()) {
                    if let Err(err) = xwm.raise_window(x11) {
                        tracing::warn!(?err, "fenêtre X11 non remontée");
                    }
                }
                // Ses boîtes de dialogue X11 restent au-dessus d'elle.
                if let Some(id) = window.x11_surface().map(|x11| x11.window_id()) {
                    let dialogs: Vec<Window> = self
                        .space
                        .elements()
                        .filter(|other| {
                            other
                                .x11_surface()
                                .is_some_and(|x11| x11.is_transient_for() == Some(id))
                        })
                        .cloned()
                        .collect();
                    for dialog in dialogs {
                        self.space.raise_element(&dialog, false);
                        if let (Some(x11), Some(xwm)) = (dialog.x11_surface(), self.xwm.as_mut()) {
                            let _ = xwm.raise_window(x11);
                        }
                    }
                }
            }
            None => self.space.elements().for_each(|w| {
                w.set_activated(false);
            }),
        }
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, surface, serial);
        }
        for window in self.space.elements() {
            if let Some(toplevel) = window.toplevel() {
                toplevel.send_pending_configure();
            }
        }
        // La fenêtre remontée peut se trouver sous le pointeur immobile (nouvelle fenêtre,
        // Alt+Tab, Dock) : le survol passe à elle sans attendre un mouvement.
        if self
            .seat
            .get_pointer()
            .is_some_and(|pointer| !pointer.is_grabbed())
        {
            self.refresh_pointer_focus();
        }
    }

    /// Fenêtre de devant, s'il y en a une : la plus haute des fenêtres ordinaires, sinon
    /// une fenêtre gardée au-dessus (elle reste devant sans être celle qu'on utilise).
    pub fn top_window(&self) -> Option<Window> {
        self.space
            .elements()
            .rev()
            .find(|window| !keep_above::is_kept_above(window))
            .or_else(|| self.space.elements().next_back())
            .cloned()
    }

    /// Recherche de ce qui est sous `pos`, de la fenêtre du dessus vers celle du dessous :
    /// la barre de titre d'une fenêtre masque le contenu des fenêtres placées dessous.
    pub fn hit_test(&self, pos: Point<f64, Logical>) -> Option<Hit> {
        // Ni les aperçus de la vue d'ensemble, ni les fenêtres sous la sélection d'une zone
        // à capturer ou sous l'écran de verrouillage ne reçoivent de survol ou de clic.
        if self.overview
            || self.region_select.is_some()
            || self.region_adjust.is_some()
            || self.window_select
            || self.popover == Some(crate::desktop::panels::Popover::Screenshot)
            || self.lock.is_some()
        {
            return None;
        }
        for window in self.space.elements().rev() {
            let Some(content) = self.space.element_geometry(window) else {
                continue;
            };
            if is_server_decorated(window) {
                if let Some(part) = decoration::hit(content, pos) {
                    return Some(Hit::Decoration(window.clone(), part));
                }
            } else if is_unified(window)
                && let Some(part) = decoration::hit_border(content, pos)
            {
                return Some(Hit::Decoration(window.clone(), part));
            }
            let render_location = content.loc - window.geometry().loc;
            if let Some((surface, location)) =
                window.surface_under(pos - render_location.to_f64(), WindowSurfaceType::ALL)
            {
                return Some(Hit::Surface(
                    window.clone(),
                    surface,
                    (location + render_location).to_f64(),
                ));
            }
            // Ce qu'on voit de la fenêtre masque toujours celles du dessous.
            if content.to_f64().contains(pos) {
                return Some(Hit::Body(window.clone()));
            }
        }
        None
    }

    /// Commence le déplacement de `window` à la souris (barre de titre ou demande du client).
    pub fn start_move(&mut self, window: Window, start_data: GrabStartData<Ferrite>, serial: Serial) {
        // En tuilage, une fenêtre rangée garde sa taille : lâchée, elle échange sa place
        // ou y retourne (voir `drop_tiled`).
        let tiled = self.tiling && self.tile_order.contains(&window);
        let released = if tiled {
            None
        } else {
            self.release_snapped(&window, start_data.location)
        };
        let initial_window_location = match released {
            Some(location) => location,
            None => match self.space.element_location(&window) {
                Some(location) => location,
                None => return,
            },
        };
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let titlebar = if is_server_decorated(&window) {
            TITLEBAR_HEIGHT as i32
        } else {
            0
        };
        let grab = MoveSurfaceGrab {
            start_data,
            window,
            initial_window_location,
            min_y: self.dock_config.bar_reserved_height() + titlebar,
        };
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    /// Données de départ d'un déplacement demandé par un client X11 : le clic en cours.
    pub fn current_grab_start(&self) -> Option<GrabStartData<Ferrite>> {
        let pointer = self.seat.get_pointer()?;
        Some(pointer.grab_start_data().unwrap_or_else(|| GrabStartData {
            focus: None,
            button: BTN_LEFT,
            location: pointer.current_location(),
        }))
    }

    /// Commence le redimensionnement de `window` par les bords `edges`.
    pub fn start_resize(
        &mut self,
        window: Window,
        edges: ResizeEdge,
        start_data: GrabStartData<Ferrite>,
        serial: Serial,
    ) {
        let Some(location) = self.space.element_location(&window) else {
            return;
        };
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Resizing);
            });
            toplevel.send_pending_configure();
        } else if window.x11_surface().is_none() {
            return;
        }
        let size = window.geometry().size;
        self.resizing = Some(edges);
        let grab = ResizeSurfaceGrab::start(start_data, window, edges, Rectangle::new(location, size));
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    /// Agrandit la fenêtre, ou lui rend sa taille si elle l'est déjà.
    pub fn toggle_maximize(&mut self, window: &Window) {
        if let Some(x11) = window.x11_surface() {
            let maximized = x11.is_maximized();
            self.x11_set_maximized(window, !maximized);
            return;
        }
        let Some(toplevel) = window.toplevel().cloned() else {
            return;
        };
        let states = toplevel.current_state().states;
        if states.contains(xdg_toplevel::State::Maximized)
            || states.contains(xdg_toplevel::State::TiledLeft)
            || states.contains(xdg_toplevel::State::TiledRight)
            || states.contains(xdg_toplevel::State::TiledTop)
            || states.contains(xdg_toplevel::State::TiledBottom)
        {
            self.unmaximize_request(toplevel);
        } else {
            self.maximize_request(toplevel);
        }
    }

    /// Réduit la fenêtre : elle disparaît de l'écran jusqu'à `restore_minimized`.
    pub fn minimize(&mut self, window: &Window) {
        let Some(location) = self.space.element_location(window) else {
            return;
        };
        // La fenêtre file vers son icône du Dock.
        if let Some(content) = self.space.element_geometry(window) {
            let key = self.window_app(window);
            if let Some(icon) = self.dock_icon_rect(key.as_ref()) {
                self.window_animations
                    .borrow_mut()
                    .push(animation::WindowAnimation::new(
                        window.clone(),
                        animation::Motion::Minimize,
                        animation_frame(window, content),
                        icon,
                    ));
            }
        }
        self.space.unmap_elem(window);
        self.minimized.push(Minimized {
            window: window.clone(),
            location,
            workspace: self.workspaces.current(),
        });
        let next = self.top_window();
        self.focus_window(next);
    }

    /// Remet à l'écran la dernière fenêtre réduite (Ctrl+Alt+M).
    pub fn restore_minimized(&mut self) {
        if !self.minimized.is_empty() {
            self.restore_minimized_at(self.minimized.len() - 1);
        }
    }

    /// Remet à l'écran la fenêtre réduite `index` : elle ressort de son icône du Dock.
    pub fn restore_minimized_at(&mut self, index: usize) {
        if index >= self.minimized.len() {
            return;
        }
        let Minimized { window, location, .. } = self.minimized.remove(index);
        let key = self.window_app(&window);
        let icon = self.dock_icon_rect(key.as_ref());
        // Une animation de réduction encore en cours s'arrête.
        self.window_animations
            .borrow_mut()
            .retain(|animation| animation.window != window);
        self.space.map_element(window.clone(), location, true);
        // En tuilage, elle ressort directement à la place qui l'attend (sans le glissement
        // que le tuilage vient de lancer).
        self.retile();
        self.window_animations
            .borrow_mut()
            .retain(|animation| animation.window != window);
        if let (Some(icon), Some(content)) = (icon, self.target_content(&window)) {
            self.window_animations
                .borrow_mut()
                .push(animation::WindowAnimation::new(
                    window.clone(),
                    animation::Motion::Restore,
                    animation_frame(&window, content),
                    icon,
                ));
        }
        self.focus_window(Some(window));
    }

    /// Fenêtre réduite qui correspond à `surface`, s'il y en a une.
    pub fn minimized_index(&self, surface: &WlSurface) -> Option<usize> {
        self.minimized
            .iter()
            .position(|minimized| minimized.window.wl_surface().is_some_and(|own| *own == *surface))
    }

    /// Action d'un bouton de fenêtre cliqué.
    pub fn activate_light(&mut self, window: &Window, button: WindowButton) {
        match button {
            WindowButton::Close => close_window(window),
            WindowButton::Minimize => self.minimize(window),
            // En tuilage, le bouton vert d'une fenêtre de la pile la met en grand.
            WindowButton::Zoom => {
                if !self.promote_tiled(window) {
                    self.toggle_maximize(window);
                }
            }
        }
    }

    /// Enregistre un clic sur la barre de titre ; renvoie vrai si c'est un double-clic.
    pub fn titlebar_click(&mut self, window: &Window) -> bool {
        let now = Instant::now();
        let double = self
            .last_titlebar_click
            .as_ref()
            .is_some_and(|(at, last)| last == window && now.duration_since(*at) <= DOUBLE_CLICK);
        self.last_titlebar_click = if double { None } else { Some((now, window.clone())) };
        if double {
            info!("double-clic sur la barre de titre");
        }
        double
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn zone_utile_sous_la_barre_de_menus() {
        assert_eq!(usable_area(rect(0, 0, 1440, 900), 30, 0), rect(0, 30, 1440, 870));
        assert_eq!(usable_area(rect(0, 0, 1440, 900), 30, 84), rect(0, 30, 1440, 786));
        // Barre masquable : les fenêtres montent jusqu'en haut.
        assert_eq!(usable_area(rect(0, 0, 1440, 900), 0, 0), rect(0, 0, 1440, 900));
    }

    #[test]
    fn une_fenetre_trop_grande_est_ramenee_dans_la_zone_utile() {
        let area = rect(0, 30, 1440, 786);
        let size = |w, h| Size::from((w, h));
        assert_eq!(fitted_size(area, size(800, 600), 46, size(0, 0)), None);
        assert_eq!(
            fitted_size(area, size(1920, 1080), 46, size(0, 0)),
            Some(size(1440, 740))
        );
        // Jamais sous la taille minimale demandée par l'application.
        assert_eq!(
            fitted_size(area, size(1920, 1080), 46, size(1600, 900)),
            Some(size(1600, 900))
        );
    }

    #[test]
    fn agrandissement_sous_la_barre_de_titre() {
        let area = rect(0, 30, 1440, 870);
        assert_eq!(maximized_content(area, true), rect(0, 68, 1440, 832));
        assert_eq!(maximized_content(area, false), area);
    }

    #[test]
    fn angles_de_l_ecran() {
        let area = rect(0, 0, 1440, 900);
        let at = |x: f64, y: f64| screen_corner(area, Point::from((x, y)));
        assert_eq!(at(0.0, 0.0), Some(0));
        assert_eq!(at(1439.0, 0.0), Some(1));
        assert_eq!(at(0.0, 899.0), Some(2));
        assert_eq!(at(1439.0, 899.0), Some(3));
        assert_eq!(at(700.0, 0.0), None);
    }

    #[test]
    fn une_fenetre_s_ouvre_au_centre() {
        let area = rect(0, 30, 1440, 870);
        assert_eq!(
            centered_position(area, Size::from((800, 600))),
            Point::from((320, 165))
        );
        // Trop grande : la marge reste en haut à gauche.
        assert_eq!(
            centered_position(area, Size::from((1400, 900))),
            Point::from((WINDOW_MARGIN, 30 + WINDOW_MARGIN))
        );
    }

    #[test]
    fn un_cadre_trop_bas_remonte_dans_la_zone_utile() {
        let area = rect(0, 30, 1280, 694);
        let size = Size::from((860, 550));
        assert_eq!(
            clamp_frame(area, Point::from((210, 198)), size),
            Point::from((210, 174))
        );
        // Plus grand que la zone : collé en haut, jamais sous la barre.
        assert_eq!(
            clamp_frame(area, Point::from((0, 0)), Size::from((900, 900))),
            Point::from((0, 30))
        );
    }

    #[test]
    fn accrochage_garde_les_marges_et_l_ecart_de_12_px() {
        let area = rect(0, 30, 1440, 870);
        assert_eq!(snapped_content(area, Snap::Left, true), rect(12, 80, 702, 808));
        assert_eq!(snapped_content(area, Snap::Right, true), rect(726, 80, 702, 808));
        // Quarts : 12 px entre les fenêtres, et autour de la zone.
        assert_eq!(snap_zone(area, Snap::TopLeft), rect(12, 42, 702, 417));
        assert_eq!(snap_zone(area, Snap::BottomRight), rect(726, 471, 702, 417));
        assert_eq!(snap_zone(area, Snap::Maximize), area);
        // Centrée : 64 % de la largeur.
        let center = snap_zone(area, Snap::Center);
        assert_eq!(center.loc.x + center.size.w / 2, 720);
    }

    #[test]
    fn super_fleche_repetee_passe_par_les_deux_tiers_puis_le_tiers() {
        assert_eq!(Snap::Left.next_on_repeat(true), Snap::TwoThirdsLeft);
        assert_eq!(Snap::TwoThirdsLeft.next_on_repeat(true), Snap::ThirdLeft);
        assert_eq!(Snap::ThirdLeft.next_on_repeat(true), Snap::Left);
        assert_eq!(Snap::Left.next_on_repeat(false), Snap::Right);
    }

    #[test]
    fn une_fenetre_decrochee_reste_sous_le_pointeur() {
        // Agrandie sur 1440 px, saisie au milieu : elle reprend 800 px, centrée sur le pointeur.
        let location = unsnapped_location(
            rect(0, 60, 1440, 840),
            Size::from((800, 500)),
            Point::from((720.0, 45.0)),
        );
        assert_eq!(location, Point::from((320, 60)));
    }

    #[test]
    fn accrochage_selon_le_bord_ou_le_coin() {
        let screen = rect(0, 0, 1440, 900);
        let area = rect(0, 30, 1440, 870);
        let at = |x: f64, y: f64| snap_at(area, screen, Point::from((x, y)));
        assert_eq!(at(0.0, 450.0), Some(Snap::Left));
        assert_eq!(at(1439.0, 450.0), Some(Snap::Right));
        assert_eq!(at(720.0, 450.0), None);
        // Coins : quarts d'écran.
        assert_eq!(at(0.0, 60.0), Some(Snap::TopLeft));
        assert_eq!(at(1439.0, 890.0), Some(Snap::BottomRight));
        assert_eq!(at(5.0, 899.0), Some(Snap::BottomLeft));
        // Bord du haut : tout l'écran, ou un quart près des coins.
        assert_eq!(at(720.0, 0.0), Some(Snap::Maximize));
        assert_eq!(at(1400.0, 2.0), Some(Snap::TopRight));
    }
}
