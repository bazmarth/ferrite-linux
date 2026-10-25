//! Protocoles Wayland pris en charge, un module par protocole (voir `docs/PLAN.md` §3.2).
//!
//! - `compositor` : `wl_compositor`, `wl_subcompositor`, `wl_shm`
//! - `xdg_shell` : fenêtres et menus contextuels
//! - `seat` : clavier, pointeur, presse-papiers et glisser-déposer (`wl_data_device`)
//! - `output` : `wl_output` et `xdg-output`

mod compositor;
mod output;
mod seat;
mod xdg_shell;
