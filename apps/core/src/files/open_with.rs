//! Dialogue « Ouvrir avec… » : les applications capables d'ouvrir le fichier en tête,
//! chacune avec son logo, puis toutes les autres.

use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
};

use eframe::egui::{self, Color32, FontId, RichText, TextureHandle};
use ferrite_system::{
    applications::DesktopEntry,
    catalog, defaults,
    formats::{self, Builtin},
};

use super::{Action, Dialog, FilesState, file_name, path_icon, thumbs};
use crate::style::{self, IconStyle, Icons};

/// Côté des logos dans la liste.
const LOGO: f32 = 28.0;
/// Hauteur d'une ligne.
const ROW: f32 = 40.0;

#[derive(Debug)]
pub(super) struct OpenWith {
    path: PathBuf,
    /// Applications qui déclarent savoir ouvrir ce type de fichier.
    suggested: Vec<DesktopEntry>,
    others: Vec<DesktopEntry>,
}

impl OpenWith {
    pub(super) fn new(path: PathBuf, installed: Vec<DesktopEntry>) -> Self {
        let mime = if path.is_dir() {
            Some("inode/directory".to_owned())
        } else {
            defaults::mime_of(&path)
        };
        // Tous les noms du type : « video/x-msvideo » annoncé par une application vaut
        // pour un « .avi » (« video/vnd.avi »).
        let mimes: Vec<String> = match formats::format_of(&path).filter(|_| !path.is_dir()) {
            Some(format) => format.mimes.iter().map(|mime| (*mime).to_owned()).collect(),
            None => mime.iter().cloned().collect(),
        };
        let (mut suggested, mut others): (Vec<_>, Vec<_>) = installed
            .into_iter()
            .partition(|app| app.mime_types.iter().any(|declared| mimes.contains(declared)));
        // L'application de Ferrite de ce format en tête (ses `.desktop` sont cachés des
        // menus, donc absents de `installed`) ; TextEdit pour un texte ou un fichier sans
        // type connu, sinon en tête des autres.
        if !path.is_dir() {
            let own =
                formats::builtin_for(&path).or_else(|| is_text(mime.as_deref()).then_some(Builtin::TextEdit));
            if let Some(app) = own {
                suggested.insert(0, ferrite_entry(app));
            }
            if own != Some(Builtin::TextEdit) {
                others.insert(0, ferrite_entry(Builtin::TextEdit));
            }
        }
        Self {
            path,
            suggested,
            others,
        }
    }
}

/// Fichier texte, ou de type inconnu (sans extension) : TextEdit est proposé en tête.
fn is_text(mime: Option<&str>) -> bool {
    mime.is_none_or(|mime| {
        mime.starts_with("text/")
            || matches!(
                mime,
                "application/json" | "application/xml" | "application/x-shellscript" | "application/toml"
            )
    })
}

/// Application de Ferrite, lancée par le même exécutable que Fichiers.
fn ferrite_entry(app: Builtin) -> DesktopEntry {
    DesktopEntry {
        id: app.desktop_id().trim_end_matches(".desktop").into(),
        name: app.name().into(),
        exec: vec![
            env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            app.command().into(),
        ],
        terminal: false,
        icon: None,
        wm_class: None,
        categories: Vec::new(),
        mime_types: Vec::new(),
    }
}

/// Logos des applications, lus une fois par fichier d'icône.
#[derive(Default)]
pub(super) struct AppLogos {
    textures: HashMap<PathBuf, Option<TextureHandle>>,
}

impl std::fmt::Debug for AppLogos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppLogos")
            .field("textures", &self.textures.len())
            .finish()
    }
}

impl AppLogos {
    fn texture(&mut self, ctx: &egui::Context, file: &Path) -> Option<TextureHandle> {
        self.textures
            .entry(file.to_path_buf())
            .or_insert_with(|| {
                let image = thumbs::decode(file)?;
                Some(ctx.load_texture(file.display().to_string(), image, egui::TextureOptions::LINEAR))
            })
            .clone()
    }

    /// Logo de `app` dans `rect` : celui de son thème, sinon une pastille à son initiale.
    fn paint(&mut self, ui: &egui::Ui, icons: &mut Icons, app: &DesktopEntry, rect: egui::Rect) {
        if let Some(own) = catalog::by_app_id(&app.id) {
            icons.paint(ui, own.icon, rect, IconStyle::Color);
            return;
        }
        if let Some(texture) = app.icon.as_deref().and_then(|file| self.texture(ui.ctx(), file)) {
            // Logo pas carré : centré, proportions gardées.
            let size = texture.size_vec2();
            let scale = rect.width() / size.x.max(size.y).max(1.0);
            let fitted = egui::Rect::from_center_size(rect.center(), size * scale);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(texture.id(), fitted, uv, Color32::WHITE);
            return;
        }
        let tile = rect.shrink(rect.width() * 0.04);
        ui.painter()
            .rect_filled(tile, tile.width() * 0.26, style::accent());
        let initial = app
            .name
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default();
        ui.painter().text(
            tile.center(),
            egui::Align2::CENTER_CENTER,
            initial,
            FontId::new(tile.height() * 0.52, style::strong_family()),
            style::accent_text(),
        );
    }
}

/// Une ligne de la liste : logo, nom, fond au survol.
fn app_row(ui: &mut egui::Ui, icons: &mut Icons, logos: &mut AppLogos, app: &DesktopEntry) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 8.0, style::tint(18));
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 8.0, style::tint(10));
    }
    let logo = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + LOGO / 2.0, rect.center().y),
        egui::Vec2::splat(LOGO),
    );
    logos.paint(ui, icons, app, logo);
    let text_left = logo.right() + 12.0;
    let galley = ui.painter().layout(
        app.name.clone(),
        FontId::proportional(14.0),
        style::primary_text(),
        rect.right() - text_left - 8.0,
    );
    let galley = if galley.rows.len() > 1 {
        // Nom trop long : une seule ligne, coupée.
        let mut job = egui::text::LayoutJob::simple_singleline(
            app.name.clone(),
            FontId::proportional(14.0),
            style::primary_text(),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.right() - text_left - 8.0);
        ui.painter().layout_job(job)
    } else {
        galley
    };
    ui.painter().galley(
        egui::pos2(text_left, rect.center().y - galley.size().y / 2.0),
        galley,
        style::primary_text(),
    );
    response
}

impl FilesState {
    pub(super) fn open_with_dialog(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(Dialog::OpenWith(choice)) = &self.dialog else {
            return;
        };
        let icons = &mut self.icons;
        let logos = &mut self.app_logos;
        let modal = egui::Modal::new(egui::Id::unique("fichiers-ouvrir-avec")).show(ctx, |ui| {
            ui.set_width(380.0);
            // En-tête : icône du fichier, titre, nom du fichier.
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(40.0), egui::Sense::hover());
                icons.paint(
                    ui,
                    path_icon(&choice.path, choice.path.is_dir()),
                    rect,
                    IconStyle::Color,
                );
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.add_space(1.0);
                    ui.label(
                        RichText::new("Ouvrir avec")
                            .font(FontId::new(17.0, style::strong_family()))
                            .color(style::heading_text()),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(file_name(&choice.path)).color(style::text_secondary()),
                        )
                        .truncate(),
                    );
                });
            });
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(340.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let sections = [
                        ("Suggérées", &choice.suggested),
                        (
                            if choice.suggested.is_empty() {
                                "Applications"
                            } else {
                                "Autres applications"
                            },
                            &choice.others,
                        ),
                    ];
                    for (title, apps) in sections {
                        if apps.is_empty() {
                            continue;
                        }
                        style::sidebar_heading(ui, title);
                        for app in apps {
                            if app_row(ui, icons, logos, app).clicked() {
                                actions.push(Action::LaunchWith(choice.path.clone(), app.clone()));
                            }
                        }
                        ui.add_space(6.0);
                    }
                    if choice.suggested.is_empty() && choice.others.is_empty() {
                        ui.label(
                            RichText::new("Aucune application installée.").color(style::text_secondary()),
                        );
                    }
                });
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Annuler").clicked() {
                    actions.push(Action::CloseDialog);
                }
            });
        });
        if modal.should_close() {
            actions.push(Action::CloseDialog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, mimes: &[&str]) -> DesktopEntry {
        DesktopEntry {
            id: id.into(),
            name: id.into(),
            exec: vec![id.into()],
            terminal: false,
            icon: None,
            wm_class: None,
            categories: Vec::new(),
            mime_types: mimes.iter().map(|mime| (*mime).to_owned()).collect(),
        }
    }

    #[test]
    fn un_fichier_sans_extension_propose_textedit_en_tete() {
        let choice = OpenWith::new(
            PathBuf::from("/nexiste/pas/deepseek-code"),
            vec![app("brave", &["text/html"]), app("gparted", &[])],
        );
        assert_eq!(
            choice.suggested.first().map(|app| app.id.as_str()),
            Some("ferrite-textedit")
        );
        assert_eq!(choice.others.len(), 2);
    }

    #[test]
    fn l_application_de_ferrite_du_format_en_tete() {
        let choice = OpenWith::new(
            PathBuf::from("/nexiste/pas/film.avi"),
            vec![app("vlc", &["video/x-msvideo"]), app("gimp", &["image/png"])],
        );
        let suggested: Vec<&str> = choice.suggested.iter().map(|app| app.id.as_str()).collect();
        assert_eq!(suggested, ["ferrite-video", "vlc"]);
        assert_eq!(
            choice.others.first().map(|app| app.id.as_str()),
            Some("ferrite-textedit")
        );
        let pdf = OpenWith::new(PathBuf::from("/nexiste/pas/guide.pdf"), Vec::new());
        assert_eq!(
            pdf.suggested.first().map(|app| app.name.as_str()),
            Some("Lecteur PDF")
        );
    }

    #[test]
    fn textedit_n_est_pas_suggere_pour_une_image() {
        assert!(is_text(None));
        assert!(is_text(Some("text/plain")));
        assert!(!is_text(Some("image/png")));
    }
}
