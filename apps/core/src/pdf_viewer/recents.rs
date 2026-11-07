//! Documents récemment ouverts dans le Lecteur PDF, avec la page où l'on s'était arrêté.
//! Fichier `~/.local/share/ferrite/lecteur-pdf-recents`, une ligne par document :
//! `page<TAB>chemin`, du plus récent au plus ancien.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Nombre de documents gardés dans la liste.
pub const LIMIT: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent {
    pub path: PathBuf,
    /// Dernière page lue, numérotée depuis 1.
    pub page: usize,
}

fn file() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("ferrite/lecteur-pdf-recents")
}

pub fn parse(text: &str) -> Vec<Recent> {
    let mut list: Vec<Recent> = Vec::new();
    for line in text.lines() {
        let Some((page, path)) = line.split_once('\t') else {
            continue;
        };
        let path = PathBuf::from(path);
        if path.as_os_str().is_empty() || list.iter().any(|recent| recent.path == path) {
            continue;
        }
        list.push(Recent {
            path,
            page: page.trim().parse().unwrap_or(1).max(1),
        });
    }
    list.truncate(LIMIT);
    list
}

pub fn format(list: &[Recent]) -> String {
    list.iter()
        .take(LIMIT)
        .map(|recent| format!("{}\t{}\n", recent.page, recent.path.display()))
        .collect()
}

/// La liste, sans les documents qui n'existent plus (supprimés, clé USB retirée).
pub fn load() -> Vec<Recent> {
    fs::read_to_string(file())
        .map(|text| parse(&text))
        .unwrap_or_default()
        .into_iter()
        .filter(|recent| recent.path.is_file())
        .collect()
}

fn save(list: &[Recent]) {
    let path = file();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Écrit à côté puis renomme : une coupure ne laisse pas un fichier à moitié écrit.
    let temporary = path.with_extension("tmp");
    if fs::write(&temporary, format(list)).is_ok() {
        let _ = fs::rename(&temporary, &path);
    }
}

/// Place `path` en tête de liste, à la page `page`.
pub fn touch(list: &mut Vec<Recent>, path: &Path, page: usize) {
    list.retain(|recent| recent.path != path);
    list.insert(
        0,
        Recent {
            path: path.to_path_buf(),
            page: page.max(1),
        },
    );
    list.truncate(LIMIT);
}

/// Retient la page lue sans changer l'ordre de la liste.
pub fn set_page(list: &mut [Recent], path: &Path, page: usize) -> bool {
    match list.iter_mut().find(|recent| recent.path == path) {
        Some(recent) if recent.page != page => {
            recent.page = page.max(1);
            true
        }
        _ => false,
    }
}

/// Recharge la liste du disque (une autre fenêtre a pu l'écrire), applique `change`, enregistre.
pub fn update(change: impl FnOnce(&mut Vec<Recent>)) -> Vec<Recent> {
    let mut list = fs::read_to_string(file())
        .map(|text| parse(&text))
        .unwrap_or_default();
    change(&mut list);
    save(&list);
    list.retain(|recent| recent.path.is_file());
    list
}

/// Chemin absolu (la liste sert depuis n'importe quel dossier).
pub fn absolute(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_et_ecrit_la_liste() {
        let list = vec![
            Recent {
                path: PathBuf::from("/home/moi/Documents/rapport final.pdf"),
                page: 12,
            },
            Recent {
                path: PathBuf::from("/home/moi/notes.pdf"),
                page: 1,
            },
        ];
        assert_eq!(parse(&format(&list)), list);
    }

    #[test]
    fn ignore_les_lignes_abimees_et_les_doublons() {
        let list = parse("3\t/a.pdf\nsans tabulation\nx\t/b.pdf\n9\t/a.pdf\n0\t\n");
        assert_eq!(
            list,
            vec![
                Recent {
                    path: PathBuf::from("/a.pdf"),
                    page: 3
                },
                Recent {
                    path: PathBuf::from("/b.pdf"),
                    page: 1
                },
            ]
        );
    }

    #[test]
    fn le_dernier_ouvert_passe_en_tete() {
        let mut list = parse("2\t/a.pdf\n5\t/b.pdf\n");
        touch(&mut list, Path::new("/b.pdf"), 5);
        assert_eq!(list[0].path, PathBuf::from("/b.pdf"));
        assert_eq!(list.len(), 2);
        assert!(set_page(&mut list, Path::new("/a.pdf"), 7));
        assert!(!set_page(&mut list, Path::new("/a.pdf"), 7));
        assert_eq!(list[1].page, 7);
        for index in 0..20 {
            touch(&mut list, Path::new(&format!("/{index}.pdf")), 1);
        }
        assert_eq!(list.len(), LIMIT);
    }
}
