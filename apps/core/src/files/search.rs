//! Recherche dans les sous-dossiers.
//!
//! Le champ « Rechercher » filtre d'abord le dossier affiché ; en même temps, un fil à part
//! parcourt ses sous-dossiers et renvoie, par paquets, les éléments dont le nom contient le
//! texte cherché. Le parcours s'arrête dès que la recherche change ou que l'on quitte le
//! dossier, et ne suit pas les liens (pas de boucle possible).

use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

use eframe::egui;

use super::FileEntry;

/// Au-delà, la recherche s'arrête : mieux vaut préciser le texte cherché.
const MAX_RESULTS: usize = 2_000;

enum Found {
    Entries(Vec<FileEntry>),
    Done { truncated: bool },
}

pub(super) struct DeepSearch {
    pub root: PathBuf,
    pub query: String,
    pub hidden: bool,
    /// Éléments trouvés dans les sous-dossiers (jamais ceux du dossier lui-même).
    pub found: Vec<FileEntry>,
    pub done: bool,
    /// Vrai si la recherche s'est arrêtée à `MAX_RESULTS`.
    pub truncated: bool,
    receiver: Receiver<Found>,
    cancel: Arc<AtomicBool>,
}

impl Drop for DeepSearch {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl DeepSearch {
    /// Lance le parcours de `root` ; `query` est déjà en minuscules.
    pub fn start(root: PathBuf, query: String, hidden: bool, ctx: &egui::Context) -> Self {
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        let (walk_root, walk_query, ctx) = (root.clone(), query.clone(), ctx.clone());
        thread::spawn(move || {
            let truncated = walk(&walk_root, &walk_query, hidden, &stop, |batch| {
                let _ = sender.send(Found::Entries(batch));
                ctx.request_repaint();
            });
            let _ = sender.send(Found::Done { truncated });
            ctx.request_repaint();
        });
        Self {
            root,
            query,
            hidden,
            found: Vec::new(),
            done: false,
            truncated: false,
            receiver,
            cancel,
        }
    }

    /// Nouveaux éléments arrivés depuis le dernier appel (déjà ajoutés à `found`).
    pub fn take_new(&mut self) -> Vec<FileEntry> {
        let mut new = Vec::new();
        while let Ok(found) = self.receiver.try_recv() {
            match found {
                Found::Entries(batch) => new.extend(batch),
                Found::Done { truncated } => {
                    self.done = true;
                    self.truncated = truncated;
                }
            }
        }
        self.found.extend(new.iter().cloned());
        new
    }

    /// Dossier où se trouve un élément trouvé, relatif au dossier de départ
    /// (`None` pour un élément du dossier lui-même).
    pub fn place_of(&self, path: &Path) -> Option<String> {
        let parent = path.parent()?;
        let relative = parent.strip_prefix(&self.root).ok()?;
        (!relative.as_os_str().is_empty()).then(|| relative.display().to_string())
    }
}

/// Parcours en largeur : les résultats proches arrivent les premiers. Rend vrai si la limite
/// de résultats a été atteinte.
fn walk(
    root: &Path,
    query: &str,
    hidden: bool,
    cancel: &AtomicBool,
    mut send: impl FnMut(Vec<FileEntry>),
) -> bool {
    let mut queue = VecDeque::from([root.to_path_buf()]);
    let mut batch = Vec::new();
    let mut sent_at = Instant::now();
    let mut count = 0;
    while let Some(dir) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let Ok(listing) = fs::read_dir(&dir) else {
            continue;
        };
        let deeper = dir != root;
        for entry in listing.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !hidden && name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                queue.push_back(path.clone());
            }
            if !deeper || !name.to_lowercase().contains(query) {
                continue;
            }
            let Ok(link) = entry.metadata() else {
                continue;
            };
            let metadata = fs::metadata(&path).unwrap_or(link);
            batch.push(FileEntry {
                name,
                directory: metadata.is_dir(),
                symlink: kind.is_symlink(),
                modified: metadata.modified().ok(),
                size: (!metadata.is_dir()).then_some(metadata.len()),
                archive_member: None,
                path,
                trash: None,
            });
            count += 1;
            if count >= MAX_RESULTS {
                send(std::mem::take(&mut batch));
                return true;
            }
        }
        if !batch.is_empty() && (batch.len() >= 200 || sent_at.elapsed() > Duration::from_millis(150)) {
            send(std::mem::take(&mut batch));
            sent_at = Instant::now();
        }
    }
    if !batch.is_empty() {
        send(batch);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trouve_dans_les_sous_dossiers_sans_le_dossier_lui_meme() {
        let root = env_temp("recherche");
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(root.join(".cache")).unwrap();
        fs::write(root.join("Rapport.txt"), "").unwrap();
        fs::write(root.join("a/rapport-2.txt"), "").unwrap();
        fs::write(root.join("a/b/vieux RAPPORT.pdf"), "").unwrap();
        fs::write(root.join(".cache/rapport.tmp"), "").unwrap();
        fs::write(root.join("a/autre.txt"), "").unwrap();

        let mut found = Vec::new();
        let cancel = AtomicBool::new(false);
        let truncated = walk(&root, "rapport", false, &cancel, |batch| found.extend(batch));
        let mut names: Vec<String> = found.into_iter().map(|entry| entry.name).collect();
        names.sort();
        assert!(!truncated);
        assert_eq!(names, ["rapport-2.txt", "vieux RAPPORT.pdf"]);

        let mut found = Vec::new();
        walk(&root, "rapport", true, &cancel, |batch| found.extend(batch));
        assert_eq!(found.len(), 3);
        fs::remove_dir_all(&root).unwrap();
    }

    fn env_temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrite-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }
}
