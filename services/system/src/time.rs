//! Date, heure et fuseau horaire (`timedatectl`).

use crate::command::{self, Error};

/// Fuseau horaire actuel, par exemple « Africa/Ouagadougou ».
pub fn timezone() -> Option<String> {
    if let Ok(output) = command::run("timedatectl", &["show", "--property=Timezone", "--value"]) {
        let zone = output.trim();
        if !zone.is_empty() {
            return Some(zone.to_owned());
        }
    }
    std::fs::read_to_string("/etc/timezone")
        .ok()
        .map(|zone| zone.trim().to_owned())
        .filter(|zone| !zone.is_empty())
}

pub fn timezones() -> Result<Vec<String>, Error> {
    Ok(command::run("timedatectl", &["list-timezones"])?
        .lines()
        .map(str::trim)
        .filter(|zone| !zone.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Change le fuseau ; sans agent polkit, passe par `sudo -n` (utilisateur du système live).
pub fn set_timezone(zone: &str) -> Result<(), Error> {
    match command::run("timedatectl", &["set-timezone", zone]) {
        Ok(_) => Ok(()),
        Err(first) => command::run("sudo", &["-n", "timedatectl", "set-timezone", zone])
            .map(drop)
            .map_err(|_| first),
    }
}

/// Synchronisation automatique de l'heure par le réseau.
pub fn ntp_enabled() -> Option<bool> {
    let output = command::run("timedatectl", &["show", "--property=NTP", "--value"]).ok()?;
    Some(output.trim() == "yes")
}

pub fn set_ntp(enabled: bool) -> Result<(), Error> {
    let value = if enabled { "true" } else { "false" };
    match command::run("timedatectl", &["set-ntp", value]) {
        Ok(_) => Ok(()),
        Err(first) => command::run("sudo", &["-n", "timedatectl", "set-ntp", value])
            .map(drop)
            .map_err(|_| first),
    }
}

/// Heure locale (0 à 23), pour saluer selon le moment de la journée.
pub fn local_hour() -> Option<u32> {
    command::run("date", &["+%H"]).ok()?.trim().parse().ok()
}
