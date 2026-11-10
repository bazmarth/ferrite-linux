//! Service `ferrite-apps verifier-mises-a-jour`, sans fenêtre : lancé par le bureau à
//! l'ouverture de session. Une fois par jour (la première fois dix minutes après le
//! démarrage), il cherche les mises à jour de Debian et de Flathub et, s'il y en a, les
//! annonce par une notification ; un clic ouvre la Logithèque sur « Mises à jour ».
//! Deux minutes après le démarrage, il annonce aussi un pilote graphique manquant
//! (`drivers.rs`), une fois par semaine au plus et jamais sur le système live.

use std::{
    collections::HashMap,
    env,
    fs::{self, File},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    thread,
    time::{Duration, SystemTime},
};

use zbus::{MatchRule, blocking::MessageIterator, message::Type as MessageType};

use super::{drivers, flatpak};
use crate::packagekit;

const BUS_NAME: &str = "org.freedesktop.Notifications";
const BUS_PATH: &str = "/org/freedesktop/Notifications";
/// Une recherche par jour au plus.
const EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// Laisser la session s'installer avant la première recherche.
const FIRST_DELAY: Duration = Duration::from_secs(10 * 60);
/// Annonce d'un pilote manquant : peu après l'ouverture de session…
const DRIVERS_DELAY: Duration = Duration::from_secs(2 * 60);
/// … et une fois par semaine au plus.
const DRIVERS_EVERY: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Pas de la boucle (la machine a pu dormir : on regarde l'heure à chaque tour).
const TICK: Duration = Duration::from_secs(30 * 60);

fn state_dir() -> Option<PathBuf> {
    let base = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))?;
    Some(base.join("ferrite/logitheque"))
}

/// Un seul service par session (verrou tenu tant que le processus vit).
fn single_instance() -> Option<File> {
    let dir = state_dir()?;
    fs::create_dir_all(&dir).ok()?;
    let file = File::create(dir.join("service.verrou")).ok()?;
    file.try_lock().ok()?;
    Some(file)
}

fn stamp() -> Option<PathBuf> {
    state_dir().map(|dir| dir.join("derniere-recherche"))
}

fn drivers_stamp() -> Option<PathBuf> {
    state_dir().map(|dir| dir.join("derniere-annonce-pilotes"))
}

/// Le fichier `path` date-t-il de plus de `every` ?
fn due(path: Option<PathBuf>, every: Duration, now: SystemTime) -> bool {
    let last = path
        .and_then(|path| fs::metadata(path).ok())
        .and_then(|meta| meta.modified().ok());
    is_due(last, every, now)
}

fn is_due(last: Option<SystemTime>, every: Duration, now: SystemTime) -> bool {
    match last {
        None => true,
        // Horloge revenue en arrière : on recherche, par prudence.
        Some(last) => now.duration_since(last).map_or(true, |elapsed| elapsed >= every),
    }
}

/// Texte de la notification.
fn message(debian: usize, security: usize, flatpak: usize) -> Option<(String, String)> {
    let total = debian + flatpak;
    if total == 0 {
        return None;
    }
    let title = if total == 1 {
        "1 mise à jour disponible".to_owned()
    } else {
        format!("{total} mises à jour disponibles")
    };
    let mut parts = Vec::new();
    if debian > 0 {
        parts.push(if security > 0 {
            format!("{debian} pour le système, dont {security} de sécurité")
        } else {
            format!("{debian} pour le système")
        });
    }
    if flatpak > 0 {
        parts.push(if flatpak == 1 {
            "1 application Flathub".to_owned()
        } else {
            format!("{flatpak} applications Flathub")
        });
    }
    Some((
        title,
        format!("{}. Ouvrez la Logithèque pour les installer.", parts.join(", ")),
    ))
}

fn notify(
    connection: &zbus::blocking::Connection,
    icon: &str,
    summary: &str,
    body: &str,
) -> zbus::Result<u32> {
    use zbus::zvariant::Value;
    let actions = ["default", "Voir"];
    let mut hints: HashMap<&str, Value> = HashMap::new();
    hints.insert("desktop-entry", Value::from("ferrite-logitheque"));
    let reply = connection.call_method(
        Some(BUS_NAME),
        BUS_PATH,
        Some(BUS_NAME),
        "Notify",
        &("Logithèque", 0u32, icon, summary, body, &actions[..], hints, 0i32),
    )?;
    reply.body().deserialize::<u32>()
}

/// Ouvre la Logithèque sur une rubrique (`mises-a-jour`, `pilotes`).
fn open_store(page: &str) {
    let program = env::current_exe().unwrap_or_else(|_| PathBuf::from("ferrite-apps"));
    let mut command = std::process::Command::new(program);
    command.args(["logitheque", page]);
    if let Err(error) = ferrite_system::command::spawn_detached(command) {
        tracing::warn!(%error, "Logithèque introuvable");
    }
}

/// Écoute les clics sur nos notifications.
fn listen(connection: zbus::blocking::Connection, last: Arc<AtomicU32>, last_drivers: Arc<AtomicU32>) {
    thread::spawn(move || {
        let Ok(rule) = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface(BUS_NAME)
            .and_then(|rule| rule.member("ActionInvoked"))
            .map(|rule| rule.build())
        else {
            return;
        };
        let Ok(signals) = MessageIterator::for_match_rule(rule, &connection, Some(16)) else {
            return;
        };
        for message in signals.flatten() {
            let Ok((id, _action)) = message.body().deserialize::<(u32, String)>() else {
                continue;
            };
            if id == 0 {
                continue;
            }
            if id == last.load(Ordering::Relaxed) {
                open_store("mises-a-jour");
            } else if id == last_drivers.load(Ordering::Relaxed) {
                open_store("pilotes");
            }
        }
    });
}

/// Boucle du service ; renvoie le code de sortie.
pub fn run_service() -> i32 {
    let Some(_lock) = single_instance() else {
        return 0;
    };
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return 1;
    };
    let last = Arc::new(AtomicU32::new(0));
    let last_drivers = Arc::new(AtomicU32::new(0));
    listen(connection.clone(), Arc::clone(&last), Arc::clone(&last_drivers));
    thread::sleep(DRIVERS_DELAY);
    announce_drivers(&connection, &last_drivers);
    thread::sleep(FIRST_DELAY.saturating_sub(DRIVERS_DELAY));
    loop {
        if due(stamp(), EVERY, SystemTime::now()) {
            // Relire les dépôts (sans mot de passe pour l'utilisateur de la session).
            let debian = packagekit::check(true).unwrap_or_default();
            let security = debian.iter().filter(|package| package.security).count();
            let flatpak = flatpak::updates().unwrap_or_default();
            if let Some(path) = stamp() {
                let _ = fs::write(path, b"");
            }
            if let Some((title, body)) = message(debian.len(), security, flatpak.len()) {
                match notify(&connection, "system-software-install", &title, &body) {
                    Ok(id) => last.store(id, Ordering::Relaxed),
                    Err(error) => tracing::warn!(%error, "notification impossible"),
                }
            }
        }
        thread::sleep(TICK);
    }
}

/// Annonce les pilotes graphiques manquants (pas sur le système live : il faudrait
/// redémarrer, et ce qu'on y installe disparaît au redémarrage).
fn announce_drivers(connection: &zbus::blocking::Connection, last: &AtomicU32) {
    if drivers::is_live() || !due(drivers_stamp(), DRIVERS_EVERY, SystemTime::now()) {
        return;
    }
    let Some((title, body)) = drivers::message(&drivers::missing()) else {
        return;
    };
    match notify(connection, "video-display", &title, &body) {
        Ok(id) => {
            last.store(id, Ordering::Relaxed);
            if let Some(path) = drivers_stamp() {
                let _ = fs::write(path, b"");
            }
        }
        Err(error) => tracing::warn!(%error, "notification impossible"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_fois_par_jour() {
        let now = SystemTime::now();
        assert!(is_due(None, EVERY, now));
        assert!(!is_due(Some(now - Duration::from_secs(3600)), EVERY, now));
        assert!(is_due(Some(now - EVERY), EVERY, now));
        assert!(!is_due(Some(now - EVERY), DRIVERS_EVERY, now));
        assert!(
            is_due(Some(now + Duration::from_secs(60)), EVERY, now),
            "horloge en arrière"
        );
    }

    #[test]
    fn texte_de_la_notification() {
        assert_eq!(message(0, 0, 0), None);
        assert_eq!(
            message(3, 1, 0),
            Some((
                "3 mises à jour disponibles".to_owned(),
                "3 pour le système, dont 1 de sécurité. Ouvrez la Logithèque pour les installer.".to_owned()
            ))
        );
        assert_eq!(
            message(0, 0, 1),
            Some((
                "1 mise à jour disponible".to_owned(),
                "1 application Flathub. Ouvrez la Logithèque pour les installer.".to_owned()
            ))
        );
    }
}
