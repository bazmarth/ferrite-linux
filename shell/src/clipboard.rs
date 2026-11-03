//! Historique du presse-papiers, comme Windows + V : chaque copie (texte ou image PNG)
//! est gardée, jusqu'à 25 éléments ; Super+V affiche la liste et colle l'élément choisi
//! dans l'application active. Les éléments épinglés restent après un redémarrage
//! (`~/.local/share/ferrite/presse-papiers`), les autres s'effacent avec la session.
//!
//! Le compositeur lit le contenu copié par un tube (`wl_data_source` ou propriétaire X11)
//! dans un fil à part : un client lent ne fige jamais le bureau.

use std::{
    env, fs,
    io::{self, Read, Write},
    os::fd::OwnedFd,
    path::PathBuf,
    sync::Arc,
    thread,
};

use smithay::{
    backend::input::KeyState,
    input::keyboard::{FilterResult, Keycode},
    reexports::calloop::{LoopHandle, channel},
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

    /// Nouvelle copie : en tête de liste. Un contenu déjà présent remonte en tête au lieu
    /// d'apparaître deux fois.
    pub fn add(&mut self, content: Content) {
        let pinned = match self.entries.iter().position(|entry| entry.content == content) {
            Some(index) => self.entries.remove(index).pinned,
            None => false,
        };
        self.entries.insert(0, Entry { content, pinned });
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
pub fn read_selection(wanted: Wanted, sender: channel::Sender<Content>) -> io::Result<OwnedFd> {
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
                let _ = sender.send(content);
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

/// Lignes visibles du panneau Super+V.
pub const VISIBLE: usize = 5;

/// Codes evdev des touches envoyées pour coller (`linux/input-event-codes.h`).
const KEY_LEFTCTRL: u32 = 29;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_V: u32 = 47;

/// Branche le canal des contenus lus sur la boucle du compositeur.
pub fn start(handle: &LoopHandle<'static, Ferrite>) -> Result<channel::Sender<Content>, String> {
    let (sender, receiver) = channel::channel::<Content>();
    handle
        .insert_source(receiver, |event, _, state| {
            if let channel::Event::Msg(content) = event {
                info!(?content, "copie ajoutée à l'historique du presse-papiers");
                state.clipboard.add(content);
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
        match read_selection(wanted, sender) {
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
        let Some(xwm) = self.xwm.as_mut() else { return };
        match read_selection(wanted, sender) {
            Ok(fd) => {
                if let Err(err) = xwm.send_selection(SelectionTarget::Clipboard, mime, fd, handle) {
                    warn!(?err, "presse-papiers X11 : lecture refusée");
                }
            }
            Err(err) => warn!(%err, "presse-papiers : tube impossible"),
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

    /// Ligne choisie au clavier, gardée visible.
    pub fn move_clipboard_selection(&mut self, step: isize) {
        let count = self.clipboard.entries().len();
        if count == 0 {
            return;
        }
        let selected = (self.clipboard_selected as isize + step).clamp(0, count as isize - 1) as usize;
        self.clipboard_selected = selected;
        if selected < self.clipboard_first {
            self.clipboard_first = selected;
        } else if selected >= self.clipboard_first + VISIBLE {
            self.clipboard_first = selected + 1 - VISIBLE;
        }
    }

    /// Fait défiler la liste d'une ligne.
    pub fn scroll_clipboard(&mut self, step: isize) {
        let last = self.clipboard.entries().len().saturating_sub(VISIBLE);
        self.clipboard_first = (self.clipboard_first as isize + step).clamp(0, last as isize) as usize;
    }

    /// Après une suppression : la sélection et le défilement restent dans la liste.
    pub fn clamp_clipboard_view(&mut self) {
        let count = self.clipboard.entries().len();
        self.clipboard_selected = self.clipboard_selected.min(count.saturating_sub(1));
        self.clipboard_first = self.clipboard_first.min(count.saturating_sub(VISIBLE));
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
        history.add(text("un"));
        history.add(text("deux"));
        history.add(text("un"));
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
            history.add(text(&index.to_string()));
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
