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
        cursor_shape::CursorShapeManagerState,
        dmabuf::{DmabufGlobal, DmabufState},
        fractional_scale::FractionalScaleManagerState,
        idle_inhibit::IdleInhibitManagerState,
        idle_notify::IdleNotifierState,
        input_method::InputMethodManagerState,
        output::OutputManagerState,
        relative_pointer::RelativePointerManagerState,
        seat::WaylandFocus,
        selection::{data_device::DataDeviceState, primary_selection::PrimarySelectionState},
        shell::xdg::{XdgShellState, decoration::XdgDecorationState},
        shm::ShmState,
        socket::ListeningSocketSource,
        text_input::TextInputManagerState,
        viewporter::ViewporterState,
        xdg_activation::XdgActivationState,
        xwayland_shell::XWaylandShellState,
    },
    xwayland::X11Wm,
};
use tracing::{info, warn};

use crate::desktop::panels::Popover;
use crate::desktop::widgets::{Dashboard, DashboardEdit};
use crate::system::{SystemLink, WallpaperChoice};
use crate::wm::{Hit, Minimized, decoration::WindowButton};
use ferrite_system::{
    Snapshot,
    applications::DesktopEntry,
    config::{Accent, Appearance, Settings, split_layout},
};

/// Disposition du clavier telle qu'affichée : « FR (AZERTY) ».
fn keyboard_label(layout: &str) -> String {
    let (base, _) = split_layout(layout);
    let family = match base.as_str() {
        "fr" | "be" => " (AZERTY)",
        "us" | "gb" => " (QWERTY)",
        "de" | "ch" => " (QWERTZ)",
        _ => "",
    };
    format!("{}{family}", base.to_uppercase())
}

/// Répétition des touches : délai (ms) et fréquence (par seconde).
pub const KEY_REPEAT_DELAY: i32 = 300;
pub const KEY_REPEAT_RATE: i32 = 30;

pub struct Ferrite {
    pub start_time: Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,
    pub loop_signal: LoopSignal,
    /// Une fermeture explicite a été demandée ; les erreurs tardives du backend winit
    /// pendant sa destruction ne doivent pas transformer une fermeture normale en crash.
    pub expected_exit: bool,
    /// Code de sortie demandé par « Se déconnecter » ou « Relancer le bureau ».
    pub exit_code: u8,
    /// Menu déroulant ouvert depuis la barre du haut.
    pub popover: Option<Popover>,
    /// Applications installées (fichiers `.desktop`), relues par le fil système.
    pub applications: Vec<DesktopEntry>,
    /// État chargé avant le démarrage du fil qui fournit les applications installées.
    pub session_to_restore: Option<crate::session::Session>,
    /// Fenêtres en attente de leur application au démarrage.
    pub session_restore_queue: Vec<crate::session::SavedWindow>,
    /// Garde les espaces restaurés en place pendant le lancement des applications.
    pub session_restore_deadline: Option<Instant>,
    /// La restauration ne s'applique qu'à la vraie session, pas aux bureaux imbriqués.
    pub session_restore_enabled: bool,
    /// Recherche tapée dans le menu Applications.
    pub launcher_query: String,
    pub launcher_scroll: u32,
    /// Volume et luminosité réglés à la main récemment : la relecture du système attend.
    pub volume_held_until: Option<Instant>,
    pub brightness_held_until: Option<Instant>,
    /// Taille et masquage du Dock (Paramètres).
    pub dock_config: crate::desktop::dock::DockConfig,
    /// La barre du haut masquée a été rappelée par la souris (voir `bar_visible`).
    pub bar_revealed: std::cell::Cell<bool>,
    /// Zone (cadre) où la fenêtre déplacée s'accrochera si on la lâche maintenant.
    pub snap_preview: Option<smithay::utils::Rectangle<i32, smithay::utils::Logical>>,
    /// Historique du presse-papiers (Super+V) et état de son panneau.
    pub clipboard: crate::clipboard::History,
    pub clipboard_sender: Option<crate::clipboard::Copies>,
    /// Panneau Super+V : ligne choisie (dans la liste montrée), défilement en points,
    /// recherche, filtre, et liste d'avant la dernière suppression (pour « Annuler »).
    pub clipboard_selected: usize,
    pub clipboard_scroll: f64,
    pub clipboard_query: String,
    pub clipboard_filter: crate::clipboard::Filter,
    pub clipboard_undo: Option<crate::clipboard::Undo>,
    pub clipboard_position: Option<(i32, i32)>,
    pub clipboard_drag_offset: Option<(i32, i32)>,
    /// Touche Super enfoncée, et utilisée avec une autre touche (Super+V) : relâchée
    /// seule, elle ouvre le menu Applications, comme sous Windows.
    pub super_held: bool,
    pub super_used: bool,
    pub loop_handle: smithay::reexports::calloop::LoopHandle<'static, Ferrite>,
    /// Applications épinglées dans le Dock (`None` : Dock par défaut), applications à
    /// plusieurs fenêtres, ordre des applications ouvertes non épinglées.
    pub dock_pinned: Option<Vec<String>>,
    pub multi_window: Vec<String>,
    pub dock_order: std::cell::RefCell<Vec<crate::desktop::apps::AppKey>>,
    /// Icône épinglée tenue au clic gauche (clic ou déplacement dans le Dock).
    pub dock_drag: Option<crate::desktop::launch::DockDrag>,
    /// Bureau quitté par Super+W pour celui des widgets, où Super+W ramène.
    pub page_before_widgets: Option<usize>,
    /// Bulle d'une touche de fonction et son échéance.
    pub osd: Option<(crate::desktop::panels::Osd, std::time::Instant)>,
    pub dock_bounces: std::cell::RefCell<
        std::collections::HashMap<crate::desktop::apps::AppKey, crate::desktop::dock::Bounce>,
    >,
    /// Glissement et agrandissement du Dock.
    pub dock_motion: crate::desktop::dock::DockMotion,
    pub dock_pointer_motion_at: std::cell::Cell<Option<Instant>>,
    pub dock_fast_approach_until: std::cell::Cell<Option<Instant>>,
    /// Vitesse de descente lissée de la souris (pixels logiques par seconde).
    pub dock_descent_speed: std::cell::Cell<f64>,
    /// Le Dock était-il affiché à la dernière image.
    pub dock_was_shown: std::cell::Cell<bool>,
    /// Menu d'une icône du Dock (clic droit) : application, choix, et position.
    pub dock_menu: Option<(
        crate::desktop::apps::AppKey,
        Vec<crate::desktop::launch::DockMenuAction>,
        smithay::utils::Point<f64, smithay::utils::Logical>,
    )>,
    /// Position du clic qui a ouvert ce menu : l'agrandissement y reste figé.
    pub dock_menu_pointer: Option<smithay::utils::Point<f64, smithay::utils::Logical>>,
    /// Fenêtres en train d'être réduites ou rappelées.
    pub window_animations: std::cell::RefCell<Vec<crate::wm::animation::WindowAnimation>>,
    /// Bords tirés pendant un redimensionnement à la souris (forme du curseur).
    pub resizing: Option<crate::wm::ResizeEdge>,
    /// Curseur des réglages rapides en cours de glissement.
    pub slider_drag: Option<crate::desktop::panels::Slider>,
    /// Dernier état connu du réseau, du Bluetooth, du son et de l'écran.
    pub system: Snapshot,
    /// Dernier refus d'un réglage système, montré dans les réglages rapides.
    pub system_error: Option<String>,
    pub system_link: Option<SystemLink>,
    /// Fondu au noir avant le verrouillage ou la mise en veille.
    pub screen_fade: Option<(Instant, bool)>,
    pub pending_power: Option<PowerAction>,
    /// Fin du compte à rebours du dialogue d'arrêt : l'action choisie se fait seule.
    pub power_deadline: Option<std::time::Instant>,
    /// Photo de l'écran de verrouillage (réglage `verrouillage_fond` en chemin), décodée.
    pub lock_picture: Option<crate::desktop::panels::LockPicture>,
    /// Mode nuit : réglages, forçage depuis le centre de contrôle et teinte de l'écran.
    pub night: crate::desktop::night::Night,
    /// Inactivité : verrouillage et mise en veille automatiques.
    pub idle: crate::desktop::idle::Idle,
    pub appearance: Appearance,
    pub accent: Accent,
    pub scroll_inverted: bool,
    pub wallpaper: WallpaperChoice,

    /// Plan 2D sur lequel sont posées les fenêtres et les écrans.
    pub space: Space<Window>,
    pub workspaces: crate::wm::workspaces::Workspaces<crate::wm::workspaces::Parked>,
    pub swipe: Option<crate::wm::workspaces::Swipe>,
    pub overview: bool,
    /// Listes Wi-Fi et Bluetooth du centre de contrôle.
    pub controls: crate::desktop::controls::Controls,
    /// Écran de verrouillage affiché, et canal de ses réponses.
    pub lock: Option<crate::desktop::lock::Lock>,
    pub lock_events: Option<smithay::reexports::calloop::channel::Sender<crate::desktop::lock::Event>>,
    pub media_link: Option<crate::desktop::lock::MediaLink>,
    /// Lecteur multimédia suivi (écran verrouillé, widget Musique).
    pub media: Option<crate::desktop::lock::Media>,
    /// Tuilage automatique (Super+T), fenêtres dans leur ordre de tuilage, et fenêtres
    /// réduites par « afficher le bureau » (Super+D) à remettre.
    pub tiling: bool,
    pub tile_order: Vec<Window>,
    /// Touche maintenue dans un champ du bureau : son code, son action et sa génération
    /// (un nouvel appui rend caduque la minuterie de l'ancien).
    pub key_repeat: Option<(u32, crate::input::Action, u64)>,
    pub key_repeat_generation: u64,
    /// Zone utile au dernier tour de boucle : quand elle change (écran ou fenêtre hôte
    /// redimensionnés, Dock ou barre réglés), les fenêtres agrandies et le tuilage suivent.
    pub last_usable_area: Option<smithay::utils::Rectangle<i32, smithay::utils::Logical>>,
    pub desktop_shown: Vec<Window>,
    /// Dernier accrochage au clavier : Super+← répété passe de la moitié aux deux tiers, puis au tiers.
    pub last_snap: Option<(Window, crate::wm::Snap)>,
    /// Économie d'énergie demandée, et clavier affiché dans le centre de contrôle.
    pub power_saver: bool,
    pub keyboard_label: String,
    /// Réglages des raccourcis du bureau, rechargés à chaque changement de configuration.
    pub keyboard_shortcuts: Settings,
    /// Notifications reçues, et lien avec le service du bus de session.
    pub notifications: crate::desktop::notifications::Center,
    pub notification_link: Option<crate::desktop::notifications::Link>,
    /// Sélecteur Alt+Tab ouvert.
    pub switcher: Option<crate::wm::switcher::Switcher>,
    /// Sélection à la souris de la zone à capturer, et capture à faire à la prochaine image.
    pub region_select: Option<crate::render::capture::RegionSelect>,
    /// Zone déjà tracée, en cours d'ajustement avant la prise.
    pub region_adjust: Option<crate::render::capture::RegionAdjust>,
    pub screenshot_request: Option<crate::render::capture::Request>,
    pub screenshot_mode: crate::render::capture::Mode,
    pub screenshot_save: bool,
    pub screenshot_clipboard: bool,
    pub screenshot_cursor: bool,
    /// Délai avant la prise, en secondes (0 : immédiate).
    pub screenshot_delay: u64,
    /// Dernière zone choisie, proposée à la prochaine capture en mode Sélection.
    pub screenshot_area: Option<smithay::utils::Rectangle<i32, Logical>>,
    /// Barre de capture réglée sur la vidéo plutôt que la photo.
    pub screenshot_video: bool,
    /// Enregistrement vidéo en cours.
    pub recording: Option<crate::desktop::recording::Recording>,
    /// Minuteur de l'Horloge suivi par l'îlot, îlot déplié par un clic, et seconde suivie.
    pub island_timer: Option<crate::desktop::island::TimerActivity>,
    pub island_open_until: Option<std::time::Instant>,
    pub island_ticking: bool,
    /// Activité dépliée par un clic (une bulle), sinon la première.
    pub island_focus: Option<crate::desktop::island::IslandFocus>,
    /// Sélection d'une fenêtre à capturer : mode actif, et fenêtre survolée.
    pub window_select: bool,
    pub window_select_hover: Option<Window>,
    /// Fenêtres de la vue d'ensemble, dans l'ordre de son ouverture : une fenêtre garde
    /// sa case même si on la met au premier plan.
    pub overview_order: Vec<Window>,
    /// Part affichée de la vue d'ensemble, de 0 à 1, et dernier pas de son animation.
    pub overview_shown: std::cell::Cell<(f64, Option<std::time::Instant>)>,
    pub popups: PopupManager,
    /// Fenêtres réduites (la plus récente en dernier).
    pub minimized: Vec<Minimized>,
    pub dashboard: Dashboard,
    pub dashboard_editing: Option<DashboardEdit>,
    /// Super+W : widgets ramenés devant les fenêtres.
    pub widgets_front: bool,
    /// Mode d'édition des widgets, et widget en train d'être déplacé ou redimensionné.
    pub widgets_edit: bool,
    pub widget_drag: Option<crate::input::WidgetDrag>,
    /// Part de l'estompe des widgets, recouverts par une fenêtre.
    pub widgets_faded: std::cell::Cell<(f64, Option<std::time::Instant>)>,
    /// Molette accumulée au-dessus d'un widget.
    pub widget_scroll: f64,
    /// Coin du menu du bureau.
    pub desktop_menu: Option<(i32, i32)>,
    /// Dernier clic sur une barre de titre, pour reconnaître le double-clic.
    pub last_titlebar_click: Option<(Instant, Window)>,
    /// Feu tricolore enfoncé : l'action a lieu au relâchement, s'il est toujours dessus.
    pub pressed_light: Option<(Window, WindowButton)>,
    /// Curseur demandé par le client sous le pointeur (seul « caché » est respecté pour
    /// l'instant : sinon, ferrite-shell dessine sa flèche).
    pub cursor_status: CursorImageStatus,
    /// Changement de terminal virtuel (Ctrl+Alt+F1…F12), fourni par le backend DRM.
    pub change_vt: Option<Box<dyn FnMut(i32)>>,
    /// Fichiers glissés depuis Fichiers (voir `desktop::file_drag`).
    pub file_drag: Option<crate::desktop::file_drag::FileDrag>,
    /// Applications récemment utilisées (jetons, la plus récente d'abord).
    pub recent_apps: Vec<String>,
    /// Demande au backend DRM de dessiner sans attendre son prochain essai au repos.
    pub redraw: Option<smithay::reexports::calloop::ping::Ping>,
    /// Fraction figée pour la démonstration `--demo-genie`.
    pub genie_demo: Option<(f64, Option<Instant>)>,
    /// Options de démonstration : actions faites à l'apparition de la première fenêtre.
    pub demo_switcher: bool,
    pub demo_screenshot: bool,

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
    pub primary_selection_state: PrimarySelectionState,
    pub dmabuf_state: DmabufState,
    pub dmabuf_global: Option<DmabufGlobal>,
    pub activation_state: XdgActivationState,
    pub xwayland_shell_state: XWaylandShellState,
    /// Globaux sans état à consulter, gardés en vie avec le compositeur.
    #[allow(dead_code)]
    pub passive_globals: (
        ViewporterState,
        FractionalScaleManagerState,
        RelativePointerManagerState,
        CursorShapeManagerState,
        TextInputManagerState,
        InputMethodManagerState,
        IdleInhibitManagerState,
    ),
    /// Inactivité annoncée aux clients (`ext-idle-notify`), et surfaces qui l'empêchent.
    pub idle_notifier_state: IdleNotifierState<Ferrite>,
    pub idle_inhibitors: Vec<WlSurface>,
    /// Fenêtres de candidats des méthodes de saisie.
    pub input_popups: Vec<smithay::wayland::input_method::PopupSurface>,
    pub layer_shell_state: smithay::wayland::shell::wlr_layer::WlrLayerShellState,
    /// Verrouillage par un client (`ext-session-lock`), et son gestionnaire.
    pub session_lock_state: smithay::wayland::session_lock::SessionLockManagerState,
    pub session_lock: Option<crate::protocols::session_lock::ClientLock>,
    /// Demandes de mot de passe administrateur (polkit), la première affichée.
    pub auth: std::collections::VecDeque<crate::desktop::polkit::Auth>,
    pub polkit_link: Option<crate::desktop::polkit::Link>,
    /// Copies d'écran demandées par les clients (`wlr-screencopy`), faites à l'image suivante.
    pub screencopy: Vec<crate::protocols::screencopy::PendingCopy>,
    /// Gestionnaire des fenêtres X11, une fois XWayland prêt.
    pub xwm: Option<X11Wm>,
    /// Numéro d'affichage X11 (`DISPLAY=:N`) d'XWayland.
    pub x_display: Option<u32>,
    pub seat: Seat<Ferrite>,
}

impl Ferrite {
    pub fn new(
        event_loop: &mut EventLoop<'static, Ferrite>,
        display: Display<Ferrite>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let dh = display.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let primary_selection_state = PrimarySelectionState::new::<Self>(&dh);
        let activation_state = XdgActivationState::new::<Self>(&dh);
        let xwayland_shell_state = XWaylandShellState::new::<Self>(&dh);
        let passive_globals = (
            ViewporterState::new::<Self>(&dh),
            FractionalScaleManagerState::new::<Self>(&dh),
            RelativePointerManagerState::new::<Self>(&dh),
            CursorShapeManagerState::new::<Self>(&dh),
            TextInputManagerState::new::<Self>(&dh),
            InputMethodManagerState::new::<Self, _>(&dh, |_client| true),
            IdleInhibitManagerState::new::<Self>(&dh),
        );
        let idle_notifier_state = IdleNotifierState::new(&dh, event_loop.handle());
        crate::protocols::screencopy::init(&dh);
        let layer_shell_state = smithay::wayland::shell::wlr_layer::WlrLayerShellState::new::<Self>(&dh);
        let session_lock_state =
            smithay::wayland::session_lock::SessionLockManagerState::new::<Self, _>(&dh, |_client| true);
        let settings = Settings::load();

        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "seat0");
        // Disposition choisie dans Paramètres (QWERTY américain par défaut).
        let layout = settings.keyboard_layout().to_owned();
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
            exit_code: 0,
            popover: None,
            applications: Vec::new(),
            session_to_restore: None,
            session_restore_queue: Vec::new(),
            session_restore_deadline: None,
            session_restore_enabled: false,
            launcher_query: String::new(),
            launcher_scroll: 0,
            slider_drag: None,
            dock_config: crate::desktop::dock::DockConfig::from_settings(&settings),
            bar_revealed: Default::default(),
            snap_preview: None,
            clipboard: crate::clipboard::History::load(),
            clipboard_sender: None,
            clipboard_selected: 0,
            clipboard_scroll: 0.0,
            clipboard_query: String::new(),
            clipboard_filter: crate::clipboard::Filter::All,
            clipboard_undo: None,
            clipboard_position: None,
            clipboard_drag_offset: None,
            super_held: false,
            super_used: false,
            loop_handle: event_loop.handle(),
            dock_pinned: settings.dock_pinned(),
            multi_window: settings.multi_window_apps(),
            dock_order: Default::default(),
            dock_drag: None,
            page_before_widgets: None,
            osd: None,
            dock_bounces: Default::default(),
            dock_motion: Default::default(),
            dock_pointer_motion_at: Default::default(),
            dock_fast_approach_until: Default::default(),
            dock_descent_speed: Default::default(),
            dock_was_shown: Default::default(),
            dock_menu: None,
            window_animations: Default::default(),
            resizing: None,
            volume_held_until: None,
            brightness_held_until: None,
            system: Snapshot::default(),
            system_error: None,
            system_link: None,
            screen_fade: None,
            pending_power: None,
            power_deadline: None,
            lock_picture: None,
            night: Default::default(),
            idle: Default::default(),
            appearance: settings.appearance(),
            accent: settings.accent(),
            scroll_inverted: settings.scroll_inverted(),
            wallpaper: WallpaperChoice::default(),
            space: Space::default(),
            workspaces: Default::default(),
            swipe: None,
            overview: false,
            controls: Default::default(),
            lock: None,
            lock_events: None,
            media_link: None,
            media: None,
            tiling: false,
            tile_order: Vec::new(),
            key_repeat: None,
            key_repeat_generation: 0,
            last_usable_area: None,
            desktop_shown: Vec::new(),
            last_snap: None,
            power_saver: false,
            keyboard_label: keyboard_label(&layout),
            keyboard_shortcuts: settings.clone(),
            notifications: Default::default(),
            notification_link: None,
            switcher: None,
            region_select: None,
            region_adjust: None,
            screenshot_request: None,
            screenshot_mode: crate::render::capture::Mode::Screen,
            screenshot_save: true,
            screenshot_clipboard: true,
            screenshot_cursor: false,
            screenshot_delay: 0,
            screenshot_area: None,
            screenshot_video: false,
            recording: None,
            island_timer: None,
            island_open_until: None,
            island_ticking: false,
            island_focus: None,
            window_select: false,
            window_select_hover: None,
            overview_order: Vec::new(),
            overview_shown: std::cell::Cell::new((0.0, None)),
            popups: PopupManager::default(),
            minimized: Vec::new(),
            dashboard: Dashboard::load(),
            dashboard_editing: None,
            widgets_front: false,
            widgets_edit: false,
            widget_drag: None,
            widgets_faded: std::cell::Cell::new((0.0, None)),
            widget_scroll: 0.0,
            desktop_menu: None,
            dock_menu_pointer: None,
            last_titlebar_click: None,
            pressed_light: None,
            cursor_status: CursorImageStatus::default_named(),
            change_vt: None,
            redraw: None,
            file_drag: None,
            recent_apps: crate::desktop::launch::load_recent_apps(),
            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            primary_selection_state,
            dmabuf_state: DmabufState::new(),
            dmabuf_global: None,
            activation_state,
            xwayland_shell_state,
            passive_globals,
            idle_notifier_state,
            idle_inhibitors: Vec::new(),
            input_popups: Vec::new(),
            layer_shell_state,
            session_lock_state,
            session_lock: None,
            auth: Default::default(),
            polkit_link: None,
            screencopy: Vec::new(),
            xwm: None,
            x_display: None,
            seat,
            genie_demo: None,
            demo_switcher: false,
            demo_screenshot: false,
        })
    }

    /// Surface sous un point de l'espace, avec la position de son origine (aucune si le
    /// point est sur une décoration).
    /// Surface sous `pos` : celle d'un verrouilleur externe s'il y en a un ; sinon les
    /// calques du haut, les fenêtres, puis les calques du bas.
    pub fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        use smithay::wayland::shell::wlr_layer::Layer;
        if self.session_lock.is_some() {
            return self.lock_surface_under(pos);
        }
        if self.lock.is_some() {
            return None;
        }
        if let Some(found) = self.layer_surface_under(pos, &[Layer::Overlay, Layer::Top]) {
            return Some(found);
        }
        if self.over_shell_chrome(pos) {
            return None;
        }
        match self.hit_test(pos) {
            Some(Hit::Surface(_, surface, location)) => Some((surface, location)),
            Some(Hit::Decoration(..)) => None,
            None if self.overview || self.region_select.is_some() || self.region_adjust.is_some() => None,
            None => self.layer_surface_under(pos, &[Layer::Bottom, Layer::Background]),
        }
    }

    /// Surface qui a le clavier.
    pub fn keyboard_focus(&self) -> Option<WlSurface> {
        self.seat.get_keyboard()?.current_focus()
    }

    /// Donne le clavier à `surface` (un calque, un verrouilleur).
    pub fn set_keyboard_focus(&mut self, surface: Option<WlSurface>) {
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, surface, smithay::utils::SERIAL_COUNTER.next_serial());
        }
    }

    /// Fenêtre dont la surface principale est `surface`.
    pub fn window_for_surface(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .find(|window| window.wl_surface().is_some_and(|own| *own == *surface))
            .cloned()
    }

    /// Fenêtre sous `pos`, de celle du dessus vers celle du dessous, cadre (barre de titre
    /// comprise) pris en compte. Indépendant de `hit_test`, donc utilisable pendant la
    /// sélection d'une fenêtre à capturer.
    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<Window> {
        self.space
            .elements()
            .rev()
            .find(|window| {
                self.window_frame(window)
                    .is_some_and(|frame| frame.to_f64().contains(pos))
            })
            .cloned()
    }

    /// Variables d'environnement à donner aux applications lancées : la socket Wayland et,
    /// une fois XWayland prêt, l'affichage X11.
    pub fn client_environment(&self) -> Vec<(String, String)> {
        let mut env = vec![
            (
                "WAYLAND_DISPLAY".to_owned(),
                self.socket_name.to_string_lossy().into_owned(),
            ),
            ("XDG_CURRENT_DESKTOP".to_owned(), "Ferrite".to_owned()),
            ("XDG_SESSION_TYPE".to_owned(), "wayland".to_owned()),
            // Applications GTK, Qt, Firefox, Electron et SDL en Wayland natif : elles
            // dessinent à la résolution de l'écran au lieu d'être agrandies (floues) par
            // XWayland. X11 reste le repli.
            ("GDK_BACKEND".to_owned(), "wayland,x11".to_owned()),
            ("QT_QPA_PLATFORM".to_owned(), "wayland;xcb".to_owned()),
            ("MOZ_ENABLE_WAYLAND".to_owned(), "1".to_owned()),
            ("ELECTRON_OZONE_PLATFORM_HINT".to_owned(), "auto".to_owned()),
            ("SDL_VIDEODRIVER".to_owned(), "wayland,x11".to_owned()),
        ];
        if let Some(display) = self.x_display {
            env.push(("DISPLAY".to_owned(), format!(":{display}")));
        }
        env
    }

    /// Transmet l'environnement de session à D-Bus, pour que les services
    /// lancés à la demande (portails, dialogues de fichiers) trouvent l'affichage.
    pub fn publish_environment(&self) {
        let mut command = std::process::Command::new("dbus-update-activation-environment");
        // Aussi pour systemd (--user) : les services du bus de session qu'il démarre
        // (portails…) doivent trouver l'écran.
        command.arg("--systemd");
        for (key, value) in self.client_environment() {
            command.arg(format!("{key}={value}"));
        }
        if let Err(err) = ferrite_system::command::spawn_detached(command) {
            warn!(%err, "environnement de session non transmis à D-Bus");
        }
    }

    /// Cases du menu Applications, selon la recherche en cours.
    pub fn launcher_entries(&self) -> Vec<crate::desktop::apps::LauncherEntry<'_>> {
        crate::desktop::apps::launcher_entries(&self.applications, &self.launcher_query)
    }

    /// Pointeur qui agrandit les icônes du Dock : celui du clic tant que le menu d'une icône
    /// est ouvert, pour que l'agrandissement reste figé sous le menu.
    pub fn dock_pointer(&self) -> Option<Point<f64, Logical>> {
        if self.popover == Some(crate::desktop::panels::Popover::DockMenu)
            && let Some(frozen) = self.dock_menu_pointer
        {
            return Some(frozen);
        }
        self.pointer_location()
    }

    /// Quelque chose a changé à l'écran : l'image suivante part tout de suite.
    pub fn request_redraw(&self) {
        if let Some(redraw) = &self.redraw {
            redraw.ping();
        }
    }

    pub fn animate_opening(&mut self, window: &Window) {
        let Some(rect) = self.target_content(window) else {
            return;
        };
        let rect = crate::wm::animation_frame(window, rect);
        let mut animations = self.window_animations.borrow_mut();
        animations.retain(|animation| animation.window != *window);
        animations.push(crate::wm::animation::WindowAnimation::new(
            window.clone(),
            crate::wm::animation::Motion::Open,
            rect.to_f64(),
            rect.to_f64(),
        ));
        drop(animations);
        self.request_redraw();
    }

    pub fn animate_frame_change(&mut self, window: &Window, change: impl FnOnce(&mut Self)) {
        // Une fenêtre qui glisse encore repart d'où elle est à l'écran, pas de sa place
        // d'arrivée : le mouvement s'enchaîne sans saut.
        let moving = self.window_animations.borrow().iter().find_map(|animation| {
            (animation.window == *window && animation.motion == crate::wm::animation::Motion::Zoom)
                .then(|| animation.genie(animation.window_rect, animation.dock_rect).window)
        });
        let before = moving.or_else(|| {
            self.space
                .element_geometry(window)
                .map(|rect| crate::wm::animation_frame(window, rect))
        });
        change(self);
        let (Some(before), Some(after)) = (before, self.target_content(window)) else {
            return;
        };
        let after = crate::wm::animation_frame(window, after);
        if before == after {
            return;
        }
        let mut animations = self.window_animations.borrow_mut();
        animations.retain(|animation| animation.window != *window);
        animations.push(crate::wm::animation::WindowAnimation::new(
            window.clone(),
            crate::wm::animation::Motion::Zoom,
            before,
            after,
        ));
        drop(animations);
        self.request_redraw();
    }

    pub fn run_genie_demo(&mut self) {
        if self.top_window().is_some() {
            if std::mem::take(&mut self.demo_switcher) {
                self.switcher_step(1);
            }
            if std::mem::take(&mut self.demo_screenshot) {
                self.screenshot_request = Some(crate::render::capture::Request {
                    region: None,
                    save: true,
                    clipboard: false,
                    cursor: false,
                });
            }
        }
        let Some((fraction, None)) = self.genie_demo else {
            return;
        };
        // La fenêtre doit avoir dessiné : son animation d'ouverture remplacerait la démo.
        let Some(window) = self.top_window().filter(|window| window.geometry().size.w > 0) else {
            return;
        };
        let Some(window_rect) = self.space.element_geometry(&window) else {
            return;
        };
        let key = self.window_app(&window);
        let Some(dock_rect) = self.dock_icon_rect(key.as_ref()) else {
            return;
        };
        let now = Instant::now();
        let frame = crate::wm::animation_frame(&window, window_rect);
        let mut animation = crate::wm::animation::WindowAnimation::new(
            window,
            crate::wm::animation::Motion::Minimize,
            frame,
            dock_rect,
        );
        animation.frozen = Some(fraction);
        self.window_animations.borrow_mut().push(animation);
        self.genie_demo = Some((fraction, Some(now)));
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
            Ok(()) => {
                info!(layout, "disposition du clavier appliquée");
                self.keyboard_label = keyboard_label(layout);
            }
            Err(err) => warn!(layout, ?err, "disposition du clavier refusée"),
        }
    }

    /// Demande aux clients de fermer leurs fenêtres avant l'arrêt du compositeur.
    pub fn request_exit(&mut self) {
        self.expected_exit = true;
        for window in self.space.elements() {
            crate::wm::close_window(window);
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
    /// Mise en veille (`systemctl suspend`) : le bureau reste ouvert.
    Sleep,
    /// Ferme le bureau et revient à la console de `ferrite-session`.
    LogOut,
    /// Ferme le bureau ; `ferrite-session` le relance aussitôt.
    RelaunchDesktop,
}

impl PowerAction {
    /// Code de sortie de ferrite-shell lu par `ferrite-session`, pour les actions qui ne
    /// touchent que la session (0 reste Ctrl+Alt+Retour arrière, 1 un plantage).
    pub fn exit_code(self) -> Option<u8> {
        match self {
            PowerAction::PowerOff | PowerAction::Restart | PowerAction::Sleep => None,
            PowerAction::RelaunchDesktop => Some(3),
            PowerAction::LogOut => Some(4),
        }
    }
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
