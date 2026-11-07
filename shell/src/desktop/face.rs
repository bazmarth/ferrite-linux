//! Déverrouillage par visage : sur l'écran verrouillé, `ferrite-visage verifier` regarde
//! la caméra infrarouge quelques secondes ; reconnu, l'écran s'ouvre comme après le mot de
//! passe. Pendant ce temps un anneau tourne autour de l'avatar ; en cas d'échec, un bouton
//! « Réessayer » (ou un clic sur l'avatar) relance la caméra.
//!
//! Garde-fous (`docs/DECISIONS.md`, 2026-11-06) : réglage `visage_deverrouillage = oui`,
//! visage enregistré, et mot de passe déjà donné une fois depuis l'ouverture de la session
//! (il ouvre aussi le trousseau). Jamais pour les droits administrateur. La vérification
//! ne part que sur un geste (touche, clic, réveil), jamais au moment où l'écran se
//! verrouille : sinon il s'ouvrirait aussitôt devant la personne qui vient de le fermer.

use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

use crate::{
    desktop::{
        lock::Event,
        panels::{LockFace, LockMessage},
    },
    state::Ferrite,
};

/// Écart minimal entre deux essais lancés tout seuls (touche, réveil), pour ne pas garder
/// la caméra allumée sans fin. « Réessayer » passe outre.
const RETRY: Duration = Duration::from_secs(6);

/// Dossier des empreintes, écrit par root seul (voir `services/visage/src/store.rs`).
const PRINTS_DIR: &str = "/var/lib/ferrite/visage";

/// Réponse de `ferrite-visage verifier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceResult {
    Recognized,
    NotRecognized,
    /// Aucun visage enregistré.
    Absent,
    /// Caméra infrarouge ou modèles indisponibles.
    Unavailable,
}

impl FaceResult {
    fn from_code(code: Option<i32>) -> Self {
        match code {
            Some(0) => Self::Recognized,
            Some(1) => Self::NotRecognized,
            Some(2) => Self::Absent,
            _ => Self::Unavailable,
        }
    }
}

/// Programme `ferrite-visage` : à côté de `ferrite-shell`, sinon dans le `PATH`.
fn program() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("ferrite-visage"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("ferrite-visage"))
}

/// Déverrouillage par visage activé dans Paramètres.
fn enabled() -> bool {
    ferrite_system::config::Settings::load()
        .get("visage_deverrouillage")
        .is_some_and(|value| value == "oui")
}

/// Un visage est enregistré pour ce compte.
fn enrolled() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self")
        .map(|meta| std::path::Path::new(PRINTS_DIR).join(meta.uid().to_string()))
        .is_ok_and(|path| path.is_file())
}

impl Ferrite {
    /// Le visage peut-il servir ? (réglage, visage enregistré, mot de passe déjà donné.)
    /// Lu au verrouillage et à chaque essai, jamais à chaque image.
    pub fn refresh_face_ready(&mut self) {
        let ready = self.face_since_password && enabled() && enrolled();
        if let Some(lock) = self.lock.as_mut() {
            lock.face_ready = ready;
            if !ready {
                lock.face = LockFace::Idle;
            }
        }
    }

    /// Un geste sur l'écran verrouillé : essaie de reconnaître le visage, si tout le permet.
    pub fn try_face_unlock(&mut self) {
        if self.face_tried.is_some_and(|tried| tried.elapsed() < RETRY) {
            return;
        }
        self.start_face_unlock();
    }

    /// « Réessayer » ou clic sur l'avatar : relance la caméra tout de suite.
    pub fn lock_face_retry(&mut self) {
        self.start_face_unlock();
    }

    fn start_face_unlock(&mut self) {
        let Some(events) = self.lock_events.clone() else {
            return;
        };
        if self.face_running {
            return;
        }
        self.refresh_face_ready();
        let Some(lock) = self
            .lock
            .as_mut()
            .filter(|lock| lock.face_ready && !lock.checking && !lock.unlocking())
        else {
            return;
        };
        // L'anneau tourne autour de l'avatar, qui n'est visible qu'en saisie (au réveil,
        // la personne est devant l'écran : on y passe).
        lock.show_field();
        lock.face = LockFace::Scanning;
        lock.face_since = Instant::now();
        if lock.message != LockMessage::Hint && lock.password_length() == 0 {
            lock.message = LockMessage::Hint;
        }
        self.face_running = true;
        self.face_tried = Some(Instant::now());
        let program = program();
        let spawned = std::thread::Builder::new()
            .name("visage".to_owned())
            .spawn(move || {
                let code = Command::new(&program)
                    .arg("verifier")
                    .status()
                    .ok()
                    .and_then(|status| status.code());
                let _ = events.send(Event::Face(FaceResult::from_code(code)));
            });
        if spawned.is_err() {
            self.face_running = false;
            if let Some(lock) = self.lock.as_mut() {
                lock.face = LockFace::Unavailable;
            }
        }
        self.request_redraw();
    }

    /// Réponse de `ferrite-visage verifier`.
    pub fn on_face_checked(&mut self, result: FaceResult) {
        self.face_running = false;
        let Some(lock) = self.lock.as_mut() else {
            return;
        };
        if lock.unlocking() {
            return;
        }
        lock.face = match result {
            // Le mot de passe est déjà en cours de vérification : il décide.
            FaceResult::Recognized if lock.checking => LockFace::Idle,
            FaceResult::Recognized => {
                tracing::info!("visage reconnu : écran déverrouillé");
                lock.accept();
                LockFace::Recognized
            }
            FaceResult::NotRecognized => LockFace::Failed,
            FaceResult::Absent => {
                lock.face_ready = false;
                LockFace::Idle
            }
            FaceResult::Unavailable => LockFace::Unavailable,
        };
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_de_ferrite_visage() {
        assert_eq!(FaceResult::from_code(Some(0)), FaceResult::Recognized);
        assert_eq!(FaceResult::from_code(Some(1)), FaceResult::NotRecognized);
        assert_eq!(FaceResult::from_code(Some(2)), FaceResult::Absent);
        assert_eq!(FaceResult::from_code(Some(3)), FaceResult::Unavailable);
        assert_eq!(FaceResult::from_code(None), FaceResult::Unavailable);
    }
}
