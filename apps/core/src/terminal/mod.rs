//! Terminal Ferrite : émulation `alacritty_terminal` (256 couleurs et couleurs vraies, souris,
//! écran alternatif, collage protégé), onglets, défilement, sélection, recherche et zoom.
//! Chaque fenêtre est un processus : on en ouvre autant qu'on veut, chacune avec ses onglets.

mod colors;
mod keys;
mod session;

use std::{
    process::Command,
    time::{Duration, Instant},
};

use alacritty_terminal::{grid::Scroll, index::Side, term::TermMode};
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Sense};

use self::{
    keys::MouseAction,
    session::{Cell, GridSize, Session},
};
use crate::style;

pub use self::session::Launch;

const FONT_SIZE: f32 = 13.5;
const FONT_RANGE: std::ops::RangeInclusive<f32> = 8.0..=36.0;
/// Marge autour de la grille.
const PADDING: f32 = 10.0;
const BLINK: Duration = Duration::from_millis(530);
/// Délai entre deux clics d'un double ou triple clic.
const MULTI_CLICK: Duration = Duration::from_millis(400);
const MAX_TAB_WIDTH: f32 = 220.0;

/// La grille garde le clavier : Tab, les flèches et Échap vont au programme, jamais à la
/// navigation d'egui entre les boutons (qui volait le clavier, et qu'Entrée ou Espace
/// faisait ensuite « cliquer », jusqu'à fermer l'onglet).
fn grid_id() -> egui::Id {
    egui::Id::unique("ferrite-terminal-grid")
}

/// Champ de la barre de recherche, seul autre endroit où l'on peut taper.
fn search_field_id() -> egui::Id {
    egui::Id::unique("ferrite-terminal-search")
}

/// Le clavier va-t-il ailleurs que dans le terminal (recherche ou menu ouvert) ?
fn keyboard_elsewhere(context: &egui::Context) -> bool {
    context.memory(|memory| memory.has_focus(search_field_id())) || egui::Popup::is_any_open(context)
}

/// Options de `ferrite-apps terminal [dossier] [-e programme arguments…]`.
pub fn parse_launch(mut args: impl Iterator<Item = String>) -> Launch {
    let mut launch = Launch::default();
    while let Some(argument) = args.next() {
        if argument == "-e" {
            if let Some(program) = args.next() {
                launch.command = Some((program, args.collect()));
            }
            break;
        }
        launch.directory = Some(argument.into());
    }
    launch
}

/// Ce que demandent un raccourci, un bouton ou le menu.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    NewTab,
    CloseTab(usize),
    SelectTab(usize),
    NextTab(isize),
    NewWindow,
    Copy,
    Paste,
    Search,
    Zoom(f32),
    ZoomReset,
}

/// Raccourci de cette touche, comme dans les autres terminaux de Linux.
fn shortcut(key: Key, modifiers: Modifiers) -> Option<Action> {
    let (ctrl, shift) = (modifiers.ctrl, modifiers.shift);
    match key {
        Key::C if ctrl && shift => Some(Action::Copy),
        Key::V if ctrl && shift => Some(Action::Paste),
        Key::T if ctrl && shift => Some(Action::NewTab),
        Key::N if ctrl && shift => Some(Action::NewWindow),
        Key::F if ctrl && shift => Some(Action::Search),
        Key::Tab if ctrl => Some(Action::NextTab(if shift { -1 } else { 1 })),
        Key::PageDown if ctrl => Some(Action::NextTab(1)),
        Key::PageUp if ctrl => Some(Action::NextTab(-1)),
        Key::Plus | Key::Equals if ctrl => Some(Action::Zoom(1.0)),
        Key::Minus if ctrl => Some(Action::Zoom(-1.0)),
        Key::Num0 if ctrl => Some(Action::ZoomReset),
        _ => None,
    }
}

#[derive(Default)]
struct SearchBar {
    text: String,
    missing: bool,
    /// À la première image, le champ prend le clavier.
    focus: bool,
}

/// État de la souris sur la grille.
#[derive(Default)]
struct Mouse {
    /// Bouton signalé enfoncé au programme.
    held: Option<u8>,
    selecting: bool,
    last_cell: Option<(usize, usize)>,
    last_press: Option<(Instant, (usize, usize), u8)>,
    /// Fraction de ligne de la molette pas encore appliquée.
    wheel: f32,
}

pub struct TerminalState {
    tabs: Vec<Session>,
    active: usize,
    launch: Launch,
    /// Le premier onglet est créé à la première image, quand la taille est connue.
    started: bool,
    error: Option<String>,
    font_size: f32,
    search: Option<SearchBar>,
    /// Un collage demandé : le prochain événement `Paste` est du texte à coller, et non un
    /// Ctrl+V à transmettre au programme.
    paste_requested: bool,
    mouse: Mouse,
    focused: bool,
    blink_from: Instant,
    title: String,
}

impl TerminalState {
    pub fn new(context: &egui::Context, launch: Launch) -> Self {
        // Ctrl+plus et Ctrl+molette règlent la police du terminal, pas toute l'interface.
        context.options_mut(|options| options.zoom_with_keyboard = false);
        style::add_mono_fallbacks(context, &fallback_fonts());
        Self {
            tabs: Vec::new(),
            active: 0,
            launch,
            started: false,
            error: None,
            font_size: FONT_SIZE,
            search: None,
            paste_requested: false,
            mouse: Mouse::default(),
            focused: true,
            blink_from: Instant::now(),
            title: String::new(),
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        self.collect_events(&context);
        let events = ui.input(|input| input.events.clone());
        let mut actions = Vec::new();
        self.handle_keyboard(&context, &events, &mut actions);

        style::toolbar(ui, true, |ui| self.tab_bar(ui, &mut actions));
        if self.search.is_some() {
            self.search_bar(ui);
        }
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(colors::background()))
            .show(ui, |ui| self.grid(ui, &events, &mut actions));

        for action in actions {
            self.apply(&context, action);
        }
        self.update_title(&context);
    }

    /// Ce que les programmes ont signalé ; les onglets dont le programme est fini se ferment,
    /// et la fenêtre avec le dernier.
    fn collect_events(&mut self, context: &egui::Context) {
        for session in &mut self.tabs {
            session.poll();
            if let Some(text) = session.copied.take() {
                context.copy_text(text);
            }
        }
        let before = self.tabs.len();
        let active = self.active;
        let mut index = 0;
        let mut removed_before = 0;
        self.tabs.retain(|session| {
            let keep = !session.exited;
            if !keep && index < active {
                removed_before += 1;
            }
            index += 1;
            keep
        });
        if self.tabs.len() != before {
            self.active = (active - removed_before).min(self.tabs.len().saturating_sub(1));
            if self.tabs.is_empty() {
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn session(&self) -> Option<&Session> {
        self.tabs.get(self.active)
    }

    /// Envoie au programme une saisie : l'affichage revient en bas et la sélection s'efface.
    fn type_bytes(&mut self, bytes: &[u8]) {
        self.blink_from = Instant::now();
        if let Some(session) = self.session() {
            session.clear_selection();
            session.scroll(Scroll::Bottom);
            session.write(bytes);
        }
    }

    fn handle_keyboard(&mut self, context: &egui::Context, events: &[Event], actions: &mut Vec<Action>) {
        // Ni le champ de recherche ni un menu ouvert ne doivent écrire dans le terminal.
        let elsewhere = keyboard_elsewhere(context);
        let mode = self.session().map_or(TermMode::NONE, Session::mode);
        let focused = context.input(|input| input.focused);
        if focused != self.focused {
            self.focused = focused;
            self.blink_from = Instant::now();
            if mode.contains(TermMode::FOCUS_IN_OUT)
                && let Some(session) = self.session()
            {
                session.write(if focused { b"\x1b[I" } else { b"\x1b[O" });
            }
        }
        for event in events {
            match event {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(action) = shortcut(*key, *modifiers) {
                        actions.push(action);
                    } else if *key == Key::W && modifiers.ctrl && modifiers.shift {
                        actions.push(Action::CloseTab(self.active));
                    } else if elsewhere || *key == Key::F11 || (modifiers.shift && self.scroll_key(*key)) {
                        continue;
                    } else if let Some(bytes) = keys::key_bytes(*key, *modifiers, mode) {
                        self.type_bytes(&bytes);
                    }
                }
                Event::Text(text) if !elsewhere => {
                    let alt = context.input(|input| input.modifiers.alt);
                    let mut bytes = Vec::with_capacity(text.len() + 1);
                    if alt {
                        bytes.push(0x1b);
                    }
                    bytes.extend_from_slice(text.as_bytes());
                    self.type_bytes(&bytes);
                }
                Event::Ime(egui::ImeEvent::Commit(text)) if !elsewhere => {
                    self.type_bytes(text.as_bytes());
                }
                // egui a déjà transformé ces touches : Ctrl+C reste l'interruption du programme.
                Event::Copy if !elsewhere => self.type_bytes(&[0x03]),
                Event::Cut if !elsewhere => self.type_bytes(&[0x18]),
                Event::Paste(text) => {
                    if std::mem::take(&mut self.paste_requested) {
                        self.type_bytes(&keys::paste_bytes(text, mode));
                    } else if !elsewhere {
                        self.type_bytes(&[0x16]);
                    }
                }
                _ => {}
            }
        }
    }

    /// Maj+Page précédente, etc. : parcourt l'historique sans rien envoyer au programme.
    fn scroll_key(&self, key: Key) -> bool {
        let scroll = match key {
            Key::PageUp => Scroll::PageUp,
            Key::PageDown => Scroll::PageDown,
            Key::Home => Scroll::Top,
            Key::End => Scroll::Bottom,
            _ => return false,
        };
        // Un programme plein écran (vim, less…) n'a pas d'historique à parcourir.
        let Some(session) = self.session() else {
            return false;
        };
        if session.mode().contains(TermMode::ALT_SCREEN) {
            return false;
        }
        session.scroll(scroll);
        true
    }

    fn apply(&mut self, context: &egui::Context, action: Action) {
        match action {
            Action::NewTab => {
                let directory = self.session().and_then(Session::directory);
                self.launch = Launch {
                    directory,
                    command: None,
                };
                let size = self.session().map(Session::size);
                self.open_tab(context, size);
            }
            Action::CloseTab(index) => {
                if index < self.tabs.len() {
                    self.tabs.remove(index);
                    if index < self.active {
                        self.active -= 1;
                    }
                    self.active = self.active.min(self.tabs.len().saturating_sub(1));
                    if self.tabs.is_empty() {
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
            Action::SelectTab(index) => {
                if index < self.tabs.len() {
                    self.active = index;
                }
            }
            Action::NextTab(step) => {
                let count = self.tabs.len() as isize;
                if count > 0 {
                    self.active = (self.active as isize + step).rem_euclid(count) as usize;
                }
            }
            Action::NewWindow => self.open_window(),
            Action::Copy => {
                if let Some(text) = self.session().and_then(Session::selected_text) {
                    context.copy_text(text);
                }
            }
            Action::Paste => {
                self.paste_requested = true;
                context.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
            Action::Search => {
                self.search = Some(SearchBar {
                    focus: true,
                    ..SearchBar::default()
                });
            }
            Action::Zoom(step) => {
                self.font_size = (self.font_size + step).clamp(*FONT_RANGE.start(), *FONT_RANGE.end());
            }
            Action::ZoomReset => self.font_size = FONT_SIZE,
        }
    }

    fn open_tab(&mut self, context: &egui::Context, size: Option<GridSize>) {
        let size = size.unwrap_or(GridSize {
            columns: 80,
            lines: 24,
        });
        match Session::spawn(context, &self.launch, size, (8, 16)) {
            Ok(session) => {
                self.tabs.push(session);
                self.active = self.tabs.len() - 1;
                self.error = None;
            }
            Err(error) => self.error = Some(format!("Terminal indisponible : {error}")),
        }
    }

    /// Nouvelle fenêtre, dans le dossier de l'onglet courant : un processus à part.
    fn open_window(&mut self) {
        let result = std::env::current_exe().and_then(|program| {
            let mut command = Command::new(program);
            command.arg("terminal");
            if let Some(directory) = self.session().and_then(Session::directory) {
                command.arg(directory);
            }
            ferrite_system::command::spawn_detached(command)
        });
        if let Err(error) = result {
            self.error = Some(format!("Nouvelle fenêtre impossible : {error}"));
        }
    }

    fn update_title(&mut self, context: &egui::Context) {
        let title = self
            .session()
            .and_then(|session| session.title.clone())
            .unwrap_or_else(|| "Terminal".to_owned());
        if title != self.title {
            context.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let room = ui.available_width() - 44.0;
        let width = (room / self.tabs.len().max(1) as f32 - 4.0).clamp(90.0, MAX_TAB_WIDTH);
        ui.spacing_mut().item_spacing.x = 4.0;
        for (index, session) in self.tabs.iter().enumerate() {
            let selected = index == self.active;
            let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 30.0), Sense::click());
            let hovered = response.hovered();
            style::segment_background(ui, rect, selected, hovered);
            let close = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 15.0, rect.center().y),
                egui::Vec2::splat(18.0),
            );
            let close_response = ui.interact(close, response.id.with("fermer"), Sense::click());
            let color = if selected {
                style::primary_text()
            } else {
                style::text_secondary()
            };
            let title = session.title.as_deref().unwrap_or("Terminal");
            let mut job = egui::text::LayoutJob::simple_singleline(
                title.to_owned(),
                egui::FontId::proportional(13.0),
                color,
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(width - 42.0);
            let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
            let position = egui::pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0);
            ui.painter().galley(position, galley, color);
            if selected || hovered || close_response.hovered() {
                if close_response.hovered() {
                    ui.painter().rect_filled(close, 5.0, style::tint(20));
                }
                let stroke = egui::Stroke::new(1.3, color);
                let (center, arm) = (close.center(), 4.0);
                ui.painter().line_segment(
                    [center - egui::Vec2::splat(arm), center + egui::Vec2::splat(arm)],
                    stroke,
                );
                ui.painter().line_segment(
                    [center + egui::vec2(-arm, arm), center + egui::vec2(arm, -arm)],
                    stroke,
                );
            }
            if close_response
                .on_hover_text("Fermer l’onglet (Ctrl+Maj+W)")
                .clicked()
                || response.middle_clicked()
            {
                actions.push(Action::CloseTab(index));
            } else if response.clicked() {
                actions.push(Action::SelectTab(index));
            }
        }
        let (rect, response) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 7.0, style::tint(14));
        }
        let stroke = egui::Stroke::new(1.5, style::text_secondary());
        let (center, arm) = (rect.center(), 6.0);
        ui.painter().line_segment(
            [center - egui::vec2(arm, 0.0), center + egui::vec2(arm, 0.0)],
            stroke,
        );
        ui.painter().line_segment(
            [center - egui::vec2(0.0, arm), center + egui::vec2(0.0, arm)],
            stroke,
        );
        if response.on_hover_text("Nouvel onglet (Ctrl+Maj+T)").clicked() {
            actions.push(Action::NewTab);
        }
    }

    fn search_bar(&mut self, ui: &mut egui::Ui) {
        let Some(mut bar) = self.search.take() else {
            return;
        };
        let mut open = true;
        let mut query: Option<bool> = None;
        egui::Panel::top(ui.scope_id().with("recherche"))
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(style::header_color())
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut bar.text)
                            .id(search_field_id())
                            .hint_text("Rechercher dans l’historique")
                            .desired_width(280.0),
                    );
                    if std::mem::take(&mut bar.focus) {
                        field.request_focus();
                    }
                    if field.changed() {
                        if let Some(session) = self.tabs.get_mut(self.active) {
                            session.reset_search();
                        }
                        query = Some(true);
                    }
                    if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                        query = Some(!ui.input(|input| input.modifiers.shift));
                        field.request_focus();
                    }
                    if ui.button("↑").on_hover_text("Précédent (Entrée)").clicked() {
                        query = Some(true);
                    }
                    if ui.button("↓").on_hover_text("Suivant (Maj+Entrée)").clicked() {
                        query = Some(false);
                    }
                    if bar.missing {
                        ui.colored_label(style::error(), "Aucun résultat");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Fermer").clicked() {
                            open = false;
                        }
                    });
                });
            });
        if ui.input(|input| input.key_pressed(Key::Escape)) {
            open = false;
        }
        if let Some(upward) = query
            && let Some(session) = self.tabs.get_mut(self.active)
        {
            bar.missing = !session.search(&bar.text, upward);
        }
        if open {
            self.search = Some(bar);
        } else if let Some(session) = self.tabs.get_mut(self.active) {
            session.reset_search();
            session.clear_selection();
            session.scroll(Scroll::Bottom);
        }
    }

    /// La grille : crée le premier onglet, suit la taille de la fenêtre, gère la souris, dessine.
    fn grid(&mut self, ui: &mut egui::Ui, events: &[Event], actions: &mut Vec<Action>) {
        let context = ui.ctx().clone();
        let font = egui::FontId::monospace(self.font_size);
        let (glyph, row) =
            context.fonts_mut(|fonts| (fonts.glyph_width(&font, 'M'), fonts.row_height(&font)));
        let cell = Cell {
            width: glyph.max(1.0),
            height: row.round().max(1.0),
        };
        let full = ui.max_rect();
        let area = full.shrink(PADDING);
        let size = GridSize {
            columns: ((area.width() / cell.width) as usize).max(2),
            lines: ((area.height() / cell.height) as usize).max(1),
        };
        let pixels_per_point = context.pixels_per_point();
        let pixels = (
            (cell.width * pixels_per_point).round() as u16,
            (cell.height * pixels_per_point).round() as u16,
        );
        if !self.started {
            self.started = true;
            self.open_tab(&context, Some(size));
        }
        for session in &mut self.tabs {
            session.resize(size, pixels);
        }
        if let Some(error) = &self.error {
            ui.painter().text(
                full.center(),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(14.0),
                style::error(),
            );
        }
        if self.tabs.is_empty() {
            return;
        }

        let response = ui.interact(full, grid_id(), Sense::click());
        // Un clic dans la grille y ramène le clavier, même depuis la recherche.
        if response.is_pointer_button_down_on() {
            response.request_focus();
        }
        if !keyboard_elsewhere(&context) {
            context.memory_mut(|memory| {
                memory.request_focus(grid_id());
                memory.set_focus_lock_filter(
                    grid_id(),
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        }
        if response.hovered() {
            context.set_cursor_icon(egui::CursorIcon::Text);
        }
        self.handle_mouse(&context, area, cell, size, events, actions);
        let mode = self.session().map_or(TermMode::NONE, Session::mode);
        if !mode.intersects(TermMode::MOUSE_MODE) {
            let has_selection = self.session().and_then(Session::selected_text).is_some();
            response.context_menu(|ui| {
                let mut item = |ui: &mut egui::Ui, label: &str, shortcut: &str, enabled: bool, action| {
                    let button = egui::Button::new(label).shortcut_text(shortcut);
                    if ui.add_enabled(enabled, button).clicked() {
                        actions.push(action);
                        ui.close();
                    }
                };
                item(ui, "Copier", "Ctrl+Maj+C", has_selection, Action::Copy);
                item(ui, "Coller", "Ctrl+Maj+V", true, Action::Paste);
                ui.separator();
                item(ui, "Rechercher", "Ctrl+Maj+F", true, Action::Search);
                ui.separator();
                item(ui, "Nouvel onglet", "Ctrl+Maj+T", true, Action::NewTab);
                item(ui, "Nouvelle fenêtre", "Ctrl+Maj+N", true, Action::NewWindow);
            });
        }

        let Some(session) = self.session() else {
            return;
        };
        let blinks = self.focused && session.cursor_blinks();
        let elapsed = self.blink_from.elapsed().as_millis() / BLINK.as_millis();
        let cursor_visible = !blinks || elapsed.is_multiple_of(2);
        session.paint(
            ui.painter(),
            area.min,
            cell,
            self.font_size,
            self.focused,
            cursor_visible,
        );
        if blinks {
            context.request_repaint_after(BLINK);
        }
        // Position dans l'historique, seulement quand on y est.
        let offset = session.display_offset();
        if offset > 0 {
            let total = (session.history() + size.lines) as f32;
            let height = (area.height() * size.lines as f32 / total).max(24.0);
            let travel = area.height() - height;
            let top = area.top() + travel * (1.0 - offset as f32 / session.history().max(1) as f32);
            let thumb =
                egui::Rect::from_min_size(egui::pos2(full.right() - 7.0, top), egui::vec2(4.0, height));
            ui.painter().rect_filled(thumb, 2.0, style::tint(110));
        }
    }

    fn handle_mouse(
        &mut self,
        context: &egui::Context,
        area: egui::Rect,
        cell: Cell,
        size: GridSize,
        events: &[Event],
        actions: &mut Vec<Action>,
    ) {
        if self.tabs.is_empty() || egui::Popup::is_any_open(context) {
            return;
        }
        let cell_at = |position: egui::Pos2| {
            let x = (position.x - area.left()).max(0.0) / cell.width;
            let y = (position.y - area.top()).max(0.0) / cell.height;
            let column = (x as usize).min(size.columns - 1);
            let line = (y as usize).min(size.lines - 1);
            let side = if x.fract() > 0.5 { Side::Right } else { Side::Left };
            ((column, line), side)
        };
        let mode = self.tabs[self.active].mode();
        let report = |session: &Session, action, (column, line): (usize, usize), modifiers| {
            if let Some(bytes) = keys::mouse_report(action, column, line, modifiers, mode) {
                session.write(&bytes);
            }
        };
        for event in events {
            let session = &self.tabs[self.active];
            match *event {
                Event::PointerMoved(position) => {
                    let (at, side) = cell_at(position);
                    let moved = self.mouse.last_cell != Some(at);
                    self.mouse.last_cell = Some(at);
                    if self.mouse.selecting {
                        session.select_to(at, side);
                    } else if moved {
                        let modifiers = context.input(|input| input.modifiers);
                        if let Some(button) = self.mouse.held {
                            if mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) {
                                report(session, MouseAction::Motion(Some(button)), at, modifiers);
                            }
                        } else if mode.contains(TermMode::MOUSE_MOTION)
                            && !modifiers.shift
                            && area.contains(position)
                        {
                            report(session, MouseAction::Motion(None), at, modifiers);
                        }
                    }
                }
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    let code = match button {
                        PointerButton::Primary => 0,
                        PointerButton::Middle => 1,
                        PointerButton::Secondary => 2,
                        _ => continue,
                    };
                    let (at, side) = cell_at(pos);
                    let inside = area.expand(PADDING).contains(pos);
                    if pressed && !inside {
                        continue;
                    }
                    // Maj garde la main sur la sélection, même dans un programme à souris.
                    if mode.intersects(TermMode::MOUSE_MODE) && !modifiers.shift {
                        if pressed {
                            self.mouse.held = Some(code);
                            report(session, MouseAction::Press(code), at, modifiers);
                        } else if self.mouse.held.take().is_some() {
                            report(session, MouseAction::Release(code), at, modifiers);
                        }
                        continue;
                    }
                    match (button, pressed) {
                        (PointerButton::Primary, true) => {
                            if modifiers.ctrl
                                && let Some(url) = session.url_at(at)
                            {
                                open_url(&url);
                                continue;
                            }
                            let now = Instant::now();
                            let clicks = match self.mouse.last_press {
                                Some((time, cell, count))
                                    if cell == at && now.duration_since(time) < MULTI_CLICK =>
                                {
                                    count % 3 + 1
                                }
                                _ => 1,
                            };
                            self.mouse.last_press = Some((now, at, clicks));
                            self.mouse.selecting = true;
                            session.select_from(at, side, clicks);
                            if clicks > 1 {
                                session.select_to(at, side);
                            }
                        }
                        (PointerButton::Primary, false) => self.mouse.selecting = false,
                        (PointerButton::Middle, true) => actions.push(Action::Paste),
                        _ => {}
                    }
                }
                Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } => {
                    let Some(position) = context.input(|input| input.pointer.latest_pos()) else {
                        continue;
                    };
                    if !area.expand(PADDING).contains(position) {
                        continue;
                    }
                    let lines = match unit {
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Point => delta.y / cell.height,
                        egui::MouseWheelUnit::Page => delta.y * size.lines as f32,
                    };
                    if modifiers.ctrl {
                        actions.push(Action::Zoom(lines.signum()));
                        continue;
                    }
                    self.mouse.wheel += lines;
                    let whole = self.mouse.wheel.trunc() as i32;
                    self.mouse.wheel -= whole as f32;
                    if whole == 0 {
                        continue;
                    }
                    let (at, _) = cell_at(position);
                    let action = if whole > 0 {
                        MouseAction::WheelUp
                    } else {
                        MouseAction::WheelDown
                    };
                    if mode.intersects(TermMode::MOUSE_MODE) && !modifiers.shift {
                        for _ in 0..whole.abs() {
                            report(session, action, at, modifiers);
                        }
                    } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
                        // Un programme plein écran sans souris (less…) : la molette fait les flèches.
                        let key = if whole > 0 { Key::ArrowUp } else { Key::ArrowDown };
                        if let Some(bytes) = keys::key_bytes(key, Modifiers::NONE, mode) {
                            for _ in 0..whole.abs() {
                                session.write(&bytes);
                            }
                        }
                    } else {
                        session.scroll(Scroll::Delta(whole));
                    }
                }
                _ => {}
            }
        }
    }
}

/// Polices du système pour ce que JetBrains Mono ne couvre pas : écritures d'Asie et symboles.
/// Les polices d'emoji en couleur sont écartées : egui ne sait pas les dessiner.
fn fallback_fonts() -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for pattern in [":lang=ja:color=false", ":charset=1f680:color=false"] {
        let found = Command::new("fc-match")
            .args(["--format=%{file}", pattern])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok());
        if let Some(file) = found.filter(|file| !file.is_empty())
            && !files
                .iter()
                .any(|known: &std::path::PathBuf| known.as_os_str() == file.as_str())
        {
            files.push(file.into());
        }
    }
    files
}

fn open_url(url: &str) {
    let mut command = Command::new("xdg-open");
    command.arg(url);
    let _ = ferrite_system::command::spawn_detached(command);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(args: &[&str]) -> Launch {
        parse_launch(args.iter().map(ToString::to_string))
    }

    #[test]
    fn options_de_lancement() {
        assert_eq!(launch(&[]), Launch::default());
        assert_eq!(launch(&["/tmp"]).directory, Some("/tmp".into()));
        let command = launch(&["/tmp", "-e", "htop", "-d", "5"]);
        assert_eq!(command.directory, Some("/tmp".into()));
        assert_eq!(
            command.command,
            Some(("htop".to_owned(), vec!["-d".to_owned(), "5".to_owned()]))
        );
    }

    #[test]
    fn raccourcis() {
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        assert_eq!(shortcut(Key::C, ctrl_shift), Some(Action::Copy));
        assert_eq!(shortcut(Key::N, ctrl_shift), Some(Action::NewWindow));
        assert_eq!(shortcut(Key::Tab, ctrl_shift), Some(Action::NextTab(-1)));
        // Ctrl+C seul reste l'interruption du programme, et une lettre seule du texte.
        assert_eq!(shortcut(Key::C, Modifiers::CTRL), None);
        assert_eq!(shortcut(Key::T, Modifiers::NONE), None);
    }

    /// Fait tourner l'interface sans écran : le texte tapé atteint un vrai shell, les onglets
    /// s'ouvrent et se ferment.
    #[test]
    fn saisie_et_onglets_de_bout_en_bout() {
        let context = egui::Context::default();
        // Les polices seules : le thème est global, d'autres tests le changent en parallèle.
        context.set_fonts(style::font_definitions());
        let mut state = TerminalState::new(
            &context,
            Launch {
                directory: None,
                command: Some(("sh".to_owned(), Vec::new())),
            },
        );
        let frame = |state: &mut TerminalState, events: Vec<Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 600.0),
                )),
                events,
                focused: true,
                ..Default::default()
            };
            let mut output = context.run_ui(input, |ui| state.update(ui));
            // Sans écran, personne n'applique les textures de la police.
            output.textures_delta.clear();
        };
        frame(&mut state, Vec::new());
        assert_eq!(state.tabs.len(), 1);
        assert!(state.tabs[0].size().columns > 20);

        frame(&mut state, vec![Event::Text("echo ok$((40+2))".to_owned())]);
        let enter = Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        frame(&mut state, vec![enter]);
        let start = Instant::now();
        while !state.tabs[0].screen_text().contains("ok42") {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "écran : {:?}",
                state.tabs[0].screen_text()
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        let press = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::CTRL | Modifiers::SHIFT,
        };
        // Tab, puis Espace et Entrée : le clavier reste au shell, aucun bouton des onglets
        // ne prend le focus ni ne se fait « cliquer ».
        let key = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        frame(&mut state, vec![Event::Text("echo tab".to_owned())]);
        for _ in 0..3 {
            frame(&mut state, vec![key(Key::Tab)]);
        }
        frame(&mut state, vec![key(Key::Space)]);
        frame(&mut state, vec![Event::Text("$((1+1))".to_owned())]);
        frame(&mut state, vec![key(Key::Enter)]);
        assert_eq!(state.tabs.len(), 1);
        assert!(context.memory(|memory| memory.has_focus(grid_id())));
        let start = Instant::now();
        // Les tabulations arrivées au shell y séparent les mots : « tab 2 ».
        while !state.tabs[0].screen_text().contains("tab 2") {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "écran : {:?}",
                state.tabs[0].screen_text()
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        frame(&mut state, vec![press(Key::T)]);
        assert_eq!(state.tabs.len(), 2);
        assert_eq!(state.active, 1);
        frame(&mut state, vec![press(Key::W)]);
        assert_eq!(state.tabs.len(), 1);
        assert_eq!(state.active, 0);
    }
}
