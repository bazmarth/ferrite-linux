//! Enregistrement vidéo de l'écran, d'une zone ou d'une fenêtre : `wf-recorder` lit les
//! images par le protocole screencopy (voir `protocols::screencopy`) et les encode en MP4
//! dans le dossier Vidéos. On l'arrête par SIGINT, qui lui fait finir proprement le fichier.

use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use smithay::utils::{Logical, Rectangle};

use crate::desktop::notifications::{Event, file_notice, local_notice};

/// Enregistrement en cours.
#[derive(Debug)]
pub struct Recording {
    child: Child,
    path: PathBuf,
    started: Instant,
}

impl Recording {
    /// Démarre `wf-recorder` sur `region` (coordonnées logiques de l'écran), ou tout l'écran.
    pub fn start(
        region: Option<Rectangle<i32, Logical>>,
        environment: &[(String, String)],
    ) -> Result<Self, String> {
        let dir = videos_dir();
        std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        let stamp = crate::desktop::panels::file_timestamp(std::time::SystemTime::now());
        let path = free_path(&dir, &stamp);
        let mut command = Command::new("wf-recorder");
        command.arg("-f").arg(&path);
        if let Some(region) = region {
            command.arg("-g").arg(geometry(region));
        }
        let child = command
            .envs(environment.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => {
                    "wf-recorder n’est pas installé (sudo apt install wf-recorder)".to_owned()
                }
                _ => format!("wf-recorder : {err}"),
            })?;
        tracing::info!("enregistrement vidéo : {}", path.display());
        Ok(Self {
            child,
            path,
            started: Instant::now(),
        })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// `wf-recorder` s'est arrêté seul (erreur, écran débranché…).
    pub fn ended(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }

    /// Arrête l'enregistrement ; la notification part quand le fichier est fermé.
    pub fn stop(mut self, events: Option<smithay::reexports::calloop::channel::Sender<Event>>) {
        let interrupted = self.ended();
        if !interrupted {
            let pid = self.child.id().to_string();
            if let Err(error) = Command::new("kill").args(["-INT", &pid]).status() {
                tracing::warn!(%error, "arrêt de wf-recorder impossible ; arrêt forcé");
                let _ = self.child.kill();
            }
        }
        std::thread::spawn(move || {
            let _ = self.child.wait();
            let name = self
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let saved = std::fs::metadata(&self.path).is_ok_and(|meta| meta.len() > 0);
            let notice = match (saved, interrupted) {
                (true, false) => file_notice("Enregistrement terminé", &name, self.path.clone()),
                (true, true) => file_notice("Enregistrement interrompu", &name, self.path.clone()),
                (false, _) => local_notice("Enregistrement impossible", "wf-recorder n’a rien enregistré."),
            };
            if let Some(events) = events {
                let _ = events.send(Event::Notify(notice));
            }
        });
    }
}

/// Zone au format de `wf-recorder` : « x,y lxh ».
fn geometry(region: Rectangle<i32, Logical>) -> String {
    format!(
        "{},{} {}x{}",
        region.loc.x, region.loc.y, region.size.w, region.size.h
    )
}

/// Durée affichée pendant l'enregistrement : « 0:07 », « 12:30 ».
pub fn elapsed_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Dossier Vidéos de l'utilisateur (`XDG_VIDEOS_DIR`, sinon `~/Vidéos`).
pub fn videos_dir() -> PathBuf {
    crate::render::capture::user_dir("XDG_VIDEOS_DIR", "Vidéos")
}

fn free_path(dir: &Path, stamp: &str) -> PathBuf {
    let first = dir.join(format!("Enregistrement du {stamp}.mp4"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|number| dir.join(format!("Enregistrement du {stamp} ({number}).mp4")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_et_duree() {
        let region = Rectangle::new((10, 20).into(), (300, 200).into());
        assert_eq!(geometry(region), "10,20 300x200");
        assert_eq!(elapsed_label(Duration::from_secs(7)), "0:07");
        assert_eq!(elapsed_label(Duration::from_secs(750)), "12:30");
    }
}
