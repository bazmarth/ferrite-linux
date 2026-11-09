//! TextEdit : éditeur de texte UTF-8, à la manière d'un éditeur de code moderne.
//!
//! - barre d'activité, explorateur du dossier (arborescence, recherche de fichiers par
//!   leur nom, favoris), onglets, éditeur coloré et aperçu Markdown côte à côte ;
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

mod highlight;
mod icons;
mod remote;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
};

use eframe::egui::{
    self, Color32, FontId, Frame, Key, KeyboardShortcut, Margin, Modifiers, RichText, Sense, Stroke,
    text::{CCursor, CCursorRange, LayoutJob, TextFormat},
};

use self::icons::{FileKind, Icon};
use crate::{
    files::{FilesState, PickerMode},
    markdown, style,
};

const FONT_SIZES: [f32; 7] = [11.0, 12.0, 13.0, 14.0, 16.0, 18.0, 22.0];
const DEFAULT_FONT: usize = 3;

fn current_line() -> Color32 {
    style::content_gray()
}

fn code_block() -> Color32 {
    style::content_gray()
}

fn line_number() -> Color32 {
    style::text_secondary()
}

fn border() -> Color32 {
    style::separator()
}

fn rail() -> Color32 {
    style::header_color()
}

fn explorer() -> Color32 {
    style::background()
}

fn tab_strip() -> Color32 {
    style::content_gray()
}

fn field() -> Color32 {
    style::content_gray()
}

fn selected() -> Color32 {
    style::accent().lerp_to_gamma(style::background(), 0.86)
}

fn icon_color() -> Color32 {
    style::text_secondary()
}

fn body() -> Color32 {
    style::primary_text()
}
const RAIL_WIDTH: f32 = 56.0;
const EXPLORER_WIDTH: f32 = 272.0;
const TAB_HEIGHT: f32 = 44.0;
/// Hauteur d'une ligne de l'explorateur (fichier, dossier, en-tête des favoris).
const ROW_HEIGHT: f32 = 30.0;
/// Marge au-dessus de l'en-tête des favoris, sous le trait de séparation.
const FAVORITES_TOP: i8 = 6;
/// Marge sous la liste des favoris.
const FAVORITES_BOTTOM: i8 = 12;
const SEARCH_ID: &str = "textedit-recherche-fichiers";

#[derive(Debug)]
struct Document {
    path: Option<PathBuf>,
    contents: String,
    /// Contenu sur le disque : le document est modifié s'il en diffère.
    saved: String,
    id: egui::Id,
    markdown_mode: MarkdownMode,
    version: u64,
    markdown_preview: Option<(u64, Preview)>,
}

/// Aperçu Markdown analysé : en-tête YAML et blocs.
#[derive(Debug)]
struct Preview {
    meta: Vec<(String, String)>,
    blocks: Vec<markdown::Block>,
}

impl Document {
    fn new(path: Option<PathBuf>, contents: String) -> Self {
        let markdown_mode = if path.as_deref().is_some_and(is_markdown_path) {
            MarkdownMode::Split
        } else {
            MarkdownMode::Edit
        };
        Self {
            path,
            saved: contents.clone(),
            contents,
            id: egui::Id::unique(("document", next_id())),
            markdown_mode,
            version: 0,
            markdown_preview: None,
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

    fn is_markdown(&self) -> bool {
        self.path.as_deref().is_some_and(is_markdown_path)
    }

    fn kind(&self) -> FileKind {
        self.path.as_deref().map_or(FileKind::Text, FileKind::of)
    }
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown"))
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
    /// Donner le focus au champ de remplacement à la prochaine image.
    replace_focus: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    New,
    Open,
    OpenPath(PathBuf),
    Save,
    SaveAs,
    Undo,
    Redo,
    Close(usize),
    Select(usize),
    NextTab(bool),
    SetMode(MarkdownMode),
    Find(bool),
    CloseFind,
    FindStep(bool),
    ReplaceOne,
    ReplaceAll,
    Zoom(i32),
    ToggleWrap,
    ToggleLineNumbers,
    Format(FormatCommand),
    ToggleExplorer,
    ToggleFolder(PathBuf),
    ToggleFavorites,
    /// Affiche ce dossier dans l'explorateur.
    SetRoot(PathBuf),
    /// Montre ce chemin dans l'application Fichiers.
    ShowInFiles(PathBuf),
    /// Copie ce chemin dans le presse-papiers.
    CopyPath(PathBuf),
    /// Referme tous les dossiers de l'arborescence.
    CollapseAll,
    FocusSearch,
    ClearSearch,
    SetPreviewTab(PreviewTab),
    /// Réponse à la question : enregistrer, ne pas enregistrer, annuler.
    Answer(Option<bool>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormatCommand {
    Paragraph,
    Heading1,
    Heading2,
    Bold,
    Italic,
    BoldItalic,
    BulletList,
    NumberList,
    AlignLeft,
    AlignCenter,
    AlignRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkdownMode {
    Edit,
    Split,
    Preview,
}

/// Onglet de la colonne d'aperçu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewTab {
    Preview,
    Info,
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
    /// Recherche de fichiers par leur nom, dans le dossier de l'explorateur.
    workspace_search: String,
    /// Résultats de la dernière recherche : texte cherché, dossier, fichiers trouvés.
    search_cache: Option<(String, PathBuf, Vec<PathBuf>)>,
    focus_search: bool,
    explorer_visible: bool,
    /// Dossier de l'explorateur ; par défaut, celui du premier document ouvert.
    root: Option<PathBuf>,
    /// Dossiers ouverts dans l'arborescence.
    expanded: HashSet<PathBuf>,
    /// Contenu des dossiers déjà lus, relu au bout de deux secondes : l'arborescence ne
    /// relit pas le disque à chaque image.
    listings: RefCell<HashMap<PathBuf, Listing>>,
    favorites_open: bool,
    preview_tab: PreviewTab,
    /// Images de l'aperçu, par chemin (rien : illisible).
    images: HashMap<PathBuf, Option<(egui::TextureHandle, f32)>>,
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
            workspace_search: String::new(),
            search_cache: None,
            focus_search: false,
            explorer_visible: true,
            root: None,
            expanded: HashSet::new(),
            listings: RefCell::default(),
            favorites_open: true,
            preview_tab: PreviewTab::Preview,
            images: HashMap::new(),
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

    /// Dossier affiché par l'explorateur.
    fn root(&self) -> PathBuf {
        self.root
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    /// Montre `path` dans l'explorateur : son dossier devient la racine s'il est ailleurs,
    /// et les dossiers qui le contiennent s'ouvrent.
    fn reveal(&mut self, path: &Path) {
        let Some(parent) = path.parent() else { return };
        let root = match &self.root {
            Some(root) if path.starts_with(root) => root.clone(),
            _ => {
                self.root = Some(parent.to_path_buf());
                parent.to_path_buf()
            }
        };
        let mut folder = parent;
        while folder != root && folder.starts_with(&root) {
            self.expanded.insert(folder.to_path_buf());
            match folder.parent() {
                Some(up) => folder = up,
                None => break,
            }
        }
    }

    /// Ouvre `path` dans un onglet (ou revient à l'onglet qui l'affiche déjà).
    pub fn open(&mut self, path: PathBuf) {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if let Some(index) = self.documents.iter().position(|d| d.path.as_ref() == Some(&path)) {
            self.active = index;
            self.reveal(&path);
            return;
        }
        self.reveal(&path);
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
                self.listings.get_mut().clear();
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
            .map(Path::to_path_buf)
            // Nouveau document : il s'enregistre d'abord dans le dossier de l'explorateur.
            .or_else(|| Some(self.root()));
        let name = match &document.path {
            Some(_) => document.name(),
            None => "Sans titre.txt".to_owned(),
        };
        let picker = FilesState::picker(PickerMode::Save, start.as_deref(), &name);
        self.picker = Some((Purpose::SaveAs(index, then), picker));
    }

    fn text_edit_state(&self, ctx: &egui::Context) -> Option<egui::widgets::text_edit::TextEditState> {
        egui::TextEdit::load_state(ctx, self.document().id)
    }

    fn checkpoint_text_edit(&self, ctx: &egui::Context) {
        let document = self.document();
        let Some(mut state) = self.text_edit_state(ctx) else {
            return;
        };
        let cursor = state
            .cursor
            .char_range()
            .unwrap_or_else(|| CCursorRange::one(CCursor::end_of_str(&document.contents)));
        let mut undoer = state.undoer();
        undoer.add_undo(&(cursor, document.contents.clone()));
        state.set_undoer(undoer);
        state.store(ctx, document.id);
    }

    fn edit_history(&mut self, ctx: &egui::Context, redo: bool) {
        let document = &mut self.documents[self.active];
        let Some(mut state) = egui::TextEdit::load_state(ctx, document.id) else {
            return;
        };
        let cursor = state
            .cursor
            .char_range()
            .unwrap_or_else(|| CCursorRange::one(CCursor::end_of_str(&document.contents)));
        let current = (cursor, document.contents.clone());
        let mut undoer = state.undoer();
        let next = if redo {
            undoer.redo(&current)
        } else {
            undoer.undo(&current)
        }
        .cloned();
        if let Some((cursor, contents)) = next {
            document.contents = contents;
            document.version += 1;
            state.cursor.set_char_range(Some(cursor));
            state.set_undoer(undoer);
            state.store(ctx, document.id);
        }
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

    fn selection_char_range(&self, ctx: &egui::Context) -> Option<CCursorRange> {
        let state = self.text_edit_state(ctx)?;
        state.cursor.char_range()
    }

    fn char_to_byte(text: &str, char_index: usize) -> usize {
        text.char_indices()
            .nth(char_index)
            .map(|(byte, _)| byte)
            .unwrap_or(text.len())
    }

    fn format_heading(&mut self, ctx: &egui::Context, level: usize) {
        let document = self.document();
        let range = self.selection_char_range(ctx);
        let start_char = range
            .as_ref()
            .map(|range| range.primary.index.0.min(range.secondary.index.0))
            .unwrap_or(0);
        let start = Self::char_to_byte(&document.contents, start_char);
        let line_start = document.contents[..start]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let line_end = document.contents[start..]
            .find('\n')
            .map_or(document.contents.len(), |index| start + index);
        let line = document.contents[line_start..line_end].to_owned();
        let trimmed = line.trim_start();
        let without_heading = trimmed.trim_start_matches('#').trim_start();
        let replacement = match level {
            1 => format!("# {without_heading}"),
            2 => format!("## {without_heading}"),
            _ => without_heading.to_owned(),
        };
        self.checkpoint_text_edit(ctx);
        let document = &mut self.documents[self.active];
        document
            .contents
            .replace_range(line_start..line_end, &replacement);
        document.version += 1;
        self.pending_selection = Some(CCursorRange::one(CCursor::new(
            line_start + replacement.chars().count(),
        )));
    }

    fn format_inline(&mut self, ctx: &egui::Context, marker: &str) {
        let document = self.document();
        let range = self.selection_char_range(ctx).unwrap_or_else(|| {
            let index = CCursor::new(self.cursor.0.saturating_sub(1));
            CCursorRange::one(index)
        });
        let start_char = range.primary.index.0.min(range.secondary.index.0);
        let end_char = range.primary.index.0.max(range.secondary.index.0);
        let start = Self::char_to_byte(&document.contents, start_char);
        let end = Self::char_to_byte(&document.contents, end_char);
        self.checkpoint_text_edit(ctx);
        let document = &mut self.documents[self.active];
        document.contents.insert_str(end, marker);
        document.contents.insert_str(start, marker);
        document.version += 1;
        let marker_chars = marker.chars().count();
        if start_char == end_char {
            let pos = start_char + marker_chars;
            self.pending_selection = Some(CCursorRange::one(CCursor::new(pos)));
        } else {
            self.pending_selection = Some(CCursorRange::two(
                CCursor::new(start_char + marker_chars),
                CCursor::new(end_char + marker_chars),
            ));
        }
    }

    fn format_lines(&mut self, ctx: &egui::Context, numbered: bool) {
        let document = self.document();
        let range = self.selection_char_range(ctx);
        let start_char = range
            .as_ref()
            .map(|range| range.primary.index.0.min(range.secondary.index.0))
            .unwrap_or(0);
        let end_char = range
            .as_ref()
            .map(|range| range.primary.index.0.max(range.secondary.index.0))
            .unwrap_or(start_char);
        let start = Self::char_to_byte(&document.contents, start_char);
        let end = Self::char_to_byte(&document.contents, end_char);
        let block_start = document.contents[..start]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let block_end = document.contents[end..]
            .find('\n')
            .map_or(document.contents.len(), |index| end + index);
        let block = document.contents[block_start..block_end].to_owned();
        self.checkpoint_text_edit(ctx);
        let mut output = String::new();
        for (index, line) in block.split('\n').enumerate() {
            if index > 0 {
                output.push('\n');
            }
            if numbered {
                output.push_str(&format!("{}. ", index + 1));
            } else {
                output.push_str("- ");
            }
            output.push_str(line);
        }
        let document = &mut self.documents[self.active];
        document.contents.replace_range(block_start..block_end, &output);
        document.version += 1;
        let output_start_char = document.contents[..block_start].chars().count();
        self.pending_selection = Some(CCursorRange::one(CCursor::new(
            output_start_char + output.chars().count(),
        )));
    }

    fn format_alignment(&mut self, ctx: &egui::Context, alignment: &str) {
        let document = self.document();
        let range = self.selection_char_range(ctx);
        let start_char = range
            .as_ref()
            .map(|range| range.primary.index.0.min(range.secondary.index.0))
            .unwrap_or(0);
        let end_char = range
            .as_ref()
            .map(|range| range.primary.index.0.max(range.secondary.index.0))
            .unwrap_or(start_char);
        let start = Self::char_to_byte(&document.contents, start_char);
        let mut end = Self::char_to_byte(&document.contents, end_char);
        if start == end {
            let line_start = document.contents[..start]
                .rfind('\n')
                .map_or(0, |index| index + 1);
            end = document.contents[start..]
                .find('\n')
                .map_or(document.contents.len(), |index| start + index);
            let line = document.contents[line_start..end].to_owned();
            let replacement = format!("::: align={}\n{}\n:::", alignment, line);
            self.checkpoint_text_edit(ctx);
            let document = &mut self.documents[self.active];
            document.contents.replace_range(line_start..end, &replacement);
            document.version += 1;
            self.pending_selection = Some(CCursorRange::one(CCursor::new(
                document.contents[..line_start].chars().count() + replacement.chars().count(),
            )));
        } else {
            let selected = document.contents[start..end].to_owned();
            let replacement = format!("::: align={}\n{}\n:::", alignment, selected);
            self.checkpoint_text_edit(ctx);
            let document = &mut self.documents[self.active];
            document.contents.replace_range(start..end, &replacement);
            document.version += 1;
            self.pending_selection = Some(CCursorRange::one(CCursor::new(
                document.contents[..start].chars().count() + replacement.chars().count(),
            )));
        }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::New => self.new_document(),
            Command::Open => self.open_dialog(),
            Command::OpenPath(path) => self.open(path),
            Command::Save => {
                self.save(self.active, Then::Nothing);
            }
            Command::SaveAs => self.save_as(self.active, Then::Nothing),
            Command::Undo => self.edit_history(ctx, false),
            Command::Redo => self.edit_history(ctx, true),
            // `usize::MAX` : l'onglet actif (Ctrl+W).
            Command::Close(usize::MAX) => self.close(self.active, false),
            Command::Close(index) => self.close(index, false),
            Command::Select(index) => self.active = index.min(self.documents.len() - 1),
            Command::SetMode(mode) => self.documents[self.active].markdown_mode = mode,
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
                find.replace_focus = replace;
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
                    self.checkpoint_text_edit(ctx);
                    let replacement = find.replacement.clone();
                    let document = &mut self.documents[self.active];
                    let range_start = range.start;
                    document.contents.replace_range(range, &replacement);
                    document.version += 1;
                    let start_char = document.contents[..range_start].chars().count();
                    let end_char = start_char + replacement.chars().count();
                    self.pending_selection = Some(CCursorRange::two(
                        CCursor::new(start_char),
                        CCursor::new(end_char),
                    ));
                    self.reveal_match = true;
                    if let Some(find) = &mut self.find {
                        let left = matches.len().saturating_sub(1);
                        find.current = if left == 0 { 0 } else { find.current.min(left) };
                    }
                }
            }
            Command::ReplaceAll => {
                let matches = self.matches();
                if let Some(find) = &self.find
                    && !matches.is_empty()
                {
                    self.checkpoint_text_edit(ctx);
                    let replacement = find.replacement.clone();
                    let document = &mut self.documents[self.active];
                    for range in matches.iter().rev() {
                        document.contents.replace_range(range.clone(), &replacement);
                    }
                    document.version += 1;
                    let count = matches.len();
                    if let Some(find) = &mut self.find {
                        find.current = 0;
                    }
                    self.pending_selection = None;
                    self.reveal_match = true;
                    self.report(
                        format!("{count} remplacement{}", if count > 1 { "s" } else { "" }),
                        false,
                    );
                }
            }
            Command::Format(format) => match format {
                FormatCommand::Paragraph => self.format_heading(ctx, 0),
                FormatCommand::Heading1 => self.format_heading(ctx, 1),
                FormatCommand::Heading2 => self.format_heading(ctx, 2),
                FormatCommand::Bold => self.format_inline(ctx, "**"),
                FormatCommand::Italic => self.format_inline(ctx, "*"),
                FormatCommand::BoldItalic => self.format_inline(ctx, "***"),
                FormatCommand::BulletList => self.format_lines(ctx, false),
                FormatCommand::NumberList => self.format_lines(ctx, true),
                FormatCommand::AlignLeft => self.format_alignment(ctx, "left"),
                FormatCommand::AlignCenter => self.format_alignment(ctx, "center"),
                FormatCommand::AlignRight => self.format_alignment(ctx, "right"),
            },
            Command::Zoom(step) => {
                self.font = match step {
                    0 => DEFAULT_FONT,
                    step => (self.font as i32 + step).clamp(0, FONT_SIZES.len() as i32 - 1) as usize,
                };
            }
            Command::ToggleWrap => self.wrap = !self.wrap,
            Command::ToggleExplorer => self.explorer_visible = !self.explorer_visible,
            Command::ToggleFolder(path) => {
                if !self.expanded.remove(&path) {
                    self.expanded.insert(path);
                }
            }
            Command::ToggleFavorites => self.favorites_open = !self.favorites_open,
            Command::SetRoot(path) => {
                self.root = Some(path);
                self.workspace_search.clear();
                self.explorer_visible = true;
            }
            Command::ShowInFiles(path) => {
                let result = ferrite_system::command::current_exe().and_then(|exe| {
                    let mut command = std::process::Command::new(exe);
                    command.arg("files").arg(&path);
                    ferrite_system::command::spawn_detached(command)
                });
                if let Err(error) = result {
                    self.report(format!("Ouverture de Fichiers impossible : {error}"), true);
                }
            }
            Command::CopyPath(path) => {
                ctx.copy_text(path.display().to_string());
                self.report("Chemin copié", false);
            }
            Command::CollapseAll => self.expanded.clear(),
            Command::FocusSearch => {
                self.explorer_visible = true;
                self.focus_search = true;
            }
            Command::ClearSearch => self.workspace_search.clear(),
            Command::SetPreviewTab(tab) => self.preview_tab = tab,
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
            shortcuts(
                &ctx,
                self.find.is_some(),
                self.document().is_markdown(),
                &mut commands,
            );
        }
        let title = self.window_title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        // Comme la maquette : barre de titre et barre d'état sur toute la largeur, puis la
        // barre d'activité, l'explorateur, les onglets et enfin l'éditeur et son aperçu.
        style::header(ui, |ui| self.header(ui, &mut commands));
        self.status_bar(ui, &mut commands);
        self.activity_rail(ui, &mut commands);
        if self.explorer_visible && ui.available_width() >= 640.0 {
            self.explorer(ui, &mut commands);
        }
        self.tabs(ui, &mut commands);
        if self.find.is_some() {
            self.find_bar(ui, &mut commands);
        }
        // Trop étroit pour deux colonnes lisibles : le texte seul.
        let mode = match (self.document().is_markdown(), self.document().markdown_mode) {
            (false, _) => MarkdownMode::Edit,
            (true, MarkdownMode::Split) if ui.available_width() < 620.0 => MarkdownMode::Edit,
            (true, mode) => mode,
        };
        match mode {
            MarkdownMode::Edit => self.editor_pane(ui),
            MarkdownMode::Split => {
                let width = (ui.available_width() / 2.0).round();
                let available = ui.available_width();
                egui::Panel::right(ui.scope_id().with("textedit-apercu"))
                    .resizable(true)
                    .default_size(width)
                    .min_size(220.0)
                    .max_size((available - 220.0).max(220.0))
                    .frame(Frame::new().fill(style::background()))
                    .show(ui, |ui| self.preview_pane(ui, mode, &mut commands));
                self.editor_pane(ui);
            }
            MarkdownMode::Preview => {
                egui::CentralPanel::no_frame()
                    .frame(Frame::new().fill(style::background()))
                    .show(ui, |ui| self.preview_pane(ui, mode, &mut commands));
            }
        }
        self.picker_sheet(&ctx);
        self.ask_dialog(&ctx, &mut commands);

        for command in commands {
            self.run(command, &ctx);
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

    /// Barre de titre : logo et nom, recherche de fichiers au milieu, boutons à droite.
    fn header(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        let full = ui.max_rect();
        let (logo, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
        icons::paint_logo(ui.painter(), logo);
        ui.add_space(4.0);
        ui.label(
            RichText::new("TextEdit")
                .size(14.0)
                .family(style::strong_family())
                .color(style::primary_text()),
        );

        // Champ centré dans la fenêtre, raccourci s'il manque de place à droite.
        const BUTTONS: f32 = 4.0 * 32.0 + 12.0;
        let wanted = (full.width() * 0.38).clamp(220.0, 460.0);
        let left = (full.center().x - wanted / 2.0).max(ui.cursor().left() + 16.0);
        let width = wanted.min(full.right() - BUTTONS - left);
        if width >= 150.0 {
            ui.add_space(left - ui.cursor().left());
            self.search_field(ui, width);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let more = icon_button(ui, Icon::Settings, false, "Menu");
            egui::Popup::menu(&more).show(|ui| self.menu(ui, commands));
            let save = icon_button(ui, Icon::Save, false, "Enregistrer (Ctrl+S)");
            if self.document().dirty() {
                ui.painter().circle_filled(
                    save.rect.right_top() + egui::vec2(-8.0, 8.0),
                    3.5,
                    style::accent(),
                );
            }
            if save.clicked() {
                commands.push(Command::Save);
            }
            if self.document().is_markdown() {
                let split = self.document().markdown_mode != MarkdownMode::Edit;
                if icon_button(ui, Icon::Split, split, "Aperçu côte à côte").clicked() {
                    commands.push(Command::SetMode(if split {
                        MarkdownMode::Edit
                    } else {
                        MarkdownMode::Split
                    }));
                }
            }
            if icon_button(ui, Icon::Sidebar, self.explorer_visible, "Explorateur").clicked() {
                commands.push(Command::ToggleExplorer);
            }
        });
    }

    /// Recherche de fichiers par leur nom dans le dossier de l'explorateur (Ctrl+P).
    fn search_field(&mut self, ui: &mut egui::Ui, width: f32) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::hover());
        ui.painter().rect(
            rect,
            14.0,
            field(),
            Stroke::new(1.0, border()),
            egui::StrokeKind::Inside,
        );
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + 17.0, rect.center().y),
                egui::vec2(15.0, 15.0),
            ),
            Icon::Search,
            style::text_secondary(),
        );
        let field = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 31.0, rect.top() + 2.0),
            egui::pos2(rect.right() - 14.0, rect.bottom() - 2.0),
        );
        let response = ui.put(
            field,
            egui::TextEdit::singleline(&mut self.workspace_search)
                .id(egui::Id::unique(SEARCH_ID))
                .frame(Frame::new())
                .align(egui::Align2::LEFT_CENTER)
                .hint_text(RichText::new("Rechercher dans les fichiers…").color(style::text_secondary())),
        );
        if std::mem::take(&mut self.focus_search) {
            response.request_focus();
        }
        if response.changed() && !self.workspace_search.is_empty() {
            self.explorer_visible = true;
        }
        if response.has_focus() && ui.input(|input| input.key_pressed(Key::Escape)) {
            self.workspace_search.clear();
            response.surrender_focus();
        }
    }

    fn menu(&self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        ui.set_min_width(250.0);
        let item =
            |ui: &mut egui::Ui, commands: &mut Vec<Command>, text: &str, shortcut: &str, command: Command| {
                if ui.add(egui::Button::new(text).shortcut_text(shortcut)).clicked() {
                    commands.push(command);
                    ui.close();
                }
            };
        item(ui, commands, "Nouveau", "Ctrl+N", Command::New);
        item(ui, commands, "Ouvrir…", "Ctrl+O", Command::Open);
        item(ui, commands, "Enregistrer", "Ctrl+S", Command::Save);
        item(ui, commands, "Enregistrer sous…", "Ctrl+Maj+S", Command::SaveAs);
        ui.separator();
        item(ui, commands, "Annuler", "Ctrl+Z", Command::Undo);
        item(ui, commands, "Rétablir", "Ctrl+Y", Command::Redo);
        ui.separator();
        item(ui, commands, "Rechercher", "Ctrl+F", Command::Find(false));
        item(ui, commands, "Remplacer", "Ctrl+H", Command::Find(true));
        item(
            ui,
            commands,
            "Rechercher un fichier",
            "Ctrl+P",
            Command::FocusSearch,
        );
        if self.document().is_markdown() {
            ui.separator();
            ui.menu_button("Mise en forme", |ui| {
                ui.set_min_width(220.0);
                for (text, shortcut, format) in [
                    ("Titre 1", "", FormatCommand::Heading1),
                    ("Titre 2", "", FormatCommand::Heading2),
                    ("Paragraphe", "", FormatCommand::Paragraph),
                    ("Gras", "Ctrl+B", FormatCommand::Bold),
                    ("Italique", "Ctrl+I", FormatCommand::Italic),
                    ("Gras et italique", "", FormatCommand::BoldItalic),
                    ("Liste à puces", "", FormatCommand::BulletList),
                    ("Liste numérotée", "", FormatCommand::NumberList),
                    ("Aligner à gauche", "", FormatCommand::AlignLeft),
                    ("Centrer", "", FormatCommand::AlignCenter),
                    ("Aligner à droite", "", FormatCommand::AlignRight),
                ] {
                    item(ui, commands, text, shortcut, Command::Format(format));
                }
            });
            ui.menu_button("Affichage Markdown", |ui| {
                for (text, mode) in [
                    ("Texte seul", MarkdownMode::Edit),
                    ("Texte et aperçu", MarkdownMode::Split),
                    ("Aperçu seul", MarkdownMode::Preview),
                ] {
                    if ui.radio(self.document().markdown_mode == mode, text).clicked() {
                        commands.push(Command::SetMode(mode));
                        ui.close();
                    }
                }
            });
        }
        ui.separator();
        // Cases dessinées par egui : la police n'a pas de coche « ✔ ».
        for (text, on, command) in [
            ("Explorateur", self.explorer_visible, Command::ToggleExplorer),
            ("Retour à la ligne", self.wrap, Command::ToggleWrap),
            ("Numéros de ligne", self.line_numbers, Command::ToggleLineNumbers),
        ] {
            let mut on = on;
            if ui.checkbox(&mut on, text).clicked() {
                commands.push(command);
                ui.close();
            }
        }
        item(ui, commands, "Agrandir le texte", "Ctrl++", Command::Zoom(1));
        item(ui, commands, "Réduire le texte", "Ctrl+-", Command::Zoom(-1));
        item(ui, commands, "Taille normale", "Ctrl+0", Command::Zoom(0));
        ui.separator();
        item(
            ui,
            commands,
            "Fermer l'onglet",
            "Ctrl+W",
            Command::Close(self.active),
        );
    }

    /// Colonne d'icônes à gauche : explorateur, recherche, et le menu en bas.
    fn activity_rail(&self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        egui::Panel::left(ui.scope_id().with("textedit-activite"))
            .resizable(false)
            .exact_size(RAIL_WIDTH)
            .frame(Frame::new().fill(rail()))
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter()
                    .vline(full.right() - 0.5, full.y_range(), Stroke::new(1.0, border()));
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    let searching = !self.workspace_search.is_empty();
                    let explorer = self.explorer_visible && !searching;
                    if rail_button(ui, Icon::Files, explorer, "Explorateur").clicked() {
                        commands.push(Command::ToggleExplorer);
                    }
                    if rail_button(ui, Icon::Search, searching, "Rechercher un fichier (Ctrl+P)").clicked() {
                        commands.push(Command::FocusSearch);
                    }
                });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(12.0);
                    let settings = rail_button(ui, Icon::Settings, false, "Menu");
                    egui::Popup::menu(&settings).show(|ui| self.menu(ui, commands));
                });
            });
    }

    /// Explorateur : arborescence du dossier ouvert (ou résultats de la recherche), et
    /// les dossiers favoris en bas.
    fn explorer(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        let root = self.root();
        let query = self.workspace_search.trim().to_owned();
        if !query.is_empty()
            && self
                .search_cache
                .as_ref()
                .is_none_or(|(known_query, known_root, _)| *known_query != query || *known_root != root)
        {
            let found = search_files(&root, &query);
            self.search_cache = Some((query.clone(), root.clone(), found));
        }
        egui::Panel::left(ui.scope_id().with("textedit-explorateur"))
            .resizable(true)
            .default_size(EXPLORER_WIDTH)
            .min_size(180.0)
            .max_size(480.0)
            .frame(Frame::new().fill(explorer()))
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter()
                    .vline(full.right() - 0.5, full.y_range(), Stroke::new(1.0, border()));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.set_height(30.0);
                    ui.add_space(20.0);
                    let title = if query.is_empty() {
                        "Explorateur"
                    } else {
                        "Résultats"
                    };
                    ui.label(
                        RichText::new(title)
                            .size(15.0)
                            .family(style::strong_family())
                            .color(style::primary_text()),
                    );
                    if query.is_empty() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(10.0);
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let can_collapse = !self.expanded.is_empty();
                            let collapse = ui
                                .add_enabled_ui(can_collapse, |ui| {
                                    small_button(ui, Icon::Collapse, "Replier tous les dossiers")
                                })
                                .inner;
                            if collapse.clicked() {
                                commands.push(Command::CollapseAll);
                            }
                            if small_button(ui, Icon::Plus, "Nouveau fichier (enregistré dans ce dossier)")
                                .clicked()
                            {
                                commands.push(Command::New);
                            }
                            let parent = root.parent().map(Path::to_path_buf);
                            let up = ui
                                .add_enabled_ui(parent.is_some(), |ui| {
                                    small_button(ui, Icon::Parent, "Dossier parent")
                                })
                                .inner;
                            if up.clicked()
                                && let Some(parent) = parent
                            {
                                commands.push(Command::SetRoot(parent));
                            }
                        });
                    }
                });
                ui.add_space(4.0);
                self.favorites(ui, &root, commands);
                egui::ScrollArea::vertical()
                    .id_salt("textedit-arborescence")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if query.is_empty() {
                            let response = Row {
                                icon: RowIcon::Symbol(Icon::FolderOutline),
                                text: &display_name(&root),
                                strong: true,
                                ..Row::default()
                            }
                            .show(ui)
                            .on_hover_text(root.display().to_string());
                            folder_menu(&response, &root, false, commands);
                            self.tree(ui, &root, 1, commands);
                        } else {
                            self.search_results(ui, &root, commands);
                        }
                        ui.add_space(8.0);
                    });
            });
    }

    /// Contenu de `directory`, lu sur le disque au plus toutes les deux secondes.
    fn listing(&self, directory: &Path) -> std::rc::Rc<Vec<Entry>> {
        let mut listings = self.listings.borrow_mut();
        if let Some((read, entries)) = listings.get(directory)
            && read.elapsed() < std::time::Duration::from_secs(2)
        {
            return entries.clone();
        }
        let entries = std::rc::Rc::new(list_directory(directory));
        listings.insert(
            directory.to_path_buf(),
            (std::time::Instant::now(), entries.clone()),
        );
        entries
    }

    fn tree(&self, ui: &mut egui::Ui, directory: &Path, depth: usize, commands: &mut Vec<Command>) {
        let active = self.document().path.as_deref();
        let entries = self.listing(directory);
        if entries.is_empty() {
            Row {
                depth,
                guides: true,
                text: "Dossier vide",
                muted: true,
                ..Row::default()
            }
            .show(ui);
            return;
        }
        for (path, name, is_dir) in entries.iter() {
            if *is_dir {
                let open = self.expanded.contains(path);
                let row = Row {
                    depth,
                    guides: true,
                    chevron: Some(open),
                    icon: RowIcon::File(FileKind::Folder),
                    text: name,
                    ..Row::default()
                };
                let response = row.show(ui);
                if response.clicked() {
                    commands.push(Command::ToggleFolder(path.clone()));
                }
                folder_menu(&response, path, true, commands);
                if open && depth < 12 {
                    self.tree(ui, path, depth + 1, commands);
                }
            } else {
                let row = Row {
                    depth,
                    guides: true,
                    icon: RowIcon::File(FileKind::of(path)),
                    text: name,
                    selected: active == Some(path.as_path()),
                    ..Row::default()
                };
                let response = row.show(ui);
                if response.clicked() {
                    commands.push(Command::OpenPath(path.clone()));
                }
                file_menu(&response, path, commands);
            }
        }
    }

    fn search_results(&self, ui: &mut egui::Ui, root: &Path, commands: &mut Vec<Command>) {
        let found = self.search_cache.as_ref().map_or(&[][..], |(_, _, found)| found);
        if found.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.label(RichText::new("Aucun fichier trouvé").color(style::text_secondary()));
            });
        }
        for path in found {
            let folder = path
                .parent()
                .and_then(|parent| parent.strip_prefix(root).ok())
                .map(|relative| relative.display().to_string())
                .unwrap_or_default();
            let row = Row {
                icon: RowIcon::File(FileKind::of(path)),
                text: &display_name(path),
                detail: &folder,
                selected: self.document().path.as_deref() == Some(path.as_path()),
                ..Row::default()
            };
            let response = row.show(ui);
            if response.clicked() {
                commands.push(Command::OpenPath(path.clone()));
                commands.push(Command::ClearSearch);
            }
            file_menu(&response, path, commands);
        }
    }

    /// Favoris en bas de l'explorateur : chacun devient le dossier affiché. La séparation
    /// avec l'arborescence se tire à la souris ; replié, le panneau se réduit à son en-tête.
    fn favorites(&self, ui: &mut egui::Ui, root: &Path, commands: &mut Vec<Command>) {
        let places = favorite_places();
        if places.is_empty() {
            return;
        }
        let open = self.favorites_open;
        // Replié, le panneau garde une identité à part : la hauteur choisie pour la liste
        // n'est pas écrasée par la taille de l'en-tête seul.
        let id = if open {
            "textedit-favoris"
        } else {
            "textedit-favoris-replie"
        };
        let (default_size, min_size, max_size) = favorites_heights(places.len(), ui.available_height());
        let mut panel =
            egui::Panel::bottom(ui.scope_id().with(id)).frame(Frame::new().inner_margin(Margin {
                left: 0,
                right: 0,
                top: FAVORITES_TOP,
                bottom: FAVORITES_BOTTOM,
            }));
        panel = if open {
            panel
                .resizable(true)
                .default_size(default_size)
                .min_size(min_size)
                .max_size(max_size)
        } else {
            panel.exact_size(min_size)
        };
        panel.show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let header = Row {
                chevron: Some(open),
                text: "Favoris",
                strong: true,
                ..Row::default()
            };
            if header.show(ui).clicked() {
                commands.push(Command::ToggleFavorites);
            }
            if open {
                // Tirée plus courte que la liste, la séparation laisse faire défiler les
                // favoris au lieu de les couper.
                egui::ScrollArea::vertical()
                    .id_salt("textedit-favoris-liste")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (name, path) in places {
                            let row = Row {
                                depth: 1,
                                icon: RowIcon::Symbol(Icon::FolderOutline),
                                text: name,
                                selected: root == path,
                                ..Row::default()
                            };
                            if row.show(ui).clicked() {
                                commands.push(Command::SetRoot(path));
                            }
                        }
                    });
            }
        });
    }

    /// Onglets des documents ouverts, et « + » pour en créer un.
    fn tabs(&self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        egui::Panel::top(ui.scope_id().with("onglets"))
            .resizable(false)
            .exact_size(TAB_HEIGHT)
            .frame(Frame::new().fill(tab_strip()))
            .show(ui, |ui| {
                let full = ui.max_rect();
                ui.painter()
                    .hline(full.x_range(), full.bottom() - 0.5, Stroke::new(1.0, border()));
                egui::ScrollArea::horizontal()
                    .id_salt("textedit-onglets")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for (index, document) in self.documents.iter().enumerate() {
                                self.tab(ui, index, document, commands);
                            }
                            ui.add_space(6.0);
                            let (rect, response) =
                                ui.allocate_exact_size(egui::vec2(34.0, TAB_HEIGHT), Sense::click());
                            if response.hovered() {
                                ui.painter().rect_filled(
                                    egui::Rect::from_center_size(rect.center(), egui::vec2(30.0, 30.0)),
                                    7.0,
                                    style::tint(10),
                                );
                            }
                            icons::paint(ui.painter(), rect, Icon::Plus, icon_color());
                            if response.on_hover_text("Nouveau document (Ctrl+N)").clicked() {
                                commands.push(Command::New);
                            }
                        });
                    });
            });
    }

    fn tab(&self, ui: &mut egui::Ui, index: usize, document: &Document, commands: &mut Vec<Command>) {
        let name = document.name();
        let font = FontId::proportional(13.0);
        let text_width = ui
            .painter()
            .layout_no_wrap(name.clone(), font.clone(), style::primary_text())
            .size()
            .x;
        let width = (text_width + 84.0).clamp(120.0, 240.0);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, TAB_HEIGHT), Sense::click());
        let active = index == self.active;
        let painter = ui.painter();
        if active {
            // Le trait du bas s'interrompt sous l'onglet actif.
            painter.rect_filled(rect.expand2(egui::vec2(0.0, 0.5)), 0.0, style::background());
        } else if response.hovered() {
            painter.rect_filled(rect, 0.0, style::tint(6));
        }
        painter.vline(
            rect.right() - 0.5,
            rect.top() + 10.0..=rect.bottom() - 10.0,
            Stroke::new(1.0, border()),
        );
        let icon = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 24.0, rect.center().y),
            egui::vec2(18.0, 18.0),
        );
        icons::paint_file(painter, icon, document.kind());
        let color = if active {
            style::primary_text()
        } else {
            style::text_secondary()
        };
        let shown = elide(ui, &name, width - 84.0, 13.0);
        painter.text(
            egui::pos2(icon.right() + 9.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            shown,
            font,
            color,
        );
        // Croix à droite (onglet actif ou survolé) ; point si le document est modifié.
        let close = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 22.0, rect.center().y),
            egui::vec2(20.0, 20.0),
        );
        let close_response = ui.interact(close, response.id.with("fermer"), Sense::click());
        let hovered = response.hovered() || close_response.hovered();
        if document.dirty() && !hovered {
            painter.circle_filled(close.center(), 4.0, icon_color());
        } else if active || hovered {
            if close_response.hovered() {
                painter.rect_filled(close, 5.0, style::tint(14));
            }
            icons::paint(painter, close, Icon::Close, icon_color());
        }
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
        egui::Panel::top(ui.scope_id().with("textedit-find"))
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(tab_strip())
                    .inner_margin(Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| {
                let full = ui.max_rect().expand2(egui::vec2(12.0, 6.0));
                ui.painter()
                    .hline(full.x_range(), full.bottom() - 0.5, Stroke::new(1.0, border()));
                ui.set_min_width(ui.available_width());
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
                    if response.has_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                        let backwards = ui.input(|input| input.modifiers.shift);
                        commands.push(Command::FindStep(!backwards));
                    }
                    let status = match (find.query.is_empty(), count) {
                        (true, _) => String::new(),
                        (false, 0) => "Aucun résultat".to_owned(),
                        (false, count) => format!("{} sur {count}", find.current.min(count - 1) + 1),
                    };
                    ui.label(RichText::new(status).color(style::text_secondary()));
                    if icon_button(ui, Icon::Up, false, "Précédent (Maj+Entrée)").clicked() {
                        commands.push(Command::FindStep(false));
                    }
                    if icon_button(ui, Icon::Down, false, "Suivant (Entrée)").clicked() {
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
                    if !find.replace && ui.button("Remplacer…").on_hover_text("Ctrl+H").clicked() {
                        find.replace = true;
                        find.replace_focus = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, false, "Fermer (Échap)").clicked() {
                            commands.push(Command::CloseFind);
                        }
                    });
                });
                if find.replace {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let replacement_response = ui.add(
                            egui::TextEdit::singleline(&mut find.replacement)
                                .hint_text("Remplacer par")
                                .desired_width(220.0),
                        );
                        if std::mem::take(&mut find.replace_focus) {
                            replacement_response.request_focus();
                        }
                        if replacement_response.has_focus() && ui.input(|input| input.key_pressed(Key::Enter))
                        {
                            commands.push(Command::ReplaceOne);
                        }
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

    /// Barre d'état de la fenêtre : message à gauche ; état, langage, encodage, fins de
    /// ligne et retour à la ligne à droite.
    fn status_bar(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        let document = self.document();
        let line_ending = if document.contents.contains("\r\n") {
            "CRLF"
        } else {
            "LF"
        };
        let (state, dot) = match (document.dirty(), document.path.is_some()) {
            (true, _) => ("Modifié", style::warning()),
            (false, true) => ("Enregistré", style::success()),
            (false, false) => ("Non enregistré", style::text_secondary()),
        };
        let kind = document.kind();
        let message = self.message.clone();
        let wrap = self.wrap;
        style::status_bar(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if let Some((text, error)) = &message {
                if *error {
                    ui.colored_label(style::error(), text);
                } else {
                    ui.label(text);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(state);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 14.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, dot);
                status_separator(ui);
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
                status_separator(ui);
                ui.label(line_ending);
                status_separator(ui);
                ui.label("UTF-8");
                status_separator(ui);
                ui.label(kind.language());
                let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
                icons::paint_file(ui.painter(), rect, kind);
            });
        });
    }

    /// Colonne du texte : chemin du fichier en haut, position du curseur en bas.
    fn editor_pane(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(style::background()))
            .show(ui, |ui| {
                self.breadcrumb(ui);
                self.cursor_bar(ui);
                egui::CentralPanel::no_frame().show(ui, |ui| self.text_area(ui));
            });
    }

    /// Icône et chemin du document, depuis le dossier de l'explorateur.
    fn breadcrumb(&self, ui: &mut egui::Ui) {
        let document = self.document();
        let root = self.root();
        let mut parts: Vec<String> = document
            .path
            .as_deref()
            .and_then(Path::parent)
            .and_then(|parent| parent.strip_prefix(&root).ok())
            .map(|relative| {
                relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        parts.push(document.name());
        egui::Panel::top(ui.scope_id().with("textedit-chemin"))
            .resizable(false)
            .exact_size(36.0)
            .frame(
                Frame::new()
                    .fill(style::background())
                    .inner_margin(Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                let full = ui.max_rect().expand2(egui::vec2(16.0, 0.0));
                ui.painter()
                    .hline(full.x_range(), full.bottom() - 0.5, Stroke::new(1.0, border()));
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
                    icons::paint_file(ui.painter(), rect, document.kind());
                    let last = parts.len() - 1;
                    for (index, part) in parts.iter().enumerate() {
                        if index == last {
                            ui.label(RichText::new(part).size(12.5).color(style::primary_text()));
                        } else {
                            ui.label(RichText::new(part).size(12.5).color(style::text_secondary()));
                            ui.label(RichText::new("›").size(12.5).color(style::text_secondary()));
                        }
                    }
                });
            });
    }

    fn cursor_bar(&self, ui: &mut egui::Ui) {
        let text = &self.document().contents;
        let characters = text.chars().count();
        let words = text.split_whitespace().count();
        let (line, column) = self.cursor;
        egui::Panel::bottom(ui.scope_id().with("textedit-curseur"))
            .resizable(false)
            .exact_size(30.0)
            .frame(
                Frame::new()
                    .fill(style::background())
                    .inner_margin(Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                let full = ui.max_rect().expand2(egui::vec2(16.0, 0.0));
                ui.painter()
                    .hline(full.x_range(), full.top() + 0.5, Stroke::new(1.0, border()));
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let small = |text: String| RichText::new(text).size(11.5).color(style::text_secondary());
                    ui.label(small(format!("Ligne {line}, colonne {column}")));
                    ui.label(small("·".to_owned()));
                    ui.label(small(format!(
                        "{} caractère{}",
                        group_digits(characters),
                        plural(characters)
                    )));
                    ui.label(small("·".to_owned()));
                    ui.label(small(format!("{} mot{}", group_digits(words), plural(words))));
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
        let kind = document.kind();
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
            lines.to_string().len().max(2) as f32 * digit + 30.0
        } else {
            0.0
        };
        // Blocs de code du Markdown, en caractères, pour leur peindre un fond.
        let code_blocks: Vec<Range<usize>> = if kind == FileKind::Markdown {
            let text = &document.contents;
            highlight::code_blocks(text)
                .into_iter()
                .map(|range| text[..range.start].chars().count()..text[..range.end].chars().count())
                .collect()
        } else {
            Vec::new()
        };

        let scroll = if wrap {
            egui::ScrollArea::vertical()
        } else {
            egui::ScrollArea::both()
        };
        let reveal = std::mem::take(&mut self.reveal_match);
        let mut cursor = self.cursor;
        scroll.auto_shrink(false).show(ui, |ui| {
            // Fonds des blocs de code et de la ligne courante, peints sous le texte.
            let block_slot = ui.painter().add(egui::Shape::Noop);
            let line_slot = ui.painter().add(egui::Shape::Noop);
            let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, wrap_width: f32| {
                let text = buffer.as_str();
                let matches = find_matches(text, &query, match_case);
                let mut job = highlight::layout_job(text, kind, &font, &matches, current);
                job.wrap.max_width = if wrap { wrap_width } else { f32::INFINITY };
                ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
            };
            let output = egui::TextEdit::multiline(&mut document.contents)
                .id(id)
                .font(font.clone())
                .frame(Frame::new().inner_margin(Margin {
                    left: (gutter + 16.0).min(127.0) as i8,
                    right: 18,
                    top: 12,
                    bottom: 12,
                }))
                .desired_width(f32::INFINITY)
                .min_size(ui.available_size())
                .hint_text(RichText::new("Commence à écrire…").color(style::text_secondary()))
                .lock_focus(true)
                .layouter(&mut layouter)
                .show(ui);
            if output.response.response.changed() {
                document.version += 1;
            }
            let rect = output.response.response.rect;
            let galley = &output.galley;
            let origin = output.galley_pos;

            // Ligne et colonne du curseur.
            let cursor_index = output.cursor_range.map(|range| range.primary.index.0);
            if let Some(index) = cursor_index {
                cursor = line_column(&document.contents, index);
            }

            // Numéros de ligne, ligne courante et blocs de code.
            let clip = ui.clip_rect();
            let painter = ui.painter();
            if line_numbers {
                painter.vline(
                    rect.left() + gutter,
                    rect.y_range(),
                    Stroke::new(1.0, style::separator()),
                );
            }
            let mut line = 1;
            let mut line_start = true;
            let mut row_char = 0;
            let mut current_rows: Option<egui::Rangef> = None;
            let mut block_rows: Vec<Option<egui::Rangef>> = vec![None; code_blocks.len()];
            for row in &galley.rows {
                let row_rect = row.rect().translate(origin.to_vec2());
                let is_current = line == cursor.0 && cursor_index.is_some();
                let grow = |range: Option<egui::Rangef>| {
                    Some(match range {
                        Some(range) => range.union(row_rect.y_range()),
                        None => row_rect.y_range(),
                    })
                };
                if is_current {
                    current_rows = grow(current_rows);
                }
                if let Some(block) = code_blocks.iter().position(|block| block.contains(&row_char)) {
                    block_rows[block] = grow(block_rows[block]);
                }
                if line_numbers
                    && line_start
                    && row_rect.bottom() >= clip.top()
                    && row_rect.top() <= clip.bottom()
                {
                    let color = if is_current {
                        style::primary_text()
                    } else {
                        line_number()
                    };
                    painter.text(
                        egui::pos2(rect.left() + gutter - 12.0, row_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        line.to_string(),
                        FontId::monospace(font.size * 0.85),
                        color,
                    );
                }
                row_char += row.char_count_including_newline().0;
                line_start = row.ends_with_newline;
                if row.ends_with_newline {
                    line += 1;
                }
            }
            let band = |range: egui::Rangef| {
                egui::Rect::from_x_y_ranges(rect.left() + gutter + 8.0..=rect.right() - 10.0, range)
            };
            let blocks: Vec<egui::Shape> = block_rows
                .into_iter()
                .flatten()
                .map(|range| egui::Shape::rect_filled(band(range.expand(3.0)), 6.0, code_block()))
                .collect();
            painter.set(block_slot, egui::Shape::Vec(blocks));
            if let Some(range) = current_rows {
                let full = egui::Rect::from_x_y_ranges(rect.left() + gutter + 1.0..=rect.right(), range);
                painter.set(line_slot, egui::Shape::rect_filled(full, 0.0, current_line()));
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

    /// Colonne de l'aperçu : onglets « Aperçu » et « Infos », bouton plein écran.
    fn preview_pane(&mut self, ui: &mut egui::Ui, mode: MarkdownMode, commands: &mut Vec<Command>) {
        let full = ui.max_rect();
        if mode == MarkdownMode::Split {
            ui.painter()
                .vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0, border()));
        }
        egui::Panel::top(ui.scope_id().with("textedit-apercu-onglets"))
            .resizable(false)
            .exact_size(44.0)
            .frame(
                Frame::new()
                    .fill(style::background())
                    .inner_margin(Margin::symmetric(18, 0)),
            )
            .show(ui, |ui| {
                let bar = ui.max_rect().expand2(egui::vec2(18.0, 0.0));
                ui.painter()
                    .hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, border()));
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (tab, icon, text) in [
                        (PreviewTab::Preview, Icon::Eye, "Aperçu"),
                        (PreviewTab::Info, Icon::Info, "Infos"),
                    ] {
                        if preview_tab(ui, icon, text, self.preview_tab == tab, bar.bottom()).clicked() {
                            commands.push(Command::SetPreviewTab(tab));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (icon, hint, next) = match mode {
                            MarkdownMode::Preview => {
                                (Icon::Collapse, "Afficher le texte", MarkdownMode::Split)
                            }
                            _ => (Icon::Expand, "Aperçu seul", MarkdownMode::Preview),
                        };
                        if icon_button(ui, icon, false, hint).clicked() {
                            commands.push(Command::SetMode(next));
                        }
                    });
                });
            });
        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(style::background()))
            .show(ui, |ui| match self.preview_tab {
                PreviewTab::Preview => self.markdown_area(ui),
                PreviewTab::Info => self.info_area(ui),
            });
    }

    fn markdown_area(&mut self, ui: &mut egui::Ui) {
        let Self {
            documents,
            active,
            images,
            ..
        } = self;
        let document = &mut documents[*active];
        if document
            .markdown_preview
            .as_ref()
            .is_none_or(|(version, _)| *version != document.version)
        {
            let (meta, body) = markdown::front_matter(&document.contents);
            let blocks = markdown::parse(body);
            document.markdown_preview = Some((document.version, Preview { meta, blocks }));
        }
        let base = document
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let preview = &document.markdown_preview.as_ref().unwrap().1;
        let mut images = Images {
            textures: images,
            base,
            clicked: None,
        };
        egui::ScrollArea::vertical()
            .id_salt(("textedit-markdown-preview", document.id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // L'aperçu est du contenu : son texte se sélectionne et se copie
                // (glisser, double-clic, Ctrl+C), contrairement aux libellés d'interface.
                ui.style_mut().interaction.selectable_labels = true;
                let width = ui.available_width();
                let paper = (width - 56.0).clamp(120.0, 760.0);
                ui.add_space(28.0);
                ui.horizontal(|ui| {
                    ui.add_space(((width - paper) / 2.0).max(0.0));
                    ui.vertical(|ui| {
                        ui.set_width(paper);
                        let meta = |key: &str| {
                            preview
                                .meta
                                .iter()
                                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                                .map(|(_, value)| value.as_str())
                                .filter(|value| !value.is_empty())
                        };
                        if let Some(title) = meta("title") {
                            hero(ui, title, meta("description"));
                        }
                        let mut index = 0;
                        while index < preview.blocks.len() {
                            // Images à la suite (badges d'un README) : côte à côte.
                            let run = preview.blocks[index..]
                                .iter()
                                .take_while(|block| matches!(block, markdown::Block::Image { .. }))
                                .count();
                            if run > 1 {
                                image_row(ui, &preview.blocks[index..index + run], &mut images);
                                index += run;
                            } else {
                                render_markdown_block(ui, &preview.blocks[index], &mut images);
                                index += 1;
                            }
                        }
                    });
                });
                ui.add_space(36.0);
            });
        if let Some(link) = images.clicked.take() {
            let target = if link.contains(':') {
                link
            } else {
                images.resolve(&link).display().to_string()
            };
            let local = Path::new(&target);
            if local.is_file() && FileKind::of(local) == FileKind::Markdown {
                self.open(local.to_path_buf());
            } else if !target.starts_with('#') {
                open_link(&target);
            }
        }
    }

    /// Onglet « Infos » : ce que l'on sait du document.
    fn info_area(&self, ui: &mut egui::Ui) {
        let document = self.document();
        let text = &document.contents;
        let lines = text.matches('\n').count() + 1;
        let state = match (document.dirty(), document.path.is_some()) {
            (true, _) => "Modifié",
            (false, true) => "Enregistré",
            (false, false) => "Jamais enregistré",
        };
        let folder = document
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(short_path)
            .unwrap_or_else(|| "—".to_owned());
        let rows = [
            ("Nom", document.name()),
            ("Emplacement", folder),
            ("Type", document.kind().language().to_owned()),
            ("Taille", format_size(text.len())),
            ("Lignes", group_digits(lines)),
            ("Mots", group_digits(text.split_whitespace().count())),
            ("Caractères", group_digits(text.chars().count())),
            ("Encodage", "UTF-8".to_owned()),
            (
                "Fins de ligne",
                if text.contains("\r\n") {
                    "CRLF (Windows)"
                } else {
                    "LF (Unix)"
                }
                .to_owned(),
            ),
            ("État", state.to_owned()),
        ];
        egui::ScrollArea::vertical()
            .id_salt("textedit-infos")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(24.0);
                ui.horizontal(|ui| {
                    ui.add_space(28.0);
                    egui::Grid::new("textedit-infos-grille")
                        .num_columns(2)
                        .spacing(egui::vec2(28.0, 12.0))
                        .show(ui, |ui| {
                            for (label, value) in rows {
                                ui.label(RichText::new(label).color(style::text_secondary()));
                                ui.label(RichText::new(value).color(style::primary_text()));
                                ui.end_row();
                            }
                        });
                });
            });
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
                    .fill(style::background())
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
                    if document.is_markdown() && document.markdown_mode == MarkdownMode::Edit {
                        document.markdown_mode = MarkdownMode::Split;
                    } else if !document.is_markdown() {
                        document.markdown_mode = MarkdownMode::Edit;
                    }
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
            ui.label(RichText::new(detail).color(style::text_secondary()));
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
fn shortcuts(ctx: &egui::Context, finding: bool, markdown: bool, commands: &mut Vec<Command>) {
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
            (ctrl, Key::P, Command::FocusSearch),
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
        if markdown {
            if take(ctrl, Key::B) {
                commands.push(Command::Format(FormatCommand::Bold));
            }
            if take(ctrl, Key::I) {
                commands.push(Command::Format(FormatCommand::Italic));
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

/// En-tête de l'aperçu tiré de l'en-tête YAML : logo, titre, description, trait.
fn hero(ui: &mut egui::Ui, title: &str, description: Option<&str>) {
    ui.horizontal(|ui| {
        let (logo, _) = ui.allocate_exact_size(egui::vec2(52.0, 52.0), Sense::hover());
        icons::paint_logo(ui.painter(), logo);
        ui.add_space(14.0);
        ui.label(
            RichText::new(title)
                .size(32.0)
                .family(style::strong_family())
                .color(style::primary_text()),
        );
    });
    if let Some(description) = description {
        ui.add_space(14.0);
        ui.add(egui::Label::new(RichText::new(description).size(15.0).color(body())).wrap());
    }
    ui.add_space(20.0);
    rule(ui);
    ui.add_space(10.0);
}

fn rule(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, border()));
}

/// Images de l'aperçu : textures chargées une fois, chemins relatifs au document.
struct Images<'a> {
    /// Texture et nombre de pixels par point (2 pour un SVG dessiné en double).
    textures: &'a mut HashMap<PathBuf, Option<(egui::TextureHandle, f32)>>,
    base: Option<PathBuf>,
    /// Lien cliqué pendant cette image de l'aperçu.
    clicked: Option<String>,
}

impl Images<'_> {
    /// Chemin d'une image ou d'un lien local. « /images/a.png » (comme sur GitHub) part
    /// de la racine du projet : on le cherche dans le dossier du document puis dans ses
    /// parents ; sinon le chemin est relatif au document.
    fn resolve(&self, path: &str) -> PathBuf {
        let path = path.split(['#', '?']).next().unwrap_or(path);
        let path = path.replace("%20", " ");
        let Some(base) = &self.base else {
            return PathBuf::from(path);
        };
        if let Some(rooted) = path.strip_prefix('/') {
            if Path::new(&path).exists() {
                return PathBuf::from(path);
            }
            return base
                .ancestors()
                .map(|folder| folder.join(rooted))
                .find(|candidate| candidate.exists())
                .unwrap_or_else(|| base.join(rooted));
        }
        base.join(path)
    }

    fn get(&mut self, ctx: &egui::Context, path: &str) -> Shown {
        let path = if remote::is_remote(path) {
            match remote::fetch(ctx, path) {
                remote::Remote::Ready(file) => file,
                remote::Remote::Loading => return Shown::Loading,
                remote::Remote::Failed => return Shown::Missing,
            }
        } else if path.contains("://") {
            return Shown::Missing;
        } else {
            self.resolve(path)
        };
        let texture = self
            .textures
            .entry(path.clone())
            .or_insert_with(|| {
                let (image, scale) = remote::decode(&path)?;
                let texture =
                    ctx.load_texture(path.display().to_string(), image, egui::TextureOptions::LINEAR);
                Some((texture, scale))
            })
            .clone();
        match texture {
            Some((texture, scale)) => Shown::Ready(texture, scale),
            None => Shown::Missing,
        }
    }
}

/// État d'une image de l'aperçu.
enum Shown {
    /// Texture et nombre de pixels par point.
    Ready(egui::TextureHandle, f32),
    Loading,
    Missing,
}

fn render_markdown_block(ui: &mut egui::Ui, block: &markdown::Block, images: &mut Images) {
    use crate::markdown::{Align, Block, Marker};

    match block {
        Block::Heading(level, spans) => {
            let (size, before, after) = match level {
                1 => (28.0, 18.0, 10.0),
                2 => (22.0, 22.0, 10.0),
                3 => (18.0, 16.0, 8.0),
                _ => (15.5, 12.0, 6.0),
            };
            ui.add_space(before);
            let mut job = markdown_job(ui, spans, size, style::primary_text());
            for section in &mut job.sections {
                section.format.font_id.family = style::strong_family();
            }
            link_label(ui, job, spans, images);
            ui.add_space(after);
        }
        Block::Paragraph(spans) => {
            link_label(ui, markdown_job(ui, spans, 15.0, body()), spans, images);
            ui.add_space(12.0);
        }
        Block::Aligned(alignment, spans) => {
            let align = match alignment {
                Align::Left => egui::Align::Min,
                Align::Center => egui::Align::Center,
                Align::Right => egui::Align::Max,
            };
            ui.with_layout(egui::Layout::top_down(align), |ui| {
                link_label(ui, markdown_job(ui, spans, 15.0, body()), spans, images);
            });
            ui.add_space(10.0);
        }
        Block::Code { language, lines } => {
            code_card(ui, language, lines);
            ui.add_space(14.0);
        }
        Block::Image { alt, path, link } => {
            let shown = images.get(ui.ctx(), path);
            let response = image_card(ui, alt, path, &shown, false);
            image_link(response, link, images);
            ui.add_space(14.0);
        }
        Block::Quote(blocks) => {
            let response = egui::Frame::new()
                .inner_margin(Margin {
                    left: 16,
                    right: 0,
                    top: 2,
                    bottom: 2,
                })
                .show(ui, |ui| {
                    for child in blocks {
                        render_markdown_block(ui, child, images);
                    }
                })
                .response;
            ui.painter().vline(
                response.rect.left() + 1.5,
                response.rect.y_range(),
                Stroke::new(3.0, style::accent().gamma_multiply(0.5)),
            );
            ui.add_space(8.0);
        }
        Block::List(items) => {
            for item in items {
                ui.horizontal_top(|ui| {
                    ui.add_space(item.level as f32 * 20.0 + 4.0);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 22.0), Sense::hover());
                    let painter = ui.painter();
                    match item.marker {
                        Marker::Bullet => {
                            painter.circle_filled(
                                rect.left_center() + egui::vec2(4.0, 0.0),
                                3.0,
                                style::accent(),
                            );
                        }
                        Marker::Number(number) => {
                            painter.text(
                                rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                format!("{number}."),
                                FontId::proportional(15.0),
                                style::accent(),
                            );
                        }
                        Marker::Task(done) => {
                            let square = egui::Rect::from_min_size(
                                rect.left_center() - egui::vec2(0.0, 7.0),
                                egui::vec2(14.0, 14.0),
                            );
                            if done {
                                painter.rect_filled(square, 3.0, style::accent());
                                painter.line(
                                    vec![
                                        square.left_center() + egui::vec2(3.0, 0.0),
                                        square.center_bottom() + egui::vec2(-1.0, -3.5),
                                        square.right_top() + egui::vec2(-3.0, 3.5),
                                    ],
                                    Stroke::new(1.6, Color32::WHITE),
                                );
                            } else {
                                painter.rect_stroke(
                                    square,
                                    3.0,
                                    Stroke::new(1.3, line_number()),
                                    egui::StrokeKind::Inside,
                                );
                            }
                        }
                    }
                    link_label(ui, markdown_job(ui, &item.text, 15.0, body()), &item.text, images);
                });
                ui.add_space(6.0);
            }
            ui.add_space(8.0);
        }
        Block::Rule => {
            ui.add_space(8.0);
            rule(ui);
            ui.add_space(14.0);
        }
        Block::Table { align, header, rows } => {
            table(ui, align, header, rows);
            ui.add_space(12.0);
        }
    }
}

/// Tableau Markdown : colonnes à la largeur de leur contenu, resserrées (texte renvoyé à
/// la ligne) quand la place manque ; en-tête sur fond, traits entre rangées et colonnes.
/// Trop de colonnes même resserrées : le tableau défile de côté.
fn table(
    ui: &mut egui::Ui,
    align: &[markdown::Align],
    header: &[Vec<markdown::Span>],
    rows: &[Vec<Vec<markdown::Span>>],
) {
    const PAD_X: f32 = 12.0;
    const PAD_Y: f32 = 8.0;
    const SIZE: f32 = 13.5;
    let columns = header.len().max(1);
    use markdown::Align;
    let job = |ui: &egui::Ui, spans: &[markdown::Span], heading: bool, width: f32, align: Align| {
        let mut job = markdown_job(
            ui,
            spans,
            SIZE,
            if heading { style::primary_text() } else { body() },
        );
        if heading {
            for section in &mut job.sections {
                section.format.font_id.family = style::strong_family();
            }
        }
        job.wrap.max_width = width;
        job.halign = match align {
            Align::Center => egui::Align::Center,
            Align::Right => egui::Align::RIGHT,
            Align::Left => egui::Align::LEFT,
        };
        job
    };
    let all_rows = || std::iter::once((true, header)).chain(rows.iter().map(|row| (false, row.as_slice())));
    // Largeur naturelle (sans retour à la ligne) et largeur du mot le plus long.
    let mut natural = vec![0.0f32; columns];
    let mut narrow = vec![0.0f32; columns];
    for (heading, row) in all_rows() {
        for (column, cell) in row.iter().enumerate().take(columns) {
            let full =
                ui.fonts_mut(|fonts| fonts.layout_job(job(ui, cell, heading, f32::INFINITY, Align::Left)));
            natural[column] = natural[column].max(full.size().x);
            // Mot le plus long, mesuré seul : une colonne ne coupe jamais un mot en deux
            // (au-delà de 160 points, il passe quand même à la ligne).
            let font = if heading {
                FontId::new(SIZE, style::strong_family())
            } else {
                FontId::proportional(SIZE)
            };
            let text: String = cell.iter().map(|span| span.text.as_str()).collect();
            for word in text.split_whitespace() {
                let width = ui.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(word.to_owned(), font.clone(), body())
                        .size()
                        .x
                });
                // Un rien de marge pour l'arrondi des mesures.
                narrow[column] = narrow[column].max((width + 1.0).min(160.0).min(natural[column]));
            }
        }
    }
    let available = ui.available_width() - 2.0 * PAD_X * columns as f32 - 1.0;
    let widths = column_widths(&natural, &narrow, available);
    let galleys: Vec<(bool, Vec<std::sync::Arc<egui::Galley>>)> = all_rows()
        .map(|(heading, row)| {
            let cells = (0..columns)
                .map(|column| {
                    let spans = row.get(column).map_or(&[][..], Vec::as_slice);
                    let align = align.get(column).copied().unwrap_or(Align::Left);
                    ui.fonts_mut(|fonts| fonts.layout_job(job(ui, spans, heading, widths[column], align)))
                })
                .collect();
            (heading, cells)
        })
        .collect();
    let heights: Vec<f32> = galleys
        .iter()
        .map(|(_, cells)| cells.iter().map(|g| g.size().y).fold(0.0, f32::max) + 2.0 * PAD_Y)
        .collect();
    let width = widths.iter().map(|w| w + 2.0 * PAD_X).sum::<f32>() + 1.0;
    let height = heights.iter().sum::<f32>() + 1.0;
    egui::ScrollArea::horizontal()
        .id_salt(ui.next_auto_id())
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
            let painter = ui.painter();
            let outer = rect.shrink(0.5);
            let radius = 8.0;
            let first = heights.first().copied().unwrap_or(0.0);
            painter.rect_filled(
                egui::Rect::from_min_size(outer.min, egui::vec2(outer.width(), first)),
                egui::CornerRadius {
                    nw: radius as u8,
                    ne: radius as u8,
                    sw: 0,
                    se: 0,
                },
                code_block(),
            );
            let line = Stroke::new(1.0, border());
            let mut y = outer.top();
            for (index, ((_, cells), row_height)) in galleys.iter().zip(&heights).enumerate() {
                if index > 0 {
                    painter.hline(outer.x_range(), y, line);
                }
                let mut x = outer.left();
                for (column, galley) in cells.iter().enumerate() {
                    if column > 0 && index == 0 {
                        painter.vline(x, outer.y_range(), line);
                    }
                    let cell_width = widths[column];
                    let anchor = match align.get(column).copied().unwrap_or(Align::Left) {
                        Align::Left => x + PAD_X,
                        Align::Center => x + PAD_X + cell_width / 2.0,
                        Align::Right => x + PAD_X + cell_width,
                    };
                    // Texte de la cellule sélectionnable, comme le reste de l'aperçu.
                    let pos = egui::pos2(anchor, y + PAD_Y);
                    let response = ui.interact(
                        galley.rect.translate(pos.to_vec2()),
                        ui.scope_id().with(("cellule", index, column)),
                        Sense::click_and_drag(),
                    );
                    egui::text_selection::LabelSelectionState::label_text_selection(
                        ui,
                        &response,
                        pos,
                        galley.clone(),
                        body(),
                        Stroke::NONE,
                    );
                    x += cell_width + 2.0 * PAD_X;
                }
                y += row_height;
            }
            painter.rect_stroke(outer, radius, line, egui::StrokeKind::Middle);
        });
}

/// Largeur de chaque colonne d'un tableau. Tout tient : chacune à sa largeur naturelle.
/// Sinon les colonnes plus étroites que leur part égale gardent leur largeur, et les
/// autres se partagent le reste, sans descendre sous leur mot le plus long (`narrow`).
fn column_widths(natural: &[f32], narrow: &[f32], available: f32) -> Vec<f32> {
    if natural.iter().sum::<f32>() <= available {
        return natural.to_vec();
    }
    let mut widths = natural.to_vec();
    let mut squeezed: Vec<usize> = (0..natural.len()).collect();
    let mut room = available;
    loop {
        let share = room / squeezed.len().max(1) as f32;
        let (keep, rest): (Vec<usize>, Vec<usize>) =
            squeezed.iter().partition(|&&column| natural[column] <= share);
        if keep.is_empty() {
            break;
        }
        room -= keep.iter().map(|&column| natural[column]).sum::<f32>();
        squeezed = rest;
        if squeezed.is_empty() {
            return widths;
        }
    }
    let floor: f32 = squeezed.iter().map(|&column| narrow[column]).sum();
    let extra = (room - floor).max(0.0);
    let wanted: f32 = squeezed
        .iter()
        .map(|&column| natural[column] - narrow[column])
        .sum::<f32>()
        .max(1.0);
    for &column in &squeezed {
        widths[column] = narrow[column] + (natural[column] - narrow[column]) * extra / wanted;
    }
    widths
}

/// Bloc de code en carte : bandeau avec la langue (ou le commentaire de première ligne
/// d'un script, comme « # Installation ») et le bouton « Copier », puis le code coloré.
fn code_card(ui: &mut egui::Ui, language: &str, lines: &[String]) {
    let kind = FileKind::of(Path::new(&format!(
        "x.{}",
        match language {
            "bash" | "shell" | "console" | "zsh" => "sh",
            "rust" => "rs",
            "javascript" => "js",
            "typescript" => "ts",
            "python" => "py",
            other => other,
        }
    )));
    let (title, body) = match lines.first() {
        Some(first) if kind == FileKind::Shell && first.starts_with("# ") => {
            (Some(first[2..].trim().to_owned()), &lines[1..])
        }
        _ if !language.is_empty() => (Some(kind_title(kind, language)), lines),
        _ => (None, lines),
    };
    let code = body.join("\n");
    // Ce que le bouton « Copier » met dans le presse-papier : les commandes seules.
    let copied = without_comments(&code, kind);
    egui::Frame::new()
        .fill(code_block())
        .stroke(Stroke::new(1.0, border()))
        .corner_radius(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(title) = &title {
                egui::Frame::new()
                    .inner_margin(Margin {
                        left: 16,
                        right: 8,
                        top: 6,
                        bottom: 6,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
                            icons::paint_terminal(ui.painter(), rect);
                            ui.add_space(4.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(title)
                                        .size(13.5)
                                        .family(style::strong_family())
                                        .color(style::primary_text()),
                                )
                                .selectable(false),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                copy_button(ui, &copied);
                            });
                        });
                    });
                rule(ui);
            }
            egui::Frame::new()
                .inner_margin(Margin {
                    left: 16,
                    right: if title.is_some() { 16 } else { 8 },
                    top: if title.is_some() { 12 } else { 8 },
                    bottom: 12,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let code_view = |ui: &mut egui::Ui| {
                        let mut job = highlight::layout_job(&code, kind, &FontId::monospace(13.0), &[], 0);
                        job.wrap.max_width = ui.available_width();
                        ui.add(egui::Label::new(job).wrap());
                    };
                    if title.is_some() {
                        code_view(ui);
                    } else {
                        // Sans bandeau : le bouton « Copier » se pose en haut à droite.
                        ui.horizontal_top(|ui| {
                            let button = 28.0 + ui.spacing().item_spacing.x;
                            ui.allocate_ui(egui::vec2(ui.available_width() - button, 0.0), |ui| {
                                ui.add_space(4.0);
                                code_view(ui);
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                copy_button(ui, &copied);
                            });
                        });
                    }
                });
        });
}

/// Durée pendant laquelle le bouton « Copier » montre une coche, en secondes.
const COPIED_FEEDBACK: f64 = 1.5;

/// Bouton presse-papier d'un bloc de code : copie `text`, puis montre une coche un moment.
fn copy_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let id = ui.scope_id().with(("copier", text));
    let now = ui.input(|input| input.time);
    let copied_at: Option<f64> = ui.data(|data| data.get_temp(id));
    let copied = copied_at.is_some_and(|at| now - at < COPIED_FEEDBACK);
    let (icon, hint) = if copied {
        (Icon::Check, "Copié")
    } else {
        (Icon::Copy, "Copier")
    };
    let response = small_button(ui, icon, hint);
    if response.clicked() {
        ui.ctx().copy_text(text.to_owned());
        ui.data_mut(|data| data.insert_temp(id, now));
    }
    if let Some(at) = copied_at {
        let left = COPIED_FEEDBACK - (now - at);
        if left > 0.0 {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(left));
        }
    }
    response
}

/// `code` sans ses commentaires, pour le copier : une ligne qui n'était qu'un commentaire
/// disparaît, un commentaire en fin de ligne est retiré (avec les espaces qui le
/// précèdent). Un « # » dans une chaîne ou collé à un mot (`a#b`) reste. Un bloc sans
/// langue est lu comme du shell : ce sont presque toujours des commandes.
fn without_comments(code: &str, kind: FileKind) -> String {
    let kind = if kind == FileKind::Text {
        FileKind::Shell
    } else {
        kind
    };
    let comments: Vec<std::ops::Range<usize>> = highlight::spans(code, kind)
        .into_iter()
        .filter(|(_, role)| *role == highlight::Kind::Comment)
        .map(|(range, _)| range)
        .collect();
    let mut lines = Vec::new();
    let mut start = 0;
    for line in code.split('\n') {
        let end = start + line.len();
        let mut kept = String::new();
        let mut cursor = start;
        let mut had_comment = false;
        for range in comments
            .iter()
            .filter(|range| range.start < end && range.end > start)
        {
            had_comment = true;
            kept.push_str(&code[cursor..range.start.max(cursor)]);
            cursor = range.end.min(end).max(cursor);
        }
        kept.push_str(&code[cursor..end]);
        if had_comment {
            let trimmed = kept.trim_end();
            if !trimmed.trim_start().is_empty() {
                lines.push(trimmed.to_owned());
            }
        } else {
            lines.push(kept);
        }
        start = end + 1;
    }
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Nom d'une langue de bloc de code : celui du langage reconnu, sinon tel qu'écrit.
fn kind_title(kind: FileKind, language: &str) -> String {
    match kind {
        FileKind::Text => language.to_owned(),
        kind => kind.language().to_owned(),
    }
}

/// Image de l'aperçu, à sa taille (réduite pour tenir). Une grande image a un cadre
/// arrondi ; une petite (badge) est posée telle quelle. Si elle manque ou se télécharge :
/// un cadre (`inline` faux) ou une simple étiquette (`inline`, badges côte à côte).
fn image_card(ui: &mut egui::Ui, alt: &str, path: &str, shown: &Shown, inline: bool) -> egui::Response {
    let width = ui.available_width();
    let status = match shown {
        Shown::Ready(texture, pixels) => {
            let size = texture.size_vec2() / pixels.max(1.0);
            let scale = (width / size.x).min(1.0);
            let small = size.y <= 48.0;
            let mut image = egui::Image::new(egui::load::SizedTexture::new(texture.id(), size * scale));
            if !small {
                image = image.corner_radius(10.0);
            }
            let mut response = ui.add(image);
            if !small {
                ui.painter().rect_stroke(
                    response.rect,
                    10.0,
                    Stroke::new(1.0, border()),
                    egui::StrokeKind::Inside,
                );
            }
            if !alt.is_empty() {
                response = response.on_hover_text(alt);
            }
            return response;
        }
        Shown::Loading => "Chargement de l'image…".to_owned(),
        Shown::Missing => format!("Image introuvable ou illisible : {path}"),
    };
    if inline {
        let text = if alt.is_empty() { "image" } else { alt };
        return egui::Frame::new()
            .fill(code_block())
            .stroke(Stroke::new(1.0, border()))
            .corner_radius(5.0)
            .inner_margin(Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.label(RichText::new(text).size(12.0).color(style::text_secondary()));
            })
            .response
            .on_hover_text(status);
    }
    egui::Frame::new()
        .fill(code_block())
        .stroke(Stroke::new(1.0, border()))
        .corner_radius(10.0)
        .inner_margin(Margin::symmetric(16, 22))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::hover());
                icons::paint_file(ui.painter(), rect.shrink(4.0), FileKind::Image);
                if !alt.is_empty() {
                    ui.label(RichText::new(alt).color(style::primary_text()));
                }
                ui.label(RichText::new(status).size(11.5).color(style::text_secondary()));
            });
        })
        .response
}

/// Images qui se suivent, posées côte à côte et renvoyées à la ligne si besoin.
fn image_row(ui: &mut egui::Ui, blocks: &[markdown::Block], images: &mut Images) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for block in blocks {
            if let markdown::Block::Image { alt, path, link } = block {
                let shown = images.get(ui.ctx(), path);
                let response = image_card(ui, alt, path, &shown, true);
                image_link(response, link, images);
            }
        }
    });
    ui.add_space(14.0);
}

/// Rend une image cliquable vers `link` (main et adresse au survol).
fn image_link(response: egui::Response, link: &str, images: &mut Images) {
    if link.is_empty() {
        return;
    }
    let response = response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(link);
    if response.clicked() {
        images.clicked = Some(link.to_owned());
    }
}

/// Texte de l'aperçu dont les liens s'ouvrent au clic (main au survol, adresse en bulle).
fn link_label(ui: &mut egui::Ui, job: LayoutJob, spans: &[markdown::Span], images: &mut Images) {
    let has_links = spans.iter().any(|span| !span.link.is_empty());
    if !has_links {
        ui.add(egui::Label::new(job).wrap());
        return;
    }
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let response = ui.add(egui::Label::new(galley.clone()).sense(Sense::click()));
    let origin = response.rect.min - galley.rect.min.to_vec2();
    let link = response.hover_pos().and_then(|pointer| {
        let local = pointer - origin;
        if !galley.rect.contains(local.to_pos2()) {
            return None;
        }
        let cursor = galley.cursor_from_pos(local);
        let index = if local.x < galley.pos_from_cursor(cursor).min.x {
            cursor.index.saturating_sub(1)
        } else {
            cursor.index
        };
        let byte = egui::epaint::text::ByteIndex(galley.job.text.char_indices().nth(index.0)?.0);
        let section = galley
            .job
            .sections
            .iter()
            .position(|section| section.byte_range.contains(&byte))?;
        spans
            .get(section)
            .map(|span| span.link.clone())
            .filter(|link| !link.is_empty())
    });
    if let Some(link) = link {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if response.clicked() {
            images.clicked = Some(link.clone());
        }
        response.on_hover_text(link);
    }
}

/// Ouvre une adresse (site, courriel, fichier) avec l'application choisie par le système.
fn open_link(target: &str) {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(target);
    let _ = ferrite_system::command::spawn_detached(command);
}

fn markdown_job(ui: &egui::Ui, spans: &[markdown::Span], size: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width().max(80.0);
    for span in spans {
        let mut format = TextFormat::simple(FontId::proportional(size), color);
        format.line_height = Some(size * 1.5);
        if span.style & markdown::BOLD != 0 {
            format.font_id = FontId::new(size, style::strong_family());
            format.color = style::primary_text();
        }
        if span.style & markdown::ITALIC != 0 {
            format.italics = true;
        }
        if span.style & markdown::CODE != 0 {
            format.font_id = FontId::monospace(size * 0.9);
            format.color = highlight::code_color();
            format.background = style::content_gray();
        }
        if span.style & markdown::LINK != 0 {
            format.color = style::accent();
            format.underline = Stroke::new(1.0, format.color);
        }
        if span.style & markdown::STRIKE != 0 {
            format.strikethrough = Stroke::new(1.0, format.color);
        }
        job.append(&span.text, 0.0, format);
    }
    style::emoji_presentation(&mut job);
    job
}

/// Icône d'une ligne de l'explorateur.
#[derive(Clone, Copy, Default)]
enum RowIcon {
    #[default]
    None,
    File(FileKind),
    Symbol(Icon),
}

/// Ligne de l'explorateur : flèche (dossiers), icône, nom, et à droite un détail.
#[derive(Default)]
struct Row<'a> {
    depth: usize,
    /// Dossier ouvert ou fermé ; rien pour un fichier.
    chevron: Option<bool>,
    icon: RowIcon,
    text: &'a str,
    detail: &'a str,
    selected: bool,
    strong: bool,
    /// Texte en gris (« Dossier vide »).
    muted: bool,
    /// Traits verticaux qui relient la ligne aux dossiers qui la contiennent.
    guides: bool,
}

impl Row<'_> {
    fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
        let response = if self.muted {
            response
        } else {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        };
        let back = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 8.0, rect.top() + 1.0),
            egui::pos2(rect.right() - 8.0, rect.bottom() - 1.0),
        );
        let painter = ui.painter();
        if self.selected {
            painter.rect_filled(back, 7.0, selected());
        } else if response.hovered() && !self.muted {
            painter.rect_filled(back, 7.0, style::tint(8));
        }
        let y = rect.center().y;
        if self.guides {
            // Un trait sous la flèche de chaque dossier parent.
            for level in 1..self.depth {
                let gx = (rect.left() + 19.0 + level as f32 * 14.0).round() + 0.5;
                painter.vline(gx, rect.y_range(), Stroke::new(1.0, border()));
            }
        }
        let mut x = rect.left() + 14.0 + self.depth as f32 * 14.0;
        if let Some(open) = self.chevron {
            let chevron = if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            icons::paint(
                painter,
                egui::Rect::from_center_size(egui::pos2(x + 5.0, y), egui::vec2(12.0, 12.0)),
                chevron,
                icon_color(),
            );
        }
        x += 16.0;
        let icon = egui::Rect::from_center_size(egui::pos2(x + 9.0, y), egui::vec2(18.0, 18.0));
        match self.icon {
            RowIcon::None => {}
            RowIcon::File(kind) => icons::paint_file(painter, icon, kind),
            RowIcon::Symbol(symbol) => icons::paint(painter, icon, symbol, icon_color()),
        }
        if !matches!(self.icon, RowIcon::None) {
            x += 26.0;
        }
        let right = back.right() - 8.0;
        let detail_width = if self.detail.is_empty() {
            0.0
        } else {
            let detail = elide(ui, self.detail, ((right - x) * 0.4).max(0.0), 11.5);
            let shown = painter.text(
                egui::pos2(right, y),
                egui::Align2::RIGHT_CENTER,
                detail,
                FontId::proportional(11.5),
                style::text_secondary(),
            );
            shown.width() + 8.0
        };
        let font = if self.strong {
            FontId::new(14.0, style::strong_family())
        } else {
            FontId::proportional(13.5)
        };
        let color = if self.selected {
            style::accent()
        } else if self.muted {
            style::text_secondary()
        } else {
            style::primary_text()
        };
        let text = elide(ui, self.text, (right - x - detail_width).max(0.0), font.size);
        painter.text(egui::pos2(x, y), egui::Align2::LEFT_CENTER, text, font, color);
        response
    }
}

/// Bouton carré à symbole (barre de titre, recherche, aperçu).
fn icon_button(ui: &mut egui::Ui, icon: Icon, active: bool, hint: &str) -> egui::Response {
    crate::kit::tool_button(ui, crate::kit::TOOL, active, hint, |painter, rect, ink| {
        icons::paint(painter, rect, icon, ink);
    })
}

/// Bouton de la barre d'activité : plus grand, teinté s'il est actif.
fn rail_button(ui: &mut egui::Ui, icon: Icon, active: bool, hint: &str) -> egui::Response {
    let response = crate::kit::tool_button(ui, 40.0, active, hint, |painter, rect, ink| {
        icons::paint(painter, rect, icon, ink);
    });
    ui.add_space(6.0);
    response
}

/// Petit bouton à symbole de l'en-tête de l'explorateur.
fn small_button(ui: &mut egui::Ui, icon: Icon, hint: &str) -> egui::Response {
    crate::kit::tool_button(ui, 28.0, false, hint, |painter, rect, ink| {
        icons::paint(painter, rect, icon, ink);
    })
}

/// Menu du clic droit sur un fichier de l'explorateur.
fn file_menu(response: &egui::Response, path: &Path, commands: &mut Vec<Command>) {
    response.context_menu(|ui| {
        ui.set_min_width(200.0);
        let parent = path.parent().unwrap_or(path).to_path_buf();
        for (text, command) in [
            ("Ouvrir", Command::OpenPath(path.to_path_buf())),
            ("Afficher dans Fichiers", Command::ShowInFiles(parent)),
            ("Copier le chemin", Command::CopyPath(path.to_path_buf())),
        ] {
            if ui.button(text).clicked() {
                commands.push(command);
                ui.close();
            }
        }
    });
}

/// Menu du clic droit sur un dossier de l'explorateur ; `as_root` propose d'en faire
/// le dossier affiché.
fn folder_menu(response: &egui::Response, path: &Path, as_root: bool, commands: &mut Vec<Command>) {
    response.context_menu(|ui| {
        ui.set_min_width(200.0);
        if as_root && ui.button("Afficher ce dossier seul").clicked() {
            commands.push(Command::SetRoot(path.to_path_buf()));
            ui.close();
        }
        for (text, command) in [
            ("Ouvrir dans Fichiers", Command::ShowInFiles(path.to_path_buf())),
            ("Copier le chemin", Command::CopyPath(path.to_path_buf())),
        ] {
            if ui.button(text).clicked() {
                commands.push(command);
                ui.close();
            }
        }
    });
}

/// Onglet de l'aperçu : symbole et titre, soulignés d'un trait bleu s'il est choisi.
fn preview_tab(ui: &mut egui::Ui, icon: Icon, text: &str, selected: bool, bottom: f32) -> egui::Response {
    let font = FontId::proportional(13.5);
    let width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), style::primary_text())
        .size()
        .x
        + 40.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), Sense::click());
    let color = if selected {
        style::accent()
    } else if response.hovered() {
        style::primary_text()
    } else {
        style::text_secondary()
    };
    let painter = ui.painter();
    icons::paint(
        painter,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 12.0, rect.center().y),
            egui::vec2(18.0, 18.0),
        ),
        icon,
        color,
    );
    painter.text(
        egui::pos2(rect.left() + 28.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font,
        color,
    );
    if selected {
        painter.hline(rect.x_range(), bottom - 1.5, Stroke::new(2.0, style::accent()));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Trait vertical entre deux indications de la barre d'état.
fn status_separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 14.0), Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.y_range(), Stroke::new(1.0, border()));
}

/// Élément d'un dossier : chemin, nom, dossier ou non.
type Entry = (PathBuf, String, bool);

/// Contenu d'un dossier et l'heure de sa lecture.
type Listing = (std::time::Instant, std::rc::Rc<Vec<Entry>>);

/// Fichiers et dossiers visibles de `directory` : dossiers d'abord, par nom, sans les
/// fichiers cachés ni les dossiers de compilation.
fn list_directory(directory: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules") {
                return None;
            }
            let path = entry.path();
            let is_dir = path.is_dir();
            Some((path, name, is_dir))
        })
        .collect();
    entries.sort_by(|left, right| {
        right
            .2
            .cmp(&left.2)
            .then_with(|| left.1.to_lowercase().cmp(&right.1.to_lowercase()))
    });
    entries.truncate(400);
    entries
}

/// Fichiers sous `root` dont le nom contient `query` (sans la casse), dossiers proches
/// d'abord ; la recherche s'arrête à 100 résultats ou 5 000 entrées lues.
fn search_files(root: &Path, query: &str) -> Vec<PathBuf> {
    let query = query.to_lowercase();
    let mut found = Vec::new();
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0)]);
    let mut seen = 0;
    while let Some((directory, depth)) = queue.pop_front() {
        for (path, name, is_dir) in list_directory(&directory) {
            seen += 1;
            if seen > 5000 || found.len() >= 100 {
                return found;
            }
            if is_dir {
                if depth < 8 {
                    queue.push_back((path, depth + 1));
                }
            } else if name.to_lowercase().contains(&query) {
                found.push(path);
            }
        }
    }
    found
}

/// Hauteurs du panneau des favoris : hauteur voulue (la liste entière), minimum (l'en-tête
/// seul) et maximum (60 % de la place disponible), en points.
///
/// Le trait de séparation du panneau occupe un point de plus, au-dessus de la marge haute.
fn favorites_heights(rows: usize, available: f32) -> (f32, f32, f32) {
    let header = ROW_HEIGHT + f32::from(FAVORITES_TOP) + f32::from(FAVORITES_BOTTOM) + 1.0;
    let max = (available * 0.6).max(header);
    let wanted = (header + ROW_HEIGHT * rows as f32).clamp(header, max);
    (wanted, header, max)
}

/// Dossiers favoris qui existent : dossier personnel, Bureau, Documents, Téléchargements.
fn favorite_places() -> Vec<(&'static str, PathBuf)> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let first = |names: &[&str]| {
        names
            .iter()
            .map(|name| home.join(name))
            .find(|path| path.is_dir())
    };
    [
        ("Dossier personnel", Some(home.clone())),
        ("Bureau", first(&["Bureau", "Desktop"])),
        ("Documents", first(&["Documents"])),
        ("Téléchargements", first(&["Téléchargements", "Downloads"])),
    ]
    .into_iter()
    .filter_map(|(name, path)| Some((name, path?)))
    .collect()
}

/// Nombre avec une espace entre les milliers : « 1 842 ».
fn group_digits(number: usize) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(digit);
    }
    out
}

/// Taille en octets, Ko ou Mo, à la française.
fn format_size(bytes: usize) -> String {
    match bytes {
        0..1024 => format!("{bytes} octet{}", plural(bytes)),
        1024..1_048_576 => format!("{:.1} Ko", bytes as f64 / 1024.0).replace('.', ","),
        _ => format!("{:.1} Mo", bytes as f64 / 1_048_576.0).replace('.', ","),
    }
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
    fn l_explorateur_garde_le_contenu_des_dossiers_lu() {
        let dir = scratch();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        let mut editor = EditorState::default();
        let first = editor.listing(&dir);
        assert_eq!(first.len(), 1);
        // Un nouveau fichier n'apparaît qu'à la relecture : pas de lecture à chaque image.
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        assert!(std::rc::Rc::ptr_eq(&first, &editor.listing(&dir)));
        // Un enregistrement fait relire les dossiers.
        editor.open(dir.join("a.txt"));
        assert!(editor.save(editor.active, Then::Nothing));
        assert_eq!(editor.listing(&dir).len(), 2);
    }

    #[test]
    fn la_separation_des_favoris_se_tire() {
        let approx = |left: f32, right: f32| (left - right).abs() < 0.01;
        // Liste entière, minimum à l'en-tête, maximum à 60 % de la place.
        let (wanted, min, max) = favorites_heights(3, 600.0);
        assert!(approx(wanted, 139.0));
        assert!(approx(min, 49.0));
        assert!(approx(max, 360.0));
        // Fenêtre basse : la hauteur voulue ne dépasse pas le maximum.
        let (wanted, min, max) = favorites_heights(3, 150.0);
        assert!(approx(wanted, 90.0));
        assert!(approx(min, 49.0));
        assert!(approx(max, 90.0));
        // Replié, le panneau n'a que la hauteur de son en-tête.
        assert!(approx(favorites_heights(0, 600.0).0, 49.0));
    }

    #[test]
    fn la_copie_laisse_les_commentaires() {
        let code = "# Mettre à jour\nsudo apt update   # liste des paquets\n\n// pas du shell\necho \"a # b\" c#d\n#!/bin/sh";
        assert_eq!(
            without_comments(code, FileKind::Shell),
            "sudo apt update\n\n// pas du shell\necho \"a # b\" c#d"
        );
        // Bloc sans langue : lu comme du shell.
        assert_eq!(
            without_comments("# Compiler\ncargo build", FileKind::Text),
            "cargo build"
        );
        assert_eq!(
            without_comments("// début\nlet x = 1; // fin\n/* bloc */", FileKind::Rust),
            "let x = 1;"
        );
    }

    #[test]
    fn le_bouton_copier_met_le_code_dans_le_presse_papier() {
        use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Vec2};

        let context = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0));
        let code = "sudo apt install ferrite-textedit\nferrite-textedit";
        let frame = |events: Vec<Event>, time: f64| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                time: Some(time),
                ..Default::default()
            };
            let mut button = Rect::NOTHING;
            let mut output = context.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    button = copy_button(ui, code).rect;
                });
            });
            output.textures_delta.clear();
            (button, output.platform_output)
        };
        let (button, _) = frame(Vec::new(), 0.0);
        let click = |pressed| Event::PointerButton {
            pos: button.center(),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        frame(vec![Event::PointerMoved(button.center()), click(true)], 0.1);
        let (_, output) = frame(vec![click(false)], 0.2);
        assert!(
            output
                .commands
                .iter()
                .any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == code)),
            "le code part dans le presse-papier"
        );
    }

    #[test]
    fn tirer_la_separation_des_favoris_agrandit_le_panneau() {
        use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Vec2};

        let context = egui::Context::default();
        let mut editor = EditorState::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 800.0));
        let frame = |editor: &EditorState, events: Vec<Event>, time: f64| -> egui::Id {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                time: Some(time),
                ..Default::default()
            };
            let mut scope = egui::Id::NULL;
            let output = context.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    scope = ui.scope_id();
                    editor.favorites(ui, Path::new("/tmp"), &mut Vec::new());
                });
            });
            output.drop_without_applying_deltas();
            scope
        };

        let scope = frame(&editor, Vec::new(), 0.0);
        // Le panneau reçoit un identifiant déjà salé, que le panneau sale à nouveau.
        let id = scope.with_salt(egui::IdSalt::new(scope.with("textedit-favoris")));
        let panel = egui::PanelState::load(&context, id).expect("le panneau des favoris est posé");
        let before = panel.size().y;
        assert!(before > ROW_HEIGHT, "l'en-tête et la liste sont visibles");

        // Pression sur le trait de séparation…
        let start = panel.outer_rect.center_top();
        let press = |pos| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        };
        frame(&editor, vec![Event::PointerMoved(start), press(start)], 0.1);
        // … puis glissement de 40 points vers le haut, et relâchement.
        let end = start - Vec2::new(0.0, 40.0);
        frame(&editor, vec![Event::PointerMoved(end)], 0.2);
        frame(
            &editor,
            vec![Event::PointerButton {
                pos: end,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            0.3,
        );

        let after = egui::PanelState::load(&context, id)
            .expect("le panneau des favoris est toujours posé")
            .size()
            .y;
        assert!(
            (after - (before + 40.0)).abs() < 1.5,
            "la séparation suit la souris : {before} puis {after}"
        );

        // Replié, le panneau se réduit à son en-tête…
        editor.favorites_open = false;
        let closed_scope = frame(&editor, Vec::new(), 0.4);
        let closed_id =
            closed_scope.with_salt(egui::IdSalt::new(closed_scope.with("textedit-favoris-replie")));
        let closed = egui::PanelState::load(&context, closed_id)
            .expect("le panneau replié est posé")
            .size()
            .y;
        assert!((closed - 49.0).abs() < 0.5, "en-tête seul : {closed}");
        // … et déplié, il retrouve la hauteur choisie, pas celle de l'en-tête.
        editor.favorites_open = true;
        frame(&editor, Vec::new(), 0.5);
        let reopened = egui::PanelState::load(&context, id)
            .expect("le panneau déplié est toujours posé")
            .size()
            .y;
        assert!((reopened - after).abs() < 0.5, "hauteur retrouvée : {reopened}");
    }

    #[test]
    fn colonnes_d_un_tableau() {
        // Tout tient : largeurs naturelles.
        assert_eq!(
            column_widths(&[40.0, 60.0], &[40.0, 30.0], 200.0),
            vec![40.0, 60.0]
        );
        // Trop large : la colonne courte garde sa largeur, la longue prend le reste.
        let widths = column_widths(&[40.0, 400.0, 30.0], &[40.0, 80.0, 30.0], 200.0);
        assert_eq!(widths[0], 40.0);
        assert_eq!(widths[2], 30.0);
        assert!((widths[1] - 130.0).abs() < 0.01);
        // Même resserrées, les colonnes gardent leur mot le plus long.
        let widths = column_widths(&[300.0, 300.0], &[120.0, 120.0], 100.0);
        assert_eq!(widths, vec![120.0, 120.0]);
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
        let context = egui::Context::default();
        editor.new_document();
        editor.documents[1].contents = "à garder".into();
        editor.run(Command::Close(1), &context);
        assert_eq!(editor.ask, Some(Ask::Close(1)));
        assert_eq!(editor.documents.len(), 2);
        editor.run(Command::Answer(Some(false)), &context);
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
        let context = egui::Context::default();
        editor.run(Command::ReplaceOne, &context);
        assert_eq!(editor.document().contents, "un chien, deux chats");
        editor.run(Command::ReplaceAll, &context);
        assert_eq!(editor.document().contents, "un chien, deux chiens");
    }

    #[test]
    fn position_du_curseur() {
        assert_eq!(line_column("ab\ncdé\nf", 0), (1, 1));
        assert_eq!(line_column("ab\ncdé\nf", 5), (2, 3));
        assert_eq!(line_column("ab\ncdé\nf", 7), (3, 1));
    }

    #[test]
    fn les_documents_markdown_commencent_en_mode_partage() {
        assert_eq!(
            Document::new(Some(PathBuf::from("README.MD")), String::new()).markdown_mode,
            MarkdownMode::Split
        );
        assert_eq!(
            Document::new(Some(PathBuf::from("notes.txt")), String::new()).markdown_mode,
            MarkdownMode::Edit
        );
    }

    #[test]
    fn apercu_markdown_analyse_titres_listes_et_code() {
        let blocks = markdown::parse(
            "# Ferrite OS\n\nUn **petit** système x86-64.\n\n| Bibliothèque | Rôle | Licence |\n|---|---|---|\n| RustCrypto | Chiffrement | MIT |\n\n## Démarrer\n\n```bash\ncargo xtask run\n```",
        );
        assert!(matches!(blocks.first(), Some(markdown::Block::Heading(1, _))));
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block, markdown::Block::Paragraph(_)))
        );
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block, markdown::Block::Table { .. }))
        );
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block, markdown::Block::Heading(2, _)))
        );
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block, markdown::Block::Code { .. }))
        );
    }

    #[test]
    fn apercu_markdown_alignement_ferrite() {
        let blocks = markdown::parse("::: align=center\nTexte centré\n:::");
        assert!(matches!(
            blocks.first(),
            Some(markdown::Block::Aligned(markdown::Align::Center, _))
        ));
    }

    #[test]
    fn apercu_markdown_liste_basique() {
        let blocks = markdown::parse("- **élément**\n\n```rs\nlet x = 1;\n```");
        assert!(matches!(blocks.first(), Some(markdown::Block::List(_))));
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block, markdown::Block::List(_)))
        );
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block, markdown::Block::Code { .. }))
        );
    }

    #[test]
    fn apercu_markdown_entete_yaml_et_image() {
        let (meta, body) =
            markdown::front_matter("---\ntitle: TextEdit Pro\nversion: \"1.0\"\n---\n![Capture](./a.png)\n");
        assert_eq!(
            meta,
            vec![
                ("title".into(), "TextEdit Pro".into()),
                ("version".into(), "1.0".into())
            ]
        );
        let blocks = markdown::parse(body);
        assert!(matches!(
            blocks.first(),
            Some(markdown::Block::Image { alt, path, .. }) if alt == "Capture" && path == "./a.png"
        ));
        assert!(markdown::front_matter("---\njamais refermé").0.is_empty());
    }

    #[test]
    fn l_explorateur_s_ouvre_sur_le_dossier_du_document() {
        let directory = scratch();
        fs::create_dir_all(directory.join("src/pages")).unwrap();
        fs::write(directory.join("README.md"), "# Projet").unwrap();
        fs::write(directory.join("src/pages/Home.tsx"), "export {}").unwrap();
        let mut editor = EditorState::default();
        editor.open(directory.join("README.md"));
        let root = editor.root();
        editor.open(directory.join("src/pages/Home.tsx"));
        assert_eq!(editor.root(), root, "un fichier du même dossier garde la racine");
        assert!(editor.expanded.contains(&root.join("src")));
        assert!(editor.expanded.contains(&root.join("src/pages")));
        assert_eq!(search_files(&root, "home"), vec![root.join("src/pages/Home.tsx")]);
        assert_eq!(group_digits(1842), "1 842");
        assert_eq!(format_size(2048), "2,0 Ko");
        fs::remove_dir_all(directory).unwrap();
    }
}
