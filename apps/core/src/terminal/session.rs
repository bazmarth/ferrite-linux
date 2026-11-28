//! Une session : un programme (le shell) dans un PTY, et l'émulation de son écran.

use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
};

use alacritty_terminal::{
    event::{Event, EventListener, WindowSize},
    event_loop::{EventLoop, EventLoopSender, Msg},
    grid::{Dimensions, Scroll},
    index::{Boundary, Column, Direction, Point, Side},
    selection::{Selection, SelectionType},
    sync::FairMutex,
    term::{self, Config, Term, TermMode, search::RegexSearch},
    tty,
    vte::ansi::{CursorShape, CursorStyle},
};
use eframe::egui;

use super::{colors, keys};

/// Lignes gardées en mémoire au-dessus de l'écran.
const SCROLLBACK: usize = 20_000;

/// Programme à lancer ; rien : le shell de l'utilisateur.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    pub directory: Option<PathBuf>,
    pub command: Option<(String, Vec<String>)>,
}

/// Interpréteur ouvert quand aucun programme n'est demandé : celui de l'utilisateur
/// (`$SHELL`), sauf sh, bash ou dash, remplacés par fish quand il est installé
/// (complétion par Tab, suggestions au fil de la frappe, couleurs). Même règle que
/// `ferrite-session` ; rien : celui du compte (choix d'alacritty).
fn default_shell(shell: Option<&str>, fish: &str, fish_installed: bool) -> Option<String> {
    let plain = match shell {
        None => true,
        Some(path) => {
            let name = path.rsplit('/').next().unwrap_or(path);
            path.is_empty() || matches!(name, "sh" | "bash" | "dash")
        }
    };
    (plain && fish_installed).then(|| fish.to_owned())
}

/// Taille de la grille, en cellules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub columns: usize,
    pub lines: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// Transmet les événements de l'émulation et réveille l'interface.
#[derive(Clone)]
struct Notifier {
    events: Sender<Event>,
    context: egui::Context,
}

impl EventListener for Notifier {
    fn send_event(&self, event: Event) {
        let _ = self.events.send(event);
        self.context.request_repaint();
    }
}

/// Dimensions d'une cellule, en points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub width: f32,
    pub height: f32,
}

pub struct Session {
    term: Arc<FairMutex<Term<Notifier>>>,
    sender: EventLoopSender,
    events: Receiver<Event>,
    /// Programme lancé, pour retrouver son dossier courant.
    pid: u32,
    size: GridSize,
    pixels: (u16, u16),
    pub title: Option<String>,
    pub exited: bool,
    /// Texte que le programme demande de copier dans le presse-papiers.
    pub copied: Option<String>,
    /// Dernière occurrence trouvée par la recherche.
    found: Option<Point>,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}

fn window_size(size: GridSize, pixels: (u16, u16)) -> WindowSize {
    WindowSize {
        num_lines: size.lines as u16,
        num_cols: size.columns as u16,
        cell_width: pixels.0,
        cell_height: pixels.1,
    }
}

impl Session {
    pub fn spawn(
        context: &egui::Context,
        launch: &Launch,
        size: GridSize,
        pixels: (u16, u16),
    ) -> io::Result<Self> {
        let (sender, events) = mpsc::channel();
        let notifier = Notifier {
            events: sender,
            context: context.clone(),
        };
        let config = Config {
            scrolling_history: SCROLLBACK,
            default_cursor_style: CursorStyle {
                shape: CursorShape::Beam,
                blinking: true,
            },
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, notifier.clone())));
        let options = tty::Options {
            shell: launch
                .command
                .clone()
                .or_else(|| {
                    const FISH: &str = "/usr/bin/fish";
                    let shell = std::env::var("SHELL").ok();
                    default_shell(shell.as_deref(), FISH, std::path::Path::new(FISH).exists())
                        .map(|fish| (fish, Vec::new()))
                })
                .map(|(program, arguments)| tty::Shell::new(program, arguments)),
            working_directory: launch.directory.clone(),
            drain_on_exit: true,
            env: HashMap::from([
                ("TERM".to_owned(), "xterm-256color".to_owned()),
                ("COLORTERM".to_owned(), "truecolor".to_owned()),
                ("TERM_PROGRAM".to_owned(), "Ferrite".to_owned()),
            ]),
        };
        let pty = tty::new(&options, window_size(size, pixels), 0)?;
        let pid = pty.child().id();
        let event_loop = EventLoop::new(term.clone(), notifier, pty, true, false)?;
        let channel = event_loop.channel();
        event_loop.spawn();
        Ok(Self {
            term,
            sender: channel,
            events,
            pid,
            size,
            pixels,
            title: None,
            exited: false,
            copied: None,
            found: None,
        })
    }

    /// Traite ce que l'émulation a signalé depuis la dernière image.
    pub fn poll(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Title(title) => self.title = Some(title),
                Event::ResetTitle => self.title = None,
                Event::PtyWrite(text) => self.write(text.as_bytes()),
                Event::ClipboardStore(_, text) => self.copied = Some(text),
                Event::ColorRequest(index, format) => {
                    let color = colors::indexed(index, self.term.lock().colors());
                    self.write(format(color).as_bytes());
                }
                Event::TextAreaSizeRequest(format) => {
                    let answer = format(window_size(self.size, self.pixels));
                    self.write(answer.as_bytes());
                }
                Event::Exit | Event::ChildExit(_) => self.exited = true,
                _ => {}
            }
        }
    }

    pub fn write(&self, bytes: &[u8]) {
        let _ = self.sender.send(Msg::Input(bytes.to_vec().into()));
    }

    pub fn size(&self) -> GridSize {
        self.size
    }

    pub fn resize(&mut self, size: GridSize, pixels: (u16, u16)) {
        if (size, pixels) == (self.size, self.pixels) {
            return;
        }
        self.size = size;
        self.pixels = pixels;
        self.term.lock().resize(size);
        let _ = self.sender.send(Msg::Resize(window_size(size, pixels)));
    }

    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// Dossier courant du programme, pour y ouvrir le prochain onglet ou la prochaine fenêtre.
    pub fn directory(&self) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{}/cwd", self.pid)).ok()
    }

    pub fn display_offset(&self) -> usize {
        self.term.lock().grid().display_offset()
    }

    pub fn history(&self) -> usize {
        self.term.lock().grid().history_size()
    }

    pub fn scroll(&self, scroll: Scroll) {
        self.term.lock().scroll_display(scroll);
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    pub fn selected_text(&self) -> Option<String> {
        self.term
            .lock()
            .selection_to_string()
            .filter(|text| !text.is_empty())
    }

    /// Commence une sélection à la cellule de l'écran `(colonne, ligne)`.
    pub fn select_from(&self, cell: (usize, usize), side: Side, clicks: u8) {
        let mut term = self.term.lock();
        let point = term::viewport_to_point(term.grid().display_offset(), viewport_point(cell));
        let kind = match clicks {
            0 | 1 => SelectionType::Simple,
            2 => SelectionType::Semantic,
            _ => SelectionType::Lines,
        };
        term.selection = Some(Selection::new(kind, point, side));
    }

    pub fn select_to(&self, cell: (usize, usize), side: Side) {
        let mut term = self.term.lock();
        let point = term::viewport_to_point(term.grid().display_offset(), viewport_point(cell));
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    /// Adresse web sous la cellule : lien déclaré par le programme, sinon repérée dans le texte.
    pub fn url_at(&self, cell: (usize, usize)) -> Option<String> {
        let term = self.term.lock();
        let point = term::viewport_to_point(term.grid().display_offset(), viewport_point(cell));
        if let Some(link) = term.grid()[point].hyperlink() {
            return Some(link.uri().to_owned());
        }
        let end = Point::new(point.line, Column(term.columns() - 1));
        let text = term.bounds_to_string(Point::new(point.line, Column(0)), end);
        keys::url_at(&text, cell.0)
    }

    /// Cherche `text` à partir de la dernière occurrence, et l'amène à l'écran sélectionnée.
    /// Vers le haut, on remonte vers les lignes les plus anciennes.
    pub fn search(&mut self, text: &str, upward: bool) -> bool {
        if text.is_empty() {
            return true;
        }
        let Ok(mut regex) = RegexSearch::new(&keys::escape_regex(text)) else {
            return false;
        };
        let mut term = self.term.lock();
        let direction = if upward { Direction::Left } else { Direction::Right };
        let origin = self.found.unwrap_or_else(|| term.grid().cursor.point);
        let origin = match direction {
            Direction::Left => origin.sub(&*term, Boundary::None, 1),
            Direction::Right => origin.add(&*term, Boundary::None, 1),
        };
        let side = match direction {
            Direction::Left => Side::Right,
            Direction::Right => Side::Left,
        };
        let Some(found) = term.search_next(&mut regex, origin, direction, side, None) else {
            term.selection = None;
            drop(term);
            self.found = None;
            return false;
        };
        let (start, end) = (*found.start(), *found.end());
        term.scroll_to_point(start);
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);
        drop(term);
        self.found = Some(start);
        true
    }

    pub fn reset_search(&mut self) {
        self.found = None;
    }

    /// Dessine l'écran à partir de `origin`.
    pub fn paint(
        &self,
        painter: &egui::Painter,
        origin: egui::Pos2,
        cell: Cell,
        font_size: f32,
        focused: bool,
        cursor_visible: bool,
    ) {
        let term = self.term.lock();
        let content = term.renderable_content();
        let offset = content.display_offset;
        let palette = content.colors;
        let default_background = colors::background();
        let selection_color = crate::style::accent().lerp_to_gamma(default_background, 0.55);
        let cursor_color = colors::resolve(
            alacritty_terminal::vte::ansi::Color::Named(alacritty_terminal::vte::ansi::NamedColor::Cursor),
            palette,
        );
        let cursor = term::point_to_viewport(offset, content.cursor.point)
            .filter(|_| cursor_visible || !focused)
            .map(|point| (point.line, point.column.0, content.cursor.shape));
        let block_cursor = focused && matches!(cursor, Some((_, _, CursorShape::Block)));

        let mut backgrounds: Vec<(usize, usize, usize, egui::Color32)> = Vec::new();
        let mut runs: Vec<Run> = Vec::new();
        for indexed in content.display_iter {
            let Some(point) = term::point_to_viewport(offset, indexed.point) else {
                continue;
            };
            let (line, column) = (point.line, point.column.0);
            let flags = indexed.cell.flags;
            use alacritty_terminal::term::cell::Flags;
            if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let width = if flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
            let mut foreground = colors::resolve(indexed.cell.fg, palette);
            let mut background = colors::resolve(indexed.cell.bg, palette);
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }
            if flags.contains(Flags::DIM) {
                foreground = foreground.lerp_to_gamma(background, 0.35);
            }
            if content
                .selection
                .is_some_and(|range| range.contains(indexed.point))
            {
                background = selection_color;
            }
            if block_cursor && cursor.is_some_and(|(l, c, _)| (l, c) == (line, column)) {
                foreground = background;
                background = cursor_color;
            }
            if background != default_background {
                match backgrounds.last_mut() {
                    Some((l, _, end, color)) if *l == line && *end == column && *color == background => {
                        *end = column + width;
                    }
                    _ => backgrounds.push((line, column, column + width, background)),
                }
            }
            let character = indexed.cell.c;
            let decorated = flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT);
            if flags.contains(Flags::HIDDEN) || (character == ' ' && !decorated) {
                continue;
            }
            let style = RunStyle {
                color: foreground,
                bold: flags.intersects(Flags::BOLD),
                italic: flags.intersects(Flags::ITALIC),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                strike: flags.contains(Flags::STRIKEOUT),
            };
            // Ce qui n'est pas dans la police à chasse fixe peut venir d'une police plus large :
            // ces caractères sont posés un à un, à leur place.
            let simple = character.is_ascii() && width == 1 && indexed.cell.zerowidth().is_none();
            if simple
                && let Some(run) = runs.last_mut()
                && run.simple
                && run.line == line
                && run.style == style
                && run.column + run.length == column
            {
                run.text.push(character);
                run.length += 1;
                continue;
            }
            let mut text = String::from(character);
            if let Some(combining) = indexed.cell.zerowidth() {
                text.extend(combining);
            }
            runs.push(Run {
                line,
                column,
                length: width,
                text,
                style,
                simple,
            });
        }

        let at = |column: usize, line: usize| {
            origin + egui::vec2(column as f32 * cell.width, line as f32 * cell.height)
        };
        for (line, start, end, color) in backgrounds {
            let rect = egui::Rect::from_min_max(at(start, line), at(end, line + 1));
            painter.rect_filled(rect, 0.0, color);
        }
        for run in runs {
            let family = if run.style.bold {
                crate::style::mono_bold_family()
            } else {
                egui::FontFamily::Monospace
            };
            let mut format = egui::TextFormat::simple(egui::FontId::new(font_size, family), run.style.color);
            format.italics = run.style.italic;
            if run.style.underline {
                format.underline = egui::Stroke::new(1.0, run.style.color);
            }
            if run.style.strike {
                format.strikethrough = egui::Stroke::new(1.0, run.style.color);
            }
            let mut job = egui::text::LayoutJob::single_section(run.text, format);
            job.wrap.max_width = f32::INFINITY;
            let galley = painter.ctx().fonts_mut(|fonts| fonts.layout_job(job));
            let mut position = at(run.column, run.line);
            if !run.simple {
                // Centré dans ses cellules.
                let room = run.length as f32 * cell.width;
                position.x += ((room - galley.size().x) / 2.0).max(0.0);
            }
            painter.galley(position, galley, run.style.color);
        }

        match cursor {
            Some((line, column, _)) if !focused => {
                let rect = egui::Rect::from_min_size(at(column, line), egui::vec2(cell.width, cell.height));
                painter.rect_stroke(
                    rect.shrink(0.5),
                    1.0,
                    egui::Stroke::new(1.0, cursor_color),
                    egui::StrokeKind::Inside,
                );
            }
            Some((line, column, CursorShape::Beam)) => {
                let rect = egui::Rect::from_min_size(at(column, line), egui::vec2(2.0, cell.height));
                painter.rect_filled(rect, 0.0, cursor_color);
            }
            Some((line, column, CursorShape::Underline)) => {
                let rect = egui::Rect::from_min_size(
                    at(column, line) + egui::vec2(0.0, cell.height - 2.0),
                    egui::vec2(cell.width, 2.0),
                );
                painter.rect_filled(rect, 0.0, cursor_color);
            }
            Some((line, column, CursorShape::HollowBlock)) => {
                let rect = egui::Rect::from_min_size(at(column, line), egui::vec2(cell.width, cell.height));
                painter.rect_stroke(
                    rect.shrink(0.5),
                    1.0,
                    egui::Stroke::new(1.0, cursor_color),
                    egui::StrokeKind::Inside,
                );
            }
            _ => {}
        }
    }

    /// Le curseur clignote-t-il ?
    pub fn cursor_blinks(&self) -> bool {
        let term = self.term.lock();
        term.mode().contains(TermMode::SHOW_CURSOR) && term.cursor_style().blinking
    }

    #[cfg(test)]
    pub fn screen_text(&self) -> String {
        use alacritty_terminal::index::Line;
        let term = self.term.lock();
        let last = Point::new(Line(term.screen_lines() as i32 - 1), Column(term.columns() - 1));
        term.bounds_to_string(Point::new(Line(0), Column(0)), last)
    }
}

fn viewport_point((column, line): (usize, usize)) -> Point<usize> {
    Point::new(line, Column(column))
}

#[derive(Clone, Copy, PartialEq)]
struct RunStyle {
    color: egui::Color32,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}

/// Morceau de ligne de même style.
struct Run {
    line: usize,
    column: usize,
    length: usize,
    text: String,
    style: RunStyle,
    simple: bool,
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn session(script: &str, size: GridSize) -> Session {
        let launch = Launch {
            directory: None,
            command: Some(("sh".to_owned(), vec!["-c".to_owned(), script.to_owned()])),
        };
        Session::spawn(&egui::Context::default(), &launch, size, (8, 16)).unwrap()
    }

    fn wait_for(session: &mut Session, text: &str) {
        let start = Instant::now();
        while !session.screen_text().contains(text) {
            session.poll();
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{text:?} attendu, écran : {:?}",
                session.screen_text()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    const SIZE: GridSize = GridSize {
        columns: 80,
        lines: 24,
    };

    #[test]
    fn fish_remplace_les_shells_simples() {
        let fish = "/usr/bin/fish";
        for shell in [
            None,
            Some(""),
            Some("/bin/bash"),
            Some("/usr/bin/sh"),
            Some("/bin/dash"),
        ] {
            assert_eq!(default_shell(shell, fish, true).as_deref(), Some(fish));
        }
        assert_eq!(default_shell(Some("/usr/bin/zsh"), fish, true), None);
        assert_eq!(default_shell(Some("/bin/bash"), fish, false), None);
    }

    #[test]
    fn le_programme_ecrit_a_l_ecran() {
        let mut session = session("printf 'bonjour\\n'; sleep 5", SIZE);
        wait_for(&mut session, "bonjour");
    }

    #[test]
    fn le_clavier_arrive_au_programme() {
        let mut session = session("read ligne; echo \"recu:$ligne\"; sleep 5", SIZE);
        session.write(b"abc\r");
        wait_for(&mut session, "recu:abc");
    }

    #[test]
    fn le_programme_connait_la_taille_et_le_type_du_terminal() {
        let mut session = session("stty size; echo \"t=$TERM c=$COLORTERM\"; sleep 5", SIZE);
        wait_for(&mut session, "24 80");
        wait_for(&mut session, "t=xterm-256color c=truecolor");
    }

    #[test]
    fn redimensionner_previent_le_programme() {
        let mut session = session("read x; stty size; sleep 5", SIZE);
        session.resize(
            GridSize {
                columns: 100,
                lines: 30,
            },
            (8, 16),
        );
        session.write(b"\r");
        wait_for(&mut session, "30 100");
    }

    #[test]
    fn le_titre_et_la_fin_du_programme_sont_signales() {
        let mut session = session("printf '\\033]0;Mon titre\\007'", SIZE);
        let start = Instant::now();
        while !session.exited {
            session.poll();
            assert!(start.elapsed() < Duration::from_secs(10), "fin non signalée");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(session.title.as_deref(), Some("Mon titre"));
    }

    #[test]
    fn selection_recherche_et_historique() {
        let mut session = session(
            "i=0; while [ $i -lt 200 ]; do echo ligne$i; i=$((i+1)); done; echo aiguille; echo fin; sleep 5",
            SIZE,
        );
        wait_for(&mut session, "fin");
        assert!(session.history() > 100);
        // L'occurrence est sélectionnée, comme le ferait la souris.
        assert!(session.search("AIGUILLE".to_lowercase().as_str(), true));
        assert_eq!(session.selected_text().as_deref(), Some("aiguille"));
        assert!(!session.search("introuvable", true));
        assert_eq!(session.selected_text(), None);
        // Une occurrence ancienne fait remonter l'affichage.
        session.reset_search();
        assert!(session.search("ligne3\n", false) || session.search("ligne10", true));
        assert!(session.display_offset() > 0);
        session.scroll(Scroll::Bottom);
        assert_eq!(session.display_offset(), 0);
    }

    #[test]
    fn le_dossier_courant_est_retrouve() {
        let launch = Launch {
            directory: Some(PathBuf::from("/tmp")),
            command: Some(("sleep".to_owned(), vec!["5".to_owned()])),
        };
        let session = Session::spawn(&egui::Context::default(), &launch, SIZE, (8, 16)).unwrap();
        assert_eq!(session.directory(), Some(PathBuf::from("/tmp")));
    }
}
