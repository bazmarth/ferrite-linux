//! Lecture : décodage par symphonia dans un fil à part, son envoyé à PipeWire par `pw-cat`
//! (échantillons flottants entrelacés sur son entrée standard).
//!
//! En pause, `pw-cat` est arrêté pour que le son cesse tout de suite ; la lecture reprend
//! à la position entendue, un peu avant la dernière écrite (ce qui attendait encore dans
//! le tube et dans PipeWire est rejoué).

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread,
};

use eframe::egui;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{Decoder, DecoderOptions},
    errors::Error as DecodeError,
    formats::{FormatReader, SeekMode, SeekTo},
    units::Time,
};

use super::library;

/// Son en attente entre ce qui est écrit et ce qu'on entend (tube et latence de `pw-cat`).
const BUFFERED: f64 = 0.15;

#[derive(Debug)]
pub enum EngineCommand {
    /// Ouvre un morceau et le lit (ou le prépare en pause).
    Open {
        path: PathBuf,
        play: bool,
    },
    Play,
    Pause,
    Seek(f64),
    Volume(f32),
}

/// État lu par l'interface à chaque image.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub path: Option<PathBuf>,
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
    /// Incrémenté à chaque morceau lu jusqu'au bout.
    pub finished: u64,
    pub error: Option<String>,
}

pub struct Engine {
    commands: Sender<EngineCommand>,
    status: Arc<Mutex<Status>>,
}

impl Engine {
    pub fn start(ctx: egui::Context) -> Self {
        let (commands, receiver) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = status.clone();
        let spawned = thread::Builder::new()
            .name("lecture".into())
            .spawn(move || run(&receiver, &shared, &ctx));
        if let Err(error) = spawned {
            eprintln!("lecture impossible : {error}");
        }
        Self { commands, status }
    }

    pub fn send(&self, command: EngineCommand) {
        let _ = self.commands.send(command);
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
    }
}

/// Morceau ouvert.
struct Source {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    rate: u32,
    channels: usize,
}

impl Source {
    fn open(path: &Path) -> Result<(Self, f64), String> {
        let probed = library::probe(path)?;
        let format = probed.format;
        let track = format.default_track().ok_or("aucune piste audio")?.clone();
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|error| format!("codec non pris en charge : {error}"))?;
        let params = &track.codec_params;
        let rate = params.sample_rate.ok_or("fréquence inconnue")?;
        let channels = params.channels.map_or(2, |channels| channels.count());
        let duration = match (params.n_frames, params.time_base) {
            (Some(frames), Some(base)) => {
                let time = base.calc_time(frames);
                time.seconds as f64 + time.frac
            }
            (Some(frames), None) => frames as f64 / rate as f64,
            _ => 0.0,
        };
        Ok((
            Self {
                format,
                decoder,
                track_id: track.id,
                rate,
                channels,
            },
            duration,
        ))
    }

    fn seek(&mut self, seconds: f64) -> f64 {
        let target = SeekTo::Time {
            time: Time::from(seconds.max(0.0)),
            track_id: Some(self.track_id),
        };
        match self.format.seek(SeekMode::Accurate, target) {
            Ok(_) => {
                self.decoder.reset();
                seconds.max(0.0)
            }
            Err(_) => 0.0,
        }
    }

    /// Échantillons entrelacés du paquet suivant ; `None` à la fin du morceau.
    fn next(&mut self) -> Option<Vec<f32>> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(DecodeError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(_) => return None,
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                    buffer.copy_interleaved_ref(decoded);
                    return Some(buffer.samples().to_vec());
                }
                // Paquet abîmé : on passe au suivant.
                Err(DecodeError::DecodeError(_)) => continue,
                Err(_) => return None,
            }
        }
    }
}

/// Processus `pw-cat` qui joue le son.
struct Output {
    child: Child,
    stdin: ChildStdin,
    rate: u32,
    channels: usize,
}

impl Output {
    fn spawn(rate: u32, channels: usize) -> Result<Self, String> {
        let mut child = Command::new("pw-cat")
            .args(["--playback", "--raw", "--format", "f32", "--media-role", "Music"])
            .args(["--rate", &rate.to_string(), "--channels", &channels.to_string()])
            .args(["--properties", "application.name=Musique", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("PipeWire indisponible (pw-cat) : {error}"))?;
        let stdin = child.stdin.take().ok_or("sortie audio indisponible")?;
        Ok(Self {
            child,
            stdin,
            rate,
            channels,
        })
    }

    fn stop(mut self) {
        drop(self.stdin);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Arrête `pw-cat` s'il tourne.
fn stop(output: &mut Option<Output>) {
    if let Some(output) = output.take() {
        output.stop();
    }
}

fn run(commands: &Receiver<EngineCommand>, status: &Mutex<Status>, ctx: &egui::Context) {
    let mut source: Option<Source> = None;
    let mut output: Option<Output> = None;
    let mut playing = false;
    let mut volume = 1.0f32;
    // Secondes écrites depuis le début du morceau.
    let mut written = 0.0f64;
    let update = |change: &dyn Fn(&mut Status)| {
        if let Ok(mut status) = status.lock() {
            change(&mut status);
        }
        ctx.request_repaint();
    };
    loop {
        let command = if playing {
            match commands.try_recv() {
                Ok(command) => Some(command),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => break,
            }
        } else {
            match commands.recv() {
                Ok(command) => Some(command),
                Err(_) => break,
            }
        };
        match command {
            Some(EngineCommand::Open { path, play }) => match Source::open(&path) {
                Ok((opened, duration)) => {
                    // Même format : `pw-cat` continue, le morceau suivant s'enchaîne sans trou.
                    if output
                        .as_ref()
                        .is_some_and(|out| out.rate != opened.rate || out.channels != opened.channels)
                    {
                        stop(&mut output);
                    }
                    source = Some(opened);
                    written = 0.0;
                    playing = play;
                    update(&|status| {
                        status.path = Some(path.clone());
                        status.position = 0.0;
                        status.duration = duration;
                        status.playing = play;
                        status.error = None;
                    });
                }
                Err(error) => {
                    source = None;
                    playing = false;
                    update(&|status| {
                        status.path = Some(path.clone());
                        status.playing = false;
                        status.error = Some(error.clone());
                    });
                }
            },
            Some(EngineCommand::Play) if source.is_some() => {
                playing = true;
                update(&|status| status.playing = true);
            }
            Some(EngineCommand::Pause) => {
                if playing && let Some(source) = &mut source {
                    // On reprendra à ce qu'on entend, pas à ce qu'on a écrit.
                    written = source.seek((written - BUFFERED).max(0.0));
                }
                stop(&mut output);
                playing = false;
                let heard = written;
                update(&|status| {
                    status.playing = false;
                    status.position = heard;
                });
            }
            Some(EngineCommand::Seek(seconds)) => {
                if let Some(source) = &mut source {
                    stop(&mut output);
                    written = source.seek(seconds);
                    let at = written;
                    update(&|status| status.position = at);
                }
            }
            Some(EngineCommand::Volume(level)) => volume = level.clamp(0.0, 1.0),
            Some(EngineCommand::Play) => {}
            None => {}
        }
        if !playing {
            continue;
        }
        let Some(current) = &mut source else {
            playing = false;
            continue;
        };
        let Some(mut samples) = current.next() else {
            // Fin du morceau : l'interface choisit la suite (ou s'arrête).
            playing = false;
            update(&|status| {
                status.playing = false;
                status.finished += 1;
            });
            continue;
        };
        if output.is_none() {
            match Output::spawn(current.rate, current.channels) {
                Ok(spawned) => output = Some(spawned),
                Err(error) => {
                    playing = false;
                    update(&|status| {
                        status.playing = false;
                        status.error = Some(error.clone());
                    });
                    continue;
                }
            }
        }
        if volume < 1.0 {
            for sample in &mut samples {
                *sample *= volume;
            }
        }
        let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
        // L'écriture bloque quand le tube est plein : c'est elle qui cadence la lecture.
        if let Some(out) = &mut output
            && out.stdin.write_all(&bytes).is_err()
        {
            stop(&mut output);
        }
        written += samples.len() as f64 / (current.channels.max(1) as f64 * current.rate as f64);
        let heard = (written - BUFFERED).max(0.0);
        if let Ok(mut status) = status.lock() {
            // Une image par dixième de seconde suffit pour la barre de progression.
            let repaint = (heard - status.position).abs() >= 0.1;
            status.position = heard;
            if repaint {
                ctx.request_repaint();
            }
        }
    }
    stop(&mut output);
}
