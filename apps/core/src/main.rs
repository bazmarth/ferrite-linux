//! Applications de base Ferrite : Fichiers, TextEdit et Calculatrice.

mod calculator;
mod editor;
mod files;
mod settings;
mod style;

use std::{env, path::PathBuf};

use eframe::egui;

use crate::{calculator::CalculatorState, editor::EditorState, files::FilesState, settings::SettingsState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppKind {
    Files,
    TextEdit,
    Calculator,
    Settings,
}

impl AppKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "files" => Some(Self::Files),
            "textedit" => Some(Self::TextEdit),
            "calculator" => Some(Self::Calculator),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Files => "Fichiers",
            Self::TextEdit => "TextEdit",
            Self::Calculator => "Calculatrice",
            Self::Settings => "Paramètres",
        }
    }

    /// Identifiant Wayland, reconnu par le Dock de ferrite-shell.
    fn app_id(self) -> &'static str {
        match self {
            Self::Files => "ferrite-files",
            Self::TextEdit => "ferrite-textedit",
            Self::Calculator => "ferrite-calculator",
            Self::Settings => "ferrite-settings",
        }
    }

    /// Taille initiale et taille minimale de la fenêtre.
    fn sizes(self) -> ([f32; 2], [f32; 2]) {
        match self {
            Self::Files => ([860.0, 520.0], [560.0, 340.0]),
            Self::TextEdit => ([760.0, 540.0], [420.0, 280.0]),
            Self::Calculator => ([320.0, 470.0], [280.0, 420.0]),
            Self::Settings => ([880.0, 580.0], [680.0, 440.0]),
        }
    }
}

fn main() -> eframe::Result {
    let mut args = env::args().skip(1);
    let Some(kind) = args.next().and_then(|arg| AppKind::parse(&arg)) else {
        eprintln!("Utilisation : ferrite-apps <files|textedit|calculator|settings> [dossier ou fichier]");
        return Ok(());
    };
    let initial_file = args.next().map(PathBuf::from);
    let (size, min_size) = kind.sizes();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_app_id(kind.app_id())
            .with_inner_size(size)
            .with_min_inner_size(min_size),
        ..Default::default()
    };
    eframe::run_native(
        kind.title(),
        options,
        Box::new(move |context| Ok(Box::new(FerriteApp::new(context, kind, initial_file)))),
    )
}

struct FerriteApp {
    kind: AppKind,
    files: Option<FilesState>,
    editor: EditorState,
    calculator: CalculatorState,
    settings: Option<SettingsState>,
}

impl FerriteApp {
    fn new(context: &eframe::CreationContext<'_>, kind: AppKind, initial_file: Option<PathBuf>) -> Self {
        style::apply(&context.egui_ctx);
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
        }
    }
}

impl eframe::App for FerriteApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // F11 : plein écran, comme dans les navigateurs.
        if ui.input(|input| input.key_pressed(egui::Key::F11)) {
            let fullscreen = ui.input(|input| input.viewport().fullscreen.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
        }
        // Le client est rendu dans une surface Wayland translucide : peindre son
        // fond explicitement évite que le bureau transparaisse.
        ui.painter()
            .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, style::BACKGROUND);
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
        }
    }
}
