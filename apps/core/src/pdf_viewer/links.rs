//! Liens d'un document : renvois internes (« voir section 3 », référence [12], table des
//! matières) et adresses web. Lus dans le fichier avec `lopdf`, depuis un fil
//! d'arrière-plan : Poppler en ligne de commande ne donne ni le rectangle exact d'un lien
//! ni l'endroit visé dans la page.
//!
//! Les rectangles et les hauteurs sont en points, depuis le coin haut gauche de la page
//! affichée (rotation appliquée), comme les mots de `pdftotext`.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use eframe::egui::{Pos2, Rect, pos2};
use lopdf::{Dictionary, Document, Object, ObjectId};

/// Profondeur maximale suivie dans un arbre de noms ou une chaîne de références (protège
/// contre un fichier abîmé qui boucle).
const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// Endroit du document : page (numérotée depuis 0), et point visé s'il est connu.
    Page {
        page: usize,
        left: Option<f32>,
        top: Option<f32>,
    },
    /// Adresse web ou courriel (`http`, `https`, `mailto` seulement).
    Uri(String),
    /// Autre document PDF, à côté de celui-ci.
    File { path: PathBuf, page: Option<usize> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub rect: Rect,
    pub target: Target,
}

/// Liens de chaque page (vide si le document n'en a pas ou ne peut pas être lu).
pub fn load(path: &Path) -> Vec<Vec<Link>> {
    let Ok(document) = Document::load(path) else {
        return Vec::new();
    };
    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    read(&document, &base)
}

pub fn read(document: &Document, base: &Path) -> Vec<Vec<Link>> {
    let pages = document.get_pages();
    let ids: Vec<ObjectId> = pages.values().copied().collect();
    let index: HashMap<ObjectId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let boxes: Vec<PageBox> = ids.iter().map(|id| PageBox::of(document, *id)).collect();
    let context = Context {
        document,
        index: &index,
        boxes: &boxes,
        base,
    };
    ids.iter()
        .zip(&boxes)
        .map(|(id, page_box)| {
            document
                .get_page_annotations(*id)
                .unwrap_or_default()
                .into_iter()
                .filter(|annotation| {
                    name_of(document, annotation.get(b"Subtype").ok()) == Some(b"Link".as_slice())
                })
                .filter_map(|annotation| {
                    let rect = numbers(document, annotation.get(b"Rect").ok())?;
                    let [x0, y0, x1, y1] = <[f32; 4]>::try_from(rect).ok()?;
                    let a = page_box.to_view(x0, y0);
                    let b = page_box.to_view(x1, y1);
                    let rect = Rect::from_two_pos(a, b);
                    (rect.width() > 0.5 && rect.height() > 0.5).then_some(())?;
                    let target = context.target_of(annotation)?;
                    Some(Link { rect, target })
                })
                .collect()
        })
        .collect()
}

/// Lien sous le point `point` (en points de page), le plus petit s'ils se chevauchent.
pub fn at(links: &[Link], point: Pos2) -> Option<&Link> {
    links
        .iter()
        .filter(|link| link.rect.expand(1.0).contains(point))
        .min_by(|a, b| a.rect.area().total_cmp(&b.rect.area()))
}

/// Cadre visible d'une page et sa rotation, pour passer des coordonnées du PDF (origine en
/// bas à gauche) à celles de l'écran (origine en haut à gauche).
#[derive(Debug, Clone, Copy)]
struct PageBox {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    rotation: i64,
}

impl PageBox {
    fn of(document: &Document, page: ObjectId) -> Self {
        let media = inherited(document, page, b"MediaBox")
            .and_then(|object| numbers(document, Some(object)))
            .and_then(|values| <[f32; 4]>::try_from(values).ok())
            .unwrap_or([0.0, 0.0, 612.0, 792.0]);
        let crop = inherited(document, page, b"CropBox")
            .and_then(|object| numbers(document, Some(object)))
            .and_then(|values| <[f32; 4]>::try_from(values).ok())
            .unwrap_or(media);
        let normal = |b: [f32; 4]| [b[0].min(b[2]), b[1].min(b[3]), b[0].max(b[2]), b[1].max(b[3])];
        let (media, crop) = (normal(media), normal(crop));
        // Le cadre visible ne dépasse pas la feuille.
        let mut visible = [
            crop[0].max(media[0]),
            crop[1].max(media[1]),
            crop[2].min(media[2]),
            crop[3].min(media[3]),
        ];
        if visible[2] <= visible[0] || visible[3] <= visible[1] {
            visible = media;
        }
        let rotation = inherited(document, page, b"Rotate")
            .and_then(|object| deref(document, object).as_i64().ok())
            .unwrap_or(0)
            .rem_euclid(360);
        Self {
            x0: visible[0],
            y0: visible[1],
            x1: visible[2],
            y1: visible[3],
            rotation: rotation - rotation % 90,
        }
    }

    /// Point du PDF → point de la page affichée. `/Rotate` tourne la page dans le sens des
    /// aiguilles d'une montre.
    fn to_view(self, x: f32, y: f32) -> Pos2 {
        let (width, height) = (self.x1 - self.x0, self.y1 - self.y0);
        let (u, v) = (x - self.x0, self.y1 - y);
        match self.rotation {
            90 => pos2(height - v, u),
            180 => pos2(width - u, height - v),
            270 => pos2(v, width - u),
            _ => pos2(u, v),
        }
    }
}

struct Context<'a> {
    document: &'a Document,
    index: &'a HashMap<ObjectId, usize>,
    boxes: &'a [PageBox],
    base: &'a Path,
}

impl<'a> Context<'a> {
    fn target_of(&self, annotation: &Dictionary) -> Option<Target> {
        let document = self.document;
        if let Ok(destination) = annotation.get(b"Dest") {
            return self.destination(destination, 0);
        }
        let action = deref(document, annotation.get(b"A").ok()?).as_dict().ok()?;
        match name_of(document, action.get(b"S").ok())? {
            b"GoTo" => self.destination(action.get(b"D").ok()?, 0),
            b"URI" => {
                let uri = text(deref(document, action.get(b"URI").ok()?))?;
                let uri = uri.trim();
                let lower = uri.to_ascii_lowercase();
                (lower.starts_with("http://")
                    || lower.starts_with("https://")
                    || lower.starts_with("mailto:"))
                .then(|| Target::Uri(uri.to_owned()))
            }
            b"GoToR" => {
                let file = file_name(document, action.get(b"F").ok()?)?;
                let path = self.base.join(file);
                let is_pdf = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
                if !is_pdf {
                    return None;
                }
                // Dans un autre fichier, la page est donnée par son numéro (depuis 0).
                let page = action
                    .get(b"D")
                    .ok()
                    .and_then(|destination| deref(document, destination).as_array().ok())
                    .and_then(|array| array.first())
                    .and_then(|first| first.as_i64().ok())
                    .and_then(|page| usize::try_from(page).ok());
                Some(Target::File { path, page })
            }
            _ => None,
        }
    }

    /// Destination : tableau `[page /XYZ gauche haut zoom]`, ou nom à chercher dans le
    /// catalogue (dictionnaire `/Dests` ou arbre de noms `/Names /Dests`).
    fn destination(&self, object: &Object, depth: usize) -> Option<Target> {
        if depth > MAX_DEPTH {
            return None;
        }
        let document = self.document;
        match deref(document, object) {
            Object::Array(array) => self.explicit(array),
            Object::Dictionary(dictionary) => self.destination(dictionary.get(b"D").ok()?, depth + 1),
            Object::Name(name) | Object::String(name, _) => {
                let found = self.named(name)?;
                self.destination(found, depth + 1)
            }
            _ => None,
        }
    }

    fn explicit(&self, array: &[Object]) -> Option<Target> {
        let document = self.document;
        let page = match array.first()? {
            Object::Reference(id) => *self.index.get(id)?,
            // Certains fichiers donnent le numéro de page au lieu de la page.
            Object::Integer(number) => usize::try_from(*number).ok()?,
            _ => return None,
        };
        let page_box = *self.boxes.get(page)?;
        let number = |index: usize| {
            array
                .get(index)
                .and_then(|value| deref(document, value).as_float().ok())
        };
        let (x, y) = match name_of(document, array.get(1)) {
            Some(b"XYZ") => (number(2), number(3)),
            Some(b"FitH" | b"FitBH") => (None, number(2)),
            Some(b"FitV" | b"FitBV") => (number(2), None),
            Some(b"FitR") => (number(2), number(5)),
            _ => (None, None),
        };
        let point = page_box.to_view(x.unwrap_or(page_box.x0), y.unwrap_or(page_box.y1));
        let (left, top) = match page_box.rotation {
            // Tournée d'un quart : la hauteur visée devient une abscisse, on garde le haut.
            90 | 270 => (None, None),
            _ => (x.map(|_| point.x), y.map(|_| point.y)),
        };
        Some(Target::Page { page, left, top })
    }

    fn named(&self, name: &[u8]) -> Option<&'a Object> {
        let document = self.document;
        let catalog = document.catalog().ok()?;
        if let Ok(names) = catalog.get(b"Names")
            && let Ok(names) = deref(document, names).as_dict()
            && let Ok(tree) = names.get(b"Dests")
            && let Some(found) = self.name_tree(tree, name, 0)
        {
            return Some(found);
        }
        let dests = deref(document, catalog.get(b"Dests").ok()?).as_dict().ok()?;
        dests.get(name).ok()
    }

    fn name_tree(&self, node: &'a Object, name: &[u8], depth: usize) -> Option<&'a Object> {
        if depth > MAX_DEPTH {
            return None;
        }
        let document = self.document;
        let node = deref(document, node).as_dict().ok()?;
        if let Ok(names) = node.get(b"Names")
            && let Ok(names) = deref(document, names).as_array()
        {
            for pair in names.chunks(2) {
                if let [key, value] = pair
                    && let Object::String(key, _) | Object::Name(key) = deref(document, key)
                    && key.as_slice() == name
                {
                    return Some(value);
                }
            }
        }
        let kids = deref(document, node.get(b"Kids").ok()?).as_array().ok()?;
        for kid in kids {
            // Les bornes `/Limits` évitent de parcourir les branches qui ne peuvent pas
            // contenir le nom.
            if let Ok(child) = deref(document, kid).as_dict()
                && let Ok(limits) = child.get(b"Limits")
                && let Ok(limits) = deref(document, limits).as_array()
                && let [Object::String(low, _), Object::String(high, _)] = limits.as_slice()
                && (name < low.as_slice() || name > high.as_slice())
            {
                continue;
            }
            if let Some(found) = self.name_tree(kid, name, depth + 1) {
                return Some(found);
            }
        }
        None
    }
}

fn deref<'a>(document: &'a Document, mut object: &'a Object) -> &'a Object {
    for _ in 0..MAX_DEPTH {
        match object {
            Object::Reference(id) => match document.get_object(*id) {
                Ok(next) => object = next,
                Err(_) => return &Object::Null,
            },
            _ => return object,
        }
    }
    &Object::Null
}

fn name_of<'a>(document: &'a Document, object: Option<&'a Object>) -> Option<&'a [u8]> {
    match deref(document, object?) {
        Object::Name(name) => Some(name),
        _ => None,
    }
}

fn numbers(document: &Document, object: Option<&Object>) -> Option<Vec<f32>> {
    deref(document, object?)
        .as_array()
        .ok()?
        .iter()
        .map(|value| deref(document, value).as_float().ok())
        .collect()
}

/// Attribut d'une page, éventuellement hérité de ses parents (`/MediaBox`, `/Rotate`…).
fn inherited<'a>(document: &'a Document, page: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut node = document.get_dictionary(page).ok()?;
    for _ in 0..MAX_DEPTH {
        if let Ok(value) = node.get(key) {
            return Some(value);
        }
        node = deref(document, node.get(b"Parent").ok()?).as_dict().ok()?;
    }
    None
}

/// Chaîne de texte d'un PDF : UTF-16 si elle commence par sa marque, sinon octet par octet.
fn text(object: &Object) -> Option<String> {
    let Object::String(bytes, _) = object else {
        return None;
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    Some(match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&byte| char::from(byte)).collect(),
    })
}

/// Nom de fichier d'une spécification de fichier (chaîne, ou dictionnaire `/UF` ou `/F`).
fn file_name(document: &Document, object: &Object) -> Option<PathBuf> {
    let object = deref(document, object);
    let name = match object {
        Object::Dictionary(dictionary) => dictionary
            .get(b"UF")
            .or_else(|_| dictionary.get(b"F"))
            .ok()
            .and_then(|value| text(deref(document, value)))?,
        other => text(other)?,
    };
    let name = name.trim();
    (!name.is_empty() && !name.contains('\0')).then(|| PathBuf::from(name))
}

/// Début du texte visé par un renvoi (la référence [12] de la bibliographie, le titre de
/// la section) : les mots qui suivent le point visé, dans l'ordre de lecture.
pub fn preview(words: &[super::poppler::Word], left: Option<f32>, top: Option<f32>) -> Option<String> {
    let top = top?;
    let start = words.iter().position(|word| {
        word.rect.max.y > top + 1.0 && left.is_none_or(|left| word.rect.max.x > left - 2.0)
    })?;
    let first = words[start].rect.min.y;
    let mut text = String::new();
    for (count, word) in words[start..].iter().enumerate() {
        // Trois à quatre lignes au plus, sans remonter dans la page (autre colonne).
        if word.rect.min.y > first + 40.0 || word.rect.min.y < first - 2.0 || count >= 60 {
            break;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&word.text);
    }
    if text.chars().count() > 280 {
        text = text.chars().take(279).collect::<String>() + "…";
    }
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Stream, dictionary};

    /// Document de trois pages : un lien vers la page 3 (tableau), un vers un nom, un vers
    /// le web, un vers un autre PDF.
    fn sample(rotate_first: i64) -> Document {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let content = document.add_object(Stream::new(dictionary! {}, Vec::new()));
        let mut page_ids = Vec::new();
        for _ in 0..3 {
            page_ids.push(document.new_object_id());
        }
        let link = |rect: [i64; 4], extra: Dictionary| {
            let mut annotation = dictionary! {
                "Type" => "Annot",
                "Subtype" => "Link",
                "Rect" => rect.iter().map(|value| Object::Integer(*value)).collect::<Vec<_>>(),
            };
            annotation.extend(&extra);
            Object::Dictionary(annotation)
        };
        let annotations = vec![
            link(
                [100, 700, 150, 712],
                dictionary! { "Dest" => vec![Object::Reference(page_ids[2]), "XYZ".into(), 72.into(), 500.into(), Object::Null] },
            ),
            link(
                [100, 600, 160, 612],
                dictionary! { "A" => dictionary! { "S" => "GoTo", "D" => Object::string_literal("cite.knuth") } },
            ),
            link(
                [100, 500, 200, 512],
                dictionary! { "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://www.debian.org/") } },
            ),
            link(
                [100, 400, 200, 412],
                dictionary! { "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("file:///etc/passwd") } },
            ),
            link(
                [100, 300, 200, 312],
                dictionary! { "A" => dictionary! { "S" => "GoToR", "F" => Object::string_literal("annexe.pdf"), "D" => vec![Object::Integer(4), "Fit".into()] } },
            ),
        ];
        for (index, id) in page_ids.iter().enumerate() {
            let mut page = dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content,
            };
            if index == 0 {
                page.set("Annots", annotations.clone());
                page.set("Rotate", rotate_first);
            }
            document.objects.insert(*id, Object::Dictionary(page));
        }
        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            "Count" => 3,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        };
        document.objects.insert(pages_id, Object::Dictionary(pages));
        let names = dictionary! {
            "Dests" => dictionary! {
                "Kids" => vec![Object::Dictionary(dictionary! {
                    "Limits" => vec![Object::string_literal("a"), Object::string_literal("z")],
                    "Names" => vec![
                        Object::string_literal("cite.knuth"),
                        Object::Dictionary(dictionary! { "D" => vec![Object::Reference(page_ids[1]), "FitH".into(), 300.into()] }),
                    ],
                })],
            },
        };
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "Names" => names,
        });
        document.trailer.set("Root", catalog);
        document
    }

    #[test]
    fn lit_les_renvois_les_adresses_et_les_autres_documents() {
        let links = read(&sample(0), Path::new("/docs"));
        assert_eq!(links.len(), 3);
        let first = &links[0];
        assert_eq!(first.len(), 4, "l'adresse file:// est écartée");
        assert_eq!(
            first[0].rect,
            Rect::from_min_max(pos2(100.0, 80.0), pos2(150.0, 92.0))
        );
        assert_eq!(
            first[0].target,
            Target::Page {
                page: 2,
                left: Some(72.0),
                top: Some(292.0)
            }
        );
        assert_eq!(
            first[1].target,
            Target::Page {
                page: 1,
                left: None,
                top: Some(492.0)
            }
        );
        assert_eq!(first[2].target, Target::Uri("https://www.debian.org/".to_owned()));
        assert_eq!(
            first[3].target,
            Target::File {
                path: PathBuf::from("/docs/annexe.pdf"),
                page: Some(4)
            }
        );
        assert!(links[1].is_empty());
    }

    #[test]
    fn suit_la_rotation_de_la_page() {
        let links = read(&sample(90), Path::new("/"));
        // Page tournée d'un quart : 792 de large, 612 de haut.
        assert_eq!(
            links[0][0].rect,
            Rect::from_min_max(pos2(700.0, 100.0), pos2(712.0, 150.0))
        );
    }

    #[test]
    fn le_plus_petit_lien_l_emporte() {
        let target = Target::Uri("https://a".to_owned());
        let links = vec![
            Link {
                rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0)),
                target: target.clone(),
            },
            Link {
                rect: Rect::from_min_max(pos2(10.0, 10.0), pos2(20.0, 20.0)),
                target: Target::Uri("https://b".to_owned()),
            },
        ];
        assert_eq!(at(&links, pos2(15.0, 15.0)).unwrap().target, links[1].target);
        assert_eq!(at(&links, pos2(50.0, 50.0)).unwrap().target, target);
        assert!(at(&links, pos2(150.0, 50.0)).is_none());
    }

    #[test]
    fn apercu_du_texte_vise() {
        use super::super::poppler::Word;
        let word = |text: &str, x: f32, y: f32| Word {
            text: text.to_owned(),
            rect: Rect::from_min_max(pos2(x, y), pos2(x + 30.0, y + 10.0)),
        };
        let words = vec![
            word("Avant", 72.0, 100.0),
            word("[1]", 72.0, 300.0),
            word("Knuth,", 110.0, 300.0),
            word("TeXbook.", 72.0, 312.0),
            word("Loin", 72.0, 500.0),
        ];
        assert_eq!(
            preview(&words, Some(72.0), Some(295.0)).as_deref(),
            Some("[1] Knuth, TeXbook.")
        );
        assert!(preview(&words, None, None).is_none());
    }
}
