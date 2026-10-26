//! Écran : luminosité du rétroéclairage.

use std::{fs, path::PathBuf};

use crate::command::{self, Error};

fn backlight() -> Option<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir("/sys/class/backlight")
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    entries.into_iter().next()
}

/// Luminosité en pour cent ; `None` sans rétroéclairage réglable (écran externe, VM).
pub fn brightness() -> Option<u8> {
    let dir = backlight()?;
    let read = |name: &str| -> Option<u64> { fs::read_to_string(dir.join(name)).ok()?.trim().parse().ok() };
    let max = read("max_brightness")?.max(1);
    Some(((read("brightness")? * 100 + max / 2) / max).min(100) as u8)
}

/// Règle la luminosité (au moins 5 % : un écran noir semblerait éteint).
pub fn set_brightness(percent: u8) -> Result<(), Error> {
    if backlight().is_none() {
        return Err("Luminosité non réglable sur cet écran".into());
    }
    let percent = percent.clamp(5, 100);
    command::run("brightnessctl", &["--quiet", "set", &format!("{percent}%")]).map(drop)
}
