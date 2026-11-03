//! Backend fenêtre (winit) : ferrite-shell s'affiche comme une application dans un autre
//! système graphique. Sert au développement dans WSLg.

use std::{path::PathBuf, time::Duration};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{ExportMem, ImportDma, damage::OutputDamageTracker, gles::GlesRenderer},
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Rectangle, Transform},
};
use tracing::{error, info, warn};

use smithay::reexports::winit::window::CursorIcon;

use crate::{
    options::Options,
    render::{self, RenderAssets, capture},
    state::Ferrite,
};

/// Fréquence annoncée pour l'écran virtuel (en millihertz).
const REFRESH_MHZ: i32 = 60_000;

/// Capture demandée par `--capture` : fichier et instant (depuis le démarrage).
struct PendingCapture {
    path: PathBuf,
    at: Duration,
}

pub fn init(
    event_loop: &mut EventLoop<Ferrite>,
    state: &mut Ferrite,
    options: &Options,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut backend, winit) = winit::init::<GlesRenderer>()?;

    let scale = options.scale.unwrap_or(2.0);
    let mode = Mode {
        size: backend.window_size(),
        refresh: REFRESH_MHZ,
    };
    let output = Output::new(
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Ferrite".into(),
            model: "Fenêtre".into(),
        },
    );
    let _global = output.create_global::<Ferrite>(&state.display_handle);
    // Le tampon OpenGL a son origine en bas : l'image est retournée à l'affichage.
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        Some(Scale::Fractional(scale)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    state.space.map_output(&output, (0, 0));
    info!(size = ?mode.size, scale, "écran virtuel");

    let mut damage_tracker = OutputDamageTracker::from_output(&output);
    let dmabuf_formats = backend
        .renderer()
        .dmabuf_formats()
        .into_iter()
        .collect::<Vec<_>>();
    let egl_display = backend.renderer().egl_context().display().clone();
    state.publish_dmabuf(&egl_display, dmabuf_formats);
    let assets = RenderAssets::new(backend.renderer())?;
    let mut pending_capture = options.capture.clone().map(|path| PendingCapture {
        path,
        at: options.capture_delay,
    });

    event_loop
        .handle()
        .insert_source(winit, move |event, _, state| match event {
            WinitEvent::Resized { size, .. } => {
                output.change_current_state(
                    Some(Mode {
                        size,
                        refresh: REFRESH_MHZ,
                    }),
                    None,
                    None,
                    None,
                );
            }
            WinitEvent::Input(event) => state.process_input_event(event),
            WinitEvent::Redraw => {
                redraw(
                    &mut backend,
                    state,
                    &output,
                    &mut damage_tracker,
                    &assets,
                    &mut pending_capture,
                );
            }
            WinitEvent::CloseRequested => {
                state.request_exit();
                state.loop_signal.stop();
            }
            _ => {}
        })?;

    Ok(())
}

fn host_cursor_icon(shape: render::CursorShape) -> CursorIcon {
    match shape {
        render::CursorShape::Horizontal => CursorIcon::EwResize,
        render::CursorShape::Vertical => CursorIcon::NsResize,
        render::CursorShape::DiagonalDown => CursorIcon::NwseResize,
        render::CursorShape::DiagonalUp => CursorIcon::NeswResize,
        render::CursorShape::Arrow => CursorIcon::Default,
    }
}

fn redraw(
    backend: &mut WinitGraphicsBackend<GlesRenderer>,
    state: &mut Ferrite,
    output: &Output,
    damage_tracker: &mut OutputDamageTracker,
    assets: &RenderAssets,
    pending_capture: &mut Option<PendingCapture>,
) {
    backend
        .window()
        .set_cursor(host_cursor_icon(state.cursor_shape()));
    state.run_genie_demo();
    render::take_screenshot(backend.renderer(), state, assets, output);
    let size = backend.window_size();
    let capture_now = pending_capture
        .as_ref()
        .is_some_and(|c| state.start_time.elapsed() >= c.at);

    let damage = {
        let (renderer, mut framebuffer) = match backend.bind() {
            Ok(bound) => bound,
            Err(err) => {
                error!("impossible de dessiner : {err}");
                return;
            }
        };
        let elements = render::output_elements(renderer, state, assets, output, false);
        let result =
            damage_tracker.render_output(renderer, &mut framebuffer, 0, &elements, [0.0, 0.0, 0.0, 1.0]);
        let damage = match result {
            Ok(result) => result.damage.cloned(),
            Err(err) => {
                error!("échec du rendu : {err:?}");
                None
            }
        };

        if capture_now {
            if let Some(request) = pending_capture.take() {
                let region = Rectangle::from_size((size.w, size.h).into());
                let saved = renderer
                    .copy_framebuffer(&framebuffer, region, Fourcc::Abgr8888)
                    .map_err(|err| format!("{err:?}"))
                    .and_then(|mapping| {
                        let pixels = renderer.map_texture(&mapping).map_err(|err| format!("{err:?}"))?;
                        capture::write_png(&request.path, size.w as u32, size.h as u32, pixels, true)
                    });
                match saved {
                    Ok(()) => info!("capture enregistrée : {}", request.path.display()),
                    Err(err) => warn!("capture impossible : {err}"),
                }
                // Capture faite : on quitte sans présenter l'image (la fenêtre se ferme).
                state.request_exit();
                state.loop_signal.stop();
                return;
            }
        }
        damage
    };

    if let Err(err) = backend.submit(damage.as_deref()) {
        warn!("présentation impossible : {err}");
    }

    let elapsed = state.start_time.elapsed();
    for window in state.space.elements() {
        window.send_frame(output, elapsed, Some(Duration::ZERO), |_, _| Some(output.clone()));
    }
    state.space.refresh();
    state.popups.cleanup();
    if let Err(err) = state.display_handle.flush_clients() {
        warn!("envoi aux clients impossible : {err}");
    }
    backend.window().request_redraw();
}
