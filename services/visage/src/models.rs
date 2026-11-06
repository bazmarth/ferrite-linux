//! Les deux réseaux : YuNet trouve le visage, SFace en calcule l'empreinte (128 nombres).
//! Modèles d'OpenCV (licences MIT et Apache 2.0), exécutés en Rust pur par `tract`.

use std::path::{Path, PathBuf};

use tract_onnx::prelude::*;

use crate::geometry::{self, ALIGNED, Face, Gray};

/// Côté de l'entrée carrée de YuNet.
const DETECT_SIZE: usize = 640;
pub const DETECTOR_FILE: &str = "face_detection_yunet_2023mar.onnx";
pub const RECOGNIZER_FILE: &str = "face_recognition_sface_2021dec.onnx";

type Runnable = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

pub struct Models {
    detector: Runnable,
    recognizer: Runnable,
}

/// Dossier des modèles : `FERRITE_VISAGE_MODELES`, à côté du programme (`visage/`),
/// l'installation (`/usr/share/ferrite/visage`), ou `assets/visage` du dépôt.
pub fn models_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("FERRITE_VISAGE_MODELES") {
        candidates.push(dir.into());
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        candidates.push(exe_dir.join("visage"));
        // target/debug ou target/release du dépôt.
        candidates.push(exe_dir.join("../../assets/visage"));
    }
    candidates.push("/usr/share/ferrite/visage".into());
    candidates
        .into_iter()
        .find(|dir| dir.join(DETECTOR_FILE).is_file() && dir.join(RECOGNIZER_FILE).is_file())
}

fn load(path: &Path, shape: [usize; 4]) -> TractResult<Runnable> {
    tract_onnx::onnx()
        .model_for_path(path)?
        .with_input_fact(0, f32::fact(shape).into())?
        .into_optimized()?
        .into_runnable()
}

/// Image grise posée dans un tenseur `1 × 3 × size × size` (les trois couleurs
/// identiques), en haut à gauche, le reste noir.
fn tensor(image: &Gray, size: usize) -> Tensor {
    let mut array = tract_ndarray::Array4::<f32>::zeros((1, 3, size, size));
    for y in 0..image.height.min(size) {
        for x in 0..image.width.min(size) {
            let value = f32::from(image.pixels[y * image.width + x]);
            for channel in 0..3 {
                array[[0, channel, y, x]] = value;
            }
        }
    }
    array.into()
}

impl Models {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let detector = load(&dir.join(DETECTOR_FILE), [1, 3, DETECT_SIZE, DETECT_SIZE])
            .map_err(|err| format!("modèle de détection illisible : {err}"))?;
        let recognizer = load(&dir.join(RECOGNIZER_FILE), [1, 3, ALIGNED, ALIGNED])
            .map_err(|err| format!("modèle de reconnaissance illisible : {err}"))?;
        Ok(Self { detector, recognizer })
    }

    /// Visage le plus sûr de l'image (640 × 360 de la caméra, posée sans mise à l'échelle).
    pub fn detect(&self, image: &Gray) -> Option<Face> {
        let outputs = self.detector.run(tvec!(tensor(image, DETECT_SIZE).into())).ok()?;
        let slices: Vec<&[f32]> = outputs
            .iter()
            .filter_map(|output| output.as_slice::<f32>().ok())
            .collect();
        geometry::best_face(&slices, DETECT_SIZE)
    }

    /// Empreinte normalisée du visage aligné.
    pub fn embed(&self, image: &Gray, face: &Face) -> Option<Vec<f32>> {
        let aligned = geometry::align(image, face);
        let outputs = self
            .recognizer
            .run(tvec!(tensor(&aligned, ALIGNED).into()))
            .ok()?;
        let values = outputs.first()?.as_slice::<f32>().ok()?.to_vec();
        Some(geometry::normalized(values))
    }
}
