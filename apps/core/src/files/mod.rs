//! Fichiers : gestionnaire de fichiers de Ferrite.
//!
//! - affichage en liste (colonnes triables) ou en grille d'icônes, fichiers cachés à la
//!   demande, recherche dans le dossier ;
//! - navigation : précédent, suivant, dossier parent, chemin cliquable ou saisi (Ctrl+L),
//!   barre latérale (Favoris, Corbeille, clés USB) ;
//! - sélection multiple (Ctrl, Maj, Ctrl+A), menu contextuel, raccourcis habituels ;
//! - ouvrir les archives comme des dossiers virtuels sans les extraire, compresser et extraire
//!   des archives (en arrière-plan), renommer (F2), nouveau dossier, Corbeille (restaurer, vider),
//!   suppression définitive confirmée ;
//! - ouvrir, ouvrir avec…, propriétés, ouvrir un terminal ici ;
//! - montage et éjection des clés USB ;
//! - mode sélecteur (`FilesState::picker`) : les autres applications l'affichent pour
//!   choisir le fichier à ouvrir ou l'emplacement d'enregistrement ;
//! - fichiers récents, étiquettes, panneau d'aperçu et aperçu rapide (Espace), vignettes ;
//! - glisser-déposer vers un dossier, la barre latérale, le chemin ou la Corbeille, et
//!   hors de la fenêtre vers le Dock ou une autre application (voir `view`).
//!
//! L'interface ne modifie jamais l'état pendant son propre dessin : chaque clic produit une
//! `Action`, appliquée une fois l'image construite.

mod devices;
mod look;
mod ops;
mod places;
mod thumbs;
pub(crate) mod trash;
mod view;

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

#[derive(Debug, Clone)]
struct FileEntry {
    name: String,
    path: PathBuf,
    directory: bool,
    symlink: bool,
    modified: Option<SystemTime>,
    size: Option<u64>,
    /// Chemin interne d'un élément affiché dans une archive.
    archive_member: Option<String>,
    /// Élément de la Corbeille : emplacement d'origine et date de suppression.
    trash: Option<TrashItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Location {
    Folder(PathBuf),
    /// Dossier virtuel à l'intérieur d'une archive, sans extraction globale.
    Archive {
        archive: PathBuf,
        directory: String,
    },
    Trash,
    /// Fichiers ouverts récemment (toutes applications).
    Recent,
    /// Fichiers qui portent cette étiquette.
    Tag(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Name,
    Size,
    Modified,
    Kind,
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
    /// Droits (`0o644`…), propriétaire et groupe.
    mode: u32,
    owner: String,
    group: String,
    /// Place occupée sur le disque, en octets.
    on_disk: Option<u64>,
    icon: &'static str,
    opener: &'static str,
    /// Nom en cours de saisie dans le dialogue.
    name: String,
}

/// Espace de disques et contenu des Téléchargements, relus en arrière-plan.
struct Disks {
    devices: Vec<Device>,
    /// Système de fichiers du dossier affiché, et celui du dossier personnel.
    folder: Option<(u64, u64)>,
    system: Option<(u64, u64)>,
    downloads: usize,
}

/// Résultat d'un calcul en arrière-plan.
enum Update {
    Devices(Disks),
    /// Montage terminé (le mot de passe administrateur a pu être demandé entre-temps).
    Mounted(Result<PathBuf, String>),
    FolderSize(PathBuf, (u64, u64)),
}

/// Ce que l'utilisateur a demandé pendant le dessin.
#[derive(Debug, Clone)]
enum Action {
    Navigate(PathBuf),
    Show(Location),
    ShowTrash,
    ToggleTheme,
    TogglePreview,
    Share(Vec<PathBuf>),
    ToggleTag(PathBuf, String),
    QuickLook,
    StartDrag(usize),
    Drop {
        target: view::Target,
        paths: Vec<PathBuf>,
        copy: bool,
    },
    Back,
    Forward,
    Up,
    Open(PathBuf, bool),
    Select {
        index: usize,
        ctrl: bool,
        shift: bool,
    },
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
    Compress(ops::ArchiveFormat),
    Extract(PathBuf),
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
    /// Espace du disque du dossier affiché et du disque système.
    usage: Option<(u64, u64)>,
    system_usage: Option<(u64, u64)>,
    /// Éléments du dossier Téléchargements (compteur de la barre latérale).
    downloads: usize,
    /// Panneau d'aperçu affiché, aperçu rapide ouvert, glisser en cours.
    preview: bool,
    quick: Option<PathBuf>,
    drag: Option<view::Dragging>,
    /// Fichiers lâchés sur la fenêtre par ferrite-shell (voir `view::Deposits`).
    deposits: Option<view::Deposits>,
    /// Sélection au rectangle en cours, et place des éléments relevée au dessin.
    lasso: Option<view::Lasso>,
    item_rects: Vec<(egui::Rect, PathBuf)>,
    tags: places::Tags,
    thumbs: thumbs::Thumbnails,
    devices_checked: Option<Instant>,
    devices_pending: bool,
    sender: Sender<Update>,
    receiver: Receiver<Update>,
    message: Option<(String, bool)>,
    /// Date de modification du dossier lors de sa dernière lecture.
    loaded: Option<SystemTime>,
    /// Le dossier affiché est réservé à l'administrateur (lecture refusée).
    denied: bool,
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
            denied: false,
            view: View::Grid,
            sort: SortKey::Name,
            ascending: true,
            search: String::new(),
            path_edit: None,
            rename: None,
            clipboard: None,
            dialog: None,
            jobs: Vec::new(),
            devices: Vec::new(),
            usage: None,
            system_usage: None,
            downloads: 0,
            preview: true,
            quick: None,
            drag: None,
            deposits: None,
            lasso: None,
            item_rects: Vec::new(),
            tags: places::Tags::load(),
            thumbs: thumbs::Thumbnails::default(),
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

    /// Ouvre `path` : un dossier ou une archive s'affiche, un fichier est montré dans son dossier.
    pub fn open_at(&mut self, path: PathBuf) {
        // Adresse donnée par la corbeille du Dock.
        if path.as_os_str() == "corbeille:" {
            self.location = Location::Trash;
            self.refresh();
            return;
        }
        let path = fs::canonicalize(&path).unwrap_or(path);
        if path.is_dir() {
            self.location = Location::Folder(path);
            self.refresh();
        } else if ops::is_archive(&path) {
            self.location = Location::Archive {
                archive: path,
                directory: String::new(),
            };
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
                Some(("tri", "type")) => self.sort = SortKey::Kind,
                Some(("croissant", value)) => self.ascending = value == "oui",
                Some(("caches", value)) => self.show_hidden = value == "oui",
                Some(("apercu", value)) => self.preview = value == "oui",
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
apercu={}
",
            match self.view {
                View::List => "liste",
                View::Grid => "grille",
            },
            match self.sort {
                SortKey::Name => "nom",
                SortKey::Size => "taille",
                SortKey::Modified => "date",
                SortKey::Kind => "type",
            },
            yes_no(self.ascending),
            yes_no(self.show_hidden),
            yes_no(self.preview),
        );
        let path = preferences_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, text);
    }

    /// Rouvre le dossier affiché dans une fenêtre Fichiers administrateur (`pkexec`, qui
    /// demande le mot de passe par l'agent polkit du bureau).
    fn open_as_admin(&mut self) {
        let Some(dir) = self.current_dir().map(Path::to_path_buf) else {
            return;
        };
        let result = env::current_exe().and_then(|exe| {
            let mut command = Command::new("pkexec");
            command.arg("env");
            // pkexec vide l'environnement : la fenêtre doit retrouver l'écran.
            for key in ["WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "XDG_SESSION_TYPE", "LANG"] {
                if let Some(value) = env::var_os(key) {
                    let mut pair = std::ffi::OsString::from(format!("{key}="));
                    pair.push(value);
                    command.arg(pair);
                }
            }
            command.arg(exe).arg("files").arg(&dir);
            spawn_detached(command)
        });
        match result {
            Ok(_) => self.info("Mot de passe administrateur demandé…"),
            Err(error) => self.error(format!("Ouverture en administrateur impossible : {error}")),
        }
    }

    fn current_dir(&self) -> Option<&Path> {
        match &self.location {
            Location::Folder(path) => Some(path),
            Location::Archive { .. } | Location::Trash | Location::Recent | Location::Tag(_) => None,
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
        if let Location::Archive { archive, .. } = &location
            && !archive.is_file()
        {
            self.error(format!("Archive introuvable : {}", archive.display()));
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
                            archive_member: None,
                            trash: Some(item),
                        }
                    })
                    .collect()
            }
            Location::Recent => {
                self.loaded = None;
                places::recent()
                    .into_iter()
                    .filter_map(|path| entry_for(&path))
                    .collect()
            }
            Location::Tag(tag) => {
                self.loaded = None;
                self.tags
                    .tagged(tag)
                    .into_iter()
                    .filter_map(|path| entry_for(&path))
                    .collect()
            }
            Location::Archive { archive, directory } => {
                self.loaded = modified(archive);
                self.denied = false;
                match ops::list_archive(archive, directory) {
                    Ok(entries) => entries
                        .into_iter()
                        .map(|entry| FileEntry {
                            name: entry.name,
                            path: archive_virtual_path(archive, &entry.member),
                            directory: entry.directory,
                            symlink: false,
                            modified: None,
                            size: None,
                            archive_member: Some(entry.member),
                            trash: None,
                        })
                        .collect(),
                    Err(error) => {
                        self.error(error);
                        Vec::new()
                    }
                }
            }
            Location::Folder(dir) => {
                self.loaded = modified(dir);
                let listing = fs::read_dir(dir);
                self.denied = listing
                    .as_ref()
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied);
                match listing {
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
                                archive_member: None,
                                trash: None,
                            })
                        })
                        .collect(),
                    // Dossier réservé : la vue propose de l'ouvrir en administrateur.
                    Err(_) if self.denied => Vec::new(),
                    Err(error) => {
                        self.error(format!("Impossible de lire ce dossier : {error}"));
                        Vec::new()
                    }
                }
            }
        };
        if self.location != Location::Recent {
            self.sort_entries();
        }
        self.thumbs.forget();
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
                SortKey::Kind => natural_cmp(extension(&a.name), extension(&b.name))
                    .then_with(|| natural_cmp(&a.name, &b.name)),
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
        if let Location::Archive { archive, .. } = &self.location {
            let Some(entry) = self.entries.iter().find(|entry| entry.path == path) else {
                self.error("Élément introuvable dans l’archive");
                return;
            };
            let Some(member) = &entry.archive_member else {
                return;
            };
            if directory {
                self.go(Location::Archive {
                    archive: archive.clone(),
                    directory: member.clone(),
                });
            } else {
                match ops::materialize_archive_member(archive, member) {
                    Ok((temporary_dir, path)) => {
                        if let Err(error) = self.open_materialized(path, Some(temporary_dir.clone())) {
                            let _ = fs::remove_dir_all(temporary_dir);
                            self.error(error);
                        }
                    }
                    Err(error) => self.error(error),
                }
            }
            return;
        }
        if directory {
            self.go(Location::Folder(path));
            return;
        }
        if ops::is_archive(&path) {
            self.go(Location::Archive {
                archive: path,
                directory: String::new(),
            });
            return;
        }
        if let Err(error) = self.open_materialized(path, None) {
            self.error(error);
        }
    }

    fn open_materialized(&self, path: PathBuf, temporary_dir: Option<PathBuf>) -> Result<(), String> {
        // TextEdit et la Visionneuse sont le même exécutable que Fichiers.
        let builtin = if is_text_file(&path) {
            Some("textedit")
        } else if crate::viewer::supported(&path) {
            Some("viewer")
        } else if crate::music::supported(&path) {
            Some("music")
        } else {
            None
        };
        if let (Some(app), Some(temporary_dir)) = (builtin, temporary_dir) {
            let result = env::current_exe().and_then(|exe| {
                let mut command = Command::new(exe);
                command
                    .arg(app)
                    .arg(&path)
                    .stdin(std::process::Stdio::null())
                    .spawn()
            });
            return result
                .map(|mut child| {
                    thread::spawn(move || {
                        let _ = child.wait();
                        let _ = fs::remove_dir_all(temporary_dir);
                    });
                })
                .map_err(|error| format!("Ouverture impossible : {error}"));
        }
        let result = if let Some(app) = builtin {
            env::current_exe().map(Command::new).and_then(|mut command| {
                command.arg(app).arg(&path);
                spawn_detached(command)
            })
        } else {
            let mut command = Command::new("xdg-open");
            command.arg(&path);
            spawn_detached(command)
        };
        result
            .map(|_| ())
            .map_err(|error| format!("Ouverture impossible : {error}"))
    }

    fn in_archive(&self) -> bool {
        matches!(self.location, Location::Archive { .. })
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
        if self.in_archive()
            && matches!(
                &action,
                Action::Share(_)
                    | Action::ToggleTag(_, _)
                    | Action::StartDrag(_)
                    | Action::Drop { .. }
                    | Action::StartRename(_)
                    | Action::CommitRename
                    | Action::Copy
                    | Action::Cut
                    | Action::Paste
                    | Action::Duplicate
                    | Action::Compress(_)
                    | Action::Extract(_)
                    | Action::Trash
                    | Action::AskDeleteForever
                    | Action::ConfirmDeleteForever(_)
                    | Action::Restore
                    | Action::AskEmptyTrash
                    | Action::ConfirmEmptyTrash
                    | Action::NewFolder
                    | Action::Properties(_)
                    | Action::OpenWith(_)
                    | Action::LaunchWith(_, _)
                    | Action::TerminalHere
            )
        {
            return;
        }
        match action {
            Action::Navigate(path) => self.go(Location::Folder(path)),
            Action::Show(location) => self.go(location),
            Action::ShowTrash => self.go(Location::Trash),
            Action::ToggleTheme => {
                let mut settings = ferrite_system::config::Settings::load();
                let dark = !style::theme_is_dark();
                settings.set("apparence", if dark { "sombre" } else { "clair" });
                if let Err(error) = settings.save() {
                    self.error(format!("Thème non enregistré : {error}"));
                }
                style::apply_preferences(ctx, &settings);
            }
            Action::TogglePreview => {
                self.preview = !self.preview;
                self.save_preferences();
            }
            Action::Share(paths) => {
                let count = paths.len();
                ctx.copy_text(ferrite_system::drag::uri_list(&paths).trim_end().to_owned());
                self.info(if count > 1 {
                    format!("Adresses des {count} éléments copiées")
                } else {
                    "Adresse du fichier copiée".to_owned()
                });
            }
            Action::ToggleTag(path, tag) => {
                self.tags.toggle(&path, &tag);
                if matches!(self.location, Location::Tag(_)) {
                    self.refresh();
                }
            }
            Action::QuickLook => {
                self.quick = match (&self.quick, self.selected.len()) {
                    (None, 1) => self.selection().pop(),
                    _ => None,
                };
            }
            Action::StartDrag(index) => self.start_drag(index),
            Action::Drop { target, paths, copy } => self.drop_on(target, paths, copy),
            Action::Back => self.go_back(),
            Action::Forward => self.go_forward(),
            Action::Up => match &self.location {
                Location::Folder(dir) => {
                    if let Some(parent) = dir.parent() {
                        self.go(Location::Folder(parent.to_path_buf()));
                    }
                }
                Location::Archive { archive, directory } => {
                    if let Some((parent, _)) = directory.rsplit_once('/') {
                        self.go(Location::Archive {
                            archive: archive.clone(),
                            directory: parent.to_owned(),
                        });
                    } else {
                        self.go(Location::Folder(
                            archive.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
                        ));
                    }
                }
                Location::Trash | Location::Recent | Location::Tag(_) => {}
            },
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
            Action::Compress(format) => {
                if let Some(dir) = self.current_dir().map(Path::to_path_buf) {
                    let sources = self.selection();
                    let title = format!("Compression {}", format.label());
                    self.run_job(&title, move |job| {
                        ops::create_archive(&sources, &dir, format, job)
                    });
                }
            }
            Action::Extract(archive) => {
                self.run_job("Extraction", move |job| ops::extract_archive(&archive, job));
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
                let program = std::env::current_exe().unwrap_or_else(|_| "ferrite-apps".into());
                let mut command = Command::new(program);
                command.arg("terminal").arg(dir);
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
                    self.ascending = matches!(key, SortKey::Name | SortKey::Kind);
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
            Action::Mount(device) => {
                let sender = self.sender.clone();
                let ctx = ctx.clone();
                thread::spawn(move || {
                    let _ = sender.send(Update::Mounted(devices::mount(&device)));
                    ctx.request_repaint();
                });
            }
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
                Update::Devices(disks) => {
                    self.devices = disks.devices;
                    self.usage = disks.folder;
                    self.system_usage = disks.system;
                    self.downloads = disks.downloads;
                    self.devices_pending = false;
                }
                Update::Mounted(Ok(path)) => {
                    self.devices_checked = None;
                    self.go(Location::Folder(path));
                }
                Update::Mounted(Err(error)) => self.error(error),
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
                let home = home();
                let devices = devices::list()
                    .into_iter()
                    .map(|device| Device {
                        usage: device.mount.as_deref().and_then(places::disk_usage),
                        ..device
                    })
                    .collect();
                let disks = Disks {
                    devices,
                    folder: places::disk_usage(&dir),
                    system: places::disk_usage(&home),
                    downloads: fs::read_dir(home.join("Téléchargements"))
                        .map(|dir| dir.count())
                        .unwrap_or(0),
                };
                let _ = sender.send(Update::Devices(disks));
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
            Location::Archive { archive, .. } => modified(archive),
            Location::Trash => modified(&trash::files_dir()),
            Location::Recent | Location::Tag(_) => None,
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
            // Comme le Finder : Entrée renomme, Ctrl+O ou Ctrl+↓ ouvre, Espace montre
            // l'aperçu rapide.
            let single = self.selected.len() == 1;
            if single && (pressed(Key::F2) || (pressed(Key::Enter) && !alt && self.picker.is_none())) {
                actions.push(Action::StartRename(self.selection()[0].clone()));
            }
            if pressed(Key::Enter) && alt && single {
                actions.push(Action::Properties(self.selection()[0].clone()));
            } else if single
                && ((ctrl && (pressed(Key::O) || pressed(Key::ArrowDown)))
                    || (pressed(Key::Enter) && self.picker.is_some()))
            {
                let path = self.selection()[0].clone();
                let directory = self
                    .entries
                    .iter()
                    .find(|entry| entry.path == path)
                    .is_some_and(|entry| entry.directory);
                actions.push(Action::Open(path, directory));
            }
            if pressed(Key::Space) && self.quick.is_none() {
                actions.push(Action::QuickLook);
            }
            if pressed(Key::Escape) && self.quick.is_none() {
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
                    .fill(style::header_color())
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
                            ui.label(RichText::new(text).color(style::text_secondary()));
                        }
                    }
                    if let Some((text, true)) = &message {
                        ui.colored_label(style::error(), text);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (label, enabled) = match picker.mode {
                            PickerMode::Save => ("Enregistrer", !picker.name.trim().is_empty()),
                            PickerMode::Open => ("Ouvrir", chosen_file.is_some()),
                        };
                        let button = egui::Button::new(RichText::new(label).color(style::accent_text()))
                            .fill(style::accent())
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

    /// Menu contextuel d'un élément.
    fn item_menu(&self, ui: &mut egui::Ui, entry: &FileEntry, actions: &mut Vec<Action>) {
        let single = self.selected.len() <= 1;
        style::menu_style(ui);
        let mut item = |ui: &mut egui::Ui, text: &str, key: &str, enabled: bool, action: Action| {
            if style::menu_row(ui, text, key, false, enabled).clicked() {
                actions.push(action);
                ui.close();
            }
        };
        if self.location == Location::Trash {
            item(ui, "Restaurer", "", true, Action::Restore);
            if style::menu_row(ui, "Supprimer définitivement…", "", true, true).clicked() {
                actions.push(Action::AskDeleteForever);
                ui.close();
            }
            style::menu_separator(ui);
            let properties = Action::Properties(entry.path.clone());
            if style::menu_row(ui, "Propriétés", "Alt+Entrée", false, single).clicked() {
                actions.push(properties);
                ui.close();
            }
            return;
        }
        item(
            ui,
            "Ouvrir",
            "Entrée",
            single,
            Action::Open(entry.path.clone(), entry.directory),
        );
        if self.in_archive() {
            return;
        }
        if !entry.directory {
            item(
                ui,
                "Ouvrir avec…",
                "›",
                single,
                Action::OpenWith(entry.path.clone()),
            );
        }
        style::menu_separator(ui);
        item(ui, "Copier", "Ctrl+C", true, Action::Copy);
        item(ui, "Couper", "Ctrl+X", true, Action::Cut);
        item(ui, "Dupliquer", "Ctrl+D", true, Action::Duplicate);
        if self.picker.is_none() && self.current_dir().is_some() {
            if !entry.directory && ops::is_archive(&entry.path) {
                item(ui, "Extraire ici", "", true, Action::Extract(entry.path.clone()));
            }
            style::menu_separator(ui);
            let enabled = !self.selected.is_empty();
            for format in [
                ops::ArchiveFormat::Zip,
                ops::ArchiveFormat::Tar,
                ops::ArchiveFormat::TarGz,
                ops::ArchiveFormat::Rar,
            ] {
                item(
                    ui,
                    &format!("Compresser en {}", format.label()),
                    "",
                    enabled,
                    Action::Compress(format),
                );
            }
        }
        item(
            ui,
            "Renommer",
            "F2",
            single,
            Action::StartRename(entry.path.clone()),
        );
        if style::menu_row(ui, "Mettre à la corbeille", "Suppr", true, true).clicked() {
            actions.push(Action::Trash);
            ui.close();
        }
        style::menu_separator(ui);
        if style::menu_row(ui, "Propriétés", "Alt+Entrée", false, single).clicked() {
            actions.push(Action::Properties(entry.path.clone()));
            ui.close();
        }
    }

    /// Menu contextuel de la zone vide du dossier.
    fn folder_menu(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        style::menu_style(ui);
        let mut item = |ui: &mut egui::Ui, text: &str, enabled: bool, action: Action| {
            if style::menu_row(ui, text, "", false, enabled).clicked() {
                actions.push(action);
                ui.close();
            }
        };
        if self.in_archive() {
            item(
                ui,
                if self.show_hidden {
                    "Masquer les fichiers cachés"
                } else {
                    "Afficher les fichiers cachés"
                },
                true,
                Action::ToggleHidden,
            );
            return;
        }
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
        style::menu_separator(ui);
        item(ui, "Ouvrir un terminal ici", true, Action::TerminalHere);
        let hidden = if self.show_hidden {
            "Masquer les fichiers cachés"
        } else {
            "Afficher les fichiers cachés"
        };
        item(ui, hidden, true, Action::ToggleHidden);

        if let Some(dir) = self.current_dir() {
            style::menu_separator(ui);
            item(
                ui,
                "Propriétés du dossier",
                true,
                Action::Properties(dir.to_path_buf()),
            );
        }
        style::menu_separator(ui);
        for (label, key) in [
            ("Trier par nom", SortKey::Name),
            ("Trier par date", SortKey::Modified),
            ("Trier par taille", SortKey::Size),
            ("Trier par type", SortKey::Kind),
        ] {
            if style::radio_row(ui, label, self.sort == key).clicked() {
                actions.push(Action::Sort(key));
                ui.close();
            }
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
        if response.drag_started()
            && self.picker.is_none()
            && !self.in_archive()
            && self.location != Location::Trash
        {
            actions.push(Action::StartDrag(index));
            return;
        }
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

    fn dialogs(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if matches!(self.dialog, Some(Dialog::Properties(_))) {
            self.properties_dialog(ctx, actions);
            return;
        }
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
                    ui.label(RichText::new("Cette action est irréversible.").color(style::text_secondary()));
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
                            .color(style::text_secondary()),
                    );
                    ui.add_space(12.0);
                    match dialog_buttons(ui, "Vider") {
                        Some(true) => actions.push(Action::ConfirmEmptyTrash),
                        Some(false) => actions.push(Action::CloseDialog),
                        None => {}
                    }
                }
                // Dessiné à part (voir `properties_dialog`).
                Dialog::Properties(_) => {}
                Dialog::Replace(path) => {
                    ui.heading(format!("Remplacer « {} » ?", file_name(path)));
                    ui.label(
                        RichText::new("Un fichier de ce nom existe déjà ; son contenu sera remplacé.")
                            .color(style::text_secondary()),
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
                        .color(style::text_secondary()),
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
                                categories: Vec::new(),
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
        let button = egui::Button::new(RichText::new(confirm).color(Color32::WHITE)).fill(style::error());
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

#[derive(Clone, Copy)]
enum Glyph {
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

fn properties(path: &Path) -> Properties {
    use std::os::unix::fs::MetadataExt;
    let metadata = path.symlink_metadata().ok();
    let directory = metadata.as_ref().is_some_and(|m| m.is_dir());
    let entry = entry_for(path);
    let kind = match (&metadata, &entry) {
        (Some(m), _) if m.file_type().is_symlink() => format!(
            "Lien vers {}",
            fs::read_link(path)
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        ),
        (Some(_), Some(entry)) => view::kind_label(entry),
        (Some(_), None) => "Fichier".to_owned(),
        (None, _) => "Introuvable".to_owned(),
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
        mode: metadata.as_ref().map_or(0, |m| m.permissions().mode() & 0o7777),
        owner: metadata
            .as_ref()
            .map(|m| account_name("/etc/passwd", m.uid()))
            .unwrap_or_default(),
        group: metadata
            .as_ref()
            .map(|m| account_name("/etc/group", m.gid()))
            .unwrap_or_default(),
        on_disk: metadata.as_ref().filter(|_| !directory).map(|m| m.blocks() * 512),
        icon: entry.as_ref().map_or("file-generic", entry_icon),
        opener: entry.as_ref().map_or("—", view::opener_label),
        name: file_name(path),
    }
}

/// Nom du compte (utilisateur ou groupe) numéro `id`, lu dans `file` (`/etc/passwd`,
/// `/etc/group`) ; le numéro s'il n'y figure pas.
fn account_name(file: &str, id: u32) -> String {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let mut fields = line.split(':');
                let name = fields.next()?;
                (fields.nth(1)?.parse::<u32>().ok()? == id).then(|| name.to_owned())
            })
        })
        .unwrap_or_else(|| id.to_string())
}

/// Accès d'une classe (propriétaire, groupe, autres) : 0 aucun, 1 lecture, 2 lecture et
/// écriture ; `shift` vaut 6, 3 ou 0.
fn access_level(mode: u32, shift: u32) -> usize {
    match (mode >> shift) & 0o6 {
        0o6 | 0o2 => 2,
        0o4 => 1,
        _ => 0,
    }
}

/// `mode` avec l'accès `level` pour la classe `shift`, son droit d'exécution gardé.
fn with_access(mode: u32, shift: u32, level: usize) -> u32 {
    let bits = [0, 0o4, 0o6][level.min(2)];
    (mode & !(0o6 << shift)) | (bits << shift)
}

/// « rw-r--r-- ».
fn mode_text(mode: u32) -> String {
    (0..9)
        .rev()
        .map(|bit| {
            if mode & (1 << bit) == 0 {
                '-'
            } else {
                ['x', 'w', 'r'][bit % 3]
            }
        })
        .collect()
}

/// Extension d'un nom de fichier, pour le tri par type.
fn extension(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(_, extension)| extension)
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

/// Met `paths` à la Corbeille ; renvoie les erreurs rencontrées.
pub fn put_in_trash(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().filter_map(|path| trash::trash(path).err()).collect()
}

/// Élément de `path`, hors de la lecture d'un dossier (Récents, étiquettes).
fn entry_for(path: &Path) -> Option<FileEntry> {
    let metadata = fs::metadata(path).ok()?;
    Some(FileEntry {
        name: file_name(path),
        path: path.to_path_buf(),
        directory: metadata.is_dir(),
        symlink: path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()),
        modified: metadata.modified().ok(),
        size: (!metadata.is_dir()).then_some(metadata.len()),
        archive_member: None,
        trash: None,
    })
}

fn archive_virtual_path(archive: &Path, member: &str) -> PathBuf {
    let name = archive
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    archive
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".ferrite-archive-{name}"))
        .join(member)
}

/// Icône du dossier `icons/SVG` selon la nature de l'entrée.
fn entry_icon(entry: &FileEntry) -> &'static str {
    if entry.directory {
        return folder_icon(&entry.path, &home());
    }
    let extension = entry
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => "file-image",
        "mp3" | "ogg" | "flac" | "wav" | "m4a" | "opus" => "file-audio",
        "mp4" | "mkv" | "webm" | "avi" | "mov" => "file-video",
        "rs" | "c" | "h" | "py" | "js" | "ts" | "toml" | "json" | "yaml" | "yml" | "html" | "css" => {
            "file-script"
        }
        "sh" => "file-script",
        "pdf" => "file-pdf",
        "zip" | "tar" | "gz" | "xz" | "bz2" | "zst" | "7z" | "rar" | "deb" | "rpm" | "iso" => "file-archive",
        "appimage" | "run" | "bin" | "exe" => "file-executable",
        _ if is_text_file(&entry.path) => "file-text",
        _ => "file-generic",
    }
}

/// Icône d'un dossier : les dossiers habituels du dossier personnel ont la leur.
fn folder_icon(path: &Path, home: &Path) -> &'static str {
    if path == home {
        return "places-home";
    }
    if path.parent() != Some(home) {
        return "places-folder";
    }
    match path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
    {
        "Bureau" | "Desktop" => "places-desktop",
        "Documents" => "places-documents",
        "Téléchargements" | "Downloads" => "places-downloads",
        "Images" | "Pictures" => "places-pictures",
        "Musique" | "Music" => "places-music",
        "Vidéos" | "Videos" => "places-videos",
        "Public" => "places-public",
        "Modèles" | "Templates" => "places-templates",
        _ => "places-folder",
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

pub(crate) fn format_size(size: u64) -> String {
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

pub(crate) fn format_date(time: Option<SystemTime>) -> String {
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
    fn icones_des_dossiers() {
        let home = Path::new("/home/ferrite");
        assert_eq!(folder_icon(home, home), "places-home");
        assert_eq!(folder_icon(&home.join("Musique"), home), "places-music");
        assert_eq!(
            folder_icon(&home.join("Téléchargements"), home),
            "places-downloads"
        );
        assert_eq!(folder_icon(&home.join("projet"), home), "places-folder");
        // Ailleurs que dans le dossier personnel, un dossier « Musique » n'a rien de spécial.
        assert_eq!(folder_icon(&home.join("a/Musique"), home), "places-folder");
    }

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
        assert_eq!(mode_text(0o644), "rw-r--r--");
        assert_eq!(mode_text(0o755), "rwxr-xr-x");
        assert_eq!(access_level(0o640, 6), 2);
        assert_eq!(access_level(0o640, 3), 1);
        assert_eq!(access_level(0o640, 0), 0);
        assert_eq!(with_access(0o755, 3, 0), 0o715);
        assert_eq!(with_access(0o600, 0, 1), 0o604);
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
    fn ouvre_une_archive_comme_un_dossier_virtuel() {
        let dir = std::env::temp_dir().join(format!("ferrite-archive-ui-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let source = dir.join("source");
        fs::create_dir_all(source.join("sous-dossier")).unwrap();
        fs::write(source.join("sous-dossier/note.txt"), "contenu").unwrap();
        let archive = dir.join("test.zip");
        let status = Command::new("bsdtar")
            .args(["--format", "zip", "-cf"])
            .arg(&archive)
            .arg("-C")
            .arg(&source)
            .arg("--")
            .arg("sous-dossier")
            .status()
            .unwrap();
        assert!(status.success());

        let mut files = FilesState::default();
        files.open(archive.clone(), false);
        assert_eq!(
            files.location,
            Location::Archive {
                archive: archive.clone(),
                directory: String::new(),
            }
        );
        assert_eq!(files.entries.len(), 1);
        assert!(files.entries[0].directory);

        files.open(files.entries[0].path.clone(), true);
        assert_eq!(
            files.location,
            Location::Archive {
                archive,
                directory: "sous-dossier".into(),
            }
        );
        assert_eq!(files.entries[0].name, "note.txt");
        assert!(!dir.join("sous-dossier").exists());
        fs::remove_dir_all(dir).unwrap();
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
