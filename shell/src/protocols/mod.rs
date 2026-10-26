//! Protocoles Wayland pris en charge, un module par protocole (voir `docs/PLAN.md` §3.2).
//!
//! - `compositor` : `wl_compositor`, `wl_subcompositor`, `wl_shm`
//! - `xdg_shell` : fenêtres et menus contextuels
//! - `decoration` : décorations côté serveur (`xdg-decoration`)
//! - `seat` : clavier, pointeur, presse-papiers et glisser-déposer (`wl_data_device`)
//! - `output` : `wl_output` et `xdg-output`
//! - `dmabuf` : tampons du GPU (`linux-dmabuf`)
//! - `extra` : viewporter, mise à l'échelle fractionnaire, activation, sélection primaire,
//!   pointeur relatif
//! - `xwayland` : applications X11

mod compositor;
mod decoration;
mod dmabuf;
mod extra;
mod output;
mod seat;
mod xdg_shell;
pub mod xwayland;
