//! Sons système de Ferrite (Paramètres → Son → Sons système).
//!
//! Les sons sont fabriqués ici (sinusoïdes et bruit mis en forme), écrits une fois en WAV
//! dans le dossier d'exécution, puis joués par `pw-play` sans attendre : rien à installer,
//! aucune licence de fichier son. Réglages : `son_alerte` (`aucun` pour couper),
//! `sons_volume` (pour cent), `sons_actions` (Corbeille, capture d'écran) et
//! `son_demarrage`.

use std::{
    env,
    f32::consts::TAU,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::config::Settings;

const RATE: u32 = 48_000;
/// Augmenté quand un son change : l'ancien fichier n'est plus utilisé.
const VERSION: u32 = 1;

/// Son joué à l'arrivée d'une notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    Drop,
    Bell,
    Glass,
    Soft,
}

impl Alert {
    pub const ALL: [Alert; 4] = [Alert::Drop, Alert::Bell, Alert::Glass, Alert::Soft];

    pub fn key(self) -> &'static str {
        match self {
            Alert::Drop => "goutte",
            Alert::Bell => "cloche",
            Alert::Glass => "verre",
            Alert::Soft => "douce",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Alert::Drop => "Goutte",
            Alert::Bell => "Cloche",
            Alert::Glass => "Verre",
            Alert::Soft => "Douce",
        }
    }

    fn parse(value: &str) -> Option<Alert> {
        Alert::ALL.into_iter().find(|alert| alert.key() == value.trim())
    }
}

/// Un son système.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Alert(Alert),
    /// Fichiers mis à la Corbeille.
    Trash,
    /// Capture d'écran prise.
    Screenshot,
    /// Ouverture de la session.
    Startup,
}

impl Sound {
    fn file_name(self) -> String {
        let name = match self {
            Sound::Alert(alert) => alert.key(),
            Sound::Trash => "corbeille",
            Sound::Screenshot => "capture",
            Sound::Startup => "demarrage",
        };
        format!("{name}-{VERSION}.wav")
    }
}

pub const DEFAULT_VOLUME: u32 = 50;

impl Settings {
    /// Son des notifications ; `None` : aucun.
    pub fn alert_sound(&self) -> Option<Alert> {
        match self.get("son_alerte") {
            Some(value) if value.trim() == "aucun" => None,
            Some(value) => Some(Alert::parse(value).unwrap_or(Alert::Drop)),
            None => Some(Alert::Drop),
        }
    }

    /// Volume des sons système, en pour cent.
    pub fn sounds_volume(&self) -> u32 {
        self.get("sons_volume")
            .and_then(|value| value.trim().parse::<u32>().ok())
            .map_or(DEFAULT_VOLUME, |volume| volume.min(100))
    }

    /// Sons des actions (Corbeille, capture d'écran) ; activés par défaut.
    pub fn action_sounds(&self) -> bool {
        self.flag("sons_actions", true)
    }

    /// Son à l'ouverture de la session ; désactivé par défaut.
    pub fn startup_sound(&self) -> bool {
        self.flag("son_demarrage", false)
    }
}

/// Joue `sound` si les réglages le permettent, sans attendre. Le bureau, Fichiers et
/// Paramètres l'appellent ; une erreur (pas de son, `pw-play` absent) est seulement notée.
pub fn play(sound: Sound) {
    // Lecture des réglages et premier écriture du fichier : dans un fil, pour que
    // l'appelant (la boucle du bureau) n'attende jamais.
    std::thread::spawn(move || {
        let settings = Settings::load();
        let sound = match sound {
            // Le son d'alerte choisi dans Paramètres, quel que soit celui demandé.
            Sound::Alert(_) => settings.alert_sound().map(Sound::Alert),
            Sound::Trash | Sound::Screenshot => settings.action_sounds().then_some(sound),
            Sound::Startup => settings.startup_sound().then_some(sound),
        };
        if let Some(sound) = sound {
            play_at(sound, settings.sounds_volume());
        }
    });
}

/// Joue `sound` au volume donné, même si les réglages le coupent (essai dans Paramètres).
pub fn play_at(sound: Sound, volume: u32) {
    if volume == 0 {
        return;
    }
    let path = match ensure_file(sound) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("son système non préparé : {error}");
            return;
        }
    };
    let mut command = Command::new("pw-play");
    command
        .arg("--media-role")
        .arg("Notification")
        .arg("--volume")
        .arg(format!("{:.2}", f64::from(volume.min(100)) / 100.0))
        .arg(path);
    if let Err(error) = crate::command::spawn_detached(command) {
        eprintln!("son système non joué (pw-play) : {error}");
    }
}

/// Fichier WAV du son, écrit la première fois.
fn ensure_file(sound: Sound) -> io::Result<PathBuf> {
    let dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("ferrite-sons");
    let path = dir.join(sound.file_name());
    if !path.is_file() {
        fs::create_dir_all(&dir)?;
        write_atomic(&path, &wav(&synthesize(sound)))?;
    }
    Ok(path)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}

// ---- Fabrication des sons ------------------------------------------------------------

/// Échantillons du son, entre -1 et 1, à 48 kHz.
fn synthesize(sound: Sound) -> Vec<f32> {
    let mut samples = match sound {
        Sound::Alert(Alert::Drop) => drop_sound(),
        Sound::Alert(Alert::Bell) => chime(0.0, 1046.5, &BELL, 0.35, 0.9, 1.0),
        Sound::Alert(Alert::Glass) => chime(0.0, 1568.0, &GLASS, 0.18, 0.6, 1.0),
        Sound::Alert(Alert::Soft) => {
            let mut first = chime(0.0, 784.0, &SOFT, 0.12, 0.45, 1.0);
            mix(&mut first, &chime(0.11, 1046.5, &SOFT, 0.14, 0.45, 0.9));
            first
        }
        Sound::Trash => rustle(),
        Sound::Screenshot => shutter(),
        Sound::Startup => {
            let mut samples = vec![0.0; seconds(1.6)];
            for (index, frequency) in [523.25, 659.25, 783.99, 1046.5].into_iter().enumerate() {
                let start = index as f32 * 0.12;
                mix(&mut samples, &chime(start, frequency, &SOFT, 0.5, 1.6, 0.8));
            }
            samples
        }
    };
    normalize(&mut samples, 0.8);
    samples
}

/// Partiels (rapport de fréquence, force) : cloche, verre, son doux.
const BELL: [(f32, f32); 4] = [(1.0, 1.0), (2.0, 0.35), (3.01, 0.2), (4.2, 0.1)];
const GLASS: [(f32, f32); 3] = [(1.0, 1.0), (2.76, 0.3), (5.4, 0.12)];
const SOFT: [(f32, f32); 2] = [(1.0, 1.0), (2.0, 0.15)];

fn seconds(duration: f32) -> usize {
    (duration * RATE as f32) as usize
}

/// Note qui commence à `start` secondes, attaque de 5 ms puis décroissance exponentielle.
fn chime(
    start: f32,
    frequency: f32,
    partials: &[(f32, f32)],
    decay: f32,
    length: f32,
    gain: f32,
) -> Vec<f32> {
    let mut samples = vec![0.0; seconds(length)];
    for (index, sample) in samples.iter_mut().enumerate().skip(seconds(start)) {
        let t = (index - seconds(start)) as f32 / RATE as f32;
        let envelope = (t / 0.005).min(1.0) * (-t / decay).exp();
        let tone: f32 = partials
            .iter()
            .map(|(ratio, strength)| strength * (TAU * frequency * ratio * t).sin())
            .sum();
        *sample = gain * envelope * tone;
    }
    samples
}

/// Goutte : note qui glisse vite vers le grave.
fn drop_sound() -> Vec<f32> {
    let mut phase = 0.0f32;
    (0..seconds(0.18))
        .map(|index| {
            let t = index as f32 / RATE as f32;
            let frequency = 600.0 + 900.0 * (-t / 0.03).exp();
            phase += TAU * frequency / RATE as f32;
            let envelope = (t / 0.003).min(1.0) * (-t / 0.06).exp();
            envelope * phase.sin()
        })
        .collect()
}

/// Bruit pseudo-aléatoire reproductible.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// Bouffées de bruit adouci (passe-bas), aux instants et durées donnés.
fn bursts(length: f32, bursts: &[(f32, f32)], smoothing: f32, seed: u32) -> Vec<f32> {
    let mut noise = Noise(seed);
    let mut low = 0.0f32;
    (0..seconds(length))
        .map(|index| {
            let t = index as f32 / RATE as f32;
            let envelope: f32 = bursts
                .iter()
                .filter(|(start, _)| t >= *start)
                .map(|(start, duration)| (-(t - start) / duration).exp())
                .sum();
            low += (noise.next() - low) * smoothing;
            envelope * low
        })
        .collect()
}

/// Corbeille : froissement de papier, trois bouffées sourdes.
fn rustle() -> Vec<f32> {
    bursts(
        0.22,
        &[(0.0, 0.025), (0.05, 0.02), (0.1, 0.03)],
        0.25,
        0x5EED_1234,
    )
}

/// Capture d'écran : deux déclics d'obturateur, plus clairs.
fn shutter() -> Vec<f32> {
    let clicks = bursts(0.16, &[(0.0, 0.008), (0.07, 0.012)], 0.9, 0xC0FF_EE11);
    // Passe-haut (différence) : un déclic sec, pas un souffle.
    let mut previous = 0.0;
    clicks
        .into_iter()
        .map(|sample| {
            let high = sample - previous;
            previous = sample;
            high
        })
        .collect()
}

fn mix(into: &mut Vec<f32>, other: &[f32]) {
    if other.len() > into.len() {
        into.resize(other.len(), 0.0);
    }
    for (sample, add) in into.iter_mut().zip(other) {
        *sample += add;
    }
}

fn normalize(samples: &mut [f32], peak: f32) {
    let loudest = samples.iter().fold(0.0f32, |max, sample| max.max(sample.abs()));
    if loudest > 0.0 {
        let gain = peak / loudest;
        samples.iter_mut().for_each(|sample| *sample *= gain);
    }
}

/// WAV 16 bits mono.
fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Sound; 7] = [
        Sound::Alert(Alert::Drop),
        Sound::Alert(Alert::Bell),
        Sound::Alert(Alert::Glass),
        Sound::Alert(Alert::Soft),
        Sound::Trash,
        Sound::Screenshot,
        Sound::Startup,
    ];

    #[test]
    fn sons_courts_et_sans_saturation() {
        for sound in ALL {
            let samples = synthesize(sound);
            let length = samples.len() as f32 / RATE as f32;
            assert!(length > 0.1 && length <= 1.6, "{sound:?} : {length} s");
            let peak = samples.iter().fold(0.0f32, |max, s| max.max(s.abs()));
            assert!((peak - 0.8).abs() < 0.01, "{sound:?} : crête {peak}");
            // Le son s'éteint de lui-même : la fin est presque silencieuse.
            let tail = samples[samples.len() - 100..]
                .iter()
                .fold(0.0f32, |max, s| max.max(s.abs()));
            assert!(tail < 0.1, "{sound:?} : fin {tail}");
        }
    }

    #[test]
    fn fichier_wav_valide() {
        let bytes = wav(&[0.0, 1.0, -1.0]);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(i16::from_le_bytes([bytes[46], bytes[47]]), i16::MAX);
    }

    #[test]
    fn reglages_des_sons() {
        let defaults = Settings::parse("");
        assert_eq!(defaults.alert_sound(), Some(Alert::Drop));
        assert_eq!(defaults.sounds_volume(), 50);
        assert!(defaults.action_sounds());
        assert!(!defaults.startup_sound());
        let settings =
            Settings::parse("son_alerte=aucun\nsons_volume=250\nsons_actions=non\nson_demarrage=oui\n");
        assert_eq!(settings.alert_sound(), None);
        assert_eq!(settings.sounds_volume(), 100);
        assert!(!settings.action_sounds());
        assert!(settings.startup_sound());
        assert_eq!(
            Settings::parse("son_alerte=verre").alert_sound(),
            Some(Alert::Glass)
        );
        assert_eq!(Settings::parse("son_alerte=??").alert_sound(), Some(Alert::Drop));
    }
}
