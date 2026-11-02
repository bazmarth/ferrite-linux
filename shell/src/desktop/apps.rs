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

    /// Application de la case, pour l'ouvrir sans doublon.
    pub fn key(&self) -> AppKey {
        match self {
            LauncherEntry::Builtin(app) => AppKey::Builtin(*app),
            LauncherEntry::Installed(entry) => AppKey::Installed(entry.id.clone()),
        }
    }
}

impl LauncherEntry<'_> {
    /// Rubrique du lanceur, d'après les catégories freedesktop de l'application.
    pub fn section(&self) -> Section {
        match self {
            LauncherEntry::Builtin(App::TextEdit) => Section::Productivity,
            LauncherEntry::Builtin(_) => Section::Utilities,
            LauncherEntry::Installed(entry) => Section::of(&entry.categories),
        }
    }
}

/// Rubriques du lanceur, dans l'ordre d'affichage (après les suggestions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Productivity,
    Internet,
    Creation,
    Entertainment,
    Development,
    Utilities,
    Other,
}

impl Section {
    pub const ALL: [Section; 7] = [
        Section::Productivity,
        Section::Internet,
        Section::Creation,
        Section::Entertainment,
        Section::Development,
        Section::Utilities,
        Section::Other,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Productivity => "Productivité",
            Section::Internet => "Internet",
            Section::Creation => "Création",
            Section::Entertainment => "Divertissement",
            Section::Development => "Développement",
            Section::Utilities => "Utilitaires",
            Section::Other => "Autres",
        }
    }

    /// Rubrique d'une liste de catégories freedesktop : la première règle qui s'applique,
    /// des plus précises (développement, jeux) aux plus générales (utilitaires).
    fn of(categories: &[String]) -> Section {
        let has = |names: &[&str]| {
            categories
                .iter()
                .any(|category| names.contains(&category.as_str()))
        };
        if has(&["Development", "IDE"]) {
            Section::Development
        } else if has(&["Game", "AudioVideo", "Audio", "Video", "Player", "Music", "TV"]) {
            Section::Entertainment
        } else if has(&[
            "Graphics",
            "Photography",
            "2DGraphics",
            "3DGraphics",
            "RasterGraphics",
            "VectorGraphics",
        ]) {
            Section::Creation
        } else if has(&[
            "Network",
            "WebBrowser",
            "Email",
            "Chat",
            "InstantMessaging",
            "FileTransfer",
        ]) {
            Section::Internet
        } else if has(&[
            "Office",
            "TextEditor",
            "WordProcessor",
            "Spreadsheet",
            "Presentation",
            "Education",
            "Science",
        ]) {
            Section::Productivity
        } else if has(&[
            "System",
            "Utility",
            "Settings",
            "Monitor",
            "FileManager",
            "TerminalEmulator",
            "Accessibility",
        ]) {
            Section::Utilities
        } else {
            Section::Other
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
fn configure_terminal_zoom(command: &mut Command) {
    command
        .arg("-o")
        .arg("main.resize-keep-grid=no")
        .arg("-o")
        .arg("key-bindings.font-increase=Control+plus Control+equal Control+KP_Add")
        .arg("-o")
        .arg("key-bindings.font-decrease=Control+minus Control+KP_Subtract")
        .arg("-o")
        .arg("key-bindings.font-reset=Control+0 Control+KP_0")
        .arg("-o")
        .arg("mouse-bindings.font-increase=Control+BTN_WHEEL_BACK")
        .arg("-o")
        .arg("mouse-bindings.font-decrease=Control+BTN_WHEEL_FORWARD")
        .arg("-o")
        .arg("main.pad=12x8 center");
}

fn launch_entry(entry: &DesktopEntry, env: &[(String, String)]) {
    let Some((program, args)) = entry.exec.split_first() else {
        return;
    };
    let mut command = if entry.terminal {
        let mut command = Command::new("foot");
        configure_terminal_zoom(&mut command);
        command.arg("--").arg(program).args(args);
        command
    } else {
        let mut command = Command::new(program);
        command.args(args);
        // Navigateurs Chromium (Brave, Chrome, Edge, Vivaldi) et VS Code : en Wayland
        // natif, nets sur un écran HiDPI ; sinon XWayland les agrandit et les floute.
        if is_chromium(program) {
            command.arg("--ozone-platform-hint=auto");
        }
        command
    };
    command.envs(env.iter().map(|(key, value)| (key, value)));
    match spawn_detached(command) {
        Ok(pid) => info!(pid, app = entry.name, "application installée lancée"),
        Err(err) => tracing::warn!(app = entry.name, %err, "application installée indisponible"),
    }
}

/// Application du Dock : une application Ferrite, une application installée (fichier
/// `.desktop`), ou une fenêtre qu'on ne sait rattacher à rien (son `app_id`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AppKey {
    Builtin(App),
    Installed(String),
    Other(String),
}

/// Icône d'une application du Dock.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AppIcon {
    Builtin(&'static str),
    File(std::path::PathBuf),
    Missing,
}

impl AppKey {
    /// Identifiant gardé dans les réglages : `app_id` des applications Ferrite, nom du
    /// fichier `.desktop` des autres.
    pub fn token(&self) -> String {
        match self {
            AppKey::Builtin(app) => app.app_id().to_owned(),
            AppKey::Installed(id) | AppKey::Other(id) => id.clone(),
        }
    }

    pub fn from_token(token: &str, installed: &[DesktopEntry]) -> AppKey {
        if let Some(app) = App::ALL.into_iter().find(|app| app.app_id() == token) {
            return AppKey::Builtin(app);
        }
        if installed.iter().any(|entry| entry.id == token) {
            return AppKey::Installed(token.to_owned());
        }
        AppKey::Other(token.to_owned())
    }

    /// Application d'une fenêtre d'identifiant `app_id` (ou classe X11 en minuscules).
    pub fn for_window(app_id: &str, installed: &[DesktopEntry]) -> AppKey {
        if let Some(app) = App::ALL.into_iter().find(|app| app.app_id() == app_id) {
            return AppKey::Builtin(app);
        }
        let id = app_id.to_lowercase();
        let matches = |entry: &&DesktopEntry| {
            let desktop = entry.id.to_lowercase();
            entry.wm_class.as_deref().is_some_and(|class| class.to_lowercase() == id)
                || desktop == id
                // « org.videolan.VLC » pour une fenêtre « vlc ».
                || desktop.rsplit('.').next() == Some(id.as_str())
                || entry
                    .exec
                    .first()
                    .and_then(|program| program.rsplit('/').next())
                    .is_some_and(|program| program.to_lowercase() == id)
        };
        match installed.iter().find(matches) {
            Some(entry) => AppKey::Installed(entry.id.clone()),
            None => AppKey::Other(app_id.to_owned()),
        }
    }

    fn entry<'a>(&self, installed: &'a [DesktopEntry]) -> Option<&'a DesktopEntry> {
        match self {
            AppKey::Installed(id) => installed.iter().find(|entry| entry.id == *id),
            _ => None,
        }
    }

    pub fn icon(&self, installed: &[DesktopEntry]) -> AppIcon {
        match self {
            AppKey::Builtin(app) => AppIcon::Builtin(app.icon()),
            _ => self
                .entry(installed)
                .and_then(|entry| entry.icon.clone())
                .map_or(AppIcon::Missing, AppIcon::File),
        }
    }

    /// Lance l'application ; faux si on ne sait pas comment (fenêtre sans `.desktop`).
    pub fn launch(&self, installed: &[DesktopEntry], env: &[(String, String)]) -> bool {
        match self {
            AppKey::Builtin(app) => {
                app.launch(env);
                true
            }
            AppKey::Installed(_) => match self.entry(installed) {
                Some(entry) => {
                    launch_entry(entry, env);
                    true
                }
                None => false,
            },
            AppKey::Other(_) => false,
        }
    }
}

/// Programme de la famille Chromium (qui choisit Wayland avec `--ozone-platform-hint`).
fn is_chromium(program: &str) -> bool {
    let name = program.rsplit('/').next().unwrap_or(program);
    [
        "brave",
        "chromium",
        "google-chrome",
        "microsoft-edge",
        "vivaldi",
        "code",
        "codium",
    ]
    .iter()
    .any(|known| name.starts_with(known))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        if self == App::Terminal {
            configure_terminal_zoom(&mut command);
            command
                .arg("tmux")
                .arg("-f")
                .arg("/home/bazoum/.config/tmux.conf")
                .arg("new-session")
                .arg("-A")
                .arg("-s")
                .arg("ferrite");
        }
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
            categories: Vec::new(),
        }
    }

    #[test]
    fn rubriques_d_apres_les_categories() {
        let with =
            |categories: &[&str]| Section::of(&categories.iter().map(|c| c.to_string()).collect::<Vec<_>>());
        assert_eq!(with(&["AudioVideo", "Player"]), Section::Entertainment);
        assert_eq!(with(&["Network", "WebBrowser"]), Section::Internet);
        assert_eq!(with(&["Development", "TextEditor"]), Section::Development);
        assert_eq!(with(&["Graphics", "Photography"]), Section::Creation);
        assert_eq!(with(&["Office", "Spreadsheet"]), Section::Productivity);
        assert_eq!(with(&["GTK", "System", "Settings"]), Section::Utilities);
        assert_eq!(with(&[]), Section::Other);
        assert_eq!(
            LauncherEntry::Builtin(App::Calculator).section(),
            Section::Utilities
        );
    }

    #[test]
    fn navigateurs_chromium_reconnus() {
        assert!(is_chromium("/usr/bin/brave-browser-stable"));
        assert!(is_chromium("google-chrome"));
        assert!(is_chromium("/usr/share/code/code"));
        assert!(!is_chromium("vlc"));
        assert!(!is_chromium("firefox"));
    }

    #[test]
    fn une_fenetre_retrouve_son_application() {
        let mut vlc = entry("VLC media player");
        vlc.id = "org.videolan.VLC".into();
        let mut brave = entry("Brave");
        brave.id = "brave-browser".into();
        brave.wm_class = Some("Brave-browser".into());
        let installed = [vlc, brave];
        assert_eq!(
            AppKey::for_window("ferrite-files", &installed),
            AppKey::Builtin(App::Files)
        );
        assert_eq!(
            AppKey::for_window("vlc", &installed),
            AppKey::Installed("org.videolan.VLC".into())
        );
        assert_eq!(
            AppKey::for_window("brave-browser", &installed),
            AppKey::Installed("brave-browser".into())
        );
        assert_eq!(
            AppKey::for_window("inconnu", &installed),
            AppKey::Other("inconnu".into())
        );
        let key = AppKey::from_token("org.videolan.VLC", &installed);
        assert_eq!(key.token(), "org.videolan.VLC");
        assert_eq!(
            AppKey::from_token("ferrite-textedit", &installed),
            AppKey::Builtin(App::TextEdit)
        );
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
