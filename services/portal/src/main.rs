//! `xdg-desktop-portal-ferrite` : fournisseur du portail des sélecteurs de fichiers.
//!
//! Une application (Brave, Firefox, VS Code, une application Flatpak…) demande au portail
//! `org.freedesktop.portal.FileChooser` où ouvrir ou enregistrer ; `xdg-desktop-portal`
//! confie la demande à ce service (`ferrite-portals.conf`), qui ouvre le sélecteur de
//! Ferrite (`ferrite-apps choisir`) et rend les fichiers choisis. Les autres portails
//! (liens, impression, réglages) restent ceux de GTK.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use zbus::{
    interface,
    zvariant::{OwnedValue, Value},
};

const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.ferrite";
const PATH: &str = "/org/freedesktop/portal/desktop";

/// Réponses du portail.
const SUCCESS: u32 = 0;
const CANCELLED: u32 = 1;
const FAILED: u32 = 2;

type Options = HashMap<String, OwnedValue>;
type Results = HashMap<String, OwnedValue>;
/// Filtre du portail : nom et motifs (0 : motif `*.pdf`, 1 : type MIME).
type PortalFilter = (String, Vec<(u32, String)>);

fn main() -> zbus::Result<()> {
    let _connection = zbus::blocking::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(PATH, FileChooser)?
        .build()?;
    // Les demandes sont servies par le fil de zbus ; ce fil-ci n'a plus qu'à attendre.
    loop {
        std::thread::park();
    }
}

struct FileChooser;

#[interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooser {
    #[zbus(name = "OpenFile")]
    async fn open_file(
        &self,
        _handle: zbus::zvariant::ObjectPath<'_>,
        _app_id: String,
        _parent_window: String,
        title: String,
        options: Options,
    ) -> (u32, Results) {
        let mut args = vec![];
        if flag(&options, "directory") {
            args.push("--dossier".to_owned());
        }
        if flag(&options, "multiple") {
            args.push("--multiple".to_owned());
        }
        common_args(&mut args, &title, &options);
        respond(args).await
    }

    #[zbus(name = "SaveFile")]
    async fn save_file(
        &self,
        _handle: zbus::zvariant::ObjectPath<'_>,
        _app_id: String,
        _parent_window: String,
        title: String,
        options: Options,
    ) -> (u32, Results) {
        let mut args = vec!["--enregistrer".to_owned()];
        if let Some(name) = text(&options, "current_name") {
            args.extend(["--nom".to_owned(), name]);
        }
        if let Some(file) = bytes_path(&options, "current_file") {
            if let Some(name) = file.file_name() {
                args.extend(["--nom".to_owned(), name.to_string_lossy().into_owned()]);
            }
            if let Some(parent) = file.parent() {
                args.extend(["--dans".to_owned(), parent.to_string_lossy().into_owned()]);
            }
        }
        common_args(&mut args, &title, &options);
        respond(args).await
    }

    /// Plusieurs fichiers d'un coup : on choisit le dossier, les noms viennent de la demande.
    #[zbus(name = "SaveFiles")]
    async fn save_files(
        &self,
        _handle: zbus::zvariant::ObjectPath<'_>,
        _app_id: String,
        _parent_window: String,
        title: String,
        options: Options,
    ) -> (u32, Results) {
        let mut args = vec!["--dossier".to_owned()];
        common_args(&mut args, &title, &options);
        let names: Vec<PathBuf> = options
            .get("files")
            .and_then(|value| Vec::<Vec<u8>>::try_from(value.try_clone().ok()?).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|bytes| path_from_bytes(&bytes))
            .collect();
        let (code, chosen) = choose(args).await;
        let Some(folder) = chosen.first().filter(|_| code == SUCCESS) else {
            return (code, Results::new());
        };
        let files: Vec<PathBuf> = names
            .iter()
            .filter_map(|name| name.file_name().map(|name| folder.join(name)))
            .collect();
        (SUCCESS, uris(&files))
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        4
    }
}

/// Titre, bouton, dossier de départ et filtres, communs à toutes les demandes.
fn common_args(args: &mut Vec<String>, title: &str, options: &Options) {
    if !title.is_empty() {
        args.extend(["--titre".to_owned(), title.to_owned()]);
    }
    if let Some(label) = text(options, "accept_label") {
        args.extend(["--bouton".to_owned(), label]);
    }
    if let Some(folder) = bytes_path(options, "current_folder") {
        args.extend(["--dans".to_owned(), folder.to_string_lossy().into_owned()]);
    }
    let filters: Vec<PortalFilter> = options
        .get("filters")
        .and_then(|value| Vec::<PortalFilter>::try_from(value.try_clone().ok()?).ok())
        .unwrap_or_default();
    let current: Option<PortalFilter> = options
        .get("current_filter")
        .and_then(|value| PortalFilter::try_from(value.try_clone().ok()?).ok());
    let globs = mime_globs();
    let mut chosen = 0;
    for (index, (name, rules)) in filters.iter().enumerate() {
        if current.as_ref().is_some_and(|(current, _)| current == name) {
            chosen = index;
        }
        args.extend(["--filtre".to_owned(), filter_arg(name, rules, &globs)]);
    }
    if filters.is_empty()
        && let Some((name, rules)) = &current
    {
        args.extend(["--filtre".to_owned(), filter_arg(name, rules, &globs)]);
    }
    args.extend(["--filtre-actuel".to_owned(), chosen.to_string()]);
}

/// Filtre pour `ferrite-apps choisir` : « Nom|*.pdf;*.png ». Les types MIME deviennent les
/// motifs de la base MIME du système.
fn filter_arg(name: &str, rules: &[(u32, String)], globs: &[(String, String)]) -> String {
    let mut patterns: Vec<String> = Vec::new();
    for (kind, rule) in rules {
        if *kind == 0 {
            patterns.push(rule.clone());
            continue;
        }
        let (family, _) = rule.split_once('/').unwrap_or((rule, ""));
        for (mime, glob) in globs {
            let matches = mime == rule || (rule.ends_with("/*") && mime.starts_with(&format!("{family}/")));
            if matches && !patterns.contains(glob) {
                patterns.push(glob.clone());
            }
        }
    }
    if patterns.is_empty() {
        patterns.push("*".to_owned());
    }
    format!("{name}|{}", patterns.join(";"))
}

/// Base MIME du système : (type, motif) pour chaque ligne de `/usr/share/mime/globs`.
fn mime_globs() -> Vec<(String, String)> {
    std::fs::read_to_string("/usr/share/mime/globs")
        .map(|text| parse_globs(&text))
        .unwrap_or_default()
}

fn parse_globs(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once(':'))
        .map(|(mime, glob)| (mime.to_owned(), glob.to_owned()))
        .collect()
}

/// Ouvre le sélecteur et met le choix au format du portail.
async fn respond(args: Vec<String>) -> (u32, Results) {
    let (code, chosen) = choose(args).await;
    if code != SUCCESS {
        return (code, Results::new());
    }
    (SUCCESS, uris(&chosen))
}

/// `ferrite-apps choisir …` dans un fil à part : la fenêtre peut rester ouverte longtemps
/// sans bloquer les autres demandes.
async fn choose(args: Vec<String>) -> (u32, Vec<PathBuf>) {
    blocking::unblock(move || {
        let output = Command::new(program())
            .arg("choisir")
            .args(&args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        let Ok(output) = output else {
            return (FAILED, Vec::new());
        };
        let chosen: Vec<PathBuf> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| line.starts_with('/'))
            .map(PathBuf::from)
            .collect();
        if chosen.is_empty() {
            (CANCELLED, chosen)
        } else {
            (SUCCESS, chosen)
        }
    })
    .await
}

/// `ferrite-apps` : à côté de ce programme (installation de développement ou image),
/// sinon dans le `PATH`.
fn program() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("ferrite-apps"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("ferrite-apps"))
}

fn flag(options: &Options, key: &str) -> bool {
    options
        .get(key)
        .and_then(|value| value.downcast_ref::<bool>().ok())
        .unwrap_or(false)
}

fn text(options: &Options, key: &str) -> Option<String> {
    options
        .get(key)
        .and_then(|value| value.downcast_ref::<&str>().ok().map(str::to_owned))
        .filter(|text| !text.is_empty())
}

/// Chemin donné en octets terminés par un zéro (`ay`).
fn bytes_path(options: &Options, key: &str) -> Option<PathBuf> {
    let bytes = Vec::<u8>::try_from(options.get(key)?.try_clone().ok()?).ok()?;
    let path = path_from_bytes(&bytes);
    (!path.as_os_str().is_empty()).then_some(path)
}

fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

/// Résultat du portail : `uris`, des adresses `file://` encodées.
fn uris(paths: &[PathBuf]) -> Results {
    let uris: Vec<String> = paths.iter().map(|path| file_uri(path)).collect();
    let mut results = Results::new();
    if let Ok(value) = OwnedValue::try_from(Value::from(uris)) {
        results.insert("uris".to_owned(), value);
    }
    results
}

fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
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
    fn adresse_d_un_fichier() {
        assert_eq!(
            file_uri(Path::new("/home/a/The Open Studio Handbook.pdf")),
            "file:///home/a/The%20Open%20Studio%20Handbook.pdf"
        );
        assert_eq!(
            file_uri(Path::new("/home/a/Vidéos")),
            "file:///home/a/Vid%C3%A9os"
        );
    }

    #[test]
    fn filtres_par_motif_et_par_type() {
        let globs = parse_globs("# commentaire\nimage/png:*.png\nimage/jpeg:*.jpg\napplication/pdf:*.pdf\n");
        assert_eq!(filter_arg("PDF", &[(0, "*.pdf".into())], &globs), "PDF|*.pdf");
        assert_eq!(
            filter_arg("Images", &[(1, "image/*".into())], &globs),
            "Images|*.png;*.jpg"
        );
        assert_eq!(filter_arg("Rien", &[(1, "x/y".into())], &globs), "Rien|*");
    }

    #[test]
    fn chemin_en_octets() {
        assert_eq!(path_from_bytes(b"/tmp/a\0"), PathBuf::from("/tmp/a"));
    }
}
