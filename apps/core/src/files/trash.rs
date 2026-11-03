//! Corbeille, au format freedesktop (« Trash specification ») : les éléments vont dans
//! `~/.local/share/Trash/files`, avec une fiche `info/NOM.trashinfo` (chemin d'origine
//! et date). Les autres gestionnaires de fichiers Linux voient donc la même Corbeille.

use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::ops::{self, Job, remove_tree, unique_destination};

/// Élément de la Corbeille.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItem {
    /// Emplacement dans `Trash/files`.
    pub path: PathBuf,
    pub original: PathBuf,
    /// « 2026-10-26T18:05:00 », tel qu'écrit dans la fiche.
    pub deleted: String,
}

pub fn root() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Trash")
}

pub fn files_dir() -> PathBuf {
    root().join("files")
}

fn info_path(item: &Path) -> PathBuf {
    let name = item
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    root().join("info").join(format!("{name}.trashinfo"))
}

/// Met `path` à la Corbeille ; renvoie sa place dans la Corbeille (pour [`restore`]).
pub fn trash(path: &Path) -> Result<PathBuf, String> {
    let files = files_dir();
    let info = root().join("info");
    fs::create_dir_all(&files).map_err(|error| error.to_string())?;
    fs::create_dir_all(&info).map_err(|error| error.to_string())?;
    let absolute = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let name = absolute
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or("Élément sans nom")?;
    let target = unique_destination(&files, &name);
    let fiche = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        encode_path(&absolute),
        timestamp(SystemTime::now())
    );
    fs::write(info_path(&target), fiche).map_err(|error| error.to_string())?;
    if fs::rename(&absolute, &target).is_err() {
        // Autre disque (clé USB) : copie puis suppression.
        let job = Job::new("Corbeille");
        let result = ops::copy_into(std::slice::from_ref(&absolute), &files, &job)
            .and_then(|()| remove_tree(&absolute));
        if let Err(error) = result {
            let _ = fs::remove_file(info_path(&target));
            return Err(error);
        }
    }
    Ok(target)
}

/// Éléments de la Corbeille, les plus récents d'abord.
pub fn list() -> Vec<TrashItem> {
    let Ok(entries) = fs::read_dir(files_dir()) else {
        return Vec::new();
    };
    let mut items: Vec<TrashItem> = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            let (original, deleted) = fs::read_to_string(info_path(&path))
                .ok()
                .map(|text| parse_info(&text))
                .unwrap_or_default();
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
    let (original, _) = fs::read_to_string(info_path(item))
        .ok()
        .map(|text| parse_info(&text))
        .unwrap_or_default();
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
    fs::read_dir(files_dir()).map_or(true, |mut entries| entries.next().is_none())
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
        fs::remove_dir_all(dir).unwrap();
    }
}
