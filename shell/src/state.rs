//! État global du compositeur.

use std::{
    ffi::OsString,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use smithay::{
    desktop::{PopupManager, Space, Window},
    input::{Seat, SeatState, keyboard::XkbConfig, pointer::CursorImageStatus},
    reexports::{
        calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction, generic::Generic},
        wayland_server::{
            Display, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Point},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::xdg::{XdgShellState, decoration::XdgDecorationState},
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};
use tracing::{info, warn};

use crate::desktop::panels::Popover;
use crate::system::SystemLink;
use crate::wm::{Hit, Minimized, Placement, decoration::TrafficLight};
use ferrite_system::{
    Snapshot,
    config::{Settings, split_layout},
};

/// Répétition des touches : délai (ms) et fréquence (par seconde).
const KEY_REPEAT_DELAY: i32 = 300;
const KEY_REPEAT_RATE: i32 = 30;

pub struct Ferrite {
    pub start_time: Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,
    pub loop_signal: LoopSignal,
    /// Une fermeture explicite a été demandée ; les erreurs tardives du backend winit
    /// pendant sa destruction ne doivent pas transformer une fermeture normale en crash.
    pub expected_exit: bool,
    /// Menu déroulant ouvert depuis la barre du haut.
    pub popover: Option<Popover>,
    /// Dernier état connu du réseau, du Bluetooth, du son et de l'écran.
    pub system: Snapshot,
    /// Dernier refus d'un réglage système, montré dans les réglages rapides.
    pub system_error: Option<String>,
    pub system_link: Option<SystemLink>,
    pub pending_power: Option<PowerAction>,

    /// Plan 2D sur lequel sont posées les fenêtres et les écrans.
    pub space: Space<Window>,
    pub popups: PopupManager,
    pub placement: Placement,
    /// Fenêtres réduites (la plus récente en dernier).
    pub minimized: Vec<Minimized>,
    /// Dernier clic sur une barre de titre, pour reconnaître le double-clic.
    pub last_titlebar_click: Option<(Instant, Window)>,
    /// Feu tricolore enfoncé : l'action a lieu au relâchement, s'il est toujours dessus.
    pub pressed_light: Option<(Window, TrafficLight)>,
    /// Curseur demandé par le client sous le pointeur (seul « caché » est respecté pour
    /// l'instant : sinon, ferrite-shell dessine sa flèche).
    pub cursor_status: CursorImageStatus,
    /// Changement de terminal virtuel (Ctrl+Alt+F1…F12), fourni par le backend DRM.
    pub change_vt: Option<Box<dyn FnMut(i32)>>,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    /// Jamais lu, mais porte le global `xdg-decoration`.
    #[allow(dead_code)]
    pub xdg_decoration_state: XdgDecorationState,
    pub shm_state: ShmState,
    /// Jamais lu, mais doit vivre aussi longtemps que le compositeur : il porte le global
    /// `xdg-output`.
    #[allow(dead_code)]
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Ferrite>,
    pub data_device_state: DataDeviceState,
    pub seat: Seat<Ferrite>,
}

impl Ferrite {
    pub fn new(
        event_loop: &mut EventLoop<Ferrite>,
        display: Display<Ferrite>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let dh = display.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let data_device_state = DataDeviceState::new::<Self>(&dh);

        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "seat0");
        // Disposition choisie dans Paramètres (QWERTY américain par défaut).
        let layout = Settings::load().keyboard_layout().to_owned();
        let (base, variant) = split_layout(&layout);
        let xkb = XkbConfig {
            layout: &base,
            variant: &variant,
            ..XkbConfig::default()
        };
        if let Err(err) = seat.add_keyboard(xkb, KEY_REPEAT_DELAY, KEY_REPEAT_RATE) {
            warn!("disposition « {layout} » indisponible ({err:?}), clavier par défaut");
            seat.add_keyboard(XkbConfig::default(), KEY_REPEAT_DELAY, KEY_REPEAT_RATE)?;
        }
        seat.add_pointer();

        let socket_name = listen(display, event_loop)?;

        Ok(Ferrite {
            start_time: Instant::now(),
            socket_name,
            display_handle: dh,
            loop_signal: event_loop.get_signal(),
            expected_exit: false,
            popover: None,
            system: Snapshot::default(),
            system_error: None,
            system_link: None,
            pending_power: None,
            space: Space::default(),
            popups: PopupManager::default(),
            placement: Placement::default(),
            minimized: Vec::new(),
            last_titlebar_click: None,
            pressed_light: None,
            cursor_status: CursorImageStatus::default_named(),
            change_vt: None,
            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            seat,
        })
    }

    /// Surface sous un point de l'espace, avec la position de son origine (aucune si le
    /// point est sur une décoration).
    pub fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        match self.hit_test(pos)? {
            Hit::Surface(_, surface, location) => Some((surface, location)),
            Hit::Decoration(..) => None,
        }
    }

    /// Fenêtre dont la surface principale est `surface`.
    pub fn window_for_surface(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .find(|window| {
                window
                    .toplevel()
                    .is_some_and(|toplevel| toplevel.wl_surface() == surface)
            })
            .cloned()
    }

    /// Change la disposition du clavier (choisie dans Paramètres).
    pub fn set_keyboard_layout(&mut self, layout: &str) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let (base, variant) = split_layout(layout);
        let xkb = XkbConfig {
            layout: &base,
            variant: &variant,
            ..XkbConfig::default()
        };
        match keyboard.set_xkb_config(self, xkb) {
            Ok(()) => info!(layout, "disposition du clavier appliquée"),
            Err(err) => warn!(layout, ?err, "disposition du clavier refusée"),
        }
    }

    /// Demande aux clients de fermer leurs fenêtres avant l'arrêt du compositeur.
    pub fn request_exit(&mut self) {
        self.expected_exit = true;
        for window in self.space.elements() {
            if let Some(toplevel) = window.toplevel() {
                toplevel.send_close();
            }
        }
        if let Err(err) = self.display_handle.flush_clients() {
            warn!("envoi des demandes de fermeture impossible : {err}");
        }
        // Laisser aux clients un court instant pour recevoir le protocole xdg_toplevel.close.
        thread::sleep(Duration::from_millis(200));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    PowerOff,
    Restart,
}

/// Ouvre le socket Wayland (`wayland-N` libre suivant) et branche les clients sur la boucle.
fn listen(
    display: Display<Ferrite>,
    event_loop: &mut EventLoop<Ferrite>,
) -> Result<OsString, Box<dyn std::error::Error>> {
    let socket = ListeningSocketSource::new_auto()?;
    let socket_name = socket.socket_name().to_os_string();
    let handle = event_loop.handle();

    handle.insert_source(socket, |stream, _, state| {
        if let Err(err) = state
            .display_handle
            .insert_client(stream, Arc::new(ClientState::default()))
        {
            warn!("connexion d'un client refusée : {err}");
        }
    })?;

    handle.insert_source(
        Generic::new(display, Interest::READ, Mode::Level),
        |_, display, state| {
            // SAFETY : l'affichage n'est jamais supprimé pendant la boucle ; Smithay exige
            // `unsafe` car `get_mut` permettrait de le remplacer.
            #[allow(unsafe_code)]
            unsafe {
                display.get_mut().dispatch_clients(state)?;
            }
            Ok(PostAction::Continue)
        },
    )?;

    Ok(socket_name)
}

/// Données associées à chaque client Wayland.
#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
