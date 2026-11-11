//! `xdg-decoration` : qui dessine la barre de titre, le client ou ferrite-shell.
//!
//! Toutes les fenêtres ont la même barre de titre : celle de Ferrite (réduire, agrandir et
//! fermer à droite), même quand le client demande à dessiner son cadre, car beaucoup en
//! demandent un sans y mettre de bouton (applications egui ou winit sans cadre). Gardent le leur : les applications Ferrite (barre unifiée au même style) et les
//! applications de `cadre_propre` (navigateurs Chromium, dont les onglets sont la barre ;
//! PhotoCraft et le lanceur ArtCraft, qui dessinent leurs boutons dans la leur) et
//! toute application bâtie sur Chromium (Electron : Claude, VS Code, ChatGPT…), qui dessine
//! toujours une vraie barre avec ses boutons ; lui imposer la nôtre empilait deux barres.
//! Les applications GTK n'utilisent pas ce protocole : elles dessinent leur cadre, avec
//! l'ordre des boutons publié par `system.rs` (`button-layout`).

use std::path::{Path, PathBuf};

use smithay::{
    delegate_xdg_decoration,
    reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode,
    reexports::wayland_server::{DisplayHandle, Resource},
    wayland::shell::xdg::{ToplevelSurface, decoration::XdgDecorationHandler},
};

use crate::state::Ferrite;

impl XdgDecorationHandler for Ferrite {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        set_mode(&toplevel, Mode::ServerSide);
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: Mode) {
        tracing::info!(app_id = %app_id(&toplevel), ?mode, "décorations demandées");
        set_mode(&toplevel, imposed(&self.display_handle, &toplevel, mode));
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        set_mode(&toplevel, Mode::ServerSide);
    }
}

delegate_xdg_decoration!(Ferrite);

/// Identifiant de fenêtre du client (vide tant qu'il ne l'a pas donné).
pub fn app_id(toplevel: &ToplevelSurface) -> String {
    smithay::wayland::compositor::with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
            .and_then(|data| data.lock().ok()?.app_id.clone())
    })
    .unwrap_or_default()
}

/// Mode retenu : la barre de Ferrite, sauf pour les applications qui gardent leur cadre.
fn imposed(display: &DisplayHandle, toplevel: &ToplevelSurface, mode: Mode) -> Mode {
    if mode != Mode::ClientSide {
        return mode;
    }
    let id = app_id(toplevel);
    // L'identifiant n'est pas encore connu : la demande est gardée, revue à son arrivée.
    if id.is_empty()
        || keeps_own_frame(&id, &ferrite_system::config::Settings::load().own_frame_apps())
        || is_chromium_client(display, toplevel)
    {
        mode
    } else {
        Mode::ServerSide
    }
}

/// Le client est-il un programme bâti sur Chromium (navigateur ou Electron) ? Ces
/// programmes dessinent toujours une barre complète (titre, boutons) quand ils demandent
/// leur propre cadre. Reconnus aux fichiers que Chromium pose à côté de son exécutable ;
/// `/proc/<pid>/root` voit aussi l'intérieur d'un Flatpak. Simple lecture de `/proc`, faite
/// une fois par demande de décorations.
fn is_chromium_client(display: &DisplayHandle, toplevel: &ToplevelSurface) -> bool {
    let Some(pid) = toplevel
        .wl_surface()
        .client()
        .and_then(|client| client.get_credentials(display).ok())
        .map(|credentials| credentials.pid)
    else {
        return false;
    };
    let proc_dir = PathBuf::from(format!("/proc/{pid}"));
    let Ok(exe) = std::fs::read_link(proc_dir.join("exe")) else {
        return false;
    };
    let chromium = is_chromium_dir(&proc_dir.join("root"), &exe);
    if chromium {
        tracing::info!(app_id = %app_id(toplevel), exe = %exe.display(), "Chromium : cadre du client gardé");
    }
    chromium
}

/// Le dossier de l'exécutable `exe` (vu depuis la racine `root`) contient-il les fichiers de
/// Chromium ?
fn is_chromium_dir(root: &Path, exe: &Path) -> bool {
    let Some(dir) = exe.parent() else {
        return false;
    };
    let dir = root.join(dir.strip_prefix("/").unwrap_or(dir));
    dir.join("v8_context_snapshot.bin").is_file() && dir.join("resources.pak").is_file()
}

/// L'application `id` garde-t-elle son propre cadre ?
fn keeps_own_frame(id: &str, own_frame: &[String]) -> bool {
    let id = id.to_lowercase();
    id.starts_with("ferrite-")
        || own_frame
            .iter()
            .any(|prefix| !prefix.is_empty() && id.starts_with(&prefix.to_lowercase()))
}

/// L'identifiant arrive souvent après la demande de décorations : la règle est revue.
pub fn app_id_changed(display: &DisplayHandle, toplevel: &ToplevelSurface) {
    let current = toplevel.with_pending_state(|state| state.decoration_mode);
    if let Some(mode) = current {
        let wanted = imposed(display, toplevel, mode);
        if wanted != mode {
            tracing::info!(app_id = %app_id(toplevel), "barre de titre Ferrite posée");
            set_mode(toplevel, wanted);
        }
    }
}

fn set_mode(toplevel: &ToplevelSurface, mode: Mode) {
    toplevel.with_pending_state(|state| state.decoration_mode = Some(mode));
    // Avant la première configuration, le mode part avec elle.
    if toplevel.is_initial_configure_sent() {
        toplevel.send_pending_configure();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seuls_ferrite_et_les_navigateurs_gardent_leur_cadre() {
        let own = vec!["brave".to_owned(), "chromium".to_owned()];
        assert!(keeps_own_frame("ferrite-files", &own));
        assert!(keeps_own_frame("brave-browser", &own));
        assert!(keeps_own_frame("Chromium-browser", &own));
        assert!(!keeps_own_frame("ai.storyteller.photocraft", &own));
        assert!(!keeps_own_frame("foot", &own));
    }

    #[test]
    fn photocraft_et_le_lanceur_artcraft_gardent_leur_barre_par_defaut() {
        let own = ferrite_system::config::Settings::default().own_frame_apps();
        assert!(keeps_own_frame("ai.storyteller.photocraft", &own));
        assert!(keeps_own_frame("ai.storyteller.artcraft-launcher", &own));
        assert!(!keeps_own_frame("ai.storyteller.artcraft", &own));
    }

    #[test]
    fn chromium_reconnu_a_ses_fichiers() {
        let root = std::env::temp_dir().join(format!("ferrite-chromium-{}", std::process::id()));
        let dir = root.join("opt/appli");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = Path::new("/opt/appli/appli");
        assert!(!is_chromium_dir(&root, exe));
        std::fs::write(dir.join("v8_context_snapshot.bin"), b"").unwrap();
        assert!(!is_chromium_dir(&root, exe));
        std::fs::write(dir.join("resources.pak"), b"").unwrap();
        assert!(is_chromium_dir(&root, exe));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
