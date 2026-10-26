//! Rendu d'un écran : fenêtres (contenu, décorations, ombres) puis fond d'écran.
//! Le flou d'arrière-plan viendra ici (`docs/PLAN.md`, phase 2 tâche 7).

pub mod capture;
mod cursor;
mod decorations;
mod rounded;
mod shaders;
mod title;
mod wallpaper;

use ferrite_ui::theme::WINDOW_RADIUS;
use smithay::wayland::{compositor::with_states, seat::WaylandFocus, shell::xdg::XdgToplevelSurfaceData};
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
    input::pointer::CursorImageStatus,
    output::Output,
    render_elements,
    utils::{Logical, Point, Rectangle},
};
use tracing::warn;

use self::rounded::RoundedSurface;
pub use self::{cursor::Cursor, decorations::Decorations, wallpaper::Wallpaper};
use crate::{
    desktop::{
        apps::App,
        apps::LauncherEntry,
        dock::Dock,
        panels::{DesktopView, LAUNCHER_PAGE_SIZE, LauncherView, Panels, Popover, Tile, TileIcon},
    },
    state::Ferrite,
    wm,
};

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
    pub cursor: Cursor,
    pub dock: Dock,
    pub panels: Panels,
}

impl RenderAssets {
    pub fn new(renderer: &mut GlesRenderer) -> Result<RenderAssets, String> {
        Ok(RenderAssets {
            wallpaper: Wallpaper::landscape(),
            decorations: Decorations::new(renderer)?,
            cursor: Cursor::default(),
            dock: Dock::new(),
            panels: Panels::new()?,
        })
    }
}

/// Éléments à dessiner sur `output`, du premier plan vers l'arrière-plan. Avec
/// `draw_cursor`, ferrite-shell dessine lui-même le pointeur (backend DRM).
pub fn output_elements(
    renderer: &mut GlesRenderer,
    state: &Ferrite,
    assets: &RenderAssets,
    output: &Output,
    draw_cursor: bool,
) -> Vec<OutputElement> {
    let mut elements = Vec::new();
    let Some(output_geo) = state.space.output_geometry(output) else {
        return elements;
    };
    let scale = output.current_scale().fractional_scale();

    if draw_cursor && !matches!(state.cursor_status, CursorImageStatus::Hidden) {
        if let Some(pointer) = state.seat.get_pointer() {
            let location = pointer.current_location() - output_geo.loc.to_f64();
            if Rectangle::from_size(output_geo.size).to_f64().contains(location) {
                push_cursor(renderer, &assets.cursor, location, scale, &mut elements);
            }
        }
    }

    let running = running_apps(state);
    assets
        .dock
        .push_element(renderer, output, output_geo, scale, &running, &mut elements);
    let view = desktop_view(state);
    assets
        .panels
        .push_element(renderer, output, output_geo, scale, &view, &mut elements);

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
    // Surface Wayland de la fenêtre, qu'elle soit native ou X11 (via XWayland).
    let Some(surface) = window.wl_surface() else {
        return;
    };
    let surface = &*surface;
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

/// Curseur Ferrite, pointe en `location` (logique, repère de l'écran).
fn push_cursor(
    renderer: &mut GlesRenderer,
    cursor: &Cursor,
    location: Point<f64, Logical>,
    scale: f64,
    out: &mut Vec<OutputElement>,
) {
    let image = cursor.image(scale);
    let tip = location.to_physical(scale);
    let position = Point::from((
        (tip.x - image.hotspot.0 as f64).round(),
        (tip.y - image.hotspot.1 as f64).round(),
    ));
    // Image en pixels physiques : sa taille logique est divisée par l'échelle.
    let src = Rectangle::from_size((image.size.0 as f64, image.size.1 as f64).into());
    let size = (
        (image.size.0 as f64 / scale).round() as i32,
        (image.size.1 as f64 / scale).round() as i32,
    );
    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        position,
        &image.buffer,
        None,
        Some(src),
        Some(size.into()),
        Kind::Cursor,
    ) {
        Ok(element) => out.push(OutputElement::Memory(element)),
        Err(err) => warn!("curseur non importé : {err:?}"),
    }
}

/// État du bureau affiché par les panneaux (et utilisé pour leurs clics).
pub fn desktop_view(state: &Ferrite) -> DesktopView {
    let system = &state.system;
    DesktopView {
        popover: state.popover,
        wifi: system.wifi,
        bluetooth: system.bluetooth,
        volume: system
            .volume
            .map(|volume| if volume.muted { 0 } else { volume.level }),
        brightness: system.brightness,
        power: state.pending_power,
        error: state.system_error.clone(),
        launcher: if state.popover == Some(Popover::Launcher) {
            launcher_view(state)
        } else {
            LauncherView::default()
        },
    }
}

/// Page affichée du menu Applications.
fn launcher_view(state: &Ferrite) -> LauncherView {
    let entries = state.launcher_entries();
    let pages = entries.len().div_ceil(LAUNCHER_PAGE_SIZE).max(1);
    let page = state.launcher_page.min(pages - 1);
    let tiles = entries
        .into_iter()
        .skip(page * LAUNCHER_PAGE_SIZE)
        .take(LAUNCHER_PAGE_SIZE)
        .map(|entry| match entry {
            LauncherEntry::Builtin(app) => Tile {
                name: app.name().to_owned(),
                icon: TileIcon::Builtin(app.icon()),
            },
            LauncherEntry::Installed(entry) => Tile {
                name: entry.name.clone(),
                icon: entry.icon.clone().map_or(TileIcon::Missing, TileIcon::File),
            },
        })
        .collect();
    LauncherView {
        query: state.launcher_query.clone(),
        page,
        pages,
        tiles,
    }
}

/// Applications du Dock qui ont au moins une fenêtre, visible ou réduite.
fn running_apps(state: &Ferrite) -> Vec<App> {
    let windows = state
        .space
        .elements()
        .chain(state.minimized.iter().map(|minimized| &minimized.window));
    let ids: Vec<String> = windows.filter_map(app_id).collect();
    App::ALL
        .into_iter()
        .filter(|app| ids.iter().any(|id| id == app.app_id()))
        .collect()
}

/// Identifiant de l'application : `app_id` Wayland, ou classe X11 en minuscules.
pub fn app_id(window: &Window) -> Option<String> {
    if let Some(x11) = window.x11_surface() {
        return Some(x11.class().to_lowercase());
    }
    let toplevel = window.toplevel()?;
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|data| data.lock().ok().and_then(|data| data.app_id.clone()))
    })
}
