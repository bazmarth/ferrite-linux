//! Pinch gesture of the touchpad (`zwp_pointer_gestures_v1`).
//!
//! Added for Ferrite OS: winit 0.30 only reports `WindowEvent::PinchGesture` on macOS and
//! iOS. The absolute scale sent by the compositor becomes a logarithmic delta, like the
//! magnification reported on macOS, so `delta.exp()` is the zoom factor of the step.

use std::sync::Mutex;

use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_pointer::WlPointer;
use sctk::reexports::client::{delegate_dispatch, Connection, Dispatch, QueueHandle};
use sctk::reexports::protocols::wp::pointer_gestures::zv1::client::{
    zwp_pointer_gesture_pinch_v1::{self, ZwpPointerGesturePinchV1},
    zwp_pointer_gestures_v1::ZwpPointerGesturesV1,
};

use sctk::globals::GlobalData;

use crate::event::{TouchPhase, WindowEvent};
use crate::platform_impl::wayland::state::WinitState;
use crate::platform_impl::wayland::{self, DeviceId, WindowId};

/// Wrapper around the pointer gestures manager.
pub struct PointerGesturesState {
    manager: ZwpPointerGesturesV1,
}

impl PointerGesturesState {
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    pub fn get_pinch_gesture(
        &self,
        pointer: &WlPointer,
        queue_handle: &QueueHandle<WinitState>,
    ) -> ZwpPointerGesturePinchV1 {
        self.manager.get_pinch_gesture(pointer, queue_handle, PinchData::default())
    }
}

/// Window under the fingers and last absolute scale of the current pinch.
#[derive(Debug, Default)]
pub struct PinchData {
    inner: Mutex<Option<(WindowId, f64)>>,
}

/// Logarithmic delta between two absolute scales (0 when one of them is unusable).
pub fn pinch_delta(previous: f64, scale: f64) -> f64 {
    if previous > 0.0 && scale > 0.0 && previous.is_finite() && scale.is_finite() {
        (scale / previous).ln()
    } else {
        0.0
    }
}

impl Dispatch<ZwpPointerGesturesV1, GlobalData, WinitState> for PointerGesturesState {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZwpPointerGesturesV1,
        _event: <ZwpPointerGesturesV1 as wayland_client::Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
    }
}

impl Dispatch<ZwpPointerGesturePinchV1, PinchData, WinitState> for PointerGesturesState {
    fn event(
        state: &mut WinitState,
        _proxy: &ZwpPointerGesturePinchV1,
        event: <ZwpPointerGesturePinchV1 as wayland_client::Proxy>::Event,
        data: &PinchData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        let mut current = data.inner.lock().unwrap();
        let (window_id, delta, phase) = match event {
            zwp_pinch_event::Begin { surface, .. } => {
                let window_id = wayland::make_wid(&surface);
                *current = Some((window_id, 1.0));
                (window_id, 0.0, TouchPhase::Started)
            },
            zwp_pinch_event::Update { scale, .. } => {
                let Some((window_id, previous)) = current.as_mut() else {
                    return;
                };
                let delta = pinch_delta(*previous, scale);
                if scale > 0.0 && scale.is_finite() {
                    *previous = scale;
                }
                (*window_id, delta, TouchPhase::Moved)
            },
            zwp_pinch_event::End { cancelled, .. } => {
                let Some((window_id, _)) = current.take() else {
                    return;
                };
                let phase = if cancelled != 0 { TouchPhase::Cancelled } else { TouchPhase::Ended };
                (window_id, 0.0, phase)
            },
            _ => return,
        };
        drop(current);
        let device_id = crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(DeviceId));
        state
            .events_sink
            .push_window_event(WindowEvent::PinchGesture { device_id, delta, phase }, window_id);
    }
}

use zwp_pointer_gesture_pinch_v1::Event as zwp_pinch_event;

delegate_dispatch!(WinitState: [ZwpPointerGesturesV1: GlobalData] => PointerGesturesState);
delegate_dispatch!(WinitState: [ZwpPointerGesturePinchV1: PinchData] => PointerGesturesState);

