//! Protocoles d'appoint attendus par les applications courantes :
//!
//! - `wp_viewporter` : mise à l'échelle des vidéos (lecteurs, navigateurs) ;
//! - `wp_fractional_scale` : rendu net à 125 %, 150 %… ;
//! - `xdg_activation` : une application ouverte depuis une autre passe au premier plan ;
//! - `zwp_primary_selection` : coller avec le clic du milieu ;
//! - `zwp_relative_pointer` : mouvements bruts de la souris (jeux, 3D).

use std::time::Duration;

use smithay::{
    delegate_fractional_scale, delegate_primary_selection, delegate_relative_pointer, delegate_viewporter,
    delegate_xdg_activation,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    wayland::{
        compositor::with_states,
        fractional_scale::{FractionalScaleHandler, with_fractional_scale},
        selection::primary_selection::{PrimarySelectionHandler, PrimarySelectionState},
        xdg_activation::{
            XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
        },
    },
};

use crate::state::Ferrite;

/// Durée de validité d'une demande d'activation : au-delà, la fenêtre ne vole pas le focus.
const ACTIVATION_DELAY: Duration = Duration::from_secs(10);

impl FractionalScaleHandler for Ferrite {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self
            .space
            .outputs()
            .next()
            .map_or(1.0, |output| output.current_scale().fractional_scale());
        with_states(&surface, |states| {
            with_fractional_scale(states, |fractional| fractional.set_preferred_scale(scale));
        });
    }
}

impl XdgActivationHandler for Ferrite {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.activation_state
    }

    fn request_activation(
        &mut self,
        token: XdgActivationToken,
        data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        if data.timestamp.elapsed() < ACTIVATION_DELAY {
            if let Some(window) = self.window_for_surface(&surface) {
                self.focus_window(Some(window));
            }
        }
        self.activation_state.remove_token(&token);
    }
}

impl PrimarySelectionHandler for Ferrite {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary_selection_state
    }
}

delegate_viewporter!(Ferrite);
delegate_fractional_scale!(Ferrite);
delegate_xdg_activation!(Ferrite);
delegate_primary_selection!(Ferrite);
delegate_relative_pointer!(Ferrite);
