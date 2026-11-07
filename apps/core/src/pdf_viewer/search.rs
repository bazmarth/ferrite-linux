//! Recherche dans le texte du document, sans tenir compte des majuscules ni des accents.
//! Chaque résultat garde les rectangles des mots trouvés, pour les surligner sur la page.

use eframe::egui;

use super::poppler::Word;

#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    /// Page, numérotée depuis 0.
    pub page: usize,
    /// Rectangles surlignés, en points sur la page.
    pub rects: Vec<egui::Rect>,
}

/// Forme comparable d'un caractère : minuscule, sans accent. Un caractère donne toujours
/// un seul caractère, pour que les positions restent alignées sur le texte.
fn fold(character: char) -> char {
    let lower = character.to_lowercase().next().unwrap_or(character);
    match lower {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        '’' | '‘' => '\'',
        '\u{a0}' => ' ',
        other => other,
    }
}

/// Cherche `query` dans les mots de chaque page ; une expression peut couvrir plusieurs mots.
pub fn find(pages: &[Vec<Word>], query: &str) -> Vec<Match> {
    let needle: Vec<char> = query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .map(fold)
        .collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (page, words) in pages.iter().enumerate() {
        // Texte de la page (mots séparés d'une espace) et, pour chaque caractère, son mot
        // et sa position dans le mot.
        let mut text: Vec<char> = Vec::new();
        let mut owner: Vec<Option<(usize, usize)>> = Vec::new();
        for (index, word) in words.iter().enumerate() {
            if index > 0 {
                text.push(' ');
                owner.push(None);
            }
            for (offset, character) in word.text.chars().enumerate() {
                text.push(fold(character));
                owner.push(Some((index, offset)));
            }
        }
        let mut start = 0;
        while start + needle.len() <= text.len() {
            if text[start..start + needle.len()] != needle[..] {
                start += 1;
                continue;
            }
            let span = &owner[start..start + needle.len()];
            let mut rects: Vec<egui::Rect> = Vec::new();
            let mut current: Option<(usize, usize, usize)> = None; // mot, premier, dernier
            let flush = |current: Option<(usize, usize, usize)>, rects: &mut Vec<egui::Rect>| {
                if let Some((word, first, last)) = current {
                    rects.push(part_of(&words[word], first, last));
                }
            };
            for entry in span.iter().flatten() {
                let (word, offset) = *entry;
                current = match current {
                    Some((same, first, _)) if same == word => Some((same, first, offset)),
                    other => {
                        flush(other, &mut rects);
                        Some((word, offset, offset))
                    }
                };
            }
            flush(current, &mut rects);
            found.push(Match { page, rects });
            start += needle.len();
        }
    }
    found
}

/// Part du rectangle d'un mot occupée par ses caractères `first..=last` (largeur répartie
/// à parts égales : assez juste pour surligner).
fn part_of(word: &Word, first: usize, last: usize) -> egui::Rect {
    let count = word.text.chars().count().max(1) as f32;
    let width = word.rect.width();
    let left = word.rect.left() + width * first as f32 / count;
    let right = word.rect.left() + width * (last + 1) as f32 / count;
    egui::Rect::from_min_max(
        egui::pos2(left, word.rect.top()),
        egui::pos2(right, word.rect.bottom()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x: f32) -> Word {
        Word {
            text: text.to_owned(),
            rect: egui::Rect::from_min_size(
                egui::pos2(x, 10.0),
                egui::vec2(10.0 * text.chars().count() as f32, 12.0),
            ),
        }
    }

    #[test]
    fn trouve_sans_majuscules_ni_accents() {
        let pages = vec![
            vec![word("Été", 0.0), word("chaud", 40.0)],
            vec![word("ete", 0.0)],
        ];
        let found = find(&pages, "ete");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].page, 0);
        assert_eq!(found[1].page, 1);
        assert_eq!(found[0].rects, vec![pages[0][0].rect]);
    }

    #[test]
    fn une_expression_couvre_plusieurs_mots() {
        let pages = vec![vec![word("Le", 0.0), word("rapport", 30.0), word("final", 110.0)]];
        let found = find(&pages, "  RAPPORT   fin ");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rects.len(), 2);
        assert_eq!(found[0].rects[0], pages[0][1].rect);
        // « fin » : les trois premiers caractères de « final ».
        assert_eq!(found[0].rects[1].left(), 110.0);
        assert_eq!(found[0].rects[1].right(), 140.0);
    }

    #[test]
    fn recherche_vide_ou_absente() {
        let pages = vec![vec![word("bonjour", 0.0)]];
        assert!(find(&pages, "   ").is_empty());
        assert!(find(&pages, "adieu").is_empty());
        assert_eq!(find(&pages, "o").len(), 2);
    }
}
