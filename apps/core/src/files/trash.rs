//! Corbeille, au format freedesktop (« Trash specification ») : chaque élément va dans un
//! dossier `files`, avec une fiche `info/NOM.trashinfo` (chemin d'origine et date). Les
//! autres gestionnaires de fichiers Linux voient donc la même Corbeille.
//!
//! Un élément du disque personnel va dans `~/.local/share/Trash` ; un élément d'un autre
//! disque (partition de données, clé USB) va dans la Corbeille de ce disque,
//! `.Trash-UID` à sa racine. Rien n'est donc jamais copié : la mise à la Corbeille est un
//! simple renommage, instantané même pour un dossier de plusieurs gigaoctets. (Avant,
//! un dossier d'un autre disque était copié vers le disque système, ce qui pouvait le
//! remplir et geler le PC.)

use std::{
    env, fs,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::ops::{remove_tree, unique_destination};

/// Élément de la Corbeille.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItem {
    /// Emplacement dans `Trash/files`.
    pub path: PathBuf,
    pub original: PathBuf,
    /// « 2026-10-26T18:05:00 », tel qu'écrit dans la fiche.
    pub deleted: String,
}

/// Corbeille du dossier personnel.
pub fn root() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Trash")
}

/// Toutes les Corbeilles : la personnelle, puis celle de chaque disque monté qui en a une.
/// Relue au plus toutes les deux secondes : l'icône de la Corbeille la consulte à chaque
/// image.
pub fn roots() -> Vec<PathBuf> {
    use std::{
        sync::Mutex,
        time::{Duration, Instant},
    };
    // Les tests ne doivent jamais toucher aux Corbeilles des vrais disques.
    if cfg!(test) {
        return vec![root()];
    }
    static CACHE: Mutex<Option<(Instant, Vec<PathBuf>)>> = Mutex::new(None);
    let Ok(mut cache) = CACHE.lock() else {
        return scan_roots();
    };
    if let Some((when, roots)) = cache.as_ref()
        && when.elapsed() < Duration::from_secs(2)
    {
        return roots.clone();
    }
    let roots = scan_roots();
    *cache = Some((Instant::now(), roots.clone()));
    roots
}

fn scan_roots() -> Vec<PathBuf> {
    let mut roots = vec![root()];
    let name = volume_trash_name();
    let mounts = fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    for point in disk_mount_points(&mounts) {
        let trash = point.join(&name);
        if is_real_dir(&trash) && !roots.contains(&trash) {
            roots.push(trash);
        }
    }
    roots
}

/// Dossiers `files` de toutes les Corbeilles (pour suivre leurs changements).
pub fn files_dirs() -> Vec<PathBuf> {
    roots().into_iter().map(|root| root.join("files")).collect()
}

/// Points de montage des vrais disques dans `/proc/self/mounts` (périphérique `/dev/…`) :
/// les systèmes virtuels et les partages réseau, parfois lents à répondre, sont écartés.
fn disk_mount_points(mounts: &str) -> Vec<PathBuf> {
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let device = fields.next()?;
            let point = fields.next()?;
            device
                .starts_with("/dev/")
                .then(|| PathBuf::from(unescape_mount(point)))
        })
        .collect()
}

/// `/proc/self/mounts` écrit les espaces et quelques autres caractères en octal (`\040`).
fn unescape_mount(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && let Ok(value) = u8::from_str_radix(&text[index + 1..index + 4], 8)
        {
            out.push(value);
            index += 4;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn volume_trash_name() -> String {
    // Numéro de l'utilisateur, lu sans dépendance : propriétaire de `/proc/self`.
    let uid = fs::metadata("/proc/self").map_or(1000, |metadata| metadata.uid());
    format!(".Trash-{uid}")
}

/// Vrai dossier (pas un lien symbolique, que la spécification interdit pour la Corbeille).
fn is_real_dir(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|metadata| metadata.is_dir())
}

/// Racine du disque qui contient `path` : le plus haut dossier parent sur le même disque.
fn top_dir(path: &Path) -> Option<PathBuf> {
    let device = path.symlink_metadata().ok()?.dev();
    let mut top = path;
    while let Some(parent) = top.parent() {
        if fs::metadata(parent).ok()?.dev() != device {
            break;
        }
        top = parent;
    }
    Some(top.to_path_buf())
}

/// Corbeille du disque de `path` (créée au besoin, réservée à l'utilisateur).
fn volume_root(path: &Path) -> Result<PathBuf, String> {
    const NO_TRASH: &str =
        "Ce disque n'a pas de Corbeille utilisable : choisissez « Supprimer définitivement… »";
    let top = top_dir(path).ok_or(NO_TRASH)?;
    let trash = top.join(volume_trash_name());
    if !is_real_dir(&trash) {
        if trash.symlink_metadata().is_ok() {
            return Err(NO_TRASH.into());
        }
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&trash)
            .map_err(|_| NO_TRASH.to_owned())?;
    }
    Ok(trash)
}

/// Fiche d'un élément : `info/NOM.trashinfo`, à côté du dossier `files` qui le contient.
fn info_path(item: &Path) -> PathBuf {
    let name = item
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let trash = item
        .parent()
        .and_then(Path::parent)
        .map_or_else(root, Path::to_path_buf);
    trash.join("info").join(format!("{name}.trashinfo"))
}

/// Chemin d'origine et date d'un élément. Dans la Corbeille d'un disque, le chemin peut
/// être écrit à partir de la racine du disque (comme le font d'autres gestionnaires).
fn read_info(item: &Path) -> (Option<PathBuf>, Option<String>) {
    let (original, date) = fs::read_to_string(info_path(item))
        .ok()
        .map(|text| parse_info(&text))
        .unwrap_or_default();
    let original = original.map(|original| {
        if original.is_absolute() {
            return original;
        }
        // Élément dans `RACINE/.Trash-UID/files/` : le chemin part de `RACINE`.
        match item.ancestors().nth(3) {
            Some(top) => top.join(original),
            None => original,
        }
    });
    (original, date)
}

/// Met `path` à la Corbeille ; renvoie sa place dans la Corbeille (pour [`restore`]).
pub fn trash(path: &Path) -> Result<PathBuf, String> {
    // Le dossier parent est résolu, pas l'élément : un lien symbolique part lui-même à la
    // Corbeille, pas ce qu'il désigne.
    let name = path.file_name().ok_or("Élément sans nom")?.to_owned();
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let absolute = fs::canonicalize(parent)
        .map_err(|error| format!("{} : {error}", path.display()))?
        .join(&name);
    let device = |path: &Path| path.symlink_metadata().map(|metadata| metadata.dev()).ok();
    let home = root();
    fs::create_dir_all(&home).map_err(|error| error.to_string())?;
    let trash_root = match device(&absolute) {
        None => return Err(format!("{} : introuvable", absolute.display())),
        Some(dev) if Some(dev) == device(&home) => home,
        Some(_) => volume_root(&absolute)?,
    };
    let files = trash_root.join("files");
    let info = trash_root.join("info");
    fs::create_dir_all(&files).map_err(|error| error.to_string())?;
    fs::create_dir_all(&info).map_err(|error| error.to_string())?;
    let target = unique_destination(&files, &name.to_string_lossy());
    let fiche = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        encode_path(&absolute),
        timestamp(SystemTime::now())
    );
    fs::write(info_path(&target), fiche).map_err(|error| error.to_string())?;
    if let Err(error) = fs::rename(&absolute, &target) {
        let _ = fs::remove_file(info_path(&target));
        return Err(format!(
            "Mise à la Corbeille impossible ({}) : {error}",
            absolute.display()
        ));
    }
    trash_sound();
    Ok(target)
}

/// Son de la Corbeille : un seul pour plusieurs éléments jetés ensemble.
fn trash_sound() {
    use std::sync::Mutex;
    // Les tests de la Corbeille ne doivent pas faire de bruit.
    if cfg!(test) {
        return;
    }
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    let now = std::time::Instant::now();
    if last.is_none_or(|last| now.duration_since(last) > std::time::Duration::from_millis(500)) {
        ferrite_system::sounds::play(ferrite_system::sounds::Sound::Trash);
    }
    *last = Some(now);
}

/// Éléments de toutes les Corbeilles, les plus récents d'abord.
pub fn list() -> Vec<TrashItem> {
    let mut items: Vec<TrashItem> = files_dirs()
        .iter()
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            let (original, deleted) = read_info(&path);
            TrashItem {
                original: original.unwrap_or_else(|| PathBuf::from(entry.file_name())),
                deleted: deleted.unwrap_or_default(),
                path,
            }
        })
        .collect();
    items.sort_by(|a, b| b.deleted.cmp(&a.deleted));
    items
}

/// Remet un élément à son emplacement d'origine (sous un autre nom si la place est prise).
pub fn restore(item: &Path) -> Result<PathBuf, String> {
    let (original, _) = read_info(item);
    let original = original.ok_or("Emplacement d'origine inconnu")?;
    let parent = original.parent().ok_or("Emplacement d'origine invalide")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let name = original
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target = unique_destination(parent, &name);
    fs::rename(item, &target).map_err(|error| format!("Restauration impossible : {error}"))?;
    let _ = fs::remove_file(info_path(item));
    Ok(target)
}

/// Supprime définitivement un élément de la Corbeille.
pub fn delete(item: &Path) -> Result<(), String> {
    remove_tree(item)?;
    let _ = fs::remove_file(info_path(item));
    Ok(())
}

pub fn empty() -> Result<(), String> {
    for item in list() {
        delete(&item.path)?;
    }
    Ok(())
}

pub fn is_empty() -> bool {
    files_dirs()
        .iter()
        .all(|dir| fs::read_dir(dir).map_or(true, |mut entries| entries.next().is_none()))
}

fn parse_info(text: &str) -> (Option<PathBuf>, Option<String>) {
    let mut path = None;
    let mut date = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("Path=") {
            path = Some(PathBuf::from(decode_path(value)));
        } else if let Some(value) = line.strip_prefix("DeletionDate=") {
            date = Some(value.trim().to_owned());
        }
    }
    (path, date)
}

/// Encodage « pourcent » des octets hors caractères sûrs, comme l'exige la spécification.
fn encode_path(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decode_path(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(value) = u8::from_str_radix(&text[index + 1..index + 3], 16)
        {
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Date au format de la spécification (heure UTC, faute de fuseau sans dépendance).
fn timestamp(time: SystemTime) -> String {
    let seconds = time.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        (rest / 60) % 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn encodage_et_date() {
        let path = Path::new("/home/ferrite/Mes été/a b.txt");
        let encoded = encode_path(path);
        assert_eq!(encoded, "/home/ferrite/Mes%20%C3%A9t%C3%A9/a%20b.txt");
        assert_eq!(decode_path(&encoded), path.to_string_lossy());
        let time = UNIX_EPOCH + Duration::from_secs(20_752 * 86_400 + 17 * 3600 + 57 * 60 + 3);
        assert_eq!(timestamp(time), "2026-10-26T17:57:03");
    }

    #[test]
    fn mettre_a_la_corbeille_puis_restaurer() {
        // Corbeille isolée dans un dossier temporaire.
        let dir = std::env::temp_dir().join(format!("ferrite-corbeille-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("donnees")).unwrap();
        // SAFETY : test isolé ; la variable n'est lue que par ce module.
        #[allow(unsafe_code)]
        unsafe {
            env::set_var("XDG_DATA_HOME", dir.join("data"));
        }
        let file = dir.join("donnees/note.txt");
        fs::write(&file, "à garder").unwrap();
        trash(&file).unwrap();
        assert!(!file.exists());
        let items = list();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].original,
            fs::canonicalize(dir.join("donnees")).unwrap().join("note.txt")
        );
        let restored = restore(&items[0].path).unwrap();
        assert_eq!(fs::read_to_string(restored).unwrap(), "à garder");
        assert!(is_empty());
        fs::write(&file, "x").unwrap();
        trash(&file).unwrap();
        empty().unwrap();
        assert!(is_empty());

        // Un lien symbolique part lui-même à la Corbeille, pas ce qu'il désigne.
        fs::write(&file, "cible").unwrap();
        let link = dir.join("donnees/lien");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let placed = trash(&link).unwrap();
        assert!(placed.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&file).unwrap(), "cible");
        empty().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn points_de_montage_des_vrais_disques() {
        let mounts = "proc /proc proc rw 0 0\n\
                      /dev/sda5 / ext4 rw 0 0\n\
                      tmpfs /tmp tmpfs rw 0 0\n\
                      //serveur/partage /mnt/reseau cifs rw 0 0\n\
                      /dev/sdb1 /media/Ma\\040cle vfat rw 0 0\n";
        assert_eq!(
            disk_mount_points(mounts),
            [PathBuf::from("/"), PathBuf::from("/media/Ma cle")]
        );
    }

    #[test]
    fn fiche_d_une_corbeille_de_disque() {
        // Corbeille d'un disque : fiche dans `RACINE/.Trash-UID/info`, chemin d'origine
        // écrit depuis `RACINE` (comme le font d'autres gestionnaires) ou en entier.
        let top = std::env::temp_dir().join(format!("ferrite-disque-{}", std::process::id()));
        let _ = fs::remove_dir_all(&top);
        let trash = top.join(".Trash-1000");
        fs::create_dir_all(trash.join("files/Projet")).unwrap();
        fs::create_dir_all(trash.join("info")).unwrap();
        let item = trash.join("files/Projet");
        assert_eq!(info_path(&item), trash.join("info/Projet.trashinfo"));
        fs::write(
            info_path(&item),
            "[Trash Info]\nPath=Travail/Projet\nDeletionDate=2026-11-09T13:24:33\n",
        )
        .unwrap();
        let (original, date) = read_info(&item);
        assert_eq!(original, Some(top.join("Travail/Projet")));
        assert_eq!(date.as_deref(), Some("2026-11-09T13:24:33"));
        fs::write(info_path(&item), "[Trash Info]\nPath=/media/X/Projet\n").unwrap();
        assert_eq!(read_info(&item).0, Some(PathBuf::from("/media/X/Projet")));
        // Restauration : un simple renommage sur le même disque.
        fs::write(
            info_path(&item),
            "[Trash Info]\nPath=Travail/Projet\nDeletionDate=2026-11-09T13:24:33\n",
        )
        .unwrap();
        let restored = restore(&item).unwrap();
        assert_eq!(restored, top.join("Travail/Projet"));
        assert!(!info_path(&item).exists());
        fs::remove_dir_all(top).unwrap();
    }

    #[test]
    fn racine_du_disque() {
        // La racine est un parent sur le même disque, dont le propre parent est ailleurs.
        let path = std::env::temp_dir();
        let top = top_dir(&path).unwrap();
        assert!(path.starts_with(&top));
        let device = |path: &Path| fs::metadata(path).unwrap().dev();
        assert_eq!(device(&top), device(&path));
        if let Some(parent) = top.parent() {
            assert_ne!(device(parent), device(&top));
        }
    }
}
