//! Musique, d'après les planches « Musique » claire et sombre de la maquette : bibliothèque à
//! gauche (recherche, Écouter maintenant, Albums, Titres, Artistes), albums en grille ou en
//! liste de titres au centre, file d'attente à droite et lecteur en bas.

mod engine;
mod library;
mod mpris;

use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Key, Pos2, Rect, Sense, Stroke, TextureHandle,
    TextureOptions, pos2, vec2,
};
use ferrite_ui::wallpaper;

use crate::style;
use engine::{Engine, EngineCommand, Status};
/// Vérifié par les tests de Fichiers (chaque extension va à une application qui la lit).
#[cfg(test)]
pub use library::supported;
use library::{Album, Cover, Library, Track, format_duration};
use mpris::{Mpris, Published, Request};

const SIDEBAR_WIDTH: f32 = 232.0;
const QUEUE_WIDTH: f32 = 300.0;
const PLAYER_HEIGHT: f32 = 88.0;
/// Côté des pochettes décodées (la plus grande est affichée à 220 points).
const COVER_SIDE: u32 = 440;
/// Au-delà, « Précédent » revient au début du morceau.
const RESTART_AFTER: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Albums,
    Tracks,
    Artists,
    Album(usize),
    Artist(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repeat {
    Off,
    All,
    One,
}

/// Pochette décodée d'un album, et son fichier (pour MPRIS).
type LoadedCover = (usize, Option<(egui::ColorImage, Option<PathBuf>)>);

enum CoverSlot {
    Pending,
    Ready(TextureHandle),
    Missing,
}

pub struct MusicState {
    library: Option<Library>,
    folder: PathBuf,
    scan: Option<Receiver<Library>>,
    /// Morceau à lire dès que la bibliothèque est lue (lancement avec un fichier).
    start_with: Option<PathBuf>,
    page: Page,
    search: String,
    /// Morceaux à jouer (indices de la bibliothèque) et position dans la file.
    queue: Vec<usize>,
    current: Option<usize>,
    shuffle: bool,
    repeat: Repeat,
    volume: f32,
    show_queue: bool,
    engine: Option<Engine>,
    mpris: Option<Mpris>,
    finished_seen: u64,
    covers: HashMap<usize, CoverSlot>,
    cover_files: HashMap<usize, PathBuf>,
    cover_sender: Sender<LoadedCover>,
    cover_receiver: Receiver<LoadedCover>,
    /// Position glissée sur la barre de progression, appliquée au relâchement.
    seeking: Option<f64>,
    /// Morceau demandé avant que le moteur de lecture existe.
    pending_open: Option<(PathBuf, bool)>,
}

impl MusicState {
    pub fn new(target: Option<PathBuf>) -> Self {
        let (folder, start_with) = match target {
            Some(path) if path.is_dir() => (path, None),
            Some(path) => (
                path.parent().map_or_else(library::music_dir, Path::to_path_buf),
                Some(path),
            ),
            None => (library::music_dir(), None),
        };
        let (cover_sender, cover_receiver) = mpsc::channel();
        let mut state = Self {
            library: None,
            folder,
            scan: None,
            start_with,
            page: Page::Home,
            search: String::new(),
            queue: Vec::new(),
            current: None,
            shuffle: false,
            repeat: Repeat::Off,
            volume: 0.65,
            show_queue: true,
            engine: None,
            mpris: None,
            finished_seen: 0,
            covers: HashMap::new(),
            cover_files: HashMap::new(),
            cover_sender,
            cover_receiver,
            seeking: None,
            pending_open: None,
        };
        state.rescan();
        state
    }

    /// Un autre lancement de Musique : son fichier est lu ici.
    pub fn open(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.folder = path;
            self.rescan();
            return;
        }
        let found = self
            .library
            .as_ref()
            .and_then(|library| library.tracks.iter().position(|track| track.path == path));
        match found {
            Some(index) => self.play_list(vec![index], 0),
            None => {
                self.folder = path.parent().map_or_else(library::music_dir, Path::to_path_buf);
                self.start_with = Some(path);
                self.rescan();
            }
        }
    }

    fn rescan(&mut self) {
        let (sender, receiver) = mpsc::channel();
        let folder = self.folder.clone();
        thread::spawn(move || {
            let _ = sender.send(library::scan(&folder));
        });
        self.scan = Some(receiver);
        self.covers.clear();
        self.cover_files.clear();
    }

    fn track(&self, index: usize) -> Option<&Track> {
        self.library
            .as_ref()
            .and_then(|library| library.tracks.get(index))
    }

    fn album_of(&self, track: usize) -> Option<usize> {
        self.library.as_ref().and_then(|library| {
            library
                .albums
                .iter()
                .position(|album| album.tracks.contains(&track))
        })
    }

    fn engine(&mut self, ctx: &egui::Context) -> &Engine {
        if self.engine.is_none() {
            let engine = Engine::start(ctx.clone());
            engine.send(EngineCommand::Volume(self.volume));
            self.engine = Some(engine);
        }
        self.engine.as_ref().expect("moteur démarré juste au-dessus")
    }

    fn status(&self) -> Status {
        self.engine.as_ref().map(Engine::status).unwrap_or_default()
    }

    /// Joue `tracks` à partir de la position `start` (mélangés si l'aléatoire est actif).
    fn play_list(&mut self, mut tracks: Vec<usize>, start: usize) {
        if tracks.is_empty() {
            return;
        }
        let start = start.min(tracks.len() - 1);
        let first = tracks[start];
        if self.shuffle {
            shuffle(&mut tracks, first);
        } else {
            tracks.rotate_left(start);
        }
        self.queue = tracks;
        self.current = Some(0);
        self.open_current(true);
    }

    fn open_current(&mut self, play: bool) {
        let Some(index) = self
            .current
            .and_then(|position| self.queue.get(position))
            .copied()
        else {
            return;
        };
        let Some(path) = self.track(index).map(|track| track.path.clone()) else {
            return;
        };
        if let Some(engine) = &self.engine {
            engine.send(EngineCommand::Open { path, play });
        } else {
            self.pending_open = Some((path, play));
        }
    }

    fn step(&mut self, forward: bool, automatic: bool) {
        let Some(position) = self.current else {
            return;
        };
        let count = self.queue.len();
        if automatic && self.repeat == Repeat::One {
            self.open_current(true);
            return;
        }
        let next = if forward {
            if position + 1 < count {
                Some(position + 1)
            } else if self.repeat == Repeat::All || !automatic {
                Some(0)
            } else {
                None
            }
        } else {
            Some(position.checked_sub(1).unwrap_or(count - 1))
        };
        match next {
            Some(next) => {
                self.current = Some(next);
                self.open_current(true);
            }
            // Fin de la file : on s'arrête sur le premier morceau, en pause.
            None => {
                self.current = Some(0);
                self.open_current(false);
            }
        }
    }

    fn previous(&mut self) {
        if self.status().position > RESTART_AFTER {
            self.seek_to(0.0);
        } else {
            self.step(false, false);
        }
    }

    fn toggle_play(&mut self) {
        let status = self.status();
        match (&self.engine, status.path.is_some()) {
            (Some(engine), true) => engine.send(if status.playing {
                EngineCommand::Pause
            } else {
                EngineCommand::Play
            }),
            _ => {
                // Rien de chargé : on lance la bibliothèque depuis le début.
                let all: Vec<usize> = self.album_order();
                self.play_list(all, 0);
            }
        }
    }

    fn seek_to(&self, seconds: f64) {
        if let Some(engine) = &self.engine {
            engine.send(EngineCommand::Seek(seconds));
        }
    }

    /// Tous les morceaux, album après album.
    fn album_order(&self) -> Vec<usize> {
        self.library
            .as_ref()
            .map(|library| {
                library
                    .albums
                    .iter()
                    .flat_map(|album| album.tracks.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn handle_mpris(&mut self, ctx: &egui::Context) {
        let requests: Vec<Request> = self
            .mpris
            .as_ref()
            .map(|mpris| mpris.requests.try_iter().collect())
            .unwrap_or_default();
        for request in requests {
            match request {
                Request::PlayPause => self.toggle_play(),
                Request::Play => {
                    if !self.status().playing {
                        self.toggle_play();
                    }
                }
                Request::Pause => {
                    if let Some(engine) = &self.engine {
                        engine.send(EngineCommand::Pause);
                    }
                }
                Request::Next => self.step(true, false),
                Request::Previous => self.previous(),
                Request::SeekBy(offset) => self.seek_to(self.status().position + offset),
                Request::SetPosition(position) => self.seek_to(position),
            }
        }
        // Le moteur a été démarré pour un morceau demandé avant lui.
        if let Some((path, play)) = self.pending_open.take() {
            self.engine(ctx).send(EngineCommand::Open { path, play });
        }
    }

    fn publish(&mut self, status: &Status) {
        let Some(mpris) = &self.mpris else { return };
        let current = self
            .current
            .and_then(|position| self.queue.get(position))
            .copied();
        let published = match current.and_then(|index| self.track(index).map(|track| (index, track))) {
            Some((index, track)) => Published {
                has_track: true,
                playing: status.playing,
                title: track.title.clone(),
                artist: track.artist.clone(),
                album: track.album.clone(),
                art: self
                    .album_of(index)
                    .and_then(|album| self.cover_files.get(&album).cloned()),
                length: if status.duration > 0.0 {
                    status.duration
                } else {
                    track.duration
                },
                position: status.position,
            },
            None => Published::default(),
        };
        mpris.publish(published);
    }

    /// Pochette de l'album `index` (décodée dans un fil à part la première fois).
    fn cover(&mut self, ctx: &egui::Context, index: usize) -> Option<TextureHandle> {
        match self.covers.get(&index) {
            Some(CoverSlot::Ready(texture)) => return Some(texture.clone()),
            Some(_) => return None,
            None => {}
        }
        let cover = self
            .library
            .as_ref()
            .and_then(|library| library.albums.get(index))
            .and_then(|album| album.cover.clone());
        let Some(cover) = cover else {
            self.covers.insert(index, CoverSlot::Missing);
            return None;
        };
        self.covers.insert(index, CoverSlot::Pending);
        let sender = self.cover_sender.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let loaded = load_cover(&cover, index);
            let _ = sender.send((index, loaded));
            ctx.request_repaint();
        });
        None
    }

    fn receive_covers(&mut self, ctx: &egui::Context) {
        while let Ok((index, loaded)) = self.cover_receiver.try_recv() {
            let slot = match loaded {
                Some((image, file)) => {
                    if let Some(file) = file {
                        self.cover_files.insert(index, file);
                    }
                    CoverSlot::Ready(ctx.load_texture(
                        format!("pochette:{index}"),
                        image,
                        TextureOptions::LINEAR,
                    ))
                }
                None => CoverSlot::Missing,
            };
            self.covers.insert(index, slot);
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (space, left, right, ctrl) = ctx.input(|input| {
            (
                input.key_pressed(Key::Space),
                input.key_pressed(Key::ArrowLeft),
                input.key_pressed(Key::ArrowRight),
                input.modifiers.ctrl,
            )
        });
        if space {
            self.toggle_play();
        }
        let position = self.status().position;
        if right {
            if ctrl {
                self.step(true, false);
            } else {
                self.seek_to(position + 5.0);
            }
        }
        if left {
            if ctrl {
                self.previous();
            } else {
                self.seek_to((position - 5.0).max(0.0));
            }
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.mpris.is_none() {
            self.mpris = Some(Mpris::start(ctx.clone()));
        }
        if let Some(receiver) = &self.scan
            && let Ok(library) = receiver.try_recv()
        {
            self.scan = None;
            self.library = Some(library);
            if let Some(path) = self.start_with.take()
                && let Some(index) = self
                    .library
                    .as_ref()
                    .and_then(|library| library.tracks.iter().position(|track| track.path == path))
            {
                let album = self.album_of(index);
                let tracks = album
                    .and_then(|album| {
                        self.library
                            .as_ref()
                            .map(|library| library.albums[album].tracks.clone())
                    })
                    .unwrap_or_else(|| vec![index]);
                let start = tracks.iter().position(|&track| track == index).unwrap_or(0);
                if let Some(album) = album {
                    self.page = Page::Album(album);
                }
                self.engine(&ctx);
                self.play_list(tracks, start);
            }
        }
        self.receive_covers(&ctx);
        self.handle_mpris(&ctx);
        let status = self.status();
        if status.finished != self.finished_seen {
            self.finished_seen = status.finished;
            self.step(true, true);
        }
        self.publish(&status);
        self.keys(&ctx);
        if status.playing {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        style::sidebar(ui, "musique-bibliotheque", SIDEBAR_WIDTH, side(), true, |ui| {
            self.sidebar(ui)
        });
        egui::Panel::bottom(ui.scope_id().with("musique-lecteur"))
            .resizable(false)
            .exact_size(PLAYER_HEIGHT)
            .frame(egui::Frame::new().fill(bar()))
            .show(ui, |ui| self.player(ui, &status));
        if self.show_queue && !self.queue.is_empty() && ui.available_width() > QUEUE_WIDTH + 520.0 {
            egui::Panel::right(ui.scope_id().with("musique-file"))
                .resizable(false)
                .exact_size(QUEUE_WIDTH)
                .frame(
                    egui::Frame::new()
                        .fill(side())
                        .inner_margin(egui::Margin::symmetric(14, 20)),
                )
                .show(ui, |ui| self.queue_panel(ui, &status));
        }
        let title = self.page_title();
        style::toolbar_with(ui, false, window(), line(), |ui| {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(title)
                    .font(FontId::new(15.0, style::strong_family()))
                    .color(style::primary_text()),
            );
        });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            ui.painter()
                .rect_filled(ui.max_rect(), CornerRadius::ZERO, window());
            self.content(ui, &status);
        });
    }

    fn page_title(&self) -> String {
        let library = self.library.as_ref();
        match self.page {
            Page::Home => "Écouter maintenant".to_owned(),
            Page::Albums => "Albums".to_owned(),
            Page::Tracks => "Titres".to_owned(),
            Page::Artists => "Artistes".to_owned(),
            Page::Album(index) => library
                .and_then(|library| library.albums.get(index))
                .map_or_else(String::new, |album| album.title.clone()),
            Page::Artist(index) => artists(library).get(index).cloned().unwrap_or_default(),
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(52.0);
            ui.label(
                egui::RichText::new("Musique")
                    .font(FontId::new(15.0, style::strong_family()))
                    .color(style::primary_text()),
            );
        });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.add_sized(
                vec2(SIDEBAR_WIDTH - 28.0, 32.0),
                egui::TextEdit::singleline(&mut self.search).hint_text("Titres, artistes, albums"),
            );
        });
        ui.add_space(12.0);
        let items = [
            (Glyph::Home, "Écouter maintenant", Page::Home),
            (Glyph::Album, "Albums", Page::Albums),
            (Glyph::Song, "Titres", Page::Tracks),
            (Glyph::Artist, "Artistes", Page::Artists),
        ];
        for (index, (glyph, label, page)) in items.into_iter().enumerate() {
            if index == 1 {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(22.0);
                    ui.label(
                        egui::RichText::new("Bibliothèque")
                            .size(12.0)
                            .strong()
                            .color(style::text_secondary()),
                    );
                });
                ui.add_space(4.0);
            }
            let selected = self.page == page
                || matches!(
                    (self.page, page),
                    (Page::Album(_), Page::Albums) | (Page::Artist(_), Page::Artists)
                );
            if side_item(ui, glyph, label, selected).clicked() {
                self.page = page;
            }
        }
        // Dossier lu, et de quoi le relire après y avoir ajouté des morceaux.
        let bottom = ui.max_rect().bottom() - 16.0;
        let rect = Rect::from_min_max(
            pos2(ui.max_rect().left() + 14.0, bottom - 30.0),
            pos2(ui.max_rect().right() - 14.0, bottom),
        );
        let response = ui.interact(rect, ui.scope_id().with("actualiser"), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 8.0, style::tint(10));
        }
        let text = if self.scan.is_some() {
            "Lecture du dossier…".to_owned()
        } else {
            format!("Actualiser · {}", home_relative(&self.folder))
        };
        text_line(
            ui,
            &text,
            pos2(rect.left() + 8.0, rect.center().y - 8.0),
            rect.width() - 16.0,
            12.0,
            false,
            style::text_secondary(),
        );
        if response.clicked() && self.scan.is_none() {
            self.rescan();
        }
    }

    fn content(&mut self, ui: &mut egui::Ui, status: &Status) {
        let Some(library) = &self.library else {
            centered_message(ui, "Lecture de la bibliothèque…", &home_relative(&self.folder));
            return;
        };
        if library.tracks.is_empty() {
            centered_message(
                ui,
                "Aucune musique",
                &format!(
                    "Ajoutez vos morceaux (MP3, FLAC, OGG, M4A, WAV) dans {}, puis Actualiser.",
                    home_relative(&self.folder)
                ),
            );
            return;
        }
        let query = self.search.trim().to_lowercase();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(28, 24))
                    .show(ui, |ui| match self.page {
                        Page::Home | Page::Albums => {
                            if self.page == Page::Home && query.is_empty() {
                                ui.label(
                                    egui::RichText::new(greeting())
                                        .font(FontId::new(22.0, style::strong_family()))
                                        .color(style::primary_text()),
                                );
                                ui.add_space(16.0);
                            }
                            let albums = self.matching_albums(&query, None);
                            self.album_grid(ui, &albums);
                        }
                        Page::Artist(artist) => {
                            let name = artists(self.library.as_ref()).get(artist).cloned();
                            let albums = self.matching_albums(&query, name.as_deref());
                            self.album_grid(ui, &albums);
                        }
                        Page::Album(index) => self.album_page(ui, index, &query, status),
                        Page::Tracks => {
                            let tracks = self.matching_tracks(&query, &self.album_order());
                            self.track_table(ui, &tracks, true, status);
                        }
                        Page::Artists => self.artist_list(ui, &query),
                    });
            });
    }

    fn matching_albums(&self, query: &str, artist: Option<&str>) -> Vec<usize> {
        let Some(library) = &self.library else {
            return Vec::new();
        };
        library
            .albums
            .iter()
            .enumerate()
            .filter(|(_, album)| artist.is_none_or(|artist| album.artist == artist))
            .filter(|(_, album)| {
                query.is_empty() || album_text(album, library).to_lowercase().contains(query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn matching_tracks(&self, query: &str, tracks: &[usize]) -> Vec<usize> {
        tracks
            .iter()
            .copied()
            .filter(|&index| {
                query.is_empty()
                    || self.track(index).is_some_and(|track| {
                        format!("{} {} {}", track.title, track.artist, track.album)
                            .to_lowercase()
                            .contains(query)
                    })
            })
            .collect()
    }

    fn album_grid(&mut self, ui: &mut egui::Ui, albums: &[usize]) {
        if albums.is_empty() {
            centered_message(
                ui,
                "Aucun résultat",
                "Essayez un autre titre ou un autre artiste.",
            );
            return;
        }
        let ctx = ui.ctx().clone();
        let width = ui.available_width();
        let gap = 20.0;
        let columns = ((width + gap) / (180.0 + gap)).floor().clamp(2.0, 6.0);
        let side = (width - gap * (columns - 1.0)) / columns;
        let mut open = None;
        for row in albums.chunks(columns as usize) {
            let (rect, _) = ui.allocate_exact_size(vec2(width, side + 52.0), Sense::hover());
            for (column, &index) in row.iter().enumerate() {
                let cell = Rect::from_min_size(
                    pos2(rect.left() + column as f32 * (side + gap), rect.top()),
                    vec2(side, side + 48.0),
                );
                let response = ui.interact(cell, ui.scope_id().with(("album", index)), Sense::click());
                let art = Rect::from_min_size(cell.min, vec2(side, side));
                let texture = self.cover(&ctx, index);
                paint_cover(ui.painter(), art, texture.as_ref(), 12.0);
                if response.hovered() {
                    ui.painter().rect_filled(art, 12.0, Color32::from_white_alpha(18));
                }
                let Some(album) = self
                    .library
                    .as_ref()
                    .and_then(|library| library.albums.get(index))
                else {
                    continue;
                };
                text_line(
                    ui,
                    &album.title,
                    pos2(cell.left(), art.bottom() + 10.0),
                    side,
                    14.0,
                    true,
                    style::primary_text(),
                );
                text_line(
                    ui,
                    &album.artist,
                    pos2(cell.left(), art.bottom() + 29.0),
                    side,
                    12.0,
                    false,
                    style::text_secondary(),
                );
                if response.clicked() {
                    open = Some(index);
                }
            }
            ui.add_space(18.0);
        }
        if let Some(index) = open {
            self.page = Page::Album(index);
        }
    }

    fn album_page(&mut self, ui: &mut egui::Ui, index: usize, query: &str, status: &Status) {
        let ctx = ui.ctx().clone();
        let Some(album) = self
            .library
            .as_ref()
            .and_then(|library| library.albums.get(index))
            .cloned()
        else {
            self.page = Page::Albums;
            return;
        };
        let total: f64 = album
            .tracks
            .iter()
            .filter_map(|&track| self.track(track))
            .map(|track| track.duration)
            .sum();
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 220.0), Sense::hover());
        let art = Rect::from_min_size(header.min, vec2(220.0, 220.0));
        let texture = self.cover(&ctx, index);
        paint_cover(ui.painter(), art, texture.as_ref(), 14.0);
        let x = art.right() + 28.0;
        let room = header.right() - x;
        let painter = ui.painter();
        let year = album.year.map(|year| format!(" · {year}")).unwrap_or_default();
        painter.text(
            pos2(x, header.top() + 70.0),
            Align2::LEFT_TOP,
            format!("Album{year}"),
            FontId::new(13.0, style::strong_family()),
            style::text_secondary(),
        );
        text_line(
            ui,
            &album.title,
            pos2(x, header.top() + 92.0),
            room,
            32.0,
            true,
            style::primary_text(),
        );
        let count = album.tracks.len();
        let plural = if count > 1 { "s" } else { "" };
        let minutes = (total / 60.0).round() as u64;
        text_line(
            ui,
            &format!("{} · {count} titre{plural} · {minutes} min", album.artist),
            pos2(x, header.top() + 138.0),
            room,
            14.0,
            false,
            style::text_secondary(),
        );
        let play = Rect::from_min_size(pos2(x, header.top() + 176.0), vec2(96.0, 38.0));
        let random = Rect::from_min_size(pos2(play.right() + 10.0, play.top()), vec2(112.0, 38.0));
        let play_response = ui.interact(play, ui.scope_id().with("lire"), Sense::click());
        let random_response = ui.interact(random, ui.scope_id().with("aleatoire"), Sense::click());
        let painter = ui.painter();
        painter.rect_filled(
            play,
            19.0,
            if play_response.hovered() {
                style::accent().gamma_multiply(0.9)
            } else {
                style::accent()
            },
        );
        paint_glyph(
            painter,
            pos2(play.left() + 26.0, play.center().y),
            Glyph::Play,
            Color32::WHITE,
            0.8,
        );
        painter.text(
            pos2(play.left() + 40.0, play.center().y),
            Align2::LEFT_CENTER,
            "Lire",
            FontId::new(14.0, style::strong_family()),
            Color32::WHITE,
        );
        painter.rect_filled(
            random,
            19.0,
            if random_response.hovered() {
                style::control_hover()
            } else {
                style::control_background()
            },
        );
        painter.text(
            random.center(),
            Align2::CENTER_CENTER,
            "Aléatoire",
            FontId::new(14.0, style::strong_family()),
            style::primary_text(),
        );
        if play_response.clicked() {
            self.shuffle = false;
            self.engine(&ctx);
            self.play_list(album.tracks.clone(), 0);
        }
        if random_response.clicked() {
            self.shuffle = true;
            self.engine(&ctx);
            let start = fastrand_index(album.tracks.len());
            self.play_list(album.tracks.clone(), start);
        }
        ui.add_space(28.0);
        let tracks = self.matching_tracks(query, &album.tracks);
        self.track_table(ui, &tracks, false, status);
    }

    /// Tableau des morceaux : numéro (ou égaliseur pour celui qui joue), titre et artiste,
    /// album (liste de tous les titres), durée. Un clic lance le morceau, la liste suit.
    fn track_table(&mut self, ui: &mut egui::Ui, tracks: &[usize], with_album: bool, status: &Status) {
        if tracks.is_empty() {
            centered_message(
                ui,
                "Aucun résultat",
                "Essayez un autre titre ou un autre artiste.",
            );
            return;
        }
        let ctx = ui.ctx().clone();
        let width = ui.available_width();
        let playing_track = self
            .current
            .and_then(|position| self.queue.get(position))
            .copied();
        let (header, _) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
        let painter = ui.painter();
        let small = FontId::new(12.0, style::strong_family());
        painter.text(
            pos2(header.left() + 14.0, header.center().y),
            Align2::LEFT_CENTER,
            "#",
            small.clone(),
            style::text_secondary(),
        );
        painter.text(
            pos2(header.left() + 54.0, header.center().y),
            Align2::LEFT_CENTER,
            "Titre",
            small.clone(),
            style::text_secondary(),
        );
        if with_album {
            painter.text(
                pos2(header.left() + width * 0.55, header.center().y),
                Align2::LEFT_CENTER,
                "Album",
                small.clone(),
                style::text_secondary(),
            );
        }
        painter.text(
            pos2(header.right() - 14.0, header.center().y),
            Align2::RIGHT_CENTER,
            "Durée",
            small,
            style::text_secondary(),
        );
        painter.hline(header.x_range(), header.bottom() - 0.5, Stroke::new(1.0, line()));
        let mut clicked = None;
        for (row, &index) in tracks.iter().enumerate() {
            let Some(track) = self.track(index).cloned() else {
                continue;
            };
            let (rect, response) = ui.allocate_exact_size(vec2(width, 52.0), Sense::click());
            let on = playing_track == Some(index);
            let painter = ui.painter();
            if on {
                painter.rect_filled(rect, 10.0, row_on());
            } else if response.hovered() {
                painter.rect_filled(rect, 10.0, style::tint(8));
            }
            let ink = if on {
                style::accent()
            } else {
                style::primary_text()
            };
            if on && status.playing {
                paint_equalizer(
                    painter,
                    pos2(rect.left() + 18.0, rect.center().y),
                    style::accent(),
                    ctx.input(|input| input.time),
                );
                // 30 images par seconde suffisent aux trois barres (et non une par
                // rafraîchissement de l'écran, 90 sur ce PC).
                ctx.request_repaint_after(std::time::Duration::from_millis(33));
            } else {
                let number = track
                    .number
                    .map_or_else(|| (row + 1).to_string(), |number| number.to_string());
                painter.text(
                    pos2(rect.left() + 14.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    number,
                    FontId::proportional(13.0),
                    if on {
                        style::accent()
                    } else {
                        style::text_secondary()
                    },
                );
            }
            let title_room = if with_album {
                width * 0.55 - 70.0
            } else {
                width - 140.0
            };
            text_line(
                ui,
                &track.title,
                pos2(rect.left() + 54.0, rect.top() + 8.0),
                title_room,
                14.0,
                on,
                ink,
            );
            text_line(
                ui,
                &track.artist,
                pos2(rect.left() + 54.0, rect.top() + 28.0),
                title_room,
                12.0,
                false,
                style::text_secondary(),
            );
            if with_album {
                text_line(
                    ui,
                    &track.album,
                    pos2(rect.left() + width * 0.55, rect.center().y - 8.0),
                    width * 0.3,
                    13.0,
                    false,
                    style::text_secondary(),
                );
            }
            ui.painter().text(
                pos2(rect.right() - 14.0, rect.center().y),
                Align2::RIGHT_CENTER,
                format_duration(track.duration),
                FontId::proportional(13.0),
                style::text_secondary(),
            );
            if response.clicked() {
                clicked = Some(row);
            }
        }
        if let Some(row) = clicked {
            self.engine(&ctx);
            self.play_list(tracks.to_vec(), row);
        }
    }

    fn artist_list(&mut self, ui: &mut egui::Ui, query: &str) {
        let names = artists(self.library.as_ref());
        let width = ui.available_width();
        let mut open = None;
        for (index, name) in names.iter().enumerate() {
            if !query.is_empty() && !name.to_lowercase().contains(query) {
                continue;
            }
            let albums = self.matching_albums("", Some(name)).len();
            let (rect, response) = ui.allocate_exact_size(vec2(width, 56.0), Sense::click());
            let painter = ui.painter();
            if response.hovered() {
                painter.rect_filled(rect, 10.0, style::tint(8));
            }
            let disc = pos2(rect.left() + 30.0, rect.center().y);
            painter.circle_filled(disc, 20.0, style::control_background());
            let initial: String = name
                .chars()
                .next()
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_default();
            painter.text(
                disc,
                Align2::CENTER_CENTER,
                initial,
                FontId::new(16.0, style::strong_family()),
                style::text_secondary(),
            );
            painter.text(
                pos2(rect.left() + 64.0, rect.center().y - 9.0),
                Align2::LEFT_CENTER,
                name,
                FontId::new(14.0, style::strong_family()),
                style::primary_text(),
            );
            let plural = if albums > 1 { "s" } else { "" };
            painter.text(
                pos2(rect.left() + 64.0, rect.center().y + 10.0),
                Align2::LEFT_CENTER,
                format!("{albums} album{plural}"),
                FontId::proportional(12.0),
                style::text_secondary(),
            );
            if response.clicked() {
                open = Some(index);
            }
        }
        if let Some(index) = open {
            self.page = Page::Artist(index);
        }
    }

    fn queue_panel(&mut self, ui: &mut egui::Ui, status: &Status) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        ui.painter().vline(
            full.left() - 14.0 + 0.5,
            full.y_range().expand(20.0),
            Stroke::new(1.0, line()),
        );
        ui.label(
            egui::RichText::new("À suivre")
                .font(FontId::new(15.0, style::strong_family()))
                .color(style::primary_text()),
        );
        ui.add_space(6.0);
        let Some(position) = self.current else { return };
        let count = self.queue.len();
        let mut pick = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for offset in 0..count.min(40) {
                    let slot = (position + offset) % count;
                    let index = self.queue[slot];
                    let Some(track) = self.track(index).cloned() else {
                        continue;
                    };
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
                    let on = offset == 0;
                    if on {
                        ui.painter().rect_filled(rect, 10.0, row_on());
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 10.0, style::tint(8));
                    }
                    let art =
                        Rect::from_min_size(pos2(rect.left() + 6.0, rect.top() + 6.0), vec2(40.0, 40.0));
                    let texture = self.album_of(index).and_then(|album| self.cover(&ctx, album));
                    paint_cover(ui.painter(), art, texture.as_ref(), 6.0);
                    let room = rect.width() - 110.0;
                    let ink = if on {
                        style::accent()
                    } else {
                        style::primary_text()
                    };
                    text_line(
                        ui,
                        &track.title,
                        pos2(art.right() + 10.0, rect.top() + 9.0),
                        room,
                        13.0,
                        on,
                        ink,
                    );
                    text_line(
                        ui,
                        &track.artist,
                        pos2(art.right() + 10.0, rect.top() + 28.0),
                        room,
                        12.0,
                        false,
                        style::text_secondary(),
                    );
                    let duration = if on && status.duration > 0.0 {
                        status.duration
                    } else {
                        track.duration
                    };
                    ui.painter().text(
                        pos2(rect.right() - 8.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        format_duration(duration),
                        FontId::proportional(12.0),
                        style::text_secondary(),
                    );
                    if response.clicked() {
                        pick = Some(slot);
                    }
                }
            });
        if let Some(slot) = pick {
            self.current = Some(slot);
            self.open_current(true);
        }
    }

    fn player(&mut self, ui: &mut egui::Ui, status: &Status) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        ui.painter()
            .hline(full.x_range(), full.top() + 0.5, Stroke::new(1.0, line()));
        let side_width = 340.0f32.min(full.width() * 0.28);
        let left = Rect::from_min_max(
            pos2(full.left() + 20.0, full.top()),
            pos2(full.left() + 20.0 + side_width, full.bottom()),
        );
        let right = Rect::from_min_max(
            pos2(full.right() - 20.0 - side_width, full.top()),
            pos2(full.right() - 20.0, full.bottom()),
        );
        let middle = Rect::from_min_max(
            pos2(left.right() + 20.0, full.top()),
            pos2(right.left() - 20.0, full.bottom()),
        );

        // À gauche : pochette, titre, artiste — album.
        let current = self
            .current
            .and_then(|position| self.queue.get(position))
            .copied();
        if let Some(index) = current
            && let Some(track) = self.track(index).cloned()
        {
            let art = Rect::from_min_size(pos2(left.left(), left.center().y - 28.0), vec2(56.0, 56.0));
            let texture = self.album_of(index).and_then(|album| self.cover(&ctx, album));
            paint_cover(ui.painter(), art, texture.as_ref(), 8.0);
            let room = left.right() - art.right() - 14.0;
            text_line(
                ui,
                &track.title,
                pos2(art.right() + 14.0, left.center().y - 19.0),
                room,
                14.0,
                true,
                style::primary_text(),
            );
            text_line(
                ui,
                &format!("{} — {}", track.artist, track.album),
                pos2(art.right() + 14.0, left.center().y + 2.0),
                room,
                12.0,
                false,
                style::text_secondary(),
            );
        }

        // Au centre : commandes, puis temps et barre de progression.
        let row_y = middle.top() + 28.0;
        let center_x = middle.center().x;
        let buttons = [
            (-112.0, Glyph::Shuffle, self.shuffle, "Lecture aléatoire"),
            (-56.0, Glyph::Previous, false, "Précédent (Ctrl+←)"),
            (56.0, Glyph::Next, false, "Suivant (Ctrl+→)"),
            (112.0, Glyph::Repeat, self.repeat != Repeat::Off, "Répéter"),
        ];
        for (offset, glyph, active, hint) in buttons {
            let rect = Rect::from_center_size(pos2(center_x + offset, row_y), vec2(36.0, 36.0));
            let response = ui
                .interact(rect, ui.scope_id().with(hint), Sense::click())
                .on_hover_text(hint);
            if response.hovered() {
                ui.painter().rect_filled(rect, 10.0, style::tint(10));
            }
            let ink = if active {
                style::accent()
            } else {
                style::primary_text()
            };
            paint_glyph(ui.painter(), rect.center(), glyph, ink, 1.0);
            if glyph == Glyph::Repeat && self.repeat == Repeat::One {
                ui.painter().text(
                    rect.center() + vec2(9.0, 8.0),
                    Align2::CENTER_CENTER,
                    "1",
                    FontId::new(9.0, style::strong_family()),
                    style::accent(),
                );
            }
            if response.clicked() {
                match glyph {
                    Glyph::Shuffle => self.toggle_shuffle(),
                    Glyph::Previous => self.previous(),
                    Glyph::Next => self.step(true, false),
                    _ => {
                        self.repeat = match self.repeat {
                            Repeat::Off => Repeat::All,
                            Repeat::All => Repeat::One,
                            Repeat::One => Repeat::Off,
                        };
                    }
                }
            }
        }
        let play = Rect::from_center_size(pos2(center_x, row_y), vec2(40.0, 40.0));
        let response = ui
            .interact(play, ui.scope_id().with("lecture"), Sense::click())
            .on_hover_text("Lecture ou pause (Espace)");
        let (play_fill, play_ink) = (style::inverse(), style::inverse_text());
        ui.painter().circle_filled(
            play.center(),
            20.0,
            if response.hovered() {
                play_fill.gamma_multiply(0.85)
            } else {
                play_fill
            },
        );
        paint_glyph(
            ui.painter(),
            play.center(),
            if status.playing { Glyph::Pause } else { Glyph::Play },
            play_ink,
            1.0,
        );
        if response.clicked() {
            self.engine(&ctx);
            self.toggle_play();
        }

        let duration = if status.duration > 0.0 {
            status.duration
        } else {
            current
                .and_then(|index| self.track(index))
                .map_or(0.0, |track| track.duration)
        };
        let position = self.seeking.unwrap_or(status.position).min(duration.max(0.0));
        let bar_y = middle.top() + 66.0;
        let time_font = FontId::proportional(11.0);
        ui.painter().text(
            pos2(middle.left() + 36.0, bar_y),
            Align2::RIGHT_CENTER,
            format_duration(position),
            time_font.clone(),
            style::text_secondary(),
        );
        ui.painter().text(
            pos2(middle.right() - 36.0, bar_y),
            Align2::LEFT_CENTER,
            format_duration(duration),
            time_font,
            style::text_secondary(),
        );
        let track_rect = Rect::from_min_max(
            pos2(middle.left() + 46.0, bar_y - 8.0),
            pos2(middle.right() - 46.0, bar_y + 8.0),
        );
        let fraction = if duration > 0.0 {
            (position / duration) as f32
        } else {
            0.0
        };
        if let Some(fraction) = slider(ui, track_rect, fraction, "progression") {
            self.seeking = Some(f64::from(fraction) * duration);
        } else if let Some(target) = self.seeking.take() {
            self.seek_to(target);
        }

        // À droite : file d'attente et volume.
        let queue = Rect::from_center_size(pos2(right.right() - 170.0, full.center().y), vec2(36.0, 36.0));
        let response = ui
            .interact(queue, ui.scope_id().with("file"), Sense::click())
            .on_hover_text("File d’attente");
        if self.show_queue {
            ui.painter().rect_filled(queue, 10.0, style::sidebar_selected());
        } else if response.hovered() {
            ui.painter().rect_filled(queue, 10.0, style::tint(10));
        }
        let ink = if self.show_queue {
            style::sidebar_selected_text()
        } else {
            style::primary_text()
        };
        paint_glyph(ui.painter(), queue.center(), Glyph::Queue, ink, 1.0);
        if response.clicked() {
            self.show_queue = !self.show_queue;
        }
        paint_glyph(
            ui.painter(),
            pos2(right.right() - 128.0, full.center().y),
            Glyph::Volume,
            style::primary_text(),
            1.0,
        );
        let volume_rect = Rect::from_min_max(
            pos2(right.right() - 110.0, full.center().y - 8.0),
            pos2(right.right(), full.center().y + 8.0),
        );
        if let Some(level) = slider(ui, volume_rect, self.volume, "volume") {
            self.volume = level;
            self.engine(&ctx).send(EngineCommand::Volume(level));
        }

        if let Some(error) = &status.error {
            ui.painter().text(
                pos2(middle.center().x, full.bottom() - 6.0),
                Align2::CENTER_BOTTOM,
                error,
                FontId::proportional(11.0),
                style::error(),
            );
        }
    }

    fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        // La file se mélange (ou se remet dans l'ordre de l'album) autour du morceau en cours.
        let Some(position) = self.current else { return };
        let playing = self.queue[position];
        if self.shuffle {
            shuffle(&mut self.queue, playing);
        } else if let Some(album) = self.album_of(playing)
            && let Some(tracks) = self
                .library
                .as_ref()
                .map(|library| library.albums[album].tracks.clone())
        {
            self.queue = tracks;
        }
        self.current = self.queue.iter().position(|&track| track == playing).or(Some(0));
    }
}

/// Mélange `tracks` en gardant `first` en tête.
fn shuffle(tracks: &mut [usize], first: usize) {
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |elapsed| elapsed.as_nanos() as u64)
        | 1;
    for index in (1..tracks.len()).rev() {
        // xorshift : assez pour mélanger une liste de morceaux.
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        tracks.swap(index, (seed % (index as u64 + 1)) as usize);
    }
    if let Some(at) = tracks.iter().position(|&track| track == first) {
        tracks.swap(0, at);
    }
}

fn fastrand_index(count: usize) -> usize {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos() as usize);
    nanos % count.max(1)
}

/// Artistes de la bibliothèque, par ordre alphabétique.
fn artists(library: Option<&Library>) -> Vec<String> {
    let mut names: Vec<String> = library
        .map(|library| library.albums.iter().map(|album| album.artist.clone()).collect())
        .unwrap_or_default();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names
}

fn album_text(album: &Album, library: &Library) -> String {
    let titles: Vec<&str> = album
        .tracks
        .iter()
        .filter_map(|&index| library.tracks.get(index))
        .map(|track| track.title.as_str())
        .collect();
    format!("{} {} {}", album.title, album.artist, titles.join(" "))
}

fn greeting() -> &'static str {
    let hour = ferrite_system::time::local_hour().unwrap_or(12);
    match hour {
        5..=11 => "Bonjour",
        12..=17 => "Bon après-midi",
        _ => "Bonsoir",
    }
}

/// Pochette décodée et réduite ; pour une pochette intégrée, son fichier en cache (pour MPRIS).
fn load_cover(cover: &Cover, index: usize) -> Option<(egui::ColorImage, Option<PathBuf>)> {
    let (data, file) = match cover {
        Cover::File(path) => (fs::read(path).ok()?, Some(path.clone())),
        Cover::Embedded(track) => {
            let data = library::embedded_cover(track)?;
            let extension = if data.starts_with(b"\x89PNG") {
                "png"
            } else {
                "jpg"
            };
            let cache = env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".cache/ferrite/musique"));
            let file = cache.and_then(|cache| {
                fs::create_dir_all(&cache).ok()?;
                let file = cache.join(format!("pochette-{index}-{}.{extension}", data.len()));
                fs::write(&file, &data).ok()?;
                Some(file)
            });
            (data, file)
        }
    };
    let image = wallpaper::decode_bytes(data).ok()?;
    let image = crate::viewer::shrink(&image, COVER_SIDE);
    Some((
        egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.rgba),
        file,
    ))
}

fn home_relative(path: &Path) -> String {
    match env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) => {
            format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display())
        }
        _ => path.display().to_string(),
    }
}

/// Couleurs des planches, prises dans les rôles communs de `style`.
fn side() -> Color32 {
    style::sidebar_color()
}

fn bar() -> Color32 {
    style::header_color()
}

fn window() -> Color32 {
    style::background()
}

fn line() -> Color32 {
    style::line()
}

fn row_on() -> Color32 {
    style::sidebar_selected()
}

fn track_color() -> Color32 {
    style::track()
}

fn fill_color() -> Color32 {
    style::primary_text()
}

fn centered_message(ui: &mut egui::Ui, title: &str, text: &str) {
    let rect = ui.max_rect();
    let center = pos2(rect.center().x, rect.top() + rect.height().min(600.0) / 2.0);
    ui.painter().text(
        center - vec2(0.0, 12.0),
        Align2::CENTER_CENTER,
        title,
        FontId::new(17.0, style::strong_family()),
        style::primary_text(),
    );
    ui.painter().text(
        center + vec2(0.0, 14.0),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(13.0),
        style::text_secondary(),
    );
}

/// Texte sur une ligne, coupé d'une ellipse s'il dépasse `width`.
fn text_line(ui: &egui::Ui, text: &str, at: Pos2, width: f32, size: f32, strong: bool, color: Color32) {
    let font = if strong {
        FontId::new(size, style::strong_family())
    } else {
        FontId::proportional(size)
    };
    let mut job =
        egui::text::LayoutJob::single_section(text.to_owned(), egui::TextFormat::simple(font, color));
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(10.0));
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(at, galley, color);
}

/// Pochette (ou carré teinté avec une note de musique) aux coins arrondis.
fn paint_cover(painter: &egui::Painter, rect: Rect, texture: Option<&TextureHandle>, radius: f32) {
    match texture {
        Some(texture) => {
            let [w, h] = texture.size().map(|side| side as f32);
            let (u, v) = if w > h { (h / w, 1.0) } else { (1.0, w / h) };
            let uv = Rect::from_center_size(pos2(0.5, 0.5), vec2(u, v));
            painter.add(
                egui::epaint::RectShape::filled(rect, radius, Color32::WHITE).with_texture(texture.id(), uv),
            );
        }
        None => {
            painter.rect_filled(rect, radius, style::control_background());
            paint_glyph(
                painter,
                rect.center(),
                Glyph::Song,
                style::text_secondary(),
                (rect.width() / 56.0).clamp(0.6, 3.0),
            );
        }
    }
}

/// Curseur horizontal : renvoie la nouvelle valeur (0 à 1) pendant qu'on le clique ou le glisse.
fn slider(ui: &mut egui::Ui, rect: Rect, value: f32, salt: &str) -> Option<f32> {
    let response = ui.interact(rect, ui.scope_id().with(salt), Sense::click_and_drag());
    let line_rect = Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0));
    let painter = ui.painter();
    painter.rect_filled(line_rect, 2.0, track_color());
    let filled = Rect::from_min_size(
        line_rect.min,
        vec2(line_rect.width() * value.clamp(0.0, 1.0), 4.0),
    );
    painter.rect_filled(filled, 2.0, fill_color());
    if response.hovered() || response.dragged() {
        painter.circle_filled(pos2(filled.right(), line_rect.center().y), 6.0, fill_color());
    }
    (response.dragged() || response.clicked())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|pointer| ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0))
}

fn side_item(ui: &mut egui::Ui, glyph: Glyph, label: &str, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
    let rect = rect.shrink2(vec2(10.0, 1.0));
    let painter = ui.painter();
    let (fill, ink, icon) = if selected {
        (
            style::sidebar_selected(),
            style::sidebar_selected_text(),
            style::sidebar_selected_text(),
        )
    } else if response.hovered() {
        (style::tint(10), style::primary_text(), style::text_secondary())
    } else {
        (
            Color32::TRANSPARENT,
            style::primary_text(),
            style::text_secondary(),
        )
    };
    painter.rect_filled(rect, 8.0, fill);
    paint_glyph(
        painter,
        pos2(rect.left() + 20.0, rect.center().y),
        glyph,
        icon,
        0.85,
    );
    let font = if selected {
        FontId::new(14.0, style::strong_family())
    } else {
        FontId::proportional(14.0)
    };
    painter.text(
        pos2(rect.left() + 40.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        ink,
    );
    response
}

/// Trois barres qui dansent devant le morceau en cours.
fn paint_equalizer(painter: &egui::Painter, at: Pos2, color: Color32, time: f64) {
    for (bar, phase) in [0.0f64, 0.8, 1.6].into_iter().enumerate() {
        let height = 4.0 + 10.0 * (0.5 + 0.5 * (time * 6.0 + phase).sin()) as f32;
        let x = at.x - 6.0 + bar as f32 * 5.0;
        painter.rect_filled(
            Rect::from_min_max(pos2(x, at.y + 7.0 - height), pos2(x + 3.0, at.y + 7.0)),
            1.0,
            color,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Glyph {
    Home,
    Album,
    Song,
    Artist,
    Play,
    Pause,
    Previous,
    Next,
    Shuffle,
    Repeat,
    Queue,
    Volume,
}

/// Symboles des planches (tracés de 24 points), à l'échelle `scale`.
fn paint_glyph(painter: &egui::Painter, center: Pos2, glyph: Glyph, color: Color32, scale: f32) {
    let p = |x: f32, y: f32| {
        pos2(
            center.x + (x - 12.0) * 0.75 * scale,
            center.y + (y - 12.0) * 0.75 * scale,
        )
    };
    let stroke = Stroke::new(1.8 * scale.max(0.8), color);
    let circle = |c: Pos2, r: f32| painter.circle_stroke(c, r * 0.75 * scale, stroke);
    match glyph {
        Glyph::Home => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(4.0, 11.0),
                    p(12.0, 4.0),
                    p(20.0, 11.0),
                    p(20.0, 20.0),
                    p(4.0, 20.0),
                ],
                stroke,
            ));
        }
        Glyph::Album => {
            circle(p(12.0, 12.0), 9.0);
            circle(p(12.0, 12.0), 3.0);
        }
        Glyph::Song => {
            painter.line(
                vec![p(9.0, 18.0), p(9.0, 6.0), p(19.0, 4.0), p(19.0, 16.0)],
                stroke,
            );
            circle(p(6.5, 18.0), 2.5);
            circle(p(16.5, 16.0), 2.5);
        }
        Glyph::Artist => {
            circle(p(12.0, 8.0), 4.0);
            painter.line(
                (0..=12)
                    .map(|step| {
                        let angle = std::f32::consts::PI * (1.0 + step as f32 / 12.0);
                        p(12.0 + 8.0 * angle.cos(), 21.0 + 8.0 * angle.sin())
                    })
                    .collect(),
                stroke,
            );
        }
        Glyph::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![p(8.0, 5.0), p(19.0, 12.0), p(8.0, 19.0)],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Pause => {
            painter.rect_filled(Rect::from_min_max(p(6.0, 5.0), p(10.0, 19.0)), 1.2, color);
            painter.rect_filled(Rect::from_min_max(p(14.0, 5.0), p(18.0, 19.0)), 1.2, color);
        }
        Glyph::Previous => {
            painter.line_segment([p(5.0, 5.0), p(5.0, 19.0)], stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![p(19.0, 5.0), p(9.0, 12.0), p(19.0, 19.0)],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Next => {
            painter.line_segment([p(19.0, 5.0), p(19.0, 19.0)], stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![p(5.0, 5.0), p(15.0, 12.0), p(5.0, 19.0)],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Shuffle => {
            painter.line(
                vec![p(3.0, 7.0), p(7.0, 7.0), p(15.0, 17.0), p(21.0, 17.0)],
                stroke,
            );
            painter.line(vec![p(3.0, 17.0), p(7.0, 17.0), p(9.0, 14.5)], stroke);
            painter.line(vec![p(13.0, 9.5), p(15.0, 7.0), p(21.0, 7.0)], stroke);
            painter.line(vec![p(18.0, 4.0), p(21.0, 7.0), p(18.0, 10.0)], stroke);
            painter.line(vec![p(18.0, 14.0), p(21.0, 17.0), p(18.0, 20.0)], stroke);
        }
        Glyph::Repeat => {
            painter.line(vec![p(4.0, 11.0), p(4.0, 9.0), p(7.0, 6.0), p(19.0, 6.0)], stroke);
            painter.line(vec![p(16.0, 3.0), p(19.0, 6.0), p(16.0, 9.0)], stroke);
            painter.line(
                vec![p(20.0, 13.0), p(20.0, 15.0), p(17.0, 18.0), p(5.0, 18.0)],
                stroke,
            );
            painter.line(vec![p(8.0, 21.0), p(5.0, 18.0), p(8.0, 15.0)], stroke);
        }
        Glyph::Queue => {
            painter.line_segment([p(4.0, 6.0), p(16.0, 6.0)], stroke);
            painter.line_segment([p(4.0, 12.0), p(16.0, 12.0)], stroke);
            painter.line_segment([p(4.0, 18.0), p(11.0, 18.0)], stroke);
            painter.add(egui::Shape::closed_line(
                vec![p(15.0, 15.0), p(20.0, 18.0), p(15.0, 21.0)],
                stroke,
            ));
        }
        Glyph::Volume => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    p(4.0, 9.0),
                    p(8.0, 9.0),
                    p(13.0, 5.0),
                    p(13.0, 19.0),
                    p(8.0, 15.0),
                    p(4.0, 15.0),
                ],
                color,
                Stroke::NONE,
            ));
            painter.line(
                (0..=8)
                    .map(|step| {
                        let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * step as f32 / 8.0;
                        p(14.5 + 3.0 * angle.cos(), 12.0 + 3.0 * angle.sin())
                    })
                    .collect(),
                stroke,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn melange_garde_le_premier() {
        let mut tracks: Vec<usize> = (0..20).collect();
        shuffle(&mut tracks, 7);
        assert_eq!(tracks[0], 7);
        let mut sorted = tracks.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..20).collect::<Vec<_>>());
    }

    #[test]
    fn artistes_tries_sans_doublon() {
        let album = |artist: &str| Album {
            title: "A".into(),
            artist: artist.into(),
            year: None,
            tracks: Vec::new(),
            cover: None,
        };
        let library = Library {
            albums: vec![album("zoé"), album("Ali"), album("zoé")],
            ..Library::default()
        };
        assert_eq!(artists(Some(&library)), ["Ali", "zoé"]);
    }
}
