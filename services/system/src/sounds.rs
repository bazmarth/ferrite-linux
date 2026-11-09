//! Sons système de Ferrite (Paramètres → Son → Sons système).
//!
//! Les sons sont fabriqués ici, comme dans un petit studio : timbres de cloche, de verre,
//! de marimba et de bois (partiels mesurés sur de vrais instruments), nappe d'accord pour
//! le démarrage, papier froissé et obturateur pour les actions ; puis placement en stéréo,
//! réverbération de pièce, niveau sonore égalisé d'un son à l'autre. Ils sont écrits une
//! fois en WAV dans le dossier d'exécution, puis joués par `pw-play` sans attendre : rien à
//! installer, aucune licence de fichier son. Réglages : `son_alerte` (`aucun` pour couper),
//! `sons_volume` (pour cent), `sons_actions` (Corbeille, capture d'écran, chargeur) et
//! `son_demarrage`.

use std::{
    env,
    f32::consts::{PI, TAU},
    f64::consts::TAU as TAU64,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::config::Settings;

const RATE: u32 = 48_000;
const RATE_F: f32 = RATE as f32;
/// Augmenté quand un son change : l'ancien fichier n'est plus utilisé.
const VERSION: u32 = 2;

/// Son joué à l'arrivée d'une notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    Drop,
    Bell,
    Glass,
    Soft,
    Crystal,
    Wood,
}

impl Alert {
    pub const ALL: [Alert; 6] = [
        Alert::Drop,
        Alert::Soft,
        Alert::Glass,
        Alert::Crystal,
        Alert::Bell,
        Alert::Wood,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Alert::Drop => "goutte",
            Alert::Bell => "cloche",
            Alert::Glass => "verre",
            Alert::Soft => "douce",
            Alert::Crystal => "cristal",
            Alert::Wood => "bois",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Alert::Drop => "Goutte",
            Alert::Bell => "Cloche",
            Alert::Glass => "Verre",
            Alert::Soft => "Douce",
            Alert::Crystal => "Cristal",
            Alert::Wood => "Bois",
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
    /// Chargeur branché.
    Plugged,
    /// Chargeur débranché.
    Unplugged,
}

impl Sound {
    fn file_name(self) -> String {
        let name = match self {
            Sound::Alert(alert) => alert.key(),
            Sound::Trash => "corbeille",
            Sound::Screenshot => "capture",
            Sound::Startup => "demarrage",
            Sound::Plugged => "chargeur-branche",
            Sound::Unplugged => "chargeur-debranche",
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

    /// Sons des actions (Corbeille, capture d'écran, chargeur) ; activés par défaut.
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
            Sound::Trash | Sound::Screenshot | Sound::Plugged | Sound::Unplugged => {
                settings.action_sounds().then_some(sound)
            }
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

/// Écrit tous les sons en WAV dans `dir` (pour les écouter ou les vérifier hors du bureau).
pub fn export_all(dir: &Path) -> io::Result<Vec<PathBuf>> {
    fs::create_dir_all(dir)?;
    let mut sounds: Vec<Sound> = Alert::ALL.into_iter().map(Sound::Alert).collect();
    sounds.extend([
        Sound::Trash,
        Sound::Screenshot,
        Sound::Plugged,
        Sound::Unplugged,
        Sound::Startup,
    ]);
    sounds
        .into_iter()
        .map(|sound| {
            let path = dir.join(sound.file_name());
            write_atomic(&path, &wav(&synthesize(sound)))?;
            Ok(path)
        })
        .collect()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}

// ---- Les sons ------------------------------------------------------------------------

/// Échantillons stéréo du son, entre -1 et 1, à 48 kHz.
fn synthesize(sound: Sound) -> Vec<[f32; 2]> {
    let mut mix = Mix::default();
    // Sonie visée (LUFS, passage le plus fort) : tous les sons ont à peu près la même
    // force à l'oreille, les actions un peu en retrait des alertes.
    let loudness = match sound {
        Sound::Alert(alert) => {
            alert_sound(&mut mix, alert);
            -12.0
        }
        Sound::Trash => {
            crumple(&mut mix);
            -15.0
        }
        Sound::Screenshot => {
            shutter(&mut mix);
            -15.0
        }
        Sound::Startup => {
            startup(&mut mix);
            -13.0
        }
        Sound::Plugged => {
            charger(&mut mix, true);
            -13.5
        }
        Sound::Unplugged => {
            charger(&mut mix, false);
            -15.0
        }
    };
    mix.finish(loudness)
}

/// Cloche d'église : bourdon à l'octave basse, tierce mineure, quinte, nominale…
/// (rapport de fréquence, force, durée de décroissance en secondes).
const BELL: [(f32, f32, f32); 10] = [
    (0.5, 0.3, 0.55),
    (1.0, 1.0, 0.45),
    (1.19, 0.38, 0.3),
    (1.5, 0.2, 0.24),
    (2.0, 0.42, 0.26),
    (2.51, 0.15, 0.14),
    (2.66, 0.12, 0.11),
    (3.01, 0.09, 0.09),
    (4.16, 0.05, 0.06),
    (5.43, 0.03, 0.04),
];
/// Lame de verre (barre libre) : partiels inharmoniques, les aigus s'éteignent vite.
const GLASS: [(f32, f32, f32); 4] = [
    (1.0, 1.0, 0.3),
    (2.756, 0.36, 0.11),
    (5.404, 0.15, 0.045),
    (8.933, 0.05, 0.022),
];
/// Bloc de bois : très amorti.
const WOOD: [(f32, f32, f32); 3] = [(1.0, 1.0, 0.075), (2.7, 0.38, 0.025), (5.1, 0.14, 0.011)];
/// Carillon doux du chargeur : presque harmonique, avec une pointe de verre.
const CHIME: [(f32, f32, f32); 4] = [
    (1.0, 1.0, 0.3),
    (2.0, 0.16, 0.12),
    (2.756, 0.1, 0.06),
    (4.0, 0.04, 0.035),
];

fn alert_sound(mix: &mut Mix, alert: Alert) {
    match alert {
        // Goutte d'eau : la note monte très vite (bulle qui se referme), un petit rebond.
        Alert::Drop => {
            mix.add(0.0, &glide(760.0, 1480.0, 0.016, 0.05), 1.0, -0.05);
            mix.add(0.075, &glide(1020.0, 1760.0, 0.012, 0.032), 0.28, 0.25);
            mix.reverb(&SMALL);
        }
        // Deux notes de marimba qui montent d'une quarte, comme une question polie.
        Alert::Soft => {
            mix.add(0.0, &mallet(783.99, 0.24, 11), 0.9, -0.2);
            mix.add(0.115, &mallet(1046.5, 0.3, 12), 1.0, 0.2);
            mix.reverb(&MEDIUM);
        }
        // Un seul tintement de verre, clair et bref.
        Alert::Glass => {
            mix.add(0.0, &modal(1567.98, &GLASS, 0.0008, 1.4), 1.0, 0.0);
            mix.add(
                0.0,
                &noise_hit(Biquad::bandpass(6_200.0, 1.4), 0.0012, 21),
                1.6,
                0.0,
            );
            mix.reverb(&MEDIUM);
        }
        // Trois notes de verre en arpège (mi, sol dièse, si), de gauche à droite.
        Alert::Crystal => {
            let notes = [(1318.51, -0.4), (1661.22, 0.0), (1975.53, 0.4)];
            for (index, (frequency, pan)) in notes.into_iter().enumerate() {
                let start = index as f32 * 0.075;
                let length = 0.7 + index as f32 * 0.35;
                let gain = 1.0 - index as f32 * 0.12;
                mix.add(start, &modal(frequency, &GLASS, 0.0008, length), gain, pan);
                let tick = noise_hit(Biquad::bandpass(7_000.0, 1.5), 0.001, 31 + index as u32);
                mix.add(start, &tick, gain, pan);
            }
            mix.reverb(&SPARKLE);
        }
        // Petite cloche, avec son battement et sa longue résonance.
        Alert::Bell => {
            mix.add(0.0, &modal(880.0, &BELL, 0.0015, 2.4), 1.0, 0.0);
            mix.add(
                0.0,
                &noise_hit(Biquad::bandpass(3_600.0, 1.0), 0.002, 41),
                1.1,
                0.0,
            );
            mix.reverb(&MEDIUM);
        }
        // Deux coups de bois, secs et chaleureux.
        Alert::Wood => {
            for (index, (frequency, start, pan)) in [(880.0, 0.0, -0.15), (1174.66, 0.12, 0.15)]
                .into_iter()
                .enumerate()
            {
                mix.add(start, &modal(frequency, &WOOD, 0.0008, 0.35), 1.0, pan);
                let seed = 51 + index as u32;
                mix.add(
                    start,
                    &noise_hit(Biquad::bandpass(2_600.0, 1.6), 0.0016, seed),
                    1.3,
                    pan,
                );
            }
            mix.reverb(&SMALL);
        }
    }
}

/// Corbeille : une feuille froissée (des dizaines de crépitements de papier), puis le
/// petit choc mat de la boule au fond de la corbeille.
fn crumple(mix: &mut Mix) {
    let mut random = Noise(0x5EED_1234);
    for index in 0..110 {
        // Plus dense au début, quand on serre la feuille.
        let start = 0.3 * random.unit().powf(1.5);
        let center = 1_300.0 + 5_200.0 * random.unit();
        let q = 0.9 + 2.0 * random.unit();
        let decay = 0.0015 + 0.006 * random.unit();
        let gain = (0.35 + 0.65 * random.unit()) * (1.0 - start / 0.38);
        let pan = random.next() * 0.55;
        let grain = noise_hit(Biquad::bandpass(center, q), decay, 1000 + index);
        mix.add(start, &grain, gain, pan);
    }
    let landing = 0.34;
    mix.add(landing, &glide(190.0, 120.0, 0.025, 0.045), 0.7, 0.05);
    mix.add(
        landing,
        &noise_hit(Biquad::lowpass(600.0, 0.7), 0.02, 77),
        2.2,
        0.05,
    );
    mix.reverb(&SMALL);
}

/// Capture d'écran : obturateur d'appareil photo, deux déclics mécaniques (le miroir
/// qui se lève, le rideau qui se referme), le second un peu plus clair.
fn shutter(mix: &mut Mix) {
    for (index, (start, bright, gain, pan)) in [(0.0, 1.0, 0.75, -0.12), (0.085, 1.14, 1.0, 0.12)]
        .into_iter()
        .enumerate()
    {
        let seed = 200 + index as u32 * 10;
        // Claquement sec.
        mix.add(
            start,
            &noise_hit(Biquad::highpass(2_500.0, 0.7), 0.0018, seed),
            gain * 0.7,
            pan,
        );
        // Le rideau qui frotte, juste après.
        let rub = noise_hit(Biquad::bandpass(4_500.0, 0.8), 0.009, seed + 1);
        mix.add(start + 0.003, &rub, gain * 0.9, pan);
        // Métal qui résonne brièvement.
        let ring = [(1.0, 1.0, 0.018), (1.53, 0.6, 0.012), (2.41, 0.35, 0.007)];
        mix.add(
            start,
            &modal(3_050.0 * bright, &ring, 0.0003, 0.12),
            gain * 0.35,
            pan,
        );
        // Le boîtier.
        mix.add(
            start,
            &glide(320.0 * bright, 230.0, 0.006, 0.014),
            gain * 0.3,
            pan,
        );
    }
    // Les claquements, très brefs, sont arrondis pour sonner aussi fort que les autres.
    mix.saturate(3.0);
    mix.reverb(&SMALL);
}

/// Chargeur : deux notes de carillon, une quinte qui monte (branché) ou descend
/// (débranché, plus discret).
fn charger(mix: &mut Mix, plugged: bool) {
    let (first, second) = if plugged {
        (880.0, 1318.51)
    } else {
        (1318.51, 880.0)
    };
    let tail = if plugged { 0.9 } else { 0.6 };
    mix.add(0.0, &modal(first, &CHIME, 0.001, 0.6), 0.8, -0.15);
    mix.add(0.09, &modal(second, &CHIME, 0.001, tail), 1.0, 0.15);
    for (index, (start, frequency)) in [(0.0, first), (0.09, second)].into_iter().enumerate() {
        let tick = noise_hit(Biquad::bandpass(frequency * 4.0, 1.2), 0.001, 300 + index as u32);
        mix.add(start, &tick, 0.7, 0.0);
    }
    mix.reverb(&MEDIUM);
}

/// Démarrage : un accord de ré majeur (avec la neuvième) qui s'ouvre lentement, comme un
/// filtre qui s'éclaircit, et quatre notes cristallines qui montent au-dessus.
fn startup(mix: &mut Mix) {
    let chord = [
        (146.83, 0.0),
        (220.0, -0.4),
        (369.99, 0.35),
        (440.0, -0.2),
        (659.25, 0.45),
    ];
    for (index, (frequency, pan)) in chord.into_iter().enumerate() {
        let gain = 0.32 / (1.0 + index as f32 * 0.15);
        mix.add(
            index as f32 * 0.03,
            &pad(frequency, 3.2, 400 + index as u32),
            gain,
            pan,
        );
    }
    let sparkles = [
        (0.0, 1174.66, -0.35, 0.6),
        (0.15, 1479.98, 0.3, 0.5),
        (0.3, 1760.0, -0.1, 0.45),
        (0.48, 2217.46, 0.4, 0.3),
    ];
    for (index, (start, frequency, pan, gain)) in sparkles.into_iter().enumerate() {
        let length = 1.2 + index as f32 * 0.3;
        mix.add(start, &modal(frequency, &GLASS, 0.0015, length), gain, pan);
    }
    mix.reverb(&HALL);
}

// ---- Instruments ---------------------------------------------------------------------

fn seconds(duration: f32) -> usize {
    (duration.max(0.0) * RATE_F) as usize
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Attaque arrondie de `attack` secondes, puis décroissance exponentielle.
fn envelope(t: f32, attack: f32, decay: f32) -> f32 {
    let rise = if t < attack {
        0.5 - 0.5 * (PI * t / attack).cos()
    } else {
        1.0
    };
    rise * (-t / decay).exp()
}

/// Corps qui résonne : chaque partiel (rapport, force, décroissance) est doublé d'un jumeau
/// à peine désaccordé, ce qui donne le léger battement des vrais objets. Le son s'éteint
/// en douceur sur le dernier quart de `length`.
fn modal(frequency: f32, partials: &[(f32, f32, f32)], attack: f32, length: f32) -> Vec<f32> {
    let total = seconds(length);
    let mut samples = vec![0.0; total];
    for &(ratio, strength, decay) in partials {
        let partial = f64::from(frequency * ratio);
        if partial > 18_000.0 {
            continue;
        }
        let twin = partial * (1.0 + 0.0007 * f64::from(ratio).sqrt());
        for (index, sample) in samples.iter_mut().enumerate() {
            let t = index as f32 / RATE_F;
            let level = strength * envelope(t, attack, decay);
            if t > attack && level < 1e-5 {
                break;
            }
            let time = index as f64 / f64::from(RATE);
            let tone = (TAU64 * partial * time).sin() + 0.3 * (TAU64 * twin * time + 0.7).sin();
            *sample += level * tone as f32 / 1.3;
        }
    }
    let fade = (total / 4).max(1);
    for (index, sample) in samples.iter_mut().enumerate().skip(total - fade) {
        *sample *= smooth((total - index) as f32 / fade as f32);
    }
    samples
}

/// Lame de marimba frappée par une mailloche douce.
fn mallet(frequency: f32, decay: f32, seed: u32) -> Vec<f32> {
    let partials = [
        (1.0, 1.0, decay),
        (3.93, 0.26, decay * 0.22),
        (9.2, 0.06, decay * 0.07),
    ];
    let mut samples = modal(frequency, &partials, 0.0015, decay * 4.5);
    let knock = noise_hit(Biquad::bandpass(frequency * 2.5, 0.9), 0.0015, seed);
    mix_into(&mut samples, &knock, 0.6);
    samples
}

/// Note dont la hauteur glisse de `from` vers `to` (constante de temps `glide`).
fn glide(from: f32, to: f32, glide: f32, decay: f32) -> Vec<f32> {
    let mut phase = 0.0f64;
    (0..seconds(decay * 9.0))
        .map(|index| {
            let t = index as f32 / RATE_F;
            let frequency = to + (from - to) * (-t / glide).exp();
            phase += TAU64 * f64::from(frequency) / f64::from(RATE);
            let tone = phase.sin() + 0.1 * (2.0 * phase).sin();
            envelope(t, 0.0008, decay) * tone as f32
        })
        .collect()
}

/// Bouffée de bruit passée dans `filter` : claquement, crépitement, souffle.
fn noise_hit(mut filter: Biquad, decay: f32, seed: u32) -> Vec<f32> {
    let mut noise = Noise(seed.wrapping_mul(0x9E37_79B9) | 1);
    (0..seconds(decay * 8.0 + 0.01))
        .map(|index| {
            let t = index as f32 / RATE_F;
            filter.process(noise.next()) * envelope(t, 0.0003, decay)
        })
        .collect()
}

/// Nappe : trois voix légèrement désaccordées, riches en harmoniques qui apparaissent peu
/// à peu (filtre qui s'ouvre), puis se retirent pendant que le son s'éteint.
fn pad(frequency: f32, length: f32, seed: u32) -> Vec<f32> {
    let total = seconds(length);
    let mut samples = vec![0.0; total];
    let mut noise = Noise(seed.wrapping_mul(0x9E37_79B9) | 1);
    for cents in [-7.0f32, 0.0, 6.0] {
        let voice = f64::from(frequency * 2f32.powf(cents / 1200.0));
        let offset = f64::from(noise.unit()) * TAU64;
        let harmonics = ((7_000.0 / voice) as usize).clamp(1, 14);
        for (index, sample) in samples.iter_mut().enumerate() {
            let t = index as f32 / RATE_F;
            let level = smooth(t / 0.5) * if t < 1.2 { 1.0 } else { (-(t - 1.2) / 0.55).exp() };
            // Plus `darkness` est grand, plus les harmoniques aiguës sont faibles.
            let darkness = 1.15 - 0.75 * smooth(t / 1.0) + 0.45 * smooth((t - 1.3) / 1.5);
            let ratio = f64::from((-darkness).exp());
            let theta = TAU64 * voice * index as f64 / f64::from(RATE) + offset;
            let (sine, cosine) = theta.sin_cos();
            // sin(nθ) par récurrence : une seule fonction trigonométrique par voix.
            let (mut previous, mut current) = (0.0f64, sine);
            let (mut weight, mut sum) = (1.0f64, 0.0f64);
            for harmonic in 1..=harmonics {
                sum += weight / harmonic as f64 * current;
                let next = 2.0 * cosine * current - previous;
                previous = current;
                current = next;
                weight *= ratio;
            }
            *sample += level * sum as f32 / 3.0;
        }
    }
    let fade = total / 5;
    for (index, sample) in samples.iter_mut().enumerate().skip(total - fade) {
        *sample *= smooth((total - index) as f32 / fade as f32);
    }
    samples
}

fn mix_into(into: &mut Vec<f32>, other: &[f32], gain: f32) {
    if other.len() > into.len() {
        into.resize(other.len(), 0.0);
    }
    for (sample, add) in into.iter_mut().zip(other) {
        *sample += add * gain;
    }
}

/// Bruit pseudo-aléatoire reproductible.
#[derive(Debug)]
struct Noise(u32);

impl Noise {
    /// Entre -1 et 1.
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    /// Entre 0 et 1.
    fn unit(&mut self) -> f32 {
        (self.next() + 1.0) / 2.0
    }
}

/// Filtre du second ordre (formules classiques de R. Bristow-Johnson).
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl Biquad {
    fn new(frequency: f32, q: f32, b: impl Fn(f32, f32) -> [f32; 3]) -> Biquad {
        let omega = TAU * frequency.min(RATE_F * 0.45) / RATE_F;
        let (sine, cosine) = omega.sin_cos();
        let alpha = sine / (2.0 * q);
        let a0 = 1.0 + alpha;
        let [b0, b1, b2] = b(cosine, alpha);
        Biquad {
            b: [b0 / a0, b1 / a0, b2 / a0],
            a: [-2.0 * cosine / a0, (1.0 - alpha) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn raw(b: [f32; 3], a: [f32; 2]) -> Biquad {
        Biquad {
            b,
            a,
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn bandpass(frequency: f32, q: f32) -> Biquad {
        Biquad::new(frequency, q, |_, alpha| [alpha, 0.0, -alpha])
    }

    fn lowpass(frequency: f32, q: f32) -> Biquad {
        Biquad::new(frequency, q, |cosine, _| {
            [(1.0 - cosine) / 2.0, 1.0 - cosine, (1.0 - cosine) / 2.0]
        })
    }

    fn highpass(frequency: f32, q: f32) -> Biquad {
        Biquad::new(frequency, q, |cosine, _| {
            [(1.0 + cosine) / 2.0, -(1.0 + cosine), (1.0 + cosine) / 2.0]
        })
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.b[0] * input + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [input, self.x[0]];
        self.y = [output, self.y[0]];
        output
    }
}

// ---- Table de mixage -----------------------------------------------------------------

/// Pièce de la réverbération : taille (0 à 1), amortissement des aigus, part du son
/// réverbéré, pré-délai et durée ajoutée pour laisser la queue s'éteindre.
#[derive(Debug)]
struct Room {
    size: f32,
    damping: f32,
    wet: f32,
    predelay: f32,
    tail: f32,
}

const SMALL: Room = Room {
    size: 0.45,
    damping: 0.5,
    wet: 0.1,
    predelay: 0.004,
    tail: 0.45,
};
const MEDIUM: Room = Room {
    size: 0.66,
    damping: 0.45,
    wet: 0.15,
    predelay: 0.01,
    tail: 1.0,
};
const SPARKLE: Room = Room {
    size: 0.74,
    damping: 0.35,
    wet: 0.2,
    predelay: 0.014,
    tail: 1.2,
};
const HALL: Room = Room {
    size: 0.84,
    damping: 0.32,
    wet: 0.26,
    predelay: 0.02,
    tail: 1.6,
};

/// Délais des filtres de la réverbération (échantillons à 44,1 kHz, réglage « Freeverb »).
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
/// Décalage du canal droit : les deux côtés ne sont pas identiques, le son s'élargit.
const SPREAD: usize = 23;

#[derive(Debug)]
struct Comb {
    buffer: Vec<f32>,
    index: usize,
    feedback: f32,
    damping: f32,
    store: f32,
}

impl Comb {
    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.index];
        self.store = output * (1.0 - self.damping) + self.store * self.damping;
        self.buffer[self.index] = input + self.store * self.feedback;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }
}

#[derive(Debug)]
struct AllPass {
    buffer: Vec<f32>,
    index: usize,
}

impl AllPass {
    fn process(&mut self, input: f32) -> f32 {
        let delayed = self.buffer[self.index];
        self.buffer[self.index] = input + delayed * 0.5;
        self.index = (self.index + 1) % self.buffer.len();
        delayed - input
    }
}

/// Un canal de réverbération.
#[derive(Debug)]
struct Tank {
    combs: Vec<Comb>,
    allpasses: Vec<AllPass>,
}

impl Tank {
    fn new(room: &Room, spread: usize) -> Tank {
        let scale = RATE_F / 44_100.0;
        let length = |delay: usize| (((delay + spread) as f32 * scale) as usize).max(1);
        Tank {
            combs: COMBS
                .iter()
                .map(|&delay| Comb {
                    buffer: vec![0.0; length(delay)],
                    index: 0,
                    feedback: 0.7 + 0.28 * room.size,
                    damping: room.damping,
                    store: 0.0,
                })
                .collect(),
            allpasses: ALLPASSES
                .iter()
                .map(|&delay| AllPass {
                    buffer: vec![0.0; length(delay)],
                    index: 0,
                })
                .collect(),
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let mut output: f32 = self.combs.iter_mut().map(|comb| comb.process(input)).sum();
        for allpass in &mut self.allpasses {
            output = allpass.process(output);
        }
        output
    }
}

/// Son stéréo en cours de mixage.
#[derive(Debug, Default)]
struct Mix {
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Mix {
    /// Pose `voice` à `start` secondes, placée de -1 (gauche) à 1 (droite) à puissance
    /// constante.
    fn add(&mut self, start: f32, voice: &[f32], gain: f32, pan: f32) {
        let angle = (pan.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
        let (left, right) = (
            gain * angle.cos() * std::f32::consts::SQRT_2,
            gain * angle.sin() * std::f32::consts::SQRT_2,
        );
        let offset = seconds(start);
        let end = offset + voice.len();
        if end > self.left.len() {
            self.left.resize(end, 0.0);
            self.right.resize(end, 0.0);
        }
        for (index, sample) in voice.iter().enumerate() {
            self.left[offset + index] += sample * left;
            self.right[offset + index] += sample * right;
        }
    }

    /// Saturation douce (tangente hyperbolique) : arrondit les crêtes des sons très brefs.
    fn saturate(&mut self, drive: f32) {
        let peak = self
            .left
            .iter()
            .chain(&self.right)
            .fold(0.0f32, |max, sample| max.max(sample.abs()));
        if peak > 0.0 {
            for sample in self.left.iter_mut().chain(self.right.iter_mut()) {
                *sample = (*sample / peak * drive).tanh();
            }
        }
    }

    /// Réverbération de pièce (huit filtres en peigne et quatre passe-tout par canal).
    fn reverb(&mut self, room: &Room) {
        let length = self.left.len() + seconds(room.tail);
        self.left.resize(length, 0.0);
        self.right.resize(length, 0.0);
        let predelay = seconds(room.predelay);
        let (mut tank_left, mut tank_right) = (Tank::new(room, 0), Tank::new(room, SPREAD));
        // Pas de graves dans la réverbération : elle resterait sourde et brouillée.
        let mut cut = Biquad::highpass(280.0, 0.7);
        let mut wet = vec![[0.0f32; 2]; length];
        for (index, out) in wet.iter_mut().enumerate() {
            let input = index
                .checked_sub(predelay)
                .map_or(0.0, |at| (self.left[at] + self.right[at]) * 0.015);
            let input = cut.process(input);
            let (left, right) = (tank_left.process(input), tank_right.process(input));
            // Largeur 0,9 : un peu de chaque côté dans l'autre.
            *out = [
                room.wet * (0.95 * left + 0.05 * right),
                room.wet * (0.95 * right + 0.05 * left),
            ];
        }
        for (index, [left, right]) in wet.into_iter().enumerate() {
            self.left[index] += left * 3.0;
            self.right[index] += right * 3.0;
        }
    }

    /// Mastering : coupe le continu, égalise le niveau, retire le silence final et
    /// arrondit les bords.
    fn finish(mut self, loudness: f32) -> Vec<[f32; 2]> {
        for channel in [&mut self.left, &mut self.right] {
            // Passe-haut vers 25 Hz.
            let (mut input, mut output) = (0.0f32, 0.0f32);
            for sample in channel.iter_mut() {
                output = *sample - input + 0.9967 * output;
                input = *sample;
                *sample = output;
            }
        }
        let mut samples: Vec<[f32; 2]> = self
            .left
            .iter()
            .zip(&self.right)
            .map(|(&left, &right)| [left, right])
            .collect();
        let peak = samples
            .iter()
            .fold(0.0f32, |max, [left, right]| max.max(left.abs()).max(right.abs()));
        if peak <= 0.0 {
            return samples;
        }
        // Le niveau visé, sans jamais dépasser une crête de -1 dB.
        let gain = 10f32.powf((loudness - momentary_loudness(&samples)) / 20.0);
        let gain = gain.min(0.89 / peak);
        for frame in &mut samples {
            frame.iter_mut().for_each(|sample| *sample *= gain);
        }
        // Fin : coupée 10 ms après le dernier passage au-dessus de -56 dB.
        let last = samples
            .iter()
            .rposition(|frame| frame.iter().any(|sample| sample.abs() > 0.0015))
            .unwrap_or(0);
        samples.truncate((last + seconds(0.01)).min(samples.len()));
        let fade_out = seconds(0.04).min(samples.len() / 4).max(1);
        let total = samples.len();
        for (index, frame) in samples.iter_mut().enumerate() {
            let rise = (index as f32 / 24.0).min(1.0);
            let fall = smooth((total - index) as f32 / fade_out as f32);
            frame.iter_mut().for_each(|sample| *sample *= rise * fall);
        }
        samples
    }
}

/// Sonie du passage le plus fort (fenêtres de 400 ms), en LUFS : filtre de pondération
/// « K » de la norme ITU-R BS.1770 (oreille plus sensible aux aigus, pas aux graves).
fn momentary_loudness(samples: &[[f32; 2]]) -> f32 {
    let filters = || {
        (
            Biquad::raw(
                [1.535_124_9, -2.691_696_2, 1.198_392_8],
                [-1.690_659_3, 0.732_480_8],
            ),
            Biquad::raw([1.0, -2.0, 1.0], [-1.990_047_5, 0.990_072_25]),
        )
    };
    let mut channels = [filters(), filters()];
    let energies: Vec<f64> = samples
        .iter()
        .map(|frame| {
            frame
                .iter()
                .zip(&mut channels)
                .map(|(&sample, (shelf, high))| {
                    let weighted = high.process(shelf.process(sample));
                    f64::from(weighted * weighted)
                })
                .sum()
        })
        .collect();
    let window = seconds(0.4);
    let mut sum = 0.0f64;
    let mut loudest = 0.0f64;
    for (index, energy) in energies.iter().enumerate() {
        sum += energy;
        if index >= window {
            sum -= energies[index - window];
        }
        loudest = loudest.max(sum / window as f64);
    }
    -0.691 + 10.0 * loudest.max(1e-12).log10() as f32
}

/// WAV 16 bits stéréo, avec un bruit de dither triangulaire (pas de grésillement dans
/// les fins de son très faibles).
fn wav(samples: &[[f32; 2]]) -> Vec<u8> {
    let data_len = (samples.len() * 4) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&2u16.to_le_bytes()); // stéréo
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    let mut noise = Noise(0xD1_7E_12);
    for frame in samples {
        for sample in frame {
            let dither = (noise.unit() - noise.unit()) * 0.5;
            let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX) + dither).round();
            let value = value.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Sound> {
        let mut sounds: Vec<Sound> = Alert::ALL.into_iter().map(Sound::Alert).collect();
        sounds.extend([
            Sound::Trash,
            Sound::Screenshot,
            Sound::Startup,
            Sound::Plugged,
            Sound::Unplugged,
        ]);
        sounds
    }

    #[test]
    fn sons_courts_et_sans_saturation() {
        for sound in all() {
            let samples = synthesize(sound);
            let length = samples.len() as f32 / RATE_F;
            let limit = if sound == Sound::Startup { 5.0 } else { 3.0 };
            assert!(length > 0.1 && length <= limit, "{sound:?} : {length} s");
            let peak = samples
                .iter()
                .fold(0.0f32, |max, [l, r]| max.max(l.abs()).max(r.abs()));
            assert!((0.2..=0.9).contains(&peak), "{sound:?} : crête {peak}");
            // Le son s'éteint de lui-même : la fin est presque silencieuse.
            let tail = samples[samples.len() - 100..]
                .iter()
                .fold(0.0f32, |max, [l, r]| max.max(l.abs()).max(r.abs()));
            assert!(tail < 0.002, "{sound:?} : fin {tail}");
            // Vraiment stéréo : les deux canaux diffèrent.
            assert!(
                samples.iter().any(|[l, r]| (l - r).abs() > 0.01),
                "{sound:?} : mono"
            );
        }
    }

    /// `FERRITE_SONS=dossier cargo test -p ferrite-system exporter -- --ignored` : écrit
    /// tous les sons pour les écouter.
    #[test]
    #[ignore = "écrit des fichiers"]
    fn exporter_les_sons() {
        let dir = env::var_os("FERRITE_SONS").expect("FERRITE_SONS : dossier de sortie");
        let paths = export_all(Path::new(&dir)).unwrap();
        assert_eq!(paths.len(), all().len());
    }

    #[test]
    fn noms_de_fichiers_distincts() {
        let mut names: Vec<String> = all().into_iter().map(Sound::file_name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all().len());
    }

    #[test]
    fn fichier_wav_valide() {
        let bytes = wav(&[[0.0, 0.0], [1.0, -1.0]]);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8);
        assert_eq!(bytes.len(), 44 + 8);
        assert_eq!(i16::from_le_bytes([bytes[48], bytes[49]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([bytes[50], bytes[51]]), -i16::MAX);
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
        assert_eq!(
            Settings::parse("son_alerte=cristal").alert_sound(),
            Some(Alert::Crystal)
        );
        assert_eq!(Settings::parse("son_alerte=??").alert_sound(), Some(Alert::Drop));
    }
}
