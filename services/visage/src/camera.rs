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

/// Caméra infrarouge choisie : son nœud `/dev/videoN` et son emplacement matériel.
pub struct Found {
    pub device: String,
    /// Emplacement stable dans `/sys/devices` (port USB interne), retenu à
    /// l'enregistrement : une autre caméra branchée plus tard n'est jamais utilisée.
    pub place: String,
}

/// Caméra infrarouge à utiliser. Avec `wanted` (vérification), seule la caméra de cet
/// emplacement convient. Sans (enregistrement), une caméra intégrée à l'ordinateur passe
/// avant une caméra branchée.
pub fn find_infrared(wanted: Option<&str>) -> Option<Found> {
    gst::init().ok()?;
    let mut found: Vec<(bool, Found)> = video_nodes()
        .into_iter()
        .filter_map(|(number, device)| {
            let place = place_of(number)?;
            Some((is_fixed(&place), Found { device, place }))
        })
        .filter(|(_, found)| wanted.is_none_or(|wanted| found.place == wanted))
        .filter(|(_, found)| gives_gray(&found.device))
        .collect();
    // Tri stable : l'ordre des numéros est gardé, une caméra intégrée passe devant.
    found.sort_by_key(|(fixed, _)| !fixed);
    found.into_iter().next().map(|(_, found)| found)
}

/// Nœuds `/dev/videoN`, rangés par numéro (`video10` après `video2`).
fn video_nodes() -> Vec<(u32, String)> {
    let mut nodes: Vec<(u32, String)> = std::fs::read_dir("/dev")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let number = name.strip_prefix("video")?.parse().ok()?;
            Some((number, format!("/dev/{name}")))
        })
        .collect();
    nodes.sort();
    nodes
}

/// Emplacement matériel de `/dev/videoN` (lien `device` de sysfs, résolu).
fn place_of(number: u32) -> Option<String> {
    let link = format!("/sys/class/video4linux/video{number}/device");
    std::fs::canonicalize(link)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

/// Le port USB de cette caméra est-il marqué « fixed » (intégré à l'ordinateur) ?
fn is_fixed(place: &str) -> bool {
    // `place` est l'interface (…/3-6/3-6:1.2) ; `removable` est sur l'appareil (…/3-6).
    std::path::Path::new(place)
        .parent()
        .and_then(|usb| std::fs::read_to_string(usb.join("removable")).ok())
        .is_some_and(|value| value.trim() == "fixed")
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
