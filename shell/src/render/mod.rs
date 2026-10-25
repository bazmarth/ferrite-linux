//! Rendu d'un écran : fond d'écran puis fenêtres. Les effets (flou, ombres, coins
//! arrondis) viendront ici (`docs/PLAN.md`, phase 2 tâche 7).

pub mod capture;
mod wallpaper;

use smithay::{
    backend::renderer::{
        element::{Kind, memory::MemoryRenderBufferRenderElement, surface::WaylandSurfaceRenderElement},
        gles::GlesRenderer,
    },
    desktop::space::{SpaceRenderElements, space_render_elements},
    output::Output,
    render_elements,
};
use tracing::warn;

pub use wallpaper::Wallpaper;

use crate::state::Ferrite;

render_elements! {
    /// Tout ce qui peut être dessiné sur un écran.
    pub OutputElement<=GlesRenderer>;
    Space = SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>,
    Wallpaper = MemoryRenderBufferRenderElement<GlesRenderer>,
}

/// Éléments à dessiner sur `output`, du premier plan vers l'arrière-plan.
pub fn output_elements(
    renderer: &mut GlesRenderer,
    state: &Ferrite,
    wallpaper: &Wallpaper,
    output: &Output,
) -> Vec<OutputElement> {
    let mut elements: Vec<OutputElement> = match space_render_elements(renderer, [&state.space], output, 1.0)
    {
        Ok(windows) => windows.into_iter().map(OutputElement::Space).collect(),
        Err(err) => {
            warn!("écran sans mode : {err:?}");
            Vec::new()
        }
    };
    let Some(geometry) = state.space.output_geometry(output) else {
        return elements;
    };
    let scale = output.current_scale().fractional_scale();
    let location = geometry.loc.to_f64().to_physical(scale);
    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        location,
        wallpaper.buffer(),
        None,
        // Toute la texture, étirée à la taille de l'écran (sans `src`, Smithay prendrait
        // la taille de l'écran comme zone source et n'étirerait rien).
        Some(wallpaper.source()),
        Some(geometry.size),
        Kind::Unspecified,
    ) {
        Ok(element) => elements.push(OutputElement::Wallpaper(element)),
        Err(err) => warn!("fond d'écran non importé : {err:?}"),
    }
    elements
}
