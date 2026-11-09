//! Date, heure et fuseau horaire (`timedatectl`).

use std::{
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

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

/// Décalage de l'heure locale sur l'UTC, en secondes, à l'instant présent (heure d'été
/// comprise). Calculé par la bibliothèque C sans lancer de commande, donc sans bloquer ;
/// relu toutes les [`OFFSET_REFRESH`] : un passage à l'heure d'été ou un changement de
/// fuseau dans Réglages se voit sans redémarrer le bureau.
pub fn local_offset_seconds() -> i64 {
    static CACHE: Mutex<Option<(Instant, i64)>> = Mutex::new(None);
    let mut cached = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((checked, offset)) = *cached
        && checked.elapsed() < OFFSET_REFRESH
    {
        return offset;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as libc::time_t);
    let offset = offset_at(now).unwrap_or(0);
    *cached = Some((Instant::now(), offset));
    offset
}

const OFFSET_REFRESH: Duration = Duration::from_secs(10);

#[allow(unsafe_code)]
unsafe extern "C" {
    /// Relit le fuseau (`TZ`, sinon `/etc/localtime` s'il a changé). Absent de `libc`.
    fn tzset();
}

#[allow(unsafe_code)]
fn offset_at(stamp: libc::time_t) -> Option<i64> {
    // SAFETY : `tm` est une structure C ordinaire, valide une fois remplie de zéros ;
    // `localtime_r` n'écrit que dans `tm`, qui vit jusqu'à la fin de la fonction.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let filled = unsafe {
        tzset();
        !libc::localtime_r(&stamp, &mut tm).is_null()
    };
    filled.then_some(tm.tm_gmtoff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_decalage_local_est_plausible() {
        // Les fuseaux vont de UTC-12 à UTC+14.
        let offset = offset_at(1_700_000_000).expect("heure locale lisible");
        assert!((-12 * 3600..=14 * 3600).contains(&offset));
        assert_eq!(local_offset_seconds() % 60, 0);
    }
}
