//! Catalogue AppStream : les applications que Debian et Flathub décrivent eux-mêmes (nom,
//! résumé, description, catégories, icône, captures). Les milliers de paquets techniques
//! (bibliothèques, outils) n'y figurent pas : seules les vraies applications y sont.
//!
//! - Debian : fichiers DEP-11 (YAML) que `apt update` télécharge quand le paquet
//!   `appstream` est installé (`/var/lib/apt/lists/*_dep11_Components-*.yml.gz`), icônes
//!   extraites dans `/var/lib/swcatalog/icons` ou `/var/cache/swcatalog/icons`.
//! - Flathub : `appstream.xml` que `flatpak update --appstream` télécharge, icônes à côté.

use std::{
    collections::HashMap,
    env, fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use yaml_rust2::{Yaml, YamlLoader};

/// D'où vient une application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Origin {
    /// Dépôts de Debian (installée par PackageKit, mot de passe administrateur).
    Debian,
    /// Flathub (installée par `flatpak --user`, sans mot de passe).
    Flathub,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Self::Debian => "Debian",
            Self::Flathub => "Flathub",
        }
    }
}

/// Morceau de description : paragraphe ou élément de liste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(String),
    Item(String),
}

/// Capture d'écran : vignette (environ 750 pixels de large) et image d'origine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screenshot {
    pub thumbnail: String,
    pub caption: String,
}

/// Une application du catalogue, pour une origine.
#[derive(Debug, Clone, Default)]
pub struct Component {
    /// Identifiant AppStream (`org.videolan.VLC`), sans `.desktop` final.
    pub id: String,
    pub origin: Option<Origin>,
    /// Paquet Debian (`vlc`) ou référence Flatpak (`app/org.videolan.VLC/x86_64/stable`).
    pub package: String,
    pub name: String,
    pub summary: String,
    pub developer: String,
    pub description: Vec<Block>,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    pub icon: Option<PathBuf>,
    pub screenshots: Vec<Screenshot>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub version: Option<String>,
    /// Fichier `.desktop` qui la lance une fois installée.
    pub desktop_id: Option<String>,
    /// Éditeur vérifié par Flathub.
    pub verified: bool,
}

impl Component {
    pub fn origin(&self) -> Origin {
        self.origin.unwrap_or(Origin::Debian)
    }
}

/// Retire le `.desktop` des anciens identifiants (`gimp.desktop` → `gimp`).
pub fn normalize_id(id: &str) -> String {
    id.trim().strip_suffix(".desktop").unwrap_or(id.trim()).to_owned()
}

/// Texte traduit : le français s'il existe, sinon le texte d'origine.
#[derive(Debug, Default)]
struct Localized<T> {
    french: Option<T>,
    default: Option<T>,
}

impl<T> Localized<T> {
    fn slot(&mut self, lang: Option<&str>) -> Option<&mut Option<T>> {
        match lang {
            None | Some("C" | "en" | "en_US" | "en-US") => Some(&mut self.default),
            Some(lang) if is_french(lang) => Some(&mut self.french),
            Some(_) => None,
        }
    }
    fn set(&mut self, lang: Option<&str>, value: T) {
        if let Some(slot) = self.slot(lang)
            && slot.is_none()
        {
            *slot = Some(value);
        }
    }
    fn take(self) -> Option<T> {
        self.french.or(self.default)
    }
}

fn is_french(lang: &str) -> bool {
    lang == "fr" || lang.starts_with("fr_") || lang.starts_with("fr-")
}

// ———————————————————————————— Flathub (XML) ————————————————————————————

/// Dossier du catalogue Flathub de l'utilisateur (`…/flatpak/appstream/flathub/x86_64/active`).
pub fn flathub_dir() -> Option<PathBuf> {
    let data = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))?;
    let user = data.join("flatpak/appstream/flathub/x86_64/active");
    if user.join("appstream.xml").is_file() || user.join("appstream.xml.gz").is_file() {
        return Some(user);
    }
    let system = PathBuf::from("/var/lib/flatpak/appstream/flathub/x86_64/active");
    (system.join("appstream.xml").is_file()).then_some(system)
}

/// Lit le catalogue de Flathub (`dir` : dossier qui contient `appstream.xml` et `icons`).
pub fn load_flathub(dir: &Path) -> Vec<Component> {
    let plain = dir.join("appstream.xml");
    let reader: Box<dyn BufRead> = if let Ok(file) = fs::File::open(&plain) {
        Box::new(BufReader::new(file))
    } else if let Ok(file) = fs::File::open(dir.join("appstream.xml.gz")) {
        Box::new(BufReader::new(flate2::read::GzDecoder::new(file)))
    } else {
        return Vec::new();
    };
    parse_flathub(reader, &dir.join("icons"))
}

/// Ce qu'on est en train de lire dans un `<component>`.
#[derive(Debug, Default)]
struct XmlComponent {
    kind: String,
    id: String,
    package: String,
    name: Localized<String>,
    summary: Localized<String>,
    developer: Localized<String>,
    description: Localized<Vec<Block>>,
    categories: Vec<String>,
    keywords: Vec<String>,
    /// (taille, échelle, fichier) des icônes en cache.
    icons: Vec<(u32, u32, String)>,
    screenshots: Vec<Screenshot>,
    homepage: Option<String>,
    license: Option<String>,
    version: Option<String>,
    desktop_id: Option<String>,
    verified: bool,
}

impl XmlComponent {
    fn finish(self, icons_dir: &Path) -> Option<Component> {
        if !matches!(self.kind.as_str(), "desktop-application" | "desktop") || self.id.is_empty() {
            return None;
        }
        let name = self.name.take()?;
        // La plus grande icône en cache jusqu'à 128 pixels, en double échelle si possible.
        let icon = self
            .icons
            .iter()
            .filter(|(size, _, _)| *size <= 128)
            .max_by_key(|(size, scale, _)| size * scale)
            .map(|(size, scale, file)| {
                let folder = if *scale > 1 {
                    format!("{size}x{size}@{scale}")
                } else {
                    format!("{size}x{size}")
                };
                icons_dir.join(folder).join(file)
            })
            .filter(|path| path.is_file())
            .or_else(|| {
                // Le dossier `@2` n'existe pas toujours : on se rabat sur la taille simple.
                self.icons
                    .iter()
                    .filter(|(size, _, _)| *size <= 128)
                    .max_by_key(|(size, _, _)| *size)
                    .map(|(size, _, file)| icons_dir.join(format!("{size}x{size}")).join(file))
                    .filter(|path| path.is_file())
            });
        let package = if self.package.is_empty() {
            format!("app/{}/x86_64/stable", self.id)
        } else {
            self.package
        };
        Some(Component {
            id: normalize_id(&self.id),
            origin: Some(Origin::Flathub),
            package,
            name,
            summary: self.summary.take().unwrap_or_default(),
            developer: self.developer.take().unwrap_or_default(),
            description: self.description.take().unwrap_or_default(),
            categories: self.categories,
            keywords: self.keywords,
            icon,
            screenshots: self.screenshots,
            homepage: self.homepage,
            license: self.license,
            version: self.version,
            desktop_id: self.desktop_id,
            verified: self.verified,
        })
    }
}

fn attribute(element: &BytesStart, name: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref() == name.as_bytes())
        .and_then(|attribute| {
            attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.into_owned())
        })
}

/// Entité XML nommée ou numérique.
fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix('x') {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

/// Espaces et retours à la ligne ramenés à une espace simple.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_flathub(source: impl BufRead, icons_dir: &Path) -> Vec<Component> {
    let mut reader = Reader::from_reader(source);
    let mut buffer = Vec::new();
    let mut components = Vec::new();
    let mut current: Option<XmlComponent> = None;
    // Pile des éléments ouverts dans le composant (noms), et texte en cours.
    let mut stack: Vec<String> = Vec::new();
    let mut text = String::new();
    // Langue de l'élément traduit en cours (name, summary, description, caption…).
    let mut lang: Option<String> = None;
    let mut description_lang: Option<String> = None;
    let mut description: Vec<Block> = Vec::new();
    // Capture en cours : vignettes (largeur, adresse) et légende.
    let mut shot_thumbs: Vec<(u32, String)> = Vec::new();
    let mut shot_source: Option<String> = None;
    let mut shot_caption: Localized<String> = Localized::default();
    let mut shot_default = false;
    let mut icon: Option<(String, u32, u32)> = None;
    let mut url_kind: Option<String> = None;
    let mut image_attrs: Option<(String, u32)> = None;
    let mut custom_key: Option<String> = None;
    loop {
        let event = match reader.read_event_into(&mut buffer) {
            Ok(event) => event,
            Err(error) => {
                tracing::warn!(%error, "catalogue Flathub illisible");
                break;
            }
        };
        match event {
            Event::Eof => break,
            Event::Start(element) => {
                let name = String::from_utf8_lossy(element.name().as_ref()).into_owned();
                if name == "component" {
                    current = Some(XmlComponent {
                        kind: attribute(&element, "type").unwrap_or_default(),
                        ..XmlComponent::default()
                    });
                    stack.clear();
                    continue;
                }
                if current.is_none() {
                    continue;
                }
                text.clear();
                match name.as_str() {
                    "name" | "summary" | "developer_name" | "caption" | "keyword" => {
                        lang = attribute(&element, "xml:lang");
                    }
                    "description" if stack.is_empty() => {
                        description_lang = attribute(&element, "xml:lang");
                        description.clear();
                    }
                    "screenshot" => {
                        shot_thumbs.clear();
                        shot_source = None;
                        shot_caption = Localized::default();
                        shot_default = attribute(&element, "type").as_deref() == Some("default");
                    }
                    "icon" => {
                        icon = attribute(&element, "type").map(|kind| {
                            let size = attribute(&element, "width")
                                .and_then(|w| w.parse().ok())
                                .unwrap_or(0);
                            let scale = attribute(&element, "scale")
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(1);
                            (kind, size, scale)
                        });
                    }
                    "url" => url_kind = attribute(&element, "type"),
                    "image" => {
                        image_attrs = Some((
                            attribute(&element, "type").unwrap_or_default(),
                            attribute(&element, "width")
                                .and_then(|w| w.parse().ok())
                                .unwrap_or(0),
                        ));
                    }
                    "value" => custom_key = attribute(&element, "key"),
                    _ => {}
                }
                stack.push(name);
            }
            Event::Empty(element) => {
                // `<release version="1.2" …/>` : la première est la plus récente.
                if let Some(component) = current.as_mut()
                    && element.name().as_ref() == b"release"
                    && component.version.is_none()
                {
                    component.version = attribute(&element, "version");
                }
            }
            Event::Text(content) => {
                if current.is_some() && !stack.is_empty() {
                    text.push_str(&content.decode().unwrap_or_default());
                }
            }
            Event::CData(content) => {
                if current.is_some() && !stack.is_empty() {
                    text.push_str(&String::from_utf8_lossy(&content));
                }
            }
            Event::GeneralRef(reference) => {
                if current.is_some()
                    && !stack.is_empty()
                    && let Some(character) = reference.decode().ok().as_deref().and_then(entity)
                {
                    text.push(character);
                }
            }
            Event::End(element) => {
                let name = String::from_utf8_lossy(element.name().as_ref()).into_owned();
                if name == "component" {
                    if let Some(component) = current.take().and_then(|c| c.finish(icons_dir)) {
                        components.push(component);
                    }
                    stack.clear();
                    continue;
                }
                let Some(component) = current.as_mut() else {
                    continue;
                };
                stack.pop();
                let parent = stack.last().map(String::as_str);
                let value = squash(&text);
                match (name.as_str(), parent) {
                    ("id", None) => component.id = value,
                    ("name", None) => component.name.set(lang.as_deref(), value),
                    ("summary", None) => component.summary.set(lang.as_deref(), value),
                    ("developer_name", None) => component.developer.set(lang.as_deref(), value),
                    ("name", Some("developer")) => component.developer.set(lang.as_deref(), value),
                    ("project_license", None) => component.license = Some(value),
                    ("bundle", None) => component.package = value,
                    ("launchable", None) => component.desktop_id = Some(value),
                    ("category", Some("categories")) => component.categories.push(value),
                    ("keyword", Some("keywords")) => {
                        if lang.as_deref().is_none_or(is_french) && !component.keywords.contains(&value) {
                            component.keywords.push(value);
                        }
                    }
                    ("url", None) => {
                        if url_kind.as_deref() == Some("homepage") {
                            component.homepage = Some(value);
                        }
                    }
                    ("icon", None) => {
                        if let Some((kind, size, scale)) = icon.take()
                            && kind == "cached"
                        {
                            component.icons.push((size, scale, value));
                        }
                    }
                    ("p", Some("description")) => description.push(Block::Paragraph(value)),
                    ("li", Some("ul" | "ol")) => description.push(Block::Item(value)),
                    ("description", None) => {
                        let blocks = std::mem::take(&mut description);
                        if !blocks.is_empty() {
                            component.description.set(description_lang.as_deref(), blocks);
                        }
                    }
                    ("caption", Some("screenshot")) => shot_caption.set(lang.as_deref(), value),
                    ("image", Some("screenshot")) => {
                        if let Some((kind, width)) = image_attrs.take() {
                            if kind == "thumbnail" {
                                shot_thumbs.push((width, value));
                            } else {
                                shot_source = Some(value);
                            }
                        }
                    }
                    ("screenshot", Some("screenshots")) => {
                        // Vignette la plus proche de 752 pixels, sinon l'image d'origine.
                        let thumbnail = shot_thumbs
                            .iter()
                            .filter(|(width, _)| *width <= 1000)
                            .max_by_key(|(width, _)| *width)
                            .map(|(_, url)| url.clone())
                            .or_else(|| shot_source.take());
                        if let Some(thumbnail) = thumbnail {
                            let shot = Screenshot {
                                thumbnail,
                                caption: std::mem::take(&mut shot_caption).take().unwrap_or_default(),
                            };
                            if shot_default {
                                component.screenshots.insert(0, shot);
                            } else {
                                component.screenshots.push(shot);
                            }
                        }
                    }
                    ("value", Some("custom"))
                        if custom_key.as_deref() == Some("flathub::verification::verified") =>
                    {
                        component.verified = value == "true";
                    }
                    _ => {}
                }
                text.clear();
            }
            _ => {}
        }
        buffer.clear();
    }
    components
}

// ———————————————————————————— Debian (DEP-11) ————————————————————————————

/// Fichiers DEP-11 téléchargés par apt.
pub fn debian_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir("/var/lib/apt/lists")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("_dep11_Components-") && !name.ends_with(".diff"))
        })
        .collect();
    files.sort();
    files
}

/// Dossiers où les icônes des catalogues Debian sont extraites, selon la version d'AppStream.
const DEBIAN_ICON_ROOTS: [&str; 4] = [
    "/var/lib/swcatalog/icons",
    "/var/cache/swcatalog/icons",
    "/var/lib/app-info/icons",
    "/var/cache/app-info/icons",
];

/// Lit un fichier DEP-11 (compressé ou non).
pub fn load_debian(path: &Path) -> Vec<Component> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut text = String::new();
    let name = path.to_string_lossy();
    let read = if name.ends_with(".gz") {
        flate2::read::GzDecoder::new(file).read_to_string(&mut text)
    } else if name.ends_with(".xz") || name.ends_with(".lz4") || name.ends_with(".zst") {
        tracing::warn!(%name, "catalogue Debian dans un format de compression non lu");
        return Vec::new();
    } else {
        BufReader::new(file).read_to_string(&mut text)
    };
    if let Err(error) = read {
        tracing::warn!(%error, %name, "catalogue Debian illisible");
        return Vec::new();
    }
    let roots: Vec<PathBuf> = DEBIAN_ICON_ROOTS.iter().map(PathBuf::from).collect();
    parse_dep11(&text, &roots)
}

fn yaml_str(node: &Yaml) -> Option<String> {
    match node {
        Yaml::String(text) => Some(text.clone()),
        Yaml::Integer(number) => Some(number.to_string()),
        Yaml::Real(text) => Some(text.clone()),
        Yaml::Boolean(value) => Some(value.to_string()),
        _ => None,
    }
}

/// `{C: …, fr: …}` → le français, sinon le texte d'origine.
fn yaml_localized(node: &Yaml) -> Option<&Yaml> {
    let hash = node.as_hash()?;
    let mut default = None;
    for (key, value) in hash {
        let Some(lang) = key.as_str() else { continue };
        if is_french(lang) {
            return Some(value);
        }
        if lang == "C" {
            default = Some(value);
        }
    }
    default
}

fn yaml_list(node: &Yaml) -> Vec<String> {
    node.as_vec()
        .map(|list| list.iter().filter_map(yaml_str).collect())
        .unwrap_or_default()
}

/// Découpe une description AppStream (`<p>…</p><ul><li>…</li></ul>`) en paragraphes et
/// éléments de liste. Les autres balises (`<em>`, `<code>`) sont retirées.
pub fn parse_markup(markup: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut rest = markup;
    let mut current: Option<(bool, String)> = None;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('<') {
            let end = after.find('>').unwrap_or(after.len());
            let tag = after[..end].trim().to_ascii_lowercase();
            rest = after.get(end + 1..).unwrap_or("");
            let name = tag
                .trim_start_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or("");
            match (tag.starts_with('/'), name) {
                (false, "p") => current = Some((false, String::new())),
                (false, "li") => current = Some((true, String::new())),
                (true, "p" | "li") => {
                    if let Some((item, text)) = current.take() {
                        let text = squash(&unescape_html(&text));
                        if !text.is_empty() {
                            blocks.push(if item {
                                Block::Item(text)
                            } else {
                                Block::Paragraph(text)
                            });
                        }
                    }
                }
                _ => {}
            }
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            if let Some((_, text)) = current.as_mut() {
                text.push_str(&rest[..end]);
            } else if !rest[..end].trim().is_empty() {
                // Texte sans balise : un paragraphe à lui seul.
                blocks.push(Block::Paragraph(squash(&unescape_html(&rest[..end]))));
            }
            rest = &rest[end..];
        }
    }
    blocks
}

fn unescape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after
            .find(';')
            .filter(|end| *end <= 8)
            .and_then(|end| Some((entity(&after[..end])?, end)))
        {
            Some((character, end)) => {
                out.push(character);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn parse_dep11(text: &str, icon_roots: &[PathBuf]) -> Vec<Component> {
    let documents = match YamlLoader::load_from_str(text) {
        Ok(documents) => documents,
        Err(error) => {
            tracing::warn!(%error, "catalogue Debian mal formé");
            return Vec::new();
        }
    };
    let mut iter = documents.iter();
    let Some(header) = iter.next() else {
        return Vec::new();
    };
    let origin = header["Origin"].as_str().unwrap_or_default().to_owned();
    let media = header["MediaBaseUrl"]
        .as_str()
        .unwrap_or("https://appstream.debian.org/media/pool");
    let media = media.trim_end_matches('/').to_owned();
    let mut components = Vec::new();
    for document in iter {
        let kind = document["Type"].as_str().unwrap_or_default();
        if !matches!(kind, "desktop-application" | "desktop") {
            continue;
        }
        let (Some(id), Some(package)) = (document["ID"].as_str(), document["Package"].as_str()) else {
            continue;
        };
        let Some(name) = yaml_localized(&document["Name"]).and_then(yaml_str) else {
            continue;
        };
        let description = yaml_localized(&document["Description"])
            .and_then(yaml_str)
            .map(|markup| parse_markup(&markup))
            .unwrap_or_default();
        let keywords = yaml_localized(&document["Keywords"])
            .map(yaml_list)
            .unwrap_or_default();
        // Icônes en cache : la plus grande jusqu'à 128 pixels.
        let mut cached: Vec<(u32, u32, String)> = document["Icon"]["cached"]
            .as_vec()
            .map(|list| {
                list.iter()
                    .filter_map(|icon| {
                        let file = icon["name"].as_str()?.to_owned();
                        let width = icon["width"].as_i64().unwrap_or(0) as u32;
                        let scale = icon["scale"].as_i64().unwrap_or(1) as u32;
                        Some((width, scale, file))
                    })
                    .collect()
            })
            .unwrap_or_default();
        cached.sort_by_key(|(width, scale, _)| std::cmp::Reverse(width * scale));
        let icon = cached
            .iter()
            .filter(|(width, _, _)| *width <= 128)
            .find_map(|(width, scale, file)| {
                let folder = if *scale > 1 {
                    format!("{width}x{width}@{scale}")
                } else {
                    format!("{width}x{width}")
                };
                icon_roots
                    .iter()
                    .map(|root| root.join(&origin).join(&folder).join(file))
                    .find(|path| path.is_file())
            });
        let mut screenshots = Vec::new();
        for shot in document["Screenshots"].as_vec().into_iter().flatten() {
            let thumbnail = shot["thumbnails"]
                .as_vec()
                .and_then(|thumbs| {
                    thumbs
                        .iter()
                        .filter_map(|thumb| Some((thumb["width"].as_i64()?, thumb["url"].as_str()?)))
                        .filter(|(width, _)| *width <= 1000)
                        .max_by_key(|(width, _)| *width)
                        .map(|(_, url)| url.to_owned())
                })
                .or_else(|| shot["source-image"]["url"].as_str().map(str::to_owned));
            let Some(thumbnail) = thumbnail else { continue };
            let thumbnail = if thumbnail.starts_with("http") {
                thumbnail
            } else {
                format!("{media}/{}", thumbnail.trim_start_matches('/'))
            };
            let caption = yaml_localized(&shot["caption"])
                .and_then(yaml_str)
                .unwrap_or_default();
            let entry = Screenshot { thumbnail, caption };
            if shot["default"].as_bool() == Some(true) {
                screenshots.insert(0, entry);
            } else {
                screenshots.push(entry);
            }
        }
        let desktop_id = document["Launchable"]["desktop-id"]
            .as_vec()
            .and_then(|list| list.first())
            .and_then(yaml_str);
        let developer = yaml_localized(&document["Developer"]["name"])
            .or_else(|| yaml_localized(&document["DeveloperName"]))
            .and_then(yaml_str)
            .unwrap_or_default();
        components.push(Component {
            id: normalize_id(id),
            origin: Some(Origin::Debian),
            package: package.to_owned(),
            name: squash(&name),
            summary: yaml_localized(&document["Summary"])
                .and_then(yaml_str)
                .map(|s| squash(&s))
                .unwrap_or_default(),
            developer,
            description,
            categories: yaml_list(&document["Categories"]),
            keywords,
            icon,
            screenshots,
            homepage: document["Url"]["homepage"].as_str().map(str::to_owned),
            license: document["ProjectLicense"].as_str().map(str::to_owned),
            version: None,
            desktop_id: desktop_id.or_else(|| Some(format!("{}.desktop", normalize_id(id)))),
            verified: false,
        });
    }
    components
}

// ———————————————————————————— Catalogue réuni ————————————————————————————

/// Une application, avec une ou deux origines (Debian et Flathub).
#[derive(Debug, Clone)]
pub struct App {
    pub id: String,
    /// Versions disponibles, Debian d'abord.
    pub variants: Vec<Component>,
}

impl App {
    /// Description principale : celle de l'origine préférée (`origin`), sinon la première.
    pub fn main(&self) -> &Component {
        &self.variants[0]
    }
    pub fn variant(&self, origin: Origin) -> Option<&Component> {
        self.variants.iter().find(|variant| variant.origin() == origin)
    }
    pub fn name(&self) -> &str {
        &self.main().name
    }
}

/// Réunit les applications des deux origines par identifiant. Une application connue des
/// deux garde ses deux versions ; la fiche laisse choisir.
pub fn merge(components: Vec<Component>) -> Vec<App> {
    let mut by_id: HashMap<String, App> = HashMap::new();
    let mut order = Vec::new();
    for component in components {
        let key = component.id.to_lowercase();
        match by_id.get_mut(&key) {
            Some(app) => {
                // Un même paquet Debian décrit deux fois (versions de la distribution) :
                // on garde la première.
                if app.variant(component.origin()).is_none() {
                    app.variants.push(component);
                    app.variants.sort_by_key(Component::origin);
                }
            }
            None => {
                order.push(key.clone());
                by_id.insert(
                    key,
                    App {
                        id: component.id.clone(),
                        variants: vec![component],
                    },
                );
            }
        }
    }
    let mut apps: Vec<App> = order.into_iter().filter_map(|key| by_id.remove(&key)).collect();
    apps.sort_by_cached_key(|app| fold(app.name()));
    apps
}

/// Minuscules sans accents, pour chercher et trier.
pub fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ÿ' => 'y',
            'ñ' => 'n',
            'œ' => 'o',
            other => other,
        })
        .collect()
}

/// Pertinence d'une application pour une recherche (0 : ne correspond pas).
pub fn score(app: &App, query: &str) -> u32 {
    let words: Vec<String> = fold(query).split_whitespace().map(str::to_owned).collect();
    if words.is_empty() {
        return 0;
    }
    let main = app.main();
    let name = fold(&main.name);
    let summary = fold(&main.summary);
    let id = fold(&app.id);
    let keywords: Vec<String> = app
        .variants
        .iter()
        .flat_map(|variant| variant.keywords.iter())
        .map(|keyword| fold(keyword))
        .collect();
    let packages: Vec<String> = app
        .variants
        .iter()
        .map(|variant| fold(&variant.package))
        .collect();
    let mut total = 0;
    for word in &words {
        let mut best = 0;
        if name == *word {
            best = 100;
        } else if name
            .split(|c: char| !c.is_alphanumeric())
            .any(|part| part.starts_with(word.as_str()))
        {
            best = 60;
        } else if name.contains(word.as_str()) {
            best = 40;
        }
        if best < 30
            && (keywords.iter().any(|k| k.contains(word.as_str())) || packages.iter().any(|p| p == word))
        {
            best = 30;
        }
        if best < 20 && summary.contains(word.as_str()) {
            best = 20;
        }
        if best < 10 && id.contains(word.as_str()) {
            best = 10;
        }
        if best == 0 {
            return 0;
        }
        total += best;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLATHUB: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<components version="0.8" origin="flathub">
  <component type="runtime"><id>org.gnome.Platform</id><name>GNOME</name></component>
  <component type="desktop-application">
    <id>org.videolan.VLC</id>
    <name>VLC</name>
    <name xml:lang="fr">VLC (fr)</name>
    <summary>VLC media player</summary>
    <summary xml:lang="fr">Lecteur multimédia VLC</summary>
    <description>
      <p>VLC plays everything &amp; more.</p>
      <ul><li>DVD</li><li>Streams</li></ul>
    </description>
    <description xml:lang="fr">
      <p>VLC lit tout &amp; plus.</p>
      <ul><li>DVD</li></ul>
    </description>
    <developer><name>VideoLAN</name></developer>
    <launchable type="desktop-id">org.videolan.VLC.desktop</launchable>
    <icon type="cached" width="64" height="64">org.videolan.VLC.png</icon>
    <icon type="cached" width="128" height="128">org.videolan.VLC.png</icon>
    <url type="bugtracker">https://bugs</url>
    <url type="homepage">https://www.videolan.org</url>
    <categories><category>AudioVideo</category><category>Player</category></categories>
    <keywords><keyword>dvd</keyword><keyword xml:lang="de">film</keyword></keywords>
    <project_license>GPL-2.0+</project_license>
    <screenshots>
      <screenshot>
        <caption>Second</caption>
        <image type="source" width="1920">https://x/2_orig.png</image>
      </screenshot>
      <screenshot type="default">
        <caption>First</caption>
        <caption xml:lang="fr">Première</caption>
        <image type="source" width="1920">https://x/1_orig.png</image>
        <image type="thumbnail" width="752">https://x/1_752.png</image>
        <image type="thumbnail" width="224">https://x/1_224.png</image>
      </screenshot>
    </screenshots>
    <releases><release version="3.0.21" timestamp="1"/><release version="3.0.20" timestamp="0"/></releases>
    <bundle type="flatpak" runtime="r">app/org.videolan.VLC/x86_64/stable</bundle>
    <custom><value key="flathub::verification::verified">true</value></custom>
  </component>
</components>"#;

    #[test]
    fn flathub_lu_en_francais() {
        let dir = std::env::temp_dir().join(format!("ferrite-logi-{}", std::process::id()));
        fs::create_dir_all(dir.join("128x128")).unwrap();
        fs::write(dir.join("128x128/org.videolan.VLC.png"), b"png").unwrap();
        let components = parse_flathub(FLATHUB.as_bytes(), &dir);
        assert_eq!(components.len(), 1, "le runtime est ignoré");
        let vlc = &components[0];
        assert_eq!(vlc.id, "org.videolan.VLC");
        assert_eq!(vlc.name, "VLC (fr)");
        assert_eq!(vlc.summary, "Lecteur multimédia VLC");
        assert_eq!(
            vlc.description,
            vec![
                Block::Paragraph("VLC lit tout & plus.".into()),
                Block::Item("DVD".into())
            ]
        );
        assert_eq!(vlc.developer, "VideoLAN");
        assert_eq!(vlc.package, "app/org.videolan.VLC/x86_64/stable");
        assert_eq!(vlc.homepage.as_deref(), Some("https://www.videolan.org"));
        assert_eq!(vlc.categories, ["AudioVideo", "Player"]);
        assert_eq!(vlc.keywords, ["dvd"]);
        assert_eq!(vlc.version.as_deref(), Some("3.0.21"));
        assert_eq!(
            vlc.icon.as_deref(),
            Some(dir.join("128x128/org.videolan.VLC.png").as_path())
        );
        assert!(vlc.verified);
        assert_eq!(vlc.screenshots.len(), 2);
        assert_eq!(
            vlc.screenshots[0].thumbnail, "https://x/1_752.png",
            "la capture par défaut en premier"
        );
        assert_eq!(vlc.screenshots[0].caption, "Première");
        assert_eq!(vlc.screenshots[1].thumbnail, "https://x/2_orig.png");
        fs::remove_dir_all(dir).ok();
    }

    const DEP11: &str = r#"---
File: DEP-11
Version: '1.0'
Origin: debian-trixie-main
MediaBaseUrl: https://appstream.debian.org/media/pool
---
Type: desktop-application
ID: org.gnome.Calculator
Package: gnome-calculator
Name:
  C: Calculator
  fr: Calculatrice
Summary:
  C: Perform arithmetic
  fr_FR: Effectuer des calculs
Description:
  C: >-
    <p>Calculator solves &amp; more.</p>
    <ul>
      <li>Basic</li>
    </ul>
  fr: >-
    <p>La Calculatrice résout
    tout.</p>
    <ul>
      <li>Simple</li>
      <li>Avancée</li>
    </ul>
Categories:
- Utility
- Calculator
Keywords:
  C:
  - calculation
Icon:
  cached:
  - name: gnome-calculator_org.gnome.Calculator.png
    width: 64
    height: 64
  - name: gnome-calculator_org.gnome.Calculator.png
    width: 128
    height: 128
  stock: accessories-calculator
Url:
  homepage: https://apps.gnome.org/Calculator
ProjectLicense: GPL-3.0+
Launchable:
  desktop-id:
  - org.gnome.Calculator.desktop
Screenshots:
- caption:
    C: Basic mode
  thumbnails:
  - url: org/gnome/Calculator/a/image-1_224x126.png
    width: 224
  - url: org/gnome/Calculator/a/image-1_752x423.png
    width: 752
  source-image:
    url: org/gnome/Calculator/a/image-1_orig.png
- default: true
  source-image:
    url: org/gnome/Calculator/a/image-2_orig.png
---
Type: addon
ID: org.gnome.Calculator.plugin
Package: gnome-calculator-plugin
Name:
  C: Plugin
---
Type: desktop-application
ID: gimp.desktop
Package: gimp
Name:
  C: GIMP
Summary:
  C: Image editor
"#;

    #[test]
    fn debian_lu_en_francais() {
        let root = std::env::temp_dir().join(format!("ferrite-dep11-{}", std::process::id()));
        let icons = root.join("debian-trixie-main/128x128");
        fs::create_dir_all(&icons).unwrap();
        fs::write(icons.join("gnome-calculator_org.gnome.Calculator.png"), b"png").unwrap();
        let components = parse_dep11(DEP11, std::slice::from_ref(&root));
        assert_eq!(components.len(), 2, "l'extension est ignorée");
        let calc = &components[0];
        assert_eq!(calc.id, "org.gnome.Calculator");
        assert_eq!(calc.package, "gnome-calculator");
        assert_eq!(calc.name, "Calculatrice");
        assert_eq!(calc.summary, "Effectuer des calculs");
        assert_eq!(
            calc.description,
            vec![
                Block::Paragraph("La Calculatrice résout tout.".into()),
                Block::Item("Simple".into()),
                Block::Item("Avancée".into())
            ]
        );
        assert_eq!(calc.categories, ["Utility", "Calculator"]);
        assert_eq!(calc.keywords, ["calculation"]);
        assert_eq!(calc.license.as_deref(), Some("GPL-3.0+"));
        assert_eq!(calc.desktop_id.as_deref(), Some("org.gnome.Calculator.desktop"));
        assert!(
            calc.icon
                .as_ref()
                .is_some_and(|icon| icon.ends_with("128x128/gnome-calculator_org.gnome.Calculator.png"))
        );
        assert_eq!(calc.screenshots.len(), 2);
        assert_eq!(
            calc.screenshots[0].thumbnail,
            "https://appstream.debian.org/media/pool/org/gnome/Calculator/a/image-2_orig.png"
        );
        assert_eq!(
            calc.screenshots[1].thumbnail,
            "https://appstream.debian.org/media/pool/org/gnome/Calculator/a/image-1_752x423.png"
        );
        let gimp = &components[1];
        assert_eq!(gimp.id, "gimp");
        assert_eq!(gimp.desktop_id.as_deref(), Some("gimp.desktop"));
        assert!(gimp.icon.is_none());
        fs::remove_dir_all(root).ok();
    }

    /// Lit les vrais catalogues de la machine : `cargo test -- --ignored vrai_catalogue`.
    #[test]
    #[ignore = "lit les catalogues installés"]
    fn vrai_catalogue() {
        let start = std::time::Instant::now();
        let mut components: Vec<Component> =
            debian_files().iter().flat_map(|file| load_debian(file)).collect();
        let debian = components.len();
        let debian_time = start.elapsed();
        components.extend(flathub_dir().map(|dir| load_flathub(&dir)).unwrap_or_default());
        let flathub = components.len() - debian;
        let with_icon = components
            .iter()
            .filter(|component| component.icon.is_some())
            .count();
        let apps = merge(components);
        eprintln!(
            "Debian {debian} ({debian_time:?}), Flathub {flathub}, icônes {with_icon}, réunies {} en {:?}",
            apps.len(),
            start.elapsed()
        );
        for id in [
            "org.videolan.VLC",
            "com.spotify.Client",
            "org.gimp.GIMP",
            "org.libreoffice.LibreOffice",
        ] {
            if let Some(app) = apps.iter().find(|app| app.id.eq_ignore_ascii_case(id)) {
                let main = app.main();
                eprintln!(
                    "{id}: {} — {} [{}] {} captures, icône {:?}",
                    main.name,
                    main.summary,
                    app.variants
                        .iter()
                        .map(|v| v.origin().label())
                        .collect::<Vec<_>>()
                        .join("+"),
                    main.screenshots.len(),
                    main.icon
                );
            }
        }
    }

    #[test]
    fn description_en_blocs() {
        assert_eq!(
            parse_markup("<p>Un <em>bon</em> outil.</p><ol><li>A &lt; B</li></ol>Fin"),
            vec![
                Block::Paragraph("Un bon outil.".into()),
                Block::Item("A < B".into()),
                Block::Paragraph("Fin".into())
            ]
        );
    }

    fn component(id: &str, origin: Origin, name: &str) -> Component {
        Component {
            id: id.into(),
            origin: Some(origin),
            package: id.to_lowercase(),
            name: name.into(),
            ..Component::default()
        }
    }

    #[test]
    fn deux_origines_reunies() {
        let apps = merge(vec![
            component("org.videolan.VLC", Origin::Flathub, "VLC"),
            component("org.gimp.GIMP", Origin::Debian, "GIMP"),
            component("org.videolan.VLC", Origin::Debian, "VLC"),
            component("org.videolan.VLC", Origin::Debian, "VLC bis"),
        ]);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name(), "GIMP");
        let vlc = &apps[1];
        assert_eq!(vlc.variants.len(), 2);
        assert_eq!(vlc.main().origin(), Origin::Debian, "Debian d'abord");
        assert_eq!(vlc.main().name, "VLC");
    }

    #[test]
    fn recherche_sans_accents() {
        let mut app = merge(vec![component(
            "org.gnome.Calculator",
            Origin::Debian,
            "Calculatrice",
        )])
        .remove(0);
        app.variants[0].summary = "Effectuer des opérations".into();
        app.variants[0].keywords = vec!["math".into()];
        assert!(score(&app, "calc") > score(&app, "operations"));
        assert!(score(&app, "OPÉRATIONS") > 0);
        assert!(score(&app, "math") > 0);
        assert_eq!(score(&app, "calc photo"), 0, "tous les mots doivent correspondre");
        assert_eq!(score(&app, "  "), 0);
        assert_eq!(fold("Éditeur Œuvre"), "editeur ouvre");
    }
}
