//! Catalogue des applications Ferrite : la seule liste de leurs noms, identifiants, icônes
//! et tailles de fenêtre. `ferrite-apps` s'en sert pour ouvrir ses fenêtres, `ferrite-shell`
//! pour le menu Applications, le Dock et reconnaître les fenêtres.
//!
//! Ajouter une application : une entrée ici, puis voir `docs/NOUVELLE-APPLICATION.md`.

/// Une application Ferrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AppInfo {
    /// Argument de `ferrite-apps` qui l'ouvre (`ferrite-apps clock`).
    pub command: &'static str,
    /// Identifiant Wayland de ses fenêtres, reconnu par le Dock.
    pub app_id: &'static str,
    /// Nom affiché (menu Applications, Dock, titre de la fenêtre).
    pub name: &'static str,
    /// Icône du dossier `icons/SVG`.
    pub icon: &'static str,
    /// Taille de la fenêtre à l'ouverture, et taille minimale (pixels logiques).
    pub size: (u16, u16),
    pub min_size: (u16, u16),
    /// Épinglée dans le Dock d'origine.
    pub dock: bool,
    /// Proposée dans le menu Applications (la galerie des développeurs ne l'est pas).
    pub menu: bool,
    /// Seulement sur la clé live (Installer Ferrite) : absente d'un Ferrite installé.
    pub live: bool,
}

const fn app(
    command: &'static str,
    app_id: &'static str,
    name: &'static str,
    icon: &'static str,
    size: (u16, u16),
    min_size: (u16, u16),
    dock: bool,
) -> AppInfo {
    AppInfo {
        command,
        app_id,
        name,
        icon,
        size,
        min_size,
        dock,
        menu: true,
        live: false,
    }
}

/// Le système tourne-t-il depuis la clé live ?
pub fn is_live() -> bool {
    std::path::Path::new("/run/live/medium").exists()
}

/// L'assistant de bienvenue doit-il s'ouvrir à l'ouverture de session ? `value` est le
/// réglage `bienvenue` : `faite` une fois parcouru, `a_faire` écrit par l'installation pour
/// le nouveau compte. Sans réglage, seulement sur la clé live (un Ferrite déjà en service
/// ne le voit pas apparaître après une mise à jour).
pub fn welcome_due(value: Option<&str>, live: bool) -> bool {
    match value.map(str::trim) {
        Some("faite") => false,
        Some("a_faire") => true,
        _ => live,
    }
}

impl AppInfo {
    /// L'application est-elle proposée sur ce système (menu, Dock) ?
    pub fn available(&self) -> bool {
        !self.live || is_live()
    }
}

/// Applications Ferrite, dans l'ordre du menu Applications et du Dock.
pub const APPS: &[AppInfo] = &[
    // Sur la clé live seulement, en tête du Dock : c'est ce qu'on y vient faire.
    AppInfo {
        live: true,
        ..app(
            "installer",
            "ferrite-installer",
            "Installer Ferrite",
            "app-installer",
            (880, 760),
            (640, 520),
            true,
        )
    },
    app(
        "files",
        "ferrite-files",
        "Fichiers",
        "app-files",
        (1000, 640),
        (560, 340),
        true,
    ),
    app(
        "textedit",
        "ferrite-textedit",
        "TextEdit",
        "app-text-editor",
        (1000, 640),
        (420, 280),
        true,
    ),
    app(
        "terminal",
        "ferrite-terminal",
        "Terminal",
        "app-terminal",
        (720, 450),
        (420, 260),
        true,
    ),
    app(
        "calculator",
        "ferrite-calculator",
        "Calculatrice",
        "app-calculator",
        (320, 470),
        (280, 420),
        true,
    ),
    app(
        "calendar",
        "ferrite-calendar",
        "Calendrier",
        "app-calendar",
        (1180, 760),
        (480, 460),
        false,
    ),
    app(
        "clock",
        "ferrite-clock",
        "Horloge",
        "app-clock",
        (440, 560),
        (360, 480),
        false,
    ),
    app(
        "settings",
        "ferrite-settings",
        "Paramètres",
        "app-settings",
        (1008, 640),
        (720, 440),
        true,
    ),
    app(
        "monitor",
        "ferrite-monitor",
        "Moniteur d'activité",
        "app-monitor",
        (1120, 720),
        (680, 460),
        false,
    ),
    app(
        "viewer",
        "ferrite-viewer",
        "Visionneuse",
        "app-photos",
        (1180, 760),
        (640, 440),
        false,
    ),
    app(
        "pdf",
        "ferrite-pdf",
        "Lecteur PDF",
        "app-pdf",
        (1180, 760),
        (640, 440),
        false,
    ),
    app(
        "music",
        "ferrite-music",
        "Musique",
        "app-music",
        (1240, 800),
        (760, 520),
        false,
    ),
    app(
        "video",
        "ferrite-video",
        "Lecteur vidéo",
        "app-video",
        (1100, 660),
        (640, 400),
        false,
    ),
    app(
        "logitheque",
        "ferrite-logitheque",
        "Logithèque",
        "app-store",
        (1180, 760),
        (720, 480),
        false,
    ),
    // Sélecteur de fichiers du portail (xdg-desktop-portal-ferrite), hors du menu.
    AppInfo {
        menu: false,
        ..app(
            "choisir",
            "ferrite-chooser",
            "Choisir un fichier",
            "app-files",
            (960, 620),
            (620, 420),
            false,
        )
    },
    // Assistant du premier démarrage, ouvert par le bureau à l'ouverture de session
    // (réglage `bienvenue`), hors du menu.
    AppInfo {
        menu: false,
        ..app(
            "bienvenue",
            "ferrite-welcome",
            "Bienvenue",
            "app-settings",
            (1180, 760),
            (860, 600),
            false,
        )
    },
    // Pièces communes en démonstration, pour qui développe une application.
    AppInfo {
        menu: false,
        ..app(
            "galerie",
            "ferrite-gallery",
            "Galerie Ferrite",
            "app-store",
            (900, 640),
            (560, 420),
            false,
        )
    },
];

/// Application ouverte par `ferrite-apps <command>`.
pub fn by_command(command: &str) -> Option<&'static AppInfo> {
    APPS.iter().find(|app| app.command == command)
}

/// Application d'une fenêtre d'identifiant `app_id`.
pub fn by_app_id(app_id: &str) -> Option<&'static AppInfo> {
    APPS.iter().find(|app| app.app_id == app_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bienvenue_a_l_ouverture() {
        assert!(welcome_due(None, true));
        assert!(!welcome_due(None, false));
        assert!(welcome_due(Some("a_faire"), false));
        assert!(!welcome_due(Some("faite"), true));
    }

    #[test]
    fn identifiants_uniques_et_coherents() {
        for (index, app) in APPS.iter().enumerate() {
            assert!(app.app_id.starts_with("ferrite-"), "{}", app.app_id);
            assert!(app.icon.starts_with("app-"), "{}", app.icon);
            assert!(app.min_size.0 <= app.size.0 && app.min_size.1 <= app.size.1);
            for other in &APPS[index + 1..] {
                assert_ne!(app.command, other.command);
                assert_ne!(app.app_id, other.app_id);
            }
        }
    }

    #[test]
    fn retrouve_une_application() {
        assert_eq!(by_command("clock").map(|app| app.name), Some("Horloge"));
        assert_eq!(by_app_id("ferrite-video").map(|app| app.command), Some("video"));
        assert!(by_command("inconnue").is_none());
    }
}
