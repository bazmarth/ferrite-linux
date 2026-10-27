//! Applications proposées par le bureau (Dock et menu Applications).

use std::process::Command;

use ferrite_system::{applications::DesktopEntry, command::spawn_detached};

use tracing::info;

/// Case du menu Applications.
#[derive(Debug, Clone)]
pub enum LauncherEntry<'a> {
    Builtin(App),
    Installed(&'a DesktopEntry),
}

impl LauncherEntry<'_> {
    pub fn name(&self) -> &str {
        match self {
            LauncherEntry::Builtin(app) => app.name(),
            LauncherEntry::Installed(entry) => &entry.name,
        }
    }

    pub fn launch(&self, env: &[(String, String)]) {
        match self {
            LauncherEntry::Builtin(app) => app.launch(env),
            LauncherEntry::Installed(entry) => launch_entry(entry, env),
        }
    }
}

/// Les applications Ferrite d'abord, puis les applications installées, filtrées par
/// `query` (sans tenir compte de la casse).
pub fn launcher_entries<'a>(installed: &'a [DesktopEntry], query: &str) -> Vec<LauncherEntry<'a>> {
    let query = query.trim().to_lowercase();
    App::ALL
        .into_iter()
        .map(LauncherEntry::Builtin)
        .chain(installed.iter().map(LauncherEntry::Installed))
        .filter(|entry| query.is_empty() || entry.name().to_lowercase().contains(&query))
        .collect()
}

/// Lance une application installée ; les programmes en mode texte s'ouvrent dans le
/// Terminal.
fn launch_entry(entry: &DesktopEntry, env: &[(String, String)]) {
    let Some((program, args)) = entry.exec.split_first() else {
        return;
    };
    let mut command = if entry.terminal {
        let mut command = Command::new("foot");
        command.arg("--").arg(program).args(args);
        command
    } else {
        let mut command = Command::new(program);
        command.args(args);
        command
    };
    command.envs(env.iter().map(|(key, value)| (key, value)));
    match spawn_detached(command) {
        Ok(pid) => info!(pid, app = entry.name, "application installée lancée"),
        Err(err) => tracing::warn!(app = entry.name, %err, "application installée indisponible"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    Files,
    TextEdit,
    Terminal,
    Calculator,
    Settings,
    Monitor,
}

impl App {
    /// Applications Ferrite du menu Applications.
    pub const ALL: [App; 6] = [
        App::Files,
        App::TextEdit,
        App::Terminal,
        App::Calculator,
        App::Settings,
        App::Monitor,
    ];

    /// Applications épinglées dans le Dock.
    pub const DOCK: [App; 5] = [
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
            App::Monitor => "Moniteur d'activité",
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
            App::Monitor => "app-monitor",
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
            App::Monitor => "ferrite-monitor",
        }
    }

    fn command(self) -> (&'static str, Option<&'static str>) {
        match self {
            App::Files => ("ferrite-apps", Some("files")),
            App::TextEdit => ("ferrite-apps", Some("textedit")),
            App::Terminal => ("foot", None),
            App::Calculator => ("ferrite-apps", Some("calculator")),
            App::Settings => ("ferrite-apps", Some("settings")),
            App::Monitor => ("ferrite-apps", Some("monitor")),
        }
    }

    /// Lance l'application comme client du compositeur. Les exécutables placés à côté de
    /// ferrite-shell (compilation de développement) passent avant ceux du `PATH`.
    pub fn launch(self, env: &[(String, String)]) {
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
        command.envs(env.iter().map(|(key, value)| (key, value)));
        match spawn_detached(command) {
            Ok(pid) => info!(pid, app = self.name(), "application lancée"),
            Err(err) => tracing::warn!(app = self.name(), %err, "application indisponible"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> DesktopEntry {
        DesktopEntry {
            id: name.to_lowercase(),
            name: name.to_owned(),
            exec: vec![name.to_lowercase()],
            terminal: false,
            icon: None,
            wm_class: None,
        }
    }

    #[test]
    fn le_menu_met_ferrite_en_premier_et_filtre() {
        let installed = [entry("Brave Web Browser"), entry("VLC media player")];
        let all = launcher_entries(&installed, "");
        assert_eq!(all.len(), App::ALL.len() + 2);
        assert_eq!(all[0].name(), "Fichiers");
        let found = launcher_entries(&installed, "vlc");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name(), "VLC media player");
        assert_eq!(launcher_entries(&installed, "CALC")[0].name(), "Calculatrice");
        assert_eq!(
            launcher_entries(&installed, "moniteur")[0].name(),
            "Moniteur d'activité"
        );
    }
}
