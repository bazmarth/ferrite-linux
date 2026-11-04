//! Mode nuit du bureau : teinte orangée de l'écran selon les réglages (voir
//! `ferrite_system::night`), appliquée par la table de gamma de l'écran (backend DRM).
//! La tuile du centre de contrôle force la teinte jusqu'à la prochaine bascule programmée.

use std::time::{Duration, Instant};

use ferrite_system::{
    night::{self, NightSettings, Schedule, local_now},
    weather::Place,
};

use crate::state::Ferrite;

/// La force de la teinte est recalculée au plus toutes les 5 s (le fondu dure 20 min).
const CHECK: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct Night {
    pub settings: NightSettings,
    pub place: Option<Place>,
    /// Forçage de la tuile : teinte voulue, et si la programmation était active alors (il
    /// tombe quand elle bascule).
    pub forced: Option<(bool, bool)>,
    /// Force actuelle (0 à 1) et gains rouge, vert, bleu de l'écran.
    pub strength: f64,
    pub gains: (f64, f64, f64),
    pub checked: Option<Instant>,
}

impl Night {
    /// Force que donne la programmation seule, maintenant.
    fn scheduled(&self) -> f64 {
        let Some(place) = &self.place else {
            return if self.settings.schedule == Schedule::Always {
                1.0
            } else {
                0.0
            };
        };
        let (day, minute, offset) = local_now();
        self.settings.strength(place, day, offset, minute)
    }

    /// Fenêtre du jour (début, fin) en minutes locales, pour la tuile.
    pub fn window(&self) -> Option<(u32, u32)> {
        let (day, _, offset) = local_now();
        self.settings.window(self.place.as_ref()?, day, offset)
    }

    pub fn active(&self) -> bool {
        self.strength > 0.0
    }
}

impl Ferrite {
    /// Recalcule la teinte (toutes les [`CHECK`], ou tout de suite après un changement).
    pub fn night_tick(&mut self) {
        let now = Instant::now();
        if self
            .night
            .checked
            .is_some_and(|checked| now.duration_since(checked) < CHECK)
        {
            return;
        }
        self.night.checked = Some(now);
        let scheduled = self.night.scheduled();
        let strength = match self.night.forced {
            // La programmation a basculé depuis le forçage : elle reprend la main.
            Some((_, was_active)) if (scheduled > 0.0) != was_active => {
                self.night.forced = None;
                scheduled
            }
            Some((on, _)) => {
                if on {
                    1.0
                } else {
                    0.0
                }
            }
            None => scheduled,
        };
        let gains = night::gains(self.night.settings.temperature, strength);
        if gains != self.night.gains || strength != self.night.strength {
            self.night.strength = strength;
            self.night.gains = gains;
            self.request_redraw();
        }
    }

    /// Tuile du centre de contrôle : allume ou éteint la teinte tout de suite.
    pub fn toggle_night(&mut self) {
        let scheduled = self.night.scheduled() > 0.0;
        let on = !self.night.active();
        // Revenir à ce que veut la programmation, c'est ne plus rien forcer.
        self.night.forced = (on != scheduled).then_some((on, scheduled));
        self.night.checked = None;
        self.night_tick();
    }
}

/// Table de gamma d'une couleur : `size` valeurs de 0 à 65535, multipliées par `gain`.
pub fn gamma_ramp(size: usize, gain: f64) -> Vec<u16> {
    let last = size.saturating_sub(1).max(1) as f64;
    (0..size)
        .map(|index| (index as f64 / last * 65535.0 * gain.clamp(0.0, 1.0)).round() as u16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_de_gamma() {
        assert_eq!(gamma_ramp(3, 1.0), [0, 32768, 65535]);
        assert_eq!(gamma_ramp(3, 0.5), [0, 16384, 32768]);
        assert!(gamma_ramp(0, 1.0).is_empty());
    }

    #[test]
    fn le_forcage_tombe_a_la_bascule_suivante() {
        let mut night = Night {
            settings: NightSettings {
                schedule: Schedule::Always,
                ..NightSettings::default()
            },
            ..Night::default()
        };
        // Sans ville, « Toujours » suffit à savoir que la programmation est active.
        assert_eq!(night.scheduled(), 1.0);
        night.forced = Some((false, true));
        night.settings.schedule = Schedule::Off;
        assert_eq!(night.scheduled(), 0.0);
    }
}
