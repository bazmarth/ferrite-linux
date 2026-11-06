//! Déverrouillage par visage : sur l'écran verrouillé, `ferrite-visage verifier` regarde
//! la caméra infrarouge quelques secondes ; reconnu, l'écran s'ouvre comme après le mot de
//! passe.
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
    desktop::{lock::Event, panels::LockMessage},
    state::Ferrite,
};

/// Écart minimal entre deux essais, pour ne pas garder la caméra allumée sans fin.
const RETRY: Duration = Duration::from_secs(6);

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

impl Ferrite {
    /// Un geste sur l'écran verrouillé : essaie de reconnaître le visage, si tout le permet.
    pub fn try_face_unlock(&mut self) {
        let Some(events) = self.lock_events.clone() else {
            return;
        };
        if self.face_running
            || !self.face_since_password
            || self.face_tried.is_some_and(|tried| tried.elapsed() < RETRY)
        {
            return;
        }
        let Some(lock) = self
            .lock
            .as_mut()
            .filter(|lock| !lock.checking && !lock.unlocking())
        else {
            return;
        };
        if !enabled() {
            return;
        }
        lock.message = LockMessage::Face;
        self.face_running = true;
        self.face_tried = Some(Instant::now());
        let program = program();
        let spawned = std::thread::Builder::new()
            .name("visage".to_owned())
            .spawn(move || {
                let recognized = Command::new(&program)
                    .arg("verifier")
                    .status()
                    .is_ok_and(|status| status.success());
                let _ = events.send(Event::Face(recognized));
            });
        if spawned.is_err() {
            self.face_running = false;
        }
        self.request_redraw();
    }

    /// Réponse de `ferrite-visage verifier`.
    pub fn on_face_checked(&mut self, recognized: bool) {
        self.face_running = false;
        let Some(lock) = self.lock.as_mut() else {
            return;
        };
        if lock.message == LockMessage::Face {
            lock.message = LockMessage::Hint;
        }
        if recognized && !lock.checking {
            tracing::info!("visage reconnu : écran déverrouillé");
            lock.accept();
        }
        self.request_redraw();
    }
}
