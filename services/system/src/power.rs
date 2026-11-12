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

    /// Profil suivant d'un clic sur la bascule du centre de contrôle :
    /// Normal → Performance → Économie → Normal. Performance est sautée si la machine ne
    /// l'offre pas.
    pub fn next(self, performance: bool) -> Profile {
        match self {
            Profile::Balanced if performance => Profile::Performance,
            Profile::Balanced | Profile::Performance => Profile::PowerSaver,
            Profile::PowerSaver => Profile::Balanced,
        }
    }

    /// Nom montré à l'utilisateur.
    pub fn label(self) -> &'static str {
        match self {
            Profile::PowerSaver => "Économie",
            Profile::Balanced => "Normal",
            Profile::Performance => "Performance",
        }
    }
}

/// Profil actif et profils offerts par la machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    pub current: Profile,
    /// Profil « performance » offert (absent sans pilote de plateforme adapté).
    pub performance: bool,
}

impl State {
    /// Lit la sortie de `powerprofilesctl list` : un titre par profil (« * » devant
    /// l'actif), suivi de lignes de détail en retrait.
    pub fn parse(text: &str) -> Option<State> {
        let mut current = None;
        let mut performance = false;
        for line in text.lines() {
            // Les lignes de détail (« CpuDriver: … ») ne sont pas des noms de profil.
            let active = line.starts_with('*');
            let name = line.trim_start_matches('*').trim();
            let Some(profile) = name.strip_suffix(':').and_then(Profile::parse) else {
                continue;
            };
            performance |= profile == Profile::Performance;
            if active {
                current = Some(profile);
            }
        }
        current.map(|current| State { current, performance })
    }
}

/// Profil actuel et profils offerts. `None` : démon absent ou arrêté.
pub fn state() -> Option<State> {
    command::run_with_timeout("powerprofilesctl", &["list"], TIMEOUT)
        .ok()
        .and_then(|text| State::parse(&text))
}

/// Profil actuel. `None` : démon absent ou arrêté.
pub fn profile() -> Option<Profile> {
    command::run_with_timeout("powerprofilesctl", &["get"], TIMEOUT)
        .ok()
        .and_then(|text| Profile::parse(&text))
}

/// Change de profil.
pub fn set_profile(profile: Profile) -> Result<(), Error> {
    command::run("powerprofilesctl", &["set", profile.id()])
        .map(drop)
        .map_err(|_| "Mode d’énergie non modifiable (power-profiles-daemon absent ?)".to_owned())
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

    const LISTE: &str = "  performance:\n    CpuDriver:\tintel_pstate\n    PlatformDriver:\tplatform_profile\n    Degraded:   no\n\n* balanced:\n    CpuDriver:\tintel_pstate\n    PlatformDriver:\tplatform_profile\n\n  power-saver:\n    CpuDriver:\tintel_pstate\n    PlatformDriver:\tplatform_profile\n";

    #[test]
    fn lit_la_liste_des_profils() {
        let state = State::parse(LISTE).unwrap();
        assert_eq!(state.current, Profile::Balanced);
        assert!(state.performance);
        let saver = LISTE
            .replace("* balanced", "  balanced")
            .replace("  power-saver", "* power-saver");
        assert_eq!(State::parse(&saver).unwrap().current, Profile::PowerSaver);
        let sans =
            "* balanced:\n    CpuDriver:\tintel_pstate\n\n  power-saver:\n    CpuDriver:\tintel_pstate\n";
        assert_eq!(
            State::parse(sans),
            Some(State {
                current: Profile::Balanced,
                performance: false
            })
        );
        assert_eq!(State::parse(""), None);
    }

    #[test]
    fn le_clic_fait_le_tour_des_profils() {
        assert_eq!(Profile::Balanced.next(true), Profile::Performance);
        assert_eq!(Profile::Performance.next(true), Profile::PowerSaver);
        assert_eq!(Profile::PowerSaver.next(true), Profile::Balanced);
        assert_eq!(Profile::Balanced.next(false), Profile::PowerSaver);
    }
}
