//! Caméra infrarouge : la première caméra qui donne des images en niveaux de gris (GRAY8),
//! format des capteurs infrarouges de type Windows Hello. Une caméra couleur n'est jamais
//! utilisée : une photo affichée sur un écran n'émet pas d'infrarouge.

use std::time::Duration;

use gstreamer::{self as gst, prelude::*};
use gstreamer_app::AppSink;

use crate::geometry::Gray;

/// Luminosité moyenne en dessous de laquelle la lampe n'est pas encore allumée.
const DARK: f32 = 12.0;

pub struct Camera {
    pipeline: gst::Pipeline,
    sink: AppSink,
}

impl Drop for Camera {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// Périphérique `/dev/videoN` qui sait donner du GRAY8.
pub fn find_infrared() -> Option<String> {
    gst::init().ok()?;
    let mut devices: Vec<String> = std::fs::read_dir("/dev")
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().to_string_lossy().into_owned())
        .filter(|path| path.starts_with("/dev/video"))
        .collect();
    devices.sort();
    devices.into_iter().find(|device| gives_gray(device))
}

fn gives_gray(device: &str) -> bool {
    let Ok(source) = gst::ElementFactory::make("v4l2src")
        .property("device", device)
        .build()
    else {
        return false;
    };
    if source.set_state(gst::State::Ready).is_err() {
        let _ = source.set_state(gst::State::Null);
        return false;
    }
    let gray = source
        .static_pad("src")
        .map(|pad| pad.query_caps(None))
        .is_some_and(|caps| {
            caps.iter()
                .any(|structure| structure.get::<&str>("format").ok() == Some("GRAY8"))
        });
    let _ = source.set_state(gst::State::Null);
    gray
}

impl Camera {
    pub fn open(device: &str) -> Result<Self, String> {
        gst::init().map_err(|err| err.to_string())?;
        let description = format!(
            "v4l2src device={device} ! video/x-raw,format=GRAY8 ! videoconvert ! \
             video/x-raw,format=GRAY8 ! appsink name=sortie max-buffers=2 drop=true sync=false"
        );
        let pipeline = gst::parse::launch(&description)
            .map_err(|err| err.to_string())?
            .downcast::<gst::Pipeline>()
            .map_err(|_| "pipeline inattendu".to_owned())?;
        let sink = pipeline
            .by_name("sortie")
            .and_then(|element| element.downcast::<AppSink>().ok())
            .ok_or("sortie introuvable")?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|err| format!("caméra infrarouge indisponible : {err}"))?;
        Ok(Self { pipeline, sink })
    }

    /// Image suivante, en attendant au plus `wait`.
    pub fn frame(&self, wait: Duration) -> Option<Gray> {
        let sample = self
            .sink
            .try_pull_sample(gst::ClockTime::from_nseconds(wait.as_nanos() as u64))?;
        let caps = sample.caps()?;
        let structure = caps.structure(0)?;
        let width = structure.get::<i32>("width").ok()? as usize;
        let height = structure.get::<i32>("height").ok()? as usize;
        let buffer = sample.buffer()?;
        let map = buffer.map_readable().ok()?;
        // Les lignes peuvent être alignées sur 4 octets : on ne garde que les pixels.
        let stride = map.len() / height.max(1);
        let mut pixels = Vec::with_capacity(width * height);
        for row in 0..height {
            pixels.extend_from_slice(map.get(row * stride..row * stride + width)?);
        }
        Some(Gray {
            width,
            height,
            pixels,
        })
    }

    /// Image suivante éclairée par la lampe infrarouge (les premières sont noires).
    pub fn lit_frame(&self, wait: Duration) -> Option<Gray> {
        self.frame(wait).filter(|frame| frame.brightness() >= DARK)
    }
}
