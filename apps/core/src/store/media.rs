//! Images de la Logithèque : icônes des catalogues (fichiers locaux) et captures d'écran
//! (téléchargées une fois par `curl`, gardées dans `~/.cache/ferrite/logitheque`).
//!
//! Le décodage et le téléchargement se font dans deux fils à part ; l'interface demande une
//! image, l'affiche quand elle arrive, et ne bloque jamais.

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use ferrite_ui::wallpaper;

/// Côté maximal d'une icône décodée, et largeur maximale d'une capture.
const ICON_SIDE: u32 = 128;
const SHOT_SIDE: u32 = 1100;

/// État d'une capture d'écran.
pub enum Shot {
    Loading,
    Ready(TextureHandle),
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Icon(PathBuf),
    Shot(String),
}

enum Job {
    Icon(PathBuf),
    Shot(String),
}

/// Images chargées ou en cours.
pub struct Media {
    textures: HashMap<Key, Option<TextureHandle>>,
    pending: HashSet<Key>,
    icons: Sender<Job>,
    shots: Sender<Job>,
    results: Receiver<(Key, Option<ColorImage>)>,
}

impl std::fmt::Debug for Media {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Media")
            .field("textures", &self.textures.len())
            .field("pending", &self.pending.len())
            .finish()
    }
}

impl Media {
    pub fn new(ctx: &egui::Context) -> Self {
        let (results_tx, results) = mpsc::channel();
        let icons = worker(ctx.clone(), results_tx.clone());
        let shots = worker(ctx.clone(), results_tx);
        Self {
            textures: HashMap::new(),
            pending: HashSet::new(),
            icons,
            shots,
            results,
        }
    }

    /// Range les images arrivées. À appeler une fois par image affichée.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, image)) = self.results.try_recv() {
            self.pending.remove(&key);
            let name = match &key {
                Key::Icon(path) => path.display().to_string(),
                Key::Shot(url) => url.clone(),
            };
            let texture = image.map(|image| ctx.load_texture(name, image, TextureOptions::LINEAR));
            self.textures.insert(key, texture);
        }
    }

    /// Icône d'une application ; `None` tant qu'elle n'est pas prête (ou introuvable).
    pub fn icon(&mut self, path: &Path) -> Option<TextureHandle> {
        let key = Key::Icon(path.to_path_buf());
        if let Some(texture) = self.textures.get(&key) {
            return texture.clone();
        }
        if self.pending.insert(key) {
            let _ = self.icons.send(Job::Icon(path.to_path_buf()));
        }
        None
    }

    /// Capture d'écran, téléchargée au premier appel.
    pub fn shot(&mut self, url: &str) -> Shot {
        let key = Key::Shot(url.to_owned());
        if let Some(texture) = self.textures.get(&key) {
            return texture.clone().map_or(Shot::Failed, Shot::Ready);
        }
        if self.pending.insert(key) {
            let _ = self.shots.send(Job::Shot(url.to_owned()));
        }
        Shot::Loading
    }
}

fn worker(ctx: egui::Context, results: Sender<(Key, Option<ColorImage>)>) -> Sender<Job> {
    let (sender, jobs) = mpsc::channel::<Job>();
    thread::spawn(move || {
        for job in jobs {
            let (key, image) = match job {
                Job::Icon(path) => {
                    let image = decode_file(&path, ICON_SIDE);
                    (Key::Icon(path), image)
                }
                Job::Shot(url) => {
                    let image = download(&url).and_then(|path| decode_file(&path, SHOT_SIDE));
                    (Key::Shot(url), image)
                }
            };
            if results.send((key, image)).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    });
    sender
}

fn decode_file(path: &Path, side: u32) -> Option<ColorImage> {
    let image = wallpaper::decode(path).ok()?;
    let image = crate::viewer::shrink(&image, side);
    Some(ColorImage::from_rgba_unmultiplied(
        [image.width as usize, image.height as usize],
        &image.rgba,
    ))
}

/// Dossier des captures téléchargées.
fn cache_dir() -> Option<PathBuf> {
    let base = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("ferrite/logitheque"))
}

/// Nom de fichier stable pour une adresse.
fn cache_name(url: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    let extension = url
        .rsplit('.')
        .next()
        .filter(|extension| matches!(*extension, "png" | "jpg" | "jpeg"))
        .unwrap_or("img");
    format!("{:016x}.{extension}", hasher.finish())
}

/// Télécharge `url` dans le cache (une seule fois) et renvoie le fichier.
fn download(url: &str) -> Option<PathBuf> {
    if !url.starts_with("https://") {
        return None;
    }
    let dir = cache_dir()?;
    let path = dir.join(cache_name(url));
    if path.is_file() {
        return Some(path);
    }
    fs::create_dir_all(&dir).ok()?;
    let partial = path.with_extension("part");
    let ok = Command::new("curl")
        .args(["-fsSL", "--max-time", "30", "--max-filesize", "20000000", "-o"])
        .arg(&partial)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if ok && fs::rename(&partial, &path).is_ok() {
        Some(path)
    } else {
        let _ = fs::remove_file(&partial);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nom_de_cache_stable() {
        let a = cache_name("https://x/a_752.png");
        assert_eq!(a, cache_name("https://x/a_752.png"));
        assert!(a.ends_with(".png"));
        assert_ne!(a, cache_name("https://x/b_752.png"));
        assert!(cache_name("https://x/a?b").ends_with(".img"));
    }

    #[test]
    fn seulement_https() {
        assert!(download("http://exemple/a.png").is_none());
        assert!(download("file:///etc/passwd").is_none());
    }
}
