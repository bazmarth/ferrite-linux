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
        Bind, ExportMem, Offscreen,
        damage::OutputDamageTracker,
        element::{
            AsRenderElements, Kind,
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
        gles::{GlesRenderer, GlesTexture, Uniform, element::PixelShaderElement},
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
        apps::{AppIcon, AppKey, LauncherEntry},
        dock::Dock,
        panels::{
            ClipItem, ClipPreview, ClipboardView, DashboardNoteView, DashboardTaskView, DashboardView,
            DesktopView, DockMenuView, LauncherSection, LauncherView, LockNotice, LockView, NotificationView,
            NotificationsView, OverviewTile, OverviewView, Panels, Popover, RegionView, SpaceThumb,
            SwitcherView, Tile, TileIcon,
        },
    },
    state::Ferrite,
    wm::{self, animation::Genie},
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

/// Durée de l'ouverture et de la fermeture de la vue d'ensemble.
const OVERVIEW_TRANSITION: std::time::Duration = std::time::Duration::from_millis(260);
/// Estompe des widgets recouverts par une fenêtre.
const WIDGETS_FADE: std::time::Duration = std::time::Duration::from_millis(200);
const WIDGETS_FADED_ALPHA: f32 = 0.35;

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
            // Fond de la vue d'ensemble, #0F1620 opaque.
            &[0x20, 0x16, 0x0F, 255],
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

    let locked = state.lock.is_some();
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
    if reveal > 0.0 && !locked {
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
            &mut elements,
        );
    }
    let overview_target = if state.overview { 1.0 } else { 0.0 };
    let overview_shown =
        crate::desktop::dock::DockMotion::step(&state.overview_shown, overview_target, OVERVIEW_TRANSITION);
    let view = desktop_view(state);
    assets
        .panels
        .push_element(renderer, output, output_geo, scale, &view, &mut elements);
    let now = std::time::Instant::now();
    let visible_pages = state.workspaces.visible_pages(now);
    // Super+W ou édition : les widgets passent devant les fenêtres.
    let widgets_ahead = state.widgets_front || state.widgets_edit;
    let widgets_visible = !locked && overview_shown == 0.0;
    if widgets_visible && widgets_ahead {
        assets
            .panels
            .push_dashboard(renderer, output, output_geo, scale, &view, 1.0, &mut elements);
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

    if locked {
        // Aucune fenêtre sous l'écran de verrouillage.
    } else if overview_shown > 0.0 {
        push_overview(
            renderer,
            state,
            assets,
            output_geo,
            crate::desktop::dock::ease(overview_shown),
            dark_theme,
            scale,
            &mut elements,
        );
    } else {
        // Fenêtres qui filent vers le Dock ou en reviennent, au-dessus des autres.
        let mut animations = state.window_animations.borrow_mut();
        // « Réduire les animations » : les fenêtres changent de place sans transition.
        animations.retain(|animation| !animation.finished(now) && !state.dock_config.reduce_motion);
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
                let texture = window_texture(renderer, assets, &animation.window, dark_theme, scale)?;
                *animation.snapshot.borrow_mut() = Some(texture.clone());
                Some(texture)
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

    // Sous les fenêtres, les widgets s'estompent dès que l'une d'elles les recouvre.
    if widgets_visible && !widgets_ahead {
        let areas = crate::desktop::panels::widgets_areas(output_geo, &view);
        let covered = state
            .space
            .elements()
            .filter_map(|window| state.window_frame(window))
            .any(|frame| areas.iter().any(|area| frame.overlaps(*area)));
        let duration = if state.dock_config.reduce_motion {
            std::time::Duration::ZERO
        } else {
            WIDGETS_FADE
        };
        let faded = crate::desktop::dock::DockMotion::step(
            &state.widgets_faded,
            if covered { 1.0 } else { 0.0 },
            duration,
        );
        let alpha = 1.0 - (1.0 - WIDGETS_FADED_ALPHA) * crate::desktop::dock::ease(faded) as f32;
        assets
            .panels
            .push_dashboard(renderer, output, output_geo, scale, &view, alpha, &mut elements);
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

#[allow(clippy::too_many_arguments)] // Rendu, fenêtre, géométrie et apparence.
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
        let hover = pointer
            .filter(|_| shift == 0 && titlebar)
            .and_then(|p| wm::decoration::hovered_button(shifted, p));
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

/// Image de `window` dans une texture : barre de titre et coins arrondis compris.
fn window_texture(
    renderer: &mut GlesRenderer,
    assets: &RenderAssets,
    window: &Window,
    dark_theme: bool,
    scale: f64,
) -> Option<GlesTexture> {
    let titlebar = wm::is_server_decorated(window);
    let content_size = window.geometry().size;
    let titlebar_height = if titlebar {
        ferrite_ui::theme::TITLEBAR_HEIGHT as i32
    } else {
        0
    };
    let snapshot_size =
        smithay::utils::Size::<i32, Logical>::from((content_size.w, content_size.h + titlebar_height));
    let content = Rectangle::new(Point::from((0, titlebar_height)), content_size);
    let mut snapshot_elements = Vec::new();
    if titlebar || wm::is_unified(window) {
        push_decorated(
            renderer,
            assets,
            window,
            content,
            (wm::is_activated(window), None),
            titlebar,
            dark_theme,
            scale,
            &mut snapshot_elements,
        );
    } else if let Some(window_surface) = window.wl_surface() {
        let location = (content.loc - window.geometry().loc).to_physical_precise_round(scale);
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
        Ok(texture) => Some(texture),
        Err(err) => {
            warn!(?err, "instantané de la fenêtre indisponible");
            None
        }
    }
}

/// Vue d'ensemble : chaque fenêtre, vivante, glisse de sa place vers son aperçu selon
/// `shown` (0 à 1), sur le fond d'écran assombri. Boutons et icônes sont dans les panneaux.
#[allow(clippy::too_many_arguments)] // Rendu, écran, avancement et apparence.
fn push_overview(
    renderer: &mut GlesRenderer,
    state: &Ferrite,
    assets: &RenderAssets,
    output_geo: Rectangle<i32, Logical>,
    shown: f64,
    dark_theme: bool,
    scale: f64,
    out: &mut Vec<OutputElement>,
) {
    let layout = state.overview_layout(output_geo);
    // Du premier plan vers l'arrière, pour le temps où les fenêtres se croisent.
    for window in state.space.elements().rev() {
        let Some((_, target)) = layout.iter().find(|(other, _)| other == window) else {
            continue;
        };
        let Some(rest) = state.window_frame(window) else {
            continue;
        };
        let rest = rest.to_f64();
        let lerp = |from: f64, to: f64| from + (to - from) * shown;
        let frame = Rectangle::<f64, Logical>::new(
            Point::from((
                lerp(rest.loc.x, target.loc.x) - output_geo.loc.x as f64,
                lerp(rest.loc.y, target.loc.y) - output_geo.loc.y as f64,
            )),
            smithay::utils::Size::from((lerp(rest.size.w, target.size.w), lerp(rest.size.h, target.size.h))),
        );
        let Some(texture) = window_texture(renderer, assets, window, dark_theme, scale) else {
            continue;
        };
        // Un génie sans resserrement : l'image posée telle quelle sur `frame`.
        let shape = Genie {
            window: frame,
            icon: frame,
            top: frame.loc.y,
            bottom: frame.loc.y + frame.size.h,
            squeeze: 0.0,
            alpha: 1.0,
        };
        out.push(OutputElement::Genie(genie::GenieElement::new(
            texture,
            assets.decorations.shaders.genie.clone(),
            &shape,
            scale,
        )));
    }

    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        Point::from((0.0, 0.0)),
        &assets.overview_overlay,
        Some(shown as f32),
        Some(Rectangle::from_size((1.0, 1.0).into())),
        Some(output_geo.size),
        Kind::Unspecified,
    ) {
        Ok(element) => out.push(OutputElement::Memory(element)),
        Err(err) => warn!(?err, output = ?output_geo, "voile de la vue d'ensemble indisponible"),
    }
}

/// Icônes, titre et bouton de fermeture des aperçus, une fois la vue arrivée en place.
fn overview_view(state: &Ferrite) -> Option<OverviewView> {
    if !state.overview || state.overview_shown.get().0 < 1.0 {
        return None;
    }
    let output_geo = state
        .space
        .outputs()
        .next()
        .and_then(|output| state.space.output_geometry(output))?;
    let pointer = state.pointer_location();
    let hit = pointer.and_then(|pointer| state.overview_hit(pointer));
    let tiles = state
        .overview_layout(output_geo)
        .into_iter()
        .map(|(window, frame)| {
            let hovered = hit.as_ref().filter(|(under, _)| *under == window);
            OverviewTile {
                frame: (
                    frame.loc.x.round() as i32,
                    frame.loc.y.round() as i32,
                    frame.size.w.round() as i32,
                    frame.size.h.round() as i32,
                ),
                title: decorations::window_title(&window),
                icon: state
                    .window_app(&window)
                    .map_or(AppIcon::Missing, |key| key.icon(&state.applications)),
                hovered: hovered.is_some(),
                close_hovered: hovered.is_some_and(|(_, close)| *close),
            }
        })
        .collect();
    let pages = state.workspaces.page_count();
    let current = state.workspaces.current();
    let spaces = (0..pages)
        .map(|page| SpaceThumb {
            page,
            windows: if page == current {
                state.space.elements().count()
            } else {
                state.workspaces.parked(page).len()
            },
            current: page == current,
        })
        .collect();
    Some(OverviewView { tiles, spaces })
}

/// Fenêtre décorée par ferrite-shell : menus contextuels, titre, barre de titre, contenu
/// aux coins arrondis, ombre. Sans `titlebar` : coins arrondis partout et ombre seulement.
#[allow(clippy::too_many_arguments)] // Rendu, fenêtre, géométrie et apparence.
fn push_decorated(
    renderer: &mut GlesRenderer,
    assets: &RenderAssets,
    window: &Window,
    content: Rectangle<i32, Logical>,
    look: (bool, Option<wm::decoration::WindowButton>),
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
    let radius = if wm::is_maximized(window) {
        0.0
    } else {
        (WINDOW_RADIUS as f64 * scale) as f32
    };
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
        if self.widgets_edit {
            return self.widget_cursor();
        }
        let Some(pointer) = self.seat.get_pointer().filter(|pointer| !pointer.is_grabbed()) else {
            return CursorShape::Arrow;
        };
        match self.hit_test(pointer.current_location()) {
            Some(wm::Hit::Decoration(_, wm::decoration::Part::Border(edges))) => resize_shape(edges),
            _ => CursorShape::Arrow,
        }
    }

    /// Édition des widgets : le curseur annonce le bord qu'on va saisir.
    fn widget_cursor(&self) -> CursorShape {
        let edges = self.widget_drag.map(|drag| drag.edges).or_else(|| {
            let area = self.screen_area()?;
            let view = desktop_view(self);
            crate::desktop::panels::widget_edges_at(area, &view, self.pointer_location()?)
        });
        match edges {
            Some(edges) if (edges.left && edges.top) || (edges.right && edges.bottom) => {
                CursorShape::DiagonalDown
            }
            Some(edges) if (edges.left && edges.bottom) || (edges.right && edges.top) => {
                CursorShape::DiagonalUp
            }
            Some(edges) if edges.left || edges.right => CursorShape::Horizontal,
            Some(edges) if edges.top || edges.bottom => CursorShape::Vertical,
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
        if self.overview {
            return true;
        }
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
        bar_opaque: !state.dock_config.bar_translucent || state.space.elements().any(wm::is_maximized),
        battery_percent: state.dock_config.battery_percent,
        bluetooth_icon: state.dock_config.bluetooth_icon,
        clock_style: state.dock_config.clock,
        controls: if state.popover == Some(Popover::Controls) {
            crate::desktop::panels::ControlsView {
                page: state.controls.page,
                networks: state.controls.network_rows(),
                devices: state.controls.device_rows(),
                password: state.controls.password_row(),
                scanning: state.controls.scanning,
                current_network: state.controls.current_network().map(str::to_owned),
                connected_devices: state.controls.connected_devices(),
            }
        } else {
            Default::default()
        },
        power_saver: state.power_saver,
        keyboard: state.keyboard_label.clone(),
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
            backdrop: state.wallpaper.glass.clone(),
            layout: state.dashboard.layout.clone(),
            note_shown: state.dashboard.note_shown,
            tasks_scroll: state.dashboard.tasks_scroll,
            shown: !state.overview && state.lock.is_none(),
            front: state.widgets_front,
            edit: state.widgets_edit,
            drag: state.widget_drag.map(|drag| drag.index),
        },
        desktop_menu: state.desktop_menu,
        menu_pointer: state
            .pointer_location()
            .filter(|_| matches!(state.popover, Some(Popover::DockMenu | Popover::DesktopMenu)))
            .map(|location| (location.x as i32, location.y as i32)),
        overview: overview_view(state),
        switcher: switcher_view(state),
        lock: state.lock.as_ref().map(|lock| {
            let name = crate::desktop::lock::user_name();
            let mut letters = name.chars();
            let display = letters
                .next()
                .map(|first| first.to_uppercase().chain(letters).collect::<String>())
                .unwrap_or_else(|| "Utilisateur".to_owned());
            let reduce = state.dock_config.reduce_motion;
            let mut history = state.notifications.history();
            let latest = history.next();
            let notice = latest.map(|latest| {
                let count = 1 + history.count();
                let same_app = state
                    .notifications
                    .history()
                    .all(|notice| notice.app == latest.app);
                LockNotice {
                    app: if same_app {
                        latest.app.clone()
                    } else {
                        "Notifications".to_owned()
                    },
                    icon: latest.icon_file.clone().map(AppIcon::File).or_else(|| {
                        let key = AppKey::for_window(&latest.entry, &state.applications);
                        Some(key.icon(&state.applications))
                            .filter(|icon| same_app && !latest.entry.is_empty() && *icon != AppIcon::Missing)
                    }),
                    count,
                    when: crate::desktop::notifications::age_label(
                        latest.received,
                        std::time::SystemTime::now(),
                    ),
                }
            });
            LockView {
                initial: display.chars().next().unwrap_or('U'),
                user: display,
                length: lock.password_length(),
                field: lock.field_visible,
                progress: lock.progress(reduce),
                checking: lock.checking,
                welcome: lock.is_welcoming(),
                message: lock.message,
                shake: if reduce { 0 } else { lock.shake() },
                pulse: if reduce { 8 } else { lock.pulse() },
                spin: lock.spin(),
                dark: lock.dark,
                wall: lock.wall,
                weather: state.dashboard.weather.clone(),
                place: state.dashboard.place.clone(),
                notice,
                media: lock.media.clone(),
            }
        }),
        notifications: notifications_view(state),
        region: state.region_select.map(|select| RegionView {
            rect: select
                .start
                .zip(state.pointer_location())
                .map(|(start, pointer)| {
                    let zone = capture::region_between(start, pointer);
                    (zone.loc.x, zone.loc.y, zone.size.w, zone.size.h)
                }),
        }),
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

/// Bulles à afficher et, si son panneau est ouvert, l'historique des notifications.
fn notifications_view(state: &Ferrite) -> NotificationsView {
    let now = std::time::SystemTime::now();
    let view = |notice: &crate::desktop::notifications::Notice| NotificationView {
        id: notice.id,
        app: notice.app.clone(),
        summary: notice.summary.clone(),
        body: notice.body.replace('\n', " "),
        when: crate::desktop::notifications::age_label(notice.received, now),
        actions: notice.actions.iter().map(|(_, label)| label.clone()).collect(),
        icon: notice.icon_file.clone().map(AppIcon::File).or_else(|| {
            let key = AppKey::for_window(&notice.entry, &state.applications);
            Some(key.icon(&state.applications))
                .filter(|icon| !notice.entry.is_empty() && *icon != AppIcon::Missing)
        }),
    };
    NotificationsView {
        bubbles: if state.overview {
            Vec::new()
        } else {
            state.notifications.bubbles().map(view).collect()
        },
        history: if state.popover == Some(Popover::Notifications) {
            state.notifications.history().map(view).collect()
        } else {
            Vec::new()
        },
        do_not_disturb: state.notifications.do_not_disturb,
    }
}

/// Icônes et titre du sélecteur Alt+Tab.
fn switcher_view(state: &Ferrite) -> Option<SwitcherView> {
    let switcher = state.switcher.as_ref()?;
    Some(SwitcherView {
        icons: switcher
            .windows
            .iter()
            .map(|window| {
                state
                    .window_app(window)
                    .map_or(AppIcon::Missing, |key| key.icon(&state.applications))
            })
            .collect(),
        selected: switcher.selected,
        title: switcher
            .windows
            .get(switcher.selected)
            .map(decorations::window_title)
            .unwrap_or_default(),
    })
}

/// Fait la capture demandée, s'il y en a une : l'écran est redessiné à part, sans le curseur
/// ni le sélecteur de zone, puis enregistré en arrière-plan.
pub fn take_screenshot(
    renderer: &mut GlesRenderer,
    state: &mut Ferrite,
    assets: &RenderAssets,
    output: &Output,
) {
    let Some(request) = state.screenshot_request.take() else {
        return;
    };
    if let Err(err) = render_screenshot(renderer, state, assets, output, request) {
        warn!("capture d'écran impossible : {err}");
    }
}

fn render_screenshot(
    renderer: &mut GlesRenderer,
    state: &Ferrite,
    assets: &RenderAssets,
    output: &Output,
    request: capture::Request,
) -> Result<(), String> {
    let size = output.current_mode().ok_or("écran sans mode")?.size;
    let buffer_size = size.to_logical(1).to_buffer(1, smithay::utils::Transform::Normal);
    let mut texture: GlesTexture = renderer
        .create_buffer(Fourcc::Abgr8888, buffer_size)
        .map_err(|err| format!("{err:?}"))?;
    let mut framebuffer = renderer.bind(&mut texture).map_err(|err| format!("{err:?}"))?;
    let elements = output_elements(renderer, state, assets, output, false);
    OutputDamageTracker::from_output(output)
        .render_output(renderer, &mut framebuffer, 0, &elements, [0.0, 0.0, 0.0, 1.0])
        .map_err(|err| format!("{err:?}"))?;
    let mapping = renderer
        .copy_framebuffer(&framebuffer, Rectangle::from_size(buffer_size), Fourcc::Abgr8888)
        .map_err(|err| format!("{err:?}"))?;
    let pixels = renderer.map_texture(&mapping).map_err(|err| format!("{err:?}"))?;
    let (width, height) = (size.w as u32, size.h as u32);
    // Zone logique convertie en pixels de l'écran (tout l'écran sans zone).
    let zone = request.region.map_or((0, 0, width, height), |region| {
        let scale = output.current_scale().fractional_scale();
        let physical = |value: i32| (value.max(0) as f64 * scale).round() as u32;
        (
            physical(region.loc.x),
            physical(region.loc.y),
            physical(region.size.w),
            physical(region.size.h),
        )
    });
    // L'écran en fenêtre est retourné (voir `backend::winit`), pas celui en DRM.
    let flip = output.current_transform() == smithay::utils::Transform::Flipped180;
    let (image, width, height) = capture::crop(pixels, width, height, flip, zone);
    let events = state.notification_link.as_ref().map(|link| link.events.clone());
    capture::save_in_background(image, width, height, move |result| {
        use crate::desktop::notifications::{Event, local_notice};
        let notice = match result {
            Ok(path) => {
                tracing::info!("capture d'écran enregistrée : {}", path.display());
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
                local_notice("Capture enregistrée", &name.unwrap_or_default())
            }
            Err(err) => {
                warn!("capture d'écran impossible : {err}");
                local_notice("Capture impossible", &err)
            }
        };
        if let Some(events) = events {
            let _ = events.send(Event::Notify(notice));
        }
    });
    Ok(())
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

/// Toutes les applications du lanceur, de A à Z.
fn launcher_view(state: &Ferrite) -> LauncherView {
    let entries = state.launcher_entries();
    let title = if entries.is_empty() { "Aucun résultat" } else { "" };
    LauncherView {
        query: state.launcher_query.clone(),
        sections: vec![LauncherSection {
            title: title.to_owned(),
            tiles: entries
                .iter()
                .enumerate()
                .map(|(index, entry)| launcher_tile(index, entry))
                .collect(),
        }],
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
