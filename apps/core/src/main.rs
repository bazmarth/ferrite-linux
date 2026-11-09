//! Applications de base Ferrite : Fichiers, TextEdit, Terminal, Calculatrice, Horloge,
//! Paramètres, Moniteur d'activité, Visionneuse, lecteur PDF, Musique et Calendrier.

mod calculator;
mod calendar;
mod chooser;
mod clock;
mod details;
mod editor;
mod files;
mod gallery;
mod island;
mod kit;
mod markdown;
mod monitor;
mod music;
mod pairing;
mod pdf_viewer;
mod settings;
mod single;
mod style;
mod terminal;
mod video;
mod viewer;

use std::{
    env,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use eframe::egui;

use ferrite_system::catalog::{self, AppInfo};

use crate::{
    calculator::CalculatorState, clock::ClockState, editor::EditorState, files::FilesState,
    monitor::MonitorState, music::MusicState, pdf_viewer::ViewerApp, settings::SettingsState,
    terminal::TerminalState,
};

/// Contenu d'une fenêtre d'application. Chaque application du catalogue
/// (`ferrite_system::catalog`) en fournit une, créée par [`create`].
trait AppView {
    /// Dessine la fenêtre ; le fond, le thème et F11 sont déjà pris en charge.
    fn update(&mut self, ui: &mut egui::Ui);
    /// Fichier (ou rubrique) confié par un autre lancement de la même application.
    fn open(&mut self, _target: PathBuf) {}
    /// Événements bruts, avant qu'egui ne s'en serve (une touche à garder pour soi).
    fn raw_input(&mut self, _ctx: &egui::Context, _input: &mut egui::RawInput) {}
}

/// Crée la vue de l'application `info`, avec le fichier ou la rubrique demandé.
///
/// Nouvelle application : une ligne ici, une entrée dans le catalogue, et son module
/// (voir `docs/NOUVELLE-APPLICATION.md`).
fn create(
    info: &AppInfo,
    context: &egui::Context,
    target: Option<PathBuf>,
    launch: terminal::Launch,
) -> Box<dyn AppView> {
    let opened = |mut view: Box<dyn AppView>| {
        if let Some(target) = target.clone() {
            view.open(target);
        }
        view
    };
    match info.command {
        "files" => opened(Box::<FilesState>::default()),
        "textedit" => opened(Box::<EditorState>::default()),
        "terminal" => Box::new(TerminalState::new(context, launch)),
        "calculator" => Box::new(CalculatorState::default()),
        "calendar" => opened(Box::<calendar::CalendarState>::default()),
        // `ferrite-apps clock minuteur` : l'Horloge ouvre directement cet onglet.
        "clock" => Box::new(ClockState::with_tab(
            target.as_deref().and_then(|path| path.to_str()),
        )),
        "settings" => opened(Box::<SettingsState>::default()),
        "monitor" => Box::new(MonitorState::default()),
        "viewer" => Box::new(ViewerApp::new(target)),
        "pdf" => Box::new(Pdf(ViewerApp::new_pdf(target))),
        "music" => Box::new(MusicState::new(target)),
        "video" => Box::new(video::VideoState::new(target, context.clone())),
        "galerie" => Box::new(gallery::GalleryState::default()),
        // Arguments de la demande du portail, après `choisir`.
        "choisir" => {
            let args: Vec<String> = env::args().skip(2).collect();
            Box::new(chooser::ChooserState::new(chooser::Request::parse(&args)))
        }
        other => unreachable!("application du catalogue sans vue : {other}"),
    }
}

impl AppView for chooser::ChooserState {
    fn update(&mut self, ui: &mut egui::Ui) {
        chooser::ChooserState::update(self, ui);
    }
}

impl AppView for gallery::GalleryState {
    fn update(&mut self, ui: &mut egui::Ui) {
        gallery::GalleryState::update(self, ui);
    }
}

impl AppView for FilesState {
    fn update(&mut self, ui: &mut egui::Ui) {
        FilesState::update(self, ui);
    }
    fn open(&mut self, target: PathBuf) {
        self.open_at(target);
    }
    fn raw_input(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        FilesState::raw_input(self, ctx, input);
    }
}

impl AppView for EditorState {
    fn update(&mut self, ui: &mut egui::Ui) {
        EditorState::update(self, ui);
    }
    fn open(&mut self, target: PathBuf) {
        EditorState::open(self, target);
    }
}

impl AppView for calendar::CalendarState {
    fn update(&mut self, ui: &mut egui::Ui) {
        calendar::CalendarState::update(self, ui);
    }
    fn open(&mut self, target: PathBuf) {
        calendar::CalendarState::open(self, &target.to_string_lossy());
    }
}

impl AppView for TerminalState {
    fn update(&mut self, ui: &mut egui::Ui) {
        TerminalState::update(self, ui);
    }
}

impl AppView for CalculatorState {
    fn update(&mut self, ui: &mut egui::Ui) {
        CalculatorState::update(self, ui);
    }
}

impl AppView for ClockState {
    fn update(&mut self, ui: &mut egui::Ui) {
        ClockState::update(self, ui);
    }
}

impl AppView for SettingsState {
    fn update(&mut self, ui: &mut egui::Ui) {
        SettingsState::update(self, ui);
    }
    fn open(&mut self, target: PathBuf) {
        self.open_page(&target.to_string_lossy());
    }
}

impl AppView for MonitorState {
    fn update(&mut self, ui: &mut egui::Ui) {
        MonitorState::update(self, ui);
    }
}

impl AppView for ViewerApp {
    fn update(&mut self, ui: &mut egui::Ui) {
        ViewerApp::update(self, ui);
    }
    fn open(&mut self, target: PathBuf) {
        ViewerApp::open(self, target);
    }
}

/// La Visionneuse en lecteur PDF.
struct Pdf(ViewerApp);

impl AppView for Pdf {
    fn update(&mut self, ui: &mut egui::Ui) {
        self.0.update(ui);
    }
    fn open(&mut self, target: PathBuf) {
        self.0.open_pdf(target);
    }
}

impl AppView for MusicState {
    fn update(&mut self, ui: &mut egui::Ui) {
        MusicState::update(self, ui);
    }
    fn open(&mut self, target: PathBuf) {
        MusicState::open(self, target);
    }
}

impl AppView for video::VideoState {
    fn update(&mut self, ui: &mut egui::Ui) {
        video::VideoState::update(self, ui);
    }
    fn open(&mut self, target: PathBuf) {
        video::VideoState::open(self, target);
    }
}

fn main() -> eframe::Result {
    let mut args = env::args().skip(1);
    // Fichiers lâchés sur la Corbeille du Dock : ferrite-shell les confie ici, sans fenêtre.
    if env::args().nth(1).as_deref() == Some("corbeille") {
        for error in files::put_in_trash(&env::args().skip(2).map(PathBuf::from).collect::<Vec<_>>()) {
            eprintln!("{error}");
        }
        return Ok(());
    }
    // Image d'une vidéo pour les vignettes de Fichiers, extraite dans ce processus à part.
    if env::args().nth(1).as_deref() == Some("vignette-video") {
        let ok = env::args()
            .nth(2)
            .is_some_and(|path| files::print_video_frame(Path::new(&path)));
        std::process::exit(if ok { 0 } else { 1 });
    }
    // Service des rappels du Calendrier, sans fenêtre : lancé par le bureau à l'ouverture
    // de session (et par le Calendrier), il annonce les rappels même Calendrier fermé.
    if env::args().nth(1).as_deref() == Some("rappels") {
        std::process::exit(calendar::reminders::run_service());
    }
    // Réception de fichiers par Bluetooth, sans fenêtre : lancée par le bureau à
    // l'ouverture de session.
    if env::args().nth(1).as_deref() == Some("reception-bluetooth") {
        std::process::exit(files::run_bluetooth_reception());
    }
    let Some(info) = args.next().and_then(|arg| catalog::by_command(&arg)) else {
        let commands: Vec<&str> = catalog::APPS.iter().map(|app| app.command).collect();
        eprintln!(
            "Utilisation : ferrite-apps <{}> [dossier ou fichier]",
            commands.join("|")
        );
        return Ok(());
    };
    if info.command == "terminal" {
        style::set_always_dark();
        // Chaque lancement est une fenêtre de plus, avec son propre shell.
        let launch = terminal::parse_launch(args);
        return run(info, None, None, launch);
    }
    // Sélecteur de fichiers : une fenêtre par demande, jamais confiée à une autre.
    if info.command == "choisir" {
        return run(info, None, None, terminal::Launch::default());
    }
    let initial_file = args.next().map(PathBuf::from);
    // Une seule fenêtre par application, sauf celles choisies dans Paramètres : un
    // deuxième lancement confie son fichier à la fenêtre déjà ouverte.
    let multiple = ferrite_system::config::Settings::load()
        .multi_window_apps()
        .iter()
        .any(|id| id == info.app_id);
    let requests = if multiple {
        None
    } else {
        match single::claim(info.app_id, initial_file.as_deref()) {
            single::Start::Primary(requests) => Some(requests),
            single::Start::Forwarded => return Ok(()),
        }
    };
    run(info, initial_file, requests, terminal::Launch::default())
}

fn run(
    info: &'static AppInfo,
    initial_file: Option<PathBuf>,
    requests: Option<single::Requests>,
    launch: terminal::Launch,
) -> eframe::Result {
    let size = |(w, h): (u16, u16)| [f32::from(w), f32::from(h)];
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_app_id(info.app_id)
            // Les applications Ferrite dessinent leur haut (colonne, barre d'outils et
            // boutons) ; ferrite-shell garde coins arrondis, ombre et bords.
            .with_decorations(false)
            .with_inner_size(size(info.size))
            .with_min_inner_size(size(info.min_size)),
        ..Default::default()
    };
    eframe::run_native(
        info.name,
        options,
        Box::new(move |context| {
            Ok(Box::new(FerriteApp::new(
                context,
                info,
                initial_file,
                requests,
                launch,
            )))
        }),
    )
}

/// Ce qui est commun à toutes les fenêtres : thème suivi en direct, fond, F11, instance
/// unique ; le reste revient à la vue de l'application.
struct FerriteApp {
    view: Box<dyn AppView>,
    /// Lancements suivants de la même application (instance unique).
    requests: Option<single::Requests>,
    settings_stamp: Option<SystemTime>,
    settings_checked_at: Instant,
}

impl FerriteApp {
    fn new(
        context: &eframe::CreationContext<'_>,
        info: &'static AppInfo,
        initial_file: Option<PathBuf>,
        requests: Option<single::Requests>,
        launch: terminal::Launch,
    ) -> Self {
        style::apply(&context.egui_ctx);
        if let Some(requests) = &requests
            && let Ok(mut waker) = requests.context.lock()
        {
            *waker = Some(context.egui_ctx.clone());
        }
        Self {
            view: create(info, &context.egui_ctx, initial_file, launch),
            requests,
            settings_stamp: {
                watch_settings(context.egui_ctx.clone());
                ferrite_system::config::Settings::modified()
            },
            settings_checked_at: Instant::now(),
        }
    }

    fn refresh_preferences(&mut self, context: &egui::Context) {
        // Réglages modifiés : tout de suite (voir `watch_settings`) ; sinon une fois par
        // seconde, pour le thème qui suit l'heure.
        let stamp = ferrite_system::config::Settings::modified();
        if stamp == self.settings_stamp && self.settings_checked_at.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.settings_checked_at = Instant::now();
        let settings = ferrite_system::config::Settings::load();
        if stamp != self.settings_stamp || style::wants_dark(&settings) != style::theme_is_dark() {
            self.settings_stamp = stamp;
            style::apply_preferences(context, &settings);
        }
    }

    /// Un autre lancement de l'application : son fichier s'ouvre ici, et la fenêtre
    /// revient au premier plan (le compositeur la sort du Dock si elle était réduite).
    fn handle_requests(&mut self, ctx: &egui::Context) {
        let Some(requests) = &self.requests else { return };
        let received: Vec<Option<PathBuf>> = requests.receiver.try_iter().collect();
        if received.is_empty() {
            return;
        }
        for file in received.into_iter().flatten() {
            self.view.open(file);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

impl eframe::App for FerriteApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.view.raw_input(ctx, raw_input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.refresh_preferences(ui.ctx());
        self.handle_requests(&ui.ctx().clone());
        // F11 : plein écran, comme dans les navigateurs.
        if ui.input(|input| input.key_pressed(egui::Key::F11)) {
            let fullscreen = ui.input(|input| input.viewport().fullscreen.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
        }
        // Le client est rendu dans une surface Wayland translucide : peindre son
        // fond explicitement évite que le bureau transparaisse.
        ui.painter()
            .rect_filled(ui.max_rect(), egui::CornerRadius::ZERO, style::background());
        self.view.update(ui);
        if !style::inside_ferrite() {
            resize_edges(ui);
        }
    }
}

/// Bords de la fenêtre qui la redimensionnent (fenêtre sans décorations hors de Ferrite),
/// et un trait fin qui la détache des fenêtres voisines.
fn resize_edges(ui: &mut egui::Ui) {
    use egui::{CursorIcon, ResizeDirection};
    let rect = ui.max_rect();
    let (maximized, fullscreen) = ui.input(|input| {
        let viewport = input.viewport();
        (
            viewport.maximized.unwrap_or(false),
            viewport.fullscreen.unwrap_or(false),
        )
    });
    if maximized || fullscreen {
        return;
    }
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::ZERO,
        egui::Stroke::new(1.0, style::separator()),
        egui::StrokeKind::Inside,
    );
    const EDGE: f32 = 5.0;
    const CORNER: f32 = 14.0;
    let (left, right, top, bottom) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    let zones = [
        (
            egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + CORNER, top + EDGE)),
            ResizeDirection::NorthWest,
            CursorIcon::ResizeNorthWest,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + EDGE, top + CORNER)),
            ResizeDirection::NorthWest,
            CursorIcon::ResizeNorthWest,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(right - CORNER, top), egui::pos2(right, top + EDGE)),
            ResizeDirection::NorthEast,
            CursorIcon::ResizeNorthEast,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(right - EDGE, top), egui::pos2(right, top + CORNER)),
            ResizeDirection::NorthEast,
            CursorIcon::ResizeNorthEast,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(left, bottom - EDGE), egui::pos2(left + CORNER, bottom)),
            ResizeDirection::SouthWest,
            CursorIcon::ResizeSouthWest,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(left, bottom - CORNER), egui::pos2(left + EDGE, bottom)),
            ResizeDirection::SouthWest,
            CursorIcon::ResizeSouthWest,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(right - CORNER, bottom - EDGE),
                egui::pos2(right, bottom),
            ),
            ResizeDirection::SouthEast,
            CursorIcon::ResizeSouthEast,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(right - EDGE, bottom - CORNER),
                egui::pos2(right, bottom),
            ),
            ResizeDirection::SouthEast,
            CursorIcon::ResizeSouthEast,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, top + EDGE)),
            ResizeDirection::North,
            CursorIcon::ResizeNorth,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(left, bottom - EDGE), egui::pos2(right, bottom)),
            ResizeDirection::South,
            CursorIcon::ResizeSouth,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + EDGE, bottom)),
            ResizeDirection::West,
            CursorIcon::ResizeWest,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(right - EDGE, top), egui::pos2(right, bottom)),
            ResizeDirection::East,
            CursorIcon::ResizeEast,
        ),
    ];
    // Le premier bord touché l'emporte : les coins avant les côtés.
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let Some((zone, direction, cursor)) =
        pointer.and_then(|at| zones.into_iter().find(|(zone, ..)| zone.contains(at)))
    else {
        return;
    };
    let response = ui.interact(
        zone,
        ui.scope_id().with(("bord", format!("{direction:?}"))),
        egui::Sense::drag(),
    );
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(cursor);
    }
    if response.drag_started() {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }
}

/// Réveille l'application dès que les réglages changent : une fenêtre au repos ne se
/// redessine pas, et ne verrait sinon le nouveau thème qu'au prochain survol, longtemps
/// après les autres.
fn watch_settings(context: egui::Context) {
    let spawned = std::thread::Builder::new()
        .name("réglages".into())
        .spawn(move || {
            let mut stamp = ferrite_system::config::Settings::modified();
            loop {
                std::thread::sleep(Duration::from_millis(150));
                let current = ferrite_system::config::Settings::modified();
                if current != stamp {
                    stamp = current;
                    context.request_repaint();
                }
            }
        });
    if let Err(err) = spawned {
        eprintln!("surveillance des réglages impossible : {err}");
    }
}
