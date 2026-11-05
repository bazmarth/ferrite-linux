//! Horloge : heure locale, horloges du monde, minuteur et chronomètre.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, CornerRadius, FontId, Frame, Margin, RichText};

use crate::style;

/// Villes de l'horloge mondiale : nom et décalage fixe par rapport à l'UTC, en secondes.
/// Décalages d'hiver ; l'heure d'été n'est pas encore appliquée (voir la limite en fin de
/// journal).
const WORLD: [(&str, i64); 7] = [
    ("Temps universel", 0),
    ("Paris", 3600),
    ("Londres", 0),
    ("New York", -5 * 3600),
    ("Los Angeles", -8 * 3600),
    ("Tokyo", 9 * 3600),
    ("Sydney", 10 * 3600),
];

const MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

const WEEKDAYS: [&str; 7] = [
    "dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Horloge,
    Minuteur,
    Chronomètre,
}

/// Phase d'un compte à rebours.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Réglage de la durée, avant tout départ.
    Idle,
    Running,
    Paused,
    /// Arrivé à zéro : le message « Temps écoulé » attend d'être arrêté.
    Ended,
}

#[derive(Debug)]
struct Timer {
    hours: u32,
    minutes: u32,
    seconds: u32,
    phase: Phase,
    remaining: Duration,
    last: Option<Instant>,
}

impl Default for Timer {
    fn default() -> Self {
        Self {
            hours: 0,
            minutes: 5,
            seconds: 0,
            phase: Phase::Idle,
            remaining: Duration::ZERO,
            last: None,
        }
    }
}

impl Timer {
    fn total(&self) -> Duration {
        Duration::from_secs(self.hours as u64 * 3600 + self.minutes as u64 * 60 + self.seconds as u64)
    }

    /// Durée affichée en grand : le réglage avant départ, le reste pendant et après une pause.
    fn displayed(&self) -> Duration {
        match self.phase {
            Phase::Idle => self.total(),
            Phase::Running | Phase::Paused => self.remaining,
            Phase::Ended => Duration::ZERO,
        }
    }

    fn start(&mut self) {
        if self.total().is_zero() {
            return;
        }
        self.remaining = self.total();
        self.phase = Phase::Running;
        self.last = Some(Instant::now());
    }

    fn pause(&mut self) {
        if self.phase == Phase::Running {
            self.phase = Phase::Paused;
            self.last = None;
        }
    }

    fn resume(&mut self) {
        if self.phase == Phase::Paused {
            self.phase = Phase::Running;
            self.last = Some(Instant::now());
        }
    }

    fn stop_alert(&mut self) {
        if self.phase == Phase::Ended {
            self.phase = Phase::Idle;
            self.remaining = Duration::ZERO;
        }
    }

    fn reset(&mut self) {
        self.phase = Phase::Idle;
        self.remaining = Duration::ZERO;
        self.last = None;
    }

    /// Avance le compte à rebours de `delta` (séparé d'`advance` pour les tests).
    fn tick(&mut self, delta: Duration) {
        if self.phase != Phase::Running {
            return;
        }
        if self.remaining > delta {
            self.remaining -= delta;
        } else {
            self.remaining = Duration::ZERO;
            self.phase = Phase::Ended;
            self.last = None;
        }
    }

    /// Met à jour le compte à rebours depuis le dernier passage (temps réel).
    fn advance(&mut self) {
        if self.phase != Phase::Running {
            return;
        }
        let now = Instant::now();
        let Some(last) = self.last else {
            self.last = Some(now);
            return;
        };
        self.last = Some(now);
        self.tick(now - last);
    }
}

#[derive(Debug, Default)]
struct Stopwatch {
    elapsed: Duration,
    running: bool,
    started: Option<Instant>,
    /// Temps cumulé de chaque tour.
    laps: Vec<Duration>,
}

impl Stopwatch {
    fn current(&self) -> Duration {
        let mut total = self.elapsed;
        if let Some(started) = self.started {
            total += started.elapsed();
        }
        total
    }

    fn start(&mut self) {
        if !self.running {
            self.running = true;
            self.started = Some(Instant::now());
        }
    }

    fn stop(&mut self) {
        if self.running {
            self.elapsed = self.current();
            self.running = false;
            self.started = None;
        }
    }

    fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
        self.running = false;
        self.started = None;
        self.laps.clear();
    }

    fn lap(&mut self) {
        self.laps.push(self.current());
    }
}

#[derive(Debug)]
pub struct ClockState {
    tab: Tab,
    /// Décalage local par rapport à l'UTC, en secondes (lu avec `date +%z`).
    offset: i64,
    offset_checked: Option<Instant>,
    timer: Timer,
    stopwatch: Stopwatch,
}

impl Default for ClockState {
    fn default() -> Self {
        Self {
            tab: Tab::Horloge,
            offset: 0,
            offset_checked: None,
            timer: Timer::default(),
            stopwatch: Stopwatch::default(),
        }
    }
}

impl ClockState {
    /// Rafraîchit le décalage local toutes les minutes (l'heure d'été peut basculer).
    fn refresh_offset(&mut self) {
        if self
            .offset_checked
            .is_some_and(|checked| checked.elapsed() < Duration::from_secs(60))
        {
            return;
        }
        self.offset = ferrite_system::command::run("date", &["+%z"])
            .ok()
            .and_then(|out| parse_offset(out.trim()))
            .unwrap_or(0);
        self.offset_checked = Some(Instant::now());
    }

    /// Intervalle au bout duquel redemander un dessin, selon ce qui doit avancer.
    fn refresh_interval(&self) -> Option<Duration> {
        if self.stopwatch.running {
            Some(Duration::from_millis(40))
        } else if self.timer.phase == Phase::Running || self.tab == Tab::Horloge {
            Some(Duration::from_millis(200))
        } else {
            None
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        self.refresh_offset();
        self.timer.advance();
        if let Some(interval) = self.refresh_interval() {
            ui.ctx().request_repaint_after(interval);
        }

        style::header(ui, |_| {});
        egui::CentralPanel::no_frame()
            .frame(
                Frame::new()
                    .fill(style::background())
                    .inner_margin(Margin::same(18)),
            )
            .show(ui, |ui| {
                self.tabs(ui);
                ui.add_space(18.0);
                match self.tab {
                    Tab::Horloge => self.clock(ui),
                    Tab::Minuteur => self.timer(ui),
                    Tab::Chronomètre => self.stopwatch(ui),
                }
            });
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (tab, label) in [
                (Tab::Horloge, "Horloge"),
                (Tab::Minuteur, "Minuteur"),
                (Tab::Chronomètre, "Chronomètre"),
            ] {
                let selected = self.tab == tab;
                let ink = if selected {
                    style::accent_text()
                } else {
                    style::primary_text()
                };
                let fill = if selected {
                    style::accent()
                } else {
                    style::control_background()
                };
                let button = egui::Button::new(RichText::new(label).size(14.0).color(ink))
                    .fill(fill)
                    .corner_radius(CornerRadius::same(10));
                if ui.add(button).clicked() {
                    self.tab = tab;
                }
            }
        });
    }

    fn clock(&mut self, ui: &mut egui::Ui) {
        let local = unix_seconds() + self.offset;
        let (year, month, day) = civil_from_days(local.div_euclid(86_400));
        let second_of_day = local.rem_euclid(86_400);
        let time = format!(
            "{:02}:{:02}:{:02}",
            second_of_day / 3600,
            (second_of_day % 3600) / 60,
            second_of_day % 60
        );
        let date = format!(
            "{} {} {} {}",
            WEEKDAYS[weekday(local.div_euclid(86_400)) as usize],
            day,
            MONTHS[month as usize - 1],
            year
        );

        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(time)
                    .font(FontId::new(58.0, style::mono_bold_family()))
                    .color(style::primary_text()),
            );
            ui.label(RichText::new(date).size(16.0).color(style::text_secondary()));
        });

        ui.add_space(18.0);
        ui.separator();
        ui.add_space(10.0);

        let local_days = local.div_euclid(86_400);
        Frame::new()
            .fill(style::content_gray())
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                for (name, offset) in WORLD {
                    let city = unix_seconds() + offset;
                    let city_days = city.div_euclid(86_400);
                    let day_shift = city_days - local_days;
                    let second_of_day = city.rem_euclid(86_400);
                    let hour = format!("{:02}:{:02}", second_of_day / 3600, (second_of_day % 3600) / 60);
                    let detail = match day_shift {
                        0 => format_offset(offset),
                        -1 => format!("{} · hier", format_offset(offset)),
                        1 => format!("{} · demain", format_offset(offset)),
                        _ => format_offset(offset),
                    };
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [140.0, 20.0],
                            egui::Label::new(RichText::new(name).color(style::primary_text())),
                        );
                        ui.add_sized(
                            [72.0, 26.0],
                            egui::Label::new(
                                RichText::new(hour)
                                    .font(FontId::new(20.0, style::mono_bold_family()))
                                    .color(style::primary_text()),
                            ),
                        );
                        ui.label(RichText::new(detail).size(13.0).color(style::text_secondary()));
                    });
                    ui.add_space(6.0);
                }
            });
    }

    fn timer(&mut self, ui: &mut egui::Ui) {
        let displayed = self.timer.displayed();
        let fraction = if self.timer.total().is_zero() {
            0.0
        } else {
            (displayed.as_secs_f64() / self.timer.total().as_secs_f64()).clamp(0.0, 1.0)
        };

        ui.vertical_centered(|ui| {
            let (text, color) = if self.timer.phase == Phase::Ended {
                ("Temps écoulé".to_owned(), style::error())
            } else {
                (format_hms(displayed.as_secs()), style::primary_text())
            };
            ui.label(
                RichText::new(text)
                    .font(FontId::new(52.0, style::mono_bold_family()))
                    .color(color),
            );
            ui.add(egui::ProgressBar::new(fraction as f32).desired_width(260.0).fill(
                if self.timer.phase == Phase::Ended {
                    style::error()
                } else {
                    style::accent()
                },
            ));
        });

        ui.add_space(18.0);

        if self.timer.phase == Phase::Idle {
            ui.horizontal(|ui| {
                for (label, value, range) in [
                    ("h", &mut self.timer.hours, 0..=99),
                    ("min", &mut self.timer.minutes, 0..=59),
                    ("s", &mut self.timer.seconds, 0..=59),
                ] {
                    ui.label(RichText::new(label).color(style::text_secondary()));
                    ui.add(egui::DragValue::new(value).range(range).speed(1));
                    ui.add_space(10.0);
                }
            });
        }

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let primary = match self.timer.phase {
                Phase::Idle => ("Démarrer", !self.timer.total().is_zero()),
                Phase::Running => ("Pause", true),
                Phase::Paused => ("Reprendre", true),
                Phase::Ended => ("Arrêter", true),
            };
            let (label, enabled) = primary;
            let button = egui::Button::new(RichText::new(label).size(15.0).color(style::accent_text()))
                .fill(style::accent())
                .corner_radius(CornerRadius::same(10))
                .min_size(egui::vec2(120.0, 36.0));
            if ui.add_enabled(enabled, button).clicked() {
                match self.timer.phase {
                    Phase::Idle => self.timer.start(),
                    Phase::Running => self.timer.pause(),
                    Phase::Paused => self.timer.resume(),
                    Phase::Ended => self.timer.stop_alert(),
                }
            }
            let reset = egui::Button::new(
                RichText::new("Réinitialiser")
                    .size(15.0)
                    .color(style::primary_text()),
            )
            .fill(style::control_background())
            .corner_radius(CornerRadius::same(10))
            .min_size(egui::vec2(120.0, 36.0));
            if ui.add(reset).clicked() {
                self.timer.reset();
            }
        });
    }

    fn stopwatch(&mut self, ui: &mut egui::Ui) {
        let current = self.stopwatch.current();
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(format_centis(current))
                    .font(FontId::new(52.0, style::mono_bold_family()))
                    .color(style::primary_text()),
            );
        });

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            let (label, running) = if self.stopwatch.running {
                ("Arrêter", true)
            } else {
                ("Démarrer", true)
            };
            let start_stop = egui::Button::new(RichText::new(label).size(15.0).color(style::accent_text()))
                .fill(style::accent())
                .corner_radius(CornerRadius::same(10))
                .min_size(egui::vec2(110.0, 36.0));
            if ui.add(start_stop).clicked() {
                if self.stopwatch.running {
                    self.stopwatch.stop();
                } else {
                    self.stopwatch.start();
                }
            }
            let lap = egui::Button::new(RichText::new("Tour").size(15.0).color(style::primary_text()))
                .fill(style::control_background())
                .corner_radius(CornerRadius::same(10))
                .min_size(egui::vec2(90.0, 36.0));
            if ui.add_enabled(running, lap).clicked() {
                self.stopwatch.lap();
            }
            let reset = egui::Button::new(
                RichText::new("Réinitialiser")
                    .size(15.0)
                    .color(style::primary_text()),
            )
            .fill(style::control_background())
            .corner_radius(CornerRadius::same(10))
            .min_size(egui::vec2(110.0, 36.0));
            if ui.add(reset).clicked() {
                self.stopwatch.reset();
            }
        });

        if !self.stopwatch.laps.is_empty() {
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (index, lap) in lap_durations(&self.stopwatch.laps).iter().rev().enumerate() {
                        let number = self.stopwatch.laps.len() - index;
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [60.0, 20.0],
                                egui::Label::new(
                                    RichText::new(format!("Tour {number}")).color(style::text_secondary()),
                                ),
                            );
                            ui.label(
                                RichText::new(format_centis(*lap))
                                    .font(FontId::new(15.0, style::mono_bold_family()))
                                    .color(style::primary_text()),
                            );
                        });
                    }
                });
        }
    }
}

/// Secondes écoulées depuis le 1er janvier 1970 (UTC).
fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

/// Décalage « +HHMM » ou « -HHMM » (sortie de `date +%z`) en secondes.
fn parse_offset(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() != 5 || (bytes[0] != b'+' && bytes[0] != b'-') {
        return None;
    }
    let hours = (bytes[1] as i64 - b'0' as i64) * 10 + (bytes[2] as i64 - b'0' as i64);
    let minutes = (bytes[3] as i64 - b'0' as i64) * 10 + (bytes[4] as i64 - b'0' as i64);
    if hours > 23 || minutes > 59 {
        return None;
    }
    let seconds = hours * 3600 + minutes * 60;
    Some(if bytes[0] == b'+' { seconds } else { -seconds })
}

/// Date civile (année, mois 1-12, jour) à partir du nombre de jours depuis 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// Jours depuis 1970-01-01 correspondant à une date civile (inverse de [`civil_from_days`]).
#[cfg(test)]
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Jour de la semaine : 0 = dimanche, 1 = lundi, … 6 = samedi.
fn weekday(days: i64) -> u32 {
    (days + 4).rem_euclid(7) as u32
}

/// « HH:MM:SS ».
fn format_hms(total_seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        total_seconds / 3600,
        (total_seconds % 3600) / 60,
        total_seconds % 60
    )
}

/// « HH:MM:SS.cc » pour le chronomètre.
fn format_centis(duration: Duration) -> String {
    let total = duration.as_secs();
    let centis = duration.subsec_millis() / 10;
    format!(
        "{:02}:{:02}:{:02}.{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60,
        centis
    )
}

/// « +01:00 » ou « -05:00 ».
fn format_offset(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let absolute = offset.unsigned_abs();
    format!("{sign}{:02}:{:02}", absolute / 3600, (absolute % 3600) / 60)
}

/// Durée de chaque tour à partir des temps cumulés.
fn lap_durations(laps: &[Duration]) -> Vec<Duration> {
    let mut previous = Duration::ZERO;
    laps.iter()
        .map(|lap| {
            let duration = *lap - previous;
            previous = *lap;
            duration
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_le_decalage_horaire() {
        assert_eq!(parse_offset("+0100"), Some(3600));
        assert_eq!(parse_offset("-0530"), Some(-19_800));
        assert_eq!(parse_offset("+0000"), Some(0));
        assert_eq!(parse_offset("+2400"), None);
        assert_eq!(parse_offset("1000"), None);
        assert_eq!(parse_offset("+0160"), None);
    }

    #[test]
    fn dates_civiles_connues() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(weekday(0), 4); // 1970-01-01 était un jeudi
        assert_eq!(weekday(10_957), 6); // 2000-01-01 était un samedi
    }

    #[test]
    fn aller_retour_date_civile() {
        for days in [0, 1, 365, 10_957, 20_000, -1, -10_000, 73_000] {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days, "{days}");
        }
    }

    #[test]
    fn formate_les_durees() {
        assert_eq!(format_hms(0), "00:00:00");
        assert_eq!(format_hms(3661), "01:01:01");
        assert_eq!(format_hms(86_399), "23:59:59");
        assert_eq!(format_centis(Duration::from_millis(61_010)), "00:01:01.01");
        assert_eq!(format_offset(3600), "+01:00");
        assert_eq!(format_offset(-19_800), "-05:30");
    }

    #[test]
    fn le_minuteur_s_arrete_a_zero() {
        let mut timer = Timer {
            minutes: 1,
            ..Timer::default()
        };
        assert_eq!(timer.displayed().as_secs(), 60);
        timer.start();
        assert_eq!(timer.phase, Phase::Running);
        timer.tick(Duration::from_secs(59));
        assert_eq!(timer.phase, Phase::Running);
        assert_eq!(timer.remaining.as_secs(), 1);
        timer.tick(Duration::from_secs(1));
        assert_eq!(timer.phase, Phase::Ended);
        assert_eq!(timer.displayed().as_secs(), 0);
        timer.stop_alert();
        assert_eq!(timer.phase, Phase::Idle);
    }

    #[test]
    fn pause_et_reprise_du_minuteur() {
        let mut timer = Timer {
            minutes: 1,
            ..Timer::default()
        };
        timer.start();
        timer.tick(Duration::from_secs(10));
        timer.pause();
        assert_eq!(timer.phase, Phase::Paused);
        let frozen = timer.remaining;
        timer.tick(Duration::from_secs(5));
        assert_eq!(timer.remaining, frozen);
        timer.resume();
        assert_eq!(timer.phase, Phase::Running);
    }

    #[test]
    fn les_tours_sont_les_ecarts_entre_cumuls() {
        let laps = vec![
            Duration::from_secs(10),
            Duration::from_secs(24),
            Duration::from_secs(24),
        ];
        assert_eq!(
            lap_durations(&laps),
            vec![Duration::from_secs(10), Duration::from_secs(14), Duration::ZERO,]
        );
    }
}
