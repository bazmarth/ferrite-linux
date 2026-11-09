//! `ext_session_lock_v1` : un verrouilleur externe (swaylock, hyprlock…) remplace l'écran
//! de verrouillage de Ferrite. Tant que la session est verrouillée, seules ses surfaces
//! sont dessinées (sur fond noir) et reçoivent le clavier et le pointeur ; les raccourcis du
//! bureau ne répondent plus.

use smithay::{
    delegate_session_lock,
    output::Output,
    reexports::wayland_server::protocol::wl_output::WlOutput,
    utils::{Logical, Point},
    wayland::session_lock::{LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker},
};

use crate::state::Ferrite;

/// Session verrouillée par un client : surfaces de verrouillage, une par écran.
#[derive(Debug, Default)]
pub struct ClientLock {
    pub surfaces: Vec<(Output, LockSurface)>,
}

impl SessionLockHandler for Ferrite {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock_state
    }

    /// Plus rien du bureau n'est montré dès l'image suivante : le verrou est confirmé.
    fn lock(&mut self, confirmation: SessionLocker) {
        self.popover = None;
        self.session_lock = Some(ClientLock::default());
        confirmation.lock();
        self.request_redraw();
    }

    fn unlock(&mut self) {
        self.session_lock = None;
        let top = self.top_window();
        self.focus_window(top);
        self.request_redraw();
    }

    fn new_surface(&mut self, surface: LockSurface, output: WlOutput) {
        let Some(output) = Output::from_resource(&output) else {
            return;
        };
        let Some(lock) = self.session_lock.as_mut() else {
            return;
        };
        if let Some(geometry) = self.space.output_geometry(&output) {
            surface.with_pending_state(|state| {
                state.size = Some((geometry.size.w as u32, geometry.size.h as u32).into());
            });
        }
        surface.send_configure();
        let wl_surface = surface.wl_surface().clone();
        lock.surfaces.push((output, surface));
        self.set_keyboard_focus(Some(wl_surface));
        self.request_redraw();
    }
}

delegate_session_lock!(Ferrite);

impl Ferrite {
    /// Surface de verrouillage sous `pos`, avec la position de son origine.
    pub fn lock_surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(
        smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        Point<f64, Logical>,
    )> {
        let lock = self.session_lock.as_ref()?;
        lock.surfaces.iter().find_map(|(output, surface)| {
            let geometry = self.space.output_geometry(output)?;
            geometry
                .to_f64()
                .contains(pos)
                .then(|| (surface.wl_surface().clone(), geometry.loc.to_f64()))
        })
    }
}
