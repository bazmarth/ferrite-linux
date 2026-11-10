//! Pilotes graphiques à télécharger. L'ISO ne contient pas les microprogrammes des cartes
//! AMD et NVIDIA (55 Mo de paquets, copiés aussi dans l'initrd) : sur un PC qui a une de
//! ces cartes, la Logithèque propose de les installer (rubrique « Pilotes »), et le service
//! des mises à jour l'annonce une fois par semaine tant qu'ils manquent.
//!
//! Détection par `/sys/bus/pci/devices` (classe `0x03…` = carte graphique, fabricant) ;
//! paquet installé si `dpkg` en garde la liste de fichiers.

use std::{env, fs, path::Path};

/// Un pilote proposé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Driver {
    /// Paquet Debian.
    pub package: &'static str,
    /// Fabricant de la carte (« AMD »).
    pub brand: &'static str,
    /// Taille du téléchargement, en Mo (paquet Debian 13).
    pub download_mb: u32,
}

/// Fabricant PCI → pilote.
const DRIVERS: [(u16, Driver); 2] = [
    (
        0x1002,
        Driver {
            package: "firmware-amd-graphics",
            brand: "AMD",
            download_mb: 15,
        },
    ),
    (
        0x10de,
        Driver {
            package: "firmware-nvidia-graphics",
            brand: "NVIDIA",
            download_mb: 40,
        },
    ),
];

/// Fabricants des cartes graphiques présentes (sans doublon).
fn graphics_vendors(pci: &Path) -> Vec<u16> {
    let mut vendors = Vec::new();
    let Ok(entries) = fs::read_dir(pci) else {
        return vendors;
    };
    for entry in entries.flatten() {
        let read = |name: &str| fs::read_to_string(entry.path().join(name)).unwrap_or_default();
        // Classe 0x03 : contrôleur d'affichage (VGA, 3D, autre).
        if !read("class").trim().starts_with("0x03") {
            continue;
        }
        let vendor = read("vendor");
        if let Ok(vendor) = u16::from_str_radix(vendor.trim().trim_start_matches("0x"), 16)
            && !vendors.contains(&vendor)
        {
            vendors.push(vendor);
        }
    }
    vendors
}

/// Pilotes utiles à cette machine et absents.
fn detect(pci: &Path, dpkg_info: &Path) -> Vec<Driver> {
    let vendors = graphics_vendors(pci);
    DRIVERS
        .iter()
        .filter(|(vendor, _)| vendors.contains(vendor))
        .map(|(_, driver)| *driver)
        .filter(|driver| !dpkg_info.join(format!("{}.list", driver.package)).exists())
        .collect()
}

/// Pilotes à proposer. `FERRITE_DEMO_PILOTES=amd,nvidia` les simule (captures).
pub fn missing() -> Vec<Driver> {
    if let Ok(demo) = env::var("FERRITE_DEMO_PILOTES") {
        return DRIVERS
            .iter()
            .map(|(_, driver)| *driver)
            .filter(|driver| demo.to_lowercase().contains(&driver.brand.to_lowercase()))
            .collect();
    }
    detect(Path::new("/sys/bus/pci/devices"), Path::new("/var/lib/dpkg/info"))
}

/// Système live (clé USB) : ce qu'on y installe disparaît au redémarrage.
pub fn is_live() -> bool {
    Path::new("/run/live/medium").exists() || env::var_os("FERRITE_DEMO_LIVE").is_some()
}

/// Texte de la notification, ou rien si aucun pilote ne manque.
pub fn message(missing: &[Driver]) -> Option<(String, String)> {
    let first = missing.first()?;
    let brands: Vec<&str> = missing.iter().map(|driver| driver.brand).collect();
    let title = if missing.len() == 1 {
        format!("Pilote graphique {} disponible", first.brand)
    } else {
        "Pilotes graphiques disponibles".to_owned()
    };
    Some((
        title,
        format!(
            "Votre carte {} fonctionne sans accélération. Ouvrez la Logithèque pour \
             télécharger son pilote.",
            brands.join(" et ")
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(pci: &Path, slot: &str, class: &str, vendor: &str) {
        let dir = pci.join(slot);
        fs::create_dir_all(&dir).unwrap();
        // Écrits comme le noyau : « 0x » puis l'hexadécimal.
        fs::write(dir.join("class"), format!("0x{class}\n")).unwrap();
        fs::write(dir.join("vendor"), format!("0x{vendor}\n")).unwrap();
    }

    #[test]
    fn detection_des_cartes() {
        let root = env::temp_dir().join(format!("ferrite-pilotes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (pci, dpkg) = (root.join("pci"), root.join("dpkg"));
        fs::create_dir_all(&dpkg).unwrap();
        // Intel seule : rien à proposer ; le contrôleur audio NVIDIA (classe 0x04) ne
        // compte pas.
        card(&pci, "0000:00:02.0", "030000", "8086");
        card(&pci, "0000:01:00.1", "040300", "10de");
        assert!(detect(&pci, &dpkg).is_empty());
        // Portable hybride Intel + NVIDIA (contrôleur 3D).
        card(&pci, "0000:01:00.0", "030200", "10de");
        let found = detect(&pci, &dpkg);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].package, "firmware-nvidia-graphics");
        // Déjà installé.
        fs::write(dpkg.join("firmware-nvidia-graphics.list"), "").unwrap();
        assert!(detect(&pci, &dpkg).is_empty());
        // Dossier absent : rien.
        assert!(detect(&root.join("absent"), &dpkg).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn texte_de_la_notification() {
        assert!(message(&[]).is_none());
        let (_, nvidia) = DRIVERS[1];
        let (title, body) = message(&[nvidia]).unwrap();
        assert_eq!(title, "Pilote graphique NVIDIA disponible");
        assert!(body.contains("carte NVIDIA"));
        let (_, amd) = DRIVERS[0];
        let (title, body) = message(&[amd, nvidia]).unwrap();
        assert_eq!(title, "Pilotes graphiques disponibles");
        assert!(body.contains("AMD et NVIDIA"));
    }
}
