//! Confidentialité → Déverrouillage par visage : enregistrer son visage avec la caméra
//! infrarouge (`ferrite-visage enregistrer`), l'effacer, et activer le déverrouillage de
//! l'écran verrouillé (`visage_deverrouillage`).

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
    rows::{self, Lead, Row, group, hero},
};

/// Étape d'un enregistrement, lue sur la sortie de `ferrite-visage`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Progress(usize, usize),
    NoFace,
    Done,
    Failed(String),
}

#[derive(Debug, Default)]
pub struct FaceState {
    /// Visage enregistré (lu une fois à l'ouverture de la page).
    enrolled: Option<bool>,
    /// Enregistrement en cours : ses étapes, et la dernière.
    steps: Option<Receiver<Step>>,
    last: Option<Step>,
}

/// `ferrite-visage` : à côté de `ferrite-apps`, sinon dans le `PATH`.
fn program() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("ferrite-visage"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("ferrite-visage"))
}

fn read_enrolled() -> bool {
    Command::new(program())
        .arg("etat")
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "enregistre")
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
        hero(
            ui,
            &mut self.icons,
            (Page::Privacy.icon(), Page::Privacy.color()),
            "Confidentialité",
            "Déverrouillage par visage avec la caméra infrarouge",
            None,
        );
        ui.add_space(20.0);
        let face = &mut self.face;
        let enrolled = *face.enrolled.get_or_insert_with(read_enrolled);
        // Étapes arrivées depuis le dernier dessin.
        if let Some(steps) = &face.steps {
            for step in steps.try_iter() {
                face.last = Some(step);
            }
            if matches!(face.last, Some(Step::Done | Step::Failed(_))) {
                face.steps = None;
                face.enrolled = Some(read_enrolled());
            } else {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        let enrolling = face.steps.is_some();
        let mut settings = Settings::load();
        let mut unlock = settings
            .get("visage_deverrouillage")
            .is_some_and(|value| value == "oui");
        let mut start = false;
        let mut forget = false;
        group(ui, &mut self.icons, "Déverrouillage par visage", |rows| {
            let (status, hint) = match (&face.last, enrolling, enrolled) {
                (Some(Step::Progress(done, total)), true, _) => (
                    format!("Enregistrement… {done} sur {total}"),
                    "Regardez l’écran, sans bouger",
                ),
                (Some(Step::NoFace), true, _) => (
                    "Je ne vois pas votre visage".to_owned(),
                    "Placez-vous face à l’écran",
                ),
                (_, true, _) => ("Allumage de la caméra…".to_owned(), "Regardez l’écran"),
                (_, false, true) => (
                    "Visage enregistré".to_owned(),
                    "Seule une empreinte est gardée, jamais une photo",
                ),
                (_, false, false) => (
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
                    if enrolled && rows::button(ui, "Effacer", false).clicked() {
                        forget = true;
                    }
                    let label = if enrolled {
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
             Une photo ne suffit pas : la caméra infrarouge ne voit pas les écrans.",
        );
        if start {
            self.face.last = None;
            self.face.steps = Some(enroll(ui.ctx().clone()));
        }
        if forget {
            let _ = Command::new(program()).arg("effacer").status();
            settings.set("visage_deverrouillage", "non");
            self.face.enrolled = Some(false);
            self.face.last = None;
        }
        if settings != Settings::load() {
            self.save(&settings);
        }
    }
}

/// Lance `ferrite-visage enregistrer` et relaie ses étapes.
fn enroll(ctx: egui::Context) -> Receiver<Step> {
    let (sender, steps) = mpsc::channel();
    std::thread::spawn(move || {
        let child = Command::new(program())
            .arg("enregistrer")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                let _ = sender.send(Step::Failed(format!("ferrite-visage introuvable : {error}")));
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
        let success = child.wait().is_ok_and(|status| status.success());
        let _ = sender.send(if success {
            Step::Done
        } else {
            Step::Failed("La caméra infrarouge n’a pas pu être utilisée.".to_owned())
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
}
