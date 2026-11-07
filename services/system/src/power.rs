//! Profils d'énergie : `powerprofilesctl` (démon `power-profiles-daemon`).
//!
//! Le démon choisit le réglage du processeur (et de la plateforme, si l'ACPI le permet)
//! selon trois profils. Un utilisateur de la session active peut en changer sans mot de
//! passe (règle polkit livrée avec le démon).

use std::time::Duration;

use crate::command::{self, Error};

/// Délai de lecture : le démon répond tout de suite ou n'est pas lancé.
const TIMEOUT: Duration = Duration::from_secs(2);

/// Profil d'énergie de `power-profiles-daemon`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    PowerSaver,
    Balanced,
    Performance,
}

impl Profile {
    /// Nom attendu par `powerprofilesctl`.
    pub fn id(self) -> &'static str {
        match self {
            Profile::PowerSaver => "power-saver",
            Profile::Balanced => "balanced",
            Profile::Performance => "performance",
        }
    }

    /// Lit la sortie de `powerprofilesctl get`.
    pub fn parse(text: &str) -> Option<Profile> {
        match text.trim() {
            "power-saver" => Some(Profile::PowerSaver),
            "balanced" => Some(Profile::Balanced),
            "performance" => Some(Profile::Performance),
            _ => None,
        }
    }
}

/// Profil actuel. `None` : démon absent ou arrêté.
pub fn profile() -> Option<Profile> {
    command::run_with_timeout("powerprofilesctl", &["get"], TIMEOUT)
        .ok()
        .and_then(|text| Profile::parse(&text))
}

/// Économie d'énergie active. `None` : démon absent ou arrêté.
pub fn power_saver() -> Option<bool> {
    profile().map(|profile| profile == Profile::PowerSaver)
}

/// Change de profil.
pub fn set_profile(profile: Profile) -> Result<(), Error> {
    command::run("powerprofilesctl", &["set", profile.id()])
        .map(drop)
        .map_err(|_| "Profil d’énergie non modifiable (power-profiles-daemon absent ?)".to_owned())
}

/// Active l'économie d'énergie, ou revient au profil équilibré.
pub fn set_power_saver(enabled: bool) -> Result<(), Error> {
    set_profile(if enabled {
        Profile::PowerSaver
    } else {
        Profile::Balanced
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_les_trois_profils() {
        assert_eq!(Profile::parse("power-saver\n"), Some(Profile::PowerSaver));
        assert_eq!(Profile::parse("balanced"), Some(Profile::Balanced));
        assert_eq!(Profile::parse(" performance \n"), Some(Profile::Performance));
        assert_eq!(Profile::parse(""), None);
        assert_eq!(Profile::parse("Traceback (most recent call last):"), None);
    }

    #[test]
    fn le_nom_relu_redonne_le_profil() {
        for profile in [Profile::PowerSaver, Profile::Balanced, Profile::Performance] {
            assert_eq!(Profile::parse(profile.id()), Some(profile));
        }
    }
}
