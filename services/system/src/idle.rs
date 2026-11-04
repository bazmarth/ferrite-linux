//! Inactivité : verrouillage et mise en veille automatiques.
//!
//! Réglages, en minutes (0 : jamais) : `verrouillage_auto`, `veille_batterie` et
//! `veille_secteur`. Le bureau compte depuis la dernière entrée (clavier, souris, geste) ;
//! une vidéo affichée (`zwp_idle_inhibit_v1`) ou du son en cours retient la mise en veille.

use std::{fs, path::Path, time::Duration};

use crate::config::Settings;

/// Délais proposés dans Paramètres, en minutes (0 : jamais).
pub const LOCK_CHOICES: [u32; 7] = [0, 1, 2, 5, 10, 15, 30];
pub const SLEEP_CHOICES: [u32; 7] = [0, 5, 10, 15, 30, 60, 120];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleSettings {
    /// Minutes avant de verrouiller, puis de mettre en veille sur batterie et sur secteur.
    pub lock_after: u32,
    pub sleep_battery: u32,
    pub sleep_ac: u32,
}

impl Default for IdleSettings {
    fn default() -> Self {
        Self {
            lock_after: 5,
            sleep_battery: 15,
            sleep_ac: 30,
        }
    }
}

impl IdleSettings {
    pub fn load(settings: &Settings) -> Self {
        let defaults = Self::default();
        let minutes = |key: &str, default: u32| {
            settings
                .get(key)
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(default)
        };
        Self {
            lock_after: minutes("verrouillage_auto", defaults.lock_after),
            sleep_battery: minutes("veille_batterie", defaults.sleep_battery),
            sleep_ac: minutes("veille_secteur", defaults.sleep_ac),
        }
    }

    pub fn save(&self, settings: &mut Settings) {
        settings.set("verrouillage_auto", &self.lock_after.to_string());
        settings.set("veille_batterie", &self.sleep_battery.to_string());
        settings.set("veille_secteur", &self.sleep_ac.to_string());
    }

    pub fn lock_delay(&self) -> Option<Duration> {
        delay(self.lock_after)
    }

    pub fn sleep_delay(&self, on_battery: bool) -> Option<Duration> {
        delay(if on_battery {
            self.sleep_battery
        } else {
            self.sleep_ac
        })
    }
}

fn delay(minutes: u32) -> Option<Duration> {
    (minutes > 0).then(|| Duration::from_secs(u64::from(minutes) * 60))
}

/// « Jamais », « 1 minute », « 15 minutes », « 1 heure »…
pub fn minutes_label(minutes: u32) -> String {
    match minutes {
        0 => "Jamais".to_owned(),
        1 => "1 minute".to_owned(),
        60 => "1 heure".to_owned(),
        minutes if minutes % 60 == 0 => format!("{} heures", minutes / 60),
        minutes => format!("{minutes} minutes"),
    }
}

/// Sur batterie : aucun chargeur branché (et une batterie présente).
pub fn on_battery() -> bool {
    on_battery_in(Path::new("/sys/class/power_supply"))
}

fn on_battery_in(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    let read = |path: &Path, name: &str| fs::read_to_string(path.join(name)).unwrap_or_default();
    let mut battery = false;
    for entry in entries.flatten() {
        let path = entry.path();
        match read(&path, "type").trim() {
            "Mains" | "USB" if read(&path, "online").trim() == "1" => return false,
            "Battery" => battery = true,
            _ => {}
        }
    }
    battery
}

/// Du son sort (ou un micro enregistre) en ce moment : un flux actif dans PipeWire, quelle
/// que soit la sortie (haut-parleurs, casque Bluetooth…) ; à défaut de `wpctl`, un flux de
/// lecture ALSA en cours.
pub fn audio_playing() -> bool {
    crate::command::run("wpctl", &["status"]).map_or_else(
        |_| audio_playing_in(Path::new("/proc/asound")),
        |status| streams_active(&status),
    )
}

/// Une ligne de flux marquée « [active] » dans `wpctl status` (les flux arrêtés y sont
/// « [paused] »).
fn streams_active(status: &str) -> bool {
    let mut in_streams = false;
    for line in status.lines() {
        let text = line.trim_start_matches([' ', '│', '├', '└', '─']);
        if text.starts_with("Streams:") {
            in_streams = true;
        } else if text.trim().is_empty() {
            in_streams = false;
        } else if in_streams && line.trim_end().ends_with("[active]") {
            return true;
        }
    }
    false
}

fn audio_playing_in(root: &Path) -> bool {
    let Ok(cards) = fs::read_dir(root) else {
        return false;
    };
    cards
        .flatten()
        .filter(|card| card.file_name().to_string_lossy().starts_with("card"))
        .filter_map(|card| fs::read_dir(card.path()).ok())
        .flatten()
        .flatten()
        // Les flux de lecture : pcm0p, pcm3p… (pcm0c est une capture).
        .filter(|pcm| {
            let name = pcm.file_name().to_string_lossy().into_owned();
            name.starts_with("pcm") && name.ends_with('p')
        })
        .filter_map(|pcm| fs::read_dir(pcm.path()).ok())
        .flatten()
        .flatten()
        .any(|sub| {
            fs::read_to_string(sub.path().join("status"))
                .is_ok_and(|status| status.lines().any(|line| line.trim() == "state: RUNNING"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reglages_et_valeurs_d_origine() {
        let settings = Settings::parse("verrouillage_auto=2\nveille_secteur=0\n");
        let idle = IdleSettings::load(&settings);
        assert_eq!(idle.lock_delay(), Some(Duration::from_secs(120)));
        assert_eq!(idle.sleep_delay(true), Some(Duration::from_secs(15 * 60)));
        assert_eq!(idle.sleep_delay(false), None);
        let mut saved = Settings::parse("");
        idle.save(&mut saved);
        assert_eq!(IdleSettings::load(&saved), idle);
    }

    #[test]
    fn libelles() {
        assert_eq!(minutes_label(0), "Jamais");
        assert_eq!(minutes_label(1), "1 minute");
        assert_eq!(minutes_label(15), "15 minutes");
        assert_eq!(minutes_label(60), "1 heure");
        assert_eq!(minutes_label(120), "2 heures");
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn chargeur_et_son() {
        let root = std::env::temp_dir().join(format!("ferrite-idle-{}", std::process::id()));
        let supply = root.join("power_supply");
        write(&supply.join("BAT0/type"), "Battery\n");
        write(&supply.join("AC/type"), "Mains\n");
        write(&supply.join("AC/online"), "0\n");
        assert!(on_battery_in(&supply));
        write(&supply.join("AC/online"), "1\n");
        assert!(!on_battery_in(&supply));

        let asound = root.join("asound");
        write(&asound.join("card0/pcm0p/sub0/status"), "closed\n");
        write(&asound.join("card0/pcm0c/sub0/status"), "state: RUNNING\n");
        assert!(!audio_playing_in(&asound));
        write(
            &asound.join("card0/pcm0p/sub0/status"),
            "state: RUNNING\nowner_pid   : 1\n",
        );
        assert!(audio_playing_in(&asound));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn flux_pipewire() {
        let status = "Audio\n ├─ Sinks:\n │  *   57. Haut-parleurs [vol: 0.40]\n │\n \
                      └─ Streams:\n        84. Brave\n             87. output_FL       > \
                      soundcore Q20i:playback_FL\t[active]\n\nVideo\n";
        assert!(streams_active(status));
        assert!(!streams_active(&status.replace("[active]", "[paused]")));
        // « [active] » hors des flux (un périphérique) ne compte pas.
        assert!(!streams_active("Audio\n ├─ Devices:\n │  40. Carte [active]\n"));
    }
}
