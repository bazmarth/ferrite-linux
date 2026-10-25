//! Options de la ligne de commande.

use std::{path::PathBuf, time::Duration};

pub const USAGE: &str = "\
Utilisation : ferrite-shell [OPTIONS] [-- COMMANDE [ARGUMENTS...]]

Options :
  --winit               dans une fenêtre (développement, WSLg) — seul mode actuel
  --scale FACTEUR       échelle de l'écran (défaut : 1 en fenêtre)
  --capture FICHIER     enregistre une capture PNG puis quitte
  --capture-delay SEC   délai avant la capture (défaut : 3)
  -h, --help            affiche cette aide

COMMANDE est lancée comme client une fois le compositeur prêt (ex. -- foot).";

/// Options du compositeur.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub scale: f64,
    pub capture: Option<PathBuf>,
    pub capture_delay: Duration,
    pub command: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            scale: 1.0,
            capture: None,
            capture_delay: Duration::from_secs(3),
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
            "--winit" => {}
            "--scale" => {
                let text = value("--scale")?;
                options.scale = match text.parse::<f64>() {
                    Ok(scale) if scale.is_finite() && (0.5..=4.0).contains(&scale) => scale,
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
        assert_eq!(run(&["--winit"]), Ok(Parsed::Run(Options::default())));
    }

    #[test]
    fn options_completes() {
        let Ok(Parsed::Run(options)) = run(&[
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
        assert_eq!(options.scale, 1.75);
        assert_eq!(options.capture, Some(PathBuf::from("a.png")));
        assert_eq!(options.capture_delay, Duration::from_millis(500));
        assert_eq!(options.command, ["foot", "-e", "top"]);
    }

    #[test]
    fn erreurs() {
        assert_eq!(run(&["--help"]), Ok(Parsed::Help));
        assert!(run(&["--scale"]).is_err());
        assert!(run(&["--scale", "9"]).is_err());
        assert!(run(&["--scale", "NaN"]).is_err());
        assert!(run(&["--capture-delay", "-1"]).is_err());
        assert!(run(&["--drm"]).is_err());
    }
}
