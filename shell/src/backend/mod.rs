//! Backends d'affichage et d'entrée.
//!
//! - `winit` : dans une fenêtre d'un autre système graphique (WSLg pour le développement).
//! - `drm` (à venir) : directement sur l'écran et les périphériques (DRM/KMS, libinput).

pub mod winit;
