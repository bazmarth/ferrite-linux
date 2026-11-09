//! Glisser-déposer de fichiers entre les applications Ferrite et ferrite-shell.
//!
//! Les applications egui ne savent pas démarrer un glisser-déposer Wayland. Au début d'un
//! glisser, Fichiers annonce donc les fichiers à ferrite-shell sur une prise Unix ; dès que
//! le pointeur quitte sa fenêtre ou passe sur le Dock, ferrite-shell reprend le glisser
//! comme un vrai glisser-déposer Wayland (les autres applications reçoivent les fichiers
//! en `text/uri-list`) et sait aussi les déposer sur le Dock ou une fenêtre Ferrite.

use std::{
    env,
    io::Write,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
};

/// Fichiers glissés et nom de l'icône à montrer sous le pointeur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drag {
    pub icon: String,
    pub paths: Vec<PathBuf>,
}

const HEADER: &str = "glisser";

/// Prise de ferrite-shell pour l'affichage Wayland `display` (« wayland-1 »…).
pub fn socket_path(display: &str) -> PathBuf {
    let dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    dir.join(format!("ferrite-shell-{display}.sock"))
}

impl Drag {
    pub fn encode(&self) -> String {
        let mut text = format!("{HEADER}\n{}\n", self.icon);
        for path in &self.paths {
            text.push_str(&path.to_string_lossy());
            text.push('\n');
        }
        text
    }

    pub fn decode(text: &str) -> Option<Drag> {
        let mut lines = text.lines();
        if lines.next()? != HEADER {
            return None;
        }
        let icon = lines.next()?.to_owned();
        let paths: Vec<PathBuf> = lines.filter(|line| !line.is_empty()).map(PathBuf::from).collect();
        (!paths.is_empty()).then_some(Drag { icon, paths })
    }

    /// Annonce le glisser à ferrite-shell ; faux s'il n'écoute pas (autre compositeur).
    pub fn announce(&self) -> bool {
        let Some(display) = env::var_os("WAYLAND_DISPLAY") else {
            return false;
        };
        UnixStream::connect(socket_path(&display.to_string_lossy()))
            .and_then(|mut stream| stream.write_all(self.encode().as_bytes()))
            .is_ok()
    }
}

/// Fichiers lâchés par ferrite-shell sur la fenêtre d'une application Ferrite qui sait les
/// ranger (Fichiers) : position dans la fenêtre, en points, et copie (Ctrl ou Alt tenu) ou
/// déplacement.
#[derive(Debug, Clone, PartialEq)]
pub struct Deposit {
    pub copy: bool,
    pub x: f32,
    pub y: f32,
    pub paths: Vec<PathBuf>,
}

const DEPOSIT_HEADER: &str = "deposer";

/// Prise où le processus `pid` reçoit les dépôts.
pub fn deposit_socket_path(pid: u32) -> PathBuf {
    let dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    dir.join(format!("ferrite-depot-{pid}.sock"))
}

impl Deposit {
    pub fn encode(&self) -> String {
        let operation = if self.copy { "copier" } else { "deplacer" };
        let mut text = format!("{DEPOSIT_HEADER}\n{operation}\n{} {}\n", self.x, self.y);
        for path in &self.paths {
            text.push_str(&path.to_string_lossy());
            text.push('\n');
        }
        text
    }

    pub fn decode(text: &str) -> Option<Deposit> {
        let mut lines = text.lines();
        if lines.next()? != DEPOSIT_HEADER {
            return None;
        }
        let copy = match lines.next()? {
            "copier" => true,
            "deplacer" => false,
            _ => return None,
        };
        let (x, y) = lines.next()?.split_once(' ')?;
        let (x, y) = (x.parse().ok()?, y.parse().ok()?);
        let paths: Vec<PathBuf> = lines.filter(|line| !line.is_empty()).map(PathBuf::from).collect();
        (!paths.is_empty()).then_some(Deposit { copy, x, y, paths })
    }

    /// Confie le dépôt au processus `pid` ; faux s'il n'en reçoit pas.
    pub fn send(&self, pid: u32) -> bool {
        UnixStream::connect(deposit_socket_path(pid))
            .and_then(|mut stream| stream.write_all(self.encode().as_bytes()))
            .is_ok()
    }
}

/// Survol d'une fenêtre pendant un glisser repris par ferrite-shell : la fenêtre ne reçoit
/// pas le pointeur, ferrite-shell lui dit où il est (pour surligner le dossier visé et
/// l'ouvrir s'il s'y attarde), puis qu'il est parti.
#[derive(Debug, Clone, PartialEq)]
pub enum Visit {
    /// Pointeur en (`x`, `y`) dans la fenêtre, en points, avec les fichiers glissés.
    Over {
        x: f32,
        y: f32,
        paths: Vec<PathBuf>,
    },
    Left,
}

const VISIT_HEADER: &str = "survoler";
const LEAVE_HEADER: &str = "quitter";

impl Visit {
    pub fn encode(&self) -> String {
        match self {
            Visit::Over { x, y, paths } => {
                let mut text = format!("{VISIT_HEADER}\n{x} {y}\n");
                for path in paths {
                    text.push_str(&path.to_string_lossy());
                    text.push('\n');
                }
                text
            }
            Visit::Left => format!("{LEAVE_HEADER}\n"),
        }
    }

    pub fn decode(text: &str) -> Option<Visit> {
        let mut lines = text.lines();
        match lines.next()? {
            LEAVE_HEADER => Some(Visit::Left),
            VISIT_HEADER => {
                let (x, y) = lines.next()?.split_once(' ')?;
                let (x, y) = (x.parse().ok()?, y.parse().ok()?);
                let paths = lines.filter(|line| !line.is_empty()).map(PathBuf::from).collect();
                Some(Visit::Over { x, y, paths })
            }
            _ => None,
        }
    }

    /// Prévient le processus `pid` (sur la même prise que les dépôts) ; faux s'il n'écoute pas.
    pub fn send(&self, pid: u32) -> bool {
        UnixStream::connect(deposit_socket_path(pid))
            .and_then(|mut stream| stream.write_all(self.encode().as_bytes()))
            .is_ok()
    }
}

/// `text/uri-list` des fichiers, comme l'attendent les applications GTK, Qt et Firefox.
pub fn uri_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| format!("{}\r\n", file_uri(path)))
        .collect()
}

/// Adresse `file://` d'un chemin.
pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour() {
        let drag = Drag {
            icon: "file-pdf".into(),
            paths: vec![PathBuf::from("/tmp/a b.pdf"), PathBuf::from("/tmp/c")],
        };
        assert_eq!(Drag::decode(&drag.encode()), Some(drag));
        assert_eq!(Drag::decode("autre\nx\n/tmp"), None);
        assert_eq!(Drag::decode("glisser\nicone\n"), None);
    }

    #[test]
    fn depot_aller_retour() {
        let deposit = Deposit {
            copy: true,
            x: 120.5,
            y: 48.0,
            paths: vec![PathBuf::from("/tmp/a b.txt")],
        };
        assert_eq!(Deposit::decode(&deposit.encode()), Some(deposit));
        assert_eq!(Deposit::decode("deposer\nautre\n1 2\n/tmp/a"), None);
        assert_eq!(Deposit::decode("glisser\nicone\n/tmp/a"), None);
    }

    #[test]
    fn survol_aller_retour() {
        let over = Visit::Over {
            x: 10.5,
            y: 300.0,
            paths: vec![PathBuf::from("/tmp/un dossier")],
        };
        assert_eq!(Visit::decode(&over.encode()), Some(over));
        assert_eq!(Visit::decode(&Visit::Left.encode()), Some(Visit::Left));
        assert_eq!(Visit::decode("survoler\nx\n/tmp"), None);
        // Les dépôts ne se confondent pas avec les survols, ni l'inverse.
        assert_eq!(Visit::decode("deposer\ncopier\n1 2\n/tmp/a"), None);
        assert_eq!(Deposit::decode(&Visit::Left.encode()), None);
    }

    #[test]
    fn liste_d_adresses() {
        assert_eq!(
            uri_list(&[PathBuf::from("/tmp/un café.txt")]),
            "file:///tmp/un%20caf%C3%A9.txt\r\n"
        );
    }
}
