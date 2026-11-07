//! Lancement d'outils externes avec délai maximal.

use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// Délai par défaut d'une commande.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// Résultat d'une commande en échec, en français.
pub type Error = String;

/// Lance `program args` et renvoie sa sortie standard si elle réussit avant `timeout`.
/// Passé ce délai, le processus est tué.
pub fn run_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Result<String, Error> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("LC_ALL", "C")
        .spawn()
        .map_err(|err| format!("{program} indisponible : {err}"))?;

    // Les sorties sont lues en parallèle, pour qu'un tube plein ne bloque pas l'outil.
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out = thread::spawn(move || {
        let mut text = String::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    });
    let err = thread::spawn(move || {
        let mut text = String::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{program} ne répond pas (délai de {} s dépassé)",
                    timeout.as_secs()
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(format!("{program} : {error}")),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() {
        Ok(stdout)
    } else {
        let detail = stderr
            .lines()
            .chain(stdout.lines())
            .find(|line| !line.trim().is_empty());
        Err(match detail {
            Some(line) => format!("{program} : {}", line.trim()),
            None => format!("{program} a échoué ({status})"),
        })
    }
}

/// [`run_with_timeout`] avec le délai par défaut.
pub fn run(program: &str, args: &[&str]) -> Result<String, Error> {
    run_with_timeout(program, args, TIMEOUT)
}

/// Lance un programme graphique sans l'attendre ; un fil récupère sa fin pour ne pas
/// laisser de processus zombie.
pub fn spawn_detached(mut command: Command) -> std::io::Result<u32> {
    let mut child = command.stdin(Stdio::null()).spawn()?;
    let pid = child.id();
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_commande_bloquee_est_arretee() {
        let start = Instant::now();
        let result = run_with_timeout("sleep", &["10"], Duration::from_millis(300));
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn sortie_et_erreurs() {
        assert_eq!(run("echo", &["bonjour"]).unwrap(), "bonjour\n");
        assert!(run("false", &[]).is_err());
        assert!(
            run("ferrite-programme-inexistant", &[])
                .unwrap_err()
                .contains("indisponible")
        );
    }
}

/// Chemin du programme en cours, pour relancer le même exécutable (TextEdit depuis
/// Fichiers, par exemple). Si le fichier a été remplacé pendant que le programme tourne
/// (mise à jour), Linux renvoie « …/ferrite-apps (deleted) » : on reprend alors le chemin
/// d'origine, où se trouve la nouvelle version.
pub fn current_exe() -> std::io::Result<std::path::PathBuf> {
    let path = std::env::current_exe()?;
    Ok(without_deleted(path))
}

fn without_deleted(path: std::path::PathBuf) -> std::path::PathBuf {
    match path.to_str().and_then(|text| text.strip_suffix(" (deleted)")) {
        Some(original) if !path.exists() => std::path::PathBuf::from(original),
        _ => path,
    }
}

#[cfg(test)]
mod exe_tests {
    use super::*;

    #[test]
    fn executable_remplace() {
        let gone = std::path::PathBuf::from("/nulle-part/ferrite-apps (deleted)");
        assert_eq!(
            without_deleted(gone),
            std::path::PathBuf::from("/nulle-part/ferrite-apps")
        );
        let normal = std::path::PathBuf::from("/usr/bin/ferrite-apps");
        assert_eq!(without_deleted(normal.clone()), normal);
    }
}
