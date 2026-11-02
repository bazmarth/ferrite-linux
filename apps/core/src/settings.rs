//! Paramètres, organisés comme les Réglages de macOS : une colonne teintée par le fond
//! d'écran (rubriques sur des pastilles colorées), et à droite un grand titre suivi de
//! sections. Rubriques : à propos, Wi-Fi, Bluetooth, son, Dock et barre des menus,
//! réglages du bureau (fond d'écran, apparence, écran), clavier, date et heure.
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

use eframe::egui::{self, Color32, Frame, Margin, RichText, Stroke};
use ferrite_system::{
    Snapshot,
    about::{self, About},
    audio::{self, Sink},
    bluetooth,
    config::{Accent, Appearance, DockHiding, DockSize, KEYBOARD_LAYOUTS, ScrollBars, Settings},
    display,
    network::{self, WifiNetwork},
    time,
};
use ferrite_ui::wallpaper::{self, Builtin, Image, Source};

use crate::style::{self, Icons};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    About,
    Network,
    Bluetooth,
    Sound,
    Dock,
    Desktop,
    Keyboard,
    DateTime,
}

impl Page {
    /// Rubriques de la colonne, groupées comme dans les Réglages de macOS (sans ce qui
    /// n'existe pas sur un PC : compte Apple, partage familial, Touch ID…).
    const GROUPS: [&'static [Page]; 4] = [
        &[Page::About],
        &[Page::Network, Page::Bluetooth, Page::Sound],
        &[Page::Dock, Page::Desktop],
        &[Page::Keyboard, Page::DateTime],
    ];

    fn title(self) -> &'static str {
        match self {
            Page::About => "À propos de cet ordinateur",
            Page::Network => "Wi-Fi",
            Page::Bluetooth => "Bluetooth",
            Page::Sound => "Son",
            Page::Dock => "Dock et barre des menus",
            Page::Desktop => "Réglages du bureau",
            Page::Keyboard => "Clavier",
            Page::DateTime => "Date et heure",
        }
    }

    /// Couleur de la pastille de la rubrique.
    fn color(self) -> Color32 {
        rgb(match self {
            Page::About => 0x1E7FD8,
            Page::Network | Page::Bluetooth => 0x0A7AFF,
            Page::Sound => 0xFF2D3A,
            Page::Dock => 0x4FC8DA,
            Page::Desktop => 0x2FB2D6,
            Page::Keyboard => 0x98989D,
            Page::DateTime => 0xFF6A1A,
        })
    }

    fn icon(self) -> &'static str {
        match self {
            Page::About => "settings-computer",
            Page::Network => "settings-wifi",
            Page::Bluetooth => "settings-bluetooth",
            Page::Sound => "settings-sound",
            Page::Dock => "settings-dock",
            Page::Desktop => "settings-desktop",
            Page::Keyboard => "settings-keyboard",
            Page::DateTime => "settings-clock",
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
    Password(String),
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
    /// Teinte de la colonne (haut, bas) tirée de chaque fond d'écran.
    tints: HashMap<Source, (Color32, Color32)>,
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
            page: Page::About,
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
            tints: HashMap::new(),
        }
    }
}

impl SettingsState {
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
                    let texture = image.map(|image| {
                        self.tints.insert(source.clone(), tint_of(&image));
                        thumbnail_texture(ctx, &source.key(), &image)
                    });
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
            Page::Bluetooth => self.refresh_bluetooth(ctx),
            Page::Sound => self.spawn(ctx, || vec![Update::Sinks(audio::sinks())]),
            Page::DateTime => self.spawn(ctx, || {
                vec![
                    Update::Time(time::timezone(), time::ntp_enabled()),
                    Update::Timezones(time::timezones()),
                ]
            }),
            Page::About => self.spawn(ctx, || vec![Update::About(Box::new(about::read()))]),
            Page::Desktop => self.spawn(ctx, || vec![Update::WallpaperFiles(find_images())]),
            Page::Dock | Page::Keyboard => {}
        }
    }

    fn refresh_networks(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, || {
            vec![
                Update::NetworkDevices(network::devices()),
                Update::Networks(network::wifi_networks()),
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

        let tint = self.sidebar_tint(&ctx);
        style::tinted_sidebar(ui, "rubriques", 260.0, tint, |ui| self.sidebar(ui));
        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(style::content_gray()).inner_margin(Margin {
                left: 40,
                right: 40,
                top: 0,
                bottom: 12,
            }))
            .show(ui, |ui| {
                // Pas de barre d'outils : la bande du haut déplace la fenêtre.
                let top = ui.max_rect();
                let strip = egui::Rect::from_min_size(
                    top.min - egui::vec2(40.0, 0.0),
                    egui::vec2(top.width() + 80.0, style::TOOLBAR_HEIGHT),
                );
                style::drag_area(ui, strip, "contenu");
                ui.add_space(34.0);
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| match self.page {
                        Page::About => self.about_page(ui, &ctx),
                        Page::Network => self.network_page(ui, &ctx),
                        Page::Bluetooth => self.bluetooth_page(ui, &ctx),
                        Page::Sound => self.sound_page(ui, &ctx),
                        Page::Dock => self.dock_page(ui),
                        Page::Desktop => self.desktop_page(ui, &ctx),
                        Page::Keyboard => self.keyboard_page(ui),
                        Page::DateTime => self.datetime_page(ui, &ctx),
                    });
            });
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
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Rechercher")
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(10, 7)),
        );
        ui.add_space(10.0);
        let query = self.search.trim().to_lowercase();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let mut first = true;
            for group in Page::GROUPS {
                let pages: Vec<Page> = group
                    .iter()
                    .copied()
                    .filter(|page| query.is_empty() || page.title().to_lowercase().contains(&query))
                    .collect();
                if pages.is_empty() {
                    continue;
                }
                // Trait fin entre deux groupes, en retrait comme sur macOS.
                if !first {
                    ui.add_space(5.0);
                    let rect = ui.available_rect_before_wrap();
                    ui.painter().hline(
                        rect.left() + 12.0..=rect.right() - 12.0,
                        rect.top(),
                        Stroke::new(1.0, Color32::from_black_alpha(28)),
                    );
                    ui.add_space(6.0);
                }
                first = false;
                for page in pages {
                    let response = style::sidebar_tile_item(
                        ui,
                        &mut self.icons,
                        page.icon(),
                        page.color(),
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

    /// Teinte pastel de la colonne, tirée du fond d'écran comme sur macOS ; grise tant que
    /// l'image n'est pas lue.
    fn sidebar_tint(&mut self, ctx: &egui::Context) -> (Color32, Color32) {
        let source = Source::parse(Settings::load().wallpaper());
        if let Some(tint) = self.tints.get(&source) {
            return *tint;
        }
        match &source {
            Source::Builtin(builtin) => {
                let tint = tint_of(&wallpaper::waves(*builtin, 48, 30));
                self.tints.insert(source, tint);
                tint
            }
            Source::File(path) => {
                if !self.requested.contains(path) {
                    self.load_thumbnails(ctx, vec![path.clone()]);
                }
                (style::sidebar_color(), style::sidebar_color())
            }
        }
    }

    // ---- À propos ---------------------------------------------------------------------

    fn about_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
        let current = Source::parse(Settings::load().wallpaper());
        self.prepare_thumbnails(ctx, &current);
        ui.horizontal(|ui| {
            // Pastille du système : le fond d'écran en rond, comme le logo de macOS.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(116.0, 116.0), egui::Sense::hover());
            let disc = rect.shrink(8.0);
            ui.painter().circle_filled(
                disc.center() + egui::vec2(0.0, 2.0),
                disc.width() / 2.0 + 2.0,
                Color32::from_black_alpha(30),
            );
            ui.painter()
                .circle_filled(disc.center(), disc.width() / 2.0 + 2.0, Color32::WHITE);
            if let Some(Some(texture)) = self.thumbnails.get(&current) {
                // La vignette est en 16:10 : on n'en garde qu'un carré centré.
                let uv = egui::Rect::from_min_max(egui::pos2(0.1875, 0.0), egui::pos2(0.8125, 1.0));
                egui::Image::new(texture)
                    .uv(uv)
                    .corner_radius((disc.width() / 2.0) as u8)
                    .paint_at(ui, disc);
            }
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
        page_title(ui, "Wi-Fi");
        let wifi = self.snapshot.and_then(|snapshot| snapshot.wifi);
        let joined = self
            .networks
            .iter()
            .flatten()
            .find(|network| network.connected)
            .map(|network| network.ssid.clone());
        let detail = match (wifi, joined) {
            (Some(true), Some(ssid)) => format!("Connecté à « {ssid} »"),
            (Some(true), None) => "Aucun réseau Wi-Fi connecté".to_owned(),
            (Some(false), _) => "Désactivé".to_owned(),
            (None, _) => "Aucune carte Wi-Fi détectée".to_owned(),
        };
        let mut on = wifi == Some(true);
        let title = if on { "Wi-Fi activé" } else { "Wi-Fi" };
        if toggle(ui, &mut on, wifi.is_some(), title, &detail) {
            let message = if on { "Wi-Fi activé" } else { "Wi-Fi désactivé" };
            self.act(ctx, message, move || network::set_wifi(on));
        }

        // Connexions actives (filaire comprise).
        let connected: Vec<_> = self
            .network_devices
            .iter()
            .filter(|device| device.state.starts_with("connected"))
            .cloned()
            .collect();
        if !connected.is_empty() {
            heading(ui, "Connexions actives");
            card(ui, |ui| {
                for (index, device) in connected.iter().enumerate() {
                    if index > 0 {
                        separator(ui);
                    }
                    let kind = match device.kind.as_str() {
                        "wifi" => "Wi-Fi",
                        "ethernet" => "Filaire",
                        other => other,
                    };
                    list_row(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&device.connection).size(14.0));
                            ui.label(
                                RichText::new(format!("{kind} · {}", device.name))
                                    .size(11.5)
                                    .color(style::text_secondary()),
                            );
                        });
                    });
                }
            });
        }

        if wifi != Some(true) {
            return;
        }
        if heading_with_button(ui, "Réseaux disponibles", "Rechercher") {
            self.refresh_networks(ctx);
        }
        let Some(networks) = self.networks.clone() else {
            ui.label(RichText::new("Recherche des réseaux…").color(style::text_secondary()));
            return;
        };
        if networks.is_empty() {
            ui.label(RichText::new("Aucun réseau à portée.").color(style::text_secondary()));
            return;
        }
        let mut action = None;
        card(ui, |ui| {
            for (index, network) in networks.iter().enumerate() {
                if index > 0 {
                    separator(ui);
                }
                list_row(ui, |ui| {
                    signal_bars(ui, network.signal);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        let name = if network.connected {
                            RichText::new(&network.ssid).family(style::strong_family())
                        } else {
                            RichText::new(&network.ssid)
                        };
                        ui.label(name.size(14.0));
                        let detail = match (network.connected, network.secured) {
                            (true, _) => "Connecté",
                            (false, true) => "Protégé par un mot de passe",
                            (false, false) => "Ouvert",
                        };
                        ui.label(RichText::new(detail).size(11.5).color(style::text_secondary()));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ssid = network.ssid.clone();
                        if network.connected {
                            if ui.button("Déconnecter").clicked() {
                                action = Some(ListAction::Disconnect(ssid));
                            }
                        } else if ui.button("Se connecter").clicked() {
                            action = Some(if network.secured {
                                ListAction::Password(ssid)
                            } else {
                                ListAction::Connect(ssid)
                            });
                        }
                    });
                });
            }
        });
        match action {
            Some(ListAction::Disconnect(ssid)) => {
                self.act(ctx, "Déconnecté", move || network::disconnect_wifi(&ssid));
                self.loaded.retain(|page| *page != Page::Network);
            }
            Some(ListAction::Connect(ssid)) => {
                self.act(ctx, "Connecté", move || network::connect_wifi(&ssid, None));
                self.loaded.retain(|page| *page != Page::Network);
            }
            Some(ListAction::Password(ssid)) => {
                self.password_for = Some(ssid);
                self.password.clear();
                self.password_visible = false;
            }
            Some(ListAction::Forget(_)) | None => {}
        }
    }

    fn wifi_password_dialog(&mut self, ctx: &egui::Context) {
        let Some(ssid) = self.password_for.clone() else {
            return;
        };
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
                    ui.label(RichText::new(&ssid).size(15.0).color(style::text_secondary()));
                });
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
        } else if connect {
            let password = std::mem::take(&mut self.password);
            self.password_for = None;
            self.password_visible = false;
            self.act(ctx, "Connecté", move || {
                network::connect_wifi(&ssid, Some(&password))
            });
        }
    }

    // ---- Bluetooth --------------------------------------------------------------------

    fn bluetooth_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        page_title(ui, "Bluetooth");
        let powered = self.snapshot.and_then(|snapshot| snapshot.bluetooth);
        let detail = match powered {
            Some(true) => "Visible des appareils proches",
            Some(false) => "Désactivé",
            None => "Aucun adaptateur Bluetooth détecté",
        };
        let mut on = powered == Some(true);
        let title = if on { "Bluetooth activé" } else { "Bluetooth" };
        if toggle(ui, &mut on, powered.is_some(), title, detail) {
            let message = if on {
                "Bluetooth activé"
            } else {
                "Bluetooth désactivé"
            };
            self.act(ctx, message, move || bluetooth::set_powered(on));
        }
        if powered != Some(true) {
            return;
        }
        let Some(devices) = self.bluetooth_devices.clone() else {
            heading(ui, "Mes appareils");
            ui.label(RichText::new("Lecture des appareils…").color(style::text_secondary()));
            return;
        };
        let (paired, others): (Vec<_>, Vec<_>) = devices.into_iter().partition(|device| device.paired);
        let mut action = None;
        if !paired.is_empty() {
            heading(ui, "Mes appareils");
            action = self.device_cards(ui, &paired).or(action);
        }
        if heading_with_button(ui, "Autres appareils", "Rechercher") {
            self.spawn(ctx, || {
                let scan = bluetooth::scan();
                let mut updates = vec![Update::BluetoothDevices(bluetooth::devices())];
                if let Err(error) = scan {
                    updates.push(Update::Done(Err(error)));
                }
                updates
            });
        }
        if others.is_empty() {
            note(ui, "Mets l'appareil en mode appairage, puis Rechercher.");
        } else {
            action = self.device_cards(ui, &others).or(action);
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
            Some(ListAction::Password(_)) | None => {}
        }
    }

    /// Appareils sur une carte, deux par ligne : image, nom, état et boutons.
    fn device_cards(&mut self, ui: &mut egui::Ui, devices: &[bluetooth::Device]) -> Option<ListAction> {
        let mut action = None;
        card(ui, |ui| {
            for (index, pair) in devices.chunks(2).enumerate() {
                if index > 0 {
                    ui.add_space(14.0);
                }
                ui.columns(2, |columns| {
                    for (column, device) in columns.iter_mut().zip(pair) {
                        column.horizontal(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(84.0, 84.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, 18.0, rgb(0xF2F2F6));
                            let ink = if device.connected {
                                rgb(0x0A7AFF)
                            } else {
                                rgb(0x8E8E93)
                            };
                            self.icons.paint(
                                ui,
                                device_icon(&device.name),
                                rect.shrink(20.0),
                                style::IconStyle::Tinted(ink),
                            );
                            ui.add_space(14.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&device.name).size(14.0));
                                let status = match (device.connected, device.paired) {
                                    (true, _) => "Connecté",
                                    (false, true) => "Non connecté",
                                    _ => "Disponible",
                                };
                                ui.label(RichText::new(status).size(13.0).color(style::text_secondary()));
                                ui.add_space(4.0);
                                let address = device.address.clone();
                                let main = if device.connected {
                                    "Déconnecter"
                                } else {
                                    "Connecter"
                                };
                                if ui
                                    .add(egui::Button::new(main).min_size(egui::vec2(120.0, 0.0)))
                                    .clicked()
                                {
                                    action = Some(if device.connected {
                                        ListAction::Disconnect(address.clone())
                                    } else {
                                        ListAction::Connect(address.clone())
                                    });
                                }
                                if device.paired
                                    && ui
                                        .add(egui::Button::new("Oublier").min_size(egui::vec2(120.0, 0.0)))
                                        .clicked()
                                {
                                    action = Some(ListAction::Forget(address));
                                }
                            });
                        });
                    }
                });
            }
        });
        action
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
        page_title(ui, "Son");
        let volume = self.snapshot.and_then(|snapshot| snapshot.volume);
        let Some(current) = volume else {
            note(ui, "Aucune sortie audio : PipeWire n'est pas démarré.");
            return;
        };
        field(ui, "Volume de sortie", |ui| {
            ui.spacing_mut().slider_width = 300.0;
            let response = ui.add(egui::Slider::new(&mut self.volume, 0..=100).show_value(false));
            ui.label(RichText::new(format!("{} %", self.volume)).color(style::text_secondary()));
            if response.changed() {
                self.touched = Some(Instant::now());
            }
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                let level = self.volume;
                self.act(ctx, "", move || audio::set_volume(level));
            }
        });
        let mut muted = current.muted;
        if toggle(
            ui,
            &mut muted,
            true,
            "Couper le son",
            "Coupe la sortie sans changer le volume",
        ) {
            self.act(ctx, "", move || audio::set_muted(muted));
        }
        if self.sinks.is_empty() {
            return;
        }
        heading(ui, "Sortie");
        let mut chosen = None;
        card(ui, |ui| {
            for (index, sink) in self.sinks.iter().enumerate() {
                if index > 0 {
                    separator(ui);
                }
                list_row(ui, |ui| {
                    if radio(ui, sink.default, &sink.name).clicked() && !sink.default {
                        chosen = Some(sink.id);
                    }
                });
            }
        });
        if let Some(id) = chosen {
            self.spawn(ctx, move || {
                let result = audio::set_default_sink(id).map(|()| "Sortie changée".to_owned());
                vec![Update::Done(result), Update::Sinks(audio::sinks())]
            });
        }
    }

    // ---- Dock et barre des menus ------------------------------------------------------

    fn dock_page(&mut self, ui: &mut egui::Ui) {
        page_title(ui, "Dock et barre des menus");
        let mut settings = Settings::load();
        let (size, hiding, bar) = (
            settings.dock_size(),
            settings.dock_hiding(),
            settings.bar_hiding(),
        );
        let (mut new_size, mut new_hiding, mut new_bar) = (size, hiding, bar);
        let mut new_dock_height = settings.dock_height();
        let mut new_bar_height = settings.bar_height();
        let magnify = settings.dock_magnify();
        let mut new_magnify = magnify;
        let multi = settings.multi_window_apps();
        let mut new_multi = multi.clone();
        let custom_dock = settings.dock_pinned().is_some();
        let mut reset_dock = false;

        heading(ui, "Dock");
        field(ui, "Taille", |ui| {
            egui::ComboBox::from_id_salt("dock-taille")
                .selected_text(new_size.label())
                .width(140.0)
                .show_ui(ui, |ui| {
                    for choice in DockSize::ALL {
                        ui.selectable_value(&mut new_size, choice, choice.label());
                    }
                });
        });
        field(ui, "Hauteur du Dock", |ui| {
            ui.add(
                egui::Slider::new(&mut new_dock_height, 68..=120)
                    .suffix(" px")
                    .integer(),
            );
        });
        field(ui, "Masquer le Dock", |ui| {
            egui::ComboBox::from_id_salt("dock-masquage")
                .selected_text(new_hiding.label())
                .width(260.0)
                .show_ui(ui, |ui| {
                    for choice in DockHiding::ALL {
                        ui.selectable_value(&mut new_hiding, choice, choice.label());
                    }
                });
        });
        note(
            ui,
            "Un Dock masqué revient quand la souris touche le bas de l'écran, ou avec la touche Super.",
        );
        ui.add_space(8.0);
        toggle(
            ui,
            &mut new_magnify,
            true,
            "Agrandir les icônes sous la souris",
            "",
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    custom_dock,
                    egui::Button::new("Rétablir les applications du Dock"),
                )
                .clicked()
            {
                reset_dock = true;
            }
            note(
                ui,
                "Clic droit sur une icône du Dock pour la garder ou la retirer.",
            );
        });

        heading(ui, "Barre des menus");
        field(ui, "Hauteur", |ui| {
            ui.add(
                egui::Slider::new(&mut new_bar_height, 24..=60)
                    .suffix(" px")
                    .integer(),
            );
        });
        field(ui, "Masquer la barre des menus", |ui| {
            egui::ComboBox::from_id_salt("barre-masquage")
                .selected_text(new_bar.bar_label())
                .width(260.0)
                .show_ui(ui, |ui| {
                    for choice in DockHiding::ALL {
                        ui.selectable_value(&mut new_bar, choice, choice.bar_label());
                    }
                });
        });
        note(
            ui,
            "Elle revient quand la souris touche le bord du haut de l'écran. En plein écran, la barre et le Dock se masquent toujours.",
        );

        heading(ui, "Fenêtres");
        note(
            ui,
            "Les autres applications n'ont qu'une fenêtre : les relancer ramène celle qui est ouverte, même réduite dans le Dock.",
        );
        ui.add_space(8.0);
        card(ui, |ui| {
            for (index, (name, id)) in MULTI_WINDOW_APPS.into_iter().enumerate() {
                if index > 0 {
                    separator(ui);
                }
                let mut allowed = new_multi.iter().any(|known| known == id);
                list_row(ui, |ui| {
                    ui.label(RichText::new(name).size(14.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if style::switch(ui, &mut allowed, true).changed() {
                            if allowed {
                                new_multi.push(id.to_owned());
                            } else {
                                new_multi.retain(|known| known != id);
                            }
                        }
                        ui.label(RichText::new("Plusieurs fenêtres").color(style::text_secondary()));
                    });
                });
            }
        });

        if (new_size, new_hiding, new_bar, new_magnify) != (size, hiding, bar, magnify)
            || new_dock_height != settings.dock_height()
            || new_bar_height != settings.bar_height()
            || new_multi != multi
            || reset_dock
        {
            settings.set("dock_taille", new_size.key());
            settings.set("dock_masquage", new_hiding.key());
            settings.set("barre_masquage", new_bar.key());
            settings.set("dock_hauteur", &new_dock_height.to_string());
            settings.set("barre_hauteur", &new_bar_height.to_string());
            settings.set("dock_agrandissement", if new_magnify { "oui" } else { "non" });
            settings.set_multi_window_apps(&new_multi);
            if reset_dock {
                settings.set_dock_pinned(&DEFAULT_DOCK.map(str::to_owned));
            }
            self.message = Some(
                settings
                    .save()
                    .map(|()| "Réglage appliqué".to_owned())
                    .map_err(|error| format!("Enregistrement impossible : {error}")),
            );
        }
    }

    // ---- Réglages du bureau -----------------------------------------------------------

    /// Fond d'écran, apparence et écran sur une seule page, comme sur la maquette.
    fn desktop_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        page_title(ui, "Réglages du bureau");
        let mut settings = Settings::load();
        let current = Source::parse(settings.wallpaper());
        let mut wallpaper_choice = current.clone();
        self.prepare_thumbnails(ctx, &current);

        heading(ui, "Fond d’écran");
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                Frame::new()
                    .fill(style::background())
                    .stroke(Stroke::new(1.0, Color32::from_black_alpha(16)))
                    .corner_radius(10)
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(288.0, 180.0), egui::Sense::hover());
                        paint_thumbnail(ui, rect, self.thumbnails.get(&current));
                    });
                ui.add_space(4.0);
                note(
                    ui,
                    "Fais glisser une image dans la fenêtre\npour en faire ton fond d’écran",
                );
            });
            ui.add_space(20.0);
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(source_label(&current))
                        .size(15.0)
                        .family(style::strong_family()),
                );
                let kind = match current {
                    Source::Builtin(_) => "Fond Ferrite",
                    Source::File(_) => "Image, recadrée pour remplir l’écran",
                };
                ui.label(RichText::new(kind).color(style::text_secondary()));
            });
        });
        ui.add_space(12.0);
        subheading(ui, "Fonds Ferrite");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 10.0);
            for builtin in Builtin::ALL {
                let source = Source::Builtin(builtin);
                let thumbnail = self.thumbnails.get(&source);
                if wallpaper_tile(ui, thumbnail, builtin.label(), current == source).clicked() {
                    wallpaper_choice = source;
                }
            }
        });
        subheading(ui, "Vos images");
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
                    Ok(_) => self.fail("Donne le chemin complet de l’image (il commence par /)".to_owned()),
                    Err(error) => self.fail(format!("Image illisible : {error}")),
                }
            }
        });
        if wallpaper_choice != current {
            self.set_wallpaper(&mut settings, &wallpaper_choice);
        }

        self.appearance_section(ui, &mut settings);

        heading(ui, "Écran");
        let available = self.snapshot.and_then(|snapshot| snapshot.brightness).is_some();
        field(ui, "Luminosité", |ui| {
            if available {
                ui.spacing_mut().slider_width = 300.0;
                let response = ui.add(egui::Slider::new(&mut self.brightness, 5..=100).show_value(false));
                ui.label(RichText::new(format!("{} %", self.brightness)).color(style::text_secondary()));
                if response.changed() {
                    self.touched = Some(Instant::now());
                }
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    let level = self.brightness;
                    self.act(ctx, "", move || display::set_brightness(level));
                }
            } else {
                ui.label(RichText::new("Non réglable sur cet écran").color(style::text_secondary()));
            }
        });
        let scale = ui.ctx().pixels_per_point();
        field(ui, "Échelle", |ui| {
            ui.label(RichText::new(format!("{:.0} %", scale * 100.0)).color(style::text_secondary()));
        });
    }

    fn appearance_section(&mut self, ui: &mut egui::Ui, settings: &mut Settings) {
        let current = (
            settings.appearance(),
            settings.accent(),
            settings.highlight(),
            settings.sidebar_icons(),
            settings.scroll_bars(),
        );
        let (mut appearance, mut accent, mut highlight, mut icons, mut scroll) = current;
        heading(ui, "Apparence");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            for mode in Appearance::ALL {
                if appearance_choice(ui, mode, appearance == mode).clicked() {
                    appearance = mode;
                }
            }
        });
        note(
            ui,
            "Auto bascule entre clair (7 h–19 h) et sombre (19 h–7 h, UTC).",
        );
        ui.add_space(6.0);
        field(ui, "Couleur d’accentuation", |ui| {
            accent_picker(ui, &mut accent)
        });
        field(ui, "Couleur de surlignage", |ui| {
            let color = style::accent_color(highlight.unwrap_or(accent));
            let (swatch, _) = ui.allocate_exact_size(egui::vec2(46.0, 22.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(swatch, 5.0, color.lerp_to_gamma(style::background(), 0.6));
            egui::ComboBox::from_id_salt("surlignage")
                .selected_text(highlight.map_or("Couleur d’accentuation", Accent::label))
                .width(190.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut highlight, None, "Couleur d’accentuation");
                    for choice in Accent::ALL.into_iter().skip(1) {
                        ui.selectable_value(&mut highlight, Some(choice), choice.label());
                    }
                });
        });
        field(ui, "Icônes de la barre latérale", |ui| {
            ui.label(RichText::new("Taille\u{a0}:").size(14.0));
            egui::ComboBox::from_id_salt("icones-colonne")
                .selected_text(sidebar_icons_label(icons))
                .width(110.0)
                .show_ui(ui, |ui| {
                    for choice in DockSize::ALL {
                        ui.selectable_value(&mut icons, choice, sidebar_icons_label(choice));
                    }
                });
        });
        field(ui, "Barres de défilement", |ui| {
            egui::ComboBox::from_id_salt("defilement")
                .selected_text(scroll.label())
                .width(340.0)
                .show_ui(ui, |ui| {
                    for choice in ScrollBars::ALL {
                        ui.selectable_value(&mut scroll, choice, choice.label());
                    }
                });
        });

        if (appearance, accent, highlight, icons, scroll) != current {
            settings.set("apparence", appearance.key());
            settings.set("accent", accent.key());
            settings.set("surlignage", highlight.map_or("accent", Accent::key));
            settings.set("barre_laterale_icones", icons.key());
            settings.set("barres_defilement", scroll.key());
            style::apply_preferences(ui.ctx(), settings);
            if let Err(error) = settings.save() {
                self.fail(format!("Enregistrement impossible : {error}"));
            }
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
                    let image = wallpaper::waves(builtin, THUMBNAIL.0, THUMBNAIL.1);
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
        page_title(ui, "Clavier");
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

    // ---- Date et heure ----------------------------------------------------------------

    fn datetime_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        page_title(ui, "Date et heure");
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
/// fenêtres ».
const MULTI_WINDOW_APPS: [(&str, &str); 6] = [
    ("Fichiers", "ferrite-files"),
    ("TextEdit", "ferrite-textedit"),
    ("Terminal", "foot"),
    ("Calculatrice", "ferrite-calculator"),
    ("Paramètres", "ferrite-settings"),
    ("Moniteur d'activité", "ferrite-monitor"),
];

/// Dock d'origine (même ordre que le compositeur).
const DEFAULT_DOCK: [&str; 5] = [
    "ferrite-files",
    "ferrite-textedit",
    "foot",
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

/// Grand titre de la page.
fn page_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(28.0));
    ui.add_space(16.0);
}

/// Titre de section, en gras.
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(20.0);
    ui.label(RichText::new(text).size(17.0).family(style::strong_family()));
    ui.add_space(8.0);
}

/// Titre de section avec un bouton à droite (« Rechercher ») ; vrai s'il est cliqué.
fn heading_with_button(ui: &mut egui::Ui, text: &str, button: &str) -> bool {
    ui.add_space(20.0);
    let clicked = ui
        .horizontal(|ui| {
            ui.label(RichText::new(text).size(17.0).family(style::strong_family()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.button(button).clicked()
            })
            .inner
        })
        .inner;
    ui.add_space(8.0);
    clicked
}

/// Petit titre au-dessus d'une rangée de vignettes.
fn subheading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(
        RichText::new(text)
            .size(12.5)
            .family(style::strong_family())
            .color(style::text_secondary()),
    );
    ui.add_space(4.0);
}

/// Texte d'explication, en gris.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(style::text_secondary()));
}

/// Carte blanche bordée, pour une liste (réseaux, appareils, sorties).
fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(style::background())
        .stroke(Stroke::new(1.0, Color32::from_black_alpha(16)))
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
    painter.rect_filled(body, 7.0, rgb(0xA9D7F4));
    let screen = egui::Rect::from_min_max(body.min + egui::vec2(5.0, 5.0), body.max - egui::vec2(5.0, 20.0));
    painter.rect_filled(screen, 3.0, rgb(0xDDF0FD));
    let chin = egui::Rect::from_min_max(egui::pos2(body.left(), body.bottom() - 15.0), body.max);
    painter.rect_filled(
        chin,
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: 7,
            se: 7,
        },
        rgb(0x8EC9EE),
    );
    let neck = egui::Rect::from_center_size(
        egui::pos2(body.center().x, body.bottom() + 11.0),
        egui::vec2(44.0, 22.0),
    );
    painter.rect_filled(neck, 2.0, rgb(0x9CCFEF));
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(neck.center().x, neck.bottom() + 1.0),
            egui::vec2(60.0, 4.0),
        ),
        2.0,
        rgb(0x7FB9DF),
    );
}

/// Barre d'occupation du disque : partie utilisée à la couleur d'accentuation.
fn storage_bar(ui: &mut egui::Ui, used: f64) {
    let width = ui.available_width().min(800.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 14.0, rgb(0xD1D1D6));
    let filled = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width() * used.clamp(0.02, 1.0) as f32, rect.height()),
    );
    painter.rect_filled(filled, 14.0, style::accent());
    painter.rect_stroke(
        rect,
        14.0,
        Stroke::new(1.0, Color32::from_black_alpha(20)),
        egui::StrokeKind::Inside,
    );
}

/// Force du signal en quatre barres.
fn signal_bars(ui: &mut egui::Ui, signal: u8) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 18.0), egui::Sense::hover());
    let lit = (signal as usize).div_ceil(25).clamp(1, 4);
    for index in 0..4 {
        let height = 5.0 + index as f32 * 4.0;
        let left = rect.left() + index as f32 * 5.5;
        let bar = egui::Rect::from_min_max(
            egui::pos2(left, rect.bottom() - height),
            egui::pos2(left + 3.5, rect.bottom()),
        );
        let color = if index < lit {
            style::primary_text()
        } else {
            Color32::from_black_alpha(40)
        };
        ui.painter().rect_filled(bar, 1.0, color);
    }
}

/// Pictogramme d'un appareil Bluetooth, deviné d'après son nom.
fn device_icon(name: &str) -> &'static str {
    let name = name.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|word| name.contains(word));
    if any(&[
        "casque",
        "headphone",
        "headset",
        "airpods",
        "buds",
        "écouteur",
        "ecouteur",
        "speaker",
        "enceinte",
    ]) {
        "device-headphones"
    } else if any(&["phone", "redmi", "galaxy", "pixel", "xiaomi", "oneplus", "note"]) {
        "device-phone"
    } else if any(&["mouse", "souris"]) {
        "device-mouse"
    } else if any(&["keyboard", "clavier", "keys"]) {
        "settings-keyboard"
    } else {
        "settings-bluetooth"
    }
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

/// Teinte pastel (haut, bas) de la colonne pour un fond : la couleur moyenne de chaque
/// moitié de l'image, très éclaircie.
fn tint_of(image: &Image) -> (Color32, Color32) {
    let row = image.width as usize * 4;
    let half = (image.height / 2).max(1) as usize;
    let average = |pixels: &[u8]| {
        let mut sum = [0u64; 3];
        let count = (pixels.len() / 4).max(1) as u64;
        for pixel in pixels.as_chunks::<4>().0 {
            for (total, channel) in sum.iter_mut().zip(pixel) {
                *total += *channel as u64;
            }
        }
        let [r, g, b] = sum.map(|total| (total / count) as u8);
        Color32::from_rgb(r, g, b).lerp_to_gamma(Color32::WHITE, 0.74)
    };
    let split = (half * row).min(image.rgba.len());
    (average(&image.rgba[..split]), average(&image.rgba[split..]))
}

/// Vignette arrondie ; absente : en cours de lecture ; `Some(None)` : image illisible.
fn paint_thumbnail(ui: &egui::Ui, rect: egui::Rect, thumbnail: Option<&Option<egui::TextureHandle>>) {
    match thumbnail {
        Some(Some(texture)) => {
            egui::Image::new(texture).corner_radius(6).paint_at(ui, rect);
        }
        _ => {
            ui.painter().rect_filled(rect, 6.0, rgb(0xE4E4E8));
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
        Stroke::new(1.0, Color32::from_black_alpha(60))
    } else {
        Stroke::new(1.0, Color32::from_black_alpha(22))
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

/// Bouton radio à la macOS : pastille pleine de la couleur d'accentuation quand il est
/// choisi ; toute la ligne est cliquable.
fn radio(ui: &mut egui::Ui, selected: bool, text: &str) -> egui::Response {
    let font = egui::FontId::proportional(14.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, style::primary_text());
    let size = egui::vec2(26.0 + galley.size().x, galley.size().y.max(24.0));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let center = egui::pos2(rect.left() + 8.0, rect.center().y);
    let painter = ui.painter();
    if selected {
        painter.circle_filled(center, 8.0, style::accent());
        painter.circle_filled(center, 3.0, Color32::WHITE);
    } else {
        painter.circle_filled(center, 8.0, Color32::WHITE);
        painter.circle_stroke(center, 7.5, Stroke::new(1.0, rgb(0xC4C4C9)));
    }
    painter.galley(
        egui::pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0),
        galley,
        style::primary_text(),
    );
    response
}

/// Fenêtre miniature d'un mode d'apparence, comme sur la maquette : claire, sombre, ou
/// moitié-moitié ; son nom dessous.
fn appearance_choice(ui: &mut egui::Ui, mode: Appearance, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(96.0, 92.0), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let window = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 33.0),
        egui::vec2(84.0, 62.0),
    );
    let painter = ui.painter();
    let paint = |painter: &egui::Painter, dark: bool| {
        let (frame, body) = if dark {
            (rgb(0x111113), rgb(0x48484C))
        } else {
            (rgb(0xD2D2D6), Color32::WHITE)
        };
        painter.rect_filled(window, 8.0, frame);
        painter.rect_filled(
            egui::Rect::from_min_max(
                window.min + egui::vec2(6.0, 16.0),
                window.max - egui::vec2(6.0, 6.0),
            ),
            3.0,
            body,
        );
        for (index, color) in [0xFF5F57u32, 0xFEBC2E, 0x28C840].into_iter().enumerate() {
            painter.circle_filled(
                window.min + egui::vec2(10.0 + index as f32 * 8.0, 8.5),
                2.6,
                rgb(color),
            );
        }
    };
    match mode {
        Appearance::Light => paint(painter, false),
        Appearance::Dark => paint(painter, true),
        Appearance::Auto => {
            paint(painter, false);
            let right = egui::Rect::from_min_max(window.center_top(), window.right_bottom());
            paint(&painter.with_clip_rect(right), true);
        }
    }
    if selected {
        painter.rect_stroke(
            window.expand(2.5),
            10.0,
            Stroke::new(2.5, style::accent()),
            egui::StrokeKind::Outside,
        );
    }
    let label = match mode {
        Appearance::Auto => "Automatique",
        other => other.label(),
    };
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 10.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(13.0),
        style::primary_text(),
    );
    response
}

/// Pastilles des couleurs d'accentuation, en ligne ; la choisie est cerclée.
fn accent_picker(ui: &mut egui::Ui, accent: &mut Accent) {
    const STEP: f32 = 26.0;
    let count = Accent::ALL.len() as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(count * STEP, 28.0), egui::Sense::hover());
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
        let radius = 9.0;
        if *accent == choice {
            let ring = choice
                .rgb()
                .map_or(style::text_secondary(), |_| style::accent_color(choice));
            painter.circle_stroke(center, radius + 3.0, Stroke::new(2.0, ring));
        }
        match choice.rgb() {
            Some(_) => {
                painter.circle_filled(center, radius, style::accent_color(choice));
            }
            None => paint_multicolor(painter, center, radius),
        }
        painter.circle_stroke(
            center,
            radius - 0.5,
            Stroke::new(1.0, Color32::from_black_alpha(30)),
        );
    }
}

/// Pastille « Multicolore » : un disque en parts aux couleurs d'accentuation.
fn paint_multicolor(painter: &egui::Painter, center: egui::Pos2, radius: f32) {
    let colors: Vec<Color32> = Accent::ALL[1..8]
        .iter()
        .map(|accent| style::accent_color(*accent))
        .collect();
    let parts = colors.len();
    for (index, color) in colors.into_iter().enumerate() {
        let from = index as f32 / parts as f32 * std::f32::consts::TAU;
        let to = (index + 1) as f32 / parts as f32 * std::f32::consts::TAU;
        let mut points = vec![center];
        for step in 0..=6 {
            let angle = from + (to - from) * step as f32 / 6.0 - std::f32::consts::FRAC_PI_2;
            points.push(center + radius * egui::vec2(angle.cos(), angle.sin()));
        }
        painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
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
    fn icone_d_un_appareil_d_apres_son_nom() {
        assert_eq!(device_icon("AirPods Max"), "device-headphones");
        assert_eq!(device_icon("Souris MX Master"), "device-mouse");
        assert_eq!(device_icon("Magic Keyboard"), "settings-keyboard");
        assert_eq!(device_icon("Redmi Note 11 SE"), "device-phone");
        assert_eq!(device_icon("Manette"), "settings-bluetooth");
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

    #[test]
    fn teinte_de_la_colonne_tiree_du_fond() {
        let image = Image {
            width: 2,
            height: 2,
            rgba: vec![255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255],
        };
        let (top, bottom) = tint_of(&image);
        // Pastels : très clairs, mais encore rouge en haut et bleu en bas.
        assert!(top.r() > top.b() && bottom.b() > bottom.r());
        assert!(top.g() > 150 && bottom.g() > 150);
    }
}
