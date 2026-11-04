//! `zwlr_screencopy_manager_v1` : captures d'écran et enregistrements par les outils
//! habituels (grim, wf-recorder, OBS…). Le client demande un écran entier ou une zone ;
//! ferrite-shell annonce un tampon `XRGB8888` à sa taille en pixels, puis, quand le client
//! le fournit, y copie l'image suivante de l'écran (voir `render::serve_screencopy`).

use smithay::{
    output::Output,
    reexports::{
        wayland_protocols_wlr::screencopy::v1::server::{
            zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
            zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
            protocol::{wl_buffer::WlBuffer, wl_shm},
        },
    },
    utils::{Logical, Physical, Rectangle},
    wayland::shm::with_buffer_contents,
};

use crate::state::Ferrite;

const VERSION: u32 = 3;

/// Crée le global ; rien d'autre à garder.
pub fn init(display: &DisplayHandle) {
    display.create_global::<Ferrite, ZwlrScreencopyManagerV1, ()>(VERSION, ());
}

/// Ce qu'une demande copie : écran, zone en pixels de l'écran, curseur ou non.
#[derive(Debug, Clone)]
pub struct FrameInfo {
    pub output: Output,
    pub region: Rectangle<i32, Physical>,
    pub cursor: bool,
}

/// Copie demandée, faite à la prochaine image de son écran.
#[derive(Debug)]
pub struct PendingCopy {
    pub frame: ZwlrScreencopyFrameV1,
    pub buffer: WlBuffer,
    pub info: FrameInfo,
    /// Le client veut les zones changées (`copy_with_damage`) : tout l'écran, ici.
    pub with_damage: bool,
}

impl GlobalDispatch<ZwlrScreencopyManagerV1, ()> for Ferrite {
    fn bind(
        _state: &mut Self,
        _display: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrScreencopyManagerV1>,
        _data: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        init.init(resource, ());
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for Ferrite {
    fn request(
        state: &mut Self,
        _client: &Client,
        _manager: &ZwlrScreencopyManagerV1,
        request: zwlr_screencopy_manager_v1::Request,
        _data: &(),
        _display: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        use zwlr_screencopy_manager_v1::Request;
        let (frame, overlay_cursor, output, region) = match request {
            Request::CaptureOutput {
                frame,
                overlay_cursor,
                output,
            } => (frame, overlay_cursor, output, None),
            Request::CaptureOutputRegion {
                frame,
                overlay_cursor,
                output,
                x,
                y,
                width,
                height,
            } => (
                frame,
                overlay_cursor,
                output,
                Some(Rectangle::<i32, Logical>::new(
                    (x, y).into(),
                    (width, height).into(),
                )),
            ),
            _ => return,
        };
        let info = Output::from_resource(&output)
            .and_then(|output| frame_info(state, output, region, overlay_cursor != 0));
        let frame = init.init(frame, info.clone());
        let Some(info) = info else {
            frame.failed();
            return;
        };
        let (width, height) = (info.region.size.w as u32, info.region.size.h as u32);
        frame.buffer(wl_shm::Format::Xrgb8888, width, height, width * 4);
        if frame.version() >= 3 {
            frame.buffer_done();
        }
    }
}

/// Zone à copier en pixels de l'écran : l'écran entier, ou la zone logique demandée
/// (relative à l'écran), bornée à lui. Rien pour une zone vide ou un écran sans mode.
fn frame_info(
    state: &Ferrite,
    output: Output,
    region: Option<Rectangle<i32, Logical>>,
    cursor: bool,
) -> Option<FrameInfo> {
    let size = output.current_mode()?.size;
    state.space.output_geometry(&output)?;
    let full = Rectangle::<i32, Physical>::from_size(size);
    let region = match region {
        None => full,
        Some(region) => {
            let scale = output.current_scale().fractional_scale();
            region
                .to_f64()
                .to_physical(scale)
                .to_i32_round()
                .intersection(full)?
        }
    };
    (region.size.w > 0 && region.size.h > 0).then_some(FrameInfo {
        output,
        region,
        cursor,
    })
}

impl Dispatch<ZwlrScreencopyFrameV1, Option<FrameInfo>> for Ferrite {
    fn request(
        state: &mut Self,
        _client: &Client,
        frame: &ZwlrScreencopyFrameV1,
        request: zwlr_screencopy_frame_v1::Request,
        info: &Option<FrameInfo>,
        _display: &DisplayHandle,
        _init: &mut DataInit<'_, Self>,
    ) {
        use zwlr_screencopy_frame_v1::Request;
        let (buffer, with_damage) = match request {
            Request::Copy { buffer } => (buffer, false),
            Request::CopyWithDamage { buffer } => (buffer, true),
            _ => return,
        };
        let Some(info) = info.clone() else {
            frame.failed();
            return;
        };
        if state.screencopy.iter().any(|copy| copy.frame == *frame) {
            frame.post_error(
                zwlr_screencopy_frame_v1::Error::AlreadyUsed,
                "image déjà demandée",
            );
            return;
        }
        let (width, height) = (info.region.size.w, info.region.size.h);
        let fits = with_buffer_contents(&buffer, |_, _, data| {
            matches!(data.format, wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888)
                && data.width == width
                && data.height == height
                && data.stride >= width * 4
        })
        .unwrap_or(false);
        if !fits {
            frame.post_error(
                zwlr_screencopy_frame_v1::Error::InvalidBuffer,
                "tampon de mauvaise taille ou de mauvais format",
            );
            return;
        }
        state.screencopy.push(PendingCopy {
            frame: frame.clone(),
            buffer,
            info,
            with_damage,
        });
        state.request_redraw();
    }
}
