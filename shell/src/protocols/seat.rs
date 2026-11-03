//! `wl_seat` (clavier, pointeur) et `wl_data_device` (presse-papiers, glisser-déposer).

use smithay::{
    delegate_data_device, delegate_seat,
    input::{Seat, SeatHandler, SeatState, pointer::CursorImageStatus},
    reexports::wayland_server::{Resource, protocol::wl_surface::WlSurface},
    wayland::selection::{
        SelectionHandler, SelectionSource, SelectionTarget,
        data_device::{ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, set_data_device_focus},
        primary_selection::set_primary_focus,
    },
};
use std::os::fd::OwnedFd;
use tracing::warn;

use crate::{
    clipboard::{self, Owner},
    state::Ferrite,
};

impl SeatHandler for Ferrite {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Ferrite> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        // Le presse-papiers suit le client qui a le clavier.
        let dh = &self.display_handle;
        let client = focused.and_then(|surface| dh.get_client(surface.id()).ok());
        set_data_device_focus(dh, seat, client.clone());
        set_primary_focus(dh, seat, client);
    }
}

delegate_seat!(Ferrite);

impl SelectionHandler for Ferrite {
    type SelectionUserData = Owner;

    /// Une application Wayland copie : les applications X11 la voient, et le contenu
    /// rejoint l'historique (Super+V).
    fn new_selection(&mut self, ty: SelectionTarget, source: Option<SelectionSource>, _seat: Seat<Self>) {
        let mime_types = source.map(|source| source.mime_types());
        if let Some(xwm) = self.xwm.as_mut()
            && let Err(err) = xwm.new_selection(ty, mime_types.clone())
        {
            warn!(?err, "sélection non transmise à X11");
        }
        // Smithay prévient avant d'enregistrer la nouvelle sélection : on la lit au tour
        // suivant de la boucle, quand elle est en place.
        if ty == SelectionTarget::Clipboard
            && let Some(mime_types) = mime_types
        {
            self.loop_handle
                .insert_idle(move |state| state.capture_wayland_clipboard(&mime_types));
        }
    }

    /// Une application colle une sélection posée par le compositeur : élément de
    /// l'historique, ou contenu d'une application X11.
    fn send_selection(
        &mut self,
        ty: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        owner: &Owner,
    ) {
        match owner {
            Owner::History(content) => clipboard::write_selection(fd, content.bytes()),
            Owner::X11 => {
                let handle = self.loop_handle.clone();
                if let Some(xwm) = self.xwm.as_mut()
                    && let Err(err) = xwm.send_selection(ty, mime_type, fd, handle)
                {
                    warn!(?err, "sélection X11 non lue");
                }
            }
        }
    }
}

impl DataDeviceHandler for Ferrite {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for Ferrite {}

delegate_data_device!(Ferrite);
