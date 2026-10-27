//! Fichiers : gestionnaire de fichiers de Ferrite.
//!
//! - affichage en liste (colonnes triables) ou en grille d'icônes, fichiers cachés à la
//!   demande, recherche dans le dossier ;
//! - navigation : précédent, suivant, dossier parent, chemin cliquable ou saisi (Ctrl+L),
//!   barre latérale (Favoris, Corbeille, clés USB) ;
//! - sélection multiple (Ctrl, Maj, Ctrl+A), menu contextuel, raccourcis habituels ;
//! - copier, couper, coller, dupliquer (en arrière-plan, avec progression), renommer (F2),
//!   nouveau dossier, Corbeille (restaurer, vider), suppression définitive confirmée ;
//! - ouvrir, ouvrir avec…, propriétés, ouvrir un terminal ici ;
//! - montage et éjection des clés USB ;
//! - mode sélecteur (`FilesState::picker`) : les autres applications l'affichent pour
//!   choisir le fichier à ouvrir ou l'emplacement d'enregistrement.
//!
//! L'interface ne modifie jamais l'état pendant son propre dessin : chaque clic produit une
//! `Action`, appliquée une fois l'image construite.

mod devices;
mod ops;
mod trash;

use std::{
    collections::BTreeSet,
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant, SystemTime},
};

use eframe::egui::{self, Color32, CornerRadius, Frame, Key, Margin, RichText, Sense, Stroke};
use ferrite_system::{applications::DesktopEntry, command::spawn_detached};

use self::{devices::Device, ops::Job, trash::TrashItem};
use crate::style::{self, IconStyle, Icons};

const ROW: f32 = 32.0;
const GRID_CELL: egui::Vec2 = egui::vec2(108.0, 104.0);

#[derive(Debug, Clone)]
struct FileEntry {
    name: String,
    path: PathBuf,
    directory: bool,
    symlink: bool,
    modified: Option<SystemTime>,
    size: Option<u64>,
    /// Élément de la Corbeille : emplacement d'origine et date de suppression.
    trash: Option<TrashItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Location {
    Folder(PathBuf),
    Trash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Name,
    Size,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    List,
    Grid,
}

/// Usage du sélecteur de fichiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerMode {
    Open,
    Save,
}

#[derive(Debug)]
struct Picker {
    mode: PickerMode,
    /// Nom du fichier à enregistrer.
    name: String,
    name_focused: bool,
    /// Réponse : `Some(Some(chemin))` choisi, `Some(None)` annulé.
    choice: Option<Option<PathBuf>>,
}

#[derive(Debug)]
enum Dialog {
    Replace(PathBuf),
    DeleteForever(Vec<PathBuf>),
    EmptyTrash,
    Properties(Box<Properties>),
    OpenWith(PathBuf, Vec<DesktopEntry>),
}

#[derive(Debug)]
struct Properties {
    path: PathBuf,
    kind: String,
    size: Option<(u64, u64)>,
    modified: Option<SystemTime>,
    permissions: String,
}

/// Résultat d'un calcul en arrière-plan.
enum Update {
    Devices(Vec<Device>, Option<u64>),
    FolderSize(PathBuf, (u64, u64)),
}

/// Ce que l'utilisateur a demandé pendant le dessin.
#[derive(Debug, Clone)]
enum Action {
    Navigate(PathBuf),
    ShowTrash,
    Back,
    Forward,
    Up,
    Open(PathBuf, bool),
    Select { index: usize, ctrl: bool, shift: bool },
    SelectOnly(usize),
    SelectAll,
    ClearSelection,
    StartRename(PathBuf),
    CommitRename,
    CancelRename,
    Copy,
    Cut,
    Paste,
    Duplicate,
    Trash,
    AskDeleteForever,
    ConfirmDeleteForever(Vec<PathBuf>),
    Restore,
    AskEmptyTrash,
    ConfirmEmptyTrash,
    NewFolder,
    Properties(PathBuf),
    OpenWith(PathBuf),
    LaunchWith(PathBuf, DesktopEntry),
    TerminalHere,
    ToggleHidden,
    SetView(View),
    Sort(SortKey),
    EditPath,
    CommitPath,
    Mount(Device),
    Eject(Device),
    CloseDialog,
    CancelJob(usize),
    Choose(PathBuf),
    ConfirmSave,
    CancelPicker,
}

pub struct FilesState {
    location: Location,
    entries: Vec<FileEntry>,
    selected: BTreeSet<PathBuf>,
    /// Dernier élément cliqué, point de départ d'une sélection avec Maj.
    anchor: Option<usize>,
    back: Vec<Location>,
    forward: Vec<Location>,
    show_hidden: bool,
    view: View,
    sort: SortKey,
    ascending: bool,
    search: String,
    path_edit: Option<String>,
    rename: Option<(PathBuf, String, bool)>,
    clipboard: Option<(Vec<PathBuf>, bool)>,
    dialog: Option<Dialog>,
    jobs: Vec<Job>,
    devices: Vec<Device>,
    free_space: Option<u64>,
    devices_checked: Option<Instant>,
    devices_pending: bool,
    sender: Sender<Update>,
    receiver: Receiver<Update>,
    message: Option<(String, bool)>,
    /// Date de modification du dossier lors de sa dernière lecture.
    loaded: Option<SystemTime>,
    icons: Icons,
    picker: Option<Picker>,
}

impl std::fmt::Debug for FilesState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilesState")
            .field("location", &self.location)
            .finish()
    }
}

impl Default for FilesState {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        let mut state = Self {
            location: Location::Folder(home()),
            entries: Vec::new(),
            selected: BTreeSet::new(),
            anchor: None,
            back: Vec::new(),
            forward: Vec::new(),
            show_hidden: false,
            view: View::List,
            sort: SortKey::Name,
            ascending: true,
            search: String::new(),
            path_edit: None,
            rename: None,
            clipboard: None,
            dialog: None,
            jobs: Vec::new(),
            devices: Vec::new(),
            free_space: None,
            devices_checked: None,
            devices_pending: false,
            sender,
            receiver,
            message: None,
            loaded: None,
            icons: Icons::default(),
            picker: None,
        };
        state.load_preferences();
        state.refresh();
        state
    }
}

/// Préférences d'affichage, gardées d'une ouverture à l'autre.
fn preferences_path() -> PathBuf {
    ferrite_system::config::Settings::path().with_file_name("fichiers")
}

fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

impl FilesState {
    /// Sélecteur de fichier, ouvert dans `start` (ou le dossier personnel). En
    /// enregistrement, `name` est le nom proposé.
    pub fn picker(mode: PickerMode, start: Option<&Path>, name: &str) -> Self {
        let mut state = Self {
            picker: Some(Picker {
                mode,
                name: name.to_owned(),
                name_focused: false,
                choice: None,
            }),
            ..Self::default()
        };
        if let Some(dir) = start.filter(|dir| dir.is_dir()) {
            state.location = Location::Folder(dir.to_path_buf());
            state.refresh();
        }
        state
    }

    /// Réponse du sélecteur, une fois donnée : `Some(None)` si l'utilisateur a annulé.
    pub fn take_choice(&mut self) -> Option<Option<PathBuf>> {
        self.picker.as_mut()?.choice.take()
    }

    /// Ouvre `path` : un dossier s'affiche, un fichier est montré sélectionné dans le sien.
    pub fn open_at(&mut self, path: PathBuf) {
        let path = fs::canonicalize(&path).unwrap_or(path);
        if path.is_dir() {
            self.location = Location::Folder(path);
            self.refresh();
        } else if let Some(parent) = path.parent().filter(|parent| parent.is_dir()) {
            self.location = Location::Folder(parent.to_path_buf());
            self.refresh();
            if let Some(index) = self.entries.iter().position(|entry| entry.path == path) {
                self.show_hidden |= self.entries[index].name.starts_with('.');
                self.selected.insert(path);
                self.anchor = Some(index);
            }
        }
    }

    fn load_preferences(&mut self) {
        let Ok(text) = fs::read_to_string(preferences_path()) else {
            return;
        };
        for line in text.lines() {
            match line.split_once('=') {
                Some(("vue", "grille")) => self.view = View::Grid,
                Some(("vue", "liste")) => self.view = View::List,
                Some(("tri", "nom")) => self.sort = SortKey::Name,
                Some(("tri", "taille")) => self.sort = SortKey::Size,
                Some(("tri", "date")) => self.sort = SortKey::Modified,
                Some(("croissant", value)) => self.ascending = value == "oui",
                Some(("caches", value)) => self.show_hidden = value == "oui",
                _ => {}
            }
        }
    }

    fn save_preferences(&self) {
        let yes_no = |value: bool| if value { "oui" } else { "non" };
        let text = format!(
            "vue={}
tri={}
croissant={}
caches={}
",
            match self.view {
                View::List => "liste",
                View::Grid => "grille",
            },
            match self.sort {
                SortKey::Name => "nom",
                SortKey::Size => "taille",
                SortKey::Modified => "date",
            },
            yes_no(self.ascending),
            yes_no(self.show_hidden),
        );
        let path = preferences_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, text);
    }

    fn current_dir(&self) -> Option<&Path> {
        match &self.location {
            Location::Folder(path) => Some(path),
            Location::Trash => None,
        }
    }

    fn go(&mut self, location: Location) {
        if location == self.location {
            return;
        }
        if let Location::Folder(path) = &location
            && !path.is_dir()
        {
            self.error(format!("Dossier introuvable : {}", path.display()));
            return;
        }
        let previous = std::mem::replace(&mut self.location, location);
        self.back.push(previous);
        self.forward.clear();
        self.search.clear();
        self.refresh();
    }

    fn go_back(&mut self) {
        if let Some(location) = self.back.pop() {
            let previous = std::mem::replace(&mut self.location, location);
            self.forward.push(previous);
            self.refresh();
        }
    }

    fn go_forward(&mut self) {
        if let Some(location) = self.forward.pop() {
            let previous = std::mem::replace(&mut self.location, location);
            self.back.push(previous);
            self.refresh();
        }
    }

    fn error(&mut self, message: impl Into<String>) {
        self.message = Some((message.into(), true));
    }

    fn info(&mut self, message: impl Into<String>) {
        self.message = Some((message.into(), false));
    }

    fn refresh(&mut self) {
        self.rename = None;
        self.entries = match &self.location {
            Location::Trash => {
                self.loaded = modified(&trash::files_dir());
                trash::list()
                    .into_iter()
                    .map(|item| {
                        let metadata = item.path.symlink_metadata().ok();
                        FileEntry {
                            name: item
                                .original
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            path: item.path.clone(),
                            directory: metadata.as_ref().is_some_and(|m| m.is_dir()),
                            symlink: false,
                            modified: metadata.as_ref().and_then(|m| m.modified().ok()),
                            size: metadata.filter(|m| !m.is_dir()).map(|m| m.len()),
                            trash: Some(item),
                        }
                    })
                    .collect()
            }
            Location::Folder(dir) => {
                self.loaded = modified(dir);
                match fs::read_dir(dir) {
                    Ok(entries) => entries
                        .filter_map(Result::ok)
                        .filter_map(|entry| {
                            let path = entry.path();
                            let link = entry.metadata().ok()?;
                            let symlink = link.file_type().is_symlink();
                            // Un lien vers un dossier s'ouvre comme un dossier.
                            let metadata = fs::metadata(&path).unwrap_or(link);
                            Some(FileEntry {
                                name: entry.file_name().to_string_lossy().into_owned(),
                                directory: metadata.is_dir(),
                                symlink,
                                modified: metadata.modified().ok(),
                                size: (!metadata.is_dir()).then_some(metadata.len()),
                                path,
                                trash: None,
                            })
                        })
                        .collect(),
                    Err(error) => {
                        self.error(format!("Impossible de lire ce dossier : {error}"));
                        Vec::new()
                    }
                }
            }
        };
        self.sort_entries();
        let paths: BTreeSet<PathBuf> = self.entries.iter().map(|e| e.path.clone()).collect();
        self.selected.retain(|path| paths.contains(path));
        self.anchor = None;
    }

    fn sort_entries(&mut self) {
        let (key, ascending) = (self.sort, self.ascending);
        self.entries.sort_by(|a, b| {
            let order = match key {
                SortKey::Name => natural_cmp(&a.name, &b.name),
                SortKey::Size => a.size.unwrap_or(0).cmp(&b.size.unwrap_or(0)),
                SortKey::Modified => a.modified.cmp(&b.modified),
            };
            let order = if ascending { order } else { order.reverse() };
            // Les dossiers toujours en tête, comme dans le Finder et Fichiers d'elementary.
            b.directory.cmp(&a.directory).then(order)
        });
    }

    /// Éléments affichés : cachés et recherche appliqués.
    fn visible(&self) -> Vec<usize> {
        let query = self.search.trim().to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| self.show_hidden || !entry.name.starts_with('.'))
            .filter(|(_, entry)| query.is_empty() || entry.name.to_lowercase().contains(&query))
            .map(|(index, _)| index)
            .collect()
    }

    fn selection(&self) -> Vec<PathBuf> {
        self.selected.iter().cloned().collect()
    }

    fn open(&mut self, path: PathBuf, directory: bool) {
        if directory {
            self.go(Location::Folder(path));
            return;
        }
        let result = if is_text_file(&path) {
            // TextEdit est le même exécutable que Fichiers.
            env::current_exe().map(Command::new).and_then(|mut command| {
                command.arg("textedit").arg(&path);
                spawn_detached(command)
            })
        } else {
            let mut command = Command::new("xdg-open");
            command.arg(&path);
            spawn_detached(command)
        };
        if let Err(error) = result {
            self.error(format!("Ouverture impossible : {error}"));
        }
    }

    /// Lance une tâche longue (copie, déplacement) dans un fil à part.
    fn run_job(&mut self, title: &str, work: impl FnOnce(&Job) -> Result<(), String> + Send + 'static) {
        let job = Job::new(title);
        let worker = job.clone();
        thread::spawn(move || {
            let result = work(&worker);
            worker.finish(result);
        });
        self.jobs.push(job);
    }

    fn apply(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::Navigate(path) => self.go(Location::Folder(path)),
            Action::ShowTrash => self.go(Location::Trash),
            Action::Back => self.go_back(),
            Action::Forward => self.go_forward(),
            Action::Up => {
                if let Some(parent) = self.current_dir().and_then(Path::parent) {
                    self.go(Location::Folder(parent.to_path_buf()));
                }
            }
            Action::Open(path, directory) if self.picker.is_some() => {
                if directory {
                    self.go(Location::Folder(path));
                } else if let Some(picker) = &mut self.picker {
                    match picker.mode {
                        PickerMode::Open => picker.choice = Some(Some(path)),
                        PickerMode::Save => {
                            picker.name = file_name(&path);
                            self.apply(Action::ConfirmSave, ctx);
                        }
                    }
                }
            }
            Action::Open(path, directory) => {
                if self.location == Location::Trash {
                    self.dialog = Some(Dialog::Properties(Box::new(properties(&path))));
                } else {
                    self.open(path, directory);
                }
            }
            Action::Select { index, ctrl, shift } => {
                let path = self.entries[index].path.clone();
                if shift && let Some(anchor) = self.anchor {
                    let visible = self.visible();
                    let from = visible.iter().position(|&i| i == anchor);
                    let to = visible.iter().position(|&i| i == index);
                    if let (Some(from), Some(to)) = (from, to) {
                        if !ctrl {
                            self.selected.clear();
                        }
                        for &i in &visible[from.min(to)..=from.max(to)] {
                            self.selected.insert(self.entries[i].path.clone());
                        }
                    }
                } else if ctrl {
                    if !self.selected.remove(&path) {
                        self.selected.insert(path);
                    }
                    self.anchor = Some(index);
                } else {
                    self.selected.clear();
                    self.selected.insert(path.clone());
                    self.anchor = Some(index);
                    // Enregistrer : cliquer un fichier reprend son nom (pour le remplacer).
                    if let Some(picker) = &mut self.picker
                        && picker.mode == PickerMode::Save
                        && !self.entries[index].directory
                    {
                        picker.name = file_name(&path);
                    }
                }
            }
            Action::SelectOnly(index) => {
                let path = self.entries[index].path.clone();
                if !self.selected.contains(&path) {
                    self.selected.clear();
                    self.selected.insert(path);
                    self.anchor = Some(index);
                }
            }
            Action::SelectAll => {
                self.selected = self
                    .visible()
                    .into_iter()
                    .map(|i| self.entries[i].path.clone())
                    .collect();
            }
            Action::ClearSelection => self.selected.clear(),
            Action::StartRename(path) => {
                if self.location != Location::Trash {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.rename = Some((path, name, false));
                }
            }
            Action::CommitRename => {
                if let Some((path, name, _)) = self.rename.take() {
                    match ops::rename(&path, &name) {
                        Ok(target) => {
                            self.refresh();
                            self.selected = BTreeSet::from([target]);
                        }
                        Err(error) => self.error(error),
                    }
                }
            }
            Action::CancelRename => self.rename = None,
            Action::Copy | Action::Cut => {
                let selection = self.selection();
                if !selection.is_empty() && self.location != Location::Trash {
                    let cut = matches!(action, Action::Cut);
                    let count = selection.len();
                    // Les chemins vont aussi dans le presse-papiers du système : on peut les
                    // coller dans un texte, et Ctrl+V arrive bien à Fichiers (sous Wayland,
                    // egui ne signale « coller » que si le presse-papiers contient du texte).
                    let paths: Vec<String> =
                        selection.iter().map(|path| path.display().to_string()).collect();
                    ctx.copy_text(paths.join(
                        "
",
                    ));
                    self.clipboard = Some((selection, cut));
                    self.info(format!(
                        "{count} élément{} {}",
                        plural(count),
                        if cut { "coupé(s)" } else { "copié(s)" }
                    ));
                }
            }
            Action::Paste => {
                if let (Some((sources, cut)), Some(dir)) = (self.clipboard.clone(), self.current_dir()) {
                    let dir = dir.to_path_buf();
                    if cut {
                        self.clipboard = None;
                        self.run_job("Déplacement", move |job| ops::move_into(&sources, &dir, job));
                    } else {
                        self.run_job("Copie", move |job| ops::copy_into(&sources, &dir, job));
                    }
                }
            }
            Action::Duplicate => {
                if let Some(dir) = self.current_dir() {
                    let dir = dir.to_path_buf();
                    let sources = self.selection();
                    if !sources.is_empty() {
                        self.run_job("Duplication", move |job| ops::copy_into(&sources, &dir, job));
                    }
                }
            }
            Action::Trash => {
                let selection = self.selection();
                let mut failed = None;
                for path in &selection {
                    if let Err(error) = trash::trash(path) {
                        failed = Some(error);
                    }
                }
                self.refresh();
                match failed {
                    Some(error) => self.error(error),
                    None if !selection.is_empty() => self.info(format!(
                        "{} élément{} mis à la Corbeille",
                        selection.len(),
                        plural(selection.len())
                    )),
                    None => {}
                }
            }
            Action::AskDeleteForever => {
                let selection = self.selection();
                if !selection.is_empty() {
                    self.dialog = Some(Dialog::DeleteForever(selection));
                }
            }
            Action::ConfirmDeleteForever(paths) => {
                self.dialog = None;
                let in_trash = self.location == Location::Trash;
                for path in paths {
                    let result = if in_trash {
                        trash::delete(&path)
                    } else {
                        ops::remove_tree(&path)
                    };
                    if let Err(error) = result {
                        self.error(error);
                    }
                }
                self.refresh();
            }
            Action::Restore => {
                for path in self.selection() {
                    if let Err(error) = trash::restore(&path) {
                        self.error(error);
                    }
                }
                self.refresh();
            }
            Action::AskEmptyTrash => self.dialog = Some(Dialog::EmptyTrash),
            Action::ConfirmEmptyTrash => {
                self.dialog = None;
                if let Err(error) = trash::empty() {
                    self.error(error);
                }
                self.refresh();
            }
            Action::NewFolder => {
                if let Some(dir) = self.current_dir() {
                    match ops::create_folder(dir) {
                        Ok(folder) => {
                            self.refresh();
                            self.selected = BTreeSet::from([folder.clone()]);
                            self.apply(Action::StartRename(folder), ctx);
                        }
                        Err(error) => self.error(error),
                    }
                }
            }
            Action::Properties(path) => {
                let properties = properties(&path);
                if properties.size.is_none() {
                    // Taille d'un dossier : calculée à part, elle peut être longue.
                    let sender = self.sender.clone();
                    let ctx = ctx.clone();
                    thread::spawn(move || {
                        let size = ops::tree_size(&path);
                        let _ = sender.send(Update::FolderSize(path, size));
                        ctx.request_repaint();
                    });
                }
                self.dialog = Some(Dialog::Properties(Box::new(properties)));
            }
            Action::OpenWith(path) => {
                let apps = ferrite_system::applications::installed();
                self.dialog = Some(Dialog::OpenWith(path, apps));
            }
            Action::LaunchWith(path, app) => {
                self.dialog = None;
                if let Some((program, args)) = app.exec.split_first() {
                    let mut command = Command::new(program);
                    command.args(args).arg(&path);
                    if let Err(error) = spawn_detached(command) {
                        self.error(format!("{} : {error}", app.name));
                    }
                }
            }
            Action::TerminalHere => {
                let dir = self.current_dir().map(Path::to_path_buf).unwrap_or_else(home);
                let mut command = Command::new("foot");
                command.arg(format!("--working-directory={}", dir.display()));
                if let Err(error) = spawn_detached(command) {
                    self.error(format!("Terminal indisponible : {error}"));
                }
            }
            Action::ToggleHidden => {
                self.show_hidden = !self.show_hidden;
                self.save_preferences();
            }
            Action::SetView(view) => {
                self.view = view;
                self.save_preferences();
            }
            Action::Sort(key) => {
                if self.sort == key {
                    self.ascending = !self.ascending;
                } else {
                    self.sort = key;
                    self.ascending = key == SortKey::Name;
                }
                self.sort_entries();
                self.save_preferences();
            }
            Action::EditPath => {
                if let Some(dir) = self.current_dir() {
                    self.path_edit = Some(dir.display().to_string());
                }
            }
            Action::CommitPath => {
                if let Some(text) = self.path_edit.take() {
                    let path = match text.strip_prefix('~') {
                        Some(rest) => PathBuf::from(format!("{}{rest}", home().display())),
                        None => PathBuf::from(text),
                    };
                    self.go(Location::Folder(path));
                }
            }
            Action::Mount(device) => match devices::mount(&device) {
                Ok(path) => {
                    self.devices_checked = None;
                    self.go(Location::Folder(path));
                }
                Err(error) => self.error(error),
            },
            Action::Eject(device) => {
                if self
                    .current_dir()
                    .zip(device.mount.as_deref())
                    .is_some_and(|(dir, mount)| dir.starts_with(mount))
                {
                    self.go(Location::Folder(home()));
                }
                match devices::eject(&device) {
                    Ok(()) => self.info(format!("« {} » peut être retiré", device.label)),
                    Err(error) => self.error(error),
                }
                self.devices_checked = None;
            }
            Action::CloseDialog => self.dialog = None,
            Action::CancelJob(index) => {
                if let Some(job) = self.jobs.get(index) {
                    job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            Action::Choose(path) => {
                self.dialog = None;
                if let Some(picker) = &mut self.picker {
                    picker.choice = Some(Some(path));
                }
            }
            Action::CancelPicker => {
                if let Some(picker) = &mut self.picker {
                    picker.choice = Some(None);
                }
            }
            Action::ConfirmSave => {
                let Some(picker) = &self.picker else { return };
                let name = match ops::validate_name(&picker.name) {
                    Ok(name) => name.to_owned(),
                    Err(error) => return self.error(error),
                };
                let Some(dir) = self.current_dir() else {
                    return self.error("Choisis un dossier pour enregistrer");
                };
                let target = dir.join(&name);
                if target.is_dir() {
                    self.go(Location::Folder(target));
                } else if target.exists() {
                    self.dialog = Some(Dialog::Replace(target));
                } else {
                    self.apply(Action::Choose(target), ctx);
                }
            }
        }
    }

    /// Travaux de fond : périphériques, tailles de dossier, fin des copies.
    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(update) = self.receiver.try_recv() {
            match update {
                Update::Devices(devices, free) => {
                    self.devices = devices;
                    self.free_space = free;
                    self.devices_pending = false;
                }
                Update::FolderSize(path, size) => {
                    if let Some(Dialog::Properties(properties)) = &mut self.dialog
                        && properties.path == path
                    {
                        properties.size = Some(size);
                    }
                }
            }
        }
        if !self.devices_pending
            && self
                .devices_checked
                .is_none_or(|at| at.elapsed() > Duration::from_secs(3))
        {
            self.devices_pending = true;
            self.devices_checked = Some(Instant::now());
            let sender = self.sender.clone();
            let dir = self.current_dir().map(Path::to_path_buf).unwrap_or_else(home);
            let ctx = ctx.clone();
            thread::spawn(move || {
                let _ = sender.send(Update::Devices(devices::removable(), devices::free_space(&dir)));
                ctx.request_repaint();
            });
        }
        let mut finished = false;
        let mut errors = Vec::new();
        self.jobs.retain(|job| {
            let progress = job.snapshot();
            if progress.finished {
                finished = true;
                if let Some(error) = progress.error {
                    errors.push(format!("{} : {error}", job.title));
                }
            }
            !progress.finished
        });
        if finished {
            self.refresh();
            if let Some(error) = errors.pop() {
                self.error(error);
            }
        }
        // Le dossier suit les changements faits ailleurs (F5 force la relecture).
        let stamp = match &self.location {
            Location::Folder(dir) => modified(dir),
            Location::Trash => modified(&trash::files_dir()),
        };
        if self.rename.is_none() && stamp != self.loaded {
            self.refresh();
        }
        if !self.jobs.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(Duration::from_secs(2));
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let mut actions = Vec::new();
        self.keyboard(ui, &mut actions);

        // Colonne sur toute la hauteur (boutons de fenêtre en haut, sauf dans une feuille),
        // puis barre d'outils et contenu à droite.
        let buttons = self.picker.is_none();
        style::sidebar(ui, "emplacements", 210.0, style::SIDEBAR, buttons, |ui| {
            self.sidebar(ui, &mut actions)
        });
        style::toolbar(ui, false, |ui| self.header(ui, &mut actions));
        if self.picker.is_some() {
            self.picker_bar(ui, &mut actions);
        } else {
            self.status_bar(ui, &mut actions);
        }
        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(style::BACKGROUND))
            .show(ui, |ui| match self.view {
                View::List => self.list(ui, &mut actions),
                View::Grid => self.grid(ui, &mut actions),
            });
        self.dialogs(&ctx, &mut actions);

        for action in actions {
            self.apply(action, &ctx);
        }
    }

    fn keyboard(&mut self, ui: &egui::Ui, actions: &mut Vec<Action>) {
        if self.rename.is_some() || self.path_edit.is_some() || self.dialog.is_some() {
            return;
        }
        let typing = ui.ctx().memory(|memory| memory.focused().is_some());
        ui.input(|input| {
            let (ctrl, shift, alt) = (
                input.modifiers.command,
                input.modifiers.shift,
                input.modifiers.alt,
            );
            let pressed = |key| input.key_pressed(key);
            if ctrl && pressed(Key::L) {
                actions.push(Action::EditPath);
            }
            if ctrl && pressed(Key::H) {
                actions.push(Action::ToggleHidden);
            }
            if ctrl && shift && pressed(Key::N) {
                actions.push(Action::NewFolder);
            }
            if (alt && pressed(Key::ArrowLeft)) || (!typing && pressed(Key::Backspace)) {
                actions.push(Action::Back);
            }
            if alt && pressed(Key::ArrowRight) {
                actions.push(Action::Forward);
            }
            if alt && pressed(Key::ArrowUp) {
                actions.push(Action::Up);
            }
            if typing {
                return;
            }
            if ctrl && pressed(Key::A) {
                actions.push(Action::SelectAll);
            }
            // Sous Wayland, egui livre Ctrl+C, Ctrl+X et Ctrl+V comme des événements
            // copier / couper / coller, et non comme des touches.
            let event = |wanted: fn(&egui::Event) -> bool| input.events.iter().any(wanted);
            if (ctrl && pressed(Key::C)) || event(|e| matches!(e, egui::Event::Copy)) {
                actions.push(Action::Copy);
            }
            if (ctrl && pressed(Key::X)) || event(|e| matches!(e, egui::Event::Cut)) {
                actions.push(Action::Cut);
            }
            if (ctrl && pressed(Key::V)) || event(|e| matches!(e, egui::Event::Paste(_))) {
                actions.push(Action::Paste);
            }
            if ctrl && pressed(Key::D) {
                actions.push(Action::Duplicate);
            }
            if pressed(Key::Delete) {
                actions.push(if shift || self.location == Location::Trash {
                    Action::AskDeleteForever
                } else {
                    Action::Trash
                });
            }
            if pressed(Key::F2) && self.selected.len() == 1 {
                actions.push(Action::StartRename(self.selection()[0].clone()));
            }
            if pressed(Key::Enter) && self.selected.len() == 1 {
                let path = self.selection()[0].clone();
                actions.push(Action::Open(path.clone(), path.is_dir()));
            }
            if pressed(Key::Escape) {
                actions.push(if self.picker.is_some() {
                    Action::CancelPicker
                } else {
                    Action::ClearSelection
                });
            }
            if pressed(Key::F5) {
                self.loaded = None;
            }
        });
    }

    fn header(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        if nav_button(ui, Glyph::Back, !self.back.is_empty(), "Précédent (Alt+←)").clicked() {
            actions.push(Action::Back);
        }
        if nav_button(ui, Glyph::Forward, !self.forward.is_empty(), "Suivant (Alt+→)").clicked() {
            actions.push(Action::Forward);
        }
        ui.add_space(4.0);
        // Côté droit d'abord (vue, recherche), puis le chemin dans la place restante.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if view_button(ui, Glyph::Grid, self.view == View::Grid, "Grille").clicked() {
                actions.push(Action::SetView(View::Grid));
            }
            if view_button(ui, Glyph::List, self.view == View::List, "Liste").clicked() {
                actions.push(Action::SetView(View::List));
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Rechercher")
                    .desired_width(180.0),
            );
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if let Some(text) = &mut self.path_edit {
                    let response =
                        ui.add(egui::TextEdit::singleline(text).desired_width(ui.available_width() - 8.0));
                    if !response.has_focus() {
                        // Ouverture du champ : tout le chemin est sélectionné, la saisie le remplace.
                        response.request_focus();
                        select_all(ui, response.id, text);
                    }
                    if ui.input(|input| input.key_pressed(Key::Enter)) {
                        actions.push(Action::CommitPath);
                    } else if ui.input(|input| input.key_pressed(Key::Escape)) || response.lost_focus() {
                        self.path_edit = None;
                    }
                } else {
                    self.breadcrumb(ui, actions);
                }
            });
        });
    }

    fn breadcrumb(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let Some(dir) = self.current_dir() else {
            ui.label(RichText::new("Corbeille").family(style::strong_family()));
            return;
        };
        let home = home();
        // Le dossier personnel s'appelle « Accueil », comme dans la barre latérale.
        let (start, mut parts): (Option<PathBuf>, Vec<PathBuf>) = if dir.starts_with(&home) {
            (
                Some(home.clone()),
                dir.ancestors()
                    .take_while(|p| *p != home)
                    .map(Path::to_path_buf)
                    .collect(),
            )
        } else {
            (None, dir.ancestors().map(Path::to_path_buf).collect())
        };
        if let Some(home) = start {
            parts.push(home);
        }
        parts.reverse();
        let last = parts.len().saturating_sub(1);
        for (index, part) in parts.iter().enumerate() {
            let name = if *part == home {
                "Accueil".to_owned()
            } else {
                part.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Ordinateur".to_owned())
            };
            if index > 0 {
                ui.label(RichText::new("›").color(style::TEXT_SECONDARY));
            }
            let text = if index == last {
                RichText::new(name).family(style::strong_family())
            } else {
                RichText::new(name)
            };
            let response = ui.add(egui::Button::new(text).frame_when_inactive(false));
            if response.clicked() {
                if index == last {
                    actions.push(Action::EditPath);
                } else {
                    actions.push(Action::Navigate(part.clone()));
                }
            }
        }
        // Un clic dans la zone vide du chemin permet de le saisir.
        let rest = ui.allocate_response(ui.available_size(), Sense::click());
        if rest.clicked() {
            actions.push(Action::EditPath);
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let home = home();
        let mut favorites = vec![("Accueil", home.clone(), "places-home")];
        for (name, folder, icon) in [
            ("Bureau", "Bureau", "places-desktop"),
            ("Documents", "Documents", "places-documents"),
            ("Téléchargements", "Téléchargements", "places-downloads"),
            ("Images", "Images", "places-pictures"),
            ("Musique", "Musique", "places-music"),
            ("Vidéos", "Vidéos", "places-videos"),
        ] {
            let path = home.join(folder);
            if path.is_dir() {
                favorites.push((name, path, icon));
            }
        }
        let tint = IconStyle::Tinted(style::ACCENT);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            style::sidebar_heading(ui, "Favoris");
            for (name, path, icon) in favorites {
                let selected = self.current_dir() == Some(path.as_path());
                if style::sidebar_item(ui, &mut self.icons, icon, tint, name, selected).clicked() {
                    actions.push(Action::Navigate(path));
                }
            }
            let trash_selected = self.location == Location::Trash;
            let trash_icon = if trash::is_empty() {
                "places-trash"
            } else {
                "places-trash-full"
            };
            if self.picker.is_none()
                && style::sidebar_item(ui, &mut self.icons, trash_icon, tint, "Corbeille", trash_selected)
                    .clicked()
            {
                actions.push(Action::ShowTrash);
            }
            ui.add_space(8.0);
            style::sidebar_heading(ui, "Appareils");
            let root = PathBuf::from("/");
            let selected = self.current_dir() == Some(root.as_path());
            if style::sidebar_item(
                ui,
                &mut self.icons,
                "places-computer",
                tint,
                "Ordinateur",
                selected,
            )
            .clicked()
            {
                actions.push(Action::Navigate(root));
            }
            for device in self.devices.clone() {
                let selected = device
                    .mount
                    .as_deref()
                    .is_some_and(|mount| self.current_dir() == Some(mount));
                let response =
                    style::sidebar_item(ui, &mut self.icons, "places-usb", tint, &device.label, selected)
                        .on_hover_text(format!("{} · {}", device.size, device.path));
                // Bouton d'éjection à droite de la ligne.
                let eject = egui::Rect::from_center_size(
                    egui::pos2(response.rect.right() - 16.0, response.rect.center().y),
                    egui::vec2(22.0, 22.0),
                );
                let eject_response = ui
                    .interact(eject, response.id.with("ejecter"), Sense::click())
                    .on_hover_text("Éjecter");
                paint_glyph(
                    ui,
                    Glyph::Eject,
                    eject,
                    if eject_response.hovered() {
                        style::TEXT
                    } else {
                        style::TEXT_SECONDARY
                    },
                );
                if eject_response.clicked() {
                    actions.push(Action::Eject(device));
                } else if response.clicked() {
                    actions.push(match &device.mount {
                        Some(mount) => Action::Navigate(mount.clone()),
                        None => Action::Mount(device),
                    });
                }
            }
        });
    }

    /// Barre du bas du sélecteur : nom du fichier (enregistrement), Annuler, Ouvrir ou Enregistrer.
    fn picker_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let chosen_file = (self.selected.len() == 1)
            .then(|| self.selection().remove(0))
            .filter(|path| !path.is_dir());
        let message = self.message.clone();
        let Some(picker) = &mut self.picker else { return };
        egui::Panel::bottom(ui.scope_id().with("selecteur"))
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(style::HEADER)
                    .inner_margin(Margin::symmetric(12, 10)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    match picker.mode {
                        PickerMode::Save => {
                            ui.label("Enregistrer sous :");
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut picker.name)
                                    .desired_width((ui.available_width() - 240.0).clamp(120.0, 360.0)),
                            );
                            if !picker.name_focused {
                                // Nom sélectionné sans l'extension, prêt à être remplacé.
                                response.request_focus();
                                select_stem(ui, response.id, &picker.name);
                                picker.name_focused = true;
                            }
                            if response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                                actions.push(Action::ConfirmSave);
                            }
                        }
                        PickerMode::Open => {
                            let text = match &chosen_file {
                                Some(path) => file_name(path),
                                None => "Choisis un fichier".to_owned(),
                            };
                            ui.label(RichText::new(text).color(style::TEXT_SECONDARY));
                        }
                    }
                    if let Some((text, true)) = &message {
                        ui.colored_label(style::ERROR, text);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (label, enabled) = match picker.mode {
                            PickerMode::Save => ("Enregistrer", !picker.name.trim().is_empty()),
                            PickerMode::Open => ("Ouvrir", chosen_file.is_some()),
                        };
                        let button = egui::Button::new(RichText::new(label).color(Color32::WHITE))
                            .fill(style::ACCENT)
                            .min_size(egui::vec2(96.0, 0.0));
                        if ui.add_enabled(enabled, button).clicked() {
                            actions.push(match (&chosen_file, picker.mode) {
                                (Some(path), PickerMode::Open) => Action::Choose(path.clone()),
                                _ => Action::ConfirmSave,
                            });
                        }
                        if ui
                            .add(egui::Button::new("Annuler").min_size(egui::vec2(80.0, 0.0)))
                            .clicked()
                        {
                            actions.push(Action::CancelPicker);
                        }
                    });
                });
            });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let visible = self.visible().len();
        let selected = self.selected.len();
        let selected_size: u64 = self
            .entries
            .iter()
            .filter(|entry| self.selected.contains(&entry.path))
            .filter_map(|entry| entry.size)
            .sum();
        let jobs: Vec<(String, ops::Progress)> = self
            .jobs
            .iter()
            .map(|job| (job.title.clone(), job.snapshot()))
            .collect();
        let message = self.message.clone();
        let free = self.free_space;
        let in_trash = self.location == Location::Trash;
        style::status_bar(ui, |ui| {
            if let Some((index, (title, progress))) = jobs.iter().enumerate().next() {
                let fraction = if progress.total_bytes > 0 {
                    progress.done_bytes as f32 / progress.total_bytes as f32
                } else {
                    0.0
                };
                ui.label(format!("{title} : {}", progress.current));
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .desired_width(160.0)
                        .desired_height(6.0),
                );
                if ui.small_button("Annuler").clicked() {
                    actions.push(Action::CancelJob(index));
                }
                return;
            }
            let summary = if selected > 0 {
                let size = if selected_size > 0 {
                    format!(" ({})", format_size(selected_size))
                } else {
                    String::new()
                };
                format!("{selected} sur {visible} sélectionné{}{size}", plural(selected))
            } else {
                match visible {
                    0 => "Dossier vide".to_owned(),
                    1 => "1 élément".to_owned(),
                    count => format!("{count} éléments"),
                }
            };
            ui.label(summary);
            if let Some((text, error)) = &message {
                ui.label("·");
                if *error {
                    ui.colored_label(style::ERROR, text);
                } else {
                    ui.label(text);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if in_trash {
                    if ui.small_button("Vider la Corbeille").clicked() {
                        actions.push(Action::AskEmptyTrash);
                    }
                } else if let Some(free) = free {
                    ui.label(format!("{} disponibles", format_size(free)));
                }
            });
        });
    }

    /// Menu contextuel d'un élément.
    fn item_menu(&self, ui: &mut egui::Ui, entry: &FileEntry, actions: &mut Vec<Action>) {
        let single = self.selected.len() <= 1;
        let mut item = |ui: &mut egui::Ui, text: &str, enabled: bool, action: Action| {
            if ui
                .add_enabled(enabled, egui::Button::new(text).frame_when_inactive(false))
                .clicked()
            {
                actions.push(action);
                ui.close();
            }
        };
        if self.location == Location::Trash {
            item(ui, "Restaurer", true, Action::Restore);
            item(ui, "Supprimer définitivement…", true, Action::AskDeleteForever);
            ui.separator();
            item(ui, "Propriétés", single, Action::Properties(entry.path.clone()));
            return;
        }
        item(
            ui,
            "Ouvrir",
            single,
            Action::Open(entry.path.clone(), entry.directory),
        );
        if !entry.directory {
            item(ui, "Ouvrir avec…", single, Action::OpenWith(entry.path.clone()));
        }
        ui.separator();
        item(ui, "Couper", true, Action::Cut);
        item(ui, "Copier", true, Action::Copy);
        item(ui, "Dupliquer", true, Action::Duplicate);
        item(ui, "Renommer…", single, Action::StartRename(entry.path.clone()));
        ui.separator();
        item(ui, "Mettre à la Corbeille", true, Action::Trash);
        ui.separator();
        item(ui, "Propriétés", single, Action::Properties(entry.path.clone()));
    }

    /// Menu contextuel de la zone vide du dossier.
    fn folder_menu(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let mut item = |ui: &mut egui::Ui, text: &str, enabled: bool, action: Action| {
            if ui
                .add_enabled(enabled, egui::Button::new(text).frame_when_inactive(false))
                .clicked()
            {
                actions.push(action);
                ui.close();
            }
        };
        if self.location == Location::Trash {
            item(
                ui,
                "Vider la Corbeille…",
                !trash::is_empty(),
                Action::AskEmptyTrash,
            );
            return;
        }
        item(ui, "Nouveau dossier", true, Action::NewFolder);
        item(ui, "Coller", self.clipboard.is_some(), Action::Paste);
        ui.separator();
        item(ui, "Ouvrir un terminal ici", true, Action::TerminalHere);
        let hidden = if self.show_hidden {
            "Masquer les fichiers cachés"
        } else {
            "Afficher les fichiers cachés"
        };
        item(ui, hidden, true, Action::ToggleHidden);
        if let Some(dir) = self.current_dir() {
            ui.separator();
            item(
                ui,
                "Propriétés du dossier",
                true,
                Action::Properties(dir.to_path_buf()),
            );
        }
    }

    /// Clic sur un élément : sélection, double-clic : ouverture, clic droit : menu.
    fn item_response(
        &self,
        response: &egui::Response,
        index: usize,
        ui: &egui::Ui,
        actions: &mut Vec<Action>,
    ) {
        let entry = &self.entries[index];
        if response.double_clicked() {
            actions.push(Action::Open(entry.path.clone(), entry.directory));
        } else if response.clicked() {
            let modifiers = ui.input(|input| input.modifiers);
            actions.push(Action::Select {
                index,
                ctrl: modifiers.command,
                shift: modifiers.shift,
            });
        } else if response.secondary_clicked() {
            actions.push(Action::SelectOnly(index));
        }
    }

    fn rename_field(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        width: f32,
        actions: &mut Vec<Action>,
    ) -> bool {
        let Some((target, text, focused)) = &mut self.rename else {
            return false;
        };
        if target != path {
            return false;
        }
        let response = ui.add(egui::TextEdit::singleline(text).desired_width(width));
        if !*focused {
            response.request_focus();
            // Sélectionne le nom sans l'extension, comme le Finder.
            let stem = text.rfind('.').filter(|&i| i > 0).unwrap_or(text.len());
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                let range = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(text[..stem].chars().count()),
                );
                state.cursor.set_char_range(Some(range));
                state.store(ui.ctx(), response.id);
            }
            *focused = true;
        }
        if ui.input(|input| input.key_pressed(Key::Enter)) {
            actions.push(Action::CommitRename);
        } else if ui.input(|input| input.key_pressed(Key::Escape)) {
            actions.push(Action::CancelRename);
        } else if response.lost_focus() {
            actions.push(Action::CommitRename);
        }
        true
    }

    fn list(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let width = ui.available_width();
        let in_trash = self.location == Location::Trash;
        let third = if in_trash { "Supprimé le" } else { "Modifié" };
        let columns = [width - 300.0, 110.0, 160.0];
        // En-têtes cliquables : tri.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(46.0);
            for (index, (title, key)) in [
                ("Nom", SortKey::Name),
                ("Taille", SortKey::Size),
                (third, SortKey::Modified),
            ]
            .into_iter()
            .enumerate()
            {
                let arrow = match (self.sort == key, self.ascending) {
                    (true, true) => " ▲",
                    (true, false) => " ▼",
                    _ => "",
                };
                let width = columns[index] - if index == 0 { 46.0 } else { 0.0 };
                let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::click());
                let color = if self.sort == key {
                    style::TEXT
                } else {
                    style::TEXT_SECONDARY
                };
                ui.painter().text(
                    egui::pos2(rect.left(), rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("{title}{arrow}"),
                    egui::FontId::proportional(11.5),
                    color,
                );
                if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    actions.push(Action::Sort(key));
                }
            }
        });
        let line = ui.cursor().top();
        ui.painter()
            .hline(ui.max_rect().x_range(), line, Stroke::new(1.0, style::SEPARATOR));

        let visible = self.visible();
        let scroll = egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (row_index, &index) in visible.iter().enumerate() {
                let entry = self.entries[index].clone();
                let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ROW), Sense::click());
                let selected = self.selected.contains(&entry.path);
                let fill = if selected {
                    Some(style::ACCENT)
                } else if response.hovered() {
                    Some(Color32::from_rgb(0xF0, 0xF3, 0xF8))
                } else if row_index % 2 == 1 {
                    Some(Color32::from_rgb(0xFA, 0xFA, 0xFB))
                } else {
                    None
                };
                if let Some(fill) = fill {
                    ui.painter()
                        .rect_filled(rect.shrink2(egui::vec2(6.0, 1.0)), 6.0, fill);
                }
                let ink = if selected { Color32::WHITE } else { style::TEXT };
                let secondary = if selected {
                    Color32::from_white_alpha(220)
                } else {
                    style::TEXT_SECONDARY
                };
                let icon = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 28.0, rect.center().y),
                    egui::vec2(22.0, 22.0),
                );
                self.icons.paint(ui, entry_icon(&entry), icon, IconStyle::Color);
                let name_left = rect.left() + 46.0;
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(egui::Rect::from_min_size(
                            egui::pos2(name_left, rect.top() + 2.0),
                            egui::vec2(columns[0] - 52.0, ROW - 4.0),
                        ))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                if !self.rename_field(&mut child, &entry.path, columns[0] - 60.0, actions) {
                    let name = if entry.symlink {
                        format!("{} (lien)", entry.name)
                    } else {
                        entry.name.clone()
                    };
                    paint_text(
                        ui,
                        &name,
                        egui::pos2(name_left, rect.center().y),
                        columns[0] - 56.0,
                        ink,
                        false,
                    );
                }
                let size = match (entry.directory, entry.size) {
                    (true, _) => "—".to_owned(),
                    (false, Some(size)) => format_size(size),
                    (false, None) => String::new(),
                };
                let x = rect.left() + columns[0];
                paint_text(
                    ui,
                    &size,
                    egui::pos2(x, rect.center().y),
                    columns[1] - 8.0,
                    secondary,
                    false,
                );
                let date = match &entry.trash {
                    Some(item) => item.deleted.replace('T', " à ").chars().take(18).collect(),
                    None => format_time(entry.modified),
                };
                paint_text(
                    ui,
                    &date,
                    egui::pos2(x + columns[1], rect.center().y),
                    columns[2] - 8.0,
                    secondary,
                    false,
                );
                let response = response.on_hover_text_at_pointer(match &entry.trash {
                    Some(item) => format!("Origine : {}", item.original.display()),
                    None => entry.name.clone(),
                });
                self.item_response(&response, index, ui, actions);
                response.context_menu(|ui| self.item_menu(ui, &entry, actions));
            }
            // Zone vide sous la liste : désélection et menu du dossier.
            let rest = ui.allocate_response(ui.available_size().max(egui::vec2(width, 80.0)), Sense::click());
            if rest.clicked() {
                actions.push(Action::ClearSelection);
            }
            rest.context_menu(|ui| self.folder_menu(ui, actions));
        });
        let _ = scroll;
        if visible.is_empty() {
            empty_state(ui, &self.search, self.location == Location::Trash);
        }
    }

    fn grid(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let visible = self.visible();
        let width = ui.available_width();
        let columns = ((width - 16.0) / GRID_CELL.x).floor().max(1.0) as usize;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.add_space(10.0);
            for row in visible.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for &index in row {
                        let entry = self.entries[index].clone();
                        let (rect, response) = ui.allocate_exact_size(GRID_CELL, Sense::click());
                        let selected = self.selected.contains(&entry.path);
                        let icon_rect = egui::Rect::from_center_size(
                            egui::pos2(rect.center().x, rect.top() + 36.0),
                            egui::vec2(56.0, 56.0),
                        );
                        if selected || response.hovered() {
                            let fill = if selected {
                                Color32::from_black_alpha(18)
                            } else {
                                Color32::from_black_alpha(8)
                            };
                            ui.painter().rect_filled(icon_rect.expand(6.0), 10.0, fill);
                        }
                        self.icons
                            .paint(ui, entry_icon(&entry), icon_rect, IconStyle::Color);
                        let label_center = egui::pos2(rect.center().x, rect.top() + 84.0);
                        let mut child = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(egui::Rect::from_center_size(
                                    label_center,
                                    egui::vec2(GRID_CELL.x - 8.0, 24.0),
                                ))
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        if !self.rename_field(&mut child, &entry.path, GRID_CELL.x - 12.0, actions) {
                            let galley_width = paint_text(
                                ui,
                                &entry.name,
                                label_center,
                                GRID_CELL.x - 10.0,
                                style::TEXT,
                                true,
                            );
                            if selected {
                                let pill = egui::Rect::from_center_size(
                                    label_center,
                                    egui::vec2(galley_width + 12.0, 20.0),
                                );
                                ui.painter().rect_filled(pill, 6.0, style::ACCENT);
                                paint_text(
                                    ui,
                                    &entry.name,
                                    label_center,
                                    GRID_CELL.x - 10.0,
                                    Color32::WHITE,
                                    true,
                                );
                            }
                        }
                        let response = response.on_hover_text_at_pointer(&entry.name);
                        self.item_response(&response, index, ui, actions);
                        response.context_menu(|ui| self.item_menu(ui, &entry, actions));
                    }
                });
            }
            let rest = ui.allocate_response(ui.available_size().max(egui::vec2(width, 80.0)), Sense::click());
            if rest.clicked() {
                actions.push(Action::ClearSelection);
            }
            rest.context_menu(|ui| self.folder_menu(ui, actions));
        });
        if visible.is_empty() {
            empty_state(ui, &self.search, self.location == Location::Trash);
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(dialog) = &self.dialog else { return };
        let modal = egui::Modal::new(egui::Id::unique("fichiers-dialogue")).show(ctx, |ui| {
            ui.set_width(380.0);
            match dialog {
                Dialog::DeleteForever(paths) => {
                    ui.heading(if paths.len() == 1 {
                        "Supprimer définitivement cet élément ?".to_owned()
                    } else {
                        format!("Supprimer définitivement {} éléments ?", paths.len())
                    });
                    ui.label(RichText::new("Cette action est irréversible.").color(style::TEXT_SECONDARY));
                    ui.add_space(12.0);
                    match dialog_buttons(ui, "Supprimer") {
                        Some(true) => actions.push(Action::ConfirmDeleteForever(paths.clone())),
                        Some(false) => actions.push(Action::CloseDialog),
                        None => {}
                    }
                }
                Dialog::EmptyTrash => {
                    ui.heading("Vider la Corbeille ?");
                    ui.label(
                        RichText::new("Tous ses éléments seront supprimés définitivement.")
                            .color(style::TEXT_SECONDARY),
                    );
                    ui.add_space(12.0);
                    match dialog_buttons(ui, "Vider") {
                        Some(true) => actions.push(Action::ConfirmEmptyTrash),
                        Some(false) => actions.push(Action::CloseDialog),
                        None => {}
                    }
                }
                Dialog::Properties(properties) => {
                    let name = properties
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| properties.path.display().to_string());
                    ui.heading(name);
                    ui.add_space(8.0);
                    egui::Grid::new("proprietes")
                        .num_columns(2)
                        .spacing([16.0, 8.0])
                        .show(ui, |ui| {
                            let mut row = |title: &str, value: String| {
                                ui.label(RichText::new(title).color(style::TEXT_SECONDARY));
                                ui.label(value);
                                ui.end_row();
                            };
                            row("Type", properties.kind.clone());
                            row(
                                "Taille",
                                match properties.size {
                                    Some((bytes, files)) if properties.path.is_dir() => {
                                        format!(
                                            "{} · {files} fichier{}",
                                            format_size(bytes),
                                            plural(files as usize)
                                        )
                                    }
                                    Some((bytes, _)) => format_size(bytes),
                                    None => "Calcul en cours…".to_owned(),
                                },
                            );
                            row(
                                "Emplacement",
                                properties
                                    .path
                                    .parent()
                                    .map(|parent| parent.display().to_string())
                                    .unwrap_or_default(),
                            );
                            row("Modifié", format_date(properties.modified));
                            row("Droits", properties.permissions.clone());
                        });
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if style::suggested_button(ui, "Fermer").clicked() {
                            actions.push(Action::CloseDialog);
                        }
                    });
                }
                Dialog::Replace(path) => {
                    ui.heading(format!("Remplacer « {} » ?", file_name(path)));
                    ui.label(
                        RichText::new("Un fichier de ce nom existe déjà ; son contenu sera remplacé.")
                            .color(style::TEXT_SECONDARY),
                    );
                    ui.add_space(12.0);
                    match dialog_buttons(ui, "Remplacer") {
                        Some(true) => actions.push(Action::Choose(path.clone())),
                        Some(false) => actions.push(Action::CloseDialog),
                        None => {}
                    }
                }
                Dialog::OpenWith(path, apps) => {
                    ui.heading("Ouvrir avec");
                    ui.label(
                        RichText::new(
                            path.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        )
                        .color(style::TEXT_SECONDARY),
                    );
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        if ui
                            .add(egui::Button::new("TextEdit").frame_when_inactive(false))
                            .clicked()
                        {
                            let app = DesktopEntry {
                                id: "textedit".into(),
                                name: "TextEdit".into(),
                                exec: vec![
                                    env::current_exe()
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_default(),
                                    "textedit".into(),
                                ],
                                terminal: false,
                                icon: None,
                                wm_class: None,
                            };
                            actions.push(Action::LaunchWith(path.clone(), app));
                        }
                        for app in apps {
                            if ui
                                .add(egui::Button::new(&app.name).frame_when_inactive(false))
                                .clicked()
                            {
                                actions.push(Action::LaunchWith(path.clone(), app.clone()));
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Annuler").clicked() {
                            actions.push(Action::CloseDialog);
                        }
                    });
                }
            }
        });
        if modal.should_close() {
            actions.push(Action::CloseDialog);
        }
    }
}

/// Boutons d'une confirmation destructrice : `Some(true)` pour confirmer, `Some(false)` pour annuler.
fn dialog_buttons(ui: &mut egui::Ui, confirm: &str) -> Option<bool> {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let button = egui::Button::new(RichText::new(confirm).color(Color32::WHITE)).fill(style::ERROR);
        if ui.add(button).clicked() {
            Some(true)
        } else if ui.button("Annuler").clicked() {
            Some(false)
        } else {
            None
        }
    })
    .inner
}

fn empty_state(ui: &mut egui::Ui, search: &str, trash: bool) {
    let text = if !search.trim().is_empty() {
        "Aucun résultat dans ce dossier"
    } else if trash {
        "La Corbeille est vide"
    } else {
        "Ce dossier est vide"
    };
    let center = ui.max_rect().center();
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(15.0),
        style::TEXT_SECONDARY,
    );
}

/// Écrit `text` sur une ligne, coupé avec « … » au-delà de `max_width` ; renvoie sa largeur.
fn paint_text(
    ui: &egui::Ui,
    text: &str,
    anchor: egui::Pos2,
    max_width: f32,
    color: Color32,
    centered: bool,
) -> f32 {
    let font = egui::FontId::proportional(13.0);
    let mut shown = text.to_owned();
    let mut galley = ui.painter().layout_no_wrap(shown.clone(), font.clone(), color);
    while galley.size().x > max_width && shown.chars().count() > 1 {
        shown.pop();
        galley = ui
            .painter()
            .layout_no_wrap(format!("{}…", shown.trim_end()), font.clone(), color);
    }
    let width = galley.size().x;
    let pos = if centered {
        egui::pos2(anchor.x - width / 2.0, anchor.y - galley.size().y / 2.0)
    } else {
        egui::pos2(anchor.x, anchor.y - galley.size().y / 2.0)
    };
    ui.painter().galley(pos, galley, color);
    width
}

#[derive(Clone, Copy)]
enum Glyph {
    Back,
    Forward,
    List,
    Grid,
    Eject,
}

/// Petits pictogrammes dessinés au trait, nets à toute échelle.
fn paint_glyph(ui: &egui::Ui, glyph: Glyph, rect: egui::Rect, color: Color32) {
    let painter = ui.painter();
    let c = rect.center();
    let stroke = Stroke::new(1.8, color);
    let p = |x: f32, y: f32| egui::pos2(c.x + x, c.y + y);
    match glyph {
        Glyph::Back => {
            painter.line(vec![p(3.0, -6.0), p(-3.0, 0.0), p(3.0, 6.0)], stroke);
        }
        Glyph::Forward => {
            painter.line(vec![p(-3.0, -6.0), p(3.0, 0.0), p(-3.0, 6.0)], stroke);
        }
        Glyph::List => {
            for y in [-5.0, 0.0, 5.0] {
                painter.line_segment([p(-7.0, y), p(7.0, y)], stroke);
            }
        }
        Glyph::Grid => {
            for (x, y) in [(-4.0, -4.0), (4.0, -4.0), (-4.0, 4.0), (4.0, 4.0)] {
                painter.rect_stroke(
                    egui::Rect::from_center_size(p(x, y), egui::vec2(5.5, 5.5)),
                    CornerRadius::same(1),
                    Stroke::new(1.5, color),
                    egui::StrokeKind::Inside,
                );
            }
        }
        Glyph::Eject => {
            painter.add(egui::Shape::convex_polygon(
                vec![p(0.0, -6.0), p(6.0, 1.0), p(-6.0, 1.0)],
                color,
                Stroke::NONE,
            ));
            painter.line_segment([p(-6.0, 5.0), p(6.0, 5.0)], Stroke::new(2.0, color));
        }
    }
}

fn nav_button(ui: &mut egui::Ui, glyph: Glyph, enabled: bool, hint: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(32.0, 28.0),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 7.0, Color32::from_black_alpha(14));
    }
    let color = if enabled {
        style::TEXT
    } else {
        Color32::from_rgb(0xBB, 0xBB, 0xC0)
    };
    paint_glyph(ui, glyph, rect, color);
    response.on_hover_text(hint)
}

/// Bouton de vue (liste, grille) : la vue active reste enfoncée.
fn view_button(ui: &mut egui::Ui, glyph: Glyph, active: bool, hint: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 28.0), Sense::click());
    if active {
        ui.painter().rect_filled(rect, 7.0, Color32::from_black_alpha(22));
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 7.0, Color32::from_black_alpha(10));
    }
    paint_glyph(ui, glyph, rect, style::TEXT);
    response.on_hover_text(hint)
}

fn properties(path: &Path) -> Properties {
    let metadata = path.symlink_metadata().ok();
    let directory = metadata.as_ref().is_some_and(|m| m.is_dir());
    let kind = match &metadata {
        Some(m) if m.file_type().is_symlink() => format!(
            "Lien vers {}",
            fs::read_link(path)
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        ),
        Some(_) if directory => "Dossier".to_owned(),
        Some(_) => match path.extension().and_then(|e| e.to_str()) {
            Some(extension) => format!("Fichier {}", extension.to_uppercase()),
            None => "Fichier".to_owned(),
        },
        None => "Introuvable".to_owned(),
    };
    Properties {
        path: path.to_path_buf(),
        kind,
        size: if directory {
            None
        } else {
            metadata.as_ref().map(|m| (m.len(), 1))
        },
        modified: metadata.as_ref().and_then(|m| m.modified().ok()),
        permissions: metadata
            .map(|m| permissions(m.permissions().mode()))
            .unwrap_or_default(),
    }
}

/// « rwxr-xr-x » et sa lecture en français.
fn permissions(mode: u32) -> String {
    let mut text = String::new();
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        text.push(if bits & 4 != 0 { 'r' } else { '-' });
        text.push(if bits & 2 != 0 { 'w' } else { '-' });
        text.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    let owner = match (mode >> 6) & 0o7 {
        7 | 6 => "lecture et écriture",
        5 | 4 => "lecture seule",
        _ => "aucun accès",
    };
    format!("{text} (vous : {owner})")
}

/// Comparaison « naturelle » : « image2 » avant « image10 », sans tenir compte de la casse.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut number = String::new();
                    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
                        number.push(c);
                        chars.next();
                    }
                    number
                };
                let (na, nb) = (take(&mut a), take(&mut b));
                let order = na
                    .trim_start_matches('0')
                    .len()
                    .cmp(&nb.trim_start_matches('0').len())
                    .then_with(|| na.trim_start_matches('0').cmp(nb.trim_start_matches('0')));
                if order != std::cmp::Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != std::cmp::Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Icône du dossier `icons/SVG` selon la nature de l'entrée.
fn entry_icon(entry: &FileEntry) -> &'static str {
    if entry.directory {
        return "app-files";
    }
    let extension = entry
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => "app-photos",
        "mp3" | "ogg" | "flac" | "wav" | "m4a" | "opus" => "app-music",
        "mp4" | "mkv" | "webm" | "avi" | "mov" => "app-video",
        "rs" | "c" | "h" | "py" | "js" | "ts" | "toml" | "json" | "yaml" | "yml" | "html" | "css" => {
            "app-code"
        }
        "sh" => "app-terminal",
        "deb" => "app-store",
        _ if is_text_file(&entry.path) => "file-text",
        _ => "file-generic",
    }
}

pub fn is_text_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "txt"
                    | "md"
                    | "rs"
                    | "toml"
                    | "json"
                    | "yaml"
                    | "yml"
                    | "sh"
                    | "conf"
                    | "log"
                    | "ini"
                    | "csv"
            )
        })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn select_all(ui: &egui::Ui, id: egui::Id, text: &str) {
    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
    let range = egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(text.chars().count()),
    );
    state.cursor.set_char_range(Some(range));
    state.store(ui.ctx(), id);
}

/// Sélectionne le nom sans l'extension dans le champ `id`, comme le Finder.
fn select_stem(ui: &egui::Ui, id: egui::Id, text: &str) {
    let stem = text.rfind('.').filter(|&i| i > 0).unwrap_or(text.len());
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
        let range = egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(text[..stem].chars().count()),
        );
        state.cursor.set_char_range(Some(range));
        state.store(ui.ctx(), id);
    }
}

fn plural(count: usize) -> &'static str {
    if count > 1 { "s" } else { "" }
}

fn format_size(size: u64) -> String {
    const UNITS: [&str; 5] = ["o", "Ko", "Mo", "Go", "To"];
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{size} o")
    } else {
        format!("{value:.1} {}", UNITS[unit]).replace('.', ",")
    }
}

fn format_time(time: Option<SystemTime>) -> String {
    let Some(age) = time.and_then(|time| SystemTime::now().duration_since(time).ok()) else {
        return String::new();
    };
    match age.as_secs() {
        0..60 => "À l'instant".into(),
        60..3600 => format!("Il y a {} min", age.as_secs() / 60),
        3600..86_400 => format!("Il y a {} h", age.as_secs() / 3600),
        86_400..172_800 => "Hier".into(),
        seconds => format!("Il y a {} jours", seconds / 86_400),
    }
}

fn format_date(time: Option<SystemTime>) -> String {
    let relative = format_time(time);
    if relative.is_empty() {
        "Inconnu".into()
    } else {
        relative
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailles_lisibles() {
        assert_eq!(format_size(512), "512 o");
        assert_eq!(format_size(1_500), "1,5 Ko");
        assert_eq!(format_size(4_200_000_000), "4,2 Go");
    }

    #[test]
    fn tri_naturel() {
        let mut names = vec!["image10.png", "Image2.png", "image1.png", "Notes"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["image1.png", "Image2.png", "image10.png", "Notes"]);
    }

    #[test]
    fn droits_lisibles() {
        assert_eq!(permissions(0o644), "rw-r--r-- (vous : lecture et écriture)");
        assert_eq!(permissions(0o555), "r-xr-xr-x (vous : lecture seule)");
    }

    #[test]
    fn historique_precedent_suivant() {
        let mut files = FilesState::default();
        let start = files.location.clone();
        files.go(Location::Folder(PathBuf::from("/")));
        assert_eq!(files.location, Location::Folder(PathBuf::from("/")));
        files.go_back();
        assert_eq!(files.location, start);
        files.go_forward();
        assert_eq!(files.location, Location::Folder(PathBuf::from("/")));
    }

    #[test]
    fn selection_avec_maj_et_ctrl() {
        let dir = std::env::temp_dir().join(format!("ferrite-selection-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for name in ["a", "b", "c", "d"] {
            fs::write(dir.join(name), name).unwrap();
        }
        let mut files = FilesState::default();
        files.go(Location::Folder(dir.clone()));
        let ctx = egui::Context::default();
        files.apply(
            Action::Select {
                index: 0,
                ctrl: false,
                shift: false,
            },
            &ctx,
        );
        files.apply(
            Action::Select {
                index: 2,
                ctrl: false,
                shift: true,
            },
            &ctx,
        );
        assert_eq!(files.selected.len(), 3);
        files.apply(
            Action::Select {
                index: 1,
                ctrl: true,
                shift: false,
            },
            &ctx,
        );
        assert_eq!(files.selected.len(), 2);
        files.apply(Action::SelectAll, &ctx);
        assert_eq!(files.selected.len(), 4);
        fs::remove_dir_all(dir).unwrap();
    }
}
