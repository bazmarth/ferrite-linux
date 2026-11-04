//! Inactivité :
//!
//! - `ext_idle_notify_v1` : les outils comme swayidle apprennent que l'utilisateur ne fait
//!   plus rien (pour éteindre l'écran ou verrouiller) ; toute entrée compte comme activité ;
//! - `zwp_idle_inhibit_v1` : une vidéo en cours ou une présentation empêche cette
//!   inactivité, tant que sa surface est affichée.

use smithay::{
    delegate_idle_inhibit, delegate_idle_notify,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    wayland::{
        idle_inhibit::IdleInhibitHandler,
        idle_notify::{IdleNotifierHandler, IdleNotifierState},
    },
};

use crate::state::Ferrite;

impl IdleNotifierHandler for Ferrite {
    fn idle_notifier_state(&mut self) -> &mut IdleNotifierState<Self> {
        &mut self.idle_notifier_state
    }
}

impl IdleInhibitHandler for Ferrite {
    fn inhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.push(surface);
        self.idle_notifier_state.set_is_inhibited(true);
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.retain(|known| *known != surface);
        let inhibited = !self.idle_inhibitors.is_empty();
        self.idle_notifier_state.set_is_inhibited(inhibited);
    }
}

delegate_idle_notify!(Ferrite);
delegate_idle_inhibit!(Ferrite);
