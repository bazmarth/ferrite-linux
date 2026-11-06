//! Fichiers glissés depuis Fichiers (voir `ferrite_system::drag`).
//!
//! Fichiers annonce les fichiers au début d'un glisser. Tant que le pointeur reste dans sa
//! fenêtre, il gère lui-même le dépôt ; dès qu'il en sort ou passe sur le Dock,
//! ferrite-shell reprend le glisser comme un glisser-déposer Wayland :
//! - une application qui accepte les fichiers (`text/uri-list`) les reçoit ;
//! - lâchés sur une icône du Dock, ils s'ouvrent dans cette application (la Corbeille les
//!   met à la Corbeille) ; une icône survolée un instant fait venir son application au
//!   premier plan, pour déposer dans sa fenêtre ;
//! - lâchés sur une fenêtre de Fichiers, ils y sont déplacés (copiés avec Ctrl ou Alt) ;
//! - lâchés sur la fenêtre d'une autre application Ferrite, ils s'y ouvrent.

use std::{
    fs,
    io::{Read, Write},
    os::{fd::OwnedFd, unix::net::UnixListener},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use ferrite_system::drag::{self, Deposit, Drag};
use smithay::reexports::wayland_server::Resource;
use smithay::{
    desktop::Window,
    input::Seat,
    reexports::{
        calloop::{
            LoopHandle, channel,
            timer::{TimeoutAction, Timer},
        },
        wayland_server::protocol::wl_data_device_manager::DndAction,
    },
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::selection::data_device::{ServerDndGrabHandler, SourceMetadata, start_dnd},
};
use tracing::{info, warn};

use crate::{
    desktop::apps::{App, AppKey},
    state::Ferrite,
};

/// Attente sur une icône du Dock avant que son application passe au premier plan.
const SPRING: Duration = ferrite_ui::tokens::motion::LONG;
/// Bande au-dessus du Dock qui le fait apparaître pendant un glisser de fichiers.
pub const DOCK_REACH: f64 = 60.0;

/// Glisser de fichiers annoncé par Fichiers.
#[derive(Debug)]
pub struct FileDrag {
    pub drag: Drag,
    /// Fenêtre d'où part le glisser.
    source: Option<Window>,
    /// Repris par ferrite-shell (glisser-déposer Wayland en cours).
    pub active: bool,
    /// Lâché : il ne reste qu'à fournir les fichiers à l'application qui les a acceptés.
    pub dropped: bool,
    /// Icône du Dock survolée, depuis quand, et si son application est déjà venue.
    hover: Option<(AppKey, Instant, bool)>,
    /// Dernière position du pointeur : au dépôt, le pointeur est verrouillé par Smithay.
    at: Point<f64, Logical>,
}

impl FileDrag {
    /// Glisser en cours, à montrer sous le pointeur.
    pub fn shown(&self) -> bool {
        self.active && !self.dropped
    }
}

/// Écoute les annonces de Fichiers sur la prise propre à l'affichage `display`.
pub fn listen(handle: &LoopHandle<'static, Ferrite>, display: &str) {
    let path = drag::socket_path(display);
    let _ = fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(err) => {
            warn!(%err, ?path, "glisser-déposer de fichiers indisponible");
            return;
        }
    };
    let (sender, channel) = channel::channel::<Drag>();
    let inserted = handle.insert_source(channel, |event, _, state| {
        if let channel::Event::Msg(drag) = event {
            state.offer_file_drag(drag);
            state.request_redraw();
        }
    });
    if let Err(err) = inserted {
        warn!(%err, "glisser-déposer de fichiers indisponible");
        return;
    }
    thread::spawn(move || {
        for stream in listener.incoming().filter_map(Result::ok) {
            let mut text = String::new();
            if stream.take(1 << 20).read_to_string(&mut text).is_ok()
                && let Some(drag) = Drag::decode(&text)
            {
                let _ = sender.send(drag);
            }
        }
    });
}

impl Ferrite {
    /// Fichiers annoncés : le glisser part de la fenêtre qui a le pointeur, bouton enfoncé.
    fn offer_file_drag(&mut self, drag: Drag) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        if !pointer.is_grabbed() {
            // Le bouton est déjà relâché : le glisser s'est terminé dans Fichiers.
            return;
        }
        let source = pointer
            .current_focus()
            .and_then(|surface| self.window_for_surface(&surface));
        self.file_drag = Some(FileDrag {
            drag,
            source,
            active: false,
            dropped: false,
            hover: None,
            at: pointer.current_location(),
        });
    }

    /// À chaque mouvement du pointeur : reprise du glisser hors de la fenêtre d'origine ou
    /// sur le Dock ; ensuite, une icône du Dock survolée fait venir son application.
    pub fn file_drag_motion(&mut self, location: Point<f64, Logical>) {
        let Some(file) = &mut self.file_drag else {
            return;
        };
        file.at = location;
        let Some(file) = &self.file_drag else {
            return;
        };
        if !file.active {
            let inside = file
                .source
                .as_ref()
                .and_then(|window| self.window_frame(window))
                .is_some_and(|frame| frame.to_f64().contains(location));
            if !inside || self.dock_item_at(location).is_some() {
                self.start_file_drag();
            }
            return;
        }
        if file.dropped {
            return;
        }
        let item = self
            .dock_item_at(location)
            .map(|(_, item)| item)
            .filter(|item| item.running && item.key != AppKey::Builtin(App::TRASH));
        let now = Instant::now();
        let spring = match (&self.file_drag.as_ref().and_then(|file| file.hover.clone()), item) {
            (Some((key, since, fired)), Some(item)) if *key == item.key => {
                let due = !fired && now.duration_since(*since) >= SPRING;
                Some((key.clone(), *since, *fired || due, due))
            }
            (_, Some(item)) => {
                // Le pointeur peut s'arrêter sur l'icône : l'attente est revue à son terme
                // même sans mouvement, à la dernière position connue.
                let timer = Timer::from_duration(SPRING);
                let _ = self.loop_handle.insert_source(timer, |_, _, state| {
                    if let Some(at) = state.file_drag.as_ref().map(|file| file.at) {
                        state.file_drag_motion(at);
                    }
                    TimeoutAction::Drop
                });
                Some((item.key, now, false, false))
            }
            (_, None) => None,
        };
        let due = spring.as_ref().is_some_and(|(.., due)| *due);
        if let Some(file) = &mut self.file_drag {
            file.hover = spring.map(|(key, since, fired, _)| (key, since, fired));
        }
        if due && let Some((key, ..)) = self.file_drag.as_ref().and_then(|file| file.hover.clone()) {
            self.bring_app_forward(&key);
        }
    }

    /// Reprend le glisser : glisser-déposer Wayland, avec ferrite-shell pour source.
    fn start_file_drag(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let Some(start) = pointer.grab_start_data() else {
            self.file_drag = None;
            return;
        };
        if let Some(file) = &mut self.file_drag {
            file.active = true;
            info!(fichiers = file.drag.paths.len(), "glisser de fichiers repris");
        }
        let display = self.display_handle.clone();
        let seat = self.seat.clone();
        start_dnd(
            &display,
            &seat,
            self,
            SERIAL_COUNTER.next_serial(),
            Some(start),
            None,
            SourceMetadata {
                mime_types: vec![
                    "text/uri-list".into(),
                    "text/plain;charset=utf-8".into(),
                    "text/plain".into(),
                ],
                dnd_action: DndAction::Copy | DndAction::Move,
            },
        );
    }

    /// Bouton relâché sans que le glisser ait été repris (Fichiers l'a terminé lui-même), ou
    /// après un dépôt que l'application n'a jamais déclaré fini.
    pub fn file_drag_released(&mut self) {
        if self
            .file_drag
            .as_ref()
            .is_some_and(|file| !file.active || file.dropped)
        {
            self.file_drag = None;
        }
    }

    /// Fichiers lâchés là où aucune application ne les a pris : Dock ou fenêtre Ferrite.
    fn drop_files(&mut self, file: FileDrag, location: Point<f64, Logical>) {
        let paths = file.drag.paths.clone();
        let env = self.client_environment();
        if let Some((_, item)) = self.dock_item_at(location) {
            if item.key == AppKey::Builtin(App::TRASH) {
                App::TRASH.launch_with(&env, &paths);
            } else if item.key.launch_with(&self.applications, &env, &paths) {
                if item.running {
                    self.bring_app_forward(&item.key);
                } else {
                    self.start_bounce(&item.key);
                }
            }
            return;
        }
        let Some((surface, origin)) = self.surface_under(location) else {
            return;
        };
        let Some(window) = self.window_for_surface(&surface) else {
            return;
        };
        // Une fenêtre qui sait ranger les fichiers (Fichiers) les reçoit à l'endroit lâché :
        // déplacés, ou copiés si Ctrl ou Alt est tenu.
        let copy = self.seat.get_keyboard().is_some_and(|keyboard| {
            let modifiers = keyboard.modifier_state();
            modifiers.ctrl || modifiers.alt
        });
        let deposit = Deposit {
            copy,
            x: (location.x - origin.x) as f32,
            y: (location.y - origin.y) as f32,
            paths,
        };
        let pid = surface
            .client()
            .and_then(|client| client.get_credentials(&self.display_handle).ok())
            .map(|credentials| credentials.pid as u32);
        if pid.is_some_and(|pid| deposit.send(pid)) {
            self.focus_window(Some(window));
            return;
        }
        // Sinon, l'application les ouvre (pas la fenêtre d'où ils viennent).
        if Some(&window) != file.source.as_ref()
            && let Some(key) = self.window_app(&window)
            && key.launch_with(&self.applications, &env, &deposit.paths)
        {
            self.focus_window(Some(window));
        }
    }
}

/// Texte des fichiers glissés dans le format `mime_type`.
fn payload(mime_type: &str, paths: &[PathBuf]) -> String {
    if mime_type == "text/uri-list" {
        drag::uri_list(paths)
    } else {
        paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl ServerDndGrabHandler for Ferrite {
    fn send(&mut self, mime_type: String, fd: OwnedFd, _seat: Seat<Self>) {
        let Some(file) = &self.file_drag else {
            return;
        };
        let text = payload(&mime_type, &file.drag.paths);
        // Écrit à part : l'application lit à son rythme.
        thread::spawn(move || {
            let _ = fs::File::from(fd).write_all(text.as_bytes());
        });
    }

    fn dropped(&mut self, _seat: Seat<Self>) {
        if let Some(file) = &mut self.file_drag {
            file.dropped = true;
        }
    }

    fn cancelled(&mut self, _seat: Seat<Self>) {
        let Some(file) = self.file_drag.take() else {
            return;
        };
        // Appelé pendant le relâchement, pointeur et données du siège encore verrouillés par
        // Smithay : redemander le pointeur ou donner le focus bloquerait ferrite-shell.
        let location = file.at;
        self.loop_handle
            .insert_idle(move |state| state.drop_files(file, location));
    }

    fn finished(&mut self, _seat: Seat<Self>) {
        self.file_drag = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contenu_selon_le_format() {
        let paths = [PathBuf::from("/tmp/a b.txt"), PathBuf::from("/tmp/c")];
        assert_eq!(
            payload("text/uri-list", &paths),
            "file:///tmp/a%20b.txt\r\nfile:///tmp/c\r\n"
        );
        assert_eq!(payload("text/plain", &paths), "/tmp/a b.txt\n/tmp/c");
    }
}
