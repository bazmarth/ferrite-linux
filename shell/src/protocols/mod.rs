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
//! - `input_method` : saisie de texte (`text-input-v3`, `input-method-v2`) et forme du
//!   curseur (`cursor-shape-v1`)
//! - `idle` : inactivité (`ext-idle-notify-v1`, `idle-inhibit-v1`)
//! - `layer_shell` : barres, lanceurs et fonds d'autres bureaux (`wlr-layer-shell`)
//! - `session_lock` : verrouilleurs externes (`ext-session-lock-v1`)
//! - `screencopy` : captures et enregistrements de l'écran (`wlr-screencopy-v1`)
//! - `xwayland` : applications X11

mod compositor;
mod decoration;
mod dmabuf;
mod extra;
mod idle;
mod input_method;
mod layer_shell;
mod output;
pub mod screencopy;
mod seat;
pub mod session_lock;
pub mod xdg_shell;
pub mod xwayland;
