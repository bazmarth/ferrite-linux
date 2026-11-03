//! Écran : luminosité du rétroéclairage.
//!
//! Un portable expose souvent plusieurs rétroéclairages (`acpi_video0` et
//! `intel_backlight` sur Intel), dont un seul agit vraiment : on les règle donc tous, à la
//! même proportion. Pour chacun, trois moyens sont essayés dans l'ordre : écriture directe
//! dans `/sys` (règle udev de `brightnessctl`, groupe `video`), `SetBrightness` de logind
//! (session active), puis `brightnessctl`.

use std::{fs, path::PathBuf};

use crate::command::{self, Error};

/// Rétroéclairages, le plus fiable d'abord : `raw` (pilote graphique) avant `platform`
/// et `firmware` (ACPI).
fn backlights() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir("/sys/class/backlight") else {
        return Vec::new();
    };
    let mut devices: Vec<(u8, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .map(|path| {
            let kind = fs::read_to_string(path.join("type")).unwrap_or_default();
            (type_rank(kind.trim()), path)
        })
        .collect();
    devices.sort();
    devices.into_iter().map(|(_, path)| path).collect()
}

fn type_rank(kind: &str) -> u8 {
    match kind {
        "raw" => 0,
        "platform" => 1,
        "firmware" => 2,
        _ => 3,
    }
}

fn read(dir: &std::path::Path, name: &str) -> Option<u64> {
    fs::read_to_string(dir.join(name)).ok()?.trim().parse().ok()
}

/// Luminosité en pour cent ; `None` sans rétroéclairage réglable (écran externe, VM).
pub fn brightness() -> Option<u8> {
    let dir = backlights().into_iter().next()?;
    let max = read(&dir, "max_brightness")?.max(1);
    Some(((read(&dir, "brightness")? * 100 + max / 2) / max).min(100) as u8)
}

/// Valeur brute pour `percent` d'un maximum `max` (jamais 0 : un écran noir semblerait éteint).
pub fn raw_value(percent: u8, max: u64) -> u64 {
    (max * percent.min(100) as u64).div_ceil(100).max(1)
}

/// Règle la luminosité de tous les rétroéclairages (au moins 5 %).
pub fn set_brightness(percent: u8) -> Result<(), Error> {
    let devices = backlights();
    if devices.is_empty() {
        return Err("Luminosité non réglable sur cet écran".into());
    }
    let percent = percent.clamp(5, 100);
    let mut first_error = None;
    let mut done = false;
    for dir in devices {
        let Some(max) = read(&dir, "max_brightness") else {
            continue;
        };
        let value = raw_value(percent, max);
        match set_device(&dir, value) {
            Ok(()) => done = true,
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    if done {
        Ok(())
    } else {
        Err(first_error.unwrap_or_else(|| "Luminosité non réglable".into()))
    }
}

fn set_device(dir: &std::path::Path, value: u64) -> Result<(), Error> {
    if fs::write(dir.join("brightness"), value.to_string()).is_ok() {
        return Ok(());
    }
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let logind = command::run(
        "busctl",
        &[
            "call",
            "org.freedesktop.login1",
            "/org/freedesktop/login1/session/auto",
            "org.freedesktop.login1.Session",
            "SetBrightness",
            "ssu",
            "backlight",
            &name,
            &value.to_string(),
        ],
    );
    if logind.is_ok() {
        return Ok(());
    }
    command::run(
        "brightnessctl",
        &["--quiet", "--device", &name, "set", &value.to_string()],
    )
    .map(drop)
    .map_err(|error| format!("Luminosité refusée ({name}) : {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valeurs_brutes_et_ordre_des_retroeclairages() {
        assert_eq!(raw_value(50, 96000), 48000);
        assert_eq!(raw_value(5, 10), 1);
        assert_eq!(raw_value(0, 255), 1);
        assert!(type_rank("raw") < type_rank("firmware"));
    }
}

/// Rétroéclairage du clavier (`/sys/class/leds/*kbd_backlight`) : niveau et maximum.
/// `None` sans clavier rétroéclairé.
pub fn keyboard_light() -> Option<(u8, u8)> {
    let dir = keyboard_light_dir()?;
    let max = read(&dir, "max_brightness")?.min(u8::MAX as u64) as u8;
    let level = read(&dir, "brightness")?.min(max as u64) as u8;
    (max > 0).then_some((level, max))
}

fn keyboard_light_dir() -> Option<PathBuf> {
    fs::read_dir("/sys/class/leds")
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with("kbd_backlight"))
        })
}

/// Règle le rétroéclairage du clavier : écriture directe, sinon `SetBrightness` de logind
/// (sous-système `leds`), qui ne demande pas les droits de root à la session active.
pub fn set_keyboard_light(level: u8) -> Result<(), Error> {
    let dir = keyboard_light_dir().ok_or("Pas de clavier rétroéclairé")?;
    let value = level.to_string();
    if fs::write(dir.join("brightness"), &value).is_ok() {
        return Ok(());
    }
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    command::run(
        "busctl",
        &[
            "call",
            "org.freedesktop.login1",
            "/org/freedesktop/login1/session/auto",
            "org.freedesktop.login1.Session",
            "SetBrightness",
            "ssu",
            "leds",
            &name,
            &value,
        ],
    )
    .map(drop)
}
