//! Bluetooth, par BlueZ (`bluetoothctl`).
//!
//! Sans adaptateur, `bluetoothctl` attend indéfiniment le démon : on vérifie donc d'abord
//! la présence d'un adaptateur dans `/sys/class/bluetooth`, et chaque appel a un délai.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

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
    /// Approuvé : il peut se reconnecter seul.
    pub trusted: bool,
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
        // Les appareils sans nom (balises, téléphones d'autres personnes) encombrent la
        // liste : seuls restent ceux qui ont un nom, ou qu'on connaît déjà.
        .filter(|(address, name)| {
            !is_unnamed(address, name) || paired.contains(address) || connected.contains(address)
        })
        .map(|(address, name)| {
            let info = command::run("bluetoothctl", &["info", &address]).unwrap_or_default();
            Device {
                paired: paired.contains(&address),
                connected: connected.contains(&address),
                icon: parse_field(&info, "Icon").unwrap_or_default().to_owned(),
                battery: parse_battery(&info),
                trusted: parse_field(&info, "Trusted") == Some("yes"),
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

/// BlueZ nomme un appareil anonyme d'après son adresse (« AA-BB-CC-DD-EE-FF »).
pub fn is_unnamed(address: &str, name: &str) -> bool {
    name.is_empty() || name.replace('-', ":").eq_ignore_ascii_case(address)
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

/// Délai d'un appairage ou d'une connexion.
const LONG: Duration = Duration::from_secs(30);
/// Temps laissé à un appareil pour se montrer pendant la recherche.
const DISCOVERY: Duration = Duration::from_secs(15);

/// Session `bluetoothctl` pilotée par son entrée standard : l'agent d'appairage reste
/// enregistré pendant tout l'échange, et la recherche peut tourner pendant qu'on attend
/// l'appareil (en ligne de commande, chaque appel est un client à part, sans agent, et la
/// recherche s'arrête avec lui).
struct Session {
    child: Child,
    input: ChildStdin,
    lines: Receiver<String>,
}

impl Session {
    fn start() -> Result<Self, Error> {
        let mut child = Command::new("bluetoothctl")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("LC_ALL", "C")
            .spawn()
            .map_err(|err| format!("bluetoothctl indisponible : {err}"))?;
        let input = child.stdin.take().ok_or("bluetoothctl : entrée indisponible")?;
        let output = child.stdout.take().ok_or("bluetoothctl : sortie indisponible")?;
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(output);
            let mut buffer = Vec::new();
            while reader.read_until(b'\n', &mut buffer).is_ok_and(|read| read > 0) {
                let line = strip_escapes(&String::from_utf8_lossy(&buffer));
                buffer.clear();
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut session = Self { child, input, lines };
        // NoInputNoOutput : appairage « Just Works » avec tout appareil, sans code à saisir
        // ni à confirmer ici (un téléphone demande seulement d'accepter de son côté).
        session.send("agent NoInputNoOutput")?;
        session.send("default-agent")?;
        session.send("pairable on")?;
        let _ = session.expect(
            &["Default agent request successful", "Failed"],
            Duration::from_secs(3),
        );
        Ok(session)
    }

    fn send(&mut self, command: &str) -> Result<(), Error> {
        writeln!(self.input, "{command}")
            .and_then(|()| self.input.flush())
            .map_err(|err| format!("bluetoothctl : {err}"))
    }

    /// Attend une ligne contenant l'un des `patterns` ; renvoie la ligne.
    fn expect(&self, patterns: &[&str], timeout: Duration) -> Option<String> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            let line = self.lines.recv_timeout(left).ok()?;
            if patterns.iter().any(|pattern| line.contains(pattern)) {
                return Some(line);
            }
        }
    }

    /// Oublie ce qui a été lu jusqu'ici.
    fn drain(&self) {
        while self.lines.try_recv().is_ok() {}
    }

    fn paired(&mut self, address: &str) -> bool {
        self.drain();
        if self.send(&format!("info {address}")).is_err() {
            return false;
        }
        self.expect(&["Paired:", "not available"], Duration::from_secs(3))
            .is_some_and(|line| line.contains("Paired: yes"))
    }

    /// Recherche jusqu'à voir l'appareil (ou au bout du délai), puis l'arrête : l'appairage
    /// se passe mieux sans recherche en cours.
    fn discover(&mut self, address: &str) -> Result<(), Error> {
        self.drain();
        self.send("scan on")?;
        let seen = self.expect(&[&format!("Device {address}")], DISCOVERY);
        self.send("scan off")?;
        if seen.is_none() {
            return Err("Appareil introuvable : allumez-le et mettez-le en mode appairage".into());
        }
        Ok(())
    }

    fn pair(&mut self, address: &str) -> Result<(), Error> {
        self.discover(address)?;
        self.drain();
        self.send(&format!("pair {address}"))?;
        let line = self
            .expect(&["Pairing successful", "Failed to pair", "not available"], LONG)
            .ok_or("L'appareil n'a pas répondu à la demande d'appairage")?;
        if line.contains("Pairing successful") || line.contains("AlreadyExists") {
            return Ok(());
        }
        Err(explain(&line))
    }

    fn connect(&mut self, address: &str) -> Result<(), String> {
        self.drain();
        self.send(&format!("connect {address}"))?;
        let line = self
            .expect(
                &["Connection successful", "Failed to connect", "not available"],
                LONG,
            )
            .ok_or("L'appareil n'a pas répondu à temps")?;
        if line.contains("Connection successful") {
            Ok(())
        } else {
            Err(line)
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.send("quit");
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Retire les couleurs et les invites de `bluetoothctl`.
fn strip_escapes(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\u{1b}' => {
                // CSI … lettre
                if characters.peek() == Some(&'[') {
                    characters.next();
                    for next in characters.by_ref() {
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
            '\u{1}' | '\u{2}' | '\r' => {}
            character => clean.push(character),
        }
    }
    clean.trim().to_owned()
}

/// Clé d'appairage perdue d'un côté : il faut appairer à nouveau.
fn key_missing(failure: &str) -> bool {
    failure.contains("key-missing") || failure.contains("AuthenticationFailed")
}

/// Message en français pour une erreur de BlueZ.
pub fn explain(failure: &str) -> String {
    let known = [
        (
            "profile-unavailable",
            "Aucun profil compatible : le module Bluetooth du son (PipeWire) est absent",
        ),
        (
            "page-timeout",
            "L'appareil ne répond pas : allumez-le et rapprochez-le",
        ),
        (
            "ConnectionAttemptFailed",
            "L'appareil ne répond pas : allumez-le et rapprochez-le",
        ),
        (
            "key-missing",
            "L'appareil a oublié l'appairage : mettez-le en mode appairage",
        ),
        (
            "Authentication",
            "Appairage refusé : mettez l'appareil en mode appairage",
        ),
        (
            "not available",
            "Appareil introuvable : allumez-le et mettez-le en mode appairage",
        ),
        ("InProgress", "Une connexion est déjà en cours"),
        ("NotReady", "Le Bluetooth n'est pas prêt : allumez-le"),
        ("Blocked", "Le Bluetooth est bloqué (mode avion ?)"),
    ];
    known
        .iter()
        .find(|(pattern, _)| failure.contains(pattern))
        .map_or_else(
            || {
                let detail = failure.rsplit(": ").next().unwrap_or(failure).trim();
                format!("Échec Bluetooth : {detail}")
            },
            |(_, message)| (*message).to_owned(),
        )
}

/// Appaire (si besoin, en recherchant l'appareil), fait confiance puis connecte. Si
/// l'appareil a oublié l'appairage (casque réinitialisé, appairé ailleurs), il est oublié
/// ici aussi puis appairé de nouveau. Un appareil audio devient la sortie du son.
pub fn connect(address: &str) -> Result<(), Error> {
    if !adapter_present() {
        return Err("Aucun adaptateur Bluetooth".into());
    }
    let mut session = Session::start()?;
    if !session.paired(address) {
        session.pair(address)?;
    }
    session.send(&format!("trust {address}"))?;
    if let Err(failure) = session.connect(address) {
        if !key_missing(&failure) {
            return Err(explain(&failure));
        }
        session.send(&format!("remove {address}"))?;
        let _ = session.expect(
            &["Device has been removed", "not available"],
            Duration::from_secs(3),
        );
        session.pair(address)?;
        session.send(&format!("trust {address}"))?;
        session.connect(address).map_err(|failure| explain(&failure))?;
    }
    drop(session);
    crate::audio::follow_bluetooth(address);
    Ok(())
}

pub fn disconnect(address: &str) -> Result<(), Error> {
    command::run("bluetoothctl", &["disconnect", address])
        .map(drop)
        .map_err(|failure| explain(&failure))
}

/// Approuve l'appareil (il se reconnecte seul) ou lui retire sa confiance.
pub fn set_trusted(address: &str, trusted: bool) -> Result<(), Error> {
    let verb = if trusted { "trust" } else { "untrust" };
    command::run("bluetoothctl", &[verb, address]).map(drop)
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
    fn sortie_de_bluetoothctl_et_erreurs() {
        assert_eq!(
            strip_escapes("\u{1b}[0;94m[bluetooth]\u{1b}[0m# Pairing successful\r\n"),
            "[bluetooth]# Pairing successful"
        );
        let missing = "Failed to connect: org.bluez.Error.Failed br-connection-key-missing";
        assert!(key_missing(missing));
        assert!(explain(missing).contains("mode appairage"));
        assert!(
            explain("Failed to connect: org.bluez.Error.Failed br-connection-profile-unavailable")
                .contains("PipeWire")
        );
        assert_eq!(
            explain("Failed to pair: org.bluez.Error.Bizarre"),
            "Échec Bluetooth : org.bluez.Error.Bizarre"
        );
        assert!(is_unnamed("B0:38:E2:60:C8:E5", "B0-38-E2-60-C8-E5"));
        assert!(!is_unnamed("B0:38:E2:60:C8:E4", "soundcore Q20i"));
    }

    #[test]
    fn genre_et_batterie_d_un_appareil() {
        let info = "Device AA\n\tName: Casque\n\tIcon: audio-headset\n\tBattery Percentage: 0x50 (80)\n";
        assert_eq!(parse_field(info, "Icon"), Some("audio-headset"));
        assert_eq!(parse_battery(info), Some(80));
        assert_eq!(parse_battery("Name: x"), None);
    }
}

#[cfg(test)]
mod live {
    /// Sur une machine avec un appareil appairé : `FERRITE_BT=AA:BB:… cargo test -- --ignored`.
    #[test]
    #[ignore = "demande un adaptateur et un appareil réels"]
    fn session_reelle() {
        let address = std::env::var("FERRITE_BT").expect("FERRITE_BT");
        let mut session = super::Session::start().unwrap();
        eprintln!("appairé : {}", session.paired(&address));
        eprintln!("connexion : {:?}", session.connect(&address));
    }
}
