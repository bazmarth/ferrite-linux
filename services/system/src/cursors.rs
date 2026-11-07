//! Thèmes de curseurs, taille du curseur et taille du texte (Paramètres → Apparence).
//!
//! Le bureau dessine lui-même le pointeur. Deux thèmes Ferrite sont toujours là, dans le
//! binaire du bureau : les formes vectorielles d'origine (`ferrite`, par défaut) et le pack
//! `Ferrite-Cursors` de `ferrite-theme`. S'y ajoutent les thèmes X installés sur la machine
//! (dossiers `icons/<thème>/cursors`, comme Adwaita).

use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Formes vectorielles du bureau, sans fichier.
pub const BUILTIN: &str = "ferrite";
/// Pack de curseurs de `ferrite-theme`, embarqué dans le bureau.
pub const FERRITE_PACK: &str = "Ferrite-Cursors";

/// Tailles proposées, en pixels logiques (24 : celle d'origine).
pub const SIZES: [(u32, &str); 3] = [(24, "Normale"), (32, "Grande"), (48, "Très grande")];
pub const DEFAULT_SIZE: u32 = 24;

/// Tailles du texte proposées, en pour cent.
pub const TEXT_SIZES: [u32; 5] = [90, 100, 110, 125, 150];

/// Un thème de curseurs : son dossier (identifiant) et son nom affiché.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorTheme {
    pub id: String,
    pub name: String,
}

/// Thèmes proposés : les deux de Ferrite, puis ceux installés, triés par nom.
pub fn themes() -> Vec<CursorTheme> {
    themes_in(&icon_dirs())
}

/// Dossiers où chercher les thèmes, comme libXcursor : `~/.icons`, puis `icons` de chaque
/// dossier de données.
fn icon_dirs() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let mut dirs = Vec::new();
    if let Some(home) = &home {
        dirs.push(home.join(".icons"));
    }
    let user_data = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|home| home.join(".local/share")));
    if let Some(user_data) = user_data {
        dirs.push(user_data.join("icons"));
    }
    let system = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.extend(system.split(':').map(|dir| Path::new(dir).join("icons")));
    dirs.push(PathBuf::from("/usr/share/pixmaps"));
    dirs
}

fn themes_in(roots: &[PathBuf]) -> Vec<CursorTheme> {
    let mut installed: Vec<CursorTheme> = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            let Some(id) = dir.file_name().and_then(|name| name.to_str()).map(str::to_owned) else {
                continue;
            };
            // Un thème d'icônes sans curseurs (hicolor…) n'est pas proposé.
            if !dir.join("cursors").is_dir()
                || id == BUILTIN
                || id == FERRITE_PACK
                || installed.iter().any(|theme| theme.id == id)
            {
                continue;
            }
            let name = theme_name(&dir.join("index.theme")).unwrap_or_else(|| id.clone());
            installed.push(CursorTheme { id, name });
        }
    }
    installed.sort_by_key(|theme| theme.name.to_lowercase());
    let mut themes = vec![
        CursorTheme {
            id: BUILTIN.to_owned(),
            name: "Ferrite".to_owned(),
        },
        CursorTheme {
            id: FERRITE_PACK.to_owned(),
            name: "Ferrite Cursors".to_owned(),
        },
    ];
    themes.extend(installed);
    themes
}

/// `Name=` de la section `[Icon Theme]`.
fn theme_name(index: &Path) -> Option<String> {
    let text = fs::read_to_string(index).ok()?;
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[Icon Theme]";
        } else if in_section && let Some(name) = line.strip_prefix("Name=") {
            let name = name.trim();
            return (!name.is_empty()).then(|| name.to_owned());
        }
    }
    None
}

/// Taille enregistrée ramenée à la plus proche des tailles proposées.
pub fn parse_size(value: Option<&str>) -> u32 {
    let Some(size) = value.and_then(|value| value.trim().parse::<u32>().ok()) else {
        return DEFAULT_SIZE;
    };
    nearest(size, SIZES.map(|(size, _)| size))
}

/// Taille du texte enregistrée ramenée à la plus proche de celles proposées.
pub fn parse_text_size(value: Option<&str>) -> u32 {
    let Some(size) = value.and_then(|value| value.trim().parse::<u32>().ok()) else {
        return 100;
    };
    nearest(size, TEXT_SIZES)
}

fn nearest<const N: usize>(value: u32, choices: [u32; N]) -> u32 {
    choices
        .into_iter()
        .min_by_key(|choice| choice.abs_diff(value))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailles_ramenees_aux_choix() {
        assert_eq!(parse_size(None), 24);
        assert_eq!(parse_size(Some("32")), 32);
        assert_eq!(parse_size(Some("40")), 32);
        assert_eq!(parse_size(Some("200")), 48);
        assert_eq!(parse_size(Some("abc")), 24);
        assert_eq!(parse_text_size(None), 100);
        assert_eq!(parse_text_size(Some("125")), 125);
        assert_eq!(parse_text_size(Some("300")), 150);
        assert_eq!(parse_text_size(Some("10")), 90);
    }

    #[test]
    fn themes_installes_avec_leur_nom() {
        let root = env::temp_dir().join(format!("ferrite-curseurs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Zeta/cursors")).unwrap();
        fs::create_dir_all(root.join("Adwaita/cursors")).unwrap();
        fs::write(
            root.join("Adwaita/index.theme"),
            "[Icon Theme]\nName=Adwaita\nComment=x\n",
        )
        .unwrap();
        // Thème d'icônes sans curseurs : ignoré.
        fs::create_dir_all(root.join("hicolor/48x48")).unwrap();
        // Le pack Ferrite installé ne fait pas doublon avec celui du bureau.
        fs::create_dir_all(root.join("Ferrite-Cursors/cursors")).unwrap();

        let themes = themes_in(&[root.clone(), root.clone()]);
        let ids: Vec<&str> = themes.iter().map(|theme| theme.id.as_str()).collect();
        assert_eq!(ids, [BUILTIN, FERRITE_PACK, "Adwaita", "Zeta"]);
        assert_eq!(themes[2].name, "Adwaita");
        assert_eq!(themes[3].name, "Zeta");
        let _ = fs::remove_dir_all(&root);
    }
}
