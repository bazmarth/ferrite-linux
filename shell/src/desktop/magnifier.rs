//! Zoom de l'écran (loupe d'accessibilité) : tout l'écran grossi autour du pointeur.
//!
//! Le point de l'écran sous le pointeur reste sous le pointeur : la partie montrée glisse
//! quand la souris bouge, et arrive au bord de l'écran quand la souris y arrive. Les entrées
//! n'ont donc rien à convertir : ce que montre le pointeur est ce qu'il vise.
//!
//! Réglages : `loupe` (en marche) et `loupe_grossissement` (pour cent), dans Paramètres →
//! Accessibilité ou par Super+Alt+8, Super+Alt+= / ), et Super+Alt+molette.

use smithay::utils::{Logical, Point, Rectangle, Size};
use tracing::warn;

use ferrite_system::config::{
    MAGNIFIER, MAGNIFIER_MAX, MAGNIFIER_MIN, MAGNIFIER_STEPS, MAGNIFIER_ZOOM, Settings,
};

use crate::{desktop::dock::DockConfig, state::Ferrite};

/// Durée du passage d'un grossissement à l'autre, pour un doublement (log2).
pub const ZOOM_EASE: std::time::Duration = ferrite_ui::tokens::motion::SHORT;
/// Molette (en unités du pavé tactile) pour un cran de grossissement.
pub const SCROLL_NOTCH: f64 = 15.0;
/// Un cran de molette multiplie le grossissement par ce facteur.
const SCROLL_FACTOR: f64 = 1.15;

/// Grossissement voulu par les réglages, en log2 (0 : loupe arrêtée).
pub fn target_level(config: DockConfig) -> f64 {
    if config.magnifier {
        (f64::from(config.magnifier_zoom) / 100.0).log2().max(0.0)
    } else {
        0.0
    }
}

/// Grossissement suivant (`up`) ou précédent dans [`MAGNIFIER_STEPS`] ; `None` : sous le
/// plus petit, la loupe s'arrête.
pub fn next_zoom(current: u32, up: bool) -> Option<u32> {
    if up {
        Some(
            MAGNIFIER_STEPS
                .iter()
                .copied()
                .find(|step| *step > current)
                .unwrap_or(current.max(MAGNIFIER_STEPS[MAGNIFIER_STEPS.len() - 1])),
        )
    } else {
        MAGNIFIER_STEPS.iter().rev().copied().find(|step| *step < current)
    }
}

/// Grossissement après `notches` crans de molette (positifs : plus gros) ; `None` : sous le
/// minimum, la loupe s'arrête.
pub fn scrolled_zoom(current: u32, notches: i32) -> Option<u32> {
    let zoom = f64::from(current) * SCROLL_FACTOR.powi(notches);
    if zoom < f64::from(MAGNIFIER_MIN) {
        return None;
    }
    Some((zoom.round() as u32).clamp(MAGNIFIER_MIN, MAGNIFIER_MAX))
}

/// Partie de l'écran montrée, grossie `zoom` fois, le pointeur en `pointer` (coordonnées de
/// l'écran) : le point sous le pointeur reste à sa place.
pub fn visible_area(
    screen: Size<f64, Logical>,
    pointer: Point<f64, Logical>,
    zoom: f64,
) -> Rectangle<f64, Logical> {
    let zoom = zoom.max(1.0);
    let x = pointer.x.clamp(0.0, screen.w);
    let y = pointer.y.clamp(0.0, screen.h);
    let shrink = 1.0 - 1.0 / zoom;
    Rectangle::new(
        Point::from((x * shrink, y * shrink)),
        Size::from((screen.w / zoom, screen.h / zoom)),
    )
}

impl Ferrite {
    /// Super+Alt+8 : loupe en marche ou arrêtée, au dernier grossissement choisi.
    pub fn toggle_magnifier(&mut self) {
        let config = self.dock_config;
        self.set_magnifier(!config.magnifier, config.magnifier_zoom);
    }

    /// Super+Alt+= ou ) : grossissement suivant ou précédent. Arrêtée, la loupe démarre en
    /// grossissant ; sous le plus petit grossissement, elle s'arrête.
    pub fn step_magnifier(&mut self, up: bool) {
        let config = self.dock_config;
        if !config.magnifier {
            if up {
                self.set_magnifier(true, config.magnifier_zoom);
            }
            return;
        }
        match next_zoom(config.magnifier_zoom, up) {
            Some(zoom) => self.set_magnifier(true, zoom),
            None => self.set_magnifier(false, config.magnifier_zoom),
        }
    }

    /// Super+Alt+molette : grossissement progressif (vers le haut : plus gros).
    pub fn scroll_magnifier(&mut self, amount: f64) {
        self.magnifier_scroll += amount;
        let notches = (self.magnifier_scroll / SCROLL_NOTCH).trunc();
        if notches == 0.0 {
            return;
        }
        self.magnifier_scroll -= notches * SCROLL_NOTCH;
        // Molette vers le haut : quantité négative.
        let notches = -(notches as i32);
        let config = self.dock_config;
        if !config.magnifier {
            if notches > 0 {
                self.set_magnifier(true, MAGNIFIER_MIN.max(125));
            }
            return;
        }
        match scrolled_zoom(config.magnifier_zoom, notches) {
            Some(zoom) => self.set_magnifier(true, zoom),
            None => self.set_magnifier(false, MAGNIFIER_STEPS[1]),
        }
    }

    /// Applique et enregistre l'état de la loupe (Paramètres le relit).
    fn set_magnifier(&mut self, on: bool, zoom: u32) {
        let zoom = zoom.clamp(MAGNIFIER_MIN, MAGNIFIER_MAX);
        if self.dock_config.magnifier == on && self.dock_config.magnifier_zoom == zoom {
            return;
        }
        self.dock_config.magnifier = on;
        self.dock_config.magnifier_zoom = zoom;
        let mut settings = Settings::load();
        settings.set_flag(MAGNIFIER, on);
        settings.set(MAGNIFIER_ZOOM, &zoom.to_string());
        if let Err(err) = settings.save() {
            warn!(%err, "zoom de l'écran non enregistré");
        }
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_point_sous_le_pointeur_reste_a_sa_place() {
        let screen = Size::from((1440.0, 900.0));
        for (x, y) in [(0.0, 0.0), (720.0, 450.0), (1440.0, 900.0), (100.0, 800.0)] {
            for zoom in [1.0, 1.5, 2.0, 4.0] {
                let area = visible_area(screen, Point::from((x, y)), zoom);
                // Point de l'écran montré sous le pointeur.
                let shown_x = area.loc.x + x / screen.w * area.size.w;
                let shown_y = area.loc.y + y / screen.h * area.size.h;
                assert!((shown_x - x).abs() < 1e-9 && (shown_y - y).abs() < 1e-9);
                // La partie montrée ne sort jamais de l'écran.
                assert!(area.loc.x >= 0.0 && area.loc.y >= 0.0);
                assert!(area.loc.x + area.size.w <= screen.w + 1e-9);
                assert!(area.loc.y + area.size.h <= screen.h + 1e-9);
                assert!((area.size.w - screen.w / zoom).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn crans_du_grossissement() {
        assert_eq!(next_zoom(200, true), Some(250));
        assert_eq!(next_zoom(220, true), Some(250));
        assert_eq!(next_zoom(800, true), Some(800));
        assert_eq!(next_zoom(1000, true), Some(1000));
        assert_eq!(next_zoom(200, false), Some(150));
        assert_eq!(next_zoom(150, false), None);
        assert_eq!(scrolled_zoom(200, 1), Some(230));
        assert_eq!(scrolled_zoom(200, 0), Some(200));
        assert_eq!(scrolled_zoom(MAGNIFIER_MIN, -1), None);
        assert_eq!(scrolled_zoom(990, 3), Some(MAGNIFIER_MAX));
    }

    #[test]
    fn grossissement_voulu() {
        let mut config = DockConfig::default();
        assert_eq!(target_level(config), 0.0);
        config.magnifier = true;
        config.magnifier_zoom = 400;
        assert!((target_level(config) - 2.0).abs() < 1e-9);
    }
}
