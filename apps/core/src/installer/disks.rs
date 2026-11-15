//! Disques de l'ordinateur, lus par `lsblk` (sans droits d'administrateur), et ce que la
//! sonde (`ferrite-installation sonder`, en administrateur) y a trouvé : systèmes installés
//! (os-prober) et taille minimale des partitions Windows (ntfsresize).
//!
//! Tout est compté en secteurs du disque (`sector` octets), comme dans la table des
//! partitions ; les tailles affichées sont en Go (puissances de 1024, comme Windows).

use std::collections::HashMap;

/// Type GPT d'une partition EFI.
pub const GPT_EFI: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
/// Type GPT « BIOS boot » (GRUB sur un disque GPT démarré en mode BIOS).
pub const GPT_BIOS_BOOT: &str = "21686148-6449-6e6f-744e-656564454649";
/// Type GPT des partitions Linux.
pub const GPT_LINUX: &str = "0fc63daf-8483-4772-8e79-3d69d8477de4";
/// Partition réservée de Microsoft (MSR).
const GPT_MSR: &str = "e3c9e316-0b5c-4db8-817d-f92df00215ae";
/// Partition de récupération de Windows.
const GPT_WINDOWS_RECOVERY: &str = "de94bba4-06d1-4d40-a16a-bfd50179d6ac";
/// Données de base Microsoft (NTFS, FAT…).
const GPT_MS_DATA: &str = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7";

/// Un mébioctet : l'alignement des partitions créées.
pub const MIB: u64 = 1024 * 1024;
/// Un Go affiché (Gio, comme l'Explorateur de Windows).
pub const GIB: u64 = 1024 * MIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Gpt,
    Msdos,
    /// Disque neuf, sans table des partitions.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    pub path: String,
    pub model: String,
    /// Taille en secteurs.
    pub sectors: u64,
    /// Taille d'un secteur logique, en octets.
    pub sector: u64,
    pub table: Table,
    pub removable: bool,
    pub transport: String,
    /// La clé USB de Ferrite elle-même : on ne peut pas s'installer dessus.
    pub live: bool,
    /// Partitions, dans l'ordre du disque.
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub path: String,
    pub number: u32,
    /// Premier secteur et taille en secteurs.
    pub start: u64,
    pub sectors: u64,
    pub fstype: String,
    pub label: String,
    pub parttype: String,
    pub partlabel: String,
    pub mountpoint: String,
    /// Octets occupés, connus seulement si la partition est montée.
    pub used: Option<u64>,
    /// Système trouvé par os-prober (« Windows », « Fedora Linux 42 »…).
    pub system: Option<String>,
    /// Ce que ntfsresize dit d'une partition Windows.
    pub ntfs: Option<Ntfs>,
}

/// Une partition NTFS vue par ntfsresize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ntfs {
    /// Peut être réduite jusqu'à ce nombre d'octets.
    Shrinkable(u64),
    /// Windows est en veille prolongée (démarrage rapide) : on n'y touche pas.
    Hibernated,
    /// Chiffrée par BitLocker.
    BitLocker,
    /// Erreur de lecture (disque à vérifier dans Windows).
    Unreadable,
}

/// Une zone du disque : une partition, ou un espace libre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Part(usize),
    Free { start: u64, sectors: u64 },
}

/// Ce que la sonde a trouvé, par partition.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    /// Système par partition. Une entrée EFI de Windows est rangée sous la partition EFI.
    pub systems: HashMap<String, String>,
    pub ntfs: HashMap<String, Ntfs>,
}

impl Part {
    fn kind(&self) -> &str {
        self.parttype.as_str()
    }

    pub fn is_efi(&self) -> bool {
        self.kind() == GPT_EFI || self.kind() == "0xef"
    }

    pub fn is_bios_boot(&self) -> bool {
        self.kind() == GPT_BIOS_BOOT
    }

    /// Partition MBR étendue (conteneur des partitions logiques).
    pub fn is_extended(&self) -> bool {
        matches!(self.kind(), "0x5" | "0xf" | "0x85")
    }

    /// Partition qu'on ne propose jamais d'effacer seule : démarrage, réservée,
    /// récupération, conteneur.
    pub fn is_technical(&self) -> bool {
        self.is_efi()
            || self.is_bios_boot()
            || self.is_extended()
            || matches!(self.kind(), GPT_MSR | GPT_WINDOWS_RECOVERY | "0x27")
    }

    pub fn bytes(&self, sector: u64) -> u64 {
        self.sectors * sector
    }

    /// Partition Windows de données (NTFS), éventuellement chiffrée.
    pub fn is_windows_data(&self) -> bool {
        (self.fstype == "ntfs" || self.fstype == "BitLocker")
            && !self.is_technical()
            && (self.kind() == GPT_MS_DATA || self.kind() == "0x7" || self.kind().is_empty())
    }

    /// Nom à montrer : le système, sinon l'étiquette, sinon la nature.
    pub fn title(&self) -> String {
        if let Some(system) = &self.system {
            return system.clone();
        }
        let nature = match self.kind() {
            GPT_EFI | "0xef" => "Démarrage (EFI)",
            GPT_BIOS_BOOT => "Démarrage BIOS",
            GPT_MSR => "Réservée par Windows",
            GPT_WINDOWS_RECOVERY | "0x27" => "Récupération de Windows",
            "0x5" | "0xf" | "0x85" => "Partition étendue",
            _ => match self.fstype.as_str() {
                "ntfs" => "Données Windows",
                "BitLocker" => "Windows chiffré (BitLocker)",
                "vfat" | "exfat" => "Données (FAT)",
                "swap" => "Échange Linux",
                "ext4" | "ext3" | "ext2" | "btrfs" | "xfs" => "Linux",
                "crypto_LUKS" => "Linux chiffré",
                "" => "Partition vide",
                other => other,
            },
        };
        if self.label.is_empty() || self.is_technical() {
            nature.to_owned()
        } else {
            format!("{nature} « {} »", self.label)
        }
    }
}

impl Disk {
    pub fn bytes(&self) -> u64 {
        self.sectors * self.sector
    }

    /// Secteurs par mébioctet (alignement).
    pub fn align(&self) -> u64 {
        (MIB / self.sector).max(1)
    }

    /// Premier secteur utilisable pour une nouvelle partition.
    pub fn first_usable(&self) -> u64 {
        self.align()
    }

    /// Fin (exclue) de la zone utilisable : on laisse le dernier Mio, où GPT range sa
    /// copie de secours.
    pub fn end_usable(&self) -> u64 {
        self.sectors.saturating_sub(self.align())
    }

    pub fn align_up(&self, sector: u64) -> u64 {
        sector.div_ceil(self.align()) * self.align()
    }

    pub fn align_down(&self, sector: u64) -> u64 {
        sector / self.align() * self.align()
    }

    /// Partitions et espaces libres (d'au moins 16 Mio), dans l'ordre du disque.
    pub fn regions(&self) -> Vec<Region> {
        let mut regions = Vec::new();
        let mut cursor = self.first_usable();
        let free = |from: u64, to: u64, regions: &mut Vec<Region>| {
            let start = self.align_up(from);
            let end = self.align_down(to);
            if end > start && (end - start) * self.sector >= 16 * MIB {
                regions.push(Region::Free {
                    start,
                    sectors: end - start,
                });
            }
        };
        // Les partitions logiques sont dans l'étendue : on ne compte l'espace qu'entre les
        // partitions de premier niveau.
        let extended: Vec<(u64, u64)> = self
            .parts
            .iter()
            .filter(|part| part.is_extended())
            .map(|part| (part.start, part.start + part.sectors))
            .collect();
        for (index, part) in self.parts.iter().enumerate() {
            let inside = extended
                .iter()
                .any(|(from, to)| !part.is_extended() && part.start >= *from && part.start < *to);
            if !inside {
                free(cursor, part.start, &mut regions);
                cursor = cursor.max(part.start + part.sectors);
            }
            regions.push(Region::Part(index));
        }
        free(cursor, self.end_usable(), &mut regions);
        regions
    }

    /// Le plus grand espace libre.
    pub fn largest_free(&self) -> Option<(u64, u64)> {
        self.regions()
            .into_iter()
            .filter_map(|region| match region {
                Region::Free { start, sectors } => Some((start, sectors)),
                Region::Part(_) => None,
            })
            .max_by_key(|(_, sectors)| *sectors)
    }

    /// Premier secteur après `part` qui appartient à une autre partition (ou la fin
    /// utilisable du disque) : jusqu'où une partition Windows réduite libère de la place.
    pub fn next_start_after(&self, part: &Part) -> u64 {
        self.parts
            .iter()
            .filter(|other| other.start > part.start && !other.is_extended())
            .map(|other| other.start)
            .min()
            .unwrap_or_else(|| self.end_usable())
            .min(self.end_usable())
    }

    /// Le disque contient-il Windows ?
    pub fn has_windows(&self) -> bool {
        self.parts.iter().any(|part| {
            part.system
                .as_deref()
                .is_some_and(|system| system.contains("Windows"))
        })
    }

    /// Partition EFI existante (la première).
    pub fn efi(&self) -> Option<&Part> {
        self.parts.iter().find(|part| part.is_efi())
    }

    /// Nom à montrer : « Disque de 477 Go (Micron MTFDKBA512TFH) ».
    pub fn title(&self) -> String {
        let size = size_text(self.bytes());
        let kind = if self.removable || self.transport == "usb" {
            "Disque externe"
        } else {
            "Disque"
        };
        if self.model.is_empty() {
            format!("{kind} de {size}")
        } else {
            format!("{kind} de {size} ({})", self.model)
        }
    }

    /// Systèmes installés sur ce disque, sans doublon.
    pub fn systems(&self) -> Vec<String> {
        let mut systems: Vec<String> = Vec::new();
        for system in self.parts.iter().filter_map(|part| part.system.clone()) {
            if !systems.contains(&system) {
                systems.push(system);
            }
        }
        systems
    }
}

/// Taille lisible : « 512 Mo », « 12,5 Go », « 477 Go », « 1,8 To ».
pub fn size_text(bytes: u64) -> String {
    let gib = bytes as f64 / GIB as f64;
    if gib >= 1024.0 {
        return format!("{} To", decimal(gib / 1024.0));
    }
    if gib >= 1.0 {
        return if gib >= 100.0 {
            format!("{gib:.0} Go")
        } else {
            format!("{} Go", decimal(gib))
        };
    }
    format!("{} Mo", (bytes as f64 / MIB as f64).round())
}

/// Un chiffre après la virgule (française), sauf s'il vaut zéro.
fn decimal(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0")
        .map_or_else(|| text.replace('.', ","), str::to_owned)
}

/// Colonnes demandées à `lsblk -P -b -o`.
pub const LSBLK_COLUMNS: &str = "PATH,PKNAME,TYPE,SIZE,START,PARTN,LOG-SEC,FSTYPE,LABEL,PARTTYPE,PARTLABEL,PTTYPE,MOUNTPOINT,MODEL,RM,RO,TRAN,FSUSED";

/// Lit la sortie de `lsblk -P -b -o LSBLK_COLUMNS`.
pub fn parse_lsblk(text: &str) -> Vec<Disk> {
    let rows: Vec<HashMap<String, String>> = text.lines().map(pairs).collect();
    let get = |row: &HashMap<String, String>, key: &str| row.get(key).cloned().unwrap_or_default();
    let number = |row: &HashMap<String, String>, key: &str| get(row, key).trim().parse::<u64>().unwrap_or(0);
    let mut disks: Vec<Disk> = Vec::new();
    for row in rows.iter().filter(|row| get(row, "TYPE") == "disk") {
        let path = get(row, "PATH");
        let name = path.trim_start_matches("/dev/").to_owned();
        if name.starts_with("zram") || name.starts_with("loop") || get(row, "RO") == "1" {
            continue;
        }
        let sector = number(row, "LOG-SEC").max(512);
        let table = match get(row, "PTTYPE").as_str() {
            "gpt" => Table::Gpt,
            "dos" => Table::Msdos,
            _ => Table::None,
        };
        let mut live = get(row, "MOUNTPOINT").starts_with("/run/live");
        let mut parts = Vec::new();
        for part in rows
            .iter()
            .filter(|part| get(part, "TYPE") == "part" && get(part, "PKNAME") == name)
        {
            let mountpoint = get(part, "MOUNTPOINT");
            live |= mountpoint.starts_with("/run/live");
            parts.push(Part {
                path: get(part, "PATH"),
                number: number(part, "PARTN") as u32,
                start: number(part, "START"),
                sectors: number(part, "SIZE") / sector,
                fstype: get(part, "FSTYPE"),
                label: get(part, "LABEL"),
                parttype: get(part, "PARTTYPE").to_lowercase(),
                partlabel: get(part, "PARTLABEL"),
                mountpoint,
                used: get(part, "FSUSED").trim().parse().ok(),
                system: None,
                ntfs: None,
            });
        }
        parts.sort_by_key(|part| part.start);
        // Disque ISO gravé tel quel sur une clé (iso-hybrid) : système de fichiers sans
        // table de partitions.
        live |= get(row, "FSTYPE") == "iso9660";
        disks.push(Disk {
            path,
            model: get(row, "MODEL").trim().to_owned(),
            sectors: number(row, "SIZE") / sector,
            sector,
            table,
            removable: get(row, "RM") == "1",
            transport: get(row, "TRAN"),
            live,
            parts,
        });
    }
    disks
}

/// Une ligne `CLÉ="valeur"` de lsblk -P (les caractères spéciaux sont en `\xNN`).
fn pairs(line: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut rest = line.trim();
    while let Some(equal) = rest.find("=\"") {
        let key = rest[..equal].trim().to_owned();
        let after = &rest[equal + 2..];
        let Some(end) = after.find('"') else { break };
        map.insert(key, unescape(&after[..end]));
        rest = &after[end + 1..];
    }
    map
}

fn unescape(value: &str) -> String {
    let mut bytes = Vec::with_capacity(value.len());
    let raw = value.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'\\'
            && raw.get(index + 1) == Some(&b'x')
            && let Some(hex) = value.get(index + 2..index + 4)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            bytes.push(byte);
            index += 4;
            continue;
        }
        bytes.push(raw[index]);
        index += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Lit la sortie de `ferrite-installation sonder` :
/// `systeme LIGNE_OS_PROBER` et `ntfs CHEMIN OCTETS_MIN ETAT`.
pub fn parse_probe(text: &str) -> Probe {
    let mut probe = Probe::default();
    for line in text.lines() {
        if let Some(entry) = line.strip_prefix("systeme ") {
            // /dev/sda1@/efi/Microsoft/Boot/bootmgfw.efi:Windows Boot Manager:Windows:efi
            // /dev/nvme0n1p6:Fedora Linux 42:Fedora:linux
            let mut fields = entry.split(':');
            let device = fields.next().unwrap_or_default();
            let device = device.split('@').next().unwrap_or_default().to_owned();
            let long = fields.next().unwrap_or_default().trim();
            let short = fields.next().unwrap_or_default().trim();
            let name = if long.contains("Windows") || short == "Windows" {
                "Windows".to_owned()
            } else if long.is_empty() {
                short.to_owned()
            } else {
                long.to_owned()
            };
            if !device.is_empty() && !name.is_empty() {
                probe.systems.insert(device, name);
            }
        } else if let Some(entry) = line.strip_prefix("ntfs ") {
            let fields: Vec<&str> = entry.split_whitespace().collect();
            let [path, min, state] = fields[..] else { continue };
            let state = match state {
                "ok" => match min.parse::<u64>() {
                    Ok(min) if min > 0 => Ntfs::Shrinkable(min),
                    _ => Ntfs::Unreadable,
                },
                "hiberne" => Ntfs::Hibernated,
                "bitlocker" => Ntfs::BitLocker,
                _ => Ntfs::Unreadable,
            };
            probe.ntfs.insert(path.to_owned(), state);
        }
    }
    probe
}

/// Range ce que la sonde a trouvé dans les disques. Une entrée EFI de Windows désigne la
/// partition EFI : Windows est alors la plus grande partition NTFS du même disque.
pub fn apply_probe(disks: &mut [Disk], probe: &Probe) {
    for disk in disks {
        let mut windows_on_efi = false;
        for part in &mut disk.parts {
            part.ntfs = probe.ntfs.get(&part.path).copied();
            if part.fstype == "BitLocker" {
                part.ntfs = Some(Ntfs::BitLocker);
            }
            if let Some(system) = probe.systems.get(&part.path) {
                if part.is_efi() {
                    windows_on_efi |= system == "Windows";
                } else {
                    part.system = Some(system.clone());
                }
            }
        }
        if windows_on_efi
            && !disk
                .parts
                .iter()
                .any(|part| part.system.as_deref() == Some("Windows"))
            && let Some(largest) = disk
                .parts
                .iter_mut()
                .filter(|part| part.is_windows_data())
                .max_by_key(|part| part.sectors)
        {
            largest.system = Some("Windows".to_owned());
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Le SSD NVMe du ThinkBook : Windows, sa récupération, EFI, Fedora (lu le 2026-11-13),
    /// et un disque SATA de données où Ferrite est déjà installé.
    pub const THINKBOOK: &str = r#"PATH="/dev/sda" PKNAME="" TYPE="disk" SIZE="512110190592" START="" PARTN="" LOG-SEC="512" FSTYPE="" LABEL="" PARTTYPE="" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="" MODEL="iCoolax M.2 NGFF 512GB" RM="0" RO="0" TRAN="sata" FSUSED=""
PATH="/dev/sda1" PKNAME="sda" TYPE="part" SIZE="1023410176" START="2048" PARTN="1" LOG-SEC="512" FSTYPE="vfat" LABEL="" PARTTYPE="c12a7328-f81f-11d2-ba4b-00a0c93ec93b" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="" FSUSED=""
PATH="/dev/sda2" PKNAME="sda" TYPE="part" SIZE="178169839616" START="2000896" PARTN="2" LOG-SEC="512" FSTYPE="ext4" LABEL="" PARTTYPE="0fc63daf-8483-4772-8e79-3d69d8477de4" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="" FSUSED=""
PATH="/dev/sda3" PKNAME="sda" TYPE="part" SIZE="13185843200" START="473528320" PARTN="3" LOG-SEC="512" FSTYPE="swap" LABEL="" PARTTYPE="0657fd6d-a4ab-43c4-84e5-0933c84b4f4f" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="[SWAP]" MODEL="" RM="0" RO="0" TRAN="" FSUSED=""
PATH="/dev/sda4" PKNAME="sda" TYPE="part" SIZE="256476446720" START="499281920" PARTN="4" LOG-SEC="512" FSTYPE="ntfs" LABEL="FICHIERS" PARTTYPE="ebd0a0a2-b9e5-4433-87c0-68b6b72699c7" PARTLABEL="Basic data partition" PTTYPE="gpt" MOUNTPOINT="/media/FICHIERS" MODEL="" RM="0" RO="0" TRAN="" FSUSED="206033616896"
PATH="/dev/sda5" PKNAME="sda" TYPE="part" SIZE="63252201472" START="349988864" PARTN="5" LOG-SEC="512" FSTYPE="ext4" LABEL="FERRITE" PARTTYPE="0fc63daf-8483-4772-8e79-3d69d8477de4" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="/" MODEL="" RM="0" RO="0" TRAN="" FSUSED="47537364992"
PATH="/dev/zram0" PKNAME="" TYPE="disk" SIZE="8242855936" START="" PARTN="" LOG-SEC="4096" FSTYPE="swap" LABEL="zram0" PARTTYPE="" PARTLABEL="" PTTYPE="" MOUNTPOINT="[SWAP]" MODEL="" RM="0" RO="0" TRAN="" FSUSED=""
PATH="/dev/nvme0n1" PKNAME="" TYPE="disk" SIZE="512110190592" START="" PARTN="" LOG-SEC="512" FSTYPE="" LABEL="" PARTTYPE="" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="" MODEL="Micron MTFDKBA512TFH" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p1" PKNAME="nvme0n1" TYPE="part" SIZE="16777216" START="2048" PARTN="1" LOG-SEC="512" FSTYPE="" LABEL="" PARTTYPE="e3c9e316-0b5c-4db8-817d-f92df00215ae" PARTLABEL="Microsoft reserved partition" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p2" PKNAME="nvme0n1" TYPE="part" SIZE="313116327936" START="34816" PARTN="2" LOG-SEC="512" FSTYPE="ntfs" LABEL="" PARTTYPE="ebd0a0a2-b9e5-4433-87c0-68b6b72699c7" PARTLABEL="Basic data partition" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p3" PKNAME="nvme0n1" TYPE="part" SIZE="944766976" START="611590144" PARTN="3" LOG-SEC="512" FSTYPE="ntfs" LABEL="" PARTTYPE="de94bba4-06d1-4d40-a16a-bfd50179d6ac" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p4" PKNAME="nvme0n1" TYPE="part" SIZE="1024458752" START="613437440" PARTN="4" LOG-SEC="512" FSTYPE="vfat" LABEL="" PARTTYPE="c12a7328-f81f-11d2-ba4b-00a0c93ec93b" PARTLABEL="EFI System Partition" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p5" PKNAME="nvme0n1" TYPE="part" SIZE="2147483648" START="615438336" PARTN="5" LOG-SEC="512" FSTYPE="ext4" LABEL="" PARTTYPE="0fc63daf-8483-4772-8e79-3d69d8477de4" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/nvme0n1p6" PKNAME="nvme0n1" TYPE="part" SIZE="194857926656" START="619632640" PARTN="6" LOG-SEC="512" FSTYPE="btrfs" LABEL="fedora" PARTTYPE="0fc63daf-8483-4772-8e79-3d69d8477de4" PARTLABEL="" PTTYPE="gpt" MOUNTPOINT="" MODEL="" RM="0" RO="0" TRAN="nvme" FSUSED=""
PATH="/dev/sdb" PKNAME="" TYPE="disk" SIZE="15518924800" START="" PARTN="" LOG-SEC="512" FSTYPE="iso9660" LABEL="FERRITE" PARTTYPE="" PARTLABEL="" PTTYPE="dos" MOUNTPOINT="" MODEL="SanDisk" RM="1" RO="0" TRAN="usb" FSUSED=""
PATH="/dev/sdb1" PKNAME="sdb" TYPE="part" SIZE="2500000000" START="0" PARTN="1" LOG-SEC="512" FSTYPE="iso9660" LABEL="FERRITE" PARTTYPE="0x0" PARTLABEL="" PTTYPE="dos" MOUNTPOINT="/run/live/medium" MODEL="" RM="1" RO="0" TRAN="usb" FSUSED=""
"#;

    pub const THINKBOOK_PROBE: &str =
        "systeme /dev/nvme0n1p4@/efi/Microsoft/Boot/bootmgfw.efi:Windows Boot Manager:Windows:efi
systeme /dev/nvme0n1p6:Fedora Linux 42 (Workstation Edition):Fedora:linux
ntfs /dev/nvme0n1p2 120000000000 ok
ntfs /dev/nvme0n1p3 0 erreur
ntfs /dev/sda4 0 hiberne
";

    pub fn thinkbook() -> Vec<Disk> {
        let mut disks = parse_lsblk(THINKBOOK);
        apply_probe(&mut disks, &parse_probe(THINKBOOK_PROBE));
        disks
    }

    #[test]
    fn disques_lus_sans_zram_et_partitions_dans_l_ordre() {
        let disks = thinkbook();
        let names: Vec<&str> = disks.iter().map(|disk| disk.path.as_str()).collect();
        assert_eq!(names, ["/dev/sda", "/dev/nvme0n1", "/dev/sdb"]);
        let sda: Vec<u32> = disks[0].parts.iter().map(|part| part.number).collect();
        assert_eq!(sda, [1, 2, 5, 3, 4]);
        assert!(disks[2].live && !disks[0].live && !disks[1].live);
        assert_eq!(disks[0].parts[4].used, Some(206_033_616_896));
    }

    #[test]
    fn windows_reconnu_par_son_entree_efi() {
        let disks = thinkbook();
        let nvme = &disks[1];
        assert!(nvme.has_windows());
        assert_eq!(nvme.parts[1].system.as_deref(), Some("Windows"));
        assert_eq!(nvme.parts[1].ntfs, Some(Ntfs::Shrinkable(120_000_000_000)));
        assert_eq!(nvme.parts[5].title(), "Fedora Linux 42 (Workstation Edition)");
        assert_eq!(nvme.parts[0].title(), "Réservée par Windows");
        assert_eq!(nvme.parts[2].title(), "Récupération de Windows");
        assert!(!disks[0].has_windows());
        assert_eq!(disks[0].parts[4].title(), "Données Windows « FICHIERS »");
        assert_eq!(disks[0].parts[4].ntfs, Some(Ntfs::Hibernated));
    }

    #[test]
    fn espaces_libres_entre_les_partitions() {
        let mut disks = thinkbook();
        // Le disque SATA est plein : ses trous font moins de 16 Mio.
        assert_eq!(disks[0].largest_free(), None);
        // Sans sa partition de données, la fin du disque est libre, alignée.
        disks[0].parts.pop();
        let end_sda3 = 473_528_320 + 13_185_843_200 / 512;
        let (start, sectors) = disks[0].largest_free().unwrap();
        assert_eq!(start, end_sda3);
        assert_eq!(start % 2048, 0);
        assert_eq!(start + sectors, disks[0].align_down(disks[0].end_usable()));
        // Le NVMe aussi est plein.
        assert_eq!(disks[1].largest_free(), None);
    }

    #[test]
    fn tailles_lisibles() {
        assert_eq!(size_text(512 * MIB), "512 Mo");
        assert_eq!(size_text(GIB * 25 / 2), "12,5 Go");
        assert_eq!(size_text(512_110_190_592), "477 Go");
        assert_eq!(size_text(2 * 1024 * GIB), "2 To");
        assert_eq!(size_text(40 * GIB), "40 Go");
    }

    #[test]
    fn lsblk_caracteres_echappes() {
        let disks = parse_lsblk(
            r#"PATH="/dev/vda" PKNAME="" TYPE="disk" SIZE="1073741824" LOG-SEC="512" PTTYPE="" MODEL="QEMU\x20HARDDISK" RO="0""#,
        );
        assert_eq!(disks[0].model, "QEMU HARDDISK");
        assert_eq!(disks[0].table, Table::None);
        assert_eq!(disks[0].title(), "Disque de 1 Go (QEMU HARDDISK)");
    }
}
