//! ferrite-shell : compositeur Wayland et bureau de Ferrite.
//!
//! Phase 2 du plan (`docs/PLAN.md`). Deux modes : en fenêtre (backend winit, pour WSLg)
//! ou directement sur l'écran (backend DRM/KMS + libinput). Protocoles de base (compositor, shm, seat, output, xdg-shell, data-device),
//! placement centré, déplacement et redimensionnement, fond d'écran Ferrite.
//! La structure suit d'abord l'exemple `smallvil` de Smithay 0.7.

mod backend;
mod clipboard;
mod desktop;
mod input;
mod options;
mod protocols;
mod render;
mod session;
mod state;
mod system;
mod wm;

use std::process::{Command, ExitCode};

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};
use tracing::{error, info, warn};

use crate::{
    options::{BackendChoice, Options, Parsed},
    state::Ferrite,
};

fn main() -> ExitCode {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let options = match options::parse(std::env::args().skip(1)) {
        Ok(Parsed::Run(options)) => options,
        Ok(Parsed::Help) => {
            println!("{}", options::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("ferrite-shell : {message}\n\n{}", options::USAGE);
            return ExitCode::from(2);
        }
    };

    match run(options) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            error!("{err}");
            ExitCode::FAILURE
        }
    }
}

/// Renvoie le code de sortie choisi par l'utilisateur (voir `PowerAction::exit_code`).
fn run(options: Options) -> Result<u8, Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<'static, Ferrite> = EventLoop::try_new()?;
    let display: Display<Ferrite> = Display::new()?;
    let has_display = std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some();
    let mut state = Ferrite::new(&mut event_loop, display)?;
    state.prepare_session_restore(
        !has_display && std::env::var_os("FERRITE_SESSION").is_some_and(|value| value == "1"),
    );
    state.popover = options.menu;
    if let Some(position) = options.demo_page {
        state.workspaces.freeze(position);
    }
    if options.demo_overview {
        state.open_overview();
    }
    state.demo_switcher = options.demo_switcher;
    state.demo_screenshot = options.demo_screenshot;
    // Après la création de l'écran : la zone proposée en dépend.
    state.screenshot_video = options.demo_video;
    if options.demo_island {
        state.island_open_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(3600));
    }
    if let Some(delay) = options.demo_island_press {
        let timer = smithay::reexports::calloop::timer::Timer::from_duration(
            std::time::Duration::from_secs_f64(delay),
        );
        let pressed = event_loop.handle().insert_source(timer, |_, _, state| {
            state.island_press(desktop::panels::IslandHit::Button(1));
            smithay::reexports::calloop::timer::TimeoutAction::Drop
        });
        if let Err(err) = pressed {
            warn!("appui de démonstration sur l'îlot non prévu : {err}");
        }
    }
    let demo_capture = if options.demo_region || options.demo_video {
        Some(render::capture::Mode::Area)
    } else if options.demo_capture {
        Some(render::capture::Mode::Screen)
    } else {
        None
    };
    if let Some(mode) = demo_capture {
        event_loop
            .handle()
            .insert_idle(move |state| state.open_capture(mode));
    }
    state.genie_demo = options.genie_demo.map(|fraction| (fraction, None));
    state.swap_demo = options.swap_demo.map(|fraction| (fraction, None, false));
    match system::start(&event_loop.handle()) {
        Ok(link) => state.system_link = Some(link),
        Err(err) => warn!("services système indisponibles : {err}"),
    }
    match clipboard::start(&event_loop.handle()) {
        Ok(sender) => state.clipboard_sender = Some(sender),
        Err(err) => warn!("historique du presse-papiers indisponible : {err}"),
    }
    match desktop::notifications::start(&event_loop.handle()) {
        Ok(link) => state.notification_link = Some(link),
        Err(err) => warn!("notifications indisponibles : {err}"),
    }
    match desktop::lock::start(&event_loop.handle()) {
        Ok((events, media)) => {
            state.lock_events = Some(events);
            state.media_link = Some(media);
            // Verrouillé avant que le lien existe (`--demo-lock`), ou widget Musique posé.
            state.refresh_media_watching();
        }
        Err(err) => warn!("verrouillage indisponible : {err}"),
    }
    if let Some(page) = options.demo_controls {
        state.popover = Some(desktop::panels::Popover::Controls);
        state.controls.page = page;
    }
    if options.demo_auth {
        state.auth.push_back(desktop::polkit::Auth::demo());
    }
    if options.demo_power {
        state.open_power_dialog(crate::state::PowerAction::Sleep);
    }
    if options.demo_osd {
        let osd = desktop::panels::Osd {
            icon: "cc-volume",
            label: "Volume",
            level: Some(60),
        };
        state.osd = Some((
            osd,
            std::time::Instant::now() + std::time::Duration::from_secs(3600),
        ));
    }
    if options.demo_lock {
        state.demo_face = options.demo_face;
        state.lock_session();
        state.lock_reveal();
    }
    if let Err(err) = desktop::idle::start(&event_loop.handle()) {
        warn!(%err, "verrouillage et veille automatiques indisponibles");
    }
    if let Err(err) = desktop::widgets::start_weather(&event_loop.handle()) {
        warn!(%err, "météo du tableau de bord indisponible");
    }

    // Ouverture de session (ferrite-session, connexion automatique) : le bureau démarre
    // verrouillé. Un bureau imbriqué, lancé depuis un terminal, hérite de la variable : il
    // ne se verrouille pas.
    if !has_display && std::env::var_os("FERRITE_LOCK").is_some_and(|value| value == "1") {
        state.lock_session();
        // Son d'ouverture (s'il est activé) : pas à une simple relance du bureau.
        ferrite_system::sounds::play(ferrite_system::sounds::Sound::Startup);
    }
    // Un seul agent polkit par session : pas pour un bureau imbriqué.
    if !has_display {
        match desktop::polkit::start(&event_loop.handle()) {
            Ok(link) => state.polkit_link = Some(link),
            Err(err) => warn!(%err, "agent polkit indisponible"),
        }
    }
    match options.backend.resolve(has_display) {
        BackendChoice::Drm => backend::drm::init(&mut event_loop, &mut state, &options)?,
        _ => backend::winit::init(&mut event_loop, &mut state, &options)?,
    }
    info!(socket = ?state.socket_name, "ferrite-shell prêt");
    desktop::file_drag::listen(&event_loop.handle(), &state.socket_name.to_string_lossy());
    protocols::xwayland::start(&event_loop.handle(), &mut state);
    state.publish_environment();
    // Rappels du Calendrier : un service par session (pas pour un bureau imbriqué, qui
    // partage la session de celui qui l'a lancé).
    if !has_display {
        desktop::apps::App::start_reminders(&state.client_environment());
    }

    if let Some((program, args)) = options.command.split_first() {
        // WAYLAND_DISPLAY est passé explicitement au client, sans toucher à notre
        // propre environnement (le backend winit est lui-même client de WSLg).
        let mut executable = std::env::current_exe()
            .ok()
            .map(|path| path.with_file_name(program))
            .filter(|path| path.is_file())
            .map(Command::new)
            .unwrap_or_else(|| Command::new(program));
        executable.args(args).env("WAYLAND_DISPLAY", &state.socket_name);
        match ferrite_system::command::spawn_detached(executable) {
            Ok(pid) => info!(pid, "client lancé : {program}"),
            Err(err) => warn!("impossible de lancer {program} : {err}"),
        }
    }

    let loop_result = event_loop.run(None, &mut state, |state| {
        // Après chaque tour de boucle : fenêtres fermées retirées, messages envoyés.
        render::profile::timed("tour de boucle", || {
            state.space.refresh();
            state.sync_desktops();
            state.lock_tick();
            state.power_tick();
            state.night_tick();
            state.expire_notifications();
            state.retile();
            state.refit_to_area();
            state.refresh_controls();
            state.popups.cleanup();
            let _ = state.display_handle.flush_clients();
        });
    });
    if let Err(err) = loop_result {
        if state.expected_exit {
            info!(%err, "erreur de fermeture du backend ignorée après une fermeture demandée");
            return Ok(state.exit_code);
        }
        return Err(Box::new(err));
    }
    Ok(state.exit_code)
}
