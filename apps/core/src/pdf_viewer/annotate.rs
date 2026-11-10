//! Lecteur PDF : sélection du texte à la souris, annotations (surligner, souligner,
//! barrer, notes, commentaires, signets) et colonne latérale (pages, notes, plan).
//! Les données et leur enregistrement sont dans `annotations.rs`, la géométrie de la
//! sélection dans `selection.rs` ; ici, ce qui se passe à l'écran.

use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, pos2, vec2};

use super::{
    Editing, Glyph, Jump, PdfViewerState, SIDEBAR_WIDTH, Side, Tool,
    annotations::{self, Kind, Notes},
    file_name, icon_button,
    poppler::Word,
    selection::{self, Cursor, Selection},
};
use crate::{kit, style};

/// Côté de la pastille d'une note posée sur la page, en points de PDF.
const NOTE_SIZE: f32 = 14.0;
/// Retours en arrière gardés pour les annotations.
const UNDO_LIMIT: usize = 50;
/// Durée d'affichage d'un message (« Texte copié »), en secondes.
const MESSAGE_SECONDS: f64 = 2.5;
/// Largeur de la bulle de commentaire.
const EDITOR_WIDTH: f32 = 272.0;

/// Ce que demande la barre posée au-dessus d'une sélection.
enum BarAction {
    Mark(Kind, usize),
    Comment,
    Copy,
}

impl PdfViewerState {
    /// Mots de la page, vides tant que le texte n'est pas lu (ou page scannée).
    pub(super) fn words(&self, page: usize) -> &[Word] {
        self.search
            .words
            .as_ref()
            .and_then(|pages| pages.get(page))
            .map_or(&[], Vec::as_slice)
    }

    fn save_notes(&self) {
        if let Some(key) = &self.notes_key {
            annotations::save(key, &self.notes, &self.path);
        }
    }

    fn keep_undo(&mut self) {
        self.undo.push(self.notes.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Modifie les annotations : l'état d'avant est gardé pour Ctrl+Z, le fichier est écrit.
    fn change(&mut self, edit: impl FnOnce(&mut Notes)) {
        self.keep_undo();
        edit(&mut self.notes);
        self.save_notes();
    }

    pub(super) fn undo(&mut self, ctx: &egui::Context) {
        if let Some(before) = self.undo.pop() {
            self.editing = None;
            self.redo.push(std::mem::replace(&mut self.notes, before));
            self.save_notes();
            self.say(ctx, "Annotation annulée");
        }
    }

    pub(super) fn redo(&mut self, ctx: &egui::Context) {
        if let Some(after) = self.redo.pop() {
            self.editing = None;
            self.undo.push(std::mem::replace(&mut self.notes, after));
            self.save_notes();
            self.say(ctx, "Annotation rétablie");
        }
    }

    pub(super) fn say(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        self.message = Some((text.into(), ctx.input(|input| input.time)));
    }

    /// Ferme la bulle de commentaire ; une note laissée vide est retirée.
    pub(super) fn close_editor(&mut self) {
        if let Some(editing) = self.editing.take()
            && self
                .notes
                .get(editing.id)
                .is_some_and(|item| item.kind == Kind::Note && item.comment.trim().is_empty())
        {
            self.notes.remove(editing.id);
            self.save_notes();
        }
    }

    fn edit(&mut self, id: u64, focus: bool, undo_kept: bool) {
        self.close_editor();
        self.selection = None;
        self.editing = Some(Editing { id, focus, undo_kept });
    }

    /// Sélection, page par page : rectangles (un par ligne) et texte.
    fn selection_parts(&self) -> Vec<(usize, Vec<Rect>, String)> {
        let Some(chosen) = self.selection else {
            return Vec::new();
        };
        let (start, end) = chosen.ordered();
        (start.page..=end.page)
            .filter_map(|page| {
                let words = self.words(page);
                let (first, last) = chosen.on_page(page, words.len())?;
                Some((
                    page,
                    selection::rects(words, first, last),
                    selection::text(words, first, last),
                ))
            })
            .collect()
    }

    fn selection_text(&self) -> String {
        self.selection_parts()
            .into_iter()
            .map(|(_, _, text)| text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(super) fn copy_selection(&mut self, ctx: &egui::Context) {
        let text = self.selection_text();
        if !text.is_empty() {
            ctx.copy_text(text);
            self.say(ctx, "Texte copié");
        }
    }

    /// Marque le texte choisi (une annotation par page) ; rend la première créée.
    fn mark_selection(&mut self, kind: Kind, color: usize) -> Option<u64> {
        let parts = self.selection_parts();
        if parts.is_empty() {
            return None;
        }
        self.keep_undo();
        let mut first = None;
        for (page, rects, text) in parts {
            let id = self.notes.add(page, kind, color, rects, text);
            first.get_or_insert(id);
        }
        self.save_notes();
        self.selection = None;
        self.ink = color;
        self.mark = kind;
        first
    }

    fn add_note(&mut self, page: usize, point: Pos2) {
        self.close_editor();
        let rect = Rect::from_center_size(point, vec2(NOTE_SIZE, NOTE_SIZE));
        let ink = self.ink;
        self.keep_undo();
        let id = self.notes.add(page, Kind::Note, ink, vec![rect], String::new());
        self.save_notes();
        self.tool = Tool::Read;
        self.edit(id, true, true);
    }

    /// Tout le texte de la page lue.
    pub(super) fn select_page(&mut self) {
        let page = self.current;
        let count = self.words(page).len();
        if count > 0 {
            self.close_editor();
            self.selection = Some(Selection {
                anchor: Cursor { page, word: 0 },
                focus: Cursor {
                    page,
                    word: count - 1,
                },
            });
        }
    }

    pub(super) fn toggle_bookmark(&mut self, ctx: &egui::Context, page: usize) {
        let had = self.notes.has_bookmark(page);
        self.change(|notes| notes.toggle_bookmark(page));
        let text = if had {
            format!("Signet retiré de la page {}", page + 1)
        } else {
            format!("Signet posé sur la page {}", page + 1)
        };
        self.say(ctx, text);
    }

    /// États figés pour les captures d'écran : `FERRITE_DEMO_PDF=selection,notes,plan,editeur`.
    pub(super) fn demo(&mut self) {
        let Ok(wanted) = std::env::var("FERRITE_DEMO_PDF") else {
            return;
        };
        for state in wanted.split(',') {
            match state.trim() {
                "selection" => {
                    let page = self.current;
                    let count = self.words(page).len();
                    if count > 12 {
                        self.selection = Some(Selection {
                            anchor: Cursor { page, word: 8 },
                            focus: Cursor {
                                page,
                                word: (count - 1).min(40),
                            },
                        });
                    }
                }
                "notes" => self.side = Side::Notes,
                "plan" => self.side = Side::Outline,
                "editeur" => {
                    if let Some(id) = self.notes.items.first().map(|item| item.id) {
                        self.edit(id, false, false);
                    }
                }
                _ => {}
            }
        }
    }

    /// Souris sur une page : sélection en glissant, clic sur une annotation, double et
    /// triple clic, menu au clic droit. `rect` est la page à l'écran.
    pub(super) fn page_input(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, page: usize, rect: Rect) {
        let response = ui.interact(
            rect,
            ui.scope_id().with(("page-pdf", page)),
            Sense::click_and_drag(),
        );
        let scale = self.scale.max(0.01);
        let to_page = |position: Pos2| ((position - rect.min) / scale).to_pos2();
        let shift = ctx.input(|input| input.modifiers.shift);

        if let Some(point) = response.hover_pos().map(to_page) {
            let over = self.notes.at(page, point);
            let icon = if self.tool == Tool::Note {
                egui::CursorIcon::Crosshair
            } else if self.selecting {
                egui::CursorIcon::Text
            } else if over.is_some() {
                egui::CursorIcon::PointingHand
            } else if selection::word_at(self.words(page), point).is_some() {
                egui::CursorIcon::Text
            } else {
                egui::CursorIcon::Default
            };
            ctx.set_cursor_icon(icon);
            if !self.selecting
                && let Some(item) = over.and_then(|id| self.notes.get(id))
                && !item.comment.trim().is_empty()
                && self.editing.as_ref().is_none_or(|editing| editing.id != item.id)
            {
                response.clone().on_hover_text_at_pointer(item.comment.clone());
            }
        }

        if response.drag_started_by(egui::PointerButton::Primary) && self.tool != Tool::Note {
            let origin = ctx.input(|input| input.pointer.press_origin()).map(to_page);
            let words = self.words(page);
            let word = origin.and_then(|point| {
                selection::word_at(words, point).or_else(|| selection::nearest(words, point))
            });
            if let Some(word) = word {
                self.close_editor();
                let cursor = Cursor { page, word };
                self.selection = Some(match self.selection {
                    Some(chosen) if shift => Selection {
                        anchor: chosen.anchor,
                        focus: cursor,
                    },
                    _ => Selection::single(cursor),
                });
                self.selecting = true;
            }
        }
        let point = response.interact_pointer_pos().map(to_page);
        if response.clicked()
            && let Some(point) = point
        {
            self.page_click(page, point, shift);
        }
        if (response.double_clicked() || response.triple_clicked())
            && self.tool != Tool::Note
            && let Some(point) = point
            && let Some(word) = selection::word_at(self.words(page), point)
        {
            let (first, last) = if response.triple_clicked() {
                selection::line_of(self.words(page), word)
            } else {
                (word, word)
            };
            self.close_editor();
            self.selection = Some(Selection {
                anchor: Cursor { page, word: first },
                focus: Cursor { page, word: last },
            });
        }
        if response.secondary_clicked() {
            self.menu_at = point.map(|point| (page, point));
        }
        response.context_menu(|ui| self.page_menu(ui, ctx, page));
    }

    fn page_click(&mut self, page: usize, point: Pos2, shift: bool) {
        if self.tool == Tool::Note {
            self.add_note(page, point);
            return;
        }
        if shift
            && let Some(chosen) = self.selection
            && let Some(word) = selection::nearest(self.words(page), point)
        {
            self.selection = Some(Selection {
                anchor: chosen.anchor,
                focus: Cursor { page, word },
            });
            return;
        }
        match self.notes.at(page, point) {
            Some(id) => self.edit(id, false, false),
            None => {
                self.close_editor();
                self.selection = None;
            }
        }
    }

    /// Pendant qu'on glisse : la sélection suit le pointeur, même sur une autre page, et
    /// la vue défile quand il approche du haut ou du bas.
    pub(super) fn track_selection(&mut self, ctx: &egui::Context, view: Rect) {
        if !self.selecting {
            return;
        }
        if let Some(position) = ctx.input(|input| input.pointer.latest_pos()) {
            let away = |rect: &Rect| (rect.top() - position.y).max(position.y - rect.bottom()).max(0.0);
            let target = self
                .page_rects
                .iter()
                .min_by(|a, b| away(&a.1).total_cmp(&away(&b.1)))
                .copied();
            if let Some((page, rect)) = target {
                let point = ((position - rect.min) / self.scale.max(0.01)).to_pos2();
                let word = selection::nearest(self.words(page), point);
                if let (Some(word), Some(chosen)) = (word, self.selection.as_mut()) {
                    chosen.focus = Cursor { page, word };
                }
            }
            if position.y < view.top() + 28.0 {
                self.jump = Some(Jump::Offset(self.offset.y - 24.0));
                ctx.request_repaint();
            } else if position.y > view.bottom() - 28.0 {
                self.jump = Some(Jump::Offset(self.offset.y + 24.0));
                ctx.request_repaint();
            }
        }
        if !ctx.input(|input| input.pointer.primary_down()) {
            self.selecting = false;
            if self.tool == Tool::Marker {
                self.mark_selection(self.mark, self.ink);
            }
        }
    }

    /// Annotations, sélection et signet de la page, par-dessus son image.
    pub(super) fn paint_marks(&self, ui: &egui::Ui, page: usize, rect: Rect) {
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let scale = self.scale;
        let place = |part: &Rect| {
            Rect::from_min_max(
                rect.min + part.min.to_vec2() * scale,
                rect.min + part.max.to_vec2() * scale,
            )
        };
        let edited = self.editing.as_ref().map(|editing| editing.id);
        let line = (scale * 1.1).clamp(1.5, 4.0);
        for item in self.notes.items.iter().filter(|item| item.page == page) {
            let ink = style::marker(item.color);
            for part in &item.rects {
                let area = place(part);
                match item.kind {
                    Kind::Highlight => {
                        painter.rect_filled(
                            area.expand2(vec2(1.0, 0.5)),
                            CornerRadius::same(2),
                            ink.gamma_multiply(0.4),
                        );
                    }
                    Kind::Underline => {
                        painter.hline(area.x_range(), area.bottom(), Stroke::new(line, ink));
                    }
                    Kind::Strike => {
                        painter.hline(area.x_range(), area.center().y, Stroke::new(line, ink));
                    }
                    Kind::Note => paint_note(&painter, area, ink),
                }
                if edited == Some(item.id) {
                    painter.rect_stroke(
                        area.expand(2.5),
                        CornerRadius::same(4),
                        Stroke::new(1.5, style::accent()),
                        egui::StrokeKind::Outside,
                    );
                }
            }
            // Pastille : ce passage porte un commentaire.
            if item.kind != Kind::Note
                && !item.comment.trim().is_empty()
                && let Some(first) = item.rects.first()
            {
                let at = place(first).right_top() + vec2(3.0, -2.0);
                painter.circle_filled(at, 4.5, ink);
                painter.circle_stroke(at, 4.5, Stroke::new(1.0, Color32::WHITE));
            }
        }
        if let Some(chosen) = self.selection {
            let words = self.words(page);
            if let Some((first, last)) = chosen.on_page(page, words.len()) {
                for part in selection::rects(words, first, last) {
                    painter.rect_filled(
                        place(&part).expand2(vec2(1.0, 0.5)),
                        CornerRadius::same(2),
                        style::accent().gamma_multiply(0.3),
                    );
                }
            }
        }
        if self.notes.has_bookmark(page) {
            let (x, y) = (rect.right() - 34.0, rect.top());
            // Ruban à encoche, en deux moitiés convexes.
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(x, y),
                    pos2(x + 8.0, y),
                    pos2(x + 8.0, y + 20.0),
                    pos2(x, y + 26.0),
                ],
                style::error(),
                Stroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(x + 8.0, y),
                    pos2(x + 16.0, y),
                    pos2(x + 16.0, y + 26.0),
                    pos2(x + 8.0, y + 20.0),
                ],
                style::error(),
                Stroke::NONE,
            ));
        }
    }

    /// Rectangle à l'écran d'une zone d'une page visible.
    fn on_screen(&self, page: usize, part: Rect) -> Option<Rect> {
        let (_, rect) = self.page_rects.iter().find(|(visible, _)| *visible == page)?;
        Some(Rect::from_min_max(
            rect.min + part.min.to_vec2() * self.scale,
            rect.min + part.max.to_vec2() * self.scale,
        ))
    }

    /// Barre posée au-dessus du texte choisi : couleurs du surligneur, souligner, barrer,
    /// commenter, copier.
    pub(super) fn selection_bar(&mut self, ctx: &egui::Context, view: Rect) {
        if self.selecting || self.editing.is_some() || self.selection.is_none() {
            return;
        }
        let anchor = self.selection_parts().iter().find_map(|(page, rects, _)| {
            Some((
                self.on_screen(*page, *rects.first()?)?,
                self.on_screen(*page, *rects.last()?)?,
            ))
        });
        let Some((first, last)) = anchor else {
            return;
        };
        let size = vec2(266.0, 42.0);
        let mut y = first.top() - size.y - 8.0;
        if y < view.top() + 4.0 {
            y = last.bottom() + 8.0;
        }
        let y = y.clamp(
            view.top() + 4.0,
            (view.bottom() - size.y - 4.0).max(view.top() + 4.0),
        );
        let x = first.left().clamp(
            view.left() + 8.0,
            (view.right() - size.x - 8.0).max(view.left() + 8.0),
        );
        let mut action = None;
        egui::Area::new(egui::Id::unique("pdf-barre-selection"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(x, y))
            .show(ctx, |ui| {
                bubble().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if let Some(color) = color_dots(ui, None, "Surligner en") {
                            action = Some(BarAction::Mark(Kind::Highlight, color));
                        }
                        ui.add_space(6.0);
                        if icon_button(ui, Glyph::Underline, false, "Souligner", true).clicked() {
                            action = Some(BarAction::Mark(Kind::Underline, self.ink));
                        }
                        if icon_button(ui, Glyph::Strike, false, "Barrer", true).clicked() {
                            action = Some(BarAction::Mark(Kind::Strike, self.ink));
                        }
                        if icon_button(ui, Glyph::Comment, false, "Commenter", true).clicked() {
                            action = Some(BarAction::Comment);
                        }
                        if icon_button(ui, Glyph::Copy, false, "Copier (Ctrl+C)", true).clicked() {
                            action = Some(BarAction::Copy);
                        }
                    });
                });
            });
        match action {
            Some(BarAction::Mark(kind, color)) => {
                self.mark_selection(kind, color);
            }
            Some(BarAction::Comment) => {
                if let Some(id) = self.mark_selection(Kind::Highlight, self.ink) {
                    self.edit(id, true, true);
                }
            }
            Some(BarAction::Copy) => self.copy_selection(ctx),
            None => {}
        }
    }

    /// Bulle d'une annotation : sa couleur, son commentaire, la supprimer.
    pub(super) fn editor(&mut self, ctx: &egui::Context, view: Rect) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        let (id, focus, undo_kept) = (editing.id, std::mem::take(&mut editing.focus), editing.undo_kept);
        let Some(item) = self.notes.get(id).cloned() else {
            self.editing = None;
            return;
        };
        let place = item
            .rects
            .first()
            .zip(item.rects.last())
            .and_then(|(first, last)| {
                Some((
                    self.on_screen(item.page, *first)?,
                    self.on_screen(item.page, *last)?,
                ))
            });
        // Page hors de la vue : la bulle revient quand on y retourne.
        let Some((first, last)) = place else {
            if focus && let Some(editing) = self.editing.as_mut() {
                editing.focus = true;
            }
            return;
        };
        let height = 156.0;
        let mut y = last.bottom() + 8.0;
        if y + height > view.bottom() {
            y = first.top() - height - 8.0;
        }
        let y = y.clamp(
            view.top() + 4.0,
            (view.bottom() - height - 4.0).max(view.top() + 4.0),
        );
        let x = first.left().clamp(
            view.left() + 8.0,
            (view.right() - EDITOR_WIDTH - 22.0).max(view.left() + 8.0),
        );
        let mut draft = item.comment.clone();
        let (mut color, mut delete, mut done) = (None, false, false);
        egui::Area::new(egui::Id::unique("pdf-bulle-annotation"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(x, y))
            .show(ctx, |ui| {
                bubble().show(ui, |ui| {
                    ui.set_width(EDITOR_WIDTH);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        color = color_dots(ui, Some(item.color), "");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button(ui, Glyph::Trash, false, "Supprimer l’annotation", true).clicked()
                            {
                                delete = true;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    let field = ui.add(
                        egui::TextEdit::multiline(&mut draft)
                            .hint_text("Ajouter un commentaire…")
                            .desired_width(f32::INFINITY)
                            .desired_rows(3),
                    );
                    if focus {
                        field.request_focus();
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{} · page {}", item.kind.label(), item.page + 1))
                                .size(12.0)
                                .color(style::text_secondary()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if kit::button(ui, "Terminer", true).clicked() {
                                done = true;
                            }
                        });
                    });
                });
            });
        if draft != item.comment {
            if !undo_kept {
                self.keep_undo();
                if let Some(editing) = self.editing.as_mut() {
                    editing.undo_kept = true;
                }
            }
            if let Some(item) = self.notes.get_mut(id) {
                item.comment = draft;
            }
            self.save_notes();
        }
        if let Some(color) = color
            && color != item.color
        {
            self.change(|notes| {
                if let Some(item) = notes.get_mut(id) {
                    item.color = color;
                }
            });
            self.ink = color;
        }
        if delete {
            self.editing = None;
            self.change(|notes| notes.remove(id));
        } else if done {
            self.close_editor();
        }
    }

    /// Message bref en bas de la vue (« Texte copié »).
    pub(super) fn paint_message(&mut self, ui: &egui::Ui, view: Rect) {
        let Some((text, since)) = &self.message else {
            return;
        };
        if ui.input(|input| input.time) - since > MESSAGE_SECONDS {
            self.message = None;
            return;
        }
        let painter = ui.painter().with_clip_rect(view);
        let galley = painter.layout_no_wrap(
            text.clone(),
            egui::FontId::proportional(13.0),
            style::inverse_text(),
        );
        let pill = Rect::from_center_size(
            pos2(view.center().x, view.bottom() - 34.0),
            galley.size() + vec2(28.0, 16.0),
        );
        painter.rect_filled(pill, CornerRadius::same(15), style::inverse());
        painter.galley(pill.center() - galley.size() / 2.0, galley, style::inverse_text());
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }

    /// Menu du clic droit sur une page.
    fn page_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, page: usize) {
        style::menu_style(ui);
        let at = self.menu_at.filter(|(clicked, _)| *clicked == page);
        let over = at.and_then(|(page, point)| self.notes.at(page, point));
        if self.selection.is_some() {
            if style::menu_row(ui, "Copier", "Ctrl+C", false, true).clicked() {
                self.copy_selection(ctx);
            }
            let mut picked = None;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.add_space(6.0);
                picked = color_dots(ui, None, "Surligner en");
            });
            if let Some(color) = picked {
                self.mark_selection(Kind::Highlight, color);
                ui.close();
            }
            if style::menu_row(ui, "Souligner", "", false, true).clicked() {
                self.mark_selection(Kind::Underline, self.ink);
            }
            if style::menu_row(ui, "Barrer", "", false, true).clicked() {
                self.mark_selection(Kind::Strike, self.ink);
            }
            if style::menu_row(ui, "Commenter…", "", false, true).clicked()
                && let Some(id) = self.mark_selection(Kind::Highlight, self.ink)
            {
                self.edit(id, true, true);
            }
            if style::menu_row(ui, "Rechercher dans le document", "", false, true).clicked() {
                let text = self.selection_text();
                self.search.query = text.split_whitespace().take(8).collect::<Vec<_>>().join(" ");
                self.open_search(ctx);
            }
            style::menu_separator(ui);
        }
        if let Some(id) = over {
            if style::menu_row(ui, "Modifier le commentaire", "", false, true).clicked() {
                self.edit(id, true, false);
            }
            if style::menu_row(ui, "Supprimer l’annotation", "", true, true).clicked() {
                self.close_editor();
                self.change(|notes| notes.remove(id));
            }
            style::menu_separator(ui);
        }
        if style::menu_row(ui, "Ajouter une note ici", "", false, at.is_some()).clicked()
            && let Some((page, point)) = at
        {
            self.add_note(page, point);
        }
        let label = if self.notes.has_bookmark(page) {
            "Retirer le signet de la page"
        } else {
            "Poser un signet sur la page"
        };
        if style::menu_row(ui, label, "", false, true).clicked() {
            self.toggle_bookmark(ctx, page);
        }
        let has_text = !self.words(page).is_empty();
        if style::menu_row(ui, "Tout sélectionner sur la page", "", false, has_text).clicked() {
            self.current = page;
            self.select_page();
        }
    }

    /// Menu du surligneur (barre d'outils) : son encre et son trait.
    pub(super) fn ink_menu(&mut self, ui: &mut egui::Ui) {
        style::menu_style(ui);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(6.0);
            if let Some(color) = color_dots(ui, Some(self.ink), "") {
                self.ink = color;
            }
        });
        style::menu_separator(ui);
        for (label, kind) in [
            ("Surligner", Kind::Highlight),
            ("Souligner", Kind::Underline),
            ("Barrer", Kind::Strike),
        ] {
            if style::radio_row(ui, label, self.mark == kind).clicked() {
                self.mark = kind;
            }
        }
    }

    /// Colonne de gauche : pages en miniature, notes et signets, plan du document.
    pub(super) fn side_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Panel::left(ui.scope_id().with("miniatures-pdf"))
            .resizable(false)
            .exact_size(SIDEBAR_WIDTH)
            .frame(egui::Frame::new().fill(style::sidebar_color()))
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter().vline(
                    full.right() - 0.5,
                    full.y_range(),
                    Stroke::new(1.0, style::line()),
                );
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut options = vec!["Pages", "Notes"];
                if !self.outline.is_empty() {
                    options.push("Plan");
                } else if self.side == Side::Outline {
                    self.side = Side::Pages;
                }
                let selected = match self.side {
                    Side::Pages => 0,
                    Side::Notes => 1,
                    Side::Outline => 2,
                };
                let (header, _) = ui.allocate_exact_size(vec2(full.width(), 46.0), Sense::hover());
                let count = options.len() as f32;
                let segment = (full.width() - 24.0 - 6.0 - 2.0 * (count - 1.0)) / count;
                if let Some(pick) = kit::segments_at(ui, header.center(), segment, &options, selected) {
                    self.side = [Side::Pages, Side::Notes, Side::Outline][pick];
                    self.follow_thumb = true;
                }
                match self.side {
                    Side::Pages => self.thumbnails(ui, ctx),
                    Side::Notes => self.notes_list(ui, ctx),
                    Side::Outline => self.outline_list(ui),
                }
            });
    }

    fn notes_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let width = ui.available_width();
        let inner = width - 20.0;
        let mut go = None;
        let mut open = None;
        let mut remove = None;
        let mut unmark = None;
        let mut export = false;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("notes-pdf")
            .show(ui, |ui| {
                ui.set_width(width);
                if self.notes.is_empty() {
                    ui.add_space(26.0);
                    let rows = [
                        ("Aucune note", style::primary_text(), 14.0),
                        (
                            "Sélectionnez du texte pour le surligner ou le commenter. Vos notes \
                             et vos signets apparaîtront ici.",
                            style::text_secondary(),
                            12.5,
                        ),
                    ];
                    for (text, color, size) in rows {
                        let galley =
                            wrapped(ui, text, egui::FontId::proportional(size), color, inner - 12.0, 8);
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(width, galley.size().y + 8.0), Sense::hover());
                        ui.painter()
                            .galley(pos2(rect.left() + 16.0, rect.top()), galley, color);
                    }
                    return;
                }
                if !self.notes.bookmarks.is_empty() {
                    list_heading(ui, "Signets");
                    for page in self.notes.bookmarks.clone() {
                        let (row, response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
                        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                        let item = row.shrink2(vec2(8.0, 1.0));
                        if ui.rect_contains_pointer(item) || page == self.current {
                            ui.painter()
                                .rect_filled(item, CornerRadius::same(7), style::tint(10));
                        }
                        super::paint_glyph(
                            ui.painter(),
                            pos2(item.left() + 16.0, item.center().y),
                            Glyph::Bookmark,
                            style::error(),
                        );
                        ui.painter().text(
                            pos2(item.left() + 32.0, item.center().y),
                            egui::Align2::LEFT_CENTER,
                            format!("Page {}", page + 1),
                            egui::FontId::proportional(13.0),
                            style::primary_text(),
                        );
                        if response.clicked() {
                            go = Some(Jump::Page(page));
                        }
                        if ui.rect_contains_pointer(item)
                            && small_close(ui, item, ("signet", page), "Retirer le signet")
                        {
                            unmark = Some(page);
                        }
                    }
                    ui.add_space(6.0);
                }
                if !self.notes.items.is_empty() {
                    list_heading(ui, &format!("Annotations ({})", self.notes.items.len()));
                }
                let edited = self.editing.as_ref().map(|editing| editing.id);
                for item in &self.notes.items {
                    let caption = wrapped(
                        ui,
                        &format!("Page {} · {}", item.page + 1, item.kind.label()),
                        egui::FontId::proportional(ferrite_ui::tokens::typography::CAPTION),
                        style::text_secondary(),
                        inner - 44.0,
                        1,
                    );
                    let quote = item.quote.split_whitespace().collect::<Vec<_>>().join(" ");
                    let quote = (!quote.is_empty()).then(|| {
                        wrapped(
                            ui,
                            &quote,
                            egui::FontId::proportional(ferrite_ui::tokens::typography::SMALL),
                            style::text_secondary(),
                            inner - 26.0,
                            3,
                        )
                    });
                    let comment = (!item.comment.trim().is_empty()).then(|| {
                        wrapped(
                            ui,
                            item.comment.trim(),
                            egui::FontId::proportional(13.0),
                            style::primary_text(),
                            inner - 26.0,
                            5,
                        )
                    });
                    let height = 18.0
                        + caption.size().y
                        + quote.as_ref().map_or(0.0, |galley| galley.size().y + 4.0)
                        + comment.as_ref().map_or(0.0, |galley| galley.size().y + 4.0);
                    let (row, response) = ui.allocate_exact_size(vec2(width, height + 4.0), Sense::click());
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    let card = row.shrink2(vec2(8.0, 2.0));
                    let fill = if edited == Some(item.id) {
                        style::tint(18)
                    } else if ui.rect_contains_pointer(card) {
                        style::tint(10)
                    } else {
                        style::tint(5)
                    };
                    let painter = ui.painter();
                    painter.rect_filled(card, CornerRadius::same(8), fill);
                    painter.rect_filled(
                        Rect::from_min_max(
                            pos2(card.left() + 7.0, card.top() + 9.0),
                            pos2(card.left() + 10.5, card.bottom() - 9.0),
                        ),
                        CornerRadius::same(2),
                        style::marker(item.color),
                    );
                    let left = card.left() + 18.0;
                    let mut y = card.top() + 8.0;
                    let caption_height = caption.size().y;
                    painter.galley(pos2(left, y), caption, style::text_secondary());
                    y += caption_height + 4.0;
                    if let Some(galley) = quote {
                        let size = galley.size().y;
                        painter.galley(pos2(left, y), galley, style::text_secondary());
                        y += size + 4.0;
                    }
                    if let Some(galley) = comment {
                        painter.galley(pos2(left, y), galley, style::primary_text());
                    }
                    if response.clicked() {
                        go = Some(Jump::Point(item.page, item.top()));
                        open = Some(item.id);
                    }
                    if ui.rect_contains_pointer(card)
                        && small_close(ui, card, ("annotation", item.id), "Supprimer l’annotation")
                    {
                        remove = Some(item.id);
                    }
                }
                if !self.notes.items.is_empty() {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        if kit::button(ui, "Exporter les notes", false)
                            .on_hover_text("Écrit toutes les notes dans un fichier texte, à côté du document")
                            .clicked()
                        {
                            export = true;
                        }
                    });
                }
                ui.add_space(14.0);
            });
        if let Some(jump) = go {
            if let Jump::Page(page) | Jump::Point(page, _) = jump {
                self.current = page;
            }
            self.jump = Some(jump);
        }
        if let Some(id) = open {
            self.edit(id, false, false);
        }
        if let Some(id) = remove {
            if self.editing.as_ref().is_some_and(|editing| editing.id == id) {
                self.editing = None;
            }
            self.change(|notes| notes.remove(id));
        }
        if let Some(page) = unmark {
            self.toggle_bookmark(ctx, page);
        }
        if export {
            let text = match annotations::export(&self.notes, &self.path) {
                Ok(path) => format!("Notes écrites dans « {} »", file_name(&path)),
                Err(error) => error,
            };
            self.say(ctx, text);
        }
    }

    fn outline_list(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        // Chapitre en cours : la dernière entrée qui commence avant ou sur la page lue.
        let here = self.outline.iter().rposition(|item| item.page <= self.current);
        let mut go = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("plan-pdf")
            .show(ui, |ui| {
                ui.set_width(width);
                for (index, item) in self.outline.iter().enumerate() {
                    let (row, response) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
                    let response = response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(format!("{} — page {}", item.title, item.page + 1));
                    let line = row.shrink2(vec2(8.0, 1.0));
                    if Some(index) == here {
                        ui.painter()
                            .rect_filled(line, CornerRadius::same(7), style::sidebar_selected());
                    } else if response.hovered() {
                        ui.painter()
                            .rect_filled(line, CornerRadius::same(7), style::tint(10));
                    }
                    let ink = if Some(index) == here {
                        style::sidebar_selected_text()
                    } else if item.level == 0 {
                        style::primary_text()
                    } else {
                        style::text_secondary()
                    };
                    let indent = 10.0 + 12.0 * item.level.min(4) as f32;
                    let number = ui.painter().layout_no_wrap(
                        (item.page + 1).to_string(),
                        egui::FontId::proportional(ferrite_ui::tokens::typography::CAPTION),
                        ink,
                    );
                    let title = wrapped(
                        ui,
                        &item.title,
                        egui::FontId::proportional(13.0),
                        ink,
                        (line.width() - indent - number.size().x - 18.0).max(20.0),
                        1,
                    );
                    ui.painter().galley(
                        pos2(line.left() + indent, line.center().y - title.size().y / 2.0),
                        title,
                        ink,
                    );
                    ui.painter().galley(
                        pos2(
                            line.right() - 8.0 - number.size().x,
                            line.center().y - number.size().y / 2.0,
                        ),
                        number,
                        ink,
                    );
                    if response.clicked() {
                        go = Some(item.page);
                    }
                }
                ui.add_space(12.0);
            });
        if let Some(page) = go {
            self.go_to(page);
        }
    }
}

/// Fond d'une bulle flottante posée sur la page.
fn bubble() -> egui::Frame {
    egui::Frame::new()
        .fill(style::background())
        .stroke(Stroke::new(1.0, style::border()))
        .corner_radius(11.0)
        .inner_margin(egui::Margin::same(7))
        .shadow(style::elevation(ferrite_ui::tokens::elevation::MENU))
}

/// Les encres du surligneur, en ligne : rend celle qu'on vient de cliquer. `selected` est
/// cerclée ; `hint` précède le nom de la couleur dans l'info-bulle.
fn color_dots(ui: &mut egui::Ui, selected: Option<usize>, hint: &str) -> Option<usize> {
    let mut picked = None;
    for (index, name) in style::MARKER_NAMES.iter().enumerate() {
        let (rect, response) = ui.allocate_exact_size(vec2(24.0, kit::TOOL), Sense::click());
        let radius = if response.hovered() { 9.0 } else { 8.0 };
        let painter = ui.painter();
        painter.circle_filled(rect.center(), radius, style::marker(index));
        painter.circle_stroke(rect.center(), radius, Stroke::new(1.0, style::tint(40)));
        if selected == Some(index) {
            painter.circle_stroke(rect.center(), 11.0, Stroke::new(1.6, style::primary_text()));
        }
        let text = if hint.is_empty() {
            (*name).to_owned()
        } else {
            format!("{hint} {}", name.to_lowercase())
        };
        if response.on_hover_text(text).clicked() {
            picked = Some(index);
        }
    }
    picked
}

/// Pastille d'une note posée sur la page : un petit feuillet de la couleur de l'encre.
fn paint_note(painter: &egui::Painter, area: Rect, ink: Color32) {
    let area = Rect::from_center_size(area.center(), area.size().max(vec2(14.0, 14.0)));
    painter.rect_filled(area, CornerRadius::same(3), ink);
    painter.rect_stroke(
        area,
        CornerRadius::same(3),
        Stroke::new(1.0, Color32::from_black_alpha(70)),
        egui::StrokeKind::Inside,
    );
    let step = area.height() / 4.0;
    for row in 1..=3 {
        let y = area.top() + step * row as f32;
        let right = if row == 3 {
            area.center().x
        } else {
            area.right() - 3.0
        };
        painter.line_segment(
            [pos2(area.left() + 3.0, y), pos2(right, y)],
            Stroke::new(1.0, Color32::from_black_alpha(150)),
        );
    }
}

/// Texte mis en page sur `width`, coupé par « … » au-delà de `rows` lignes.
fn wrapped(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, width);
    job.wrap.max_rows = rows;
    // Une seule ligne : couper au plus près du bord, pas au mot.
    job.wrap.break_anywhere = rows == 1;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

fn list_heading(ui: &mut egui::Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 16.0, rect.center().y + 1.0),
        egui::Align2::LEFT_CENTER,
        text.to_uppercase(),
        egui::FontId::new(11.0, style::strong_family()),
        style::text_secondary(),
    );
}

/// Petite croix en haut à droite d'une ligne survolée ; vrai si on vient de la cliquer.
fn small_close(
    ui: &mut egui::Ui,
    row: Rect,
    salt: impl std::hash::Hash + std::fmt::Debug,
    hint: &str,
) -> bool {
    let rect = Rect::from_center_size(pos2(row.right() - 14.0, row.top() + 14.0), vec2(20.0, 20.0));
    let response = ui
        .interact(rect, ui.scope_id().with(("retirer", salt)), Sense::click())
        .on_hover_text(hint);
    let ink = if response.hovered() {
        style::primary_text()
    } else {
        style::text_secondary()
    };
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), style::tint(18));
    }
    super::paint_glyph(ui.painter(), rect.center(), Glyph::Small, ink);
    response.clicked()
}
