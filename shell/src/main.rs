//! ferrite-shell : compositeur Wayland et bureau de Ferrite.
//!
//! Phase 2 du plan (`docs/PLAN.md`). Deux modes : en fenêtre (backend winit, pour WSLg)
//! ou directement sur l'écran (backend DRM/KMS + libinput). Protocoles de base (compositor, shm, seat, output, xdg-shell, data-device),
//! placement en cascade, déplacement et redimensionnement, fond d'écran Ferrite.
//! La structure suit d'abord l'exemple `smallvil` de Smithay 0.7.

mod backend;
mod desktop;
mod input;
mod options;
mod protocols;
mod render;
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
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<'static, Ferrite> = EventLoop::try_new()?;
    let display: Display<Ferrite> = Display::new()?;
    let mut state = Ferrite::new(&mut event_loop, display)?;
    state.popover = options.menu;
    match system::start(&event_loop.handle()) {
        Ok(link) => state.system_link = Some(link),
        Err(err) => warn!("services système indisponibles : {err}"),
    }

    let has_display = std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some();
    match options.backend.resolve(has_display) {
        BackendChoice::Drm => backend::drm::init(&mut event_loop, &mut state, &options)?,
        _ => backend::winit::init(&mut event_loop, &mut state, &options)?,
    }
    info!(socket = ?state.socket_name, "ferrite-shell prêt");
    protocols::xwayland::start(&event_loop.handle(), &mut state);
    state.publish_environment();

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
        state.space.refresh();
        state.popups.cleanup();
        let _ = state.display_handle.flush_clients();
    });
    if let Err(err) = loop_result {
        if state.expected_exit {
            info!(%err, "erreur de fermeture du backend ignorée après une fermeture demandée");
            return Ok(());
        }
        return Err(Box::new(err));
    }
    Ok(())
}
