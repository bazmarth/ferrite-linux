//! Réseau, par NetworkManager (`nmcli`).

use crate::command::{self, Error};
use std::time::Duration;

/// Radio Wi-Fi active ? `None` s'il n'y a pas de carte Wi-Fi ou pas de NetworkManager.
pub fn wifi_enabled() -> Option<bool> {
    let devices = command::run("nmcli", &["-t", "-f", "TYPE", "device"]).ok()?;
    if !devices.lines().any(|line| line.trim() == "wifi") {
        return None;
    }
    let radio = command::run("nmcli", &["-t", "-f", "WIFI", "radio"]).ok()?;
    Some(radio.trim() == "enabled")
}

pub fn set_wifi(enabled: bool) -> Result<(), Error> {
    command::run("nmcli", &["radio", "wifi", if enabled { "on" } else { "off" }]).map(drop)
}

/// Réseau Wi-Fi visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub secured: bool,
    pub connected: bool,
}

/// Réseaux visibles, du plus fort au plus faible (relance un balayage).
pub fn wifi_networks() -> Result<Vec<WifiNetwork>, Error> {
    let output = command::run_with_timeout(
        "nmcli",
        &[
            "-t",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "--rescan",
            "yes",
        ],
        Duration::from_secs(15),
    )?;
    Ok(parse_wifi_list(&output))
}

/// Analyse `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY device wifi list`.
pub fn parse_wifi_list(output: &str) -> Vec<WifiNetwork> {
    let mut networks: Vec<WifiNetwork> = Vec::new();
    for line in output.lines() {
        let fields = split_terse(line);
        let [in_use, ssid, signal, security] = fields.as_slice() else {
            continue;
        };
        if ssid.is_empty() {
            continue;
        }
        let network = WifiNetwork {
            ssid: ssid.clone(),
            signal: signal.parse().unwrap_or(0),
            secured: !security.is_empty() && security != "--",
            connected: in_use == "*",
        };
        // Un même réseau peut apparaître sur plusieurs points d'accès : on garde le meilleur.
        match networks.iter_mut().find(|known| known.ssid == network.ssid) {
            Some(known) => {
                known.connected |= network.connected;
                known.signal = known.signal.max(network.signal);
            }
            None => networks.push(network),
        }
    }
    networks.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
    networks
}

pub fn connect_wifi(ssid: &str, password: Option<&str>) -> Result<(), Error> {
    let mut args = vec!["device", "wifi", "connect", ssid];
    if let Some(password) = password.filter(|password| !password.is_empty()) {
        args.extend(["password", password]);
    }
    command::run_with_timeout("nmcli", &args, Duration::from_secs(30)).map(drop)
}

pub fn disconnect_wifi(ssid: &str) -> Result<(), Error> {
    command::run("nmcli", &["connection", "down", "id", ssid]).map(drop)
}

/// Interface réseau et son état.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub kind: String,
    pub state: String,
    pub connection: String,
}

pub fn devices() -> Result<Vec<Device>, Error> {
    let output = command::run("nmcli", &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"])?;
    Ok(output
        .lines()
        .filter_map(|line| match split_terse(line).as_slice() {
            [name, kind, state, connection] if kind != "loopback" => Some(Device {
                name: name.clone(),
                kind: kind.clone(),
                state: state.clone(),
                connection: connection.clone(),
            }),
            _ => None,
        })
        .collect())
}

/// Découpe une ligne du format « terse » de nmcli (`:` séparateur, `\:` littéral).
pub fn split_terse(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                if let (Some(next), Some(field)) = (chars.next(), fields.last_mut()) {
                    field.push(next);
                }
            }
            ':' => fields.push(String::new()),
            other => {
                if let Some(field) = fields.last_mut() {
                    field.push(other);
                }
            }
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liste_des_reseaux() {
        let output = "\
 :Voisins:40:WPA2
*:Maison\\:5G:82:WPA2 WPA3
 :Maison\\:5G:60:WPA2
 :Café:0:
 ::30:WPA2
 :Gare:55:
";
        let networks = parse_wifi_list(output);
        assert_eq!(networks[0].ssid, "Maison:5G");
        assert!(networks[0].connected && networks[0].secured);
        assert_eq!(networks[0].signal, 82);
        assert_eq!(networks[1].ssid, "Gare");
        assert!(!networks[1].secured);
        assert_eq!(networks.len(), 4);
    }
}
