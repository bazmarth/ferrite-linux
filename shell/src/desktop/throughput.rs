//! Débit réseau en temps réel, affiché dans la barre du haut (réglage `barre_debit`).
//!
//! Les compteurs de `/proc/net/dev` sont relus chaque seconde dans la boucle du
//! compositeur : c'est un fichier du noyau, sa lecture ne bloque pas. La barre n'est
//! redessinée que si le texte affiché change.

use std::{fs, time::Duration, time::Instant};

use smithay::reexports::calloop::{
    LoopHandle,
    timer::{TimeoutAction, Timer},
};

use crate::state::Ferrite;

/// Intervalle entre deux mesures.
const STEP: Duration = Duration::from_secs(1);

/// Mesure en cours : derniers compteurs lus et débit qui en découle.
#[derive(Debug, Default)]
pub struct Throughput {
    previous: Option<(Instant, u64, u64)>,
    /// Octets reçus et envoyés par seconde ; `None` avant la deuxième mesure.
    pub rate: Option<(u64, u64)>,
}

impl Throughput {
    /// Nouvelle mesure ; vrai si le texte affiché change.
    fn sample(&mut self) -> bool {
        let Some((received, sent)) = counters() else {
            return std::mem::take(&mut self.rate).is_some();
        };
        let now = Instant::now();
        let rate = self.previous.and_then(|(at, previous_received, previous_sent)| {
            let elapsed = now.duration_since(at).as_secs_f64();
            (elapsed > 0.0).then(|| {
                (
                    (received.saturating_sub(previous_received) as f64 / elapsed) as u64,
                    (sent.saturating_sub(previous_sent) as f64 / elapsed) as u64,
                )
            })
        });
        self.previous = Some((now, received, sent));
        let shown = |rate: Option<(u64, u64)>| rate.map(|(down, up)| (label(down), label(up)));
        let changed = shown(rate) != shown(self.rate);
        self.rate = rate;
        changed
    }

    /// Réglage coupé : plus rien d'affiché, la prochaine mesure repart de zéro.
    fn reset(&mut self) -> bool {
        self.previous = None;
        self.rate.take().is_some()
    }
}

/// Mesure le débit chaque seconde tant que le réglage est actif.
pub fn start(handle: &LoopHandle<'static, Ferrite>) -> Result<(), String> {
    handle
        .insert_source(Timer::from_duration(STEP), |_, _, state| {
            let changed = if state.dock_config.network_rate {
                state.throughput.sample()
            } else {
                state.throughput.reset()
            };
            if changed && state.bar_visible() {
                state.request_redraw();
            }
            TimeoutAction::ToDuration(STEP)
        })
        .map(drop)
        .map_err(|error| error.to_string())
}

/// Octets reçus et envoyés par toutes les interfaces, sauf la boucle locale.
pub fn counters() -> Option<(u64, u64)> {
    parse(&fs::read_to_string("/proc/net/dev").ok()?)
}

fn parse(text: &str) -> Option<(u64, u64)> {
    text.lines()
        .skip(2)
        .filter_map(|line| {
            let (interface, counters) = line.split_once(':')?;
            if interface.trim() == "lo" {
                return None;
            }
            let counters: Vec<u64> = counters
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            Some((*counters.first()?, *counters.get(8)?))
        })
        .fold(None, |totals, (received, sent)| {
            Some(totals.map_or((received, sent), |(rx, tx)| (rx + received, tx + sent)))
        })
}

/// Débit lisible : « 0 Ko/s », « 340 Ko/s », « 2,4 Mo/s », « 18 Mo/s », « 1,1 Go/s ».
pub fn label(bytes_per_second: u64) -> String {
    const KIB: f64 = 1024.0;
    let kib = bytes_per_second as f64 / KIB;
    if kib < 999.5 {
        return format!("{} Ko/s", kib.round() as u64);
    }
    let (value, unit) = if kib < 999.5 * KIB {
        (kib / KIB, "Mo/s")
    } else {
        (kib / KIB / KIB, "Go/s")
    };
    if value < 9.95 {
        format!("{} {unit}", format!("{value:.1}").replace('.', ","))
    } else {
        format!("{} {unit}", value.round() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_rounds_and_uses_french_units() {
        assert_eq!(label(0), "0 Ko/s");
        assert_eq!(label(340 * 1024), "340 Ko/s");
        assert_eq!(label(1000 * 1024), "1,0 Mo/s");
        assert_eq!(label(2_516_582), "2,4 Mo/s");
        assert_eq!(label(18 * 1024 * 1024), "18 Mo/s");
        assert_eq!(label(1_181_116_006), "1,1 Go/s");
    }

    #[test]
    fn parse_skips_loopback_and_sums_interfaces() {
        let text = "Inter-|   Receive                                                |  Transmit\n \
             face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    \
             lo: 5000 10 0 0 0 0 0 0 5000 10 0 0 0 0 0 0\n  \
             wlp2s0: 1000 5 0 0 0 0 0 0 200 3 0 0 0 0 0 0\n  \
             enp3s0: 30 1 0 0 0 0 0 0 4 1 0 0 0 0 0 0\n";
        assert_eq!(parse(text), Some((1030, 204)));
    }
}
