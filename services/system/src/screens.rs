//! Plusieurs écrans : choix entre dupliquer l'écran principal et étendre le bureau, et
//! liste des écrans branchés publiée par le bureau pour Paramètres.

use std::path::PathBuf;

use crate::config::Settings;

/// Clé du réglage dans `~/.config/ferrite/reglages`.
const KEY: &str = "ecrans_mode";
/// Fichier où le bureau écrit les écrans branchés (un par ligne).
const STATUS_FILE: &str = "ferrite-ecrans";

/// Que montrer sur un écran branché en plus de l'écran principal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenMode {
    /// Même image que l'écran principal, mise à l'échelle (présentation, télévision).
    #[default]
    Duplicate,
    /// Bureau agrandi : le second écran prolonge le principal à sa droite, on y glisse
    /// des fenêtres.
    Extend,
}

impl ScreenMode {
    pub const ALL: [ScreenMode; 2] = [ScreenMode::Duplicate, ScreenMode::Extend];

    pub fn label(self) -> &'static str {
        match self {
            ScreenMode::Duplicate => "Dupliquer",
            ScreenMode::Extend => "Étendre",
        }
    }

    fn value(self) -> &'static str {
        match self {
            ScreenMode::Duplicate => "dupliquer",
            ScreenMode::Extend => "etendre",
        }
    }

    pub fn load(settings: &Settings) -> ScreenMode {
        match settings.get(KEY) {
            Some("etendre") => ScreenMode::Extend,
            _ => ScreenMode::Duplicate,
        }
    }

    pub fn store(self, settings: &mut Settings) {
        settings.set(KEY, self.value());
    }

    /// L'autre mode (raccourci Super+P).
    pub fn toggled(self) -> ScreenMode {
        match self {
            ScreenMode::Duplicate => ScreenMode::Extend,
            ScreenMode::Extend => ScreenMode::Duplicate,
        }
    }
}

/// Un écran branché, tel que le bureau l'utilise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenInfo {
    /// Nom du branchement (`eDP-1`, `HDMI-A-1`…).
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh: u32,
    pub primary: bool,
}

impl ScreenInfo {
    /// Nom lisible : « Écran intégré », « HDMI », « DisplayPort »…
    pub fn kind(&self) -> &'static str {
        let name = self.name.to_ascii_lowercase();
        if name.starts_with("edp") || name.starts_with("lvds") || name.starts_with("dsi") {
            "Écran intégré"
        } else if name.starts_with("hdmi") {
            "HDMI"
        } else if name.starts_with("dp") || name.starts_with("displayport") {
            "DisplayPort"
        } else if name.starts_with("dvi") {
            "DVI"
        } else if name.starts_with("vga") {
            "VGA"
        } else {
            "Écran externe"
        }
    }

    fn line(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.name,
            self.width,
            self.height,
            self.refresh,
            if self.primary { "principal" } else { "second" }
        )
    }

    fn parse(line: &str) -> Option<ScreenInfo> {
        let mut parts = line.split_whitespace();
        Some(ScreenInfo {
            name: parts.next()?.to_owned(),
            width: parts.next()?.parse().ok()?,
            height: parts.next()?.parse().ok()?,
            refresh: parts.next()?.parse().ok()?,
            primary: parts.next()? == "principal",
        })
    }
}

fn status_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join(STATUS_FILE))
}

/// Écrit la liste des écrans branchés (appelé par le bureau à chaque changement).
pub fn publish(screens: &[ScreenInfo]) {
    let Some(path) = status_path() else {
        return;
    };
    let text: String = screens.iter().map(|screen| screen.line() + "\n").collect();
    if let Err(err) = std::fs::write(&path, text) {
        eprintln!("liste des écrans non écrite ({}) : {err}", path.display());
    }
}

/// Écrans branchés, d'après le bureau ; vide si le bureau ne l'a pas (encore) publiée.
pub fn connected() -> Vec<ScreenInfo> {
    status_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| text.lines().filter_map(ScreenInfo::parse).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_ligne_relue_donne_le_meme_ecran() {
        let screen = ScreenInfo {
            name: "HDMI-A-1".into(),
            width: 1920,
            height: 1080,
            refresh: 60,
            primary: false,
        };
        assert_eq!(ScreenInfo::parse(&screen.line()), Some(screen.clone()));
        assert_eq!(screen.kind(), "HDMI");
        assert_eq!(ScreenInfo::parse("eDP-1 2880"), None);
    }

    #[test]
    fn dupliquer_par_defaut() {
        let mut settings = Settings::default();
        assert_eq!(ScreenMode::load(&settings), ScreenMode::Duplicate);
        ScreenMode::Extend.store(&mut settings);
        assert_eq!(ScreenMode::load(&settings), ScreenMode::Extend);
        assert_eq!(ScreenMode::Extend.toggled(), ScreenMode::Duplicate);
    }
}
