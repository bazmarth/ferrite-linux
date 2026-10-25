//! ferrite-shell : compositeur Wayland et bureau de Ferrite.
//!
//! Phase 2 du plan (`docs/PLAN.md`). Pour l'instant : mode fenêtre (backend winit, pour
//! WSLg), protocoles de base (compositor, shm, seat, output, xdg-shell, data-device),
//! placement en cascade, déplacement et redimensionnement, fond d'écran Ferrite.
//! La structure suit d'abord l'exemple `smallvil` de Smithay 0.7.

mod backend;
mod input;
mod options;
mod protocols;
mod render;
mod state;
mod wm;

use std::process::{Command, ExitCode};

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};
use tracing::{error, info, warn};

use crate::{
    options::{Options, Parsed},
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
    let mut event_loop: EventLoop<Ferrite> = EventLoop::try_new()?;
    let display: Display<Ferrite> = Display::new()?;
    let mut state = Ferrite::new(&mut event_loop, display)?;

    backend::winit::init(&mut event_loop, &mut state, &options)?;
    info!(socket = ?state.socket_name, "ferrite-shell prêt");

    if let Some((program, args)) = options.command.split_first() {
        // WAYLAND_DISPLAY est passé explicitement au client, sans toucher à notre
        // propre environnement (le backend winit est lui-même client de WSLg).
        match Command::new(program)
            .args(args)
            .env("WAYLAND_DISPLAY", &state.socket_name)
            .spawn()
        {
            Ok(child) => info!(pid = child.id(), "client lancé : {program}"),
            Err(err) => warn!("impossible de lancer {program} : {err}"),
        }
    }

    event_loop.run(None, &mut state, |_| {})?;
    Ok(())
}
