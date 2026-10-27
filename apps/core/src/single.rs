//! Une seule fenêtre par application (sauf celles choisies dans Paramètres) : la première
//! instance écoute sur `$XDG_RUNTIME_DIR/ferrite-<application>.sock` ; une deuxième lui
//! transmet le fichier à ouvrir, puis s'arrête. La première ouvre le fichier et revient au
//! premier plan.

use std::{
    env, fs,
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
    thread,
};

use eframe::egui;

/// Demandes des lancements suivants : un fichier à ouvrir, ou rien (revenir devant).
pub struct Requests {
    pub receiver: Receiver<Option<PathBuf>>,
    /// Contexte de l'interface, pour la réveiller quand une demande arrive.
    pub context: Arc<Mutex<Option<egui::Context>>>,
}

pub enum Start {
    /// Cette instance est la première : elle reçoit les demandes des suivantes.
    Primary(Requests),
    /// Une instance tournait déjà : elle a reçu la demande.
    Forwarded,
}

fn socket_path(app_id: &str) -> PathBuf {
    let dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    dir.join(format!("{app_id}.sock"))
}

/// Prend la place de première instance, ou transmet `file` à celle qui tourne.
pub fn claim(app_id: &str, file: Option<&Path>) -> Start {
    let path = socket_path(app_id);
    if let Ok(mut stream) = UnixStream::connect(&path) {
        let file = file
            .map(|file| fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf()))
            .map(|file| file.to_string_lossy().into_owned())
            .unwrap_or_default();
        if stream.write_all(file.as_bytes()).is_ok() {
            return Start::Forwarded;
        }
    }
    // Personne n'écoute : un fichier laissé par une instance arrêtée est remplacé.
    let _ = fs::remove_file(&path);
    let (sender, receiver) = mpsc::channel();
    let context: Arc<Mutex<Option<egui::Context>>> = Arc::default();
    if let Ok(listener) = UnixListener::bind(&path) {
        let waker = context.clone();
        thread::spawn(move || {
            for mut stream in listener.incoming().filter_map(Result::ok) {
                let mut text = String::new();
                let _ = stream.read_to_string(&mut text);
                let file = (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()));
                if sender.send(file).is_err() {
                    return;
                }
                if let Some(context) = waker.lock().ok().and_then(|context| context.clone()) {
                    context.request_repaint();
                }
            }
        });
    }
    Start::Primary(Requests { receiver, context })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_deuxieme_instance_transmet_son_fichier() {
        let id = format!("ferrite-test-{}", std::process::id());
        let Start::Primary(requests) = claim(&id, None) else {
            panic!("première instance attendue");
        };
        assert!(matches!(
            claim(&id, Some(Path::new("/tmp/note.txt"))),
            Start::Forwarded
        ));
        let received = requests
            .receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert_eq!(received, Some(PathBuf::from("/tmp/note.txt")));
        let _ = fs::remove_file(socket_path(&id));
    }
}
