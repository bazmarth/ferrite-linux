//! Rendu d'un écran : fenêtres (contenu, décorations, ombres) puis fond d'écran.
//! Fond d'écran, fenêtres, animations et panneaux du bureau.

pub mod capture;
mod cursor;
mod decorations;
mod genie;
mod ghost;
pub mod profile;
mod rounded;
mod shaders;
mod title;
mod wallpaper;

use ferrite_system::config::DockHiding;
use ferrite_ui::theme::WINDOW_RADIUS;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::shell::wlr_layer::Layer;
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
    cursor::{Cursor, CursorShape, ThemeCursors},
    decorations::Decorations,
    wallpaper::Wallpaper,
};
use crate::{
    desktop::{
        apps::{AppIcon, AppKey, LauncherEntry},
        dock::Dock,
        panels::{
            ClipRow, ClipboardView, DashboardNoteView, DashboardTaskView, DashboardView, DesktopView,
            DockMenuView, LauncherSection, LauncherView, LockNotice, LockView, NotificationView,
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
const OVERVIEW_TRANSITION: std::time::Duration = ferrite_ui::tokens::motion::NORMAL;
/// Estompe des widgets recouverts par une fenêtre.
const WIDGETS_FADE: std::time::Duration = ferrite_ui::tokens::motion::SHORT;
const WIDGETS_FADED_ALPHA: f32 = 0.35;

/// Ressources de rendu créées avec le moteur de rendu.
#[derive(Debug)]
pub struct RenderAssets {
    pub wallpaper: Wallpaper,
    /// Fond du second écran en mode « étendre » (autre taille : cache à part).
    pub second_wallpaper: Wallpaper,
    pub decorations: Decorations,
    pub cursor: Cursor,
    /// Image des fichiers glissés sous le pointeur.
    pub ghost: ghost::Ghost,
    pub dock: Dock,
    pub panels: Panels,
    /// Fond de la vue d'ensemble : clair puis sombre, selon le thème.
    overview_overlay: [MemoryRenderBuffer; 2],
    screen_fade_overlay: MemoryRenderBuffer,
}

impl RenderAssets {
    pub fn new(renderer: &mut GlesRenderer) -> Result<RenderAssets, String> {
        // Fond de la vue d'ensemble, opaque : #E8ECF1 en thème clair, #0F1620 en sombre.
        let flat = |bgra: [u8; 4]| {
            MemoryRenderBuffer::from_slice(
                &bgra,
                Fourcc::Argb8888,
                (1, 1),
                1,
                smithay::utils::Transform::Normal,
                None,
            )
        };
        let overview_overlay = [flat([0xF1, 0xEC, 0xE8, 255]), flat([0x20, 0x16, 0x0F, 255])];
        let screen_fade_overlay = MemoryRenderBuffer::from_slice(
            &[0, 0, 0, 255],
            Fourcc::Argb8888,
            (1, 1),
            1,
            smithay::utils::Transform::Normal,
            None,
        );
        Ok(RenderAssets {
            wallpaper: Wallpaper::default(),
            second_wallpaper: Wallpaper::default(),
            decorations: Decorations::new(renderer)?,
            cursor: Cursor::default(),
            ghost: ghost::Ghost::default(),
            dock: Dock::new(),
            panels: Panels::new()?,
            overview_overlay,
            screen_fade_overlay,
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
    let start = std::time::Instant::now();
    let elements = profile::timed("image", || {
        build_output_elements(renderer, state, assets, output, draw_cursor)
    });
    profile::frame(start.elapsed());
    elements
}

fn build_output_elements(
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
                let image =
                    assets
                        .cursor
                        .image(shape, scale, state.cursor_size, state.cursor_theme.as_deref());
                push_cursor(renderer, &image, location, scale, &mut elements);
            }
        }
    }

    // Fichiers glissés, sous le curseur et au-dessus de tout le reste.
    if let Some(file) = state.file_drag.as_ref().filter(|file| file.shown())
        && let Some(pointer) = state.seat.get_pointer()
    {
        let location = pointer.current_location() - output_geo.loc.to_f64();
        assets.ghost.push(
            renderer,
            &file.drag.icon,
            file.drag.paths.len(),
            location,
            scale,
            &mut elements,
        );
    }
    // Tout ce qui précède reste net pendant les passages ; le reste est la scène zoomée.
    let overlays = elements.len();

    // Verrouillé par un client : ses surfaces seules, sur le fond noir de l'écran.
    if let Some(lock) = &state.session_lock {
        for (lock_output, surface) in &lock.surfaces {
            if lock_output == output {
                push_surface_tree(
                    renderer,
                    surface.wl_surface(),
                    Point::from((0, 0)),
                    scale,
                    &mut elements,
                );
            }
        }
        push_screen_fade(renderer, state, assets, output_geo, &mut elements);
        return elements;
    }

    let locked = state.lock.is_some();
    // Fenêtres de candidats des méthodes de saisie, puis calques « superposition ».
    if !locked {
        for popup in &state.input_popups {
            let parent = popup
                .get_parent()
                .map(|parent| state.input_popup_parent(&parent.surface))
                .unwrap_or_default();
            let location = (parent.loc + popup.location() - output_geo.loc).to_physical_precise_round(scale);
            push_surface_tree(renderer, popup.wl_surface(), location, scale, &mut elements);
        }
        push_layers(renderer, output, Layer::Overlay, scale, &mut elements);
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
    if reveal > 0.0 && !locked {
        let bounces = state.dock_bounce_heights(&items);
        profile::timed("dock", || {
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
            )
        });
    }
    let overview_target = if state.overview { 1.0 } else { 0.0 };
    let overview_shown =
        crate::desktop::dock::DockMotion::step(&state.overview_shown, overview_target, OVERVIEW_TRANSITION);
    // Lanceur fermé : ses icônes sont préparées d'avance, dans le fil « icônes ». La liste
    // des applications n'est triée qu'une fois par image.
    let closed_launcher = (state.popover != Some(crate::desktop::panels::Popover::Launcher))
        .then(|| state.launcher_entries().len());
    if let Some(entries) = closed_launcher
        && state.launcher_query.is_empty()
        && assets.panels.launcher_icons_pending(entries)
    {
        let launcher = launcher_view(state);
        let tiles = launcher
            .sections
            .first()
            .map_or(&[][..], |section| &section.tiles[..]);
        assets.panels.warm_launcher_icons(tiles, scale);
    }
    let view = profile::timed("vue du bureau", || desktop_view(state));
    // Puis, les icônes prêtes, son ombre et son verre (une fois, ~45 ms).
    if let Some(entries) = closed_launcher
        && !assets.panels.launcher_icons_pending(entries)
    {
        assets.panels.warm_launcher_backdrop(output_geo, scale, &view);
    }
    profile::timed("panneaux", || {
        assets
            .panels
            .push_element(renderer, output, output_geo, scale, &view, &mut elements)
    });
    let now = std::time::Instant::now();
    let visible_pages = state.workspaces.visible_pages(now);
    // Super+W ou édition : les widgets passent devant les fenêtres.
    let widgets_ahead = state.widgets_front || state.widgets_edit;
    // Les widgets vivent sur le bureau 0 : ils s'effacent à mesure qu'on le quitte.
    let widgets_page = visible_pages
        .iter()
        .find(|(page, _)| *page == 0)
        .map(|(_, offset)| (1.0 - offset.abs()).clamp(0.0, 1.0) as f32);
    let widgets_visible = !locked && overview_shown == 0.0 && widgets_page.is_some();
    let widgets_page = widgets_page.unwrap_or(0.0);
    if widgets_visible && widgets_ahead {
        assets.panels.push_dashboard(
            renderer,
            output,
            output_geo,
            scale,
            &view,
            widgets_page,
            &mut elements,
        );
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
    if !locked {
        push_layers(renderer, output, Layer::Top, scale, &mut elements);
    }

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
        animations.retain(|animation| {
            animation.advance(now);
            !animation.finished() && !state.dock_config.reduce_motion
        });
        for animation in animations.iter() {
            let frame = Rectangle::new(
                animation.window_rect.loc - output_geo.loc.to_f64(),
                animation.window_rect.size,
            );
            // Vers le Dock ou depuis lui : l'icône est suivie à chaque image, car le Dock
            // glisse et ses icônes changent de taille sous la souris.
            let dock_rect = match animation.motion {
                wm::animation::Motion::Minimize | wm::animation::Motion::Restore => state
                    .dock_icon_rect(state.window_app(&animation.window).as_ref())
                    .unwrap_or(animation.dock_rect),
                _ => animation.dock_rect,
            };
            let target = Rectangle::new(dock_rect.loc - output_geo.loc.to_f64(), dock_rect.size);
            let shape = animation.genie(frame, target);
            let cached = animation.snapshot.borrow().clone();
            let texture = cached.or_else(|| {
                let texture = window_texture(renderer, assets, &animation.window, dark_theme, scale)?;
                *animation.snapshot.borrow_mut() = Some(texture.clone());
                Some(texture)
            });
            // Changement de cadre : l'image suit le cadre qui glisse et s'étire, coins arrondis
            // redessinés à sa taille ; l'ancien et le nouveau contenu y sont posés tous deux,
            // d'où un fondu enchaîné sans trou ni recadrage.
            let radius =
                if animation.motion == wm::animation::Motion::Zoom && !wm::is_maximized(&animation.window) {
                    f64::from(ferrite_ui::theme::WINDOW_RADIUS)
                } else {
                    0.0
                };
            // Changement de taille : le nouveau contenu, dès qu'il existe, en fondu au-dessus.
            if animation.wants_next()
                && let Some(next) = window_texture(renderer, assets, &animation.window, dark_theme, scale)
            {
                animation.set_next(next);
            }
            let next = animation.next.borrow().clone();
            if let Some(next) = next {
                let faded = wm::animation::Genie {
                    alpha: shape.alpha * animation.next_alpha(),
                    ..shape
                };
                elements.push(OutputElement::Genie(genie::GenieElement::new(
                    next,
                    assets.decorations.shaders.genie.clone(),
                    &faded,
                    scale,
                    radius,
                )));
            }
            if let Some(texture) = texture {
                elements.push(OutputElement::Genie(genie::GenieElement::new(
                    texture,
                    assets.decorations.shaders.genie.clone(),
                    &shape,
                    scale,
                    radius,
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
        let alpha =
            (1.0 - (1.0 - WIDGETS_FADED_ALPHA) * crate::desktop::dock::ease(faded) as f32) * widgets_page;
        profile::timed("widgets", || {
            assets
                .panels
                .push_dashboard(renderer, output, output_geo, scale, &view, alpha, &mut elements)
        });
    }

    // Raccourcis du bureau : sous les fenêtres, sur tous les bureaux sauf celui des widgets,
    // d'où ils s'effacent à mesure qu'on s'en approche.
    if !locked && overview_shown == 0.0 {
        let alpha = 1.0 - widgets_page;
        profile::timed("raccourcis", || {
            assets
                .panels
                .push_shortcuts(renderer, output, output_geo, scale, &view, alpha, &mut elements)
        });
    }

    if !locked {
        push_layers(renderer, output, Layer::Bottom, scale, &mut elements);
        push_layers(renderer, output, Layer::Background, scale, &mut elements);
    }

    // Fond à la résolution physique de l'écran : net à toute échelle.
    let physical = output_geo.size.to_f64().to_physical(scale);
    let (wallpaper, source) = profile::timed("fond d'écran", || {
        assets.wallpaper.buffer(
            &state.wallpaper,
            physical.w.round() as i32,
            physical.h.round() as i32,
            state.redraw.as_ref(),
        )
    });
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
    push_screen_fade(renderer, state, assets, output_geo, &mut elements);
    push_screen_swap(
        renderer,
        state,
        assets,
        output,
        output_geo,
        scale,
        overlays,
        &mut elements,
    );
    elements
}

/// Second écran en mode « étendre » : pointeur, fenêtres du bureau courant qui le
/// touchent, et fond d'écran. La barre, le Dock, les menus et les widgets restent sur
/// l'écran principal ; verrouillé ou écran éteint, il reste noir.
pub fn extended_elements(
    renderer: &mut GlesRenderer,
    state: &Ferrite,
    assets: &RenderAssets,
    output: &Output,
) -> Vec<OutputElement> {
    let mut elements = Vec::new();
    let Some(output_geo) = state.space.output_geometry(output) else {
        return elements;
    };
    if state.lock.is_some() || state.session_lock.is_some() {
        return elements;
    }
    let scale = output.current_scale().fractional_scale();
    let pointer = state.seat.get_pointer();
    if !matches!(state.cursor_status, CursorImageStatus::Hidden)
        && let Some(pointer) = &pointer
    {
        let location = pointer.current_location() - output_geo.loc.to_f64();
        if Rectangle::from_size(output_geo.size).to_f64().contains(location) {
            let image = assets.cursor.image(
                state.cursor_shape(),
                scale,
                state.cursor_size,
                state.cursor_theme.as_deref(),
            );
            push_cursor(renderer, &image, location, scale, &mut elements);
        }
    }
    let hover = pointer
        .filter(|pointer| !pointer.is_grabbed())
        .map(|pointer| pointer.current_location());
    let dark_theme = state.appearance.is_dark();
    for window in state.space.elements().rev() {
        let Some(content) = state.space.element_geometry(window) else {
            continue;
        };
        let frame = state.window_frame(window).unwrap_or(content);
        if !frame.overlaps(output_geo) {
            continue;
        }
        push_window(
            renderer,
            assets,
            window,
            content,
            output_geo,
            0,
            hover,
            dark_theme,
            scale,
            &mut elements,
        );
    }
    let physical = output_geo.size.to_f64().to_physical(scale);
    let (wallpaper, source) = assets.second_wallpaper.buffer(
        &state.wallpaper,
        physical.w.round() as i32,
        physical.h.round() as i32,
        state.redraw.as_ref(),
    );
    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        Point::from((0.0, 0.0)),
        &wallpaper,
        None,
        Some(source),
        Some(output_geo.size),
        Kind::Unspecified,
    ) {
        Ok(element) => elements.push(OutputElement::Memory(element)),
        Err(err) => warn!("fond du second écran non importé : {err:?}"),
    }
    elements
}

/// Passage entre le bureau et l'écran verrouillé : la scène (tout sauf le pointeur) est
/// dessinée dans une image, posée zoomée sur fond noir, sous la photo de l'ancien écran qui
/// s'efface en zoomant.
#[allow(clippy::too_many_arguments)]
fn push_screen_swap(
    renderer: &mut GlesRenderer,
    state: &Ferrite,
    assets: &RenderAssets,
    output: &Output,
    output_geo: Rectangle<i32, Logical>,
    scale: f64,
    overlays: usize,
    elements: &mut Vec<OutputElement>,
) {
    use crate::desktop::lock::ScreenFade;
    let Some(fade) = state.screen_fade else {
        return;
    };
    let physical = output_geo.size.to_f64().to_physical(scale);
    let size = smithay::utils::Size::from((physical.w.round() as i32, physical.h.round() as i32));
    if let ScreenFade::Capture { .. } = fade {
        // L'écran présent, tel quel : sa photo servira d'image qui part.
        let mut shots = state.screen_fade_shots.borrow_mut();
        if let std::collections::hash_map::Entry::Vacant(slot) = shots.entry(output.name()) {
            match genie::snapshot(renderer, &elements[overlays..], size, scale) {
                Ok(texture) => {
                    slot.insert(texture);
                }
                Err(err) => warn!(?err, "photo de l'écran avant le passage indisponible"),
            }
        }
        return;
    }
    let Some(frame) = fade.frame() else {
        return;
    };
    let scene = elements.split_off(overlays);
    let arriving = match genie::snapshot(renderer, &scene, size, scale) {
        Ok(texture) => texture,
        Err(err) => {
            warn!(?err, "image de la scène pendant le passage indisponible");
            elements.extend(scene);
            return;
        }
    };
    let screen = output_geo.size.to_f64();
    let zoomed = |zoom: f32, alpha: f32| {
        let zoom = f64::from(zoom);
        let size = smithay::utils::Size::<f64, Logical>::from((screen.w * zoom, screen.h * zoom));
        let rect = Rectangle::new(
            Point::from(((screen.w - size.w) / 2.0, (screen.h - size.h) / 2.0)),
            size,
        );
        Genie {
            window: rect,
            icon: rect,
            top: rect.loc.y,
            bottom: rect.loc.y + rect.size.h,
            squeeze: 0.0,
            alpha: f64::from(alpha),
        }
    };
    let program = assets.decorations.shaders.genie.clone();
    let leaving = state.screen_fade_shots.borrow().get(&output.name()).cloned();
    let leaving = leaving.filter(|_| frame.leaving_alpha > 0.0).map(|texture| {
        OutputElement::Genie(genie::GenieElement::new(
            texture,
            program.clone(),
            &zoomed(frame.leaving_zoom, frame.leaving_alpha),
            scale,
            0.0,
        ))
    });
    let arriving = (frame.arriving_alpha > 0.0).then(|| {
        OutputElement::Genie(genie::GenieElement::new(
            arriving,
            program,
            &zoomed(frame.arriving_zoom, frame.arriving_alpha),
            scale,
            0.0,
        ))
    });
    // Le premier élément est dessiné au-dessus : le bureau, qui recule, passe devant
    // l'écran verrouillé, plein.
    let (top, below) = if frame.leaving_on_top {
        (leaving, arriving)
    } else {
        (arriving, leaving)
    };
    elements.extend(top);
    elements.extend(below);
    // Fond noir sous le passage (si la photo de l'ancien écran manque).
    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        Point::from((0.0, 0.0)),
        &assets.screen_fade_overlay,
        None,
        Some(Rectangle::from_size((1.0, 1.0).into())),
        Some(output_geo.size),
        Kind::Unspecified,
    ) {
        Ok(element) => elements.push(OutputElement::Memory(element)),
        Err(err) => warn!("fond noir du passage indisponible : {err:?}"),
    }
}

fn push_screen_fade(
    renderer: &mut GlesRenderer,
    state: &Ferrite,
    assets: &RenderAssets,
    output_geo: Rectangle<i32, Logical>,
    elements: &mut Vec<OutputElement>,
) {
    // Écran éteint jusqu'au réveil : noir complet.
    if state.screen_fade != Some(crate::desktop::lock::ScreenFade::Dark) {
        return;
    }
    let alpha = 1.0;
    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        Point::from((0.0, 0.0)),
        &assets.screen_fade_overlay,
        Some(alpha),
        Some(Rectangle::from_size((1.0, 1.0).into())),
        Some(output_geo.size),
        Kind::Unspecified,
    ) {
        Ok(element) => elements.insert(0, OutputElement::Memory(element)),
        Err(err) => warn!("voile du passage au noir indisponible : {err:?}"),
    }
}

/// Arbre de surfaces de `surface`, son origine en `location` (pixels de l'écran).
fn push_surface_tree(
    renderer: &mut GlesRenderer,
    surface: &WlSurface,
    location: Point<i32, smithay::utils::Physical>,
    scale: f64,
    out: &mut Vec<OutputElement>,
) {
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
        .map(OutputElement::Surface),
    );
}

/// Calques de la couche `layer` sur `output` (voir `protocols::layer_shell`), du dessus vers
/// le dessous, avec leurs menus.
fn push_layers(
    renderer: &mut GlesRenderer,
    output: &Output,
    layer: Layer,
    scale: f64,
    out: &mut Vec<OutputElement>,
) {
    let map = smithay::desktop::layer_map_for_output(output);
    for surface in map.layers_on(layer).rev() {
        let Some(geometry) = map.layer_geometry(surface) else {
            continue;
        };
        for (popup, offset) in PopupManager::popups_for_surface(surface.wl_surface()) {
            let location = (geometry.loc + offset - popup.geometry().loc).to_physical_precise_round(scale);
            push_surface_tree(renderer, popup.wl_surface(), location, scale, out);
        }
        let location = geometry.loc.to_physical_precise_round(scale);
        push_surface_tree(renderer, surface.wl_surface(), location, scale, out);
    }
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
            0.0,
        )));
    }

    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        Point::from((0.0, 0.0)),
        &assets.overview_overlay[usize::from(dark_theme)],
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
    let fixed = state.dock_config.fixed_workspaces.is_some();
    Some(OverviewView { tiles, spaces, fixed })
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
    image: &cursor::CursorImage,
    location: Point<f64, Logical>,
    scale: f64,
    out: &mut Vec<OutputElement>,
) {
    let tip = location.to_physical(scale);
    let position = Point::from((
        (tip.x - image.hotspot.0 as f64).round(),
        (tip.y - image.hotspot.1 as f64).round(),
    ));
    // Image entière, affichée à sa taille logique (agrandie ou réduite par le GPU pour un
    // thème dont aucune image n'a la taille exacte).
    let src = Rectangle::from_size((image.size.0 as f64, image.size.1 as f64).into());
    let size = (
        image.logical.0.round().max(1.0) as i32,
        image.logical.1.round().max(1.0) as i32,
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
        if self.popover == Some(Popover::Screenshot) {
            return self.capture_cursor();
        }
        if let Some(edges) = self.resizing {
            return resize_shape(edges);
        }
        if self.widgets_edit {
            return self.widget_cursor();
        }
        let Some(pointer) = self.seat.get_pointer().filter(|pointer| !pointer.is_grabbed()) else {
            return CursorShape::Arrow;
        };
        if self.over_shell_chrome(pointer.current_location()) {
            return CursorShape::Arrow;
        }
        match self.hit_test(pointer.current_location()) {
            Some(wm::Hit::Decoration(_, wm::decoration::Part::Border(edges))) => resize_shape(edges),
            // Sur une application : la forme qu'elle a demandée par son nom.
            Some(wm::Hit::Surface(..)) => named_shape(&self.cursor_status),
            _ => CursorShape::Arrow,
        }
    }

    /// Écran de capture : le curseur annonce le coin ou le bord qu'on va saisir.
    fn capture_cursor(&self) -> CursorShape {
        use crate::render::capture::RegionGrip;
        let Some(pointer) = self.pointer_location() else {
            return CursorShape::Arrow;
        };
        let on_toolbar = self
            .space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))
            .is_some_and(|output| {
                crate::desktop::panels::capture_toolbar_at(output, pointer, true).is_some()
            });
        if self.region_select.is_some() {
            return CursorShape::Crosshair;
        }
        if on_toolbar || !self.screenshot_mode.selects_area() {
            return CursorShape::Arrow;
        }
        let grip = self.region_adjust.and_then(|adjust| {
            adjust
                .grip
                .map(|(grip, _, _)| grip)
                .or_else(|| capture::region_grip(adjust.rect, pointer))
        });
        match grip {
            Some(RegionGrip::NorthWest | RegionGrip::SouthEast) => CursorShape::DiagonalDown,
            Some(RegionGrip::NorthEast | RegionGrip::SouthWest) => CursorShape::DiagonalUp,
            Some(RegionGrip::North | RegionGrip::South) => CursorShape::Vertical,
            Some(RegionGrip::West | RegionGrip::East) => CursorShape::Horizontal,
            Some(RegionGrip::Move) => CursorShape::Move,
            None => CursorShape::Crosshair,
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

    /// Cadre de la fenêtre qui tape du texte, pour placer les candidats d'une méthode de
    /// saisie (voir `protocols::input_method`).
    pub fn input_popup_parent(&self, parent: &WlSurface) -> Rectangle<i32, Logical> {
        self.window_for_surface(parent)
            .and_then(|window| self.space.element_geometry(&window))
            .unwrap_or_default()
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
    /// Le pointeur est-il sur le Dock ou la barre du haut, dessinés au-dessus des fenêtres ?
    /// L'application dessous ne reçoit alors pas le pointeur et ne choisit pas sa forme
    /// (sans cela, un champ de texte derrière le Dock y imposait le curseur texte).
    pub fn over_shell_chrome(&self, pos: Point<f64, Logical>) -> bool {
        if self.lock.is_some() {
            return false;
        }
        let Some(output) = self.screen_area() else {
            return false;
        };
        // La barre et le Dock ne sont que sur l'écran principal.
        if !output.to_f64().contains(pos) {
            return false;
        }
        let bar_bottom = f64::from(output.loc.y) + f64::from(self.dock_config.bar_height);
        if pos.y < bar_bottom && self.bar_visible() {
            return true;
        }
        if !self.dock_shown(output) {
            return false;
        }
        self.dock_geometry()
            .is_some_and(|(_, _, geometry)| geometry.plate.contains(pos) || geometry.item_at(pos).is_some())
    }

    pub fn dock_shown(&self, output: Rectangle<i32, Logical>) -> bool {
        // La barre de capture occupe le bas de l'écran.
        if self.popover == Some(Popover::Screenshot) {
            return false;
        }
        if self.overview {
            return true;
        }
        let frames = self.window_frames();
        let items = self.dock_items();
        let dock = crate::desktop::dock::layout(output, self.dock_config, &items, 1.0, None, 0.0).plate;
        let pointer = self.pointer_location();
        let now = std::time::Instant::now();
        // Souris glissée vite vers le bas, au-dessus de l'emplacement du Dock.
        let approached = self
            .dock_fast_approach_until
            .get()
            .is_some_and(|until| now < until)
            && pointer
                .is_some_and(|pointer| pointer.x >= dock.loc.x && pointer.x <= dock.loc.x + dock.size.w);
        // Sorti, il reste tant que la souris est dessus (ou dessous, dans la marge du bas),
        // et se cache dès qu'elle en part.
        let on_dock = self.dock_was_shown.get()
            && pointer.is_some_and(|pointer| {
                pointer.y >= dock.loc.y && pointer.x >= dock.loc.x && pointer.x <= dock.loc.x + dock.size.w
            });
        // Fichiers glissés vers le bas de l'écran : le Dock vient les recevoir.
        let dragging = self.file_drag.as_ref().is_some_and(|file| !file.dropped)
            && pointer.is_some_and(|pointer| pointer.y >= dock.loc.y - crate::desktop::file_drag::DOCK_REACH);
        // Une fenêtre qui file vers son icône, ou en ressort, la garde affichée.
        let genie = self.window_animations.try_borrow().is_ok_and(|animations| {
            animations.iter().any(|animation| {
                matches!(
                    animation.motion,
                    wm::animation::Motion::Minimize | wm::animation::Motion::Restore
                )
            })
        });
        let called = genie || approached || on_dock || dragging;
        let shown = crate::desktop::dock::should_show(crate::desktop::dock::Visibility {
            config: self.dock_config,
            dock,
            called,
            windows: &frames,
            fullscreen: self.top_fullscreen(),
            // Le menu d'une fenêtre emprunte le même panneau sans appeler le Dock.
            launcher_open: self.popover == Some(Popover::Launcher)
                || (self.popover == Some(Popover::DockMenu) && self.window_menu.is_none()),
        });
        self.dock_was_shown.set(shown);
        shown
    }

    /// La barre du haut est-elle affichée ? Masquée (plein écran ou réglage de
    /// Paramètres), elle revient comme le Dock : quand on glisse vite la souris vers le
    /// haut de l'écran, pas quand elle y passe doucement (la barre de titre d'une
    /// fenêtre agrandie, juste dessous, reste accessible). Elle reste ensuite tant que
    /// la souris est dessus ou qu'un menu est ouvert.
    pub fn bar_visible(&self) -> bool {
        let height = self.dock_config.bar_height as f64;
        let revealed = self.bar_revealed.get();
        let approached = self
            .bar_fast_approach_until
            .get()
            .is_some_and(|until| std::time::Instant::now() < until);
        let pointer_calls = self
            .pointer_location()
            .is_some_and(|pointer| (approached || revealed) && pointer.y < height);
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
    let notifications = notifications_view(state);
    let mut island = state.island_view(&notifications);
    // Plein écran : la pastille repliée se cache avec la barre ; les annonces passent.
    if !state.bar_visible() && island.kind.as_ref().is_some_and(|kind| kind.compact()) {
        island.kind = None;
        island.others.clear();
    }
    let system = &state.system;
    let dark_theme = state.appearance.is_dark();
    Panels::set_theme(dark_theme, state.accent);
    DesktopView {
        dark_theme,
        popover: state.popover,
        wifi: system.wifi,
        wifi_signal: system.wifi_signal,
        bluetooth: system.bluetooth,
        volume: system
            .volume
            .map(|volume| if volume.muted { 0 } else { volume.level }),
        brightness: system.brightness,
        airplane: system.airplane,
        keyboard_light: system.keyboard_light,
        power_saver: system.power_saver,
        night: night_view(state),
        second_screen: (state.screen_count > 1).then_some(state.screen_mode),
        power: state.pending_power,
        // Pas par-dessus l'écran verrouillé : elle attend le déverrouillage.
        auth: state.auth.front().filter(|_| state.lock.is_none()).map(|auth| {
            crate::desktop::panels::AuthView {
                message: auth.message.clone(),
                user: auth.user.clone(),
                length: auth.password.chars().count(),
                checking: auth.checking,
                refused: auth.refused,
            }
        }),
        power_left: state.power_deadline.map_or(0, |deadline| {
            deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_secs_f64()
                .ceil() as u32
        }),
        power_apps: state.open_app_count(),
        error: state.system_error.clone(),
        bar_visible: state.bar_visible(),
        bar_opaque: !state.dock_config.bar_translucent || state.space.elements().any(wm::is_maximized),
        battery_percent: state.dock_config.battery_percent,
        bluetooth_icon: state.dock_config.bluetooth_icon,
        clock_style: state.dock_config.clock,
        controls: if state.popover == Some(Popover::Controls) {
            crate::desktop::panels::ControlsView {
                page: state.controls.page,
                // Radio coupée : les listes gardées de la dernière analyse ne s'affichent plus.
                networks: if system.wifi == Some(true) {
                    state.controls.network_rows()
                } else {
                    Vec::new()
                },
                devices: if system.bluetooth == Some(true) {
                    state.controls.device_rows()
                } else {
                    Vec::new()
                },
                password: state.controls.password_row(),
                scanning: state.controls.scanning,
                current_network: state.controls.current_network().map(str::to_owned),
                connected_devices: state.controls.connected_devices(),
            }
        } else {
            Default::default()
        },
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
                    lines: note.text.lines().map(str::to_owned).collect(),
                })
                .collect(),
            lists: state
                .dashboard
                .lists
                .iter()
                .map(|list| list.name.clone())
                .collect(),
            list_shown: state.dashboard.list_shown,
            loose_list: state.dashboard.loose_list_shown(),
            busy_month: {
                let (_, (year, month)) =
                    crate::desktop::panels::calendar_months(state.dashboard.month_offset);
                state.dashboard.busy_days(year, month)
            },
            busy_today: {
                let ((year, month), _) = crate::desktop::panels::calendar_months(0);
                state.dashboard.busy_days(year, month)
            },
            tasks: state
                .dashboard
                .tasks()
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
            shown: !state.overview && state.lock.is_none() && state.workspaces.current() == 0,
            front: state.widgets_front,
            edit: state.widgets_edit,
            drag: state.widget_drag.map(|drag| drag.index),
            target: state.widget_drag.and_then(|drag| {
                let area = state.screen_area()?;
                crate::desktop::widgets::place_on_grid(
                    &state.dashboard.layout,
                    drag.index,
                    area.size.w,
                    area.size.h,
                    state.dock_config.bar_height as i32,
                )
            }),
            media: state.media().cloned(),
            apps: state
                .recent_app_keys()
                .into_iter()
                .map(|key| crate::desktop::panels::DashboardApp {
                    icon: key.icon(&state.applications),
                    name: key.name(&state.applications),
                    running: state.app_running(&key),
                })
                .collect(),
            shortcuts: state.shortcut_views(),
            shortcuts_shown: !state.overview && state.lock.is_none(),
            icon_size: state.desktop_shortcuts.icon_size,
            shortcut_drag: state.shortcut_drag.and_then(|drag| {
                let (dx, dy) = drag.offset()?;
                Some((drag.index, (dx.round() as i32, dy.round() as i32)))
            }),
            shortcut_lasso: state.shortcut_lasso.as_ref().map(|lasso| {
                let (x, y, w, h) = lasso.rect();
                (
                    x.round() as i32,
                    y.round() as i32,
                    w.round() as i32,
                    h.round() as i32,
                )
            }),
        },
        desktop_menu: state.desktop_menu,
        desktop_menu_entries: state
            .desktop_menu_items()
            .into_iter()
            .map(|item| item.label().to_owned())
            .collect(),
        menu_pointer: state
            .pointer_location()
            .filter(|_| matches!(state.popover, Some(Popover::DockMenu | Popover::DesktopMenu)))
            .map(|location| (location.x as i32, location.y as i32)),
        osd: state.osd_view(),
        capture: crate::desktop::panels::CaptureView {
            mode: state.screenshot_mode,
            cursor: state.screenshot_cursor,
            delay: state.screenshot_delay,
            windows: state.space.elements().next().is_some(),
            video: state.screenshot_video,
        },
        recording: state
            .recording
            .as_ref()
            .map(|recording| crate::desktop::recording::elapsed_label(recording.elapsed())),
        overview: overview_view(state),
        switcher: switcher_view(state),
        lock: state.lock.as_ref().map(|lock| {
            let name = crate::desktop::lock::user_name();
            let mut letters = name.chars();
            let display = letters
                .next()
                .map(|first| first.to_uppercase().chain(letters).collect::<String>())
                .unwrap_or_else(|| "Utilisateur".to_owned());
            // Écran de connexion : le compte choisi.
            let display = match state.greeter.as_ref().and_then(|greeter| greeter.current()) {
                Some(user) => user.name.clone(),
                None => display,
            };
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
                initial: display
                    .chars()
                    .next()
                    .and_then(|first| first.to_uppercase().next())
                    .unwrap_or('U'),
                avatar: match &state.greeter {
                    Some(greeter) => greeter.current().and_then(|user| user.avatar.clone()),
                    None => state.lock_avatar.clone(),
                },
                user: display,
                length: lock.password_length(),
                cursor: lock.cursor(),
                shown: lock.peeked(),
                field: lock.field_visible,
                progress: lock.progress(reduce),
                checking: lock.checking,
                welcome: lock.is_welcoming(),
                message: lock.message,
                face: lock.face,
                face_ready: lock.face_ready,
                switch_user: state
                    .greeter
                    .as_ref()
                    .is_some_and(|greeter| greeter.users.len() > 1),
                greeter: state.greeter.is_some(),
                face_tick: lock.face_tick(reduce),
                shake: if reduce { 0 } else { lock.shake() },
                pulse: if reduce { 8 } else { lock.pulse() },
                spin: lock.spin(),
                dark: lock.dark,
                // La photo choisie mais pas encore décodée : le premier fond en attendant.
                wall: if lock.wall >= crate::desktop::panels::LOCK_WALLS.len() && state.lock_picture.is_none()
                {
                    0
                } else {
                    lock.wall
                },
                picture: state.lock_picture.clone(),
                blur: lock.blur,
                weather: state.dashboard.weather.clone(),
                place: state.dashboard.place.clone(),
                notice,
                media: lock.media.clone(),
            }
        }),
        notifications,
        island,
        region: state.region_select.map(|select| RegionView {
            rect: select
                .start
                .zip(state.pointer_location())
                .map(|(start, pointer)| {
                    let zone = capture::region_between(start, pointer);
                    (zone.loc.x, zone.loc.y, zone.size.w, zone.size.h)
                }),
        }),
        window_select: state
            .window_select
            .then(|| {
                state
                    .window_select_hover
                    .as_ref()
                    .and_then(|window| state.window_frame(window))
                    .map(|rect| (rect.loc.x, rect.loc.y, rect.size.w, rect.size.h))
            })
            .flatten(),
        region_adjust: state.region_adjust.map(|adjust| {
            (
                adjust.rect.loc.x,
                adjust.rect.loc.y,
                adjust.rect.size.w,
                adjust.rect.size.h,
            )
        }),
        dock_menu: match (&state.dock_menu, state.popover) {
            (_, Some(Popover::DockMenu)) if state.window_menu.is_some() => {
                state
                    .window_menu
                    .as_ref()
                    .map(|(_, actions, anchor)| DockMenuView {
                        x: anchor.x,
                        bottom: anchor.y,
                        entries: actions.iter().map(|action| action.label().to_owned()).collect(),
                        at_pointer: true,
                    })
            }
            (Some((_, actions, anchor)), Some(Popover::DockMenu)) => Some(DockMenuView {
                x: anchor.x,
                bottom: anchor.y,
                entries: actions.iter().map(|action| action.label().to_owned()).collect(),
                // Seul le menu d'une icône du Dock fige le pointeur du Dock.
                at_pointer: state.dock_menu_pointer.is_none(),
            }),
            _ => None,
        },
    }
}

/// Bulles à afficher et, si son panneau est ouvert, l'historique des notifications.
fn notifications_view(state: &Ferrite) -> NotificationsView {
    let now = std::time::SystemTime::now();
    // Aperçu coupé (Paramètres → Notifications) : la bannière ne montre que le titre ;
    // l'historique garde le texte.
    let view = |notice: &crate::desktop::notifications::Notice, preview: bool| NotificationView {
        id: notice.id,
        app: notice.app.clone(),
        summary: notice.summary.clone(),
        body: if preview {
            notice.body.replace('\n', " ")
        } else {
            String::new()
        },
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
            state
                .notifications
                .bubbles()
                .map(|notice| view(notice, state.notifications.preview))
                .collect()
        },
        history: if state.popover == Some(Popover::Notifications) {
            state
                .notifications
                .history()
                .map(|notice| view(notice, true))
                .collect()
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

/// Forme Ferrite pour le curseur nommé qu'une application demande (`cursor-shape-v1`,
/// ou thème de curseur) ; la flèche pour une image propre au client.
fn named_shape(status: &CursorImageStatus) -> CursorShape {
    use smithay::input::pointer::CursorIcon;
    let CursorImageStatus::Named(icon) = status else {
        return CursorShape::Arrow;
    };
    match icon {
        CursorIcon::Text | CursorIcon::VerticalText => CursorShape::Text,
        CursorIcon::Pointer | CursorIcon::Grab | CursorIcon::Grabbing => CursorShape::Pointer,
        CursorIcon::Move | CursorIcon::AllScroll | CursorIcon::AllResize => CursorShape::Move,
        CursorIcon::Crosshair | CursorIcon::Cell => CursorShape::Crosshair,
        CursorIcon::EwResize | CursorIcon::EResize | CursorIcon::WResize | CursorIcon::ColResize => {
            CursorShape::Horizontal
        }
        CursorIcon::NsResize | CursorIcon::NResize | CursorIcon::SResize | CursorIcon::RowResize => {
            CursorShape::Vertical
        }
        CursorIcon::NwseResize | CursorIcon::NwResize | CursorIcon::SeResize => CursorShape::DiagonalDown,
        CursorIcon::NeswResize | CursorIcon::NeResize | CursorIcon::SwResize => CursorShape::DiagonalUp,
        _ => CursorShape::Arrow,
    }
}

/// Fait la capture demandée, s'il y en a une : l'écran est redessiné sans son panneau de
/// réglages, puis enregistré et/ou copié en arrière-plan.
pub fn take_screenshot(
    renderer: &mut GlesRenderer,
    state: &mut Ferrite,
    assets: &RenderAssets,
    output: &Output,
) {
    let Some(request) = state.screenshot_request.take() else {
        return;
    };
    ferrite_system::sounds::play(ferrite_system::sounds::Sound::Screenshot);
    if let Err(err) = render_screenshot(renderer, state, assets, output, request) {
        warn!("capture d'écran impossible : {err}");
    }
}

/// Copies d'écran demandées par les clients pour `output` (voir `protocols::screencopy`) :
/// l'écran est redessiné à part, avec le curseur si on le veut, puis copié dans leur tampon.
pub fn serve_screencopy(
    renderer: &mut GlesRenderer,
    state: &mut Ferrite,
    assets: &RenderAssets,
    output: &Output,
) {
    let (copies, others): (Vec<_>, Vec<_>) = std::mem::take(&mut state.screencopy)
        .into_iter()
        .partition(|copy| copy.info.output == *output);
    state.screencopy = others;
    if copies.is_empty() {
        return;
    }
    // Une image sans curseur, une avec : chacune dessinée au plus une fois.
    let mut images: [Option<Result<Pixels, String>>; 2] = [None, None];
    let time = std::time::Duration::from(smithay::utils::Clock::<smithay::utils::Monotonic>::new().now());
    for copy in copies {
        let slot = usize::from(copy.info.cursor);
        let image = images[slot]
            .get_or_insert_with(|| render_pixels(renderer, state, assets, output, copy.info.cursor));
        let written = match image {
            Ok((pixels, width, _)) => write_screencopy(&copy, pixels, *width),
            Err(err) => Err(err.clone()),
        };
        match written {
            Ok(()) => {
                copy.frame.flags(smithay::reexports::wayland_protocols_wlr::screencopy::v1::server::zwlr_screencopy_frame_v1::Flags::empty());
                if copy.with_damage {
                    let size = copy.info.region.size;
                    copy.frame.damage(0, 0, size.w as u32, size.h as u32);
                }
                let seconds = time.as_secs();
                copy.frame
                    .ready((seconds >> 32) as u32, seconds as u32, time.subsec_nanos());
            }
            Err(err) => {
                warn!("copie d'écran impossible : {err}");
                copy.frame.failed();
            }
        }
    }
}

/// Image `RGBA`, sa largeur et sa hauteur.
type Pixels = (Vec<u8>, u32, u32);

/// L'écran entier en pixels `RGBA`, dans le bon sens (l'écran en fenêtre est retourné).
fn render_pixels(
    renderer: &mut GlesRenderer,
    state: &Ferrite,
    assets: &RenderAssets,
    output: &Output,
    cursor: bool,
) -> Result<Pixels, String> {
    let size = output.current_mode().ok_or("écran sans mode")?.size;
    let buffer_size = size.to_logical(1).to_buffer(1, smithay::utils::Transform::Normal);
    let mut texture: GlesTexture = renderer
        .create_buffer(Fourcc::Abgr8888, buffer_size)
        .map_err(|err| format!("{err:?}"))?;
    let mut framebuffer = renderer.bind(&mut texture).map_err(|err| format!("{err:?}"))?;
    let elements = output_elements(renderer, state, assets, output, cursor);
    OutputDamageTracker::from_output(output)
        .render_output(renderer, &mut framebuffer, 0, &elements, [0.0, 0.0, 0.0, 1.0])
        .map_err(|err| format!("{err:?}"))?;
    let mapping = renderer
        .copy_framebuffer(&framebuffer, Rectangle::from_size(buffer_size), Fourcc::Abgr8888)
        .map_err(|err| format!("{err:?}"))?;
    let pixels = renderer.map_texture(&mapping).map_err(|err| format!("{err:?}"))?;
    let (width, height) = (size.w as u32, size.h as u32);
    let flip = output.current_transform() == smithay::utils::Transform::Flipped180;
    let (image, width, height) = capture::crop(pixels, width, height, flip, (0, 0, width, height));
    Ok((image, width, height))
}

/// Zone demandée de l'image `RGBA` (large de `width`) vers le tampon `XRGB8888` du client.
fn write_screencopy(
    copy: &crate::protocols::screencopy::PendingCopy,
    rgba: &[u8],
    width: u32,
) -> Result<(), String> {
    let region = copy.info.region;
    smithay::wayland::shm::with_buffer_contents_mut(&copy.buffer, |pointer, length, data| {
        // SAFETY : Smithay donne l'adresse et la longueur du bassin partagé du client, en
        // vie pendant l'appel ; on n'écrit que dans le tampon décrit par `data`.
        #[allow(unsafe_code)]
        let pool = unsafe { std::slice::from_raw_parts_mut(pointer, length) };
        let stride = data.stride as usize;
        for row in 0..region.size.h as usize {
            let line = (region.loc.y as usize + row) * width as usize + region.loc.x as usize;
            let start = data.offset as usize + row * stride;
            let Some(target) = pool.get_mut(start..start + region.size.w as usize * 4) else {
                return Err("tampon trop petit".to_owned());
            };
            for (column, pixel) in target.chunks_exact_mut(4).enumerate() {
                let source = &rgba[(line + column) * 4..(line + column) * 4 + 4];
                pixel.copy_from_slice(&[source[2], source[1], source[0], 255]);
            }
        }
        Ok(())
    })
    .map_err(|err| format!("{err:?}"))?
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
    let elements = output_elements(renderer, state, assets, output, request.cursor);
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
    let clipboard_sender = request
        .clipboard
        .then(|| state.clipboard_sender.clone())
        .flatten();
    let clipboard_sender_text = state.clipboard_sender.clone();
    let copy = request.clipboard;
    if request.text {
        // Mode Texte : reconnaissance dans un fil à part, puis copie du texte.
        std::thread::spawn(move || {
            use crate::desktop::notifications::{Event, local_notice};
            let notice = match crate::desktop::ocr::recognize(&image, width, height) {
                Ok(text) if text.trim().is_empty() => local_notice(
                    "Aucun texte trouvé",
                    "Essayez une zone plus grande, ou un texte plus net.",
                ),
                Ok(text) => {
                    let preview = crate::desktop::ocr::preview(&text);
                    let content = crate::clipboard::Content::Text(text.into());
                    match clipboard_sender_text {
                        Some(sender)
                            if sender
                                .send((content, Some("Texte de l’écran".to_owned())))
                                .is_ok() =>
                        {
                            local_notice("Texte copié", &preview)
                        }
                        _ => local_notice(
                            "Copie du texte impossible",
                            "Le presse-papiers n’est pas disponible.",
                        ),
                    }
                }
                Err(error) => {
                    warn!("texte de l'écran non reconnu : {}", error.body);
                    local_notice(error.summary, &error.body)
                }
            };
            if let Some(events) = events {
                let _ = events.send(Event::Notify(notice));
            }
        });
        return Ok(());
    }
    capture::capture_in_background(image, width, height, request.save, move |result| {
        use crate::desktop::notifications::{Event, file_notice, local_notice};
        let notice = match result {
            Ok((path, png)) => {
                let copied = if copy {
                    match (
                        clipboard_sender,
                        crate::clipboard::decode(
                            &crate::clipboard::Wanted::Image("image/png".to_owned()),
                            png,
                        ),
                    ) {
                        (Some(sender), Some(content)) => {
                            if let Err(error) = sender.send((content, Some("Capture d’écran".to_owned())))
                            {
                                warn!(%error, "capture non transmise au presse-papiers");
                                false
                            } else {
                                true
                            }
                        }
                        (_, None) => {
                            warn!("capture PNG illisible, copie impossible");
                            false
                        }
                        (None, Some(_)) => {
                            warn!("historique du presse-papiers indisponible");
                            false
                        }
                    }
                } else {
                    false
                };
                match (path, copy, copied) {
                    (Some(path), true, true) => {
                        tracing::info!("capture d'écran enregistrée : {}", path.display());
                        let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
                        file_notice("Capture copiée et enregistrée", &name.unwrap_or_default(), path)
                    }
                    (Some(path), true, false) => {
                        let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
                        file_notice(
                            "Capture enregistrée, copie impossible",
                            &name.unwrap_or_default(),
                            path,
                        )
                    }
                    (Some(path), false, _) => {
                        tracing::info!("capture d'écran enregistrée : {}", path.display());
                        let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
                        file_notice("Capture enregistrée", &name.unwrap_or_default(), path)
                    }
                    (None, true, true) => local_notice("Capture copiée", "L’image est prête à être collée."),
                    (None, true, false) => local_notice(
                        "Copie de capture impossible",
                        "Le presse-papiers n’est pas disponible.",
                    ),
                    (None, false, _) => {
                        local_notice("Capture impossible", "Aucune destination n’est activée.")
                    }
                }
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

/// Panneau Super+V : lignes visibles de la liste filtrée, à leur place après défilement.
fn clipboard_view(state: &Ferrite) -> ClipboardView {
    use crate::clipboard::{self, Content, Kind};
    let history = &state.clipboard;
    let shown = state.clipboard_shown();
    let placement = history.place(&shown);
    let visible = state.clipboard_list_height();
    let scroll = state.clipboard_scroll;
    let now = std::time::SystemTime::now();
    let rows = placement
        .rows
        .iter()
        .enumerate()
        .filter(|(_, placed)| placed.y + placed.h > scroll && placed.y < scroll + visible)
        .map(|(position, placed)| {
            let entry = &history.entries()[placed.index];
            let kind = entry.kind();
            let age = clipboard::ago(now, entry.copied);
            let file = match &entry.content {
                Content::Text(text) if kind == Kind::Files => clipboard::file_paths(text).into_iter().next(),
                _ => None,
            };
            ClipRow {
                index: placed.index,
                kind,
                label: clipboard::label(entry),
                sub: match &entry.source {
                    Some(source) => format!("{source} · {age}"),
                    None => age,
                },
                key: (position < 9).then_some(position + 1),
                pinned: entry.pinned,
                selected: position == state.clipboard_selected,
                y: placed.y - scroll,
                h: placed.h,
                thumbnail: match &entry.content {
                    Content::Image { thumbnail, .. } => Some(thumbnail.clone()),
                    Content::Text(_) => None,
                },
                file_icon: file.as_deref().map_or("file-generic", file_icon),
                swatch: match &entry.content {
                    Content::Text(text) if kind == Kind::Color => hex_color(text.trim()),
                    _ => None,
                },
            }
        })
        .collect();
    let total = history.entries().len();
    ClipboardView {
        rows,
        sections: placement
            .sections
            .iter()
            .map(|(title, y)| (*title, y - scroll))
            .filter(|(_, y)| *y + crate::clipboard::SECTION > 0.0 && *y < visible)
            .collect(),
        count: total,
        query: state.clipboard_query.clone(),
        filter: state.clipboard_filter,
        empty: shown.is_empty().then_some({
            if total == 0 {
                (
                    "Rien à coller pour l’instant",
                    "Copiez du texte, une image ou un fichier : il apparaîtra ici.",
                )
            } else {
                ("Aucun résultat", "Essayez un autre mot ou un autre filtre.")
            }
        }),
        toast: state.clipboard_toast(),
        position: state.clipboard_position,
    }
}

/// Icône d'un fichier copié, d'après son extension.
pub(crate) fn file_icon(path: &std::path::Path) -> &'static str {
    if path.is_dir() {
        return "places-folder";
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match extension.as_str() {
        "pdf" => "file-pdf",
        "doc" | "docx" | "odt" | "rtf" => "file-document",
        "xls" | "xlsx" | "ods" => "file-spreadsheet",
        "ppt" | "pptx" | "odp" => "file-presentation",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => "file-image",
        "mp3" | "ogg" | "flac" | "wav" | "opus" | "m4a" => "file-audio",
        "mp4" | "mkv" | "webm" | "avi" | "mov" => "file-video",
        "zip" | "tar" | "gz" | "xz" | "zst" | "7z" | "rar" => "file-archive",
        "sh" | "py" | "rs" | "js" | "ts" | "c" | "h" | "cpp" | "go" => "file-script",
        _ => "file-text",
    }
}

/// Couleur d'un texte `#RGB`, `#RRGGBB` ou `#RRGGBBAA` (l'opacité est ignorée).
fn hex_color(text: &str) -> Option<ferrite_ui::theme::Color> {
    let hex = text.strip_prefix('#')?;
    let full: String = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => hex[..6].to_owned(),
        _ => return None,
    };
    u32::from_str_radix(&full, 16)
        .ok()
        .map(ferrite_ui::theme::Color::rgb)
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

/// Tuile du mode nuit : active ou non, et son état (« Activé · jusqu'à 06:05 »…).
fn night_view(state: &Ferrite) -> (bool, String) {
    use ferrite_system::night::{Schedule, format_time};
    let night = &state.night;
    let active = night.active();
    let text = match (night.settings.schedule, active, night.window()) {
        (Schedule::Always, true, _) => "Activé · toujours".to_owned(),
        (Schedule::Sun | Schedule::Hours, true, Some((_, end))) if night.forced.is_none() => {
            format!("Activé · jusqu’à {}", format_time(end))
        }
        (Schedule::Sun | Schedule::Hours, false, Some((start, _))) if night.forced.is_none() => {
            format!("Auto · dès {}", format_time(start))
        }
        (_, true, _) => "Activé".to_owned(),
        (_, false, _) => "Désactivé".to_owned(),
    };
    (active, text)
}
