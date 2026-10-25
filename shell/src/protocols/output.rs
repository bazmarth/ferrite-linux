//! `wl_output` et `xdg-output` : description des écrans aux clients.

use smithay::{delegate_output, wayland::output::OutputHandler};

use crate::state::Ferrite;

impl OutputHandler for Ferrite {}

delegate_output!(Ferrite);
