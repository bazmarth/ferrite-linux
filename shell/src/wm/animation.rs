//! Animation de réduction, comme sur macOS (effet « échelle ») : la fenêtre rétrécit et
//! s'estompe en filant vers son icône du Dock, et en ressort quand on la rappelle.

use std::time::{Duration, Instant};

use smithay::{
    desktop::Window,
    utils::{Logical, Point, Rectangle, Size},
};

/// Durée d'une réduction ou d'une réapparition.
pub const DURATION: Duration = Duration::from_millis(320);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// Vers le Dock.
    Minimize,
    /// Depuis le Dock.
    Restore,
}

#[derive(Debug, Clone)]
pub struct WindowAnimation {
    pub window: Window,
    pub motion: Motion,
    /// Contenu de la fenêtre à sa place.
    pub window_rect: Rectangle<f64, Logical>,
    /// Icône du Dock.
    pub dock_rect: Rectangle<f64, Logical>,
    pub start: Instant,
}

impl WindowAnimation {
    pub fn new(
        window: Window,
        motion: Motion,
        window_rect: Rectangle<f64, Logical>,
        dock_rect: Rectangle<f64, Logical>,
    ) -> Self {
        Self {
            window,
            motion,
            window_rect,
            dock_rect,
            start: Instant::now(),
        }
    }

    pub fn finished(&self, now: Instant) -> bool {
        now.duration_since(self.start) >= DURATION
    }

    /// Rectangle et opacité de la fenêtre à l'instant `now`.
    pub fn frame(&self, now: Instant) -> (Rectangle<f64, Logical>, f32) {
        frame(
            self.motion,
            self.window_rect,
            self.dock_rect,
            now.duration_since(self.start),
        )
    }
}

/// Rectangle et opacité, `elapsed` après le début de l'animation.
fn frame(
    motion: Motion,
    window_rect: Rectangle<f64, Logical>,
    dock_rect: Rectangle<f64, Logical>,
    elapsed: Duration,
) -> (Rectangle<f64, Logical>, f32) {
    let t = (elapsed.as_secs_f64() / DURATION.as_secs_f64()).clamp(0.0, 1.0);
    // Part du chemin vers le Dock : accélère en partant, freine en revenant.
    let toward_dock = match motion {
        Motion::Minimize => t * t,
        Motion::Restore => (1.0 - t).powi(3),
    };
    (
        lerp_rect(window_rect, dock_rect, toward_dock),
        (1.0 - 0.75 * toward_dock) as f32,
    )
}

fn lerp_rect(a: Rectangle<f64, Logical>, b: Rectangle<f64, Logical>, t: f64) -> Rectangle<f64, Logical> {
    let lerp = |x: f64, y: f64| x + (y - x) * t;
    // La fenêtre garde ses proportions : elle se réduit vers la largeur de l'icône.
    let width = lerp(a.size.w, b.size.w);
    let height = a.size.h * width / a.size.w.max(1.0);
    let center_x = lerp(a.loc.x + a.size.w / 2.0, b.loc.x + b.size.w / 2.0);
    let bottom = lerp(a.loc.y + a.size.h, b.loc.y + b.size.h);
    Rectangle::new(
        Point::from((center_x - width / 2.0, bottom - height)),
        Size::from((width, height)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_fenetre_file_vers_son_icone() {
        let window_rect = Rectangle::new(Point::from((200.0, 100.0)), Size::from((800.0, 600.0)));
        let icon = Rectangle::new(Point::from((600.0, 650.0)), Size::from((52.0, 52.0)));
        let (begin, opaque) = frame(Motion::Minimize, window_rect, icon, Duration::ZERO);
        assert_eq!(begin, window_rect);
        assert_eq!(opaque, 1.0);
        let (end, faded) = frame(Motion::Minimize, window_rect, icon, DURATION);
        assert!((end.size.w - 52.0).abs() < 1e-6);
        assert!(
            (end.loc.y + end.size.h - 702.0).abs() < 1e-6,
            "posée sur le bas de l'icône"
        );
        assert!(faded < 0.5);
        // La réapparition fait le chemin inverse : elle part de l'icône.
        let (start, _) = frame(Motion::Restore, window_rect, icon, Duration::ZERO);
        assert!((start.size.w - 52.0).abs() < 1e-6);
        assert_eq!(frame(Motion::Restore, window_rect, icon, DURATION).0, window_rect);
    }
}
