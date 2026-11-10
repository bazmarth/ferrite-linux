//! Les fenêtres ne sont ni mémorisées ni rouvertes : le bureau repart toujours vide.
//! Il reste seulement à effacer le fichier laissé par les anciennes versions.

use std::{env, fs, path::PathBuf};

/// Fichier où les anciennes versions mémorisaient les fenêtres ouvertes.
fn old_session_file() -> Option<PathBuf> {
    let state_home = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))?;
    Some(state_home.join("ferrite").join("session"))
}

/// Efface les fenêtres mémorisées par une ancienne version, s'il en reste.
pub fn forget_old_session() {
    if let Some(path) = old_session_file() {
        let _ = fs::remove_file(path);
    }
}
