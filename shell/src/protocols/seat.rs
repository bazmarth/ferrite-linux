//! `wl_seat` (clavier, pointeur) et `wl_data_device` (presse-papiers, glisser-déposer).

use smithay::{
    delegate_data_device, delegate_seat,
    input::{Seat, SeatHandler, SeatState, pointer::CursorImageStatus},
    reexports::wayland_server::{Resource, protocol::wl_surface::WlSurface},
    wayland::selection::{
        SelectionHandler,
        data_device::{
            ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            set_data_device_focus,
        },
    },
};

use crate::state::Ferrite;

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
        set_data_device_focus(dh, seat, client);
    }
}

delegate_seat!(Ferrite);

impl SelectionHandler for Ferrite {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Ferrite {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for Ferrite {}
impl ServerDndGrabHandler for Ferrite {}

delegate_data_device!(Ferrite);
