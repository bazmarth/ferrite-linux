//! `linux-dmabuf` : tampons graphiques partagés avec le GPU.
//!
//! Les navigateurs, lecteurs vidéo et jeux dessinent avec le GPU et transmettent leurs
//! images en dmabuf. Sans ce protocole, Mesa retombe sur un rendu par le processeur.
//! Le « retour » (feedback) indique aux clients la carte graphique de rendu à utiliser.

use smithay::{
    backend::{
        allocator::{Format, dmabuf::Dmabuf},
        egl::{EGLDevice, EGLDisplay},
    },
    delegate_dmabuf,
    wayland::dmabuf::{DmabufFeedbackBuilder, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
};
use tracing::{info, warn};

use crate::state::Ferrite;

impl Ferrite {
    /// Publie `linux-dmabuf` avec les formats que le moteur de rendu sait importer.
    pub fn publish_dmabuf(&mut self, display: &EGLDisplay, formats: Vec<Format>) {
        if formats.is_empty() {
            info!("aucun format dmabuf : rendu des clients par mémoire partagée seulement");
            return;
        }
        let node = EGLDevice::device_for_display(display)
            .ok()
            .and_then(|device| device.try_get_render_node().ok().flatten());
        let global = match node {
            Some(node) => match DmabufFeedbackBuilder::new(node.dev_id(), formats.clone()).build() {
                Ok(feedback) => {
                    info!(noeud = ?node.dev_path(), "dmabuf avec retour du périphérique de rendu");
                    self.dmabuf_state
                        .create_global_with_default_feedback::<Ferrite>(&self.display_handle, &feedback)
                }
                Err(err) => {
                    warn!(?err, "retour dmabuf impossible, version simple");
                    self.dmabuf_state
                        .create_global::<Ferrite>(&self.display_handle, formats)
                }
            },
            None => self
                .dmabuf_state
                .create_global::<Ferrite>(&self.display_handle, formats),
        };
        self.dmabuf_global = Some(global);
    }
}

impl DmabufHandler for Ferrite {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, _dmabuf: Dmabuf, notifier: ImportNotifier) {
        // L'import réel a lieu au premier rendu ; un tampon inutilisable n'est simplement
        // pas dessiné, sans gêner les autres fenêtres.
        let _ = notifier.successful::<Ferrite>();
    }
}

delegate_dmabuf!(Ferrite);
