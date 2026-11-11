//! Inactivité du bureau : écran éteint, verrouillage puis mise en veille automatiques (réglages de
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
/// Sur l'écran verrouillé, l'écran s'éteint plus tôt : personne n'y travaille.
const LOCKED_SCREEN_OFF: Duration = Duration::from_secs(60);

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
    /// Écran éteint pour inactivité (le backend DRM coupe l'affichage) ; la moindre entrée
    /// le rallume.
    pub screen_off: bool,
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
            screen_off: false,
        }
    }
}

impl Idle {
    /// Temps écoulé depuis la dernière entrée (clavier, souris, pavé tactile).
    pub fn quiet_for(&self) -> Duration {
        self.last_input.elapsed()
    }

    pub fn activity(&mut self) {
        let now = Instant::now();
        self.last_input = now;
        self.sleep_from = now;
        self.screen_off = false;
    }

    /// L'écran doit-il s'éteindre ? Après le délai réglé, ou une minute sur l'écran
    /// verrouillé ; jamais si le réglage vaut « Jamais ».
    fn screen_off_due(&self, now: Instant, locked: bool) -> bool {
        let Some(delay) = self.settings.screen_off_delay() else {
            return false;
        };
        let delay = if locked {
            delay.min(LOCKED_SCREEN_OFF)
        } else {
            delay
        };
        !self.screen_off && now.duration_since(self.last_input) >= delay
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
        if self.idle.screen_off_due(now, self.lock.is_some()) {
            tracing::info!("écran éteint après inactivité");
            self.idle.screen_off = true;
            self.request_redraw();
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
    fn l_ecran_s_eteint_apres_le_delai_et_plus_tot_verrouille() {
        let start = Instant::now();
        let mut idle = Idle {
            last_input: start,
            ..Idle::default()
        };
        // Par défaut : 10 minutes, une seule sur l'écran verrouillé.
        assert!(!idle.screen_off_due(start + Duration::from_secs(9 * 60), false));
        assert!(idle.screen_off_due(start + Duration::from_secs(10 * 60), false));
        assert!(!idle.screen_off_due(start + Duration::from_secs(59), true));
        assert!(idle.screen_off_due(start + LOCKED_SCREEN_OFF, true));
        // Déjà éteint : rien à refaire ; une entrée le rallume.
        idle.screen_off = true;
        assert!(!idle.screen_off_due(start + Duration::from_secs(3600), false));
        idle.activity();
        assert!(!idle.screen_off);
        // « Jamais » : pas même sur l'écran verrouillé.
        idle.settings.screen_off = 0;
        assert!(!idle.screen_off_due(Instant::now() + Duration::from_secs(3600), true));
        // Un délai plus court que la minute de l'écran verrouillé est gardé.
        idle.settings.screen_off = 1;
        let now = Instant::now();
        idle.last_input = now;
        assert!(idle.screen_off_due(now + Duration::from_secs(60), false));
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
