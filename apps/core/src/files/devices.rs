//! Appareils par `lsblk` et `udisksctl` (paquet udisks2 de l'image) : clés USB et cartes
//! mémoire (montage sans mot de passe, éjection), et partitions internes qui portent des
//! données (un autre disque, la partition de Windows). Une partition interne pas encore
//! montée s'ouvre en lecture seule, après le mot de passe administrateur : on ne risque
//! pas d'abîmer un autre système.

use std::path::PathBuf;

use ferrite_system::command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Partition, par exemple `/dev/sdb1`.
    pub path: String,
    /// Disque entier, pour l'éjection, par exemple `/dev/sdb`.
    pub disk: String,
    pub label: String,
    pub size: String,
    pub mount: Option<PathBuf>,
    /// Espace libre et taille, une fois montée.
    pub usage: Option<(u64, u64)>,
    /// Clé ou carte (éjectable), plutôt que partition d'un disque interne.
    pub removable: bool,
}

/// Systèmes de fichiers de données qu'on propose d'ouvrir.
const DATA_FILESYSTEMS: [&str; 9] = [
    "ntfs", "exfat", "vfat", "ext4", "ext3", "ext2", "btrfs", "xfs", "f2fs",
];
/// Partitions internes plus petites : démarrage d'un autre système, outils du fabricant.
const INTERNAL_MIN_BYTES: u64 = 4 << 30;

/// Clés, cartes et partitions internes de données.
pub fn list() -> Vec<Device> {
    command::run(
        "lsblk",
        &[
            "-P",
            "-o",
            "PATH,PKNAME,LABEL,SIZE,RM,HOTPLUG,TYPE,FSTYPE,MOUNTPOINT,PARTTYPENAME",
        ],
    )
    .map(|output| parse_lsblk(&output))
    .unwrap_or_default()
}

pub fn parse_lsblk(output: &str) -> Vec<Device> {
    output
        .lines()
        .filter_map(|line| {
            let field = |key: &str| pair(line, key).unwrap_or_default();
            let removable = field("RM") == "1" || field("HOTPLUG") == "1";
            let usable = matches!(field("TYPE").as_str(), "part" | "disk") && !field("FSTYPE").is_empty();
            let mount = field("MOUNTPOINT");
            if !usable
                || (!removable
                    && !internal_data(&field("FSTYPE"), &field("PARTTYPENAME"), &mount, &field("SIZE")))
            {
                return None;
            }
            let path = field("PATH");
            let parent = field("PKNAME");
            let label = field("LABEL");
            Some(Device {
                disk: if parent.is_empty() {
                    path.clone()
                } else {
                    format!("/dev/{parent}")
                },
                label: friendly_label(&label, &field("FSTYPE"), &field("SIZE"), removable),
                size: field("SIZE"),
                mount: Some(mount).filter(|mount| !mount.is_empty()).map(PathBuf::from),
                usage: None,
                removable,
                path,
            })
        })
        .collect()
}

/// Partition interne à proposer : des données, ni partition technique (EFI, réservée,
/// récupération, échange), ni système en cours (montée ailleurs que sous /media ou /mnt),
/// ni petite partition de démarrage.
fn internal_data(fstype: &str, kind: &str, mount: &str, size: &str) -> bool {
    let technical = ["EFI", "reserved", "recovery", "swap"]
        .iter()
        .any(|word| kind.to_lowercase().contains(&word.to_lowercase()));
    if technical || !DATA_FILESYSTEMS.iter().any(|known| fstype.starts_with(known)) {
        return false;
    }
    if mount.is_empty() {
        return size_bytes(size).is_some_and(|bytes| bytes >= INTERNAL_MIN_BYTES);
    }
    ["/media/", "/mnt/", "/run/media/"]
        .iter()
        .any(|place| mount.starts_with(place))
}

/// Nom montré dans la barre latérale : l'étiquette du volume (« fedora » devient
/// « Fedora »), sinon sa nature et sa taille (« Clé USB de 4 Go », « Windows (291,6 Go) »).
fn friendly_label(label: &str, fstype: &str, size: &str, removable: bool) -> String {
    if !label.is_empty() {
        if label.chars().any(char::is_uppercase) {
            return label.to_owned();
        }
        let mut chars = label.chars();
        return chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect())
            .unwrap_or_default();
    }
    let size = french_size(size);
    if removable {
        format!("Clé USB de {size}")
    } else if fstype == "ntfs" {
        format!("Windows ({size})")
    } else {
        format!("Disque de {size}")
    }
}

/// « 165.9G » ou « 165,9G » de `lsblk` en « 165,9 Go ».
fn french_size(size: &str) -> String {
    let size = size.trim().replace('.', ",");
    let Some(split) = size.find(|c: char| c.is_ascii_alphabetic()) else {
        return size;
    };
    let (number, unit) = size.split_at(split);
    let unit = match unit {
        "B" => "o",
        "K" => "Ko",
        "M" => "Mo",
        "G" => "Go",
        "T" => "To",
        other => other,
    };
    format!("{number} {unit}")
}

/// Taille écrite par `lsblk` (« 901M », « 165,9G », « 1,8T ») en octets.
fn size_bytes(size: &str) -> Option<u64> {
    let size = size.trim().replace(',', ".");
    let (number, unit) = size.split_at(size.find(|c: char| c.is_ascii_alphabetic())?);
    let shift = match unit {
        "B" => 0,
        "K" => 10,
        "M" => 20,
        "G" => 30,
        "T" => 40,
        _ => return None,
    };
    Some((number.parse::<f64>().ok()? * (1u64 << shift) as f64) as u64)
}

/// Valeur de `CLÉ="valeur"` dans une ligne de `lsblk -P`.
fn pair(line: &str, key: &str) -> Option<String> {
    let start = line.find(&format!("{key}=\""))? + key.len() + 2;
    let end = line[start..].find('"')? + start;
    Some(line[start..end].replace("\\x20", " "))
}

/// Monte la partition et renvoie son point de montage. Une clé se monte sans mot de
/// passe ; une partition interne le demande (fenêtre de l'agent polkit du bureau) et
/// s'ouvre en lecture seule.
pub fn mount(device: &Device) -> Result<PathBuf, String> {
    let output = if device.removable {
        command::run(
            "udisksctl",
            &["mount", "--no-user-interaction", "-b", &device.path],
        )?
    } else {
        command::run("udisksctl", &["mount", "-o", "ro", "-b", &device.path])?
    };
    // « Mounted /dev/sdb1 at /media/ferrite/CLE »
    output
        .split(" at ")
        .nth(1)
        .map(|path| PathBuf::from(path.trim().trim_end_matches('.')))
        .ok_or_else(|| format!("Montage inattendu : {}", output.trim()))
}

/// Démonte et met hors tension : la clé peut être retirée.
pub fn eject(device: &Device) -> Result<(), String> {
    if device.mount.is_some() {
        command::run(
            "udisksctl",
            &["unmount", "--no-user-interaction", "-b", &device.path],
        )?;
    }
    let _ = command::run(
        "udisksctl",
        &["power-off", "--no-user-interaction", "-b", &device.disk],
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_de_lsblk() {
        let output = r#"PATH="/dev/nvme0n1p2" PKNAME="nvme0n1" LABEL="" SIZE="291,6G" RM="0" HOTPLUG="0" TYPE="part" FSTYPE="ntfs" MOUNTPOINT="" PARTTYPENAME="Microsoft basic data"
PATH="/dev/nvme0n1p3" PKNAME="nvme0n1" LABEL="" SIZE="901M" RM="0" HOTPLUG="0" TYPE="part" FSTYPE="ntfs" MOUNTPOINT="" PARTTYPENAME="Windows recovery environment"
PATH="/dev/nvme0n1p4" PKNAME="nvme0n1" LABEL="" SIZE="977M" RM="0" HOTPLUG="0" TYPE="part" FSTYPE="vfat" MOUNTPOINT="" PARTTYPENAME="EFI System"
PATH="/dev/nvme0n1p5" PKNAME="nvme0n1" LABEL="" SIZE="2G" RM="0" HOTPLUG="0" TYPE="part" FSTYPE="ext4" MOUNTPOINT="" PARTTYPENAME="Linux filesystem"
PATH="/dev/sda4" PKNAME="sda" LABEL="FICHIERS" SIZE="238,9G" RM="0" HOTPLUG="0" TYPE="part" FSTYPE="ntfs" MOUNTPOINT="/media/FICHIERS" PARTTYPENAME="Microsoft basic data"
PATH="/dev/sda5" PKNAME="sda" LABEL="FERRITE" SIZE="58,9G" RM="0" HOTPLUG="0" TYPE="part" FSTYPE="ext4" MOUNTPOINT="/" PARTTYPENAME="Linux filesystem"
PATH="/dev/sda3" PKNAME="sda" LABEL="" SIZE="12,3G" RM="0" HOTPLUG="0" TYPE="part" FSTYPE="swap" MOUNTPOINT="[SWAP]" PARTTYPENAME="Linux swap"
PATH="/dev/sdb" PKNAME="" LABEL="" SIZE="28,9G" RM="1" HOTPLUG="1" TYPE="disk" FSTYPE="" MOUNTPOINT="" PARTTYPENAME=""
PATH="/dev/sdb1" PKNAME="sdb" LABEL="MA\x20CLE" SIZE="28,9G" RM="1" HOTPLUG="1" TYPE="part" FSTYPE="vfat" MOUNTPOINT="/media/ferrite/MA CLE" PARTTYPENAME="EFI (FAT-12/16/32)"
PATH="/dev/sdc1" PKNAME="sdc" LABEL="" SIZE="4G" RM="0" HOTPLUG="1" TYPE="part" FSTYPE="exfat" MOUNTPOINT="" PARTTYPENAME=""
"#;
        let devices = parse_lsblk(output);
        let labels: Vec<&str> = devices.iter().map(|device| device.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Windows (291,6 Go)", "FICHIERS", "MA CLE", "Clé USB de 4 Go"]
        );
        assert!(!devices[0].removable && devices[0].mount.is_none());
        assert_eq!(devices[1].mount, Some(PathBuf::from("/media/FICHIERS")));
        assert_eq!(devices[2].disk, "/dev/sdb");
        assert!(devices[2].removable);
        assert_eq!(devices[2].mount, Some(PathBuf::from("/media/ferrite/MA CLE")));
    }

    #[test]
    fn noms_lisibles() {
        assert_eq!(friendly_label("fedora", "btrfs", "120G", false), "Fedora");
        assert_eq!(friendly_label("MonDisque", "ext4", "1T", false), "MonDisque");
        assert_eq!(friendly_label("", "ext4", "165.9G", false), "Disque de 165,9 Go");
        assert_eq!(french_size("901M"), "901 Mo");
    }

    #[test]
    fn tailles() {
        assert_eq!(size_bytes("2G"), Some(2 << 30));
        assert_eq!(size_bytes("901M"), Some(901 << 20));
        assert!(size_bytes("165,9G").is_some_and(|bytes| bytes > 165 << 30));
        assert_eq!(size_bytes("?"), None);
    }
}
