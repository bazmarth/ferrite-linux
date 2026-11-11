//! Fichiers lâchés sur une application Flatpak.
//!
//! Une application Flatpak ne voit que les dossiers que son bac à sable lui ouvre : un
//! chemin `~/Téléchargements/photo.png` reçu tel quel lui reste introuvable. Avant de les
//! lui donner, ferrite-shell partage donc les fichiers avec elle par le portail de documents
//! (`org.freedesktop.portal.Documents`), qui les lui montre sous `/run/user/…/doc/…`.

use std::{
    collections::HashMap,
    ffi::OsString,
    fs::{self, File},
    os::unix::ffi::OsStringExt,
    path::PathBuf,
};

use tracing::warn;
use zbus::zvariant::{Fd, OwnedValue};

/// Réutiliser le document si le fichier est déjà partagé.
const REUSE_EXISTING: u32 = 1;
/// Ne rien partager si l'application voit déjà le fichier : son chemin reste le bon.
const AS_NEEDED_BY_APP: u32 = 4;
/// Partager des dossiers (tous les fichiers de l'appel doivent alors en être).
const EXPORT_DIRECTORY: u32 = 8;

/// Application Flatpak du processus `pid`, d'après le `.flatpak-info` de son bac à sable.
pub fn flatpak_app(pid: u32) -> Option<String> {
    let info = fs::read_to_string(format!("/proc/{pid}/root/.flatpak-info")).ok()?;
    app_name(&info)
}

/// Nom de l'application (`[Application]`, `name=`) d'un `.flatpak-info`.
fn app_name(info: &str) -> Option<String> {
    let mut application = false;
    for line in info.lines().map(str::trim) {
        if line.starts_with('[') {
            application = line == "[Application]";
        } else if application && let Some(name) = line.strip_prefix("name=") {
            let name = name.trim();
            return (!name.is_empty()).then(|| name.to_owned());
        }
    }
    None
}

/// Chemins de `paths` tels que l'application `app` peut les ouvrir. En cas d'échec du
/// portail, les chemins d'origine (l'application dira qu'elle ne les trouve pas).
pub fn export(app: &str, paths: &[PathBuf]) -> Vec<PathBuf> {
    match try_export(app, paths) {
        Ok(exported) => exported,
        Err(err) => {
            warn!(%err, app, "fichiers non partagés avec l'application Flatpak");
            paths.to_vec()
        }
    }
}

fn try_export(app: &str, paths: &[PathBuf]) -> zbus::Result<Vec<PathBuf>> {
    let connection = zbus::blocking::Connection::session()?;
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.portal.Documents",
        "/org/freedesktop/portal/documents",
        "org.freedesktop.portal.Documents",
    )?;
    let mut mount: Vec<u8> = proxy.call("GetMountPoint", &())?;
    if mount.last() == Some(&0) {
        mount.pop();
    }
    let mount = PathBuf::from(OsString::from_vec(mount));
    let mut exported = paths.to_vec();
    // Fichiers et dossiers vont dans deux appels : un appel ne mélange pas les deux.
    for directories in [false, true] {
        let opened: Vec<(usize, File)> = paths
            .iter()
            .enumerate()
            .filter(|(_, path)| path.is_dir() == directories)
            .filter_map(|(index, path)| Some((index, File::open(path).ok()?)))
            .collect();
        if opened.is_empty() {
            continue;
        }
        let fds: Vec<Fd> = opened.iter().map(|(_, file)| Fd::from(file)).collect();
        let mut flags = REUSE_EXISTING | AS_NEEDED_BY_APP;
        if directories {
            flags |= EXPORT_DIRECTORY;
        }
        let (ids, _): (Vec<String>, HashMap<String, OwnedValue>) =
            proxy.call("AddFull", &(fds, flags, app, vec!["read"]))?;
        for ((index, _), id) in opened.iter().zip(ids) {
            // Identifiant vide : l'application voit déjà le fichier à son chemin.
            if let (false, Some(name)) = (id.is_empty(), paths[*index].file_name()) {
                exported[*index] = mount.join(id).join(name);
            }
        }
    }
    Ok(exported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nom_de_l_application() {
        let info = "[Instance]\nname=pas-ca\n\n[Application]\nname=ai.storyteller.photocraft\n\
                    runtime=runtime/org.freedesktop.Platform/x86_64/26.08\n";
        assert_eq!(app_name(info).as_deref(), Some("ai.storyteller.photocraft"));
        assert_eq!(app_name("[Runtime]\nname=org.gnome.Platform\n"), None);
        assert_eq!(app_name("[Application]\nname=\n"), None);
    }
}
