//! Options de la ligne de commande.

use std::{path::PathBuf, time::Duration};

use crate::desktop::panels::{ControlsPage, Popover};

pub const USAGE: &str = "\
Utilisation : ferrite-shell [OPTIONS] [-- COMMANDE [ARGUMENTS...]]

Options :
  --winit               dans une fenêtre (développement, WSLg)
  --drm                 directement sur l'écran (session sur un terminal virtuel)
                        Sans l'une ni l'autre : fenêtre si un affichage existe déjà
                        (WAYLAND_DISPLAY ou DISPLAY), écran sinon.
  --scale FACTEUR       échelle de l'écran (défaut : 2 en fenêtre, selon la densité
                        de l'écran sinon, par exemple 2 sur le ThinkBook)
  --capture FICHIER     en fenêtre : enregistre une capture PNG puis quitte
  --capture-delay SEC   délai avant la capture (défaut : 3)
  --menu MENU           ouvre un menu au démarrage, pour les captures
                        (applications, reglages, presse-papiers ou notifications)
  --demo-genie FRACTION réduit la première fenêtre ouverte et fige l'animation
                        « génie » à cette fraction (0 à 1), pour les captures
  --demo-swap FRACTION  en tuilage, échange les deux premières fenêtres et fige le
                        glissement à cette fraction (0 à 1), pour les captures
  --demo-pointer X,Y    pointeur figé à cette place (pixels logiques) pour le zoom de
                        l'écran (réglage « loupe »), pour les captures
  --demo-overview       ouvre la vue d'ensemble au démarrage (pour les captures)
  --demo-switcher       ouvre le sélecteur Alt+Tab dès qu'une fenêtre existe (pour les captures)
  --demo-keep-above     garde la première fenêtre ouverte au-dessus des autres (pour les captures)
  --demo-snap           ouvre le panneau « Ancrer la fenêtre » de la première fenêtre (captures)
  --demo-controls LISTE ouvre le centre de contrôle sur sa liste (principal, wifi ou bluetooth)
  --greeter             écran de connexion (lancé par greetd : choix du compte, mot de passe)
  --demo-lock           affiche l'écran de verrouillage au démarrage (pour les captures)
  --demo-face ÉTAT      avec --demo-lock : visage en cours (scan), reconnu ou echec
  --demo-power          ouvre le dialogue d'arrêt au démarrage (pour les captures)
  --demo-auth           affiche une demande de mot de passe administrateur (pour les captures)
  --demo-osd            affiche la bulle du volume au démarrage (pour les captures)
  --demo-screens        simule un second écran branché (pour les captures)
  --demo-selection MODE icônes du bureau sélectionnées : groupe (deux icônes en cours de
                        déplacement) ou rectangle (rectangle de sélection tiré)
  --demo-region         ouvre la sélection d'une zone à capturer au démarrage
  --demo-capture        ouvre le panneau de capture au démarrage
  --demo-video          ouvre le panneau de capture en mode vidéo, avec la pastille d'arrêt
  --demo-island         garde l'îlot du haut déplié (minuteur ou musique en cours)
  --demo-island-press S avec --demo-island : appuie sur lecture/pause de l'îlot après S secondes
  --demo-screenshot     enregistre une capture dans le dossier Images dès qu'une fenêtre existe
  --demo-page POSITION  fige l'affichage à cette position entre les bureaux (0 : le premier,
                        0.5 : à mi-chemin), pour les captures
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
    /// Animation « génie » figée à cette fraction sur la première fenêtre.
    pub genie_demo: Option<f64>,
    /// Échange de places en tuilage figé à cette fraction.
    pub swap_demo: Option<f64>,
    /// Position figée entre les pages.
    pub demo_page: Option<f64>,
    /// Ouvre la vue d'ensemble au démarrage (pour les captures).
    pub demo_overview: bool,
    /// `--demo-pointer` : place du pointeur que suit le zoom de l'écran.
    pub demo_pointer: Option<(f64, f64)>,
    /// Sélecteur Alt+Tab, sélection d'une zone et capture demandés d'avance.
    pub demo_switcher: bool,
    /// Première fenêtre gardée au-dessus des autres (`--demo-keep-above`).
    pub demo_keep_above: bool,
    /// Panneau d'ancrage de la première fenêtre ouvert (`--demo-snap`).
    pub demo_snap: bool,
    pub demo_region: bool,
    pub demo_lock: bool,
    /// Écran de connexion (`--greeter`), lancé par greetd.
    pub greeter: bool,
    /// Déverrouillage par visage figé sur l'écran verrouillé (`--demo-face`).
    pub demo_face: Option<crate::desktop::panels::LockFace>,
    pub demo_auth: bool,
    /// Ouvre le dialogue d'arrêt au démarrage (`--demo-power`).
    pub demo_power: bool,
    /// Bulle du volume affichée au démarrage (`--demo-osd`).
    pub demo_osd: bool,
    /// Simule un second écran branché (`--demo-screens`) : tuile du centre de contrôle et
    /// bulle « Écran branché ».
    pub demo_screens: bool,
    /// Sélection des icônes du bureau figée (`--demo-selection groupe|rectangle`).
    pub demo_selection: Option<String>,
    pub demo_controls: Option<ControlsPage>,
    pub demo_screenshot: bool,
    /// Ouvre le panneau de capture au démarrage (`--demo-capture`).
    pub demo_capture: bool,
    /// Panneau de capture en mode vidéo et pastille d'enregistrement (`--demo-video`).
    pub demo_video: bool,
    /// Îlot du haut gardé déplié (`--demo-island`).
    pub demo_island: bool,
    /// Appui sur lecture/pause de l'îlot déplié après ce délai (`--demo-island-press`).
    pub demo_island_press: Option<f64>,
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
            genie_demo: None,
            swap_demo: None,
            demo_page: None,
            demo_overview: false,
            demo_pointer: None,
            demo_switcher: false,
            demo_keep_above: false,
            demo_snap: false,
            demo_region: false,
            demo_lock: false,
            greeter: false,
            demo_face: None,
            demo_auth: false,
            demo_power: false,
            demo_osd: false,
            demo_screens: false,
            demo_selection: None,
            demo_controls: None,
            demo_screenshot: false,
            demo_capture: false,
            demo_video: false,
            demo_island: false,
            demo_island_press: None,
            command: Vec::new(),
        }
    }
}

/// Résultat de l'analyse : options à utiliser ou demande d'aide (lu une seule fois).
#[derive(Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
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
                    "presse-papiers" => Popover::Clipboard,
                    "notifications" => Popover::Notifications,
                    other => return Err(format!("menu inconnu : {other}")),
                });
            }
            "--demo-genie" => {
                let text = value("--demo-genie")?;
                options.genie_demo = Some(
                    text.parse::<f64>()
                        .ok()
                        .filter(|fraction| (0.0..=1.0).contains(fraction))
                        .ok_or_else(|| format!("fraction invalide : {text} (entre 0 et 1)"))?,
                );
            }
            "--demo-swap" => {
                let text = value("--demo-swap")?;
                options.swap_demo = Some(
                    text.parse::<f64>()
                        .ok()
                        .filter(|fraction| (0.0..=1.0).contains(fraction))
                        .ok_or_else(|| format!("fraction invalide : {text} (entre 0 et 1)"))?,
                );
            }
            "--demo-pointer" => {
                let text = value("--demo-pointer")?;
                options.demo_pointer = Some(
                    text.split_once(',')
                        .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
                        .ok_or_else(|| format!("place invalide : {text} (X,Y)"))?,
                );
            }
            "--demo-overview" => options.demo_overview = true,
            "--demo-switcher" => options.demo_switcher = true,
            "--demo-keep-above" => options.demo_keep_above = true,
            "--demo-snap" => options.demo_snap = true,
            "--demo-region" => options.demo_region = true,
            "--demo-capture" => options.demo_capture = true,
            "--demo-video" => options.demo_video = true,
            "--demo-island" => options.demo_island = true,
            "--demo-lock" => options.demo_lock = true,
            "--greeter" => options.greeter = true,
            "--demo-face" => {
                use crate::desktop::panels::LockFace;
                let text = value("--demo-face")?;
                options.demo_face = Some(match text.as_str() {
                    "scan" => LockFace::Scanning,
                    "reconnu" => LockFace::Recognized,
                    "echec" => LockFace::Failed,
                    _ => return Err(format!("état invalide : {text} (scan, reconnu ou echec)")),
                });
            }
            "--demo-power" => options.demo_power = true,
            "--demo-auth" => options.demo_auth = true,
            "--demo-osd" => options.demo_osd = true,
            "--demo-screens" => options.demo_screens = true,
            "--demo-selection" => options.demo_selection = Some(value("--demo-selection")?),
            "--demo-controls" => {
                options.demo_controls = Some(match value("--demo-controls")?.as_str() {
                    "principal" => ControlsPage::Main,
                    "wifi" => ControlsPage::Wifi,
                    "bluetooth" => ControlsPage::Bluetooth,
                    other => return Err(format!("liste inconnue : {other}")),
                });
            }
            "--demo-screenshot" => options.demo_screenshot = true,
            "--demo-island-press" => {
                let text = value("--demo-island-press")?;
                options.demo_island_press = Some(
                    text.parse::<f64>()
                        .ok()
                        .filter(|delay| (0.0..=60.0).contains(delay))
                        .ok_or_else(|| format!("délai invalide : {text} (entre 0 et 60 s)"))?,
                );
            }
            "--demo-page" => {
                let text = value("--demo-page")?;
                options.demo_page = Some(
                    text.parse::<f64>()
                        .ok()
                        .filter(|position| (0.0..=16.0).contains(position))
                        .ok_or_else(|| format!("position invalide : {text} (entre 0 et 16)"))?,
                );
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
