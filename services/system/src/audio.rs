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

/// Entrées audio (micros).
pub fn sources() -> Result<Vec<Sink>, Error> {
    Ok(parse_nodes(&command::run("wpctl", &["status"])?, "Sources:"))
}

/// Section « Sinks: » de la partie « Audio » de `wpctl status`.
pub fn parse_sinks(status: &str) -> Vec<Sink> {
    parse_nodes(status, "Sinks:")
}

/// Une section de la partie « Audio » de `wpctl status`.
fn parse_nodes(status: &str, section: &str) -> Vec<Sink> {
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
            in_sinks = clean == section;
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

/// Volume de l'entrée par défaut, de 0 à 100.
pub fn input_volume() -> Option<u8> {
    let output = command::run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SOURCE@"]).ok()?;
    parse_volume(&output).map(|volume| volume.level)
}

pub fn set_input_volume(level: u8) -> Result<(), Error> {
    let value = format!("{}%", level.min(100));
    command::run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SOURCE@", &value]).map(drop)
}

/// Balance de la sortie, de -100 (tout à gauche) à 100 (tout à droite).
pub fn balance() -> Option<i8> {
    let output = command::run("pactl", &["get-sink-volume", "@DEFAULT_SINK@"]).ok()?;
    parse_balance(&output)
}

/// Pourcentages du canal gauche et du canal droit dans `pactl get-sink-volume`.
pub fn parse_balance(output: &str) -> Option<i8> {
    let levels: Vec<i32> = output
        .split('/')
        .filter_map(|part| part.trim().strip_suffix('%')?.trim().parse().ok())
        .collect();
    let [left, right, ..] = levels.as_slice() else {
        return None;
    };
    let loudest = (*left).max(*right);
    if loudest == 0 {
        return Some(0);
    }
    Some(((right - left) * 100 / loudest).clamp(-100, 100) as i8)
}

/// Règle la balance en gardant le volume : le canal opposé est atténué.
pub fn set_balance(level: u8, balance: i8) -> Result<(), Error> {
    let level = i32::from(level.min(100));
    let side = |attenuated: bool| {
        if attenuated {
            level * (100 - i32::from(balance).abs()) / 100
        } else {
            level
        }
    };
    let (left, right) = (side(balance > 0), side(balance < 0));
    command::run(
        "pactl",
        &[
            "set-sink-volume",
            "@DEFAULT_SINK@",
            &format!("{left}%"),
            &format!("{right}%"),
        ],
    )
    .map(drop)
}

/// Application qui joue du son.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stream {
    pub id: u32,
    pub name: String,
    pub volume: u8,
}

pub fn streams() -> Result<Vec<Stream>, Error> {
    Ok(parse_streams(&command::run("pactl", &["list", "sink-inputs"])?))
}

/// Analyse `pactl list sink-inputs`.
pub fn parse_streams(output: &str) -> Vec<Stream> {
    let mut streams: Vec<Stream> = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if let Some(id) = line.strip_prefix("Sink Input #").and_then(|id| id.parse().ok()) {
            streams.push(Stream {
                id,
                name: String::new(),
                volume: 100,
            });
            continue;
        }
        let Some(stream) = streams.last_mut() else {
            continue;
        };
        if let Some(volume) = line.strip_prefix("Volume:") {
            let percent = volume
                .split('/')
                .find_map(|part| part.trim().strip_suffix('%')?.trim().parse::<u8>().ok());
            stream.volume = percent.unwrap_or(stream.volume);
        } else if let Some(name) = line.strip_prefix("application.name = ") {
            stream.name = name.trim_matches('"').to_owned();
        }
    }
    streams
}

pub fn set_stream_volume(id: u32, level: u8) -> Result<(), Error> {
    command::run(
        "pactl",
        &[
            "set-sink-input-volume",
            &id.to_string(),
            &format!("{}%", level.min(100)),
        ],
    )
    .map(drop)
}

/// Son d'essai sur la sortie par défaut.
pub fn test_speakers() -> Result<(), Error> {
    command::run_with_timeout(
        "speaker-test",
        &["-t", "sine", "-f", "440", "-l", "1", "-c", "2"],
        std::time::Duration::from_secs(6),
    )
    .map(drop)
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

    #[test]
    fn balance_des_canaux() {
        let centre = "Volume: front-left: 42598 /  65% / -11 dB,   front-right: 42598 /  65% / -11 dB";
        assert_eq!(parse_balance(centre), Some(0));
        let droite = "Volume: front-left: 1 /  32% / -1 dB,   front-right: 1 /  64% / -1 dB";
        assert_eq!(parse_balance(droite), Some(50));
        assert_eq!(parse_balance("rien"), None);
    }

    #[test]
    fn son_par_application() {
        let output = "Sink Input #12\n\tVolume: front-left: 52428 /  80% / -5 dB\n\tProperties:\n\t\tapplication.name = \"Firefox\"\nSink Input #15\n\tVolume: mono: 65536 / 100% / 0 dB\n";
        let streams = parse_streams(output);
        assert_eq!(streams[0].name, "Firefox");
        assert_eq!(streams[0].volume, 80);
        assert_eq!((streams[1].id, streams[1].volume), (15, 100));
    }
}
