//! Raccourcis posés sur le bureau : applications, dossiers et fichiers.
//!
//! Gardés dans `~/.config/ferrite/raccourcis-bureau`, un par ligne, dans l'ordre
//! d'affichage : `application <identifiant>` ou `chemin <chemin absolu>`, précédé de la
//! case choisie à la souris (`3,1 chemin …` : colonne comptée depuis le bord droit, puis
//! ligne). Une ligne `taille <pixels>` donne la taille des icônes. Le bureau les relit dès
//! que le fichier change ; Fichiers et le bureau lui-même y ajoutent des lignes.

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

/// Taille des icônes du bureau, en pixels logiques : par défaut, plus petite, plus grande.
pub const ICON_SIZE: u32 = 52;
pub const ICON_SIZE_MIN: u32 = 36;
pub const ICON_SIZE_MAX: u32 = 112;

/// Case de la grille du bureau : colonne comptée depuis le bord droit, puis ligne.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell {
    pub column: u16,
    pub row: u16,
}

/// Un raccourci et la case où l'utilisateur l'a posé (aucune : rangé à la suite).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub shortcut: Shortcut,
    pub cell: Option<Cell>,
}

/// Tout le bureau : la taille des icônes et les raccourcis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desktop {
    pub icon_size: u32,
    pub items: Vec<Placed>,
}

impl Default for Desktop {
    fn default() -> Self {
        Self {
            icon_size: ICON_SIZE,
            items: Vec::new(),
        }
    }
}

impl Desktop {
    pub fn contains(&self, shortcut: &Shortcut) -> bool {
        self.items.iter().any(|item| item.shortcut == *shortcut)
    }

    /// Ajoute à la fin, sans doubler ; rend le nombre de raccourcis ajoutés.
    pub fn add(&mut self, shortcuts: &[Shortcut]) -> usize {
        let before = self.items.len();
        for shortcut in shortcuts {
            if shortcut.line().is_some() && !self.contains(shortcut) {
                self.items.push(Placed {
                    shortcut: shortcut.clone(),
                    cell: None,
                });
            }
        }
        self.items.len() - before
    }

    /// Taille ramenée dans les bornes.
    pub fn clamp_size(size: u32) -> u32 {
        size.clamp(ICON_SIZE_MIN, ICON_SIZE_MAX)
    }
}

fn parse_cell(word: &str) -> Option<Cell> {
    let (column, row) = word.split_once(',')?;
    Some(Cell {
        column: column.parse().ok()?,
        row: row.parse().ok()?,
    })
}

/// Fichier des raccourcis, à côté des réglages.
pub fn path() -> PathBuf {
    Settings::path().with_file_name("raccourcis-bureau")
}

/// Date de dernière modification du fichier (pour voir qu'il a changé).
pub fn modified() -> Option<SystemTime> {
    fs::metadata(path()).and_then(|metadata| metadata.modified()).ok()
}

fn parse(text: &str) -> Desktop {
    let mut desktop = Desktop::default();
    for line in text.lines() {
        if let Some(size) = line.strip_prefix("taille ") {
            if let Ok(size) = size.trim().parse() {
                desktop.icon_size = Desktop::clamp_size(size);
            }
            continue;
        }
        let (cell, rest) = match line.split_once(' ') {
            Some((word, rest)) => match parse_cell(word) {
                Some(cell) => (Some(cell), rest),
                None => (None, line),
            },
            None => (None, line),
        };
        if let Some(shortcut) = Shortcut::parse(rest)
            && !desktop.contains(&shortcut)
        {
            desktop.items.push(Placed { shortcut, cell });
        }
    }
    desktop
}

fn text(desktop: &Desktop) -> String {
    let mut text = String::new();
    if desktop.icon_size != ICON_SIZE {
        text.push_str(&format!("taille {}\n", desktop.icon_size));
    }
    for item in &desktop.items {
        let Some(line) = item.shortcut.line() else {
            continue;
        };
        if let Some(cell) = item.cell {
            text.push_str(&format!("{},{} ", cell.column, cell.row));
        }
        text.push_str(&line);
        text.push('\n');
    }
    text
}

/// Bureau enregistré ; vide si le fichier manque.
pub fn load() -> Desktop {
    fs::read_to_string(path())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Enregistre le bureau (écrit à côté puis renommé : le bureau ne lit jamais un fichier
/// à moitié écrit).
pub fn save(desktop: &Desktop) -> io::Result<()> {
    let target = path();
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = target.with_file_name("raccourcis-bureau.tmp");
    fs::write(&temporary, text(desktop))?;
    fs::rename(temporary, target)
}

/// Ajoute des raccourcis à la fin ; ceux déjà présents ne sont pas doublés. Rend le
/// nombre de raccourcis ajoutés.
pub fn add(shortcuts: &[Shortcut]) -> io::Result<usize> {
    let mut desktop = load();
    let added = desktop.add(shortcuts);
    if added > 0 {
        save(&desktop)?;
    }
    Ok(added)
}

/// Retire un raccourci.
pub fn remove(shortcut: &Shortcut) -> io::Result<()> {
    let mut desktop = load();
    let before = desktop.items.len();
    desktop.items.retain(|item| item.shortcut != *shortcut);
    if desktop.items.len() == before {
        return Ok(());
    }
    save(&desktop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour() {
        let desktop = Desktop {
            icon_size: 80,
            items: vec![
                Placed {
                    shortcut: Shortcut::App("ferrite-files".to_owned()),
                    cell: None,
                },
                Placed {
                    shortcut: Shortcut::Path(PathBuf::from("/home/moi/Documents")),
                    cell: Some(Cell { column: 3, row: 1 }),
                },
                Placed {
                    shortcut: Shortcut::Path(PathBuf::from("/home/moi/Mon fichier.pdf")),
                    cell: Some(Cell { column: 0, row: 0 }),
                },
            ],
        };
        assert_eq!(parse(&text(&desktop)), desktop);
    }

    #[test]
    fn ancien_fichier_sans_case_ni_taille() {
        let desktop = parse("chemin /tmp\napplication brave.desktop\ntaille 9999\n");
        assert_eq!(desktop.icon_size, ICON_SIZE_MAX);
        assert!(desktop.items.iter().all(|item| item.cell.is_none()));
        assert_eq!(desktop.items.len(), 2);
    }

    #[test]
    fn lignes_inconnues_relatives_et_doublons_ignores() {
        let text = "application brave.desktop\nchemin relatif/dossier\nautre chose\n\napplication brave.desktop\nchemin /tmp\n";
        let list: Vec<Shortcut> = parse(text).items.into_iter().map(|item| item.shortcut).collect();
        assert_eq!(
            list,
            vec![
                Shortcut::App("brave.desktop".to_owned()),
                Shortcut::Path(PathBuf::from("/tmp")),
            ]
        );
    }

    #[test]
    fn un_chemin_avec_retour_a_la_ligne_n_est_pas_ecrit() {
        let mut desktop = Desktop::default();
        desktop.items.push(Placed {
            shortcut: Shortcut::Path(PathBuf::from("/tmp/a\nb")),
            cell: None,
        });
        assert_eq!(text(&desktop), "");
    }

    #[test]
    fn nom_affiche() {
        assert_eq!(Shortcut::file_name(Path::new("/home/moi/Images")), "Images");
        assert_eq!(Shortcut::file_name(Path::new("/")), "/");
    }
}
