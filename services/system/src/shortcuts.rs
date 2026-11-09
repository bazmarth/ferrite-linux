//! Raccourcis posés sur le bureau : applications, dossiers et fichiers.
//!
//! Gardés dans `~/.config/ferrite/raccourcis-bureau`, un par ligne, dans l'ordre
//! d'affichage : `application <identifiant>` ou `chemin <chemin absolu>`. Le bureau les
//! relit dès que le fichier change ; Fichiers et le bureau lui-même y ajoutent des lignes.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::config::Settings;

/// Un raccourci du bureau.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Shortcut {
    /// Application : `app_id` d'une application Ferrite, ou nom du fichier `.desktop`.
    App(String),
    /// Dossier ou fichier, ouvert dans son application.
    Path(PathBuf),
}

impl Shortcut {
    fn line(&self) -> Option<String> {
        match self {
            Shortcut::App(token) if !token.is_empty() && !token.contains('\n') => {
                Some(format!("application {token}"))
            }
            Shortcut::Path(path) => {
                let text = path.to_str()?;
                (path.is_absolute() && !text.contains('\n')).then(|| format!("chemin {text}"))
            }
            Shortcut::App(_) => None,
        }
    }

    fn parse(line: &str) -> Option<Shortcut> {
        if let Some(token) = line.strip_prefix("application ") {
            let token = token.trim();
            return (!token.is_empty()).then(|| Shortcut::App(token.to_owned()));
        }
        let path = Path::new(line.strip_prefix("chemin ")?);
        path.is_absolute().then(|| Shortcut::Path(path.to_owned()))
    }

    /// Nom affiché d'un dossier ou d'un fichier (celui d'une application vient du bureau).
    pub fn file_name(path: &Path) -> String {
        path.file_name().map_or_else(
            || path.to_string_lossy().into_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
    }
}

/// Fichier des raccourcis, à côté des réglages.
pub fn path() -> PathBuf {
    Settings::path().with_file_name("raccourcis-bureau")
}

/// Date de dernière modification du fichier (pour voir qu'il a changé).
pub fn modified() -> Option<SystemTime> {
    fs::metadata(path()).and_then(|metadata| metadata.modified()).ok()
}

fn parse(text: &str) -> Vec<Shortcut> {
    let mut list: Vec<Shortcut> = Vec::new();
    for shortcut in text.lines().filter_map(Shortcut::parse) {
        if !list.contains(&shortcut) {
            list.push(shortcut);
        }
    }
    list
}

fn text(list: &[Shortcut]) -> String {
    list.iter()
        .filter_map(Shortcut::line)
        .map(|line| line + "\n")
        .collect()
}

/// Raccourcis enregistrés ; aucun si le fichier manque.
pub fn load() -> Vec<Shortcut> {
    fs::read_to_string(path())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Enregistre la liste (écrite à côté puis renommée : le bureau ne lit jamais un fichier
/// à moitié écrit).
pub fn save(list: &[Shortcut]) -> io::Result<()> {
    let target = path();
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = target.with_file_name("raccourcis-bureau.tmp");
    fs::write(&temporary, text(list))?;
    fs::rename(temporary, target)
}

/// Ajoute des raccourcis à la fin ; ceux déjà présents ne sont pas doublés. Rend le
/// nombre de raccourcis ajoutés.
pub fn add(shortcuts: &[Shortcut]) -> io::Result<usize> {
    let mut list = load();
    let before = list.len();
    for shortcut in shortcuts {
        if shortcut.line().is_some() && !list.contains(shortcut) {
            list.push(shortcut.clone());
        }
    }
    let added = list.len() - before;
    if added > 0 {
        save(&list)?;
    }
    Ok(added)
}

/// Retire un raccourci.
pub fn remove(shortcut: &Shortcut) -> io::Result<()> {
    let mut list = load();
    let before = list.len();
    list.retain(|known| known != shortcut);
    if list.len() == before {
        return Ok(());
    }
    save(&list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour() {
        let list = vec![
            Shortcut::App("ferrite-files".to_owned()),
            Shortcut::Path(PathBuf::from("/home/moi/Documents")),
            Shortcut::Path(PathBuf::from("/home/moi/Mon fichier.pdf")),
        ];
        assert_eq!(parse(&text(&list)), list);
    }

    #[test]
    fn lignes_inconnues_relatives_et_doublons_ignores() {
        let text = "application brave.desktop\nchemin relatif/dossier\nautre chose\n\napplication brave.desktop\nchemin /tmp\n";
        assert_eq!(
            parse(text),
            vec![
                Shortcut::App("brave.desktop".to_owned()),
                Shortcut::Path(PathBuf::from("/tmp")),
            ]
        );
    }

    #[test]
    fn un_chemin_avec_retour_a_la_ligne_n_est_pas_ecrit() {
        let list = vec![Shortcut::Path(PathBuf::from("/tmp/a\nb"))];
        assert_eq!(text(&list), "");
    }

    #[test]
    fn nom_affiche() {
        assert_eq!(Shortcut::file_name(Path::new("/home/moi/Images")), "Images");
        assert_eq!(Shortcut::file_name(Path::new("/")), "/");
    }
}
