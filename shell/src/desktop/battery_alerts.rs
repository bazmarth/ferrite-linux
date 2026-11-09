//! Alertes de la batterie : chargeur branché ou débranché, batterie faible, presque vide,
//! chargée. Un fil lit `/sys/class/power_supply` et envoie des notifications du bureau au
//! centre de notifications ; chaque alerte remplace la précédente.
//!
//! Lire `capacity` interroge le contrôleur de la carte mère (parfois 0,2 s) : le fil ne le
//! relit que toutes les [`CAPACITY_REFRESH`] ou quand le chargeur change, alors que l'état
//! du chargeur (`online`, instantané) est relu toutes les [`PLUG_REFRESH`].
//!
//! Pour les essais, `FERRITE_ALIMENTATION` remplace le dossier `/sys/class/power_supply`.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use smithay::reexports::calloop::channel;

use crate::desktop::notifications::{Event, Notice, local_notice};

/// Seuils d'alerte, en pourcentage, sur batterie.
const LOW: u8 = 20;
const CRITICAL: u8 = 10;
/// Fréquence de relecture du chargeur, et du niveau de charge.
const PLUG_REFRESH: Duration = Duration::from_secs(2);
const CAPACITY_REFRESH: Duration = Duration::from_secs(30);
/// Durée de la bulle « batterie faible ».
const LOW_TIMEOUT: Duration = Duration::from_secs(12);

/// État lu de l'alimentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub percent: u8,
    /// Chargeur branché (secteur ou USB).
    pub plugged: bool,
    /// La batterie se dit pleine.
    pub full: bool,
}

/// Alerte à afficher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    Plugged(u8),
    Unplugged(u8),
    Low(u8),
    Critical(u8),
    Full,
}

impl Alert {
    fn notice(self) -> Notice {
        match self {
            Alert::Plugged(percent) => {
                local_notice("Chargeur branché", &format!("Batterie à {percent} %, en charge."))
            }
            Alert::Unplugged(percent) => local_notice(
                "Chargeur débranché",
                &format!("Sur batterie, {percent} % restants."),
            ),
            Alert::Low(percent) => Notice {
                timeout: Some(LOW_TIMEOUT),
                sound: true,
                ..local_notice(
                    "Batterie faible",
                    &format!("Il reste {percent} %. Pensez à brancher le chargeur."),
                )
            },
            // Reste affichée jusqu'à ce qu'on la ferme, même en « Ne pas déranger ».
            Alert::Critical(percent) => Notice {
                timeout: None,
                critical: true,
                sound: true,
                ..local_notice(
                    "Batterie presque vide",
                    &format!("Il reste {percent} %. Branchez vite le chargeur."),
                )
            },
            Alert::Full => local_notice("Batterie chargée", "Vous pouvez débrancher le chargeur."),
        }
    }
}

/// Mémoire des alertes déjà données, pour ne pas les répéter.
#[derive(Debug, Default)]
pub struct Watcher {
    last: Option<Reading>,
    warned_low: bool,
    warned_critical: bool,
    warned_full: bool,
}

impl Watcher {
    /// Alerte à donner pour ce nouvel état, s'il y en a une.
    pub fn step(&mut self, reading: Reading) -> Option<Alert> {
        let previous = self.last.replace(reading);
        let mut alert = None;
        match previous {
            // Au démarrage, le chargeur n'a pas « changé » : seuls les seuils comptent.
            None => self.warned_full = reading.full,
            Some(previous) if previous.plugged != reading.plugged => {
                if reading.plugged {
                    self.warned_low = false;
                    self.warned_critical = false;
                    alert = Some(Alert::Plugged(reading.percent));
                } else {
                    self.warned_full = false;
                    alert = Some(Alert::Unplugged(reading.percent));
                }
            }
            Some(_) => {}
        }
        if reading.plugged {
            if reading.full && !self.warned_full {
                self.warned_full = true;
                alert = Some(Alert::Full);
            }
        } else if reading.percent <= CRITICAL && !self.warned_critical {
            self.warned_critical = true;
            self.warned_low = true;
            alert = Some(Alert::Critical(reading.percent));
        } else if reading.percent <= LOW && !self.warned_low {
            self.warned_low = true;
            alert = Some(Alert::Low(reading.percent));
        }
        alert
    }
}

/// Dossier des alimentations : `FERRITE_ALIMENTATION` ou `/sys/class/power_supply`.
fn supply_root() -> PathBuf {
    std::env::var_os("FERRITE_ALIMENTATION")
        .map_or_else(|| PathBuf::from("/sys/class/power_supply"), PathBuf::from)
}

fn read(path: &Path, name: &str) -> String {
    fs::read_to_string(path.join(name))
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

/// Première batterie (`BAT0`, `BAT1`…), et chargeurs (secteur, USB).
fn supplies(root: &Path) -> (Option<PathBuf>, Vec<PathBuf>) {
    let mut batteries = Vec::new();
    let mut chargers = Vec::new();
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        match read(&path, "type").as_str() {
            "Battery" => batteries.push(path),
            "Mains" | "USB" => chargers.push(path),
            _ => {}
        }
    }
    batteries.sort();
    (batteries.into_iter().next(), chargers)
}

/// Chargeur branché : un chargeur en ligne, ou à défaut une batterie qui ne se vide pas.
fn plugged(battery: &Path, chargers: &[PathBuf]) -> bool {
    if chargers.is_empty() {
        return read(battery, "status") != "Discharging";
    }
    chargers.iter().any(|charger| read(charger, "online") == "1")
}

fn capacity(battery: &Path) -> Option<u8> {
    read(battery, "capacity")
        .parse::<u8>()
        .ok()
        .map(|percent| percent.min(100))
}

/// Lance le fil de surveillance ; rien sans batterie.
pub fn start(events: channel::Sender<Event>) {
    let root = supply_root();
    let (Some(battery), chargers) = supplies(&root) else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("alertes-batterie".to_owned())
        .spawn(move || watch(&battery, &chargers, &events));
}

fn watch(battery: &Path, chargers: &[PathBuf], events: &channel::Sender<Event>) {
    let mut watcher = Watcher::default();
    let mut shown: Option<u32> = None;
    let mut percent: Option<u8> = None;
    let mut read_at = Instant::now();
    let mut was_plugged = None;
    loop {
        let plugged = plugged(battery, chargers);
        // Petit carillon au branchement et au débranchement (sons des actions).
        if was_plugged.is_some_and(|was| was != plugged) {
            use ferrite_system::sounds::{self, Sound};
            sounds::play(if plugged { Sound::Plugged } else { Sound::Unplugged });
        }
        if percent.is_none() || was_plugged != Some(plugged) || read_at.elapsed() >= CAPACITY_REFRESH {
            percent = capacity(battery).or(percent);
            read_at = Instant::now();
        }
        was_plugged = Some(plugged);
        if let Some(percent) = percent {
            let reading = Reading {
                percent,
                plugged,
                full: read(battery, "status") == "Full" || (plugged && percent >= 100),
            };
            if let Some(alert) = watcher.step(reading) {
                if let Some(id) = shown.take() {
                    let _ = events.send(Event::Close(id));
                }
                let notice = alert.notice();
                shown = Some(notice.id);
                if events.send(Event::Notify(notice)).is_err() {
                    return;
                }
            }
        }
        std::thread::sleep(PLUG_REFRESH);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_battery(percent: u8) -> Reading {
        Reading {
            percent,
            plugged: false,
            full: false,
        }
    }

    fn on_mains(percent: u8) -> Reading {
        Reading {
            percent,
            plugged: true,
            full: percent >= 100,
        }
    }

    #[test]
    fn rien_au_demarrage_sans_seuil() {
        let mut watcher = Watcher::default();
        assert_eq!(watcher.step(on_battery(60)), None);
        assert_eq!(watcher.step(on_battery(59)), None);
        let mut watcher = Watcher::default();
        assert_eq!(watcher.step(on_mains(100)), None);
    }

    #[test]
    fn branche_puis_debranche() {
        let mut watcher = Watcher::default();
        watcher.step(on_battery(60));
        assert_eq!(watcher.step(on_mains(60)), Some(Alert::Plugged(60)));
        assert_eq!(watcher.step(on_mains(61)), None);
        assert_eq!(watcher.step(on_battery(61)), Some(Alert::Unplugged(61)));
    }

    #[test]
    fn faible_puis_presque_vide_une_seule_fois() {
        let mut watcher = Watcher::default();
        watcher.step(on_battery(25));
        assert_eq!(watcher.step(on_battery(20)), Some(Alert::Low(20)));
        assert_eq!(watcher.step(on_battery(19)), None);
        assert_eq!(watcher.step(on_battery(10)), Some(Alert::Critical(10)));
        assert_eq!(watcher.step(on_battery(9)), None);
    }

    #[test]
    fn presque_vide_des_le_demarrage() {
        let mut watcher = Watcher::default();
        assert_eq!(watcher.step(on_battery(7)), Some(Alert::Critical(7)));
        assert_eq!(watcher.step(on_battery(6)), None);
    }

    #[test]
    fn debrancher_bas_annonce_le_seuil() {
        let mut watcher = Watcher::default();
        watcher.step(on_mains(15));
        assert_eq!(watcher.step(on_battery(15)), Some(Alert::Low(15)));
    }

    #[test]
    fn rebrancher_rearme_les_seuils() {
        // L'alerte « faible » revient après un passage sur secteur.
        let mut watcher = Watcher::default();
        watcher.step(on_battery(30));
        assert_eq!(watcher.step(on_battery(18)), Some(Alert::Low(18)));
        assert_eq!(watcher.step(on_mains(18)), Some(Alert::Plugged(18)));
        assert_eq!(watcher.step(on_battery(18)), Some(Alert::Low(18)));
    }

    #[test]
    fn chargee_une_fois_par_branchement() {
        let mut watcher = Watcher::default();
        watcher.step(on_mains(98));
        assert_eq!(watcher.step(on_mains(100)), Some(Alert::Full));
        assert_eq!(watcher.step(on_mains(100)), None);
        watcher.step(on_battery(99));
        watcher.step(on_mains(99));
        assert_eq!(watcher.step(on_mains(100)), Some(Alert::Full));
    }

    #[test]
    fn lit_le_dossier_des_alimentations() {
        let root = std::env::temp_dir().join(format!("ferrite-alim-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (dir, files) in [
            (
                "BAT0",
                [("type", "Battery"), ("capacity", "42"), ("status", "Discharging")],
            ),
            ("ADP1", [("type", "Mains"), ("online", "0"), ("status", "")]),
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
            for (name, text) in files {
                fs::write(root.join(dir).join(name), format!("{text}\n")).unwrap();
            }
        }
        let (battery, chargers) = supplies(&root);
        let battery = battery.unwrap();
        assert_eq!(capacity(&battery), Some(42));
        assert!(!plugged(&battery, &chargers));
        fs::write(root.join("ADP1/online"), "1\n").unwrap();
        assert!(plugged(&battery, &chargers));
        let _ = fs::remove_dir_all(&root);
    }
}
