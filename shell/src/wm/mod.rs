//! Gestion des fenêtres : placement, focus, décorations, déplacement, redimensionnement,
//! agrandissement et réduction.

pub mod decoration;
mod move_grab;
mod resize_grab;

pub use move_grab::{BTN_LEFT, MoveSurfaceGrab};
pub use resize_grab::{ResizeEdge, ResizeSurfaceGrab, handle_commit as handle_resize_commit};

use std::time::{Duration, Instant};

use ferrite_ui::theme::{MENUBAR_HEIGHT, TITLEBAR_HEIGHT};
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
    wayland::shell::xdg::XdgShellHandler,
};
use tracing::info;

use self::decoration::{Part, TrafficLight};
use crate::state::Ferrite;

/// Décalage entre deux fenêtres ouvertes à la suite.
const CASCADE_STEP: i32 = 30;
/// Marge entre la première fenêtre et le coin de la zone utile.
const CASCADE_MARGIN: i32 = 40;
/// Délai maximal entre deux clics pour un double-clic.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Placement en cascade des nouvelles fenêtres, comme sur macOS : chaque fenêtre est
/// décalée vers le bas et la droite ; on repart du coin quand elle sortirait de l'écran.
#[derive(Debug, Default)]
pub struct Placement {
    next: u32,
}

impl Placement {
    /// Position du cadre de la prochaine fenêtre dans la zone `area` (l'écran sous la
    /// barre de menus).
    pub fn next_position(
        &mut self,
        area: Rectangle<i32, Logical>,
        window: Size<i32, Logical>,
    ) -> Point<i32, Logical> {
        let origin = area.loc + Point::from((CASCADE_MARGIN, CASCADE_MARGIN));
        let room = |total: i32, used: i32| (total - 2 * CASCADE_MARGIN - used).max(0) / CASCADE_STEP;
        let steps = room(area.size.w, window.w).min(room(area.size.h, window.h)) as u32;
        let index = if steps == 0 { 0 } else { self.next % (steps + 1) };
        self.next = self.next.wrapping_add(1);
        let offset = index as i32 * CASCADE_STEP;
        origin + Point::from((offset, offset))
    }
}

/// Zone utile d'un écran de taille `output` : tout sauf la barre de menus.
pub fn usable_area(output: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    let menubar = MENUBAR_HEIGHT as i32;
    Rectangle::new(
        output.loc + Point::from((0, menubar)),
        Size::from((output.size.w, (output.size.h - menubar).max(0))),
    )
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

/// La fenêtre est-elle décorée par ferrite-shell (barre de titre, ombre, coins) ?
/// Les clients qui dessinent leurs propres décorations (mode client) ne le sont pas, ni les
/// fenêtres en plein écran.
pub fn is_server_decorated(window: &Window) -> bool {
    window.toplevel().is_some_and(|toplevel| {
        let state = toplevel.current_state();
        state.decoration_mode == Some(DecorationMode::ServerSide)
            && !state.states.contains(xdg_toplevel::State::Fullscreen)
    })
}

/// La fenêtre est-elle active (au premier plan, avec le clavier) ?
pub fn is_activated(window: &Window) -> bool {
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
}

/// Fenêtre réduite, avec la position où la remettre.
#[derive(Debug)]
pub struct Minimized {
    pub window: Window,
    pub location: Point<i32, Logical>,
}

impl Ferrite {
    /// Met `window` au premier plan et lui donne le clavier (ou retire le focus si `None`).
    pub fn focus_window(&mut self, window: Option<Window>) {
        let serial = SERIAL_COUNTER.next_serial();
        let surface: Option<WlSurface> = window
            .as_ref()
            .and_then(|w| w.toplevel())
            .map(|t| t.wl_surface().clone());
        match &window {
            Some(window) => self.space.raise_element(window, true),
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
    }

    /// Fenêtre du dessus, s'il y en a une.
    pub fn top_window(&self) -> Option<Window> {
        self.space.elements().next_back().cloned()
    }

    /// Recherche de ce qui est sous `pos`, de la fenêtre du dessus vers celle du dessous :
    /// la barre de titre d'une fenêtre masque le contenu des fenêtres placées dessous.
    pub fn hit_test(&self, pos: Point<f64, Logical>) -> Option<Hit> {
        for window in self.space.elements().rev() {
            let Some(content) = self.space.element_geometry(window) else {
                continue;
            };
            if is_server_decorated(window) {
                if let Some(part) = decoration::hit(content, pos) {
                    return Some(Hit::Decoration(window.clone(), part));
                }
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
        }
        None
    }

    /// Commence le déplacement de `window` à la souris (barre de titre ou demande du client).
    pub fn start_move(&mut self, window: Window, start_data: GrabStartData<Ferrite>, serial: Serial) {
        let Some(initial_window_location) = self.space.element_location(&window) else {
            return;
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
            min_y: MENUBAR_HEIGHT as i32 + titlebar,
        };
        pointer.set_grab(self, grab, serial, Focus::Clear);
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
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        toplevel.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Resizing);
        });
        toplevel.send_pending_configure();
        let size = window.geometry().size;
        let grab = ResizeSurfaceGrab::start(start_data, window, edges, Rectangle::new(location, size));
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    /// Agrandit la fenêtre, ou lui rend sa taille si elle l'est déjà.
    pub fn toggle_maximize(&mut self, window: &Window) {
        let Some(toplevel) = window.toplevel().cloned() else {
            return;
        };
        if toplevel
            .current_state()
            .states
            .contains(xdg_toplevel::State::Maximized)
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
        self.space.unmap_elem(window);
        self.minimized.push(Minimized {
            window: window.clone(),
            location,
        });
        let next = self.top_window();
        self.focus_window(next);
    }

    /// Remet à l'écran la dernière fenêtre réduite. En attendant le Dock (phase 3), c'est
    /// le raccourci Ctrl+Alt+M.
    pub fn restore_minimized(&mut self) {
        if let Some(Minimized { window, location }) = self.minimized.pop() {
            self.space.map_element(window.clone(), location, true);
            self.focus_window(Some(window));
        }
    }

    /// Action d'un feu tricolore cliqué.
    pub fn activate_light(&mut self, window: &Window, light: TrafficLight) {
        match light {
            TrafficLight::Close => {
                if let Some(toplevel) = window.toplevel() {
                    toplevel.send_close();
                }
            }
            TrafficLight::Minimize => self.minimize(window),
            TrafficLight::Zoom => self.toggle_maximize(window),
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
        assert_eq!(usable_area(rect(0, 0, 1440, 900)), rect(0, 25, 1440, 875));
    }

    #[test]
    fn agrandissement_sous_la_barre_de_titre() {
        let area = rect(0, 25, 1440, 875);
        assert_eq!(maximized_content(area, true), rect(0, 55, 1440, 845));
        assert_eq!(maximized_content(area, false), area);
    }

    #[test]
    fn cascade() {
        let area = rect(0, 25, 1440, 875);
        let mut placement = Placement::default();
        let size = Size::from((800, 600));
        assert_eq!(placement.next_position(area, size), Point::from((40, 65)));
        assert_eq!(placement.next_position(area, size), Point::from((70, 95)));
        // Zone trop petite : toutes les fenêtres au même endroit.
        let mut placement = Placement::default();
        let big = Size::from((1400, 900));
        assert_eq!(placement.next_position(area, big), Point::from((40, 65)));
        assert_eq!(placement.next_position(area, big), Point::from((40, 65)));
    }

    #[test]
    fn la_cascade_reste_dans_l_ecran() {
        let area = rect(0, 25, 1000, 700);
        let size = Size::from((600, 400));
        let mut placement = Placement::default();
        for _ in 0..50 {
            let pos = placement.next_position(area, size);
            assert!(pos.x + size.w <= area.loc.x + area.size.w - CASCADE_MARGIN);
            assert!(pos.y + size.h <= area.loc.y + area.size.h - CASCADE_MARGIN);
        }
    }
}
