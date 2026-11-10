use std::{
    env, fs,
    io::{self, ErrorKind},
    path::PathBuf,
};

const HEADER: &str = "FERRITE-SESSION-1";
const MAX_FILE_SIZE: u64 = 1024 * 1024;
const MAX_WINDOWS: usize = 128;
const MAX_WORKSPACE: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedWindow {
    pub application: String,
    pub workspace: usize,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub minimized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub active_workspace: usize,
    pub windows: Vec<SavedWindow>,
}

fn path() -> io::Result<PathBuf> {
    let state_home = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "dossier personnel indisponible"))?;
    Ok(state_home.join("ferrite").join("session"))
}

pub fn save(session: &Session) -> io::Result<()> {
    if session.active_workspace > MAX_WORKSPACE
        || session.windows.len() > MAX_WINDOWS
        || session
            .windows
            .iter()
            .any(|window| window.workspace > MAX_WORKSPACE)
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "session trop volumineuse",
        ));
    }
    let path = path()?;
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("chemin de session invalide"))?;
    fs::create_dir_all(directory)?;

    let mut contents = format!("{HEADER}\nactive\t{}\n", session.active_workspace);
    for window in &session.windows {
        contents.push_str(&format!(
            "window\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            window.workspace,
            window.x,
            window.y,
            window.width,
            window.height,
            u8::from(window.minimized),
            encode(&window.application),
        ));
    }

    let temporary = path.with_extension("tmp");
    fs::write(&temporary, contents)?;
    fs::rename(temporary, path)
}

pub fn take() -> io::Result<Option<Session>> {
    let path = path()?;
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if contents.len() as u64 > MAX_FILE_SIZE {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "état de session trop volumineux",
        ));
    }
    let session = parse(&contents)?;
    fs::remove_file(path)?;
    Ok(Some(session))
}

fn parse(contents: &str) -> io::Result<Session> {
    let mut lines = contents.lines();
    if lines.next() != Some(HEADER) {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "version de session inconnue",
        ));
    }
    let active_workspace = lines
        .next()
        .and_then(|line| line.strip_prefix("active\t"))
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|workspace| *workspace <= MAX_WORKSPACE)
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "espace actif invalide"))?;
    let mut windows = Vec::new();
    for line in lines {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 8 || fields[0] != "window" || windows.len() == MAX_WINDOWS {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "fenêtre de session invalide",
            ));
        }
        let workspace = parse_field::<usize>(fields[1])?;
        let x = parse_field::<i32>(fields[2])?;
        let y = parse_field::<i32>(fields[3])?;
        let width = parse_field::<i32>(fields[4])?;
        let height = parse_field::<i32>(fields[5])?;
        let minimized = match fields[6] {
            "0" => false,
            "1" => true,
            _ => return Err(io::Error::new(ErrorKind::InvalidData, "état de fenêtre invalide")),
        };
        let application = decode(fields[7])?;
        if workspace > MAX_WORKSPACE || !(1..=16_384).contains(&width) || !(1..=16_384).contains(&height) {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "géométrie de fenêtre invalide",
            ));
        }
        windows.push(SavedWindow {
            application,
            workspace,
            x,
            y,
            width,
            height,
            minimized,
        });
    }
    Ok(Session {
        active_workspace,
        windows,
    })
}

fn parse_field<T: std::str::FromStr>(field: &str) -> io::Result<T> {
    field
        .parse()
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "champ de session invalide"))
}

fn encode(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode(value: &str) -> io::Result<String> {
    if value.is_empty() || value.len() % 2 != 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "identifiant d’application invalide",
        ));
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair)
                .map_err(|_| io::Error::new(ErrorKind::InvalidData, "identifiant d’application invalide"))?;
            u8::from_str_radix(pair, 16)
                .map_err(|_| io::Error::new(ErrorKind::InvalidData, "identifiant d’application invalide"))
        })
        .collect::<io::Result<Vec<_>>>()?;
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "identifiant d’application non UTF-8"))
}

/// Réglage « Rouvrir les fenêtres au démarrage » (Paramètres > Fenêtres), coupé par défaut.
fn reopen_windows_wanted() -> bool {
    ferrite_system::config::Settings::load().flag(ferrite_system::config::REOPEN_WINDOWS, false)
}

impl crate::state::Ferrite {
    pub fn prepare_session_restore(&mut self, enabled: bool) {
        self.session_restore_enabled = enabled;
        if !enabled {
            return;
        }
        if !reopen_windows_wanted() {
            // Réglage coupé : oublie les fenêtres mémorisées au dernier arrêt.
            if let Ok(path) = path() {
                let _ = fs::remove_file(path);
            }
            return;
        }
        match take() {
            Ok(session) => self.session_to_restore = session,
            Err(error) => tracing::warn!(%error, "session précédente non restaurée"),
        }
    }

    pub fn restore_session_after_applications(&mut self) {
        let Some(session) = self.session_to_restore.take() else {
            return;
        };
        let last_workspace = session
            .windows
            .iter()
            .map(|window| window.workspace)
            .max()
            .unwrap_or(session.active_workspace)
            .max(session.active_workspace);
        self.workspaces.ensure_page(last_workspace.max(1));
        // On n'arrive jamais sur le bureau des widgets, même quitté dessus.
        self.go_to_page(session.active_workspace.max(1));

        let mut launches = Vec::new();
        let mut seen = Vec::new();
        for window in session.windows {
            let key = crate::desktop::apps::AppKey::from_token(&window.application, &self.applications);
            if matches!(key, crate::desktop::apps::AppKey::Other(_))
                || matches!(&key, crate::desktop::apps::AppKey::Installed(id)
                    if self.applications.iter().any(|entry| entry.id == *id && entry.terminal))
            {
                tracing::warn!(
                    application = window.application,
                    "application de la session précédente impossible à relancer"
                );
                continue;
            }
            if seen.contains(&key) && !self.allows_multiple_windows(&key) {
                tracing::warn!(
                    application = window.application,
                    "fenêtre supplémentaire non restaurée : l’application n’accepte qu’une fenêtre"
                );
                continue;
            }
            seen.push(key.clone());
            launches.push((key, window));
        }
        self.session_restore_queue = launches.iter().map(|(_, window)| window.clone()).collect();
        if !self.session_restore_queue.is_empty() {
            self.session_restore_deadline =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
        }
        for (key, _) in launches {
            self.launch_app(&key);
        }
    }

    pub fn take_window_placement(&mut self, window: &smithay::desktop::Window) -> Option<SavedWindow> {
        let app_id = crate::render::app_id(window)?;
        let key = crate::desktop::apps::AppKey::for_window(&app_id, &self.applications);
        let index = self
            .session_restore_queue
            .iter()
            .position(|saved| saved.application == key.token())?;
        self.session_restore_deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
        Some(self.session_restore_queue.remove(index))
    }

    pub fn save_session_before_power(&self) {
        // Le réglage est relu ici : il a pu changer pendant la session.
        if !self.session_restore_enabled || !reopen_windows_wanted() {
            return;
        }
        let Some(screen) = self.screen_area() else {
            tracing::warn!("fenêtres non mémorisées : aucun écran actif");
            return;
        };
        let mut windows = Vec::new();
        for window in self.space.elements() {
            if let Some(location) = self.space.element_location(window) {
                save_window(
                    self,
                    window,
                    self.workspaces.current(),
                    location,
                    false,
                    screen.loc,
                    &mut windows,
                );
            }
        }
        for workspace in 0..self.workspaces.page_count() {
            for parked in self.workspaces.parked(workspace) {
                save_window(
                    self,
                    &parked.window,
                    workspace,
                    parked.location,
                    false,
                    screen.loc,
                    &mut windows,
                );
            }
        }
        for minimized in &self.minimized {
            save_window(
                self,
                &minimized.window,
                minimized.workspace,
                minimized.location,
                true,
                screen.loc,
                &mut windows,
            );
        }
        let session = Session {
            active_workspace: self.workspaces.current(),
            windows,
        };
        if let Err(error) = save(&session) {
            tracing::warn!(%error, "fenêtres et espaces de travail non mémorisés");
        }
    }
}

fn save_window(
    state: &crate::state::Ferrite,
    window: &smithay::desktop::Window,
    workspace: usize,
    location: smithay::utils::Point<i32, smithay::utils::Logical>,
    minimized: bool,
    screen: smithay::utils::Point<i32, smithay::utils::Logical>,
    saved: &mut Vec<SavedWindow>,
) {
    let Some(application) = state.window_app(window) else {
        return;
    };
    let titlebar = if crate::wm::is_server_decorated(window) {
        ferrite_ui::theme::TITLEBAR_HEIGHT as i32
    } else {
        0
    };
    let size = window.geometry().size;
    saved.push(SavedWindow {
        application: application.token(),
        workspace,
        x: location.x - screen.x,
        y: location.y - screen.y - titlebar,
        width: size.w,
        height: size.h + titlebar,
        minimized,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_avec_fenetres_reduites_se_serialize_et_se_relit() {
        let session = Session {
            active_workspace: 2,
            windows: vec![
                SavedWindow {
                    application: "ferrite-files".into(),
                    workspace: 1,
                    x: 30,
                    y: -4,
                    width: 900,
                    height: 600,
                    minimized: false,
                },
                SavedWindow {
                    application: "org.example.Editor".into(),
                    workspace: 2,
                    x: 20,
                    y: 40,
                    width: 800,
                    height: 500,
                    minimized: true,
                },
            ],
        };
        let encoded = format!(
            "{HEADER}\nactive\t2\nwindow\t1\t30\t-4\t900\t600\t0\t{}\nwindow\t2\t20\t40\t800\t500\t1\t{}\n",
            encode(&session.windows[0].application),
            encode(&session.windows[1].application),
        );
        assert_eq!(parse(&encoded).unwrap(), session);
    }

    #[test]
    fn session_invalide_est_refusee() {
        assert!(parse("FERRITE-SESSION-0\n").is_err());
        assert!(parse(&format!("{HEADER}\nactive\t999\n")).is_err());
        assert!(
            parse(&format!(
                "{HEADER}\nactive\t0\nwindow\t1\t0\t0\t0\t500\t0\t666f6f\n"
            ))
            .is_err()
        );
    }
}
