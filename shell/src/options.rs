//! Options de la ligne de commande.

use std::{path::PathBuf, time::Duration};

use crate::desktop::panels::Popover;

pub const USAGE: &str = "\
Utilisation : ferrite-shell [OPTIONS] [-- COMMANDE [ARGUMENTS...]]

Options :
  --winit               dans une fenêtre (développement, WSLg)
  --drm                 directement sur l'écran (session sur un terminal virtuel)
                        Sans l'une ni l'autre : fenêtre si un affichage existe déjà
                        (WAYLAND_DISPLAY ou DISPLAY), écran sinon.
  --scale FACTEUR       échelle de l'écran (défaut : 1 en fenêtre, selon la densité
                        de l'écran sinon, par exemple 2 sur le ThinkBook)
  --capture FICHIER     en fenêtre : enregistre une capture PNG puis quitte
  --capture-delay SEC   délai avant la capture (défaut : 3)
  --menu MENU           ouvre un menu au démarrage, pour les captures
                        (applications, reglages, session ou presse-papiers)
  -h, --help            affiche cette aide

COMMANDE est lancée comme client une fois le compositeur prêt (ex. -- foot).";

/// Backend d'affichage demandé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendChoice {
    /// Fenêtre si un affichage existe déjà, écran sinon.
    #[default]
    Auto,
    Winit,
    Drm,
}

impl BackendChoice {
    /// Choix effectif selon l'environnement (`has_display` : WAYLAND_DISPLAY ou DISPLAY).
    pub fn resolve(self, has_display: bool) -> BackendChoice {
        match self {
            BackendChoice::Auto if has_display => BackendChoice::Winit,
            BackendChoice::Auto => BackendChoice::Drm,
            other => other,
        }
    }
}

/// Options du compositeur.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub backend: BackendChoice,
    /// Échelle imposée ; `None` : choisie par le backend.
    pub scale: Option<f64>,
    pub capture: Option<PathBuf>,
    pub capture_delay: Duration,
    /// Menu ouvert au démarrage.
    pub menu: Option<Popover>,
    pub command: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            backend: BackendChoice::Auto,
            scale: None,
            capture: None,
            capture_delay: Duration::from_secs(3),
            menu: None,
            command: Vec::new(),
        }
    }
}

/// Résultat de l'analyse : options à utiliser ou demande d'aide.
#[derive(Debug, PartialEq)]
pub enum Parsed {
    Run(Options),
    Help,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} attend une valeur"));
        match arg.as_str() {
            "--winit" => options.backend = BackendChoice::Winit,
            "--drm" => options.backend = BackendChoice::Drm,
            "--scale" => {
                let text = value("--scale")?;
                options.scale = match text.parse::<f64>() {
                    Ok(scale) if scale.is_finite() && (0.5..=4.0).contains(&scale) => Some(scale),
                    _ => return Err(format!("échelle invalide : {text} (entre 0,5 et 4)")),
                };
            }
            "--capture" => options.capture = Some(PathBuf::from(value("--capture")?)),
            "--capture-delay" => {
                let text = value("--capture-delay")?;
                let seconds = text
                    .parse::<f64>()
                    .ok()
                    .filter(|s| s.is_finite() && *s >= 0.0)
                    .ok_or_else(|| format!("délai invalide : {text}"))?;
                options.capture_delay = Duration::from_secs_f64(seconds);
            }
            "--menu" => {
                options.menu = Some(match value("--menu")?.as_str() {
                    "applications" => Popover::Launcher,
                    "reglages" => Popover::Controls,
                    "session" => Popover::Session,
                    "presse-papiers" => Popover::Clipboard,
                    other => return Err(format!("menu inconnu : {other}")),
                });
            }
            "-h" | "--help" => return Ok(Parsed::Help),
            "--" => {
                options.command = args.collect();
                break;
            }
            other => return Err(format!("option inconnue : {other}")),
        }
    }
    Ok(Parsed::Run(options))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Parsed, String> {
        parse(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn options_par_defaut() {
        assert_eq!(run(&[]), Ok(Parsed::Run(Options::default())));
        let Ok(Parsed::Run(options)) = run(&["--drm"]) else {
            panic!("analyse échouée");
        };
        assert_eq!(options.backend, BackendChoice::Drm);
    }

    #[test]
    fn options_completes() {
        let Ok(Parsed::Run(options)) = run(&[
            "--winit",
            "--scale",
            "1.75",
            "--capture",
            "a.png",
            "--capture-delay",
            "0.5",
            "--",
            "foot",
            "-e",
            "top",
        ]) else {
            panic!("analyse échouée");
        };
        assert_eq!(options.backend, BackendChoice::Winit);
        assert_eq!(options.scale, Some(1.75));
        assert_eq!(options.capture, Some(PathBuf::from("a.png")));
        assert_eq!(options.capture_delay, Duration::from_millis(500));
        assert_eq!(options.command, ["foot", "-e", "top"]);
    }

    #[test]
    fn choix_du_backend() {
        assert_eq!(BackendChoice::Auto.resolve(true), BackendChoice::Winit);
        assert_eq!(BackendChoice::Auto.resolve(false), BackendChoice::Drm);
        assert_eq!(BackendChoice::Drm.resolve(true), BackendChoice::Drm);
        assert_eq!(BackendChoice::Winit.resolve(false), BackendChoice::Winit);
    }

    #[test]
    fn erreurs() {
        assert_eq!(run(&["--help"]), Ok(Parsed::Help));
        assert!(run(&["--scale"]).is_err());
        assert!(run(&["--scale", "9"]).is_err());
        assert!(run(&["--scale", "NaN"]).is_err());
        assert!(run(&["--capture-delay", "-1"]).is_err());
        assert!(run(&["--x11"]).is_err());
    }
}
