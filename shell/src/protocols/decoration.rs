//! `xdg-decoration` : qui dessine la barre de titre, le client ou ferrite-shell.
//!
//! Toutes les fenêtres ont la même barre de titre : celle de Ferrite (fermer à gauche,
//! agrandir à droite), même quand le client demande à dessiner son cadre, car beaucoup en
//! demandent un sans y mettre de bouton (ArtCraft et les applications egui ou winit sans
//! cadre). Gardent le leur : les applications Ferrite (barre unifiée au même style) et les
//! applications de `cadre_propre` (navigateurs Chromium, dont les onglets sont la barre).
//! Les applications GTK n'utilisent pas ce protocole : elles dessinent leur cadre, avec
//! l'ordre des boutons publié par `system.rs` (`button-layout`).

use smithay::{
    delegate_xdg_decoration,
    reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode,
    wayland::shell::xdg::{ToplevelSurface, decoration::XdgDecorationHandler},
};

use crate::state::Ferrite;

impl XdgDecorationHandler for Ferrite {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        set_mode(&toplevel, Mode::ServerSide);
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: Mode) {
        tracing::info!(app_id = %app_id(&toplevel), ?mode, "décorations demandées");
        set_mode(&toplevel, imposed(&toplevel, mode));
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
fn imposed(toplevel: &ToplevelSurface, mode: Mode) -> Mode {
    if mode != Mode::ClientSide {
        return mode;
    }
    let id = app_id(toplevel);
    // L'identifiant n'est pas encore connu : la demande est gardée, revue à son arrivée.
    if id.is_empty() || keeps_own_frame(&id, &ferrite_system::config::Settings::load().own_frame_apps()) {
        mode
    } else {
        Mode::ServerSide
    }
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
pub fn app_id_changed(toplevel: &ToplevelSurface) {
    let current = toplevel.with_pending_state(|state| state.decoration_mode);
    if let Some(mode) = current {
        let wanted = imposed(toplevel, mode);
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
        assert!(!keeps_own_frame("ai.storyteller.artcraft-launcher", &own));
        assert!(!keeps_own_frame("foot", &own));
    }
}
