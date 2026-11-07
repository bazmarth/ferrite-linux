//! `ferrite-visage` : déverrouillage par visage avec la caméra infrarouge.
//!
//! ```text
//! pkexec ferrite-visage enregistrer   enregistre le visage (affiche « progression N/M »)
//! pkexec ferrite-visage effacer       oublie le visage enregistré
//! ferrite-visage verifier      0 : reconnu ; 1 : pas reconnu ; 2 : rien d'enregistré ;
//!                              3 : caméra ou modèles indisponibles
//! ferrite-visage etat          « enregistre » ou « absent »
//! ferrite-visage essai         5 s de détection, sans rien garder (réglage, dépannage)
//! ```
//!
//! Seules des empreintes (128 nombres par image) sont gardées, jamais d'image, dans
//! `/var/lib/ferrite/visage/<uid>` : l'utilisateur peut les lire, seul l'administrateur
//! les écrire (`enregistrer` et `effacer` demandent son mot de passe par `pkexec`). Le
//! compositeur lance `verifier` sur l'écran verrouillé ; il ne sert jamais aux droits
//! administrateur (voir `docs/DECISIONS.md`).

mod camera;
mod geometry;
mod models;
mod store;

use std::{
    process::ExitCode,
    time::{Duration, Instant},
};

/// Images retenues pour l'enregistrement, et confiance minimale du visage trouvé.
const ENROLL_FRAMES: usize = 8;
const FACE_SCORE: f32 = 0.75;
/// Ressemblance minimale (cosinus) avec une empreinte enregistrée, et nombre d'images qui
/// doivent l'atteindre pendant une vérification.
const MATCH: f32 = 0.6;
const MATCHES_NEEDED: usize = 2;
const VERIFY_FOR: Duration = Duration::from_secs(4);
const ENROLL_FOR: Duration = Duration::from_secs(25);
const FRAME_WAIT: Duration = Duration::from_millis(500);

fn main() -> ExitCode {
    let command = std::env::args().nth(1).unwrap_or_default();
    let result = match command.as_str() {
        "enregistrer" => enroll(),
        "verifier" => verify(),
        "effacer" => store::target_uid()
            .and_then(|uid| store::forget(std::path::Path::new(store::DIR), uid))
            .map(|()| ExitCode::SUCCESS),
        "essai" => trial(),
        "etat" => {
            println!("{}", if store::exists() { "enregistre" } else { "absent" });
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            eprintln!("Utilisation : ferrite-visage <enregistrer|verifier|effacer|etat|essai>");
            Ok(ExitCode::from(2))
        }
    };
    result.unwrap_or_else(|error| {
        eprintln!("ferrite-visage : {error}");
        ExitCode::from(3)
    })
}

fn open() -> Result<(camera::Camera, models::Models), String> {
    let dir = models::models_dir().ok_or("modèles introuvables (tools/visage-modeles.sh)")?;
    let device = camera::find_infrared().ok_or("aucune caméra infrarouge")?;
    let models = models::Models::load(&dir)?;
    Ok((camera::Camera::open(&device)?, models))
}

fn enroll() -> Result<ExitCode, String> {
    // Avant d'allumer la caméra : sans droits administrateur, rien ne sera gardé.
    let uid = store::target_uid()?;
    let (camera, models) = open()?;
    let started = Instant::now();
    let mut prints = Vec::new();
    println!("progression 0/{ENROLL_FRAMES}");
    while prints.len() < ENROLL_FRAMES && started.elapsed() < ENROLL_FOR {
        let Some(frame) = camera.lit_frame(FRAME_WAIT) else {
            continue;
        };
        let Some(face) = models.detect(&frame).filter(|face| face.score >= FACE_SCORE) else {
            println!("visage introuvable");
            continue;
        };
        if let Some(print) = models.embed(&frame, &face) {
            prints.push(print);
            println!("progression {}/{ENROLL_FRAMES}", prints.len());
        }
    }
    if prints.len() < ENROLL_FRAMES {
        println!("echec");
        return Ok(ExitCode::from(1));
    }
    store::save(std::path::Path::new(store::DIR), uid, &prints)?;
    println!("termine");
    Ok(ExitCode::SUCCESS)
}

/// Détection seule pendant 5 s : luminosité, visage trouvé et sa confiance, temps de
/// calcul. Rien n'est gardé.
fn trial() -> Result<ExitCode, String> {
    let (camera, models) = open()?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        let Some(frame) = camera.frame(FRAME_WAIT) else {
            continue;
        };
        let at = Instant::now();
        let face = models.detect(&frame);
        let embedded = face
            .filter(|face| face.score >= FACE_SCORE)
            .and_then(|face| models.embed(&frame, &face))
            .is_some();
        println!(
            "lumière {:5.1}  visage {:.2}  empreinte {}  {:?}",
            frame.brightness(),
            face.map_or(0.0, |face| face.score),
            if embedded { "oui" } else { "non" },
            at.elapsed()
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn verify() -> Result<ExitCode, String> {
    let known = store::load()?;
    if known.is_empty() {
        return Ok(ExitCode::from(2));
    }
    let (camera, models) = open()?;
    let started = Instant::now();
    let (mut matches, mut best) = (0, f32::MIN);
    while started.elapsed() < VERIFY_FOR {
        let Some(frame) = camera.lit_frame(FRAME_WAIT) else {
            continue;
        };
        let Some(face) = models.detect(&frame).filter(|face| face.score >= FACE_SCORE) else {
            continue;
        };
        let Some(print) = models.embed(&frame, &face) else {
            continue;
        };
        let score = known
            .iter()
            .map(|other| geometry::similarity_of(&print, other))
            .fold(f32::MIN, f32::max);
        best = best.max(score);
        if score >= MATCH {
            matches += 1;
            if matches >= MATCHES_NEEDED {
                eprintln!("reconnu (ressemblance {score:.2})");
                return Ok(ExitCode::SUCCESS);
            }
        }
    }
    eprintln!("pas reconnu (meilleure ressemblance {best:.2})");
    Ok(ExitCode::from(1))
}
