//! TextEdit : éditeur de texte UTF-8.
//!
//! - onglets (plusieurs documents), glisser-déposer de fichiers sur la fenêtre ;
//! - Ouvrir et Enregistrer sous passent par le sélecteur de Fichiers, affiché en feuille
//!   par-dessus la fenêtre ;
//! - numéros de ligne, ligne courante surlignée, retour à la ligne au choix, taille du
//!   texte réglable ;
//! - rechercher (occurrences surlignées) et remplacer ;
//! - confirmation avant de fermer un document ou de quitter avec des modifications.
//!
//! Comme dans Fichiers, l'interface ne modifie pas l'état pendant son dessin : les clics
//! produisent des `Command`, appliquées une fois l'image construite.

use std::{
    ops::Range,
    path::{Path, PathBuf},
};

use eframe::egui::{
    self, Color32, FontId, Frame, Key, KeyboardShortcut, Margin, Modifiers, RichText, Sense, Stroke,
    text::{CCursor, CCursorRange, LayoutJob, TextFormat},
};

use crate::{
    files::{FilesState, PickerMode},
    style,
};

const FONT_SIZES: [f32; 7] = [11.0, 12.0, 13.0, 14.0, 16.0, 18.0, 22.0];
const DEFAULT_FONT: usize = 3;
const MATCH: Color32 = Color32::from_rgb(0xFF, 0xEE, 0xA0);
const MATCH_CURRENT: Color32 = Color32::from_rgb(0xFF, 0xBE, 0x4D);
const CURRENT_LINE: Color32 = Color32::from_rgb(0xF3, 0xF6, 0xFB);
const GUTTER: Color32 = Color32::from_rgb(0xF7, 0xF7, 0xF8);

#[derive(Debug)]
struct Document {
    path: Option<PathBuf>,
    contents: String,
    /// Contenu sur le disque : le document est modifié s'il en diffère.
    saved: String,
    id: egui::Id,
}

impl Document {
    fn new(path: Option<PathBuf>, contents: String) -> Self {
        Self {
            path,
            saved: contents.clone(),
            contents,
            id: egui::Id::unique(("document", next_id())),
        }
    }

    fn dirty(&self) -> bool {
        self.contents != self.saved
    }

    fn name(&self) -> String {
        self.path
            .as_deref()
            .map(display_name)
            .unwrap_or_else(|| "Sans titre".to_owned())
    }

    fn is_blank(&self) -> bool {
        self.path.is_none() && self.contents.is_empty()
    }
}

fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Default)]
struct Find {
    query: String,
    replacement: String,
    replace: bool,
    match_case: bool,
    /// Occurrence courante, parmi celles du document actif.
    current: usize,
    /// Donner le focus au champ de recherche à la prochaine image.
    focus: bool,
}

/// Ce qui suit un enregistrement réussi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    Nothing,
    Close(usize),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Open,
    SaveAs(usize, Then),
}

/// Question posée avant de perdre des modifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Close(usize),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Close(usize),
    Select(usize),
    NextTab(bool),
    Find(bool),
    CloseFind,
    FindStep(bool),
    ReplaceOne,
    ReplaceAll,
    Zoom(i32),
    ToggleWrap,
    ToggleLineNumbers,
    /// Réponse à la question : enregistrer, ne pas enregistrer, annuler.
    Answer(Option<bool>),
}

pub struct EditorState {
    documents: Vec<Document>,
    active: usize,
    find: Option<Find>,
    picker: Option<(Purpose, FilesState)>,
    ask: Option<Ask>,
    wrap: bool,
    line_numbers: bool,
    font: usize,
    message: Option<(String, bool)>,
    /// Ligne et colonne du curseur (à partir de 1).
    cursor: (usize, usize),
    /// Sélection à imposer au champ de texte à la prochaine image.
    pending_selection: Option<CCursorRange>,
    /// Faire défiler jusqu'à l'occurrence courante.
    reveal_match: bool,
    quitting: bool,
    close_window: bool,
    title: String,
    started: bool,
}

impl std::fmt::Debug for EditorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorState")
            .field("documents", &self.documents)
            .field("active", &self.active)
            .finish()
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            documents: vec![Document::new(None, String::new())],
            active: 0,
            find: None,
            picker: None,
            ask: None,
            wrap: true,
            line_numbers: true,
            font: DEFAULT_FONT,
            message: None,
            cursor: (1, 1),
            // Le curseur est dans le texte dès l'ouverture : on peut écrire tout de suite.
            pending_selection: Some(CCursorRange::one(CCursor::new(0))),
            reveal_match: false,
            quitting: false,
            close_window: false,
            title: String::new(),
            started: false,
        }
    }
}

impl EditorState {
    fn document(&self) -> &Document {
        &self.documents[self.active]
    }

    fn report(&mut self, message: impl Into<String>, error: bool) {
        self.message = Some((message.into(), error));
    }

    /// Ouvre `path` dans un onglet (ou revient à l'onglet qui l'affiche déjà).
    pub fn open(&mut self, path: PathBuf) {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if let Some(index) = self.documents.iter().position(|d| d.path.as_ref() == Some(&path)) {
            self.active = index;
            return;
        }
        let document = match std::fs::read(&path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(contents) => {
                    self.report(format!("« {} » ouvert", display_name(&path)), false);
                    Document::new(Some(path), contents)
                }
                Err(_) => return self.report("Ce fichier n'est pas du texte (UTF-8)", true),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.report("Nouveau fichier", false);
                Document::new(Some(path), String::new())
            }
            Err(error) => return self.report(format!("Lecture impossible : {error}"), true),
        };
        // Un document vide et sans nom laisse sa place, comme dans les éditeurs modernes.
        if self.document().is_blank() {
            self.documents[self.active] = document;
        } else {
            self.documents.push(document);
            self.active = self.documents.len() - 1;
        }
        self.pending_selection = Some(CCursorRange::one(CCursor::new(0)));
    }

    fn new_document(&mut self) {
        self.documents.push(Document::new(None, String::new()));
        self.active = self.documents.len() - 1;
        self.pending_selection = Some(CCursorRange::one(CCursor::new(0)));
    }

    /// Enregistre le document `index` ; sans chemin, ouvre le sélecteur. Renvoie vrai si
    /// c'est fait.
    fn save(&mut self, index: usize, then: Then) -> bool {
        let Some(path) = self.documents[index].path.clone() else {
            self.save_as(index, then);
            return false;
        };
        let document = &mut self.documents[index];
        match std::fs::write(&path, &document.contents) {
            Ok(()) => {
                document.saved = document.contents.clone();
                self.report(format!("« {} » enregistré", display_name(&path)), false);
                self.after_save(then);
                true
            }
            Err(error) => {
                self.report(format!("Enregistrement impossible : {error}"), true);
                false
            }
        }
    }

    fn save_as(&mut self, index: usize, then: Then) {
        let document = &self.documents[index];
        let start = document
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let name = match &document.path {
            Some(_) => document.name(),
            None => "Sans titre.txt".to_owned(),
        };
        let picker = FilesState::picker(PickerMode::Save, start.as_deref(), &name);
        self.picker = Some((Purpose::SaveAs(index, then), picker));
    }

    fn open_dialog(&mut self) {
        let start = self
            .document()
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let picker = FilesState::picker(PickerMode::Open, start.as_deref(), "");
        self.picker = Some((Purpose::Open, picker));
    }

    fn after_save(&mut self, then: Then) {
        match then {
            Then::Nothing => {}
            Then::Close(index) => self.close(index, true),
            Then::Quit => self.save_all_then_quit(),
        }
    }

    /// Ferme l'onglet `index` ; avec des modifications, demande d'abord (sauf `force`).
    fn close(&mut self, index: usize, force: bool) {
        if index >= self.documents.len() {
            return;
        }
        if !force && self.documents[index].dirty() {
            self.active = index;
            self.ask = Some(Ask::Close(index));
            return;
        }
        self.documents.remove(index);
        if self.documents.is_empty() {
            self.documents.push(Document::new(None, String::new()));
        }
        if self.active > index || self.active >= self.documents.len() {
            self.active = self.active.saturating_sub(1);
        }
    }

    /// Enregistre tous les documents modifiés puis quitte ; un document sans nom ouvre
    /// d'abord le sélecteur, et l'on reprend après lui.
    fn save_all_then_quit(&mut self) {
        for index in 0..self.documents.len() {
            if self.documents[index].dirty() {
                self.active = index;
                if !self.save(index, Then::Nothing) {
                    if let Some((Purpose::SaveAs(_, then), _)) = &mut self.picker {
                        *then = Then::Quit;
                    }
                    return;
                }
            }
        }
        self.quitting = true;
        self.close_window = true;
    }

    /// Titre de la fenêtre : nom du document, marqué s'il a des modifications.
    fn window_title(&self) -> String {
        let document = self.document();
        let mark = if document.dirty() { "• " } else { "" };
        format!("{mark}{} — TextEdit", document.name())
    }

    fn font_id(&self) -> FontId {
        FontId::monospace(FONT_SIZES[self.font])
    }

    fn matches(&self) -> Vec<Range<usize>> {
        match &self.find {
            Some(find) => find_matches(&self.document().contents, &find.query, find.match_case),
            None => Vec::new(),
        }
    }

    fn run(&mut self, command: Command) {
        match command {
            Command::New => self.new_document(),
            Command::Open => self.open_dialog(),
            Command::Save => {
                self.save(self.active, Then::Nothing);
            }
            Command::SaveAs => self.save_as(self.active, Then::Nothing),
            // `usize::MAX` : l'onglet actif (Ctrl+W).
            Command::Close(usize::MAX) => self.close(self.active, false),
            Command::Close(index) => self.close(index, false),
            Command::Select(index) => self.active = index.min(self.documents.len() - 1),
            Command::NextTab(forward) => {
                let count = self.documents.len();
                self.active = if forward {
                    (self.active + 1) % count
                } else {
                    (self.active + count - 1) % count
                };
            }
            Command::Find(replace) => {
                let find = self.find.get_or_insert_with(Find::default);
                find.replace |= replace;
                find.focus = true;
                self.reveal_match = true;
            }
            Command::CloseFind => {
                // La recherche fermée laisse l'occurrence courante sélectionnée.
                let matches = self.matches();
                if let Some(find) = self.find.take()
                    && let Some(range) = matches.get(find.current)
                {
                    let text = &self.document().contents;
                    self.pending_selection = Some(CCursorRange::two(
                        CCursor::new(text[..range.start].chars().count()),
                        CCursor::new(text[..range.end].chars().count()),
                    ));
                } else {
                    self.pending_selection = None;
                }
            }
            Command::FindStep(forward) => {
                let count = self.matches().len();
                if let Some(find) = &mut self.find
                    && count > 0
                {
                    find.current = if forward {
                        (find.current + 1) % count
                    } else {
                        (find.current + count - 1) % count
                    };
                    self.reveal_match = true;
                }
            }
            Command::ReplaceOne => {
                let matches = self.matches();
                if let Some(find) = &self.find
                    && let Some(range) = matches.get(find.current).cloned()
                {
                    let replacement = find.replacement.clone();
                    self.documents[self.active]
                        .contents
                        .replace_range(range, &replacement);
                    self.reveal_match = true;
                    if let Some(find) = &mut self.find {
                        let left = matches.len() - 1;
                        find.current = if left == 0 { 0 } else { find.current % left };
                    }
                }
            }
            Command::ReplaceAll => {
                let matches = self.matches();
                if let Some(find) = &self.find
                    && !matches.is_empty()
                {
                    let replacement = find.replacement.clone();
                    let document = &mut self.documents[self.active];
                    for range in matches.iter().rev() {
                        document.contents.replace_range(range.clone(), &replacement);
                    }
                    let count = matches.len();
                    self.report(
                        format!("{count} remplacement{}", if count > 1 { "s" } else { "" }),
                        false,
                    );
                }
            }
            Command::Zoom(step) => {
                self.font = match step {
                    0 => DEFAULT_FONT,
                    step => (self.font as i32 + step).clamp(0, FONT_SIZES.len() as i32 - 1) as usize,
                };
            }
            Command::ToggleWrap => self.wrap = !self.wrap,
            Command::ToggleLineNumbers => self.line_numbers = !self.line_numbers,
            Command::Answer(answer) => {
                let ask = self.ask.take();
                match (ask, answer) {
                    (Some(Ask::Close(index)), Some(true)) => {
                        self.save(index, Then::Close(index));
                    }
                    (Some(Ask::Close(index)), Some(false)) => self.close(index, true),
                    (Some(Ask::Quit), Some(true)) => self.save_all_then_quit(),
                    (Some(Ask::Quit), Some(false)) => {
                        self.quitting = true;
                        self.close_window = true;
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !self.started {
            // Ctrl+plus et Ctrl+moins règlent la taille du texte, pas celle de l'interface.
            ctx.options_mut(|options| options.zoom_with_keyboard = false);
            self.started = true;
        }
        self.window_events(&ctx);

        let mut commands = Vec::new();
        if self.picker.is_none() && self.ask.is_none() {
            shortcuts(&ctx, self.find.is_some(), &mut commands);
        }
        let title = self.window_title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        style::header(ui, |ui| self.header(ui, &mut commands));
        if self.documents.len() > 1 {
            self.tabs(ui, &mut commands);
        }
        if self.find.is_some() {
            self.find_bar(ui, &mut commands);
        }
        self.status_bar(ui, &mut commands);
        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(style::BACKGROUND))
            .show(ui, |ui| self.text_area(ui));
        self.picker_sheet(&ctx);
        self.ask_dialog(&ctx, &mut commands);

        for command in commands {
            self.run(command);
        }
    }

    /// Fermeture de la fenêtre et fichiers déposés dessus.
    fn window_events(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.viewport().close_requested())
            && !self.quitting
            && self.documents.iter().any(Document::dirty)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.ask = Some(Ask::Quit);
        }
        if std::mem::take(&mut self.close_window) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        for path in dropped {
            self.open(path);
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        if ui.button("Ouvrir…").on_hover_text("Ctrl+O").clicked() {
            commands.push(Command::Open);
        }
        if glyph_button(ui, Glyph::Plus, false, "Nouveau document (Ctrl+N)").clicked() {
            commands.push(Command::New);
        }
        ui.add_space(8.0);
        let document = self.document();
        let name = document.name();
        let folder = document
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(short_path)
            .unwrap_or_default();
        let dirty = document.dirty();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let more = glyph_button(ui, Glyph::More, false, "Menu");
            egui::Popup::menu(&more).show(|ui| self.menu(ui, commands));
            let searching = self.find.is_some();
            if glyph_button(ui, Glyph::Search, searching, "Rechercher (Ctrl+F)").clicked() {
                commands.push(if searching {
                    Command::CloseFind
                } else {
                    Command::Find(false)
                });
            }
            if style::suggested_button(ui, "Enregistrer")
                .on_hover_text("Ctrl+S")
                .clicked()
            {
                commands.push(Command::Save);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(RichText::new(name).family(style::strong_family()));
                if dirty {
                    ui.label(RichText::new("— modifié").color(style::TEXT_SECONDARY));
                }
                if !folder.is_empty() {
                    ui.label(RichText::new(folder).small().color(style::TEXT_SECONDARY));
                }
            });
        });
    }

    fn menu(&self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        ui.set_min_width(250.0);
        let mut item = |ui: &mut egui::Ui, text: &str, shortcut: &str, command: Command| {
            if ui.add(egui::Button::new(text).shortcut_text(shortcut)).clicked() {
                commands.push(command);
                ui.close();
            }
        };
        item(ui, "Nouveau", "Ctrl+N", Command::New);
        item(ui, "Ouvrir…", "Ctrl+O", Command::Open);
        item(ui, "Enregistrer", "Ctrl+S", Command::Save);
        item(ui, "Enregistrer sous…", "Ctrl+Maj+S", Command::SaveAs);
        ui.separator();
        item(ui, "Rechercher", "Ctrl+F", Command::Find(false));
        item(ui, "Remplacer", "Ctrl+H", Command::Find(true));
        ui.separator();
        // Cases dessinées par egui : la police n'a pas de coche « ✔ ».
        for (text, on, command) in [
            ("Retour à la ligne", self.wrap, Command::ToggleWrap),
            ("Numéros de ligne", self.line_numbers, Command::ToggleLineNumbers),
        ] {
            let mut on = on;
            if ui.checkbox(&mut on, text).clicked() {
                commands.push(command);
                ui.close();
            }
        }
        let mut item = |ui: &mut egui::Ui, text: &str, shortcut: &str, command: Command| {
            if ui.add(egui::Button::new(text).shortcut_text(shortcut)).clicked() {
                commands.push(command);
                ui.close();
            }
        };
        item(ui, "Agrandir le texte", "Ctrl++", Command::Zoom(1));
        item(ui, "Réduire le texte", "Ctrl+-", Command::Zoom(-1));
        item(ui, "Taille normale", "Ctrl+0", Command::Zoom(0));
        ui.separator();
        item(ui, "Fermer l'onglet", "Ctrl+W", Command::Close(self.active));
    }

    fn tabs(&self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        egui::Panel::top(ui.scope_id().with("onglets"))
            .resizable(false)
            .frame(Frame::new().fill(style::HEADER).inner_margin(Margin {
                left: 8,
                right: 8,
                top: 0,
                bottom: 6,
            }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let count = self.documents.len() as f32;
                    let width = ((ui.available_width() - 4.0 * count) / count).clamp(90.0, 220.0);
                    for (index, document) in self.documents.iter().enumerate() {
                        self.tab(ui, index, document, width, commands);
                    }
                });
            });
    }

    fn tab(
        &self,
        ui: &mut egui::Ui,
        index: usize,
        document: &Document,
        width: f32,
        commands: &mut Vec<Command>,
    ) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::click());
        let active = index == self.active;
        let painter = ui.painter();
        if active {
            painter.rect_filled(rect, 7.0, style::BACKGROUND);
            painter.rect_stroke(
                rect,
                7.0,
                Stroke::new(1.0, style::SEPARATOR),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_filled(rect, 7.0, Color32::from_black_alpha(10));
        }
        // Bouton de fermeture à gauche (au survol), point si le document est modifié.
        let close = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 16.0, rect.center().y),
            egui::vec2(18.0, 18.0),
        );
        let close_response = ui.interact(close, response.id.with("fermer"), Sense::click());
        if response.hovered() || close_response.hovered() {
            if close_response.hovered() {
                painter.rect_filled(close, 4.0, Color32::from_black_alpha(18));
            }
            paint_glyph(ui, Glyph::Close, close, style::TEXT_SECONDARY);
        } else if document.dirty() {
            painter.circle_filled(close.center(), 3.5, style::TEXT_SECONDARY);
        }
        let color = if active {
            style::TEXT
        } else {
            style::TEXT_SECONDARY
        };
        let name = elide(ui, &document.name(), width - 44.0, 12.5);
        painter.text(
            egui::pos2(rect.center().x + 6.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            name,
            FontId::proportional(12.5),
            color,
        );
        if close_response.clicked() || response.middle_clicked() {
            commands.push(Command::Close(index));
        } else if response.clicked() {
            commands.push(Command::Select(index));
        }
        let path = document.path.as_deref().map(|path| path.display().to_string());
        response.on_hover_text(path.unwrap_or_else(|| "Document non enregistré".to_owned()));
    }

    fn find_bar(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        let count = self.matches().len();
        let Some(find) = &mut self.find else { return };
        egui::Panel::top(ui.scope_id().with("recherche"))
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(style::HEADER)
                    .inner_margin(Margin::symmetric(10, 6))
                    .stroke(Stroke::new(1.0, style::SEPARATOR)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let before = find.query.clone();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut find.query)
                            .hint_text("Rechercher")
                            .desired_width(220.0),
                    );
                    if std::mem::take(&mut find.focus) {
                        response.request_focus();
                    }
                    if find.query != before {
                        find.current = 0;
                        self.reveal_match = true;
                    }
                    if response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                        let backwards = ui.input(|input| input.modifiers.shift);
                        commands.push(Command::FindStep(!backwards));
                        response.request_focus();
                    }
                    let status = match (find.query.is_empty(), count) {
                        (true, _) => String::new(),
                        (false, 0) => "Aucun résultat".to_owned(),
                        (false, count) => format!("{} sur {count}", find.current.min(count - 1) + 1),
                    };
                    ui.label(RichText::new(status).color(style::TEXT_SECONDARY));
                    if glyph_button(ui, Glyph::Up, false, "Précédent (Maj+Entrée)").clicked() {
                        commands.push(Command::FindStep(false));
                    }
                    if glyph_button(ui, Glyph::Down, false, "Suivant (Entrée)").clicked() {
                        commands.push(Command::FindStep(true));
                    }
                    if ui
                        .selectable_label(find.match_case, "Aa")
                        .on_hover_text("Respecter la casse")
                        .clicked()
                    {
                        find.match_case = !find.match_case;
                        find.current = 0;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if glyph_button(ui, Glyph::Close, false, "Fermer (Échap)").clicked() {
                            commands.push(Command::CloseFind);
                        }
                        if !find.replace && ui.button("Remplacer…").on_hover_text("Ctrl+H").clicked() {
                            find.replace = true;
                        }
                    });
                });
                if find.replace {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut find.replacement)
                                .hint_text("Remplacer par")
                                .desired_width(220.0),
                        );
                        let enabled = count > 0;
                        if ui.add_enabled(enabled, egui::Button::new("Remplacer")).clicked() {
                            commands.push(Command::ReplaceOne);
                        }
                        if ui
                            .add_enabled(enabled, egui::Button::new("Tout remplacer"))
                            .clicked()
                        {
                            commands.push(Command::ReplaceAll);
                        }
                    });
                }
            });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        let document = self.document();
        let text = &document.contents;
        let words = text.split_whitespace().count();
        let characters = text.chars().count();
        let line_ending = if text.contains("\r\n") { "CRLF" } else { "LF" };
        let (line, column) = self.cursor;
        let message = self.message.clone();
        let wrap = self.wrap;
        style::status_bar(ui, |ui| {
            ui.label(format!("Ligne {line}, colonne {column}"));
            ui.label("·");
            ui.label(format!(
                "{words} mot{} · {characters} caractère{}",
                plural(words),
                plural(characters)
            ));
            if let Some((text, error)) = &message {
                ui.label("·");
                if *error {
                    ui.colored_label(style::ERROR, text);
                } else {
                    ui.label(text);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("UTF-8 · {line_ending}"));
                let wrap_text = if wrap {
                    "Retour à la ligne"
                } else {
                    "Sans retour à la ligne"
                };
                if ui
                    .add(egui::Button::new(wrap_text).frame_when_inactive(false).small())
                    .clicked()
                {
                    commands.push(Command::ToggleWrap);
                }
            });
        });
    }

    fn text_area(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let font = self.font_id();
        let (query, match_case, current) = match &self.find {
            Some(find) => (find.query.clone(), find.match_case, find.current),
            None => (String::new(), false, 0),
        };
        let wrap = self.wrap;
        let line_numbers = self.line_numbers;
        let document = &mut self.documents[self.active];
        let id = document.id;
        if let Some(selection) = self.pending_selection.take() {
            let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(selection));
            state.store(&ctx, id);
            ctx.memory_mut(|memory| memory.request_focus(id));
        }
        let lines = document.contents.matches('\n').count() + 1;
        let digit = ctx.fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
        let gutter = if line_numbers {
            lines.to_string().len().max(2) as f32 * digit + 26.0
        } else {
            0.0
        };

        let scroll = if wrap {
            egui::ScrollArea::vertical()
        } else {
            egui::ScrollArea::both()
        };
        let reveal = std::mem::take(&mut self.reveal_match);
        let mut cursor = self.cursor;
        scroll.auto_shrink(false).show(ui, |ui| {
            // Fond de la ligne courante, peint sous le texte.
            let line_slot = ui.painter().add(egui::Shape::Noop);
            let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, wrap_width: f32| {
                let text = buffer.as_str();
                let mut job = LayoutJob::default();
                job.wrap.max_width = if wrap { wrap_width } else { f32::INFINITY };
                let normal = TextFormat::simple(font.clone(), style::TEXT);
                let mut last = 0;
                for (index, range) in find_matches(text, &query, match_case).into_iter().enumerate() {
                    job.append(&text[last..range.start], 0.0, normal.clone());
                    let mut format = normal.clone();
                    format.background = if index == current { MATCH_CURRENT } else { MATCH };
                    job.append(&text[range.clone()], 0.0, format);
                    last = range.end;
                }
                job.append(&text[last..], 0.0, normal);
                ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
            };
            let output = egui::TextEdit::multiline(&mut document.contents)
                .id(id)
                .font(font.clone())
                .frame(Frame::new().inner_margin(Margin {
                    left: (gutter + 14.0).min(127.0) as i8,
                    right: 18,
                    top: 12,
                    bottom: 12,
                }))
                .desired_width(f32::INFINITY)
                .min_size(ui.available_size())
                .hint_text(RichText::new("Commence à écrire…").color(style::TEXT_SECONDARY))
                .lock_focus(true)
                .layouter(&mut layouter)
                .show(ui);
            let rect = output.response.response.rect;
            let galley = &output.galley;
            let origin = output.galley_pos;

            // Ligne et colonne du curseur.
            let cursor_index = output.cursor_range.map(|range| range.primary.index.0);
            if let Some(index) = cursor_index {
                cursor = line_column(&document.contents, index);
            }

            // Numéros de ligne et ligne courante.
            let clip = ui.clip_rect();
            let painter = ui.painter();
            if line_numbers {
                let gutter_rect =
                    egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + gutter, rect.bottom()));
                painter.rect_filled(gutter_rect, 0.0, GUTTER);
                painter.vline(
                    gutter_rect.right(),
                    gutter_rect.y_range(),
                    Stroke::new(1.0, style::SEPARATOR),
                );
            }
            let mut line = 1;
            let mut line_start = true;
            let mut current_rows: Option<egui::Rangef> = None;
            for row in &galley.rows {
                let row_rect = row.rect().translate(origin.to_vec2());
                let is_current = line == cursor.0 && cursor_index.is_some();
                if is_current {
                    current_rows = Some(match current_rows {
                        Some(range) => range.union(row_rect.y_range()),
                        None => row_rect.y_range(),
                    });
                }
                if line_numbers
                    && line_start
                    && row_rect.bottom() >= clip.top()
                    && row_rect.top() <= clip.bottom()
                {
                    let color = if is_current {
                        style::TEXT
                    } else {
                        Color32::from_gray(0xA8)
                    };
                    painter.text(
                        egui::pos2(rect.left() + gutter - 10.0, row_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        line.to_string(),
                        FontId::monospace(font.size * 0.85),
                        color,
                    );
                }
                line_start = row.ends_with_newline;
                if row.ends_with_newline {
                    line += 1;
                }
            }
            if let Some(range) = current_rows {
                let band = egui::Rect::from_x_y_ranges(rect.left() + gutter + 1.0..=rect.right(), range);
                painter.set(line_slot, egui::Shape::rect_filled(band, 0.0, CURRENT_LINE));
            }

            // Occurrence courante visible.
            if reveal {
                let text = &document.contents;
                if let Some(range) = find_matches(text, &query, match_case).get(current) {
                    let index = text[..range.start].chars().count();
                    let target = galley
                        .pos_from_cursor(CCursor::new(index))
                        .translate(origin.to_vec2());
                    ui.scroll_to_rect(target.expand(24.0), Some(egui::Align::Center));
                }
            }
        });
        self.cursor = cursor;
    }

    /// Sélecteur de Fichiers en feuille, par-dessus la fenêtre.
    fn picker_sheet(&mut self, ctx: &egui::Context) {
        let Some((purpose, picker)) = &mut self.picker else {
            return;
        };
        let screen = ctx.content_rect();
        let size = egui::vec2(
            (screen.width() - 48.0).clamp(420.0, 900.0),
            (screen.height() - 64.0).clamp(300.0, 560.0),
        );
        egui::Modal::new(egui::Id::unique("textedit-selecteur"))
            .frame(
                Frame::new()
                    .fill(style::BACKGROUND)
                    .corner_radius(12.0)
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_black_alpha(60),
                    }),
            )
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("selecteur"));
                child.set_clip_rect(rect);
                picker.update(&mut child);
            });
        let purpose = *purpose;
        let Some(choice) = picker.take_choice() else {
            return;
        };
        self.picker = None;
        let Some(path) = choice else {
            if purpose != Purpose::Open {
                self.report("Enregistrement annulé", false);
            }
            return;
        };
        match purpose {
            Purpose::Open => self.open(path),
            Purpose::SaveAs(index, then) => {
                if let Some(document) = self.documents.get_mut(index) {
                    document.path = Some(path);
                    self.save(index, then);
                }
            }
        }
    }

    fn ask_dialog(&self, ctx: &egui::Context, commands: &mut Vec<Command>) {
        let Some(ask) = self.ask else { return };
        let (title, detail) = match ask {
            Ask::Close(index) => (
                format!(
                    "Enregistrer « {} » avant de le fermer ?",
                    self.documents[index].name()
                ),
                "Sinon, ses modifications seront perdues.".to_owned(),
            ),
            Ask::Quit => {
                let count = self.documents.iter().filter(|d| d.dirty()).count();
                (
                    if count == 1 {
                        "Enregistrer les modifications avant de quitter ?".to_owned()
                    } else {
                        format!("Enregistrer les {count} documents modifiés avant de quitter ?")
                    },
                    "Sinon, les modifications seront perdues.".to_owned(),
                )
            }
        };
        let modal = egui::Modal::new(egui::Id::unique("textedit-question")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.heading(title);
            ui.label(RichText::new(detail).color(style::TEXT_SECONDARY));
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.button("Ne pas enregistrer").clicked() {
                    commands.push(Command::Answer(Some(false)));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if style::suggested_button(ui, "Enregistrer").clicked() {
                        commands.push(Command::Answer(Some(true)));
                    }
                    if ui.button("Annuler").clicked() {
                        commands.push(Command::Answer(None));
                    }
                });
            });
        });
        if modal.should_close() {
            commands.push(Command::Answer(None));
        }
    }
}

/// Raccourcis clavier, consommés avant que le champ de texte ne les voie.
fn shortcuts(ctx: &egui::Context, finding: bool, commands: &mut Vec<Command>) {
    let ctrl = Modifiers::COMMAND;
    let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
    ctx.input_mut(|input| {
        let mut take =
            |modifiers: Modifiers, key: Key| input.consume_shortcut(&KeyboardShortcut::new(modifiers, key));
        let bindings = [
            (ctrl_shift, Key::S, Command::SaveAs),
            (ctrl_shift, Key::Tab, Command::NextTab(false)),
            (ctrl, Key::S, Command::Save),
            (ctrl, Key::O, Command::Open),
            (ctrl, Key::N, Command::New),
            (ctrl, Key::T, Command::New),
            (ctrl, Key::W, Command::Close(usize::MAX)),
            (ctrl, Key::F, Command::Find(false)),
            (ctrl, Key::H, Command::Find(true)),
            (ctrl, Key::Tab, Command::NextTab(true)),
            (ctrl, Key::PageDown, Command::NextTab(true)),
            (ctrl, Key::PageUp, Command::NextTab(false)),
            (ctrl, Key::Plus, Command::Zoom(1)),
            (ctrl, Key::Equals, Command::Zoom(1)),
            (ctrl, Key::Minus, Command::Zoom(-1)),
            (ctrl, Key::Num0, Command::Zoom(0)),
        ];
        for (modifiers, key, command) in bindings {
            if take(modifiers, key) {
                commands.push(command);
            }
        }
        if take(Modifiers::NONE, Key::F3) {
            commands.push(Command::FindStep(true));
        }
        if take(Modifiers::SHIFT, Key::F3) {
            commands.push(Command::FindStep(false));
        }
        if finding && take(Modifiers::NONE, Key::Escape) {
            commands.push(Command::CloseFind);
        }
    });
}

/// Occurrences de `query` dans `text` (positions en octets), sans chevauchement.
fn find_matches(text: &str, query: &str, match_case: bool) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let found = |haystack: &str, needle: &str| -> Vec<Range<usize>> {
        haystack
            .match_indices(needle)
            .map(|(start, _)| start..start + needle.len())
            .collect()
    };
    if match_case {
        return found(text, query);
    }
    // Sans la casse : on compare les minuscules, si elles gardent les positions intactes
    // (c'est le cas du français) ; sinon, recherche exacte.
    let (lower_text, lower_query) = (text.to_lowercase(), query.to_lowercase());
    if lower_text.len() != text.len() || lower_query.len() != query.len() {
        return found(text, query);
    }
    found(&lower_text, &lower_query)
        .into_iter()
        .filter(|range| text.is_char_boundary(range.start) && text.is_char_boundary(range.end))
        .collect()
}

/// Ligne et colonne (à partir de 1) du caractère d'indice `index`.
fn line_column(text: &str, index: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for character in text.chars().take(index) {
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Chemin avec `~` pour le dossier personnel.
fn short_path(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) => {
            let rest = path.strip_prefix(&home).unwrap_or(path);
            if rest.as_os_str().is_empty() {
                "~".to_owned()
            } else {
                format!("~/{}", rest.display())
            }
        }
        _ => path.display().to_string(),
    }
}

/// `text` raccourci avec « … » pour tenir dans `width`.
fn elide(ui: &egui::Ui, text: &str, width: f32, size: f32) -> String {
    let font = FontId::proportional(size);
    let measure = |text: String| {
        ui.painter()
            .layout_no_wrap(text, font.clone(), Color32::WHITE)
            .size()
            .x
    };
    if measure(text.to_owned()) <= width {
        return text.to_owned();
    }
    let mut shown: String = text.to_owned();
    while !shown.is_empty() && measure(format!("{shown}…")) > width {
        shown.pop();
    }
    format!("{}…", shown.trim_end())
}

fn plural(count: usize) -> &'static str {
    if count > 1 { "s" } else { "" }
}

#[derive(Clone, Copy)]
enum Glyph {
    Plus,
    Search,
    Close,
    Up,
    Down,
    More,
}

fn paint_glyph(ui: &egui::Ui, glyph: Glyph, rect: egui::Rect, color: Color32) {
    let painter = ui.painter();
    let c = rect.center();
    let stroke = Stroke::new(1.7, color);
    let p = |x: f32, y: f32| egui::pos2(c.x + x, c.y + y);
    match glyph {
        Glyph::Plus => {
            painter.line_segment([p(-6.0, 0.0), p(6.0, 0.0)], stroke);
            painter.line_segment([p(0.0, -6.0), p(0.0, 6.0)], stroke);
        }
        Glyph::Search => {
            painter.circle_stroke(p(-1.5, -1.5), 5.0, stroke);
            painter.line_segment([p(2.2, 2.2), p(6.5, 6.5)], Stroke::new(2.0, color));
        }
        Glyph::Close => {
            painter.line_segment([p(-4.0, -4.0), p(4.0, 4.0)], stroke);
            painter.line_segment([p(4.0, -4.0), p(-4.0, 4.0)], stroke);
        }
        Glyph::Up => {
            painter.line(vec![p(-5.0, 3.0), p(0.0, -2.5), p(5.0, 3.0)], stroke);
        }
        Glyph::More => {
            for x in [-6.0, 0.0, 6.0] {
                painter.circle_filled(p(x, 0.0), 1.8, color);
            }
        }
        Glyph::Down => {
            painter.line(vec![p(-5.0, -2.5), p(0.0, 3.0), p(5.0, -2.5)], stroke);
        }
    }
}

fn glyph_button(ui: &mut egui::Ui, glyph: Glyph, active: bool, hint: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(30.0, 28.0), Sense::click());
    if active {
        ui.painter().rect_filled(rect, 7.0, Color32::from_black_alpha(22));
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 7.0, Color32::from_black_alpha(12));
    }
    paint_glyph(ui, glyph, rect, style::TEXT);
    response.on_hover_text(hint)
}

#[cfg(test)]
mod tests {
    use std::{env, fs, time::SystemTime};

    use super::*;

    fn scratch() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = env::temp_dir().join(format!("ferrite-textedit-{}-{unique}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn textedit_ouvre_et_enregistre_un_fichier_utf8() {
        let directory = scratch();
        let path = directory.join("note.txt");
        fs::write(&path, "Bonjour, ferrite !").unwrap();

        let mut editor = EditorState::default();
        editor.open(path.clone());
        assert_eq!(editor.documents.len(), 1, "le document vide laisse sa place");
        assert_eq!(editor.document().contents, "Bonjour, ferrite !");
        assert_eq!(editor.window_title(), "note.txt — TextEdit");
        editor.documents[0].contents = "Texte modifié : été".into();
        assert_eq!(editor.window_title(), "• note.txt — TextEdit");
        assert!(editor.save(0, Then::Nothing));
        assert_eq!(fs::read_to_string(&path).unwrap(), "Texte modifié : été");
        assert!(!editor.document().dirty());
        // Rouvrir le même fichier revient à son onglet.
        editor.new_document();
        editor.open(path);
        assert_eq!((editor.documents.len(), editor.active), (2, 0));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn enregistrer_sans_nom_ouvre_le_selecteur() {
        let mut editor = EditorState::default();
        editor.documents[0].contents = "brouillon".into();
        assert!(!editor.save(0, Then::Nothing));
        assert!(matches!(
            editor.picker,
            Some((Purpose::SaveAs(0, Then::Nothing), _))
        ));
    }

    #[test]
    fn fermer_un_document_modifie_demande_confirmation() {
        let mut editor = EditorState::default();
        editor.new_document();
        editor.documents[1].contents = "à garder".into();
        editor.run(Command::Close(1));
        assert_eq!(editor.ask, Some(Ask::Close(1)));
        assert_eq!(editor.documents.len(), 2);
        editor.run(Command::Answer(Some(false)));
        assert_eq!(editor.documents.len(), 1);
        assert_eq!(editor.active, 0);
    }

    #[test]
    fn recherche_et_remplacement() {
        let text = "Été, été, ÉTÉ et etage";
        assert_eq!(find_matches(text, "été", false).len(), 3);
        assert_eq!(find_matches(text, "été", true).len(), 1);
        let mut editor = EditorState::default();
        editor.documents[0].contents = "un chat, deux chats".into();
        editor.find = Some(Find {
            query: "chat".into(),
            replacement: "chien".into(),
            ..Find::default()
        });
        editor.run(Command::ReplaceOne);
        assert_eq!(editor.document().contents, "un chien, deux chats");
        editor.run(Command::ReplaceAll);
        assert_eq!(editor.document().contents, "un chien, deux chiens");
    }

    #[test]
    fn position_du_curseur() {
        assert_eq!(line_column("ab\ncdé\nf", 0), (1, 1));
        assert_eq!(line_column("ab\ncdé\nf", 5), (2, 3));
        assert_eq!(line_column("ab\ncdé\nf", 7), (3, 1));
    }
}
