//! Écran de verrouillage : au repos (heure, météo, notifications masquées, lecture en cours,
//! personnalisation), puis en saisie (avatar et mot de passe). Dessiné sur une toile de
//! 1600 × 1000, mise à l'échelle de l'écran.

use super::*;

/// Fonds proposés : nom, puis fond et trois taches de couleur, en sombre et en clair.
type WallColors = [u32; 4];
pub const LOCK_WALLS: [(&str, WallColors, WallColors); 4] = [
    (
        "Ferrite",
        [0x0E1622, 0x0E9AA7, 0x2779E0, 0x7C4DE8],
        [0xE9F3F5, 0x7FDCE3, 0x9CC7FF, 0xC9B6FF],
    ),
    (
        "Savane",
        [0x1C140E, 0xE5571A, 0xFFB547, 0xC2410C],
        [0xFBF2E6, 0xFFC98A, 0xFFB4A0, 0xFFE3A3],
    ),
    (
        "Océan",
        [0x0A1826, 0x1E6FD9, 0x0E9AA7, 0x3CC7D1],
        [0xE8F1FB, 0x9CC7FF, 0xA6E8EE, 0xC5D8FF],
    ),
    (
        "Lavande",
        [0x16122A, 0x7C4DE8, 0xDB2C57, 0x2779E0],
        [0xF3EFFB, 0xC9B6FF, 0xFFB8CC, 0xB9D3FF],
    ),
];

/// Part de la transition du repos à la saisie, à chaque extrémité.
const CLOCK_TOP: (f64, f64) = (170.0, 90.0);
const DATE_SIZE: (f64, f64) = (22.0, 16.0);
const TIME_SIZE: (f64, f64) = (168.0, 72.0);

const CARDS_TOP: f64 = 560.0;
const CARDS_WIDTH: f64 = 420.0;
const NOTICE_HEIGHT: f64 = 65.0;
const MEDIA_HEIGHT: f64 = 74.0;
const FIELD_TOP: f64 = 330.0 + 104.0 + 14.0 + 27.0 + 14.0;

const DAYS: [&str; 7] = [
    "dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi",
];
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

/// Couleurs d'un thème de l'écran.
struct Tokens {
    fg: Color,
    fg2: Color,
    fg3: Color,
    glass: Color,
    border: Color,
    chip: Color,
    button: Color,
    stack: [Color; 2],
    track: Color,
    bar: Color,
    field: Color,
    field_edge: Color,
    ring: Color,
    /// Voile sur le fond, au repos et en saisie.
    veil: (Color, Color),
    error: (Color, Color),
    /// Bague autour du fond choisi.
    swatch_gap: Color,
}

fn tokens(dark: bool) -> Tokens {
    let white = Color::WHITE;
    let ink = Color::rgb(0x14202B);
    if dark {
        Tokens {
            fg: Color::rgb(0xF3F6F9),
            fg2: Color::rgb(0xDDE5EE),
            fg3: Color::rgb(0xA9B4C2),
            glass: Color::rgb(0x161E2C).with_alpha(140),
            border: white.with_alpha(36),
            chip: white.with_alpha(26),
            button: white.with_alpha(26),
            stack: [white.with_alpha(20), white.with_alpha(28)],
            track: white.with_alpha(46),
            bar: white,
            field: white.with_alpha(31),
            field_edge: white.with_alpha(56),
            ring: white.with_alpha(41),
            veil: (
                Color::rgb(0x080C12).with_alpha(31),
                Color::rgb(0x080C12).with_alpha(102),
            ),
            error: (Color::rgb(0xF07A6E), Color::rgb(0xFFB4AB)),
            swatch_gap: Color::rgb(0x0E1622),
        }
    } else {
        Tokens {
            fg: ink,
            fg2: Color::rgb(0x2B3744),
            fg3: Color::rgb(0x4B5563),
            glass: white.with_alpha(140),
            border: white.with_alpha(204),
            chip: white.with_alpha(128),
            button: white.with_alpha(140),
            stack: [white.with_alpha(82), white.with_alpha(115)],
            track: ink.with_alpha(38),
            bar: ink,
            field: white.with_alpha(166),
            field_edge: ink.with_alpha(46),
            ring: white.with_alpha(217),
            veil: (white.with_alpha(15), white.with_alpha(82)),
            error: (Color::rgb(0xC42B20), Color::rgb(0xB42318)),
            swatch_gap: white,
        }
    }
}

fn mix(from: f64, to: f64, part: f64) -> f64 {
    from + (to - from) * part
}

fn rgb(color: u32) -> [f32; 3] {
    [(color >> 16) & 0xFF, (color >> 8) & 0xFF, color & 0xFF].map(|part| part as f32)
}

/// Toile de l'écran : un point vaut `scale` points de l'écran.
/// Valeur de `LockView::pulse` qui retire l'invitation de l'image de l'écran : elle a sa
/// propre petite image, dont la carte graphique fait varier l'opacité. Sinon tout l'écran
/// serait redessiné à chaque pas de la pulsation.
pub(super) const PULSE_APART: u8 = u8::MAX;

/// Opacité de l'invitation pour un niveau de pulsation de 0 à 8.
pub(super) fn pulse_level(pulse: u8) -> f64 {
    0.5 + 0.5 * f64::from(pulse.min(8)) / 8.0
}

/// Bande de l'écran (en points de la toile) qui contient l'invitation : haut et hauteur.
fn invitation_band(places_h: f64) -> (f64, f64) {
    (places_h - 46.0 - 33.0 - 6.0, 40.0)
}

pub(super) fn lock_scale(width: f64, height: f64) -> f64 {
    (width / 1600.0).min(height / 1000.0).clamp(0.5, 2.0)
}

/// Emplacements cliquables de l'écran, sur la toile de `w` × `h`.
#[derive(Clone, Copy)]
struct Places {
    w: f64,
    h: f64,
}

impl Places {
    fn new(width: f64, height: f64) -> Self {
        let scale = lock_scale(width, height);
        Self {
            w: width / scale,
            h: height / scale,
        }
    }

    fn power(self) -> Rect {
        Rect::new(self.w - 32.0 - 44.0, self.h - 30.0 - 44.0, 44.0, 44.0)
    }

    fn keyboard(self) -> Rect {
        Rect::new(self.power().x - 10.0 - 50.0, self.power().y, 50.0, 44.0)
    }

    fn accessibility(self) -> Rect {
        Rect::new(self.keyboard().x - 10.0 - 44.0, self.power().y, 44.0, 44.0)
    }

    /// Groupe de personnalisation, au repos.
    fn customize(self) -> Rect {
        Rect::new(32.0, self.h - 30.0 - 52.0, 300.0, 52.0)
    }

    fn theme(self) -> Rect {
        let group = self.customize();
        Rect::new(group.x + 7.0, group.y + 7.0, 38.0, 38.0)
    }

    fn swatch(self, index: usize) -> Rect {
        let theme = self.theme();
        Rect::new(
            theme.right() + 21.0 + index as f64 * 40.0,
            theme.y + 4.0,
            30.0,
            30.0,
        )
    }

    fn cancel(self) -> Rect {
        Rect::new(32.0, self.h - 30.0 - 40.0, 86.0, 40.0)
    }

    fn switch_user(self) -> Rect {
        Rect::new(self.cancel().right() + 10.0, self.cancel().y, 186.0, 40.0)
    }

    fn field(self) -> Rect {
        Rect::new((self.w - 300.0) / 2.0, FIELD_TOP, 300.0, 48.0)
    }

    fn submit(self) -> Rect {
        let field = self.field();
        Rect::new(field.right() - 42.0, field.y + 6.0, 36.0, 36.0)
    }

    fn media(self, lock: &LockView) -> Rect {
        let top = if lock.notice.is_some() {
            CARDS_TOP + NOTICE_HEIGHT + 24.0
        } else {
            CARDS_TOP
        };
        Rect::new((self.w - CARDS_WIDTH) / 2.0, top, CARDS_WIDTH, MEDIA_HEIGHT)
    }

    /// Précédent, lecture ou pause, suivant.
    fn media_button(self, lock: &LockView, index: usize) -> Rect {
        let card = self.media(lock);
        Rect::new(
            card.right() - 15.0 - 104.0 + index as f64 * 36.0,
            card.y + 21.0,
            32.0,
            32.0,
        )
    }
}

/// Ce qu'un clic demande à l'écran de verrouillage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockAction {
    Reveal,
    Submit,
    Cancel,
    ToggleTheme,
    Wall(usize),
    Keyboard,
    Power,
    Media(crate::desktop::lock::MediaCommand),
    Consume,
}

/// Action du clic en `pos` (position à l'écran).
pub fn lock_action_at(
    output: Rectangle<i32, Logical>,
    view: &DesktopView,
    pos: Point<f64, Logical>,
) -> LockAction {
    let Some(lock) = &view.lock else {
        return LockAction::Consume;
    };
    let (width, height) = (output.size.w as f64, output.size.h as f64);
    let scale = lock_scale(width, height);
    let places = Places::new(width, height);
    let pos = Point::from((
        (pos.x - output.loc.x as f64) / scale,
        (pos.y - output.loc.y as f64) / scale,
    ));
    if places.power().contains(pos) {
        return LockAction::Power;
    }
    if places.keyboard().contains(pos) {
        return LockAction::Keyboard;
    }
    if places.accessibility().contains(pos) {
        return LockAction::Consume;
    }
    if lock.field {
        if places.cancel().contains(pos) {
            return LockAction::Cancel;
        }
        if places.submit().contains(pos) {
            return LockAction::Submit;
        }
        return LockAction::Consume;
    }
    if places.theme().contains(pos) {
        return LockAction::ToggleTheme;
    }
    if let Some(index) = (0..LOCK_WALLS.len()).find(|index| places.swatch(*index).contains(pos)) {
        return LockAction::Wall(index);
    }
    if places.customize().contains(pos) {
        return LockAction::Consume;
    }
    if lock.media.is_some() {
        use crate::desktop::lock::MediaCommand;
        let commands = [
            MediaCommand::Previous,
            MediaCommand::PlayPause,
            MediaCommand::Next,
        ];
        for (index, command) in commands.into_iter().enumerate() {
            if places.media_button(lock, index).contains(pos) {
                return LockAction::Media(command);
            }
        }
    }
    LockAction::Reveal
}

/// Trait de l'hexagone : ses extrémités, son opacité et sa couleur.
type HexagonEdge = ((f32, f32), (f32, f32), f32, [f32; 3]);

/// Fond de l'écran en pixels BGRA : taches de couleur très floues et hexagone du logo.
pub(super) fn paint_lock_wall(width: u32, height: u32, dark: bool, wall: usize) -> Vec<u8> {
    let (_, night, day) = LOCK_WALLS[wall.min(LOCK_WALLS.len() - 1)];
    let colors = if dark { night } else { day };
    let opacity = if dark { 0.38_f32 } else { 0.85 };
    // Centre, rayon et flou de chaque tache, sur la toile de 1600 × 1000.
    let blobs = [
        (220.0_f32, 800.0_f32, 380.0_f32, 110.0_f32),
        (910.0, 130.0, 350.0, 120.0),
        (1360.0, 780.0, 380.0, 130.0),
    ];
    let hexagon: [HexagonEdge; 9] = {
        let (edge, inner, edge_tone, inner_tone) = if dark {
            (0.07, 0.18, [255.0, 255.0, 255.0], rgb(0x7FDCE3))
        } else {
            (0.08, 0.25, rgb(0x14202B), rgb(0x0E9AA7))
        };
        let e = (edge, edge_tone);
        let i = (inner, inner_tone);
        let top = (800.0, 230.0);
        let (right_up, right_down) = ((1060.0, 380.0), (1060.0, 680.0));
        let bottom = (800.0, 830.0);
        let (left_down, left_up) = ((540.0, 680.0), (540.0, 380.0));
        let middle = (800.0, 530.0);
        [
            (top, right_up, e.0, e.1),
            (right_up, right_down, e.0, e.1),
            (right_down, bottom, e.0, e.1),
            (bottom, left_down, e.0, e.1),
            (left_down, left_up, e.0, e.1),
            (left_up, top, e.0, e.1),
            (top, middle, i.0, i.1),
            (middle, right_down, i.0, i.1),
            (middle, left_down, i.0, i.1),
        ]
    };
    // Tout est flou : le calcul se fait au huitième, puis s'agrandit sans se voir.
    const STEP: u32 = 8;
    let (cols, rows) = (width / STEP + 2, height / STEP + 2);
    let mut coarse = Vec::with_capacity((cols * rows) as usize);
    for row in 0..rows {
        for col in 0..cols {
            let x = (col * STEP) as f32 * 1600.0 / width as f32;
            let y = (row * STEP) as f32 * 1000.0 / height as f32;
            let mut pixel = rgb(colors[0]);
            for (index, (cx, cy, radius, blur)) in blobs.into_iter().enumerate() {
                let distance = (x - cx).hypot(y - cy);
                let part = opacity / (1.0 + ((distance - radius) / (blur * 0.7)).exp());
                let tone = rgb(colors[index + 1]);
                for channel in 0..3 {
                    pixel[channel] += (tone[channel] - pixel[channel]) * part;
                }
            }
            // Le trait le plus proche seul : additionnés, ils feraient des taches aux sommets.
            let mut nearest = (0.0_f32, [0.0_f32; 3]);
            for (from, to, alpha, tone) in hexagon {
                let (dx, dy) = (to.0 - from.0, to.1 - from.1);
                let along = (((x - from.0) * dx + (y - from.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                let distance = (x - from.0 - along * dx).hypot(y - from.1 - along * dy);
                let part = alpha * 0.1 * (-distance * distance / 288.0).exp();
                if part > nearest.0 {
                    nearest = (part, tone);
                }
            }
            for (value, tone) in pixel.iter_mut().zip(nearest.1) {
                *value += (tone - *value) * nearest.0;
            }
            coarse.push(pixel);
        }
    }
    // Tramage léger, pour éviter les bandes dans les dégradés.
    const BAYER: [f32; 16] = [
        0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0,
    ];
    let mut pixels = vec![0_u8; width as usize * height as usize * 4];
    for py in 0..height {
        let (row, fy) = ((py / STEP) as usize, (py % STEP) as f32 / STEP as f32);
        for px in 0..width {
            let (col, fx) = ((px / STEP) as usize, (px % STEP) as f32 / STEP as f32);
            let at = |c: usize, r: usize| coarse[r * cols as usize + c];
            let (a, b, c, d) = (
                at(col, row),
                at(col + 1, row),
                at(col, row + 1),
                at(col + 1, row + 1),
            );
            let dither = BAYER[((py % 4) * 4 + px % 4) as usize] / 16.0;
            let offset = (py as usize * width as usize + px as usize) * 4;
            for channel in 0..3 {
                let top = a[channel] + (b[channel] - a[channel]) * fx;
                let bottom = c[channel] + (d[channel] - c[channel]) * fx;
                // BGRA : le rouge en dernier.
                pixels[offset + 2 - channel] = (top + (bottom - top) * fy + dither).min(255.0) as u8;
            }
            pixels[offset + 3] = 255;
        }
    }
    pixels
}

impl Canvas<'_> {
    /// Dégradé à 135° (du coin haut gauche au coin bas droit).
    fn gradient(&mut self, rect: Rect, radius: f32, stops: &[(f32, u32)]) {
        let s = self.scale;
        let (x, y, w, h) = (
            rect.x as f32 * s,
            rect.y as f32 * s,
            rect.w as f32 * s,
            rect.h as f32 * s,
        );
        let radius = (radius * s).min(w.min(h) * 0.5);
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
                let part = ((sx - x) / w + (sy - y) / h) / 2.0;
                let next = stops
                    .iter()
                    .position(|stop| stop.0 >= part)
                    .unwrap_or(stops.len() - 1);
                let (from, to) = (stops[next.saturating_sub(1)], stops[next]);
                let span = (to.0 - from.0).max(0.001);
                let t = ((part - from.0) / span).clamp(0.0, 1.0);
                let (a, b) = (rgb(from.1), rgb(to.1));
                let [r, g, b] = [0, 1, 2].map(|c| (a[c] + (b[c] - a[c]) * t).round() as u8);
                self.blend(px, py, Color { r, g, b, a: 255 }, coverage);
            }
        }
    }

    /// Texte centré sur `center`.
    fn centered(&mut self, font: &Font, text: &str, center: f64, y: f64, size: f64, color: Color) {
        let width = self.css_width(font, text, size);
        self.text(font, text, center - width / 2.0, y, size, color);
    }

    /// Verre de l'écran : le fond est déjà flou, une teinte et un liseré suffisent.
    fn lock_glass(&mut self, rect: Rect, radius: f32, tokens: &Tokens) {
        self.fill(rect, radius, tokens.glass);
        self.stroke(rect, radius, tokens.border, 1.0);
    }

    /// Bouton rond ou en pilule, avec son liseré.
    fn lock_button(&mut self, rect: Rect, tokens: &Tokens) {
        self.fill(rect, (rect.h / 2.0) as f32, tokens.button);
        self.stroke(rect, (rect.h / 2.0) as f32, tokens.border, 1.0);
    }
}

impl Panels {
    /// Fond de l'écran, gardé tant que l'écran, le thème et le choix ne changent pas.
    pub(super) fn lock_wall(&self, width: u32, height: u32, lock: &LockView) -> Vec<u8> {
        let key = (width, height, lock.dark, lock.wall);
        let mut cache = self.lock_wall.borrow_mut();
        if let Some((known, pixels)) = cache.as_ref()
            && *known == key
        {
            return pixels.clone();
        }
        let pixels = paint_lock_wall(width, height, lock.dark, lock.wall);
        *cache = Some((key, pixels.clone()));
        pixels
    }

    /// Chevron et texte de l'invitation ; `baseline` est le bas de l'écran moins sa marge.
    fn draw_invitation(&self, canvas: &mut Canvas, center: f64, baseline: f64, tokens: &Tokens, level: f64) {
        let ink = tokens.fg2.with_alpha((level * f64::from(tokens.fg2.a)) as u8);
        let chevron = baseline - 33.0;
        canvas.line((center - 9.0, chevron + 8.0), (center, chevron), 2.4, ink);
        canvas.line((center, chevron), (center + 9.0, chevron + 8.0), 2.4, ink);
        canvas.centered(
            &self.regular,
            "Appuyez sur une touche ou cliquez pour déverrouiller",
            center,
            baseline - 17.0,
            14.0,
            ink,
        );
    }

    /// Invitation seule, pleinement opaque, dans une image de la largeur de l'écran ;
    /// renvoie aussi sa place en logique (haut et hauteur).
    pub(super) fn rasterize_invitation(
        &self,
        state: &VisualState,
        lock: &LockView,
        scale: f64,
    ) -> (MemoryRenderBuffer, (f64, f64)) {
        let (width, height) = (state.width as f64, state.height as f64);
        let ratio = lock_scale(width, height);
        let places = Places::new(width, height);
        let (top, band) = invitation_band(places.h);
        let pixels_w = (width * scale).round().max(1.0) as u32;
        let pixels_h = (band * ratio * scale).ceil().max(1.0) as u32;
        let mut canvas = Canvas {
            pixels: vec![0; pixels_w as usize * pixels_h as usize * 4],
            width: pixels_w,
            height: pixels_h,
            scale: (scale * ratio) as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        let tokens = tokens(lock.dark);
        self.draw_invitation(&mut canvas, places.w / 2.0, places.h - 46.0 - top, &tokens, 1.0);
        let buffer = MemoryRenderBuffer::from_slice(
            &canvas.pixels,
            Fourcc::Argb8888,
            (pixels_w as i32, pixels_h as i32),
            1,
            smithay::utils::Transform::Normal,
            None,
        );
        (buffer, (top * ratio, f64::from(pixels_h) / scale))
    }

    pub(super) fn draw_lock(
        &self,
        canvas: &mut Canvas,
        width: f64,
        height: f64,
        state: &VisualState,
        lock: &LockView,
    ) {
        let places = Places::new(width, height);
        let tokens = tokens(lock.dark);
        let part = f64::from(lock.progress) / 1000.0;
        let (w, h) = (places.w, places.h);
        let center = w / 2.0;

        // Le voile s'épaissit en saisie.
        let (rest, open) = tokens.veil;
        let alpha = mix(f64::from(rest.a), f64::from(open.a), part).round() as u8;
        canvas.fill(Rect::new(0.0, 0.0, w, h), 0.0, open.with_alpha(alpha));

        self.lock_status(canvas, &places, state, &tokens);

        // Date et heure : grandes au repos, petites et en haut pendant la saisie.
        let clock = state.clock;
        let top = mix(CLOCK_TOP.0, CLOCK_TOP.1, part);
        let date_size = mix(DATE_SIZE.0, DATE_SIZE.1, part);
        let time_size = mix(TIME_SIZE.0, TIME_SIZE.1, part);
        let date = format!(
            "{} {} {}",
            DAYS[clock.weekday],
            clock.day,
            MONTHS[clock.month as usize - 1]
        );
        canvas.centered(&self.regular, &date, center, top, date_size, tokens.fg2);
        let time = format!("{:02}:{:02}", clock.hour, clock.minute);
        let time_top = top + date_size * 1.21 + 6.0;
        let width_time = canvas.tracked_width(&self.light, &time, time_size, -0.04);
        canvas.tracked(
            &self.light,
            &time,
            center - width_time / 2.0,
            time_top - time_size * 0.13,
            time_size,
            -0.04,
            tokens.fg,
        );

        if lock.field {
            self.lock_entry(canvas, &places, lock, &tokens);
        } else {
            self.lock_rest(canvas, &places, lock, &tokens, time_top + time_size * 0.95);
        }

        // Toujours là : accessibilité, clavier, arrêt.
        let access = places.accessibility();
        canvas.lock_button(access, &tokens);
        // Sans fonction d'accessibilité à proposer ici : le bouton reste atténué.
        let (cx, cy) = access.center();
        let ink = tokens.fg.with_alpha(110);
        canvas.fill(Rect::new(cx - 1.5, cy - 9.0, 3.0, 3.0), 1.5, ink);
        canvas.line((cx - 7.0, cy - 4.0), (cx + 7.0, cy - 4.0), 2.0, ink);
        canvas.line((cx, cy - 4.0), (cx, cy + 2.0), 2.0, ink);
        canvas.line((cx, cy + 2.0), (cx - 4.0, cy + 9.0), 2.0, ink);
        canvas.line((cx, cy + 2.0), (cx + 4.0, cy + 9.0), 2.0, ink);

        let keyboard = places.keyboard();
        canvas.lock_button(keyboard, &tokens);
        let layout_name = state.view.keyboard.to_uppercase();
        let layout_name = layout_name.split_whitespace().next().unwrap_or("FR");
        canvas.centered(
            &self.bold,
            layout_name,
            keyboard.center().0,
            keyboard.y + 14.0,
            13.0,
            tokens.fg,
        );

        let power = places.power();
        canvas.lock_button(power, &tokens);
        canvas.icon("cc-power", power.center(), 20.0, Style::Symbolic(tokens.fg));
    }

    /// En haut à droite : Wi-Fi et batterie.
    fn lock_status(&self, canvas: &mut Canvas, places: &Places, state: &VisualState, tokens: &Tokens) {
        let mut right = places.w - 28.0;
        if let Some(battery) = state.battery {
            let text = format!("{} %", battery.percent);
            let width = canvas.css_width(&self.bold, &text, 14.0);
            right -= width;
            canvas.text(&self.bold, &text, right, 22.0, 14.0, tokens.fg);
            right -= 6.0 + 24.0;
            let body = Rect::new(right + 1.0, 25.0, 19.0, 11.0);
            canvas.stroke(body, 3.0, tokens.fg, 1.4);
            let filled = 14.0 * f64::from(battery.percent.min(100)) / 100.0;
            canvas.fill(
                Rect::new(body.x + 2.5, body.y + 2.5, filled.max(1.5), 6.0),
                1.5,
                tokens.fg,
            );
            canvas.fill(
                Rect::new(body.right() + 1.5, body.y + 3.5, 2.0, 4.0),
                1.0,
                tokens.fg,
            );
            right -= 16.0;
        }
        if state.view.wifi == Some(true) {
            canvas.icon(
                "status-wifi-4",
                (right - 9.0, 30.0),
                18.0,
                Style::Symbolic(tokens.fg),
            );
        }
    }

    /// Repos : météo, notifications masquées, lecture en cours, invitation et personnalisation.
    fn lock_rest(
        &self,
        canvas: &mut Canvas,
        places: &Places,
        lock: &LockView,
        tokens: &Tokens,
        clock_bottom: f64,
    ) {
        let center = places.w / 2.0;
        if let Some(weather) = &lock.weather {
            let text = format!(
                "{}° · {} · {}",
                weather.temperature,
                weather.sky.label(),
                lock.place
            );
            let width = canvas.css_width(&self.regular, &text, 14.0);
            let chip = Rect::new(
                center - (width + 28.0 + 26.0) / 2.0,
                clock_bottom + 16.0,
                width + 54.0,
                31.0,
            );
            canvas.fill(chip, 15.5, tokens.chip);
            canvas.stroke(chip, 15.5, tokens.border, 1.0);
            self.weather_symbol(canvas, (chip.x + 23.0, chip.y + 15.5), weather.sky, 0.94);
            canvas.text(&self.regular, &text, chip.x + 40.0, chip.y + 7.0, 14.0, tokens.fg);
        }

        let left = center - CARDS_WIDTH / 2.0;
        if let Some(notice) = &lock.notice {
            if notice.count > 1 {
                // Seul ce qui dépasse sous la carte se voit : son verre ne montre pas la pile.
                let behind = canvas.save_rows(CARDS_TOP, CARDS_TOP + NOTICE_HEIGHT);
                canvas.fill(
                    Rect::new(
                        left + 16.0,
                        CARDS_TOP + NOTICE_HEIGHT - 20.0,
                        CARDS_WIDTH - 32.0,
                        30.0,
                    ),
                    18.0,
                    tokens.stack[0],
                );
                canvas.fill(
                    Rect::new(
                        left + 8.0,
                        CARDS_TOP + NOTICE_HEIGHT - 25.0,
                        CARDS_WIDTH - 16.0,
                        30.0,
                    ),
                    18.0,
                    tokens.stack[1],
                );
                canvas.restore_rows(behind);
            }
            let card = Rect::new(left, CARDS_TOP, CARDS_WIDTH, NOTICE_HEIGHT);
            canvas.lock_glass(card, 20.0, tokens);
            let icon = (card.x + 33.0, card.y + NOTICE_HEIGHT / 2.0);
            match &notice.icon {
                Some(picture) => canvas.app_icon(picture, icon, 34.0),
                None => canvas.icon("status-notifications", icon, 30.0, Style::Symbolic(tokens.fg)),
            }
            let x = card.x + 62.0;
            let room = card.right() - 16.0 - x;
            let when = canvas.css_width(&self.regular, &notice.when, 12.0);
            let app = canvas.fit_text(&self.bold, &notice.app, px(14.0), room - when - 8.0);
            canvas.text(&self.bold, &app, x, card.y + 14.0, 14.0, tokens.fg);
            canvas.text(
                &self.regular,
                &notice.when,
                card.right() - 16.0 - when,
                card.y + 16.0,
                12.0,
                tokens.fg3,
            );
            let count = if notice.count > 1 {
                format!("{} notifications · contenu masqué", notice.count)
            } else {
                "1 notification · contenu masqué".to_owned()
            };
            canvas.text(&self.regular, &count, x, card.y + 34.0, 13.0, tokens.fg2);
        }
        if let Some(media) = &lock.media {
            let card = places.media(lock);
            canvas.lock_glass(card, 20.0, tokens);
            let art = Rect::new(card.x + 14.0, card.y + 13.0, 48.0, 48.0);
            canvas.gradient(art, 12.0, &[(0.0, 0xFF7E9D), (1.0, 0x7C4DE8)]);
            let (ax, ay) = art.center();
            canvas.line((ax - 3.0, ay + 7.0), (ax - 3.0, ay - 6.0), 2.0, Color::WHITE);
            canvas.line((ax - 3.0, ay - 6.0), (ax + 7.0, ay - 8.0), 2.0, Color::WHITE);
            canvas.line((ax + 7.0, ay - 8.0), (ax + 7.0, ay + 5.0), 2.0, Color::WHITE);
            canvas.fill(Rect::new(ax - 8.0, ay + 4.5, 5.2, 5.2), 2.6, Color::WHITE);
            canvas.fill(Rect::new(ax + 2.0, ay + 2.5, 5.2, 5.2), 2.6, Color::WHITE);
            let x = art.right() + 14.0;
            let first = places.media_button(lock, 0).x;
            let room = first - 10.0 - x;
            canvas.text(&self.bold, "Lecture en cours", x, card.y + 13.0, 14.0, tokens.fg);
            let detail = canvas.fit_text(&self.regular, &media.detail, px(12.0), room);
            canvas.text(&self.regular, &detail, x, card.y + 31.0, 12.0, tokens.fg3);
            let track = Rect::new(x, card.y + 53.0, room, 4.0);
            canvas.fill(track, 2.0, tokens.track);
            let filled = room * f64::from(media.progress.min(100)) / 100.0;
            canvas.fill(Rect::new(x, track.y, filled.max(4.0), 4.0), 2.0, tokens.bar);
            let (ink, previous, play, next) = (
                tokens.fg,
                places.media_button(lock, 0),
                places.media_button(lock, 1),
                places.media_button(lock, 2),
            );
            let (px_, py_) = previous.center();
            canvas.polygon(
                &[(px_ + 5.0, py_ - 5.0), (px_ - 2.0, py_), (px_ + 5.0, py_ + 5.0)],
                ink,
            );
            canvas.line((px_ - 5.0, py_ - 5.0), (px_ - 5.0, py_ + 5.0), 1.6, ink);
            canvas.fill(play, 16.0, tokens.button);
            let (cx, cy) = play.center();
            if media.playing {
                canvas.fill(Rect::new(cx - 4.0, cy - 5.0, 3.0, 10.0), 1.0, ink);
                canvas.fill(Rect::new(cx + 1.0, cy - 5.0, 3.0, 10.0), 1.0, ink);
            } else {
                canvas.polygon(&[(cx - 3.0, cy - 5.5), (cx + 5.0, cy), (cx - 3.0, cy + 5.5)], ink);
            }
            let (nx, ny) = next.center();
            canvas.polygon(&[(nx - 5.0, ny - 5.0), (nx + 2.0, ny), (nx - 5.0, ny + 5.0)], ink);
            canvas.line((nx + 5.0, ny - 5.0), (nx + 5.0, ny + 5.0), 1.6, ink);
        }

        // Invitation, qui pulse doucement (d'ordinaire dessinée à part, voir
        // `PULSE_APART`).
        if lock.pulse != PULSE_APART {
            self.draw_invitation(canvas, center, places.h - 46.0, tokens, pulse_level(lock.pulse));
        }

        // Personnaliser : thème et fond de cet écran.
        let group = places.customize();
        let name = LOCK_WALLS[lock.wall.min(LOCK_WALLS.len() - 1)].0;
        let name_x = places.swatch(LOCK_WALLS.len() - 1).right() + 10.0;
        let name_width = canvas.css_width(&self.regular, name, 12.0);
        let group = Rect::new(group.x, group.y, name_x + name_width + 12.0 - group.x, group.h);
        canvas.lock_glass(group, 26.0, tokens);
        let theme = places.theme();
        canvas.fill(theme, 19.0, tokens.button);
        let (cx, cy) = theme.center();
        if lock.dark {
            // Soleil : passe au thème clair.
            canvas.stroke(Rect::new(cx - 4.5, cy - 4.5, 9.0, 9.0), 4.5, tokens.fg, 1.6);
            for ray in 0..8 {
                let angle = std::f64::consts::TAU * f64::from(ray) / 8.0;
                let (sin, cos) = angle.sin_cos();
                canvas.line(
                    (cx + cos * 7.0, cy + sin * 7.0),
                    (cx + cos * 9.0, cy + sin * 9.0),
                    1.6,
                    tokens.fg,
                );
            }
        } else {
            // Lune : passe au thème sombre.
            canvas.fill(Rect::new(cx - 7.0, cy - 7.0, 14.0, 14.0), 7.0, tokens.fg);
            canvas.fill(
                Rect::new(cx - 3.5, cy - 8.5, 13.0, 13.0),
                6.5,
                Color::rgb(0xEEF2F5),
            );
        }
        canvas.fill(
            Rect::new(theme.right() + 10.0, theme.y + 7.0, 1.0, 24.0),
            0.0,
            tokens.border,
        );
        for (index, (_, night, day)) in LOCK_WALLS.into_iter().enumerate() {
            let colors = if lock.dark { night } else { day };
            let swatch = places.swatch(index);
            if index == lock.wall {
                canvas.fill(swatch.expand(4.0), 19.0, Color::rgb(0x0E9AA7));
                canvas.fill(swatch.expand(2.0), 17.0, tokens.swatch_gap);
            }
            canvas.gradient(
                swatch,
                15.0,
                &[(0.0, colors[1]), (0.55, colors[2]), (1.0, colors[3])],
            );
        }
        canvas.text(&self.regular, name, name_x, group.y + 18.0, 12.0, tokens.fg2);
    }

    /// Saisie : avatar, nom, champ du mot de passe et message.
    fn lock_entry(&self, canvas: &mut Canvas, places: &Places, lock: &LockView, tokens: &Tokens) {
        let center = places.w / 2.0;
        let avatar = Rect::new(center - 52.0, 330.0, 104.0, 104.0);
        canvas.panel_shadow(avatar, 52.0);
        canvas.fill(avatar.expand(4.0), 56.0, tokens.ring);
        canvas.gradient(avatar, 52.0, &[(0.0, 0x3CC7D1), (1.0, 0x2779E0)]);
        canvas.centered(
            &self.bold,
            &lock.initial.to_string(),
            center,
            avatar.y + 27.0,
            42.0,
            Color::WHITE,
        );
        canvas.centered(
            &self.bold,
            &lock.user,
            center,
            avatar.bottom() + 14.0,
            22.0,
            tokens.fg,
        );

        let field = places.field();
        if lock.checking || lock.welcome {
            let text = if lock.welcome {
                "Bienvenue, ouverture de la session…"
            } else {
                "Vérification…"
            };
            let width = canvas.css_width(&self.regular, text, 14.0);
            let x = center - (width + 30.0) / 2.0;
            let spinner = (x + 10.0, field.y + 24.0);
            canvas.stroke(
                Rect::new(spinner.0 - 8.0, spinner.1 - 8.0, 16.0, 16.0),
                8.0,
                tokens.track,
                2.2,
            );
            let start = f64::from(lock.spin) / 36.0 * std::f64::consts::TAU;
            for step in 0..8 {
                let (a, b) = (start + f64::from(step) * 0.2, start + f64::from(step + 1) * 0.2);
                canvas.line(
                    (spinner.0 + a.cos() * 7.5, spinner.1 + a.sin() * 7.5),
                    (spinner.0 + b.cos() * 7.5, spinner.1 + b.sin() * 7.5),
                    2.2,
                    Color::rgb(0x0E9AA7),
                );
            }
            canvas.text(&self.regular, text, x + 30.0, field.y + 15.0, 14.0, tokens.fg2);
        } else {
            let shake = f64::from(lock.shake) / 10.0;
            let field = Rect::new(field.x + shake, field.y, field.w, field.h);
            let wrong = lock.message != LockMessage::Hint;
            canvas.fill(field, 24.0, tokens.field);
            let edge = if wrong { tokens.error.0 } else { tokens.field_edge };
            canvas.stroke(field, 24.0, edge, 1.5);
            let x = field.x + 16.0;
            if lock.length == 0 {
                canvas.text(&self.regular, "Mot de passe", x, field.y + 14.5, 15.0, tokens.fg3);
            } else {
                for index in 0..lock.length.min(24) {
                    let dot = Rect::new(x + index as f64 * 10.0, field.y + 20.0, 8.0, 8.0);
                    canvas.fill(dot, 4.0, tokens.fg);
                }
            }
            let submit = places.submit();
            let submit = Rect::new(submit.x + shake, submit.y, submit.w, submit.h);
            canvas.fill(submit, 18.0, Color::rgb(0x0B7C86));
            let (cx, cy) = submit.center();
            canvas.line((cx - 5.0, cy), (cx + 5.0, cy), 2.0, Color::WHITE);
            canvas.line((cx + 1.0, cy - 4.0), (cx + 5.0, cy), 2.0, Color::WHITE);
            canvas.line((cx + 1.0, cy + 4.0), (cx + 5.0, cy), 2.0, Color::WHITE);
            let (text, ink) = match lock.message {
                LockMessage::Hint => ("Appuyez sur Entrée pour déverrouiller", tokens.fg3),
                LockMessage::Empty => ("Saisissez votre mot de passe", tokens.error.1),
                LockMessage::Wrong => ("Mot de passe incorrect", tokens.error.1),
            };
            canvas.centered(&self.regular, text, center, field.bottom() + 14.0, 13.0, ink);
        }

        let cancel = places.cancel();
        canvas.lock_button(cancel, tokens);
        canvas.centered(
            &self.regular,
            "Annuler",
            cancel.center().0,
            cancel.y + 12.0,
            13.0,
            tokens.fg,
        );
        // Aucun second compte à ouvrir depuis cette session : le bouton reste atténué.
        let switch = places.switch_user();
        canvas.lock_button(switch, tokens);
        canvas.centered(
            &self.regular,
            "Changer d’utilisateur",
            switch.center().0,
            switch.y + 12.0,
            13.0,
            tokens.fg.with_alpha(110),
        );
    }
}
