//! Rendu d'un écran : fenêtres (contenu, décorations, ombres) puis fond d'écran.
//! Le flou d'arrière-plan viendra ici (`docs/PLAN.md`, phase 2 tâche 7).

pub mod capture;
mod cursor;
mod decorations;
mod rounded;
mod shaders;
mod title;
mod wallpaper;

use ferrite_system::config::DockHiding;
use ferrite_ui::theme::WINDOW_RADIUS;
use smithay::wayland::{compositor::with_states, seat::WaylandFocus, shell::xdg::XdgToplevelSurfaceData};
use smithay::{
    backend::renderer::{
        element::{
            AsRenderElements, Kind,
            memory::MemoryRenderBufferRenderElement,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
        gles::{GlesRenderer, Uniform, element::PixelShaderElement},
    },
    desktop::{PopupManager, Window},
    input::pointer::CursorImageStatus,
    output::Output,
    render_elements,
    utils::{Logical, Point, Rectangle},
};
use tracing::warn;

use self::rounded::RoundedSurface;
pub use self::{
    cursor::{Cursor, CursorShape},
    decorations::Decorations,
    wallpaper::Wallpaper,
};
use crate::{
    desktop::{
        apps::App,
        apps::LauncherEntry,
        dock::Dock,
        panels::{
            ClipItem, ClipPreview, ClipboardView, DesktopView, LAUNCHER_PAGE_SIZE, LauncherView, Panels,
            Popover, Tile, TileIcon,
        },
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
                let shape = state.cursor_shape();
                push_cursor(renderer, &assets.cursor, shape, location, scale, &mut elements);
            }
        }
    }

    let running = running_apps(state);
    let dock_shown = state.dock_shown(output_geo);
    assets.dock.push_element(
        renderer,
        output,
        output_geo,
        scale,
        &running,
        state.dock_config,
        dock_shown,
        &mut elements,
    );
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

    // Aperçu d'accrochage, juste sous la fenêtre déplacée (celle du dessus).
    let snap_preview = state.snap_preview.map(|zone| {
        PixelShaderElement::new(
            assets.decorations.shaders.snap.clone(),
            Rectangle::new(zone.loc - output_geo.loc, zone.size),
            None,
            1.0,
            vec![Uniform::new("scale", scale as f32)],
            Kind::Unspecified,
        )
    });
    let mut snap_preview = snap_preview.map(OutputElement::Shader);

    // Du dessus vers le dessous.
    for (index, window) in state.space.elements().rev().enumerate() {
        if index == 1 {
            elements.extend(snap_preview.take());
        }
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
    elements.extend(snap_preview);

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
    shape: CursorShape,
    location: Point<f64, Logical>,
    scale: f64,
    out: &mut Vec<OutputElement>,
) {
    let image = cursor.image(shape, scale);
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

/// Hauteur de la bande du haut qui rappelle une barre masquée.
const BAR_REVEAL_EDGE: f64 = 2.0;

/// Double flèche qui correspond aux bords tirés.
fn resize_shape(edges: wm::ResizeEdge) -> CursorShape {
    use wm::ResizeEdge;
    if edges == ResizeEdge::TOP_LEFT || edges == ResizeEdge::BOTTOM_RIGHT {
        CursorShape::DiagonalDown
    } else if edges == ResizeEdge::TOP_RIGHT || edges == ResizeEdge::BOTTOM_LEFT {
        CursorShape::DiagonalUp
    } else if edges.intersects(ResizeEdge::LEFT | ResizeEdge::RIGHT) {
        CursorShape::Horizontal
    } else {
        CursorShape::Vertical
    }
}

impl Ferrite {
    /// Forme du curseur : double flèche pendant un redimensionnement ou sur le bord d'une
    /// fenêtre, flèche sinon.
    pub fn cursor_shape(&self) -> CursorShape {
        if let Some(edges) = self.resizing {
            return resize_shape(edges);
        }
        let Some(pointer) = self.seat.get_pointer().filter(|pointer| !pointer.is_grabbed()) else {
            return CursorShape::Arrow;
        };
        match self.hit_test(pointer.current_location()) {
            Some(wm::Hit::Decoration(_, wm::decoration::Part::Border(edges))) => resize_shape(edges),
            _ => CursorShape::Arrow,
        }
    }

    /// La fenêtre du dessus est-elle en plein écran ?
    pub fn top_fullscreen(&self) -> bool {
        self.space
            .elements()
            .rfind(|window| !window.x11_surface().is_some_and(|x11| x11.is_override_redirect()))
            .is_some_and(wm::is_fullscreen)
    }

    fn pointer_location(&self) -> Option<Point<f64, Logical>> {
        self.seat.get_pointer().map(|pointer| pointer.current_location())
    }

    /// Cadres des fenêtres affichées, barre de titre comprise.
    fn window_frames(&self) -> Vec<Rectangle<i32, Logical>> {
        self.space
            .elements()
            .filter(|window| !window.x11_surface().is_some_and(|x11| x11.is_override_redirect()))
            .filter_map(|window| {
                let content = self.space.element_geometry(window)?;
                Some(if wm::is_server_decorated(window) {
                    wm::decoration::frame(content)
                } else {
                    content
                })
            })
            .collect()
    }

    /// Le Dock doit-il être affiché sur l'écran `output` ?
    pub fn dock_shown(&self, output: Rectangle<i32, Logical>) -> bool {
        let frames = self.window_frames();
        crate::desktop::dock::should_show(crate::desktop::dock::Visibility {
            config: self.dock_config,
            output,
            pointer: self.pointer_location(),
            windows: &frames,
            fullscreen: self.top_fullscreen(),
            launcher_open: self.popover == Some(Popover::Launcher),
        })
    }

    /// La barre du haut est-elle affichée ? Masquée (plein écran ou réglage de
    /// Paramètres), elle revient quand la souris touche le bord du haut, et reste tant
    /// qu'elle est dessus ou qu'un menu est ouvert. Il faut toucher le bord lui-même :
    /// la barre de titre d'une fenêtre agrandie, juste dessous, reste accessible.
    pub fn bar_visible(&self) -> bool {
        let height = ferrite_ui::theme::MENUBAR_HEIGHT as f64;
        let revealed = self.bar_revealed.get();
        let pointer_calls = self
            .pointer_location()
            .is_some_and(|pointer| pointer.y < BAR_REVEAL_EDGE || (revealed && pointer.y < height));
        self.bar_revealed.set(pointer_calls);
        if pointer_calls || self.popover.is_some() || self.pending_power.is_some() {
            return true;
        }
        if self.top_fullscreen() {
            return false;
        }
        match self.dock_config.bar {
            DockHiding::Never => true,
            DockHiding::Always => false,
            DockHiding::Smart => {
                let Some(output) = self
                    .space
                    .outputs()
                    .next()
                    .and_then(|output| self.space.output_geometry(output))
                else {
                    return true;
                };
                let strip = Rectangle::new(output.loc, (output.size.w, height as i32).into());
                !self.window_frames().iter().any(|frame| frame.overlaps(strip))
            }
        }
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
        bar_visible: state.bar_visible(),
        launcher: if state.popover == Some(Popover::Launcher) {
            launcher_view(state)
        } else {
            LauncherView::default()
        },
        clipboard: if state.popover == Some(Popover::Clipboard) {
            clipboard_view(state)
        } else {
            ClipboardView::default()
        },
    }
}

/// Lignes visibles du panneau Super+V.
fn clipboard_view(state: &Ferrite) -> ClipboardView {
    let entries = state.clipboard.entries();
    let first = state.clipboard_first.min(entries.len());
    let end = (first + crate::clipboard::VISIBLE).min(entries.len());
    let items = entries[first..end]
        .iter()
        .enumerate()
        .map(|(offset, entry)| ClipItem {
            index: first + offset,
            pinned: entry.pinned,
            preview: match &entry.content {
                crate::clipboard::Content::Text(text) => ClipPreview::Text(
                    text.lines()
                        .map(|line| line.trim().replace('\t', " "))
                        .filter(|line| !line.is_empty())
                        .take(2)
                        .map(|line| line.chars().take(300).collect())
                        .collect(),
                ),
                crate::clipboard::Content::Image { thumbnail, .. } => ClipPreview::Image(thumbnail.clone()),
            },
        })
        .collect();
    ClipboardView {
        items,
        selected: state.clipboard_selected,
        more: entries.len() - end,
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
