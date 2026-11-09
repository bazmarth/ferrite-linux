//! Fenêtres gardées toujours au-dessus des autres (Super+Maj+T, menu du Dock).
//!
//! La marque est portée par la fenêtre elle-même : elle la suit quand elle est réduite ou
//! rangée sur un autre bureau. L'ordre est rétabli après chaque tour de boucle et avant
//! chaque image, où qu'une fenêtre ordinaire ait été remontée (clic, Alt+Tab, déplacement,
//! nouvelle fenêtre).

use std::sync::atomic::{AtomicBool, Ordering};

use smithay::desktop::Window;

use crate::{desktop::panels::Osd, state::Ferrite};

/// Nombre de parents remontés pour savoir si une boîte de dialogue suit une fenêtre gardée.
const MAX_PARENTS: usize = 8;

/// Marque « toujours au-dessus » d'une fenêtre.
struct KeptAbove(AtomicBool);

/// La fenêtre a-t-elle été mise « toujours au-dessus » ?
pub fn is_kept_above(window: &Window) -> bool {
    window
        .user_data()
        .get::<KeptAbove>()
        .is_some_and(|kept| kept.0.load(Ordering::Relaxed))
}

fn set_kept_above(window: &Window, on: bool) {
    window
        .user_data()
        .get_or_insert_threadsafe(|| KeptAbove(AtomicBool::new(false)))
        .0
        .store(on, Ordering::Relaxed);
}

/// Étage d'une fenêtre dans la pile, du plus bas au plus haut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Ordinary,
    /// Gardée au-dessus, ou boîte de dialogue d'une fenêtre gardée.
    Above,
    /// Menu ou bulle X11 : au-dessus de tout, sinon le menu d'une fenêtre ordinaire
    /// s'ouvrirait sous une fenêtre gardée.
    Menu,
}

/// La pile (du bas vers le haut) a-t-elle une fenêtre ordinaire au-dessus d'une gardée ?
fn out_of_order(layers: &[Layer]) -> bool {
    let mut above_seen = false;
    for layer in layers {
        match layer {
            Layer::Above => above_seen = true,
            Layer::Ordinary if above_seen => return true,
            _ => {}
        }
    }
    false
}

impl Ferrite {
    /// Fenêtre dont `window` est la boîte de dialogue, si elle est à l'écran.
    fn parent_window(&self, window: &Window) -> Option<Window> {
        if let Some(x11) = window.x11_surface() {
            let parent = x11.is_transient_for()?;
            return self
                .space
                .elements()
                .find(|other| other.x11_surface().is_some_and(|x11| x11.window_id() == parent))
                .cloned();
        }
        let parent = window.toplevel()?.parent()?;
        self.space
            .elements()
            .find(|other| {
                other
                    .toplevel()
                    .is_some_and(|toplevel| *toplevel.wl_surface() == parent)
            })
            .cloned()
    }

    fn layer_of(&self, window: &Window) -> Layer {
        if window.x11_surface().is_some_and(|x11| x11.is_override_redirect()) {
            return Layer::Menu;
        }
        let mut current = window.clone();
        for _ in 0..MAX_PARENTS {
            if is_kept_above(&current) {
                return Layer::Above;
            }
            match self.parent_window(&current) {
                Some(parent) => current = parent,
                None => break,
            }
        }
        Layer::Ordinary
    }

    /// Remet les fenêtres gardées au-dessus des autres, si une fenêtre ordinaire vient de
    /// passer devant. Ne fait rien, et ne coûte presque rien, quand l'ordre est bon.
    pub fn restack_kept_above(&mut self) {
        if !self.space.elements().any(is_kept_above) {
            return;
        }
        let windows: Vec<Window> = self.space.elements().cloned().collect();
        let layers: Vec<Layer> = windows.iter().map(|window| self.layer_of(window)).collect();
        if !out_of_order(&layers) {
            return;
        }
        // Chaque étage remonte en gardant son ordre : les gardées, puis les menus.
        for wanted in [Layer::Above, Layer::Menu] {
            for (window, _) in windows.iter().zip(&layers).filter(|(_, layer)| **layer == wanted) {
                self.space.raise_element(window, false);
                if let (Some(x11), Some(xwm)) = (window.x11_surface(), self.xwm.as_mut())
                    && let Err(err) = xwm.raise_window(x11)
                {
                    tracing::warn!(?err, "fenêtre X11 non remontée");
                }
            }
        }
        self.request_redraw();
        if self
            .seat
            .get_pointer()
            .is_some_and(|pointer| !pointer.is_grabbed())
        {
            self.refresh_pointer_focus();
        }
    }

    /// Garde `window` au-dessus des autres, ou la remet avec elles.
    pub fn set_keep_above(&mut self, window: &Window, on: bool) {
        set_kept_above(window, on);
        self.restack_kept_above();
        self.show_osd(Osd {
            icon: "clip-pin",
            label: if on {
                "Fenêtre toujours au-dessus"
            } else {
                "Fenêtre remise avec les autres"
            },
            level: None,
        });
    }

    /// Super+Maj+T : garde la fenêtre active au-dessus des autres, ou arrête.
    pub fn toggle_keep_above_active(&mut self) {
        if let Some(window) = self.active_window() {
            self.set_keep_above(&window, !is_kept_above(&window));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Layer::*, out_of_order};

    #[test]
    fn pile_en_ordre() {
        assert!(!out_of_order(&[]));
        assert!(!out_of_order(&[Ordinary, Ordinary]));
        assert!(!out_of_order(&[Ordinary, Above, Above]));
        assert!(!out_of_order(&[Ordinary, Above, Menu]));
        assert!(!out_of_order(&[Above]));
    }

    #[test]
    fn fenetre_ordinaire_passee_devant() {
        assert!(out_of_order(&[Above, Ordinary]));
        assert!(out_of_order(&[Ordinary, Above, Ordinary, Above]));
        assert!(out_of_order(&[Above, Menu, Ordinary]));
    }

    #[test]
    fn un_menu_devant_ne_derange_rien() {
        assert!(!out_of_order(&[Ordinary, Menu, Above]));
        assert!(!out_of_order(&[Above, Menu]));
    }
}
