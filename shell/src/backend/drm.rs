//! Backend écran (DRM/KMS) : ferrite-shell prend directement l'écran, le clavier, la souris
//! et le pavé tactile, depuis une session sur un terminal virtuel (tty1 de l'image live).
//!
//! - Session : libseat (logind), qui donne accès aux périphériques sans être root et
//!   gère le changement de terminal virtuel.
//! - Affichage : la carte graphique principale (udev), un seul écran (l'écran interne eDP
//!   en priorité), son mode préféré, rendu GLES dans des tampons GBM présentés par
//!   `DrmCompositor` au rythme du VBlank.
//! - Entrées : libinput.
//!
//! Plusieurs écrans et le branchement à chaud viendront ensuite.

use std::{cell::RefCell, error::Error, rc::Rc, time::Duration};

use smithay::{
    backend::{
        allocator::{
            Fourcc,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent,
            compositor::{DrmCompositor, FrameFlags},
            exporter::gbm::GbmFramebufferExporter,
        },
        egl::{EGLContext, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{ImportDma, ImportMemWl, gles::GlesRenderer},
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{all_gpus, primary_gpu},
    },
    output::{Mode, Output, PhysicalProperties, Scale},
    reexports::{
        calloop::{
            EventLoop, LoopHandle, RegistrationToken,
            ping::make_ping,
            timer::{TimeoutAction, Timer},
        },
        drm::control::{Device as _, ModeTypeFlags, connector, crtc},
        input::{self as libinput, Libinput},
        rustix::fs::OFlags,
    },
    utils::{DeviceFd, Transform},
};
use tracing::{error, info, warn};

use crate::{
    options::Options,
    render::{self, RenderAssets},
    state::Ferrite,
};

type Compositor =
    DrmCompositor<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

/// Délai entre deux essais de rendu quand rien n'a changé à l'écran (environ 60 Hz).
const IDLE_REDRAW: Duration = Duration::from_millis(16);
/// Après une demi-seconde sans changement, les essais au repos s'espacent : commits des
/// clients, entrées et événements du bureau déclenchent de toute façon un rendu immédiat.
const IDLE_STREAK: u32 = 30;
const IDLE_SLOW: Duration = Duration::from_millis(100);
/// Formats des tampons d'affichage, par ordre de préférence.
const COLOR_FORMATS: [Fourcc; 2] = [Fourcc::Argb8888, Fourcc::Abgr8888];

struct Drm {
    device: DrmDevice,
    renderer: GlesRenderer,
    compositor: Compositor,
    output: Output,
    assets: RenderAssets,
    /// La session a la main sur l'écran (faux pendant qu'un autre terminal est affiché).
    active: bool,
    /// Une image a été envoyée et attend d'être affichée (VBlank).
    frame_pending: bool,
    /// Rendu déjà programmé, et s'il s'agit d'un essai au repos (`IDLE_REDRAW`), qu'un
    /// changement à l'écran peut avancer.
    render_scheduled: Option<(RegistrationToken, bool)>,
    /// Contrôleur de l'écran, taille de sa table de gamma et gains déjà appliqués (mode nuit).
    crtc: crtc::Handle,
    gamma_size: usize,
    gamma_applied: Option<(f64, f64, f64)>,
    /// Essais au repos de suite sans rien à redessiner : au-delà de [`IDLE_STREAK`], ils
    /// s'espacent à [`IDLE_SLOW`].
    idle_streak: u32,
}

pub fn init(
    event_loop: &mut EventLoop<'static, Ferrite>,
    state: &mut Ferrite,
    options: &Options,
) -> Result<(), Box<dyn Error>> {
    if options.capture.is_some() {
        warn!("--capture n'est disponible qu'en fenêtre (--winit) ; ignoré");
    }

    let (mut session, session_notifier) =
        LibSeatSession::new().map_err(|err| format!("ouverture de la session (libseat) : {err}"))?;
    let seat = session.seat();
    info!(seat, "session ouverte");
    // Bouton d'alimentation physique : géré par le bureau tant qu'il tourne. Le verrou est
    // rangé dans l'état : cette fonction se termine avant la boucle du bureau.
    state.power_key_inhibitor = crate::desktop::lock::take_power_key();

    // Carte graphique principale.
    let path = primary_gpu(&seat)?
        .or_else(|| all_gpus(&seat).ok()?.into_iter().next())
        .ok_or("aucune carte graphique trouvée")?;
    info!(?path, "carte graphique");
    let fd = session
        .open(
            &path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .map_err(|err| format!("ouverture de {} : {err}", path.display()))?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (mut device, drm_notifier) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd)?;

    // SAFETY : `gbm` reste valide aussi longtemps que l'affichage EGL qui l'utilise (le
    // renderer le garde vivant à travers son contexte).
    #[allow(unsafe_code)]
    let display = unsafe { EGLDisplay::new(gbm.clone())? };
    let context = EGLContext::new(&display)?;
    // SAFETY : le contexte EGL vient d'être créé et n'est utilisé que par ce renderer.
    #[allow(unsafe_code)]
    let mut renderer = unsafe { GlesRenderer::new(context)? };

    // Écran.
    let (connector, crtc, drm_mode) = pick_connector(&device)?;
    let name = format!("{}-{}", connector.interface().as_str(), connector.interface_id());
    let (width_mm, height_mm) = connector.size().unwrap_or((0, 0));
    let (width_px, height_px) = drm_mode.size();
    let scale = options
        .scale
        .unwrap_or_else(|| super::auto_scale(width_px as u32, width_mm));
    info!(
        name,
        mode = format!("{width_px}×{height_px} à {} Hz", drm_mode.vrefresh()),
        taille_mm = format!("{width_mm}×{height_mm}"),
        scale,
        "écran"
    );

    let output = Output::new(
        name,
        PhysicalProperties {
            size: (width_mm as i32, height_mm as i32).into(),
            subpixel: connector.subpixel().into(),
            make: "Ferrite".into(),
            model: "Écran".into(),
        },
    );
    let mode = Mode::from(drm_mode);
    output.change_current_state(
        Some(mode),
        Some(Transform::Normal),
        Some(Scale::Fractional(scale)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    let _global = output.create_global::<Ferrite>(&state.display_handle);
    state.space.map_output(&output, (0, 0));

    let surface = device.create_surface(crtc, drm_mode, &[connector.handle()])?;
    let allocator = GbmAllocator::new(gbm.clone(), GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT);
    let exporter = GbmFramebufferExporter::new(gbm.clone(), None);
    let render_formats = renderer.egl_context().dmabuf_render_formats().clone();
    let compositor = DrmCompositor::new(
        &output,
        surface,
        None,
        allocator,
        exporter,
        COLOR_FORMATS,
        render_formats,
        device.cursor_size(),
        Some(gbm),
    )
    .map_err(|err| format!("préparation de l'affichage : {err:?}"))?;

    state.shm_state.update_formats(renderer.shm_formats());
    let dmabuf_formats = renderer.dmabuf_formats().into_iter().collect::<Vec<_>>();
    state.publish_dmabuf(&display, dmabuf_formats);
    let assets = RenderAssets::new(&mut renderer)?;

    let gamma_size = device
        .get_crtc(crtc)
        .map_or(0, |info| info.gamma_length() as usize);
    let drm = Rc::new(RefCell::new(Drm {
        crtc,
        gamma_size,
        gamma_applied: None,
        device,
        renderer,
        compositor,
        output,
        assets,
        active: true,
        frame_pending: false,
        render_scheduled: None,
        idle_streak: 0,
    }));
    let handle = event_loop.handle();

    // Fin d'affichage d'une image : on prépare la suivante.
    {
        let drm = drm.clone();
        let loop_handle = handle.clone();
        handle.insert_source(drm_notifier, move |event, _, _state| match event {
            DrmEvent::VBlank(_) => {
                {
                    let mut drm = drm.borrow_mut();
                    drm.frame_pending = false;
                    if let Err(err) = drm.compositor.frame_submitted() {
                        warn!("image non confirmée : {err:?}");
                    }
                }
                schedule_render(&drm, &loop_handle, Duration::ZERO);
            }
            DrmEvent::Error(err) => error!("erreur DRM : {err:?}"),
        })?;
    }

    // Commit d'un client, animation qui démarre : on dessine sans attendre l'essai au
    // repos, qui tomberait n'importe où entre deux VBlank et ferait sauter des images.
    {
        let (ping, source) = make_ping()?;
        let drm = drm.clone();
        let loop_handle = handle.clone();
        handle.insert_source(source, move |(), _, _state| {
            schedule_render(&drm, &loop_handle, Duration::ZERO);
        })?;
        state.redraw = Some(ping);
    }

    // Entrées.
    let mut libinput_context =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput_context
        .udev_assign_seat(&seat)
        .map_err(|()| format!("libinput : siège « {seat} » introuvable"))?;
    handle.insert_source(LibinputInputBackend::new(libinput_context.clone()), {
        let mut keyboards = Vec::new();
        move |mut event, _, state| {
            let mut update_leds = false;
            if let InputEvent::DeviceAdded { device } = &mut event {
                configure_input_device(device);
                if device.has_capability(libinput::DeviceCapability::Keyboard) {
                    keyboards.push(device.clone());
                    update_leds = true;
                }
            }
            if let InputEvent::DeviceRemoved { device } = &event {
                keyboards.retain(|keyboard| keyboard != device);
            }
            if matches!(event, InputEvent::Keyboard { .. }) {
                update_leds = true;
            }
            // Gestes : les déplacements bruts, pour que la page suive les doigts de façon
            // régulière (l'accélération de libinput dépend de la vitesse).
            if let InputEvent::GestureSwipeUpdate { event } = &event {
                use smithay::reexports::input::event::gesture::GestureEventCoordinates;
                state.swipe_update(event.dx_unaccelerated(), event.dy_unaccelerated());
                state.request_redraw();
                return;
            }
            state.process_input_event(event);
            if update_leds && let Some(keyboard) = state.seat.get_keyboard() {
                let leds = libinput::Led::from(keyboard.led_state());
                for device in &mut keyboards {
                    device.led_update(leds);
                }
            }
            state.request_redraw();
        }
    })?;

    // Changement de terminal virtuel : la session se met en pause puis reprend.
    {
        let drm = drm.clone();
        let loop_handle = handle.clone();
        handle.insert_source(session_notifier, move |event, _, _state| match event {
            SessionEvent::PauseSession => {
                info!("session en pause (autre terminal)");
                libinput_context.suspend();
                let mut drm = drm.borrow_mut();
                drm.active = false;
                drm.device.pause();
            }
            SessionEvent::ActivateSession => {
                info!("session reprise");
                if let Err(err) = libinput_context.resume() {
                    error!("reprise de libinput impossible : {err:?}");
                }
                {
                    let mut drm = drm.borrow_mut();
                    if let Err(err) = drm.device.activate(false) {
                        error!("reprise de l'écran impossible : {err}");
                    }
                    if let Err(err) = drm.compositor.reset_state() {
                        warn!("réinitialisation de l'affichage : {err}");
                    }
                    drm.active = true;
                    drm.frame_pending = false;
                    // Un autre terminal a pu changer la table de gamma : on la réapplique.
                    drm.gamma_applied = None;
                }
                schedule_render(&drm, &loop_handle, Duration::ZERO);
            }
        })?;
    }

    state.change_vt = Some(Box::new(move |vt| {
        if let Err(err) = session.change_vt(vt) {
            warn!("passage au terminal {vt} impossible : {err}");
        }
    }));

    schedule_render(&drm, &handle, Duration::ZERO);
    Ok(())
}

/// Premier écran branché (l'écran interne d'abord), son mode préféré et un CRTC libre.
fn pick_connector(
    device: &DrmDevice,
) -> Result<
    (
        connector::Info,
        crtc::Handle,
        smithay::reexports::drm::control::Mode,
    ),
    String,
> {
    let resources = device
        .resource_handles()
        .map_err(|err| format!("ressources DRM : {err}"))?;
    let connected: Vec<connector::Info> = resources
        .connectors()
        .iter()
        .filter_map(|handle| device.get_connector(*handle, true).ok())
        .filter(|info| info.state() == connector::State::Connected && !info.modes().is_empty())
        .collect();
    let connector = connected
        .iter()
        .find(|info| info.interface() == connector::Interface::EmbeddedDisplayPort)
        .or_else(|| connected.first())
        .cloned()
        .ok_or("aucun écran branché")?;
    let mode = connector
        .modes()
        .iter()
        .find(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or_else(|| connector.modes().first())
        .copied()
        .ok_or("écran sans mode d'affichage")?;
    let crtc = connector
        .encoders()
        .iter()
        .filter_map(|handle| device.get_encoder(*handle).ok())
        .flat_map(|encoder| resources.filter_crtcs(encoder.possible_crtcs()))
        .next()
        .ok_or("aucun contrôleur d'affichage (CRTC) pour cet écran")?;
    Ok((connector, crtc, mode))
}

/// Réglages des périphériques d'entrée : pavé tactile avec clic par tape et défilement
/// naturel (comme sur macOS).
fn configure_input_device(device: &mut libinput::Device) {
    if device.config_tap_finger_count() > 0 {
        let _ = device.config_tap_set_enabled(true);
    }
    if device.config_scroll_has_natural_scroll() {
        let _ = device.config_scroll_set_natural_scroll_enabled(true);
    }
    info!(nom = device.name(), "périphérique d'entrée");
}

/// Programme un rendu dans `delay` (sauf s'il y en a déjà un de prévu ; un essai au repos
/// est avancé par un rendu immédiat).
fn schedule_render(drm: &Rc<RefCell<Drm>>, handle: &LoopHandle<'static, Ferrite>, delay: Duration) {
    let mut guard = drm.borrow_mut();
    match guard.render_scheduled.take() {
        Some((token, true)) if delay.is_zero() => handle.remove(token),
        Some(scheduled) => {
            guard.render_scheduled = Some(scheduled);
            return;
        }
        None => {}
    }
    let drm_for_timer = drm.clone();
    let loop_handle = handle.clone();
    let timer = Timer::from_duration(delay);
    let inserted = handle.insert_source(timer, move |_, _, state| {
        let drm = &drm_for_timer;
        let idle = {
            let mut drm = drm.borrow_mut();
            drm.render_scheduled = None;
            let idle = drm.active && !drm.frame_pending && !render(&mut drm, state);
            drm.idle_streak = if idle {
                drm.idle_streak.saturating_add(1)
            } else {
                0
            };
            idle.then_some(drm.idle_streak)
        };
        // Rien n'a changé : on réessaie un peu plus tard (de moins en moins souvent). Sinon
        // le VBlank relancera.
        if let Some(streak) = idle {
            let delay = if streak > IDLE_STREAK {
                IDLE_SLOW
            } else {
                IDLE_REDRAW
            };
            schedule_render(drm, &loop_handle, delay);
        }
        TimeoutAction::Drop
    });
    match inserted {
        Ok(token) => guard.render_scheduled = Some((token, !delay.is_zero())),
        Err(err) => error!("programmation du rendu impossible : {err}"),
    }
}

/// Mode nuit : table de gamma de l'écran multipliée par les gains (rouge, vert, bleu).
fn apply_night(drm: &mut Drm, gains: (f64, f64, f64)) {
    // Gains pas encore calculés (premier tour) : écran neutre.
    let gains = if gains == (0.0, 0.0, 0.0) {
        (1.0, 1.0, 1.0)
    } else {
        gains
    };
    if drm.gamma_size == 0 || drm.gamma_applied == Some(gains) {
        return;
    }
    let ramp = |gain| crate::desktop::night::gamma_ramp(drm.gamma_size, gain);
    let (red, green, blue) = (ramp(gains.0), ramp(gains.1), ramp(gains.2));
    match drm.device.set_gamma(drm.crtc, &red, &green, &blue) {
        Ok(()) => drm.gamma_applied = Some(gains),
        Err(err) => {
            warn!("mode nuit : table de gamma refusée ({err})");
            // Inutile de réessayer à chaque image.
            drm.gamma_size = 0;
        }
    }
}

/// Dessine l'écran ; renvoie vrai si une nouvelle image a été envoyée.
fn render(drm: &mut Drm, state: &mut Ferrite) -> bool {
    apply_night(drm, state.night.gains);
    let Drm {
        renderer,
        compositor,
        output,
        assets,
        frame_pending,
        ..
    } = drm;
    state.run_genie_demo();
    render::take_screenshot(renderer, state, assets, output);
    render::serve_screencopy(renderer, state, assets, output);
    let elements = render::output_elements(renderer, state, assets, output, true);
    let sent = match compositor.render_frame(renderer, &elements, [0.0, 0.0, 0.0, 1.0], FrameFlags::DEFAULT) {
        Ok(result) if !result.is_empty => match compositor.queue_frame(()) {
            Ok(()) => {
                *frame_pending = true;
                render::profile::sent();
                true
            }
            Err(err) => {
                warn!("envoi de l'image impossible : {err:?}");
                false
            }
        },
        Ok(_) => false,
        Err(err) => {
            warn!("échec du rendu : {err:?}");
            false
        }
    };

    let elapsed = state.start_time.elapsed();
    for window in state.space.elements() {
        window.send_frame(output, elapsed, Some(Duration::ZERO), |_, _| Some(output.clone()));
    }
    sent
}
