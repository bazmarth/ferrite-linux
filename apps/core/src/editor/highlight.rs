//! Coloration syntaxique légère, ligne à ligne pour le Markdown et par jetons pour le
//! code : assez pour lire un fichier, sans analyseur complet. Les fragments ne commencent
//! et ne finissent que sur des octets ASCII, donc toujours entre deux caractères.

use std::ops::Range;

use eframe::egui::{
    Color32, FontId,
    text::{LayoutJob, TextFormat},
};

use super::icons::FileKind;
use crate::style;

/// Rôle d'un fragment de texte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Heading,
    Strong,
    Emphasis,
    Key,
    Value,
    String,
    Number,
    Keyword,
    Comment,
    Muted,
    Code,
    Link,
    Marker,
    Function,
    Type,
}

/// Fond d'une occurrence de la recherche, plus fort pour l'occurrence courante.
fn match_background(current: bool) -> Color32 {
    match current {
        false => shade((0xFF, 0xEE, 0xA0), (0x5C, 0x4A, 0x00)),
        true => shade((0xFF, 0xBE, 0x4D), (0x9A, 0x6A, 0x00)),
    }
}

/// Couleur des clés et du code en ligne.
pub fn code_color() -> Color32 {
    shade((0xC8, 0x36, 0x6B), (0xFF, 0x7A, 0xA2))
}

fn shade(light: (u8, u8, u8), dark: (u8, u8, u8)) -> Color32 {
    let (r, g, b) = style::themed(light, dark);
    Color32::from_rgb(r, g, b)
}

fn format(kind: Kind, font: &FontId) -> TextFormat {
    let color = match kind {
        Kind::Plain | Kind::Emphasis => style::primary_text(),
        Kind::Heading | Kind::Strong | Kind::Value | Kind::Link | Kind::Function => style::accent(),
        Kind::Key | Kind::Code => code_color(),
        Kind::String => shade((0x1F, 0x7F, 0x4C), (0x7F, 0xD6, 0xA0)),
        Kind::Number => shade((0xB5, 0x53, 0x1A), (0xF0, 0xA3, 0x6B)),
        Kind::Keyword => shade((0x8B, 0x3F, 0xD9), (0xC3, 0x9B, 0xFF)),
        Kind::Comment => shade((0x8C, 0x93, 0x9D), (0x8C, 0x93, 0x9D)),
        Kind::Muted => shade((0x9A, 0xA1, 0xAC), (0x7A, 0x81, 0x8C)),
        Kind::Marker => style::accent(),
        Kind::Type => shade((0x0F, 0x7B, 0x8A), (0x5C, 0xC8, 0xD6)),
    };
    let mut format = TextFormat::simple(font.clone(), color);
    format.italics = matches!(kind, Kind::Emphasis | Kind::Comment);
    if kind == Kind::Code {
        format.background = style::content_gray();
    }
    format
}

/// Mise en page colorée de `text`, avec les occurrences `matches` de la recherche
/// surlignées (`current` plus fort).
pub fn layout_job(
    text: &str,
    kind: FileKind,
    font: &FontId,
    matches: &[Range<usize>],
    current: usize,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut m = 0;
    for (range, role) in covering(text, spans(text, kind)) {
        let mut start = range.start;
        while start < range.end {
            while matches.get(m).is_some_and(|found| found.end <= start) {
                m += 1;
            }
            let (end, background) = match matches.get(m) {
                Some(found) if found.start <= start => {
                    (found.end.min(range.end), Some(match_background(m == current)))
                }
                Some(found) if found.start < range.end => (found.start, None),
                _ => (range.end, None),
            };
            let mut format = format(role, font);
            if let Some(background) = background {
                format.background = background;
            }
            job.append(&text[start..end], 0.0, format);
            start = end;
        }
    }
    crate::style::emoji_presentation(&mut job);
    job
}

/// Fragments `spans` (dans l'ordre, sans chevauchement) complétés de texte simple pour
/// couvrir tout `text`.
fn covering(text: &str, spans: Vec<(Range<usize>, Kind)>) -> Vec<(Range<usize>, Kind)> {
    let mut out = Vec::with_capacity(spans.len() * 2 + 1);
    let mut at = 0;
    for (range, kind) in spans {
        if range.start < at || range.start >= range.end {
            continue;
        }
        if range.start > at {
            out.push((at..range.start, Kind::Plain));
        }
        at = range.end;
        out.push((range, kind));
    }
    if at < text.len() {
        out.push((at..text.len(), Kind::Plain));
    }
    out
}

/// Fragments colorés de `text`, selon le type de fichier.
pub fn spans(text: &str, kind: FileKind) -> Vec<(Range<usize>, Kind)> {
    let mut out = Vec::new();
    match kind {
        FileKind::Markdown => markdown(text, &mut out),
        FileKind::Text | FileKind::Folder | FileKind::Image => {}
        _ => code(text, 0, kind, &mut out),
    }
    out
}

/// Blocs de code d'un texte Markdown (de la ligne « ``` » d'ouverture à celle de
/// fermeture comprises), en octets : l'éditeur leur peint un fond.
pub fn code_blocks(text: &str) -> Vec<Range<usize>> {
    let mut blocks = Vec::new();
    let mut open: Option<(usize, &str)> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim();
        match open {
            Some((from, mark)) if trimmed.starts_with(mark) => {
                blocks.push(from..offset);
                open = None;
            }
            Some(_) => {}
            None => open = fence(trimmed).map(|mark| (start, mark)),
        }
    }
    if let Some((from, _)) = open {
        blocks.push(from..text.len());
    }
    blocks
}

fn fence(trimmed: &str) -> Option<&'static str> {
    if trimmed.starts_with("```") {
        Some("```")
    } else if trimmed.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    }
}

fn markdown(text: &str, out: &mut Vec<(Range<usize>, Kind)>) {
    let mut offset = 0;
    let mut front = text.starts_with("---\n") || text.starts_with("---\r\n");
    let mut first = true;
    // Bloc de code ouvert : marque, langue et début de son contenu.
    let mut open: Option<(&str, FileKind, usize)> = None;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let content = line.trim_end_matches(['\n', '\r']);
        let end = start + content.len();
        let trimmed = content.trim();
        let indent = content.len() - content.trim_start().len();
        if front {
            if trimmed == "---" && !first {
                front = false;
                out.push((start..end, Kind::Muted));
            } else if let Some(colon) = content.find(':')
                && !first
            {
                out.push((start..start + colon + 1, Kind::Key));
                out.push((start + colon + 1..end, Kind::Value));
            } else {
                out.push((start..end, Kind::Muted));
            }
            first = false;
            continue;
        }
        first = false;
        if let Some((mark, language, from)) = open {
            if trimmed.starts_with(mark) {
                code(&text[from..start], from, language, out);
                out.push((start..end, Kind::Muted));
                open = None;
            }
            continue;
        }
        if let Some(mark) = fence(trimmed) {
            let language = trimmed[3..].trim();
            let language = FileKind::of(std::path::Path::new(&format!("x.{}", fence_extension(language))));
            out.push((start..end, Kind::Muted));
            open = Some((mark, language, offset));
            continue;
        }
        let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            out.push((start..end, Kind::Heading));
        } else if trimmed.starts_with('>') {
            out.push((start..end, Kind::Muted));
        } else if let Some(marker) = list_marker(&content[indent..]) {
            let marker_end = start + indent + marker;
            out.push((start + indent..marker_end, Kind::Marker));
            inline(&text[marker_end..end], marker_end, out);
        } else {
            inline(content, start, out);
        }
    }
    if let Some((_, language, from)) = open {
        code(&text[from..], from, language, out);
    }
}

/// Extension de fichier correspondant à la langue d'un bloc de code.
fn fence_extension(language: &str) -> &str {
    match language {
        "bash" | "shell" | "console" | "zsh" => "sh",
        "rust" => "rs",
        "javascript" => "js",
        "typescript" => "ts",
        "python" => "py",
        other => other,
    }
}

/// Longueur de la puce « - », « * », « + » ou « 1. » (espace compris) en tête de `line`.
fn list_marker(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    if matches!(bytes.first(), Some(b'-' | b'*' | b'+')) && bytes.get(1) == Some(&b' ') {
        return Some(2);
    }
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    (digits > 0 && matches!(bytes.get(digits), Some(b'.' | b')')) && bytes.get(digits + 1) == Some(&b' '))
        .then_some(digits + 2)
}

/// Styles en ligne : `code`, **gras**, *italique*, [liens](adresse).
fn inline(text: &str, base: usize, out: &mut Vec<(Range<usize>, Kind)>) {
    let bytes = text.as_bytes();
    let find = |from: usize, needle: &[u8]| {
        bytes[from..]
            .windows(needle.len())
            .position(|window| window == needle)
            .map(|at| from + at)
    };
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        if rest[0] == b'[' || rest.starts_with(b"![") {
            let open = i + usize::from(rest[0] == b'!');
            if let Some(middle) = find(open, b"](")
                && let Some(close) = find(middle + 2, b")")
            {
                out.push((base + i..base + middle + 1, Kind::Link));
                out.push((base + middle + 1..base + close + 1, Kind::Value));
                i = close + 1;
                continue;
            }
        }
        let token = if rest[0] == b'`' {
            find(i + 1, b"`").map(|close| (close + 1, Kind::Code))
        } else if rest.starts_with(b"**") || rest.starts_with(b"__") {
            find(i + 2, &rest[..2]).map(|close| (close + 2, Kind::Strong))
        } else if matches!(rest[0], b'*' | b'_')
            && rest.get(1).is_some_and(|b| !b.is_ascii_whitespace())
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
        {
            find(i + 1, &rest[..1]).map(|close| (close + 1, Kind::Emphasis))
        } else {
            None
        };
        match token {
            Some((end, kind)) => {
                out.push((base + i..base + end, kind));
                i = end;
            }
            None => i += 1,
        }
    }
}

/// Règles d'un langage de programmation.
struct Syntax {
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    quotes: &'static [u8],
    keywords: &'static [&'static str],
    /// Un mot suivi de ce signe est une clé (CSS, TOML, YAML).
    key_before: Option<u8>,
    /// Les mots en majuscule sont des types.
    types: bool,
}

const RUST: Syntax = Syntax {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: b"\"",
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "false", "fn",
        "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
        "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
        "while",
    ],
    key_before: None,
    types: true,
};

const SCRIPT: Syntax = Syntax {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: b"\"'`",
    keywords: &[
        "as",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "default",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "from",
        "function",
        "if",
        "implements",
        "import",
        "in",
        "instanceof",
        "interface",
        "let",
        "new",
        "null",
        "of",
        "private",
        "protected",
        "public",
        "readonly",
        "return",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "type",
        "typeof",
        "undefined",
        "var",
        "while",
        "yield",
    ],
    key_before: None,
    types: true,
};

const JSON: Syntax = Syntax {
    line_comments: &["//"],
    block_comment: None,
    quotes: b"\"",
    keywords: &["true", "false", "null"],
    key_before: None,
    types: false,
};

const CSS: Syntax = Syntax {
    line_comments: &[],
    block_comment: Some(("/*", "*/")),
    quotes: b"\"'",
    keywords: &["important", "media", "import", "keyframes", "root"],
    key_before: Some(b':'),
    types: false,
};

const SHELL: Syntax = Syntax {
    line_comments: &["#"],
    block_comment: None,
    quotes: b"\"'",
    keywords: &[
        "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function", "if", "in", "local",
        "return", "then", "while",
    ],
    key_before: None,
    types: false,
};

const PYTHON: Syntax = Syntax {
    line_comments: &["#"],
    block_comment: None,
    quotes: b"\"'",
    keywords: &[
        "and", "as", "async", "await", "break", "class", "continue", "def", "elif", "else", "except",
        "False", "finally", "for", "from", "if", "import", "in", "is", "lambda", "None", "not", "or", "pass",
        "raise", "return", "self", "True", "try", "while", "with", "yield",
    ],
    key_before: None,
    types: false,
};

const TOML: Syntax = Syntax {
    line_comments: &["#"],
    block_comment: None,
    quotes: b"\"'",
    keywords: &["true", "false"],
    key_before: Some(b'='),
    types: false,
};

const YAML: Syntax = Syntax {
    line_comments: &["#"],
    block_comment: None,
    quotes: b"\"'",
    keywords: &["true", "false", "null", "yes", "no"],
    key_before: Some(b':'),
    types: false,
};

const HTML: Syntax = Syntax {
    line_comments: &[],
    block_comment: Some(("<!--", "-->")),
    quotes: b"\"'",
    keywords: &[],
    key_before: Some(b'='),
    types: false,
};

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

/// Jetons de code de `text`, qui commence à l'octet `base` du document.
fn code(text: &str, base: usize, kind: FileKind, out: &mut Vec<(Range<usize>, Kind)>) {
    let syntax = match kind {
        FileKind::Rust => &RUST,
        FileKind::React | FileKind::TypeScript | FileKind::JavaScript => &SCRIPT,
        FileKind::Json => &JSON,
        FileKind::Css => &CSS,
        FileKind::Shell => &SHELL,
        FileKind::Python => &PYTHON,
        FileKind::Toml => &TOML,
        FileKind::Yaml => &YAML,
        FileKind::Html => &HTML,
        FileKind::Markdown | FileKind::Text | FileKind::Folder | FileKind::Image => return,
    };
    let bytes = text.as_bytes();
    let next_visible = |from: usize| bytes[from..].iter().copied().find(|b| *b != b' ' && *b != b'\t');
    let line_start = |at: usize| {
        bytes[..at]
            .iter()
            .rev()
            .take_while(|b| **b != b'\n')
            .all(|b| b.is_ascii_whitespace())
    };
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        let byte = rest[0];
        let push = |out: &mut Vec<_>, end: usize, role| out.push((base + i..base + end, role));
        if let Some((open, close)) = syntax.block_comment
            && rest.starts_with(open.as_bytes())
        {
            let end = rest[open.len()..]
                .windows(close.len())
                .position(|window| window == close.as_bytes())
                .map_or(bytes.len(), |at| i + open.len() + at + close.len());
            push(out, end, Kind::Comment);
            i = end;
        } else if syntax
            .line_comments
            .iter()
            .any(|mark| rest.starts_with(mark.as_bytes()))
            && (byte != b'#' || i == 0 || bytes[i - 1].is_ascii_whitespace())
        {
            let end = rest
                .iter()
                .position(|&b| b == b'\n')
                .map_or(bytes.len(), |at| i + at);
            push(out, end, Kind::Comment);
            i = end;
        } else if syntax.quotes.contains(&byte) {
            let mut end = i + 1;
            while end < bytes.len() && bytes[end] != byte && (byte == b'`' || bytes[end] != b'\n') {
                end += if bytes[end] == b'\\' { 2 } else { 1 };
            }
            let end = (end + 1).min(bytes.len());
            // Une chaîne suivie de « : » est une clé (JSON, objets).
            let role = if kind == FileKind::Json && end < bytes.len() && next_visible(end) == Some(b':') {
                Kind::Key
            } else {
                Kind::String
            };
            push(out, end, role);
            i = end;
        } else if byte.is_ascii_digit() && (i == 0 || !is_word(bytes[i - 1])) {
            let end = i + rest
                .iter()
                .position(|&b| !(is_word(b) || b == b'.'))
                .unwrap_or(rest.len());
            push(out, end, Kind::Number);
            i = end;
        } else if kind == FileKind::Html
            && byte == b'<'
            && rest.get(1).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'/')
        {
            let from = i + if rest[1] == b'/' { 2 } else { 1 };
            let end = from
                + bytes[from..]
                    .iter()
                    .position(|&b| !(is_word(b) || b == b'-'))
                    .unwrap_or(bytes.len() - from);
            out.push((base + from..base + end, Kind::Keyword));
            i = end;
        } else if is_word(byte) && (i == 0 || !is_word(bytes[i - 1])) {
            let end = i + rest
                .iter()
                .position(|&b| !(is_word(b) || (syntax.key_before.is_some() && b == b'-')))
                .unwrap_or(rest.len());
            let word = &text[i..end];
            let after = next_visible(end);
            let role = if syntax.keywords.contains(&word) {
                Some(Kind::Keyword)
            } else if syntax.key_before.is_some() && after == syntax.key_before {
                Some(Kind::Key)
            } else if after == Some(b'(') || (kind == FileKind::Shell && line_start(i)) {
                Some(Kind::Function)
            } else if syntax.types && byte.is_ascii_uppercase() {
                Some(Kind::Type)
            } else {
                None
            };
            if let Some(role) = role {
                push(out, end, role);
            }
            i = end;
        } else if (kind == FileKind::Toml && byte == b'[' && line_start(i))
            || (kind == FileKind::React
                && byte == b'<'
                && rest.get(1).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'/'))
        {
            // Section TOML « [nom] », ou balise JSX « <Composant ».
            let stop = if byte == b'[' { b']' } else { b' ' };
            let end = i + rest
                .iter()
                .position(|&b| b == stop || b == b'\n' || b == b'>')
                .map_or(rest.len(), |at| at + usize::from(stop == b']'));
            push(out, end, Kind::Type);
            i = end;
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(text: &str, kind: FileKind) -> Vec<(&str, Kind)> {
        spans(text, kind)
            .into_iter()
            .map(|(range, role)| (&text[range], role))
            .collect()
    }

    #[test]
    fn le_markdown_colore_entete_titres_et_code() {
        let text = "---\ntitle: TextEdit\n---\n# Titre\n- **gras** et `code`\n```bash\n# commentaire\nsudo dnf\n```\n";
        let found = roles(text, FileKind::Markdown);
        assert!(found.contains(&("title:", Kind::Key)));
        assert!(found.contains(&(" TextEdit", Kind::Value)));
        assert!(found.contains(&("# Titre", Kind::Heading)));
        assert!(found.contains(&("- ", Kind::Marker)));
        assert!(found.contains(&("**gras**", Kind::Strong)));
        assert!(found.contains(&("`code`", Kind::Code)));
        assert!(found.contains(&("# commentaire", Kind::Comment)));
        assert_eq!(code_blocks(text).len(), 1);
    }

    #[test]
    fn le_code_distingue_mots_cles_chaines_et_cles() {
        let found = roles("fn main() { let s = \"été\"; } // fin", FileKind::Rust);
        assert!(found.contains(&("fn", Kind::Keyword)));
        assert!(found.contains(&("main", Kind::Function)));
        assert!(found.contains(&("\"été\"", Kind::String)));
        assert!(found.contains(&("// fin", Kind::Comment)));
        let json = roles("{ \"nom\": \"ferrite\", \"version\": 2 }", FileKind::Json);
        assert!(json.contains(&("\"nom\"", Kind::Key)));
        assert!(json.contains(&("\"ferrite\"", Kind::String)));
        assert!(json.contains(&("2", Kind::Number)));
    }

    #[test]
    fn les_occurrences_coupent_les_fragments_sans_perdre_de_texte() {
        let text = "# Été été";
        let job = layout_job(
            text,
            FileKind::Markdown,
            &FontId::monospace(13.0),
            &[2..7, 8..13],
            1,
        );
        assert_eq!(job.text, text);
        assert_eq!(
            job.sections.last().map(|section| section.format.background),
            Some(match_background(true))
        );
    }
}
