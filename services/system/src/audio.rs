//! Son, par PipeWire/WirePlumber (`wpctl`).

use crate::command::{self, Error};

const SINK: &str = "@DEFAULT_AUDIO_SINK@";

/// Volume de la sortie par défaut, de 0 à 100 (au-delà, amplification non proposée).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Volume {
    pub level: u8,
    pub muted: bool,
}

pub fn volume() -> Option<Volume> {
    parse_volume(&command::run("wpctl", &["get-volume", SINK]).ok()?)
}

/// Analyse « Volume: 0.65 » ou « Volume: 0.40 [MUTED] ».
pub fn parse_volume(output: &str) -> Option<Volume> {
    let rest = output.trim().strip_prefix("Volume:")?.trim();
    let value: f32 = rest.split_whitespace().next()?.parse().ok()?;
    Some(Volume {
        level: (value * 100.0).round().clamp(0.0, 100.0) as u8,
        muted: rest.contains("[MUTED]"),
    })
}

pub fn set_volume(level: u8) -> Result<(), Error> {
    let value = format!("{:.2}", level.min(100) as f32 / 100.0);
    command::run("wpctl", &["set-volume", SINK, &value])?;
    if level > 0 {
        command::run("wpctl", &["set-mute", SINK, "0"])?;
    }
    Ok(())
}

pub fn set_muted(muted: bool) -> Result<(), Error> {
    command::run("wpctl", &["set-mute", SINK, if muted { "1" } else { "0" }]).map(drop)
}

/// Sortie audio (haut-parleurs, casque, HDMI…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sink {
    pub id: u32,
    pub name: String,
    pub default: bool,
}

pub fn sinks() -> Result<Vec<Sink>, Error> {
    Ok(parse_sinks(&command::run("wpctl", &["status"])?))
}

/// Section « Sinks: » de la partie « Audio » de `wpctl status`.
pub fn parse_sinks(status: &str) -> Vec<Sink> {
    let mut in_audio = false;
    let mut in_sinks = false;
    let mut sinks = Vec::new();
    for line in status.lines() {
        let clean: String = line
            .chars()
            .filter(|character| !matches!(character, '│' | '├' | '└' | '─'))
            .collect();
        let clean = clean.trim();
        if !line.starts_with(' ') && !line.starts_with('│') && !line.trim().is_empty() {
            in_audio = line.trim() == "Audio";
            in_sinks = false;
            continue;
        }
        if !in_audio {
            continue;
        }
        if clean.ends_with(':') {
            in_sinks = clean == "Sinks:";
            continue;
        }
        if !in_sinks || clean.is_empty() {
            continue;
        }
        let default = clean.starts_with('*');
        let entry = clean.trim_start_matches('*').trim();
        let Some((id, rest)) = entry.split_once('.') else {
            continue;
        };
        let Ok(id) = id.trim().parse() else { continue };
        let name = rest.split(" [vol").next().unwrap_or(rest).trim().to_owned();
        sinks.push(Sink { id, name, default });
    }
    sinks
}

pub fn set_default_sink(id: u32) -> Result<(), Error> {
    command::run("wpctl", &["set-default", &id.to_string()]).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_et_sorties() {
        assert_eq!(
            parse_volume("Volume: 0.40 [MUTED]\n"),
            Some(Volume {
                level: 40,
                muted: true
            })
        );
        assert_eq!(parse_volume("Volume: 1.00").map(|v| v.level), Some(100));
        assert_eq!(parse_volume("erreur"), None);

        let status = "\
PipeWire 'pipewire-0' [1.4.2, ferrite@ferrite, cookie:1]
 └─ Clients:
        33. WirePlumber

Audio
 ├─ Devices:
 │      45. Audio interne
 │
 ├─ Sinks:
 │  *   52. Haut-parleurs intégrés             [vol: 0.65]
 │      60. HDMI / DisplayPort                 [vol: 1.00]
 │
 ├─ Sources:
 │      53. Microphone                          [vol: 1.00]

Video
 ├─ Sinks:
 │      70. Pas de l'audio
";
        let sinks = parse_sinks(status);
        assert_eq!(sinks.len(), 2);
        assert_eq!(
            sinks[0],
            Sink {
                id: 52,
                name: "Haut-parleurs intégrés".into(),
                default: true
            }
        );
        assert_eq!(sinks[1].name, "HDMI / DisplayPort");
    }
}
