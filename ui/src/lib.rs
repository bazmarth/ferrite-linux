//! Boîte à outils graphique de Ferrite.
//!
//! La crate contient les tokens visuels indépendants du rendu (`tokens`), la charte
//! (`theme`) et les fonds d'écran (`wallpaper`). Les widgets arriveront en phase 4
//! (voir `docs/PLAN.md`).

pub mod glyphs;
pub mod icons;
pub mod theme;
pub mod tokens;
pub mod wallpaper;

pub use theme::{Color, Easing, Palette, Scale, ThemeMode};
