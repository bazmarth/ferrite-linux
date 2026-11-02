//! Rendu d'un écran : fenêtres (contenu, décorations, ombres) puis fond d'écran.
//! Fond d'écran, fenêtres, animations et panneaux du bureau.

pub mod capture;
mod cursor;
mod decorations;
mod genie;
mod rounded;
mod shaders;
mod title;
mod wallpaper;

use ferrite_system::config::DockHiding;
use ferrite_ui::theme::WINDOW_RADIUS;
use smithay::wayland::{compositor::with_states, seat::WaylandFocus, shell::xdg::XdgToplevelSurfaceData};
use smithay::{
    backend::allocator::Fourcc,
    backend::renderer::{
        element::{
            AsRenderElements, Kind,
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
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
        apps::LauncherEntry,
        dock::Dock,
        panels::{
            ClipItem, ClipPreview, ClipboardView, DashboardNoteView, DashboardTaskView, DashboardView,
            DesktopView, DockMenuView, LauncherSection, LauncherView, Panels, Popover, SectionLink, Tile,
            TileIcon,
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
    Genie = genie::GenieElement,
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
    overview_overlay: MemoryRenderBuffer,
}

impl RenderAssets {
    pub fn new(renderer: &mut GlesRenderer) -> Result<RenderAssets, String> {
        let overview_overlay = MemoryRenderBuffer::from_slice(
            &[0, 0, 0, 112],
            Fourcc::Argb8888,
            (1, 1),
            1,
            smithay::utils::Transform::Normal,
            None,
        );
        Ok(RenderAssets {
            wallpaper: Wallpaper::default(),
            decorations: Decorations::new(renderer)?,
            cursor: Cursor::default(),
            dock: Dock::new(),
            panels: Panels::new()?,
            overview_overlay,
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

    let items = state.dock_items();
    let dock_shown = state.dock_shown(output_geo);
    let dark_theme = state.appearance.is_dark();
    let (dock_layout, reveal) = state.dock_motion.frame_layout(
        output_geo,
        state.dock_config,
        &items,
        dock_shown,
        state.dock_pointer(),
    );
    if reveal > 0.0 {
        let bounces = state.dock_bounce_heights(&items);
        assets.dock.push_elements(
            renderer,
            &assets.decorations.shaders.dock,
            output,
            output_geo,
            scale,
            &items,
            state.dock_config,
            &dock_layout,
            &bounces,
            dark_theme,
            &mut elements,
        );
    }
    let view = desktop_view(state);
    assets
        .panels
        .push_element(renderer, output, output_geo, scale, &view, &mut elements);
    let now = std::time::Instant::now();
    let visible_pages = state.workspaces.visible_pages(now);
    for (page, offset) in &visible_pages {
        if !state.overview && *page == crate::wm::workspaces::WIDGETS {
            let shift = (*offset * output_geo.size.w as f64).round() as i32;
            assets
                .panels
                .push_dashboard(renderer, output, output_geo, scale, &view, shift, &mut elements);
        }
    }

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

    if state.overview {
        push_overview(renderer, state, assets, output_geo, scale, &mut elements);
    } else {
        // Fenêtres qui filent vers le Dock ou en reviennent, au-dessus des autres.
        let mut animations = state.window_animations.borrow_mut();
        animations.retain(|animation| !animation.finished(now));
        for animation in animations.iter() {
            let frame = Rectangle::new(
                animation.window_rect.loc - output_geo.loc.to_f64(),
                animation.window_rect.size,
            );
            let target = Rectangle::new(
                animation.dock_rect.loc - output_geo.loc.to_f64(),
                animation.dock_rect.size,
            );
            let shape = animation.genie(frame, target, now);
            let cached = animation.snapshot.borrow().clone();
            let texture = cached.or_else(|| {
                let titlebar = wm::is_server_decorated(&animation.window);
                let content_size = animation.window.geometry().size;
                let titlebar_height = if titlebar {
                    ferrite_ui::theme::TITLEBAR_HEIGHT as i32
                } else {
                    0
                };
                let snapshot_size = smithay::utils::Size::<i32, Logical>::from((
                    content_size.w,
                    content_size.h + titlebar_height,
                ));
                let content = Rectangle::new(Point::from((0, titlebar_height)), content_size);
                let mut snapshot_elements = Vec::new();
                if wm::is_server_decorated(&animation.window) || wm::is_unified(&animation.window) {
                    push_decorated(
                        renderer,
                        assets,
                        &animation.window,
                        content,
                        (wm::is_activated(&animation.window), false),
                        titlebar,
                        dark_theme,
                        scale,
                        &mut snapshot_elements,
                    );
                } else if let Some(window_surface) = animation.window.wl_surface() {
                    let location =
                        (content.loc - animation.window.geometry().loc).to_physical_precise_round(scale);
                    snapshot_elements.extend(
                        render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<GlesRenderer>>(
                            renderer,
                            &window_surface,
                            location,
                            scale,
                            1.0,
                            Kind::Unspecified,
                        )
                        .into_iter()
                        .map(OutputElement::Surface),
                    );
                }
                let physical = snapshot_size.to_f64().to_physical(scale);
                match genie::snapshot(
                    renderer,
                    &snapshot_elements,
                    smithay::utils::Size::from((physical.w.round() as i32, physical.h.round() as i32)),
                    scale,
                ) {
                    Ok(texture) => {
                        *animation.snapshot.borrow_mut() = Some(texture.clone());
                        Some(texture)
                    }
                    Err(err) => {
                        warn!(?err, "instantané de l'animation Genie indisponible");
                        None
                    }
                }
            });
            if let Some(texture) = texture {
                elements.push(OutputElement::Genie(genie::GenieElement::new(
                    texture,
                    assets.decorations.shaders.genie.clone(),
                    &shape,
                    scale,
                )));
            } else {
                push_animation_fallback(renderer, animation, &shape, scale, &mut elements);
            }
        }
        let animated: Vec<Window> = animations
            .iter()
            .map(|animation| animation.window.clone())
            .collect();
        drop(animations);

        // Fenêtres du bureau courant et du bureau voisin visibles pendant le glissement.
        for (page, offset) in &visible_pages {
            if *page == crate::wm::workspaces::WIDGETS {
                continue;
            }
            let shift = (*offset * output_geo.size.w as f64).round() as i32;
            if *page == state.workspaces.current() {
                for (index, window) in state.space.elements().rev().enumerate() {
                    if index == 1 {
                        elements.extend(snap_preview.take());
                    }
                    if animated.contains(window) {
                        continue;
                    }
                    if let Some(content) = state.space.element_geometry(window) {
                        push_window(
                            renderer,
                            assets,
                            window,
                            content,
                            output_geo,
                            shift,
                            pointer,
                            dark_theme,
                            scale,
                            &mut elements,
                        );
                    }
                }
            } else {
                for parked in state.workspaces.parked(*page).iter().rev() {
                    push_window(
                        renderer,
                        assets,
                        &parked.window,
                        parked.content(),
                        output_geo,
                        shift,
                        None,
                        dark_theme,
                        scale,
                        &mut elements,
                    );
                }
            }
        }
        elements.extend(snap_preview);
    }

    // Fond à la résolution physique de l'écran : net à toute échelle.
    let physical = output_geo.size.to_f64().to_physical(scale);
    let (wallpaper, source) = assets.wallpaper.buffer(
        &state.wallpaper,
        physical.w.round() as i32,
        physical.h.round() as i32,
    );
    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        Point::from((0.0, 0.0)),
        &wallpaper,
        None,
        // Toute la texture, à la taille de l'écran (sans `src`, Smithay prendrait la
        // taille de l'écran comme zone source).
        Some(source),
        Some(output_geo.size),
        Kind::Unspecified,
    ) {
        Ok(element) => elements.push(OutputElement::Memory(element)),
        Err(err) => warn!("fond d'écran non importé : {err:?}"),
    }
    elements
}

fn push_animation_fallback(
    renderer: &mut GlesRenderer,
    animation: &wm::animation::WindowAnimation,
    shape: &wm::animation::Genie,
    scale: f64,
    out: &mut Vec<OutputElement>,
) {
    let bounds = shape.bounds();
    let factor = bounds.size.w / animation.window_rect.size.w.max(1.0);
    if !factor.is_finite() || factor <= 0.0 {
        return;
    }

    let geometry = animation.window.geometry();
    let titlebar = if wm::is_server_decorated(&animation.window) {
        ferrite_ui::theme::TITLEBAR_HEIGHT as f64
    } else {
        0.0
    };
    let location = Point::<f64, Logical>::from((
        bounds.loc.x - geometry.loc.x as f64 * factor,
        bounds.loc.y + titlebar * factor - geometry.loc.y as f64 * factor,
    ))
    .to_physical_precise_round(scale);
    out.extend(
        animation
            .window
            .render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                renderer,
                location,
                (scale * factor).into(),
                shape.alpha as f32,
            )
            .into_iter()
            .map(OutputElement::Surface),
    );
}

fn push_window(
    renderer: &mut GlesRenderer,
    assets: &RenderAssets,
    window: &Window,
    content: Rectangle<i32, Logical>,
    output_geo: Rectangle<i32, Logical>,
    shift: i32,
    pointer: Option<Point<f64, Logical>>,
    dark_theme: bool,
    scale: f64,
    out: &mut Vec<OutputElement>,
) {
    let shifted = Rectangle::new(content.loc + Point::from((shift, 0)), content.size);
    let local = Rectangle::new(shifted.loc - output_geo.loc, shifted.size);
    let titlebar = wm::is_server_decorated(window);
    if titlebar || wm::is_unified(window) {
        let hover =
            shift == 0 && titlebar && pointer.is_some_and(|p| wm::decoration::lights_hovered(shifted, p));
        push_decorated(
            renderer,
            assets,
            window,
            local,
            (wm::is_activated(window), hover),
            titlebar,
            dark_theme,
            scale,
            out,
        );
    } else {
        let location = (local.loc - window.geometry().loc).to_physical_precise_round(scale);
        out.extend(
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

fn push_overview(
    renderer: &mut GlesRenderer,
    state: &Ferrite,
    assets: &RenderAssets,
    output_geo: Rectangle<i32, Logical>,
    scale: f64,
    out: &mut Vec<OutputElement>,
) {
    let windows: Vec<Window> = state.space.elements().rev().cloned().collect();
    let tiles = crate::wm::workspaces::overview_tiles(output_geo, windows.len());
    for (window, tile) in windows.iter().zip(tiles) {
        let Some(content) = state.space.element_geometry(window) else {
            continue;
        };
        let frame = if wm::is_server_decorated(window) {
            wm::decoration::frame(content)
        } else {
            content
        };
        let factor = ((tile.size.w - 24).max(1) as f64 / frame.size.w.max(1) as f64)
            .min((tile.size.h - 24).max(1) as f64 / frame.size.h.max(1) as f64)
            .min(1.0);
        let frame_x = tile.loc.x as f64 + (tile.size.w as f64 - frame.size.w as f64 * factor) / 2.0;
        let frame_y = tile.loc.y as f64 + (tile.size.h as f64 - frame.size.h as f64 * factor) / 2.0;
        let content_origin = Point::<f64, Logical>::from((
            frame_x + (content.loc.x - frame.loc.x) as f64 * factor,
            frame_y + (content.loc.y - frame.loc.y) as f64 * factor,
        ));
        let geometry_origin = window.geometry().loc.to_f64();
        let surface_origin = Point::<f64, Logical>::from((
            content_origin.x - geometry_origin.x * factor - output_geo.loc.x as f64,
            content_origin.y - geometry_origin.y * factor - output_geo.loc.y as f64,
        ));
        out.extend(
            window
                .render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                    renderer,
                    surface_origin.to_physical_precise_round(scale),
                    (scale * factor).into(),
                    1.0,
                )
                .into_iter()
                .map(OutputElement::Surface),
        );
    }

    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        Point::from((0.0, 0.0)),
        &assets.overview_overlay,
        None,
        Some(Rectangle::from_size((1.0, 1.0).into())),
        Some(output_geo.size),
        Kind::Unspecified,
    ) {
        Ok(element) => out.push(OutputElement::Memory(element)),
        Err(err) => warn!(?err, output = ?output_geo, "voile de la vue d'ensemble indisponible"),
    }
}

/// Fenêtre décorée par ferrite-shell : menus contextuels, titre, barre de titre, contenu
/// aux coins arrondis, ombre. Sans `titlebar` : coins arrondis partout et ombre seulement.
#[allow(clippy::too_many_arguments)] // Rendu, fenêtre, géométrie et apparence.
fn push_decorated(
    renderer: &mut GlesRenderer,
    assets: &RenderAssets,
    window: &Window,
    content: Rectangle<i32, Logical>,
    look: (bool, bool),
    titlebar: bool,
    dark_theme: bool,
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
    assets.decorations.push_elements(
        renderer,
        window,
        content,
        look,
        titlebar,
        dark_theme,
        scale,
        &mut decorations,
    );
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
        .map(|element| {
            OutputElement::Rounded(RoundedSurface::new(
                element,
                program.clone(),
                clip,
                radius,
                !titlebar,
            ))
        }),
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

    pub fn pointer_location(&self) -> Option<Point<f64, Logical>> {
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
        let items = self.dock_items();
        let dock = crate::desktop::dock::layout(output, self.dock_config, &items, 1.0, None, 0.0).plate;
        let pointer = self.pointer_location();
        let pointer_over_dock = pointer.is_some_and(|pointer| dock.contains(pointer))
            || self
                .dock_fast_approach_until
                .get()
                .is_some_and(|until| std::time::Instant::now() < until);
        crate::desktop::dock::should_show(crate::desktop::dock::Visibility {
            config: self.dock_config,
            output,
            dock,
            pointer,
            pointer_over_dock,
            windows: &frames,
            fullscreen: self.top_fullscreen(),
            launcher_open: matches!(self.popover, Some(Popover::Launcher | Popover::DockMenu)),
        })
    }

    /// La barre du haut est-elle affichée ? Masquée (plein écran ou réglage de
    /// Paramètres), elle revient quand la souris touche le bord du haut, et reste tant
    /// qu'elle est dessus ou qu'un menu est ouvert. Il faut toucher le bord lui-même :
    /// la barre de titre d'une fenêtre agrandie, juste dessous, reste accessible.
    pub fn bar_visible(&self) -> bool {
        let height = self.dock_config.bar_height as f64;
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
    let dark_theme = state.appearance.is_dark();
    Panels::set_theme(dark_theme, state.accent);
    DesktopView {
        dark_theme,
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
        bar_dark: state.wallpaper.bar_dark,
        bar_height: state.dock_config.bar_height,
        page: state.workspaces.current(),
        page_count: state.workspaces.page_count(),
        dock_height: state.dock_config.shown_height(),
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
        dashboard: DashboardView {
            notes: state
                .dashboard
                .notes
                .iter()
                .map(|note| DashboardNoteView {
                    title: note.title().to_owned(),
                    preview: note.preview(),
                    updated: note.updated,
                })
                .collect(),
            tasks: state
                .dashboard
                .tasks
                .iter()
                .map(|task| DashboardTaskView {
                    text: task.text.clone(),
                    done: task.done,
                    priority: task.priority,
                })
                .collect(),
            draft: state.dashboard.draft.clone(),
            editing: state.dashboard_editing,
            month_offset: state.dashboard.month_offset,
            weather: state.dashboard.weather.clone(),
            place: state.dashboard.place.clone(),
        },
        dock_menu: match (&state.dock_menu, state.popover) {
            (Some((_, actions, anchor)), Some(Popover::DockMenu)) => Some(DockMenuView {
                x: anchor.x,
                bottom: anchor.y,
                entries: actions.iter().map(|action| action.label().to_owned()).collect(),
            }),
            _ => None,
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
        position: state.clipboard_position,
    }
}

/// Rubriques visibles du menu Applications.
fn launcher_view(state: &Ferrite) -> LauncherView {
    let entries = state.launcher_entries();
    let tiles = |indices: Vec<usize>| {
        indices
            .into_iter()
            .map(|index| launcher_tile(index, &entries[index]))
            .collect::<Vec<_>>()
    };
    let mut sections = Vec::new();
    if let Some(section) = state.launcher_section {
        let indices = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (entry.section() == section).then_some(index))
            .collect();
        sections.push(LauncherSection {
            title: section.title().to_owned(),
            tiles: tiles(indices),
            link: Some(SectionLink::Less),
        });
    } else if !state.launcher_query.trim().is_empty() {
        sections.push(LauncherSection {
            title: if entries.is_empty() {
                "Aucun résultat".to_owned()
            } else {
                "Résultats".to_owned()
            },
            tiles: tiles((0..entries.len()).collect()),
            link: None,
        });
    } else {
        let recent = state
            .launcher_recent
            .iter()
            .filter_map(|key| entries.iter().position(|entry| entry.key() == key.clone()))
            .take(10)
            .collect::<Vec<_>>();
        if !recent.is_empty() {
            sections.push(LauncherSection {
                title: "Récents".to_owned(),
                tiles: tiles(recent),
                link: None,
            });
        }
        for section in crate::desktop::apps::Section::ALL {
            let indices = entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| (entry.section() == section).then_some(index))
                .collect::<Vec<_>>();
            if indices.is_empty() {
                continue;
            }
            let link = (indices.len() > 5).then_some(SectionLink::More(section));
            sections.push(LauncherSection {
                title: section.title().to_owned(),
                tiles: tiles(indices.into_iter().take(5).collect()),
                link,
            });
        }
    }
    LauncherView {
        query: state.launcher_query.clone(),
        sections,
        scroll: state.launcher_scroll,
        backdrop: state.wallpaper.backdrop.clone(),
    }
}

fn launcher_tile(index: usize, entry: &LauncherEntry<'_>) -> Tile {
    let (name, icon) = match entry {
        LauncherEntry::Builtin(app) => (app.name().to_owned(), TileIcon::Builtin(app.icon())),
        LauncherEntry::Installed(entry) => (
            entry.name.clone(),
            entry.icon.clone().map_or(TileIcon::Missing, TileIcon::File),
        ),
    };
    Tile { name, icon, index }
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
