//! Sélection du texte d'un document : quel mot est sous le pointeur, quels rectangles
//! peindre pour une plage de mots, et quel texte copier. Tout se calcule sur les mots et
//! leurs rectangles donnés par Poppler, dans l'ordre de lecture.

use eframe::egui;

use super::poppler::Word;

/// Un mot du document : page et rang dans la page (numérotés depuis 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
    pub page: usize,
    pub word: usize,
}

/// Plage de mots, du mot où l'on a appuyé (`anchor`) à celui sous le pointeur (`focus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Cursor,
    pub focus: Cursor,
}

impl Selection {
    pub fn single(cursor: Cursor) -> Self {
        Self {
            anchor: cursor,
            focus: cursor,
        }
    }

    /// Premier et dernier mot, dans l'ordre de lecture.
    pub fn ordered(&self) -> (Cursor, Cursor) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    /// Mots choisis sur la page `page` (`premier..=dernier`), s'il y en a.
    pub fn on_page(&self, page: usize, count: usize) -> Option<(usize, usize)> {
        let (start, end) = self.ordered();
        if count == 0 || page < start.page || page > end.page {
            return None;
        }
        let first = if page == start.page { start.word } else { 0 };
        let last = if page == end.page { end.word } else { count - 1 };
        (first < count).then(|| (first, last.min(count - 1)))
    }
}

/// Deux mots qui se suivent sont sur la même ligne s'ils se recouvrent en hauteur et que
/// le second est à droite du premier.
fn same_line(before: &Word, after: &Word) -> bool {
    let overlap = before.rect.bottom().min(after.rect.bottom()) - before.rect.top().max(after.rect.top());
    let height = before.rect.height().min(after.rect.height()).max(0.1);
    overlap > height * 0.5 && after.rect.left() >= before.rect.left()
}

/// Mot qui contient `point` (avec une petite marge), en points de la page.
pub fn word_at(words: &[Word], point: egui::Pos2) -> Option<usize> {
    words
        .iter()
        .position(|word| word.rect.expand(1.5).contains(point))
}

/// Mot le plus proche de `point` : pour étendre la sélection quand le pointeur est dans
/// une marge ou entre deux lignes. L'écart en hauteur compte davantage, pour rester sur
/// la ligne visée.
pub fn nearest(words: &[Word], point: egui::Pos2) -> Option<usize> {
    let distance = |word: &Word| {
        let dx = (word.rect.left() - point.x)
            .max(point.x - word.rect.right())
            .max(0.0);
        let dy = (word.rect.top() - point.y)
            .max(point.y - word.rect.bottom())
            .max(0.0);
        dx + 4.0 * dy
    };
    words
        .iter()
        .enumerate()
        .min_by(|a, b| distance(a.1).total_cmp(&distance(b.1)))
        .map(|(index, _)| index)
}

/// Ligne entière qui contient le mot `index` (`premier..=dernier`).
pub fn line_of(words: &[Word], index: usize) -> (usize, usize) {
    let mut first = index;
    while first > 0 && same_line(&words[first - 1], &words[first]) {
        first -= 1;
    }
    let mut last = index;
    while last + 1 < words.len() && same_line(&words[last], &words[last + 1]) {
        last += 1;
    }
    (first, last)
}

/// Rectangles à peindre pour les mots `first..=last` : un par ligne.
pub fn rects(words: &[Word], first: usize, last: usize) -> Vec<egui::Rect> {
    let mut found: Vec<egui::Rect> = Vec::new();
    let last = last.min(words.len().saturating_sub(1));
    for index in first..=last {
        let Some(word) = words.get(index) else {
            break;
        };
        match found.last_mut() {
            Some(line) if index > first && same_line(&words[index - 1], word) => {
                *line = line.union(word.rect);
            }
            _ => found.push(word.rect),
        }
    }
    found
}

/// Texte des mots `first..=last` : une espace entre deux mots, un retour à chaque ligne.
/// Un mot coupé par un trait d'union en fin de ligne est recollé.
pub fn text(words: &[Word], first: usize, last: usize) -> String {
    let mut out = String::new();
    let last = last.min(words.len().saturating_sub(1));
    for index in first..=last {
        let Some(word) = words.get(index) else {
            break;
        };
        if index > first {
            if same_line(&words[index - 1], word) {
                out.push(' ');
            } else if out.ends_with('-') && out.chars().rev().nth(1).is_some_and(char::is_alphabetic) {
                out.pop();
            } else {
                out.push('\n');
            }
        }
        out.push_str(&word.text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x: f32, y: f32) -> Word {
        Word {
            text: text.to_owned(),
            rect: egui::Rect::from_min_size(
                egui::pos2(x, y),
                egui::vec2(10.0 * text.chars().count() as f32, 12.0),
            ),
        }
    }

    /// Deux lignes : « Le rapport an- » puis « nuel est prêt ».
    fn page() -> Vec<Word> {
        vec![
            word("Le", 0.0, 10.0),
            word("rapport", 30.0, 10.0),
            word("an-", 110.0, 10.0),
            word("nuel", 0.0, 30.0),
            word("est", 50.0, 30.0),
            word("prêt", 90.0, 30.0),
        ]
    }

    #[test]
    fn trouve_le_mot_sous_le_pointeur_ou_le_plus_proche() {
        let words = page();
        assert_eq!(word_at(&words, egui::pos2(35.0, 15.0)), Some(1));
        assert_eq!(word_at(&words, egui::pos2(300.0, 15.0)), None);
        // À droite de la première ligne : son dernier mot, pas un mot de la ligne du dessous.
        assert_eq!(nearest(&words, egui::pos2(300.0, 15.0)), Some(2));
        assert_eq!(nearest(&words, egui::pos2(60.0, 200.0)), Some(4));
        assert_eq!(nearest(&[], egui::pos2(0.0, 0.0)), None);
    }

    #[test]
    fn une_ligne_et_ses_rectangles() {
        let words = page();
        assert_eq!(line_of(&words, 1), (0, 2));
        assert_eq!(line_of(&words, 3), (3, 5));
        let found = rects(&words, 1, 4);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].left(), 30.0);
        assert_eq!(found[0].right(), 140.0);
        assert_eq!(found[1].left(), 0.0);
        assert_eq!(found[1].right(), 80.0);
    }

    #[test]
    fn le_texte_copie_suit_les_lignes() {
        let words = page();
        assert_eq!(text(&words, 0, 1), "Le rapport");
        assert_eq!(text(&words, 1, 4), "rapport annuel est");
        let lines = vec![word("Titre", 0.0, 10.0), word("Suite", 0.0, 30.0)];
        assert_eq!(text(&lines, 0, 1), "Titre\nSuite");
    }

    #[test]
    fn une_selection_couvre_plusieurs_pages() {
        let selection = Selection {
            anchor: Cursor { page: 2, word: 1 },
            focus: Cursor { page: 0, word: 4 },
        };
        assert_eq!(selection.ordered().0, Cursor { page: 0, word: 4 });
        assert_eq!(selection.on_page(0, 6), Some((4, 5)));
        assert_eq!(selection.on_page(1, 6), Some((0, 5)));
        assert_eq!(selection.on_page(2, 6), Some((0, 1)));
        assert_eq!(selection.on_page(3, 6), None);
        assert_eq!(selection.on_page(1, 0), None);
    }
}
