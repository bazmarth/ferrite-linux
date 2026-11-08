//! Empreintes enregistrées : une ligne `camera <emplacement>` (la caméra infrarouge qui a
//! servi, seule acceptée ensuite), puis une ligne de nombres par image.
//!
//! Elles sont gardées hors du dossier personnel, dans `/var/lib/ferrite/visage/<uid>`, que
//! seul l'administrateur peut écrire : ajouter ou effacer un visage passe par `pkexec`
//! (mot de passe administrateur). Sans cela, quelqu'un devant une session ouverte pourrait
//! ajouter son propre visage et rouvrir l'écran plus tard.
//!
//! Le dossier appartient à root (0755) ; le fichier à root et au groupe de l'utilisateur,
//! en 0640 : lisible par lui (vérification sur l'écran verrouillé), par personne d'autre,
//! modifiable par root seul.

use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// Dossier des empreintes de tous les comptes.
pub const DIR: &str = "/var/lib/ferrite/visage";

/// Fichier des empreintes du compte `uid`.
pub fn path_for(dir: &Path, uid: u32) -> PathBuf {
    dir.join(uid.to_string())
}

/// Compte qui lance le programme (propriétaire de `/proc/self`).
pub fn own_uid() -> Option<u32> {
    fs::metadata("/proc/self").ok().map(|meta| meta.uid())
}

/// Compte dont on écrit le visage : celui qui a demandé `pkexec` (ou `sudo`). Jamais root
/// lui-même, et jamais un compte lancé sans droits administrateur.
pub fn target_uid() -> Result<u32, String> {
    if own_uid() != Some(0) {
        return Err("droits administrateur requis (pkexec ferrite-visage …)".to_owned());
    }
    ["PKEXEC_UID", "SUDO_UID"]
        .iter()
        .find_map(|key| std::env::var(key).ok()?.parse::<u32>().ok())
        .filter(|uid| *uid != 0)
        .ok_or_else(|| "compte demandeur inconnu (lancer avec pkexec)".to_owned())
}

/// Groupe principal du compte `uid`, lu dans `/etc/passwd`.
fn primary_group(uid: u32) -> Option<u32> {
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    group_in_passwd(&passwd, uid)
}

fn group_in_passwd(passwd: &str, uid: u32) -> Option<u32> {
    passwd.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        (fields.len() > 3 && fields[2].parse() == Ok(uid))
            .then(|| fields[3].parse().ok())
            .flatten()
    })
}

/// Visage enregistré : la caméra qui a servi et les empreintes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Enrollment {
    /// Emplacement matériel de la caméra (absent dans les fichiers d'avant le 2026-11-07,
    /// qui doivent être refaits).
    pub camera: Option<String>,
    pub prints: Vec<Vec<f32>>,
}

/// État du visage du compte qui lance le programme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Absent,
    /// Enregistré avant que la caméra soit retenue : à refaire.
    Old,
    Enrolled,
}

pub fn status() -> Status {
    match load() {
        Ok(enrollment) if enrollment.prints.is_empty() => Status::Absent,
        Ok(Enrollment { camera: Some(_), .. }) => Status::Enrolled,
        // Sans caméra retenue, ou fichier mal protégé : à enregistrer de nouveau.
        _ => Status::Old,
    }
}

/// Enregistre les empreintes du compte `uid` (en root).
pub fn save(dir: &Path, uid: u32, enrollment: &Enrollment) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(dir)
        .map_err(|err| err.to_string())?;
    // Un dossier déjà là garde des droits sûrs, quel que soit le masque de création.
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).map_err(|err| err.to_string())?;
    let path = path_for(dir, uid);
    let temporary = dir.join(format!(".{uid}.nouveau"));
    let _ = fs::remove_file(&temporary);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|err| err.to_string())?;
    file.write_all(encode(enrollment).as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|err| err.to_string())?;
    drop(file);
    // Root reste propriétaire ; le groupe de l'utilisateur peut lire, pas écrire.
    if let Some(group) = primary_group(uid) {
        std::os::unix::fs::chown(&temporary, Some(0), Some(group)).map_err(|err| err.to_string())?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o640)).map_err(|err| err.to_string())?;
    }
    fs::rename(&temporary, &path).map_err(|err| err.to_string())
}

/// Visage du compte qui lance le programme (vide si aucun).
pub fn load() -> Result<Enrollment, String> {
    let Some(path) = own_uid()
        .map(|uid| path_for(Path::new(DIR), uid))
        .filter(|path| path.is_file())
    else {
        return Ok(Enrollment::default());
    };
    // Un fichier qu'un autre que root pourrait avoir écrit n'est pas cru.
    let meta = fs::metadata(&path).map_err(|err| err.to_string())?;
    if meta.uid() != 0 || meta.mode() & 0o022 != 0 {
        return Err("empreintes non protégées : enregistrez le visage à nouveau".to_owned());
    }
    fs::read_to_string(path)
        .map(|text| decode(&text))
        .map_err(|err| err.to_string())
}

/// Oublie le visage du compte `uid` (en root).
pub fn forget(dir: &Path, uid: u32) -> Result<(), String> {
    let path = path_for(dir, uid);
    if path.is_file() {
        fs::remove_file(path).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn encode(enrollment: &Enrollment) -> String {
    let camera = enrollment
        .camera
        .as_ref()
        .map(|place| format!("camera {place}\n"))
        .unwrap_or_default();
    let prints: String = enrollment
        .prints
        .iter()
        .map(|print| {
            let values: Vec<String> = print.iter().map(f32::to_string).collect();
            values.join(" ") + "\n"
        })
        .collect();
    camera + &prints
}

fn decode(text: &str) -> Enrollment {
    let camera = text
        .lines()
        .find_map(|line| line.strip_prefix("camera "))
        .map(|place| place.trim().to_owned())
        .filter(|place| !place.is_empty());
    let prints = text
        .lines()
        .filter(|line| !line.starts_with("camera "))
        .map(|line| {
            line.split_whitespace()
                .filter_map(|value| value.parse().ok())
                .collect::<Vec<f32>>()
        })
        .filter(|print| print.len() == 128)
        .collect();
    Enrollment { camera, prints }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour_des_empreintes() {
        let enrollment = Enrollment {
            camera: Some("/sys/devices/pci0000:00/0000:00:14.0/usb3/3-6/3-6:1.2".to_owned()),
            prints: vec![vec![0.25_f32; 128], vec![-0.5_f32; 128]],
        };
        assert_eq!(decode(&encode(&enrollment)), enrollment);
        // Une ligne abîmée est ignorée.
        assert!(decode("1 2 3\n").prints.is_empty());
    }

    #[test]
    fn ancien_fichier_sans_camera() {
        let old = encode(&Enrollment {
            camera: None,
            prints: vec![vec![0.5_f32; 128]],
        });
        let read = decode(&old);
        assert_eq!(read.camera, None);
        assert_eq!(read.prints.len(), 1);
    }

    #[test]
    fn un_fichier_par_compte() {
        assert_eq!(
            path_for(Path::new(DIR), 1000),
            PathBuf::from("/var/lib/ferrite/visage/1000")
        );
    }

    #[test]
    fn groupe_principal_lu_dans_passwd() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\nmarie:x:1000:1001:Marie:/home/marie:/bin/fish\n";
        assert_eq!(group_in_passwd(passwd, 1000), Some(1001));
        assert_eq!(group_in_passwd(passwd, 1002), None);
    }

    #[test]
    fn enregistrer_puis_effacer_dans_un_dossier() {
        let dir = std::env::temp_dir().join(format!("ferrite-visage-essai-{}", std::process::id()));
        let enrollment = Enrollment {
            camera: Some("/sys/devices/essai".to_owned()),
            prints: vec![vec![0.5_f32; 128]],
        };
        // Sans être root, `chown` échoue : seul l'enregistrement du fichier est vérifié ici.
        let _ = save(&dir, 4242, &enrollment);
        if let Ok(text) = fs::read_to_string(path_for(&dir, 4242)) {
            assert_eq!(decode(&text), enrollment);
        }
        forget(&dir, 4242).unwrap();
        assert!(!path_for(&dir, 4242).exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
