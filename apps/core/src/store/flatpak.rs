//! Applications Flathub par la commande `flatpak`, installées pour l'utilisateur seul
//! (`--user`) : ni mot de passe, ni droits d'administrateur.
//!
//! Chaque fonction bloque : à appeler depuis un fil à part.

use std::{
    io::Read,
    process::{Command, Stdio},
};

const REMOTE: &str = "flathub";
const REMOTE_URL: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";

fn flatpak() -> Command {
    let mut command = Command::new("flatpak");
    // Messages et nombres stables pour la lecture ; rien n'est montré tel quel.
    command.env("LC_ALL", "C").stdin(Stdio::null());
    command
}

fn output(command: &mut Command) -> Result<String, String> {
    let output = command.output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "Flatpak n’est pas installé.".to_owned()
        } else {
            error.to_string()
        }
    })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(failure(&String::from_utf8_lossy(&output.stderr)))
    }
}

/// Dernière ligne utile des erreurs de flatpak, traduite pour les cas courants.
fn failure(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("Flatpak a échoué.");
    let lower = line.to_lowercase();
    if lower.contains("could not resolve") || lower.contains("unable to connect") || lower.contains("timeout")
    {
        "Pas de connexion à Flathub : vérifiez Internet.".to_owned()
    } else if lower.contains("no space") {
        "Plus assez de place sur le disque.".to_owned()
    } else {
        line.trim_start_matches("error:").trim().to_owned()
    }
}

/// Flathub est-il configuré pour cet utilisateur (ou pour tout le système) ?
pub fn has_flathub() -> bool {
    output(flatpak().args(["remotes", "--columns=name"]))
        .is_ok_and(|text| text.lines().any(|line| line.trim() == REMOTE))
}

/// Ajoute Flathub pour l'utilisateur, puis télécharge son catalogue.
pub fn add_flathub() -> Result<(), String> {
    output(flatpak().args(["remote-add", "--user", "--if-not-exists", REMOTE, REMOTE_URL]))?;
    refresh_catalog()
}

/// Télécharge le catalogue de Flathub (descriptions, icônes) : quelques secondes.
pub fn refresh_catalog() -> Result<(), String> {
    output(flatpak().args(["update", "--appstream", "--user", REMOTE])).map(|_| ())
}

/// Application installée : identifiant et installation (« user » ou « system »).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub system: bool,
}

/// Applications Flatpak installées.
pub fn installed() -> Result<Vec<Installed>, String> {
    let text = output(flatpak().args(["list", "--app", "--columns=application,installation"]))?;
    Ok(parse_installed(&text))
}

fn parse_installed(text: &str) -> Vec<Installed> {
    text.lines()
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let id = columns.next()?.trim();
            (!id.is_empty()).then(|| Installed {
                id: id.to_owned(),
                system: columns
                    .next()
                    .is_some_and(|installation| installation.trim() == "system"),
            })
        })
        .collect()
}

/// Taille à télécharger et taille installée (« 57,9 Mo »), demandées à Flathub.
pub fn sizes(reference: &str) -> Result<(Option<String>, Option<String>), String> {
    let text = output(flatpak().args(["remote-info", "--user", REMOTE, reference]))?;
    Ok(parse_sizes(&text))
}

fn parse_sizes(text: &str) -> (Option<String>, Option<String>) {
    let field = |name: &str| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(name))
            .map(french_size)
    };
    (field("Download:"), field("Installed:"))
}

/// « 57.9 MB » (espace insécable ou non) → « 57,9 Mo ».
fn french_size(value: &str) -> String {
    let value = value.trim().replace(['\u{a0}', '\u{202f}'], " ");
    let (number, unit) = value
        .split_once(|c: char| c.is_whitespace())
        .unwrap_or((value.as_str(), ""));
    let unit = match unit.trim() {
        "kB" | "KB" => "ko",
        "MB" => "Mo",
        "GB" => "Go",
        "bytes" | "B" => "octets",
        other => other,
    };
    format!("{}\u{a0}{unit}", number.replace('.', ","))
}

/// Mises à jour disponibles des applications installées (identifiants).
pub fn updates() -> Result<Vec<String>, String> {
    let text = output(flatpak().args(["remote-ls", "--updates", "--app", "--columns=application"]))?;
    let mut ids: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Lance `flatpak` et suit sa progression (pourcentages qu'il écrit au fil de l'eau).
fn run_with_progress(command: &mut Command, mut progress: impl FnMut(u32)) -> Result<(), String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(stderr) = stderr.as_mut() {
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });
    if let Some(mut stdout) = child.stdout.take() {
        let mut chunk = [0u8; 4096];
        let mut line = String::new();
        while let Ok(read) = stdout.read(&mut chunk) {
            if read == 0 {
                break;
            }
            for character in String::from_utf8_lossy(&chunk[..read]).chars() {
                if character == '\r' || character == '\n' {
                    if let Some(percent) = last_percent(&line) {
                        progress(percent);
                    }
                    line.clear();
                } else {
                    line.push(character);
                }
            }
        }
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    let errors = errors.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        Err(failure(&errors))
    }
}

/// Dernier « 42% » d'une ligne.
fn last_percent(line: &str) -> Option<u32> {
    let end = line.rfind('%')?;
    let digits: String = line[..end]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok().filter(|percent| *percent <= 100)
}

/// Installe une application de Flathub (et son environnement d'exécution s'il manque).
pub fn install(reference: &str, progress: impl FnMut(u32)) -> Result<(), String> {
    run_with_progress(
        flatpak().args(["install", "--user", "--noninteractive", "-y", REMOTE, reference]),
        progress,
    )
}

/// Désinstalle une application (là où elle est installée).
pub fn uninstall(id: &str, system: bool, progress: impl FnMut(u32)) -> Result<(), String> {
    let place = if system { "--system" } else { "--user" };
    run_with_progress(
        flatpak().args(["uninstall", place, "--noninteractive", "-y", id]),
        progress,
    )
}

/// Met à jour les applications Flatpak de l'utilisateur.
pub fn update_all(progress: impl FnMut(u32)) -> Result<(), String> {
    run_with_progress(
        flatpak().args(["update", "--user", "--noninteractive", "-y"]),
        progress,
    )
}

/// Ouvre une application installée.
pub fn launch(id: &str) -> Result<(), String> {
    flatpak()
        .args(["run", id])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liste_des_installees() {
        let list = parse_installed("org.videolan.VLC\tuser\ncom.spotify.Client\tsystem\n\n");
        assert_eq!(
            list,
            vec![
                Installed {
                    id: "org.videolan.VLC".into(),
                    system: false
                },
                Installed {
                    id: "com.spotify.Client".into(),
                    system: true
                },
            ]
        );
    }

    #[test]
    fn tailles_en_francais() {
        let text = "VLC\n\n        ID: org.videolan.VLC\n  Download: 57.9\u{a0}MB\n Installed: 1.2\u{a0}GB\n";
        assert_eq!(
            parse_sizes(text),
            (Some("57,9\u{a0}Mo".into()), Some("1,2\u{a0}Go".into()))
        );
        assert_eq!(parse_sizes("rien"), (None, None));
    }

    #[test]
    fn pourcentage_dans_la_sortie() {
        assert_eq!(last_percent("Installing 1/2… ███▌  45%  3.1 MB/s"), Some(45));
        assert_eq!(last_percent("Installing 2/2… 100%"), Some(100));
        assert_eq!(last_percent("Looking for matches…"), None);
    }

    #[test]
    fn erreurs_lisibles() {
        assert_eq!(
            failure("error: Could not resolve hostname\n"),
            "Pas de connexion à Flathub : vérifiez Internet."
        );
        assert_eq!(
            failure("Warning: x\nerror: Nothing matches foo\n"),
            "Nothing matches foo"
        );
    }
}
