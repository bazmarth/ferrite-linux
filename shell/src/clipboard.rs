//! Historique du presse-papiers, comme Windows + V : chaque copie (texte ou image PNG)
//! est gardée, jusqu'à 25 éléments, avec l'application d'où elle vient ; Super+V affiche
//! la liste (recherche, filtres, épinglés en haut) et colle l'élément choisi dans
//! l'application active. Les éléments épinglés restent après un redémarrage
//! (`~/.local/share/ferrite/presse-papiers`), les autres s'effacent avec la session.
//!
//! Le compositeur lit le contenu copié par un tube (`wl_data_source` ou propriétaire X11)
//! dans un fil à part : un client lent ne fige jamais le bureau.

use std::{
    env, fs,
    io::{self, Read, Write},
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime},
};

use smithay::{
    backend::input::KeyState,
    input::keyboard::{FilterResult, Keycode},
    reexports::calloop::{
        LoopHandle, channel,
        timer::{TimeoutAction, Timer},
    },
    utils::SERIAL_COUNTER,
    wayland::selection::{
        SelectionTarget,
        data_device::{request_data_device_client_selection, set_data_device_selection},
    },
};
use tracing::{info, warn};

use crate::state::Ferrite;

/// Éléments gardés (épinglés compris), comme sous Windows.
pub const MAX_ENTRIES: usize = 25;
/// Taille maximale d'un élément.
const MAX_BYTES: usize = 4 << 20;
/// Taille maximale de la vignette d'une image, en pixels.
const THUMBNAIL: (u32, u32) = (200, 112);

/// Types de texte, du plus précis au plus ancien (X11).
const TEXT_TYPES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "STRING",
    "TEXT",
];
/// Indication des gestionnaires de mots de passe : ce contenu ne doit pas être gardé.
const SECRET_HINT: &str = "x-kde-passwordManagerHint";

/// Vignette d'image en BGRA prémultiplié, prête à être dessinée.
#[derive(Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<Vec<u8>>,
}

impl std::fmt::Debug for Thumbnail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Thumbnail({}×{})", self.width, self.height)
    }
}

/// Contenu d'un élément copié.
#[derive(Clone, PartialEq, Eq)]
pub enum Content {
    Text(Arc<str>),
    Image { png: Arc<Vec<u8>>, thumbnail: Thumbnail },
}

impl std::fmt::Debug for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Content::Text(text) => write!(f, "Text({} caractères)", text.chars().count()),
            Content::Image { thumbnail, .. } => write!(f, "Image({thumbnail:?})"),
        }
    }
}

impl Content {
    /// Types proposés aux applications quand cet élément est collé.
    pub fn mime_types(&self) -> Vec<String> {
        match self {
            Content::Text(_) => TEXT_TYPES.iter().map(|mime| mime.to_string()).collect(),
            Content::Image { .. } => vec!["image/png".to_owned()],
        }
    }

    /// Octets à écrire pour le type `mime` demandé.
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Content::Text(text) => text.as_bytes().to_vec(),
            Content::Image { png, .. } => png.as_ref().clone(),
        }
    }
}

/// Propriétaire de la sélection posée par le compositeur.
#[derive(Debug, Clone)]
pub enum Owner {
    /// Une application X11 (via XWayland).
    X11,
    /// Un élément de l'historique, collé avec Super+V.
    History(Content),
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub content: Content,
    pub pinned: bool,
    /// Application d'où vient la copie (« Terminal »), si on la connaît.
    pub source: Option<String>,
    pub copied: SystemTime,
}

/// Nature d'un élément, d'après son contenu : elle choisit sa tuile, sa police et son filtre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Code,
    Color,
    Link,
    Files,
    Image,
}

/// Filtres du panneau, dans l'ordre de leurs pastilles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Text,
    Links,
    Images,
    Files,
}

impl Filter {
    pub const ALL: [Filter; 5] = [
        Filter::All,
        Filter::Text,
        Filter::Links,
        Filter::Images,
        Filter::Files,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "Tout",
            Filter::Text => "Texte",
            Filter::Links => "Liens",
            Filter::Images => "Images",
            Filter::Files => "Fichiers",
        }
    }

    fn accepts(self, kind: Kind) -> bool {
        match self {
            Filter::All => true,
            Filter::Text => matches!(kind, Kind::Text | Kind::Code | Kind::Color),
            Filter::Links => kind == Kind::Link,
            Filter::Images => kind == Kind::Image,
            Filter::Files => kind == Kind::Files,
        }
    }
}

impl Entry {
    pub fn kind(&self) -> Kind {
        let Content::Text(text) = &self.content else {
            return Kind::Image;
        };
        let text = text.trim();
        let single = !text.contains('\n');
        if single && is_color(text) {
            Kind::Color
        } else if !file_paths(text).is_empty() {
            Kind::Files
        } else if single
            && !text.contains(' ')
            && ["http://", "https://", "ftp://", "mailto:", "www."]
                .iter()
                .any(|scheme| text.starts_with(scheme))
        {
            Kind::Link
        } else if self.source.as_deref() == Some("Terminal") || looks_like_code(text) {
            Kind::Code
        } else {
            Kind::Text
        }
    }

    /// Texte à chercher : jamais le contenu d'une image, mais son application d'origine.
    fn matches(&self, query: &str) -> bool {
        let in_text = match &self.content {
            Content::Text(text) => text.to_lowercase().contains(query),
            Content::Image { .. } => "image capture".contains(query),
        };
        in_text
            || self
                .source
                .as_ref()
                .is_some_and(|source| source.to_lowercase().contains(query))
    }
}

/// Couleur `#RGB`, `#RRGGBB` ou `#RRGGBBAA`.
pub fn is_color(text: &str) -> bool {
    text.strip_prefix('#')
        .is_some_and(|hex| matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Fichiers copiés : une adresse `file://` par ligne (Fichiers, `text/uri-list`) ou des
/// chemins absolus qui existent. Vide si le texte n'est pas que ça.
pub fn file_paths(text: &str) -> Vec<PathBuf> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let paths: Vec<PathBuf> = lines
        .iter()
        .filter_map(|line| match line.strip_prefix("file://") {
            Some(rest) => Some(PathBuf::from(percent_decode(rest))),
            None => (line.starts_with('/') && Path::new(line).exists()).then(|| PathBuf::from(line)),
        })
        .collect();
    if paths.len() == lines.len() {
        paths
    } else {
        Vec::new()
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(value)) => {
                out.push(value);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Commande ou bout de programme plutôt que phrase.
fn looks_like_code(text: &str) -> bool {
    const MARKS: [&str; 10] = ["{", "};", "();", "=>", "::", " --", "&&", "$(", "#include", "fn "];
    let first = text.lines().next().unwrap_or_default();
    MARKS.iter().any(|mark| text.contains(mark))
        || ["cargo ", "git ", "sudo ", "cd ", "ls ", "dnf ", "npm ", "./"]
            .iter()
            .any(|command| first.starts_with(command))
}

/// Âge d'une copie, comme sous la maquette : « il y a 2 min », « hier ».
pub fn ago(now: SystemTime, then: SystemTime) -> String {
    let seconds = now.duration_since(then).unwrap_or(Duration::ZERO).as_secs();
    match seconds {
        0..60 => "à l’instant".to_owned(),
        60..3_600 => format!("il y a {} min", seconds / 60),
        3_600..86_400 => format!("il y a {} h", seconds / 3_600),
        86_400..172_800 => "hier".to_owned(),
        _ => format!("il y a {} j", seconds / 86_400),
    }
}

#[derive(Debug, Default)]
pub struct History {
    entries: Vec<Entry>,
    /// Fichier des éléments épinglés ; `None` dans les tests.
    path: Option<PathBuf>,
}

impl History {
    /// Historique de la session, avec les éléments épinglés lors des précédentes.
    pub fn load() -> Self {
        let path = data_path();
        let entries = fs::read_to_string(&path)
            .map(|text| parse_pinned(&text))
            .unwrap_or_default()
            .into_iter()
            .map(|text| Entry {
                content: Content::Text(text.into()),
                pinned: true,
                source: None,
                copied: SystemTime::now(),
            })
            .collect();
        Self {
            entries,
            path: Some(path),
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Nouvelle copie, faite dans `source` : en tête de liste. Un contenu déjà présent
    /// remonte en tête au lieu d'apparaître deux fois.
    pub fn add(&mut self, content: Content, source: Option<String>) {
        let pinned = match self.entries.iter().position(|entry| entry.content == content) {
            Some(index) => self.entries.remove(index).pinned,
            None => false,
        };
        self.entries.insert(
            0,
            Entry {
                content,
                pinned,
                source,
                copied: SystemTime::now(),
            },
        );
        // Au-delà de la limite, les plus anciens non épinglés s'en vont.
        while self.entries.len() > MAX_ENTRIES {
            match self.entries.iter().rposition(|entry| !entry.pinned) {
                Some(index) => {
                    self.entries.remove(index);
                }
                None => break,
            }
        }
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            let pinned = self.entries.remove(index).pinned;
            if pinned {
                self.save();
            }
        }
    }

    pub fn toggle_pin(&mut self, index: usize) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.pinned = !entry.pinned;
            self.save();
        }
    }

    /// « Tout effacer » : comme sous Windows, les éléments épinglés restent.
    pub fn clear(&mut self) {
        self.entries.retain(|entry| entry.pinned);
    }

    /// Éléments montrés pour la recherche `query` et le filtre `filter`, dans l'ordre du
    /// panneau : les épinglés d'abord, puis les récents.
    pub fn shown(&self, query: &str, filter: Filter) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        let keep =
            |entry: &Entry| filter.accepts(entry.kind()) && (query.is_empty() || entry.matches(&query));
        let (pinned, recent): (Vec<usize>, Vec<usize>) = (0..self.entries.len())
            .filter(|&index| keep(&self.entries[index]))
            .partition(|&index| self.entries[index].pinned);
        pinned.into_iter().chain(recent).collect()
    }

    /// Copie de la liste, pour annuler une suppression.
    pub fn snapshot(&self) -> Vec<Entry> {
        self.entries.clone()
    }

    pub fn restore(&mut self, entries: Vec<Entry>) {
        self.entries = entries;
        self.save();
    }

    /// Élément collé : il remonte en tête.
    pub fn promote(&mut self, index: usize) {
        if index < self.entries.len() {
            let entry = self.entries.remove(index);
            self.entries.insert(0, entry);
        }
    }

    /// Les textes épinglés sont écrits sur le disque (les images restent en mémoire).
    fn save(&self) {
        let Some(path) = &self.path else { return };
        let mut text = String::new();
        for entry in self.entries.iter().filter(|entry| entry.pinned) {
            if let Content::Text(value) = &entry.content {
                text.push_str(&format!("{}\n{value}\n", value.len()));
            }
        }
        let result = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(path, text));
        if let Err(err) = result {
            warn!(%err, "éléments épinglés du presse-papiers non enregistrés");
        }
    }
}

fn data_path() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("ferrite/presse-papiers")
}

/// Fichier des éléments épinglés : pour chacun, sa longueur en octets puis le texte.
fn parse_pinned(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut rest = text;
    while let Some((length, after)) = rest.split_once('\n') {
        let Ok(length) = length.trim().parse::<usize>() else {
            break;
        };
        let Some(value) = after.get(..length) else { break };
        items.push(value.to_owned());
        rest = after.get(length + 1..).unwrap_or("");
    }
    items
}

/// Canal des copies lues, avec leur application d'origine.
pub type Copies = channel::Sender<(Content, Option<String>)>;

/// Ce qu'on lit d'une sélection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wanted {
    Text(String),
    Image(String),
}

impl Wanted {
    pub fn mime(&self) -> &str {
        match self {
            Wanted::Text(mime) | Wanted::Image(mime) => mime,
        }
    }
}

/// Type à lire parmi ceux proposés : le texte d'abord, sinon une image PNG. Rien pour un
/// mot de passe copié depuis un gestionnaire de mots de passe.
pub fn choose(mime_types: &[String]) -> Option<Wanted> {
    if mime_types.iter().any(|mime| mime == SECRET_HINT) {
        return None;
    }
    TEXT_TYPES
        .iter()
        .find(|wanted| mime_types.iter().any(|mime| mime == *wanted))
        .map(|mime| Wanted::Text(mime.to_string()))
        .or_else(|| {
            mime_types
                .iter()
                .any(|mime| mime == "image/png")
                .then(|| Wanted::Image("image/png".to_owned()))
        })
}

/// Contenu lu, converti ; `None` pour un texte vide ou une image illisible.
pub fn decode(wanted: &Wanted, bytes: Vec<u8>) -> Option<Content> {
    match wanted {
        Wanted::Text(_) => {
            let text = String::from_utf8(bytes)
                .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
            (!text.trim().is_empty()).then(|| Content::Text(text.into()))
        }
        Wanted::Image(_) => {
            let thumbnail = thumbnail(&bytes)?;
            Some(Content::Image {
                png: Arc::new(bytes),
                thumbnail,
            })
        }
    }
}

/// Tube de lecture : l'extrémité d'écriture va au propriétaire de la sélection, la
/// lecture se fait dans un fil qui envoie le contenu au compositeur.
// `io::pipe` date de Rust 1.87 ; Ferrite est compilé avec Rust stable via rustup
// (voir DECISIONS.md), plus récent que la version minimale déclarée.
#[allow(clippy::incompatible_msrv)]
pub fn read_selection(wanted: Wanted, sender: Copies, source: Option<String>) -> io::Result<OwnedFd> {
    let (mut reader, writer) = io::pipe()?;
    thread::Builder::new()
        .name("ferrite-presse-papiers".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            if (&mut reader)
                .take(MAX_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .is_err()
                || bytes.len() > MAX_BYTES
            {
                return;
            }
            if let Some(content) = decode(&wanted, bytes) {
                let _ = sender.send((content, source));
            }
        })?;
    Ok(OwnedFd::from(writer))
}

/// Écrit `bytes` dans le tube d'une application qui colle, sans bloquer le compositeur.
pub fn write_selection(fd: OwnedFd, bytes: Vec<u8>) {
    let spawned = thread::Builder::new()
        .name("ferrite-coller".into())
        .spawn(move || {
            let mut file = fs::File::from(fd);
            let _ = file.write_all(&bytes);
        });
    if let Err(err) = spawned {
        warn!(%err, "presse-papiers : écriture impossible");
    }
}

/// Vignette d'une image PNG, réduite pour tenir dans [`THUMBNAIL`].
fn thumbnail(png_bytes: &[u8]) -> Option<Thumbnail> {
    let mut decoder = png::Decoder::new(io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let (width, height) = (info.width, info.height);
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    if width == 0 || height == 0 {
        return None;
    }
    let ratio = (THUMBNAIL.0 as f64 / width as f64)
        .min(THUMBNAIL.1 as f64 / height as f64)
        .min(1.0);
    let (out_w, out_h) = (
        ((width as f64 * ratio).round() as u32).max(1),
        ((height as f64 * ratio).round() as u32).max(1),
    );
    let mut pixels = Vec::with_capacity((out_w * out_h * 4) as usize);
    for y in 0..out_h {
        for x in 0..out_w {
            let sx = ((x as f64 + 0.5) / ratio) as u32;
            let sy = ((y as f64 + 0.5) / ratio) as u32;
            let i = ((sy.min(height - 1) * width + sx.min(width - 1)) as usize) * channels;
            let source = &buffer[i..i + channels];
            let (r, g, b, a) = match channels {
                4 => (source[0], source[1], source[2], source[3]),
                3 => (source[0], source[1], source[2], 255),
                2 => (source[0], source[0], source[0], source[1]),
                _ => (source[0], source[0], source[0], 255),
            };
            let premultiply = |c: u8| (c as u32 * a as u32 / 255) as u8;
            pixels.extend_from_slice(&[premultiply(b), premultiply(g), premultiply(r), a]);
        }
    }
    Some(Thumbnail {
        width: out_w,
        height: out_h,
        pixels: Arc::new(pixels),
    })
}

/// Liste du panneau Super+V, en points : titre de section, ligne (plus haute si son texte
/// tient sur deux lignes) et écart entre deux lignes.
pub const SECTION: f64 = 31.0;
pub const ROW: f64 = 64.0;
pub const ROW_TALL: f64 = 82.0;
pub const ROW_GAP: f64 = 2.0;
/// Caractères au-delà desquels un texte passe sur deux lignes.
const ONE_LINE: usize = 34;

/// Ligne placée dans la liste : élément, ordonnée depuis le haut de la liste, hauteur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub index: usize,
    pub y: f64,
    pub h: f64,
}

/// Liste disposée : lignes, titres de section (texte, ordonnée) et hauteur totale.
#[derive(Debug, Default)]
pub struct Placement {
    pub rows: Vec<Placed>,
    pub sections: Vec<(&'static str, f64)>,
    pub height: f64,
}

impl History {
    /// Dispose les éléments `shown` (voir [`History::shown`]) : section « Épinglés » puis
    /// « Récents ».
    pub fn place(&self, shown: &[usize]) -> Placement {
        let mut placement = Placement::default();
        let mut y = 0.0;
        let mut pinned_section = None;
        for &index in shown {
            let entry = &self.entries[index];
            if pinned_section != Some(entry.pinned) {
                pinned_section = Some(entry.pinned);
                let title = if entry.pinned { "Épinglés" } else { "Récents" };
                placement.sections.push((title, y));
                y += SECTION;
            }
            let h = row_height(entry);
            placement.rows.push(Placed { index, y, h });
            y += h + ROW_GAP;
        }
        placement.height = y;
        placement
    }
}

/// Texte montré pour un élément, sur une ligne (les retours deviennent des espaces).
pub fn label(entry: &Entry) -> String {
    match &entry.content {
        Content::Image { png, thumbnail } => match png_size(png) {
            Some((w, h)) => format!("Image · {w} × {h}"),
            None => format!("Image · {} × {}", thumbnail.width, thumbnail.height),
        },
        Content::Text(text) => {
            let paths = file_paths(text);
            match paths.as_slice() {
                [] => text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(300)
                    .collect(),
                [one] => one.display().to_string(),
                [first, rest @ ..] => {
                    let name = first.file_name().unwrap_or(first.as_os_str()).to_string_lossy();
                    format!(
                        "{name} et {} autre{}",
                        rest.len(),
                        if rest.len() > 1 { "s" } else { "" }
                    )
                }
            }
        }
    }
}

fn row_height(entry: &Entry) -> f64 {
    if matches!(entry.content, Content::Text(_)) && label(entry).chars().count() > ONE_LINE {
        ROW_TALL
    } else {
        ROW
    }
}

/// Taille d'une image PNG, lue dans son en-tête.
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    let header = png.get(16..24)?;
    let width = u32::from_be_bytes(header[..4].try_into().ok()?);
    let height = u32::from_be_bytes(header[4..].try_into().ok()?);
    Some((width, height))
}

/// Défilement qui garde la ligne `row` entière dans une liste haute de `visible`.
pub fn scroll_to_show(scroll: f64, row: Placed, visible: f64) -> f64 {
    if row.y < scroll {
        row.y
    } else if row.y + row.h > scroll + visible {
        row.y + row.h - visible
    } else {
        scroll
    }
}

/// Liste d'avant une suppression, que « Annuler » remet, et jusqu'à quand le proposer.
#[derive(Debug)]
pub struct Undo {
    entries: Vec<Entry>,
    text: &'static str,
    until: Instant,
}

/// Durée du bandeau « Annuler ».
const UNDO: Duration = Duration::from_millis(4500);

/// Codes evdev des touches envoyées pour coller (`linux/input-event-codes.h`).
const KEY_LEFTCTRL: u32 = 29;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_V: u32 = 47;

/// Branche le canal des contenus lus sur la boucle du compositeur.
pub fn start(handle: &LoopHandle<'static, Ferrite>) -> Result<Copies, String> {
    let (sender, receiver) = channel::channel::<(Content, Option<String>)>();
    handle
        .insert_source(receiver, |event, _, state| {
            if let channel::Event::Msg((content, source)) = event {
                info!(
                    ?content,
                    ?source,
                    "copie ajoutée à l'historique du presse-papiers"
                );
                state.clipboard.add(content, source);
                state.request_redraw();
            }
        })
        .map_err(|err| format!("canal du presse-papiers : {err}"))?;
    Ok(sender)
}

impl Ferrite {
    /// Une application Wayland vient de copier : son contenu rejoint l'historique.
    pub fn capture_wayland_clipboard(&mut self, mime_types: &[String]) {
        info!(?mime_types, "nouvelle copie d'une application Wayland");
        let (Some(wanted), Some(sender)) = (choose(mime_types), self.clipboard_sender.clone()) else {
            return;
        };
        let mime = wanted.mime().to_owned();
        match read_selection(wanted, sender, self.focused_app_name()) {
            Ok(fd) => {
                if let Err(err) = request_data_device_client_selection(&self.seat, mime, fd) {
                    warn!(?err, "presse-papiers : lecture refusée");
                }
            }
            Err(err) => warn!(%err, "presse-papiers : tube impossible"),
        }
    }

    /// Une application X11 vient de copier : son contenu rejoint l'historique.
    pub fn capture_x11_clipboard(&mut self, mime_types: &[String]) {
        let (Some(wanted), Some(sender)) = (choose(mime_types), self.clipboard_sender.clone()) else {
            return;
        };
        let mime = wanted.mime().to_owned();
        let handle = self.loop_handle.clone();
        let source = self.focused_app_name();
        let Some(xwm) = self.xwm.as_mut() else { return };
        match read_selection(wanted, sender, source) {
            Ok(fd) => {
                if let Err(err) = xwm.send_selection(SelectionTarget::Clipboard, mime, fd, handle) {
                    warn!(?err, "presse-papiers X11 : lecture refusée");
                }
            }
            Err(err) => warn!(%err, "presse-papiers : tube impossible"),
        }
    }

    /// Nom de l'application au premier plan, d'où vient une copie ; pour une application
    /// inconnue, la fin de son identifiant (« wl-clipboard »).
    fn focused_app_name(&self) -> Option<String> {
        let surface = self.seat.get_keyboard()?.current_focus()?;
        let window = self.window_for_surface(&surface)?;
        match self.window_app(&window)? {
            crate::desktop::apps::AppKey::Other(id) => id.rsplit('.').next().map(str::to_owned),
            key => Some(key.name(&self.applications)),
        }
    }

    /// Colle l'élément `index` de l'historique dans l'application active : il devient le
    /// contenu du presse-papiers, puis Ctrl+V (Ctrl+Maj+V dans le Terminal) est envoyé.
    pub fn paste_history(&mut self, index: usize) {
        let Some(entry) = self.clipboard.entries().get(index) else {
            return;
        };
        let content = entry.content.clone();
        self.clipboard.promote(index);
        self.popover = None;
        let mime_types = content.mime_types();
        set_data_device_selection(
            &self.display_handle,
            &self.seat,
            mime_types.clone(),
            Owner::History(content),
        );
        if let Some(xwm) = self.xwm.as_mut()
            && let Err(err) = xwm.new_selection(SelectionTarget::Clipboard, Some(mime_types))
        {
            warn!(?err, "presse-papiers X11 non mis à jour");
        }
        self.send_paste_shortcut();
    }

    fn send_paste_shortcut(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let terminal = keyboard
            .current_focus()
            .and_then(|surface| self.window_for_surface(&surface))
            .and_then(|window| crate::render::app_id(&window))
            .is_some_and(|id| id == "ferrite-terminal" || id == "foot");
        let mut keys = vec![KEY_LEFTCTRL];
        if terminal {
            keys.push(KEY_LEFTSHIFT);
        }
        keys.push(KEY_V);
        let time = self.start_time.elapsed().as_millis() as u32;
        let presses = keys.iter().map(|key| (*key, KeyState::Pressed));
        let releases = keys.iter().rev().map(|key| (*key, KeyState::Released));
        for (key, state) in presses.chain(releases).collect::<Vec<_>>() {
            // Codes XKB : evdev + 8.
            keyboard.input::<(), _>(
                self,
                Keycode::from(key + 8),
                state,
                SERIAL_COUNTER.next_serial(),
                time,
                |_, _, _| FilterResult::Forward,
            );
        }
    }

    /// Éléments montrés, dans l'ordre du panneau (recherche et filtre appliqués).
    pub fn clipboard_shown(&self) -> Vec<usize> {
        self.clipboard.shown(&self.clipboard_query, self.clipboard_filter)
    }

    /// Hauteur visible de la liste, sur l'écran principal.
    pub fn clipboard_list_height(&self) -> f64 {
        let screen = self
            .space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))
            .map_or(800.0, |geometry| geometry.size.h as f64);
        crate::desktop::panels::clipboard_list_height(screen)
    }

    /// Élément de la ligne choisie.
    pub fn clipboard_selected_entry(&self) -> Option<usize> {
        self.clipboard_shown().get(self.clipboard_selected).copied()
    }

    /// Ouverture du panneau : tout en haut, sans recherche ni filtre.
    pub fn reset_clipboard_view(&mut self) {
        self.clipboard_selected = 0;
        self.clipboard_scroll = 0.0;
        self.clipboard_query.clear();
        self.clipboard_filter = Filter::All;
        self.clipboard_undo = None;
    }

    /// Ligne choisie au clavier, gardée entière à l'écran.
    pub fn move_clipboard_selection(&mut self, step: isize) {
        let shown = self.clipboard_shown();
        if shown.is_empty() {
            return;
        }
        self.clipboard_selected =
            (self.clipboard_selected as isize + step).clamp(0, shown.len() as isize - 1) as usize;
        let placement = self.clipboard.place(&shown);
        if let Some(row) = placement.rows.get(self.clipboard_selected) {
            self.clipboard_scroll = scroll_to_show(self.clipboard_scroll, *row, self.clipboard_list_height());
        }
    }

    /// Fait défiler la liste de `delta` points.
    pub fn scroll_clipboard(&mut self, delta: f64) {
        let height = self.clipboard.place(&self.clipboard_shown()).height;
        let last = (height - self.clipboard_list_height()).max(0.0);
        self.clipboard_scroll = (self.clipboard_scroll + delta).clamp(0.0, last);
    }

    /// Après un changement de la liste : sélection et défilement restent dedans.
    pub fn clamp_clipboard_view(&mut self) {
        let count = self.clipboard_shown().len();
        self.clipboard_selected = self.clipboard_selected.min(count.saturating_sub(1));
        self.scroll_clipboard(0.0);
    }

    /// Recherche tapée : la liste repart du haut.
    pub fn type_clipboard_query(&mut self, character: Option<char>) {
        match character {
            Some(character) => self.clipboard_query.push(character),
            None => {
                self.clipboard_query.pop();
            }
        }
        self.clipboard_selected = 0;
        self.clipboard_scroll = 0.0;
    }

    pub fn set_clipboard_filter(&mut self, filter: Filter) {
        self.clipboard_filter = filter;
        self.clipboard_selected = 0;
        self.clipboard_scroll = 0.0;
    }

    /// Filtre suivant (Tab).
    pub fn next_clipboard_filter(&mut self) {
        let at = Filter::ALL
            .iter()
            .position(|filter| *filter == self.clipboard_filter)
            .unwrap_or(0);
        self.set_clipboard_filter(Filter::ALL[(at + 1) % Filter::ALL.len()]);
    }

    /// Retire l'élément `index`, en gardant de quoi l'annuler.
    pub fn delete_clipboard_entry(&mut self, index: usize) {
        self.remember_clipboard("Élément retiré");
        self.clipboard.remove(index);
        self.clamp_clipboard_view();
    }

    /// « Tout effacer » : les épinglés restent ; annulable.
    pub fn clear_clipboard(&mut self) {
        if self.clipboard.entries().iter().all(|entry| entry.pinned) {
            return;
        }
        self.remember_clipboard("Historique effacé (épinglés conservés)");
        self.clipboard.clear();
        self.clamp_clipboard_view();
    }

    fn remember_clipboard(&mut self, text: &'static str) {
        self.clipboard_undo = Some(Undo {
            entries: self.clipboard.snapshot(),
            text,
            until: Instant::now() + UNDO,
        });
        // Le bandeau disparaît seul : une image de plus à son échéance.
        let _ = self
            .loop_handle
            .insert_source(Timer::from_duration(UNDO), |_, _, state| {
                state.request_redraw();
                TimeoutAction::Drop
            });
    }

    /// Bandeau à montrer, tant qu'il est temps d'annuler.
    pub fn clipboard_toast(&self) -> Option<&'static str> {
        self.clipboard_undo
            .as_ref()
            .filter(|undo| Instant::now() < undo.until)
            .map(|undo| undo.text)
    }

    pub fn undo_clipboard(&mut self) {
        if let Some(undo) = self.clipboard_undo.take()
            && Instant::now() < undo.until
        {
            self.clipboard.restore(undo.entries);
            self.clamp_clipboard_view();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Content {
        Content::Text(value.into())
    }

    #[test]
    fn historique_limite_doublons_et_epingles() {
        let mut history = History::default();
        history.add(text("un"), None);
        history.add(text("deux"), None);
        history.add(text("un"), None);
        let texts: Vec<String> = history
            .entries()
            .iter()
            .map(|entry| match &entry.content {
                Content::Text(value) => value.to_string(),
                Content::Image { .. } => String::new(),
            })
            .collect();
        assert_eq!(texts, ["un", "deux"], "un doublon remonte en tête");

        history.toggle_pin(1);
        for index in 0..40 {
            history.add(text(&index.to_string()), None);
        }
        assert_eq!(history.entries().len(), MAX_ENTRIES);
        assert!(
            history.entries().iter().any(|entry| entry.pinned),
            "l'épinglé reste"
        );
        history.clear();
        assert_eq!(history.entries().len(), 1);
        assert!(history.entries()[0].pinned);
    }

    #[test]
    fn nature_des_elements() {
        let entry = |value: &str, source: Option<&str>| Entry {
            content: text(value),
            pinned: false,
            source: source.map(str::to_owned),
            copied: SystemTime::now(),
        };
        assert_eq!(entry("#2779E0", None).kind(), Kind::Color);
        assert_eq!(entry("https://smithay.github.io/", None).kind(), Kind::Link);
        assert_eq!(entry("file:///tmp/un%20caf%C3%A9.txt", None).kind(), Kind::Files);
        assert_eq!(
            file_paths("file:///tmp/un%20caf%C3%A9.txt"),
            [PathBuf::from("/tmp/un café.txt")]
        );
        assert_eq!(entry("cargo run --release", None).kind(), Kind::Code);
        assert_eq!(entry("ls", Some("Terminal")).kind(), Kind::Code);
        assert_eq!(
            entry("Le rendu est planifié avant le vblank.", None).kind(),
            Kind::Text
        );
    }

    #[test]
    fn epingles_d_abord_puis_filtre_et_recherche() {
        let mut history = History::default();
        history.add(text("https://exemple.org"), Some("Navigateur".into()));
        history.add(text("bonjour"), Some("Notes".into()));
        history.add(text("#fff"), None);
        history.toggle_pin(2);
        assert_eq!(history.shown("", Filter::All), [2, 0, 1]);
        assert_eq!(history.shown("", Filter::Links), [2]);
        assert_eq!(history.shown("", Filter::Text), [0, 1]);
        assert_eq!(history.shown("BONJ", Filter::All), [1]);
        assert_eq!(history.shown("navig", Filter::All), [2]);
    }

    #[test]
    fn liste_disposee_en_sections() {
        let mut history = History::default();
        history.add(text("court"), None);
        history.add(text(&"long ".repeat(20)), None);
        history.add(text("épinglé"), None);
        history.toggle_pin(0);
        let shown = history.shown("", Filter::All);
        let placement = history.place(&shown);
        assert_eq!(
            placement.sections,
            [("Épinglés", 0.0), ("Récents", SECTION + ROW + ROW_GAP)]
        );
        let tall = placement.rows[1];
        assert_eq!((tall.index, tall.h), (1, ROW_TALL));
        assert_eq!(scroll_to_show(0.0, tall, 100.0), tall.y + tall.h - 100.0);
        assert_eq!(scroll_to_show(500.0, tall, 100.0), tall.y);
    }

    #[test]
    fn age_lisible() {
        let now = SystemTime::now();
        assert_eq!(ago(now, now), "à l’instant");
        assert_eq!(ago(now, now - Duration::from_secs(150)), "il y a 2 min");
        assert_eq!(ago(now, now - Duration::from_secs(7_300)), "il y a 2 h");
        assert_eq!(ago(now, now - Duration::from_secs(90_000)), "hier");
    }

    #[test]
    fn choix_du_type_et_secrets() {
        let types = |list: &[&str]| list.iter().map(|mime| mime.to_string()).collect::<Vec<_>>();
        assert_eq!(
            choose(&types(&["text/html", "UTF8_STRING", "text/plain"])),
            Some(Wanted::Text("UTF8_STRING".into()))
        );
        assert_eq!(
            choose(&types(&["text/html", "image/png"])),
            Some(Wanted::Image("image/png".into()))
        );
        assert_eq!(choose(&types(&["text/plain", SECRET_HINT])), None);
        assert_eq!(choose(&types(&["application/x-inconnu"])), None);
        assert_eq!(decode(&Wanted::Text(String::new()), b"  \n".to_vec()), None);
    }

    #[test]
    fn epingles_relus_tels_quels() {
        let file = "5\nhello\n12\nligne\nsuivie\n";
        assert_eq!(parse_pinned(file), ["hello", "ligne\nsuivie"]);
        assert_eq!(parse_pinned("x\n"), Vec::<String>::new());
    }

    #[test]
    fn vignette_d_une_image_png() {
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 400, 100);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[200u8; 400 * 100 * 4]).unwrap();
        }
        let Some(Content::Image { thumbnail, .. }) = decode(&Wanted::Image("image/png".into()), png_bytes)
        else {
            panic!("image non lue");
        };
        assert_eq!((thumbnail.width, thumbnail.height), (200, 50));
        assert_eq!(thumbnail.pixels.len(), 200 * 50 * 4);
    }
}
