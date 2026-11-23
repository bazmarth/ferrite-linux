//! Horloge : heure locale, horloges du monde, minuteur et chronomètre.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, CornerRadius, FontId, Frame, Margin, RichText};

use crate::{kit, style};

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
    /// Îlot du haut de l'écran, ouvert au premier dessin.
    island: Option<crate::island::IslandLink>,
}

impl Default for ClockState {
    fn default() -> Self {
        Self {
            tab: Tab::Horloge,
            offset: 0,
            offset_checked: None,
            timer: Timer::default(),
            stopwatch: Stopwatch::default(),
            island: None,
        }
    }
}

impl ClockState {
    /// Horloge ouverte sur l'onglet nommé (« minuteur », « chronometre »), sinon l'heure.
    pub fn with_tab(name: Option<&str>) -> Self {
        let tab = match name.map(str::to_lowercase).as_deref() {
            Some("minuteur") => Tab::Minuteur,
            Some("chronometre" | "chronomètre") => Tab::Chronomètre,
            _ => Tab::Horloge,
        };
        Self {
            tab,
            ..Self::default()
        }
    }

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

    /// Prochain dessin utile : la seconde suivante pour l'heure et le minuteur, un
    /// centième affiché sur deux pour le chronomètre, rien quand tout est à l'arrêt.
    fn refresh_interval(&self) -> Option<Duration> {
        if self.stopwatch.running && self.tab == Tab::Chronomètre {
            return Some(Duration::from_millis(50));
        }
        let to_next_second = || {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
            Duration::from_millis(1000 - u64::from(now.subsec_millis()) + 5)
        };
        if self.timer.phase == Phase::Running {
            return Some(Duration::from_millis(250));
        }
        if self.tab == Tab::Horloge {
            return Some(to_next_second());
        }
        // Chronomètre en marche sur un autre onglet : pas besoin de dessiner.
        None
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        self.refresh_offset();
        self.island_commands(ui.ctx());
        self.timer.advance();
        if let Some(interval) = self.refresh_interval() {
            ui.ctx().request_repaint_after(interval);
        }

        style::header(ui, |ui| self.tabs(ui));
        // Thème « Fusion » : barre sur le chrome, contenu dans la carte encastrée.
        style::inset_panel(ui, true, false, |ui| {
            egui::CentralPanel::no_frame()
                .frame(
                    Frame::new()
                        .fill(style::background())
                        .inner_margin(Margin::symmetric(22, 18)),
                )
                .show(ui, |ui| match self.tab {
                    Tab::Horloge => self.clock(ui),
                    Tab::Minuteur => self.timer(ui),
                    Tab::Chronomètre => self.stopwatch(ui),
                });
        });
        self.report_to_island();
    }

    /// Boutons touchés dans l'îlot depuis le dernier dessin.
    fn island_commands(&mut self, ctx: &egui::Context) {
        let island = self
            .island
            .get_or_insert_with(|| crate::island::IslandLink::start(ctx.clone()));
        let commands: Vec<_> = island.commands.try_iter().collect();
        for command in commands {
            match command {
                crate::island::Command::Pause => self.timer.pause(),
                crate::island::Command::Resume => self.timer.resume(),
                crate::island::Command::Stop if self.timer.phase == Phase::Ended => self.timer.stop_alert(),
                crate::island::Command::Stop => self.timer.reset(),
            }
        }
    }

    /// Annonce le minuteur à l'îlot quand il change de phase.
    fn report_to_island(&mut self) {
        let Some(island) = self.island.as_mut() else {
            return;
        };
        let phase = match self.timer.phase {
            Phase::Idle => "arret",
            Phase::Running => "marche",
            Phase::Paused => "pause",
            Phase::Ended => "fini",
        };
        let millis = |duration: Duration| duration.as_millis().min(u128::from(u64::MAX)) as u64;
        island.report(phase, millis(self.timer.remaining), millis(self.timer.total()));
    }

    /// Onglets en segments, au milieu de la barre de la fenêtre.
    fn tabs(&mut self, ui: &mut egui::Ui) {
        let tabs = [Tab::Horloge, Tab::Minuteur, Tab::Chronomètre];
        let selected = tabs.iter().position(|tab| *tab == self.tab).unwrap_or(0);
        let center = ui.max_rect().center();
        if let Some(index) = kit::segments_at(
            ui,
            center,
            104.0,
            &["Horloge", "Minuteur", "Chronomètre"],
            selected,
        ) {
            self.tab = tabs[index];
        }
    }

    fn clock(&mut self, ui: &mut egui::Ui) {
        let now = unix_seconds();
        let local = now + self.offset;
        let local_days = local.div_euclid(86_400);
        let (year, month, day) = civil_from_days(local_days);
        let second_of_day = local.rem_euclid(86_400);
        let date = format!(
            "{} {} {} {}",
            WEEKDAYS[weekday(local_days) as usize],
            day,
            MONTHS[month as usize - 1],
            year
        );

        // Heure : heures et minutes en grand, secondes plus petites à côté.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 132.0), egui::Sense::hover());
        let painter = ui.painter();
        kit::caption(
            painter,
            rect.left_top(),
            &format!("Heure locale · UTC{}", format_offset(self.offset)),
        );
        let hours_minutes = format!("{:02}:{:02}", second_of_day / 3600, (second_of_day % 3600) / 60);
        let top = rect.top() + 22.0;
        let big = kit::tabular_text(
            painter,
            egui::pos2(rect.left() - 3.0, top),
            egui::Align2::LEFT_TOP,
            &hours_minutes,
            light_font(76.0),
            style::primary_text(),
        );
        let seconds_height = painter
            .layout_no_wrap("00".to_owned(), light_font(30.0), style::text_secondary())
            .size()
            .y;
        kit::tabular_text(
            painter,
            egui::pos2(big.right() + 8.0, big.bottom() - seconds_height - 12.0),
            egui::Align2::LEFT_TOP,
            &format!("{:02}", second_of_day % 60),
            light_font(30.0),
            style::text_secondary(),
        );
        painter.text(
            egui::pos2(rect.left(), rect.bottom() - 4.0),
            egui::Align2::LEFT_BOTTOM,
            capitalize(&date),
            FontId::proportional(15.0),
            style::text_secondary(),
        );

        ui.add_space(22.0);
        let (heading, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::hover());
        kit::caption(ui.painter(), heading.left_top(), "Horloges du monde");
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                kit::soft_card(ui, |ui| {
                    for (index, (name, offset)) in WORLD.into_iter().enumerate() {
                        if index > 0 {
                            kit::separator(ui);
                        }
                        let city = now + offset;
                        let shift = city.div_euclid(86_400) - local_days;
                        let hours = (offset - self.offset) as f64 / 3600.0;
                        let day = match shift {
                            -1 => "Hier",
                            1 => "Demain",
                            _ => "Aujourd’hui",
                        };
                        let detail = format!("{day}, {}", format_hours(hours));
                        let second_of_day = city.rem_euclid(86_400);
                        let time = format!("{:02}:{:02}", second_of_day / 3600, (second_of_day % 3600) / 60);
                        let (row, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 50.0),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter();
                        painter.text(
                            egui::pos2(row.left(), row.top() + 15.0),
                            egui::Align2::LEFT_CENTER,
                            name,
                            FontId::new(14.0, style::strong_family()),
                            style::primary_text(),
                        );
                        painter.text(
                            egui::pos2(row.left(), row.top() + 34.0),
                            egui::Align2::LEFT_CENTER,
                            detail,
                            FontId::proportional(12.0),
                            style::text_secondary(),
                        );
                        painter.text(
                            egui::pos2(row.right(), row.center().y),
                            egui::Align2::RIGHT_CENTER,
                            time,
                            light_font(28.0),
                            style::primary_text(),
                        );
                    }
                });
            });
    }

    fn timer(&mut self, ui: &mut egui::Ui) {
        let displayed = self.timer.displayed();
        let ended = self.timer.phase == Phase::Ended;
        let fraction = if self.timer.total().is_zero() {
            0.0
        } else {
            (displayed.as_secs_f64() / self.timer.total().as_secs_f64()).clamp(0.0, 1.0) as f32
        };

        // Anneau de progression, temps restant au centre.
        let (area, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 250.0), egui::Sense::hover());
        let center = area.center();
        let radius = 108.0;
        let painter = ui.painter();
        painter.circle_stroke(
            center,
            radius,
            egui::Stroke::new(8.0, style::control_background()),
        );
        let color = if ended { style::error() } else { style::accent() };
        if self.timer.phase != Phase::Idle {
            arc(painter, center, radius, fraction, egui::Stroke::new(8.0, color));
        }
        let text = if ended {
            "Temps écoulé".to_owned()
        } else if self.timer.phase == Phase::Idle {
            format_hms(self.timer.total().as_secs())
        } else {
            format_hms(displayed.as_secs())
        };
        kit::tabular_text(
            painter,
            center,
            egui::Align2::CENTER_CENTER,
            &text,
            if ended {
                FontId::new(24.0, style::strong_family())
            } else {
                light_font(46.0)
            },
            if ended {
                style::error()
            } else {
                style::primary_text()
            },
        );
        if self.timer.phase == Phase::Paused {
            painter.text(
                center + egui::vec2(0.0, 40.0),
                egui::Align2::CENTER_CENTER,
                "En pause",
                FontId::proportional(13.0),
                style::text_secondary(),
            );
        }

        ui.add_space(12.0);
        if self.timer.phase == Phase::Idle {
            // Réglage : trois colonnes, et des durées toutes prêtes.
            ui.horizontal(|ui| {
                let column = (ui.available_width() - 16.0) / 3.0;
                for (label, value, max) in [
                    ("heures", &mut self.timer.hours, 99),
                    ("minutes", &mut self.timer.minutes, 59),
                    ("secondes", &mut self.timer.seconds, 59),
                ] {
                    stepper(ui, column, label, value, max);
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for minutes in [1, 3, 5, 10, 15, 30] {
                    let chip = egui::Button::new(
                        RichText::new(format!("{minutes} min"))
                            .size(13.0)
                            .color(style::primary_text()),
                    )
                    .fill(style::control_background())
                    .corner_radius(CornerRadius::same(14))
                    .min_size(egui::vec2(0.0, 28.0));
                    if ui.add(chip).clicked() {
                        self.timer.hours = minutes / 60;
                        self.timer.minutes = minutes % 60;
                        self.timer.seconds = 0;
                    }
                }
            });
        }

        ui.add_space(16.0);
        let (label, enabled) = match self.timer.phase {
            Phase::Idle => ("Démarrer", !self.timer.total().is_zero()),
            Phase::Running => ("Pause", true),
            Phase::Paused => ("Reprendre", true),
            Phase::Ended => ("Arrêter", true),
        };
        let reset_enabled = self.timer.phase != Phase::Idle;
        let (secondary, primary) = button_pair(ui, "Réinitialiser", reset_enabled, label, enabled, ended);
        if secondary {
            self.timer.reset();
        }
        if primary {
            match self.timer.phase {
                Phase::Idle => self.timer.start(),
                Phase::Running => self.timer.pause(),
                Phase::Paused => self.timer.resume(),
                Phase::Ended => self.timer.stop_alert(),
            }
        }
    }

    fn stopwatch(&mut self, ui: &mut egui::Ui) {
        let current = self.stopwatch.current();
        let (area, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 120.0), egui::Sense::hover());
        kit::tabular_text(
            ui.painter(),
            area.center(),
            egui::Align2::CENTER_CENTER,
            &format_centis(current),
            light_font(60.0),
            style::primary_text(),
        );

        let running = self.stopwatch.running;
        let started = running || !current.is_zero();
        let (secondary_label, secondary_enabled) = if running {
            ("Tour", true)
        } else {
            ("Réinitialiser", started)
        };
        let primary_label = match (running, started) {
            (true, _) => "Arrêter",
            (false, true) => "Reprendre",
            (false, false) => "Démarrer",
        };
        let (secondary, primary) = button_pair(
            ui,
            secondary_label,
            secondary_enabled,
            primary_label,
            true,
            running,
        );
        if secondary {
            if running {
                self.stopwatch.lap();
            } else {
                self.stopwatch.reset();
            }
        }
        if primary {
            if running {
                self.stopwatch.stop();
            } else {
                self.stopwatch.start();
            }
        }

        if self.stopwatch.laps.is_empty() {
            return;
        }
        ui.add_space(18.0);
        let (heading, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::hover());
        kit::caption(ui.painter(), heading.left_top(), "Tours");
        ui.add_space(8.0);
        let laps = lap_durations(&self.stopwatch.laps);
        // Au moins trois tours : le plus rapide en vert, le plus lent en rouge.
        let (fastest, slowest) = if laps.len() >= 3 {
            (laps.iter().min().copied(), laps.iter().max().copied())
        } else {
            (None, None)
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                kit::soft_card(ui, |ui| {
                    for (index, lap) in laps.iter().enumerate().rev() {
                        if index + 1 < laps.len() {
                            kit::separator(ui);
                        }
                        let color = if Some(*lap) == fastest {
                            style::success()
                        } else if Some(*lap) == slowest {
                            style::error()
                        } else {
                            style::primary_text()
                        };
                        let (row, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 36.0),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter();
                        painter.text(
                            egui::pos2(row.left(), row.center().y),
                            egui::Align2::LEFT_CENTER,
                            format!("Tour {}", index + 1),
                            FontId::proportional(14.0),
                            color,
                        );
                        painter.text(
                            egui::pos2(row.right(), row.center().y),
                            egui::Align2::RIGHT_CENTER,
                            format_centis(*lap),
                            FontId::monospace(14.0),
                            color,
                        );
                    }
                });
            });
    }
}

/// Grands chiffres fins (Inter Display Light, comme le widget Horloge du bureau).
fn light_font(size: f32) -> FontId {
    FontId::new(size, style::light_family())
}

/// Arc de cercle partant du haut, dans le sens des aiguilles d'une montre.
fn arc(painter: &egui::Painter, center: egui::Pos2, radius: f32, fraction: f32, stroke: egui::Stroke) {
    if fraction <= 0.0 {
        return;
    }
    let steps = ((fraction * 120.0).ceil() as usize).max(2);
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|step| {
            let angle =
                std::f32::consts::TAU * fraction * step as f32 / steps as f32 - std::f32::consts::FRAC_PI_2;
            center + egui::vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(points, stroke));
}

/// Valeur du minuteur : − nombre + et son unité dessous.
fn stepper(ui: &mut egui::Ui, width: f32, label: &str, value: &mut u32, max: u32) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 70.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let small = |text: &str| {
                    egui::Button::new(RichText::new(text).size(16.0).color(style::primary_text()))
                        .fill(style::control_background())
                        .corner_radius(CornerRadius::same(14))
                        .min_size(egui::vec2(28.0, 28.0))
                };
                if ui.add(small("−")).clicked() {
                    *value = if *value == 0 { max } else { *value - 1 };
                }
                ui.add(
                    egui::DragValue::new(value)
                        .range(0..=max)
                        .speed(0.2)
                        .custom_formatter(|number, _| format!("{number:02}")),
                );
                if ui.add(small("+")).clicked() {
                    *value = if *value >= max { 0 } else { *value + 1 };
                }
            });
            ui.label(RichText::new(label).size(12.0).color(style::text_secondary()));
        },
    );
}

/// Écart avec l'heure locale : « même heure », « +1 h », « −5 h 30 ».
/// Deux grands boutons côte à côte : secondaire à gauche, principal (rouge s'il arrête)
/// à droite ; renvoie leurs clics.
fn button_pair(
    ui: &mut egui::Ui,
    secondary: &str,
    secondary_enabled: bool,
    primary: &str,
    primary_enabled: bool,
    primary_stops: bool,
) -> (bool, bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let width = (ui.available_width() - 12.0) / 2.0;
        let left = kit::wide_button(ui, width, secondary, kit::Tone::Plain, secondary_enabled).clicked();
        let tone = if primary_stops {
            kit::Tone::Danger
        } else {
            kit::Tone::Accent
        };
        let right = kit::wide_button(ui, width, primary, tone, primary_enabled).clicked();
        (left, right)
    })
    .inner
}

fn format_hours(hours: f64) -> String {
    if hours == 0.0 {
        return "même heure".to_owned();
    }
    let sign = if hours > 0.0 { "+" } else { "−" };
    let minutes = (hours.abs() * 60.0).round() as i64;
    match minutes % 60 {
        0 => format!("{sign}{} h", minutes / 60),
        rest => format!("{sign}{} h {rest:02}", minutes / 60),
    }
}

fn capitalize(text: &str) -> String {
    let mut characters = text.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
        .unwrap_or_default()
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
    fn ecart_avec_l_heure_locale() {
        assert_eq!(format_hours(0.0), "même heure");
        assert_eq!(format_hours(1.0), "+1 h");
        assert_eq!(format_hours(-5.5), "−5 h 30");
        assert_eq!(capitalize("vendredi 6"), "Vendredi 6");
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
