//! Plan d'installation : à partir du disque et du choix fait dans l'assistant, les
//! partitions exactes (en secteurs) à réduire, créer ou formater, écrites dans un fichier
//! `clé=valeur` lu par `ferrite-installation installer`. Le script ne calcule rien : tout
//! est décidé (et testé) ici.

use super::disks::{Disk, GIB, GPT_BIOS_BOOT, GPT_EFI, GPT_LINUX, MIB, Ntfs, Part, Region, Table, size_text};

/// Place minimale de Ferrite (système, mises à jour, quelques applications).
pub const MIN_ROOT: u64 = 16 * GIB;
/// Place conseillée.
pub const RECOMMENDED_ROOT: u64 = 40 * GIB;
/// Partition EFI créée quand le disque n'en a pas.
const ESP_SIZE: u64 = 512 * MIB;
/// Partition « BIOS boot » de GRUB (disque GPT démarré en mode BIOS).
const BIOS_BOOT_SIZE: u64 = MIB;
/// Place libre laissée à Windows en plus de ce qu'il occupe : ses mises à jour en ont
/// besoin.
pub const WINDOWS_MARGIN: u64 = 10 * GIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Firmware {
    Efi,
    Bios,
}

impl Firmware {
    pub fn current() -> Self {
        if std::path::Path::new("/sys/firmware/efi").exists() {
            Self::Efi
        } else {
            Self::Bios
        }
    }
}

/// Où installer Ferrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Réduire la partition Windows `part` (indice dans `Disk::parts`) pour donner
    /// `ferrite` octets à Ferrite.
    Beside { part: usize, ferrite: u64 },
    /// Un espace libre du disque.
    Free { start: u64, sectors: u64 },
    /// Effacer tout le disque.
    Erase,
    /// Effacer la partition `part` et y mettre Ferrite.
    Replace { part: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u64,
    pub sectors: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Root {
    New(Span),
    /// Partition existante, effacée ; son type devient « Linux ».
    Existing {
        path: String,
        number: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Esp {
    /// Démarrage BIOS : pas de partition EFI.
    None,
    /// Partition EFI existante, gardée telle quelle (Windows y a son chargeur).
    Existing(String),
    New(Span),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shrink {
    pub path: String,
    pub number: u32,
    /// Nouvelle taille de la partition, en secteurs (et en octets pour ntfsresize).
    pub sectors: u64,
    pub bytes: u64,
    /// Taille d'avant, en octets (pour le résumé).
    pub before: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub disk: String,
    pub firmware: Firmware,
    /// Table utilisée (après création éventuelle).
    pub table: Table,
    /// Nouvelle table : tout le disque est effacé.
    pub new_table: bool,
    pub shrink: Option<Shrink>,
    pub esp: Esp,
    pub bios_boot: Option<Span>,
    pub root: Root,
    /// Taille de la partition de Ferrite, en octets.
    pub root_bytes: u64,
}

/// Bornes du curseur « à côté de Windows » : place minimale, maximale et proposée pour
/// Ferrite, en octets ; `None` si la partition ne peut pas être réduite assez.
pub fn beside_limits(disk: &Disk, part: &Part, firmware: Firmware) -> Option<(u64, u64, u64)> {
    let Some(Ntfs::Shrinkable(used)) = part.ntfs else {
        return None;
    };
    let windows_floor = used + WINDOWS_MARGIN;
    let floor_end = disk.align_up(part.start + windows_floor.div_ceil(disk.sector));
    let space_end = disk.next_start_after(part);
    let max = space_end.checked_sub(floor_end)? * disk.sector;
    let min = MIN_ROOT + extras(disk, firmware);
    if max < min {
        return None;
    }
    let shrinkable = part.bytes(disk.sector).saturating_sub(windows_floor);
    let proposed = (shrinkable / 2).max(RECOMMENDED_ROOT).clamp(min, max);
    // Arrondi au Go : le curseur avance de Go en Go.
    let round = |bytes: u64| bytes / GIB * GIB;
    Some((
        min.div_ceil(GIB) * GIB,
        round(max),
        round(proposed).max(min.div_ceil(GIB) * GIB).min(round(max)),
    ))
}

/// Place prise en plus de la racine dans un nouvel espace (partition EFI ou BIOS boot à
/// créer).
fn extras(disk: &Disk, firmware: Firmware) -> u64 {
    match firmware {
        Firmware::Efi if disk.efi().is_none() => ESP_SIZE,
        Firmware::Bios if disk.table != Table::Msdos && !disk.parts.iter().any(Part::is_bios_boot) => {
            BIOS_BOOT_SIZE
        }
        _ => 0,
    }
}

/// Construit le plan, ou explique pourquoi ce choix n'est pas possible.
pub fn build(disk: &Disk, choice: Choice, firmware: Firmware) -> Result<Plan, String> {
    if disk.live {
        return Err("C'est la clé de Ferrite elle-même : choisissez un autre disque.".into());
    }
    let mut plan = Plan {
        disk: disk.path.clone(),
        firmware,
        table: disk.table,
        new_table: false,
        shrink: None,
        esp: Esp::None,
        bios_boot: None,
        root: Root::Existing {
            path: String::new(),
            number: 0,
        },
        root_bytes: 0,
    };
    let space = match choice {
        Choice::Erase => {
            plan.new_table = true;
            plan.table = Table::Gpt;
            Some(Span {
                start: disk.first_usable(),
                sectors: disk.end_usable() - disk.first_usable(),
            })
        }
        Choice::Free { start, sectors } => {
            let inside = disk.regions().into_iter().any(|region| match region {
                Region::Free {
                    start: free,
                    sectors: length,
                } => start >= free && start + sectors <= free + length,
                Region::Part(_) => false,
            });
            if !inside {
                return Err("Cet espace n'est plus libre : relancez la recherche des disques.".into());
            }
            if disk.table == Table::None {
                plan.new_table = true;
                plan.table = Table::Gpt;
            }
            Some(Span { start, sectors })
        }
        Choice::Beside { part, ferrite } => {
            let windows = disk.parts.get(part).ok_or("Partition introuvable.")?;
            let (min, max, _) = beside_limits(disk, windows, firmware)
                .ok_or("Cette partition ne peut pas être réduite assez pour Ferrite.")?;
            let ferrite = ferrite.clamp(min, max);
            let space_end = disk.next_start_after(windows);
            let start = disk.align_down(space_end - ferrite / disk.sector);
            let sectors = start - windows.start;
            if sectors > windows.sectors {
                return Err("La partition Windows est déjà plus petite que prévu.".into());
            }
            plan.shrink = Some(Shrink {
                path: windows.path.clone(),
                number: windows.number,
                sectors,
                bytes: sectors * disk.sector,
                before: windows.bytes(disk.sector),
            });
            Some(Span {
                start,
                sectors: space_end - start,
            })
        }
        Choice::Replace { part } => {
            let target = disk.parts.get(part).ok_or("Partition introuvable.")?;
            replace_allowed(target, disk.sector)?;
            plan.root = Root::Existing {
                path: target.path.clone(),
                number: target.number,
            };
            plan.root_bytes = target.bytes(disk.sector);
            None
        }
    };

    // Démarrage : partition EFI (mode UEFI) ou BIOS boot (mode BIOS sur un disque GPT).
    let mut cursor = space.map(|span| span.start);
    let mut take = |bytes: u64, what: &str| -> Result<Span, String> {
        let Some(start) = cursor else {
            return Err(format!(
                "Il manque une partition {what} sur ce disque : choisissez plutôt un espace libre, ou effacez le disque."
            ));
        };
        let sectors = disk.align_up(bytes.div_ceil(disk.sector));
        cursor = Some(start + sectors);
        Ok(Span { start, sectors })
    };
    match firmware {
        Firmware::Efi => {
            plan.esp = match disk.efi() {
                Some(esp) if !plan.new_table => Esp::Existing(esp.path.clone()),
                _ => Esp::New(take(ESP_SIZE, "EFI")?),
            };
        }
        Firmware::Bios => {
            let has_bios_boot = !plan.new_table && disk.parts.iter().any(Part::is_bios_boot);
            if plan.table == Table::Gpt && !has_bios_boot {
                plan.bios_boot = Some(take(BIOS_BOOT_SIZE, "« BIOS boot »")?);
            }
        }
    }
    if let (Some(span), Some(start)) = (space, cursor) {
        let end = span.start + span.sectors;
        if end <= start {
            return Err("Il n'y a pas assez de place.".into());
        }
        plan.root = Root::New(Span {
            start,
            sectors: end - start,
        });
        plan.root_bytes = (end - start) * disk.sector;
    }
    if plan.root_bytes < MIN_ROOT {
        return Err(format!(
            "Il faut au moins {} pour Ferrite ({} ici).",
            size_text(MIN_ROOT),
            size_text(plan.root_bytes)
        ));
    }

    // Table MBR : quatre partitions principales au plus, pas de partition logique créée.
    if plan.table == Table::Msdos && !plan.new_table {
        let created =
            usize::from(matches!(plan.root, Root::New(_))) + usize::from(matches!(plan.esp, Esp::New(_)));
        let primaries = disk.parts.iter().filter(|part| part.number <= 4).count();
        if primaries + created > 4 {
            return Err(
                "Ce disque (table MBR) a déjà quatre partitions principales : choisissez une partition à remplacer."
                    .into(),
            );
        }
    }
    Ok(plan)
}

/// Une partition peut-elle être effacée pour Ferrite ?
pub fn replace_allowed(part: &Part, sector: u64) -> Result<(), String> {
    if part.is_technical() {
        return Err("Partition nécessaire au démarrage ou à Windows : on n'y touche pas.".into());
    }
    if !part.mountpoint.is_empty() {
        return Err("Partition en cours d'utilisation.".into());
    }
    if part.bytes(sector) < MIN_ROOT {
        return Err(format!("Trop petite : il faut au moins {}.", size_text(MIN_ROOT)));
    }
    Ok(())
}

/// Ton d'une phrase du résumé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Warning,
    Danger,
}

impl Plan {
    /// Ce qui va se passer, en phrases simples (écran « Vérification »).
    pub fn summary(&self, disk: &Disk) -> Vec<(Tone, String)> {
        let mut lines = Vec::new();
        if self.new_table {
            let systems = disk.systems();
            let lost = if systems.is_empty() {
                "tous les fichiers qu'il contient".to_owned()
            } else {
                format!("{} et tous les fichiers", systems.join(", "))
            };
            lines.push((
                Tone::Danger,
                format!("Tout le disque « {} » sera effacé : {lost}.", disk.title()),
            ));
        }
        if let Some(shrink) = &self.shrink {
            lines.push((
                Tone::Warning,
                format!(
                    "Windows passera de {} à {}. Ses fichiers sont gardés.",
                    size_text(shrink.before),
                    size_text(shrink.bytes)
                ),
            ));
        }
        if let Root::Existing { path, .. } = &self.root {
            let title = disk
                .parts
                .iter()
                .find(|part| &part.path == path)
                .map_or_else(|| path.clone(), Part::title);
            lines.push((
                Tone::Danger,
                format!(
                    "La partition « {title} » ({}) sera effacée.",
                    size_text(self.root_bytes)
                ),
            ));
        }
        lines.push((
            Tone::Info,
            format!("Ferrite recevra {}.", size_text(self.root_bytes)),
        ));
        if matches!(self.root, Root::New(_)) && !self.new_table && self.shrink.is_none() {
            lines.push((
                Tone::Info,
                "Rien n'est effacé : Ferrite prend l'espace libre.".into(),
            ));
        }
        let others: Vec<String> = if self.new_table {
            Vec::new()
        } else {
            disk.systems()
        };
        if others.is_empty() {
            lines.push((Tone::Info, "Ferrite démarrera tout seul à l'allumage.".into()));
        } else {
            lines.push((
                Tone::Info,
                format!(
                    "À l'allumage, un menu proposera Ferrite ou {}.",
                    others.join(", ")
                ),
            ));
        }
        lines
    }

    /// Fichier lu par `ferrite-installation installer`.
    pub fn to_file(&self, account: &Account, locale: &Locale) -> String {
        let gpt = self.table == Table::Gpt;
        let (linux, efi) = if gpt { (GPT_LINUX, GPT_EFI) } else { ("83", "ef") };
        let mut out = String::new();
        let mut line = |key: &str, value: &str| {
            out.push_str(key);
            out.push('=');
            out.push_str(&clean(value));
            out.push('\n');
        };
        line("disque", &self.disk);
        line(
            "micrologiciel",
            match self.firmware {
                Firmware::Efi => "efi",
                Firmware::Bios => "bios",
            },
        );
        line(
            "table",
            match (self.new_table, self.table) {
                (false, _) => "garder",
                (true, Table::Msdos) => "dos",
                (true, _) => "gpt",
            },
        );
        if let Some(shrink) = &self.shrink {
            line(
                "reduire",
                &format!(
                    "{}:{}:{}:{}",
                    shrink.path, shrink.number, shrink.sectors, shrink.bytes
                ),
            );
        }
        match &self.esp {
            Esp::None => {}
            Esp::Existing(path) => line("efi", &format!("existante:{path}")),
            Esp::New(span) => line("efi", &format!("nouvelle:{}:{}:{efi}", span.start, span.sectors)),
        }
        if let Some(span) = self.bios_boot {
            line(
                "biosboot",
                &format!("nouvelle:{}:{}:{GPT_BIOS_BOOT}", span.start, span.sectors),
            );
        }
        match &self.root {
            Root::New(span) => line(
                "racine",
                &format!("nouvelle:{}:{}:{linux}", span.start, span.sectors),
            ),
            Root::Existing { path, number } => line("racine", &format!("existante:{path}:{number}:{linux}")),
        }
        line("nom", &account.full_name);
        line("utilisateur", &account.user);
        line("machine", &account.host);
        line("connexion_auto", if account.autologin { "1" } else { "0" });
        line("clavier", locale.keyboard.layout);
        line("variante", locale.keyboard.variant);
        line("fuseau", locale.timezone);
        out
    }
}

/// Valeur d'une ligne : ni retour à la ligne, ni deux-points (champ GECOS de useradd).
fn clean(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).collect()
}

/// Compte créé sur l'ordinateur.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Account {
    pub full_name: String,
    pub user: String,
    pub host: String,
    pub password: String,
    pub autologin: bool,
}

/// Nom de compte proposé à partir du nom complet : « Aïssata Ouédraogo » → « aissata ».
pub fn user_from(full_name: &str) -> String {
    let first = full_name.split_whitespace().next().unwrap_or_default();
    let user: String = fold(first)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    let user = user.trim_start_matches(|c: char| c.is_ascii_digit()).to_owned();
    user.chars().take(32).collect()
}

/// Nom de l'ordinateur proposé : « pc-aissata ».
pub fn host_from(user: &str) -> String {
    if user.is_empty() {
        "ferrite".to_owned()
    } else {
        format!("pc-{user}")
    }
}

/// Lettres accentuées ramenées à leur lettre de base.
fn fold(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
            'À' | 'Â' | 'Ä' | 'Á' | 'Ã' | 'Å' => 'A',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'Î' | 'Ï' | 'Í' | 'Ì' => 'I',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
            'Ô' | 'Ö' | 'Ó' | 'Ò' | 'Õ' => 'O',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'Ù' | 'Û' | 'Ü' | 'Ú' => 'U',
            'ç' => 'c',
            'Ç' => 'C',
            'ñ' => 'n',
            'Ñ' => 'N',
            'ÿ' => 'y',
            other => other,
        })
        .collect()
}

/// Comptes réservés au système.
const RESERVED: &[&str] = &[
    "root",
    "daemon",
    "bin",
    "sys",
    "sync",
    "games",
    "man",
    "lp",
    "mail",
    "news",
    "uucp",
    "proxy",
    "www-data",
    "backup",
    "list",
    "irc",
    "nobody",
    "systemd-network",
    "messagebus",
    "ferrite",
    "_greetd",
    "polkitd",
    "sudo",
    "admin",
    "users",
    "audio",
    "video",
];

impl Account {
    /// Ce qui manque ou ne va pas, pour griser « Continuer ».
    pub fn problem(&self, confirm: &str) -> Option<&'static str> {
        if self.full_name.trim().is_empty() {
            return Some("Écrivez votre nom.");
        }
        let user_ok = self.user.len() <= 32
            && self.user.starts_with(|c: char| c.is_ascii_lowercase())
            && self
                .user
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !user_ok {
            return Some("Le nom du compte : lettres minuscules sans accent et chiffres, sans espace.");
        }
        if RESERVED.contains(&self.user.as_str()) {
            return Some("Ce nom de compte est réservé : choisissez-en un autre.");
        }
        let host_ok = !self.host.is_empty()
            && self.host.len() <= 63
            && !self.host.starts_with('-')
            && !self.host.ends_with('-')
            && self
                .host
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !host_ok {
            return Some("Le nom de l'ordinateur : lettres minuscules, chiffres et tirets.");
        }
        if self.password.is_empty() {
            return Some("Choisissez un mot de passe.");
        }
        if self.password != confirm {
            return Some("Les deux mots de passe ne sont pas les mêmes.");
        }
        if self.password.contains(['\n', '\r']) {
            return Some("Le mot de passe ne peut pas contenir de retour à la ligne.");
        }
        None
    }
}

/// Disposition du clavier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keyboard {
    pub name: &'static str,
    pub layout: &'static str,
    pub variant: &'static str,
}

impl Keyboard {
    /// Valeur du réglage `clavier` : « fr », ou « ch(fr) » avec une variante.
    pub fn setting(&self) -> String {
        if self.variant.is_empty() {
            self.layout.to_owned()
        } else {
            format!("{}({})", self.layout, self.variant)
        }
    }
}

pub const KEYBOARDS: &[Keyboard] = &[
    Keyboard {
        name: "Français (AZERTY)",
        layout: "fr",
        variant: "",
    },
    Keyboard {
        name: "Belge (AZERTY)",
        layout: "be",
        variant: "",
    },
    Keyboard {
        name: "Suisse romand (QWERTZ)",
        layout: "ch",
        variant: "fr",
    },
    Keyboard {
        name: "Canadien français",
        layout: "ca",
        variant: "",
    },
    Keyboard {
        name: "Anglais américain (QWERTY)",
        layout: "us",
        variant: "",
    },
    Keyboard {
        name: "Anglais britannique (QWERTY)",
        layout: "gb",
        variant: "",
    },
];

/// Fuseaux horaires proposés : (ville affichée, fuseau).
pub const TIMEZONES: &[(&str, &str)] = &[
    ("Ouagadougou", "Africa/Ouagadougou"),
    ("Abidjan", "Africa/Abidjan"),
    ("Dakar", "Africa/Dakar"),
    ("Bamako", "Africa/Bamako"),
    ("Niamey", "Africa/Niamey"),
    ("Lomé", "Africa/Lome"),
    ("Cotonou", "Africa/Porto-Novo"),
    ("Conakry", "Africa/Conakry"),
    ("Douala, Yaoundé", "Africa/Douala"),
    ("Libreville", "Africa/Libreville"),
    ("Kinshasa", "Africa/Kinshasa"),
    ("Brazzaville", "Africa/Brazzaville"),
    ("Antananarivo", "Indian/Antananarivo"),
    ("Alger", "Africa/Algiers"),
    ("Tunis", "Africa/Tunis"),
    ("Casablanca", "Africa/Casablanca"),
    ("Paris", "Europe/Paris"),
    ("Bruxelles", "Europe/Brussels"),
    ("Genève, Zurich", "Europe/Zurich"),
    ("Luxembourg", "Europe/Luxembourg"),
    ("Montréal", "America/Toronto"),
    ("La Réunion", "Indian/Reunion"),
    ("Guadeloupe, Martinique", "America/Martinique"),
    ("Cayenne", "America/Cayenne"),
    ("Nouméa", "Pacific/Noumea"),
    ("Papeete", "Pacific/Tahiti"),
    ("Temps universel (UTC)", "Etc/UTC"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locale {
    pub keyboard: &'static Keyboard,
    pub timezone: &'static str,
}

impl Default for Locale {
    fn default() -> Self {
        // Le fuseau du système live, s'il est dans la liste.
        let current = std::fs::read_to_string("/etc/timezone").unwrap_or_default();
        let timezone = TIMEZONES
            .iter()
            .find(|(_, zone)| *zone == current.trim())
            .map_or(TIMEZONES[0].1, |(_, zone)| zone);
        // Le clavier choisi dans la session (assistant de bienvenue, Paramètres), s'il est
        // dans la liste.
        let settings = ferrite_system::config::Settings::load();
        let current = settings.keyboard_layout();
        let keyboard = KEYBOARDS
            .iter()
            .find(|keyboard| keyboard.setting() == current)
            .unwrap_or(&KEYBOARDS[0]);
        Self { keyboard, timezone }
    }
}

#[cfg(test)]
mod tests {
    use super::super::disks::tests::thinkbook;
    use super::*;

    fn account() -> Account {
        Account {
            full_name: "Aïssata Ouédraogo".into(),
            user: user_from("Aïssata Ouédraogo"),
            host: host_from("aissata"),
            password: "secret".into(),
            autologin: false,
        }
    }

    #[test]
    fn a_cote_de_windows_reduit_sans_deborder() {
        let disks = thinkbook();
        let nvme = &disks[1];
        let windows = &nvme.parts[1];
        let (min, max, proposed) = beside_limits(nvme, windows, Firmware::Efi).unwrap();
        assert_eq!(min, MIN_ROOT);
        assert!(min <= proposed && proposed <= max);
        // Windows garde au moins ce qu'il occupe plus la marge.
        let windows_bytes = windows.bytes(512);
        assert!(windows_bytes - max >= 120_000_000_000 + WINDOWS_MARGIN);

        let plan = build(
            nvme,
            Choice::Beside {
                part: 1,
                ferrite: 60 * GIB,
            },
            Firmware::Efi,
        )
        .unwrap();
        let shrink = plan.shrink.clone().unwrap();
        assert_eq!(shrink.number, 2);
        assert_eq!(shrink.bytes, shrink.sectors * 512);
        let Root::New(root) = plan.root else {
            panic!("racine attendue")
        };
        // La racine commence après la partition Windows réduite, alignée, et finit avant
        // la partition de récupération.
        assert_eq!(root.start % 2048, 0);
        assert!(root.start >= windows.start + shrink.sectors);
        assert_eq!(root.start + root.sectors, nvme.parts[2].start);
        assert_eq!(plan.esp, Esp::Existing("/dev/nvme0n1p4".into()));
        assert!(plan.root_bytes >= 60 * GIB && plan.root_bytes < 61 * GIB);
        let file = plan.to_file(&account(), &Locale::default());
        assert!(file.contains("table=garder\n"));
        assert!(file.contains(&format!(
            "reduire=/dev/nvme0n1p2:2:{}:{}\n",
            shrink.sectors, shrink.bytes
        )));
        assert!(file.contains("efi=existante:/dev/nvme0n1p4\n"));
        assert!(file.contains(&format!(
            "racine=nouvelle:{}:{}:{GPT_LINUX}\n",
            root.start, root.sectors
        )));
        assert!(file.contains("utilisateur=aissata\n") && file.contains("machine=pc-aissata\n"));
    }

    #[test]
    fn taille_hors_bornes_ramenee_aux_bornes() {
        let disks = thinkbook();
        let (_, max, _) = beside_limits(&disks[1], &disks[1].parts[1], Firmware::Efi).unwrap();
        let plan = build(
            &disks[1],
            Choice::Beside {
                part: 1,
                ferrite: 10_000 * GIB,
            },
            Firmware::Efi,
        )
        .unwrap();
        assert!(plan.root_bytes <= max + MIB);
        let plan = build(&disks[1], Choice::Beside { part: 1, ferrite: 0 }, Firmware::Efi).unwrap();
        assert!(plan.root_bytes >= MIN_ROOT);
    }

    #[test]
    fn windows_en_veille_ou_illisible_non_reductible() {
        let disks = thinkbook();
        assert!(beside_limits(&disks[0], &disks[0].parts[4], Firmware::Efi).is_none());
        assert!(
            build(
                &disks[0],
                Choice::Beside {
                    part: 4,
                    ferrite: 40 * GIB
                },
                Firmware::Efi
            )
            .is_err()
        );
    }

    #[test]
    fn effacer_le_disque_cree_efi_et_racine() {
        let disks = thinkbook();
        let plan = build(&disks[1], Choice::Erase, Firmware::Efi).unwrap();
        assert!(plan.new_table);
        let Esp::New(esp) = plan.esp.clone() else {
            panic!("EFI attendue")
        };
        assert_eq!((esp.start, esp.sectors), (2048, 512 * 2048));
        let Root::New(root) = plan.root.clone() else {
            panic!()
        };
        assert_eq!(root.start, esp.start + esp.sectors);
        assert!(root.start + root.sectors <= disks[1].sectors - 34);
        assert!(
            plan.summary(&disks[1])
                .iter()
                .any(|(tone, text)| *tone == Tone::Danger
                    && text.contains("Windows")
                    && text.contains("Fedora"))
        );
        let file = plan.to_file(&account(), &Locale::default());
        assert!(file.contains("table=gpt\n"));

        let bios = build(&disks[1], Choice::Erase, Firmware::Bios).unwrap();
        assert_eq!(bios.esp, Esp::None);
        assert_eq!(bios.bios_boot.map(|span| span.sectors), Some(2048));
    }

    #[test]
    fn remplacer_une_partition() {
        let disks = thinkbook();
        // Fedora (btrfs) : effacée, son type devient Linux ; l'EFI de Windows est gardée.
        let plan = build(&disks[1], Choice::Replace { part: 5 }, Firmware::Efi).unwrap();
        assert_eq!(
            plan.root,
            Root::Existing {
                path: "/dev/nvme0n1p6".into(),
                number: 6
            }
        );
        assert!(
            plan.summary(&disks[1])
                .iter()
                .any(|(tone, _)| *tone == Tone::Danger)
        );
        // EFI, réservée, récupération, trop petite : refusées.
        for part in [0, 2, 3, 4] {
            assert!(
                build(&disks[1], Choice::Replace { part }, Firmware::Efi).is_err(),
                "{part}"
            );
        }
        // La racine du système en marche : refusée.
        assert!(build(&disks[0], Choice::Replace { part: 2 }, Firmware::Efi).is_err());
        // Disque GPT sans BIOS boot, démarré en BIOS : pas de place pour GRUB.
        assert!(build(&disks[1], Choice::Replace { part: 5 }, Firmware::Bios).is_err());
    }

    #[test]
    fn espace_libre_et_cle_live() {
        let mut disks = thinkbook();
        disks[0].parts.pop();
        let (start, sectors) = disks[0].largest_free().unwrap();
        let plan = build(&disks[0], Choice::Free { start, sectors }, Firmware::Efi).unwrap();
        assert_eq!(plan.esp, Esp::Existing("/dev/sda1".into()));
        assert_eq!(plan.root, Root::New(Span { start, sectors }));
        assert!(
            plan.summary(&disks[0])
                .iter()
                .any(|(_, text)| text.contains("Rien n'est effacé"))
        );
        assert!(build(&disks[2], Choice::Erase, Firmware::Efi).is_err());
        assert!(
            build(
                &disks[0],
                Choice::Free {
                    start: 2048,
                    sectors: 4096
                },
                Firmware::Efi
            )
            .is_err()
        );
    }

    #[test]
    fn noms_de_compte() {
        assert_eq!(user_from("Aïssata Ouédraogo"), "aissata");
        assert_eq!(user_from("  Jean-Éric  "), "jeaneric");
        assert_eq!(user_from("2Pac"), "pac");
        let mut good = account();
        assert_eq!(good.problem("secret"), None);
        assert!(good.problem("autre").is_some());
        good.user = "Marie".into();
        assert!(good.problem("secret").is_some());
        good.user = "root".into();
        assert!(good.problem("secret").is_some());
        let mut host = account();
        host.host = "-pc".into();
        assert!(host.problem("secret").is_some());
    }

    #[test]
    fn valeurs_sans_retour_a_la_ligne() {
        let disks = thinkbook();
        let plan = build(&disks[1], Choice::Erase, Firmware::Efi).unwrap();
        let mut account = account();
        account.full_name = "Nom\nracine=/dev/sda1".into();
        let file = plan.to_file(&account, &Locale::default());
        assert!(file.contains("nom=Nomracine=/dev/sda1\n"));
        assert_eq!(file.lines().filter(|line| line.starts_with("racine=")).count(), 1);
    }
}
