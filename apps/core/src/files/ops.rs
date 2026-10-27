//! Opérations sur les fichiers : copie, déplacement, suppression, création, renommage.
//!
//! Les copies et déplacements tournent dans un fil à part et publient leur progression
//! (`Progress`) ; une demande d'annulation est vérifiée entre deux blocs.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// Avancement d'une opération longue.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub current: String,
    pub finished: bool,
    pub error: Option<String>,
}

/// Opération en cours, partagée entre l'interface et le fil qui travaille.
#[derive(Debug, Clone)]
pub struct Job {
    pub title: String,
    pub progress: Arc<Mutex<Progress>>,
    pub cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn new(title: impl Into<String>) -> Job {
        Job {
            title: title.into(),
            progress: Arc::new(Mutex::new(Progress::default())),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn snapshot(&self) -> Progress {
        self.progress.lock().map(|p| p.clone()).unwrap_or_default()
    }

    fn update(&self, change: impl FnOnce(&mut Progress)) {
        if let Ok(mut progress) = self.progress.lock() {
            change(&mut progress);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn finish(&self, result: Result<(), String>) {
        self.update(|progress| {
            progress.finished = true;
            progress.error = result.err();
        });
    }
}

/// Nom libre dans `dir` : « nom », sinon « nom (copie) », « nom (copie 2) »…
pub fn unique_destination(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() && candidate.symlink_metadata().is_err() {
        return candidate;
    }
    let (stem, extension) = split_name(name);
    for index in 1.. {
        let suffix = if index == 1 {
            " (copie)".to_owned()
        } else {
            format!(" (copie {index})")
        };
        let candidate = dir.join(format!("{stem}{suffix}{extension}"));
        if candidate.symlink_metadata().is_err() {
            return candidate;
        }
    }
    unreachable!("la boucle trouve toujours un nom libre")
}

/// « photo.tar.gz » → (« photo », « .tar.gz ») ; un dossier ou « .bashrc » n'a pas d'extension.
fn split_name(name: &str) -> (&str, &str) {
    let trimmed = name.trim_start_matches('.');
    let offset = name.len() - trimmed.len();
    match trimmed.find('.') {
        Some(index) if index > 0 => (&name[..offset + index], &name[offset + index..]),
        _ => (name, ""),
    }
}

/// Vérifie un nom de fichier saisi par l'utilisateur.
pub fn validate_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Le nom ne peut pas être vide".into());
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err("Le nom ne peut pas contenir « / »".into());
    }
    Ok(name)
}

pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let new_name = validate_name(new_name)?;
    let target = path.with_file_name(new_name);
    if target == path {
        return Ok(target);
    }
    if target.symlink_metadata().is_ok() {
        return Err(format!("« {new_name} » existe déjà"));
    }
    fs::rename(path, &target).map_err(|error| format!("Renommage impossible : {error}"))?;
    Ok(target)
}

pub fn create_folder(dir: &Path) -> Result<PathBuf, String> {
    let target = unique_destination(dir, "Nouveau dossier");
    fs::create_dir(&target).map_err(|error| format!("Création impossible : {error}"))?;
    Ok(target)
}

/// Taille totale et nombre de fichiers (sans suivre les liens symboliques).
pub fn tree_size(path: &Path) -> (u64, u64) {
    let Ok(metadata) = path.symlink_metadata() else {
        return (0, 0);
    };
    if !metadata.is_dir() {
        return (metadata.len(), 1);
    }
    let mut total = (0, 0);
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.filter_map(Result::ok) {
            let (bytes, files) = tree_size(&entry.path());
            total.0 += bytes;
            total.1 += files;
        }
    }
    total
}

/// Copie `sources` dans `dir`. Un élément copié dans son propre dossier devient « (copie) ».
pub fn copy_into(sources: &[PathBuf], dir: &Path, job: &Job) -> Result<(), String> {
    let total = sources.iter().map(|source| tree_size(source).0).sum();
    job.update(|progress| progress.total_bytes = total);
    for source in sources {
        if dir.starts_with(source) {
            return Err("Impossible de copier un dossier dans lui-même".into());
        }
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or("Élément sans nom")?;
        let target = unique_destination(dir, &name);
        copy_tree(source, &target, job)?;
    }
    Ok(())
}

/// Déplace `sources` dans `dir` : renommage direct, ou copie puis suppression entre
/// deux disques.
pub fn move_into(sources: &[PathBuf], dir: &Path, job: &Job) -> Result<(), String> {
    for source in sources {
        if dir.starts_with(source) {
            return Err("Impossible de déplacer un dossier dans lui-même".into());
        }
        if source.parent() == Some(dir) {
            continue;
        }
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or("Élément sans nom")?;
        let target = unique_destination(dir, &name);
        if fs::rename(source, &target).is_ok() {
            continue;
        }
        job.update(|progress| progress.total_bytes += tree_size(source).0);
        copy_tree(source, &target, job)?;
        remove_tree(source)?;
    }
    Ok(())
}

fn copy_tree(source: &Path, target: &Path, job: &Job) -> Result<(), String> {
    if job.cancelled() {
        return Err("Annulé".into());
    }
    let metadata = source
        .symlink_metadata()
        .map_err(|error| format!("{} : {error}", source.display()))?;
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    job.update(|progress| progress.current = name);
    if metadata.file_type().is_symlink() {
        let link = fs::read_link(source).map_err(|error| error.to_string())?;
        std::os::unix::fs::symlink(link, target).map_err(|error| error.to_string())?;
    } else if metadata.is_dir() {
        fs::create_dir_all(target).map_err(|error| format!("{} : {error}", target.display()))?;
        for entry in fs::read_dir(source)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
        {
            copy_tree(&entry.path(), &target.join(entry.file_name()), job)?;
        }
        let _ = fs::set_permissions(target, metadata.permissions());
    } else {
        let mut input = fs::File::open(source).map_err(|error| format!("{} : {error}", source.display()))?;
        let mut output =
            fs::File::create(target).map_err(|error| format!("{} : {error}", target.display()))?;
        let mut buffer = vec![0; 1 << 20];
        loop {
            if job.cancelled() {
                drop(output);
                let _ = fs::remove_file(target);
                return Err("Annulé".into());
            }
            let read = input.read(&mut buffer).map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            output
                .write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
            job.update(|progress| progress.done_bytes += read as u64);
        }
        let _ = fs::set_permissions(target, metadata.permissions());
    }
    Ok(())
}

/// Supprime définitivement (dossiers compris).
pub fn remove_tree(path: &Path) -> Result<(), String> {
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("{} : {error}", path.display()))?;
    let result = if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|error| format!("Suppression impossible ({}) : {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    pub fn scratch(tag: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("ferrite-fichiers-{tag}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn noms_de_copie_et_extensions() {
        let dir = scratch("noms");
        fs::write(dir.join("photo.tar.gz"), "x").unwrap();
        assert_eq!(
            unique_destination(&dir, "photo.tar.gz"),
            dir.join("photo (copie).tar.gz")
        );
        fs::write(dir.join("photo (copie).tar.gz"), "x").unwrap();
        assert_eq!(
            unique_destination(&dir, "photo.tar.gz"),
            dir.join("photo (copie 2).tar.gz")
        );
        assert_eq!(split_name(".bashrc"), (".bashrc", ""));
        assert_eq!(split_name("Notes"), ("Notes", ""));
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("  ").is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn copie_deplacement_renommage_et_suppression() {
        let dir = scratch("ops");
        let source = dir.join("Projet");
        fs::create_dir_all(source.join("src")).unwrap();
        fs::write(source.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(source.join("lisez-moi.txt"), "Bonjour").unwrap();
        let destination = dir.join("Sauvegarde");
        fs::create_dir(&destination).unwrap();

        let job = Job::new("Copie");
        copy_into(std::slice::from_ref(&source), &destination, &job).unwrap();
        assert_eq!(
            fs::read_to_string(destination.join("Projet/src/main.rs")).unwrap(),
            "fn main() {}"
        );
        assert_eq!(job.snapshot().done_bytes, job.snapshot().total_bytes);
        // Copier dans le même dossier donne « (copie) ».
        copy_into(std::slice::from_ref(&source), &dir, &job).unwrap();
        assert!(dir.join("Projet (copie)/lisez-moi.txt").exists());
        // Pas de copie d'un dossier dans lui-même.
        assert!(copy_into(std::slice::from_ref(&source), &source.join("src"), &job).is_err());

        let renamed = rename(&dir.join("Projet (copie)"), "Archive").unwrap();
        assert!(renamed.ends_with("Archive"));
        assert!(rename(&renamed, "Sauvegarde").is_err());

        move_into(std::slice::from_ref(&renamed), &destination, &job).unwrap();
        assert!(destination.join("Archive/src/main.rs").exists());
        assert!(!renamed.exists());

        let folder = create_folder(&dir).unwrap();
        assert!(folder.ends_with("Nouveau dossier"));
        assert_eq!(tree_size(&source), (19, 2));
        remove_tree(&destination).unwrap();
        assert!(!destination.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
