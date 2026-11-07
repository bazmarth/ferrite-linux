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
        media::MediaCommand,
        panels::{IslandHit, IslandKind, IslandView, NotificationsView},
    },
    state::Ferrite,
};

/// Temps pendant lequel l'îlot reste déplié après un clic.
const OPEN_FOR: Duration = Duration::from_secs(6);

/// Activité que l'îlot déplie : celle de la pastille, ou celle d'une bulle touchée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandFocus {
    Timer,
    Media,
}

impl IslandFocus {
    /// Activité qui se déplie, pour ce qu'affiche une pastille ou une bulle.
    fn of(kind: &IslandKind) -> Option<Self> {
        match kind {
            IslandKind::Timer { .. } => Some(Self::Timer),
            IslandKind::Media { .. } => Some(Self::Media),
            _ => None,
        }
    }

    /// Encore en cours ?
    fn matches(self, live: &[IslandKind]) -> bool {
        live.iter().any(|kind| Self::of(kind) == Some(self))
    }
}

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

/// Temps écoulé ou restant d'un morceau, à la seconde : « 1:05 », « 1:02:03 ».
pub fn track_label(seconds: u32) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Temps jusqu'à la prochaine seconde entière d'un morceau qui avance.
fn until_next_whole_second(position: Duration) -> Duration {
    Duration::from_nanos(1_000_000_000 - u64::from(position.subsec_nanos())) + Duration::from_millis(5)
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
        let media = self.media.current(now);
        // Activités en cours, de la plus pressante à la plus discrète.
        let mut live: Vec<IslandKind> = Vec::new();
        if let Some(recording) = &self.recording {
            live.push(IslandKind::Recording(crate::desktop::recording::elapsed_label(
                recording.elapsed(),
            )));
        }
        if let Some((left, paused, done, false)) = timer.clone() {
            live.push(IslandKind::Timer { left, paused, done });
        }
        // Lecteurs qui jouent : le suivi d'abord, puis chacun des autres dans sa bulle.
        let playing = media
            .filter(|media| media.playing)
            .into_iter()
            .chain(self.media.others_playing(now));
        live.extend(playing.map(|media| IslandKind::Media {
            playing: true,
            player: media.player.clone(),
        }));
        // Activité dépliée : celle choisie (une bulle), sinon la première qui se déplie. Le
        // lecteur déplié le reste une fois en pause, avec son bouton de lecture.
        let focus = self
            .island_focus
            .filter(|focus| focus.matches(&live) || (*focus == IslandFocus::Media && media.is_some()))
            .or_else(|| live.iter().find_map(IslandFocus::of));
        let kind = if let Some(osd) = self.osd_view() {
            Some(IslandKind::Osd(osd))
        } else if let Some(notification) = notifications.bubbles.first() {
            Some(IslandKind::Notification {
                notification: notification.clone(),
                more: notifications.bubbles.len() - 1,
            })
        } else if timer.as_ref().is_some_and(|timer| timer.3) {
            Some(IslandKind::TimerDone)
        } else if let (true, Some(IslandFocus::Timer), Some((left, paused, done, _))) =
            (opened, focus, timer.clone())
        {
            Some(IslandKind::TimerOpen { left, paused, done })
        } else if let (true, Some(IslandFocus::Media), Some(media)) = (opened, focus, media) {
            Some(IslandKind::MediaOpen {
                title: media.title.clone(),
                artist: media.artist.clone(),
                playing: media.playing,
                cover: media.cover.clone(),
                position: media.position_at(now).as_secs() as u32,
                length: media.length.as_secs() as u32,
            })
        } else if !live.is_empty() {
            // Repliée : la première activité dans la pastille, les autres en bulles.
            let main = live.remove(0);
            return IslandView {
                kind: Some(main),
                others: live,
            };
        } else {
            None
        };
        IslandView {
            kind,
            others: Vec::new(),
        }
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
        let view = self.island_view(&notifications);
        let Some(kind) = view.kind.clone() else {
            return;
        };
        // Bulle d'une autre activité : l'enregistrement s'arrête, le reste se déplie.
        if let IslandHit::Other(index) = hit {
            match view.others.get(index) {
                Some(IslandKind::Recording(_)) => self.stop_recording(),
                Some(IslandKind::Media { player, .. }) => {
                    // Un autre lecteur : c'est lui que l'îlot déplie et que ses boutons visent.
                    self.media.follow(player, Instant::now());
                    self.island_open(Some(IslandFocus::Media));
                }
                Some(other) => self.island_open(IslandFocus::of(other)),
                None => {}
            }
            self.request_redraw();
            return;
        }
        match (kind, hit) {
            // Premier clic : l'îlot se déplie avec ses boutons. Un clic sur l'îlot déplié
            // (hors boutons) ouvre l'application, et il se replie.
            (main @ (IslandKind::Timer { .. } | IslandKind::Media { .. }), _) => {
                self.island_open(IslandFocus::of(&main));
            }
            (IslandKind::TimerOpen { .. }, IslandHit::Body | IslandHit::Side) => {
                self.island_open_until = None;
                self.open_clock();
            }
            (IslandKind::MediaOpen { .. }, IslandHit::Body | IslandHit::Side) => {
                self.island_open_until = None;
                self.open_media_player();
            }
            (IslandKind::TimerDone, IslandHit::Body) => {
                self.island_timer = None;
                self.send_timer_command("arreter");
                self.open_clock();
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
                self.press_media(command);
                self.keep_island_open();
            }
            _ => {}
        }
        self.request_redraw();
    }

    /// Clic (gauche ou droit) sur la pastille repliée : elle se déplie avec ses boutons (pause,
    /// arrêter, morceau précédent ou suivant) pendant [`OPEN_FOR`].
    pub fn island_expand(&mut self) {
        let notifications = crate::render::desktop_view(self).notifications;
        let main = self.island_view(&notifications).kind;
        if let Some(focus) = main.as_ref().and_then(IslandFocus::of) {
            self.island_open(Some(focus));
        }
    }

    /// Déplie l'îlot sur `focus` (sinon sur la première activité) pendant [`OPEN_FOR`].
    fn island_open(&mut self, focus: Option<IslandFocus>) {
        self.island_focus = focus;
        self.island_open_until = Some(Instant::now() + OPEN_FOR);
        if focus == Some(IslandFocus::Media) {
            // Ses boutons visent le lecteur montré, même si un autre se met à jouer.
            self.media.hold(Instant::now());
        }
        self.island_media_tick();
        // Il se replie seul : une image de plus à l'échéance.
        let _ = self
            .loop_handle
            .insert_source(Timer::from_duration(OPEN_FOR), |_, _, state| {
                state.request_redraw();
                TimeoutAction::Drop
            });
        self.request_redraw();
    }

    /// L'Horloge, sur son minuteur.
    fn open_clock(&mut self) {
        self.show_app(&crate::desktop::apps::AppKey::Builtin(
            crate::desktop::apps::App::CLOCK,
        ));
    }

    /// La fenêtre du lecteur qui joue (Musique, Brave, VLC…), lancée s'il le faut.
    fn open_media_player(&mut self) {
        let Some(entry) = self.media().map(|media| media.entry.clone()) else {
            return;
        };
        let mut key = crate::desktop::apps::AppKey::for_window(&entry, &self.applications);
        if matches!(key, crate::desktop::apps::AppKey::Other(_)) && !self.app_running(&key) {
            // Brave ne donne pas son `DesktopEntry` : sa fenêtre « brave-browser » est
            // retrouvée par le nom du lecteur (« brave »).
            let name = entry.to_lowercase();
            let found = self.space.elements().find_map(|window| {
                let id = crate::render::app_id(window)?.to_lowercase();
                id.starts_with(&name).then(|| self.window_app(window)).flatten()
            });
            match found {
                Some(found) => key = found,
                None => return,
            }
        }
        self.show_app(&key);
    }

    /// Clic ailleurs que sur l'îlot déplié : il se replie aussitôt (minuteur ou musique
    /// ouverts, son, notification qui file dans l'historique). La fin d'un minuteur, elle,
    /// attend qu'on l'arrête. Le clic poursuit ensuite son chemin, sauf vers l'îlot replié
    /// (ses bulles n'étaient pas visibles).
    pub fn island_click_outside(&mut self, location: smithay::utils::Point<f64, smithay::utils::Logical>) {
        self.island_click_folded = false;
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
        self.island_click_folded = true;
        self.request_redraw();
    }

    /// Un bouton touché : l'îlot reste déplié encore un moment.
    fn keep_island_open(&mut self) {
        self.island_open_until = Some(Instant::now() + OPEN_FOR);
        self.island_media_tick();
    }

    /// L'îlot déplié montre un morceau qui avance : sa barre bouge à chaque seconde.
    fn island_media_moving(&self) -> Option<Duration> {
        let now = Instant::now();
        if !self.island_open_until.is_some_and(|until| now < until) {
            return None;
        }
        let media = self.media.current(now).filter(|media| media.playing)?;
        (!media.length.is_zero()).then(|| media.position_at(now))
    }

    /// Une image à chaque seconde du morceau, tant que l'îlot déplié le montre.
    pub(crate) fn island_media_tick(&mut self) {
        if self.media.ticking {
            return;
        }
        let Some(position) = self.island_media_moving() else {
            return;
        };
        self.media.ticking = true;
        let inserted = self.loop_handle.insert_source(
            Timer::from_duration(until_next_whole_second(position)),
            |_, _, state| {
                state.request_redraw();
                match state.island_media_moving() {
                    Some(position) => TimeoutAction::ToDuration(until_next_whole_second(position)),
                    None => {
                        state.media.ticking = false;
                        TimeoutAction::Drop
                    }
                }
            },
        );
        if inserted.is_err() {
            self.media.ticking = false;
        }
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
    fn temps_d_un_morceau() {
        assert_eq!(track_label(0), "0:00");
        assert_eq!(track_label(65), "1:05");
        assert_eq!(track_label(3723), "1:02:03");
        assert_eq!(
            until_next_whole_second(Duration::from_millis(4_300)),
            Duration::from_millis(705)
        );
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
