//! Applications proposées par le bureau (Dock et menu Applications).

use std::{ffi::OsStr, process::Command};

use ferrite_system::command::spawn_detached;

use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    Files,
    TextEdit,
    Terminal,
    Calculator,
    Settings,
}

impl App {
    pub const ALL: [App; 5] = [
        App::Files,
        App::TextEdit,
        App::Terminal,
        App::Calculator,
        App::Settings,
    ];

    pub fn name(self) -> &'static str {
        match self {
            App::Files => "Fichiers",
            App::TextEdit => "TextEdit",
            App::Terminal => "Terminal",
            App::Calculator => "Calculatrice",
            App::Settings => "Paramètres",
        }
    }

    /// Icône du dossier `icons/SVG`.
    pub fn icon(self) -> &'static str {
        match self {
            App::Files => "app-files",
            App::TextEdit => "app-text-editor",
            App::Terminal => "app-terminal",
            App::Calculator => "app-calculator",
            App::Settings => "app-settings",
        }
    }

    /// Identifiant Wayland (`app_id`) des fenêtres de l'application.
    pub fn app_id(self) -> &'static str {
        match self {
            App::Files => "ferrite-files",
            App::TextEdit => "ferrite-textedit",
            App::Terminal => "foot",
            App::Calculator => "ferrite-calculator",
            App::Settings => "ferrite-settings",
        }
    }

    fn command(self) -> (&'static str, Option<&'static str>) {
        match self {
            App::Files => ("ferrite-apps", Some("files")),
            App::TextEdit => ("ferrite-apps", Some("textedit")),
            App::Terminal => ("foot", None),
            App::Calculator => ("ferrite-apps", Some("calculator")),
            App::Settings => ("ferrite-apps", Some("settings")),
        }
    }

    /// Lance l'application comme client du compositeur. Les exécutables placés à côté de
    /// ferrite-shell (compilation de développement) passent avant ceux du `PATH`.
    pub fn launch(self, socket_name: &OsStr) {
        let (program, argument) = self.command();
        let mut command = std::env::current_exe()
            .ok()
            .map(|path| path.with_file_name(program))
            .filter(|path| path.is_file())
            .map(Command::new)
            .unwrap_or_else(|| Command::new(program));
        if let Some(argument) = argument {
            command.arg(argument);
        }
        command.env("WAYLAND_DISPLAY", socket_name);
        match spawn_detached(command) {
            Ok(pid) => info!(pid, app = self.name(), "application lancée"),
            Err(err) => tracing::warn!(app = self.name(), %err, "application indisponible"),
        }
    }
}
