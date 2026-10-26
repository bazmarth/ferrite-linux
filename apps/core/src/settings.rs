//! Paramètres : réseau, Bluetooth, son, écran, clavier, date et heure, à propos.
//!
//! Toutes les commandes système passent par `ferrite-system` dans des fils d'exécution
//! à part : l'interface ne se fige jamais, même si un service ne répond pas.

use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

/// Durée pendant laquelle un curseur réglé à la main n'est pas remplacé par une relecture.
const HOLD: Duration = Duration::from_secs(2);

use eframe::egui::{self, Frame, Margin, RichText};
use ferrite_system::{
    Snapshot,
    about::{self, About},
    audio::{self, Sink},
    bluetooth,
    config::{DockHiding, DockSize, KEYBOARD_LAYOUTS, Settings},
    display,
    network::{self, WifiNetwork},
    time,
};

use crate::style::{self, Icons};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Network,
    Bluetooth,
    Sound,
    Display,
    Desktop,
    Keyboard,
    DateTime,
    About,
}

impl Page {
    const ALL: [Page; 8] = [
        Page::Network,
        Page::Bluetooth,
        Page::Sound,
        Page::Display,
        Page::Desktop,
        Page::Keyboard,
        Page::DateTime,
        Page::About,
    ];

    fn title(self) -> &'static str {
        match self {
            Page::Network => "Wi-Fi et réseau",
            Page::Bluetooth => "Bluetooth",
            Page::Sound => "Son",
            Page::Display => "Écran",
            Page::Desktop => "Bureau et Dock",
            Page::Keyboard => "Clavier",
            Page::DateTime => "Date et heure",
            Page::About => "À propos",
        }
    }

    /// Couleur de la pastille de la rubrique.
    fn color(self) -> egui::Color32 {
        let hex: u32 = match self {
            Page::Network => 0x3689E6,
            Page::Bluetooth => 0x366FD9,
            Page::Sound => 0xE54B6B,
            Page::Display => 0x489D5A,
            Page::Desktop => 0xE08A2C,
            Page::Keyboard => 0x8E939E,
            Page::DateTime => 0x9B59D6,
            Page::About => 0x8E939E,
        };
        let [_, r, g, b] = hex.to_be_bytes();
        egui::Color32::from_rgb(r, g, b)
    }

    fn icon(self) -> &'static str {
        match self {
            Page::Network => "status-wifi-4",
            Page::Bluetooth => "status-bluetooth-on",
            Page::Sound => "status-volume-4",
            Page::Display => "status-display",
            Page::Desktop => "app-files",
            Page::Keyboard => "status-keyboard",
            Page::DateTime => "app-calendar",
            Page::About => "app-settings",
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
    bluetooth_devices: Option<Vec<bluetooth::Device>>,
    sinks: Vec<Sink>,
    keyboard: String,
    keyboard_sample: String,
    /// Dernier mouvement d'un curseur (volume, luminosité).
    touched: Option<Instant>,
    timezone: Option<String>,
    ntp: Option<bool>,
    timezones: Vec<String>,
    timezone_filter: String,
    about: Option<About>,
    loaded: Vec<Page>,
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
            page: Page::Network,
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
            bluetooth_devices: None,
            sinks: Vec::new(),
            keyboard: Settings::load().keyboard_layout().to_owned(),
            keyboard_sample: String::new(),
            touched: None,
            timezone: None,
            ntp: None,
            timezones: Vec::new(),
            timezone_filter: String::new(),
            about: None,
            loaded: Vec::new(),
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

    fn receive(&mut self) {
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
            Page::Display | Page::Desktop | Page::Keyboard => {}
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
        self.receive();
        self.load(&ctx, self.page);

        let running = self.running > 0;
        let message = self.message.clone();
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
                    ui.colored_label(style::ERROR, error);
                }
                _ => {}
            }
        });

        egui::Panel::left("rubriques")
            .resizable(false)
            .exact_size(210.0)
            .frame(
                Frame::new()
                    .fill(style::SIDEBAR)
                    .inner_margin(Margin::symmetric(8, 12)),
            )
            .show(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::no_frame()
            .frame(
                Frame::new()
                    .fill(style::BACKGROUND)
                    .inner_margin(Margin::symmetric(28, 22)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.heading(self.page.title());
                    ui.add_space(14.0);
                    match self.page {
                        Page::Network => self.network_page(ui, &ctx),
                        Page::Bluetooth => self.bluetooth_page(ui, &ctx),
                        Page::Sound => self.sound_page(ui, &ctx),
                        Page::Display => self.display_page(ui, &ctx),
                        Page::Desktop => self.desktop_page(ui),
                        Page::Keyboard => self.keyboard_page(ui),
                        Page::DateTime => self.datetime_page(ui, &ctx),
                        Page::About => self.about_page(ui),
                    }
                });
            });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        for page in Page::ALL {
            let response = style::sidebar_item(
                ui,
                &mut self.icons,
                page.icon(),
                style::IconStyle::Tile(page.color()),
                page.title(),
                self.page == page,
            );
            if response.clicked() {
                self.page = page;
                self.message = None;
            }
        }
    }

    // ---- Wi-Fi et réseau -------------------------------------------------------------

    fn network_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let wifi = self.snapshot.and_then(|snapshot| snapshot.wifi);
        group(ui, |ui| {
            row(ui, "Wi-Fi", wifi_status(wifi), |ui| {
                let mut on = wifi == Some(true);
                if style::switch(ui, &mut on, wifi.is_some()).changed() {
                    let message = if on { "Wi-Fi activé" } else { "Wi-Fi désactivé" };
                    self.act(ctx, message, move || network::set_wifi(on));
                }
            });
        });

        // Connexions actives (filaire comprise).
        let connected: Vec<_> = self
            .network_devices
            .iter()
            .filter(|device| device.state.starts_with("connected"))
            .cloned()
            .collect();
        if !connected.is_empty() {
            section(ui, "Connexions actives");
            group(ui, |ui| {
                for device in &connected {
                    let kind = match device.kind.as_str() {
                        "wifi" => "Wi-Fi",
                        "ethernet" => "Filaire",
                        other => other,
                    };
                    row(
                        ui,
                        &device.connection,
                        &format!("{kind} · {}", device.name),
                        |_| {},
                    );
                }
            });
        }

        if wifi != Some(true) {
            return;
        }
        ui.horizontal(|ui| {
            section(ui, "Réseaux disponibles");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Rechercher").clicked() {
                    self.refresh_networks(ctx);
                }
            });
        });
        let Some(networks) = self.networks.clone() else {
            ui.label(RichText::new("Recherche des réseaux…").color(style::TEXT_SECONDARY));
            return;
        };
        if networks.is_empty() {
            ui.label(RichText::new("Aucun réseau à portée.").color(style::TEXT_SECONDARY));
            return;
        }
        group(ui, |ui| {
            for network in networks {
                let detail = format!(
                    "Signal {} %{}",
                    network.signal,
                    if network.secured { " · protégé" } else { "" }
                );
                let title = if network.connected {
                    format!("{}  ✓", network.ssid)
                } else {
                    network.ssid.clone()
                };
                row(ui, &title, &detail, |ui| {
                    if network.connected {
                        if ui.button("Déconnecter").clicked() {
                            let ssid = network.ssid.clone();
                            self.act(ctx, "Déconnecté", move || network::disconnect_wifi(&ssid));
                            self.loaded.retain(|page| *page != Page::Network);
                        }
                    } else if ui.button("Se connecter").clicked() {
                        if network.secured {
                            self.password_for = Some(network.ssid.clone());
                            self.password.clear();
                        } else {
                            let ssid = network.ssid.clone();
                            self.act(ctx, "Connecté", move || network::connect_wifi(&ssid, None));
                            self.loaded.retain(|page| *page != Page::Network);
                        }
                    }
                });
                if self.password_for.as_deref() == Some(network.ssid.as_str()) {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut self.password)
                                .password(true)
                                .hint_text("Mot de passe du réseau")
                                .desired_width(240.0),
                        );
                        let submit =
                            field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                        if style::suggested_button(ui, "Connecter").clicked() || submit {
                            let ssid = network.ssid.clone();
                            let password = std::mem::take(&mut self.password);
                            self.password_for = None;
                            self.act(ctx, "Connecté", move || {
                                network::connect_wifi(&ssid, Some(&password))
                            });
                            self.loaded.retain(|page| *page != Page::Network);
                        }
                        if ui.button("Annuler").clicked() {
                            self.password_for = None;
                        }
                    });
                }
            }
        });
    }

    // ---- Bluetooth --------------------------------------------------------------------

    fn bluetooth_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let powered = self.snapshot.and_then(|snapshot| snapshot.bluetooth);
        group(ui, |ui| {
            let status = match powered {
                Some(true) => "Visible des appareils proches",
                Some(false) => "Désactivé",
                None => "Aucun adaptateur Bluetooth détecté",
            };
            row(ui, "Bluetooth", status, |ui| {
                let mut on = powered == Some(true);
                if style::switch(ui, &mut on, powered.is_some()).changed() {
                    let message = if on {
                        "Bluetooth activé"
                    } else {
                        "Bluetooth désactivé"
                    };
                    self.act(ctx, message, move || bluetooth::set_powered(on));
                }
            });
        });
        if powered != Some(true) {
            return;
        }
        ui.horizontal(|ui| {
            section(ui, "Appareils");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Rechercher").clicked() {
                    self.spawn(ctx, || {
                        let scan = bluetooth::scan();
                        let mut updates = vec![Update::BluetoothDevices(bluetooth::devices())];
                        if let Err(error) = scan {
                            updates.push(Update::Done(Err(error)));
                        }
                        updates
                    });
                }
            });
        });
        let Some(devices) = self.bluetooth_devices.clone() else {
            ui.label(RichText::new("Lecture des appareils…").color(style::TEXT_SECONDARY));
            return;
        };
        if devices.is_empty() {
            ui.label(
                RichText::new("Aucun appareil. Mets l'appareil en mode appairage, puis Rechercher.")
                    .color(style::TEXT_SECONDARY),
            );
            return;
        }
        group(ui, |ui| {
            for device in devices {
                let status = match (device.connected, device.paired) {
                    (true, _) => "Connecté",
                    (false, true) => "Appairé",
                    _ => "Disponible",
                };
                row(ui, &device.name, status, |ui| {
                    let address = device.address.clone();
                    if device.connected {
                        if ui.button("Déconnecter").clicked() {
                            self.bluetooth_action(ctx, "Déconnecté", move || {
                                bluetooth::disconnect(&address)
                            });
                        }
                    } else {
                        if device.paired && ui.button("Oublier").clicked() {
                            let address = address.clone();
                            self.bluetooth_action(ctx, "Appareil oublié", move || {
                                bluetooth::forget(&address)
                            });
                        }
                        if ui.button("Connecter").clicked() {
                            self.bluetooth_action(ctx, "Connecté", move || bluetooth::connect(&address));
                        }
                    }
                });
            }
        });
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
        let volume = self.snapshot.and_then(|snapshot| snapshot.volume);
        let Some(current) = volume else {
            ui.label(
                RichText::new("Aucune sortie audio : PipeWire n'est pas démarré.")
                    .color(style::TEXT_SECONDARY),
            );
            return;
        };
        group(ui, |ui| {
            row(ui, "Volume", &format!("{} %", self.volume), |ui| {
                ui.spacing_mut().slider_width = 240.0;
                let response = ui.add(egui::Slider::new(&mut self.volume, 0..=100).show_value(false));
                if response.changed() {
                    self.touched = Some(Instant::now());
                }
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    let level = self.volume;
                    self.act(ctx, "", move || audio::set_volume(level));
                }
            });
            row(ui, "Muet", "Coupe le son de la sortie", |ui| {
                let mut muted = current.muted;
                if style::switch(ui, &mut muted, true).changed() {
                    self.act(ctx, "", move || audio::set_muted(muted));
                }
            });
        });
        if self.sinks.len() > 1 {
            section(ui, "Sortie");
            group(ui, |ui| {
                for sink in self.sinks.clone() {
                    row(ui, &sink.name, if sink.default { "Utilisée" } else { "" }, |ui| {
                        if !sink.default && ui.button("Utiliser").clicked() {
                            self.spawn(ctx, move || {
                                let result =
                                    audio::set_default_sink(sink.id).map(|()| "Sortie changée".to_owned());
                                vec![Update::Done(result), Update::Sinks(audio::sinks())]
                            });
                        }
                    });
                }
            });
        }
    }

    // ---- Écran ------------------------------------------------------------------------

    fn display_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let available = self.snapshot.and_then(|snapshot| snapshot.brightness).is_some();
        group(ui, |ui| {
            if available {
                row(ui, "Luminosité", &format!("{} %", self.brightness), |ui| {
                    ui.spacing_mut().slider_width = 240.0;
                    let response = ui.add(egui::Slider::new(&mut self.brightness, 5..=100).show_value(false));
                    if response.changed() {
                        self.touched = Some(Instant::now());
                    }
                    if response.drag_stopped() || (response.changed() && !response.dragged()) {
                        let level = self.brightness;
                        self.act(ctx, "", move || display::set_brightness(level));
                    }
                });
            } else {
                row(ui, "Luminosité", "Non réglable sur cet écran", |_| {});
            }
            let scale = ui.ctx().pixels_per_point();
            row(ui, "Échelle", &format!("{:.0} %", scale * 100.0), |_| {});
        });
    }

    // ---- Bureau et Dock ---------------------------------------------------------------

    fn desktop_page(&mut self, ui: &mut egui::Ui) {
        let mut settings = Settings::load();
        let (size, hiding) = (settings.dock_size(), settings.dock_hiding());
        let (mut new_size, mut new_hiding) = (size, hiding);
        section(ui, "Dock");
        group(ui, |ui| {
            row(ui, "Taille des icônes", "", |ui| {
                for choice in DockSize::ALL.into_iter().rev() {
                    ui.selectable_value(&mut new_size, choice, choice.label());
                }
            });
            row(
                ui,
                "Masquer le Dock",
                "Il revient quand la souris touche le bas de l'écran, ou avec la touche Super",
                |_| {},
            );
            for choice in DockHiding::ALL {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.radio_value(&mut new_hiding, choice, choice.label());
                });
            }
            ui.add_space(6.0);
        });
        section(ui, "Plein écran");
        group(ui, |ui| {
            row(
                ui,
                "Barre du haut et Dock masqués",
                "Ils reviennent quand la souris touche le bord de l'écran",
                |ui| {
                    ui.label(RichText::new("Automatique").color(style::TEXT_SECONDARY));
                },
            );
        });
        if (new_size, new_hiding) != (size, hiding) {
            settings.set("dock_taille", new_size.key());
            settings.set("dock_masquage", new_hiding.key());
            self.message = Some(
                settings
                    .save()
                    .map(|()| "Réglage du Dock appliqué".to_owned())
                    .map_err(|error| format!("Enregistrement impossible : {error}")),
            );
        }
    }

    // ---- Clavier ----------------------------------------------------------------------

    fn keyboard_page(&mut self, ui: &mut egui::Ui) {
        group(ui, |ui| {
            row(
                ui,
                "Disposition",
                "S'applique aussitôt à toutes les applications",
                |ui| {
                    let current = KEYBOARD_LAYOUTS
                        .iter()
                        .find(|(code, _)| *code == self.keyboard)
                        .map_or(self.keyboard.as_str(), |(_, name)| name)
                        .to_owned();
                    let mut chosen = self.keyboard.clone();
                    egui::ComboBox::from_id_salt("clavier")
                        .selected_text(current)
                        .width(250.0)
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
                },
            );
        });
        ui.add_space(10.0);
        ui.label("Essai :");
        ui.add(
            egui::TextEdit::singleline(&mut self.keyboard_sample)
                .hint_text("Tape ici pour vérifier la disposition")
                .desired_width(360.0),
        );
    }

    // ---- Date et heure ----------------------------------------------------------------

    fn datetime_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        group(ui, |ui| {
            row(
                ui,
                "Fuseau horaire",
                self.timezone.as_deref().unwrap_or("Inconnu"),
                |_| {},
            );
            row(ui, "Mise à l'heure automatique", "Par le réseau (NTP)", |ui| {
                let mut on = self.ntp == Some(true);
                if style::switch(ui, &mut on, self.ntp.is_some()).changed() {
                    self.ntp = Some(on);
                    self.spawn(ctx, move || {
                        let result = time::set_ntp(on).map(|()| "Réglage enregistré".to_owned());
                        vec![
                            Update::Done(result),
                            Update::Time(time::timezone(), time::ntp_enabled()),
                        ]
                    });
                }
            });
        });
        section(ui, "Changer de fuseau");
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
        group(ui, |ui| {
            for zone in matches {
                row(ui, &zone.replace('_', " "), "", |ui| {
                    if ui.button("Choisir").clicked() {
                        let chosen = zone.clone();
                        self.spawn(ctx, move || {
                            let result = time::set_timezone(&chosen).map(|()| format!("Fuseau : {chosen}"));
                            vec![
                                Update::Done(result),
                                Update::Time(time::timezone(), time::ntp_enabled()),
                            ]
                        });
                    }
                });
            }
        });
    }

    // ---- À propos ---------------------------------------------------------------------

    fn about_page(&mut self, ui: &mut egui::Ui) {
        let Some(about) = &self.about else {
            ui.label(RichText::new("Lecture des informations…").color(style::TEXT_SECONDARY));
            return;
        };
        ui.label(RichText::new(&about.os).size(15.0).family(style::strong_family()));
        ui.add_space(8.0);
        let gib = |bytes: u64| format!("{:.1} Go", bytes as f64 / 1_073_741_824.0).replace('.', ",");
        let lines = [
            ("Nom de l'ordinateur", about.hostname.clone()),
            (
                "Processeur",
                format!("{} ({} cœurs)", about.processor, about.cores),
            ),
            ("Mémoire", gib(about.memory_kib * 1024)),
            (
                "Disque",
                format!("{} libres sur {}", gib(about.disk_free), gib(about.disk_total)),
            ),
            ("Noyau Linux", about.kernel.clone()),
            ("Allumé depuis", format_uptime(about.uptime_seconds)),
        ];
        group(ui, |ui| {
            for (title, value) in lines {
                row(ui, title, "", |ui| {
                    ui.label(RichText::new(value).color(style::TEXT_SECONDARY));
                });
            }
        });
    }
}

fn wifi_status(wifi: Option<bool>) -> &'static str {
    match wifi {
        Some(true) => "Activé",
        Some(false) => "Désactivé",
        None => "Aucune carte Wi-Fi détectée",
    }
}

fn format_uptime(seconds: u64) -> String {
    let (hours, minutes) = (seconds / 3600, (seconds / 60) % 60);
    match hours {
        0 => format!("{minutes} min"),
        _ => format!("{hours} h {minutes:02}"),
    }
}

/// Titre de section, au-dessus d'un groupe.
fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(
        RichText::new(title)
            .family(style::strong_family())
            .color(style::TEXT_SECONDARY),
    );
    ui.add_space(4.0);
}

/// Groupe de lignes sur une carte blanche bordée.
fn group(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(egui::Color32::WHITE)
        .stroke(egui::Stroke::new(1.0, style::SEPARATOR))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(14, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// Ligne de réglage : titre et détail à gauche, commande à droite.
fn row(ui: &mut egui::Ui, title: &str, detail: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(46.0);
        ui.vertical(|ui| {
            ui.add_space(6.0);
            ui.label(title);
            if !detail.is_empty() {
                ui.label(RichText::new(detail).small().color(style::TEXT_SECONDARY));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duree_d_allumage() {
        assert_eq!(format_uptime(59 * 60), "59 min");
        assert_eq!(format_uptime(2 * 3600 + 5 * 60), "2 h 05");
    }
}
