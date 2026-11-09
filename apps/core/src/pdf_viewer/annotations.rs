//! Annotations d'un document : surlignages, soulignés, texte barré, notes posées sur la
//! page, commentaires et signets. Le PDF lui-même n'est jamais modifié : tout est gardé
//! dans `~/.local/share/ferrite/lecteur-pdf-annotations/EMPREINTE.txt`, l'empreinte étant
//! tirée du contenu du fichier (un document renommé ou déplacé garde ses annotations).
//!
//! Format, une ligne par élément, champs séparés par des tabulations :
//! `signet<TAB>page` et
//! `annotation<TAB>id<TAB>page<TAB>genre<TAB>couleur<TAB>x0,y0,x1,y1;…<TAB>extrait<TAB>commentaire`.

use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use eframe::egui;

const HEADER: &str = "ferrite-annotations 1";
/// Part du fichier lue à chaque bout pour calculer l'empreinte.
const SAMPLE: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Highlight,
    Underline,
    Strike,
    /// Note posée à un endroit de la page, sans texte choisi.
    Note,
}

impl Kind {
    fn code(self) -> &'static str {
        match self {
            Self::Highlight => "surligne",
            Self::Underline => "souligne",
            Self::Strike => "barre",
            Self::Note => "note",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Some(match code {
            "surligne" => Self::Highlight,
            "souligne" => Self::Underline,
            "barre" => Self::Strike,
            "note" => Self::Note,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Highlight => "Surligné",
            Self::Underline => "Souligné",
            Self::Strike => "Barré",
            Self::Note => "Note",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub id: u64,
    /// Page, numérotée depuis 0.
    pub page: usize,
    pub kind: Kind,
    /// Rang de l'encre dans `style::marker`.
    pub color: usize,
    /// Zones marquées, en points de la page (une par ligne de texte ; une seule, petite,
    /// pour une note).
    pub rects: Vec<egui::Rect>,
    /// Texte marqué (vide pour une note).
    pub quote: String,
    pub comment: String,
}

impl Annotation {
    pub fn top(&self) -> f32 {
        self.rects.first().map_or(0.0, |rect| rect.top())
    }

    pub fn contains(&self, point: egui::Pos2) -> bool {
        self.rects.iter().any(|rect| rect.expand(1.5).contains(point))
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notes {
    pub items: Vec<Annotation>,
    /// Pages marquées d'un signet (numérotées depuis 0), triées.
    pub bookmarks: Vec<usize>,
}

impl Notes {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.bookmarks.is_empty()
    }

    /// Ajoute une annotation et rend son identifiant.
    pub fn add(
        &mut self,
        page: usize,
        kind: Kind,
        color: usize,
        rects: Vec<egui::Rect>,
        quote: String,
    ) -> u64 {
        let id = self.items.iter().map(|item| item.id).max().unwrap_or(0) + 1;
        self.items.push(Annotation {
            id,
            page,
            kind,
            color,
            rects,
            quote,
            comment: String::new(),
        });
        self.items
            .sort_by(|a, b| a.page.cmp(&b.page).then(a.top().total_cmp(&b.top())));
        id
    }

    pub fn get(&self, id: u64) -> Option<&Annotation> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Annotation> {
        self.items.iter_mut().find(|item| item.id == id)
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|item| item.id != id);
    }

    /// Annotation sous `point` : la dernière posée d'abord, une note avant un surlignage.
    pub fn at(&self, page: usize, point: egui::Pos2) -> Option<u64> {
        let on_page = || self.items.iter().rev().filter(move |item| item.page == page);
        on_page()
            .find(|item| item.kind == Kind::Note && item.contains(point))
            .or_else(|| on_page().find(|item| item.contains(point)))
            .map(|item| item.id)
    }

    pub fn has_bookmark(&self, page: usize) -> bool {
        self.bookmarks.contains(&page)
    }

    pub fn toggle_bookmark(&mut self, page: usize) {
        if self.has_bookmark(page) {
            self.bookmarks.retain(|marked| *marked != page);
        } else {
            self.bookmarks.push(page);
            self.bookmarks.sort_unstable();
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "")
        .replace('\n', "\\n")
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

pub fn format(notes: &Notes, document: &Path) -> String {
    let mut out = format!(
        "{HEADER}\ndocument\t{}\n",
        escape(&document.display().to_string())
    );
    for page in &notes.bookmarks {
        out.push_str(&format!("signet\t{}\n", page + 1));
    }
    for item in &notes.items {
        let rects: Vec<String> = item
            .rects
            .iter()
            .map(|rect| {
                format!(
                    "{:.2},{:.2},{:.2},{:.2}",
                    rect.left(),
                    rect.top(),
                    rect.right(),
                    rect.bottom()
                )
            })
            .collect();
        out.push_str(&format!(
            "annotation\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            item.id,
            item.page + 1,
            item.kind.code(),
            item.color,
            rects.join(";"),
            escape(&item.quote),
            escape(&item.comment),
        ));
    }
    out
}

fn parse_rect(text: &str) -> Option<egui::Rect> {
    let mut numbers = text.split(',').map(|part| part.trim().parse::<f32>());
    let (x0, y0, x1, y1) = (
        numbers.next()?.ok()?,
        numbers.next()?.ok()?,
        numbers.next()?.ok()?,
        numbers.next()?.ok()?,
    );
    [x0, y0, x1, y1]
        .iter()
        .all(|value| value.is_finite())
        .then(|| egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1)))
}

/// Lit un fichier d'annotations ; une ligne abîmée est ignorée, les autres sont gardées.
pub fn parse(text: &str) -> Notes {
    let mut notes = Notes::default();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["signet", page] => {
                if let Some(page) = page.parse::<usize>().ok().and_then(|page| page.checked_sub(1))
                    && !notes.has_bookmark(page)
                {
                    notes.bookmarks.push(page);
                }
            }
            ["annotation", id, page, kind, color, rects, quote, comment] => {
                let (Ok(id), Some(page), Some(kind), Ok(color)) = (
                    id.parse::<u64>(),
                    page.parse::<usize>().ok().and_then(|page| page.checked_sub(1)),
                    Kind::from_code(kind),
                    color.parse::<usize>(),
                ) else {
                    continue;
                };
                let rects: Vec<egui::Rect> = rects.split(';').filter_map(parse_rect).collect();
                if rects.is_empty() || notes.get(id).is_some() {
                    continue;
                }
                notes.items.push(Annotation {
                    id,
                    page,
                    kind,
                    color,
                    rects,
                    quote: unescape(quote),
                    comment: unescape(comment),
                });
            }
            _ => {}
        }
    }
    notes.bookmarks.sort_unstable();
    notes
        .items
        .sort_by(|a, b| a.page.cmp(&b.page).then(a.top().total_cmp(&b.top())));
    notes
}

fn folder() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("ferrite/lecteur-pdf-annotations")
}

/// FNV-1a sur 64 bits : simple, et le même d'une version de Rust à l'autre.
fn fnv(hash: u64, data: &[u8]) -> u64 {
    data.iter().fold(hash, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// Empreinte d'un contenu : sa taille, puis son début et sa fin.
fn fingerprint_of(size: u64, head: &[u8], tail: &[u8]) -> String {
    let hash = fnv(fnv(0xCBF2_9CE4_8422_2325, head), tail);
    format!("{size:x}-{hash:016x}")
}

/// Empreinte du document : elle nomme son fichier d'annotations.
pub fn fingerprint(document: &Path) -> Option<String> {
    let mut file = fs::File::open(document).ok()?;
    let size = file.metadata().ok()?.len();
    let mut head = Vec::new();
    file.by_ref().take(SAMPLE).read_to_end(&mut head).ok()?;
    let mut tail = Vec::new();
    if size > SAMPLE {
        file.seek(SeekFrom::Start(size - SAMPLE.min(size - SAMPLE)))
            .ok()?;
        file.read_to_end(&mut tail).ok()?;
    }
    Some(fingerprint_of(size, &head, &tail))
}

pub fn load(key: &str) -> Notes {
    fs::read_to_string(folder().join(format!("{key}.txt")))
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Enregistre les annotations ; sans annotation ni signet, le fichier est retiré.
pub fn save(key: &str, notes: &Notes, document: &Path) {
    let folder = folder();
    let path = folder.join(format!("{key}.txt"));
    if notes.is_empty() {
        let _ = fs::remove_file(path);
        return;
    }
    let _ = fs::create_dir_all(&folder);
    // Écrit à côté puis renomme : une coupure ne laisse pas un fichier à moitié écrit.
    let temporary = path.with_extension("tmp");
    if fs::write(&temporary, format(notes, document)).is_ok() {
        let _ = fs::rename(&temporary, &path);
    }
}

/// Toutes les notes en Markdown, page par page : pour les relire ou les envoyer.
pub fn markdown(notes: &Notes, name: &str) -> String {
    let mut out = format!("# Notes — {name}\n");
    if !notes.bookmarks.is_empty() {
        let pages: Vec<String> = notes
            .bookmarks
            .iter()
            .map(|page| (page + 1).to_string())
            .collect();
        out.push_str(&format!("\nSignets : pages {}\n", pages.join(", ")));
    }
    let mut page = None;
    for item in &notes.items {
        if page != Some(item.page) {
            page = Some(item.page);
            out.push_str(&format!("\n## Page {}\n", item.page + 1));
        }
        let quote = item.quote.split_whitespace().collect::<Vec<_>>().join(" ");
        if !quote.is_empty() {
            out.push_str(&format!("\n> {quote}\n"));
        }
        let comment = item.comment.trim();
        if !comment.is_empty() {
            out.push_str(&format!("\n{comment}\n"));
        } else if quote.is_empty() {
            out.push_str("\n(note vide)\n");
        }
    }
    out
}

/// Écrit les notes à côté du document (« NOM — notes.md »), sinon dans Documents ou le
/// dossier personnel ; rend le fichier écrit.
pub fn export(notes: &Notes, document: &Path) -> Result<PathBuf, String> {
    let stem = document.file_stem().map_or_else(
        || "document".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let name = document
        .file_name()
        .map_or_else(|| stem.clone(), |name| name.to_string_lossy().into_owned());
    let text = markdown(notes, &name);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let folders = [
        document.parent().map(Path::to_path_buf),
        home.as_ref().map(|home| home.join("Documents")),
        home,
    ];
    for folder in folders.into_iter().flatten() {
        let target = folder.join(format!("{stem} — notes.md"));
        if fs::write(&target, &text).is_ok() {
            return Ok(target);
        }
    }
    Err("Impossible d’écrire le fichier des notes.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(100.0, 12.0))
    }

    fn sample() -> Notes {
        let mut notes = Notes::default();
        let later = notes.add(
            2,
            Kind::Highlight,
            1,
            vec![rect(10.0, 300.0)],
            "plus bas".to_owned(),
        );
        notes.add(
            2,
            Kind::Underline,
            3,
            vec![rect(10.5, 20.25), rect(10.0, 34.0)],
            "deux\nlignes".to_owned(),
        );
        notes.add(0, Kind::Note, 0, vec![rect(40.0, 40.0)], String::new());
        notes.get_mut(later).unwrap().comment = "À vérifier\tavec\\Paul\nvite".to_owned();
        notes.toggle_bookmark(7);
        notes.toggle_bookmark(2);
        notes
    }

    #[test]
    fn lit_et_ecrit_les_annotations() {
        let notes = sample();
        let text = format(&notes, Path::new("/home/moi/rapport.pdf"));
        assert!(text.starts_with(HEADER));
        assert_eq!(parse(&text), notes);
    }

    #[test]
    fn annotations_triees_par_page_puis_par_hauteur() {
        let notes = sample();
        let order: Vec<(usize, Kind)> = notes.items.iter().map(|item| (item.page, item.kind)).collect();
        assert_eq!(
            order,
            vec![(0, Kind::Note), (2, Kind::Underline), (2, Kind::Highlight)]
        );
        assert_eq!(notes.bookmarks, vec![2, 7]);
    }

    #[test]
    fn ignore_les_lignes_abimees() {
        let notes = parse(
            "n'importe quoi\nsignet\t0\nsignet\t4\nsignet\t4\n\
             annotation\t1\t2\tsurligne\t0\t1,2,3,4\textrait\t\n\
             annotation\t1\t2\tsurligne\t0\t1,2,3,4\tdoublon\t\n\
             annotation\t2\t2\tinconnu\t0\t1,2,3,4\textrait\t\n\
             annotation\t3\t2\tbarre\t0\tpas un rectangle\textrait\t\n",
        );
        assert_eq!(notes.bookmarks, vec![3]);
        assert_eq!(notes.items.len(), 1);
        assert_eq!(notes.items[0].quote, "extrait");
        assert_eq!(notes.items[0].page, 1);
    }

    #[test]
    fn trouve_l_annotation_sous_le_pointeur() {
        let mut notes = Notes::default();
        let under = notes.add(0, Kind::Highlight, 0, vec![rect(0.0, 0.0)], "texte".to_owned());
        let note = notes.add(
            0,
            Kind::Note,
            0,
            vec![egui::Rect::from_min_size(
                egui::pos2(50.0, 0.0),
                egui::vec2(12.0, 12.0),
            )],
            String::new(),
        );
        assert_eq!(notes.at(0, egui::pos2(10.0, 5.0)), Some(under));
        assert_eq!(notes.at(0, egui::pos2(55.0, 5.0)), Some(note));
        assert_eq!(notes.at(1, egui::pos2(10.0, 5.0)), None);
        assert_eq!(notes.at(0, egui::pos2(10.0, 80.0)), None);
        notes.remove(under);
        assert_eq!(notes.at(0, egui::pos2(10.0, 5.0)), None);
    }

    #[test]
    fn l_empreinte_depend_du_contenu_pas_du_nom() {
        let one = fingerprint_of(3, b"abc", b"");
        assert_eq!(one, fingerprint_of(3, b"abc", b""));
        assert_ne!(one, fingerprint_of(3, b"abd", b""));
        assert_ne!(one, fingerprint_of(4, b"abc", b""));
        assert!(one.starts_with("3-"));
    }

    #[test]
    fn exporte_les_notes_en_markdown() {
        let text = markdown(&sample(), "rapport.pdf");
        assert!(text.starts_with("# Notes — rapport.pdf\n"));
        assert!(text.contains("Signets : pages 3, 8"));
        assert!(text.contains("## Page 1\n\n(note vide)"));
        assert!(text.contains("## Page 3\n\n> deux lignes\n\n> plus bas\n\nÀ vérifier"));
    }
}
