//! Données persistantes des widgets du bureau : disposition, notes, tâches et météo.

use std::{
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ferrite_system::{
    config::Settings,
    weather::{self, Place, Weather},
};
use smithay::reexports::calloop::{LoopHandle, channel};
use tracing::warn;

use crate::state::Ferrite;

/// Délai entre deux relevés de la météo, et avant de réessayer après un échec.
const WEATHER_REFRESH: Duration = Duration::from_secs(30 * 60);
const WEATHER_RETRY: Duration = Duration::from_secs(5 * 60);

/// Disposition d'origine : colonnes de 170 px, écart de 16, à gauche sous la barre.
const CELL: i32 = 170;
const GAP: i32 = 16;
const LEFT: i32 = 32;
const TOP: i32 = 58;
/// Marge gardée autour d'un widget, pour qu'il ne sorte jamais de l'écran.
const SCREEN_MARGIN: i32 = 8;
/// Pas de la grille des widgets : une demi-case. Places et tailles s'y alignent, les
/// bords des widgets tombent donc en face les uns des autres, écart de 16 compris.
pub const GRID: i32 = (CELL + GAP) / 2;

/// Widgets disponibles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetKind {
    Clock,
    Calendar,
    System,
    Tasks,
    Notes,
    Quote,
    /// Mini-Dock des applications récemment utilisées.
    Apps,
}

impl WidgetKind {
    pub const ALL: [WidgetKind; 7] = [
        WidgetKind::Clock,
        WidgetKind::Calendar,
        WidgetKind::System,
        WidgetKind::Tasks,
        WidgetKind::Notes,
        WidgetKind::Quote,
        WidgetKind::Apps,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WidgetKind::Clock => "Horloge",
            WidgetKind::Calendar => "Calendrier",
            WidgetKind::System => "Système",
            WidgetKind::Tasks => "Tâches",
            WidgetKind::Notes => "Notes",
            WidgetKind::Quote => "Citation",
            WidgetKind::Apps => "Applications",
        }
    }

    fn token(self) -> &'static str {
        match self {
            WidgetKind::Clock => "horloge",
            WidgetKind::Calendar => "calendrier",
            WidgetKind::System => "systeme",
            WidgetKind::Tasks => "taches",
            WidgetKind::Notes => "notes",
            WidgetKind::Quote => "citation",
            WidgetKind::Apps => "applications",
        }
    }

    /// Taille en dessous de laquelle le contenu ne tiendrait plus.
    fn min_size(self) -> (i32, i32) {
        match self {
            WidgetKind::Clock => (170, 130),
            WidgetKind::Calendar => (170, 170),
            WidgetKind::System => (170, 170),
            WidgetKind::Tasks => (220, 100),
            WidgetKind::Notes => (160, 100),
            WidgetKind::Quote => (200, 80),
            WidgetKind::Apps => (170, 110),
        }
    }

    /// Place d'origine (celle de la maquette).
    fn default_slot(self) -> WidgetSlot {
        let wide = 2 * CELL + GAP;
        let second_row = TOP + CELL + GAP;
        let (x, y, w, h) = match self {
            WidgetKind::Clock => (LEFT, TOP, wide, CELL),
            WidgetKind::Calendar => (LEFT + wide + GAP, TOP, CELL, CELL),
            WidgetKind::System => (LEFT + wide + 2 * GAP + CELL, TOP, CELL, CELL),
            WidgetKind::Tasks => (LEFT, second_row, wide, CELL),
            WidgetKind::Notes => (LEFT + wide + GAP, second_row, wide, CELL),
            WidgetKind::Quote => (LEFT, second_row + CELL + GAP, 2 * wide + GAP, CELL),
            WidgetKind::Apps => (LEFT + 2 * (wide + GAP), second_row + CELL + GAP, wide, CELL),
        };
        WidgetSlot {
            kind: self,
            x,
            y,
            w,
            h,
        }
    }
}

/// Un widget posé sur le bureau : place et taille libres, en points de l'écran. Le dernier
/// de la liste passe devant les autres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetSlot {
    pub kind: WidgetKind,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// `value` sur la ligne de la grille la plus proche, en partant de `origin`.
fn snap(value: i32, origin: i32) -> i32 {
    origin + ((value - origin) as f64 / f64::from(GRID)).round() as i32 * GRID
}

/// Taille d'un nombre entier de pas, écart compris, jamais sous `min`.
fn snap_size(size: i32, min: i32) -> i32 {
    let steps = (f64::from(size + GAP) / f64::from(GRID)).round().max(1.0) as i32;
    let fitting = (f64::from(min + GAP) / f64::from(GRID)).ceil() as i32;
    steps.max(fitting) * GRID - GAP
}

impl WidgetSlot {
    /// Aligné sur la grille : place et taille.
    pub fn snapped(self) -> Self {
        let (min_w, min_h) = self.kind.min_size();
        Self {
            x: snap(self.x, LEFT),
            y: snap(self.y, TOP),
            w: snap_size(self.w, min_w),
            h: snap_size(self.h, min_h),
            ..self
        }
    }

    fn overlaps(&self, other: &WidgetSlot) -> bool {
        self.x < other.x + other.w + GAP
            && other.x < self.x + self.w + GAP
            && self.y < other.y + other.h + GAP
            && other.y < self.y + self.h + GAP
    }

    /// Ramené dans l'écran de `width` × `height`, sous la barre de hauteur `bar`.
    pub fn clamped(self, width: i32, height: i32, bar: i32) -> Self {
        let (min_w, min_h) = self.kind.min_size();
        let w = self.w.max(min_w).min((width - 2 * SCREEN_MARGIN).max(min_w));
        let h = self
            .h
            .max(min_h)
            .min((height - bar - 2 * SCREEN_MARGIN).max(min_h));
        Self {
            w,
            h,
            x: self.x.min(width - w - SCREEN_MARGIN).max(SCREEN_MARGIN),
            y: self.y.min(height - h - SCREEN_MARGIN).max(bar + SCREEN_MARGIN),
            ..self
        }
    }
}

/// Bords saisis pour redimensionner ; aucun : le widget se déplace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WidgetEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl WidgetEdges {
    /// Le widget `start` après un glissement de (`dx`, `dy`). Le bord opposé ne bouge pas,
    /// même quand la taille minimale arrête le geste.
    pub fn apply(self, start: WidgetSlot, dx: i32, dy: i32) -> WidgetSlot {
        if self == WidgetEdges::default() {
            return WidgetSlot {
                x: start.x + dx,
                y: start.y + dy,
                ..start
            };
        }
        let (min_w, min_h) = start.kind.min_size();
        let mut slot = start;
        if self.right {
            slot.w = (start.w + dx).max(min_w);
        }
        if self.left {
            slot.w = (start.w - dx).max(min_w);
            slot.x = start.x + start.w - slot.w;
        }
        if self.bottom {
            slot.h = (start.h + dy).max(min_h);
        }
        if self.top {
            slot.h = (start.h - dy).max(min_h);
            slot.y = start.y + start.h - slot.h;
        }
        slot
    }
}

/// Place sur la grille du widget `index`, lâché à sa position actuelle : la case la plus
/// proche, ou la plus proche qui ne chevauche aucun autre widget, dans l'écran de
/// `width` × `height` sous la barre de hauteur `bar`.
pub fn place_on_grid(
    layout: &[WidgetSlot],
    index: usize,
    width: i32,
    height: i32,
    bar: i32,
) -> Option<WidgetSlot> {
    let wanted = layout.get(index)?.snapped();
    let others: Vec<WidgetSlot> = layout
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != index)
        .map(|(_, slot)| slot.clamped(width, height, bar))
        .collect();
    let fits = |slot: &WidgetSlot| {
        slot.x >= SCREEN_MARGIN
            && slot.y >= bar + SCREEN_MARGIN
            && slot.x + slot.w <= width - SCREEN_MARGIN
            && slot.y + slot.h <= height - SCREEN_MARGIN
            && others.iter().all(|other| !slot.overlaps(other))
    };
    if fits(&wanted) {
        return Some(wanted);
    }
    // Cases de la grille, de la plus proche à la plus lointaine.
    let columns = (width - LEFT) / GRID + 1;
    let rows = (height - TOP) / GRID + 1;
    let mut candidates: Vec<WidgetSlot> = (-1..columns)
        .flat_map(|column| (-1..rows).map(move |row| (column, row)))
        .map(|(column, row)| WidgetSlot {
            x: LEFT + column * GRID,
            y: TOP + row * GRID,
            ..wanted
        })
        .filter(fits)
        .collect();
    candidates.sort_by_key(|slot| (slot.x - wanted.x).pow(2) + (slot.y - wanted.y).pow(2));
    Some(candidates.first().copied().unwrap_or(wanted))
}

/// Disposition d'origine : celle de la maquette.
pub fn default_layout() -> Vec<WidgetSlot> {
    WidgetKind::ALL
        .into_iter()
        .map(WidgetKind::default_slot)
        .collect()
}

/// Lit `horloge:32:58:356:170,…` ; ce qui n'existe plus est ignoré, sans doublon.
fn parse_layout(text: &str) -> Vec<WidgetSlot> {
    let mut layout: Vec<WidgetSlot> = Vec::new();
    for item in text.split(',') {
        let mut parts = item.split(':');
        let token = parts.next().unwrap_or_default();
        let kind = WidgetKind::ALL.into_iter().find(|kind| kind.token() == token);
        let numbers: Vec<i32> = parts.filter_map(|part| part.parse().ok()).collect();
        if let (Some(kind), [x, y, w, h]) = (kind, numbers.as_slice())
            && layout.iter().all(|slot| slot.kind != kind)
        {
            // Une disposition d'avant la grille s'y aligne.
            layout.push(
                WidgetSlot {
                    kind,
                    x: *x,
                    y: *y,
                    w: *w,
                    h: *h,
                }
                .snapped(),
            );
        }
    }
    layout
}

fn layout_text(layout: &[WidgetSlot]) -> String {
    layout
        .iter()
        .map(|slot| {
            format!(
                "{}:{}:{}:{}:{}",
                slot.kind.token(),
                slot.x,
                slot.y,
                slot.w,
                slot.h
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardEdit {
    Note(usize),
    NewTask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority {
    High,
    #[default]
    Medium,
    Low,
}

impl Priority {
    /// Priorité suivante, au clic sur l'étiquette.
    pub fn next(self) -> Self {
        match self {
            Priority::High => Priority::Medium,
            Priority::Medium => Priority::Low,
            Priority::Low => Priority::High,
        }
    }

    fn token(self) -> &'static str {
        match self {
            Priority::High => "haute",
            Priority::Medium => "moyenne",
            Priority::Low => "basse",
        }
    }

    fn from_token(token: Option<&str>) -> Self {
        match token {
            Some("haute") => Priority::High,
            Some("basse") => Priority::Low,
            _ => Priority::Medium,
        }
    }
}

/// Note : la première ligne sert de titre, le reste d'aperçu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub text: String,
    /// Dernière modification, en secondes depuis 1970.
    pub updated: u64,
}

impl Note {
    pub fn title(&self) -> &str {
        self.text.lines().next().unwrap_or("").trim()
    }

    /// Texte après le titre, sur une ligne.
    pub fn preview(&self) -> String {
        self.text
            .lines()
            .skip(1)
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub text: String,
    pub done: bool,
    pub priority: Priority,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dashboard {
    pub notes: Vec<Note>,
    pub tasks: Vec<Task>,
    pub draft: String,
    pub month_offset: i32,
    /// Widgets posés, du dessous vers le dessus.
    pub layout: Vec<WidgetSlot>,
    /// Note affichée par le widget Notes.
    pub note_shown: usize,
    /// Première tâche affichée, quand elles ne tiennent pas toutes.
    pub tasks_scroll: usize,
    /// Dernier relevé ; `None` tant qu'aucun n'a abouti.
    pub weather: Option<Weather>,
    pub place: String,
}

pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl Dashboard {
    pub fn load() -> Self {
        let settings = Settings::load();
        let notes = load_notes(&settings);
        let tasks = if let Some(count) = settings
            .get("tableau_taches")
            .and_then(|value| value.parse::<usize>().ok())
        {
            (0..count.min(100))
                .filter_map(|index| {
                    let text = settings.get(&format!("tableau_tache_{index}"))?;
                    Some(Task {
                        text: decode(text),
                        done: settings.get(&format!("tableau_tache_{index}_faite")) == Some("oui"),
                        priority: Priority::from_token(
                            settings.get(&format!("tableau_tache_{index}_priorite")),
                        ),
                    })
                })
                .collect()
        } else {
            sample_tasks()
        };
        let mut layout = settings
            .get("widgets_disposition")
            .map_or_else(default_layout, parse_layout);
        // Le mini-Dock des applications récentes rejoint une fois les dispositions d'avant.
        if settings.get("widgets_applications").is_none()
            && layout.iter().all(|slot| slot.kind != WidgetKind::Apps)
        {
            let mut slot = WidgetKind::Apps.default_slot();
            let bottom = layout.iter().map(|slot| slot.y + slot.h).max().unwrap_or(TOP);
            if layout.iter().any(|other| other.overlaps(&slot)) {
                slot.x = LEFT;
                slot.y = snap(bottom + GAP, TOP);
            }
            layout.push(slot);
        }
        Self {
            notes,
            tasks,
            layout,
            place: Place::from_settings(&settings).name,
            ..Self::default()
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let mut settings = Settings::load();
        settings.set("widgets_disposition", &layout_text(&self.layout));
        settings.set("widgets_applications", "oui");
        settings.set("tableau_notes_nombre", &self.notes.len().to_string());
        for (index, note) in self.notes.iter().enumerate() {
            settings.set(&format!("tableau_note_{index}"), &encode(&note.text));
            settings.set(&format!("tableau_note_{index}_date"), &note.updated.to_string());
        }
        settings.set("tableau_taches", &self.tasks.len().to_string());
        for (index, task) in self.tasks.iter().enumerate() {
            settings.set(&format!("tableau_tache_{index}"), &encode(&task.text));
            settings.set(
                &format!("tableau_tache_{index}_faite"),
                if task.done { "oui" } else { "non" },
            );
            settings.set(&format!("tableau_tache_{index}_priorite"), task.priority.token());
        }
        settings.save()
    }

    /// Widgets qu'on peut encore ajouter.
    pub fn absent_widgets(layout: &[WidgetSlot]) -> Vec<WidgetKind> {
        WidgetKind::ALL
            .into_iter()
            .filter(|kind| layout.iter().all(|slot| slot.kind != *kind))
            .collect()
    }

    pub fn add_widget(&mut self, kind: WidgetKind) {
        if self.layout.iter().all(|slot| slot.kind != kind) {
            self.layout.push(kind.default_slot());
        }
    }

    pub fn remove_widget(&mut self, index: usize) {
        if index < self.layout.len() {
            self.layout.remove(index);
        }
    }

    /// Passe le widget devant les autres ; renvoie sa nouvelle place dans la liste.
    pub fn raise_widget(&mut self, index: usize) -> usize {
        if index < self.layout.len() {
            let slot = self.layout.remove(index);
            self.layout.push(slot);
        }
        self.layout.len().saturating_sub(1)
    }

    /// Commence une note, en tête de liste.
    pub fn add_note(&mut self, now: u64) -> Option<DashboardEdit> {
        if self.notes.len() >= 100 {
            return None;
        }
        self.notes.insert(
            0,
            Note {
                text: String::new(),
                updated: now,
            },
        );
        self.note_shown = 0;
        Some(DashboardEdit::Note(0))
    }

    /// Fin de la saisie : une note restée vide disparaît. Renvoie sa place.
    pub fn finish_editing(&mut self, editing: Option<DashboardEdit>) -> Option<usize> {
        let Some(DashboardEdit::Note(index)) = editing else {
            return None;
        };
        if self.notes.get(index)?.text.trim().is_empty() {
            self.notes.remove(index);
            return Some(index);
        }
        None
    }

    /// Ajoute `character` à la saisie en cours ; renvoie vrai s'il faut enregistrer.
    pub fn type_char(&mut self, editing: Option<DashboardEdit>, character: char, now: u64) -> bool {
        match editing {
            Some(DashboardEdit::Note(index)) => self.notes.get_mut(index).is_some_and(|note| {
                note.text.push(character);
                note.updated = now;
                true
            }),
            Some(DashboardEdit::NewTask) => {
                self.draft.push(character);
                false
            }
            None => false,
        }
    }

    /// Efface le dernier caractère ; renvoie vrai s'il faut enregistrer.
    pub fn erase_char(&mut self, editing: Option<DashboardEdit>, now: u64) -> bool {
        match editing {
            Some(DashboardEdit::Note(index)) => self.notes.get_mut(index).is_some_and(|note| {
                note.text.pop();
                note.updated = now;
                true
            }),
            Some(DashboardEdit::NewTask) => {
                self.draft.pop();
                false
            }
            None => false,
        }
    }

    pub fn commit_draft(&mut self) -> bool {
        if self.tasks.len() >= 100 {
            return false;
        }
        let text = self.draft.trim();
        if text.is_empty() {
            self.draft.clear();
            return false;
        }
        self.tasks.push(Task {
            text: text.to_owned(),
            done: false,
            priority: Priority::default(),
        });
        self.draft.clear();
        true
    }
}

/// Notes enregistrées, ou anciennes notes en un seul texte reprises comme une note.
fn load_notes(settings: &Settings) -> Vec<Note> {
    if let Some(count) = settings
        .get("tableau_notes_nombre")
        .and_then(|value| value.parse::<usize>().ok())
    {
        return (0..count.min(100))
            .filter_map(|index| {
                Some(Note {
                    text: decode(settings.get(&format!("tableau_note_{index}"))?),
                    updated: settings
                        .get(&format!("tableau_note_{index}_date"))
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                })
            })
            .collect();
    }
    let legacy = settings.get("tableau_notes").map(decode).unwrap_or_else(|| {
        let count = settings
            .get("widgets_nombre")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0)
            .min(100);
        (0..count)
            .filter_map(|index| settings.get(&format!("widget_{index}_texte")))
            .map(|text| text.replace("\\n", "\n").replace("\\s", "="))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    });
    if legacy.trim().is_empty() {
        return sample_notes();
    }
    vec![Note {
        text: legacy,
        updated: now_seconds(),
    }]
}

fn sample_notes() -> Vec<Note> {
    vec![Note {
        text: "Idées pour Ferrite\nCentre de contrôle : chevrons Wi-Fi et Bluetooth. Widgets en grille \
               4 colonnes, verre sombre. Tester le scanout direct…"
            .to_owned(),
        updated: now_seconds(),
    }]
}

fn sample_tasks() -> Vec<Task> {
    [
        ("Préparer la démo Ferrite", Priority::High, false),
        ("Relire le rapport", Priority::Medium, false),
        ("Mettre à jour les icônes", Priority::Low, true),
    ]
    .into_iter()
    .map(|(text, priority, done)| Task {
        text: text.to_owned(),
        done,
        priority,
    })
    .collect()
}

/// Relève la météo en arrière-plan, sans jamais retenir le bureau.
pub fn start_weather(handle: &LoopHandle<'static, Ferrite>) -> Result<(), String> {
    let (sender, receiver) = channel::channel::<Weather>();
    handle
        .insert_source(receiver, |event, _, state| {
            if let channel::Event::Msg(weather) = event {
                state.dashboard.weather = Some(weather);
            }
        })
        .map_err(|err| err.to_string())?;
    let place = Place::from_settings(&Settings::load());
    thread::Builder::new()
        .name("ferrite-meteo".to_owned())
        .spawn(move || {
            loop {
                let delay = match weather::fetch(&place) {
                    Ok(weather) => {
                        if sender.send(weather).is_err() {
                            return;
                        }
                        WEATHER_REFRESH
                    }
                    Err(err) => {
                        warn!(%err, "météo indisponible");
                        WEATHER_RETRY
                    }
                };
                thread::sleep(delay);
            }
        })
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn encode(text: &str) -> String {
    let mut encoded = String::new();
    for character in text.chars() {
        match character {
            '\\' => encoded.push_str("\\\\"),
            '=' => encoded.push_str("\\e"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            ' ' => encoded.push_str("\\p"),
            character => encoded.push(character),
        }
    }
    encoded
}

fn decode(text: &str) -> String {
    let mut chars = text.chars();
    let mut decoded = String::new();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('\\') => decoded.push('\\'),
                Some('e') => decoded.push('='),
                Some('n') => decoded.push('\n'),
                Some('r') => decoded.push('\r'),
                Some('t') => decoded.push('\t'),
                Some('p') => decoded.push(' '),
                Some(other) => {
                    decoded.push('\\');
                    decoded.push(other);
                }
                None => decoded.push('\\'),
            }
        } else {
            decoded.push(character);
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(kind: WidgetKind) -> WidgetSlot {
        kind.default_slot()
    }

    #[test]
    fn la_disposition_d_origine_reproduit_la_maquette() {
        let places: Vec<_> = default_layout()
            .iter()
            .map(|slot| (slot.x, slot.y, slot.w, slot.h))
            .collect();
        assert_eq!(
            places,
            vec![
                (32, 58, 356, 170),
                (404, 58, 170, 170),
                (590, 58, 170, 170),
                (32, 244, 356, 170),
                (404, 244, 356, 170),
                (32, 430, 728, 170),
                (776, 430, 356, 170),
            ]
        );
        // Toutes sur la grille.
        assert!(default_layout().iter().all(|slot| slot.snapped() == *slot));
    }

    #[test]
    fn un_widget_lache_s_aligne_sur_la_grille() {
        let loose = WidgetSlot {
            x: 900,
            y: 300,
            w: 400,
            h: 260,
            ..slot(WidgetKind::Notes)
        };
        let snapped = loose.snapped();
        assert_eq!((snapped.x, snapped.y, snapped.w, snapped.h), (869, 337, 356, 263));
        // Jamais sous la taille minimale : la citation garde deux pas de haut.
        let thin = WidgetSlot {
            h: 40,
            ..slot(WidgetKind::Quote)
        };
        assert_eq!(thin.snapped().h, 170);
    }

    #[test]
    fn un_widget_lache_sur_un_autre_prend_la_place_libre_la_plus_proche() {
        let mut layout = default_layout();
        // Le calendrier lâché presque sur l'horloge.
        layout[1].x = 60;
        layout[1].y = 70;
        let placed = place_on_grid(&layout, 1, 1600, 1000, 34).expect("widget");
        let others: Vec<WidgetSlot> = layout
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != 1)
            .map(|(_, slot)| *slot)
            .collect();
        assert!(others.iter().all(|other| !placed.overlaps(other)));
        assert_eq!(placed.snapped(), placed);
        // Lâché sur une place libre, il y reste.
        layout[1].x = 1200;
        layout[1].y = 60;
        let free = place_on_grid(&layout, 1, 1600, 1000, 34).expect("widget");
        assert_eq!((free.x, free.y), (1241, 58));
    }

    #[test]
    fn la_disposition_se_relit_et_ignore_l_inconnu() {
        let layout = default_layout();
        assert_eq!(parse_layout(&layout_text(&layout)), layout);
        let read = parse_layout("notes:900:300:400:260,inconnu:1:1:1:1,notes:0:0:9:9,taches:2");
        assert_eq!(
            read,
            vec![WidgetSlot {
                kind: WidgetKind::Notes,
                x: 869,
                y: 337,
                w: 356,
                h: 263
            }]
        );
    }

    #[test]
    fn un_widget_se_deplace_librement() {
        let moved = WidgetEdges::default().apply(slot(WidgetKind::Notes), 300, 123);
        assert_eq!((moved.x, moved.y, moved.w, moved.h), (704, 367, 356, 170));
    }

    #[test]
    fn un_widget_se_redimensionne_par_ses_bords_sans_passer_sous_son_minimum() {
        let start = slot(WidgetKind::Calendar);
        let corner = WidgetEdges {
            right: true,
            bottom: true,
            ..WidgetEdges::default()
        };
        let bigger = corner.apply(start, 200, 90);
        assert_eq!((bigger.x, bigger.y, bigger.w, bigger.h), (404, 58, 370, 260));
        // Le bord opposé reste en place, même arrêté par la taille minimale.
        let left = WidgetEdges {
            left: true,
            top: true,
            ..WidgetEdges::default()
        };
        let smaller = left.apply(bigger, 500, 500);
        assert_eq!((smaller.w, smaller.h), (170, 170));
        assert_eq!(smaller.x + smaller.w, bigger.x + bigger.w);
        assert_eq!(smaller.y + smaller.h, bigger.y + bigger.h);
    }

    #[test]
    fn un_widget_reste_dans_l_ecran() {
        let far = WidgetSlot {
            x: 5000,
            y: -300,
            w: 9000,
            h: 400,
            ..slot(WidgetKind::Tasks)
        };
        let shown = far.clamped(1280, 800, 34);
        assert_eq!(shown.w, 1264);
        assert_eq!(shown.x, 8);
        assert_eq!(shown.y, 42);
        assert!(shown.y + shown.h <= 792);
    }

    #[test]
    fn un_widget_saisi_passe_devant_et_un_retire_peut_revenir() {
        let mut dashboard = Dashboard {
            layout: default_layout(),
            ..Dashboard::default()
        };
        assert_eq!(dashboard.raise_widget(1), 6);
        assert_eq!(dashboard.layout[6].kind, WidgetKind::Calendar);
        dashboard.remove_widget(6);
        assert_eq!(
            Dashboard::absent_widgets(&dashboard.layout),
            vec![WidgetKind::Calendar]
        );
        dashboard.add_widget(WidgetKind::Calendar);
        dashboard.add_widget(WidgetKind::Calendar);
        assert_eq!(dashboard.layout.len(), 7);
    }

    #[test]
    fn les_notes_encodent_les_separateurs_du_fichier_de_reglages() {
        let text = "  Ligne 1 = été\\n\n\tLigne 2  ";
        assert_eq!(decode(&encode(text)), text);
    }

    #[test]
    fn la_premiere_ligne_d_une_note_est_son_titre() {
        let note = Note {
            text: "Projet web\n\n  Finaliser\nl'interface ".to_owned(),
            updated: 0,
        };
        assert_eq!(note.title(), "Projet web");
        assert_eq!(note.preview(), "Finaliser l'interface");
    }

    #[test]
    fn une_note_restee_vide_disparait() {
        let mut dashboard = Dashboard::default();
        let editing = dashboard.add_note(5);
        assert_eq!(editing, Some(DashboardEdit::Note(0)));
        assert_eq!(dashboard.finish_editing(editing), Some(0));
        assert!(dashboard.notes.is_empty());

        let editing = dashboard.add_note(5);
        assert!(dashboard.type_char(editing, 'a', 9));
        assert_eq!(dashboard.notes[0].updated, 9);
        assert_eq!(dashboard.finish_editing(editing), None);
        assert_eq!(dashboard.notes[0].text, "a");
    }

    #[test]
    fn la_priorite_tourne_au_clic() {
        let priority = Priority::High;
        assert_eq!(priority.next().next().next(), priority);
        assert_eq!(Priority::from_token(Some(priority.token())), priority);
        assert_eq!(Priority::from_token(None), Priority::Medium);
    }

    #[test]
    fn une_tache_vide_n_est_pas_ajoutee() {
        let mut dashboard = Dashboard {
            draft: "   ".to_owned(),
            ..Dashboard::default()
        };
        assert!(!dashboard.commit_draft());
        assert!(dashboard.tasks.is_empty());
    }
}
