//! Périphériques amovibles (clés USB, cartes mémoire) par `lsblk` et `udisksctl`
//! (paquet udisks2 de l'image) : liste, montage sans mot de passe et éjection.

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
}

/// Partitions amovibles portant un système de fichiers.
pub fn removable() -> Vec<Device> {
    command::run(
        "lsblk",
        &[
            "-P",
            "-o",
            "PATH,PKNAME,LABEL,SIZE,RM,HOTPLUG,TYPE,FSTYPE,MOUNTPOINT",
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
            if !removable || !usable {
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
                label: if label.is_empty() {
                    format!("Clé {}", field("SIZE"))
                } else {
                    label
                },
                size: field("SIZE"),
                mount: Some(field("MOUNTPOINT"))
                    .filter(|mount| !mount.is_empty())
                    .map(PathBuf::from),
                path,
            })
        })
        .collect()
}

/// Valeur de `CLÉ="valeur"` dans une ligne de `lsblk -P`.
fn pair(line: &str, key: &str) -> Option<String> {
    let start = line.find(&format!("{key}=\""))? + key.len() + 2;
    let end = line[start..].find('"')? + start;
    Some(line[start..end].replace("\\x20", " "))
}

/// Monte la partition et renvoie son point de montage.
pub fn mount(device: &Device) -> Result<PathBuf, String> {
    let output = command::run(
        "udisksctl",
        &["mount", "--no-user-interaction", "-b", &device.path],
    )?;
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

/// Espace libre du système de fichiers qui contient `path`, en octets.
pub fn free_space(path: &std::path::Path) -> Option<u64> {
    let output = command::run("df", &["-B1", "--output=avail", &path.to_string_lossy()]).ok()?;
    output.lines().nth(1)?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_de_lsblk() {
        let output = r#"PATH="/dev/nvme0n1p2" PKNAME="nvme0n1" LABEL="" SIZE="476G" RM="0" HOTPLUG="0" TYPE="part" FSTYPE="ntfs" MOUNTPOINT=""
PATH="/dev/sdb" PKNAME="" LABEL="" SIZE="28,9G" RM="1" HOTPLUG="1" TYPE="disk" FSTYPE="" MOUNTPOINT=""
PATH="/dev/sdb1" PKNAME="sdb" LABEL="MA\x20CLE" SIZE="28,9G" RM="1" HOTPLUG="1" TYPE="part" FSTYPE="vfat" MOUNTPOINT="/media/ferrite/MA CLE"
PATH="/dev/sdc1" PKNAME="sdc" LABEL="" SIZE="4G" RM="0" HOTPLUG="1" TYPE="part" FSTYPE="exfat" MOUNTPOINT=""
"#;
        let devices = parse_lsblk(output);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].label, "MA CLE");
        assert_eq!(devices[0].disk, "/dev/sdb");
        assert_eq!(devices[0].mount, Some(PathBuf::from("/media/ferrite/MA CLE")));
        assert_eq!(devices[1].label, "Clé 4G");
        assert_eq!(devices[1].mount, None);
    }
}
