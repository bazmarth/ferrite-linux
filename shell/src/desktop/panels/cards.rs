//! Cartes des widgets : verre sombre, à la place et à la taille choisies, sous les fenêtres.

use crate::desktop::widgets::{Dashboard, DashboardEdit, Priority, WidgetEdges, WidgetKind, WidgetSlot};

use super::*;

/// Barre de l'éditeur, au-dessus du Dock.
const EDITOR_BAR: (f64, f64) = (908.0, 44.0);
/// Largeur de la zone des bords qui redimensionnent.
const EDGE: f64 = 8.0;
/// Largeurs à partir desquelles une carte montre plus de choses.
const CLOCK_FULL: f64 = 300.0;
const CLOCK_FORECAST: f64 = 600.0;
const CALENDAR_ARROWS: f64 = 240.0;
const SYSTEM_CHART: f64 = 320.0;
const NOTE_ARROWS: f64 = 230.0;
const RADIUS: f32 = 26.0;
/// Hauteur et pas d'une ligne de tâche.
const TASK_ROW: f64 = 30.0;
const TASK_STEP: f64 = 36.0;
/// Haut de la première tâche, sous les onglets des feuilles.
const TASKS_TOP: f64 = 84.0;
/// Onglets des feuilles de tâches : hauteur, écart, largeurs extrêmes.
const TAB_H: f64 = 28.0;
const TAB_GAP: f64 = 6.0;
const TAB_W: (f64, f64) = (64.0, 124.0);

const INK: Color = Color::rgb(0xF3F6F9);
const SOFT: Color = Color::rgb(0xC8D1DC);
const DIM: Color = Color::rgb(0xA9B4C2);
const FAINT: Color = Color::rgb(0x8995A3);
const GLASS_TONE: Color = Color::rgb(0x121A26);
const NOTE_FILL: Color = Color::rgb(0xF6E7A8);
const NOTE_INK: Color = Color::rgb(0x3A2F0B);
const NOTE_BODY: Color = Color::rgb(0x5C4D1A);
const NOTE_LABEL: Color = Color::rgb(0x7A6418);
const TEAL: Color = Color::rgb(0x0E9AA7);

const MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];
const DAYS: [&str; 7] = [
    "dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi",
];

impl Layout {
    /// Place des widgets à l'écran : jamais dehors, même sur un écran plus petit.
    pub(super) fn dashboard_cards(&self, slots: &[WidgetSlot]) -> Vec<Rect> {
        slots
            .iter()
            .map(|slot| {
                let slot = slot.clamped(self.w as i32, self.h as i32, self.bar as i32);
                Rect::new(
                    f64::from(slot.x),
                    f64::from(slot.y),
                    f64::from(slot.w),
                    f64::from(slot.h),
                )
            })
            .collect()
    }

    /// Barre de l'éditeur : widgets à ajouter, Rétablir et Terminé.
    pub(super) fn editor_bar(&self) -> Rect {
        let (w, h) = EDITOR_BAR;
        Rect::new(
            ((self.w - w) / 2.0).max(8.0),
            self.h - self.dock_height - h - 16.0,
            w,
            h,
        )
    }
}

fn done_button(bar: Rect) -> Rect {
    Rect::new(bar.right() - 100.0, bar.y + 6.0, 94.0, bar.h - 12.0)
}

fn reset_button(bar: Rect) -> Rect {
    Rect::new(done_button(bar).x - 104.0, bar.y + 6.0, 98.0, bar.h - 12.0)
}

fn add_chip(bar: Rect, index: usize) -> Rect {
    Rect::new(
        bar.x + 6.0 + index as f64 * 100.0,
        bar.y + 6.0,
        96.0,
        bar.h - 12.0,
    )
}

/// Pastille « retirer » d'un widget, à cheval sur son coin.
fn remove_badge(card: Rect) -> Rect {
    Rect::new(card.x - 6.0, card.y - 6.0, 24.0, 24.0)
}

/// Bords saisis en `pos`, sur le pourtour d'une carte.
fn edges_at(card: Rect, pos: Point<f64, Logical>) -> WidgetEdges {
    WidgetEdges {
        left: pos.x < card.x + EDGE,
        right: pos.x >= card.right() - EDGE,
        top: pos.y < card.y + EDGE,
        bottom: pos.y >= card.bottom() - EDGE,
    }
}

/// Bouton rond en haut à droite d'une carte de tâches ou de notes.
fn add_button(card: Rect) -> Rect {
    Rect::new(card.right() - 44.0, card.y + 16.0, 26.0, 26.0)
}

/// Widget Musique : pochette carrée à gauche (si la carte est assez large).
fn music_art(card: Rect) -> Option<Rect> {
    let side = card.h - 36.0;
    (card.w >= side + 200.0).then(|| Rect::new(card.x + 18.0, card.y + 18.0, side, side))
}

/// Boutons précédent, lecture/pause et suivant, en bas de la colonne de texte.
pub(super) fn music_buttons(card: Rect) -> [Rect; 3] {
    let left = music_art(card).map_or(card.x + 18.0, |art| art.right() + 16.0);
    let y = card.bottom() - 18.0 - 36.0;
    [
        Rect::new(left - 6.0, y, 36.0, 36.0),
        Rect::new(left + 36.0, y, 36.0, 36.0),
        Rect::new(left + 78.0, y, 36.0, 36.0),
    ]
}

/// Note rapide : grand bouton « Écrire » au milieu de la carte.
fn quick_button(card: Rect) -> Rect {
    Rect::new(
        card.x + 16.0,
        card.y + 48.0,
        card.w - 32.0,
        (card.h - 96.0).clamp(44.0, 64.0),
    )
}

/// Notes récentes : indices des notes, la plus récemment modifiée d'abord.
pub(super) fn recent_notes(view: &DashboardView) -> Vec<usize> {
    let mut order: Vec<usize> = (0..view.notes.len()).collect();
    order.sort_by_key(|&index| std::cmp::Reverse(view.notes[index].updated));
    order
}

/// Lignes des notes récentes qui tiennent dans la carte.
pub(super) fn recent_rows(card: Rect, count: usize) -> Vec<Rect> {
    let fit = ((card.bottom() - 12.0 - (card.y + 44.0)) / 36.0).floor().max(0.0) as usize;
    (0..count.min(fit))
        .map(|row| {
            Rect::new(
                card.x + 18.0,
                card.y + 44.0 + row as f64 * 36.0,
                card.w - 36.0,
                36.0,
            )
        })
        .collect()
}

/// Note épinglée : la plus récente qui a une liste à cocher, sinon la plus récente.
pub(super) fn pinned_note(view: &DashboardView) -> Option<usize> {
    let recent = recent_notes(view);
    recent
        .iter()
        .copied()
        .find(|&index| !crate::desktop::widgets::checklist(&view.notes[index].lines).is_empty())
        .or_else(|| recent.first().copied())
}

/// Lignes à cocher de la note épinglée qui tiennent dans la carte.
pub(super) fn pinned_rows(card: Rect, count: usize) -> Vec<Rect> {
    let fit = ((card.bottom() - 40.0 - (card.y + 94.0)) / 34.0).floor().max(0.0) as usize;
    (0..count.min(fit))
        .map(|row| {
            Rect::new(
                card.x + 18.0,
                card.y + 94.0 + row as f64 * 34.0,
                card.w - 36.0,
                34.0,
            )
        })
        .collect()
}

/// Corbeille de la note affichée, à gauche du bouton d'ajout.
fn note_trash(card: Rect) -> Rect {
    let add = add_button(card);
    Rect::new(add.x - 30.0, add.y, 26.0, 26.0)
}

/// Note précédente et suivante, à gauche de la corbeille.
fn note_arrows(card: Rect) -> [Rect; 2] {
    let trash = note_trash(card);
    [
        Rect::new(trash.x - 64.0, trash.y, 26.0, 26.0),
        Rect::new(trash.x - 34.0, trash.y, 26.0, 26.0),
    ]
}

/// Onglets des feuilles de tâches visibles (place dans la liste et cadre), puis l'onglet
/// « + » qui en crée une. La feuille affichée reste toujours visible.
fn list_tabs(card: Rect, count: usize, shown: usize) -> (Vec<(usize, Rect)>, Rect) {
    let y = card.y + 44.0;
    let room = (card.w - 36.0 - TAB_H - TAB_GAP).max(TAB_W.0);
    let count = count.max(1);
    let width = ((room - (count - 1) as f64 * TAB_GAP) / count as f64).clamp(TAB_W.0, TAB_W.1);
    let visible = (((room + TAB_GAP) / (width + TAB_GAP)).floor() as usize).clamp(1, count);
    let first = (shown + 1).saturating_sub(visible).min(count - visible);
    let tabs = (first..first + visible)
        .enumerate()
        .map(|(slot, index)| {
            let x = card.x + 18.0 + slot as f64 * (width + TAB_GAP);
            (index, Rect::new(x, y, width, TAB_H))
        })
        .collect::<Vec<_>>();
    let plus_x = card.x + 18.0 + visible as f64 * (width + TAB_GAP);
    (tabs, Rect::new(plus_x, y, TAB_H, TAB_H))
}

/// Croix qui supprime la feuille, dans son onglet.
fn tab_close(tab: Rect) -> Rect {
    Rect::new(tab.right() - 24.0, tab.y + 4.0, 20.0, 20.0)
}

/// Croix qui supprime une tâche, au bout de sa ligne ; la priorité est juste avant.
fn task_delete(row: Rect) -> Rect {
    Rect::new(row.right() - 20.0, row.y + 5.0, 20.0, 20.0)
}

fn task_priority(row: Rect) -> Rect {
    Rect::new(row.right() - 46.0, row.y, 24.0, row.h)
}

fn month_arrows(card: Rect) -> [Rect; 2] {
    [
        Rect::new(card.right() - 62.0, card.y + 8.0, 24.0, 24.0),
        Rect::new(card.right() - 36.0, card.y + 8.0, 24.0, 24.0),
    ]
}

fn task_row(card: Rect, row: usize) -> Rect {
    Rect::new(
        card.x + 18.0,
        card.y + TASKS_TOP + row as f64 * TASK_STEP,
        card.w - 36.0,
        TASK_ROW,
    )
}

/// Cases des applications du mini-Dock : icône de 44 et nom dessous, sur des lignes
/// centrées, autant qu'il en tient.
fn app_cells(card: Rect, count: usize) -> Vec<Rect> {
    const CELL: (f64, f64) = (68.0, 74.0);
    let area = Rect::new(card.x + 12.0, card.y + 40.0, card.w - 24.0, card.h - 48.0);
    let columns = ((area.w / CELL.0).floor() as usize).max(1);
    let rows = ((area.h / CELL.1).floor() as usize).max(1);
    let shown = count.min(columns * rows);
    let used_rows = shown.div_ceil(columns).max(1);
    let top = area.y + (area.h - used_rows as f64 * CELL.1) / 2.0;
    (0..shown)
        .map(|index| {
            let row = index / columns;
            let in_row = (shown - row * columns).min(columns);
            let left = area.x + (area.w - in_row as f64 * CELL.0) / 2.0;
            Rect::new(
                left + (index % columns) as f64 * CELL.0,
                top + row as f64 * CELL.1,
                CELL.0,
                CELL.1,
            )
        })
        .collect()
}

/// Lignes de tâches qui tiennent dans la carte.
fn task_rows(card: Rect) -> usize {
    (((card.h - TASKS_TOP - 8.0 + TASK_STEP - TASK_ROW) / TASK_STEP).floor() as usize).max(1)
}

/// Tâches à afficher : celles à faire d'abord. La saisie en cours prend une ligne.
pub(super) fn shown_tasks(view: &DashboardView, card: Rect) -> Vec<usize> {
    let mut order: Vec<usize> = (0..view.tasks.len()).filter(|&i| !view.tasks[i].done).collect();
    order.extend((0..view.tasks.len()).filter(|&i| view.tasks[i].done));
    let editing = usize::from(view.editing == Some(DashboardEdit::NewTask));
    let room = task_rows(card).saturating_sub(editing);
    order.into_iter().skip(view.tasks_scroll).take(room).collect()
}

/// Défilement maximal de la liste des tâches : la dernière reste visible.
pub fn tasks_scroll_limit(tasks: usize) -> usize {
    tasks.saturating_sub(1)
}

/// Widget sous `pos` (position à l'écran), celui de devant s'ils se chevauchent.
pub fn widget_at(
    output: Rectangle<i32, Logical>,
    view: &DesktopView,
    pos: Point<f64, Logical>,
) -> Option<usize> {
    let layout = Layout::new(output.size.w as f64, output.size.h as f64, view);
    let pos = Point::from((pos.x - output.loc.x as f64, pos.y - output.loc.y as f64));
    layout
        .dashboard_cards(&view.dashboard.layout)
        .into_iter()
        .rposition(|card| card.contains(pos))
}

/// Bords sous `pos` en mode d'édition, pour la forme du curseur.
pub fn widget_edges_at(
    output: Rectangle<i32, Logical>,
    view: &DesktopView,
    pos: Point<f64, Logical>,
) -> Option<WidgetEdges> {
    let index = widget_at(output, view, pos)?;
    let layout = Layout::new(output.size.w as f64, output.size.h as f64, view);
    let card = *layout.dashboard_cards(&view.dashboard.layout).get(index)?;
    let pos = Point::from((pos.x - output.loc.x as f64, pos.y - output.loc.y as f64));
    Some(edges_at(card, pos))
}

/// Place des widgets à l'écran, pour savoir si une fenêtre en recouvre un.
pub fn widgets_areas(output: Rectangle<i32, Logical>, view: &DesktopView) -> Vec<Rectangle<i32, Logical>> {
    Layout::new(output.size.w as f64, output.size.h as f64, view)
        .dashboard_cards(&view.dashboard.layout)
        .into_iter()
        .map(|card| {
            Rectangle::new(
                (output.loc.x + card.x as i32, output.loc.y + card.y as i32).into(),
                (card.w as i32, card.h as i32).into(),
            )
        })
        .collect()
}

/// Clic en mode d'édition : tout est absorbé, les fenêtres restent à l'abri.
fn editor_action_at(layout: &Layout, view: &DesktopView, pos: Point<f64, Logical>) -> DesktopAction {
    let slots = &view.dashboard.layout;
    let bar = layout.editor_bar();
    if bar.contains(pos) {
        if done_button(bar).contains(pos) {
            return DesktopAction::WidgetsDone;
        }
        if reset_button(bar).contains(pos) {
            return DesktopAction::WidgetsReset;
        }
        for (index, kind) in Dashboard::absent_widgets(slots).into_iter().enumerate() {
            if add_chip(bar, index).contains(pos) {
                return DesktopAction::WidgetAdd(kind);
            }
        }
        return DesktopAction::Consume;
    }
    let cards = layout.dashboard_cards(slots);
    for (index, card) in cards.iter().enumerate().rev() {
        if remove_badge(*card).contains(pos) {
            return DesktopAction::WidgetRemove(index);
        }
        if card.contains(pos) {
            return DesktopAction::WidgetDragStart(index, edges_at(*card, pos));
        }
    }
    DesktopAction::Consume
}

/// Action d'un clic sur les widgets ; `None` : à côté.
pub(super) fn dashboard_action_at(
    layout: &Layout,
    view: &DesktopView,
    pos: Point<f64, Logical>,
) -> Option<DesktopAction> {
    if view.dashboard.edit {
        return Some(editor_action_at(layout, view, pos));
    }
    let cards = layout.dashboard_cards(&view.dashboard.layout);
    let (index, card) = cards
        .iter()
        .enumerate()
        .rev()
        .find(|(_, card)| card.contains(pos))?;
    let slot = view.dashboard.layout[index];
    Some(match slot.kind {
        WidgetKind::Calendar if card.w >= CALENDAR_ARROWS => {
            let [previous, next] = month_arrows(*card);
            if previous.contains(pos) {
                DesktopAction::DashboardMonth(-1)
            } else if next.contains(pos) {
                DesktopAction::DashboardMonth(1)
            } else {
                DesktopAction::DashboardConsume
            }
        }
        WidgetKind::Notes => {
            let [previous, next] = note_arrows(*card);
            let several = view.dashboard.notes.len() > 1 && card.w >= NOTE_ARROWS;
            let shown = view
                .dashboard
                .note_shown
                .min(view.dashboard.notes.len().saturating_sub(1));
            if add_button(*card).contains(pos) {
                DesktopAction::DashboardAddNote
            } else if !view.dashboard.notes.is_empty() && note_trash(*card).contains(pos) {
                DesktopAction::DashboardDeleteNote(shown)
            } else if several && previous.contains(pos) {
                DesktopAction::DashboardNoteStep(-1)
            } else if several && next.contains(pos) {
                DesktopAction::DashboardNoteStep(1)
            } else if view.dashboard.notes.is_empty() {
                DesktopAction::DashboardConsume
            } else {
                DesktopAction::DashboardEditNote(
                    view.dashboard.note_shown.min(view.dashboard.notes.len() - 1),
                )
            }
        }
        WidgetKind::Music => {
            use crate::desktop::lock::MediaCommand;
            let commands = [
                MediaCommand::Previous,
                MediaCommand::PlayPause,
                MediaCommand::Next,
            ];
            let pressed = view.dashboard.media.as_ref().and_then(|_| {
                music_buttons(*card)
                    .iter()
                    .position(|button| button.contains(pos))
            });
            pressed.map_or(DesktopAction::DashboardOpenMusic, |index| {
                DesktopAction::DashboardMedia(commands[index])
            })
        }
        WidgetKind::NoteQuick => {
            if quick_button(*card).contains(pos) {
                DesktopAction::DashboardAddNote
            } else {
                DesktopAction::DashboardConsume
            }
        }
        WidgetKind::NotesRecent => {
            let order = recent_notes(&view.dashboard);
            recent_rows(*card, order.len())
                .iter()
                .position(|row| row.contains(pos))
                .map_or(DesktopAction::DashboardConsume, |row| {
                    DesktopAction::DashboardShowNote(order[row])
                })
        }
        WidgetKind::NotePinned => match pinned_note(&view.dashboard) {
            Some(note) => {
                let checks = crate::desktop::widgets::checklist(&view.dashboard.notes[note].lines);
                pinned_rows(*card, checks.len())
                    .iter()
                    .position(|row| row.contains(pos))
                    .map_or(DesktopAction::DashboardShowNote(note), |row| {
                        DesktopAction::DashboardToggleCheck(note, checks[row].0)
                    })
            }
            None => DesktopAction::DashboardAddNote,
        },
        WidgetKind::Apps => app_cells(*card, view.dashboard.apps.len())
            .iter()
            .position(|cell| cell.contains(pos))
            .map_or(DesktopAction::DashboardConsume, DesktopAction::DashboardApp),
        WidgetKind::Tasks => {
            if add_button(*card).contains(pos) {
                return Some(DesktopAction::DashboardAddTask);
            }
            let lists = view.dashboard.lists.len();
            let (tabs, plus) = list_tabs(*card, lists, view.dashboard.list_shown);
            for (index, tab) in tabs {
                if tab.contains(pos) {
                    let current = index == view.dashboard.list_shown;
                    return Some(if current && lists > 1 && tab_close(tab).contains(pos) {
                        DesktopAction::DashboardDeleteList(index)
                    } else {
                        DesktopAction::DashboardList(index)
                    });
                }
            }
            if plus.contains(pos) {
                return Some(DesktopAction::DashboardAddList);
            }
            let first = usize::from(view.dashboard.editing == Some(DashboardEdit::NewTask));
            for (row, task) in shown_tasks(&view.dashboard, *card).into_iter().enumerate() {
                let rect = task_row(*card, row + first);
                if rect.contains(pos) {
                    return Some(if task_delete(rect).contains(pos) {
                        DesktopAction::DashboardDeleteTask(task)
                    } else if task_priority(rect).contains(pos) {
                        DesktopAction::DashboardCyclePriority(task)
                    } else {
                        DesktopAction::DashboardToggleTask(task)
                    });
                }
            }
            DesktopAction::DashboardConsume
        }
        _ => DesktopAction::DashboardConsume,
    })
}

impl Canvas<'_> {
    /// Verre sombre : le fond flouté vu au travers, teinté à 55 %, liseré blanc à 10 %.
    fn frosted(&mut self, rect: Rect, radius: f32, backdrop: Option<&Image>, placement: Option<Placement>) {
        let s = self.scale;
        let (x, y, w, h) = (
            rect.x as f32 * s,
            rect.y as f32 * s,
            rect.w as f32 * s,
            rect.h as f32 * s,
        );
        let radius = (radius * s).min(w.min(h) * 0.5);
        let tone = [GLASS_TONE.r, GLASS_TONE.g, GLASS_TONE.b].map(f32::from);
        let (screen_w, screen_h) = placement.map_or((self.width as f64, self.height as f64), |at| at.screen);
        let (shift_x, shift_y) =
            placement.map_or((0.0, 0.0), |at| (at.x * f64::from(s), at.y * f64::from(s)));
        let left = x.floor().max(0.0) as u32;
        let top = y.floor().max(0.0) as u32;
        let right = ((x + w).ceil() as u32).min(self.width);
        let bottom = ((y + h).ceil() as u32).min(self.height);
        for py in top..bottom {
            for px in left..right {
                let (sx, sy) = (px as f32 + 0.5, py as f32 + 0.5);
                let (cx, cy) = (sx - (x + w * 0.5), sy - (y + h * 0.5));
                let qx = cx.abs() - (w * 0.5 - radius);
                let qy = cy.abs() - (h * 0.5 - radius);
                let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                let coverage = (0.5 - distance).clamp(0.0, 1.0);
                if coverage == 0.0 {
                    continue;
                }
                match backdrop {
                    Some(image) => {
                        let behind = sample(
                            image,
                            (f64::from(sx) + shift_x) / screen_w,
                            (f64::from(sy) + shift_y) / screen_h,
                        );
                        let luma = 0.3 * behind[0] + 0.59 * behind[1] + 0.11 * behind[2];
                        let [r, g, b] = [0, 1, 2].map(|c| {
                            let vivid = (luma + (behind[c] - luma) * 1.5).clamp(0.0, 255.0);
                            (vivid + (tone[c] - vivid) * 0.55).round() as u8
                        });
                        self.blend(px, py, Color { r, g, b, a: 255 }, coverage);
                    }
                    None => self.blend(px, py, GLASS_TONE.with_alpha(140), coverage),
                }
                let edge = (1.0 + distance / s).clamp(0.0, 1.0);
                self.blend(px, py, Color::WHITE, 0.1 * edge * coverage);
            }
        }
    }
}

/// Données d'une carte qui changent toutes seules (heure, mesures, lecture) : quand seules
/// elles changent, la carte est redessinée seule (voir `DashboardCache`).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CardLive {
    clock: Option<ClockSnapshot>,
    system: Option<SystemMetrics>,
    media: Option<Option<crate::desktop::lock::Media>>,
}

pub(super) fn card_live(kind: WidgetKind, state: &VisualState) -> CardLive {
    let uses_clock = matches!(
        kind,
        WidgetKind::Clock | WidgetKind::Calendar | WidgetKind::Notes | WidgetKind::NotesRecent
    );
    CardLive {
        clock: uses_clock.then_some(state.clock),
        system: (kind == WidgetKind::System).then(|| state.system.clone()),
        media: (kind == WidgetKind::Music).then(|| state.view.dashboard.media.clone()),
    }
}

/// L'état sans ses parties vivantes : ce qui, en changeant, oblige à tout redessiner.
pub(super) fn base_key(state: &VisualState) -> VisualState {
    let mut base = state.clone();
    base.clock = ClockSnapshot::default();
    base.system = SystemMetrics::default();
    base.view.dashboard.media = None;
    base
}

/// Ce dont dépend le verre des cartes (fond dépoli, papier des notes) : calculé pixel par
/// pixel, il est coûteux (plus de 200 ms en plein écran) et ne change qu'avec la taille de
/// l'écran, la place des cartes et le fond d'écran, pas avec leur contenu (heure,
/// mesures du système toutes les 2 s).
#[derive(Debug, PartialEq)]
pub(super) struct GlassKey {
    size: (u32, u32, u32),
    cards: Vec<(bool, Rect)>,
    backdrop: Option<usize>,
}

impl Panels {
    /// Verre de toutes les cartes, depuis le cache s'il est à jour.
    pub(super) fn glass_pixels(
        &self,
        layout: &Layout,
        state: &VisualState,
        width: u32,
        height: u32,
        scale: f64,
    ) -> Vec<u8> {
        let view = &state.view.dashboard;
        let cards: Vec<(bool, Rect)> = view
            .layout
            .iter()
            .zip(layout.dashboard_cards(&view.layout))
            .map(|(slot, card)| (slot.kind == WidgetKind::Notes, card))
            .collect();
        let key = GlassKey {
            size: (width, height, state.scale_milli),
            cards,
            backdrop: view.backdrop.as_ref().map(|image| Arc::as_ptr(image) as usize),
        };
        let mut cache = self.glass.borrow_mut();
        if let Some((cached, pixels)) = cache.as_ref()
            && *cached == key
        {
            return pixels.clone();
        }
        let mut canvas = Canvas {
            pixels: vec![0; width as usize * height as usize * 4],
            width,
            height,
            scale: scale as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        for (notes, card) in &key.cards {
            card_glass(&mut canvas, *notes, *card, view.backdrop.as_deref(), None);
        }
        let pixels = canvas.pixels;
        *cache = Some((key, pixels.clone()));
        pixels
    }

    /// `glass_drawn` : le verre des cartes est déjà posé (voir `glass_pixels`) ; `skip` :
    /// carte saisie, dessinée à part pendant qu'elle est déplacée (voir `push_dashboard`).
    pub(super) fn draw_dashboard(
        &self,
        canvas: &mut Canvas,
        layout: &Layout,
        state: &VisualState,
        glass_drawn: bool,
        skip: Option<usize>,
    ) {
        let view = &state.view.dashboard;
        let cards = layout.dashboard_cards(&view.layout);
        // Case de la grille visée par le widget déplacé, en creux sous lui.
        if let Some(target) = view.target {
            let slot = target.clamped(layout.w as i32, layout.h as i32, layout.bar as i32);
            let rect = Rect::new(
                f64::from(slot.x),
                f64::from(slot.y),
                f64::from(slot.w),
                f64::from(slot.h),
            );
            target_hollow(canvas, rect);
        }
        // Du dessous vers le dessus : le verre de chaque carte cache ce qu'elle recouvre.
        for (index, card) in cards.iter().copied().enumerate() {
            if Some(index) != skip {
                self.draw_card(canvas, state, index, card, !glass_drawn, None);
            }
        }
        if view.edit {
            self.draw_editor_bar(canvas, layout, view);
        }
    }

    /// Carte `index` dans `card`, avec son verre si `glass` ; `placement` : où se trouve
    /// sur l'écran une image plus petite que lui (pour voir le bon morceau du fond).
    pub(super) fn draw_card(
        &self,
        canvas: &mut Canvas,
        state: &VisualState,
        index: usize,
        card: Rect,
        glass: bool,
        placement: Option<Placement>,
    ) {
        let view = &state.view.dashboard;
        let kind = view.layout[index].kind;
        if glass {
            card_glass(
                canvas,
                kind == WidgetKind::Notes,
                card,
                view.backdrop.as_deref(),
                placement,
            );
        }
        match kind {
            WidgetKind::Clock => self.clock_card(canvas, card, state),
            WidgetKind::Calendar => self.calendar_card(canvas, card, state),
            WidgetKind::System => self.system_card(canvas, card, state),
            WidgetKind::Tasks => self.tasks_card(canvas, card, view),
            WidgetKind::Notes => self.notes_card(canvas, card, state),
            WidgetKind::Quote => self.quote_card(canvas, card),
            WidgetKind::Apps => self.apps_card(canvas, card, view),
            WidgetKind::Music => self.music_card(canvas, card, view),
            WidgetKind::NoteQuick => self.quick_note_card(canvas, card, view),
            WidgetKind::NotesRecent => self.recent_notes_card(canvas, card, state),
            WidgetKind::NotePinned => self.pinned_note_card(canvas, card, view),
        }
        if view.edit {
            self.draw_handles(canvas, card, kind, view.drag == Some(index));
        }
    }

    /// Libellé en capitales espacées, comme les titres de toutes les cartes.
    fn caption(&self, canvas: &mut Canvas, text: &str, x: f64, y: f64, color: Color) {
        canvas.tracked(&self.bold, &text.to_uppercase(), x, y, 11.0, 0.08, color);
    }

    fn round_button(&self, canvas: &mut Canvas, rect: Rect, fill: Color, ink: Color, glyph: Glyph) {
        canvas.fill(rect, (rect.w / 2.0) as f32, fill);
        let (cx, cy) = rect.center();
        match glyph {
            Glyph::Plus => {
                canvas.line((cx - 4.5, cy), (cx + 4.5, cy), 1.8, ink);
                canvas.line((cx, cy - 4.5), (cx, cy + 4.5), 1.8, ink);
            }
            Glyph::Previous => {
                canvas.line((cx + 2.0, cy - 4.5), (cx - 2.5, cy), 1.8, ink);
                canvas.line((cx - 2.5, cy), (cx + 2.0, cy + 4.5), 1.8, ink);
            }
            Glyph::Next => {
                canvas.line((cx - 2.0, cy - 4.5), (cx + 2.5, cy), 1.8, ink);
                canvas.line((cx + 2.5, cy), (cx - 2.0, cy + 4.5), 1.8, ink);
            }
            Glyph::Minus => canvas.line((cx - 4.5, cy), (cx + 4.5, cy), 2.0, ink),
            Glyph::Trash => {
                canvas.line((cx - 5.0, cy - 3.5), (cx + 5.0, cy - 3.5), 1.6, ink);
                canvas.line((cx - 1.8, cy - 5.5), (cx + 1.8, cy - 5.5), 1.6, ink);
                canvas.line((cx - 3.6, cy - 3.0), (cx - 2.8, cy + 5.0), 1.6, ink);
                canvas.line((cx + 3.6, cy - 3.0), (cx + 2.8, cy + 5.0), 1.6, ink);
                canvas.line((cx - 2.8, cy + 5.0), (cx + 2.8, cy + 5.0), 1.6, ink);
            }
        }
    }

    /// Barres grises à la place de ce qui n'est pas encore chargé.
    fn skeleton(&self, canvas: &mut Canvas, rect: Rect, radius: f32) {
        canvas.fill(rect, radius, Color::WHITE.with_alpha(20));
    }

    fn clock_card(&self, canvas: &mut Canvas, card: Rect, state: &VisualState) {
        let view = &state.view.dashboard;
        let clock = state.clock;
        let time = format!("{:02}:{:02}", clock.hour, clock.minute);
        let date = format!(
            "{} {} {}",
            DAYS[clock.weekday],
            clock.day,
            MONTHS[clock.month as usize - 1]
        );
        let place = view.place.to_uppercase();
        if card.w < CLOCK_FULL {
            let (x, width) = (card.x + 16.0, card.w - 32.0);
            let place = canvas.fit_text(&self.bold, &place, px(11.0), width);
            canvas.tracked(&self.bold, &place, x, card.y + 16.0, 11.0, 0.06, DIM);
            let size = (width / 2.75).clamp(48.0, card.h * 0.5);
            let top = card.y + (card.h - size) / 2.0 - size * 0.08;
            canvas.tracked(&self.light, &time, x, top, size, -0.03, INK);
            let date = canvas.fit_text(&self.regular, &date, px(13.0), width);
            canvas.text(&self.regular, &date, x, card.bottom() - 32.0, 13.0, SOFT);
            return;
        }
        let x = card.x + 20.0;
        let right = card.right() - 20.0;
        let place = canvas.fit_text(&self.bold, &place, px(12.0), 190.0);
        canvas.tracked(&self.bold, &place, x, card.y + 20.0, 12.0, 0.08, DIM);
        // L'heure grandit avec la carte.
        let size = (card.h * 64.0 / 170.0).clamp(48.0, (card.w - 170.0) / 2.6);
        let top = card.y + (card.h - size) / 2.0 - size * 0.08;
        canvas.tracked(&self.light, &time, x, top, size, -0.03, INK);
        canvas.text(&self.regular, &date, x, card.bottom() - 37.0, 14.0, SOFT);

        let symbol = (right - 23.0, card.y + 43.0);
        let Some(weather) = view.weather.as_ref() else {
            // Rien de faux tant que le relevé n'est pas arrivé.
            self.skeleton(canvas, Rect::new(right - 44.0, card.y + 22.0, 44.0, 44.0), 22.0);
            self.skeleton(canvas, Rect::new(right - 58.0, card.y + 80.0, 58.0, 26.0), 8.0);
            self.skeleton(
                canvas,
                Rect::new(right - 96.0, card.bottom() - 48.0, 96.0, 10.0),
                5.0,
            );
            self.skeleton(
                canvas,
                Rect::new(right - 76.0, card.bottom() - 32.0, 76.0, 10.0),
                5.0,
            );
            return;
        };
        self.weather_symbol(canvas, symbol, weather.sky, 2.0);
        let temperature = format!("{}°", weather.temperature);
        let width = canvas.css_width(&self.regular, &temperature, 34.0);
        canvas.text(
            &self.regular,
            &temperature,
            right - width,
            card.y + 76.0,
            34.0,
            INK,
        );
        let sky = weather.sky.label();
        let width = canvas.css_width(&self.regular, sky, 12.0);
        canvas.text(&self.regular, sky, right - width, card.bottom() - 51.0, 12.0, DIM);
        let range = format!("max {}° · min {}°", weather.today.0, weather.today.1);
        let width = canvas.css_width(&self.regular, &range, 12.0);
        canvas.text(
            &self.regular,
            &range,
            right - width,
            card.bottom() - 36.0,
            12.0,
            DIM,
        );
        if card.w >= CLOCK_FORECAST {
            // Assez large : les prévisions s'ajoutent.
            let (start, cell) = (card.x + 270.0, (card.w - 270.0 - 150.0) / 4.0);
            for (index, day) in weather.days.iter().take(4).enumerate() {
                let center = start + cell * (index as f64 + 0.5);
                let name = weekday_for(day.date.0, day.date.1, day.date.2);
                let width = canvas.css_width(&self.regular, name, 12.0);
                canvas.text(
                    &self.regular,
                    name,
                    center - width / 2.0,
                    card.y + 40.0,
                    12.0,
                    DIM,
                );
                self.weather_symbol(canvas, (center, card.y + 82.0), day.sky, 1.0);
                let range = format!("{}° / {}°", day.max, day.min);
                let width = canvas.css_width(&self.regular, &range, 12.0);
                canvas.text(
                    &self.regular,
                    &range,
                    center - width / 2.0,
                    card.y + 112.0,
                    12.0,
                    SOFT,
                );
            }
        }
    }

    fn calendar_card(&self, canvas: &mut Canvas, card: Rect, state: &VisualState) {
        let view = &state.view.dashboard;
        let (year, month) = shift_month(state.clock.year, state.clock.month, view.month_offset);
        let title = if view.month_offset == 0 {
            MONTHS[month as usize - 1].to_owned()
        } else {
            format!("{} {year}", MONTHS[month as usize - 1])
        };
        self.caption(canvas, &title, card.x + 16.0, card.y + 14.0, Color::rgb(0xF07A6E));
        if card.w >= CALENDAR_ARROWS {
            let [previous, next] = month_arrows(card);
            self.round_button(
                canvas,
                previous,
                Color::WHITE.with_alpha(26),
                Color::WHITE,
                Glyph::Previous,
            );
            self.round_button(
                canvas,
                next,
                Color::WHITE.with_alpha(26),
                Color::WHITE,
                Glyph::Next,
            );
        }
        // Les jours occupent toute la carte : le texte grandit avec elle.
        let (left, width) = (card.x + 14.0, card.w - 28.0);
        let cell = width / 7.0;
        let top = card.y + 34.0;
        let row = ((card.bottom() - 10.0 - top) / 7.0).max(14.0);
        let text = (row * 0.62).min(cell * 0.5).clamp(10.0, 28.0);
        let head = (text * 0.9).max(9.0);
        for (index, name) in ["L", "M", "M", "J", "V", "S", "D"].iter().enumerate() {
            let w = canvas.css_width(&self.bold, name, head);
            let x = left + cell * (index as f64 + 0.5) - w / 2.0;
            canvas.text(&self.bold, name, x, top + (row - head * 1.21) / 2.0, head, FAINT);
        }
        let first = (days_from_civil(year, month, 1) + 3).rem_euclid(7) as usize;
        for day in 1..=days_in_month(year, month) {
            let slot = first + day as usize - 1;
            let (column, line) = (slot % 7, slot / 7);
            let y = top + row * (line as f64 + 1.0);
            let today = view.month_offset == 0 && day == state.clock.day;
            if today {
                let rect = Rect::new(left + cell * column as f64, y, cell, row - 1.0);
                canvas.fill(rect, ((row - 1.0).min(cell) / 2.0) as f32, Color::rgb(0xE5574A));
            }
            let number = day.to_string();
            let font = if today { &self.bold } else { &self.regular };
            let w = canvas.css_width(font, &number, text);
            let ink = if today {
                Color::WHITE
            } else if column >= 5 {
                FAINT
            } else {
                Color::rgb(0xE3E8EE)
            };
            let x = left + cell * (column as f64 + 0.5) - w / 2.0;
            canvas.text(font, &number, x, y + (row - 1.0 - text * 1.21) / 2.0, text, ink);
        }
    }

    fn system_card(&self, canvas: &mut Canvas, card: Rect, state: &VisualState) {
        let metrics = &state.system;
        let chart = card.w >= SYSTEM_CHART;
        let width = if chart {
            (card.w - 48.0) * 0.45
        } else {
            card.w - 32.0
        };
        let x = card.x + 16.0;
        self.caption(canvas, "Système", x, card.y + 16.0, DIM);
        let values = [
            ("CPU", metrics.cpu, Color::rgb(0x3CC7D1)),
            ("Mémoire", metrics.memory, Color::rgb(0x6CB8FF)),
            ("Disque", metrics.disk, Color::rgb(0xA98BFF)),
        ];
        let step = (card.h - 40.0 - 30.0) / 3.0;
        for (index, (name, value, color)) in values.into_iter().enumerate() {
            let y = card.y + 40.0 + index as f64 * step;
            canvas.text(&self.regular, name, x, y, 12.0, SOFT);
            let percent = format!("{value} %");
            let w = canvas.css_width(&self.bold, &percent, 12.0);
            canvas.text(&self.bold, &percent, x + width - w, y, 12.0, INK);
            let track = Rect::new(x, y + 19.0, width, 5.0);
            canvas.fill(track, 2.5, Color::WHITE.with_alpha(26));
            let filled = (width * f64::from(value.min(100)) / 100.0).max(5.0);
            canvas.fill(Rect::new(x, y + 19.0, filled, 5.0), 2.5, color);
        }
        let (down, down_unit) = network_rate(metrics.download_kib);
        let (up, up_unit) = network_rate(metrics.upload_kib);
        let rates = format!("↓ {down} {down_unit}/s · ↑ {up} {up_unit}/s");
        let rates = canvas.fit_text(&self.regular, &rates, px(11.0), width);
        canvas.text(&self.regular, &rates, x, card.bottom() - 30.0, 11.0, DIM);
        if chart {
            // Assez large : l'historique du réseau s'ajoute.
            let left = x + width + 16.0;
            let graph = Rect::new(
                left,
                card.y + 44.0,
                card.right() - 16.0 - left,
                card.h - 44.0 - 28.0,
            );
            canvas.network_chart(graph, &metrics.network_history);
        }
    }

    fn tasks_card(&self, canvas: &mut Canvas, card: Rect, view: &DashboardView) {
        let remaining = view.tasks.iter().filter(|task| !task.done).count();
        // Le titre s'arrête avant le bouton d'ajout.
        let room = card.w - 36.0 - 34.0;
        let title = match remaining {
            0 => "Tâches · tout est fait".to_owned(),
            1 => "Tâches · 1 restante".to_owned(),
            count => format!("Tâches · {count} restantes"),
        };
        let title = canvas.fit_text(&self.bold, &title, px(12.5), room);
        self.caption(canvas, &title, card.x + 18.0, card.y + 22.0, DIM);
        self.round_button(
            canvas,
            add_button(card),
            Color::WHITE.with_alpha(26),
            Color::WHITE,
            Glyph::Plus,
        );
        let (tabs, plus) = list_tabs(card, view.lists.len(), view.list_shown);
        for (index, tab) in tabs {
            let current = index == view.list_shown;
            let fill = if current {
                TEAL.with_alpha(210)
            } else {
                Color::WHITE.with_alpha(22)
            };
            canvas.fill(tab, (TAB_H / 2.0) as f32, fill);
            let naming = view.editing == Some(DashboardEdit::ListName(index));
            let name = view.lists.get(index).map_or("", String::as_str);
            let name = if naming {
                format!("{name}|")
            } else {
                name.to_owned()
            };
            let closable = current && view.lists.len() > 1;
            let room = tab.w - 20.0 - if closable { 20.0 } else { 0.0 };
            let name = canvas.fit_text(&self.bold, &name, px(12.5), room);
            let ink = if current { Color::WHITE } else { SOFT };
            canvas.text(&self.bold, &name, tab.x + 11.0, tab.y + 6.0, 12.5, ink);
            if closable {
                let (cx, cy) = tab_close(tab).center();
                canvas.line(
                    (cx - 3.5, cy - 3.5),
                    (cx + 3.5, cy + 3.5),
                    1.6,
                    Color::WHITE.with_alpha(220),
                );
                canvas.line(
                    (cx - 3.5, cy + 3.5),
                    (cx + 3.5, cy - 3.5),
                    1.6,
                    Color::WHITE.with_alpha(220),
                );
            }
        }
        self.round_button(canvas, plus, Color::WHITE.with_alpha(22), SOFT, Glyph::Plus);
        let editing = view.editing == Some(DashboardEdit::NewTask);
        if editing {
            let row = task_row(card, 0);
            let ring = Rect::new(row.x, row.y + 6.0, 18.0, 18.0);
            canvas.stroke(ring, 9.0, TEAL, 1.6);
            let (text, ink) = if view.draft.is_empty() {
                ("Saisir une tâche puis Entrée…".to_owned(), FAINT)
            } else {
                (format!("{}|", view.draft), INK)
            };
            let text = canvas.fit_text(&self.regular, &text, px(14.0), row.w - 28.0);
            canvas.text(&self.regular, &text, row.x + 28.0, row.y + 6.0, 14.0, ink);
        }
        if view.tasks.is_empty() && !editing {
            let hint = canvas.fit_text(
                &self.regular,
                "Aucune tâche. Cliquez sur + pour en ajouter.",
                px(13.0),
                card.w - 36.0,
            );
            canvas.text(
                &self.regular,
                &hint,
                card.x + 18.0,
                card.y + TASKS_TOP + 6.0,
                13.0,
                FAINT,
            );
        }
        for (row, index) in shown_tasks(view, card).into_iter().enumerate() {
            let task = &view.tasks[index];
            let rect = task_row(card, row + usize::from(editing));
            let ring = Rect::new(rect.x, rect.y + 6.0, 18.0, 18.0);
            if task.done {
                canvas.fill(ring, 9.0, TEAL);
                let (cx, cy) = ring.center();
                canvas.line((cx - 4.0, cy + 0.2), (cx - 1.2, cy + 3.0), 1.8, Color::WHITE);
                canvas.line((cx - 1.2, cy + 3.0), (cx + 4.0, cy - 2.6), 1.8, Color::WHITE);
            } else {
                canvas.stroke(ring, 9.0, Color::WHITE.with_alpha(115), 1.6);
            }
            let label = canvas.fit_text(&self.regular, &task.text, px(14.0), rect.w - 28.0 - 52.0);
            let ink = if task.done { Color::rgb(0x7D8896) } else { INK };
            canvas.text(&self.regular, &label, rect.x + 28.0, rect.y + 6.0, 14.0, ink);
            if task.done {
                let width = canvas.css_width(&self.regular, &label, 14.0);
                canvas.fill(Rect::new(rect.x + 28.0, rect.y + 14.5, width, 1.2), 0.0, ink);
            }
            let dot = if task.done {
                Color::WHITE.with_alpha(38)
            } else {
                match task.priority {
                    Priority::High => Color::rgb(0xF07A6E),
                    Priority::Medium => Color::rgb(0xFFB547),
                    Priority::Low => Color::rgb(0x3CC7D1),
                }
            };
            let (px_, py_) = task_priority(rect).center();
            canvas.fill(Rect::new(px_ - 4.0, py_ - 4.0, 8.0, 8.0), 4.0, dot);
            let (cx, cy) = task_delete(rect).center();
            let cross = Color::WHITE.with_alpha(110);
            canvas.line((cx - 3.5, cy - 3.5), (cx + 3.5, cy + 3.5), 1.5, cross);
            canvas.line((cx - 3.5, cy + 3.5), (cx + 3.5, cy - 3.5), 1.5, cross);
        }
    }

    fn notes_card(&self, canvas: &mut Canvas, card: Rect, state: &VisualState) {
        let view = &state.view.dashboard;
        let fill = NOTE_INK.with_alpha(26);
        let (x, width) = (card.x + 18.0, card.w - 36.0);
        self.round_button(canvas, add_button(card), fill, NOTE_INK, Glyph::Plus);
        let Some(note) = view
            .notes
            .get(view.note_shown.min(view.notes.len().saturating_sub(1)))
        else {
            self.caption(canvas, "Notes", x, card.y + 22.0, NOTE_LABEL);
            canvas.text(
                &self.regular,
                "Cliquez sur + pour créer une note",
                x,
                card.y + 56.0,
                13.0,
                NOTE_BODY,
            );
            return;
        };
        let stamp = note_timestamp(note.updated, state.clock);
        let several = view.notes.len() > 1 && card.w >= NOTE_ARROWS;
        self.round_button(canvas, note_trash(card), fill, NOTE_INK, Glyph::Trash);
        // Trop étroit : le titre seul, avant les boutons.
        let room = card.w - 36.0 - 64.0 - if several { 64.0 } else { 0.0 };
        let caption = if view.notes.len() > 1 {
            let shown = view.note_shown.min(view.notes.len() - 1) + 1;
            format!("Notes {shown}/{} · {stamp}", view.notes.len())
        } else {
            format!("Notes · {stamp}")
        };
        let caption = canvas.fit_text(&self.bold, &caption, px(12.5), room);
        self.caption(canvas, &caption, x, card.y + 22.0, NOTE_LABEL);
        if several {
            let [previous, next] = note_arrows(card);
            self.round_button(canvas, previous, fill, NOTE_INK, Glyph::Previous);
            self.round_button(canvas, next, fill, NOTE_INK, Glyph::Next);
        }
        let editing = matches!(view.editing, Some(DashboardEdit::Note(index)) if index == view.note_shown);
        let title = if editing && note.title.is_empty() {
            "Écrire une note…".to_owned()
        } else if editing && note.preview.is_empty() {
            format!("{}|", note.title)
        } else {
            note.title.clone()
        };
        let title = canvas.fit_text(&self.bold, &title, px(17.0), width);
        canvas.text(&self.bold, &title, x, card.y + 49.0, 17.0, NOTE_INK);
        let body = if editing && !note.preview.is_empty() {
            format!("{}|", note.preview)
        } else {
            note.preview.clone()
        };
        let lines = wrap_dashboard_text(&self.regular, &body, px(13.0), width);
        let room = ((card.bottom() - 14.0 - (card.y + 76.0)) / 19.5).floor().max(0.0) as usize;
        for (index, line) in lines.iter().take(room).enumerate() {
            let mut line = line.clone();
            if index + 1 == room && lines.len() > room {
                line = canvas.fit_text(&self.regular, &format!("{line} …"), px(13.0), width);
            }
            canvas.text(
                &self.regular,
                &line,
                x,
                card.y + 76.0 + index as f64 * 19.5,
                13.0,
                NOTE_BODY,
            );
        }
    }

    /// Mini-Dock : les applications récemment utilisées, un point sous celles ouvertes.
    fn apps_card(&self, canvas: &mut Canvas, card: Rect, view: &DashboardView) {
        let x = card.x + 16.0;
        self.caption(canvas, "Applications récentes", x, card.y + 16.0, DIM);
        if view.apps.is_empty() {
            let hint = canvas.fit_text(
                &self.regular,
                "Les applications utilisées apparaissent ici.",
                px(13.0),
                card.w - 32.0,
            );
            canvas.text(&self.regular, &hint, x, card.y + card.h / 2.0, 13.0, SOFT);
            return;
        }
        for (app, cell) in view.apps.iter().zip(app_cells(card, view.apps.len())) {
            let center = (cell.x + cell.w / 2.0, cell.y + 24.0);
            canvas.app_icon(&app.icon, center, 44.0);
            let name = canvas.fit_text(&self.regular, &app.name, px(11.0), cell.w - 6.0);
            let width = canvas.css_width(&self.regular, &name, 11.0);
            canvas.text(
                &self.regular,
                &name,
                cell.x + (cell.w - width) / 2.0,
                cell.y + 52.0,
                11.0,
                SOFT,
            );
            if app.running {
                canvas.fill(Rect::new(center.0 - 2.0, cell.y + 68.0, 4.0, 4.0), 2.0, INK);
            }
        }
    }

    /// Lecture en cours : pochette, titre, artiste, progression et boutons ; sans lecteur,
    /// une invitation à ouvrir Musique.
    fn music_card(&self, canvas: &mut Canvas, card: Rect, view: &DashboardView) {
        let media = view.media.as_ref();
        let art = music_art(card);
        if let Some(art) = art {
            match media.and_then(|media| media.cover.as_ref()) {
                Some(cover) => canvas.image_cover(art, cover, 16.0),
                None => {
                    // Note de musique sur un dégradé, comme sur l'écran de verrouillage.
                    canvas.gradient(art, 16.0, &[(0.0, 0xFF7E9D), (1.0, 0x7C4DE8)]);
                    let (ax, ay) = art.center();
                    let k = art.w / 48.0;
                    canvas.line(
                        (ax - 3.0 * k, ay + 7.0 * k),
                        (ax - 3.0 * k, ay - 6.0 * k),
                        2.0 * k,
                        Color::WHITE,
                    );
                    canvas.line(
                        (ax - 3.0 * k, ay - 6.0 * k),
                        (ax + 7.0 * k, ay - 8.0 * k),
                        2.0 * k,
                        Color::WHITE,
                    );
                    canvas.line(
                        (ax + 7.0 * k, ay - 8.0 * k),
                        (ax + 7.0 * k, ay + 5.0 * k),
                        2.0 * k,
                        Color::WHITE,
                    );
                    canvas.fill(
                        Rect::new(ax - 8.0 * k, ay + 4.5 * k, 5.2 * k, 5.2 * k),
                        (2.6 * k) as f32,
                        Color::WHITE,
                    );
                    canvas.fill(
                        Rect::new(ax + 2.0 * k, ay + 2.5 * k, 5.2 * k, 5.2 * k),
                        (2.6 * k) as f32,
                        Color::WHITE,
                    );
                }
            }
        }
        let x = art.map_or(card.x + 18.0, |art| art.right() + 16.0);
        let room = card.right() - 18.0 - x;
        let Some(media) = media else {
            self.caption(canvas, "Musique", x, card.y + 20.0, DIM);
            canvas.text(&self.bold, "Rien en lecture", x, card.y + 46.0, 17.0, INK);
            let hint = canvas.fit_text(&self.regular, "Cliquer pour ouvrir Musique", px(13.0), room);
            canvas.text(&self.regular, &hint, x, card.y + 70.0, 13.0, DIM);
            return;
        };
        let caption = if media.playing { "En lecture" } else { "En pause" };
        self.caption(canvas, caption, x, card.y + 20.0, DIM);
        let title = canvas.fit_text(&self.bold, &media.title, px(17.0), room);
        canvas.text(&self.bold, &title, x, card.y + 42.0, 17.0, INK);
        let artist = canvas.fit_text(&self.regular, &media.artist, px(13.0), room);
        canvas.text(&self.regular, &artist, x, card.y + 66.0, 13.0, DIM);
        let track = Rect::new(x, card.y + 92.0, room, 4.0);
        canvas.fill(track, 2.0, Color::WHITE.with_alpha(26));
        let filled = room * f64::from(media.progress.min(100)) / 100.0;
        canvas.fill(
            Rect::new(x, track.y, filled.max(4.0), 4.0),
            2.0,
            Color::rgb(0x6CB8FF),
        );
        let [previous, play, next] = music_buttons(card);
        let (px_, py_) = previous.center();
        canvas.polygon(
            &[(px_ + 5.0, py_ - 6.0), (px_ - 3.0, py_), (px_ + 5.0, py_ + 6.0)],
            INK,
        );
        canvas.line((px_ - 5.0, py_ - 6.0), (px_ - 5.0, py_ + 6.0), 1.8, INK);
        canvas.fill(play, 18.0, Color::WHITE.with_alpha(36));
        let (cx, cy) = play.center();
        if media.playing {
            canvas.fill(Rect::new(cx - 5.0, cy - 6.0, 3.5, 12.0), 1.0, INK);
            canvas.fill(Rect::new(cx + 1.5, cy - 6.0, 3.5, 12.0), 1.0, INK);
        } else {
            canvas.polygon(&[(cx - 3.5, cy - 6.5), (cx + 6.0, cy), (cx - 3.5, cy + 6.5)], INK);
        }
        let (nx, ny) = next.center();
        canvas.polygon(&[(nx - 5.0, ny - 6.0), (nx + 3.0, ny), (nx - 5.0, ny + 6.0)], INK);
        canvas.line((nx + 5.0, ny - 6.0), (nx + 5.0, ny + 6.0), 1.8, INK);
    }

    /// Note rapide : un grand bouton pour écrire une note tout de suite.
    fn quick_note_card(&self, canvas: &mut Canvas, card: Rect, view: &DashboardView) {
        let x = card.x + 18.0;
        self.caption(canvas, "Note rapide", x, card.y + 20.0, DIM);
        let button = quick_button(card);
        canvas.stroke(button, 16.0, Color::WHITE.with_alpha(90), 1.5);
        let label = "Écrire";
        let width = canvas.css_width(&self.bold, label, 13.0) + 22.0;
        let (cx, cy) = button.center();
        let left = cx - width / 2.0;
        canvas.line((left + 2.0, cy), (left + 12.0, cy), 2.0, INK);
        canvas.line((left + 7.0, cy - 5.0), (left + 7.0, cy + 5.0), 2.0, INK);
        canvas.text(&self.bold, label, left + 22.0, cy - 9.0, 13.0, INK);
        let count = view.notes.len();
        let plural = if count > 1 { "s" } else { "" };
        canvas.text(
            &self.regular,
            &format!("{count} note{plural}"),
            x,
            card.bottom() - 30.0,
            12.0,
            DIM,
        );
    }

    /// Notes récentes : titre et date des dernières notes ; un clic l'affiche dans Notes.
    fn recent_notes_card(&self, canvas: &mut Canvas, card: Rect, state: &VisualState) {
        let view = &state.view.dashboard;
        let x = card.x + 18.0;
        self.caption(canvas, "Notes récentes", x, card.y + 20.0, DIM);
        let order = recent_notes(view);
        if order.is_empty() {
            canvas.text(
                &self.regular,
                "Aucune note pour l’instant",
                x,
                card.y + 52.0,
                13.0,
                DIM,
            );
            return;
        }
        for (row, (rect, &index)) in recent_rows(card, order.len()).iter().zip(&order).enumerate() {
            let note = &view.notes[index];
            if row > 0 {
                canvas.fill(
                    Rect::new(rect.x, rect.y, rect.w, 1.0),
                    0.0,
                    Color::WHITE.with_alpha(26),
                );
            }
            let stamp = note_timestamp(note.updated, state.clock);
            let stamp_width = canvas.css_width(&self.regular, &stamp, 12.0);
            canvas.text(
                &self.regular,
                &stamp,
                rect.right() - stamp_width,
                rect.y + 10.0,
                12.0,
                DIM,
            );
            let title = if note.title.is_empty() {
                "Sans titre"
            } else {
                note.title.as_str()
            };
            let title = canvas.fit_text(&self.bold, title, px(14.0), rect.w - stamp_width - 12.0);
            canvas.text(&self.bold, &title, rect.x, rect.y + 9.0, 14.0, INK);
        }
    }

    /// Note épinglée : la liste à cocher d'une note, avec son avancement.
    fn pinned_note_card(&self, canvas: &mut Canvas, card: Rect, view: &DashboardView) {
        let x = card.x + 18.0;
        let width = card.w - 36.0;
        self.caption(canvas, "Note épinglée", x, card.y + 20.0, DIM);
        let Some(index) = pinned_note(view) else {
            canvas.text(
                &self.regular,
                "Cliquez pour créer une note",
                x,
                card.y + 52.0,
                13.0,
                DIM,
            );
            return;
        };
        let note = &view.notes[index];
        let checks = crate::desktop::widgets::checklist(&note.lines);
        let done = checks.iter().filter(|(_, done, _)| *done).count();
        if !checks.is_empty() {
            let text = format!("{done}/{} faites", checks.len());
            let text_width = canvas.css_width(&self.regular, &text, 12.0);
            canvas.text(
                &self.regular,
                &text,
                card.right() - 18.0 - text_width,
                card.y + 19.0,
                12.0,
                DIM,
            );
        }
        let title = if note.title.is_empty() {
            "Sans titre"
        } else {
            note.title.as_str()
        };
        let title = canvas.fit_text(&self.bold, title, px(18.0), width);
        canvas.text(&self.bold, &title, x, card.y + 44.0, 18.0, INK);
        if checks.is_empty() {
            // Pas de liste : le texte de la note.
            let lines = wrap_dashboard_text(&self.regular, &note.preview, px(13.0), width);
            let room = ((card.bottom() - 40.0 - (card.y + 76.0)) / 19.5).floor().max(0.0) as usize;
            for (row, line) in lines.iter().take(room).enumerate() {
                canvas.text(
                    &self.regular,
                    line,
                    x,
                    card.y + 76.0 + row as f64 * 19.5,
                    13.0,
                    DIM,
                );
            }
            canvas.text(
                &self.regular,
                "Lignes « - [ ] » : une liste à cocher",
                x,
                card.bottom() - 28.0,
                11.0,
                DIM,
            );
            return;
        }
        let track = Rect::new(x, card.y + 76.0, width, 4.0);
        canvas.fill(track, 2.0, Color::WHITE.with_alpha(31));
        let filled = width * done as f64 / checks.len() as f64;
        if filled > 0.0 {
            canvas.fill(Rect::new(x, track.y, filled, 4.0), 2.0, Color::rgb(0x6CB8FF));
        }
        let accent = Color::rgb(0x2779E0);
        for (rect, (_, done, text)) in pinned_rows(card, checks.len()).iter().zip(&checks) {
            let (cx, cy) = (rect.x + 9.0, rect.y + 17.0);
            let ring = Rect::new(cx - 9.0, cy - 9.0, 18.0, 18.0);
            if *done {
                canvas.fill(ring, 9.0, accent);
                canvas.line((cx - 4.0, cy + 0.2), (cx - 1.2, cy + 3.0), 1.8, Color::WHITE);
                canvas.line((cx - 1.2, cy + 3.0), (cx + 4.0, cy - 2.6), 1.8, Color::WHITE);
            } else {
                canvas.stroke(ring, 9.0, Color::WHITE.with_alpha(115), 1.6);
            }
            let ink = if *done { Color::rgb(0x7D8896) } else { INK };
            let label = canvas.fit_text(&self.regular, text, px(14.0), rect.w - 30.0);
            canvas.text(&self.regular, &label, rect.x + 28.0, cy - 9.5, 14.0, ink);
            if *done {
                let strike = canvas.css_width(&self.regular, &label, 14.0);
                canvas.line(
                    (rect.x + 28.0, cy + 0.5),
                    (rect.x + 28.0 + strike, cy + 0.5),
                    1.2,
                    ink,
                );
            }
        }
        canvas.text(
            &self.regular,
            "Un clic coche ou décoche la ligne",
            x,
            card.bottom() - 28.0,
            11.0,
            DIM,
        );
    }

    fn quote_card(&self, canvas: &mut Canvas, card: Rect) {
        let x = card.x + 22.0;
        let text_x = x + 44.0;
        let lines = wrap_dashboard_text(
            &self.regular,
            "Chaque petit pas compte.",
            px(17.0),
            card.right() - 22.0 - text_x,
        );
        let height = lines.len() as f64 * 24.0 + 22.0;
        let top = card.y + (card.h - height) / 2.0;
        canvas.text(
            &self.bold,
            "“",
            x,
            card.y + card.h / 2.0 - 21.0,
            56.0,
            Color::rgb(0x7FDCE3),
        );
        for (index, line) in lines.iter().enumerate() {
            canvas.text(&self.regular, line, text_x, top + index as f64 * 24.0, 17.0, INK);
        }
        canvas.text(
            &self.regular,
            "Citation du jour",
            text_x,
            top + lines.len() as f64 * 24.0 + 6.0,
            12.0,
            DIM,
        );
    }

    /// Mode d'édition : contour, poignée et pastille d'un widget, à sa place dans la pile.
    fn draw_handles(&self, canvas: &mut Canvas, card: Rect, kind: WidgetKind, selected: bool) {
        let edge = if selected {
            Color::rgb(0x3CC7D1)
        } else {
            Color::WHITE.with_alpha(90)
        };
        canvas.stroke(card, RADIUS, edge, if selected { 2.5 } else { 1.0 });
        let ink = if kind == WidgetKind::Notes {
            NOTE_INK.with_alpha(150)
        } else {
            Color::WHITE.with_alpha(190)
        };
        let (right, bottom) = (card.right() - 9.0, card.bottom() - 9.0);
        canvas.line((right - 12.0, bottom), (right, bottom - 12.0), 1.8, ink);
        canvas.line((right - 6.0, bottom), (right, bottom - 6.0), 1.8, ink);
        self.round_button(
            canvas,
            remove_badge(card),
            Color::rgb(0xE5574A),
            Color::WHITE,
            Glyph::Minus,
        );
    }

    /// Barre de l'éditeur : widgets à ajouter, Rétablir et Terminé.
    fn draw_editor_bar(&self, canvas: &mut Canvas, layout: &Layout, view: &DashboardView) {
        let bar = layout.editor_bar();
        canvas.frosted(bar, 22.0, view.backdrop.as_deref(), None);
        for (index, kind) in Dashboard::absent_widgets(&view.layout).into_iter().enumerate() {
            let chip = add_chip(bar, index);
            canvas.fill(chip, 16.0, Color::WHITE.with_alpha(26));
            let text = format!("+ {}", kind.label());
            let text = canvas.fit_text(&self.regular, &text, px(12.0), chip.w - 12.0);
            let width = canvas.css_width(&self.regular, &text, 12.0);
            canvas.text(
                &self.regular,
                &text,
                chip.x + (chip.w - width) / 2.0,
                chip.y + 8.5,
                12.0,
                INK,
            );
        }
        let reset = reset_button(bar);
        canvas.fill(reset, 16.0, Color::WHITE.with_alpha(26));
        let width = canvas.css_width(&self.regular, "Rétablir", 13.0);
        canvas.text(
            &self.regular,
            "Rétablir",
            reset.x + (reset.w - width) / 2.0,
            reset.y + 8.0,
            13.0,
            INK,
        );
        let done = done_button(bar);
        canvas.fill(done, 16.0, Color::rgb(0x0B7C86));
        let width = canvas.css_width(&self.bold, "Terminé", 13.0);
        canvas.text(
            &self.bold,
            "Terminé",
            done.x + (done.w - width) / 2.0,
            done.y + 8.0,
            13.0,
            Color::WHITE,
        );
    }
}

/// Dessin d'un petit bouton rond.
#[derive(Clone, Copy)]
enum Glyph {
    Plus,
    Minus,
    Previous,
    Next,
    Trash,
}

/// Fond d'une carte : papier pour les notes, verre dépoli pour les autres.
fn card_glass(
    canvas: &mut Canvas,
    notes: bool,
    card: Rect,
    backdrop: Option<&Image>,
    placement: Option<Placement>,
) {
    if notes {
        canvas.fill(card, RADIUS, NOTE_FILL);
    } else {
        canvas.frosted(card, RADIUS, backdrop, placement);
    }
}

/// Case visée par le widget déplacé : en creux, liseré blanc.
pub(super) fn target_hollow(canvas: &mut Canvas, rect: Rect) {
    canvas.fill(rect, RADIUS, Color::WHITE.with_alpha(38));
    canvas.stroke(rect, RADIUS, Color::WHITE.with_alpha(110), 1.5);
}

/// Place sur l'écran d'une image qui n'en couvre qu'une partie : origine logique de
/// l'image et taille de l'écran en pixels.
#[derive(Debug, Clone, Copy)]
pub(super) struct Placement {
    pub x: f64,
    pub y: f64,
    pub screen: (f64, f64),
}
