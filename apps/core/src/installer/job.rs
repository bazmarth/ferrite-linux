//! Travail en administrateur, dans un fil à part : la sonde des disques et l'installation,
//! toutes deux par `ferrite-installation` (lancé par `sudo -n` : sur la clé live, le compte
//! `ferrite` est administrateur sans mot de passe).

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use super::disks::{self, Disk};

/// Le script, installé avec l'image (`image/config/includes.chroot/usr/local/bin`).
const SCRIPT: &str = "/usr/local/bin/ferrite-installation";
/// Journal détaillé, écrit par le script.
pub const LOG: &str = "/var/log/ferrite-installation.log";

/// Démo pour les captures : disques du ThinkBook, installation simulée.
pub fn demo() -> bool {
    std::env::var_os("FERRITE_INSTALLATION_DEMO").is_some()
}

/// Écran demandé en démo (`FERRITE_INSTALLATION_DEMO=compte`…).
pub fn demo_screen() -> Option<String> {
    std::env::var("FERRITE_INSTALLATION_DEMO").ok()
}

/// Recherche des disques et des systèmes installés, en cours.
pub struct Probe {
    receiver: Receiver<Result<Vec<Disk>, String>>,
}

impl Probe {
    pub fn start(context: &eframe::egui::Context) -> Self {
        let (sender, receiver) = mpsc::channel();
        let context = context.clone();
        thread::spawn(move || {
            let _ = sender.send(probe());
            context.request_repaint();
        });
        Self { receiver }
    }

    /// Le résultat, une fois arrivé.
    pub fn poll(&self) -> Option<Result<Vec<Disk>, String>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("La recherche des disques s'est arrêtée.".into())),
        }
    }
}

fn probe() -> Result<Vec<Disk>, String> {
    if demo() {
        thread::sleep(std::time::Duration::from_millis(600));
        let mut disks = disks::parse_lsblk(DEMO_LSBLK);
        disks::apply_probe(&mut disks, &disks::parse_probe(DEMO_PROBE));
        return Ok(disks);
    }
    let sonde = Command::new("sudo")
        .args(["-n", SCRIPT, "sonder"])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Impossible de lancer la recherche des disques ({error})."))?;
    if !sonde.status.success() {
        return Err(
            "La recherche des disques demande les droits d'administrateur. L'installation se lance depuis la clé de Ferrite."
                .into(),
        );
    }
    let listing = Command::new("lsblk")
        .args(["-P", "-b", "-o", disks::LSBLK_COLUMNS])
        .output()
        .map_err(|error| format!("Impossible de lire les disques ({error})."))?;
    let mut found = disks::parse_lsblk(&String::from_utf8_lossy(&listing.stdout));
    disks::apply_probe(
        &mut found,
        &disks::parse_probe(&String::from_utf8_lossy(&sonde.stdout)),
    );
    Ok(found)
}

/// Nouvelle d'une installation en cours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Step(String),
    Progress(u8),
    Done,
    Failed(String),
}

/// Installation en cours.
pub struct Install {
    receiver: Receiver<Event>,
}

impl Install {
    /// Lance `ferrite-installation installer PLAN` ; le mot de passe passe par l'entrée
    /// standard, jamais par un fichier.
    pub fn start(context: &eframe::egui::Context, plan: String, password: String) -> Self {
        let (sender, receiver) = mpsc::channel();
        let context = context.clone();
        thread::spawn(move || {
            let send = |event: Event| {
                let _ = sender.send(event);
                context.request_repaint();
            };
            if demo() {
                return simulate(&send);
            }
            if let Err(message) = run(&plan, &password, &send) {
                send(Event::Failed(message));
            }
        });
        Self { receiver }
    }

    /// Nouvelles arrivées depuis la dernière image.
    pub fn poll(&self) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            match self.receiver.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        events
    }
}

fn run(plan: &str, password: &str, send: &impl Fn(Event)) -> Result<(), String> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let path = std::path::Path::new(&runtime).join("ferrite-installation.plan");
    std::fs::write(&path, plan).map_err(|error| format!("Plan non écrit ({error})."))?;
    let mut child = Command::new("sudo")
        .arg("-n")
        .arg(SCRIPT)
        .arg("installer")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Impossible de lancer l'installation ({error})."))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{password}");
    }
    let mut finished = false;
    let mut failed = false;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(event) = parse_line(&line) {
                finished |= event == Event::Done;
                failed |= matches!(event, Event::Failed(_));
                send(event);
            }
        }
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(&path);
    if !finished && !failed {
        return Err(format!("L'installation s'est arrêtée ({status})."));
    }
    Ok(())
}

/// Une ligne du script : `etape TEXTE`, `progres N`, `erreur TEXTE` ou `fini`.
pub fn parse_line(line: &str) -> Option<Event> {
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    match word {
        "etape" => Some(Event::Step(rest.to_owned())),
        "progres" => rest
            .trim()
            .parse()
            .ok()
            .map(|value: u8| Event::Progress(value.min(100))),
        "erreur" => Some(Event::Failed(rest.to_owned())),
        "fini" => Some(Event::Done),
        _ => None,
    }
}

/// Installation simulée (captures d'écran), arrêtée à 42 %.
fn simulate(send: &impl Fn(Event)) {
    send(Event::Step("Préparation du disque".into()));
    send(Event::Progress(4));
    thread::sleep(std::time::Duration::from_millis(400));
    send(Event::Step("Copie de Ferrite sur le disque".into()));
    send(Event::Progress(42));
}

const DEMO_LSBLK: &str = r#"PATH="/dev/nvme0n1" PKNAME="" TYPE="disk" SIZE="512110190592" START="" PARTN="" LOG-SEC="512" FSTYPE="" LABEL="" PARTTYPE="" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="" MODEL="Micron MTFDKBA512TFH" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p1" PKNAME="nvme0n1" TYPE="part" SIZE="16777216" START="2048" PARTN="1" LOG-SEC="512" FSTYPE="" LABEL="" PARTTYPE="e3c9e316-0b5c-4db8-817d-f92df00215ae" PARTLABEL="Microsoft reserved partition" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p2" PKNAME="nvme0n1" TYPE="part" SIZE="313116327936" START="34816" PARTN="2" LOG-SEC="512" FSTYPE="ntfs" LABEL="" PARTTYPE="ebd0a0a2-b9e5-4433-87c0-68b6b72699c7" PARTLABEL="Basic data partition" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p3" PKNAME="nvme0n1" TYPE="part" SIZE="944766976" START="611590144" PARTN="3" LOG-SEC="512" FSTYPE="ntfs" LABEL="" PARTTYPE="de94bba4-06d1-4d40-a16a-bfd50179d6ac" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p4" PKNAME="nvme0n1" TYPE="part" SIZE="1024458752" START="613437440" PARTN="4" LOG-SEC="512" FSTYPE="vfat" LABEL="" PARTTYPE="c12a7328-f81f-11d2-ba4b-00a0c93ec93b" PARTLABEL="EFI System Partition" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p5" PKNAME="nvme0n1" TYPE="part" SIZE="2147483648" START="615438336" PARTN="5" LOG-SEC="512" FSTYPE="ext4" LABEL="" PARTTYPE="0fc63daf-8483-4772-8e79-3d69d8477de4" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p6" PKNAME="nvme0n1" TYPE="part" SIZE="194857926656" START="619632640" PARTN="6" LOG-SEC="512" FSTYPE="btrfs" LABEL="fedora" PARTTYPE="0fc63daf-8483-4772-8e79-3d69d8477de4" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/sdb" PKNAME="" TYPE="disk" SIZE="15518924800" START="" PARTN="" LOG-SEC="512" FSTYPE="iso9660" LABEL="FERRITE" PARTTYPE="" PARTLABEL="" PTTYPE="dos" MOUNTPOINT="/run/live/medium" MODEL="SanDisk" RM="1" RO="0" TRAN="usb" FSUSED=""
"#;

const DEMO_PROBE: &str =
    "systeme /dev/nvme0n1p4@/efi/Microsoft/Boot/bootmgfw.efi:Windows Boot Manager:Windows:efi
systeme /dev/nvme0n1p6:Fedora Linux 42 (Workstation Edition):Fedora:linux
ntfs /dev/nvme0n1p2 120000000000 ok
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lignes_du_script() {
        assert_eq!(parse_line("etape Copie"), Some(Event::Step("Copie".into())));
        assert_eq!(parse_line("progres 42"), Some(Event::Progress(42)));
        assert_eq!(parse_line("progres 250"), Some(Event::Progress(100)));
        assert_eq!(
            parse_line("erreur Disque plein"),
            Some(Event::Failed("Disque plein".into()))
        );
        assert_eq!(parse_line("fini"), Some(Event::Done));
        assert_eq!(parse_line("bruit"), None);
    }

    #[test]
    fn demo_lisible() {
        let mut found = disks::parse_lsblk(DEMO_LSBLK);
        disks::apply_probe(&mut found, &disks::parse_probe(DEMO_PROBE));
        assert_eq!(found.len(), 2);
        assert!(found[0].has_windows() && found[1].live);
    }
}
