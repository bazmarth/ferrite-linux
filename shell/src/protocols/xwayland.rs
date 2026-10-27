//! XWayland : applications X11 (Brave et Chromium par défaut, VLC, jeux, vieux logiciels).
//!
//! ferrite-shell lance `Xwayland` à la demande de Smithay et en devient le gestionnaire de
//! fenêtres (X11Wm). Une fenêtre X11 est une `Window` de l'espace comme les autres : elle
//! reçoit les décorations Ferrite si elle ne dessine pas les siennes, se déplace, s'agrandit
//! et passe en plein écran. Les menus et infobulles X11 (« override-redirect ») sont posés
//! tels quels à la position qu'ils demandent.

use std::{cell::RefCell, process::Stdio};

use ferrite_ui::theme::TITLEBAR_HEIGHT;
use smithay::{
    delegate_xwayland_shell,
    desktop::Window,
    reexports::calloop::LoopHandle,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Size},
    wayland::{
        selection::{
            SelectionTarget,
            data_device::{
                clear_data_device_selection, current_data_device_selection_userdata,
                request_data_device_client_selection, set_data_device_selection,
            },
            primary_selection::{
                clear_primary_selection, request_primary_client_selection, set_primary_selection,
            },
        },
        xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    },
    xwayland::{
        X11Surface, X11Wm, XWayland, XWaylandEvent, XwmHandler,
        xwm::{Reorder, ResizeEdge as X11ResizeEdge, XwmId},
    },
};
use std::os::fd::OwnedFd;
use tracing::{info, warn};

use crate::{
    clipboard::{self, Owner},
    state::Ferrite,
    wm::{self, ResizeEdge},
};

/// Taille donnée à une fenêtre X11 qui ne demande rien de raisonnable.
const DEFAULT_SIZE: (i32, i32) = (900, 600);

/// Géométrie d'une fenêtre X11 avant agrandissement ou plein écran.
#[derive(Default)]
struct X11Restore(RefCell<Option<Rectangle<i32, Logical>>>);

/// Démarre XWayland ; les applications X11 pourront se connecter dès qu'il sera prêt.
pub fn start(handle: &LoopHandle<'static, Ferrite>, state: &mut Ferrite) {
    // Sous WSLg (développement en fenêtre), le dossier des sockets X11 est en lecture seule
    // et appartient au serveur X de Windows : XWayland n'y a pas sa place.
    let sockets = std::path::Path::new("/tmp/.X11-unix");
    if sockets.exists() && !writable(sockets) {
        info!("dossier /tmp/.X11-unix en lecture seule : XWayland non démarré");
        return;
    }
    let spawned = XWayland::spawn(
        &state.display_handle,
        None,
        std::iter::empty::<(String, String)>(),
        true,
        Stdio::null(),
        Stdio::null(),
        |_| (),
    );
    let (xwayland, client) = match spawned {
        Ok(spawned) => spawned,
        Err(err) => {
            warn!(%err, "XWayland indisponible : les applications X11 ne s'afficheront pas");
            return;
        }
    };
    let wm_handle = handle.clone();
    let inserted = handle.insert_source(xwayland, move |event, _, state| match event {
        XWaylandEvent::Ready {
            x11_socket,
            display_number,
        } => match X11Wm::start_wm(wm_handle.clone(), x11_socket, client.clone()) {
            Ok(wm) => {
                info!(display = display_number, "XWayland prêt");
                state.xwm = Some(wm);
                state.x_display = Some(display_number);
                state.publish_environment();
            }
            Err(err) => warn!(%err, "gestionnaire de fenêtres X11 impossible à démarrer"),
        },
        XWaylandEvent::Error => warn!("XWayland s'est arrêté au démarrage"),
    });
    if let Err(err) = inserted {
        warn!(%err, "XWayland non branché sur la boucle");
    }
}

impl Ferrite {
    /// Fenêtre de l'espace (ou réduite) qui porte la surface X11 `surface`.
    fn x11_window(&self, surface: &X11Surface) -> Option<Window> {
        self.space
            .elements()
            .chain(self.minimized.iter().map(|minimized| &minimized.window))
            .find(|window| window.x11_surface() == Some(surface))
            .cloned()
    }

    /// Pose une fenêtre X11 à `content` (zone du contenu, sans barre de titre) et en
    /// informe le serveur X, pour que ses menus s'ouvrent au bon endroit.
    pub fn place_x11(&mut self, window: &Window, content: Rectangle<i32, Logical>) {
        if let Some(surface) = window.x11_surface() {
            if let Err(err) = surface.configure(content) {
                warn!(?err, "fenêtre X11 non reconfigurée");
            }
        }
        self.space.map_element(window.clone(), content.loc, true);
    }

    pub fn x11_save_geometry(&self, window: &Window) {
        let Some(location) = self.space.element_location(window) else {
            return;
        };
        let restore = window.user_data().get_or_insert(X11Restore::default);
        let mut saved = restore.0.borrow_mut();
        if saved.is_none() {
            *saved = Some(Rectangle::new(location, window.geometry().size));
        }
    }

    fn x11_restore_geometry(&mut self, window: &Window) {
        let previous = window
            .user_data()
            .get::<X11Restore>()
            .and_then(|restore| restore.0.borrow_mut().take());
        if let Some(previous) = previous {
            self.place_x11(window, previous);
        }
    }

    /// Fenêtre X11 agrandie ou accrochée qu'on commence à déplacer : elle reprend sa
    /// taille d'avant sous le pointeur (voir `release_snapped`).
    pub fn x11_release_snapped(
        &mut self,
        window: &Window,
        pointer: Point<f64, Logical>,
    ) -> Option<Point<i32, Logical>> {
        let surface = window.x11_surface()?.clone();
        if surface.is_fullscreen() {
            return None;
        }
        let location = self.space.element_location(window)?;
        let size = window.geometry().size;
        let previous = window
            .user_data()
            .get::<X11Restore>()
            .and_then(|restore| restore.0.borrow_mut().take())?;
        let _ = surface.set_maximized(false);
        let target = wm::unsnapped_location(Rectangle::new(location, size), previous.size, pointer);
        self.place_x11(window, Rectangle::new(target, previous.size));
        Some(target)
    }

    pub fn x11_set_maximized(&mut self, window: &Window, maximized: bool) {
        let Some(surface) = window.x11_surface().cloned() else {
            return;
        };
        if maximized {
            let Some(area) = self.usable_area() else { return };
            self.x11_save_geometry(window);
            let _ = surface.set_maximized(true);
            let content = wm::maximized_content(area, wm::is_server_decorated(window));
            self.place_x11(window, content);
        } else {
            let _ = surface.set_maximized(false);
            self.x11_restore_geometry(window);
        }
    }

    fn x11_set_fullscreen(&mut self, window: &Window, fullscreen: bool) {
        let Some(surface) = window.x11_surface().cloned() else {
            return;
        };
        if fullscreen {
            let Some(output) = self.space.outputs().next().cloned() else {
                return;
            };
            let Some(geometry) = self.space.output_geometry(&output) else {
                return;
            };
            self.x11_save_geometry(window);
            let _ = surface.set_fullscreen(true);
            self.place_x11(window, geometry);
            self.focus_window(Some(window.clone()));
        } else {
            let _ = surface.set_fullscreen(false);
            self.x11_restore_geometry(window);
        }
    }
}

impl XwmHandler for Ferrite {
    /// Presse-papiers partagé entre applications X11 et Wayland.
    fn allow_selection_access(&mut self, _xwm: XwmId, _selection: SelectionTarget) -> bool {
        true
    }

    /// Une application X11 colle : on lui donne la sélection Wayland (ou l'élément de
    /// l'historique collé avec Super+V).
    fn send_selection(&mut self, _xwm: XwmId, selection: SelectionTarget, mime_type: String, fd: OwnedFd) {
        match selection {
            SelectionTarget::Clipboard => {
                let owner = current_data_device_selection_userdata(&self.seat).map(|owner| owner.clone());
                match owner {
                    Some(Owner::History(content)) => clipboard::write_selection(fd, content.bytes()),
                    Some(Owner::X11) => {}
                    None => {
                        if let Err(err) = request_data_device_client_selection(&self.seat, mime_type, fd) {
                            warn!(?err, "sélection Wayland non transmise à X11");
                        }
                    }
                }
            }
            SelectionTarget::Primary => {
                if let Err(err) = request_primary_client_selection(&self.seat, mime_type, fd) {
                    warn!(?err, "sélection primaire non transmise à X11");
                }
            }
        }
    }

    /// Une application X11 copie : les applications Wayland la voient, et le contenu
    /// rejoint l'historique.
    fn new_selection(&mut self, _xwm: XwmId, selection: SelectionTarget, mime_types: Vec<String>) {
        match selection {
            SelectionTarget::Clipboard => {
                set_data_device_selection(&self.display_handle, &self.seat, mime_types.clone(), Owner::X11);
                self.capture_x11_clipboard(&mime_types);
            }
            SelectionTarget::Primary => {
                set_primary_selection(&self.display_handle, &self.seat, mime_types, Owner::X11);
            }
        }
    }

    fn cleared_selection(&mut self, _xwm: XwmId, selection: SelectionTarget) {
        match selection {
            SelectionTarget::Clipboard => {
                let owned_by_x11 = current_data_device_selection_userdata(&self.seat)
                    .is_some_and(|owner| matches!(*owner, Owner::X11));
                if owned_by_x11 {
                    clear_data_device_selection(&self.display_handle, &self.seat);
                }
            }
            SelectionTarget::Primary => clear_primary_selection(&self.display_handle, &self.seat),
        }
    }

    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.xwm
            .as_mut()
            .expect("événement X11 sans gestionnaire de fenêtres")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn map_window_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Err(err) = surface.set_mapped(true) {
            warn!(?err, "fenêtre X11 non affichée");
            return;
        }
        let window = Window::new_x11_window(surface.clone());
        let requested = surface.geometry();
        let size = if requested.size.w > 1 && requested.size.h > 1 {
            requested.size
        } else {
            Size::from(DEFAULT_SIZE)
        };
        let decorated = wm::is_server_decorated(&window);
        let titlebar = if decorated { TITLEBAR_HEIGHT as i32 } else { 0 };
        // Boîte de dialogue : centrée sur sa fenêtre parente.
        let parent = surface.is_transient_for().and_then(|parent| {
            self.space
                .elements()
                .find(|window| window.x11_surface().is_some_and(|x11| x11.window_id() == parent))
                .and_then(|window| self.space.element_geometry(window))
        });
        let content = match self.usable_area() {
            Some(area) if parent.is_some() => {
                let parent = parent.unwrap_or(area);
                let size = Size::from((size.w.min(area.size.w), size.h.min(area.size.h - titlebar).max(1)));
                let frame_size = Size::from((size.w, size.h + titlebar));
                let frame = parent.loc
                    + Point::from((
                        (parent.size.w - frame_size.w) / 2,
                        (parent.size.h - frame_size.h) / 2,
                    ));
                let frame = wm::clamp_frame(area, frame, frame_size);
                Rectangle::new(frame + Point::from((0, titlebar)), size)
            }
            Some(area) => {
                // Jamais plus grande que la zone utile : le bas resterait sous le Dock.
                let size = Size::from((size.w.min(area.size.w), size.h.min(area.size.h - titlebar).max(1)));
                let frame_size = Size::from((size.w, size.h + titlebar));
                let frame = self.placement.next_position(area, frame_size);
                let frame = wm::clamp_frame(area, frame, frame_size);
                Rectangle::new(frame + Point::from((0, titlebar)), size)
            }
            None => Rectangle::new(requested.loc, size),
        };
        self.place_x11(&window, content);
        self.focus_window(Some(window));
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        let location = surface.geometry().loc;
        self.space
            .map_element(Window::new_x11_window(surface), location, true);
    }

    fn unmapped_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(window) = self.x11_window(&surface) {
            self.space.unmap_elem(&window);
            self.minimized.retain(|minimized| minimized.window != window);
        }
        if !surface.is_override_redirect() {
            let _ = surface.set_mapped(false);
            let next = self.top_window();
            self.focus_window(next);
        }
    }

    fn destroyed_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(window) = self.x11_window(&surface) {
            self.space.unmap_elem(&window);
            self.minimized.retain(|minimized| minimized.window != window);
        }
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        surface: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        let mut geometry = surface.geometry();
        if let Some(w) = w {
            geometry.size.w = w as i32;
        }
        if let Some(h) = h {
            geometry.size.h = h as i32;
        }
        match self.x11_window(&surface) {
            // Fenêtre déjà placée par ferrite-shell : elle garde sa position.
            Some(window) => {
                if let Some(location) = self.space.element_location(&window) {
                    geometry.loc = location;
                }
                // Une fenêtre qui s'agrandit d'elle-même reste dans la zone utile.
                if let Some(area) = self.usable_area() {
                    let titlebar = if wm::is_server_decorated(&window) {
                        TITLEBAR_HEIGHT as i32
                    } else {
                        0
                    };
                    geometry.size.w = geometry.size.w.min(area.size.w);
                    geometry.size.h = geometry.size.h.min(area.size.h - titlebar).max(1);
                }
                let _ = surface.configure(geometry);
            }
            None => {
                if let Some(x) = x {
                    geometry.loc.x = x;
                }
                if let Some(y) = y {
                    geometry.loc.y = y;
                }
                let _ = surface.configure(geometry);
            }
        }
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        surface: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _above: Option<u32>,
    ) {
        // Seuls les menus et infobulles se déplacent d'eux-mêmes.
        if !surface.is_override_redirect() {
            return;
        }
        if let Some(window) = self.x11_window(&surface) {
            self.space.map_element(window, geometry.loc, false);
        }
    }

    fn maximize_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(window) = self.x11_window(&surface) {
            self.x11_set_maximized(&window, true);
        }
    }

    fn unmaximize_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(window) = self.x11_window(&surface) {
            self.x11_set_maximized(&window, false);
        }
    }

    fn fullscreen_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(window) = self.x11_window(&surface) {
            self.x11_set_fullscreen(&window, true);
        }
    }

    fn unfullscreen_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(window) = self.x11_window(&surface) {
            self.x11_set_fullscreen(&window, false);
        }
    }

    fn minimize_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Some(window) = self.x11_window(&surface) {
            self.minimize(&window);
        }
    }

    fn resize_request(&mut self, _xwm: XwmId, surface: X11Surface, _button: u32, edge: X11ResizeEdge) {
        let Some(window) = self.x11_window(&surface) else {
            return;
        };
        let Some(start_data) = self.current_grab_start() else {
            return;
        };
        let edges = match edge {
            X11ResizeEdge::Top => ResizeEdge::TOP,
            X11ResizeEdge::Bottom => ResizeEdge::BOTTOM,
            X11ResizeEdge::Left => ResizeEdge::LEFT,
            X11ResizeEdge::Right => ResizeEdge::RIGHT,
            X11ResizeEdge::TopLeft => ResizeEdge::TOP_LEFT,
            X11ResizeEdge::TopRight => ResizeEdge::TOP_RIGHT,
            X11ResizeEdge::BottomLeft => ResizeEdge::BOTTOM_LEFT,
            X11ResizeEdge::BottomRight => ResizeEdge::BOTTOM_RIGHT,
        };
        self.start_resize(window, edges, start_data, SERIAL_COUNTER.next_serial());
    }

    fn move_request(&mut self, _xwm: XwmId, surface: X11Surface, _button: u32) {
        let Some(window) = self.x11_window(&surface) else {
            return;
        };
        let Some(start_data) = self.current_grab_start() else {
            return;
        };
        self.start_move(window, start_data, SERIAL_COUNTER.next_serial());
    }

    fn disconnected(&mut self, _xwm: XwmId) {
        warn!("XWayland s'est arrêté");
        self.xwm = None;
        self.x_display = None;
    }
}

impl XWaylandShellHandler for Ferrite {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell_state
    }
}

delegate_xwayland_shell!(Ferrite);

/// Peut-on créer un fichier dans `dir` ?
fn writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(format!(".ferrite-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}
