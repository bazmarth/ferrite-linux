//! Traduction du clavier et de la souris en séquences pour le programme du terminal.
//! Tout est pur et testable : rien ne dépend de l'affichage.

use alacritty_terminal::term::TermMode;
use eframe::egui::{Key, Modifiers};

/// Modificateurs au format xterm : `1 + Maj + 2 × Alt + 4 × Ctrl`.
fn modifier_code(modifiers: Modifiers) -> u8 {
    1 + u8::from(modifiers.shift) + 2 * u8::from(modifiers.alt) + 4 * u8::from(modifiers.ctrl)
}

/// Flèches, Début et Fin : `CSI lettre`, `SS3 lettre` en mode application, ou
/// `CSI 1;mod lettre` avec un modificateur.
fn cursor_key(letter: char, modifiers: Modifiers, mode: TermMode) -> Vec<u8> {
    let code = modifier_code(modifiers);
    if code > 1 {
        format!("\x1b[1;{code}{letter}").into_bytes()
    } else if mode.contains(TermMode::APP_CURSOR) {
        format!("\x1bO{letter}").into_bytes()
    } else {
        format!("\x1b[{letter}").into_bytes()
    }
}

/// Touches de la forme `CSI n ~`, comme Page précédente ou Suppr.
fn tilde_key(number: u8, modifiers: Modifiers) -> Vec<u8> {
    let code = modifier_code(modifiers);
    if code > 1 {
        format!("\x1b[{number};{code}~").into_bytes()
    } else {
        format!("\x1b[{number}~").into_bytes()
    }
}

/// F1 à F4 : `SS3 lettre`, ou `CSI 1;mod lettre` avec un modificateur.
fn function_key(letter: char, modifiers: Modifiers) -> Vec<u8> {
    let code = modifier_code(modifiers);
    if code > 1 {
        format!("\x1b[1;{code}{letter}").into_bytes()
    } else {
        format!("\x1bO{letter}").into_bytes()
    }
}

/// Octet de contrôle de Ctrl + `key` (Ctrl+A : 1, Ctrl+[ : Échap…).
fn control_byte(key: Key) -> Option<u8> {
    let name = key.name();
    let mut letters = name.chars();
    if let (Some(letter), None) = (letters.next(), letters.next())
        && letter.is_ascii_alphabetic()
    {
        return Some(letter.to_ascii_uppercase() as u8 - b'A' + 1);
    }
    match key {
        Key::Space | Key::Num2 => Some(0),
        Key::OpenBracket | Key::Num3 => Some(0x1b),
        Key::Backslash | Key::Num4 => Some(0x1c),
        Key::CloseBracket | Key::Num5 => Some(0x1d),
        Key::Num6 => Some(0x1e),
        Key::Minus | Key::Slash | Key::Num7 => Some(0x1f),
        Key::Num8 => Some(0x7f),
        _ => None,
    }
}

/// Séquence d'une touche, si elle n'est pas transmise comme du texte.
pub fn key_bytes(key: Key, modifiers: Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    let escaped = |byte: u8| {
        if modifiers.alt {
            vec![0x1b, byte]
        } else {
            vec![byte]
        }
    };
    Some(match key {
        Key::Enter => escaped(b'\r'),
        Key::Backspace if modifiers.ctrl => escaped(0x08),
        Key::Backspace => escaped(0x7f),
        Key::Tab if modifiers.shift => b"\x1b[Z".to_vec(),
        Key::Tab => escaped(b'\t'),
        Key::Escape => escaped(0x1b),
        Key::ArrowUp => cursor_key('A', modifiers, mode),
        Key::ArrowDown => cursor_key('B', modifiers, mode),
        Key::ArrowRight => cursor_key('C', modifiers, mode),
        Key::ArrowLeft => cursor_key('D', modifiers, mode),
        Key::Home => cursor_key('H', modifiers, mode),
        Key::End => cursor_key('F', modifiers, mode),
        Key::Insert => tilde_key(2, modifiers),
        Key::Delete => tilde_key(3, modifiers),
        Key::PageUp => tilde_key(5, modifiers),
        Key::PageDown => tilde_key(6, modifiers),
        Key::F1 => function_key('P', modifiers),
        Key::F2 => function_key('Q', modifiers),
        Key::F3 => function_key('R', modifiers),
        Key::F4 => function_key('S', modifiers),
        Key::F5 => tilde_key(15, modifiers),
        Key::F6 => tilde_key(17, modifiers),
        Key::F7 => tilde_key(18, modifiers),
        Key::F8 => tilde_key(19, modifiers),
        Key::F9 => tilde_key(20, modifiers),
        Key::F10 => tilde_key(21, modifiers),
        Key::F11 => tilde_key(23, modifiers),
        Key::F12 => tilde_key(24, modifiers),
        key if modifiers.ctrl => escaped(control_byte(key)?),
        _ => return None,
    })
}

/// Ce que la souris signale au programme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press(u8),
    Release(u8),
    /// Mouvement, bouton enfoncé ou non.
    Motion(Option<u8>),
    WheelUp,
    WheelDown,
}

/// Rapport de souris au format demandé par le programme : SGR, sinon le format d'origine
/// de xterm, qui ne sait pas décrire au-delà de la colonne 222.
pub fn mouse_report(
    action: MouseAction,
    column: usize,
    line: usize,
    modifiers: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let held = |button: u8| 32 + button;
    let (button, released) = match action {
        MouseAction::Press(button) => (button, false),
        MouseAction::Release(button) => (button, true),
        MouseAction::Motion(Some(button)) => (held(button), false),
        MouseAction::Motion(None) => (35, false),
        MouseAction::WheelUp => (64, false),
        MouseAction::WheelDown => (65, false),
    };
    let code =
        button + 4 * u8::from(modifiers.shift) + 8 * u8::from(modifiers.alt) + 16 * u8::from(modifiers.ctrl);
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if released { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{};{}{end}", column + 1, line + 1).into_bytes());
    }
    if column > 222 || line > 222 {
        return None;
    }
    // Ce format ne dit pas quel bouton est relâché.
    let code = if released { 3 + (code & !3) } else { code };
    Some(vec![
        0x1b,
        b'[',
        b'M',
        32 + code,
        33 + column as u8,
        33 + line as u8,
    ])
}

/// Texte collé : entre marqueurs si le programme les comprend (il ne l'exécute alors pas au
/// fil de l'eau), sans échappements, pour qu'un texte ne puisse pas en sortir.
pub fn paste_bytes(text: &str, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        let clean = text.replace(['\x1b', '\u{9b}'], "");
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// Texte de recherche, pris à la lettre par le moteur d'expressions régulières.
pub fn escape_regex(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if r"\.+*?()|[]{}^$#&-~".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// Adresse web de `line` à la colonne `column`, pour Ctrl+clic.
pub fn url_at(line: &str, column: usize) -> Option<String> {
    let characters: Vec<char> = line.chars().collect();
    let is_url_char = |c: char| !c.is_whitespace() && !"<>\"'`".contains(c);
    if !characters.get(column).copied().is_some_and(is_url_char) {
        return None;
    }
    let start = characters[..column]
        .iter()
        .rposition(|c| !is_url_char(*c))
        .map_or(0, |index| index + 1);
    let end = characters[column..]
        .iter()
        .position(|c| !is_url_char(*c))
        .map_or(characters.len(), |index| column + index);
    let word: String = characters[start..end].iter().collect();
    let begin = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| word.find(scheme))
        .min()?;
    let url = word[begin..].trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '!', '?']);
    // La colonne doit tomber dans l'adresse, pas dans ce qui la précède.
    let before = word[..begin].chars().count();
    let length = url.chars().count();
    (column >= start + before && column < start + before + length).then(|| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;
    const CTRL: Modifiers = Modifiers::CTRL;
    const SHIFT: Modifiers = Modifiers::SHIFT;
    const ALT: Modifiers = Modifiers::ALT;

    fn bytes(key: Key, modifiers: Modifiers) -> Vec<u8> {
        key_bytes(key, modifiers, TermMode::NONE).unwrap()
    }

    #[test]
    fn touches_de_base() {
        assert_eq!(bytes(Key::Enter, NONE), b"\r");
        assert_eq!(bytes(Key::Backspace, NONE), [0x7f]);
        assert_eq!(bytes(Key::Backspace, CTRL), [0x08]);
        assert_eq!(bytes(Key::Tab, SHIFT), b"\x1b[Z");
        assert_eq!(bytes(Key::Escape, NONE), [0x1b]);
        assert_eq!(bytes(Key::Delete, NONE), b"\x1b[3~");
        assert_eq!(bytes(Key::PageDown, CTRL), b"\x1b[6;5~");
        assert_eq!(bytes(Key::F1, NONE), b"\x1bOP");
        assert_eq!(bytes(Key::F5, NONE), b"\x1b[15~");
        assert_eq!(bytes(Key::F12, SHIFT), b"\x1b[24;2~");
    }

    #[test]
    fn le_mode_application_change_les_fleches() {
        assert_eq!(bytes(Key::ArrowUp, NONE), b"\x1b[A");
        assert_eq!(
            key_bytes(Key::ArrowUp, NONE, TermMode::APP_CURSOR).unwrap(),
            b"\x1bOA"
        );
        // Un modificateur l'emporte sur le mode application.
        assert_eq!(
            key_bytes(Key::ArrowLeft, CTRL, TermMode::APP_CURSOR).unwrap(),
            b"\x1b[1;5D"
        );
    }

    #[test]
    fn controle_et_alt() {
        assert_eq!(bytes(Key::C, CTRL), [3]);
        assert_eq!(bytes(Key::Z, CTRL), [26]);
        assert_eq!(bytes(Key::OpenBracket, CTRL), [0x1b]);
        assert_eq!(bytes(Key::Space, CTRL), [0]);
        assert_eq!(bytes(Key::Enter, ALT), b"\x1b\r");
        // Une simple lettre passe par le texte, pour respecter la disposition du clavier.
        assert_eq!(key_bytes(Key::A, NONE, TermMode::NONE), None);
    }

    #[test]
    fn rapports_de_souris() {
        let sgr = TermMode::SGR_MOUSE;
        assert_eq!(
            mouse_report(MouseAction::Press(0), 4, 9, NONE, sgr).unwrap(),
            b"\x1b[<0;5;10M"
        );
        assert_eq!(
            mouse_report(MouseAction::Release(2), 0, 0, NONE, sgr).unwrap(),
            b"\x1b[<2;1;1m"
        );
        assert_eq!(
            mouse_report(MouseAction::WheelDown, 1, 1, CTRL, sgr).unwrap(),
            b"\x1b[<81;2;2M"
        );
        assert_eq!(
            mouse_report(MouseAction::Motion(Some(0)), 1, 1, NONE, sgr).unwrap(),
            b"\x1b[<32;2;2M"
        );
        let old = TermMode::NONE;
        assert_eq!(
            mouse_report(MouseAction::Press(0), 0, 0, NONE, old).unwrap(),
            [0x1b, b'[', b'M', 32, 33, 33]
        );
        assert_eq!(mouse_report(MouseAction::Press(0), 300, 0, NONE, old), None);
    }

    #[test]
    fn coller_sans_laisser_sortir_du_marqueur() {
        assert_eq!(paste_bytes("a\nb", TermMode::NONE), b"a\rb");
        assert_eq!(
            paste_bytes("ls\x1b[201~; rm", TermMode::BRACKETED_PASTE),
            b"\x1b[200~ls[201~; rm\x1b[201~"
        );
    }

    #[test]
    fn la_recherche_prend_le_texte_a_la_lettre() {
        assert_eq!(escape_regex("a.b*c"), r"a\.b\*c");
        assert_eq!(escape_regex("été"), "été");
    }

    #[test]
    fn adresse_sous_le_pointeur() {
        let line = "voir (https://ferrite.dev/a?b=1), puis";
        assert_eq!(url_at(line, 10).as_deref(), Some("https://ferrite.dev/a?b=1"));
        assert_eq!(url_at(line, 2), None);
        // Ni la parenthèse d'avant ni la ponctuation d'après.
        assert_eq!(url_at(line, 5), None);
        assert_eq!(url_at(line, 31), None);
        assert_eq!(url_at(line, 500), None);
        assert_eq!(url_at("pas d'adresse", 1), None);
    }
}
