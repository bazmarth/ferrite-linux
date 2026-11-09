//! Mode nuit : l'écran se teinte en orangé le soir pour reposer les yeux.
//!
//! Réglages (`mode_nuit`) : désactivé, toujours, du coucher au lever du soleil (calculé pour
//! la ville de la météo) ou entre deux heures choisies (`mode_nuit_debut`, `mode_nuit_fin`).
//! La teinte est une température de couleur (`mode_nuit_temperature`, en kelvins).
//! Aux heures de bascule, la teinte arrive et s'en va en fondu sur [`RAMP_MINUTES`].

use crate::{config::Settings, weather::Place};

/// Durée du fondu à l'arrivée et au départ du mode nuit.
pub const RAMP_MINUTES: f64 = 20.0;
/// Bornes et valeur d'origine de la température de couleur.
pub const TEMPERATURE_MIN: u32 = 2700;
pub const TEMPERATURE_MAX: u32 = 5500;
pub const TEMPERATURE_DEFAULT: u32 = 3800;
/// Soirée et matin d'origine (heures choisies, ou soleil introuvable).
const DEFAULT_START: u32 = 21 * 60;
const DEFAULT_END: u32 = 7 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Schedule {
    #[default]
    Off,
    Always,
    Sun,
    Hours,
}

impl Schedule {
    pub const ALL: [Schedule; 4] = [Schedule::Off, Schedule::Always, Schedule::Sun, Schedule::Hours];

    fn key(self) -> &'static str {
        match self {
            Schedule::Off => "desactive",
            Schedule::Always => "toujours",
            Schedule::Sun => "soleil",
            Schedule::Hours => "heures",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Schedule::Off => "Désactivé",
            Schedule::Always => "Toujours",
            Schedule::Sun => "Du coucher au lever du soleil",
            Schedule::Hours => "Heures choisies",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NightSettings {
    pub schedule: Schedule,
    /// Début et fin, en minutes depuis minuit (heures choisies).
    pub start: u32,
    pub end: u32,
    pub temperature: u32,
}

impl Default for NightSettings {
    fn default() -> Self {
        Self {
            schedule: Schedule::Off,
            start: DEFAULT_START,
            end: DEFAULT_END,
            temperature: TEMPERATURE_DEFAULT,
        }
    }
}

/// « 21:30 » → 1290 minutes.
pub fn parse_time(text: &str) -> Option<u32> {
    let (hours, minutes) = text.trim().split_once(':')?;
    let (hours, minutes): (u32, u32) = (hours.parse().ok()?, minutes.parse().ok()?);
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// 1290 minutes → « 21:30 ».
pub fn format_time(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60)
}

impl NightSettings {
    pub fn load(settings: &Settings) -> Self {
        let defaults = Self::default();
        let schedule = settings
            .get("mode_nuit")
            .and_then(|key| Schedule::ALL.into_iter().find(|schedule| schedule.key() == key))
            .unwrap_or_default();
        Self {
            schedule,
            start: settings
                .get("mode_nuit_debut")
                .and_then(parse_time)
                .unwrap_or(defaults.start),
            end: settings
                .get("mode_nuit_fin")
                .and_then(parse_time)
                .unwrap_or(defaults.end),
            temperature: settings
                .get("mode_nuit_temperature")
                .and_then(|value| value.parse().ok())
                .map_or(defaults.temperature, |value: u32| {
                    value.clamp(TEMPERATURE_MIN, TEMPERATURE_MAX)
                }),
        }
    }

    pub fn store(&self, settings: &mut Settings) {
        settings.set("mode_nuit", self.schedule.key());
        settings.set("mode_nuit_debut", &format_time(self.start));
        settings.set("mode_nuit_fin", &format_time(self.end));
        settings.set("mode_nuit_temperature", &self.temperature.to_string());
    }

    /// Soirée et matin du jour (`day` : rang dans l'année, 1 à 366), en minutes locales.
    /// `offset` : décalage horaire en minutes. `None` : jamais de mode nuit.
    pub fn window(&self, place: &Place, day: u32, offset: i32) -> Option<(u32, u32)> {
        match self.schedule {
            Schedule::Off => None,
            Schedule::Always => Some((0, 24 * 60)),
            Schedule::Hours => Some((self.start, self.end)),
            Schedule::Sun => Some(
                sun_times(place.latitude, place.longitude, day, offset)
                    .map_or((DEFAULT_START, DEFAULT_END), |(sunrise, sunset)| {
                        (sunset, sunrise)
                    }),
            ),
        }
    }

    /// Force de la teinte (0 à 1) à `minute` (depuis minuit, heure locale).
    pub fn strength(&self, place: &Place, day: u32, offset: i32, minute: f64) -> f64 {
        match self.window(place, day, offset) {
            None => 0.0,
            Some((0, 1440)) => 1.0,
            Some((start, end)) => window_strength(start, end, minute),
        }
    }
}

/// Force de la teinte entre `start` et `end` (la fenêtre peut passer minuit), avec un fondu
/// de [`RAMP_MINUTES`] après le début et avant la fin.
fn window_strength(start: u32, end: u32, minute: f64) -> f64 {
    let day = 24.0 * 60.0;
    let (start, end) = (f64::from(start), f64::from(end));
    let length = (end - start).rem_euclid(day);
    if length == 0.0 {
        return 0.0;
    }
    let inside = (minute - start).rem_euclid(day);
    if inside >= length {
        return 0.0;
    }
    let ramp = RAMP_MINUTES.min(length / 2.0);
    (inside / ramp).min((length - inside) / ramp).clamp(0.0, 1.0)
}

/// Rang du jour dans l'année (1 à 366), minute du jour et décalage horaire (minutes), en
/// heure locale (heure d'été comprise, voir [`crate::time::local_offset_seconds`]).
pub fn local_now() -> (u32, f64, i32) {
    let offset = (crate::time::local_offset_seconds() / 60) as i32;
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        + f64::from(offset) * 60.0;
    let mut days = (seconds / 86_400.0).floor() as i64;
    let minute = (seconds - days as f64 * 86_400.0) / 60.0;
    let mut year = 1970;
    loop {
        let length = if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
            366
        } else {
            365
        };
        if days < length {
            break;
        }
        days -= length;
        year += 1;
    }
    (days as u32 + 1, minute, offset)
}

/// Lever et coucher du soleil (minutes locales depuis minuit) d'après l'algorithme simplifié
/// de la NOAA ; `None` pendant la nuit ou le jour polaires.
pub fn sun_times(latitude: f64, longitude: f64, day: u32, offset: i32) -> Option<(u32, u32)> {
    let gamma = 2.0 * std::f64::consts::PI / 365.0 * (f64::from(day) - 1.0);
    let equation = 229.18
        * (0.000075 + 0.001868 * gamma.cos()
            - 0.032077 * gamma.sin()
            - 0.014615 * (2.0 * gamma).cos()
            - 0.040849 * (2.0 * gamma).sin());
    let declination = 0.006918 - 0.399912 * gamma.cos() + 0.070257 * gamma.sin()
        - 0.006758 * (2.0 * gamma).cos()
        + 0.000907 * (2.0 * gamma).sin()
        - 0.002697 * (3.0 * gamma).cos()
        + 0.00148 * (3.0 * gamma).sin();
    let latitude = latitude.to_radians();
    let cos_hour = 90.833_f64.to_radians().cos() / (latitude.cos() * declination.cos())
        - latitude.tan() * declination.tan();
    if !(-1.0..=1.0).contains(&cos_hour) {
        return None;
    }
    let hour_angle = cos_hour.acos().to_degrees();
    let local = |utc: f64| (utc + f64::from(offset)).rem_euclid(24.0 * 60.0).round() as u32 % 1440;
    let sunrise = 720.0 - 4.0 * (longitude + hour_angle) - equation;
    let sunset = 720.0 - 4.0 * (longitude - hour_angle) - equation;
    Some((local(sunrise), local(sunset)))
}

/// Gains rouge, vert, bleu (0 à 1) d'une température de couleur, d'après l'approximation
/// de Tanner Helland (6600 K et plus : blanc).
pub fn rgb_gains(kelvin: u32) -> (f64, f64, f64) {
    let t = f64::from(kelvin.clamp(1000, 6600)) / 100.0;
    let green = (99.470_802_586_1 * t.ln() - 161.119_568_166_1) / 255.0;
    let blue = if t >= 66.0 {
        1.0
    } else if t <= 19.0 {
        0.0
    } else {
        (138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7) / 255.0
    };
    (1.0, green.clamp(0.0, 1.0), blue.clamp(0.0, 1.0))
}

/// Gains appliqués avec la force `strength` : blanc à 0, la température choisie à 1.
pub fn gains(temperature: u32, strength: f64) -> (f64, f64, f64) {
    let (r, g, b) = rgb_gains(temperature);
    let mix = |target: f64| 1.0 + (target - 1.0) * strength.clamp(0.0, 1.0);
    (mix(r), mix(g), mix(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heures() {
        assert_eq!(parse_time("21:30"), Some(1290));
        assert_eq!(parse_time("7:05"), Some(425));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(format_time(425), "07:05");
    }

    #[test]
    fn la_nuit_passe_minuit_avec_un_fondu() {
        let (start, end) = (21 * 60, 7 * 60);
        assert_eq!(window_strength(start, end, 12.0 * 60.0), 0.0);
        assert_eq!(window_strength(start, end, 23.0 * 60.0), 1.0);
        assert_eq!(window_strength(start, end, 3.0 * 60.0), 1.0);
        // Dix minutes après le début : à mi-fondu.
        assert!((window_strength(start, end, 21.0 * 60.0 + 10.0) - 0.5).abs() < 1e-9);
        assert!((window_strength(start, end, 7.0 * 60.0 - 10.0) - 0.5).abs() < 1e-9);
        assert_eq!(window_strength(start, end, 7.0 * 60.0), 0.0);
    }

    #[test]
    fn soleil_a_ouagadougou() {
        // 3 novembre (jour 307), UTC+0 : lever vers 5 h 50, coucher vers 17 h 30.
        let (sunrise, sunset) = sun_times(12.3714, -1.5197, 307, 0).unwrap();
        assert!((5 * 60 + 30..6 * 60 + 15).contains(&sunrise), "lever {sunrise}");
        assert!((17 * 60 + 15..17 * 60 + 50).contains(&sunset), "coucher {sunset}");
        // Nuit polaire au cap Nord en décembre.
        assert_eq!(sun_times(71.17, 25.78, 355, 60), None);
    }

    #[test]
    fn teinte_chaude() {
        assert_eq!(gains(3800, 0.0), (1.0, 1.0, 1.0));
        let (r, g, b) = gains(3800, 1.0);
        assert_eq!(r, 1.0);
        assert!(g < 0.9 && b < g, "{g} {b}");
        assert_eq!(rgb_gains(6600), (1.0, 1.0, 1.0));
    }

    #[test]
    fn reglages_relus() {
        let mut settings = Settings::default();
        let night = NightSettings {
            schedule: Schedule::Hours,
            start: 20 * 60 + 15,
            end: 6 * 60,
            temperature: 3400,
        };
        night.store(&mut settings);
        assert_eq!(NightSettings::load(&settings), night);
    }
}
