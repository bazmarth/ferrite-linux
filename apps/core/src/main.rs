//! Applications de base Ferrite : Fichiers, TextEdit, Calculatrice, Paramètres et
//! Moniteur d'activité.

mod calculator;
mod editor;
mod files;
mod markdown;
mod monitor;
mod settings;
mod single;
mod style;

use std::{
    env,
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use eframe::egui;

use crate::{
    calculator::CalculatorState, editor::EditorState, files::FilesState, monitor::MonitorState,
    settings::SettingsState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppKind {
    Files,
    TextEdit,
    Calculator,
    Settings,
    Monitor,
}

impl AppKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "files" => Some(Self::Files),
            "textedit" => Some(Self::TextEdit),
            "calculator" => Some(Self::Calculator),
            "settings" => Some(Self::Settings),
            "monitor" => Some(Self::Monitor),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Files => "Fichiers",
            Self::TextEdit => "TextEdit",
            Self::Calculator => "Calculatrice",
            Self::Settings => "Paramètres",
            Self::Monitor => "Moniteur d'activité",
        }
    }

    /// Identifiant Wayland, reconnu par le Dock de ferrite-shell.
    fn app_id(self) -> &'static str {
        match self {
            Self::Files => "ferrite-files",
            Self::TextEdit => "ferrite-textedit",
            Self::Calculator => "ferrite-calculator",
            Self::Settings => "ferrite-settings",
            Self::Monitor => "ferrite-monitor",
        }
    }

    /// Taille initiale et taille minimale de la fenêtre.
    fn sizes(self) -> ([f32; 2], [f32; 2]) {
        match self {
            Self::Files => ([860.0, 520.0], [560.0, 340.0]),
            Self::TextEdit => ([1180.0, 760.0], [420.0, 280.0]),
            Self::Calculator => ([320.0, 470.0], [280.0, 420.0]),
            Self::Settings => ([880.0, 580.0], [680.0, 440.0]),
            Self::Monitor => ([920.0, 620.0], [680.0, 460.0]),
        }
    }
}

fn main() -> eframe::Result {
    let mut args = env::args().skip(1);
    let Some(kind) = args.next().and_then(|arg| AppKind::parse(&arg)) else {
        eprintln!(
            "Utilisation : ferrite-apps <files|textedit|calculator|settings|monitor> [dossier ou fichier]"
        );
        return Ok(());
    };
    let initial_file = args.next().map(PathBuf::from);
    // Une seule fenêtre par application, sauf celles choisies dans Paramètres : un
    // deuxième lancement confie son fichier à la fenêtre déjà ouverte.
    let multiple = ferrite_system::config::Settings::load()
        .multi_window_apps()
        .iter()
        .any(|id| id == kind.app_id());
    let requests = if multiple {
        None
    } else {
        match single::claim(kind.app_id(), initial_file.as_deref()) {
            single::Start::Primary(requests) => Some(requests),
            single::Start::Forwarded => return Ok(()),
        }
    };
    let (size, min_size) = kind.sizes();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_app_id(kind.app_id())
            // Les applications Ferrite dessinent leur haut (colonne, barre d'outils et
            // boutons) ; ferrite-shell garde coins arrondis, ombre et bords.
            .with_decorations(false)
            .with_inner_size(size)
            .with_min_inner_size(min_size),
        ..Default::default()
    };
    eframe::run_native(
        kind.title(),
        options,
        Box::new(move |context| Ok(Box::new(FerriteApp::new(context, kind, initial_file, requests)))),
    )
}

struct FerriteApp {
    kind: AppKind,
    files: Option<FilesState>,
    editor: EditorState,
    calculator: CalculatorState,
    settings: Option<SettingsState>,
    monitor: Option<MonitorState>,
    /// Lancements suivants de la même application (instance unique).
    requests: Option<single::Requests>,
    settings_stamp: Option<SystemTime>,
    settings_checked_at: Instant,
}

impl FerriteApp {
    fn new(
        context: &eframe::CreationContext<'_>,
        kind: AppKind,
        initial_file: Option<PathBuf>,
        requests: Option<single::Requests>,
    ) -> Self {
        style::apply(&context.egui_ctx);
        if let Some(requests) = &requests
            && let Ok(mut waker) = requests.context.lock()
        {
            *waker = Some(context.egui_ctx.clone());
        }
        let mut editor = EditorState::default();
        let mut files = (kind == AppKind::Files).then(FilesState::default);
        if let Some(path) = initial_file {
            match &mut files {
                Some(files) => files.open_at(path),
                None => editor.open(path),
            }
        }
        Self {
            kind,
            files,
            editor,
            calculator: CalculatorState::default(),
            settings: (kind == AppKind::Settings).then(SettingsState::default),
            monitor: (kind == AppKind::Monitor).then(MonitorState::default),
            requests,
            settings_stamp: ferrite_system::config::Settings::modified(),
            settings_checked_at: Instant::now(),
        }
    }

    fn refresh_preferences(&mut self, context: &egui::Context) {
        if self.settings_checked_at.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.settings_checked_at = Instant::now();
        let stamp = ferrite_system::config::Settings::modified();
        let settings = ferrite_system::config::Settings::load();
        if stamp != self.settings_stamp || settings.appearance().is_dark() != style::theme_is_dark() {
            self.settings_stamp = stamp;
            style::apply_preferences(context, &settings);
        }
    }

    /// Un autre lancement de l'application : son fichier s'ouvre ici, et la fenêtre
    /// revient au premier plan (le compositeur la sort du Dock si elle était réduite).
    fn handle_requests(&mut self, ctx: &egui::Context) {
        let Some(requests) = &self.requests else { return };
        let received: Vec<Option<PathBuf>> = requests.receiver.try_iter().collect();
        if received.is_empty() {
            return;
        }
        for file in received.into_iter().flatten() {
            match &mut self.files {
                Some(files) => files.open_at(file),
                None => self.editor.open(file),
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

impl eframe::App for FerriteApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.refresh_preferences(ui.ctx());
        self.handle_requests(&ui.ctx().clone());
        // F11 : plein écran, comme dans les navigateurs.
        if ui.input(|input| input.key_pressed(egui::Key::F11)) {
            let fullscreen = ui.input(|input| input.viewport().fullscreen.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
        }
        // Le client est rendu dans une surface Wayland translucide : peindre son
        // fond explicitement évite que le bureau transparaisse.
        ui.painter()
            .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, style::background());
        match self.kind {
            AppKind::Files => {
                if let Some(files) = &mut self.files {
                    files.update(ui);
                }
            }
            AppKind::TextEdit => self.editor.update(ui),
            AppKind::Calculator => self.calculator.update(ui),
            AppKind::Settings => {
                if let Some(settings) = &mut self.settings {
                    settings.update(ui);
                }
            }
            AppKind::Monitor => {
                if let Some(monitor) = &mut self.monitor {
                    monitor.update(ui);
                }
            }
        }
    }
}
