//! Animation de réduction « génie », comme celle du Ferrite d'origine : le bas de la
//! fenêtre s'étire jusqu'à son icône du Dock en se resserrant en entonnoir, puis toute
//! la fenêtre s'y engouffre. La réapparition fait le même chemin à l'envers. Une nouvelle
//! fenêtre, elle, apparaît en grandissant légèrement et en se dévoilant, comme sur macOS.
//! Agrandie par le bouton vert (ou rendue à sa taille), elle glisse d'un cadre à l'autre.
//!
//! Ce module ne calcule que la géométrie ([`Genie`]) ; le rendu (`render::genie`) pose
//! une image de la fenêtre entre les bords courbes de l'entonnoir, ligne par ligne.

use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

use ferrite_ui::theme::TITLEBAR_HEIGHT;
use smithay::{
    backend::renderer::gles::GlesTexture,
    desktop::Window,
    utils::{Logical, Point, Rectangle, Size},
};

/// Durée d'une réduction ou d'une réapparition.
pub const DURATION: Duration = Duration::from_millis(520);
/// Durée de l'apparition d'une nouvelle fenêtre.
pub const OPEN_DURATION: Duration = Duration::from_millis(260);
/// Durée du passage d'une fenêtre à sa taille agrandie, ou de son retour.
pub const ZOOM_DURATION: Duration = Duration::from_millis(300);
/// Taille de départ d'une nouvelle fenêtre, par rapport à sa taille finale.
const OPEN_SCALE: f64 = 0.88;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// Vers le Dock.
    Minimize,
    /// Depuis le Dock.
    Restore,
    /// Nouvelle fenêtre, qui apparaît à sa place.
    Open,
    /// Agrandissement ou retour à la taille d'avant (bouton vert).
    Zoom,
}

impl Motion {
    fn duration(self) -> Duration {
        match self {
            Motion::Minimize | Motion::Restore => DURATION,
            Motion::Open => OPEN_DURATION,
            Motion::Zoom => ZOOM_DURATION,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WindowAnimation {
    pub window: Window,
    pub motion: Motion,
    /// Contenu de la fenêtre à sa place.
    pub window_rect: Rectangle<f64, Logical>,
    /// Icône du Dock, ou contenu de la fenêtre à sa nouvelle taille pour un
    /// agrandissement (inutilisé pour une ouverture).
    pub dock_rect: Rectangle<f64, Logical>,
    pub start: Instant,
    /// Image de la fenêtre (barre de titre comprise), prise à la première image de
    /// l'animation puis réutilisée.
    pub snapshot: RefCell<Option<GlesTexture>>,
    /// Animation figée à cette fraction (démonstration `--demo-genie`).
    pub frozen: Option<f64>,
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
            snapshot: RefCell::new(None),
            frozen: None,
        }
    }

    pub fn finished(&self, now: Instant) -> bool {
        let elapsed = now.duration_since(self.start);
        if self.frozen.is_some() || elapsed < self.motion.duration() {
            return false;
        }
        // Un agrandissement attend (un peu) que l'application ait pris sa nouvelle taille,
        // pour ne pas la montrer à l'ancienne une fraction de seconde.
        if self.motion != Motion::Zoom {
            return true;
        }
        let titlebar = if crate::wm::is_server_decorated(&self.window) {
            TITLEBAR_HEIGHT as f64
        } else {
            0.0
        };
        let target =
            Size::<f64, Logical>::from((self.dock_rect.size.w, (self.dock_rect.size.h - titlebar).max(1.0)));
        self.window.geometry().size.to_f64() == target || elapsed >= self.motion.duration() * 4
    }

    /// Déformation à l'instant `now` d'une fenêtre dont le cadre au repos est `frame` ;
    /// `target` est l'icône du Dock, ou le cadre d'arrivée d'un agrandissement.
    pub fn genie(
        &self,
        frame: Rectangle<f64, Logical>,
        target: Rectangle<f64, Logical>,
        now: Instant,
    ) -> Genie {
        let elapsed = match self.frozen {
            Some(fraction) => self.motion.duration().mul_f64(fraction),
            None => now.duration_since(self.start),
        };
        genie(self.motion, frame, target, elapsed)
    }
}

/// Fenêtre déformée : entre `top` et `bottom`, chaque ligne de l'image est posée entre
/// les bords donnés par [`Genie::span`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Genie {
    /// Cadre de la fenêtre au repos, et icône du Dock.
    pub window: Rectangle<f64, Logical>,
    pub icon: Rectangle<f64, Logical>,
    /// Ordonnées du haut et du bas de l'image étirée.
    pub top: f64,
    pub bottom: f64,
    /// Resserrement de l'entonnoir : 0, bords droits ; 1, col à la largeur de l'icône.
    pub squeeze: f64,
    /// Opacité de la fenêtre.
    pub alpha: f64,
}

impl Genie {
    /// Bords gauche et droit de la fenêtre déformée à l'ordonnée `y` : de la largeur de
    /// la fenêtre en haut à celle de l'icône au niveau du Dock, en courbe douce. Le shader
    /// `GENIE_TEXTURE` fait le même calcul sur le GPU.
    pub fn span(&self, y: f64) -> (f64, f64) {
        let (window, icon) = (self.window, self.icon);
        let neck = icon.loc.y.max(window.loc.y + 1.0);
        let s = smoothstep((y - window.loc.y) / (neck - window.loc.y)) * self.squeeze;
        (
            lerp(window.loc.x, icon.loc.x, s),
            lerp(window.loc.x + window.size.w, icon.loc.x + icon.size.w, s),
        )
    }

    /// Rectangle qui contient la fenêtre déformée : les bords vont en s'éloignant ou en
    /// se rapprochant d'un bout à l'autre, leurs extrêmes sont en haut et en bas.
    pub fn bounds(&self) -> Rectangle<f64, Logical> {
        let (top_left, top_right) = self.span(self.top);
        let (bottom_left, bottom_right) = self.span(self.bottom);
        let left = top_left.min(bottom_left);
        let right = top_right.max(bottom_right);
        Rectangle::new(
            Point::from((left, self.top)),
            Size::from((right - left, (self.bottom - self.top).max(1.0))),
        )
    }
}

/// Déformation `elapsed` après le début de l'animation.
fn genie(
    motion: Motion,
    window: Rectangle<f64, Logical>,
    icon: Rectangle<f64, Logical>,
    elapsed: Duration,
) -> Genie {
    let linear = (elapsed.as_secs_f64() / motion.duration().as_secs_f64()).clamp(0.0, 1.0);
    // 0 : fenêtre à sa place ; 1 : rentrée dans l'icône.
    let t = match motion {
        Motion::Minimize => smoothstep(linear),
        Motion::Restore => 1.0 - smoothstep(linear),
        Motion::Open => return opening(window, linear),
        Motion::Zoom => return zoom(window, icon, linear),
    };
    let phase = |from: f64, to: f64| smoothstep((t - from) / (to - from));
    Genie {
        window,
        icon,
        // D'abord le bas file vers l'icône et l'entonnoir se forme ; le haut suit ensuite.
        squeeze: phase(0.0, 0.4),
        bottom: lerp(
            window.loc.y + window.size.h,
            icon.loc.y + icon.size.h,
            phase(0.0, 0.5),
        ),
        top: lerp(window.loc.y, icon.loc.y, phase(0.3, 1.0)),
        alpha: 1.0,
    }
}

/// Nouvelle fenêtre : elle grandit autour de son centre, vite puis en douceur, et se
/// dévoile un peu plus vite qu'elle ne grandit.
fn opening(window: Rectangle<f64, Logical>, linear: f64) -> Genie {
    let ease_out = |t: f64| 1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3);
    let scale = lerp(OPEN_SCALE, 1.0, ease_out(linear));
    let size = Size::from((window.size.w * scale, window.size.h * scale));
    let center = (
        window.loc.x + window.size.w / 2.0,
        window.loc.y + window.size.h / 2.0,
    );
    let rect = Rectangle::new(
        Point::from((center.0 - size.w / 2.0, center.1 - size.h / 2.0)),
        size,
    );
    Genie {
        window: rect,
        icon: rect,
        top: rect.loc.y,
        bottom: rect.loc.y + rect.size.h,
        squeeze: 0.0,
        alpha: ease_out(linear * 1.6),
    }
}

/// Agrandissement : le cadre glisse et s'étire de `from` à `to`, vite puis en douceur.
fn zoom(from: Rectangle<f64, Logical>, to: Rectangle<f64, Logical>, linear: f64) -> Genie {
    let t = 1.0 - (1.0 - linear).powi(3);
    let rect = Rectangle::new(
        Point::from((lerp(from.loc.x, to.loc.x, t), lerp(from.loc.y, to.loc.y, t))),
        Size::from((lerp(from.size.w, to.size.w, t), lerp(from.size.h, to.size.h, t))),
    );
    Genie {
        window: rect,
        icon: rect,
        top: rect.loc.y,
        bottom: rect.loc.y + rect.size.h,
        squeeze: 0.0,
        alpha: 1.0,
    }
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects() -> (Rectangle<f64, Logical>, Rectangle<f64, Logical>) {
        (
            Rectangle::new(Point::from((200.0, 100.0)), Size::from((800.0, 600.0))),
            Rectangle::new(Point::from((300.0, 820.0)), Size::from((52.0, 52.0))),
        )
    }

    #[test]
    fn la_fenetre_part_de_sa_place_et_finit_dans_l_icone() {
        let (window, icon) = rects();
        let start = genie(Motion::Minimize, window, icon, Duration::ZERO);
        assert_eq!(start.bounds(), window);
        assert_eq!(start.span(400.0), (200.0, 1000.0));
        let end = genie(Motion::Minimize, window, icon, DURATION);
        assert_eq!(end.bounds(), icon);
        assert_eq!(end.span(840.0), (300.0, 352.0));
        // La réapparition fait le chemin inverse.
        assert_eq!(
            genie(Motion::Restore, window, icon, Duration::ZERO).bounds(),
            icon
        );
        assert_eq!(genie(Motion::Restore, window, icon, DURATION).bounds(), window);
    }

    #[test]
    fn une_nouvelle_fenetre_grandit_en_apparaissant() {
        let (window, icon) = rects();
        let start = genie(Motion::Open, window, icon, Duration::ZERO);
        assert_eq!(start.alpha, 0.0);
        assert!((start.bounds().size.w - 800.0 * OPEN_SCALE).abs() < 0.01);
        // Centrée sur sa place, sans entonnoir.
        let bounds = start.bounds();
        assert!((bounds.loc.x + bounds.size.w / 2.0 - 600.0).abs() < 0.01);
        assert_eq!(start.span(400.0), (bounds.loc.x, bounds.loc.x + bounds.size.w));
        let end = genie(Motion::Open, window, icon, OPEN_DURATION);
        assert_eq!(end.bounds(), window);
        assert_eq!(end.alpha, 1.0);
    }

    #[test]
    fn a_mi_chemin_le_haut_reste_large_et_le_bas_rejoint_l_icone() {
        let (window, icon) = rects();
        let middle = genie(Motion::Minimize, window, icon, DURATION / 2);
        // Le bas touche déjà l'icône, le haut est encore près de sa place.
        assert!(middle.bottom > icon.loc.y);
        assert!(middle.top < window.loc.y + 200.0);
        let (top_left, top_right) = middle.span(middle.top);
        let (neck_left, neck_right) = middle.span(icon.loc.y);
        assert!(top_right - top_left > 2.0 * (neck_right - neck_left));
        // Le col est à la largeur de l'icône, aligné sur elle.
        assert!((neck_left - icon.loc.x).abs() < 1.0);
        assert!((neck_right - (icon.loc.x + icon.size.w)).abs() < 1.0);
    }

    #[test]
    fn la_fenetre_glisse_vers_sa_taille_agrandie() {
        let (window, _) = rects();
        let screen = Rectangle::new(Point::from((0.0, 30.0)), Size::from((1280.0, 700.0)));
        assert_eq!(
            genie(Motion::Zoom, window, screen, Duration::ZERO).bounds(),
            window
        );
        let middle = genie(Motion::Zoom, window, screen, ZOOM_DURATION / 2).bounds();
        assert!(middle.size.w > 800.0 && middle.size.w < 1280.0);
        assert!(middle.loc.x < 200.0 && middle.loc.x > 0.0);
        assert_eq!(
            genie(Motion::Zoom, window, screen, ZOOM_DURATION).bounds(),
            screen
        );
    }
}
