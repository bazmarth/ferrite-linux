//! Système → Applications par défaut : l'application qui ouvre les pages web, les
//! courriels, les dossiers, les images… (`ferrite_system::defaults`, `mimeapps.list`).

use eframe::egui;
use ferrite_system::{
    applications::{self, DesktopEntry},
    defaults::{self, CATEGORIES},
};

use super::{
    SettingsState,
    rows::{self, Row, group},
};

impl SettingsState {
    pub(super) fn defaults_page(&mut self, ui: &mut egui::Ui) {
        let installed: &Vec<DesktopEntry> = self.installed_apps.get_or_insert_with(applications::installed);
        let mut chosen: Option<(usize, String)> = None;
        group(ui, &mut self.icons, "Ouvrir avec", |rows| {
            for (index, category) in CATEGORIES.iter().enumerate() {
                let choices = defaults::candidates(category, installed);
                let current = defaults::current(category);
                let mut options: Vec<(usize, String)> = choices
                    .iter()
                    .enumerate()
                    .map(|(position, choice)| (position, choice.name.clone()))
                    .collect();
                // Sans choix enregistré, c'est la première application capable (celle de
                // Ferrite en tête) qui s'ouvre : elle est montrée. Un choix absent de la
                // liste (application désinstallée) est affiché tel quel.
                let mut selected = match &current {
                    Some(id) => choices.iter().position(|choice| &choice.id == id),
                    None => (!choices.is_empty()).then_some(0),
                };
                if selected.is_none() {
                    let label = current.as_deref().map_or_else(
                        || "Aucune".to_owned(),
                        |id| id.trim_end_matches(".desktop").to_owned(),
                    );
                    options.push((usize::MAX, label));
                    selected = Some(usize::MAX);
                }
                let labels: Vec<(usize, &str)> = options
                    .iter()
                    .map(|(key, label)| (*key, label.as_str()))
                    .collect();
                let mut value = selected.unwrap_or(usize::MAX);
                let (changed, _) = rows.row(Row::new(category.label), |ui| {
                    rows::pick(ui, category.label, &mut value, &labels)
                });
                if changed && let Some(choice) = choices.get(value) {
                    chosen = Some((index, choice.id.clone()));
                }
            }
        });
        if let Some((index, id)) = chosen {
            self.message = Some(
                defaults::set(&CATEGORIES[index], &id)
                    .map(|()| "Application par défaut enregistrée".to_owned())
                    .map_err(|error| format!("Enregistrement impossible : {error}")),
            );
        }
        ui.add_space(8.0);
        crate::kit::note(
            ui,
            "Ces choix valent pour Fichiers et pour toutes les applications (liens, pièces jointes, téléchargements).",
        );
    }
}
