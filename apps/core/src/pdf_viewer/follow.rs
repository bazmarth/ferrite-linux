//! Suivre les liens d'un document : au survol, la main et une info-bulle (l'adresse, ou le
//! début du texte visé : l'entrée de la bibliographie pour une référence [12]) ; au clic,
//! le saut, et un bouton « Revenir » pour retrouver l'endroit quitté (Alt+←).

use std::path::PathBuf;

use eframe::egui::{self, Pos2, Rect, pos2};

use super::{
    Jump, PdfViewerState,
    annotate::bubble,
    links::{self, Link, Target},
};
use crate::kit;

/// Positions gardées pour « Revenir ».
const MAX_BACK: usize = 50;

impl PdfViewerState {
    pub(super) fn link_at(&self, page: usize, point: Pos2) -> Option<&Link> {
        links::at(self.links.get(page)?, point)
    }

    /// Texte de l'info-bulle d'un lien.
    pub(super) fn link_hint(&self, target: &Target) -> String {
        match target {
            Target::Uri(uri) => format!("Ouvrir {}", uri.strip_prefix("mailto:").unwrap_or(uri)),
            Target::File { path, page } => {
                let name = super::file_name(path);
                match page {
                    Some(page) => format!("Ouvrir {name}, page {}", page + 1),
                    None => format!("Ouvrir {name}"),
                }
            }
            Target::Page { page, left, top } => {
                let preview = links::preview(self.words(*page), *left, *top);
                match preview {
                    Some(text) => format!("Page {} — {text}", page + 1),
                    None => format!("Aller à la page {}", page + 1),
                }
            }
        }
    }

    pub(super) fn follow(&mut self, ctx: &egui::Context, target: Target) {
        match target {
            Target::Page { page, top, .. } => {
                if page >= self.page_count() {
                    return;
                }
                if self.back.len() >= MAX_BACK {
                    self.back.remove(0);
                }
                self.back.push((self.offset.y, self.current));
                self.close_editor();
                self.selection = None;
                self.jump = Some(match top {
                    Some(top) => Jump::Top(page, top),
                    None => Jump::Page(page),
                });
                self.current = page;
                self.follow_thumb = true;
            }
            Target::Uri(uri) => {
                let mut command = std::process::Command::new("xdg-open");
                command.arg(&uri);
                if ferrite_system::command::spawn_detached(command).is_ok() {
                    self.say(ctx, "Ouverture dans le navigateur…");
                } else {
                    self.say(ctx, "Impossible d’ouvrir ce lien.");
                }
            }
            Target::File { path, page } => {
                if path.is_file() {
                    self.request_file = Some((path, page.map_or(1, |page| page + 1)));
                } else {
                    let name = super::file_name(&path);
                    self.say(ctx, format!("Document introuvable : {name}"));
                }
            }
        }
    }

    /// Capture d'essai (`FERRITE_DEMO_PDF=lien`) : suit le premier renvoi de la page lue.
    pub(super) fn demo_link(&mut self, ctx: &egui::Context) {
        let wanted = std::env::var("FERRITE_DEMO_PDF").unwrap_or_default();
        if !wanted.split(',').any(|state| state.trim() == "lien") {
            return;
        }
        let target = self.links.get(self.current).and_then(|links| {
            links
                .iter()
                .find(|link| matches!(link.target, Target::Page { .. }))
                .map(|link| link.target.clone())
        });
        if let Some(target) = target {
            self.follow(ctx, target);
        }
    }

    /// Retour à l'endroit quitté par le dernier renvoi suivi.
    pub(super) fn go_back(&mut self) {
        if let Some((offset, page)) = self.back.pop() {
            self.jump = Some(Jump::Offset(offset));
            self.current = page;
            self.follow_thumb = true;
        }
    }

    /// Bouton « Revenir » en bas à gauche de la vue, tant qu'un renvoi a été suivi.
    pub(super) fn back_button(&mut self, ctx: &egui::Context, view: Rect) {
        let Some((_, page)) = self.back.last().copied() else {
            return;
        };
        let mut clicked = false;
        egui::Area::new(egui::Id::unique(("pdf-revenir", self.id)))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(view.left() + 14.0, view.bottom() - 58.0))
            .show(ctx, |ui| {
                bubble().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let label = format!("← Revenir à la page {}", page + 1);
                        clicked = kit::button(ui, &label, false).on_hover_text("Alt+←").clicked();
                        if super::icon_button(ui, super::Glyph::Close, false, "Oublier", true).clicked() {
                            self.back.clear();
                        }
                    });
                });
            });
        if clicked {
            self.go_back();
        }
    }
}

/// Document à ouvrir depuis un lien vers un autre fichier, et sa page (depuis 1).
pub type FileRequest = (PathBuf, usize);
