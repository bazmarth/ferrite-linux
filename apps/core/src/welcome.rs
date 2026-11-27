//! Bienvenue (`ferrite-apps bienvenue`) : l'assistant du premier démarrage, d'après la
//! maquette « Configurons FerriteOS ensemble ».
//!
//! Deux déroulés :
//! - **après l'installation** (sept étapes) : langue et région, réseau, compte,
//!   confidentialité, apparence, applications, fin ;
//! - **sur la clé live** (quatre étapes) : langue et région, réseau, apparence, puis une fin
//!   qui propose d'installer Ferrite ou de continuer l'essai. Le compte se crée pendant
//!   l'installation, et une application installée dans la session d'essai disparaîtrait à
//!   l'arrêt : ces étapes n'y ont pas de sens.
//!
//! Chaque choix agit tout de suite (réglages `~/.config/ferrite/reglages`, fuseau par
//! `timedatectl`, Wi-Fi par NetworkManager, applications par Flathub) : rien n'est simulé.
//! Le bureau ouvre l'assistant à l'ouverture de session tant que le réglage `bienvenue` ne
//! vaut pas `faite` (voir `shell/src/desktop/apps.rs`, `start_welcome`).

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align2, FontId, Frame, Margin, RichText, Sense, Stroke};
use ferrite_system::{
    config::{Accent, Appearance, Settings},
    network::{self, WifiNetwork},
    time,
};
use ferrite_ui::{
    tokens::typography,
    wallpaper::{self, Builtin, Source},
};

use crate::{
    installer::plan::{KEYBOARDS, Keyboard, TIMEZONES},
    kit,
    store::{drivers, flatpak},
    style::{self, IconStyle, Icons},
};

/// Réglage qui dit si l'assistant a été parcouru : `faite`, ou `a_faire` (écrit par
/// l'installation dans le dossier du nouveau compte).
pub const SETTING: &str = "bienvenue";

/// Largeur de la colonne des étapes.
const COLUMN: f32 = 320.0;
/// Largeur maximale du contenu d'une étape.
const CONTENT: f32 = 660.0;
/// Taille des vignettes de fond d'écran.
const THUMBNAIL: (u32, u32) = (192, 120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Region,
    Network,
    Account,
    Privacy,
    Look,
    Apps,
    Done,
}

impl Step {
    const INSTALLED: [Step; 7] = [
        Step::Region,
        Step::Network,
        Step::Account,
        Step::Privacy,
        Step::Look,
        Step::Apps,
        Step::Done,
    ];
    const LIVE: [Step; 4] = [Step::Region, Step::Network, Step::Look, Step::Done];

    /// Nom dans la colonne des étapes.
    fn name(self) -> &'static str {
        match self {
            Step::Region => "Langue et région",
            Step::Network => "Réseau",
            Step::Account => "Compte",
            Step::Privacy => "Confidentialité",
            Step::Look => "Apparence",
            Step::Apps => "Applications",
            Step::Done => "Terminé",
        }
    }

    fn optional(self) -> bool {
        matches!(self, Step::Network | Step::Apps)
    }

    /// Symbole au centre de l'anneau.
    fn glyph(self) -> &'static str {
        match self {
            Step::Region => "glyph-globe",
            Step::Network => "glyph-wifi",
            Step::Account => "glyph-account",
            Step::Privacy => "glyph-privacy",
            Step::Look => "glyph-appearance",
            Step::Apps => "glyph-app",
            Step::Done => "cc-check",
        }
    }
}

/// Application proposée à l'étape « Applications » (Flathub).
struct Suggestion {
    id: &'static str,
    name: &'static str,
    detail: &'static str,
    glyph: &'static str,
}

const SUGGESTIONS: &[Suggestion] = &[
    Suggestion {
        id: "org.mozilla.firefox",
        name: "Firefox",
        detail: "Navigateur web libre",
        glyph: "glyph-globe",
    },
    Suggestion {
        id: "org.libreoffice.LibreOffice",
        name: "LibreOffice",
        detail: "Traitement de texte, tableur, présentations",
        glyph: "glyph-doc",
    },
    Suggestion {
        id: "org.mozilla.Thunderbird",
        name: "Thunderbird",
        detail: "Courriel et agenda",
        glyph: "glyph-bell",
    },
    Suggestion {
        id: "org.videolan.VLC",
        name: "VLC",
        detail: "Lit toutes les vidéos et musiques",
        glyph: "glyph-play",
    },
    Suggestion {
        id: "org.gimp.GIMP",
        name: "GIMP",
        detail: "Retouche de photos",
        glyph: "glyph-brush",
    },
    Suggestion {
        id: "org.telegram.desktop",
        name: "Telegram",
        detail: "Messagerie",
        glyph: "glyph-share",
    },
];

/// Où en est une application proposée.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AppState {
    /// Liste des applications installées pas encore lue.
    Unknown,
    Available,
    Waiting,
    Installing(u32),
    Installed,
    Failed(String),
}

/// Nouvelles du fil qui lit ou installe les applications.
enum AppEvent {
    Present(Vec<String>),
    ListFailed,
    Started(usize),
    Progress(usize, u32),
    Done(usize),
    Failed(usize, String),
}

/// Compte de la session : nom complet, identifiant, nom de l'ordinateur.
#[derive(Debug, Default)]
struct AccountInfo {
    full_name: String,
    login: String,
    host: String,
}

impl AccountInfo {
    fn read() -> Self {
        let login = std::env::var("USER").unwrap_or_default();
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let full_name = passwd
            .lines()
            .map(|line| line.split(':').collect::<Vec<_>>())
            .find(|fields| fields.first() == Some(&login.as_str()))
            .and_then(|fields| {
                fields
                    .get(4)
                    .map(|gecos| gecos.split(',').next().unwrap_or("").to_owned())
            })
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| login.clone());
        let host = std::fs::read_to_string("/etc/hostname")
            .map(|text| text.trim().to_owned())
            .unwrap_or_default();
        Self {
            full_name,
            login,
            host,
        }
    }
}

pub struct WelcomeState {
    context: egui::Context,
    live: bool,
    steps: &'static [Step],
    index: usize,
    icons: Icons,
    /// Message en bas de l'écran (erreur d'enregistrement, fuseau refusé…).
    message: Option<String>,
    keyboard: Option<&'static Keyboard>,
    /// Champ pour essayer le clavier choisi.
    try_keys: String,
    timezone: String,
    zone_job: Option<Receiver<Result<String, String>>>,
    wifi: Option<Result<Vec<WifiNetwork>, String>>,
    wifi_job: Option<Receiver<Result<Vec<WifiNetwork>, String>>>,
    /// Réseau protégé choisi, dont on attend le mot de passe.
    chosen: Option<String>,
    password: String,
    /// Le champ du mot de passe reprend le clavier (réseau choisi, ou mot de passe refusé).
    password_focus: bool,
    connect_job: Option<Receiver<Result<String, String>>>,
    wifi_message: Option<Result<String, String>>,
    account: AccountInfo,
    thumbnails: Vec<(Builtin, egui::TextureHandle)>,
    apps: Vec<(bool, AppState)>,
    app_events: Receiver<AppEvent>,
    app_sender: Sender<AppEvent>,
    installing: bool,
}

impl WelcomeState {
    pub fn new(context: &egui::Context) -> Self {
        let live = drivers::is_live();
        let settings = Settings::load();
        let keyboard = keyboard_of(settings.keyboard_layout());
        let timezone = time::timezone().unwrap_or_else(|| TIMEZONES[0].1.to_owned());
        let (app_sender, app_events) = mpsc::channel();
        let mut state = Self {
            context: context.clone(),
            live,
            steps: if live { &Step::LIVE } else { &Step::INSTALLED },
            index: 0,
            icons: Icons::default(),
            message: None,
            keyboard,
            try_keys: String::new(),
            timezone,
            zone_job: None,
            wifi: None,
            wifi_job: None,
            chosen: None,
            password: String::new(),
            password_focus: false,
            connect_job: None,
            wifi_message: None,
            account: AccountInfo::read(),
            thumbnails: Vec::new(),
            apps: SUGGESTIONS.iter().map(|_| (false, AppState::Unknown)).collect(),
            app_events,
            app_sender,
            installing: false,
        };
        if !live {
            state.read_installed();
        }
        // Le Wi-Fi déjà connecté se voit dès la première étape et dans le résumé.
        state.scan();
        if let Ok(step) = std::env::var("FERRITE_BIENVENUE_DEMO") {
            state.demo(&step);
        }
        state
    }

    /// Captures : `FERRITE_BIENVENUE_DEMO=reseau|compte|confidentialite|apparence|
    /// applications|fin` ouvre directement cette étape.
    fn demo(&mut self, name: &str) {
        let step = match name {
            "reseau" => Step::Network,
            "compte" => Step::Account,
            "confidentialite" => Step::Privacy,
            "apparence" => Step::Look,
            "applications" => Step::Apps,
            "fin" => Step::Done,
            _ => Step::Region,
        };
        if let Some(index) = self.steps.iter().position(|candidate| *candidate == step) {
            self.go(index);
        }
    }

    fn step(&self) -> Step {
        self.steps[self.index]
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        self.poll();
        if ui.ctx().input(|input| input.viewport().close_requested()) {
            let _ = mark_done();
        }
        style::sidebar(
            ui,
            "bienvenue-etapes",
            COLUMN,
            style::sidebar_color(),
            true,
            |ui| {
                self.column(ui);
            },
        );
        style::inset_panel(ui, false, true, |ui| {
            let card = ui.max_rect();
            style::toolbar(ui, true, |_| {});
            self.progress_line(ui, card);
            self.footer(ui);
            egui::CentralPanel::no_frame().show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    let width = (ui.available_width() - 96.0).clamp(280.0, CONTENT);
                    ui.horizontal(|ui| {
                        ui.add_space(48.0);
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.add_space(8.0);
                            match self.step() {
                                Step::Region => self.region(ui),
                                Step::Network => self.network(ui),
                                Step::Account => self.account(ui),
                                Step::Privacy => privacy(ui),
                                Step::Look => self.look(ui),
                                Step::Apps => self.applications(ui),
                                Step::Done => self.done(ui),
                            }
                            ui.add_space(24.0);
                        });
                    });
                });
            });
        });
    }

    /// Résultats des fils (fuseau, Wi-Fi, applications).
    fn poll(&mut self) {
        let mut busy = false;
        if let Some(job) = &self.zone_job {
            busy = true;
            if let Ok(result) = job.try_recv() {
                self.zone_job = None;
                match result {
                    Ok(zone) => self.timezone = zone,
                    Err(error) => self.message = Some(format!("Fuseau horaire inchangé : {error}")),
                }
            }
        }
        if let Some(job) = &self.wifi_job {
            busy = true;
            if let Ok(result) = job.try_recv() {
                self.wifi_job = None;
                self.wifi = Some(result);
            }
        }
        if let Some(job) = &self.connect_job {
            busy = true;
            if let Ok(result) = job.try_recv() {
                self.connect_job = None;
                if result.is_ok() {
                    self.chosen = None;
                    self.password.clear();
                    self.scan();
                }
                // Mot de passe refusé : il reste dans le champ, à corriger.
                let result = result.map_err(|error| {
                    if network::wrong_password(&error) {
                        self.password_focus = true;
                        "Mot de passe incorrect : corrigez-le et réessayez.".to_owned()
                    } else {
                        error
                    }
                });
                self.wifi_message = Some(result);
            }
        }
        while let Ok(event) = self.app_events.try_recv() {
            match event {
                AppEvent::Present(ids) => {
                    for (suggestion, (_, state)) in SUGGESTIONS.iter().zip(&mut self.apps) {
                        if *state == AppState::Unknown {
                            *state = if ids.iter().any(|id| id == suggestion.id) {
                                AppState::Installed
                            } else {
                                AppState::Available
                            };
                        }
                    }
                }
                AppEvent::ListFailed => {
                    for (_, state) in &mut self.apps {
                        if *state == AppState::Unknown {
                            *state = AppState::Available;
                        }
                    }
                }
                AppEvent::Started(index) => self.apps[index].1 = AppState::Installing(0),
                AppEvent::Progress(index, percent) => self.apps[index].1 = AppState::Installing(percent),
                AppEvent::Done(index) => {
                    self.apps[index] = (false, AppState::Installed);
                }
                AppEvent::Failed(index, error) => self.apps[index].1 = AppState::Failed(error),
            }
        }
        self.installing = self
            .apps
            .iter()
            .any(|(_, state)| matches!(state, AppState::Waiting | AppState::Installing(_)));
        if busy || self.installing {
            self.context.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn go(&mut self, index: usize) {
        self.index = index.min(self.steps.len() - 1);
        self.message = None;
        if self.step() == Step::Network && self.wifi.is_none() && self.wifi_job.is_none() {
            self.scan();
        }
    }

    // ── Colonne des étapes ──────────────────────────────────────────────────────────

    fn column(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        let inner = full.shrink2(egui::vec2(12.0, 0.0));
        // Marque et numéro de l'étape.
        let brand_y = inner.top() + 6.0;
        let logo = egui::pos2(inner.left() + 8.0, brand_y);
        ui.painter()
            .circle_stroke(logo, 6.5, Stroke::new(2.5, style::accent()));
        ui.painter().text(
            egui::pos2(logo.x + 15.0, brand_y),
            Align2::LEFT_CENTER,
            "Ferrite",
            FontId::new(15.0, style::strong_family()),
            style::heading_text(),
        );
        ui.painter().text(
            egui::pos2(inner.right(), brand_y),
            Align2::RIGHT_CENTER,
            format!("Étape {} sur {}", self.index + 1, self.steps.len()),
            FontId::proportional(typography::SMALL),
            style::text_secondary(),
        );

        // Anneau, avec le symbole de l'étape.
        let center = egui::pos2(inner.center().x, brand_y + 128.0);
        self.ring(ui, center);

        // Liste des étapes.
        let mut y = center.y + 128.0;
        let mut clicked = None;
        for (index, step) in self.steps.iter().enumerate() {
            let rect =
                egui::Rect::from_min_size(egui::pos2(inner.left(), y), egui::vec2(inner.width(), 42.0));
            let response = ui
                .interact(rect, ui.scope_id().with(("etape", index)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                clicked = Some(index);
            }
            step_row(ui, rect, index, *step, index.cmp(&self.index), response.hovered());
            y += 46.0;
        }
        if let Some(index) = clicked {
            self.go(index);
        }

        // Accessibilité et clavier, en bas.
        let bottom = inner.bottom() - 26.0;
        let access = chip(
            ui,
            egui::pos2(inner.left(), bottom),
            &mut self.icons,
            "glyph-access",
            "Accessibilité",
        );
        if access.clicked() {
            open_settings("accessibilite");
        }
        let layout = Settings::load().keyboard_layout().to_owned();
        let short = layout.split('(').next().unwrap_or("").to_uppercase();
        let keys = chip(
            ui,
            egui::pos2(access.rect.right() + 8.0, bottom),
            &mut self.icons,
            "glyph-keyboard",
            &short,
        )
        .on_hover_text("Disposition du clavier");
        if keys.clicked() {
            self.go(0);
        }
    }

    /// Grand anneau d'accent, halo autour, symbole de l'étape au centre.
    fn ring(&mut self, ui: &egui::Ui, center: egui::Pos2) {
        let painter = ui.painter();
        let accent = style::accent();
        for (radius, alpha) in [(118.0, 0.025), (110.0, 0.04), (102.0, 0.06)] {
            painter.circle_filled(center, radius, accent.gamma_multiply(alpha));
        }
        painter.circle_stroke(center, 124.0, Stroke::new(1.0, accent.gamma_multiply(0.18)));
        // Anneau plein de 93 à 67 px de rayon, creusé par le fond de la colonne.
        painter.circle_filled(center, 93.0, accent);
        painter.circle_filled(center, 67.0, style::sidebar_color());
        let side = 40.0;
        let icon = egui::Rect::from_center_size(center, egui::Vec2::splat(side));
        self.icons.paint(
            ui,
            self.step().glyph(),
            icon,
            IconStyle::Tinted(style::primary_text()),
        );
    }

    /// Fine barre d'avancement en haut de la carte.
    fn progress_line(&self, ui: &egui::Ui, card: egui::Rect) {
        let track = egui::Rect::from_min_size(
            egui::pos2(card.left() + 10.0, card.top()),
            egui::vec2(card.width() - 20.0, 3.0),
        );
        let share = (self.index + 1) as f32 / self.steps.len() as f32;
        let done = egui::Rect::from_min_size(track.min, egui::vec2(track.width() * share, 3.0));
        let painter = ui.painter();
        painter.rect_filled(track, 1.5, style::tint(style::themed(14, 22)));
        painter.rect_filled(done, 1.5, style::accent());
    }

    // ── Pied : Retour et Suivant ────────────────────────────────────────────────────

    fn footer(&mut self, ui: &mut egui::Ui) {
        let last = self.index + 1 == self.steps.len();
        egui::Panel::bottom(ui.scope_id().with("bienvenue-pied"))
            .resizable(false)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(style::background())
                    .inner_margin(Margin::symmetric(28, 14)),
            )
            .show(ui, |ui| {
                let full = ui.max_rect().expand2(egui::vec2(28.0, 14.0));
                ui.painter().hline(
                    full.x_range(),
                    full.top() + 0.5,
                    Stroke::new(1.0, style::hairline()),
                );
                ui.horizontal(|ui| {
                    if self.index > 0
                        && kit::wide_button(ui, 110.0, "Retour", kit::Tone::Plain, true).clicked()
                    {
                        self.go(self.index - 1);
                    }
                    if let Some(message) = &self.message {
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(message)
                                .size(typography::SMALL)
                                .color(style::error()),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = match (last, self.live) {
                            (true, true) => "Continuer l'essai",
                            (true, false) => "Commencer",
                            _ => "Suivant  →",
                        };
                        if kit::wide_button(ui, 150.0, label, kit::Tone::Accent, true).clicked() {
                            if last {
                                self.finish(ui.ctx());
                            } else {
                                self.go(self.index + 1);
                            }
                        }
                    });
                });
            });
    }

    fn finish(&mut self, ctx: &egui::Context) {
        if let Err(error) = mark_done() {
            self.message = Some(format!("Enregistrement impossible : {error}"));
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    // ── 1. Langue et région ─────────────────────────────────────────────────────────

    fn region(&mut self, ui: &mut egui::Ui) {
        intro(
            ui,
            "Bienvenue",
            "Configurons Ferrite ensemble",
            "Commencez par la langue, le clavier et le fuseau horaire. Ils fixent aussi le format des dates et des heures.",
        );
        kit::heading(ui, "Langue");
        kit::card(ui, |ui| {
            let languages = [
                ("Français", "Langue de Ferrite", true),
                ("English", "Traduction à venir", false),
                ("Mooré", "Traduction à venir", false),
                ("Dioula", "Traduction à venir", false),
            ];
            for (index, (name, detail, available)) in languages.into_iter().enumerate() {
                if index > 0 {
                    kit::separator(ui);
                }
                radio_line(ui, name, Some(detail), available, available);
            }
        });

        kit::heading(ui, "Clavier");
        kit::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Disposition").size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let current = self.keyboard.map_or_else(
                        || Settings::load().keyboard_layout().to_owned(),
                        |keyboard| keyboard.name.to_owned(),
                    );
                    let mut chosen = None;
                    egui::ComboBox::from_id_salt("clavier")
                        .icon(style::combo_chevron)
                        .width(260.0)
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for keyboard in KEYBOARDS {
                                let on = self.keyboard == Some(keyboard);
                                if ui.selectable_label(on, keyboard.name).clicked() {
                                    chosen = Some(keyboard);
                                }
                            }
                        });
                    if let Some(keyboard) = chosen {
                        self.keyboard = Some(keyboard);
                        self.save(ui.ctx(), |settings| settings.set("clavier", &keyboard.setting()));
                    }
                });
            });
            kit::separator(ui);
            ui.add(
                egui::TextEdit::singleline(&mut self.try_keys)
                    .hint_text("Essayez le clavier ici : é, à, ç, @…")
                    .desired_width(f32::INFINITY),
            );
        });

        kit::heading(ui, "Fuseau horaire");
        kit::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Ville la plus proche").size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.zone_job.is_some() {
                        ui.add(egui::Spinner::new().size(16.0));
                    }
                    let current = TIMEZONES
                        .iter()
                        .find(|(_, zone)| *zone == self.timezone)
                        .map_or(self.timezone.as_str(), |(name, _)| name)
                        .to_owned();
                    let mut chosen = None;
                    ui.add_enabled_ui(self.zone_job.is_none(), |ui| {
                        egui::ComboBox::from_id_salt("fuseau")
                            .icon(style::combo_chevron)
                            .width(260.0)
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                for (name, zone) in TIMEZONES {
                                    if ui.selectable_label(*zone == self.timezone, *name).clicked() {
                                        chosen = Some(*zone);
                                    }
                                }
                            });
                    });
                    if let Some(zone) = chosen.filter(|zone| *zone != self.timezone) {
                        self.set_timezone(zone);
                    }
                });
            });
        });

        ui.add_space(14.0);
        ui.columns(2, |columns| {
            fact(&mut columns[0], "Fuseau horaire", &zone_label(&self.timezone));
            fact(&mut columns[1], "Aperçu des formats", &format_preview());
        });
    }

    fn set_timezone(&mut self, zone: &'static str) {
        self.message = None;
        self.zone_job = Some(spawn(&self.context, move || {
            time::set_timezone(zone).map(|()| zone.to_owned())
        }));
    }

    // ── 2. Réseau ───────────────────────────────────────────────────────────────────

    fn scan(&mut self) {
        self.wifi_job = Some(spawn(&self.context, network::wifi_networks));
    }

    fn network(&mut self, ui: &mut egui::Ui) {
        intro(
            ui,
            "Facultatif",
            "Se connecter à Internet",
            "Pour l'heure exacte, les mises à jour et les applications. Vous pourrez aussi le faire plus tard depuis la barre du haut.",
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Réseaux Wi-Fi")
                    .size(15.0)
                    .family(style::strong_family())
                    .color(style::heading_text()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let busy = self.wifi_job.is_some();
                if busy {
                    ui.add(egui::Spinner::new().size(16.0));
                } else if kit::button(ui, "Actualiser", false).clicked() {
                    self.scan();
                }
            });
        });
        ui.add_space(8.0);
        let networks = match &self.wifi {
            None => {
                kit::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(16.0));
                        ui.label(RichText::new("Recherche des réseaux…").size(14.0));
                    });
                });
                return;
            }
            Some(Err(_)) => {
                kit::card(ui, |ui| {
                    ui.label(RichText::new("Aucun Wi-Fi sur cet ordinateur").size(14.0));
                    kit::note(
                        ui,
                        "Branchez un câble réseau si vous en avez un : la connexion se fait seule.",
                    );
                });
                return;
            }
            Some(Ok(networks)) => networks.clone(),
        };
        if networks.is_empty() {
            kit::card(ui, |ui| {
                ui.label(RichText::new("Aucun réseau Wi-Fi à portée").size(14.0));
                kit::note(ui, "Rapprochez-vous de la box, puis actualisez.");
            });
            return;
        }
        let connecting = self.connect_job.is_some();
        let mut connect: Option<(String, Option<String>)> = None;
        kit::card(ui, |ui| {
            for (row, net) in networks.iter().take(8).enumerate() {
                if row > 0 {
                    kit::separator(ui);
                }
                let response = ui
                    .horizontal(|ui| {
                        ui.set_width(ui.available_width());
                        let (icon, _) = ui.allocate_exact_size(egui::Vec2::splat(20.0), Sense::hover());
                        let name = match net.level() {
                            0 | 1 => "status-wifi-1",
                            2 => "status-wifi-2",
                            _ => "status-wifi-3",
                        };
                        self.icons
                            .paint(ui, name, icon, IconStyle::Tinted(style::primary_text()));
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.add_space(6.0);
                            ui.label(RichText::new(&net.ssid).size(14.0));
                            let detail = if net.connected {
                                "Connecté".to_owned()
                            } else if net.secured {
                                format!("Protégé · {}", net.summary())
                            } else {
                                "Ouvert, sans mot de passe".to_owned()
                            };
                            ui.label(
                                RichText::new(detail)
                                    .size(typography::SMALL)
                                    .color(if net.connected {
                                        style::success()
                                    } else {
                                        style::text_secondary()
                                    }),
                            );
                            ui.add_space(6.0);
                        });
                    })
                    .response;
                let row_click = ui
                    .interact(response.rect, ui.scope_id().with(("wifi", row)), Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if row_click.clicked() && !net.connected && !connecting {
                    if net.secured {
                        self.chosen = Some(net.ssid.clone());
                        self.password.clear();
                        self.password_focus = true;
                        self.wifi_message = None;
                    } else {
                        connect = Some((net.ssid.clone(), None));
                    }
                }
                if self.chosen.as_deref() == Some(net.ssid.as_str()) {
                    ui.horizontal(|ui| {
                        let field = ui.add_enabled(
                            !connecting,
                            egui::TextEdit::singleline(&mut self.password)
                                .password(true)
                                .hint_text("Mot de passe du Wi-Fi")
                                .desired_width(ui.available_width() - 130.0),
                        );
                        if !connecting && std::mem::take(&mut self.password_focus) {
                            field.request_focus();
                            let mut state =
                                egui::text_edit::TextEditState::load(ui.ctx(), field.id).unwrap_or_default();
                            let end = egui::text::CCursor::new(self.password.chars().count());
                            state
                                .cursor
                                .set_char_range(Some(egui::text::CCursorRange::one(end)));
                            state.store(ui.ctx(), field.id);
                        }
                        let enter =
                            field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                        if connecting {
                            ui.add(egui::Spinner::new().size(16.0));
                        } else if (kit::button(ui, "Se connecter", true).clicked() || enter)
                            && !self.password.is_empty()
                        {
                            connect = Some((net.ssid.clone(), Some(self.password.clone())));
                        }
                    });
                    ui.add_space(6.0);
                }
            }
        });
        if let Some((ssid, password)) = connect {
            self.wifi_message = None;
            self.connect_job = Some(spawn(&self.context, move || {
                network::connect_wifi(&ssid, password.as_deref()).map(|()| ssid)
            }));
        }
        match &self.wifi_message {
            Some(Ok(ssid)) => {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(format!("Connecté à « {ssid} »."))
                        .size(14.0)
                        .color(style::success()),
                );
            }
            Some(Err(error)) => {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(format!("Connexion impossible : {error}"))
                        .size(14.0)
                        .color(style::error()),
                );
            }
            None => {}
        }
    }

    // ── 3. Compte ───────────────────────────────────────────────────────────────────

    fn account(&mut self, ui: &mut egui::Ui) {
        intro(
            ui,
            "Votre compte",
            &format!("Bonjour {}", first_name(&self.account.full_name)),
            "Votre compte a été créé pendant l'installation. Ajoutez une photo ou le déverrouillage par visage si vous le souhaitez.",
        );
        kit::card(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(56.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 28.0, style::accent());
                let initial = self
                    .account
                    .full_name
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_default();
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    initial,
                    FontId::new(24.0, style::strong_family()),
                    style::ink_on(style::accent()),
                );
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(&self.account.full_name)
                            .size(17.0)
                            .family(style::strong_family()),
                    );
                    kit::note(
                        ui,
                        &format!(
                            "Identifiant : {} · Ordinateur : {}",
                            self.account.login, self.account.host
                        ),
                    );
                });
            });
        });
        kit::heading(ui, "À compléter quand vous voulez");
        kit::card(ui, |ui| {
            if action_row(ui, "Photo et mot de passe", "Dans Paramètres › Comptes", "Ouvrir") {
                open_settings("comptes");
            }
            kit::separator(ui);
            if action_row(
                ui,
                "Déverrouillage par visage",
                "Avec une caméra infrarouge compatible",
                "Configurer",
            ) {
                open_settings("visage");
            }
        });
    }

    // ── 5. Apparence ────────────────────────────────────────────────────────────────

    fn look(&mut self, ui: &mut egui::Ui) {
        intro(
            ui,
            "Apparence",
            "À votre goût",
            "Le thème, la couleur d'accent et le fond d'écran changent tout de suite. Tout se modifie ensuite dans Paramètres › Apparence.",
        );
        let settings = Settings::load();
        kit::heading(ui, "Thème");
        kit::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Clair, sombre ou selon l'heure").size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let labels: Vec<&str> = Appearance::ALL.iter().map(|mode| mode.label()).collect();
                    let current = Appearance::ALL
                        .iter()
                        .position(|mode| *mode == settings.appearance())
                        .unwrap_or(0);
                    if let Some(index) = kit::segments(ui, &labels, current) {
                        let key = Appearance::ALL[index].key();
                        self.save(ui.ctx(), |settings| settings.set("apparence", key));
                    }
                });
            });
        });

        kit::heading(ui, "Couleur d'accent");
        kit::card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                for accent in Accent::ALL {
                    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(30.0), Sense::click());
                    let response = response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(accent.label());
                    let fill = style::accent_color(accent);
                    ui.painter().circle_filled(rect.center(), 12.0, fill);
                    if settings.accent() == accent {
                        ui.painter()
                            .circle_stroke(rect.center(), 14.5, Stroke::new(2.0, fill));
                        ui.painter()
                            .circle_filled(rect.center(), 4.0, style::ink_on(fill));
                    }
                    if response.clicked() {
                        self.save(ui.ctx(), |settings| settings.set("accent", accent.key()));
                    }
                }
            });
        });

        kit::heading(ui, "Fond d'écran");
        if self.thumbnails.is_empty() {
            self.thumbnails = Builtin::ALL
                .into_iter()
                .map(|builtin| {
                    let image = wallpaper::paint(builtin, THUMBNAIL.0, THUMBNAIL.1);
                    let pixels = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width as usize, image.height as usize],
                        &image.rgba,
                    );
                    let texture = ui.ctx().load_texture(
                        format!("bienvenue-fond:{}", builtin.key()),
                        pixels,
                        egui::TextureOptions::LINEAR,
                    );
                    (builtin, texture)
                })
                .collect();
        }
        let current = Source::parse(settings.wallpaper());
        let mut chosen = None;
        let gap = 10.0;
        let width = ((ui.available_width() - 3.0 * gap) / 4.0).floor();
        let height = (width * 0.625).round();
        for row in self.thumbnails.chunks(4) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (builtin, texture) in row {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(width, height + 20.0), Sense::click());
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    let image = egui::Rect::from_min_size(rect.min, egui::vec2(width, height));
                    egui::Image::new(texture).corner_radius(8).paint_at(ui, image);
                    let selected = current == Source::Builtin(*builtin);
                    if selected {
                        ui.painter().rect_stroke(
                            image.expand(2.0),
                            10.0,
                            Stroke::new(2.0, style::accent()),
                            egui::StrokeKind::Outside,
                        );
                    } else if response.hovered() {
                        ui.painter().rect_stroke(
                            image,
                            8.0,
                            Stroke::new(1.0, style::hairline()),
                            egui::StrokeKind::Outside,
                        );
                    }
                    ui.painter().text(
                        egui::pos2(image.center().x, image.bottom() + 11.0),
                        Align2::CENTER_CENTER,
                        builtin.label(),
                        FontId::proportional(typography::SMALL),
                        if selected {
                            style::accent_ink()
                        } else {
                            style::text_secondary()
                        },
                    );
                    if response.clicked() {
                        chosen = Some(*builtin);
                    }
                }
            });
            ui.add_space(gap);
        }
        if let Some(builtin) = chosen {
            self.save(ui.ctx(), |settings| {
                settings.set("fond_ecran", &Source::Builtin(builtin).key());
            });
        }
    }

    // ── 6. Applications ─────────────────────────────────────────────────────────────

    fn read_installed(&self) {
        let sender = self.app_sender.clone();
        let repaint = self.context.clone();
        std::thread::spawn(move || {
            let _ = sender.send(match flatpak::installed() {
                Ok(list) => AppEvent::Present(list.into_iter().map(|app| app.id).collect()),
                Err(_) => AppEvent::ListFailed,
            });
            repaint.request_repaint();
        });
    }

    fn applications(&mut self, ui: &mut egui::Ui) {
        intro(
            ui,
            "Facultatif",
            "Vos premières applications",
            "Choisissez celles qui vous servent : elles viennent de Flathub et s'installent pendant que vous continuez. La Logithèque en propose des milliers d'autres.",
        );
        let mut start = false;
        kit::card(ui, |ui| {
            for (index, suggestion) in SUGGESTIONS.iter().enumerate() {
                if index > 0 {
                    kit::separator(ui);
                }
                let (wanted, state) = &mut self.apps[index];
                ui.horizontal(|ui| {
                    ui.set_width(ui.available_width());
                    let (tile, _) = ui.allocate_exact_size(egui::Vec2::splat(34.0), Sense::hover());
                    ui.painter().rect_filled(tile, 9.0, style::accent_soft());
                    self.icons.paint(
                        ui,
                        suggestion.glyph,
                        tile.shrink(8.0),
                        IconStyle::Tinted(style::accent_ink()),
                    );
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.add_space(5.0);
                        ui.label(RichText::new(suggestion.name).size(14.0));
                        let (detail, color) = match state {
                            AppState::Failed(error) => (format!("Échec : {error}"), style::error()),
                            AppState::Installed => ("Installée".to_owned(), style::success()),
                            AppState::Waiting => ("En attente…".to_owned(), style::text_secondary()),
                            AppState::Installing(percent) => {
                                (format!("Installation… {percent} %"), style::text_secondary())
                            }
                            _ => (suggestion.detail.to_owned(), style::text_secondary()),
                        };
                        ui.label(RichText::new(detail).size(typography::SMALL).color(color));
                        ui.add_space(5.0);
                    });
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| match state {
                            AppState::Available | AppState::Failed(_) => {
                                kit::switch(ui, wanted);
                            }
                            AppState::Installing(_) | AppState::Waiting | AppState::Unknown => {
                                ui.add(egui::Spinner::new().size(16.0));
                            }
                            AppState::Installed => {}
                        },
                    );
                });
            }
        });
        let count = self
            .apps
            .iter()
            .filter(|(wanted, state)| *wanted && matches!(state, AppState::Available | AppState::Failed(_)))
            .count();
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let label = if count > 1 {
                format!("Installer les {count} applications")
            } else {
                "Installer".to_owned()
            };
            if kit::wide_button(
                ui,
                240.0,
                &label,
                kit::Tone::Accent,
                count > 0 && !self.installing,
            )
            .clicked()
            {
                start = true;
            }
            ui.add_space(10.0);
            kit::note(ui, "Il faut une connexion à Internet.");
        });
        if start {
            self.install_selected();
        }
    }

    fn install_selected(&mut self) {
        let chosen: Vec<usize> = self
            .apps
            .iter()
            .enumerate()
            .filter(|(_, (wanted, state))| {
                *wanted && matches!(state, AppState::Available | AppState::Failed(_))
            })
            .map(|(index, _)| index)
            .collect();
        for index in &chosen {
            self.apps[*index].1 = AppState::Waiting;
        }
        self.installing = true;
        let sender = self.app_sender.clone();
        let repaint = self.context.clone();
        std::thread::spawn(move || {
            if !flatpak::has_flathub()
                && let Err(error) = flatpak::add_flathub()
            {
                for index in chosen {
                    let _ = sender.send(AppEvent::Failed(index, error.clone()));
                }
                repaint.request_repaint();
                return;
            }
            for index in chosen {
                let _ = sender.send(AppEvent::Started(index));
                repaint.request_repaint();
                let result = flatpak::install(SUGGESTIONS[index].id, |percent| {
                    let _ = sender.send(AppEvent::Progress(index, percent));
                    repaint.request_repaint();
                });
                let _ = sender.send(match result {
                    Ok(()) => AppEvent::Done(index),
                    Err(error) => AppEvent::Failed(index, error),
                });
                repaint.request_repaint();
            }
        });
    }

    // ── Fin ─────────────────────────────────────────────────────────────────────────

    fn done(&mut self, ui: &mut egui::Ui) {
        if self.live {
            intro(
                ui,
                "C'est prêt",
                "Ferrite est à essayer",
                "Rien n'est écrit sur l'ordinateur pendant l'essai : vos réglages s'effacent à l'arrêt. Installez Ferrite pour les garder.",
            );
        } else {
            intro(
                ui,
                "C'est prêt",
                "Ferrite est à vous",
                "Tout se retrouve dans Paramètres. Bonne découverte !",
            );
        }
        let settings = Settings::load();
        kit::card(ui, |ui| {
            let keyboard = self.keyboard.map_or_else(
                || settings.keyboard_layout().to_owned(),
                |keyboard| keyboard.name.to_owned(),
            );
            summary_row(ui, "Clavier", &keyboard);
            kit::separator(ui);
            summary_row(ui, "Fuseau horaire", &zone_label(&self.timezone));
            kit::separator(ui);
            let connected = self
                .wifi
                .as_ref()
                .and_then(|wifi| wifi.as_ref().ok())
                .and_then(|list| list.iter().find(|net| net.connected))
                .map_or_else(|| "Pas de Wi-Fi choisi".to_owned(), |net| net.ssid.clone());
            summary_row(ui, "Wi-Fi", &connected);
            kit::separator(ui);
            let look = format!(
                "{} · accent {} · fond {}",
                settings.appearance().label(),
                settings.accent().label().to_lowercase(),
                match Source::parse(settings.wallpaper()) {
                    Source::Builtin(builtin) => builtin.label(),
                    Source::File(_) => "personnel".to_owned(),
                }
            );
            summary_row(ui, "Apparence", &look);
            if !self.live {
                kit::separator(ui);
                let installing = self
                    .apps
                    .iter()
                    .filter(|(_, state)| matches!(state, AppState::Waiting | AppState::Installing(_)))
                    .count();
                let text = if installing > 0 {
                    format!("{installing} en cours d'installation, elles continuent en arrière-plan")
                } else {
                    let names: Vec<&str> = SUGGESTIONS
                        .iter()
                        .zip(&self.apps)
                        .filter(|(_, (_, state))| *state == AppState::Installed)
                        .map(|(suggestion, _)| suggestion.name)
                        .collect();
                    if names.is_empty() {
                        "Aucune pour l'instant".to_owned()
                    } else {
                        names.join(", ")
                    }
                };
                summary_row(ui, "Applications", &text);
            }
        });
        if self.live {
            ui.add_space(16.0);
            kit::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Installer Ferrite sur cet ordinateur")
                                .size(15.0)
                                .family(style::strong_family()),
                        );
                        kit::note(ui, "À côté de Windows ou à sa place. Comptez 10 à 20 minutes.");
                        ui.add_space(4.0);
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::button(ui, "Installer Ferrite", true).clicked() {
                            launch(&["installer"]);
                            self.finish(ui.ctx());
                        }
                    });
                });
            });
        }
    }

    /// Change un réglage, l'applique à cette fenêtre et l'enregistre (le bureau le relit).
    fn save(&mut self, ctx: &egui::Context, change: impl FnOnce(&mut Settings)) {
        let mut settings = Settings::load();
        change(&mut settings);
        style::apply_preferences(ctx, &settings);
        if let Err(error) = settings.save() {
            self.message = Some(format!("Enregistrement impossible : {error}"));
        }
    }
}

// ── Pièces ──────────────────────────────────────────────────────────────────────────

/// Surtitre en accent, titre et phrase d'introduction d'une étape.
fn intro(ui: &mut egui::Ui, overline: &str, title: &str, lead: &str) {
    ui.add_space(18.0);
    let galley = style::overline(ui, overline, style::accent_ink());
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, style::accent_ink());
    ui.add_space(6.0);
    ui.label(
        RichText::new(title)
            .size(30.0)
            .family(style::strong_family())
            .color(style::heading_text()),
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new(lead)
            .size(typography::BODY)
            .color(style::text_secondary()),
    );
    ui.add_space(10.0);
}

/// Ligne d'une étape dans la colonne : pastille numérotée (cochée si l'étape est passée),
/// nom, « Facultatif ».
fn step_row(
    ui: &egui::Ui,
    rect: egui::Rect,
    index: usize,
    step: Step,
    place: std::cmp::Ordering,
    hovered: bool,
) {
    use std::cmp::Ordering;
    let painter = ui.painter();
    if place == Ordering::Equal {
        painter.rect_filled(rect, 10.0, style::accent_soft());
    } else if hovered {
        painter.rect_filled(rect, 10.0, style::surface_hover());
    }
    let badge = egui::pos2(rect.left() + 24.0, rect.center().y);
    match place {
        Ordering::Equal => {
            painter.circle_filled(badge, 12.0, style::accent());
            painter.text(
                badge,
                Align2::CENTER_CENTER,
                (index + 1).to_string(),
                FontId::new(12.0, style::strong_family()),
                style::ink_on(style::accent()),
            );
        }
        Ordering::Less => {
            painter.circle_filled(badge, 12.0, style::accent_soft());
            painter.text(
                badge,
                Align2::CENTER_CENTER,
                "✓",
                FontId::new(12.0, style::strong_family()),
                style::accent_ink(),
            );
        }
        Ordering::Greater => {
            painter.circle_filled(badge, 12.0, style::tint(style::themed(10, 18)));
            painter.text(
                badge,
                Align2::CENTER_CENTER,
                (index + 1).to_string(),
                FontId::proportional(12.0),
                style::text_secondary(),
            );
        }
    }
    let current = place == Ordering::Equal;
    painter.text(
        egui::pos2(badge.x + 24.0, rect.center().y),
        Align2::LEFT_CENTER,
        step.name(),
        if current {
            FontId::new(typography::LABEL + 1.0, style::strong_family())
        } else {
            FontId::proportional(typography::LABEL + 1.0)
        },
        if current {
            style::heading_text()
        } else {
            style::text_secondary()
        },
    );
    if step.optional() {
        painter.text(
            egui::pos2(rect.right() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            "Facultatif",
            FontId::proportional(typography::CAPTION),
            style::tertiary_text(),
        );
    }
}

/// Pastille arrondie du bas de la colonne (Accessibilité, clavier).
fn chip(
    ui: &mut egui::Ui,
    left_center: egui::Pos2,
    icons: &mut Icons,
    glyph: &'static str,
    text: &str,
) -> egui::Response {
    let font = FontId::proportional(typography::LABEL);
    let width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), style::primary_text())
        .size()
        .x
        + 46.0;
    let rect = egui::Rect::from_min_size(
        egui::pos2(left_center.x, left_center.y - 17.0),
        egui::vec2(width, 34.0),
    );
    let response = ui
        .interact(rect, ui.scope_id().with(("pastille", glyph)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if response.hovered() {
        style::surface_hover()
    } else {
        style::tint(style::themed(8, 14))
    };
    ui.painter().rect_filled(rect, 17.0, fill);
    let icon = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 21.0, rect.center().y),
        egui::Vec2::splat(15.0),
    );
    icons.paint(ui, glyph, icon, IconStyle::Tinted(style::primary_text()));
    ui.painter().text(
        egui::pos2(rect.left() + 35.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        style::primary_text(),
    );
    response
}

/// Ligne à bouton rond, cliquable sur toute sa largeur ; vrai si elle vient d'être choisie.
fn radio_line(ui: &mut egui::Ui, name: &str, detail: Option<&str>, on: bool, enabled: bool) -> bool {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                let ink = if enabled {
                    style::primary_text()
                } else {
                    style::tertiary_text()
                };
                ui.vertical(|ui| {
                    ui.add_space(5.0);
                    ui.label(RichText::new(name).size(14.0).color(ink));
                    if let Some(detail) = detail {
                        ui.label(
                            RichText::new(detail)
                                .size(typography::SMALL)
                                .color(style::text_secondary()),
                        );
                    }
                    ui.add_space(5.0);
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    kit::radio(ui, on && enabled)
                })
                .inner
            })
        })
        .inner;
    let row = ui.interact(
        response.response.rect,
        response.response.id.with("ligne"),
        Sense::click(),
    );
    enabled && (row.clicked() || response.inner.clicked())
}

/// Ligne « titre, explication » avec un bouton à droite ; vrai au clic.
fn action_row(ui: &mut egui::Ui, title: &str, detail: &str, button: &str) -> bool {
    ui.horizontal(|ui| {
        ui.set_width(ui.available_width());
        ui.vertical(|ui| {
            ui.add_space(5.0);
            ui.label(RichText::new(title).size(14.0));
            ui.label(
                RichText::new(detail)
                    .size(typography::SMALL)
                    .color(style::text_secondary()),
            );
            ui.add_space(5.0);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            kit::button(ui, button, false).clicked()
        })
        .inner
    })
    .inner
}

/// Ligne d'un interrupteur lié à un réglage oui/non.
fn switch_row(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    key: &str,
    default: bool,
    title: &str,
    detail: &str,
) -> bool {
    let mut on = settings.flag(key, default);
    let changed = ui
        .horizontal(|ui| {
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width() - 70.0);
                ui.add_space(5.0);
                ui.label(RichText::new(title).size(14.0));
                ui.label(
                    RichText::new(detail)
                        .size(typography::SMALL)
                        .color(style::text_secondary()),
                );
                ui.add_space(5.0);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                kit::switch(ui, &mut on)
            })
            .inner
        })
        .inner;
    if changed {
        settings.set_flag(key, on);
    }
    changed
}

/// Ligne du résumé final : libellé à gauche, valeur à droite.
fn summary_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.set_width(ui.available_width());
        ui.add_space(2.0);
        ui.label(RichText::new(label).size(14.0).color(style::text_secondary()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(14.0));
        });
    });
    ui.add_space(2.0);
}

/// Petite carte « libellé, valeur en gras » (fuseau, aperçu des formats).
fn fact(ui: &mut egui::Ui, label: &str, value: &str) {
    kit::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(label)
                .size(typography::SMALL)
                .color(style::text_secondary()),
        );
        ui.label(RichText::new(value).size(15.0).family(style::strong_family()));
    });
}

// ── 4. Confidentialité ──────────────────────────────────────────────────────────────

fn privacy(ui: &mut egui::Ui) {
    intro(
        ui,
        "Confidentialité",
        "Vos données restent chez vous",
        "Ferrite n'envoie aucune statistique d'utilisation et n'a pas de compte en ligne. Voici ce que vous pouvez régler.",
    );
    let mut settings = Settings::load();
    let mut changed = false;
    kit::card(ui, |ui| {
        changed |= switch_row(
            ui,
            &mut settings,
            "notifications_apercu",
            true,
            "Texte des notifications",
            "Montrer le contenu des messages dans les bulles de notification",
        );
        kit::separator(ui);
        changed |= switch_row(
            ui,
            &mut settings,
            "dock_recentes",
            true,
            "Applications récentes dans le Dock",
            "Garder dans le Dock les dernières applications ouvertes",
        );
    });
    if changed && let Err(error) = settings.save() {
        ui.label(
            RichText::new(format!("Enregistrement impossible : {error}"))
                .size(typography::SMALL)
                .color(style::error()),
        );
    }
    kit::heading(ui, "Ce qui passe par Internet");
    kit::card(ui, |ui| {
        summary_row(ui, "Météo du bureau", "la ville choisie, envoyée à Open-Meteo");
        kit::separator(ui);
        summary_row(ui, "Mises à jour", "les serveurs de Debian et de Flathub");
        kit::separator(ui);
        summary_row(ui, "Logithèque", "le catalogue de Flathub");
    });
    ui.add_space(8.0);
    kit::note(
        ui,
        "Rien d'autre ne quitte l'ordinateur sans que vous le demandiez. Le reste se règle dans Paramètres › Confidentialité.",
    );
}

// ── Aides ───────────────────────────────────────────────────────────────────────────

/// Note dans les réglages que l'assistant a été parcouru (ou fermé).
fn mark_done() -> std::io::Result<()> {
    let mut settings = Settings::load();
    if settings.get(SETTING) == Some("faite") {
        return Ok(());
    }
    settings.set(SETTING, "faite");
    settings.save()
}

/// Lance `ferrite-apps args…` sans l'attendre.
fn launch(args: &[&str]) {
    if let Ok(exe) = ferrite_system::command::current_exe() {
        let mut command = std::process::Command::new(exe);
        command.args(args);
        if let Err(error) = ferrite_system::command::spawn_detached(command) {
            tracing::warn!(%error, "lancement impossible");
        }
    }
}

fn open_settings(page: &str) {
    launch(&["settings", page]);
}

/// Travail dans un fil ; la fenêtre est redessinée quand il a fini.
fn spawn<T: Send + 'static>(ctx: &egui::Context, work: impl FnOnce() -> T + Send + 'static) -> Receiver<T> {
    let (sender, receiver) = mpsc::channel();
    let repaint = ctx.clone();
    std::thread::spawn(move || {
        let _ = sender.send(work());
        repaint.request_repaint();
    });
    receiver
}

/// Clavier de la liste qui correspond au réglage, s'il y en a un.
fn keyboard_of(value: &str) -> Option<&'static Keyboard> {
    KEYBOARDS.iter().find(|keyboard| keyboard.setting() == value)
}

/// « Ouagadougou (UTC+0) » : ville de la liste, sinon le nom du fuseau.
fn zone_label(zone: &str) -> String {
    let city = TIMEZONES
        .iter()
        .find(|(_, candidate)| *candidate == zone)
        .map_or_else(
            || zone.rsplit('/').next().unwrap_or(zone).replace('_', " "),
            |(name, _)| (*name).to_owned(),
        );
    let offset = time::local_offset_seconds();
    let hours = offset / 3600;
    let minutes = (offset.abs() % 3600) / 60;
    let sign = if offset < 0 { '−' } else { '+' };
    if minutes == 0 {
        format!("{city} (UTC{sign}{})", hours.abs())
    } else {
        format!("{city} (UTC{sign}{}:{minutes:02})", hours.abs())
    }
}

/// « 23/11/2026 · 14:05 · 1 250 000,50 » : date, heure et nombres comme Ferrite les écrit.
fn format_preview() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
        + time::local_offset_seconds();
    let (year, month, day) = crate::clock::civil_from_days(now.div_euclid(86_400));
    let minute = now.rem_euclid(86_400) / 60;
    format!(
        "{day:02}/{month:02}/{year} · {:02}:{:02} · 1\u{202f}250\u{202f}000,50",
        minute / 60,
        minute % 60
    )
}

/// Premier mot du nom complet (« Aïssata » pour « Aïssata Ouédraogo »).
fn first_name(full: &str) -> &str {
    full.split_whitespace().next().unwrap_or(full)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valeur_du_clavier() {
        let swiss = KEYBOARDS.iter().find(|keyboard| keyboard.layout == "ch").unwrap();
        assert_eq!(swiss.setting(), "ch(fr)");
        assert_eq!(keyboard_of("ch(fr)"), Some(swiss));
        assert_eq!(
            keyboard_of("fr").map(|keyboard| keyboard.name),
            Some("Français (AZERTY)")
        );
        assert_eq!(keyboard_of("de"), None);
    }

    #[test]
    fn deroules() {
        assert_eq!(Step::INSTALLED.len(), 7);
        assert_eq!(Step::LIVE.last(), Some(&Step::Done));
        assert!(!Step::LIVE.contains(&Step::Account));
        assert!(!Step::LIVE.contains(&Step::Apps));
    }

    #[test]
    fn prenom() {
        assert_eq!(first_name("Aïssata Ouédraogo"), "Aïssata");
        assert_eq!(first_name("ferrite"), "ferrite");
    }
}
