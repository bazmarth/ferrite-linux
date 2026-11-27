//! Fenêtre d'authentification (agent polkit, voir `desktop::polkit`) : modale, centrée,
//! dans le style du dialogue d'arrêt. Le mot de passe n'y arrive que par sa longueur.

use super::*;

/// Taille de la fenêtre.
const AUTH: (f64, f64) = (440.0, 292.0);
/// Écart entre deux points du mot de passe.
const AUTH_DOT: f64 = 10.0;

/// Points visibles du mot de passe : le curseur reste dans le champ (`text_field::window`).
fn auth_window(layout: &Layout, auth: &AuthView) -> (usize, Vec<f64>) {
    let room = layout.auth_field().w - 28.0;
    text_field::window(&vec![AUTH_DOT; auth.length], auth.cursor, room)
}

impl Layout {
    fn auth(&self) -> Rect {
        Rect::new(
            ((self.w - AUTH.0) / 2.0).max(8.0),
            ((self.h - AUTH.1) / 2.0).max(self.bar + 8.0),
            AUTH.0,
            AUTH.1,
        )
    }

    fn auth_field(&self) -> Rect {
        let dialog = self.auth();
        Rect::new(dialog.x + 24.0, dialog.y + 150.0, dialog.w - 48.0, 42.0)
    }

    /// Curseur du champ du mot de passe.
    pub(super) fn auth_caret(&self, auth: &AuthView) -> Rect {
        let rect = self.auth_field();
        let (first, offsets) = auth_window(self, auth);
        let index = auth
            .cursor
            .min(auth.length)
            .saturating_sub(first)
            .min(offsets.len() - 1);
        let x = rect.x + 14.0 + offsets[index] - if auth.length == 0 { 0.0 } else { 1.0 };
        Rect::new(x, rect.y + 11.0, 1.6, 20.0)
    }

    fn auth_confirm(&self) -> Rect {
        let dialog = self.auth();
        Rect::new(
            dialog.right() - 24.0 - 130.0,
            dialog.bottom() - 24.0 - 40.0,
            130.0,
            40.0,
        )
    }

    fn auth_cancel(&self) -> Rect {
        let confirm = self.auth_confirm();
        Rect::new(confirm.x - 10.0 - 104.0, confirm.y, 104.0, confirm.h)
    }
}

/// Clic dans la fenêtre (ou à côté : il ne passe pas aux fenêtres de dessous).
pub(super) fn action_at(layout: &Layout, pos: Point<f64, Logical>) -> DesktopAction {
    if layout.auth_confirm().contains(pos) {
        DesktopAction::AuthSubmit
    } else if layout.auth_cancel().contains(pos) {
        DesktopAction::AuthCancel
    } else {
        DesktopAction::Consume
    }
}

impl Panels {
    pub(super) fn draw_auth(&self, canvas: &mut Canvas, layout: &Layout, auth: &AuthView) {
        let dark = palette().text.r > 128;
        let pick = |light: u32, dark_value: u32| Color::rgb(if dark { dark_value } else { light });
        let (fg, fg2, fg3) = (
            palette().text,
            palette().text_secondary,
            palette().window_title_inactive,
        );
        let (panel, line, chip, field) = (
            palette().window_background,
            palette().separator,
            palette().control_hover,
            pick(0xF5F6F8, 0x1B1F25),
        );
        let blue = accent();
        let red = pick(0xD92D20, 0xFF8A80);
        let scrim = if dark {
            Color::BLACK.with_alpha(128)
        } else {
            Color::rgb(0x0F172A).with_alpha(82)
        };
        canvas.fill(Rect::new(0.0, 0.0, layout.w, layout.h), 0.0, scrim);

        let dialog = layout.auth();
        canvas.panel_shadow(dialog, 20.0);
        canvas.fill(dialog, 20.0, panel);
        let x = dialog.x + 24.0;

        // Cadenas, titre et compte.
        let badge = Rect::new(x, dialog.y + 24.0, 40.0, 40.0);
        canvas.fill(badge, 20.0, blue);
        canvas.icon("cc-lock", badge.center(), 20.0, Style::Symbolic(Color::WHITE));
        canvas.text(
            &self.bold,
            "Authentification requise",
            x + 54.0,
            dialog.y + 24.0,
            18.0,
            fg,
        );
        let account = format!("Mot de passe administrateur de {}", auth.user);
        let account = canvas.fit_text(&self.regular, &account, px(13.0), dialog.w - 102.0);
        canvas.text(&self.regular, &account, x + 54.0, dialog.y + 48.0, 13.0, fg3);

        // Ce que l'application veut faire, en trois lignes au plus.
        let lines = wrap_dashboard_text(&self.regular, &auth.message, px(14.0), dialog.w - 48.0);
        for (index, text) in lines.iter().take(3).enumerate() {
            canvas.text(
                &self.regular,
                text,
                x,
                dialog.y + 80.0 + index as f64 * 20.0,
                14.0,
                fg2,
            );
        }

        // Champ du mot de passe.
        let rect = layout.auth_field();
        canvas.fill(rect, 11.0, field);
        canvas.stroke(rect, 11.0, if auth.refused { red } else { blue }, 1.5);
        let text_x = rect.x + 14.0;
        if auth.checking {
            canvas.text(&self.regular, "Vérification…", text_x, rect.y + 12.0, 14.0, fg3);
        } else if auth.length == 0 {
            // Le curseur est posé à part, au-dessus (`Layout::auth_caret`).
            canvas.text(
                &self.regular,
                "Mot de passe",
                text_x + 6.0,
                rect.y + 12.0,
                14.0,
                fg3,
            );
        } else {
            let (_, offsets) = auth_window(layout, auth);
            for index in 0..offsets.len() - 1 {
                let dot = Rect::new(text_x + index as f64 * AUTH_DOT, rect.y + 17.0, 8.0, 8.0);
                canvas.fill(dot, 4.0, fg);
            }
        }
        if auth.refused {
            let text = if auth.length == 0 {
                "Mot de passe incorrect, ou vide"
            } else {
                "Mot de passe incorrect"
            };
            canvas.text(&self.regular, text, rect.x + 2.0, rect.bottom() + 6.0, 13.0, red);
        }

        // Boutons.
        let separator = layout.auth_cancel().y - 14.0;
        canvas.fill(Rect::new(dialog.x, separator, dialog.w, 1.0), 0.0, line);
        for (rect, label, fill, ink) in [
            (layout.auth_cancel(), "Annuler", chip, fg),
            (layout.auth_confirm(), "Autoriser", blue, Color::WHITE),
        ] {
            canvas.fill(rect, 11.0, fill);
            let width = canvas.css_width(&self.bold, label, 14.0);
            canvas.text(
                &self.bold,
                label,
                rect.x + (rect.w - width) / 2.0,
                rect.y + 11.0,
                14.0,
                ink,
            );
        }
    }
}
