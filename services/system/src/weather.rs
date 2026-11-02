//! Météo du lieu choisi, relevée auprès d'Open-Meteo (service gratuit, sans compte). Seules
//! les coordonnées du lieu sont envoyées. `curl` fait la requête, avec un délai borné.

use std::time::Duration;

use crate::{
    command::{Error, run_with_timeout},
    config::Settings,
};

const TIMEOUT: Duration = Duration::from_secs(20);
/// Jours de prévision montrés après aujourd'hui.
const FORECAST_DAYS: usize = 4;

/// Lieu de la météo : réglages `meteo_ville`, `meteo_latitude`, `meteo_longitude`.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    pub fn from_settings(settings: &Settings) -> Self {
        let number = |key: &str| settings.get(key).and_then(|value| value.parse::<f64>().ok());
        Place {
            name: settings.get("meteo_ville").unwrap_or("Ouagadougou").to_owned(),
            latitude: number("meteo_latitude").unwrap_or(12.3714),
            longitude: number("meteo_longitude").unwrap_or(-1.5197),
        }
    }
}

/// Ciel, d'après les codes météo de l'OMM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sky {
    Clear,
    Partly,
    Cloudy,
    Fog,
    Rain,
    Snow,
    Storm,
}

impl Sky {
    pub fn from_code(code: u32) -> Self {
        match code {
            0 | 1 => Sky::Clear,
            2 => Sky::Partly,
            3 => Sky::Cloudy,
            45 | 48 => Sky::Fog,
            71..=77 | 85 | 86 => Sky::Snow,
            95..=99 => Sky::Storm,
            _ => Sky::Rain,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sky::Clear => "Ensoleillé",
            Sky::Partly => "Éclaircies",
            Sky::Cloudy => "Nuageux",
            Sky::Fog => "Brouillard",
            Sky::Rain => "Pluie",
            Sky::Snow => "Neige",
            Sky::Storm => "Orage",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayForecast {
    /// Année, mois, jour.
    pub date: (i64, u32, u32),
    pub sky: Sky,
    pub max: i32,
    pub min: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weather {
    pub temperature: i32,
    pub feels_like: i32,
    pub sky: Sky,
    pub days: Vec<DayForecast>,
}

pub fn fetch(place: &Place) -> Result<Weather, Error> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}\
         &current=temperature_2m,apparent_temperature,weather_code\
         &daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days={}",
        place.latitude,
        place.longitude,
        FORECAST_DAYS + 1,
    );
    let body = run_with_timeout("curl", &["-fsS", "--max-time", "15", &url], TIMEOUT)?;
    parse(&body).ok_or_else(|| "réponse de la météo illisible".to_owned())
}

/// Partie de `json` qui suit `"section":{` ou `"section":[`.
fn after<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("\"{key}\":");
    let start = json.find(&marker)? + marker.len();
    Some(&json[start..])
}

fn number(section: &str, key: &str) -> Option<f64> {
    let value = after(section, key)?;
    let end = value.find([',', '}', ']'])?;
    value[..end].trim().parse().ok()
}

fn list<'a>(section: &'a str, key: &str) -> Option<Vec<&'a str>> {
    let value = after(section, key)?.strip_prefix('[')?;
    let end = value.find(']')?;
    Some(value[..end].split(',').map(str::trim).collect())
}

/// Réponse d'Open-Meteo : sa forme est fixe, un petit lecteur suffit.
fn parse(json: &str) -> Option<Weather> {
    let current = after(json, "current")?;
    let daily = after(json, "daily")?;
    let dates = list(daily, "time")?;
    let codes = list(daily, "weather_code")?;
    let highs = list(daily, "temperature_2m_max")?;
    let lows = list(daily, "temperature_2m_min")?;
    let days = (1..dates.len().min(FORECAST_DAYS + 1))
        .map(|index| {
            let mut date = dates[index].trim_matches('"').split('-');
            Some(DayForecast {
                date: (
                    date.next()?.parse().ok()?,
                    date.next()?.parse().ok()?,
                    date.next()?.parse().ok()?,
                ),
                sky: Sky::from_code(codes.get(index)?.parse().ok()?),
                max: highs.get(index)?.parse::<f64>().ok()?.round() as i32,
                min: lows.get(index)?.parse::<f64>().ok()?.round() as i32,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Weather {
        temperature: number(current, "temperature_2m")?.round() as i32,
        feels_like: number(current, "apparent_temperature")?.round() as i32,
        sky: Sky::from_code(number(current, "weather_code")? as u32),
        days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPONSE: &str = r#"{"latitude":12.375,"current_units":{"time":"iso8601","temperature_2m":"°C"},
        "current":{"time":"2026-10-30T12:30","interval":900,"temperature_2m":32.4,"apparent_temperature":33.6,"weather_code":2},
        "daily_units":{"time":"iso8601","weather_code":"wmo code"},
        "daily":{"time":["2026-10-30","2026-10-31","2026-11-01","2026-11-02","2026-11-03"],
        "weather_code":[0,1,61,3,95],"temperature_2m_max":[33.1,33.5,31.0,32.0,-1.6],
        "temperature_2m_min":[22.0,22.4,21.6,21.0,-3.5]}}"#;

    #[test]
    fn la_reponse_est_lue_sans_le_jour_meme() {
        let weather = parse(RESPONSE).expect("réponse valide");
        assert_eq!(weather.temperature, 32);
        assert_eq!(weather.feels_like, 34);
        assert_eq!(weather.sky, Sky::Partly);
        assert_eq!(weather.days.len(), 4);
        assert_eq!(
            weather.days[0],
            DayForecast {
                date: (2026, 10, 31),
                sky: Sky::Clear,
                max: 34,
                min: 22,
            }
        );
        assert_eq!(weather.days[1].sky, Sky::Rain);
        assert_eq!(weather.days[3].sky, Sky::Storm);
        assert_eq!(weather.days[3].min, -4);
    }

    #[test]
    fn une_reponse_tronquee_ne_donne_pas_de_meteo() {
        assert_eq!(parse(r#"{"current":{"temperature_2m":32"#), None);
        assert_eq!(parse("erreur"), None);
    }

    #[test]
    fn le_lieu_par_defaut_est_ouagadougou() {
        let place = Place::from_settings(&Settings::default());
        assert_eq!(place.name, "Ouagadougou");
    }
}
