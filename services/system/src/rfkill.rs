//! Mode avion : blocage logiciel de toutes les radios (Wi-Fi, Bluetooth) par `/dev/rfkill`,
//! ouvert en écriture à l'utilisateur de la session (règle udev `uaccess`).

use std::{fs, io::Write, path::Path};

use crate::command::Error;

/// Mode avion : toutes les radios Wi-Fi et Bluetooth sont bloquées. `None` sans radio.
pub fn airplane() -> Option<bool> {
    airplane_in(Path::new("/sys/class/rfkill"))
}

/// Comme [`airplane`], dans un dossier au format de `/sys/class/rfkill`.
pub fn airplane_in(dir: &Path) -> Option<bool> {
    let read = |path: &Path, name: &str| fs::read_to_string(path.join(name)).unwrap_or_default();
    let mut radios = 0;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if !matches!(read(&path, "type").trim(), "wlan" | "bluetooth") {
            continue;
        }
        radios += 1;
        let blocked = read(&path, "soft").trim() == "1" || read(&path, "hard").trim() == "1";
        if !blocked {
            return Some(false);
        }
    }
    (radios > 0).then_some(true)
}

/// Sortes de radios (`RFKILL_TYPE_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Radio {
    All = 0,
    Wlan = 1,
    Bluetooth = 2,
}

/// Bloque (`true`) ou débloque toutes les radios d'un coup.
pub fn set_airplane(on: bool) -> Result<(), Error> {
    set_blocked(Radio::All, on)
}

/// Bloque ou débloque toutes les radios d'une sorte. Allumer le Wi-Fi ou le Bluetooth
/// commence par là : un blocage resté d'un mode avion ferait échouer NetworkManager ou BlueZ.
pub fn set_blocked(radio: Radio, blocked: bool) -> Result<(), Error> {
    // `struct rfkill_event` : idx (u32), type, op, soft, hard ; RFKILL_OP_CHANGE_ALL = 3.
    let event = [0, 0, 0, 0, radio as u8, 3, u8::from(blocked), 0];
    fs::OpenOptions::new()
        .write(true)
        .open("/dev/rfkill")
        .and_then(|mut device| device.write_all(&event))
        .map_err(|err| format!("Radios non réglables : {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radio(dir: &Path, name: &str, kind: &str, soft: u8) {
        let path = dir.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("type"), format!("{kind}\n")).unwrap();
        fs::write(path.join("soft"), format!("{soft}\n")).unwrap();
        fs::write(path.join("hard"), "0\n").unwrap();
    }

    #[test]
    fn mode_avion_quand_toutes_les_radios_sont_bloquees() {
        let dir = std::env::temp_dir().join(format!("ferrite-rfkill-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(airplane_in(&dir), None);
        radio(&dir, "rfkill0", "wlan", 1);
        radio(&dir, "rfkill1", "bluetooth", 0);
        // Une autre sorte de radio ne compte pas.
        radio(&dir, "rfkill2", "nfc", 0);
        assert_eq!(airplane_in(&dir), Some(false));
        radio(&dir, "rfkill1", "bluetooth", 1);
        assert_eq!(airplane_in(&dir), Some(true));
        fs::remove_dir_all(&dir).unwrap();
    }
}
