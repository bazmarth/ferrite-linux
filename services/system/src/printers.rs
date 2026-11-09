//! Imprimantes, par CUPS : `lpstat` pour l'état, `lpadmin` pour ajouter ou retirer,
//! `lpoptions` pour l'imprimante par défaut de l'utilisateur, `lp` pour la page de test.
//!
//! Ferrite ne gère que les imprimantes « sans pilote » (IPP Everywhere, AirPrint, et les
//! imprimantes USB récentes par `ipp-usb`) : CUPS les trouve seul sur le réseau
//! (`lpstat -e`) et sait leur parler sans pilote (`lpadmin -m everywhere`). Ajouter ou
//! retirer demande les droits d'administration : si `lpadmin` les refuse, il est relancé
//! par `pkexec` (mot de passe demandé par l'agent polkit du bureau).

use std::{path::Path, time::Duration};

use crate::command::{self, Error};

/// Page de test fournie par CUPS.
const TEST_PAGE: &str = "/usr/share/cups/data/testprint";
/// Délai des commandes qui interrogent le réseau.
const SLOW: Duration = Duration::from_secs(20);

/// État d'une imprimante installée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printer {
    /// Nom de la file CUPS.
    pub name: String,
    /// Nom lisible (« Description » de CUPS), sinon le nom de la file.
    pub label: String,
    pub location: String,
    pub state: State,
    pub default: bool,
    /// Documents en attente.
    pub jobs: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Printing,
    Stopped,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Idle => "Prête",
            State::Printing => "Impression en cours",
            State::Stopped => "Arrêtée",
        }
    }
}

/// Ce qu'on sait du service d'impression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// CUPS n'est pas installé.
    Missing,
    /// Installé mais arrêté.
    Stopped,
    /// Imprimantes installées.
    Ready(Vec<Printer>),
}

/// CUPS est-il installé ?
pub fn installed() -> bool {
    Path::new("/usr/bin/lpstat").exists() && Path::new("/usr/sbin/lpadmin").exists()
}

/// Imprimantes installées, avec leur état.
pub fn status() -> Status {
    if !installed() {
        return Status::Missing;
    }
    let listing = match command::run("lpstat", &["-l", "-p"]) {
        Ok(text) => text,
        Err(error) if no_printer(&error) => String::new(),
        Err(error) if error.contains("not running") || error.contains("Unable to connect") => {
            return Status::Stopped;
        }
        Err(_) => return Status::Stopped,
    };
    let default = command::run("lpstat", &["-d"])
        .ok()
        .and_then(|text| parse_default(&text));
    let jobs = command::run("lpstat", &["-o"]).unwrap_or_default();
    let mut printers = parse_printers(&listing);
    for printer in &mut printers {
        printer.default = default.as_deref() == Some(printer.name.as_str());
        printer.jobs = count_jobs(&jobs, &printer.name);
    }
    Status::Ready(printers)
}

/// Imprimantes trouvées sur le réseau ou en USB, pas encore installées.
pub fn discover(installed: &[Printer]) -> Result<Vec<String>, Error> {
    let text = match command::run_with_timeout("lpstat", &["-e"], SLOW) {
        Ok(text) => text,
        Err(error) if no_printer(&error) => String::new(),
        Err(error) => return Err(explain(&error)),
    };
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .filter(|name| !installed.iter().any(|printer| printer.name == *name))
        .map(str::to_owned)
        .collect())
}

/// Installe une imprimante trouvée par [`discover`], sans pilote.
pub fn add(name: &str) -> Result<(), Error> {
    let uri = command::run_with_timeout("lpstat", &["-v", name], SLOW)
        .ok()
        .and_then(|text| parse_uri(&text))
        .ok_or_else(|| format!("Adresse de « {name} » introuvable"))?;
    let queue = queue_name(name);
    admin(&["-p", &queue, "-E", "-v", &uri, "-m", "everywhere", "-D", name])
}

/// Retire une imprimante installée.
pub fn remove(name: &str) -> Result<(), Error> {
    admin(&["-x", name])
}

/// Imprimante par défaut de l'utilisateur (sans droits d'administration).
pub fn set_default(name: &str) -> Result<(), Error> {
    command::run("lpoptions", &["-d", name])
        .map(drop)
        .map_err(|error| explain(&error))
}

/// Envoie la page de test de CUPS.
pub fn print_test_page(name: &str) -> Result<(), Error> {
    command::run("lp", &["-d", name, "-t", "Page de test Ferrite", TEST_PAGE])
        .map(drop)
        .map_err(|error| explain(&error))
}

/// Annule tous les documents en attente de cette imprimante.
pub fn cancel_all(name: &str) -> Result<(), Error> {
    command::run("cancel", &["-a", name])
        .map(drop)
        .map_err(|error| explain(&error))
}

/// `lpadmin`, relancé par `pkexec` s'il manque les droits.
fn admin(args: &[&str]) -> Result<(), Error> {
    match command::run_with_timeout("lpadmin", args, SLOW) {
        Ok(_) => Ok(()),
        Err(error) if forbidden(&error) => {
            let mut full = vec!["/usr/sbin/lpadmin"];
            full.extend_from_slice(args);
            // Le temps de taper le mot de passe.
            command::run_with_timeout("pkexec", &full, Duration::from_secs(120))
                .map(drop)
                .map_err(|error| explain(&error))
        }
        Err(error) => Err(explain(&error)),
    }
}

fn forbidden(error: &str) -> bool {
    ["Forbidden", "not authorized", "Unauthorized", "Permission denied"]
        .iter()
        .any(|word| error.contains(word))
}

fn no_printer(error: &str) -> bool {
    error.contains("No destinations added")
}

/// Message d'erreur en français.
fn explain(error: &str) -> String {
    if error.contains("not running") || error.contains("Unable to connect") {
        "Le service d’impression est arrêté".to_owned()
    } else if error.contains("Request dismissed")
        || error.contains("Not authorized")
        || error.contains("status: 126")
    {
        "Mot de passe non donné : rien n’a changé".to_owned()
    } else if forbidden(error) {
        "Droits d’administration nécessaires".to_owned()
    } else if error.contains("does not exist") || error.contains("Unknown destination") {
        "Imprimante introuvable".to_owned()
    } else {
        error.to_owned()
    }
}

/// Nom de file CUPS : lettres, chiffres, « - » et « _ » seulement.
pub fn queue_name(name: &str) -> String {
    let mut queue: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    while queue.contains("__") {
        queue = queue.replace("__", "_");
    }
    let queue = queue.trim_matches('_').to_owned();
    if queue.is_empty() {
        "Imprimante".to_owned()
    } else {
        queue
    }
}

/// Lit `lpstat -l -p`.
fn parse_printers(text: &str) -> Vec<Printer> {
    let mut printers: Vec<Printer> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("printer ") {
            let Some(name) = rest.split_whitespace().next() else {
                continue;
            };
            let after = &rest[name.len()..];
            let state = if after.contains("disabled") {
                State::Stopped
            } else if after.contains("now printing") {
                State::Printing
            } else {
                State::Idle
            };
            printers.push(Printer {
                name: name.to_owned(),
                label: name.to_owned(),
                location: String::new(),
                state,
                default: false,
                jobs: 0,
            });
        } else if let Some(printer) = printers.last_mut() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("Description:") {
                let value = value.trim();
                if !value.is_empty() {
                    printer.label = value.to_owned();
                }
            } else if let Some(value) = line.strip_prefix("Location:") {
                printer.location = value.trim().to_owned();
            }
        }
    }
    printers
}

/// Lit `lpstat -d`.
fn parse_default(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("system default destination:"))
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
}

/// Lit `lpstat -v NOM` : « device for NOM: URI ».
fn parse_uri(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            line.strip_prefix("device for ")?
                .split_once(": ")
                .map(|(_, uri)| uri)
        })
        .map(|uri| uri.trim().to_owned())
        .filter(|uri| uri.contains("://"))
}

/// Documents de `name` dans `lpstat -o` (« NOM-12 utilisateur taille date »).
fn count_jobs(text: &str, name: &str) -> usize {
    text.lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|job| {
            job.rsplit_once('-')
                .is_some_and(|(queue, id)| queue == name && id.chars().all(|c| c.is_ascii_digit()))
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etat_des_imprimantes() {
        let text = "printer HP_LaserJet is idle.  enabled since Mon 09 Nov 2026 01:00:00 PM CET\n\
                    \tForm mounted:\n\
                    \tDescription: HP LaserJet Pro M15w\n\
                    \tAlerts: none\n\
                    \tLocation: Bureau\n\
                    printer Brother now printing Brother-12.  enabled since Mon 09 Nov 2026\n\
                    \tDescription:\n\
                    printer Epson disabled since Mon 09 Nov 2026 -\n\
                    \treason unknown\n";
        let printers = parse_printers(text);
        assert_eq!(printers.len(), 3);
        assert_eq!(printers[0].label, "HP LaserJet Pro M15w");
        assert_eq!(printers[0].location, "Bureau");
        assert_eq!(printers[0].state, State::Idle);
        assert_eq!(printers[1].label, "Brother");
        assert_eq!(printers[1].state, State::Printing);
        assert_eq!(printers[2].state, State::Stopped);
    }

    #[test]
    fn defaut_adresse_et_documents() {
        assert_eq!(
            parse_default("system default destination: HP_LaserJet\n").as_deref(),
            Some("HP_LaserJet")
        );
        assert_eq!(parse_default("no system default destination\n"), None);
        assert_eq!(
            parse_uri("device for HP: ipp://hp.local:631/ipp/print\n").as_deref(),
            Some("ipp://hp.local:631/ipp/print")
        );
        assert_eq!(parse_uri("device for HP: ///dev/null\n"), None);
        let jobs = "HP-12 bazoum 1024 Mon 09 Nov\nHP-13 bazoum 2048 Mon 09 Nov\nHP_2-4 bazoum 1 Mon\n";
        assert_eq!(count_jobs(jobs, "HP"), 2);
        assert_eq!(count_jobs(jobs, "HP_2"), 1);
    }

    #[test]
    fn nom_de_file() {
        assert_eq!(queue_name("HP LaserJet Pro (Bureau)"), "HP_LaserJet_Pro_Bureau");
        assert_eq!(queue_name("Brother-HL"), "Brother-HL");
        assert_eq!(queue_name("€€"), "Imprimante");
    }

    #[test]
    fn erreurs_en_francais() {
        assert_eq!(
            explain("lpstat : Scheduler is not running."),
            "Le service d’impression est arrêté"
        );
        assert_eq!(
            explain("lpadmin : Forbidden"),
            "Droits d’administration nécessaires"
        );
    }
}
