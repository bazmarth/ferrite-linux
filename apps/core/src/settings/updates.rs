//! Mises à jour du système par PackageKit (`org.freedesktop.PackageKit`, bus système).
//!
//! Chaque opération est une « transaction » propre à la connexion qui l'a créée : on la
//! crée, on s'abonne à ses signaux, on lance la méthode, puis on lit les signaux jusqu'à
//! `Finished`. Installer demande le mot de passe administrateur (agent polkit du bureau) ;
//! la liste des mises à jour, elle, se lit sans droits.

use std::collections::HashMap;

use zbus::{
    MatchRule,
    blocking::{Connection, MessageIterator},
    message::Type as MessageType,
    zvariant::{OwnedObjectPath, OwnedValue},
};

const SERVICE: &str = "org.freedesktop.PackageKit";
const PATH: &str = "/org/freedesktop/PackageKit";
const TRANSACTION: &str = "org.freedesktop.PackageKit.Transaction";

/// Filtre « aucun » (`1 << PK_FILTER_ENUM_NONE`).
const FILTER_NONE: u64 = 1 << 1;
/// Paquets signés seulement (`1 << PK_TRANSACTION_FLAG_ENUM_ONLY_TRUSTED`).
const ONLY_TRUSTED: u64 = 1 << 1;
/// `PK_INFO_ENUM_SECURITY`.
const INFO_SECURITY: u32 = 8;
/// `PK_EXIT_ENUM_SUCCESS`.
const EXIT_SUCCESS: u32 = 1;
/// Pourcentage inconnu.
const PERCENT_UNKNOWN: u32 = 101;

/// Paquet à mettre à jour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Identifiant PackageKit : « nom;version;architecture;dépôt ».
    pub id: String,
    pub name: String,
    pub version: String,
    pub summary: String,
    /// Correctif de sécurité.
    pub security: bool,
}

impl Package {
    fn from_signal(info: u32, id: String, summary: String) -> Self {
        let mut parts = id.split(';');
        let name = parts.next().unwrap_or_default().to_owned();
        let version = parts.next().unwrap_or_default().to_owned();
        Self {
            name,
            version,
            summary,
            security: info == INFO_SECURITY,
            id,
        }
    }
}

/// Mises à jour disponibles. `refresh` : relit d'abord les dépôts (réseau, peut prendre
/// une minute) ; sinon, la liste connue depuis la dernière recherche.
pub fn check(refresh: bool) -> Result<Vec<Package>, String> {
    let connection = Connection::system().map_err(|error| unavailable(&error))?;
    if refresh {
        run(
            &connection,
            "RefreshCache",
            |connection, path, method| {
                connection.call_method(Some(SERVICE), path, Some(TRANSACTION), method, &(false,))
            },
            |_| {},
        )?;
    }
    let mut packages = Vec::new();
    let call = |connection: &Connection, path: &OwnedObjectPath, method: &str| {
        connection.call_method(Some(SERVICE), path, Some(TRANSACTION), method, &(FILTER_NONE,))
    };
    run(&connection, "GetUpdates", call, |event| {
        if let Event::Package(package) = event {
            packages.push(package);
        }
    })?;
    packages.sort_by(|a, b| b.security.cmp(&a.security).then_with(|| a.name.cmp(&b.name)));
    packages.dedup_by(|a, b| a.id == b.id);
    Ok(packages)
}

/// Installe les paquets `ids` ; `progress` reçoit le pourcentage quand il est connu.
pub fn install(ids: &[String], mut progress: impl FnMut(u32)) -> Result<(), String> {
    let connection = Connection::system().map_err(|error| unavailable(&error))?;
    let call = |connection: &Connection, path: &OwnedObjectPath, method: &str| {
        connection.call_method(
            Some(SERVICE),
            path,
            Some(TRANSACTION),
            method,
            &(ONLY_TRUSTED, ids),
        )
    };
    run(&connection, "UpdatePackages", call, |event| {
        if let Event::Percent(percent) = event {
            progress(percent);
        }
    })
}

enum Event {
    Package(Package),
    Percent(u32),
}

/// Crée une transaction, lance `method` et lit ses signaux jusqu'à la fin.
fn run(
    connection: &Connection,
    method: &str,
    call: impl FnOnce(&Connection, &OwnedObjectPath, &str) -> zbus::Result<zbus::Message>,
    mut on_event: impl FnMut(Event),
) -> Result<(), String> {
    let path: OwnedObjectPath = connection
        .call_method(Some(SERVICE), PATH, Some(SERVICE), "CreateTransaction", &())
        .and_then(|reply| reply.body().deserialize())
        .map_err(|error| unavailable(&error))?;
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(SERVICE)
        .and_then(|rule| rule.path(path.as_ref()))
        .map_err(|error| error.to_string())?
        .build();
    let signals =
        MessageIterator::for_match_rule(rule, connection, Some(64)).map_err(|error| error.to_string())?;
    // « interactive » : polkit peut demander le mot de passe ; messages en français.
    let hints = ["interactive=true", "locale=fr_FR.UTF-8"];
    connection
        .call_method(
            Some(SERVICE),
            &path,
            Some(TRANSACTION),
            "SetHints",
            &(&hints[..],),
        )
        .map_err(|error| error.to_string())?;
    call(connection, &path, method).map_err(|error| refused(&error))?;
    let mut failure: Option<String> = None;
    for message in signals {
        let message = message.map_err(|error| error.to_string())?;
        let header = message.header();
        let Some(member) = header.member() else {
            continue;
        };
        let body = message.body();
        match member.as_str() {
            "Package" => {
                if let Ok((info, id, summary)) = body.deserialize::<(u32, String, String)>() {
                    on_event(Event::Package(Package::from_signal(info, id, summary)));
                }
            }
            "PropertiesChanged" => {
                if let Ok((_, changed, _)) =
                    body.deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                    && let Some(percent) = changed
                        .get("Percentage")
                        .and_then(|value| u32::try_from(value).ok())
                    && percent < PERCENT_UNKNOWN
                {
                    on_event(Event::Percent(percent));
                }
            }
            "ErrorCode" => {
                if let Ok((_, details)) = body.deserialize::<(u32, String)>() {
                    failure = Some(details);
                }
            }
            "Finished" => {
                let exit = body.deserialize::<(u32, u32)>().map_or(0, |(exit, _)| exit);
                return if exit == EXIT_SUCCESS {
                    Ok(())
                } else {
                    Err(failure.unwrap_or_else(|| "L’opération n’a pas abouti.".to_owned()))
                };
            }
            "Destroy" => break,
            _ => {}
        }
    }
    Err(failure.unwrap_or_else(|| "PackageKit s’est arrêté en cours d’opération.".to_owned()))
}

fn unavailable(error: &zbus::Error) -> String {
    tracing::warn!(%error, "PackageKit injoignable");
    "Le service de mises à jour (PackageKit) ne répond pas.".to_owned()
}

/// Refus de polkit (mot de passe annulé ou faux) ou autre erreur de la méthode.
fn refused(error: &zbus::Error) -> String {
    let text = error.to_string();
    if text.contains("RefusedByPolicy") || text.contains("NotAuthorized") {
        "Autorisation refusée : le mot de passe administrateur est nécessaire.".to_owned()
    } else {
        text
    }
}

/// « 3 mises à jour disponibles, dont 1 de sécurité » ; « Le système est à jour ».
pub fn label(list: &[Package]) -> String {
    let security = list.iter().filter(|package| package.security).count();
    let mut label = match list.len() {
        0 => return "Le système est à jour".to_owned(),
        1 => "1 mise à jour disponible".to_owned(),
        count => format!("{count} mises à jour disponibles"),
    };
    if security > 0 {
        label.push_str(&format!(", dont {security} de sécurité"));
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paquet_lu_depuis_son_identifiant() {
        let package = Package::from_signal(
            INFO_SECURITY,
            "openssl;3.5.1-1+deb13u2;amd64;debian-security".into(),
            "Outils SSL".into(),
        );
        assert_eq!(package.name, "openssl");
        assert_eq!(package.version, "3.5.1-1+deb13u2");
        assert!(package.security);
        assert!(!Package::from_signal(5, "a;1;all;x".into(), String::new()).security);
    }

    #[test]
    fn resume_de_la_liste() {
        let package = |name: &str, security| Package {
            id: format!("{name};1;amd64;debian"),
            name: name.to_owned(),
            version: "1".to_owned(),
            summary: String::new(),
            security,
        };
        assert_eq!(label(&[]), "Le système est à jour");
        assert_eq!(label(&[package("a", false)]), "1 mise à jour disponible");
        assert_eq!(
            label(&[package("a", true), package("b", false), package("c", false)]),
            "3 mises à jour disponibles, dont 1 de sécurité"
        );
    }

    /// Interroge le vrai PackageKit (lecture seule) : `cargo test -- --ignored pk_`.
    #[test]
    #[ignore = "demande PackageKit sur le bus système"]
    fn pk_liste_des_mises_a_jour() {
        let refresh = std::env::var_os("PK_RELIRE").is_some();
        let packages = check(refresh).expect("PackageKit");
        eprintln!("{} mises à jour", packages.len());
        for package in packages.iter().take(10) {
            eprintln!(
                "{} {} sécurité={} — {}",
                package.name, package.version, package.security, package.summary
            );
        }
    }
}
