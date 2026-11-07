//! Applications installées : fichiers `.desktop` (spécification freedesktop) de Debian, de
//! Flatpak et de l'utilisateur, avec leur icône.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Application affichable dans le menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    /// Nom du fichier sans `.desktop` (identifiant).
    pub id: String,
    pub name: String,
    /// Commande et arguments, codes de champ (`%U`, `%f`…) retirés.
    pub exec: Vec<String>,
    /// À lancer dans un terminal (programmes en mode texte).
    pub terminal: bool,
    /// Fichier d'icône trouvé (SVG ou PNG).
    pub icon: Option<PathBuf>,
    /// Classe de fenêtre annoncée (`StartupWMClass`), pour reconnaître ses fenêtres.
    pub wm_class: Option<String>,
    /// Catégories freedesktop (`Categories`), pour ranger le lanceur par rubriques.
    pub categories: Vec<String>,
    /// Types de fichiers qu'elle sait ouvrir (`MimeType`), pour les applications par défaut.
    pub mime_types: Vec<String>,
}

/// Entrées déjà fournies par le bureau Ferrite lui-même (Dock, menu).
const HIDDEN_IDS: &[&str] = &["foot", "footclient", "foot-server", "org.codeberg.dnkl.foot"];

fn data_dirs() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let user_data = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".local/share")));
    let system = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    // Ordre croissant de priorité : les dernières entrées remplacent les premières.
    let mut dirs: Vec<PathBuf> = system.split(':').rev().map(PathBuf::from).collect();
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    if let Some(user_data) = user_data {
        dirs.push(user_data.join("flatpak/exports/share"));
        dirs.push(user_data);
    }
    dirs
}

/// Dossiers `applications` surveillés.
pub fn directories() -> Vec<PathBuf> {
    data_dirs()
        .into_iter()
        .map(|dir| dir.join("applications"))
        .collect()
}

/// Empreinte des dossiers : change quand une application est installée ou retirée.
pub fn stamp() -> Vec<Option<SystemTime>> {
    directories()
        .iter()
        .map(|dir| fs::metadata(dir).and_then(|metadata| metadata.modified()).ok())
        .collect()
}

/// Applications installées, triées par nom.
pub fn installed() -> Vec<DesktopEntry> {
    let mut by_id: BTreeMap<String, Option<DesktopEntry>> = BTreeMap::new();
    for dir in directories() {
        let Ok(files) = fs::read_dir(&dir) else { continue };
        for file in files.filter_map(Result::ok) {
            let path = file.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("desktop") {
                continue;
            }
            let Some(id) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
                continue;
            };
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            // Une entrée masquée plus prioritaire cache celle du système.
            by_id.insert(id.clone(), parse(&text, &id).filter(available));
        }
    }
    let mut entries: Vec<DesktopEntry> = by_id
        .into_values()
        .flatten()
        .map(|mut entry| {
            entry.icon = entry
                .icon
                .as_deref()
                .and_then(|icon| resolve_icon(&icon.to_string_lossy()));
            entry
        })
        .collect();
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    entries
}

/// Le programme existe-t-il (dans le `PATH` ou par son chemin) ?
fn available(entry: &DesktopEntry) -> bool {
    let Some(program) = entry.exec.first() else {
        return false;
    };
    let path = Path::new(program);
    if path.is_absolute() {
        return path.exists();
    }
    env::var_os("PATH").is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(program).exists()))
}

/// Analyse un fichier `.desktop` ; `None` s'il ne doit pas apparaître dans le menu.
/// L'icône est rendue telle quelle (nom ou chemin), à résoudre ensuite.
pub fn parse(text: &str, id: &str) -> Option<DesktopEntry> {
    if HIDDEN_IDS.contains(&id) {
        return None;
    }
    let mut in_entry = false;
    let mut keys: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            keys.entry(key.trim().to_owned())
                .or_insert_with(|| value.trim().to_owned());
        }
    }
    let flag = |key: &str| keys.get(key).is_some_and(|value| value == "true");
    if keys.get("Type").map(String::as_str) != Some("Application") || flag("NoDisplay") || flag("Hidden") {
        return None;
    }
    let shows_in = |key: &str| {
        keys.get(key)
            .map(|list| list.split(';').any(|desktop| desktop == "Ferrite"))
    };
    if shows_in("OnlyShowIn") == Some(false) || shows_in("NotShowIn") == Some(true) {
        return None;
    }
    let name = ["Name[fr_FR]", "Name[fr]", "Name"]
        .iter()
        .find_map(|key| keys.get(*key))
        .filter(|name| !name.is_empty())?
        .clone();
    let exec = split_exec(keys.get("Exec")?);
    if exec.is_empty() {
        return None;
    }
    Some(DesktopEntry {
        id: id.to_owned(),
        name,
        exec,
        terminal: flag("Terminal"),
        icon: keys
            .get("Icon")
            .filter(|icon| !icon.is_empty())
            .map(PathBuf::from),
        wm_class: keys.get("StartupWMClass").cloned(),
        categories: keys
            .get("Categories")
            .map(|list| {
                list.split(';')
                    .filter(|category| !category.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        mime_types: keys
            .get("MimeType")
            .map(|list| {
                list.split(';')
                    .filter(|mime| !mime.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Découpe la ligne `Exec` en arguments (guillemets et échappements) et retire les codes
/// de champ (`%f`, `%U`…), le lancement depuis le menu ne transmettant aucun fichier.
pub fn split_exec(exec: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = exec.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ' ' | '\t' if !quoted => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            // `%%` est un « % » littéral ; les autres codes de champ sont supprimés.
            '%' => {
                if chars.next() == Some('%') {
                    current.push('%');
                    started = true;
                }
            }
            other => {
                current.push(other);
                started = true;
            }
        }
    }
    if started {
        args.push(current);
    }
    args.retain(|arg| !arg.is_empty());
    args
}

/// Trouve le fichier d'une icône : chemin absolu, thème `hicolor` (le plus grand), puis
/// `/usr/share/pixmaps`.
pub fn resolve_icon(icon: &str) -> Option<PathBuf> {
    let path = Path::new(icon);
    if path.is_absolute() {
        return path.exists().then(|| path.to_path_buf());
    }
    const SIZES: &[&str] = &[
        "scalable", "512x512", "256x256", "192x192", "128x128", "96x96", "72x72", "64x64", "48x48", "32x32",
    ];
    for dir in data_dirs().iter().rev() {
        for theme in ["hicolor", "Adwaita"] {
            for size in SIZES {
                for extension in ["svg", "png"] {
                    let candidate = dir.join(format!("icons/{theme}/{size}/apps/{icon}.{extension}"));
                    if candidate.exists() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    ["svg", "png"]
        .iter()
        .map(|extension| PathBuf::from(format!("/usr/share/pixmaps/{icon}.{extension}")))
        .find(|candidate| candidate.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_d_un_fichier_desktop() {
        let text = "\
[Desktop Entry]
Type=Application
Name=VLC media player
Name[fr]=Lecteur multimédia VLC
Exec=/usr/bin/vlc --started-from-file %U
Icon=vlc
StartupWMClass=vlc
Categories=AudioVideo;Player;Recorder;

[Desktop Action new]
Name=Nouvelle fenêtre
Exec=vlc --other
";
        let entry = parse(text, "vlc").unwrap();
        assert_eq!(entry.name, "Lecteur multimédia VLC");
        assert_eq!(entry.exec, vec!["/usr/bin/vlc", "--started-from-file"]);
        assert_eq!(entry.icon, Some(PathBuf::from("vlc")));
        assert_eq!(entry.wm_class.as_deref(), Some("vlc"));
        assert_eq!(entry.categories, ["AudioVideo", "Player", "Recorder"]);
        assert!(!entry.terminal);
    }

    #[test]
    fn entrees_masquees() {
        let hidden = "[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n";
        assert_eq!(parse(hidden, "x"), None);
        let gnome = "[Desktop Entry]\nType=Application\nName=X\nExec=x\nOnlyShowIn=GNOME;\n";
        assert_eq!(parse(gnome, "x"), None);
        let link = "[Desktop Entry]\nType=Link\nName=X\nURL=https://example.org\n";
        assert_eq!(parse(link, "x"), None);
        let foot = "[Desktop Entry]\nType=Application\nName=Foot\nExec=foot\n";
        assert_eq!(parse(foot, "foot"), None);
    }

    #[test]
    fn decoupage_de_la_commande() {
        assert_eq!(
            split_exec(r#"/opt/brave.com/brave/brave-browser --flag "un dossier/avec espace" %U"#),
            vec![
                "/opt/brave.com/brave/brave-browser",
                "--flag",
                "un dossier/avec espace"
            ]
        );
        assert_eq!(split_exec("env A=1 app %% %f"), vec!["env", "A=1", "app", "%"]);
    }
}
