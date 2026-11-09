//! Opérations sur les fichiers : copie, déplacement, suppression, création, renommage.
//!
//! Les copies et déplacements tournent dans un fil à part et publient leur progression
//! (`Progress`) ; une demande d'annulation est vérifiée entre deux blocs.

use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub name: String,
    pub member: String,
    pub directory: bool,
}

/// Liste le contenu immédiat d'un dossier d'archive sans l'extraire.
pub fn list_archive(archive: &Path, directory: &str) -> Result<Vec<ArchiveEntry>, String> {
    let output = Command::new("bsdtar")
        .args(["-tf"])
        .arg(archive)
        .output()
        .map_err(|error| format!("Lecture de l’archive impossible : {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Lecture de l’archive impossible : {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let mut children = std::collections::BTreeMap::<String, ArchiveEntry>::new();
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let is_directory = line.ends_with('/');
        let Some(member) = safe_archive_member(line) else {
            continue;
        };
        let relative = if prefix.is_empty() {
            member.as_str()
        } else if let Some(relative) = member.strip_prefix(&prefix) {
            relative
        } else {
            continue;
        };
        let (name, remainder) = match relative.split_once('/') {
            Some((name, rest)) => (name, Some(rest)),
            None if !relative.is_empty() => (relative, None),
            None => continue,
        };
        if name.is_empty() {
            continue;
        }
        let child_member = if directory.is_empty() {
            name.to_owned()
        } else {
            format!("{directory}/{name}")
        };
        let directory_entry = remainder.is_some() || is_directory;
        children
            .entry(name.to_owned())
            .and_modify(|entry| entry.directory |= directory_entry)
            .or_insert_with(|| ArchiveEntry {
                name: name.to_owned(),
                member: child_member,
                directory: directory_entry,
            });
    }
    Ok(children.into_values().collect())
}

/// Extrait un seul fichier vers un répertoire temporaire privé.
pub fn materialize_archive_member(archive: &Path, member: &str) -> Result<(PathBuf, PathBuf), String> {
    let member = safe_archive_member(member).ok_or("Chemin de fichier invalide dans l’archive")?;
    let name = Path::new(&member)
        .file_name()
        .ok_or("Nom de fichier invalide dans l’archive")?;
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let (directory, file) = loop {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!("ferrite-archive-{}-{id}", std::process::id()));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&directory) {
            Ok(()) => break (directory.clone(), directory.join(name)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Ouverture du fichier de l’archive impossible : {error}")),
        }
    };
    let output = match fs::File::create(&file) {
        Ok(output) => output,
        Err(error) => {
            let _ = fs::remove_dir_all(&directory);
            return Err(format!("Ouverture du fichier de l’archive impossible : {error}"));
        }
    };
    let result = Command::new("bsdtar")
        .args(["-xOf"])
        .arg(archive)
        .arg("--")
        .arg(&member)
        .stdout(Stdio::from(output))
        .output()
        .map_err(|error| format!("Ouverture du fichier de l’archive impossible : {error}"));
    match result {
        Ok(output) if output.status.success() => Ok((directory, file)),
        Ok(output) => {
            let _ = fs::remove_dir_all(&directory);
            Err(format!(
                "Ouverture du fichier de l’archive impossible : {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&directory);
            Err(error)
        }
    }
}

fn safe_archive_member(name: &str) -> Option<String> {
    use std::path::Component;

    let mut parts = Vec::new();
    for component in Path::new(name.trim_end_matches('/')).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    Rar,
}

impl ArchiveFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
            Self::Rar => "rar",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::Tar => "TAR",
            Self::TarGz => "TAR.GZ",
            Self::Rar => "RAR",
        }
    }
}

pub fn is_archive(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    [".zip", ".tar", ".tar.gz", ".tgz", ".rar"]
        .iter()
        .any(|extension| name.ends_with(extension))
}

/// Crée une archive à côté du dossier affiché.
pub fn create_archive(
    sources: &[PathBuf],
    dir: &Path,
    format: ArchiveFormat,
    job: &Job,
) -> Result<(), String> {
    if sources.is_empty() {
        return Err("Aucun élément à compresser".into());
    }
    if sources.iter().any(|source| source.parent() != Some(dir)) {
        return Err("Les éléments doivent être dans le dossier affiché".into());
    }
    let base = if sources.len() == 1 {
        sources[0]
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Archive".into())
    } else {
        "Archive".into()
    };
    let name = format!("{base}.{}", format.extension());
    let target = unique_destination(dir, &name);
    job.update(|progress| progress.current = name.clone());

    let result = match format {
        ArchiveFormat::Rar => {
            let mut command = Command::new("rar");
            command
                .args(["a", "-idq", "-y"])
                .arg(target.file_name().unwrap_or_default());
            command.args(
                sources
                    .iter()
                    .filter_map(|path| path.file_name())
                    .map(|name| PathBuf::from("./").join(name)),
            );
            command.current_dir(dir);
            run_command(command, job, "Création RAR impossible")
        }
        ArchiveFormat::Zip | ArchiveFormat::Tar | ArchiveFormat::TarGz => {
            let mut command = Command::new("bsdtar");
            match format {
                ArchiveFormat::Zip => command.args(["--format", "zip", "-cf"]),
                ArchiveFormat::TarGz => command.args(["-a", "-cf"]),
                ArchiveFormat::Tar => command.args(["-cf"]),
                ArchiveFormat::Rar => unreachable!(),
            }
            .arg(&target)
            .arg("-C")
            .arg(dir)
            .arg("--");
            command.args(sources.iter().filter_map(|path| path.file_name()));
            run_command(command, job, "Création de l’archive impossible")
        }
    };
    if result.is_err() {
        let _ = fs::remove_file(target);
    }
    result
}

/// Extrait une archive dans un nouveau dossier voisin, sans écraser un contenu existant.
pub fn extract_archive(archive: &Path, job: &Job) -> Result<(), String> {
    if !is_archive(archive) {
        return Err("Format d’archive non pris en charge".into());
    }
    let parent = archive.parent().unwrap_or_else(|| Path::new("."));
    let name = archive.file_name().unwrap_or_default().to_string_lossy();
    let lower = name.to_ascii_lowercase();
    let suffix = [".tar.gz", ".tgz", ".tar", ".zip", ".rar"]
        .iter()
        .find(|suffix| lower.ends_with(**suffix))
        .ok_or("Format d’archive non pris en charge")?;
    let folder_name = &name[..name.len() - suffix.len()];
    let folder_name = if folder_name.is_empty() {
        "Archive"
    } else {
        folder_name
    };
    let destination = unique_destination(parent, folder_name);
    fs::create_dir(&destination).map_err(|error| format!("Extraction impossible : {error}"))?;
    job.update(|progress| progress.current = name.into_owned());

    let mut command = Command::new("bsdtar");
    command.args(["-xf"]).arg(archive).arg("-C").arg(&destination);
    let result = run_command(command, job, "Extraction impossible");
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

fn run_command(mut command: Command, job: &Job, failure: &str) -> Result<(), String> {
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| format!("{failure} : {error}"))?;
    let mut stderr = child.stderr.take().ok_or("Sortie d’erreur indisponible")?;
    let stderr_reader = thread::spawn(move || {
        let mut output = Vec::new();
        let _ = stderr.read_to_end(&mut output);
        output
    });
    loop {
        if job.cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Annulé".into());
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{failure} : {error}"));
            }
        }
    }
    let status = child.wait().map_err(|error| format!("{failure} : {error}"))?;
    let detail = String::from_utf8_lossy(&stderr_reader.join().unwrap_or_default())
        .trim()
        .to_owned();
    if status.success() {
        Ok(())
    } else {
        Err(if detail.is_empty() {
            format!("{failure} (code {})", status)
        } else {
            format!("{failure} : {detail}")
        })
    }
}

/// Avancement d'une opération longue.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub current: String,
    pub finished: bool,
    pub error: Option<String>,
    /// Message affiché quand l'opération réussit (« Envoyé à… »).
    pub success: Option<String>,
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

    pub(super) fn update(&self, change: impl FnOnce(&mut Progress)) {
        if let Ok(mut progress) = self.progress.lock() {
            change(&mut progress);
        }
    }

    pub(super) fn cancelled(&self) -> bool {
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
        copy_or_clean(source, &target, job)?;
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
        copy_or_clean(source, &target, job)?;
        remove_tree(source)?;
    }
    Ok(())
}

/// Copie `source` vers `target` (une place neuve) ; en cas d'échec ou d'annulation, ce qui
/// était déjà copié est effacé, pour ne pas laisser de copie incomplète.
fn copy_or_clean(source: &Path, target: &Path, job: &Job) -> Result<(), String> {
    let result = copy_tree(source, target, job);
    if result.is_err() && target.symlink_metadata().is_ok() {
        let _ = remove_tree(target);
    }
    result
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
            let read = input
                .read(&mut buffer)
                .map_err(|error| format!("{} : {error}", source.display()))?;
            if read == 0 {
                break;
            }
            output
                .write_all(&buffer[..read])
                .map_err(|error| format!("{} : {error}", target.display()))?;
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
    fn reconnait_les_archives_prises_en_charge() {
        for name in [
            "fichier.zip",
            "sauvegarde.tar",
            "sauvegarde.tar.gz",
            "sauvegarde.tgz",
            "fichier.rar",
            "FICHIER.ZIP",
        ] {
            assert!(is_archive(Path::new(name)), "{name}");
        }
        for name in ["fichier.gz", "fichier.7z", "document.txt"] {
            assert!(!is_archive(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn navigue_dans_une_archive_sans_l_extraire() {
        let dir = scratch("archive");
        let source = dir.join("contenu");
        fs::create_dir_all(source.join("sous-dossier")).unwrap();
        fs::write(source.join("racine.txt"), "à la racine").unwrap();
        fs::write(source.join("sous-dossier/note.txt"), "texte interne").unwrap();
        let archive = dir.join("test.zip");
        let status = Command::new("bsdtar")
            .args(["--format", "zip", "-cf"])
            .arg(&archive)
            .arg("-C")
            .arg(&source)
            .arg("--")
            .args(["racine.txt", "sous-dossier"])
            .status()
            .unwrap();
        assert!(status.success());

        let root = list_archive(&archive, "").unwrap();
        assert!(
            root.iter()
                .any(|entry| entry.name == "racine.txt" && !entry.directory)
        );
        assert!(
            root.iter()
                .any(|entry| entry.name == "sous-dossier" && entry.directory)
        );
        let nested = list_archive(&archive, "sous-dossier").unwrap();
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].member, "sous-dossier/note.txt");
        let (temporary_dir, file) = materialize_archive_member(&archive, &nested[0].member).unwrap();
        assert_eq!(fs::read_to_string(file).unwrap(), "texte interne");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&temporary_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(!dir.join("sous-dossier").exists());
        fs::remove_dir_all(temporary_dir).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refuse_les_chemins_d_archive_hors_racine() {
        assert_eq!(
            safe_archive_member("./dossier/fichier.txt"),
            Some("dossier/fichier.txt".into())
        );
        assert_eq!(safe_archive_member("../fichier.txt"), None);
        assert_eq!(safe_archive_member("/fichier.txt"), None);
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
