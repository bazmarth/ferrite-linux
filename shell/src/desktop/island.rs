//! Îlot du haut de l'écran, côté état : ce qu'il montre, le minuteur de l'Horloge qu'il
//! suit, et ses clics. Le dessin est dans `panels/island.rs`.
//!
//! L'Horloge annonce son minuteur sur le bus de session (`org.ferrite.Ilot`, servi avec les
//! notifications) à chaque changement de phase ; l'îlot compte ensuite lui-même les
//! secondes, et lui renvoie Pause, Reprendre et Arrêter par le signal `Commande`.

use std::time::{Duration, Instant};

use smithay::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::{
    desktop::{
        lock::MediaCommand,
        panels::{IslandHit, IslandKind, IslandView, NotificationsView},
    },
    state::Ferrite,
};

/// Temps pendant lequel l'îlot reste déplié après un clic.
const OPEN_FOR: Duration = Duration::from_secs(6);

/// Minuteur de l'Horloge, vu par l'îlot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerActivity {
    /// Fin prévue, pendant qu'il tourne.
    pub end: Option<Instant>,
    /// Reste, quand il est en pause.
    pub remaining: Duration,
    pub total: Duration,
    /// Arrivé à zéro : l'îlot l'annonce jusqu'à ce qu'on l'arrête.
    pub ended: bool,
}

impl TimerActivity {
    fn left(&self, now: Instant) -> Duration {
        self.end
            .map_or(self.remaining, |end| end.saturating_duration_since(now))
    }

    /// Part écoulée, sur 1000.
    fn done(&self, now: Instant) -> u16 {
        if self.total.is_zero() {
            return 0;
        }
        let left = self.left(now).as_secs_f64() / self.total.as_secs_f64();
        ((1.0 - left).clamp(0.0, 1.0) * 1000.0).round() as u16
    }
}

/// Reste d'un minuteur, arrondi à la seconde supérieure : « 4:59 », « 1:02:03 ».
pub fn timer_label(left: Duration) -> String {
    let seconds = left.as_millis().div_ceil(1000) as u64;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Temps jusqu'au prochain changement de la seconde affichée.
fn until_next_second(left: Duration) -> Duration {
    let fraction = left.subsec_nanos();
    let wait = if fraction == 0 {
        Duration::from_secs(1)
    } else {
        Duration::from_nanos(u64::from(fraction))
    };
    wait + Duration::from_millis(5)
}

impl Ferrite {
    /// Ce que montre l'îlot, du plus pressant au plus discret.
    pub fn island_view(&self, notifications: &NotificationsView) -> IslandView {
        let now = Instant::now();
        // Écran verrouillé, panneau ouvert, mot de passe demandé : l'îlot s'efface.
        if self.lock.is_some() || self.popover.is_some() || !self.auth.is_empty() {
            return IslandView::default();
        }
        let timer = self.island_timer.map(|timer| {
            (
                timer_label(timer.left(now)),
                timer.end.is_none(),
                timer.done(now),
                timer.ended,
            )
        });
        let opened = self.island_open_until.is_some_and(|until| now < until);
        let media = self.media.as_ref();
        let kind = if let Some(osd) = self.osd_view() {
            Some(IslandKind::Osd(osd))
        } else if let Some(notification) = notifications.bubbles.first() {
            Some(IslandKind::Notification {
                notification: notification.clone(),
                more: notifications.bubbles.len() - 1,
            })
        } else if timer.as_ref().is_some_and(|timer| timer.3) {
            Some(IslandKind::TimerDone)
        } else if let (true, Some((left, paused, done, _))) = (opened, timer.clone()) {
            Some(IslandKind::TimerOpen { left, paused, done })
        } else if let (true, Some(media)) = (opened, media) {
            Some(IslandKind::MediaOpen {
                title: media.title.clone(),
                artist: media.artist.clone(),
                playing: media.playing,
                cover: media.cover.clone(),
            })
        } else if let Some(recording) = &self.recording {
            Some(IslandKind::Recording(crate::desktop::recording::elapsed_label(
                recording.elapsed(),
            )))
        } else if let Some((left, paused, done, _)) = timer {
            Some(IslandKind::Timer { left, paused, done })
        } else {
            media
                .filter(|media| media.playing)
                .map(|media| IslandKind::Media {
                    playing: media.playing,
                })
        };
        IslandView { kind }
    }

    /// L'Horloge annonce son minuteur : « marche », « pause », « fini » ou « arret ».
    pub fn on_timer_report(&mut self, phase: &str, remaining: Duration, total: Duration) {
        let now = Instant::now();
        self.island_timer = match phase {
            "marche" => Some(TimerActivity {
                end: Some(now + remaining),
                remaining,
                total,
                ended: false,
            }),
            "pause" => Some(TimerActivity {
                end: None,
                remaining,
                total,
                ended: false,
            }),
            // L'îlot a souvent vu la fin le premier : l'annonce reste la même.
            "fini" => Some(TimerActivity {
                end: None,
                remaining: Duration::ZERO,
                total,
                ended: true,
            }),
            _ => None,
        };
        self.island_tick();
        self.request_redraw();
    }

    /// Une image à chaque seconde affichée, tant que le minuteur tourne.
    fn island_tick(&mut self) {
        let Some(end) = self.island_timer.and_then(|timer| timer.end) else {
            return;
        };
        if self.island_ticking {
            return;
        }
        self.island_ticking = true;
        let first = until_next_second(end.saturating_duration_since(Instant::now()));
        let inserted = self
            .loop_handle
            .insert_source(Timer::from_duration(first), |_, _, state| {
                state.request_redraw();
                let now = Instant::now();
                let Some(timer) = state.island_timer.as_mut() else {
                    state.island_ticking = false;
                    return TimeoutAction::Drop;
                };
                let Some(end) = timer.end else {
                    state.island_ticking = false;
                    return TimeoutAction::Drop;
                };
                if now >= end {
                    timer.end = None;
                    timer.remaining = Duration::ZERO;
                    timer.ended = true;
                    state.island_ticking = false;
                    return TimeoutAction::Drop;
                }
                TimeoutAction::ToDuration(until_next_second(end - now))
            });
        if inserted.is_err() {
            self.island_ticking = false;
        }
    }

    /// Clic dans l'îlot.
    pub fn island_press(&mut self, hit: IslandHit) {
        let notifications = crate::render::desktop_view(self).notifications;
        let Some(kind) = self.island_view(&notifications).kind else {
            return;
        };
        match (kind, hit) {
            (IslandKind::Timer { .. } | IslandKind::Media { .. }, _) => {
                let until = Instant::now() + OPEN_FOR;
                self.island_open_until = Some(until);
                // Il se replie seul : une image de plus à l'échéance.
                let _ = self
                    .loop_handle
                    .insert_source(Timer::from_duration(OPEN_FOR), |_, _, state| {
                        state.request_redraw();
                        TimeoutAction::Drop
                    });
            }
            (IslandKind::TimerDone, _) | (IslandKind::TimerOpen { .. }, IslandHit::Button(1)) => {
                self.island_timer = None;
                self.island_open_until = None;
                self.send_timer_command("arreter");
            }
            (IslandKind::TimerOpen { paused, .. }, IslandHit::Button(_)) => {
                self.send_timer_command(if paused { "reprendre" } else { "pause" });
                self.keep_island_open();
            }
            (IslandKind::MediaOpen { .. }, IslandHit::Button(index)) => {
                let command = match index {
                    0 => MediaCommand::Previous,
                    1 => MediaCommand::PlayPause,
                    _ => MediaCommand::Next,
                };
                self.lock_media(command);
                self.keep_island_open();
            }
            (IslandKind::TimerOpen { .. } | IslandKind::MediaOpen { .. }, _) => {
                self.island_open_until = None;
            }
            _ => {}
        }
        self.request_redraw();
    }

    /// Clic ailleurs que sur l'îlot déplié : il se replie aussitôt (minuteur ou musique
    /// ouverts, son, notification qui file dans l'historique). La fin d'un minuteur, elle,
    /// attend qu'on l'arrête. Le clic poursuit ensuite son chemin.
    pub fn island_click_outside(&mut self, location: smithay::utils::Point<f64, smithay::utils::Logical>) {
        let view = crate::render::desktop_view(self);
        let Some(kind) = view.island.kind.as_ref() else {
            return;
        };
        if kind.compact() || matches!(kind, IslandKind::TimerDone) {
            return;
        }
        let Some(area) = self.screen_area() else {
            return;
        };
        if crate::desktop::panels::island_contains(area, location, &view) {
            return;
        }
        match kind {
            IslandKind::Osd(_) => self.osd = None,
            IslandKind::Notification { .. } => {
                self.notifications.hide_bubbles();
            }
            _ => self.island_open_until = None,
        }
        self.request_redraw();
    }

    /// Un bouton touché : l'îlot reste déplié encore un moment.
    fn keep_island_open(&mut self) {
        self.island_open_until = Some(Instant::now() + OPEN_FOR);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reste_arrondi_a_la_seconde_superieure() {
        assert_eq!(timer_label(Duration::from_millis(299_100)), "5:00");
        assert_eq!(timer_label(Duration::from_secs(299)), "4:59");
        assert_eq!(timer_label(Duration::from_secs(3723)), "1:02:03");
        assert_eq!(timer_label(Duration::ZERO), "0:00");
    }

    #[test]
    fn part_ecoulee_du_minuteur() {
        let now = Instant::now();
        let timer = TimerActivity {
            end: None,
            remaining: Duration::from_secs(30),
            total: Duration::from_secs(120),
            ended: false,
        };
        assert_eq!(timer.done(now), 750);
    }

    #[test]
    fn la_prochaine_image_tombe_sur_la_seconde() {
        assert_eq!(
            until_next_second(Duration::from_millis(4_300)),
            Duration::from_millis(305)
        );
        assert_eq!(
            until_next_second(Duration::from_secs(4)),
            Duration::from_millis(1005)
        );
    }
}
