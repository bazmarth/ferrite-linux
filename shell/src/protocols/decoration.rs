//! `xdg-decoration` : qui dessine la barre de titre, le client ou ferrite-shell.
//!
//! ferrite-shell propose toujours ses propres décorations (style Ferrite). Un client qui
//! demande explicitement à dessiner les siennes (GTK, par exemple) est respecté.

use smithay::{
    delegate_xdg_decoration,
    reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode,
    wayland::shell::xdg::{ToplevelSurface, decoration::XdgDecorationHandler},
};

use crate::state::Ferrite;

impl XdgDecorationHandler for Ferrite {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        set_mode(&toplevel, Mode::ServerSide);
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: Mode) {
        set_mode(&toplevel, mode);
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        set_mode(&toplevel, Mode::ServerSide);
    }
}

delegate_xdg_decoration!(Ferrite);

fn set_mode(toplevel: &ToplevelSurface, mode: Mode) {
    toplevel.with_pending_state(|state| state.decoration_mode = Some(mode));
    // Avant la première configuration, le mode part avec elle.
    if toplevel.is_initial_configure_sent() {
        toplevel.send_pending_configure();
    }
}
