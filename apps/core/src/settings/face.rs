//! Confidentialité → Déverrouillage par visage : enregistrer son visage avec la caméra
//! infrarouge (`pkexec ferrite-visage enregistrer`), l'effacer, et activer le
//! déverrouillage de l'écran verrouillé (`visage_deverrouillage`).
//!
//! Ajouter ou effacer un visage demande le mot de passe administrateur (agent polkit du
//! bureau) : les empreintes sont dans `/var/lib/ferrite/visage`, que seul root écrit.

use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
};

use eframe::egui;
use ferrite_system::config::Settings;

use super::{
    Page, SettingsState,
    rows::{self, Lead, Row, group},
};

/// Étape d'un enregistrement, lue sur la sortie de `ferrite-visage`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// Mot de passe administrateur demandé (avant que la caméra s'allume).
    Asking,
    Progress(usize, usize),
    NoFace,
    Done,
    /// Visage effacé.
    Forgotten,
    Failed(String),
}

/// Ce que l'on demande à `ferrite-visage`, en administrateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Job {
    Enroll,
    Forget,
}

impl Job {
    fn arg(self) -> &'static str {
        match self {
            Self::Enroll => "enregistrer",
            Self::Forget => "effacer",
        }
    }
}

/// Réponse de `ferrite-visage etat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Enrolled {
    No,
    /// Enregistré avant que la caméra soit retenue : à refaire.
    Old,
    Yes,
}

#[derive(Debug, Default)]
pub struct FaceState {
    /// État du visage (lu une fois à l'ouverture de la page).
    enrolled: Option<Enrolled>,
    /// Enregistrement ou effacement en cours : ses étapes, et la dernière.
    steps: Option<Receiver<Step>>,
    job: Option<Job>,
    last: Option<Step>,
}

/// `ferrite-visage` : à côté de `ferrite-apps`, sinon dans le `PATH`.
fn program() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("ferrite-visage"))
        .filter(|path| path.is_file())
        .or_else(|| {
            // pkexec veut un chemin complet : on cherche dans le `PATH`.
            std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join("ferrite-visage"))
                    .find(|path| path.is_file())
            })
        })
        .unwrap_or_else(|| PathBuf::from("/usr/local/bin/ferrite-visage"))
}

/// Message pour un code de sortie de `pkexec ferrite-visage` qui n'a pas abouti.
fn failure(job: Job, code: Option<i32>) -> String {
    match (code, job) {
        // 126 : demande fermée ; 127 : mot de passe refusé (codes de pkexec).
        (Some(126 | 127), _) => "Mot de passe administrateur non donné : rien n’a changé.".to_owned(),
        (_, Job::Enroll) => "La caméra infrarouge n’a pas pu être utilisée.".to_owned(),
        (_, Job::Forget) => "Le visage n’a pas pu être effacé.".to_owned(),
    }
}

fn read_enrolled() -> Enrolled {
    let answer = Command::new(program())
        .arg("etat")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    match answer.as_str() {
        "enregistre" => Enrolled::Yes,
        "ancien" => Enrolled::Old,
        _ => Enrolled::No,
    }
}

fn parse(line: &str) -> Option<Step> {
    let line = line.trim();
    if let Some(progress) = line.strip_prefix("progression ") {
        let (done, total) = progress.split_once('/')?;
        return Some(Step::Progress(done.parse().ok()?, total.parse().ok()?));
    }
    match line {
        "visage introuvable" => Some(Step::NoFace),
        "termine" => Some(Step::Done),
        "echec" => Some(Step::Failed(
            "Votre visage n’a pas été vu assez longtemps. Regardez l’écran et recommencez.".to_owned(),
        )),
        _ => None,
    }
}

impl SettingsState {
    pub(super) fn privacy_page(&mut self, ui: &mut egui::Ui) {
        let face = &mut self.face;
        let state = *face.enrolled.get_or_insert_with(read_enrolled);
        let enrolled = state == Enrolled::Yes;
        // Étapes arrivées depuis le dernier dessin.
        let mut forgotten = false;
        if let Some(steps) = &face.steps {
            for step in steps.try_iter() {
                face.last = Some(step);
            }
            if matches!(face.last, Some(Step::Done | Step::Forgotten | Step::Failed(_))) {
                forgotten = face.last == Some(Step::Forgotten);
                face.steps = None;
                face.job = None;
                face.enrolled = Some(read_enrolled());
            } else {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        let enrolling = face.steps.is_some();
        let mut settings = Settings::load();
        // Visage effacé : plus rien pour déverrouiller.
        if forgotten {
            settings.set("visage_deverrouillage", "non");
        }
        let mut unlock = settings
            .get("visage_deverrouillage")
            .is_some_and(|value| value == "oui");
        let mut start = false;
        let mut forget = false;
        group(ui, &mut self.icons, "Déverrouillage par visage", |rows| {
            let (status, hint) = match (&face.last, enrolling, state) {
                (Some(Step::Asking) | None, true, _) => (
                    "Mot de passe administrateur demandé…".to_owned(),
                    "Ajouter ou effacer un visage demande ce mot de passe",
                ),
                (Some(Step::Progress(done, total)), true, _) => (
                    format!("Enregistrement… {done} sur {total}"),
                    "Regardez l’écran, sans bouger",
                ),
                (Some(Step::NoFace), true, _) => (
                    "Je ne vois pas votre visage".to_owned(),
                    "Placez-vous face à l’écran",
                ),
                (_, true, _) if face.job == Some(Job::Forget) => ("Effacement…".to_owned(), "Un instant"),
                (_, true, _) => ("Allumage de la caméra…".to_owned(), "Regardez l’écran"),
                (_, false, Enrolled::Yes) => (
                    "Visage enregistré".to_owned(),
                    "Une empreinte, jamais une photo. Le modifier demande le mot de passe administrateur",
                ),
                (_, false, Enrolled::Old) => (
                    "Visage à enregistrer de nouveau".to_owned(),
                    "La caméra utilisée est désormais retenue, pour plus de sûreté",
                ),
                (_, false, Enrolled::No) => (
                    "Aucun visage enregistré".to_owned(),
                    "Avec la caméra infrarouge de l’ordinateur",
                ),
            };
            rows.row(
                Row::new(&status)
                    .hint(hint)
                    .lead(Lead::Tile("glyph-account", Page::Privacy.color())),
                |ui| {
                    if enrolling {
                        ui.add(egui::Spinner::new().size(16.0));
                        return;
                    }
                    if state != Enrolled::No && rows::button(ui, "Effacer", false).clicked() {
                        forget = true;
                    }
                    let label = if state != Enrolled::No {
                        "Recommencer"
                    } else {
                        "Enregistrer mon visage"
                    };
                    if rows::button(ui, label, true).clicked() {
                        start = true;
                    }
                },
            );
            let (changed, _) = rows.row(
                Row::new("Déverrouiller l’écran avec mon visage")
                    .hint("Le mot de passe reste demandé au premier déverrouillage de la session"),
                |ui| {
                    ui.add_enabled_ui(enrolled && !enrolling, |ui| rows::switch(ui, &mut unlock))
                        .inner
                },
            );
            if changed {
                settings.set("visage_deverrouillage", if unlock { "oui" } else { "non" });
            }
        });
        if let Some(Step::Failed(message)) = &face.last {
            ui.colored_label(crate::style::error(), message);
        }
        ui.add_space(8.0);
        crate::kit::note(
            ui,
            "Le visage ne sert qu’à l’écran verrouillé, jamais pour les droits administrateur. \
             Une photo sur un téléphone ou un écran ne suffit pas : la caméra infrarouge ne \
             les voit pas. Une photo imprimée sur papier peut en revanche tromper la caméra. \
             Si quelqu’un pourrait chercher à entrer, gardez le mot de passe seul. \
             Ajouter ou effacer un visage demande le mot de passe administrateur.",
        );
        for (wanted, job) in [(start, Job::Enroll), (forget, Job::Forget)] {
            if wanted {
                self.face.last = Some(Step::Asking);
                self.face.job = Some(job);
                self.face.steps = Some(run(job, ui.ctx().clone()));
            }
        }
        if settings != Settings::load() {
            self.save(&settings);
        }
    }
}

/// Lance `pkexec ferrite-visage enregistrer` (ou `effacer`) et relaie ses étapes ; le
/// mot de passe administrateur est demandé par l'agent polkit du bureau.
fn run(job: Job, ctx: egui::Context) -> Receiver<Step> {
    let (sender, steps) = mpsc::channel();
    std::thread::spawn(move || {
        let child = Command::new("pkexec")
            .arg(program())
            .arg(job.arg())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                let _ = sender.send(Step::Failed(format!("pkexec introuvable : {error}")));
                ctx.request_repaint();
                return;
            }
        };
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(step) = parse(&line) {
                    let _ = sender.send(step);
                    ctx.request_repaint();
                }
            }
        }
        let code = child.wait().ok().and_then(|status| status.code());
        let _ = sender.send(match (code, job) {
            (Some(0), Job::Enroll) => Step::Done,
            (Some(0), Job::Forget) => Step::Forgotten,
            // Visage pas vu assez longtemps : le message est déjà arrivé (« echec »).
            (Some(1), Job::Enroll) => Step::Failed(
                "Votre visage n’a pas été vu assez longtemps. Regardez l’écran et recommencez.".to_owned(),
            ),
            _ => Step::Failed(failure(job, code)),
        });
        ctx.request_repaint();
    });
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_les_etapes_de_ferrite_visage() {
        assert_eq!(parse("progression 3/8"), Some(Step::Progress(3, 8)));
        assert_eq!(parse("visage introuvable"), Some(Step::NoFace));
        assert_eq!(parse("termine"), Some(Step::Done));
        assert!(matches!(parse("echec"), Some(Step::Failed(_))));
        assert_eq!(parse("autre chose"), None);
    }

    #[test]
    fn refus_du_mot_de_passe_administrateur() {
        assert!(failure(Job::Enroll, Some(126)).contains("administrateur"));
        assert!(failure(Job::Forget, Some(127)).contains("administrateur"));
        assert!(failure(Job::Forget, Some(3)).contains("effacé"));
    }
}
