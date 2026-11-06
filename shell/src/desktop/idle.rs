//! Inactivité du bureau : verrouillage puis mise en veille automatiques (réglages de
//! `ferrite_system::idle`). Vérifiée toutes les [`CHECK`] par une minuterie : la boucle
//! peut dormir longtemps quand rien ne bouge.

use std::time::{Duration, Instant};

use ferrite_system::idle::{self, IdleSettings};
use smithay::reexports::calloop::{
    LoopHandle,
    timer::{TimeoutAction, Timer},
};

use crate::state::Ferrite;

const CHECK: Duration = Duration::from_secs(5);
const KEYBOARD_LIGHT_IDLE: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct Idle {
    pub settings: IdleSettings,
    /// Dernière entrée (clavier, souris, pavé tactile).
    last_input: Instant,
    /// Dernière frappe, utilisée uniquement pour le rétroéclairage du clavier.
    last_keyboard_input: Instant,
    /// Niveau à rétablir après l'extinction automatique.
    keyboard_light_restore: Option<u8>,
    /// Départ du compte avant la mise en veille : la dernière entrée, ou le dernier report
    /// (son en cours, veille demandée).
    sleep_from: Instant,
}

impl Default for Idle {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            settings: IdleSettings::default(),
            last_input: now,
            last_keyboard_input: now,
            keyboard_light_restore: None,
            sleep_from: now,
        }
    }
}

impl Idle {
    pub fn activity(&mut self) {
        let now = Instant::now();
        self.last_input = now;
        self.sleep_from = now;
    }

    pub fn keyboard_activity(&mut self, now: Instant) -> Option<u8> {
        self.last_keyboard_input = now;
        self.keyboard_light_restore
    }

    pub fn manual_keyboard_light(&mut self, level: u8, now: Instant) {
        self.last_keyboard_input = now;
        self.keyboard_light_restore = (level > 0).then_some(level);
    }

    fn dim_keyboard_light(&mut self, now: Instant, level: Option<(u8, u8)>) -> Option<u8> {
        if now.duration_since(self.last_keyboard_input) < KEYBOARD_LIGHT_IDLE {
            return None;
        }
        let level = level.filter(|(level, _)| *level > 0)?.0;
        Some(*self.keyboard_light_restore.get_or_insert(level))
    }
}

/// Branche la vérification régulière sur la boucle.
pub fn start(handle: &LoopHandle<'static, Ferrite>) -> Result<(), String> {
    handle
        .insert_source(Timer::from_duration(CHECK), |_, _, state| {
            state.idle_tick();
            TimeoutAction::ToDuration(CHECK)
        })
        .map(drop)
        .map_err(|error| error.to_string())
}

impl Ferrite {
    fn idle_tick(&mut self) {
        let now = Instant::now();
        if self
            .idle
            .dim_keyboard_light(now, self.system.keyboard_light)
            .is_some()
        {
            tracing::info!("rétroéclairage du clavier éteint après une minute sans frappe");
            self.request_system_change(crate::system::Change::KeyboardLight(0));
        }
        // Une vidéo ou une présentation affichée (`zwp_idle_inhibit_v1`) vaut une présence.
        if !self.idle_inhibitors.is_empty() {
            self.idle.activity();
            return;
        }
        if let Some(delay) = self.idle.settings.lock_delay()
            && now.duration_since(self.idle.last_input) >= delay
            && self.lock.is_none()
            && self.session_lock.is_none()
        {
            tracing::info!(?delay, "verrouillage après inactivité");
            self.lock_session();
        }
        let Some(delay) = self.idle.settings.sleep_delay(idle::on_battery()) else {
            return;
        };
        if now.duration_since(self.idle.sleep_from) < delay {
            return;
        }
        // De la musique sans personne devant : on attend qu'elle s'arrête.
        if idle::audio_playing() {
            self.idle.sleep_from = now;
            return;
        }
        tracing::info!(?delay, "mise en veille après inactivité");
        self.idle.sleep_from = now;
        self.lock_and_sleep();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_clavier_s_eteint_apres_une_minute_et_retrouve_son_niveau_a_la_frappe() {
        let start = Instant::now();
        let mut idle = Idle {
            last_keyboard_input: start,
            ..Idle::default()
        };

        assert_eq!(
            idle.dim_keyboard_light(start + Duration::from_secs(59), Some((2, 2))),
            None
        );
        assert_eq!(
            idle.dim_keyboard_light(start + KEYBOARD_LIGHT_IDLE, Some((2, 2))),
            Some(2)
        );
        assert_eq!(
            idle.dim_keyboard_light(start + KEYBOARD_LIGHT_IDLE + CHECK, Some((0, 2))),
            None
        );
        assert_eq!(
            idle.keyboard_activity(start + KEYBOARD_LIGHT_IDLE + CHECK),
            Some(2)
        );
        assert_eq!(
            idle.dim_keyboard_light(
                start + KEYBOARD_LIGHT_IDLE + CHECK + KEYBOARD_LIGHT_IDLE,
                Some((2, 2))
            ),
            Some(2)
        );
    }

    #[test]
    fn une_modification_manuelle_du_retroeclairage_remplace_le_niveau_a_restaurer() {
        let start = Instant::now();
        let mut idle = Idle {
            keyboard_light_restore: Some(2),
            ..Idle::default()
        };
        idle.manual_keyboard_light(1, start);
        assert_eq!(idle.keyboard_activity(start), Some(1));
        idle.manual_keyboard_light(0, start);
        assert_eq!(idle.keyboard_activity(start), None);
    }
}
