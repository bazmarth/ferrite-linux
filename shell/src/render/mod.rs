//! Rendu d'un écran : fenêtres (contenu, décorations, ombres) puis fond d'écran.
//! Le flou d'arrière-plan viendra ici (`docs/PLAN.md`, phase 2 tâche 7).

pub mod capture;
mod decorations;
mod rounded;
mod shaders;
mod title;
mod wallpaper;

use ferrite_ui::theme::WINDOW_RADIUS;
use smithay::{
    backend::renderer::{
        element::{
            AsRenderElements, Kind,
            memory::MemoryRenderBufferRenderElement,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
        gles::{GlesRenderer, element::PixelShaderElement},
    },
    desktop::{PopupManager, Window},
    output::Output,
    render_elements,
    utils::{Logical, Point, Rectangle},
};
use tracing::warn;

use self::rounded::RoundedSurface;
pub use self::{decorations::Decorations, wallpaper::Wallpaper};
use crate::{state::Ferrite, wm};

render_elements! {
    /// Tout ce qui peut être dessiné sur un écran.
    pub OutputElement<=GlesRenderer>;
    Surface = WaylandSurfaceRenderElement<GlesRenderer>,
    Rounded = RoundedSurface,
    Shader = PixelShaderElement,
    Memory = MemoryRenderBufferRenderElement<GlesRenderer>,
}

/// Ressources de rendu créées avec le moteur de rendu.
#[derive(Debug)]
pub struct RenderAssets {
    pub wallpaper: Wallpaper,
    pub decorations: Decorations,
}

impl RenderAssets {
    pub fn new(renderer: &mut GlesRenderer) -> Result<RenderAssets, String> {
        Ok(RenderAssets {
            wallpaper: Wallpaper::gradient(),
            decorations: Decorations::new(renderer)?,
        })
    }
}

/// Éléments à dessiner sur `output`, du premier plan vers l'arrière-plan.
pub fn output_elements(
    renderer: &mut GlesRenderer,
    state: &Ferrite,
    assets: &RenderAssets,
    output: &Output,
) -> Vec<OutputElement> {
    let mut elements = Vec::new();
    let Some(output_geo) = state.space.output_geometry(output) else {
        return elements;
    };
    let scale = output.current_scale().fractional_scale();

    // Le survol des feux n'est montré que si aucun déplacement n'est en cours.
    let pointer = state
        .seat
        .get_pointer()
        .filter(|pointer| !pointer.is_grabbed())
        .map(|pointer| pointer.current_location());

    // Du dessus vers le dessous.
    for window in state.space.elements().rev() {
        let Some(content) = state.space.element_geometry(window) else {
            continue;
        };
        let local = Rectangle::new(content.loc - output_geo.loc, content.size);
        if wm::is_server_decorated(window) {
            let hover = pointer.is_some_and(|p| wm::decoration::lights_hovered(content, p));
            push_decorated(
                renderer,
                assets,
                window,
                local,
                (wm::is_activated(window), hover),
                scale,
                &mut elements,
            );
        } else {
            let location = (local.loc - window.geometry().loc).to_physical_precise_round(scale);
            elements.extend(
                window
                    .render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                        renderer,
                        location,
                        scale.into(),
                        1.0,
                    )
                    .into_iter()
                    .map(OutputElement::Surface),
            );
        }
    }

    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        Point::from((0.0, 0.0)),
        assets.wallpaper.buffer(),
        None,
        // Toute la texture, étirée à la taille de l'écran (sans `src`, Smithay prendrait
        // la taille de l'écran comme zone source et n'étirerait rien).
        Some(assets.wallpaper.source()),
        Some(output_geo.size),
        Kind::Unspecified,
    ) {
        Ok(element) => elements.push(OutputElement::Memory(element)),
        Err(err) => warn!("fond d'écran non importé : {err:?}"),
    }
    elements
}

/// Fenêtre décorée par ferrite-shell : menus contextuels, titre, barre de titre, contenu
/// aux coins arrondis, ombre.
fn push_decorated(
    renderer: &mut GlesRenderer,
    assets: &RenderAssets,
    window: &Window,
    content: Rectangle<i32, Logical>,
    look: (bool, bool),
    scale: f64,
    out: &mut Vec<OutputElement>,
) {
    let Some(toplevel) = window.toplevel() else {
        return;
    };
    let surface = toplevel.wl_surface();
    let location = (content.loc - window.geometry().loc).to_physical_precise_round(scale);

    for (popup, offset) in PopupManager::popups_for_surface(surface) {
        let offset = (window.geometry().loc + offset - popup.geometry().loc).to_physical_precise_round(scale);
        out.extend(
            render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<GlesRenderer>>(
                renderer,
                popup.wl_surface(),
                location + offset,
                scale,
                1.0,
                Kind::Unspecified,
            )
            .into_iter()
            .map(OutputElement::Surface),
        );
    }

    let mut decorations = Vec::new();
    assets
        .decorations
        .push_elements(renderer, window, content, look, scale, &mut decorations);
    // Le titre et la barre passent devant le contenu, l'ombre derrière.
    let shadow = decorations.pop();
    out.extend(decorations);

    let clip = content.to_physical_precise_round(scale);
    let radius = (WINDOW_RADIUS as f64 * scale) as f32;
    let program = assets.decorations.shaders.rounded.clone();
    out.extend(
        render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<GlesRenderer>>(
            renderer,
            surface,
            location,
            scale,
            1.0,
            Kind::Unspecified,
        )
        .into_iter()
        .map(|element| OutputElement::Rounded(RoundedSurface::new(element, program.clone(), clip, radius))),
    );
    out.extend(shadow);
}
