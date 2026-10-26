//! Boîte à outils graphique de Ferrite.
//!
//! Pour l'instant, la crate contient la charte visuelle (`theme`) : les mesures, couleurs,
//! polices et durées d'animation partagées par le compositeur `ferrite-shell` et les
//! applications. Les widgets arriveront en phase 4 (voir `docs/PLAN.md`).

pub mod icons;
pub mod theme;

pub use theme::{Color, Easing, Palette, Scale, ThemeMode};
