//! Analyse de l'espace : parcourt un dossier en arrière-plan et retient la taille de chaque
//! sous-dossier et les plus gros fichiers, pour retrouver ce qui encombre le disque.
//!
//! Le parcours reste sur le disque de départ (il ne descend ni dans `/proc` ni dans une
//! clé montée dessous) et ne suit pas les liens. Les tailles sont publiées au fil de l'eau :
//! un dossier a la sienne dès qu'il est entièrement compté.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use eframe::egui;

/// Plus gros fichiers retenus.
const LARGEST: usize = 200;
/// Cadence des nouvelles publiées pendant le parcours.
const PUBLISH_EVERY: Duration = Duration::from_millis(400);

#[derive(Debug, Default)]
struct Shared {
    /// Taille de chaque dossier entièrement compté.
    sizes: HashMap<PathBuf, u64>,
    /// Plus gros fichiers, du plus gros au plus petit.
    largest: Vec<(u64, PathBuf)>,
    files: u64,
    bytes: u64,
    /// Dossiers illisibles (droits), ignorés.
    unreadable: u64,
    done: bool,
}

/// Avancement d'une analyse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub files: u64,
    pub bytes: u64,
    pub unreadable: u64,
    pub done: bool,
}

pub struct Scan {
    pub root: PathBuf,
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
}

impl std::fmt::Debug for Scan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scan").field("root", &self.root).finish()
    }
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// État du parcours, propre au fil qui le mène.
struct Walk<'a> {
    device: u64,
    shared: &'a Mutex<Shared>,
    cancel: &'a AtomicBool,
    heap: BinaryHeap<Reverse<(u64, PathBuf)>>,
    files: u64,
    bytes: u64,
    unreadable: u64,
    published: Instant,
    notify: &'a dyn Fn(),
}

impl Walk<'_> {
    /// Taille de `dir` (fichiers de tous ses sous-dossiers) ; publiée une fois comptée.
    fn dir(&mut self, dir: &Path) -> u64 {
        let Ok(entries) = fs::read_dir(dir) else {
            self.unreadable += 1;
            return 0;
        };
        let mut total = 0;
        for entry in entries.filter_map(Result::ok) {
            if self.cancel.load(Ordering::Relaxed) {
                return total;
            }
            let Ok(metadata) = entry.metadata() else { continue };
            let path = entry.path();
            if metadata.is_dir() {
                if metadata.dev() == self.device {
                    total += self.dir(&path);
                }
            } else if metadata.is_file() {
                let size = metadata.len();
                total += size;
                self.files += 1;
                self.bytes += size;
                self.keep(size, path);
            }
        }
        lock(self.shared).sizes.insert(dir.to_path_buf(), total);
        if self.published.elapsed() >= PUBLISH_EVERY {
            self.publish(false);
        }
        total
    }

    fn keep(&mut self, size: u64, path: PathBuf) {
        if self.heap.len() < LARGEST {
            self.heap.push(Reverse((size, path)));
        } else if self
            .heap
            .peek()
            .is_some_and(|Reverse((smallest, _))| size > *smallest)
        {
            self.heap.pop();
            self.heap.push(Reverse((size, path)));
        }
    }

    fn publish(&mut self, done: bool) {
        let mut largest: Vec<(u64, PathBuf)> = self.heap.iter().map(|Reverse(item)| item.clone()).collect();
        largest.sort_by(|a, b| b.cmp(a));
        {
            let mut shared = lock(self.shared);
            shared.largest = largest;
            shared.files = self.files;
            shared.bytes = self.bytes;
            shared.unreadable = self.unreadable;
            shared.done = done;
        }
        self.published = Instant::now();
        (self.notify)();
    }
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Parcourt `root` jusqu'au bout (ou jusqu'à `cancel`), en publiant dans `shared`.
fn run(root: &Path, shared: &Mutex<Shared>, cancel: &AtomicBool, notify: &dyn Fn()) {
    let device = fs::metadata(root).map(|metadata| metadata.dev()).unwrap_or(0);
    let mut walk = Walk {
        device,
        shared,
        cancel,
        heap: BinaryHeap::new(),
        files: 0,
        bytes: 0,
        unreadable: 0,
        published: Instant::now(),
        notify,
    };
    walk.dir(root);
    walk.publish(!cancel.load(Ordering::Relaxed));
}

impl Scan {
    /// Lance l'analyse de `root` dans un fil à part ; l'interface est redessinée à chaque
    /// nouvelle.
    pub fn start(root: PathBuf, ctx: &egui::Context) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (thread_shared, thread_cancel, ctx) = (Arc::clone(&shared), Arc::clone(&cancel), ctx.clone());
        let thread_root = root.clone();
        thread::spawn(move || {
            run(&thread_root, &thread_shared, &thread_cancel, &|| {
                ctx.request_repaint()
            });
        });
        Self { root, shared, cancel }
    }

    pub fn progress(&self) -> Progress {
        let shared = lock(&self.shared);
        Progress {
            files: shared.files,
            bytes: shared.bytes,
            unreadable: shared.unreadable,
            done: shared.done,
        }
    }

    /// Taille d'un dossier, une fois entièrement compté.
    pub fn size_of(&self, dir: &Path) -> Option<u64> {
        lock(&self.shared).sizes.get(dir).copied()
    }

    /// Plus gros fichiers sous `dir`, du plus gros au plus petit.
    pub fn largest_under(&self, dir: &Path, limit: usize) -> Vec<(u64, PathBuf)> {
        lock(&self.shared)
            .largest
            .iter()
            .filter(|(_, path)| path.starts_with(dir))
            .take(limit)
            .cloned()
            .collect()
    }

    /// `path` (de taille `size`) vient d'être supprimé : les dossiers qui le contenaient
    /// maigrissent d'autant, sans tout recompter.
    pub fn forget(&self, path: &Path, size: u64) {
        let mut shared = lock(&self.shared);
        shared.sizes.retain(|known, _| !known.starts_with(path));
        for ancestor in path.ancestors().skip(1) {
            if let Some(known) = shared.sizes.get_mut(ancestor) {
                *known = known.saturating_sub(size);
            }
            if ancestor == self.root {
                break;
            }
        }
        shared.largest.retain(|(_, known)| !known.starts_with(path));
        shared.bytes = shared.bytes.saturating_sub(size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailles_des_dossiers_et_plus_gros_fichiers() {
        let root = std::env::temp_dir().join(format!("ferrite-analyse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(root.join("vide")).unwrap();
        fs::write(root.join("a/b/gros.bin"), vec![0u8; 5000]).unwrap();
        fs::write(root.join("a/moyen.bin"), vec![0u8; 300]).unwrap();
        fs::write(root.join("petit.txt"), b"abc").unwrap();

        let shared = Mutex::new(Shared::default());
        run(&root, &shared, &AtomicBool::new(false), &|| {});
        let scan = Scan {
            root: root.clone(),
            shared: Arc::new(shared),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let progress = scan.progress();
        assert!(progress.done);
        assert_eq!((progress.files, progress.bytes), (3, 5303));
        assert_eq!(scan.size_of(&root.join("a")), Some(5300));
        assert_eq!(scan.size_of(&root.join("vide")), Some(0));
        assert_eq!(scan.size_of(&root), Some(5303));
        let largest = scan.largest_under(&root, 2);
        assert_eq!(largest[0], (5000, root.join("a/b/gros.bin")));
        assert_eq!(largest[1], (300, root.join("a/moyen.bin")));
        assert_eq!(scan.largest_under(&root.join("vide"), 10), []);

        // Le gros fichier supprimé : ses dossiers maigrissent, il quitte la liste.
        scan.forget(&root.join("a/b/gros.bin"), 5000);
        assert_eq!(scan.size_of(&root.join("a")), Some(300));
        assert_eq!(scan.size_of(&root), Some(303));
        assert_eq!(scan.largest_under(&root, 1)[0].1, root.join("a/moyen.bin"));
        let _ = fs::remove_dir_all(&root);
    }
}
