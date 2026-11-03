//! Bluetooth, par BlueZ (`bluetoothctl`).
//!
//! Sans adaptateur, `bluetoothctl` attend indéfiniment le démon : on vérifie donc d'abord
//! la présence d'un adaptateur dans `/sys/class/bluetooth`, et chaque appel a un délai.

use std::{fs, time::Duration};

use crate::command::{self, Error};

/// Un adaptateur Bluetooth est-il présent ?
pub fn adapter_present() -> bool {
    fs::read_dir("/sys/class/bluetooth")
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with("hci"))
        })
        .unwrap_or(false)
}

/// Adaptateur allumé ? `None` sans adaptateur ou sans démon BlueZ.
pub fn powered() -> Option<bool> {
    if !adapter_present() {
        return None;
    }
    let output = command::run_with_timeout("bluetoothctl", &["show"], Duration::from_secs(3)).ok()?;
    parse_powered(&output)
}

pub fn parse_powered(show: &str) -> Option<bool> {
    show.lines()
        .find_map(|line| line.trim().strip_prefix("Powered:"))
        .map(|value| value.trim() == "yes")
}

pub fn set_powered(enabled: bool) -> Result<(), Error> {
    if !adapter_present() {
        return Err("Aucun adaptateur Bluetooth".into());
    }
    if enabled {
        // Un blocage logiciel (mode avion) empêche l'allumage.
        let _ = command::run("rfkill", &["unblock", "bluetooth"]);
    }
    let output = command::run("bluetoothctl", &["power", if enabled { "on" } else { "off" }])?;
    if output.contains("Failed") {
        return Err(output.lines().last().unwrap_or("Échec").trim().to_owned());
    }
    Ok(())
}

/// Appareil connu ou découvert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub address: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    /// Genre annoncé (« audio-headset », « input-mouse »…).
    pub icon: String,
    pub battery: Option<u8>,
}

/// Nom sous lequel l'ordinateur est visible.
pub fn adapter_name() -> Option<String> {
    let show = command::run("bluetoothctl", &["show"]).ok()?;
    parse_field(&show, "Alias").map(str::to_owned)
}

/// Valeur d'une ligne « Nom: valeur » de `bluetoothctl show` ou `info`.
fn parse_field<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(name)?.strip_prefix(':').map(str::trim))
}

/// Pourcentage de « Battery Percentage: 0x50 (80) ».
pub fn parse_battery(info: &str) -> Option<u8> {
    let value = parse_field(info, "Battery Percentage")?;
    value.split_once('(')?.1.trim_end_matches(')').parse().ok()
}

/// Appareils connus, avec leur état d'appairage et de connexion.
pub fn devices() -> Result<Vec<Device>, Error> {
    if !adapter_present() {
        return Ok(Vec::new());
    }
    let list = command::run("bluetoothctl", &["devices"])?;
    let paired = command::run("bluetoothctl", &["devices", "Paired"]).unwrap_or_default();
    let connected = command::run("bluetoothctl", &["devices", "Connected"]).unwrap_or_default();
    let addresses = |text: &str| {
        parse_devices(text)
            .into_iter()
            .map(|(address, _)| address)
            .collect::<Vec<_>>()
    };
    let (paired, connected) = (addresses(&paired), addresses(&connected));
    let mut devices: Vec<Device> = parse_devices(&list)
        .into_iter()
        .map(|(address, name)| {
            let info = command::run("bluetoothctl", &["info", &address]).unwrap_or_default();
            Device {
                paired: paired.contains(&address),
                connected: connected.contains(&address),
                icon: parse_field(&info, "Icon").unwrap_or_default().to_owned(),
                battery: parse_battery(&info),
                address,
                name,
            }
        })
        .collect();
    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.paired.cmp(&a.paired))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(devices)
}

/// Analyse les lignes « Device AA:BB:CC:DD:EE:FF Nom ».
pub fn parse_devices(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("Device ")?;
            let (address, name) = rest.split_once(' ').unwrap_or((rest, rest));
            Some((address.to_owned(), name.trim().to_owned()))
        })
        .collect()
}

/// Recherche des appareils proches pendant quelques secondes.
pub fn scan() -> Result<(), Error> {
    command::run_with_timeout(
        "bluetoothctl",
        &["--timeout", "8", "scan", "on"],
        Duration::from_secs(12),
    )
    .map(drop)
}

/// Appaire (si besoin), fait confiance puis connecte un appareil.
pub fn connect(address: &str) -> Result<(), Error> {
    let long = Duration::from_secs(20);
    let _ = command::run_with_timeout("bluetoothctl", &["pair", address], long);
    let _ = command::run("bluetoothctl", &["trust", address]);
    let output = command::run_with_timeout("bluetoothctl", &["connect", address], long)?;
    if output.contains("Failed") {
        return Err("Connexion refusée par l'appareil".into());
    }
    Ok(())
}

pub fn disconnect(address: &str) -> Result<(), Error> {
    command::run("bluetoothctl", &["disconnect", address]).map(drop)
}

pub fn forget(address: &str) -> Result<(), Error> {
    command::run("bluetoothctl", &["remove", address]).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etat_et_appareils() {
        let show = "Controller 00:11:22:33:44:55 (public)\n\tName: ferrite\n\tPowered: yes\n";
        assert_eq!(parse_powered(show), Some(true));
        assert_eq!(parse_powered("Powered: no"), Some(false));
        assert_eq!(parse_powered("rien"), None);
        let devices = parse_devices("Device AA:BB:CC:DD:EE:FF Casque Audio\nautre\n");
        assert_eq!(
            devices,
            vec![("AA:BB:CC:DD:EE:FF".to_owned(), "Casque Audio".to_owned())]
        );
    }

    #[test]
    fn genre_et_batterie_d_un_appareil() {
        let info = "Device AA\n\tName: Casque\n\tIcon: audio-headset\n\tBattery Percentage: 0x50 (80)\n";
        assert_eq!(parse_field(info, "Icon"), Some("audio-headset"));
        assert_eq!(parse_battery(info), Some(80));
        assert_eq!(parse_battery("Name: x"), None);
    }
}
