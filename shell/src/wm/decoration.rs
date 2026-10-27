//! Géométrie des décorations serveur : barre de titre, feux tricolores, bords de
//! redimensionnement. Tout est en pixels logiques, relatif au contenu de la fenêtre
//! (la surface du client) ; la barre de titre est posée juste au-dessus.

use ferrite_ui::theme::{TITLEBAR_HEIGHT, TRAFFIC_LIGHT_FIRST_X, TRAFFIC_LIGHT_STEP};
use smithay::utils::{Logical, Point, Rectangle};

use super::resize_grab::ResizeEdge;

/// Largeur de la zone de saisie des bords, autour du cadre (invisible, comme sous Windows).
pub const RESIZE_BORDER: i32 = 10;
/// Près d'un coin, sur cette longueur le long des bords, on tire les deux côtés à la fois.
const RESIZE_CORNER: f64 = 20.0;
/// Rayon de clic d'un feu (un peu plus large que le feu dessiné).
const LIGHT_HIT_RADIUS: f64 = 9.5;

/// Les trois feux, de gauche à droite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficLight {
    Close,
    Minimize,
    Zoom,
}

impl TrafficLight {
    pub const ALL: [TrafficLight; 3] = [TrafficLight::Close, TrafficLight::Minimize, TrafficLight::Zoom];

    fn index(self) -> i32 {
        match self {
            TrafficLight::Close => 0,
            TrafficLight::Minimize => 1,
            TrafficLight::Zoom => 2,
        }
    }
}

/// Partie de la décoration sous le pointeur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Titlebar,
    Light(TrafficLight),
    Border(ResizeEdge),
}

/// Barre de titre d'une fenêtre dont le contenu occupe `content`.
pub fn titlebar(content: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    let height = TITLEBAR_HEIGHT as i32;
    Rectangle::new(
        (content.loc.x, content.loc.y - height).into(),
        (content.size.w, height).into(),
    )
}

/// Cadre complet : barre de titre et contenu.
pub fn frame(content: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    let height = TITLEBAR_HEIGHT as i32;
    Rectangle::new(
        (content.loc.x, content.loc.y - height).into(),
        (content.size.w, content.size.h + height).into(),
    )
}

/// Centre d'un feu, en coordonnées globales.
pub fn light_center(content: Rectangle<i32, Logical>, light: TrafficLight) -> Point<f64, Logical> {
    let bar = titlebar(content);
    let x = bar.loc.x + (TRAFFIC_LIGHT_FIRST_X + TRAFFIC_LIGHT_STEP * light.index() as u32) as i32;
    let y = bar.loc.y as f64 + TITLEBAR_HEIGHT as f64 / 2.0;
    Point::from((x as f64, y))
}

/// Le pointeur survole-t-il le groupe des feux (les symboles ×, −, + apparaissent) ?
pub fn lights_hovered(content: Rectangle<i32, Logical>, pos: Point<f64, Logical>) -> bool {
    let first = light_center(content, TrafficLight::Close);
    let last = light_center(content, TrafficLight::Zoom);
    let margin = LIGHT_HIT_RADIUS;
    pos.x >= first.x - margin && pos.x <= last.x + margin && (pos.y - first.y).abs() <= margin
}

/// Partie de la décoration sous `pos`, ou `None` si `pos` est dans le contenu ou hors du
/// cadre et de ses bords.
pub fn hit(content: Rectangle<i32, Logical>, pos: Point<f64, Logical>) -> Option<Part> {
    hit_frame(content, frame(content), pos)
}

/// Bords d'une fenêtre sans barre de titre (elle dessine la sienne) : seulement le
/// redimensionnement.
pub fn hit_border(content: Rectangle<i32, Logical>, pos: Point<f64, Logical>) -> Option<Part> {
    hit_frame(content, content, pos).filter(|part| matches!(part, Part::Border(_)))
}

fn hit_frame(
    content: Rectangle<i32, Logical>,
    frame: Rectangle<i32, Logical>,
    pos: Point<f64, Logical>,
) -> Option<Part> {
    let (left, top) = (frame.loc.x as f64, frame.loc.y as f64);
    let (right, bottom) = (left + frame.size.w as f64, top + frame.size.h as f64);
    let border = RESIZE_BORDER as f64;
    if pos.x < left - border || pos.x >= right + border || pos.y < top - border || pos.y >= bottom + border {
        return None;
    }

    let inside_x = pos.x >= left && pos.x < right;
    let inside_y = pos.y >= top && pos.y < bottom;
    if !(inside_x && inside_y) {
        let mut edges = ResizeEdge::empty();
        if pos.x < left {
            edges |= ResizeEdge::LEFT;
        } else if pos.x >= right {
            edges |= ResizeEdge::RIGHT;
        }
        if pos.y < top {
            edges |= ResizeEdge::TOP;
        } else if pos.y >= bottom {
            edges |= ResizeEdge::BOTTOM;
        }
        // Coins larges : un bord saisi près d'un coin tire aussi l'autre côté.
        if !edges.intersects(ResizeEdge::TOP | ResizeEdge::BOTTOM) {
            if pos.y < top + RESIZE_CORNER {
                edges |= ResizeEdge::TOP;
            } else if pos.y >= bottom - RESIZE_CORNER {
                edges |= ResizeEdge::BOTTOM;
            }
        }
        if !edges.intersects(ResizeEdge::LEFT | ResizeEdge::RIGHT) {
            if pos.x < left + RESIZE_CORNER {
                edges |= ResizeEdge::LEFT;
            } else if pos.x >= right - RESIZE_CORNER {
                edges |= ResizeEdge::RIGHT;
            }
        }
        return Some(Part::Border(edges));
    }

    if pos.y >= content.loc.y as f64 {
        return None;
    }
    for light in TrafficLight::ALL {
        let center = light_center(content, light);
        let (dx, dy) = (pos.x - center.x, pos.y - center.y);
        if dx * dx + dy * dy <= LIGHT_HIT_RADIUS * LIGHT_HIT_RADIUS {
            return Some(Part::Light(light));
        }
    }
    Some(Part::Titlebar)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> Rectangle<i32, Logical> {
        Rectangle::new((100, 130).into(), (400, 300).into())
    }

    fn p(x: f64, y: f64) -> Point<f64, Logical> {
        Point::from((x, y))
    }

    #[test]
    fn cadre_et_barre_de_titre() {
        assert_eq!(
            titlebar(content()),
            Rectangle::new((100, 100).into(), (400, 30).into())
        );
        assert_eq!(
            frame(content()),
            Rectangle::new((100, 100).into(), (400, 330).into())
        );
        assert_eq!(light_center(content(), TrafficLight::Close), p(122.0, 115.0));
        assert_eq!(light_center(content(), TrafficLight::Zoom), p(168.0, 115.0));
    }

    #[test]
    fn zones_de_clic() {
        let c = content();
        assert_eq!(hit(c, p(120.0, 115.0)), Some(Part::Light(TrafficLight::Close)));
        assert_eq!(hit(c, p(144.0, 116.0)), Some(Part::Light(TrafficLight::Minimize)));
        // Sans barre de titre : seuls les bords comptent.
        assert_eq!(hit_border(c, p(300.0, 135.0)), None);
        assert_eq!(
            hit_border(c, p(300.0, 125.0)),
            Some(Part::Border(ResizeEdge::TOP))
        );
        assert_eq!(hit(c, p(300.0, 110.0)), Some(Part::Titlebar));
        assert_eq!(hit(c, p(300.0, 200.0)), None, "dans le contenu");
        assert_eq!(hit(c, p(50.0, 50.0)), None, "loin de la fenêtre");
        assert_eq!(hit(c, p(97.0, 200.0)), Some(Part::Border(ResizeEdge::LEFT)));
        assert_eq!(
            hit(c, p(502.0, 432.0)),
            Some(Part::Border(ResizeEdge::BOTTOM_RIGHT))
        );
        assert_eq!(hit(c, p(300.0, 96.0)), Some(Part::Border(ResizeEdge::TOP)));
        // Zone élargie : 10 px autour du cadre.
        assert_eq!(hit(c, p(91.0, 200.0)), Some(Part::Border(ResizeEdge::LEFT)));
        assert_eq!(hit(c, p(89.0, 200.0)), None);
        // Coins larges : le bord droit près du bas tire aussi le bas.
        assert_eq!(
            hit(c, p(505.0, 420.0)),
            Some(Part::Border(ResizeEdge::BOTTOM_RIGHT))
        );
        assert_eq!(hit(c, p(110.0, 97.0)), Some(Part::Border(ResizeEdge::TOP_LEFT)));
    }

    #[test]
    fn survol_des_feux() {
        let c = content();
        assert!(lights_hovered(c, p(140.0, 115.0)));
        assert!(lights_hovered(c, p(166.0, 118.0)));
        assert!(!lights_hovered(c, p(200.0, 115.0)));
        assert!(!lights_hovered(c, p(140.0, 140.0)));
    }
}
