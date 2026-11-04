//! Inactivité du bureau : verrouillage puis mise en veille automatiques (réglages de
//! `ferrite_system::idle`). Vérifiée toutes les [`CHECK`] par une minuterie : la boucle
//! peut dormir longtemps quand rien ne bouge.

use std::time::{Duration, Instant};

use ferrite_system::idle::{self, IdleSettings};
use smithay::reexports::calloop::{
    LoopHandle,
    timer::{TimeoutAction, Timer},
};

use crate::{
    state::{Ferrite, PowerAction},
    system::Change,
};

const CHECK: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct Idle {
    pub settings: IdleSettings,
    /// Dernière entrée (clavier, souris, pavé tactile).
    last_input: Instant,
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
        // Une vidéo ou une présentation affichée (`zwp_idle_inhibit_v1`) vaut une présence.
        if !self.idle_inhibitors.is_empty() {
            self.idle.activity();
            return;
        }
        let now = Instant::now();
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
        self.lock_session();
        self.request_system_change(Change::Power(PowerAction::Sleep));
    }
}
