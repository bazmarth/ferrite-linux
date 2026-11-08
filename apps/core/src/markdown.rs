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
    /// Adresse du lien (vide hors d'un lien).
    pub link: String,
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
    /// Image seule sur sa ligne : « ![texte](chemin) », ou dans un lien
    /// « [![texte](chemin)](adresse) » (vignette d'une vidéo, par exemple).
    Image {
        alt: String,
        path: String,
        /// Adresse du lien autour de l'image (vide sinon).
        link: String,
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
    let text = strip_comments(text);
    let lines: Vec<&str> = text.split('\n').collect();
    parse_lines(&lines)
}

/// Retire les commentaires HTML « <!-- … --> » (même sur plusieurs lignes), sauf dans les
/// blocs de code et le code en ligne. Une ligne qui ne contenait qu'un commentaire devient
/// vide : elle sépare encore les paragraphes.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_comment = false;
    let mut in_fence: Option<&str> = None;
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if !in_comment {
            if let Some(open) = in_fence {
                if fence(line) == Some(open) {
                    in_fence = None;
                }
                out.push_str(line);
                continue;
            }
            if let Some(open) = fence(line) {
                in_fence = Some(open);
                out.push_str(line);
                continue;
            }
        }
        let mut kept = String::new();
        let mut rest = line;
        let mut removed = in_comment;
        while !rest.is_empty() {
            if in_comment {
                match rest.find("-->") {
                    Some(end) => {
                        rest = &rest[end + 3..];
                        in_comment = false;
                    }
                    None => rest = "",
                }
                continue;
            }
            let comment = rest.find("<!--");
            let code = rest.find('`');
            match (comment, code) {
                (Some(start), Some(tick)) if tick < start => {
                    // Code en ligne : recopié tel quel jusqu'aux accents graves fermants.
                    let run = rest[tick..].chars().take_while(|&c| c == '`').count();
                    let ticks = &rest[tick..tick + run];
                    let end = rest[tick + run..]
                        .find(ticks)
                        .map_or(rest.len(), |at| tick + run + at + run);
                    kept.push_str(&rest[..end]);
                    rest = &rest[end..];
                }
                (Some(start), _) => {
                    kept.push_str(&rest[..start]);
                    rest = &rest[start + 4..];
                    in_comment = true;
                    removed = true;
                }
                (None, _) => {
                    kept.push_str(rest);
                    rest = "";
                }
            }
        }
        if !(removed && kept.trim().is_empty()) {
            out.push_str(&kept);
        }
    }
    out
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
            && let Some((alt, path, link)) = image(line)
        {
            blocks.push(Block::Image { alt, path, link });
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
fn image(line: &str) -> Option<(String, String, String)> {
    let chars: Vec<char> = line.trim().chars().collect();
    if chars.first() == Some(&'[') {
        let (start, end, after) = link_at(&chars, 0)?;
        if after != chars.len() {
            return None;
        }
        let inner: String = chars[start..end].iter().collect();
        let target: String = chars[end + 2..after - 1].iter().collect();
        let (alt, path, _) = image(&inner)?;
        return Some((alt, path, link_target(&target)));
    }
    if chars.first() != Some(&'!') {
        return None;
    }
    let (start, end, after) = link_at(&chars, 1)?;
    (after == chars.len()).then(|| {
        let alt = chars[start..end].iter().collect();
        let path: String = chars[end + 2..after - 1].iter().collect();
        // « chemin "titre" » : le titre est ignoré.
        let path = path.split_whitespace().next().unwrap_or("");
        (alt, String::from(path), String::new())
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
    parse_inline(&chars, 0, "", &mut out);
    // Fusion des fragments voisins de même style.
    let mut merged: Vec<Span> = Vec::new();
    for span in out {
        match merged.last_mut() {
            Some(last) if last.style == span.style && last.link == span.link => {
                last.text.push_str(&span.text)
            }
            _ => merged.push(span),
        }
    }
    merged
}

fn push(out: &mut Vec<Span>, text: &str, style: u8, link: &str) {
    if !text.is_empty() {
        out.push(Span {
            text: String::from(text),
            style,
            link: String::from(link),
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
    // L'adresse peut contenir des parenthèses équilibrées : « badge/IA%20(.NET)-violet ».
    let mut depth = 0;
    let mut close = None;
    let mut k = j + 2;
    while k < chars.len() {
        match chars[k] {
            '\\' => k += 1,
            '(' => depth += 1,
            ')' if depth == 0 => {
                close = Some(k);
                break;
            }
            ')' => depth -= 1,
            _ => {}
        }
        k += 1;
    }
    Some((i + 1, j, close? + 1))
}

/// Adresse d'un lien « [texte](adresse "titre") » : sans le titre ni les chevrons.
fn link_target(raw: &str) -> String {
    let raw = raw.trim();
    let raw = raw.split_once(" \"").map_or(raw, |(address, _)| address).trim();
    String::from(
        raw.strip_prefix('<')
            .and_then(|r| r.strip_suffix('>'))
            .unwrap_or(raw),
    )
}

fn parse_inline(chars: &[char], style: u8, link: &str, out: &mut Vec<Span>) {
    let mut text = String::new();
    let mut i = 0;
    let flush = |text: &mut String, out: &mut Vec<Span>| {
        push(out, text, style, link);
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
                push(out, "\n", style, link);
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
                    push(out, &code, style | CODE, link);
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
                    push(out, &alloc::format!("[image: {alt}]"), style | ITALIC, link);
                    i = after;
                    continue;
                }
            }
            '[' => {
                if let Some((start, end, after)) = link_at(chars, i) {
                    flush(&mut text, out);
                    let target: String = chars[end + 2..after - 1].iter().collect();
                    parse_inline(&chars[start..end], style | LINK, &link_target(&target), out);
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
                        let target = if inner.contains('@') && !inner.contains("://") {
                            alloc::format!("mailto:{inner}")
                        } else {
                            inner.clone()
                        };
                        push(out, &inner, style | LINK, &target);
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
                        parse_inline(&chars[i + run..end], style | added, link, out);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liens_gardent_leur_adresse() {
        let spans = inline(r#"Voir [le **site**](https://exemple.fr "titre") et <https://a.b>."#);
        let links: Vec<(&str, &str)> = spans
            .iter()
            .filter(|span| span.style & LINK != 0)
            .map(|span| (span.text.as_str(), span.link.as_str()))
            .collect();
        assert_eq!(
            links,
            [
                ("le ", "https://exemple.fr"),
                ("site", "https://exemple.fr"),
                ("https://a.b", "https://a.b")
            ]
        );
        assert!(spans[0].link.is_empty());
    }

    #[test]
    fn commentaires_retires() {
        let text = "Avant\n<!-- CO-OP START -->\nTexte <!-- caché --> visible `<!-- code -->`\n<!-- sur\ndeux lignes -->\n```\n<!-- dans le code -->\n```\nAprès";
        assert_eq!(
            strip_comments(text),
            "Avant\n\nTexte  visible `<!-- code -->`\n\n\n```\n<!-- dans le code -->\n```\nAprès"
        );
    }

    #[test]
    fn adresse_avec_parentheses() {
        let blocks = parse("[![IA (.NET)](https://b.io/badge/IA%20(.NET)-93?s=1)](https://g.com/x)\n");
        match blocks.first() {
            Some(Block::Image { alt, path, link }) => {
                assert_eq!(alt, "IA (.NET)");
                assert_eq!(path, "https://b.io/badge/IA%20(.NET)-93?s=1");
                assert_eq!(link, "https://g.com/x");
            }
            other => panic!("pas une image : {other:?}"),
        }
    }

    #[test]
    fn image_dans_un_lien() {
        let blocks = parse("[![Voir la vidéo](images/a.png)](https://video.fr/?id=1)\n");
        match blocks.first() {
            Some(Block::Image { alt, path, link }) => {
                assert_eq!((alt.as_str(), path.as_str()), ("Voir la vidéo", "images/a.png"));
                assert_eq!(link, "https://video.fr/?id=1");
            }
            other => panic!("pas une image : {other:?}"),
        }
    }
}
