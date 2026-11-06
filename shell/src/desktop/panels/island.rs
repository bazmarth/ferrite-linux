//! Îlot du haut de l'écran : une pastille noire autour de l'heure de la barre, qui montre
//! ce qui se passe (minuteur, enregistrement, musique) et s'agrandit un instant pour le
//! son, la luminosité, une notification ou la fin d'un minuteur.
//!
//! Il a ses propres images, petites : la forme noire, redessinée à chaque pas quand elle
//! change de taille, et son contenu, dessiné une fois à sa taille d'arrivée puis étiré
//! avec elle et révélé en fondu. Le reste du bureau n'est jamais redessiné pour lui.

use super::*;

/// Durée d'un changement de forme.
const MORPH: Duration = ferrite_ui::tokens::motion::LONG;
/// Largeur de la pastille repliée autour de l'heure.
const COMPACT_W: f64 = 340.0;
/// Côtés de la pastille repliée où se lit l'activité (le milieu reste l'horloge).
const SIDE_W: f64 = 74.0;
/// Pastille de départ et d'arrivée quand l'îlot apparaît ou s'efface.
const SEED_W: f64 = 200.0;
/// Haut de l'îlot déplié, et marge autour de sa forme pour son ombre.
const OPEN_TOP: f64 = 6.0;
const SHADOW: f64 = 6.0;

const ISLAND_BG: Color = Color::rgb(0x050506);
const INK: Color = Color::WHITE;
const DIM: Color = Color::rgb(0x9A9AA2);
const ORANGE: Color = Color::rgb(0xFF9F0A);
const RED: Color = Color::rgb(0xFF453A);
const GREEN: Color = Color::rgb(0x30D158);
const BUTTON: Color = Color::rgb(0x2C2C30);

/// Ce que montre l'îlot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IslandKind {
    /// Replié : enregistrement vidéo en cours (durée).
    Recording(String),
    /// Replié : minuteur (reste, en pause, part écoulée sur 1000).
    Timer { left: String, paused: bool, done: u16 },
    /// Replié : musique.
    Media { playing: bool },
    /// Déplié : touche de son, de luminosité, de micro.
    Osd(Osd),
    /// Déplié : dernière notification, et nombre d'autres en attente.
    Notification {
        notification: NotificationView,
        more: usize,
    },
    /// Déplié : minuteur arrivé à zéro.
    TimerDone,
    /// Déplié au clic : minuteur avec ses boutons.
    TimerOpen { left: String, paused: bool, done: u16 },
    /// Déplié au clic : lecteur avec ses boutons.
    MediaOpen {
        title: String,
        artist: String,
        playing: bool,
        cover: Option<Thumbnail>,
    },
}

impl IslandKind {
    /// Pastille repliée dans la barre, plutôt qu'annonce dépliée.
    pub fn compact(&self) -> bool {
        matches!(self, Self::Recording(_) | Self::Timer { .. } | Self::Media { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IslandView {
    pub kind: Option<IslandKind>,
}

/// Endroit cliqué dans l'îlot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandHit {
    /// Un côté de la pastille repliée : l'activité.
    Side,
    /// Un bouton de l'îlot déplié, de gauche à droite.
    Button(usize),
    /// Ailleurs sur l'îlot déplié.
    Body,
}

impl Layout {
    /// Pastille repliée, centrée dans la barre.
    fn island_compact(&self) -> Rect {
        let h = (self.bar - 6.0).max(24.0);
        Rect::new(
            ((self.w - COMPACT_W) / 2.0).round(),
            ((self.bar - h) / 2.0).round(),
            COMPACT_W,
            h,
        )
    }

    /// Forme d'où l'îlot naît et où il s'efface : une pastille juste autour de l'heure.
    fn island_seed(&self) -> Rect {
        let compact = self.island_compact();
        Rect::new(((self.w - SEED_W) / 2.0).round(), compact.y, SEED_W, compact.h)
    }

    /// Place de l'îlot, s'il est affiché.
    pub(super) fn island(&self, island: &IslandView) -> Option<Rect> {
        let kind = island.kind.as_ref()?;
        if kind.compact() {
            return Some(self.island_compact());
        }
        let (w, h) = match kind {
            IslandKind::Osd(_) => (380.0, 64.0),
            IslandKind::Notification { notification, .. } => (
                440.0,
                if notification.actions.is_empty() {
                    84.0
                } else {
                    126.0
                },
            ),
            IslandKind::TimerDone => (400.0, 72.0),
            IslandKind::TimerOpen { .. } => (420.0, 88.0),
            _ => (440.0, 96.0),
        };
        Some(Rect::new(((self.w - w) / 2.0).round(), OPEN_TOP, w, h))
    }

    /// Boutons de l'îlot déplié, de gauche à droite.
    pub(super) fn island_buttons(&self, island: &IslandView) -> Vec<Rect> {
        let (Some(kind), Some(rect)) = (island.kind.as_ref(), self.island(island)) else {
            return Vec::new();
        };
        let round = |index: usize, count: usize, side: f64| {
            let right = rect.right() - 18.0 - (count - 1 - index) as f64 * (side + 12.0);
            Rect::new(right - side, rect.center().1 - side / 2.0, side, side)
        };
        match kind {
            IslandKind::Notification { notification, .. } => (0..notification.actions.len())
                .map(|index| {
                    Rect::new(
                        rect.x + 66.0 + index as f64 * 100.0,
                        rect.bottom() - 46.0,
                        92.0,
                        32.0,
                    )
                })
                .collect(),
            IslandKind::TimerDone => vec![Rect::new(
                rect.right() - 16.0 - 104.0,
                rect.center().1 - 18.0,
                104.0,
                36.0,
            )],
            IslandKind::TimerOpen { .. } => (0..2).map(|index| round(index, 2, 46.0)).collect(),
            IslandKind::MediaOpen { .. } => (0..3).map(|index| round(index, 3, 40.0)).collect(),
            _ => Vec::new(),
        }
    }
}

/// Endroit de l'îlot sous `pos`. Le milieu de la pastille repliée n'en fait pas partie :
/// c'est l'heure, qui ouvre toujours le calendrier.
pub(super) fn island_at(layout: &Layout, island: &IslandView, pos: Point<f64, Logical>) -> Option<IslandHit> {
    let kind = island.kind.as_ref()?;
    let rect = layout.island(island)?;
    if !rect.contains(pos) {
        return None;
    }
    if kind.compact() {
        let side = pos.x < rect.x + SIDE_W || pos.x >= rect.right() - SIDE_W;
        return side.then_some(IslandHit::Side);
    }
    let button = layout
        .island_buttons(island)
        .iter()
        .position(|button| button.contains(pos));
    Some(button.map_or(IslandHit::Body, IslandHit::Button))
}

/// Le point `pos` (coordonnées de l'écran `output`) est-il sur l'îlot affiché ?
pub fn island_contains(
    output: Rectangle<i32, Logical>,
    pos: Point<f64, Logical>,
    view: &DesktopView,
) -> bool {
    let layout = Layout::new(output.size.w as f64, output.size.h as f64, view);
    let pos = Point::from((pos.x - output.loc.x as f64, pos.y - output.loc.y as f64));
    layout.island(&view.island).is_some_and(|rect| rect.contains(pos))
}

/// Mouvement de l'îlot d'une image à l'autre.
#[derive(Debug, Default)]
pub(super) struct IslandMotion {
    /// Place visée, forme au départ du mouvement, et son heure.
    target: Option<Rect>,
    from: Option<Rect>,
    started: Option<Instant>,
    /// Forme dessinée à l'image précédente.
    shown: Option<Rect>,
    /// Contenu dessiné : ce qu'il montre, son heure, l'échelle, sa taille, ses pixels.
    content: Option<(IslandKind, String, u32, Rect, MemoryRenderBuffer)>,
    /// Forme noire dessinée : taille en pixels physiques, et ses pixels.
    shape: Option<((u32, u32), MemoryRenderBuffer)>,
}

/// Pas d'un mouvement : un léger dépassement à l'arrivée, comme un ressort.
fn spring(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let c = 1.2;
    1.0 + (c + 1.0) * (t - 1.0).powi(3) + c * (t - 1.0).powi(2)
}

fn mix(from: Rect, to: Rect, k: f64) -> Rect {
    let lerp = |a: f64, b: f64| a + (b - a) * k;
    Rect::new(
        lerp(from.x, to.x),
        lerp(from.y, to.y),
        lerp(from.w, to.w).max(1.0),
        lerp(from.h, to.h).max(1.0),
    )
}

/// Rayon de la forme : une pilule quand elle est basse, des coins doux sinon.
fn island_radius(rect: Rect) -> f32 {
    (rect.h / 2.0).min(26.0) as f32
}

impl Panels {
    /// Pose l'îlot (forme puis contenu) dans `out`, devant le reste du bureau.
    pub(super) fn push_island(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        layout: &Layout,
        scale: f64,
        view: &DesktopView,
        out: &mut Vec<crate::render::OutputElement>,
    ) {
        let mut motion = self.island.borrow_mut();
        let target = layout.island(&view.island);
        let now = Instant::now();
        if target != motion.target {
            motion.from = Some(motion.shown.unwrap_or_else(|| layout.island_seed()));
            motion.target = target;
            motion.started = Some(now);
        }
        let t = motion
            .started
            .map_or(1.0, |started| {
                now.duration_since(started).as_secs_f64() / MORPH.as_secs_f64()
            })
            .min(1.0);
        let from = motion.from.unwrap_or_else(|| layout.island_seed());
        let (rect, alpha, content_alpha) = match target {
            Some(to) => {
                let moved = from != to;
                let reveal = if moved {
                    ((t - 0.3) / 0.7).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                (mix(from, to, spring(t)), 1.0, reveal)
            }
            None if t >= 1.0 => {
                motion.shown = None;
                motion.content = None;
                return;
            }
            None => {
                let k = crate::desktop::dock::ease(t);
                (
                    mix(from, layout.island_seed(), k),
                    1.0 - k,
                    (1.0 - t * 2.5).max(0.0),
                )
            }
        };
        motion.shown = Some(rect);

        // Contenu, à sa taille d'arrivée.
        let clock = clock_text(view.clock_style, clock_label(SystemTime::now(), local_offset()));
        let scale_milli = (scale * 1000.0).round() as u32;
        if let (Some(kind), Some(to)) = (view.island.kind.as_ref(), target) {
            let fresh = motion
                .content
                .as_ref()
                .is_some_and(|(known, known_clock, known_scale, size, _)| {
                    known == kind && *known_clock == clock && *known_scale == scale_milli && *size == to
                });
            if !fresh {
                let buffer = crate::render::profile::raster("îlot", || {
                    self.rasterize_island(kind, &clock, to, scale)
                });
                motion.content = Some((kind.clone(), clock.clone(), scale_milli, to, buffer));
            }
        }

        // Forme noire, à la taille de cette image.
        let size = (
            ((rect.w + SHADOW * 2.0) * scale).ceil() as u32,
            ((rect.h + SHADOW * 2.0) * scale).ceil() as u32,
        );
        if !motion.shape.as_ref().is_some_and(|(known, _)| *known == size) {
            let mut canvas = Canvas {
                pixels: vec![0; size.0 as usize * size.1 as usize * 4],
                width: size.0,
                height: size.1,
                scale: scale as f32,
                bold: &self.bold,
                icons: &self.icons,
            };
            let local = Rect::new(SHADOW, SHADOW, rect.w, rect.h);
            let radius = island_radius(local);
            for (grow, shade) in [(3.0, 16), (2.0, 22), (1.0, 34)] {
                canvas.fill(
                    Rect::new(
                        local.x - grow,
                        local.y - grow + 1.5,
                        local.w + grow * 2.0,
                        local.h + grow * 2.0,
                    ),
                    radius + grow as f32,
                    Color::BLACK.with_alpha(shade),
                );
            }
            canvas.fill(local, radius, ISLAND_BG);
            let buffer = MemoryRenderBuffer::from_slice(
                &canvas.pixels,
                Fourcc::Argb8888,
                (size.0 as i32, size.1 as i32),
                1,
                smithay::utils::Transform::Normal,
                None,
            );
            motion.shape = Some((size, buffer));
        }

        let mut push = |buffer: &MemoryRenderBuffer, place: Rect, pixels: (u32, u32), alpha: f64| {
            if alpha <= 0.0 {
                return;
            }
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                Point::<f64, Logical>::from((place.x, place.y)).to_physical(scale),
                buffer,
                (alpha < 1.0).then_some(alpha as f32),
                Some(Rectangle::<f64, Logical>::from_size(
                    (f64::from(pixels.0), f64::from(pixels.1)).into(),
                )),
                Some(Size::from((
                    place.w.round().max(1.0) as i32,
                    place.h.round().max(1.0) as i32,
                ))),
                Kind::Unspecified,
            ) {
                Ok(element) => out.push(crate::render::OutputElement::Memory(element)),
                Err(err) => tracing::warn!(?err, output = output.name(), "îlot non importé"),
            }
        };
        if let Some((_, _, _, size, buffer)) = motion.content.as_ref() {
            let pixels = (
                (size.w * scale).round().max(1.0) as u32,
                (size.h * scale).round().max(1.0) as u32,
            );
            push(buffer, rect, pixels, content_alpha * alpha);
        }
        if let Some((pixels, buffer)) = motion.shape.as_ref() {
            push(buffer, rect.expand(SHADOW), *pixels, alpha);
        }
    }

    /// Contenu de l'îlot, sur fond transparent, à la taille `rect`.
    fn rasterize_island(&self, kind: &IslandKind, clock: &str, rect: Rect, scale: f64) -> MemoryRenderBuffer {
        let width = (rect.w * scale).round().max(1.0) as u32;
        let height = (rect.h * scale).round().max(1.0) as u32;
        let mut canvas = Canvas {
            pixels: vec![0; width as usize * height as usize * 4],
            width,
            height,
            scale: scale as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        let local = Rect::new(0.0, 0.0, rect.w, rect.h);
        if kind.compact() {
            self.draw_island_compact(&mut canvas, local, kind, clock);
        } else {
            self.draw_island_open(&mut canvas, local, kind);
        }
        MemoryRenderBuffer::from_slice(
            &canvas.pixels,
            Fourcc::Argb8888,
            (width as i32, height as i32),
            1,
            smithay::utils::Transform::Normal,
            None,
        )
    }

    /// Pastille repliée : l'heure au milieu, l'activité de part et d'autre.
    fn draw_island_compact(&self, canvas: &mut Canvas, rect: Rect, kind: &IslandKind, clock: &str) {
        let cy = rect.center().1;
        let clock_width = canvas.css_width(&self.bold, clock, 14.0);
        canvas.text(
            &self.bold,
            clock,
            (rect.center().0 - clock_width / 2.0).round(),
            cy - 9.0,
            14.0,
            INK,
        );
        let left = rect.x + 24.0;
        let right = rect.right() - 20.0;
        let value = |canvas: &mut Canvas, text: &str, color: Color| {
            let width = canvas.css_width(&self.bold, text, 13.0);
            canvas.text(&self.bold, text, right - width, cy - 8.5, 13.0, color);
        };
        match kind {
            IslandKind::Recording(elapsed) => {
                canvas.fill(Rect::new(left - 5.0, cy - 5.0, 10.0, 10.0), 5.0, RED);
                value(canvas, elapsed, RED);
            }
            IslandKind::Timer {
                left: text,
                paused,
                done,
            } => {
                let color = if *paused { DIM } else { ORANGE };
                ring(
                    canvas,
                    (left + 2.0, cy),
                    8.0,
                    2.4,
                    f64::from(*done) / 1000.0,
                    color,
                );
                value(canvas, text, color);
            }
            IslandKind::Media { playing } => {
                canvas.icon("app-music", (left + 2.0, cy), 18.0, Style::Color);
                // Égaliseur : des barres hautes en lecture, basses en pause.
                let heights: [f64; 4] = if *playing {
                    [7.0, 13.0, 9.0, 11.0]
                } else {
                    [3.0; 4]
                };
                let color = if *playing { GREEN } else { DIM };
                for (index, height) in heights.iter().enumerate() {
                    let x = right - 22.0 + index as f64 * 6.0;
                    canvas.fill(Rect::new(x, cy - height / 2.0, 3.0, *height), 1.5, color);
                }
            }
            _ => {}
        }
    }

    /// Îlot déplié.
    fn draw_island_open(&self, canvas: &mut Canvas, rect: Rect, kind: &IslandKind) {
        let layout_buttons = |count: usize, side: f64| -> Vec<Rect> {
            (0..count)
                .map(|index| {
                    let right = rect.right() - 18.0 - (count - 1 - index) as f64 * (side + 12.0);
                    Rect::new(right - side, rect.center().1 - side / 2.0, side, side)
                })
                .collect()
        };
        let cy = rect.center().1;
        match kind {
            IslandKind::Osd(osd) => {
                canvas.icon(osd.icon, (rect.x + 36.0, cy), 24.0, Style::Symbolic(INK));
                let x = rect.x + 64.0;
                let Some(level) = osd.level else {
                    canvas.text(&self.bold, osd.label, x, cy - 9.0, 15.0, INK);
                    return;
                };
                let percent = format!("{} %", level.min(100));
                let percent_width = canvas.css_width(&self.bold, &percent, 13.0);
                canvas.text(&self.bold, osd.label, x, rect.y + 13.0, 13.0, DIM);
                canvas.text(
                    &self.bold,
                    &percent,
                    rect.right() - 22.0 - percent_width,
                    rect.y + 13.0,
                    13.0,
                    INK,
                );
                let track = Rect::new(x, rect.y + 40.0, rect.right() - 22.0 - x, 6.0);
                canvas.fill(track, 3.0, BUTTON);
                let filled = track.w * f64::from(level.min(100)) / 100.0;
                if filled > 0.0 {
                    canvas.fill(Rect::new(track.x, track.y, filled.max(6.0), track.h), 3.0, INK);
                }
            }
            IslandKind::Notification { notification, more } => {
                let icon = (rect.x + 36.0, rect.y + 40.0);
                match &notification.icon {
                    Some(app) => canvas.app_icon(app, icon, 36.0),
                    None => canvas.icon("status-notifications", icon, 26.0, Style::Symbolic(INK)),
                }
                let x = rect.x + 66.0;
                let right = rect.right() - 20.0;
                let when = if *more > 0 {
                    format!("{} · +{more}", notification.when)
                } else {
                    notification.when.clone()
                };
                let when_width = canvas.css_width(&self.regular, &when, 12.0);
                canvas.text(&self.regular, &when, right - when_width, rect.y + 16.0, 12.0, DIM);
                let app = canvas.fit_text(
                    &self.regular,
                    &notification.app,
                    px(12.0),
                    right - when_width - 10.0 - x,
                );
                canvas.text(&self.regular, &app, x, rect.y + 16.0, 12.0, DIM);
                let title = if notification.summary.is_empty() {
                    &notification.app
                } else {
                    &notification.summary
                };
                let title = canvas.fit_text(&self.bold, title, px(14.0), right - x);
                canvas.text(&self.bold, &title, x, rect.y + 34.0, 14.0, INK);
                if !notification.body.is_empty() {
                    let body = canvas.fit_text(&self.regular, &notification.body, px(13.0), right - x);
                    canvas.text(&self.regular, &body, x, rect.y + 54.0, 13.0, Color::rgb(0xC7C7CC));
                }
                for (index, label) in notification.actions.iter().enumerate() {
                    let button = Rect::new(
                        rect.x + 66.0 + index as f64 * 100.0,
                        rect.bottom() - 46.0,
                        92.0,
                        32.0,
                    );
                    let (fill, ink) = if index == 0 {
                        (INK, Color::BLACK)
                    } else {
                        (BUTTON, INK)
                    };
                    canvas.fill(button, 16.0, fill);
                    let label = canvas.fit_text(&self.bold, label, px(13.0), button.w - 16.0);
                    centered_text(canvas, &self.bold, &label, button, 13.0, ink);
                }
            }
            IslandKind::TimerDone => {
                ring(canvas, (rect.x + 38.0, cy), 15.0, 3.0, 1.0, ORANGE);
                canvas.text(&self.regular, "Minuteur", rect.x + 66.0, cy - 19.0, 12.0, DIM);
                canvas.text(&self.bold, "Temps écoulé", rect.x + 66.0, cy - 2.0, 16.0, INK);
                let button = Rect::new(rect.right() - 16.0 - 104.0, cy - 18.0, 104.0, 36.0);
                canvas.fill(button, 18.0, ORANGE);
                centered_text(canvas, &self.bold, "Arrêter", button, 14.0, Color::BLACK);
            }
            IslandKind::TimerOpen { left, paused, done } => {
                let color = if *paused { DIM } else { ORANGE };
                ring(
                    canvas,
                    (rect.x + 42.0, cy),
                    20.0,
                    4.0,
                    f64::from(*done) / 1000.0,
                    color,
                );
                let label = if *paused {
                    "Minuteur · en pause"
                } else {
                    "Minuteur"
                };
                canvas.text(&self.regular, label, rect.x + 76.0, cy - 24.0, 12.0, DIM);
                canvas.text(&self.light, left, rect.x + 74.0, cy - 8.0, 30.0, color);
                let buttons = layout_buttons(2, 46.0);
                canvas.fill(buttons[0], 23.0, color.with_alpha(64));
                let (bx, by) = buttons[0].center();
                if *paused {
                    canvas.polygon(
                        &[(bx - 5.0, by - 8.0), (bx + 9.0, by), (bx - 5.0, by + 8.0)],
                        color,
                    );
                } else {
                    canvas.fill(Rect::new(bx - 7.0, by - 8.0, 5.0, 16.0), 1.5, color);
                    canvas.fill(Rect::new(bx + 2.0, by - 8.0, 5.0, 16.0), 1.5, color);
                }
                canvas.fill(buttons[1], 23.0, BUTTON);
                let (sx, sy) = buttons[1].center();
                canvas.line((sx - 6.0, sy - 6.0), (sx + 6.0, sy + 6.0), 2.4, INK);
                canvas.line((sx + 6.0, sy - 6.0), (sx - 6.0, sy + 6.0), 2.4, INK);
            }
            IslandKind::MediaOpen {
                title,
                artist,
                playing,
                cover,
            } => {
                let art = Rect::new(rect.x + 16.0, cy - 30.0, 60.0, 60.0);
                match cover {
                    Some(cover) => canvas.image_cover(art, cover, 12.0),
                    None => {
                        canvas.fill(art, 12.0, BUTTON);
                        canvas.icon("app-music", art.center(), 36.0, Style::Color);
                    }
                }
                let buttons = layout_buttons(3, 40.0);
                let x = art.right() + 14.0;
                let room = buttons[0].x - 14.0 - x;
                let title = canvas.fit_text(&self.bold, title, px(15.0), room);
                canvas.text(&self.bold, &title, x, cy - 20.0, 15.0, INK);
                let artist = canvas.fit_text(&self.regular, artist, px(13.0), room);
                canvas.text(&self.regular, &artist, x, cy + 2.0, 13.0, DIM);
                for (index, button) in buttons.iter().enumerate() {
                    let (bx, by) = button.center();
                    if index == 1 {
                        canvas.fill(*button, 20.0, INK);
                        if *playing {
                            canvas.fill(Rect::new(bx - 6.0, by - 7.0, 4.5, 14.0), 1.5, Color::BLACK);
                            canvas.fill(Rect::new(bx + 1.5, by - 7.0, 4.5, 14.0), 1.5, Color::BLACK);
                        } else {
                            canvas.polygon(
                                &[(bx - 4.0, by - 8.0), (bx + 8.0, by), (bx - 4.0, by + 8.0)],
                                Color::BLACK,
                            );
                        }
                        continue;
                    }
                    // Précédent et suivant : deux triangles accolés.
                    let direction = if index == 0 { -1.0 } else { 1.0 };
                    for offset in [-5.0, 4.0] {
                        let tip = bx + (offset + 5.0) * direction;
                        let base = tip - 9.0 * direction;
                        canvas.polygon(&[(base, by - 7.0), (tip, by), (base, by + 7.0)], INK);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Anneau de progression : piste discrète et arc de `fraction` (de midi, sens horaire).
fn ring(canvas: &mut Canvas, center: (f64, f64), radius: f64, width: f64, fraction: f64, color: Color) {
    let arc = |canvas: &mut Canvas, fraction: f64, color: Color| {
        let steps = ((fraction * 72.0).ceil() as usize).max(2);
        let point = |r: f64, step: usize| {
            let angle =
                -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * fraction * step as f64 / steps as f64;
            (center.0 + r * angle.cos(), center.1 + r * angle.sin())
        };
        let (outer, inner) = (radius + width / 2.0, radius - width / 2.0);
        let mut points: Vec<(f64, f64)> = (0..=steps).map(|step| point(outer, step)).collect();
        points.extend((0..=steps).rev().map(|step| point(inner, step)));
        canvas.polygon(&points, color);
    };
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction < 1.0 {
        arc(canvas, 0.9999, color.with_alpha(56));
    }
    if fraction > 0.0 {
        arc(canvas, fraction.min(0.9999), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        let view = DesktopView {
            bar_visible: true,
            bar_height: 34,
            ..DesktopView::default()
        };
        Layout::new(1440.0, 900.0, &view)
    }

    #[test]
    fn le_milieu_de_la_pastille_reste_l_horloge() {
        let layout = layout();
        let island = IslandView {
            kind: Some(IslandKind::Timer {
                left: "4:59".to_owned(),
                paused: false,
                done: 10,
            }),
        };
        let rect = layout.island(&island).expect("affiché");
        assert!(rect.y >= 0.0 && rect.bottom() <= 34.0);
        let at = |x: f64| Point::from((x, rect.center().1));
        assert_eq!(
            island_at(&layout, &island, at(rect.x + 10.0)),
            Some(IslandHit::Side)
        );
        assert_eq!(
            island_at(&layout, &island, at(rect.right() - 10.0)),
            Some(IslandHit::Side)
        );
        assert_eq!(island_at(&layout, &island, at(rect.center().0)), None);
    }

    #[test]
    fn boutons_du_minuteur_deplie() {
        let layout = layout();
        let island = IslandView {
            kind: Some(IslandKind::TimerOpen {
                left: "4:59".to_owned(),
                paused: true,
                done: 10,
            }),
        };
        let buttons = layout.island_buttons(&island);
        assert_eq!(buttons.len(), 2);
        for (index, button) in buttons.iter().enumerate() {
            assert_eq!(
                island_at(&layout, &island, Point::from(button.center())),
                Some(IslandHit::Button(index))
            );
        }
        let rect = layout.island(&island).expect("affiché");
        assert_eq!(
            island_at(&layout, &island, Point::from((rect.x + 20.0, rect.y + 10.0))),
            Some(IslandHit::Body)
        );
    }

    #[test]
    fn l_ilot_deplie_sait_ce_qui_est_dessus() {
        let output = Rectangle::from_size((1440, 900).into());
        let view = DesktopView {
            bar_visible: true,
            bar_height: 34,
            island: IslandView {
                kind: Some(IslandKind::TimerDone),
            },
            ..DesktopView::default()
        };
        assert!(island_contains(output, Point::from((720.0, 30.0)), &view));
        assert!(!island_contains(output, Point::from((720.0, 400.0)), &view));
        assert!(!island_contains(output, Point::from((40.0, 30.0)), &view));
    }

    #[test]
    fn le_ressort_arrive_a_destination() {
        assert!(spring(0.0).abs() < 1e-9);
        assert!((spring(1.0) - 1.0).abs() < 1e-9);
        assert!((0..=100).any(|step| spring(f64::from(step) / 100.0) > 1.0));
    }
}
