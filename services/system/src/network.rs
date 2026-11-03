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
    /// Sécurité telle que donnée par NetworkManager (« WPA2 WPA3 »), fréquence, débit.
    pub security: String,
    pub frequency: String,
    pub rate: String,
}

impl WifiNetwork {
    /// Niveau du signal, de 1 à 4 barres.
    pub fn level(&self) -> u8 {
        match self.signal {
            0..=24 => 1,
            25..=49 => 2,
            50..=74 => 3,
            _ => 4,
        }
    }

    /// Détails d'une connexion : « WPA3 · 5 GHz · 433 Mbit/s ».
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(security) = self
            .security
            .split_whitespace()
            .last()
            .filter(|text| *text != "--")
        {
            parts.push(security.to_owned());
        }
        if let Some(megahertz) = self
            .frequency
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<u32>().ok())
        {
            parts.push(if megahertz >= 5900 {
                "6 GHz".to_owned()
            } else if megahertz >= 4900 {
                "5 GHz".to_owned()
            } else {
                "2,4 GHz".to_owned()
            });
        }
        if !self.rate.is_empty() {
            parts.push(self.rate.clone());
        }
        parts.join(" · ")
    }
}

/// Réseaux visibles, du plus fort au plus faible (relance un balayage).
pub fn wifi_networks() -> Result<Vec<WifiNetwork>, Error> {
    let output = command::run_with_timeout(
        "nmcli",
        &[
            "-t",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY,FREQ,RATE",
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

/// Analyse `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY,FREQ,RATE device wifi list`.
pub fn parse_wifi_list(output: &str) -> Vec<WifiNetwork> {
    let mut networks: Vec<WifiNetwork> = Vec::new();
    for line in output.lines() {
        let fields = split_terse(line);
        let (in_use, ssid, signal, security, frequency, rate) = match fields.as_slice() {
            [in_use, ssid, signal, security] => (in_use, ssid, signal, security, "", ""),
            [in_use, ssid, signal, security, frequency, rate] => {
                (in_use, ssid, signal, security, frequency.as_str(), rate.as_str())
            }
            _ => continue,
        };
        if ssid.is_empty() {
            continue;
        }
        let network = WifiNetwork {
            ssid: ssid.clone(),
            signal: signal.parse().unwrap_or(0),
            secured: !security.is_empty() && security != "--",
            connected: in_use == "*",
            security: security.clone(),
            frequency: frequency.to_owned(),
            rate: rate.to_owned(),
        };
        // Un même réseau peut apparaître sur plusieurs points d'accès : on garde le meilleur.
        match networks.iter_mut().find(|known| known.ssid == network.ssid) {
            Some(known) => {
                // Les détails sont ceux du point d'accès utilisé.
                if network.connected {
                    known.security = network.security;
                    known.frequency = network.frequency;
                    known.rate = network.rate;
                }
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

/// Réseau masqué : son nom ne se voit pas dans la liste.
pub fn connect_hidden_wifi(ssid: &str, password: Option<&str>) -> Result<(), Error> {
    let mut args = vec!["device", "wifi", "connect", ssid];
    if let Some(password) = password.filter(|password| !password.is_empty()) {
        args.extend(["password", password]);
    }
    args.extend(["hidden", "yes"]);
    command::run_with_timeout("nmcli", &args, Duration::from_secs(30)).map(drop)
}

/// Réseau Wi-Fi enregistré.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownWifi {
    pub name: String,
    pub autoconnect: bool,
}

/// Réseaux Wi-Fi enregistrés par NetworkManager.
pub fn known_wifi() -> Result<Vec<KnownWifi>, Error> {
    let output = command::run(
        "nmcli",
        &["-t", "-f", "NAME,TYPE,AUTOCONNECT", "connection", "show"],
    )?;
    Ok(parse_known_wifi(&output))
}

/// Analyse `nmcli -t -f NAME,TYPE,AUTOCONNECT connection show`.
pub fn parse_known_wifi(output: &str) -> Vec<KnownWifi> {
    output
        .lines()
        .filter_map(|line| match split_terse(line).as_slice() {
            [name, kind, autoconnect] if kind.contains("wireless") => Some(KnownWifi {
                name: name.clone(),
                autoconnect: autoconnect == "yes",
            }),
            _ => None,
        })
        .collect()
}

pub fn forget_wifi(name: &str) -> Result<(), Error> {
    command::run("nmcli", &["connection", "delete", "id", name]).map(drop)
}

/// Connexion automatique à tous les réseaux enregistrés.
pub fn set_autoconnect(enabled: bool) -> Result<(), Error> {
    let value = if enabled { "yes" } else { "no" };
    for known in known_wifi()? {
        command::run(
            "nmcli",
            &[
                "connection",
                "modify",
                "id",
                &known.name,
                "connection.autoconnect",
                value,
            ],
        )?;
    }
    Ok(())
}

/// Point d'accès : renvoie son nom et son mot de passe.
pub fn start_hotspot() -> Result<String, Error> {
    command::run_with_timeout("nmcli", &["device", "wifi", "hotspot"], Duration::from_secs(20))?;
    let shown = command::run("nmcli", &["device", "wifi", "show-password"]).unwrap_or_default();
    let field = |label: &str| {
        shown
            .lines()
            .find_map(|line| line.strip_prefix(label))
            .map(str::trim)
            .unwrap_or_default()
            .to_owned()
    };
    Ok(format!(
        "Point d'accès « {} », mot de passe {}",
        field("SSID:"),
        field("Password:")
    ))
}

pub fn stop_hotspot() -> Result<(), Error> {
    command::run("nmcli", &["connection", "down", "id", "Hotspot"]).map(drop)
}

/// Adresse, passerelle, DNS et adresse matérielle d'une interface : libellés et valeurs.
pub fn device_details(device: &str) -> Result<Vec<(String, String)>, Error> {
    let output = command::run(
        "nmcli",
        &[
            "-t",
            "-f",
            "IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,GENERAL.HWADDR",
            "device",
            "show",
            device,
        ],
    )?;
    Ok(parse_device_details(&output))
}

/// Analyse `nmcli -t -f IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,GENERAL.HWADDR device show`.
pub fn parse_device_details(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let fields = split_terse(line);
            let [key, value] = fields.as_slice() else {
                return None;
            };
            let label = match key.split('[').next().unwrap_or(key) {
                "IP4.ADDRESS" => "Adresse IP",
                "IP4.GATEWAY" => "Passerelle",
                "IP4.DNS" => "DNS",
                "GENERAL.HWADDR" => "Adresse matérielle",
                _ => return None,
            };
            (!value.is_empty() && value != "--").then(|| (label.to_owned(), value.clone()))
        })
        .collect()
}

/// Adresse matérielle et DNS d'une connexion enregistrée.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WifiOptions {
    /// `stable` : aléatoire par réseau ; `permanent` : la vraie adresse.
    pub mac: String,
    /// Vide : automatique.
    pub dns: String,
}

pub fn wifi_options(name: &str) -> Result<WifiOptions, Error> {
    let output = command::run(
        "nmcli",
        &[
            "-g",
            "802-11-wireless.cloned-mac-address,ipv4.dns",
            "connection",
            "show",
            "id",
            name,
        ],
    )?;
    let mut lines = output.lines();
    Ok(WifiOptions {
        mac: lines.next().unwrap_or_default().trim().to_owned(),
        dns: lines.next().unwrap_or_default().trim().to_owned(),
    })
}

/// Change l'adresse matérielle présentée, puis reconnecte.
pub fn set_wifi_mac(name: &str, mac: &str) -> Result<(), Error> {
    command::run(
        "nmcli",
        &[
            "connection",
            "modify",
            "id",
            name,
            "802-11-wireless.cloned-mac-address",
            mac,
        ],
    )?;
    command::run_with_timeout(
        "nmcli",
        &["connection", "up", "id", name],
        Duration::from_secs(30),
    )
    .map(drop)
}

/// Serveurs DNS de la connexion (vide : ceux du réseau), puis reconnecte.
pub fn set_wifi_dns(name: &str, dns: &str) -> Result<(), Error> {
    let ignore = if dns.is_empty() { "no" } else { "yes" };
    command::run(
        "nmcli",
        &[
            "connection",
            "modify",
            "id",
            name,
            "ipv4.dns",
            dns,
            "ipv4.ignore-auto-dns",
            ignore,
        ],
    )?;
    command::run_with_timeout(
        "nmcli",
        &["connection", "up", "id", name],
        Duration::from_secs(30),
    )
    .map(drop)
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

    #[test]
    fn details_du_reseau_connecte() {
        let output = " :Maison:60:WPA2:2437 MHz:130 Mbit/s\n*:Maison:90:WPA2 WPA3:5180 MHz:433 Mbit/s\n";
        let network = &parse_wifi_list(output)[0];
        assert_eq!(network.summary(), "WPA3 · 5 GHz · 433 Mbit/s");
        assert_eq!(network.level(), 4);
    }

    #[test]
    fn reseaux_enregistres() {
        let known = parse_known_wifi(
            "Maison:802-11-wireless:yes\nFilaire:802-3-ethernet:yes\nAtelier:802-11-wireless:no\n",
        );
        assert_eq!(known.len(), 2);
        assert!(known[0].autoconnect && !known[1].autoconnect);
    }

    #[test]
    fn details_d_une_interface() {
        let details = parse_device_details(
            "GENERAL.HWADDR:AA\\:BB\nIP4.ADDRESS[1]:192.168.1.2/24\nIP4.GATEWAY:--\nIP4.DNS[1]:1.1.1.1\n",
        );
        assert_eq!(details[0], ("Adresse matérielle".to_owned(), "AA:BB".to_owned()));
        assert_eq!(details[1].1, "192.168.1.2/24");
        assert_eq!(details.len(), 3);
    }
}
