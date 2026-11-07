//! Jauge « Niveau d'entrée » de la page Son : niveau du micro par défaut, mesuré par
//! GStreamer (`pulsesrc ! level`, PipeWire répond par sa couche PulseAudio). Le micro n'est
//! ouvert que tant que la page Son est affichée.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
};

use gstreamer as gst;
use gstreamer::prelude::*;

/// Cases de la jauge (voir `rows::meter`).
pub const CELLS: usize = 16;
/// Niveau le plus bas montré, en décibels : en dessous, la jauge est vide.
const FLOOR_DB: f64 = -60.0;

#[derive(Debug)]
pub struct InputMeter {
    pipeline: gst::Pipeline,
    /// Cases allumées, mises à jour par le fil de lecture.
    level: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
}

impl InputMeter {
    /// Ouvre le micro par défaut ; `None` si GStreamer ou le son ne sont pas disponibles.
    pub fn start() -> Option<InputMeter> {
        gst::init().ok()?;
        let pipeline =
            gst::parse::launch("pulsesrc ! level interval=80000000 post-messages=true ! fakesink sync=false")
                .ok()?
                .downcast::<gst::Pipeline>()
                .ok()?;
        let bus = pipeline.bus()?;
        pipeline.set_state(gst::State::Playing).ok()?;
        let level = Arc::new(AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, stopped) = (level.clone(), stop.clone());
        thread::Builder::new()
            .name("niveau-micro".into())
            .spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    let Some(message) = bus.timed_pop(gst::ClockTime::from_mseconds(200)) else {
                        continue;
                    };
                    match message.view() {
                        gst::MessageView::Element(element) => {
                            // Un nombre par canal, en décibels.
                            if let Some(rms) = element
                                .structure()
                                .filter(|structure| structure.name() == "level")
                                .and_then(|structure| structure.get::<gst::glib::ValueArray>("rms").ok())
                            {
                                let decibels: Vec<f64> =
                                    rms.iter().filter_map(|value| value.get::<f64>().ok()).collect();
                                shared.store(cells(&decibels) as u32, Ordering::Relaxed);
                            }
                        }
                        gst::MessageView::Error(_) | gst::MessageView::Eos(_) => {
                            shared.store(0, Ordering::Relaxed);
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .ok()?;
        Some(InputMeter {
            pipeline,
            level,
            stop,
        })
    }

    /// Cases allumées (0 à [`CELLS`]).
    pub fn level(&self) -> usize {
        self.level.load(Ordering::Relaxed) as usize
    }
}

impl Drop for InputMeter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Le micro est relâché tout de suite (le fil s'arrête de lui-même).
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// Le canal le plus fort, de [`FLOOR_DB`] (aucune case) à 0 dB (toutes).
fn cells(decibels: &[f64]) -> usize {
    let loudest = decibels.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !loudest.is_finite() {
        return 0;
    }
    let fraction = ((loudest - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
    (fraction * CELLS as f64).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_en_cases() {
        assert_eq!(cells(&[]), 0);
        assert_eq!(cells(&[f64::NEG_INFINITY]), 0);
        assert_eq!(cells(&[-80.0]), 0);
        assert_eq!(cells(&[-60.0]), 0);
        assert_eq!(cells(&[-30.0, -50.0]), 8);
        assert_eq!(cells(&[0.0]), CELLS);
        assert_eq!(cells(&[3.0]), CELLS);
    }
}
