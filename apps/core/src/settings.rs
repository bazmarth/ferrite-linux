//! Paramètres, d'après les planches : une colonne de rubriques par catégorie, et à droite un
//! bandeau suivi de groupes de lignes sur deux colonnes. Une ligne de la maquette sans
//! fonction réelle reste affichée, grisée (voir `rows::Row::unavailable`).
//!
//! Toutes les commandes système passent par `ferrite-system` dans des fils d'exécution
//! à part : l'interface ne se fige jamais, même si un service ne répond pas.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

/// Durée pendant laquelle un curseur réglé à la main n'est pas remplacé par une relecture.
const HOLD: Duration = Duration::from_secs(2);

mod rows;

use self::rows::{Lead, Row, group, hero};
use eframe::egui::{self, Color32, Frame, Margin, RichText, Stroke};
use ferrite_system::{
    Snapshot,
    about::{self, About},
    audio::{self, Sink, Stream},
    bluetooth,
    config::{
        Accent, Appearance, ClockStyle, CornerAction, DockHiding, DockSize, DoubleClick, KEYBOARD_LAYOUTS,
        ScrollBars, Settings,
    },
    display,
    network::{self, KnownWifi, WifiNetwork, WifiOptions},
    time,
};
use ferrite_ui::wallpaper::{self, Builtin, Image, Source};

use crate::details;
use crate::style::{self, Icons};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Network,
    Bluetooth,
    Wired,
    Desktop,
    Dock,
    Windows,
    Notifications,
    Sound,
    Energy,
    Displays,
    Keyboard,
    Mouse,
    Printers,
    Accounts,
    Privacy,
    Accessibility,
    Updates,
    DateTime,
    About,
}

impl Page {
    /// Rubrique demandée par son nom, sur la ligne de commande.
    fn from_key(key: &str) -> Option<Page> {
        Some(match key {
            "wifi" => Page::Network,
            "bluetooth" => Page::Bluetooth,
            "reseau" => Page::Wired,
            "apparence" => Page::Desktop,
            "dock" => Page::Dock,
            "fenetres" => Page::Windows,
            "son" => Page::Sound,
            "ecrans" => Page::Displays,
            "clavier" => Page::Keyboard,
            "souris" => Page::Mouse,
            "comptes" => Page::Accounts,
            "date" => Page::DateTime,
            "apropos" => Page::About,
            _ => return None,
        })
    }

    /// Rubriques de la colonne, par catégorie.
    const GROUPS: [(&'static str, &'static [Page]); 4] = [
        ("Connexions", &[Page::Network, Page::Bluetooth, Page::Wired]),
        (
            "Personnalisation",
            &[Page::Desktop, Page::Dock, Page::Windows, Page::Notifications],
        ),
        (
            "Matériel",
            &[
                Page::Sound,
                Page::Energy,
                Page::Displays,
                Page::Keyboard,
                Page::Mouse,
                Page::Printers,
            ],
        ),
        (
            "Système",
            &[
                Page::Accounts,
                Page::Privacy,
                Page::Accessibility,
                Page::Updates,
                Page::DateTime,
                Page::About,
            ],
        ),
    ];

    fn title(self) -> &'static str {
        match self {
            Page::Network => "Wi-Fi",
            Page::Bluetooth => "Bluetooth",
            Page::Wired => "Réseau",
            Page::Desktop => "Apparence",
            Page::Dock => "Bureau et dock",
            Page::Windows => "Fenêtres",
            Page::Notifications => "Notifications",
            Page::Sound => "Son",
            Page::Energy => "Énergie",
            Page::Displays => "Écrans",
            Page::Keyboard => "Clavier",
            Page::Mouse => "Souris et pavé tactile",
            Page::Printers => "Imprimantes",
            Page::Accounts => "Comptes",
            Page::Privacy => "Confidentialité",
            Page::Accessibility => "Accessibilité",
            Page::Updates => "Mises à jour",
            Page::DateTime => "Date et heure",
            Page::About => "À propos",
        }
    }

    /// Couleur de la pastille de la rubrique.
    fn color(self) -> u32 {
        match self {
            Page::Network | Page::Bluetooth | Page::Displays | Page::Accessibility => 0x2779E0,
            Page::Wired | Page::Energy => 0x1F9D63,
            Page::Desktop => 0x7C4DE8,
            Page::Dock | Page::Windows => 0x4B5563,
            Page::Notifications => 0xE5372B,
            Page::Sound => 0xDB2C57,
            Page::Keyboard | Page::Mouse | Page::Printers | Page::Updates | Page::About => 0x6B7280,
            Page::Accounts | Page::DateTime => 0xE5571A,
            Page::Privacy => 0x374151,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Page::Network => "glyph-wifi",
            Page::Bluetooth => "glyph-bluetooth",
            Page::Wired => "glyph-network",
            Page::Desktop => "glyph-appearance",
            Page::Dock => "glyph-dock",
            Page::Windows => "glyph-windows",
            Page::Notifications => "glyph-bell",
            Page::Sound => "glyph-sound",
            Page::Energy => "glyph-energy",
            Page::Displays => "glyph-screen",
            Page::Keyboard => "glyph-keyboard",
            Page::Mouse => "glyph-mouse",
            Page::Printers => "glyph-printer",
            Page::Accounts => "glyph-account",
            Page::Privacy => "glyph-lock",
            Page::Accessibility => "glyph-access",
            Page::Updates => "glyph-update",
            Page::DateTime => "glyph-clock",
            Page::About => "glyph-about",
        }
    }
}

/// Résultat d'une tâche de fond.
enum Update {
    Snapshot(Snapshot),
    Networks(Result<Vec<WifiNetwork>, String>),
    NetworkDevices(Result<Vec<network::Device>, String>),
    BluetoothDevices(Result<Vec<bluetooth::Device>, String>),
    Sinks(Result<Vec<Sink>, String>),
    Known(Vec<KnownWifi>),
    Options(Option<WifiOptions>),
    Details(Result<Vec<(String, String)>, String>, Option<network::WifiFlags>),
    Hotspot(Result<String, String>),
    ScanDone,
    BluetoothName(Option<String>),
    Audio {
        sources: Vec<Sink>,
        input: Option<u8>,
        balance: Option<i8>,
        streams: Vec<Stream>,
    },
    Time(Option<String>, Option<bool>),
    Timezones(Result<Vec<String>, String>),
    About(Box<About>),
    /// Fin d'une action : message de réussite (vide : rien à dire) ou erreur.
    Done(Result<String, String>),
    /// Une tâche de fond est terminée.
    Finished,
    /// Images trouvées pour le fond d'écran, puis la vignette de chacune.
    WallpaperFiles(Vec<PathBuf>),
    Thumbnail(PathBuf, Option<Image>),
}

/// Action demandée depuis une liste (réseaux, appareils), faite après son dessin.
enum ListAction {
    Connect(String),
    Disconnect(String),
    Forget(String),
    Details(String),
}

pub struct SettingsState {
    page: Page,
    icons: Icons,
    sender: Sender<Update>,
    receiver: Receiver<Update>,
    /// Tâches en cours (pour afficher l'attente et éviter les doublons).
    running: usize,
    message: Option<Result<String, String>>,

    snapshot: Option<Snapshot>,
    volume: u8,
    brightness: u8,
    networks: Option<Vec<WifiNetwork>>,
    network_devices: Vec<network::Device>,
    password_for: Option<String>,
    password: String,
    password_visible: bool,
    /// Réseau masqué : le nom se tape dans la même fenêtre que le mot de passe.
    hidden_network: bool,
    known: Vec<KnownWifi>,
    wifi_options: Option<WifiOptions>,
    /// Adresse et serveurs du réseau actuel, affichés par « Détails », avec ses options.
    details: Option<Vec<(String, String)>>,
    wifi_flags: Option<network::WifiFlags>,
    /// Appareil Bluetooth dont les détails sont ouverts (son adresse).
    bluetooth_details: Option<String>,
    hotspot: bool,
    /// Dernier balayage Wi-Fi et Bluetooth, pour les relancer tant que la page est ouverte.
    wifi_scanned: Option<Instant>,
    bluetooth_scanned: Option<Instant>,
    bluetooth_scanning: bool,
    bluetooth_name: Option<String>,
    sources: Vec<Sink>,
    input_volume: u8,
    /// Balance : 0 tout à gauche, 50 au centre, 100 tout à droite.
    balance: u8,
    streams: Vec<Stream>,
    /// « Ajouter… » : images et chemin pour le fond d'écran.
    wallpaper_more: bool,
    bluetooth_devices: Option<Vec<bluetooth::Device>>,
    sinks: Vec<Sink>,
    keyboard: String,
    keyboard_sample: String,
    /// Dernier mouvement d'un curseur (volume, luminosité).
    touched: Option<Instant>,
    /// Recherche dans la colonne.
    search: String,
    /// Nom complet et identifiant de l'utilisateur, pour « À propos ».
    user: (String, String),
    timezone: Option<String>,
    ntp: Option<bool>,
    timezones: Vec<String>,
    timezone_filter: String,
    about: Option<About>,
    loaded: Vec<Page>,
    /// Images proposées comme fond d'écran, leurs vignettes (`None` : illisible), les
    /// images dont la vignette est demandée, et le chemin tapé pour une autre image.
    wallpaper_files: Option<Vec<PathBuf>>,
    thumbnails: HashMap<Source, Option<egui::TextureHandle>>,
    requested: HashSet<PathBuf>,
    wallpaper_path: String,
}

impl std::fmt::Debug for SettingsState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsState").field("page", &self.page).finish()
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            page: Page::Desktop,
            icons: Icons::default(),
            sender,
            receiver,
            running: 0,
            message: None,
            snapshot: None,
            volume: 0,
            brightness: 0,
            networks: None,
            network_devices: Vec::new(),
            password_for: None,
            password: String::new(),
            password_visible: false,
            hidden_network: false,
            known: Vec::new(),
            wifi_options: None,
            details: None,
            wifi_flags: None,
            bluetooth_details: None,
            hotspot: false,
            wifi_scanned: None,
            bluetooth_scanned: None,
            bluetooth_scanning: false,
            bluetooth_name: None,
            sources: Vec::new(),
            input_volume: 0,
            balance: 50,
            streams: Vec::new(),
            wallpaper_more: false,
            bluetooth_devices: None,
            sinks: Vec::new(),
            keyboard: Settings::load().keyboard_layout().to_owned(),
            keyboard_sample: String::new(),
            touched: None,
            search: String::new(),
            user: current_user(),
            timezone: None,
            ntp: None,
            timezones: Vec::new(),
            timezone_filter: String::new(),
            about: None,
            loaded: Vec::new(),
            wallpaper_files: None,
            thumbnails: HashMap::new(),
            requested: HashSet::new(),
            wallpaper_path: String::new(),
        }
    }
}

impl SettingsState {
    /// Ouvre la rubrique demandée au lancement (« wifi », « son »…).
    pub fn open_page(&mut self, name: &str) {
        if let Some(page) = Page::from_key(name) {
            self.go(page);
        }
    }

    /// Lance `job` dans un fil à part ; son résultat revient par le canal.
    fn spawn(&mut self, ctx: &egui::Context, job: impl FnOnce() -> Vec<Update> + Send + 'static) {
        self.running += 1;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            for update in job() {
                let _ = sender.send(update);
            }
            let _ = sender.send(Update::Finished);
            ctx.request_repaint();
        });
    }

    /// Lance une action puis relit l'état rapide du système.
    fn act(
        &mut self,
        ctx: &egui::Context,
        success: &'static str,
        action: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
        self.message = None;
        self.spawn(ctx, move || {
            let result = action().map(|()| success.to_owned());
            vec![Update::Done(result), Update::Snapshot(Snapshot::read())]
        });
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(update) = self.receiver.try_recv() {
            match update {
                Update::Snapshot(snapshot) => {
                    // Une relecture plus ancienne que le dernier réglage ne fait pas sauter
                    // le curseur en arrière.
                    let settled = self.touched.is_none_or(|at| at.elapsed() >= HOLD);
                    if settled {
                        if let Some(volume) = snapshot.volume {
                            self.volume = if volume.muted { 0 } else { volume.level };
                        }
                        self.brightness = snapshot.brightness.unwrap_or(self.brightness);
                    }
                    self.snapshot = Some(snapshot);
                }
                // Sans carte Wi-Fi ou sans NetworkManager, les listes restent vides : la page
                // l'indique déjà, une erreur en plus serait du bruit.
                Update::Networks(result) => self.networks = Some(result.unwrap_or_default()),
                Update::NetworkDevices(result) => self.network_devices = result.unwrap_or_default(),
                Update::BluetoothDevices(result) => match result {
                    Ok(devices) => self.bluetooth_devices = Some(devices),
                    Err(error) => self.fail(error),
                },
                Update::Sinks(result) => match result {
                    Ok(sinks) => self.sinks = sinks,
                    Err(error) => self.fail(error),
                },
                Update::Known(known) => self.known = known,
                Update::Options(options) => self.wifi_options = options,
                Update::Details(result, flags) => match result {
                    Ok(details) => {
                        self.details = Some(details);
                        self.wifi_flags = flags;
                    }
                    Err(error) => self.fail(error),
                },
                Update::Hotspot(result) => {
                    self.hotspot = result.is_ok();
                    self.message = Some(result);
                }
                Update::ScanDone => self.bluetooth_scanning = false,
                Update::BluetoothName(name) => self.bluetooth_name = name,
                Update::Audio {
                    sources,
                    input,
                    balance,
                    streams,
                } => {
                    self.sources = sources;
                    self.streams = streams;
                    if self.touched.is_none_or(|at| at.elapsed() >= HOLD) {
                        self.input_volume = input.unwrap_or(self.input_volume);
                        self.balance =
                            balance.map_or(self.balance, |value| (i16::from(value) / 2 + 50) as u8);
                    }
                }
                Update::Time(zone, ntp) => {
                    self.timezone = zone;
                    self.ntp = ntp;
                }
                Update::Timezones(result) => match result {
                    Ok(zones) => self.timezones = zones,
                    Err(error) => self.fail(error),
                },
                Update::About(about) => self.about = Some(*about),
                Update::Finished => self.running = self.running.saturating_sub(1),
                Update::Done(Ok(message)) if message.is_empty() => {}
                Update::Done(result) => self.message = Some(result),
                Update::WallpaperFiles(files) => self.wallpaper_files = Some(files),
                Update::Thumbnail(path, image) => {
                    let source = Source::File(path);
                    let texture = image.map(|image| thumbnail_texture(ctx, &source.key(), &image));
                    self.thumbnails.insert(source, texture);
                }
            }
        }
    }

    fn fail(&mut self, error: String) {
        self.message = Some(Err(error));
    }

    /// Charge les données d'une page à sa première ouverture.
    fn load(&mut self, ctx: &egui::Context, page: Page) {
        if self.loaded.contains(&page) {
            return;
        }
        self.loaded.push(page);
        match page {
            Page::Network => self.refresh_networks(ctx),
            Page::Wired => self.spawn(ctx, || vec![Update::NetworkDevices(network::devices())]),
            Page::Bluetooth => {
                self.refresh_bluetooth(ctx);
                self.spawn(ctx, || vec![Update::BluetoothName(bluetooth::adapter_name())]);
            }
            Page::Sound => self.refresh_audio(ctx),
            Page::DateTime => self.spawn(ctx, || {
                vec![
                    Update::Time(time::timezone(), time::ntp_enabled()),
                    Update::Timezones(time::timezones()),
                ]
            }),
            Page::About => self.spawn(ctx, || vec![Update::About(Box::new(about::read()))]),
            Page::Desktop => self.spawn(ctx, || vec![Update::WallpaperFiles(find_images())]),
            Page::Dock
            | Page::Windows
            | Page::Notifications
            | Page::Energy
            | Page::Displays
            | Page::Keyboard
            | Page::Mouse
            | Page::Printers
            | Page::Accounts
            | Page::Privacy
            | Page::Accessibility
            | Page::Updates => {}
        }
    }

    fn refresh_networks(&mut self, ctx: &egui::Context) {
        self.wifi_scanned = Some(Instant::now());
        self.spawn(ctx, || {
            let networks = network::wifi_networks();
            let current = networks
                .iter()
                .flatten()
                .find(|network| network.connected)
                .map(|network| network.ssid.clone());
            vec![
                Update::NetworkDevices(network::devices()),
                Update::Known(network::known_wifi().unwrap_or_default()),
                Update::Options(current.and_then(|name| network::wifi_options(&name).ok())),
                Update::Networks(networks),
            ]
        });
    }

    fn refresh_audio(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, || {
            vec![
                Update::Sinks(audio::sinks()),
                Update::Audio {
                    sources: audio::sources().unwrap_or_default(),
                    input: audio::input_volume(),
                    balance: audio::balance(),
                    streams: audio::streams().unwrap_or_default(),
                },
            ]
        });
    }

    fn refresh_bluetooth(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, || vec![Update::BluetoothDevices(bluetooth::devices())]);
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.snapshot.is_none() && self.loaded.is_empty() {
            self.spawn(&ctx, || vec![Update::Snapshot(Snapshot::read())]);
        }
        self.receive(&ctx);
        self.load(&ctx, self.page);
        self.accept_dropped_image(&ctx);

        let running = self.running > 0;
        let message = self.message.clone();
        // Barre d'état seulement quand il y a quelque chose à dire.
        let visible = running
            || matches!(&message, Some(Ok(text)) if !text.is_empty())
            || matches!(message, Some(Err(_)));
        if visible {
            self.status(ui, running, message);
        }

        style::sidebar(ui, "rubriques", 290.0, style::sidebar_color(), true, |ui| {
            self.sidebar(ui)
        });
        let title = self.page.title();
        style::toolbar(ui, false, |ui| {
            ui.add_space(14.0);
            ui.label(RichText::new(title).size(15.0).family(style::strong_family()));
        });
        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(style::background()).inner_margin(Margin {
                left: 32,
                right: 32,
                top: 24,
                bottom: 12,
            }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| match self.page {
                        Page::About => self.about_page(ui),
                        Page::Network => self.network_page(ui, &ctx),
                        Page::Bluetooth => self.bluetooth_page(ui, &ctx),
                        Page::Wired => self.wired_page(ui),
                        Page::Sound => self.sound_page(ui, &ctx),
                        Page::Mouse => self.mouse_page(ui),
                        Page::Dock => self.dock_page(ui),
                        Page::Windows => self.windows_page(ui),
                        Page::Desktop => self.desktop_page(ui, &ctx),
                        Page::Displays => self.displays_page(ui, &ctx),
                        Page::Keyboard => self.keyboard_page(ui),
                        Page::DateTime => self.datetime_page(ui, &ctx),
                        Page::Accounts => self.accounts_page(ui),
                        Page::Notifications
                        | Page::Energy
                        | Page::Printers
                        | Page::Privacy
                        | Page::Accessibility
                        | Page::Updates => self.pending_page(ui),
                    });
            });
        self.wifi_details_dialog(&ctx);
        self.bluetooth_details_dialog(&ctx);
        self.wifi_password_dialog(&ctx);
    }

    /// Barre d'état : attente d'une opération, réussite ou erreur.
    fn status(&self, ui: &mut egui::Ui, running: bool, message: Option<Result<String, String>>) {
        style::status_bar(ui, |ui| {
            if running {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label("Opération en cours…");
            }
            match &message {
                Some(Ok(text)) if !text.is_empty() => {
                    ui.label(text);
                }
                Some(Err(error)) => {
                    ui.colored_label(style::error(), error);
                }
                _ => {}
            }
        });
    }

    fn go(&mut self, page: Page) {
        if page != self.page {
            self.page = page;
            self.message = None;
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        // Champ de recherche sur la ligne du bouton fermer.
        let body = ui.max_rect();
        let field = egui::Rect::from_min_size(
            egui::pos2(body.left() + 34.0, body.top() - style::TOOLBAR_HEIGHT + 10.0),
            egui::vec2(body.width() - 34.0, 32.0),
        );
        ui.put(
            field,
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Rechercher un réglage")
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(10, 6)),
        );
        let query = self.search.trim().to_lowercase();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for (name, group) in Page::GROUPS {
                let pages: Vec<Page> = group
                    .iter()
                    .copied()
                    .filter(|page| query.is_empty() || page.title().to_lowercase().contains(&query))
                    .collect();
                if pages.is_empty() {
                    continue;
                }
                ui.add_space(8.0);
                style::sidebar_heading(ui, name);
                for page in pages {
                    let response = style::sidebar_tile_item(
                        ui,
                        &mut self.icons,
                        page.icon(),
                        rgb(page.color()),
                        page.title(),
                        self.page == page,
                    );
                    if response.clicked() {
                        self.go(page);
                    }
                }
            }
        });
    }

    // ---- À propos ---------------------------------------------------------------------

    fn about_page(&mut self, ui: &mut egui::Ui) {
        let Some(about) = self.about.clone() else {
            ui.label(RichText::new("Lecture des informations…").color(style::text_secondary()));
            return;
        };
        let gib = |bytes: u64| format!("{:.1} Go", bytes as f64 / 1_073_741_824.0).replace('.', ",");
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(170.0, 136.0), egui::Sense::hover());
            paint_computer(ui.painter(), rect);
            ui.add_space(26.0);
            ui.vertical(|ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&about.hostname).size(30.0));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Ferrite").size(18.0).color(style::text_secondary()));
                });
                ui.add_space(4.0);
                spec(
                    ui,
                    "Processeur",
                    &format!("{} ({} cœurs)", short_processor(&about.processor), about.cores),
                );
                spec(ui, "Mémoire", &gib(about.memory_kib * 1024));
                let (name, login) = &self.user;
                spec(ui, "Utilisateur", &format!("{name} ({login})"));
            });
        });

        heading(ui, "Stockage");
        let used = about.disk_total.saturating_sub(about.disk_free);
        ui.label(
            RichText::new(format!(
                "{} utilisés ({} disponibles)",
                gib(used),
                gib(about.disk_free)
            ))
            .size(14.0),
        );
        ui.add_space(8.0);
        storage_bar(ui, used as f64 / about.disk_total.max(1) as f64);

        heading(ui, "Logiciel");
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(116.0, 116.0), egui::Sense::hover());
            self.icons
                .paint(ui, "ferrite-logo", rect.shrink(8.0), style::IconStyle::Color);
            ui.add_space(26.0);
            ui.vertical(|ui| {
                ui.add_space(14.0);
                ui.add(egui::Label::new(RichText::new(&about.os).size(26.0)).wrap());
                ui.label(
                    RichText::new(format!("Noyau Linux {}", about.kernel))
                        .size(12.0)
                        .color(style::text_secondary()),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "Bureau Ferrite · allumé depuis {}",
                        format_uptime(about.uptime_seconds)
                    ))
                    .size(14.0),
                );
            });
        });
    }

    // ---- Wi-Fi ------------------------------------------------------------------------

    fn network_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let wifi = self.snapshot.and_then(|snapshot| snapshot.wifi);
        let bluetooth = self.snapshot.and_then(|snapshot| snapshot.bluetooth);
        let networks = self.networks.clone().unwrap_or_default();
        let current = networks.iter().find(|network| network.connected).cloned();
        // Tant que la page est ouverte, la liste suit les réseaux à portée.
        if wifi == Some(true) && self.wifi_scanned.is_none_or(|at| at.elapsed() >= WIFI_RESCAN) {
            self.refresh_networks(ctx);
        }
        ctx.request_repaint_after(WIFI_RESCAN);
        let subtitle = match (wifi, &current) {
            (Some(true), Some(network)) => format!("Connecté à {}", network.ssid),
            (Some(true), None) => "Aucun réseau connecté".to_owned(),
            (Some(false), _) => "Désactivé".to_owned(),
            (None, _) => "Aucune carte Wi-Fi détectée".to_owned(),
        };
        let mut on = wifi == Some(true);
        if hero(
            ui,
            &mut self.icons,
            (Page::Network.icon(), Page::Network.color()),
            "Wi-Fi",
            &subtitle,
            Some((&mut on, wifi.is_some())),
        ) {
            self.act(ctx, "", move || network::set_wifi(on));
            self.wifi_scanned = None;
        }
        ui.add_space(20.0);

        enum Choice {
            Details,
            Join(WifiNetwork),
            Other,
            Autoconnect(bool),
            Airplane(bool),
            Hotspot,
            Forget(String),
            Mac(String),
            Dns(String),
        }
        let mut choice = None;
        let known = self.known.clone();
        let is_known = |ssid: &str| known.iter().any(|entry| entry.name == ssid);
        let mut autoconnect = known.is_empty() || known.iter().all(|entry| entry.autoconnect);
        let mut airplane =
            wifi != Some(true) && bluetooth != Some(true) && (wifi.is_some() || bluetooth.is_some());
        let options = self.wifi_options.clone();
        let hotspot = self.hotspot;
        ui.spacing_mut().item_spacing.x = 24.0;
        ui.columns(2, |columns| {
            let [left, right] = columns else { return };
            group(left, &mut self.icons, "Réseau actuel", |rows| match &current {
                Some(network) => {
                    let hint = format!("Connecté · {}", network.summary());
                    let row = Row::new(&network.ssid)
                        .lead(Lead::Bars(network.level()))
                        .lock(network.secured)
                        .bold(true)
                        .hint(&hint)
                        .ok(true);
                    if rows.row(row, |ui| rows::button(ui, "Détails", false).clicked()).0 {
                        choice = Some(Choice::Details);
                    }
                }
                None => {
                    rows.text(Row::new("Aucun réseau connecté"));
                }
            });
            group(left, &mut self.icons, "Réseaux disponibles", |rows| {
                for network in networks.iter().filter(|network| !network.connected) {
                    let hint = if is_known(&network.ssid) {
                        "Connu"
                    } else if network.secured {
                        ""
                    } else {
                        "Ouvert · non sécurisé"
                    };
                    let row = Row::new(&network.ssid)
                        .lead(Lead::Bars(network.level()))
                        .lock(network.secured)
                        .hint(hint)
                        .clickable();
                    if rows.row(row, |_| ()).1.clicked() {
                        choice = Some(Choice::Join(network.clone()));
                    }
                }
                let other = Row::new("Autre réseau…").hint("Réseau masqué ou configuration manuelle");
                if rows
                    .row(other, |ui| rows::button(ui, "Ajouter…", false).clicked())
                    .0
                {
                    choice = Some(Choice::Other);
                }
            });
            group(right, &mut self.icons, "Options", |rows| {
                let row = Row::new("Connexion automatique").hint("Aux réseaux déjà connus");
                if rows.row(row, |ui| rows::switch(ui, &mut autoconnect)).0 {
                    choice = Some(Choice::Autoconnect(autoconnect));
                }
                let row = Row::new("Mode avion").hint("Coupe Wi-Fi et Bluetooth");
                if rows.row(row, |ui| rows::switch(ui, &mut airplane)).0 {
                    choice = Some(Choice::Airplane(airplane));
                }
                let row = Row::new("Point d’accès mobile").hint("Partager la connexion de cet ordinateur");
                let label = if hotspot { "Arrêter" } else { "Activer…" };
                if rows.row(row, |ui| rows::button(ui, label, false).clicked()).0 {
                    choice = Some(Choice::Hotspot);
                }
            });
            if !known.is_empty() {
                group(right, &mut self.icons, "Réseaux connus", |rows| {
                    for (index, entry) in known.iter().enumerate() {
                        let hint = format!("Priorité {}", index + 1);
                        let row = Row::new(&entry.name).hint(&hint);
                        if rows.row(row, |ui| rows::button(ui, "Oublier", false).clicked()).0 {
                            choice = Some(Choice::Forget(entry.name.clone()));
                        }
                    }
                });
            }
            group(right, &mut self.icons, "Avancé", |rows| {
                let mut row = Row::new("Adresse matérielle (MAC)");
                if options.is_none() {
                    row = row.unavailable();
                }
                let mut mac = options
                    .as_ref()
                    .map_or("stable", |known| match known.mac.as_str() {
                        "permanent" => "permanent",
                        _ => "stable",
                    });
                let changed = rows
                    .row(row, |ui| {
                        rows::pick(
                            ui,
                            "wifi-mac",
                            &mut mac,
                            &[
                                ("stable", "Aléatoire par réseau"),
                                ("permanent", "Adresse réelle"),
                            ],
                        )
                    })
                    .0;
                if changed {
                    choice = Some(Choice::Mac(mac.to_owned()));
                }
                let mut row = Row::new("Serveurs DNS");
                if options.is_none() {
                    row = row.unavailable();
                }
                let mut dns = options.as_ref().map_or("", |known| known.dns.as_str());
                let dns_choices = [
                    ("", "Automatique"),
                    ("1.1.1.1", "Cloudflare"),
                    ("8.8.8.8", "Google"),
                    ("9.9.9.9", "Quad9"),
                ];
                if !dns_choices.iter().any(|(value, _)| *value == dns) {
                    dns = "";
                }
                let changed = rows
                    .row(row, |ui| rows::pick(ui, "wifi-dns", &mut dns, &dns_choices))
                    .0;
                if changed {
                    choice = Some(Choice::Dns(dns.to_owned()));
                }
            });
        });
        let name = current.as_ref().map(|network| network.ssid.clone());
        match choice {
            Some(Choice::Details) => {
                let device = self
                    .network_devices
                    .iter()
                    .find(|device| device.kind == "wifi" && device.state.starts_with("connected"))
                    .map(|device| device.name.clone());
                if let Some(device) = device {
                    let name = name.clone().unwrap_or_default();
                    self.spawn(ctx, move || {
                        vec![Update::Details(
                            network::device_details(&device),
                            network::wifi_flags(&name).ok(),
                        )]
                    });
                }
            }
            Some(Choice::Join(chosen)) => {
                if chosen.secured && !is_known(&chosen.ssid) {
                    self.hidden_network = false;
                    self.password_for = Some(chosen.ssid);
                    self.password.clear();
                    self.password_visible = false;
                } else {
                    self.wifi_scanned = None;
                    self.act(ctx, "", move || network::connect_wifi(&chosen.ssid, None));
                }
            }
            Some(Choice::Other) => {
                self.hidden_network = true;
                self.password_for = Some(String::new());
                self.password.clear();
                self.password_visible = false;
            }
            Some(Choice::Autoconnect(enabled)) => {
                self.wifi_scanned = None;
                self.act(ctx, "", move || network::set_autoconnect(enabled));
            }
            Some(Choice::Airplane(enabled)) => {
                self.wifi_scanned = None;
                self.act(ctx, "", move || {
                    let wifi = network::set_wifi(!enabled);
                    let radio = bluetooth::set_powered(!enabled);
                    wifi.and(radio)
                });
            }
            Some(Choice::Hotspot) => {
                if self.hotspot {
                    self.hotspot = false;
                    self.act(ctx, "Point d’accès arrêté", network::stop_hotspot);
                } else {
                    self.spawn(ctx, || vec![Update::Hotspot(network::start_hotspot())]);
                }
            }
            Some(Choice::Forget(known)) => {
                self.wifi_scanned = None;
                self.act(ctx, "", move || network::forget_wifi(&known));
            }
            Some(Choice::Mac(mac)) => {
                if let Some(name) = name {
                    self.wifi_scanned = None;
                    self.act(ctx, "", move || network::set_wifi_mac(&name, &mac));
                }
            }
            Some(Choice::Dns(dns)) => {
                if let Some(name) = name {
                    self.wifi_scanned = None;
                    self.act(ctx, "", move || network::set_wifi_dns(&name, &dns));
                }
            }
            None => {}
        }
    }

    /// « Détails » du réseau actuel (planche « Dialogues de détails ») : sécurité, bande
    /// et signal, adresses à copier, options de la connexion ; l'oublier ou s'en
    /// déconnecter.
    fn wifi_details_dialog(&mut self, ctx: &egui::Context) {
        let Some(addresses) = self.details.clone() else {
            return;
        };
        let Some(network) = self
            .networks
            .iter()
            .flatten()
            .find(|network| network.connected)
            .cloned()
        else {
            self.details = None;
            return;
        };
        let mut flags = self.wifi_flags;
        let (mut closed, mut footer, mut changed) = (false, details::Footer::default(), None);
        let escape = details::show(ctx, "details-wifi", |ui, t| {
            closed = details::header(
                ui,
                t,
                52.0,
                43.0,
                |ui, rect| {
                    details::lead_tile(ui, rect, style::accent(), |painter, rect| {
                        wifi_glyph(painter, rect, style::accent_text());
                    })
                },
                |ui| details::title_and_state(ui, t, &network.ssid, "Connecté", t.ok),
            );
            details::tiles(
                ui,
                t,
                &[
                    ("Sécurité", security_label(&network.security)),
                    ("Bande", band_label(&network.frequency)),
                    ("Signal", signal_label(network.signal)),
                ],
            );
            details::section(ui, t, 18.0, "Adresses", None, |rows| {
                let mut dns = 0;
                for (label, value) in &addresses {
                    let label = match label.as_str() {
                        "DNS" => {
                            dns += 1;
                            match dns {
                                1 => "DNS principal".to_owned(),
                                2 => "DNS secondaire".to_owned(),
                                n => format!("DNS {n}"),
                            }
                        }
                        other => other.to_owned(),
                    };
                    rows.copy(&label, value);
                }
            });
            if let Some(flags) = flags.as_mut() {
                details::section(ui, t, 18.0, "Options", None, |rows| {
                    if rows.switch(
                        "Connexion automatique",
                        "Se reconnecter dès que le réseau est à portée",
                        &mut flags.autoconnect,
                    ) {
                        changed = Some((network::WifiFlag::Autoconnect, flags.autoconnect));
                    }
                    if rows.switch(
                        "Connexion limitée",
                        "Réduit les téléchargements en arrière-plan",
                        &mut flags.metered,
                    ) {
                        changed = Some((network::WifiFlag::Metered, flags.metered));
                    }
                });
            }
            footer = details::footer(
                ui,
                t,
                details::Left::Danger("Oublier ce réseau"),
                &["Déconnecter", "OK"],
            );
        });
        self.wifi_flags = flags;
        let ssid = network.ssid.clone();
        if let Some((flag, enabled)) = changed {
            self.act(ctx, "", move || network::set_wifi_flag(&ssid, flag, enabled));
        }
        let ssid = network.ssid;
        if footer.left {
            self.wifi_scanned = None;
            self.act(ctx, "Réseau oublié", move || network::forget_wifi(&ssid));
        } else if footer.button == Some(0) {
            self.wifi_scanned = None;
            self.act(ctx, "", move || network::disconnect_wifi(&ssid));
        }
        if closed || escape || footer.left || footer.button.is_some() {
            self.details = None;
        }
    }

    fn wifi_password_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut ssid) = self.password_for.clone() else {
            return;
        };
        let hidden = self.hidden_network;
        let mut open = true;
        let mut connect = false;
        let mut cancel = false;
        egui::Window::new("Joindre au réseau Wi-Fi")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut open)
            .frame(
                Frame::new()
                    .fill(style::background())
                    .corner_radius(16)
                    .inner_margin(Margin {
                        left: 28,
                        right: 28,
                        top: 26,
                        bottom: 22,
                    })
                    .stroke(Stroke::new(1.0, style::separator()))
                    .shadow(egui::Shadow {
                        offset: [0, 14],
                        blur: 40,
                        spread: 0,
                        color: Color32::from_black_alpha(42),
                    }),
            )
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Joindre au réseau Wi-Fi")
                            .size(22.0)
                            .family(style::strong_family()),
                    );
                    ui.add_space(7.0);
                    if !hidden {
                        ui.label(RichText::new(&ssid).size(15.0).color(style::text_secondary()));
                    }
                });
                if hidden {
                    ui.add_space(20.0);
                    ui.label(
                        RichText::new("Nom du réseau")
                            .size(14.0)
                            .family(style::strong_family()),
                    );
                    ui.add_space(6.0);
                    ui.add_sized(
                        [410.0, 44.0],
                        egui::TextEdit::singleline(&mut ssid).margin(egui::vec2(14.0, 12.0)),
                    );
                }
                ui.add_space(20.0);
                ui.label(
                    RichText::new("Mot de passe")
                        .size(14.0)
                        .family(style::strong_family()),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let mut password_edit = egui::TextEdit::singleline(&mut self.password)
                        .hint_text("Saisir le mot de passe")
                        .desired_width(356.0)
                        .margin(egui::vec2(14.0, 12.0));
                    if !self.password_visible {
                        password_edit = password_edit.password(true);
                    }
                    let field = ui.add_sized([356.0, 44.0], password_edit);
                    let eye = if self.password_visible { "◉" } else { "◎" };
                    let eye_button = ui
                        .add_sized(
                            [44.0, 44.0],
                            egui::Button::new(RichText::new(eye).size(18.0).monospace()).corner_radius(10),
                        )
                        .on_hover_text(if self.password_visible {
                            "Masquer le mot de passe"
                        } else {
                            "Afficher le mot de passe"
                        });
                    if eye_button.clicked() {
                        self.password_visible = !self.password_visible;
                    }
                    if field.has_focus() && ui.ctx().input(|input| input.key_pressed(egui::Key::Enter)) {
                        connect = true;
                    }
                });
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_sized(
                                [112.0, 40.0],
                                egui::Button::new(
                                    RichText::new("Connecter").size(14.0).color(style::accent_text()),
                                )
                                .fill(style::accent())
                                .stroke(Stroke::new(
                                    1.0,
                                    style::accent().lerp_to_gamma(Color32::BLACK, 0.12),
                                ))
                                .corner_radius(10),
                            )
                            .clicked()
                        {
                            connect = true;
                        }
                        if ui
                            .add_sized(
                                [112.0, 40.0],
                                egui::Button::new(RichText::new("Annuler").size(14.0)).corner_radius(10),
                            )
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
            });
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if !open || cancel {
            self.password_for = None;
            self.password.clear();
            self.password_visible = false;
        } else if connect && !ssid.trim().is_empty() {
            let password = std::mem::take(&mut self.password);
            self.password_for = None;
            self.password_visible = false;
            self.wifi_scanned = None;
            self.act(ctx, "Connecté", move || {
                if hidden {
                    network::connect_hidden_wifi(&ssid, Some(&password))
                } else {
                    network::connect_wifi(&ssid, Some(&password))
                }
            });
        } else {
            self.password_for = Some(ssid);
        }
    }

    // ---- Bluetooth --------------------------------------------------------------------

    fn bluetooth_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let powered = self.snapshot.and_then(|snapshot| snapshot.bluetooth);
        // Tant que la page est ouverte, la recherche des appareils se relance.
        if powered == Some(true)
            && !self.bluetooth_scanning
            && self
                .bluetooth_scanned
                .is_none_or(|at| at.elapsed() >= BLUETOOTH_RESCAN)
        {
            self.bluetooth_scanning = true;
            self.bluetooth_scanned = Some(Instant::now());
            self.spawn(ctx, || {
                let scan = bluetooth::scan();
                let mut updates = vec![Update::ScanDone, Update::BluetoothDevices(bluetooth::devices())];
                if let Err(error) = scan {
                    updates.push(Update::Done(Err(error)));
                }
                updates
            });
        }
        ctx.request_repaint_after(BLUETOOTH_RESCAN);
        let subtitle = match (powered, &self.bluetooth_name) {
            (Some(true), Some(name)) => format!("Visible sous le nom « {name} »"),
            (Some(true), None) => "Visible des appareils proches".to_owned(),
            (Some(false), _) => "Désactivé".to_owned(),
            (None, _) => "Aucun adaptateur Bluetooth détecté".to_owned(),
        };
        let mut on = powered == Some(true);
        if hero(
            ui,
            &mut self.icons,
            (Page::Bluetooth.icon(), Page::Bluetooth.color()),
            "Bluetooth",
            &subtitle,
            Some((&mut on, powered.is_some())),
        ) {
            self.act(ctx, "", move || bluetooth::set_powered(on));
        }
        ui.add_space(20.0);

        let devices = self.bluetooth_devices.clone().unwrap_or_default();
        let (paired, others): (Vec<_>, Vec<_>) = devices.into_iter().partition(|device| device.paired);
        let mut action = None;
        let scanning = self.bluetooth_scanning;
        let mut settings = Settings::load();
        let mut bar_icon = settings.flag("barre_bluetooth", true);
        let mut save_icon = false;
        ui.spacing_mut().item_spacing.x = 24.0;
        ui.columns(2, |columns| {
            let [left, right] = columns else { return };
            group(left, &mut self.icons, "Mes appareils", |rows| {
                if paired.is_empty() {
                    rows.text(Row::new("Aucun appareil associé"));
                }
                for device in &paired {
                    let hint = device_hint(device);
                    let row = Row::new(&device.name)
                        .lead(device_lead(device))
                        .hint(&hint)
                        .ok(device.connected)
                        .clickable();
                    let label = if device.connected {
                        "Déconnecter"
                    } else {
                        "Connecter"
                    };
                    let (clicked, response) =
                        rows.row(row, |ui| rows::button(ui, label, !device.connected).clicked());
                    if clicked {
                        action = Some(if device.connected {
                            ListAction::Disconnect(device.address.clone())
                        } else {
                            ListAction::Connect(device.address.clone())
                        });
                    } else if response.clicked() {
                        action = Some(ListAction::Details(device.address.clone()));
                    }
                    response.context_menu(|ui| {
                        if ui.button("Oublier cet appareil").clicked() {
                            action = Some(ListAction::Forget(device.address.clone()));
                            ui.close();
                        }
                    });
                }
            });
            group(right, &mut self.icons, "Appareils à proximité", |rows| {
                let text = if scanning {
                    "Recherche en cours…"
                } else {
                    "Recherche terminée"
                };
                rows.row(Row::new("Recherche"), |ui| rows::status(ui, text, scanning));
                for device in &others {
                    let row = Row::new(&device.name)
                        .lead(device_lead(device))
                        .hint("Prêt à être associé");
                    if rows.row(row, |ui| rows::button(ui, "Associer", true).clicked()).0 {
                        action = Some(ListAction::Connect(device.address.clone()));
                    }
                }
            });
            group(right, &mut self.icons, "Options", |rows| {
                let row = Row::new("Icône dans la barre supérieure");
                save_icon = rows.row(row, |ui| rows::switch(ui, &mut bar_icon)).0;
                rows.row(Row::new("Recevoir des fichiers").unavailable(), |ui| {
                    let mut off = false;
                    rows::switch(ui, &mut off);
                });
                rows.row(Row::new("Dossier de réception").unavailable(), |ui| {
                    let mut folder = 0;
                    rows::pick(ui, "bt-dossier", &mut folder, &[(0, "Téléchargements")]);
                });
            });
        });
        if save_icon {
            settings.set_flag("barre_bluetooth", bar_icon);
            self.save(&settings);
        }
        match action {
            Some(ListAction::Connect(address)) => {
                self.bluetooth_action(ctx, "Connecté", move || bluetooth::connect(&address));
            }
            Some(ListAction::Disconnect(address)) => {
                self.bluetooth_action(ctx, "Déconnecté", move || bluetooth::disconnect(&address));
            }
            Some(ListAction::Forget(address)) => {
                self.bluetooth_action(ctx, "Appareil oublié", move || bluetooth::forget(&address));
            }
            Some(ListAction::Details(address)) => self.bluetooth_details = Some(address),
            None => {}
        }
    }

    /// Détails d'un appareil associé (même dialogue que le Wi-Fi) : type, batterie,
    /// adresse, reconnexion automatique ; l'oublier, le connecter ou le déconnecter.
    fn bluetooth_details_dialog(&mut self, ctx: &egui::Context) {
        let Some(address) = self.bluetooth_details.clone() else {
            return;
        };
        let Some(mut device) = self
            .bluetooth_devices
            .iter()
            .flatten()
            .find(|device| device.address == address)
            .cloned()
        else {
            self.bluetooth_details = None;
            return;
        };
        let (glyph, color) = match device_lead(&device) {
            Lead::Tile(glyph, color) => (glyph, color),
            _ => ("glyph-bluetooth", 0x2779E0),
        };
        let icons = &mut self.icons;
        let (mut closed, mut footer, mut trust) = (false, details::Footer::default(), None);
        let connect_label = if device.connected {
            "Déconnecter"
        } else {
            "Connecter"
        };
        let escape = details::show(ctx, "details-bluetooth", |ui, t| {
            let (state, state_color) = if device.connected {
                ("Connecté", t.ok)
            } else {
                ("Non connecté", t.fg3)
            };
            closed = details::header(
                ui,
                t,
                52.0,
                43.0,
                |ui, rect| {
                    let fill = Color32::from_rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
                    ui.painter().rect_filled(rect, 14.0, fill);
                    icons.paint(
                        ui,
                        glyph,
                        rect.shrink(12.0),
                        style::IconStyle::Tinted(Color32::WHITE),
                    );
                },
                |ui| details::title_and_state(ui, t, &device.name, state, state_color),
            );
            details::tiles(
                ui,
                t,
                &[
                    ("Type", device_kind(glyph).to_owned()),
                    (
                        "Batterie",
                        device
                            .battery
                            .map_or_else(|| "—".to_owned(), |battery| format!("{battery} %")),
                    ),
                    ("Associé", if device.paired { "Oui" } else { "Non" }.to_owned()),
                ],
            );
            details::section(ui, t, 18.0, "Appareil", None, |rows| {
                rows.copy("Adresse", &device.address);
            });
            details::section(ui, t, 18.0, "Options", None, |rows| {
                if rows.switch(
                    "Connexion automatique",
                    "Se reconnecter dès qu’il est allumé et à portée",
                    &mut device.trusted,
                ) {
                    trust = Some(device.trusted);
                }
            });
            footer = details::footer(
                ui,
                t,
                details::Left::Danger("Oublier cet appareil"),
                &[connect_label, "OK"],
            );
        });
        if let Some(trusted) = trust {
            if let Some(known) = self
                .bluetooth_devices
                .iter_mut()
                .flatten()
                .find(|known| known.address == address)
            {
                known.trusted = trusted;
            }
            let address = address.clone();
            self.bluetooth_action(ctx, "", move || bluetooth::set_trusted(&address, trusted));
        }
        if footer.left {
            self.bluetooth_action(ctx, "Appareil oublié", move || bluetooth::forget(&address));
        } else if footer.button == Some(0) {
            if device.connected {
                self.bluetooth_action(ctx, "Déconnecté", move || bluetooth::disconnect(&address));
            } else {
                self.bluetooth_action(ctx, "Connecté", move || bluetooth::connect(&address));
            }
        }
        if closed || escape || footer.left || footer.button.is_some() {
            self.bluetooth_details = None;
        }
    }

    /// Enregistre les réglages ; seule une erreur s'affiche.
    fn save(&mut self, settings: &Settings) {
        if let Err(error) = settings.save() {
            self.fail(format!("Enregistrement impossible : {error}"));
        }
    }

    fn bluetooth_action(
        &mut self,
        ctx: &egui::Context,
        success: &'static str,
        action: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
        self.message = None;
        self.spawn(ctx, move || {
            let result = action().map(|()| success.to_owned());
            vec![
                Update::Done(result),
                Update::BluetoothDevices(bluetooth::devices()),
            ]
        });
    }

    // ---- Son --------------------------------------------------------------------------

    fn sound_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let current = self.snapshot.and_then(|snapshot| snapshot.volume);
        let sink = self
            .sinks
            .iter()
            .find(|sink| sink.default)
            .map(|sink| sink.name.clone());
        let subtitle = match (&sink, current) {
            (Some(name), Some(_)) => format!("{name} · volume {} %", self.volume),
            (None, Some(_)) => format!("Volume {} %", self.volume),
            _ => "Aucune sortie audio : PipeWire n’est pas démarré".to_owned(),
        };
        hero(
            ui,
            &mut self.icons,
            ("glyph-sound-full", Page::Sound.color()),
            "Son",
            &subtitle,
            None,
        );
        ui.add_space(20.0);

        enum Choice {
            Sink(u32),
            Source(u32),
            Volume(u8),
            Balance(u8, i8),
            Input(u8),
            Stream(u32, u8),
            Test,
        }
        let mut choice = None;
        let sinks = self.sinks.clone();
        let sources = self.sources.clone();
        let mut volume = self.volume;
        let mut balance = self.balance;
        let mut input = self.input_volume;
        let mut streams = self.streams.clone();
        let mut touched = false;
        // Un curseur agit à la fin du glissement, ou au simple clic.
        let done = |response: &egui::Response| {
            response.drag_stopped() || (response.changed() && !response.dragged())
        };
        ui.spacing_mut().item_spacing.x = 24.0;
        ui.columns(2, |columns| {
            let [left, right] = columns else { return };
            group(left, &mut self.icons, "Sortie", |rows| {
                for sink in &sinks {
                    let row = Row::new(&sink.name)
                        .lead(sink_lead(&sink.name))
                        .bold(sink.default);
                    if rows.row(row, |ui| rows::radio(ui, sink.default).clicked()).0 && !sink.default {
                        choice = Some(Choice::Sink(sink.id));
                    }
                }
                let text = format!("{volume} %");
                let (response, _) = rows.row(Row::new("Volume"), |ui| rows::slider(ui, &mut volume, &text));
                touched |= response.changed();
                if done(&response) {
                    choice = Some(Choice::Volume(volume));
                }
                let text = balance_label(balance);
                let (response, _) = rows.row(Row::new("Balance"), |ui| rows::slider(ui, &mut balance, &text));
                touched |= response.changed();
                if done(&response) {
                    choice = Some(Choice::Balance(volume, (i16::from(balance) * 2 - 100) as i8));
                }
                let test = rows.row(Row::new("Tester les haut-parleurs"), |ui| {
                    rows::button(ui, "Tester", false).clicked()
                });
                if test.0 {
                    choice = Some(Choice::Test);
                }
            });
            group(left, &mut self.icons, "Entrée", |rows| {
                for source in &sources {
                    let row = Row::new(&source.name)
                        .lead(Lead::Tile("glyph-mic", 0x4B5563))
                        .bold(source.default);
                    if rows.row(row, |ui| rows::radio(ui, source.default).clicked()).0 && !source.default {
                        choice = Some(Choice::Source(source.id));
                    }
                }
                rows.row(Row::new("Niveau d’entrée").unavailable(), |ui| {
                    rows::meter(ui, 0)
                });
                let text = format!("{input} %");
                let (response, _) = rows.row(Row::new("Volume d’entrée"), |ui| {
                    rows::slider(ui, &mut input, &text)
                });
                touched |= response.changed();
                if done(&response) {
                    choice = Some(Choice::Input(input));
                }
            });
            group(right, &mut self.icons, "Volume par application", |rows| {
                if streams.is_empty() {
                    rows.text(Row::new("Aucune application ne joue de son"));
                }
                for stream in &mut streams {
                    let name = if stream.name.is_empty() {
                        "Application"
                    } else {
                        &stream.name
                    };
                    let row = Row::new(name).lead(Lead::Tile("glyph-app", 0x4B5563));
                    let text = format!("{} %", stream.volume);
                    let (response, _) = rows.row(row, |ui| rows::slider(ui, &mut stream.volume, &text));
                    touched |= response.changed();
                    if done(&response) {
                        choice = Some(Choice::Stream(stream.id, stream.volume));
                    }
                }
            });
            group(right, &mut self.icons, "Sons système", |rows| {
                rows.row(Row::new("Volume des alertes").unavailable(), |ui| {
                    let mut value = 40;
                    rows::slider(ui, &mut value, "40 %");
                });
                rows.row(Row::new("Son d’alerte").unavailable(), |ui| {
                    let mut sound = 0;
                    rows::pick(ui, "son-alerte", &mut sound, &[(0, "Goutte")]);
                });
                let row = Row::new("Sons des actions")
                    .hint("Corbeille, capture d’écran")
                    .unavailable();
                rows.row(row, |ui| {
                    let mut off = false;
                    rows::switch(ui, &mut off);
                });
                rows.row(Row::new("Son au démarrage").unavailable(), |ui| {
                    let mut off = false;
                    rows::switch(ui, &mut off);
                });
            });
        });
        self.volume = volume;
        self.balance = balance;
        self.input_volume = input;
        self.streams = streams;
        if touched {
            self.touched = Some(Instant::now());
        }
        match choice {
            Some(Choice::Sink(id)) => self.spawn(ctx, move || {
                let result = audio::set_default_sink(id).map(|()| String::new());
                vec![Update::Done(result), Update::Sinks(audio::sinks())]
            }),
            Some(Choice::Source(id)) => self.spawn(ctx, move || {
                let result = audio::set_default_sink(id).map(|()| String::new());
                vec![
                    Update::Done(result),
                    Update::Audio {
                        sources: audio::sources().unwrap_or_default(),
                        input: audio::input_volume(),
                        balance: audio::balance(),
                        streams: audio::streams().unwrap_or_default(),
                    },
                ]
            }),
            Some(Choice::Volume(level)) => self.act(ctx, "", move || audio::set_volume(level)),
            Some(Choice::Balance(level, value)) => {
                self.act(ctx, "", move || audio::set_balance(level, value))
            }
            Some(Choice::Input(level)) => self.act(ctx, "", move || audio::set_input_volume(level)),
            Some(Choice::Stream(id, level)) => self.act(ctx, "", move || audio::set_stream_volume(id, level)),
            Some(Choice::Test) => self.act(ctx, "", audio::test_speakers),
            None => {}
        }
    }

    // ---- Bureau et dock ---------------------------------------------------------------

    fn dock_page(&mut self, ui: &mut egui::Ui) {
        let mut settings = Settings::load();
        let before = settings.clone();
        dock_preview(ui);
        ui.add_space(20.0);
        ui.spacing_mut().item_spacing.x = 24.0;
        let icons = &mut self.icons;
        ui.columns(2, |columns| {
            let [left, right] = columns else { return };
            group(left, icons, "Dock", |rows| {
                let px = settings.dock_icon();
                let mut percent = ((px - 40) * 100 / 40) as u8;
                let text = format!("{px} px");
                let (response, _) = rows.row(Row::new("Taille des icônes"), |ui| {
                    rows::slider(ui, &mut percent, &text)
                });
                if response.changed() {
                    let px = 40 + u32::from(percent) * 40 / 100;
                    settings.set("dock_icones", &px.to_string());
                }
                flag_row(
                    rows,
                    &mut settings,
                    Row::new("Agrandissement au survol"),
                    "dock_agrandissement",
                    true,
                );
                rows.row(Row::new("Position").unavailable(), |ui| {
                    rows::segments(ui, &["Bas", "Gauche", "Droite"], 0);
                });
                let mut hiding = settings.dock_hiding();
                let labels = hiding_labels();
                let row = Row::new("Masquage").hint("Se cache si une fenêtre le recouvre");
                if rows
                    .row(row, |ui| rows::pick(ui, "dock-masquage", &mut hiding, &labels))
                    .0
                {
                    settings.set("dock_masquage", hiding.key());
                }
                flag_row(
                    rows,
                    &mut settings,
                    Row::new("Apps récentes dans le dock"),
                    "dock_recentes",
                    true,
                );
                flag_row(
                    rows,
                    &mut settings,
                    Row::new("Point sous les apps ouvertes"),
                    "dock_points",
                    true,
                );
            });
            group(left, icons, "Barre supérieure", |rows| {
                flag_row(
                    rows,
                    &mut settings,
                    Row::new("Translucide"),
                    "barre_translucide",
                    true,
                );
                let mut clock = settings.clock_style();
                let labels: Vec<(ClockStyle, &str)> = ClockStyle::ALL
                    .into_iter()
                    .map(|choice| (choice, choice.label()))
                    .collect();
                if rows
                    .row(Row::new("Horloge"), |ui| {
                        rows::pick(ui, "horloge", &mut clock, &labels)
                    })
                    .0
                {
                    settings.set("horloge", clock.key());
                }
                flag_row(
                    rows,
                    &mut settings,
                    Row::new("Pourcentage de batterie"),
                    "barre_pourcentage",
                    false,
                );
                let mut bar = settings.bar_hiding();
                let labels = hiding_labels();
                if rows
                    .row(Row::new("Masquage"), |ui| {
                        rows::pick(ui, "barre-masquage", &mut bar, &labels)
                    })
                    .0
                {
                    settings.set("barre_masquage", bar.key());
                }
            });
            group(right, icons, "Coins actifs", |rows| {
                let labels: Vec<(CornerAction, &str)> = CornerAction::ALL
                    .into_iter()
                    .map(|choice| (choice, choice.label()))
                    .collect();
                for (key, name) in [
                    ("hg", "Haut gauche"),
                    ("hd", "Haut droit"),
                    ("bg", "Bas gauche"),
                    ("bd", "Bas droit"),
                ] {
                    let mut action = settings.hot_corner(key);
                    if rows
                        .row(Row::new(name), |ui| rows::pick(ui, key, &mut action, &labels))
                        .0
                    {
                        settings.set(&format!("coin_{key}"), action.key());
                    }
                }
            });
            group(right, icons, "Espaces de travail", |rows| {
                rows.row(Row::new("Mode").unavailable(), |ui| {
                    rows::segments(ui, &["Dynamiques", "Nombre fixe"], 0);
                });
                rows.row(Row::new("Écran principal uniquement").unavailable(), |ui| {
                    let mut on = true;
                    rows::switch(ui, &mut on);
                });
            });
            group(right, icons, "Fenêtres", |rows| {
                window_rows(rows, &mut settings);
            });
        });
        if settings != before {
            self.save(&settings);
        }
    }

    // ---- Fenêtres ---------------------------------------------------------------------

    fn windows_page(&mut self, ui: &mut egui::Ui) {
        let mut settings = Settings::load();
        let before = settings.clone();
        let icons = &mut self.icons;
        group(ui, icons, "Comportement", |rows| window_rows(rows, &mut settings));
        let custom_dock = settings.dock_pinned().is_some();
        group(ui, icons, "Applications à plusieurs fenêtres", |rows| {
            let mut multi = settings.multi_window_apps();
            let mut changed = false;
            for (name, id) in MULTI_WINDOW_APPS {
                let mut allowed = multi.iter().any(|known| known == id);
                if rows.row(Row::new(name), |ui| rows::switch(ui, &mut allowed)).0 {
                    changed = true;
                    if allowed {
                        multi.push(id.to_owned());
                    } else {
                        multi.retain(|known| known != id);
                    }
                }
            }
            if changed {
                settings.set_multi_window_apps(&multi);
            }
        });
        group(ui, icons, "Dock", |rows| {
            let row = Row::new("Applications du Dock")
                .hint("Clic droit sur une icône pour la garder ou la retirer");
            let reset = rows
                .row(row, |ui| {
                    ui.add_enabled_ui(custom_dock, |ui| rows::button(ui, "Rétablir", false).clicked())
                        .inner
                })
                .0;
            if reset {
                settings.set_dock_pinned(&DEFAULT_DOCK.map(str::to_owned));
            }
        });
        if settings != before {
            self.save(&settings);
        }
    }

    // ---- Réseau, écrans, comptes, rubriques à venir -----------------------------------

    fn wired_page(&mut self, ui: &mut egui::Ui) {
        let devices = self.network_devices.clone();
        hero(
            ui,
            &mut self.icons,
            (Page::Wired.icon(), Page::Wired.color()),
            "Réseau",
            "Interfaces de cet ordinateur",
            None,
        );
        ui.add_space(20.0);
        group(ui, &mut self.icons, "Interfaces", |rows| {
            if devices.is_empty() {
                rows.text(Row::new("Aucune interface détectée"));
            }
            for device in &devices {
                let kind = match device.kind.as_str() {
                    "wifi" => "Wi-Fi",
                    "ethernet" => "Filaire",
                    other => other,
                };
                let connected = device.state.starts_with("connected");
                let hint = if connected {
                    format!("{kind} · {}", device.connection)
                } else {
                    format!("{kind} · non connecté")
                };
                rows.text(Row::new(&device.name).hint(&hint).ok(connected));
            }
        });
    }

    fn displays_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let available = self.snapshot.and_then(|snapshot| snapshot.brightness).is_some();
        let scale = ui.ctx().pixels_per_point();
        hero(
            ui,
            &mut self.icons,
            (Page::Displays.icon(), Page::Displays.color()),
            "Écrans",
            "Écran principal",
            None,
        );
        ui.add_space(20.0);
        let mut brightness = self.brightness;
        let mut response = None;
        group(ui, &mut self.icons, "Écran principal", |rows| {
            let mut row = Row::new("Luminosité");
            if !available {
                row = row.hint("Non réglable sur cet écran").unavailable();
            }
            let text = format!("{brightness} %");
            response = Some(rows.row(row, |ui| rows::slider(ui, &mut brightness, &text)).0);
            let scale = format!("{:.0} %", scale * 100.0);
            rows.text(Row::new("Échelle").hint(&scale));
        });
        self.brightness = brightness;
        if let Some(response) = response {
            if response.changed() {
                self.touched = Some(Instant::now());
            }
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                self.act(ctx, "", move || display::set_brightness(brightness.max(5)));
            }
        }
        ui.add_space(20.0);
        self.night_group(ui);
    }

    /// Mode nuit : programmation, horaires et chaleur de la teinte. Le bureau suit les
    /// réglages dans la seconde.
    fn night_group(&mut self, ui: &mut egui::Ui) {
        use ferrite_system::night::{
            NightSettings, Schedule, TEMPERATURE_MAX, TEMPERATURE_MIN, format_time, local_now,
        };
        let settings = Settings::load();
        let mut night = NightSettings::load(&settings);
        let before = night;
        let place = ferrite_system::weather::Place::from_settings(&settings);
        let options: Vec<(Schedule, &str)> = Schedule::ALL
            .iter()
            .map(|schedule| (*schedule, schedule.label()))
            .collect();
        group(ui, &mut self.icons, "Mode nuit", |rows| {
            rows.row(
                Row::new("Teinte orangée le soir").hint("Repose les yeux ; aussi dans le centre de contrôle"),
                |ui| rows::pick(ui, "nuit-programme", &mut night.schedule, &options),
            );
            match night.schedule {
                Schedule::Sun => {
                    let (day, _, offset) = local_now();
                    let text = match night.window(&place, day, offset) {
                        Some((start, end)) => format!(
                            "De {} à {}, à {}",
                            format_time(start),
                            format_time(end),
                            place.name
                        ),
                        None => "Horaire indisponible".to_owned(),
                    };
                    rows.text(Row::new("Ce soir").hint(&text));
                }
                Schedule::Hours => {
                    rows.row(Row::new("Début"), |ui| time_editor(ui, &mut night.start));
                    rows.row(Row::new("Fin"), |ui| time_editor(ui, &mut night.end));
                }
                Schedule::Off | Schedule::Always => {}
            }
            if night.schedule != Schedule::Off {
                // Curseur de 0 (peu teinté) à 100 (très chaud).
                let span = TEMPERATURE_MAX - TEMPERATURE_MIN;
                let mut warmth = ((TEMPERATURE_MAX - night.temperature) * 100 / span) as u8;
                let text = format!("{} K", night.temperature);
                let response = rows
                    .row(Row::new("Chaleur"), |ui| rows::slider(ui, &mut warmth, &text))
                    .0;
                if response.changed() {
                    night.temperature = TEMPERATURE_MAX - u32::from(warmth) * span / 100;
                }
            }
        });
        if night != before {
            let mut settings = Settings::load();
            night.store(&mut settings);
            if let Err(error) = settings.save() {
                eprintln!("mode nuit non enregistré : {error}");
            }
        }
    }

    fn accounts_page(&mut self, ui: &mut egui::Ui) {
        let (full, login) = self.user.clone();
        hero(
            ui,
            &mut self.icons,
            (Page::Accounts.icon(), Page::Accounts.color()),
            "Comptes",
            "Compte de cette session",
            None,
        );
        ui.add_space(20.0);
        group(ui, &mut self.icons, "Compte actuel", |rows| {
            rows.text(
                Row::new(&full)
                    .hint(&login)
                    .lead(Lead::Tile("glyph-account", 0xE5571A)),
            );
        });
    }

    /// Rubrique de la maquette sans réglage pour l'instant : elle le dit.
    fn pending_page(&mut self, ui: &mut egui::Ui) {
        let page = self.page;
        hero(
            ui,
            &mut self.icons,
            (page.icon(), page.color()),
            page.title(),
            "Cette rubrique n’est pas encore disponible.",
            None,
        );
    }

    // ---- Réglages du bureau -----------------------------------------------------------

    /// Fond d'écran, apparence et écran sur une seule page, comme sur la maquette.
    fn desktop_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut settings = Settings::load();
        let current = Source::parse(settings.wallpaper());
        let mut wallpaper_choice = current.clone();
        self.prepare_thumbnails(ctx, &current);
        let before = settings.clone();
        ui.set_max_width(ui.available_width().min(820.0));

        let appearance = settings.appearance();
        section_title(ui, "Style");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for mode in Appearance::ALL {
                if appearance_choice(ui, mode, appearance == mode).clicked() {
                    settings.set("apparence", mode.key());
                }
            }
        });
        ui.add_space(22.0);

        section_title(ui, "Couleur d’accent");
        let mut accent = settings.accent();
        accent_picker(ui, &mut accent);
        if accent != settings.accent() {
            settings.set("accent", accent.key());
        }
        ui.add_space(22.0);

        section_title(ui, "Fond d’écran");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);
            for builtin in Builtin::ALL {
                let source = Source::Builtin(builtin);
                if builtin_wallpaper_tile(ui, builtin, current == source).clicked() {
                    wallpaper_choice = source;
                }
            }
            if let Source::File(_) = &current {
                let thumbnail = self.thumbnails.get(&current);
                let (rect, _) = ui.allocate_exact_size(WALLPAPER_TILE, egui::Sense::hover());
                paint_thumbnail(ui, rect, thumbnail);
                ui.painter().rect_stroke(
                    rect,
                    10.0,
                    Stroke::new(3.0, style::accent()),
                    egui::StrokeKind::Outside,
                );
            }
            if add_wallpaper_tile(ui).clicked() {
                self.wallpaper_more = !self.wallpaper_more;
            }
        });
        if self.wallpaper_more {
            ui.add_space(12.0);
            match &self.wallpaper_files {
                None => note(ui, "Recherche des images…"),
                Some(files) if files.is_empty() => {
                    note(
                        ui,
                        "Aucune image PNG ou JPEG dans Images, Pictures ou /usr/share/backgrounds.",
                    );
                }
                Some(files) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(12.0, 10.0);
                        for path in files {
                            let source = Source::File(path.clone());
                            let thumbnail = self.thumbnails.get(&source);
                            let response =
                                wallpaper_tile(ui, thumbnail, &source_label(&source), current == source)
                                    .on_hover_text(path.display().to_string());
                            if response.clicked() {
                                wallpaper_choice = source;
                            }
                        }
                    });
                }
            }
            ui.add_space(6.0);
            field(ui, "Autre image", |ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.wallpaper_path)
                        .hint_text("/home/moi/Images/photo.jpg")
                        .desired_width(300.0),
                );
                let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if ui.button("Utiliser").clicked() || entered {
                    let path = PathBuf::from(self.wallpaper_path.trim());
                    match wallpaper::decode(&path) {
                        Ok(_) if path.is_absolute() => wallpaper_choice = Source::File(path),
                        Ok(_) => {
                            self.fail("Donne le chemin complet de l’image (il commence par /)".to_owned())
                        }
                        Err(error) => self.fail(format!("Image illisible : {error}")),
                    }
                }
            });
        }
        ui.add_space(22.0);

        let icons = &mut self.icons;
        group(ui, icons, "Thèmes et affichage", |rows| {
            rows.row(Row::new("Thème d’icônes"), |ui| {
                let mut theme = 0;
                rows::pick(ui, "theme-icones", &mut theme, &[(0, "Ferrite")]);
            });
            rows.row(Row::new("Thème de curseurs").unavailable(), |ui| {
                let mut theme = 0;
                rows::pick(ui, "theme-curseurs", &mut theme, &[(0, "Ferrite-Cursors")]);
            });
            rows.row(Row::new("Taille du curseur").unavailable(), |ui| {
                let mut size = 24;
                rows::pick(ui, "taille-curseur", &mut size, &[(24, "24 px")]);
            });
            rows.row(Row::new("Taille du texte").hint("100 %").unavailable(), |ui| {
                let mut size = 40;
                rows::slider(ui, &mut size, "100 %");
            });
            flag_row(
                rows,
                &mut settings,
                Row::new("Barre supérieure translucide").hint("Laisse transparaître le fond d’écran"),
                "barre_translucide",
                true,
            );
            flag_row(
                rows,
                &mut settings,
                Row::new("Réduire les animations").hint("Sans effet de transition sur les fenêtres"),
                "reduire_animations",
                false,
            );
        });
        group(ui, icons, "Plus de réglages", |rows| {
            let mut highlight = settings.highlight();
            let mut choices: Vec<(Option<Accent>, &str)> = vec![(None, "Couleur d’accent")];
            choices.extend(
                Accent::ALL
                    .into_iter()
                    .map(|choice| (Some(choice), choice.label())),
            );
            if rows
                .row(Row::new("Couleur de surlignage"), |ui| {
                    rows::pick(ui, "surlignage", &mut highlight, &choices)
                })
                .0
            {
                settings.set("surlignage", highlight.map_or("accent", Accent::key));
            }
            let mut sizes = settings.sidebar_icons();
            let choices: Vec<(DockSize, &str)> = DockSize::ALL
                .into_iter()
                .map(|choice| (choice, sidebar_icons_label(choice)))
                .collect();
            if rows
                .row(Row::new("Icônes de la barre latérale"), |ui| {
                    rows::pick(ui, "icones-colonne", &mut sizes, &choices)
                })
                .0
            {
                settings.set("barre_laterale_icones", sizes.key());
            }
            let mut scroll = settings.scroll_bars();
            let choices: Vec<(ScrollBars, &str)> = ScrollBars::ALL
                .into_iter()
                .map(|choice| (choice, choice.label()))
                .collect();
            if rows
                .row(Row::new("Barres de défilement"), |ui| {
                    rows::pick(ui, "defilement", &mut scroll, &choices)
                })
                .0
            {
                settings.set("barres_defilement", scroll.key());
            }
        });

        if settings != before {
            style::apply_preferences(ui.ctx(), &settings);
            self.save(&settings);
        }
        if wallpaper_choice != current {
            self.set_wallpaper(&mut settings, &wallpaper_choice);
        }
    }

    fn set_wallpaper(&mut self, settings: &mut Settings, source: &Source) {
        settings.set("fond_ecran", &source.key());
        self.message = Some(
            settings
                .save()
                .map(|()| "Fond d’écran changé".to_owned())
                .map_err(|error| format!("Enregistrement impossible : {error}")),
        );
    }

    /// Une image glissée dans la fenêtre devient le fond d'écran.
    fn accept_dropped_image(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if let Some(path) = dropped.into_iter().find(|path| is_image(path)) {
            let mut settings = Settings::load();
            self.set_wallpaper(&mut settings, &Source::File(path));
            self.go(Page::Desktop);
        }
    }

    /// Vignettes des fonds Ferrite (dessinées ici), et lecture en arrière-plan de celles
    /// des images trouvées et du fond actuel.
    fn prepare_thumbnails(&mut self, ctx: &egui::Context, current: &Source) {
        for builtin in Builtin::ALL {
            self.thumbnails
                .entry(Source::Builtin(builtin))
                .or_insert_with(|| {
                    let image = wallpaper::paint(builtin, THUMBNAIL.0, THUMBNAIL.1);
                    Some(thumbnail_texture(ctx, builtin.key(), &image))
                });
        }
        let mut missing: Vec<PathBuf> = self.wallpaper_files.clone().unwrap_or_default();
        if let Source::File(path) = current {
            missing.push(path.clone());
        }
        missing.retain(|path| !self.requested.contains(path));
        missing.dedup();
        if !missing.is_empty() {
            self.load_thumbnails(ctx, missing);
        }
    }

    /// Lit les vignettes de `paths` l'une après l'autre dans un fil à part (une photo peut
    /// peser des dizaines de mégaoctets une fois décodée).
    fn load_thumbnails(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        // Marquées tout de suite, pour ne pas les redemander à l'image suivante.
        self.requested.extend(paths.iter().cloned());
        let sender = self.sender.clone();
        let repaint = ctx.clone();
        self.spawn(ctx, move || {
            for path in paths {
                let image = wallpaper::decode(&path)
                    .ok()
                    .map(|image| wallpaper::cover(&image, THUMBNAIL.0, THUMBNAIL.1));
                let _ = sender.send(Update::Thumbnail(path, image));
                repaint.request_repaint();
            }
            Vec::new()
        });
    }

    // ---- Clavier ----------------------------------------------------------------------

    fn keyboard_page(&mut self, ui: &mut egui::Ui) {
        field(ui, "Disposition", |ui| {
            let current = KEYBOARD_LAYOUTS
                .iter()
                .find(|(code, _)| *code == self.keyboard)
                .map_or(self.keyboard.as_str(), |(_, name)| name)
                .to_owned();
            let mut chosen = self.keyboard.clone();
            egui::ComboBox::from_id_salt("clavier")
                .selected_text(current)
                .width(260.0)
                .show_ui(ui, |ui| {
                    for (code, name) in KEYBOARD_LAYOUTS {
                        ui.selectable_value(&mut chosen, (*code).to_owned(), *name);
                    }
                });
            if chosen != self.keyboard {
                let mut settings = Settings::load();
                settings.set("clavier", &chosen);
                self.message = Some(
                    settings
                        .save()
                        .map(|()| "Disposition enregistrée".to_owned())
                        .map_err(|error| format!("Enregistrement impossible : {error}")),
                );
                self.keyboard = chosen;
            }
        });
        note(ui, "S'applique aussitôt à toutes les applications.");
        heading(ui, "Essai");
        card(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.keyboard_sample)
                    .hint_text("Tape ici pour vérifier la disposition")
                    .desired_width(f32::INFINITY),
            );
        });
    }

    fn mouse_page(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Défilement");
        let mut settings = Settings::load();
        let mut inverted = settings.scroll_inverted();
        if toggle(
            ui,
            &mut inverted,
            true,
            "Inverser le sens de défilement",
            "Inverse le sens de la molette. Le pavé tactile conserve son défilement naturel.",
        ) {
            settings.set("defilement_inverse", if inverted { "oui" } else { "non" });
            self.message = Some(
                settings
                    .save()
                    .map(|()| "Réglage enregistré".to_owned())
                    .map_err(|error| format!("Enregistrement impossible : {error}")),
            );
        }
    }

    // ---- Date et heure ----------------------------------------------------------------

    fn datetime_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut on = self.ntp == Some(true);
        if toggle(
            ui,
            &mut on,
            self.ntp.is_some(),
            "Régler la date et l’heure automatiquement",
            "Par le réseau (NTP)",
        ) {
            self.ntp = Some(on);
            self.spawn(ctx, move || {
                let result = time::set_ntp(on).map(|()| "Réglage enregistré".to_owned());
                vec![
                    Update::Done(result),
                    Update::Time(time::timezone(), time::ntp_enabled()),
                ]
            });
        }
        ui.add_space(6.0);
        field(ui, "Fuseau horaire", |ui| {
            let zone = self.timezone.as_deref().unwrap_or("Inconnu").replace('_', " ");
            ui.label(RichText::new(zone).size(14.0).family(style::strong_family()));
        });

        heading(ui, "Changer de fuseau");
        ui.add(
            egui::TextEdit::singleline(&mut self.timezone_filter)
                .hint_text("Rechercher une ville, par exemple Paris")
                .desired_width(320.0),
        );
        let filter = self.timezone_filter.to_lowercase();
        if filter.len() < 2 {
            return;
        }
        let matches: Vec<String> = self
            .timezones
            .iter()
            .filter(|zone| zone.to_lowercase().contains(&filter))
            .take(12)
            .cloned()
            .collect();
        if matches.is_empty() {
            return;
        }
        ui.add_space(8.0);
        let mut chosen = None;
        card(ui, |ui| {
            for (index, zone) in matches.iter().enumerate() {
                if index > 0 {
                    separator(ui);
                }
                list_row(ui, |ui| {
                    ui.label(RichText::new(zone.replace('_', " ")).size(14.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Choisir").clicked() {
                            chosen = Some(zone.clone());
                        }
                    });
                });
            }
        });
        if let Some(zone) = chosen {
            self.spawn(ctx, move || {
                let result = time::set_timezone(&zone).map(|()| format!("Fuseau : {zone}"));
                vec![
                    Update::Done(result),
                    Update::Time(time::timezone(), time::ntp_enabled()),
                ]
            });
        }
    }
}

fn rgb(hex: u32) -> Color32 {
    let [_, r, g, b] = hex.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

fn format_uptime(seconds: u64) -> String {
    let (hours, minutes) = (seconds / 3600, (seconds / 60) % 60);
    match hours {
        0 => format!("{minutes} min"),
        _ => format!("{hours} h {minutes:02}"),
    }
}

/// Applications Ferrite et leur identifiant de fenêtre, pour le réglage « Plusieurs
/// fenêtres ». Le Terminal n'y est pas : il peut toujours en ouvrir plusieurs.
/// Relecture de la liste des réseaux et recherche des appareils, tant que la page est ouverte.
const WIFI_RESCAN: Duration = Duration::from_secs(30);
const BLUETOOTH_RESCAN: Duration = Duration::from_secs(20);
const WALLPAPER_TILE: egui::Vec2 = egui::vec2(136.0, 84.0);

/// Titre d'une partie de la page Apparence.
fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(14.0)
            .family(style::strong_family())
            .color(style::themed(rgb(0x374151), rgb(0xD1D5DB))),
    );
    ui.add_space(12.0);
}

/// Choix de masquage du Dock et de la barre.
fn hiding_labels() -> [(DockHiding, &'static str); 3] {
    [
        (DockHiding::Smart, "Intelligent"),
        (DockHiding::Always, "Toujours"),
        (DockHiding::Never, "Jamais"),
    ]
}

/// Ligne oui/non enregistrée sous `key`.
fn flag_row(rows: &mut rows::Group<'_>, settings: &mut Settings, row: Row<'_>, key: &str, default: bool) {
    let mut on = match key {
        "dock_agrandissement" => settings.dock_magnify(),
        _ => settings.flag(key, default),
    };
    if rows.row(row, |ui| rows::switch(ui, &mut on)).0 {
        settings.set_flag(key, on);
    }
}

/// Réglages des fenêtres, communs à « Bureau et dock » et à « Fenêtres ».
fn window_rows(rows: &mut rows::Group<'_>, settings: &mut Settings) {
    flag_row(
        rows,
        settings,
        Row::new("Aimantation aux bords"),
        "aimantation",
        true,
    );
    flag_row(rows, settings, Row::new("Tuilage automatique"), "tuilage", false);
    let mut action = settings.double_click();
    let labels: Vec<(DoubleClick, &str)> = DoubleClick::ALL
        .into_iter()
        .map(|choice| (choice, choice.label()))
        .collect();
    if rows
        .row(Row::new("Double-clic sur la barre de titre"), |ui| {
            rows::pick(ui, "double-clic", &mut action, &labels)
        })
        .0
    {
        settings.set("double_clic", action.key());
    }
}

/// Pastille et symbole d'un appareil Bluetooth, selon le genre qu'il annonce.
fn device_lead(device: &bluetooth::Device) -> Lead {
    let name = device.name.to_lowercase();
    let kind = |known: &str, word: &str| device.icon.contains(known) || name.contains(word);
    if kind("audio-head", "casque") || kind("audio-card", "buds") {
        Lead::Tile("glyph-head", 0xDB2C57)
    } else if kind("mouse", "souris") {
        Lead::Tile("glyph-mouse", 0x4B5563)
    } else if kind("keyboard", "clavier") {
        Lead::Tile("glyph-keyboard", 0x4B5563)
    } else if kind("phone", "téléphone") {
        Lead::Tile("glyph-phone", 0x1F9D63)
    } else if kind("watch", "montre") {
        Lead::Tile("glyph-watch", 0x7C4DE8)
    } else if kind("computer", "laptop") {
        Lead::Tile("glyph-laptop", 0x4B5563)
    } else if kind("audio", "enceinte") {
        Lead::Tile("glyph-speaker", 0xE5571A)
    } else {
        Lead::Tile("glyph-bluetooth", 0x2779E0)
    }
}

/// Genre d'appareil, d'après son symbole (voir `device_lead`).
fn device_kind(glyph: &str) -> &'static str {
    match glyph {
        "glyph-head" => "Casque",
        "glyph-mouse" => "Souris",
        "glyph-keyboard" => "Clavier",
        "glyph-phone" => "Téléphone",
        "glyph-watch" => "Montre",
        "glyph-laptop" => "Ordinateur",
        "glyph-speaker" => "Enceinte",
        _ => "Appareil",
    }
}

/// « Connecté · batterie 80 % », « Non connecté ».
fn device_hint(device: &bluetooth::Device) -> String {
    match (device.connected, device.battery) {
        (true, Some(battery)) => format!("Connecté · batterie {battery} %"),
        (true, None) => "Connecté".to_owned(),
        (false, _) => "Non connecté".to_owned(),
    }
}

/// Pastille d'une sortie audio, d'après son nom.
fn sink_lead(name: &str) -> Lead {
    let lower = name.to_lowercase();
    if lower.contains("hdmi") || lower.contains("displayport") {
        Lead::Tile("glyph-screen", 0x2779E0)
    } else if lower.contains("head") || lower.contains("casque") || lower.contains("bluez") {
        Lead::Tile("glyph-head", 0xDB2C57)
    } else {
        Lead::Tile("glyph-laptop", 0x4B5563)
    }
}

/// « Centre », « Gauche 40 », « Droite 20 » pour une balance de 0 à 100.
fn balance_label(balance: u8) -> String {
    match i16::from(balance) * 2 - 100 {
        0 => "Centre".to_owned(),
        value if value < 0 => format!("G {}", -value),
        value => format!("D {value}"),
    }
}

/// Aperçu de « Bureau et dock » : un petit bureau et le texte de la planche.
fn dock_preview(ui: &mut egui::Ui) {
    Frame::new()
        .fill(style::header_color())
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(14)
        .inner_margin(Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(240.0, 140.0), egui::Sense::hover());
                let painter = ui.painter();
                painter.rect_filled(rect, 10.0, rgb(0x16202D));
                let bar = egui::Rect::from_min_size(rect.min, egui::vec2(240.0, 8.0));
                painter.rect_filled(
                    bar,
                    egui::CornerRadius {
                        nw: 10,
                        ne: 10,
                        sw: 0,
                        se: 0,
                    },
                    Color32::from_rgba_unmultiplied(10, 14, 20, 153),
                );
                let window =
                    egui::Rect::from_min_size(rect.min + egui::vec2(30.0, 22.0), egui::vec2(130.0, 80.0));
                painter.rect_filled(window, 5.0, Color32::WHITE);
                let colors = [0x6CB8FF, 0x363B44, 0x4FB0FF, 0xFF7E9D, 0xA3AAB5];
                let plate = egui::Rect::from_center_size(
                    egui::pos2(rect.center().x, rect.bottom() - 17.0),
                    egui::vec2(5.0 * 14.0 + 4.0 * 4.0 + 12.0, 22.0),
                );
                painter.rect(
                    plate,
                    7.0,
                    Color32::from_white_alpha(51),
                    Stroke::new(1.0, Color32::from_white_alpha(77)),
                    egui::StrokeKind::Inside,
                );
                for (index, color) in colors.into_iter().enumerate() {
                    let icon = egui::Rect::from_min_size(
                        plate.min + egui::vec2(6.0 + index as f32 * 18.0, 4.0),
                        egui::vec2(14.0, 14.0),
                    );
                    painter.rect_filled(icon, 4.0, rgb(color));
                }
                ui.vertical(|ui| {
                    ui.add_space(40.0);
                    ui.label(
                        RichText::new("Bureau et dock")
                            .size(18.0)
                            .family(style::strong_family()),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "L’aperçu suit vos réglages : taille du dock, position, coins actifs et espaces de travail.",
                        )
                        .size(13.0)
                        .color(style::text_secondary()),
                    );
                });
            });
        });
}

/// Fond de Ferrite : sa couleur et son hexagone.
fn builtin_wallpaper_tile(ui: &mut egui::Ui, builtin: Builtin, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(WALLPAPER_TILE, egui::Sense::click());
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(builtin.label());
    let (background, stroke) = builtin.colors();
    let color = |c: ferrite_ui::theme::Color| Color32::from_rgb(c.r, c.g, c.b);
    let painter = ui.painter();
    painter.rect_filled(rect, 10.0, color(background));
    let center = rect.center();
    let at = |x: f32, y: f32| center + egui::vec2(x - 22.0, y - 22.0);
    let points = [
        at(22.0, 4.0),
        at(38.0, 13.0),
        at(38.0, 31.0),
        at(22.0, 40.0),
        at(6.0, 31.0),
        at(6.0, 13.0),
        at(22.0, 4.0),
    ];
    painter.add(egui::Shape::line(
        points.to_vec(),
        Stroke::new(2.0, color(stroke)),
    ));
    let ring = if selected {
        Stroke::new(2.0, style::accent())
    } else {
        Stroke::new(1.0, rgb(0xD5D9E0))
    };
    let offset = if selected { 3.0 } else { 0.0 };
    painter.rect_stroke(
        rect.expand(offset),
        10.0 + offset,
        ring,
        egui::StrokeKind::Outside,
    );
    response
}

/// Case « Ajouter… » au trait pointillé.
fn add_wallpaper_tile(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(WALLPAPER_TILE, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter();
    let corners = rect.shrink(1.0);
    let path = [
        corners.left_top(),
        corners.right_top(),
        corners.right_bottom(),
        corners.left_bottom(),
        corners.left_top(),
    ];
    painter.extend(egui::Shape::dashed_line(
        &path,
        Stroke::new(2.0, rgb(0xAEB6C2)),
        6.0,
        4.0,
    ));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Ajouter…",
        egui::FontId::proportional(13.0),
        style::text_secondary(),
    );
    response
}

/// Cadre des fenêtres de dialogue de la page.
/// Sécurité lisible : la plus forte annoncée (« WPA2 WPA3 » → « WPA3 »).
fn security_label(security: &str) -> String {
    security
        .split_whitespace()
        .rfind(|part| *part != "--")
        .unwrap_or("Ouvert")
        .to_owned()
}

/// Bande d'après la fréquence (« 5180 MHz »).
fn band_label(frequency: &str) -> String {
    let megahertz: u32 = frequency
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    match megahertz {
        0 => "—".to_owned(),
        5900.. => "6 GHz".to_owned(),
        4900.. => "5 GHz".to_owned(),
        _ => "2,4 GHz".to_owned(),
    }
}

fn signal_label(signal: u8) -> String {
    match signal {
        75.. => "Excellent",
        50.. => "Bon",
        25.. => "Moyen",
        _ => "Faible",
    }
    .to_owned()
}

/// Ondes Wi-Fi dans `rect`, comme la planche : trois arcs centrés sous eux, et un point.
fn wifi_glyph(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let scale = rect.width() / 24.0;
    let at = |x: f32, y: f32| rect.min + egui::vec2(x, y) * scale;
    let stroke = Stroke::new(2.0 * scale, color);
    // Rayon et demi-largeur de chaque arc, de la planche (`M2 9 a15 15 0 0 1 20 0`…).
    for (radius, half) in [(15.0_f32, 10.0_f32), (10.0, 6.5), (5.0, 3.0)] {
        let rise = (radius * radius - half * half).sqrt();
        let (start, end) = ((-rise).atan2(-half), (-rise).atan2(half));
        let points: Vec<egui::Pos2> = (0..=20)
            .map(|step| {
                let angle = start + (end - start) * step as f32 / 20.0;
                at(12.0 + radius * angle.cos(), 20.0 + radius * angle.sin())
            })
            .collect();
        painter.add(egui::Shape::line(points, stroke));
    }
    painter.circle_filled(at(12.0, 19.5), 1.5 * scale, color);
}

#[cfg(test)]
mod details_tests {
    use super::*;

    #[test]
    fn libelles_du_reseau() {
        assert_eq!(security_label("WPA2 WPA3"), "WPA3");
        assert_eq!(security_label(""), "Ouvert");
        assert_eq!(band_label("5180 MHz"), "5 GHz");
        assert_eq!(band_label("2437 MHz"), "2,4 GHz");
        assert_eq!(signal_label(80), "Excellent");
    }
}

const MULTI_WINDOW_APPS: [(&str, &str); 5] = [
    ("Fichiers", "ferrite-files"),
    ("TextEdit", "ferrite-textedit"),
    ("Calculatrice", "ferrite-calculator"),
    ("Paramètres", "ferrite-settings"),
    ("Moniteur d'activité", "ferrite-monitor"),
];

/// Dock d'origine (même ordre que le compositeur).
const DEFAULT_DOCK: [&str; 5] = [
    "ferrite-files",
    "ferrite-textedit",
    "ferrite-terminal",
    "ferrite-calculator",
    "ferrite-settings",
];

/// Nom complet (champ GECOS de `/etc/passwd`) et identifiant de l'utilisateur.
fn current_user() -> (String, String) {
    let login = std::env::var("USER").unwrap_or_else(|_| "ferrite".to_owned());
    let full = std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let fields: Vec<&str> = line.split(':').collect();
                (fields.first() == Some(&login.as_str()))
                    .then(|| {
                        fields
                            .get(4)
                            .map(|gecos| gecos.split(',').next().unwrap_or("").to_owned())
                    })
                    .flatten()
            })
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            let mut name = login.clone();
            if let Some(first) = name.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            name
        });
    (full, login)
}

// ---- Mise en page commune ------------------------------------------------------------

/// Titre de section, en gras.
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(20.0);
    ui.label(RichText::new(text).size(17.0).family(style::strong_family()));
    ui.add_space(8.0);
}

/// Texte d'explication, en gris.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(style::text_secondary()));
}

/// Carte blanche bordée, pour une liste (réseaux, appareils, sorties).
fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(style::background())
        .stroke(Stroke::new(1.0, style::tint(16)))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(20, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// Ligne d'une liste dans une carte.
fn list_row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(44.0);
        add(ui);
    });
}

/// Trait fin entre deux lignes d'une carte.
fn separator(ui: &mut egui::Ui) {
    let rect = ui.available_rect_before_wrap();
    ui.painter()
        .hline(rect.x_range(), rect.top(), Stroke::new(1.0, style::separator()));
    ui.add_space(1.0);
}

/// Interrupteur suivi de son titre (et d'un détail dessous), comme sur la maquette ;
/// vrai s'il vient de changer.
fn toggle(ui: &mut egui::Ui, on: &mut bool, enabled: bool, title: &str, detail: &str) -> bool {
    ui.horizontal(|ui| {
        let changed = style::switch(ui, on, enabled).changed();
        ui.add_space(10.0);
        ui.vertical(|ui| {
            if detail.is_empty() {
                ui.add_space(3.0);
            }
            ui.label(RichText::new(title).size(14.5));
            if !detail.is_empty() {
                ui.label(RichText::new(detail).size(11.5).color(style::text_secondary()));
            }
        });
        changed
    })
    .inner
}

/// Réglage sur une ligne : « Libellé : » puis la commande.
fn field<R>(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        ui.label(RichText::new(format!("{label}\u{a0}:")).size(14.0));
        ui.add_space(6.0);
        add(ui)
    })
    .inner
}

/// Caractéristique de l'ordinateur : nom en gras, valeur à côté (renvoyée à la ligne
/// si elle est trop longue, sans élargir la page).
fn spec(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(name).size(14.0).family(style::strong_family()));
        ui.label(RichText::new(value).size(14.0));
    });
}

/// Nom du processeur sans les mentions commerciales : « 12th Gen Intel(R) Core(TM)
/// i5-12500H » devient « Intel Core i5-12500H ».
fn short_processor(name: &str) -> String {
    let cleaned = name.replace("(R)", "").replace("(TM)", "").replace(" CPU", "");
    let words: Vec<&str> = cleaned
        .split_whitespace()
        // « 12th Gen » : génération, sans intérêt ici.
        .filter(|word| {
            *word != "Gen" && !(word.starts_with(|c: char| c.is_ascii_digit()) && word.ends_with("th"))
        })
        .collect();
    let mut text = words.join(" ");
    if let Some(at) = text.find(" @ ") {
        text.truncate(at);
    }
    text
}

/// Ordinateur de bureau dessiné, dans les bleus pâles de la maquette.
fn paint_computer(painter: &egui::Painter, rect: egui::Rect) {
    let body = egui::Rect::from_min_size(rect.min + egui::vec2(6.0, 4.0), egui::vec2(158.0, 104.0));
    painter.rect_filled(
        body.translate(egui::vec2(0.0, 2.0)),
        7.0,
        Color32::from_black_alpha(22),
    );
    painter.rect_filled(body, 7.0, style::themed(rgb(0xA9D7F4), rgb(0x3F6F92)));
    let screen = egui::Rect::from_min_max(body.min + egui::vec2(5.0, 5.0), body.max - egui::vec2(5.0, 20.0));
    painter.rect_filled(screen, 3.0, style::themed(rgb(0xDDF0FD), rgb(0x1F3F56)));
    let chin = egui::Rect::from_min_max(egui::pos2(body.left(), body.bottom() - 15.0), body.max);
    painter.rect_filled(
        chin,
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: 7,
            se: 7,
        },
        style::themed(rgb(0x8EC9EE), rgb(0x345F80)),
    );
    let neck = egui::Rect::from_center_size(
        egui::pos2(body.center().x, body.bottom() + 11.0),
        egui::vec2(44.0, 22.0),
    );
    painter.rect_filled(neck, 2.0, style::themed(rgb(0x9CCFEF), rgb(0x38668A)));
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(neck.center().x, neck.bottom() + 1.0),
            egui::vec2(60.0, 4.0),
        ),
        2.0,
        style::themed(rgb(0x7FB9DF), rgb(0x2E5875)),
    );
}

/// Barre d'occupation du disque : partie utilisée à la couleur d'accentuation.
fn storage_bar(ui: &mut egui::Ui, used: f64) {
    let width = ui.available_width().min(800.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        14.0,
        style::themed(rgb(0xD1D1D6), style::control_background()),
    );
    let filled = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width() * used.clamp(0.02, 1.0) as f32, rect.height()),
    );
    painter.rect_filled(filled, 14.0, style::accent());
    painter.rect_stroke(
        rect,
        14.0,
        Stroke::new(1.0, style::tint(20)),
        egui::StrokeKind::Inside,
    );
}

/// Taille des vignettes de fond d'écran, en pixels (16:10, comme l'écran du ThinkBook).
const THUMBNAIL: (u32, u32) = (192, 120);

/// Images proposées comme fond d'écran : celles des dossiers d'images de l'utilisateur et
/// les fonds installés avec le système, sans descendre trop profond.
fn find_images() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut roots: Vec<PathBuf> = ["Images", "Pictures"]
        .into_iter()
        .map(|dir| home.join(dir))
        .collect();
    roots.push(PathBuf::from("/usr/share/backgrounds"));
    let mut found = Vec::new();
    for root in roots {
        collect_images(&root, 3, &mut found);
    }
    found.sort();
    found.dedup();
    found.truncate(60);
    found
}

fn collect_images(dir: &Path, depth: u32, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if depth > 0 {
                collect_images(&path, depth - 1, found);
            }
        } else if is_image(&path) {
            found.push(path);
        }
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
}

/// Nom affiché d'un fond : celui du fond Ferrite, ou le nom du fichier sans extension.
fn source_label(source: &Source) -> String {
    match source {
        Source::Builtin(builtin) => builtin.label().to_owned(),
        Source::File(path) => path.file_stem().map_or_else(
            || path.display().to_string(),
            |stem| stem.to_string_lossy().into_owned(),
        ),
    }
}

fn thumbnail_texture(ctx: &egui::Context, name: &str, image: &Image) -> egui::TextureHandle {
    let pixels =
        egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.rgba);
    ctx.load_texture(format!("fond:{name}"), pixels, egui::TextureOptions::LINEAR)
}

/// Vignette arrondie ; absente : en cours de lecture ; `Some(None)` : image illisible.
fn paint_thumbnail(ui: &egui::Ui, rect: egui::Rect, thumbnail: Option<&Option<egui::TextureHandle>>) {
    match thumbnail {
        Some(Some(texture)) => {
            egui::Image::new(texture).corner_radius(6).paint_at(ui, rect);
        }
        _ => {
            ui.painter().rect_filled(rect, 6.0, style::control_background());
            let text = if thumbnail.is_some() { "Illisible" } else { "…" };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(14.0),
                style::text_secondary(),
            );
        }
    }
}

/// Case d'un fond : vignette, cadre de la couleur d'accentuation s'il est choisi, nom
/// dessous.
fn wallpaper_tile(
    ui: &mut egui::Ui,
    thumbnail: Option<&Option<egui::TextureHandle>>,
    name: &str,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(124.0, 100.0), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let image = egui::Rect::from_min_size(rect.min + egui::vec2(3.0, 3.0), egui::vec2(118.0, 74.0));
    paint_thumbnail(ui, image, thumbnail);
    let ring = if selected {
        Stroke::new(3.0, style::accent())
    } else if response.hovered() {
        Stroke::new(1.0, style::tint(60))
    } else {
        Stroke::new(1.0, style::tint(22))
    };
    ui.painter()
        .rect_stroke(image, 6.0, ring, egui::StrokeKind::Outside);
    let font = egui::FontId::proportional(12.0);
    // Nom trop long : coupé avec « … ».
    let fits = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), style::primary_text())
            .size()
            .x
            <= rect.width() - 6.0
    };
    let mut label = name.to_owned();
    if !fits(&label) {
        let mut chars: Vec<char> = name.chars().collect();
        while chars.len() > 1 && !fits(&format!("{}…", chars.iter().collect::<String>())) {
            chars.pop();
        }
        label = format!("{}…", chars.into_iter().collect::<String>());
    }
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 9.0),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        if selected {
            style::primary_text()
        } else {
            style::text_secondary()
        },
    );
    response
}

fn sidebar_icons_label(size: DockSize) -> &'static str {
    match size {
        DockSize::Small => "Petite",
        DockSize::Medium => "Moyenne",
        DockSize::Large => "Grande",
    }
}

/// Fenêtre miniature d'un mode d'apparence, comme sur la maquette : claire, sombre, ou
/// moitié-moitié ; son nom dessous.
fn appearance_choice(ui: &mut egui::Ui, mode: Appearance, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(180.0, 152.0), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let card = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 128.0));
    let paint = |painter: &egui::Painter, dark: bool| {
        let (wall, window, bar, line) = if dark {
            (0x16202D, 0x22262D, 0x2A2E35, 0x3A3F47)
        } else {
            (0x6CB8FF, 0xFFFFFF, 0xF2F3F6, 0xD5D9E0)
        };
        painter.rect_filled(card, 12.0, rgb(wall));
        let body = egui::Rect::from_min_max(
            card.min + egui::vec2(18.0, 20.0),
            card.max - egui::vec2(18.0, 0.0),
        );
        let top = egui::CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        };
        painter.rect_filled(body, top, rgb(window));
        let title = egui::Rect::from_min_size(body.min, egui::vec2(body.width(), 14.0));
        painter.rect_filled(title, top, rgb(bar));
        let bar_at = |y: f32, width: f32| {
            egui::Rect::from_min_size(body.min + egui::vec2(12.0, y), egui::vec2(width, 6.0))
        };
        painter.rect_filled(bar_at(26.0, body.width() * 0.6), 3.0, rgb(line));
        painter.rect_filled(bar_at(40.0, body.width() * 0.4), 3.0, rgb(line));
        let button = egui::Rect::from_min_size(body.min + egui::vec2(12.0, 58.0), egui::vec2(56.0, 16.0));
        painter.rect_filled(button, 5.0, rgb(0x2779E0));
    };
    let painter = ui.painter();
    match mode {
        Appearance::Light => paint(painter, false),
        Appearance::Dark => paint(painter, true),
        Appearance::Auto => {
            paint(painter, false);
            let right = egui::Rect::from_min_max(card.center_top(), card.right_bottom());
            paint(&painter.with_clip_rect(right), true);
        }
    }
    if selected {
        painter.rect_stroke(
            card.expand(2.0),
            14.0,
            Stroke::new(2.0, style::accent()),
            egui::StrokeKind::Outside,
        );
    }
    let label = match mode {
        Appearance::Light => "Clair",
        Appearance::Dark => "Sombre",
        Appearance::Auto => "Automatique",
    };
    let font = if selected {
        egui::FontId::new(13.0, style::strong_family())
    } else {
        egui::FontId::proportional(13.0)
    };
    painter.text(
        egui::pos2(card.center().x, rect.bottom()),
        egui::Align2::CENTER_BOTTOM,
        label,
        font,
        style::primary_text(),
    );
    response
}

/// Pastilles des couleurs d'accentuation, en ligne ; la choisie est cerclée.
fn accent_picker(ui: &mut egui::Ui, accent: &mut Accent) {
    const STEP: f32 = 44.0;
    let count = Accent::ALL.len() as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(count * STEP, 40.0), egui::Sense::hover());
    for (index, choice) in Accent::ALL.into_iter().enumerate() {
        let center = egui::pos2(rect.left() + STEP * (index as f32 + 0.5), rect.center().y);
        let hit = egui::Rect::from_center_size(center, egui::Vec2::splat(STEP));
        let response = ui
            .interact(hit, ui.scope_id().with(("accent", index)), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(choice.label());
        if response.clicked() {
            *accent = choice;
        }
        let painter = ui.painter();
        let radius = 15.0;
        if *accent == choice {
            painter.circle_stroke(
                center,
                radius + 3.0,
                Stroke::new(2.0, style::accent_color(choice)),
            );
        }
        painter.circle_filled(center, radius, style::accent_color(choice));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duree_d_allumage() {
        assert_eq!(format_uptime(59 * 60), "59 min");
        assert_eq!(format_uptime(2 * 3600 + 5 * 60), "2 h 05");
    }

    #[test]
    fn balance_affichee() {
        assert_eq!(balance_label(50), "Centre");
        assert_eq!(balance_label(30), "G 40");
        assert_eq!(balance_label(60), "D 20");
    }

    #[test]
    fn rubrique_demandee_au_lancement() {
        assert!(Page::from_key("wifi") == Some(Page::Network));
        assert!(Page::from_key("inconnu").is_none());
    }

    #[test]
    fn nom_court_du_processeur() {
        assert_eq!(
            short_processor("12th Gen Intel(R) Core(TM) i5-12500H"),
            "Intel Core i5-12500H"
        );
        assert_eq!(
            short_processor("AMD Ryzen 7 5800U with Radeon Graphics"),
            "AMD Ryzen 7 5800U with Radeon Graphics"
        );
        assert_eq!(
            short_processor("Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz"),
            "Intel Core i7-8550U"
        );
    }
}

/// Heure modifiable (heures et minutes, par pas de 5 min) ; `minutes` depuis minuit.
fn time_editor(ui: &mut egui::Ui, minutes: &mut u32) {
    let (mut hours, mut rest) = (*minutes / 60, *minutes % 60);
    ui.add(
        egui::DragValue::new(&mut rest)
            .range(0..=55)
            .speed(0.2)
            .custom_formatter(|value, _| format!("{value:02}")),
    );
    ui.label(":");
    ui.add(
        egui::DragValue::new(&mut hours)
            .range(0..=23)
            .speed(0.1)
            .custom_formatter(|value, _| format!("{value:02}")),
    );
    *minutes = hours * 60 + rest / 5 * 5;
}
