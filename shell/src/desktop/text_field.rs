//! Champs de texte dessinés par le bureau (recherche du lanceur et du presse-papiers, mot de
//! passe Wi-Fi ou administrateur) : place du curseur, édition autour de lui, et
//! clignotement. Le curseur est posé par la carte graphique au-dessus du panneau déjà
//! dessiné : il clignote sans rien redessiner (`Panels::push_caret`).

use std::time::Duration;

use smithay::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::{desktop::panels::Popover, state::Ferrite};

/// Demi-période du clignotement.
const BLINK: Duration = Duration::from_millis(530);

/// Phase du clignotement, commune à tous les champs (un seul a le clavier à la fois).
#[derive(Debug)]
pub struct Caret {
    /// Le curseur est dessiné.
    pub on: bool,
    /// Génération du minuteur : un minuteur plus ancien s'arrête.
    generation: u32,
}

impl Default for Caret {
    fn default() -> Self {
        Self {
            on: true,
            generation: 0,
        }
    }
}

/// Position en octets du caractère numéro `index` (la fin du texte au-delà).
pub fn byte_at(text: &str, index: usize) -> usize {
    text.char_indices().nth(index).map_or(text.len(), |(at, _)| at)
}

/// Caractère tapé, à la place du curseur, qui avance.
pub fn insert(text: &mut String, cursor: &mut usize, character: char) {
    let at = (*cursor).min(text.chars().count());
    text.insert(byte_at(text, at), character);
    *cursor = at + 1;
}

/// Retour arrière : le caractère avant le curseur.
pub fn erase(text: &mut String, cursor: &mut usize) {
    let at = (*cursor).min(text.chars().count());
    if at > 0 {
        text.remove(byte_at(text, at - 1));
    }
    *cursor = at.saturating_sub(1);
}

/// Suppr : le caractère après le curseur.
pub fn delete(text: &mut String, cursor: &mut usize) {
    let length = text.chars().count();
    let at = (*cursor).min(length);
    if at < length {
        text.remove(byte_at(text, at));
    }
    *cursor = at;
}

/// ← et → (`step` de -1 ou 1), Début et Fin (`isize::MIN` ou `isize::MAX`).
pub fn step(text: &str, cursor: &mut usize, step: isize) {
    let length = text.chars().count();
    *cursor = (*cursor).min(length).saturating_add_signed(step).min(length);
}

/// ↑ et ↓ (`step` de -1 ou 1) dans un texte sur plusieurs lignes : même colonne sur la
/// ligne d'avant ou d'après (au bout de celle-ci si elle est plus courte). Sur la
/// première ou la dernière ligne, le curseur va au début ou à la fin.
pub fn line_step(text: &str, cursor: &mut usize, step: isize) {
    let chars: Vec<char> = text.chars().collect();
    let at = (*cursor).min(chars.len());
    // Début de chaque ligne, en caractères.
    let mut starts = vec![0];
    starts.extend(
        chars
            .iter()
            .enumerate()
            .filter(|(_, character)| **character == '\n')
            .map(|(index, _)| index + 1),
    );
    let line = starts.iter().rposition(|&start| start <= at).unwrap_or(0);
    let column = at - starts[line];
    let end_of = |line: usize| starts.get(line + 1).map_or(chars.len(), |next| next - 1);
    *cursor = match line.checked_add_signed(step) {
        Some(target) if target < starts.len() => (starts[target] + column).min(end_of(target)),
        _ if step < 0 => 0,
        _ => chars.len(),
    };
}

/// Partie visible d'un texte dont les caractères ont ces largeurs, dans `room` pixels :
/// premier caractère montré, puis la place de chaque limite entre caractères depuis le
/// début du texte montré (la première vaut 0). Le curseur reste toujours visible.
pub fn window(widths: &[f64], cursor: usize, room: f64) -> (usize, Vec<f64>) {
    let cursor = cursor.min(widths.len());
    let span = |from: usize, to: usize| widths[from..to].iter().sum::<f64>();
    let mut first = 0;
    while first < cursor && span(first, cursor) > room {
        first += 1;
    }
    let mut last = widths.len();
    while last > cursor && span(first, last) > room {
        last -= 1;
    }
    let mut offsets = vec![0.0];
    for width in &widths[first..last] {
        offsets.push(offsets.last().copied().unwrap_or_default() + width);
    }
    (first, offsets)
}

impl Ferrite {
    /// Place du curseur dans le texte du tableau de bord en cours de saisie : au bout du
    /// texte quand la saisie commence.
    pub fn dashboard_cursor(&self) -> usize {
        let length = self
            .dashboard
            .edited_text(self.dashboard_editing)
            .map_or(0, |text| text.chars().count());
        match self.dashboard_cursor {
            Some((editing, cursor)) if Some(editing) == self.dashboard_editing => cursor.min(length),
            _ => length,
        }
    }

    /// Change le texte du tableau de bord en cours de saisie, ou la place du curseur.
    pub fn dashboard_edit(&mut self, change: impl FnOnce(&mut String, &mut usize)) {
        let Some(editing) = self.dashboard_editing else {
            return;
        };
        let Some(before) = self.dashboard.edited_text(Some(editing)).map(str::to_owned) else {
            return;
        };
        let mut cursor = self.dashboard_cursor();
        let mut after = before.clone();
        change(&mut after, &mut cursor);
        self.dashboard_cursor = Some((editing, cursor));
        self.restart_caret();
        // Un simple déplacement du curseur ne touche ni à la note ni à sa date.
        if after == before {
            return;
        }
        let save = self
            .dashboard
            .edit_text(Some(editing), crate::desktop::widgets::now_seconds(), |text| {
                *text = after;
            });
        if save && let Err(error) = self.dashboard.save() {
            tracing::warn!(%error, "notes du tableau de bord non enregistrées");
        }
    }

    /// Un champ du bureau a-t-il le clavier ?
    pub fn caret_wanted(&self) -> bool {
        if self.lock.is_some() {
            return false;
        }
        if self.dashboard_editing.is_some() {
            return true;
        }
        if let Some(auth) = self.auth.front() {
            return !auth.checking;
        }
        match self.popover {
            Some(Popover::Launcher | Popover::Clipboard) => true,
            Some(Popover::Controls) => self.controls.password.is_some(),
            _ => false,
        }
    }

    /// Curseur visible tout de suite (après une frappe ou une ouverture), puis clignotant
    /// tant qu'un champ a le clavier ; fixe si les animations sont réduites.
    pub fn restart_caret(&mut self) {
        self.caret.on = true;
        self.caret.generation = self.caret.generation.wrapping_add(1);
        if self.dock_config.reduce_motion || !self.caret_wanted() {
            return;
        }
        let generation = self.caret.generation;
        let inserted = self
            .loop_handle
            .insert_source(Timer::from_duration(BLINK), move |_, _, state| {
                if state.caret.generation != generation {
                    return TimeoutAction::Drop;
                }
                if !state.caret_wanted() {
                    state.caret.on = true;
                    return TimeoutAction::Drop;
                }
                state.caret.on = !state.caret.on;
                state.request_redraw();
                TimeoutAction::ToDuration(BLINK)
            });
        if let Err(error) = inserted {
            tracing::warn!(%error, "clignotement du curseur impossible");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l_edition_suit_le_curseur_en_caracteres() {
        let (mut text, mut cursor) = (String::from("éte"), 3);
        step(&text, &mut cursor, -2);
        insert(&mut text, &mut cursor, 'à');
        assert_eq!((text.as_str(), cursor), ("éàte", 2));
        erase(&mut text, &mut cursor);
        assert_eq!((text.as_str(), cursor), ("éte", 1));
        delete(&mut text, &mut cursor);
        assert_eq!((text.as_str(), cursor), ("ée", 1));
        step(&text, &mut cursor, isize::MAX);
        assert_eq!(cursor, 2);
        step(&text, &mut cursor, isize::MIN);
        assert_eq!(cursor, 0);
        // Curseur resté au-delà d'un texte vidé ailleurs : ramené au bout.
        text.clear();
        let mut cursor = 7;
        insert(&mut text, &mut cursor, 'a');
        assert_eq!((text.as_str(), cursor), ("a", 1));
    }

    #[test]
    fn haut_et_bas_gardent_la_colonne() {
        let text = "Titre long\nab\nligne trois";
        let mut cursor = 7; // « Titre l|ong »
        line_step(text, &mut cursor, 1);
        assert_eq!(cursor, 13); // bout de « ab »
        line_step(text, &mut cursor, 1);
        assert_eq!(cursor, 16); // « li|gne trois »
        line_step(text, &mut cursor, 1);
        assert_eq!(cursor, text.chars().count());
        line_step(text, &mut cursor, -1);
        assert_eq!(cursor, 13);
        line_step(text, &mut cursor, -1);
        line_step(text, &mut cursor, -1);
        assert_eq!(cursor, 0);
    }

    #[test]
    fn la_fenetre_garde_le_curseur_visible() {
        let widths = [10.0; 100];
        let (first, offsets) = window(&widths, 0, 90.0);
        assert_eq!((first, offsets.len() - 1), (0, 9));
        let (first, offsets) = window(&widths, 100, 90.0);
        assert_eq!(first + offsets.len() - 1, 100);
        assert!(offsets.last().is_some_and(|&end| end <= 90.0));
    }
}
