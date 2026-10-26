//! Backends d'affichage et d'entrée.
//!
//! - `winit` : dans une fenêtre d'un autre système graphique (WSLg pour le développement).
//! - `drm` : directement sur l'écran (DRM/KMS, GBM, libinput), session ouverte via libseat.

pub mod drm;
pub mod winit;

/// Échelle choisie d'après la densité de l'écran, quand l'utilisateur n'en impose pas :
/// 2 au-delà de 200 points par pouce (ThinkBook 2880×1800 : environ 240), 1,5 au-delà de
/// 150, 1 sinon ou si la taille physique est inconnue (machines virtuelles).
pub fn auto_scale(width_px: u32, width_mm: u32) -> f64 {
    if width_mm == 0 {
        return 1.0;
    }
    let dpi = width_px as f64 / (width_mm as f64 / 25.4);
    if dpi >= 200.0 {
        2.0
    } else if dpi >= 150.0 {
        1.5
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echelle_selon_la_densite() {
        // ThinkBook 14 pouces, 2880×1800 sur environ 302 mm.
        assert_eq!(auto_scale(2880, 302), 2.0);
        // Portable 15,6 pouces en 1920×1080 (344 mm) : 142 ppp.
        assert_eq!(auto_scale(1920, 344), 1.0);
        // 2560×1440 sur un portable de 14 pouces (310 mm) : 210 ppp.
        assert_eq!(auto_scale(2560, 310), 2.0);
        // 1920×1200 sur 14 pouces (302 mm) : 161 ppp.
        assert_eq!(auto_scale(1920, 302), 1.5);
        // Taille inconnue (QEMU).
        assert_eq!(auto_scale(1280, 0), 1.0);
    }
}
