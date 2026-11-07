//! Applications par défaut : quelle application ouvre une page web, un courriel, une
//! image, une vidéo… Choix écrits dans le fichier standard `~/.config/mimeapps.list`
//! (section `[Default Applications]`), que lisent `xdg-open`, GTK, Qt, les navigateurs
//! et Fichiers. Les valeurs de Ferrite viennent de `/etc/xdg/mimeapps.list` (image).

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::applications::DesktopEntry;

/// Rubrique de Paramètres → Applications par défaut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Category {
    pub label: &'static str,
    /// Types couverts ; le premier sert à lire le choix actuel.
    pub mimes: &'static [&'static str],
    /// Application Ferrite de la rubrique (fichier `.desktop`) et son nom.
    pub ferrite: Option<(&'static str, &'static str)>,
}

pub const CATEGORIES: &[Category] = &[
    Category {
        label: "Navigateur web",
        mimes: &["x-scheme-handler/https", "x-scheme-handler/http", "text/html"],
        ferrite: None,
    },
    Category {
        label: "Courriel",
        mimes: &["x-scheme-handler/mailto"],
        ferrite: None,
    },
    Category {
        label: "Dossiers",
        mimes: &["inode/directory"],
        ferrite: Some(("ferrite-files.desktop", "Fichiers")),
    },
    Category {
        label: "Images",
        // Les formats que la Visionneuse sait ouvrir.
        mimes: &["image/png", "image/jpeg"],
        ferrite: Some(("ferrite-viewer.desktop", "Visionneuse")),
    },
    Category {
        label: "Vidéos",
        mimes: &[
            "video/mp4",
            "video/x-matroska",
            "video/webm",
            "video/x-msvideo",
            "video/quicktime",
            "video/mpeg",
            "video/ogg",
        ],
        ferrite: Some(("ferrite-video.desktop", "Lecteur vidéo")),
    },
    Category {
        label: "Musique",
        mimes: &[
            "audio/mpeg",
            "audio/flac",
            "audio/ogg",
            "audio/x-wav",
            "audio/mp4",
            "audio/aac",
        ],
        ferrite: Some(("ferrite-music.desktop", "Musique")),
    },
    Category {
        label: "Documents PDF",
        mimes: &["application/pdf"],
        ferrite: Some(("ferrite-pdf.desktop", "Lecteur PDF")),
    },
    Category {
        label: "Textes",
        mimes: &["text/plain", "text/markdown"],
        ferrite: Some(("ferrite-textedit.desktop", "TextEdit")),
    },
];

/// Application proposée : son fichier `.desktop` et son nom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub name: String,
}

/// Applications capables d'ouvrir la rubrique : celle de Ferrite d'abord, puis les
/// installées qui annoncent l'un de ses types.
pub fn candidates(category: &Category, installed: &[DesktopEntry]) -> Vec<Choice> {
    let mut choices: Vec<Choice> = category
        .ferrite
        .map(|(id, name)| Choice {
            id: id.to_owned(),
            name: name.to_owned(),
        })
        .into_iter()
        .collect();
    for entry in installed {
        let handles = entry
            .mime_types
            .iter()
            .any(|mime| category.mimes.contains(&mime.as_str()));
        let id = format!("{}.desktop", entry.id);
        if handles && !choices.iter().any(|choice| choice.id == id) {
            choices.push(Choice {
                id,
                name: entry.name.clone(),
            });
        }
    }
    choices
}

fn config_home() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

/// Fichier de l'utilisateur, où sont écrits ses choix.
pub fn user_file() -> Option<PathBuf> {
    config_home().map(|dir| dir.join("mimeapps.list"))
}

/// Fichiers lus, du plus prioritaire au moins prioritaire.
fn files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = user_file().into_iter().collect();
    files.push("/etc/xdg/mimeapps.list".into());
    if let Some(home) = env::var_os("HOME") {
        files.push(PathBuf::from(home).join(".local/share/applications/mimeapps.list"));
    }
    files.push("/usr/share/applications/mimeapps.list".into());
    files
}

/// Application par défaut de `mime` dans le texte d'un `mimeapps.list`.
fn default_in(text: &str, mime: &str) -> Option<String> {
    let mut in_defaults = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_defaults = line == "[Default Applications]";
            continue;
        }
        if !in_defaults {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == mime
        {
            return value
                .split(';')
                .map(str::trim)
                .find(|id| !id.is_empty())
                .map(str::to_owned);
        }
    }
    None
}

/// Application par défaut de `mime`, selon tous les fichiers.
pub fn default_for(mime: &str) -> Option<String> {
    files()
        .iter()
        .filter_map(|file| fs::read_to_string(file).ok())
        .find_map(|text| default_in(&text, mime))
}

/// Choix actuel de la rubrique.
pub fn current(category: &Category) -> Option<String> {
    category.mimes.first().and_then(|mime| default_for(mime))
}

/// `text` avec `id` comme application par défaut de chaque type de `mimes`, le reste du
/// fichier (autres sections, autres types, commentaires) inchangé.
fn with_defaults(text: &str, mimes: &[&str], id: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut in_defaults = false;
    let mut seen_section = false;
    let mut written = false;
    let new_lines: Vec<String> = mimes.iter().map(|mime| format!("{mime}={id};")).collect();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_defaults && !written {
                lines.extend(new_lines.iter().cloned());
                written = true;
            }
            in_defaults = trimmed == "[Default Applications]";
            seen_section |= in_defaults;
            lines.push(line.to_owned());
            continue;
        }
        if in_defaults
            && trimmed
                .split_once('=')
                .is_some_and(|(key, _)| mimes.contains(&key.trim()))
        {
            continue;
        }
        lines.push(line.to_owned());
    }
    if in_defaults && !written {
        // Section en fin de fichier : les lignes vont à sa suite, avant d'éventuelles
        // lignes vides finales.
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        lines.extend(new_lines.iter().cloned());
    } else if !seen_section {
        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("[Default Applications]".to_owned());
        lines.extend(new_lines.iter().cloned());
    }
    lines.join("\n") + "\n"
}

/// Fait de `id` l'application par défaut de la rubrique.
pub fn set(category: &Category, id: &str) -> Result<(), String> {
    let file = user_file().ok_or("dossier personnel inconnu")?;
    let text = fs::read_to_string(&file).unwrap_or_default();
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    }
    fs::write(&file, with_defaults(&text, category.mimes, id)).map_err(|err| err.to_string())
}

/// Type MIME d'un fichier d'après son extension (base `/usr/share/mime/globs`).
pub fn mime_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let globs = fs::read_to_string("/usr/share/mime/globs").ok()?;
    mime_in_globs(&globs, &name)
}

fn mime_in_globs(globs: &str, name: &str) -> Option<String> {
    globs
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once(':'))
        .find(|(_, glob)| {
            glob.strip_prefix('*')
                .is_some_and(|suffix| name.ends_with(suffix))
        })
        .map(|(mime, _)| mime.to_owned())
}

/// Application choisie pour ouvrir `path`, si elle n'est pas de Ferrite (Fichiers passe
/// alors la main à `xdg-open`, qui la lance).
pub fn foreign_default(path: &Path) -> Option<String> {
    let mime = if path.is_dir() {
        "inode/directory".to_owned()
    } else {
        mime_of(path)?
    };
    default_for(&mime).filter(|id| !id.starts_with("ferrite-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_l_application_par_defaut() {
        let text = "[Added Associations]\nimage/png=gimp.desktop;\n\n[Default Applications]\nimage/png=ferrite-viewer.desktop;gimp.desktop;\n";
        assert_eq!(
            default_in(text, "image/png").as_deref(),
            Some("ferrite-viewer.desktop")
        );
        assert_eq!(default_in(text, "image/gif"), None);
    }

    #[test]
    fn ecrit_sans_toucher_au_reste() {
        let text = "[Default Applications]\nx-scheme-handler/claude=claude.desktop\nimage/png=old.desktop;\n\n[Added Associations]\nx=y;\n";
        let written = with_defaults(text, &["image/png", "image/jpeg"], "gimp.desktop");
        assert!(written.contains("x-scheme-handler/claude=claude.desktop"));
        assert!(written.contains("image/png=gimp.desktop;"));
        assert!(written.contains("image/jpeg=gimp.desktop;"));
        assert!(!written.contains("old.desktop"));
        assert!(written.contains("[Added Associations]\nx=y;"));
        assert_eq!(
            default_in(&written, "image/jpeg").as_deref(),
            Some("gimp.desktop")
        );
        // Fichier vide : la section est créée.
        let fresh = with_defaults("", &["text/html"], "brave-browser.desktop");
        assert_eq!(
            fresh,
            "[Default Applications]\ntext/html=brave-browser.desktop;\n"
        );
    }

    #[test]
    fn type_d_un_fichier_par_son_extension() {
        let globs = "# x\napplication/pdf:*.pdf\nimage/png:*.png\n";
        assert_eq!(
            mime_in_globs(globs, "guide.pdf").as_deref(),
            Some("application/pdf")
        );
        assert_eq!(mime_in_globs(globs, "notes"), None);
    }
}
