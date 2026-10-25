//! Gestion des fenêtres : placement, focus, déplacement et redimensionnement.

mod move_grab;
mod resize_grab;

pub use move_grab::MoveSurfaceGrab;
pub use resize_grab::{ResizeSurfaceGrab, handle_commit as handle_resize_commit};

use ferrite_ui::theme::MENUBAR_HEIGHT;
use smithay::{
    desktop::Window,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Size},
};

use crate::state::Ferrite;

/// Décalage entre deux fenêtres ouvertes à la suite.
const CASCADE_STEP: i32 = 30;
/// Marge entre la première fenêtre et le coin de la zone utile.
const CASCADE_MARGIN: i32 = 40;

/// Placement en cascade des nouvelles fenêtres, comme sur macOS : chaque fenêtre est
/// décalée vers le bas et la droite ; on repart du coin quand elle sortirait de l'écran.
#[derive(Debug, Default)]
pub struct Placement {
    next: u32,
}

impl Placement {
    /// Position de la prochaine fenêtre dans la zone `area` (l'écran sous la barre de menus).
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
