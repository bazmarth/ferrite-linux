//! Saisie de texte et forme du curseur :
//!
//! - `zwp_text_input_v3` et `zwp_input_method_v2` : méthodes de saisie (claviers virtuels,
//!   saisie du chinois ou du japonais avec fcitx5 ou IBus) ; leurs fenêtres de candidats
//!   sont posées sous le texte en cours ;
//! - `wp_cursor_shape_v1` : les applications demandent une forme de curseur par son nom
//!   (texte, main…) au lieu de dessiner leur propre image.

use smithay::{
    delegate_cursor_shape, delegate_input_method_manager, delegate_text_input_manager,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Rectangle},
    wayland::{
        input_method::{InputMethodHandler, PopupSurface},
        tablet_manager::TabletSeatHandler,
    },
};

use crate::state::Ferrite;

impl InputMethodHandler for Ferrite {
    fn new_popup(&mut self, surface: PopupSurface) {
        self.input_popups.push(surface);
    }

    fn dismiss_popup(&mut self, surface: PopupSurface) {
        self.input_popups
            .retain(|popup| popup.wl_surface() != surface.wl_surface());
    }

    fn popup_repositioned(&mut self, _surface: PopupSurface) {
        self.request_redraw();
    }

    /// Cadre de la fenêtre qui reçoit le texte, pour placer les candidats sous le curseur.
    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, Logical> {
        self.window_for_surface(parent)
            .and_then(|window| self.space.element_geometry(&window))
            .unwrap_or_default()
    }
}

delegate_text_input_manager!(Ferrite);
delegate_input_method_manager!(Ferrite);
// Le curseur demandé par une tablette graphique suit celui du pointeur.
impl TabletSeatHandler for Ferrite {}

delegate_cursor_shape!(Ferrite);
