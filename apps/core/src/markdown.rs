//! Interpréteur Markdown : blocs (titres, paragraphes, listes, citations,
//! blocs de code, règles, tableaux, images) et styles en ligne (gras, italique,
//! code, liens, barré). Aussi : l'en-tête YAML (« --- ») en tête de document.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Styles d'un fragment de texte (bits).
pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const CODE: u8 = 4;
pub const LINK: u8 = 8;
pub const STRIKE: u8 = 16;

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: u8,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Marker {
    Bullet,
    Number(usize),
    /// Case à cocher (« - [x] »).
    Task(bool),
}

#[derive(Debug)]
pub struct Item {
    /// Niveau d'imbrication (0 = premier niveau).
    pub level: usize,
    pub marker: Marker,
    pub text: Vec<Span>,
}

#[derive(Debug)]
pub enum Block {
    Heading(usize, Vec<Span>),
    Paragraph(Vec<Span>),
    Aligned(Align, Vec<Span>),
    /// Bloc de code, avec la langue annoncée après « ``` » (vide sinon).
    Code {
        language: String,
        lines: Vec<String>,
    },
    /// Image seule sur sa ligne : « ![texte](chemin) ».
    Image {
        alt: String,
        path: String,
    },
    Quote(Vec<Block>),
    List(Vec<Item>),
    Rule,
    Table {
        align: Vec<Align>,
        header: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
    },
}

// ----- Blocs ---------------------------------------------------------------------------

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|&c| c == ' ').count()
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn fence(line: &str) -> Option<&str> {
    let t = line.trim_start();
    (t.starts_with("```") || t.starts_with("~~~")).then(|| &t[..3])
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    if indent_of(line) > 3 {
        return None;
    }
    let level = t.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &t[level..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    // Les « # » de fermeture sont facultatifs.
    let rest = rest.trim();
    let rest = rest.trim_end_matches('#');
    Some((level, rest.trim_end()))
}

fn is_rule(line: &str) -> bool {
    let t: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && indent_of(line) < 4 && matches!(t[0], '-' | '*' | '_') && t.iter().all(|&c| c == t[0])
}

/// Élément de liste : (indentation, marqueur, texte).
fn list_item(line: &str) -> Option<(usize, Marker, &str)> {
    let indent = indent_of(line);
    let t = &line[indent..];
    let (marker, rest) = if let Some(rest) = t
        .strip_prefix("- ")
        .or(t.strip_prefix("* "))
        .or(t.strip_prefix("+ "))
    {
        (Marker::Bullet, rest)
    } else if t == "-" || t == "*" || t == "+" {
        (Marker::Bullet, "")
    } else {
        let digits = t.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 || digits > 9 {
            return None;
        }
        let after = &t[digits..];
        let rest = after.strip_prefix(". ").or(after.strip_prefix(") "))?;
        (Marker::Number(t[..digits].parse().ok()?), rest)
    };
    if marker == Marker::Bullet {
        for (prefix, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
            if let Some(rest) = rest.strip_prefix(prefix) {
                return Some((indent, Marker::Task(done), rest));
            }
        }
    }
    Some((indent, marker, rest))
}

/// Cellules d'une ligne de tableau (« | a | b | »).
fn cells(line: &str) -> Vec<&str> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(str::trim).collect()
}

/// Ligne de séparation d'un tableau (« |---|:--:| ») : alignement des colonnes.
fn table_delimiter(line: &str) -> Option<Vec<Align>> {
    if !line.contains('-') || !line.contains('|') {
        return None;
    }
    cells(line)
        .into_iter()
        .map(|cell| {
            let left = cell.starts_with(':');
            let right = cell.ends_with(':');
            let dashes = cell.trim_matches(':');
            if dashes.is_empty() || !dashes.chars().all(|c| c == '-') {
                return None;
            }
            Some(match (left, right) {
                (true, true) => Align::Center,
                (false, true) => Align::Right,
                _ => Align::Left,
            })
        })
        .collect()
}

pub fn parse(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.split('\n').collect();
    parse_lines(&lines)
}

fn parse_lines(lines: &[&str]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut i = 0;
    let mut paragraph: Vec<&str> = Vec::new();
    let flush = |paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(inline_lines(paragraph)));
            paragraph.clear();
        }
    };
    while i < lines.len() {
        let line = lines[i];
        if is_blank(line) {
            flush(&mut paragraph, &mut blocks);
            i += 1;
            continue;
        }
        // Titre souligné (« === » ou « --- » sous un paragraphe).
        if !paragraph.is_empty() {
            let t = line.trim();
            if !t.is_empty() && (t.chars().all(|c| c == '=') || t.chars().all(|c| c == '-')) {
                let level = if t.starts_with('=') { 1 } else { 2 };
                let spans = inline_lines(&paragraph);
                paragraph.clear();
                blocks.push(Block::Heading(level, spans));
                i += 1;
                continue;
            }
        }
        if let Some(mark) = fence(line) {
            flush(&mut paragraph, &mut blocks);
            let language = String::from(line.trim_start()[3..].trim());
            let mut code = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with(mark) {
                code.push(String::from(lines[i]));
                i += 1;
            }
            i += 1;
            blocks.push(Block::Code {
                language,
                lines: code,
            });
            continue;
        }
        if paragraph.is_empty() && indent_of(line) >= 4 && list_item(line).is_none() {
            let mut code = Vec::new();
            while i < lines.len() && (indent_of(lines[i]) >= 4 || is_blank(lines[i])) {
                code.push(String::from(lines[i].get(4..).unwrap_or("")));
                i += 1;
            }
            while code.last().is_some_and(|l| l.trim().is_empty()) {
                code.pop();
            }
            blocks.push(Block::Code {
                language: String::new(),
                lines: code,
            });
            continue;
        }
        let aligned_start = line.trim().strip_prefix("::: align=");
        if let Some(rest) = aligned_start {
            flush(&mut paragraph, &mut blocks);
            let alignment = if rest.starts_with("center") {
                Align::Center
            } else if rest.starts_with("right") {
                Align::Right
            } else {
                Align::Left
            };
            i += 1;
            let mut text = Vec::new();
            while i < lines.len() && lines[i].trim() != ":::" {
                text.push(lines[i]);
                i += 1;
            }
            if i < lines.len() {
                i += 1;
            }
            let spans = inline_lines(&text);
            blocks.push(Block::Aligned(alignment, spans));
            continue;
        }
        if let Some((level, title)) = heading(line) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Heading(level, inline(title)));
            i += 1;
            continue;
        }
        if is_rule(line) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Rule);
            i += 1;
            continue;
        }
        if line.trim_start().starts_with('>') {
            flush(&mut paragraph, &mut blocks);
            let mut inner = Vec::new();
            while i < lines.len() && lines[i].trim_start().starts_with('>') {
                let t = &lines[i].trim_start()[1..];
                inner.push(t.strip_prefix(' ').unwrap_or(t));
                i += 1;
            }
            blocks.push(Block::Quote(parse_lines(&inner)));
            continue;
        }
        if let Some(align) = (line.contains('|') && i + 1 < lines.len())
            .then(|| table_delimiter(lines[i + 1]))
            .flatten()
        {
            flush(&mut paragraph, &mut blocks);
            let header: Vec<Vec<Span>> = cells(line).into_iter().map(inline).collect();
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() && lines[i].contains('|') && !is_blank(lines[i]) {
                let mut row: Vec<Vec<Span>> = cells(lines[i]).into_iter().map(inline).collect();
                row.resize(header.len(), Vec::new());
                rows.push(row);
                i += 1;
            }
            let mut align = align;
            align.resize(header.len(), Align::Left);
            blocks.push(Block::Table { align, header, rows });
            continue;
        }
        if list_item(line).is_some()
            && (paragraph.is_empty() || !matches!(list_item(line), Some((_, Marker::Number(n), _)) if n != 1))
        {
            flush(&mut paragraph, &mut blocks);
            let mut items: Vec<Item> = Vec::new();
            // Indentations rencontrées, pour calculer les niveaux.
            let mut indents: Vec<usize> = Vec::new();
            let mut text: Vec<String> = Vec::new();
            let mut current: Option<(usize, Marker)> = None;
            let finish =
                |current: &mut Option<(usize, Marker)>, text: &mut Vec<String>, items: &mut Vec<Item>| {
                    if let Some((level, marker)) = current.take() {
                        let joined: Vec<&str> = text.iter().map(String::as_str).collect();
                        items.push(Item {
                            level,
                            marker,
                            text: inline_lines(&joined),
                        });
                        text.clear();
                    }
                };
            while i < lines.len() {
                let line = lines[i];
                if let Some((indent, marker, rest)) = list_item(line) {
                    finish(&mut current, &mut text, &mut items);
                    while indents.last().is_some_and(|&last| last > indent) {
                        indents.pop();
                    }
                    if indents.last() != Some(&indent) {
                        indents.push(indent);
                    }
                    current = Some((indents.len() - 1, marker));
                    text.push(String::from(rest));
                } else if is_blank(line) {
                    // Une ligne vide termine la liste, sauf si un élément suit.
                    if lines.get(i + 1).is_some_and(|next| list_item(next).is_some()) {
                        i += 1;
                        continue;
                    }
                    break;
                } else if indent_of(line) > 0
                    || !(heading(line).is_some()
                        || is_rule(line)
                        || fence(line).is_some()
                        || line.trim_start().starts_with('>'))
                {
                    // Suite de l'élément sur la ligne suivante.
                    text.push(String::from(line.trim()));
                } else {
                    break;
                }
                i += 1;
            }
            finish(&mut current, &mut text, &mut items);
            blocks.push(Block::List(items));
            continue;
        }
        if paragraph.is_empty()
            && let Some((alt, path)) = image(line)
        {
            blocks.push(Block::Image { alt, path });
            i += 1;
            continue;
        }
        paragraph.push(line);
        i += 1;
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

/// Image seule sur la ligne : (texte de remplacement, chemin).
fn image(line: &str) -> Option<(String, String)> {
    let chars: Vec<char> = line.trim().chars().collect();
    if chars.first() != Some(&'!') {
        return None;
    }
    let (start, end, after) = link_at(&chars, 1)?;
    (after == chars.len()).then(|| {
        let alt = chars[start..end].iter().collect();
        let path: String = chars[end + 2..after - 1].iter().collect();
        // « chemin "titre" » : le titre est ignoré.
        let path = path.split_whitespace().next().unwrap_or("");
        (alt, String::from(path))
    })
}

/// En-tête YAML « --- … --- » en tête de document : ses paires « clé: valeur » (guillemets
/// ôtés) et le reste du texte. Sans en-tête : rien, et tout le texte.
pub fn front_matter(text: &str) -> (Vec<(String, String)>, &str) {
    let Some(body) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (Vec::new(), text);
    };
    let mut offset = 0;
    let mut pairs = Vec::new();
    for line in body.split_inclusive('\n') {
        offset += line.len();
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            return (pairs, &body[offset..]);
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            pairs.push((String::from(key.trim()), String::from(value)));
        }
    }
    // Jamais refermé : ce n'était pas un en-tête.
    (Vec::new(), text)
}

/// Lignes d'un paragraphe : jointes par une espace ; deux espaces ou « \ »
/// en fin de ligne forcent un retour à la ligne.
fn inline_lines(lines: &[&str]) -> Vec<Span> {
    let mut text = String::new();
    for (i, line) in lines.iter().enumerate() {
        let hard = line.ends_with("  ") || line.ends_with('\\');
        let trimmed = line.trim();
        let trimmed = if hard {
            trimmed.trim_end_matches('\\')
        } else {
            trimmed
        };
        text.push_str(trimmed);
        if i + 1 < lines.len() {
            text.push(if hard { '\n' } else { ' ' });
        }
    }
    inline(&text)
}

// ----- En ligne ------------------------------------------------------------------------

pub fn inline(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    parse_inline(&chars, 0, &mut out);
    // Fusion des fragments voisins de même style.
    let mut merged: Vec<Span> = Vec::new();
    for span in out {
        match merged.last_mut() {
            Some(last) if last.style == span.style => last.text.push_str(&span.text),
            _ => merged.push(span),
        }
    }
    merged
}

fn push(out: &mut Vec<Span>, text: &str, style: u8) {
    if !text.is_empty() {
        out.push(Span {
            text: String::from(text),
            style,
        });
    }
}

/// Position du délimiteur fermant `delim` dans `chars[from..]`.
fn closing(chars: &[char], from: usize, delim: &[char]) -> Option<usize> {
    let n = delim.len();
    let mut i = from;
    while i + n <= chars.len() {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == '`' {
            // Le code en ligne n'est pas analysé.
            let run = chars[i..].iter().take_while(|&&c| c == '`').count();
            if let Some(end) = find_run(chars, i + run, run) {
                i = end + run;
                continue;
            }
        }
        if &chars[i..i + n] == delim
            && i > from
            && !chars[i - 1].is_whitespace()
            && chars.get(i + n) != Some(&delim[0])
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Prochaine suite d'exactement `run` accents graves à partir de `from`.
fn find_run(chars: &[char], from: usize, run: usize) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '`' {
            let len = chars[i..].iter().take_while(|&&c| c == '`').count();
            if len == run {
                return Some(i);
            }
            i += len;
        } else {
            i += 1;
        }
    }
    None
}

/// Lien « [texte](adresse) » commençant à `i` : (texte, fin).
fn link_at(chars: &[char], i: usize) -> Option<(usize, usize, usize)> {
    let mut depth = 0;
    let mut j = i;
    while j < chars.len() {
        match chars[j] {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            '\\' => j += 1,
            _ => {}
        }
        j += 1;
    }
    if j >= chars.len() || chars.get(j + 1) != Some(&'(') {
        return None;
    }
    let close = chars[j + 2..].iter().position(|&c| c == ')')? + j + 2;
    Some((i + 1, j, close + 1))
}

fn parse_inline(chars: &[char], style: u8, out: &mut Vec<Span>) {
    let mut text = String::new();
    let mut i = 0;
    let flush = |text: &mut String, out: &mut Vec<Span>| {
        push(out, text, style);
        text.clear();
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if chars.get(i + 1).is_some_and(|c| c.is_ascii_punctuation()) => {
                text.push(chars[i + 1]);
                i += 2;
                continue;
            }
            '\n' => {
                flush(&mut text, out);
                out.push(Span {
                    text: String::from("\n"),
                    style,
                });
                i += 1;
                continue;
            }
            '`' => {
                let run = chars[i..].iter().take_while(|&&c| c == '`').count();
                if let Some(end) = find_run(chars, i + run, run) {
                    flush(&mut text, out);
                    let code: String = chars[i + run..end].iter().collect();
                    let code = if code.starts_with(' ') && code.ends_with(' ') && !code.trim().is_empty() {
                        String::from(&code[1..code.len() - 1])
                    } else {
                        code
                    };
                    push(out, &code, style | CODE);
                    i = end + run;
                    continue;
                }
                text.extend(&chars[i..i + run]);
                i += run;
                continue;
            }
            '!' if chars.get(i + 1) == Some(&'[') => {
                if let Some((start, end, after)) = link_at(chars, i + 1) {
                    flush(&mut text, out);
                    let alt: String = chars[start..end].iter().collect();
                    push(out, &alloc::format!("[image: {alt}]"), style | ITALIC);
                    i = after;
                    continue;
                }
            }
            '[' => {
                if let Some((start, end, after)) = link_at(chars, i) {
                    flush(&mut text, out);
                    parse_inline(&chars[start..end], style | LINK, out);
                    i = after;
                    continue;
                }
            }
            '<' => {
                // Lien automatique « <https://...> ».
                if let Some(len) = chars[i + 1..].iter().position(|&c| c == '>') {
                    let inner: String = chars[i + 1..i + 1 + len].iter().collect();
                    if (inner.starts_with("http://") || inner.starts_with("https://") || inner.contains('@'))
                        && !inner.contains(' ')
                    {
                        flush(&mut text, out);
                        push(out, &inner, style | LINK);
                        i += len + 2;
                        continue;
                    }
                }
            }
            '*' | '_' | '~' => {
                let run = chars[i..].iter().take_while(|&&x| x == c).count();
                let next = chars.get(i + run);
                let opens = next.is_some_and(|n| !n.is_whitespace());
                // « _ » au milieu d'un mot (nom_de_fichier) n'est pas un délimiteur.
                let intraword = c == '_' && i > 0 && chars[i - 1].is_alphanumeric();
                let usable = if c == '~' {
                    run == 2
                } else {
                    (1..=3).contains(&run)
                };
                if opens && !intraword && usable {
                    let delim: Vec<char> = core::iter::repeat_n(c, run).collect();
                    if let Some(end) = closing(chars, i + run, &delim)
                        && !(c == '_' && chars.get(end + run).is_some_and(|n| n.is_alphanumeric()))
                    {
                        flush(&mut text, out);
                        let added = match (c, run) {
                            ('~', _) => STRIKE,
                            (_, 1) => ITALIC,
                            (_, 2) => BOLD,
                            _ => BOLD | ITALIC,
                        };
                        parse_inline(&chars[i + run..end], style | added, out);
                        i = end + run;
                        continue;
                    }
                }
                text.extend(&chars[i..i + run]);
                i += run;
                continue;
            }
            _ => {}
        }
        text.push(c);
        i += 1;
    }
    flush(&mut text, out);
}
