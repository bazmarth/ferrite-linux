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

impl LauncherEntry<'_> {}

/// Clé de tri : sans casse ni accent, pour que « Éditeur » suive « Dock » et non « Zoom ».
fn sort_key(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .map(|character| match character {
            'à' | 'á' | 'â' | 'ä' | 'ã' => "a".to_owned(),
            'é' | 'è' | 'ê' | 'ë' => "e".to_owned(),
            'î' | 'ï' | 'í' => "i".to_owned(),
            'ô' | 'ö' | 'ó' => "o".to_owned(),
            'ù' | 'û' | 'ü' | 'ú' => "u".to_owned(),
            'ç' => "c".to_owned(),
            'œ' => "oe".to_owned(),
            'æ' => "ae".to_owned(),
            other => other.to_string(),
        })
        .collect()
}

/// Les applications Ferrite d'abord, puis les applications installées, filtrées par
/// `query` (sans tenir compte de la casse).
pub fn launcher_entries<'a>(installed: &'a [DesktopEntry], query: &str) -> Vec<LauncherEntry<'a>> {
    let query = query.trim().to_lowercase();
    let mut entries: Vec<LauncherEntry<'a>> = App::ALL
        .into_iter()
        .map(LauncherEntry::Builtin)
        .chain(installed.iter().map(LauncherEntry::Installed))
        .filter(|entry| query.is_empty() || entry.name().to_lowercase().contains(&query))
        .collect();
    entries.sort_by_cached_key(|entry| sort_key(entry.name()));
    entries
}

/// Commande d'un exécutable Ferrite : celui placé à côté de ferrite-shell (compilation de
/// développement) passe avant celui du `PATH`.
fn ferrite_command(program: &str) -> Command {
    std::env::current_exe()
        .ok()
        .map(|path| path.with_file_name(program))
        .filter(|path| path.is_file())
        .map(Command::new)
        .unwrap_or_else(|| Command::new(program))
}

/// Lance une application installée ; les programmes en mode texte s'ouvrent dans le
/// Terminal.
fn launch_entry(entry: &DesktopEntry, env: &[(String, String)]) {
    launch_entry_with(entry, env, &[]);
}

/// Comme [`launch_entry`], avec des fichiers à ouvrir (ajoutés à la ligne de commande,
/// comme les codes `%f` et `%U` du fichier `.desktop`).
fn launch_entry_with(entry: &DesktopEntry, env: &[(String, String)], files: &[std::path::PathBuf]) {
    let Some((program, args)) = entry.exec.split_first() else {
        return;
    };
    let mut command = if entry.terminal {
        let mut command = ferrite_command("ferrite-apps");
        command.arg("terminal").arg("-e").arg(program).args(args);
        command
    } else {
        let mut command = Command::new(program);
        command.args(args);
        // Navigateurs Chromium (Brave, Chrome, Edge, Vivaldi) et VS Code : en Wayland
        // natif, nets sur un écran HiDPI ; sinon XWayland les agrandit et les floute.
        if is_chromium(program) {
            command.arg("--ozone-platform-hint=auto");
        }
        command.args(files);
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

    /// Nom affiché de l'application.
    pub fn name(&self, installed: &[DesktopEntry]) -> String {
        match self {
            AppKey::Builtin(app) => app.name().to_owned(),
            AppKey::Installed(_) => self
                .entry(installed)
                .map_or_else(|| self.token(), |entry| entry.name.clone()),
            AppKey::Other(id) => id.clone(),
        }
    }

    /// Lance l'application avec `files` à ouvrir ; une application Ferrite déjà ouverte
    /// les reçoit dans sa fenêtre. Faux si on ne sait pas la lancer.
    pub fn launch_with(
        &self,
        installed: &[DesktopEntry],
        env: &[(String, String)],
        files: &[std::path::PathBuf],
    ) -> bool {
        match self {
            AppKey::Builtin(app) => {
                app.launch_with(env, files);
                true
            }
            AppKey::Installed(_) => match self.entry(installed) {
                Some(entry) => {
                    launch_entry_with(entry, env, files);
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
    /// Corbeille, toujours en dernière case du Dock : la vue Corbeille de Fichiers.
    Trash,
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
            App::Trash => "Corbeille",
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
            App::Trash => "places-trash",
        }
    }

    /// Identifiant Wayland (`app_id`) des fenêtres de l'application.
    pub fn app_id(self) -> &'static str {
        match self {
            App::Files => "ferrite-files",
            App::TextEdit => "ferrite-textedit",
            App::Terminal => "ferrite-terminal",
            App::Calculator => "ferrite-calculator",
            App::Settings => "ferrite-settings",
            App::Monitor => "ferrite-monitor",
            App::Trash => "ferrite-trash",
        }
    }

    fn command(self) -> (&'static str, Option<&'static str>) {
        match self {
            App::Files => ("ferrite-apps", Some("files")),
            App::TextEdit => ("ferrite-apps", Some("textedit")),
            App::Terminal => ("ferrite-apps", Some("terminal")),
            App::Calculator => ("ferrite-apps", Some("calculator")),
            App::Settings => ("ferrite-apps", Some("settings")),
            App::Monitor => ("ferrite-apps", Some("monitor")),
            App::Trash => ("ferrite-apps", Some("files")),
        }
    }

    /// Paramètres sur une rubrique : s'il est déjà ouvert, la fenêtre existante y va.
    pub fn open_settings_page(page: &str, env: &[(String, String)]) {
        let mut command = ferrite_command("ferrite-apps");
        command.args(["settings", page]);
        command.envs(env.iter().map(|(key, value)| (key, value)));
        if let Err(err) = spawn_detached(command) {
            tracing::warn!(%err, "Paramètres indisponibles");
        }
    }

    /// Lance l'application comme client du compositeur.
    pub fn launch(self, env: &[(String, String)]) {
        self.launch_with(env, &[]);
    }

    /// Lance l'application avec des fichiers à ouvrir : Fichiers montre le premier, TextEdit
    /// ouvre chacun, le Terminal s'ouvre dans le dossier du premier. La Corbeille, elle, y
    /// met les fichiers.
    pub fn launch_with(self, env: &[(String, String)], files: &[std::path::PathBuf]) {
        let (program, argument) = self.command();
        let mut command = ferrite_command(program);
        match (self, files.first()) {
            (App::Trash, Some(_)) => {
                command.arg("corbeille").args(files);
            }
            (App::Trash, None) => {
                command.args(["files", "corbeille:"]);
            }
            (App::Terminal, Some(first)) => {
                let dir = if first.is_dir() {
                    first.as_path()
                } else {
                    first.parent().unwrap_or(first)
                };
                command.arg("terminal").arg(dir);
            }
            (_, _) => {
                command.args(argument).args(files);
            }
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
    fn le_lanceur_liste_de_a_a_z_sans_tenir_compte_des_accents() {
        let installed = [entry("zoom"), entry("Évince"), entry("brave")];
        let entries = launcher_entries(&installed, "");
        let names: Vec<&str> = entries.iter().map(LauncherEntry::name).collect();
        let mut sorted = names.clone();
        sorted.sort_by_cached_key(|name| sort_key(name));
        assert_eq!(names, sorted);
        let position = |name: &str| names.iter().position(|known| *known == name).unwrap();
        assert!(position("brave") < position("Évince"));
        assert!(position("Évince") < position("zoom"));
        assert_eq!(sort_key("Éditeur Œuvre"), "editeur oeuvre");
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
    fn le_menu_trie_et_filtre() {
        let installed = [entry("Brave Web Browser"), entry("VLC media player")];
        let all = launcher_entries(&installed, "");
        assert_eq!(all.len(), App::ALL.len() + 2);
        assert_eq!(all[0].name(), "Brave Web Browser");
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
