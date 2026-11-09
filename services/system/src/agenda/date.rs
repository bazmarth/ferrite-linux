//! Dates du calendrier : jour civil, heure à la minute, calculs (jours, mois), lecture des
//! saisies en français et mise en forme. Tout est en heure locale « flottante » : un
//! rendez-vous à 9 h reste à 9 h, même après un changement d'heure.

use std::{
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Minutes dans une journée.
pub const DAY_MINUTES: i64 = 1440;

pub const MONTHS: [&str; 12] = [
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

pub const MONTHS_SHORT: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc.",
];

/// Jours de la semaine, lundi d'abord (semaine à la française).
pub const WEEKDAYS: [&str; 7] = [
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];

pub const WEEKDAYS_SHORT: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];

/// Initiales des jours, pour les petits calendriers.
pub const WEEKDAYS_LETTER: [&str; 7] = ["L", "M", "M", "J", "V", "S", "D"];

/// Jour du calendrier grégorien.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// Jour valide, ou rien (30 février, mois 13…).
    pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        ((1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month)).then_some(Self {
            year,
            month,
            day,
        })
    }

    /// Nombre de jours depuis le 1er janvier 1970 (algorithme de Howard Hinnant).
    pub fn days(self) -> i64 {
        let year = i64::from(self.year) - i64::from(self.month <= 2);
        let era = year.div_euclid(400);
        let yoe = year - era * 400;
        let month = i64::from(self.month);
        let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Self { year, month, day }
    }

    pub fn add_days(self, days: i64) -> Self {
        Self::from_days(self.days() + days)
    }

    /// Mois suivants (ou précédents si négatif), le jour ramené au dernier du mois au besoin
    /// (31 janvier + 1 mois = 28 ou 29 février).
    pub fn add_months(self, months: i32) -> Self {
        let index = self.year * 12 + self.month as i32 - 1 + months;
        let year = index.div_euclid(12);
        let month = index.rem_euclid(12) as u32 + 1;
        Self {
            year,
            month,
            day: self.day.min(days_in_month(year, month)),
        }
    }

    /// Jour de la semaine : 0 = lundi … 6 = dimanche.
    pub fn weekday(self) -> usize {
        // Le 1er janvier 1970 était un jeudi (3).
        (self.days() + 3).rem_euclid(7) as usize
    }

    /// Lundi de la semaine de ce jour.
    pub fn week_start(self) -> Self {
        self.add_days(-(self.weekday() as i64))
    }

    pub fn first_of_month(self) -> Self {
        Self { day: 1, ..self }
    }

    /// Numéro de semaine ISO 8601.
    pub fn iso_week(self) -> u32 {
        let thursday = self.add_days(3 - self.weekday() as i64);
        let first = Date {
            year: thursday.year,
            month: 1,
            day: 1,
        };
        ((thursday.days() - first.days()) / 7 + 1) as u32
    }

    pub fn at(self, minute: u32) -> DateTime {
        DateTime { date: self, minute }
    }

    /// « 2026-11-08 » (fichier).
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn parse_iso(text: &str) -> Option<Self> {
        let mut parts = text.trim().splitn(3, '-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        let day = parts.next()?.parse().ok()?;
        Self::new(year, month, day)
    }

    /// Saisie de l'utilisateur : « 9/11/2026 », « 9/11 » (année de `reference`),
    /// « 9.11.26 », « 2026-11-09 », « auj », « demain ».
    pub fn parse_input(text: &str, reference: Date) -> Option<Self> {
        let text = text.trim().to_lowercase();
        match text.as_str() {
            "aujourd'hui" | "aujourd’hui" | "auj" => return Some(reference),
            "demain" => return Some(reference.add_days(1)),
            "hier" => return Some(reference.add_days(-1)),
            _ => {}
        }
        if text.contains('-')
            && let Some(date) = Self::parse_iso(&text)
        {
            return Some(date);
        }
        let parts: Vec<&str> = text
            .split(['/', '.', ' '])
            .filter(|part| !part.is_empty())
            .collect();
        let day: u32 = parts.first()?.parse().ok()?;
        let month: u32 = match parts.get(1) {
            Some(part) => part.parse().ok().or_else(|| month_from_name(part))?,
            None => reference.month,
        };
        let year: i32 = match parts.get(2) {
            Some(part) => {
                let value: i32 = part.parse().ok()?;
                if part.len() <= 2 { 2000 + value } else { value }
            }
            None => reference.year,
        };
        if parts.len() > 3 {
            return None;
        }
        Self::new(year, month, day)
    }

    /// « 9/11/2026 » (champ de saisie).
    pub fn input(self) -> String {
        format!("{:02}/{:02}/{}", self.day, self.month, self.year)
    }

    /// « dimanche 8 novembre 2026 ».
    pub fn long(self) -> String {
        format!(
            "{} {} {} {}",
            WEEKDAYS[self.weekday()],
            self.day,
            MONTHS[self.month as usize - 1],
            self.year
        )
    }

    /// « dim. 8 nov. ».
    pub fn short(self) -> String {
        format!(
            "{} {} {}",
            WEEKDAYS_SHORT[self.weekday()],
            self.day,
            MONTHS_SHORT[self.month as usize - 1]
        )
    }

    /// « 8 nov. 2026 ».
    pub fn medium(self) -> String {
        format!(
            "{} {} {}",
            self.day,
            MONTHS_SHORT[self.month as usize - 1],
            self.year
        )
    }

    /// « novembre 2026 ».
    pub fn month_title(self) -> String {
        format!("{} {}", MONTHS[self.month as usize - 1], self.year)
    }

    /// Titre relatif : « Aujourd'hui », « Demain », « Hier », sinon le jour en entier.
    pub fn relative(self, today: Date) -> String {
        match self.days() - today.days() {
            0 => "Aujourd’hui".to_owned(),
            1 => "Demain".to_owned(),
            -1 => "Hier".to_owned(),
            _ => capitalize(&self.long()),
        }
    }
}

fn month_from_name(text: &str) -> Option<u32> {
    let text = text.trim_end_matches('.');
    if text.len() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|name| name.starts_with(text))
        .map(|index| index as u32 + 1)
}

pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Instant à la minute près, en heure locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    pub date: Date,
    /// Minutes depuis minuit (0 à 1439).
    pub minute: u32,
}

impl DateTime {
    /// Minutes depuis le 1er janvier 1970 à minuit.
    pub fn stamp(self) -> i64 {
        self.date.days() * DAY_MINUTES + i64::from(self.minute)
    }

    pub fn from_stamp(stamp: i64) -> Self {
        Self {
            date: Date::from_days(stamp.div_euclid(DAY_MINUTES)),
            minute: stamp.rem_euclid(DAY_MINUTES) as u32,
        }
    }

    pub fn add_minutes(self, minutes: i64) -> Self {
        Self::from_stamp(self.stamp() + minutes)
    }

    /// « 2026-11-08T09:30 » (fichier).
    pub fn iso(self) -> String {
        format!("{}T{}", self.date.iso(), clock(self.minute))
    }

    pub fn parse_iso(text: &str) -> Option<Self> {
        let (date, time) = text.trim().split_once('T')?;
        Some(Self {
            date: Date::parse_iso(date)?,
            minute: parse_time(time)?,
        })
    }

    /// « dim. 8 nov. · 09:30 ».
    pub fn short(self) -> String {
        format!("{} · {}", self.date.short(), clock(self.minute))
    }
}

/// « 09:30 ».
pub fn clock(minute: u32) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// Saisie d'une heure : « 9 », « 9h », « 9:30 », « 9h30 », « 0930 », « 21.15 ».
pub fn parse_time(text: &str) -> Option<u32> {
    let text = text.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }
    let (hours, minutes) = if let Some((h, m)) = text.split_once([':', 'h', '.']) {
        (h.trim(), m.trim())
    } else if text.len() == 4 && text.chars().all(|c| c.is_ascii_digit()) {
        text.split_at(2)
    } else {
        (text.as_str(), "")
    };
    let hours: u32 = hours.parse().ok()?;
    let minutes: u32 = if minutes.is_empty() {
        0
    } else {
        minutes.parse().ok()?
    };
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// Durée lisible : « 15 min », « 1 h », « 1 h 30 », « 2 jours », « 1 semaine ».
pub fn duration_label(minutes: u32) -> String {
    match minutes {
        0 => "À l’heure de début".to_owned(),
        m if m % 10_080 == 0 => plural(m / 10_080, "semaine", "semaines"),
        m if m % DAY_MINUTES as u32 == 0 => plural(m / DAY_MINUTES as u32, "jour", "jours"),
        m if m >= 60 && m % 60 == 0 => format!("{} h", m / 60),
        m if m >= 60 => format!("{} h {:02}", m / 60, m % 60),
        m => format!("{m} min"),
    }
}

pub fn plural(count: u32, one: &str, many: &str) -> String {
    format!("{count} {}", if count > 1 { many } else { one })
}

/// Décalage de l'heure locale sur l'UTC, en minutes (lu par `date +%z`, gardé une minute).
fn local_offset() -> i64 {
    static OFFSET: Mutex<Option<(Instant, i64)>> = Mutex::new(None);
    let Ok(mut cached) = OFFSET.lock() else {
        return 0;
    };
    if let Some((checked, offset)) = *cached
        && checked.elapsed() < Duration::from_secs(60)
    {
        return offset;
    }
    let offset = crate::command::run("date", &["+%z"])
        .ok()
        .and_then(|text| parse_offset(text.trim()))
        .unwrap_or(0);
    *cached = Some((Instant::now(), offset));
    offset
}

/// « +0100 » → 60 ; « -0530 » → -330.
pub fn parse_offset(text: &str) -> Option<i64> {
    let (sign, digits) = match text.split_at_checked(1)? {
        ("+", rest) => (1, rest),
        ("-", rest) => (-1, rest),
        _ => return None,
    };
    if digits.len() != 4 {
        return None;
    }
    let hours: i64 = digits[..2].parse().ok()?;
    let minutes: i64 = digits[2..].parse().ok()?;
    Some(sign * (hours * 60 + minutes))
}

/// Maintenant, en heure locale, à la minute près.
pub fn now() -> DateTime {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    DateTime::from_stamp(seconds.div_euclid(60) + local_offset())
}

/// Secondes écoulées dans la minute en cours (pour se réveiller juste à la minute suivante).
pub fn seconds_in_minute() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() % 60)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::new(y, m, d).unwrap()
    }

    #[test]
    fn les_jours_se_comptent_depuis_1970() {
        assert_eq!(date(1970, 1, 1).days(), 0);
        assert_eq!(date(2000, 3, 1).days(), 11_017);
        for days in [-800_000, -1, 0, 1, 19_000, 20_400, 800_000] {
            assert_eq!(Date::from_days(days).days(), days);
        }
    }

    #[test]
    fn jours_de_la_semaine_et_semaines_iso() {
        assert_eq!(date(2026, 11, 8).weekday(), 6); // dimanche
        assert_eq!(date(2026, 11, 9).weekday(), 0); // lundi
        assert_eq!(date(2026, 11, 8).week_start(), date(2026, 11, 2));
        assert_eq!(date(2026, 1, 1).iso_week(), 1);
        assert_eq!(date(2021, 1, 3).iso_week(), 53);
        assert_eq!(date(2026, 11, 8).iso_week(), 45);
    }

    #[test]
    fn les_mois_se_suivent_sans_deborder() {
        assert_eq!(date(2026, 1, 31).add_months(1), date(2026, 2, 28));
        assert_eq!(date(2028, 1, 31).add_months(1), date(2028, 2, 29));
        assert_eq!(date(2026, 1, 15).add_months(-1), date(2025, 12, 15));
        assert_eq!(date(2026, 11, 8).add_months(14), date(2028, 1, 8));
        assert!(Date::new(2026, 2, 29).is_none());
        assert!(Date::new(2026, 13, 1).is_none());
    }

    #[test]
    fn saisies_de_date_en_francais() {
        let today = date(2026, 11, 8);
        assert_eq!(Date::parse_input("9/11/2026", today), Some(date(2026, 11, 9)));
        assert_eq!(Date::parse_input("9/11", today), Some(date(2026, 11, 9)));
        assert_eq!(Date::parse_input("1.2.27", today), Some(date(2027, 2, 1)));
        assert_eq!(Date::parse_input("2026-12-25", today), Some(date(2026, 12, 25)));
        assert_eq!(Date::parse_input("25 déc 2026", today), Some(date(2026, 12, 25)));
        assert_eq!(Date::parse_input("demain", today), Some(date(2026, 11, 9)));
        assert_eq!(Date::parse_input("31/2/2026", today), None);
        assert_eq!(Date::parse_input("bonjour", today), None);
    }

    #[test]
    fn saisies_d_heure() {
        assert_eq!(parse_time("9"), Some(540));
        assert_eq!(parse_time("9h"), Some(540));
        assert_eq!(parse_time("9h30"), Some(570));
        assert_eq!(parse_time("09:05"), Some(545));
        assert_eq!(parse_time("2115"), Some(1275));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("12:60"), None);
        assert_eq!(parse_time(""), None);
    }

    #[test]
    fn mise_en_forme() {
        let day = date(2026, 11, 8);
        assert_eq!(day.long(), "dimanche 8 novembre 2026");
        assert_eq!(day.short(), "dim. 8 nov.");
        assert_eq!(day.relative(day), "Aujourd’hui");
        assert_eq!(day.add_days(1).relative(day), "Demain");
        assert_eq!(day.add_days(3).relative(day), "Mercredi 11 novembre 2026");
        assert_eq!(day.at(570).iso(), "2026-11-08T09:30");
        assert_eq!(DateTime::parse_iso("2026-11-08T09:30"), Some(day.at(570)));
        assert_eq!(duration_label(15), "15 min");
        assert_eq!(duration_label(90), "1 h 30");
        assert_eq!(duration_label(1440), "1 jour");
        assert_eq!(duration_label(2880), "2 jours");
        assert_eq!(duration_label(10_080), "1 semaine");
        assert_eq!(parse_offset("+0100"), Some(60));
        assert_eq!(parse_offset("-0530"), Some(-330));
    }

    #[test]
    fn minutes_et_jours_s_enchainent() {
        let late = date(2026, 12, 31).at(23 * 60 + 50);
        assert_eq!(late.add_minutes(20), date(2027, 1, 1).at(10));
        assert_eq!(late.add_minutes(-1440).date, date(2026, 12, 30));
    }
}
