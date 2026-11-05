//! Applications de base Ferrite : Fichiers, TextEdit, Terminal, Calculatrice, Paramètres,
//! Moniteur d'activité, Visionneuse, lecteur PDF et Musique.

mod calculator;
mod details;
mod editor;
mod files;
mod markdown;
mod monitor;
mod music;
mod pdf_viewer;
mod settings;
mod single;
mod style;
mod terminal;
mod video;
mod viewer;

use std::{
    env,
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use eframe::egui;

use crate::{
    calculator::CalculatorState, editor::EditorState, files::FilesState, monitor::MonitorState,
    music::MusicState, pdf_viewer::ViewerApp, settings::SettingsState, terminal::TerminalState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppKind {
    Files,
    TextEdit,
    Terminal,
    Calculator,
    Settings,
    Monitor,
    Viewer,
    PdfViewer,
    Music,
    Video,
}

impl AppKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "files" => Some(Self::Files),
            "textedit" => Some(Self::TextEdit),
            "terminal" => Some(Self::Terminal),
            "calculator" => Some(Self::Calculator),
            "settings" => Some(Self::Settings),
            "monitor" => Some(Self::Monitor),
            "viewer" => Some(Self::Viewer),
            "pdf" => Some(Self::PdfViewer),
            "music" => Some(Self::Music),
            "video" => Some(Self::Video),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Files => "Fichiers",
            Self::TextEdit => "TextEdit",
            Self::Terminal => "Terminal",
            Self::Calculator => "Calculatrice",
            Self::Settings => "Paramètres",
            Self::Monitor => "Moniteur d'activité",
            Self::Viewer => "Visionneuse",
            Self::PdfViewer => "Lecteur PDF",
            Self::Music => "Musique",
            Self::Video => "Lecteur vidéo",
        }
    }

    /// Identifiant Wayland, reconnu par le Dock de ferrite-shell.
    fn app_id(self) -> &'static str {
        match self {
            Self::Files => "ferrite-files",
            Self::TextEdit => "ferrite-textedit",
            Self::Terminal => "ferrite-terminal",
            Self::Calculator => "ferrite-calculator",
            Self::Settings => "ferrite-settings",
            Self::Monitor => "ferrite-monitor",
            Self::Viewer => "ferrite-viewer",
            Self::PdfViewer => "ferrite-pdf",
            Self::Music => "ferrite-music",
            Self::Video => "ferrite-video",
        }
    }

    /// Taille initiale et taille minimale de la fenêtre.
    fn sizes(self) -> ([f32; 2], [f32; 2]) {
        match self {
            Self::Files => ([1000.0, 640.0], [560.0, 340.0]),
            Self::TextEdit => ([1000.0, 640.0], [420.0, 280.0]),
            Self::Terminal => ([720.0, 450.0], [420.0, 260.0]),
            Self::Calculator => ([320.0, 470.0], [280.0, 420.0]),
            Self::Settings => ([1008.0, 640.0], [720.0, 440.0]),
            Self::Monitor => ([1000.0, 640.0], [680.0, 460.0]),
            Self::Viewer => ([1180.0, 760.0], [640.0, 440.0]),
            Self::PdfViewer => ([1180.0, 760.0], [640.0, 440.0]),
            Self::Music => ([1240.0, 800.0], [760.0, 520.0]),
            Self::Video => ([1180.0, 760.0], [640.0, 420.0]),
        }
    }
}

fn main() -> eframe::Result {
    let mut args = env::args().skip(1);
    // Fichiers lâchés sur la Corbeille du Dock : ferrite-shell les confie ici, sans fenêtre.
    if env::args().nth(1).as_deref() == Some("corbeille") {
        for error in files::put_in_trash(&env::args().skip(2).map(PathBuf::from).collect::<Vec<_>>()) {
            eprintln!("{error}");
        }
        return Ok(());
    }
    let Some(kind) = args.next().and_then(|arg| AppKind::parse(&arg)) else {
        eprintln!(
            "Utilisation : ferrite-apps <files|textedit|terminal|calculator|settings|monitor|viewer|pdf|music|video> [dossier ou fichier]"
        );
        return Ok(());
    };
    if kind == AppKind::Terminal {
        style::set_always_dark();
        // Chaque lancement est une fenêtre de plus, avec son propre shell.
        let launch = terminal::parse_launch(args);
        return run(kind, None, None, launch);
    }
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
    run(kind, initial_file, requests, terminal::Launch::default())
}

fn run(
    kind: AppKind,
    initial_file: Option<PathBuf>,
    requests: Option<single::Requests>,
    launch: terminal::Launch,
) -> eframe::Result {
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
        Box::new(move |context| {
            Ok(Box::new(FerriteApp::new(
                context,
                kind,
                initial_file,
                requests,
                launch,
            )))
        }),
    )
}

struct FerriteApp {
    kind: AppKind,
    files: Option<FilesState>,
    editor: EditorState,
    calculator: CalculatorState,
    settings: Option<SettingsState>,
    monitor: Option<MonitorState>,
    terminal: Option<TerminalState>,
    viewer: Option<ViewerApp>,
    music: Option<MusicState>,
    video: Option<video::VideoState>,
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
        launch: terminal::Launch,
    ) -> Self {
        style::apply(&context.egui_ctx);
        if let Some(requests) = &requests
            && let Ok(mut waker) = requests.context.lock()
        {
            *waker = Some(context.egui_ctx.clone());
        }
        let mut editor = EditorState::default();
        let viewer = match kind {
            AppKind::Viewer => Some(ViewerApp::new(initial_file.clone())),
            AppKind::PdfViewer => Some(ViewerApp::new_pdf(initial_file.clone())),
            _ => None,
        };
        let music = (kind == AppKind::Music).then(|| MusicState::new(initial_file.clone()));
        let video = (kind == AppKind::Video)
            .then(|| video::VideoState::new(initial_file.clone(), context.egui_ctx.clone()));
        let mut files = (kind == AppKind::Files).then(FilesState::default);
        let mut settings = (kind == AppKind::Settings).then(SettingsState::default);
        if let Some(path) = initial_file {
            if let Some(settings) = &mut settings {
                settings.open_page(&path.to_string_lossy());
            } else if let Some(files) = &mut files {
                files.open_at(path);
            } else if !matches!(
                kind,
                AppKind::Viewer | AppKind::PdfViewer | AppKind::Music | AppKind::Video
            ) {
                editor.open(path);
            }
        }
        Self {
            kind,
            files,
            editor,
            calculator: CalculatorState::default(),
            settings,
            monitor: (kind == AppKind::Monitor).then(MonitorState::default),
            terminal: (kind == AppKind::Terminal).then(|| TerminalState::new(&context.egui_ctx, launch)),
            viewer,
            music,
            video,
            requests,
            settings_stamp: {
                watch_settings(context.egui_ctx.clone());
                ferrite_system::config::Settings::modified()
            },
            settings_checked_at: Instant::now(),
        }
    }

    fn refresh_preferences(&mut self, context: &egui::Context) {
        // Réglages modifiés : tout de suite (voir `watch_settings`) ; sinon une fois par
        // seconde, pour le thème qui suit l'heure.
        let stamp = ferrite_system::config::Settings::modified();
        if stamp == self.settings_stamp && self.settings_checked_at.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.settings_checked_at = Instant::now();
        let settings = ferrite_system::config::Settings::load();
        if stamp != self.settings_stamp || style::wants_dark(&settings) != style::theme_is_dark() {
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
            if let Some(settings) = &mut self.settings {
                settings.open_page(&file.to_string_lossy());
            } else if let Some(files) = &mut self.files {
                files.open_at(file);
            } else if let Some(viewer) = &mut self.viewer {
                if self.kind == AppKind::PdfViewer {
                    viewer.open_pdf(file);
                } else {
                    viewer.open(file);
                }
            } else if let Some(music) = &mut self.music {
                music.open(file);
            } else if let Some(video) = &mut self.video {
                video.open(file);
            } else {
                self.editor.open(file);
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
            AppKind::Terminal => {
                if let Some(terminal) = &mut self.terminal {
                    terminal.update(ui);
                }
            }
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
            AppKind::Viewer => {
                if let Some(viewer) = &mut self.viewer {
                    viewer.update(ui);
                }
            }
            AppKind::PdfViewer => {
                if let Some(viewer) = &mut self.viewer {
                    viewer.update(ui);
                }
            }
            AppKind::Music => {
                if let Some(music) = &mut self.music {
                    music.update(ui);
                }
            }
            AppKind::Video => {
                if let Some(video) = &mut self.video {
                    video.update(ui);
                }
            }
        }
    }
}

/// Réveille l'application dès que les réglages changent : une fenêtre au repos ne se
/// redessine pas, et ne verrait sinon le nouveau thème qu'au prochain survol, longtemps
/// après les autres.
fn watch_settings(context: egui::Context) {
    let spawned = std::thread::Builder::new()
        .name("réglages".into())
        .spawn(move || {
            let mut stamp = ferrite_system::config::Settings::modified();
            loop {
                std::thread::sleep(Duration::from_millis(150));
                let current = ferrite_system::config::Settings::modified();
                if current != stamp {
                    stamp = current;
                    context.request_repaint();
                }
            }
        });
    if let Err(err) = spawned {
        eprintln!("surveillance des réglages impossible : {err}");
    }
}
