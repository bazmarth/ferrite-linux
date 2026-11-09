//! Géométrie des décorations serveur : barre de titre, boutons de fenêtre, bords de
//! redimensionnement. Tout est en pixels logiques, relatif au contenu de la fenêtre
//! (la surface du client) ; la barre de titre est posée juste au-dessus.

use ferrite_ui::theme::{TITLEBAR_HEIGHT, WINDOW_BUTTON_GAP, WINDOW_BUTTON_INSET, WINDOW_BUTTON_RADIUS};
use smithay::utils::{Logical, Point, Rectangle};

use super::resize_grab::ResizeEdge;

/// Largeur de la zone de saisie des bords, autour du cadre (invisible, comme sous Windows).
pub const RESIZE_BORDER: i32 = 8;
/// Près d'un coin, sur cette longueur le long des bords, on tire les deux côtés à la fois.
const RESIZE_CORNER: f64 = 16.0;
/// Rayon de clic d'un bouton : un peu plus large que la pastille dessinée.
const BUTTON_HIT_RADIUS: f64 = WINDOW_BUTTON_RADIUS as f64 + 1.0;

/// Les trois boutons d'une fenêtre, tous à droite : réduire, agrandir, fermer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowButton {
    Close,
    Zoom,
    Minimize,
}

impl WindowButton {
    pub const ALL: [WindowButton; 3] = [WindowButton::Close, WindowButton::Zoom, WindowButton::Minimize];

    /// Rang depuis le bord droit (fermer au bord).
    fn rank(self) -> i32 {
        match self {
            WindowButton::Close => 0,
            WindowButton::Zoom => 1,
            WindowButton::Minimize => 2,
        }
    }
}

/// Place réservée aux boutons à droite de la barre (le titre reste centré, la même marge
/// est gardée à gauche).
pub const BUTTONS_WIDTH: i32 =
    (WINDOW_BUTTON_INSET + 2 * WINDOW_BUTTON_GAP + WINDOW_BUTTON_RADIUS) as i32 + 12;

/// Partie de la décoration sous le pointeur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Titlebar,
    Button(WindowButton),
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

/// Centre d'un bouton, en coordonnées globales.
pub fn button_center(content: Rectangle<i32, Logical>, button: WindowButton) -> Point<f64, Logical> {
    let bar = titlebar(content);
    let x = bar.loc.x + bar.size.w - WINDOW_BUTTON_INSET as i32 - button.rank() * WINDOW_BUTTON_GAP as i32;
    Point::from((x as f64, bar.loc.y as f64 + TITLEBAR_HEIGHT as f64 / 2.0))
}

/// Bouton sous le pointeur, s'il y en a un.
pub fn hovered_button(content: Rectangle<i32, Logical>, pos: Point<f64, Logical>) -> Option<WindowButton> {
    WindowButton::ALL.into_iter().find(|button| {
        let center = button_center(content, *button);
        (pos.x - center.x).hypot(pos.y - center.y) <= BUTTON_HIT_RADIUS
    })
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
    if let Some(button) = hovered_button(content, pos) {
        return Some(Part::Button(button));
    }
    Some(Part::Titlebar)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> Rectangle<i32, Logical> {
        Rectangle::new((100, 146).into(), (400, 300).into())
    }

    fn p(x: f64, y: f64) -> Point<f64, Logical> {
        Point::from((x, y))
    }

    #[test]
    fn cadre_et_barre_de_titre() {
        assert_eq!(
            titlebar(content()),
            Rectangle::new((100, 108).into(), (400, 38).into())
        );
        assert_eq!(
            frame(content()),
            Rectangle::new((100, 108).into(), (400, 338).into())
        );
    }

    #[test]
    fn trois_boutons_a_droite_de_la_barre() {
        let c = content();
        assert_eq!(hit(c, p(481.0, 127.0)), Some(Part::Button(WindowButton::Close)));
        assert_eq!(hit(c, p(451.0, 127.0)), Some(Part::Button(WindowButton::Zoom)));
        assert_eq!(
            hit(c, p(421.0, 127.0)),
            Some(Part::Button(WindowButton::Minimize))
        );
        assert_eq!(hit(c, p(119.0, 127.0)), Some(Part::Titlebar));
        assert_eq!(hit(c, p(300.0, 123.0)), Some(Part::Titlebar));
        assert_eq!(hit(c, p(300.0, 200.0)), None);
        assert_eq!(hovered_button(c, p(300.0, 123.0)), None);
    }

    #[test]
    fn bords_de_redimensionnement() {
        let c = content();
        assert_eq!(hit(c, p(96.0, 300.0)), Some(Part::Border(ResizeEdge::LEFT)));
        assert_eq!(hit(c, p(504.0, 300.0)), Some(Part::Border(ResizeEdge::RIGHT)));
        assert_eq!(hit(c, p(300.0, 450.0)), Some(Part::Border(ResizeEdge::BOTTOM)));
        // Coin : les deux côtés à la fois, sur 16 px le long des bords.
        assert_eq!(
            hit(c, p(504.0, 445.0)),
            Some(Part::Border(ResizeEdge::BOTTOM_RIGHT))
        );
        assert_eq!(hit(c, p(96.0, 110.0)), Some(Part::Border(ResizeEdge::TOP_LEFT)));
        // Hors de portée.
        assert_eq!(hit(c, p(90.0, 300.0)), None);
    }

    #[test]
    fn bords_seuls_sans_barre_de_titre() {
        let c = content();
        assert_eq!(hit_border(c, p(300.0, 200.0)), None);
        assert_eq!(
            hit_border(c, p(96.0, 300.0)),
            Some(Part::Border(ResizeEdge::LEFT))
        );
    }
}
