//! Écran de verrouillage : au repos (heure, météo, notifications masquées, lecture en cours,
//! personnalisation), puis en saisie (avatar et mot de passe). Dessiné sur une toile de
//! 1600 × 1000, mise à l'échelle de l'écran.

use super::*;
use ferrite_ui::wallpaper::Mood;

/// Photo choisie comme fond de l'écran de verrouillage (Visionneuse) : image décodée et
/// vignette de sa pastille. Comparée par adresse : l'image entière ne l'est jamais.
#[derive(Clone)]
pub struct LockPicture {
    pub path: std::path::PathBuf,
    pub image: std::sync::Arc<ferrite_ui::wallpaper::Image>,
    pub thumb: crate::clipboard::Thumbnail,
    /// Luminosité moyenne (0 à 1) de la photo derrière la date et l'heure, et derrière
    /// l'invitation du bas.
    pub clock_luma: f32,
    pub invitation_luma: f32,
}

impl PartialEq for LockPicture {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && std::sync::Arc::ptr_eq(&self.image, &other.image)
    }
}

impl Eq for LockPicture {}

impl std::fmt::Debug for LockPicture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LockPicture({})", self.path.display())
    }
}

impl LockPicture {
    /// Décode `path` (dans le fil système : jamais pendant le rendu).
    pub fn load(path: &std::path::Path) -> Option<Self> {
        let image = ferrite_ui::wallpaper::decode(path).ok()?;
        let small = ferrite_ui::wallpaper::cover(&image, 60, 60);
        // Photo couvrant un écran 16:10 (la toile de 1600 × 1000, réduite au dixième).
        let screen = ferrite_ui::wallpaper::cover(&image, 160, 100);
        let clock_luma = zone_luma(&screen, 15..40, 50..110);
        let invitation_luma = zone_luma(&screen, 89..97, 58..102);
        Some(Self {
            path: path.to_path_buf(),
            image: std::sync::Arc::new(image),
            thumb: thumbnail(&small),
            clock_luma,
            invitation_luma,
        })
    }
}

/// Image RGBA en vignette BGRA prémultipliée, prête à être dessinée.
fn thumbnail(image: &ferrite_ui::wallpaper::Image) -> crate::clipboard::Thumbnail {
    let pixels = image
        .rgba
        .chunks_exact(4)
        .flat_map(|p| {
            let alpha = u32::from(p[3]);
            let premultiply = |value: u8| (u32::from(value) * alpha / 255) as u8;
            [premultiply(p[2]), premultiply(p[1]), premultiply(p[0]), p[3]]
        })
        .collect();
    crate::clipboard::Thumbnail {
        width: image.width,
        height: image.height,
        pixels: std::sync::Arc::new(pixels),
    }
}

/// Image de profil du compte : celle qu'AccountsService garde (choisie dans Paramètres →
/// Comptes), sinon `~/.face`.
pub fn avatar_path() -> Option<std::path::PathBuf> {
    let user = std::env::var("USER").ok().filter(|user| !user.is_empty())?;
    let kept = std::path::Path::new("/var/lib/AccountsService/icons").join(&user);
    if kept.is_file() {
        return Some(kept);
    }
    let face = std::path::PathBuf::from(std::env::var_os("HOME")?).join(".face");
    face.is_file().then_some(face)
}

/// Image de profil réduite pour l'avatar de la saisie (deux fois son côté, pour les écrans
/// à forte densité). Décodée dans le fil système : jamais pendant le rendu.
pub fn load_avatar(path: &std::path::Path) -> Option<crate::clipboard::Thumbnail> {
    // Une image de profil n'est jamais plus lourde : un fichier énorme est ignoré.
    if std::fs::metadata(path).ok()?.len() > 8 * 1024 * 1024 {
        return None;
    }
    let image = ferrite_ui::wallpaper::decode(path).ok()?;
    let side = (AVATAR * 2.0) as u32;
    Some(thumbnail(&ferrite_ui::wallpaper::cover(&image, side, side)))
}

/// Luminosité moyenne d'une zone (lignes, colonnes) de `screen`.
fn zone_luma(
    screen: &ferrite_ui::wallpaper::Image,
    rows: std::ops::Range<usize>,
    cols: std::ops::Range<usize>,
) -> f32 {
    let width = screen.width as usize;
    let (mut sum, mut count) = (0.0_f32, 0.0_f32);
    for y in rows {
        for x in cols.clone() {
            let p = &screen.rgba[(y * width + x) * 4..][..3];
            sum += (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])) / 255.0;
            count += 1.0;
        }
    }
    sum / count
}

/// Encres claires (celles du thème sombre) pour un texte posé sur la zone `zone` ? Celles
/// du thème, sauf sur une photo dont la zone est trop claire ou trop sombre pour elles.
pub(super) fn light_ink(lock: &LockView, zone: fn(&LockPicture) -> f32) -> bool {
    let Some(picture) = lock.picture.as_ref().filter(|_| lock.wall == LOCK_WALLS.len()) else {
        return lock.dark;
    };
    // Même voile que `paint_lock_picture`.
    let veil = f32::from(lock.blur) / f32::from(LOCK_BLUR_MAX);
    let luma = zone(picture);
    let luma = if lock.dark {
        luma * (1.0 - 0.38 * veil)
    } else {
        luma * (1.0 - 0.2 * veil) + 0.2 * veil
    };
    luma <= 0.55
}

pub(super) fn invitation_ink(lock: &LockView) -> bool {
    light_ink(lock, |picture| picture.invitation_luma)
}

/// Pastilles proposées : les fonds dessinés, puis la photo s'il y en a une.
pub(super) fn wall_count(lock: &LockView) -> usize {
    LOCK_WALLS.len() + usize::from(lock.picture.is_some())
}

/// Niveaux de flou de la photo (réglage `verrouillage_flou`, de 0 à ce maximum).
pub const LOCK_BLUR_MAX: u8 = 3;

/// Photo couvrant l'écran, en BGRA comme les autres fonds. Nette par défaut, telle quelle ;
/// avec du flou, elle est aussi d'autant assombrie (thème sombre) ou éclaircie (thème
/// clair) pour que l'heure et le texte se détachent.
fn paint_lock_picture(width: u32, height: u32, dark: bool, blur: u8, picture: &LockPicture) -> Vec<u8> {
    let image = if blur == 0 {
        ferrite_ui::wallpaper::cover(&picture.image, width, height)
    } else {
        // Flou calculé à mi-taille, puis agrandi : il ne s'y voit pas.
        let small = ferrite_ui::wallpaper::cover(&picture.image, (width / 2).max(1), (height / 2).max(1));
        let blurred = ferrite_ui::wallpaper::blur(&small, u32::from(blur) * 6);
        ferrite_ui::wallpaper::cover(&blurred, width, height)
    };
    let veil = f32::from(blur) / f32::from(LOCK_BLUR_MAX);
    image
        .rgba
        .chunks_exact(4)
        .flat_map(|p| {
            let tone = |value: u8| {
                let value = f32::from(value);
                (if dark {
                    value * (1.0 - 0.38 * veil)
                } else {
                    value * (1.0 - 0.2 * veil) + 255.0 * 0.2 * veil
                }) as u8
            };
            [tone(p[2]), tone(p[1]), tone(p[0]), 255]
        })
        .collect()
}

/// Fonds dessinés proposés (thème « Fusion »), chacun en sombre et en clair ; le réglage
/// `verrouillage_fond` garde leur nom.
pub const LOCK_WALLS: [Mood; 4] = Mood::ALL;

/// Part de la transition du repos à la saisie, à chaque extrémité.
const CLOCK_TOP: (f64, f64) = (170.0, 90.0);
const DATE_SIZE: (f64, f64) = (22.0, 16.0);
const TIME_SIZE: (f64, f64) = (168.0, 72.0);

const CARDS_TOP: f64 = 560.0;
const CARDS_WIDTH: f64 = 420.0;
const NOTICE_HEIGHT: f64 = 65.0;
const MEDIA_HEIGHT: f64 = 74.0;
const FIELD_TOP: f64 = 330.0 + 104.0 + 14.0 + 27.0 + 14.0;
/// Avatar de la saisie : côté et haut, en points de la toile.
const AVATAR: f64 = 104.0;
const AVATAR_TOP: f64 = 330.0;
/// Côté de l'image de l'anneau du visage, et nombre de ses images (un tour complet).
const FACE_RING: f64 = 150.0;
const FACE_FRAMES: u16 = 30;
/// Teinte de l'écran de verrouillage : attente, œil, anneau du visage. L'Oxyde vif, lisible
/// sur le fond clair comme sombre.
const ACCENT: Color = Color::rgb(0xFF7A45);

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
    /// Accent du thème (bouton d'entrée, anneau du champ) et encre posée dessus.
    accent: Color,
    on_accent: Color,
}

fn tokens(dark: bool) -> Tokens {
    let white = Color::WHITE;
    let ink = Color::rgb(0x1B1B1F);
    if dark {
        Tokens {
            fg: Color::rgb(0xF2F2F5),
            fg2: Color::rgb(0xDDDDE3),
            fg3: Color::rgb(0xA6A6B0),
            glass: Color::rgb(0x19191C).with_alpha(140),
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
                Color::rgb(0x0B0B0D).with_alpha(31),
                Color::rgb(0x0B0B0D).with_alpha(102),
            ),
            error: (Color::rgb(0xF87171), Color::rgb(0xFCA5A5)),
            swatch_gap: Color::rgb(0x141416),
            accent: ACCENT,
            on_accent: Color::rgb(0x1A0B04),
        }
    } else {
        Tokens {
            fg: ink,
            fg2: Color::rgb(0x2E2E35),
            fg3: Color::rgb(0x5A5A64),
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
            error: (Color::rgb(0xC62828), Color::rgb(0xB42318)),
            swatch_gap: white,
            accent: Color::rgb(0xB83A0A),
            on_accent: white,
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

/// Marge au-dessus et au-dessous du texte, dans les images de la date et de l'heure.
const CLOCK_PAD: f64 = 6.0;

/// Place de la date et de l'heure (haut de leur texte et taille), en points de la toile, à
/// la part `part` du passage à la saisie ; `bottom` : le bas de l'heure.
pub(super) struct ClockPlaces {
    date_top: f64,
    date_size: f64,
    time_top: f64,
    time_size: f64,
    bottom: f64,
}

/// Grandes au repos, petites et en haut pendant la saisie.
pub(super) fn clock_places(part: f64) -> ClockPlaces {
    let date_top = mix(CLOCK_TOP.0, CLOCK_TOP.1, part);
    let date_size = mix(DATE_SIZE.0, DATE_SIZE.1, part);
    let time_size = mix(TIME_SIZE.0, TIME_SIZE.1, part);
    let time_line = date_top + date_size * 1.21 + 6.0;
    ClockPlaces {
        date_top,
        date_size,
        time_top: time_line - time_size * 0.13,
        time_size,
        bottom: time_line + time_size * 0.95,
    }
}

/// Partie de l'écran de verrouillage que dessine `draw_lock` : ce qui ne bouge pas (état,
/// boutons du bas) ou le contenu du repos ou de la saisie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LockLayer {
    Chrome,
    Content,
}

/// Une couche de l'écran de verrouillage : son image, sa place en points logiques et son
/// opacité.
pub(super) struct LockElement {
    pub buffer: MemoryRenderBuffer,
    /// Taille de l'image, en pixels.
    pub pixels: (u32, u32),
    pub rect: Rect,
    pub alpha: f32,
}

/// Ce qui fait redessiner une image de la date ou de l'heure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClockKey {
    width: u32,
    height: u32,
    scale_milli: u32,
    text: String,
    light: bool,
    time: bool,
    small: bool,
}

/// Images gardées des couches de l'écran de verrouillage.
#[derive(Default)]
pub(super) struct LockLayers {
    chrome: Option<(VisualState, MemoryRenderBuffer)>,
    veil: Option<(Color, MemoryRenderBuffer)>,
    /// Image, sa taille en pixels et la hauteur de sa bande en points de la toile.
    clocks: Vec<(ClockKey, MemoryRenderBuffer, (u32, u32), f64)>,
    /// Images de l'anneau du visage (côté en pixels, thème), une par pas du tour, puis
    /// celle du visage reconnu ; dessinées une fois chacune.
    face: Option<FaceRingFrames>,
}

/// Images de l'anneau du visage : côté en pixels et thème, puis une image par pas.
type FaceRingFrames = ((u32, bool), Vec<Option<MemoryRenderBuffer>>);

impl std::fmt::Debug for LockLayers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LockLayers({} images de l'heure)", self.clocks.len())
    }
}

/// L'état dont dépendent les boutons et l'état du haut : tout, sauf ce que la saisie fait
/// changer.
fn lock_chrome_state(state: &VisualState) -> VisualState {
    let mut chrome = state.clone();
    if let Some(lock) = chrome.view.lock.as_mut() {
        lock.length = 0;
        lock.cursor = 0;
        lock.shown = None;
        lock.field = false;
        lock.progress = 0;
        lock.checking = false;
        lock.welcome = false;
        lock.message = LockMessage::Hint;
        lock.face = LockFace::Idle;
        lock.face_ready = false;
        lock.face_tick = 0;
        lock.shake = 0;
        lock.pulse = 0;
        lock.spin = 0;
        lock.notice = None;
        lock.media = None;
    }
    chrome
}

/// Image ARGB prête pour la carte graphique.
fn memory_buffer(pixels: &[u8], width: u32, height: u32) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(
        pixels,
        Fourcc::Argb8888,
        (width as i32, height as i32),
        1,
        smithay::utils::Transform::Normal,
        None,
    )
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

    /// Avatar de la saisie (un clic relance le visage).
    fn avatar(self) -> Rect {
        Rect::new(self.w / 2.0 - AVATAR / 2.0, AVATAR_TOP, AVATAR, AVATAR)
    }

    /// Image de l'anneau du visage, centrée sur l'avatar.
    fn face_ring(self) -> Rect {
        let avatar = self.avatar();
        let (cx, cy) = avatar.center();
        Rect::new(cx - FACE_RING / 2.0, cy - FACE_RING / 2.0, FACE_RING, FACE_RING)
    }

    /// « Réessayer » sous le message du champ.
    fn face_button(self) -> Rect {
        let field = self.field();
        Rect::new(self.w / 2.0 - 100.0, field.bottom() + 40.0, 200.0, 36.0)
    }

    fn field(self) -> Rect {
        Rect::new((self.w - 300.0) / 2.0, FIELD_TOP, 300.0, 48.0)
    }

    fn submit(self) -> Rect {
        let field = self.field();
        Rect::new(field.right() - 42.0, field.y + 6.0, 36.0, 36.0)
    }

    /// L'œil, juste avant le bouton de validation.
    fn eye(self) -> Rect {
        let submit = self.submit();
        Rect::new(submit.x - 38.0, submit.y, 34.0, 36.0)
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
    /// L'œil du champ, enfoncé : le mot de passe en clair tant qu'il est tenu.
    Peek,
    Cancel,
    ToggleTheme,
    Wall(usize),
    Keyboard,
    Power,
    Media(crate::desktop::lock::MediaCommand),
    /// « Réessayer » ou l'avatar : relance la reconnaissance du visage.
    Face,
    /// Écran de connexion : compte suivant.
    SwitchUser,
    Consume,
}

/// Le bouton du visage est-il proposé ? (visage utilisable, rien en cours.)
fn face_offered(lock: &LockView) -> bool {
    lock.face_ready
        && !lock.checking
        && !lock.welcome
        && !matches!(lock.face, LockFace::Scanning | LockFace::Recognized)
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
        if lock.switch_user && places.switch_user().contains(pos) {
            return LockAction::SwitchUser;
        }
        if places.submit().contains(pos) {
            return LockAction::Submit;
        }
        if lock.length > 0 && places.eye().contains(pos) {
            return LockAction::Peek;
        }
        if face_offered(lock) && (places.face_button().contains(pos) || places.avatar().contains(pos)) {
            return LockAction::Face;
        }
        return LockAction::Consume;
    }
    if places.theme().contains(pos) {
        return LockAction::ToggleTheme;
    }
    if let Some(index) = (0..wall_count(lock)).find(|index| places.swatch(*index).contains(pos)) {
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

/// Anneau du visage, sur une toile de `FACE_RING` points de côté centrée sur l'avatar.
/// Images 0 à `FACE_FRAMES - 1` : un arc à traîne qui fait le tour de l'avatar, et une
/// ligne de balayage qui monte et descend sur le visage. `FACE_FRAMES` : anneau fermé et
/// coche, visage reconnu.
fn draw_face_ring(canvas: &mut Canvas, frame: u16, track: Color) {
    use std::f64::consts::{FRAC_PI_2, TAU};
    let center = FACE_RING / 2.0;
    let radius = AVATAR / 2.0 + 11.0;
    let circle = Rect::new(center - radius, center - radius, radius * 2.0, radius * 2.0);
    if frame >= FACE_FRAMES {
        canvas.stroke(circle, radius as f32, ACCENT, 5.0);
        let badge = (center + 40.0, center + 40.0);
        canvas.fill(
            Rect::new(badge.0 - 14.0, badge.1 - 14.0, 28.0, 28.0),
            14.0,
            ACCENT,
        );
        canvas.line(
            (badge.0 - 6.0, badge.1),
            (badge.0 - 1.5, badge.1 + 5.0),
            2.4,
            ACCENT.ink_on(),
        );
        canvas.line(
            (badge.0 - 1.5, badge.1 + 5.0),
            (badge.0 + 6.5, badge.1 - 5.0),
            2.4,
            ACCENT.ink_on(),
        );
        return;
    }
    let turn = f64::from(frame) / f64::from(FACE_FRAMES);
    canvas.stroke(circle, radius as f32, track, 4.0);
    // Arc : la tête opaque, la traîne qui s'efface sur un tiers du tour ; un halo plus
    // large sous la tête. Les morceaux se chevauchent pour que l'arc reste continu.
    let head = turn * TAU - FRAC_PI_2;
    const STEPS: u8 = 40;
    const STEP: f64 = 0.055;
    for (width, strength) in [(11.0, 0.22), (5.0, 1.0)] {
        for step in (0..STEPS).rev() {
            let b = head - f64::from(step) * STEP;
            let a = b - STEP * 1.6;
            let fade = (1.0 - f64::from(step) / f64::from(STEPS)).powf(1.4);
            canvas.line(
                (center + a.cos() * radius, center + a.sin() * radius),
                (center + b.cos() * radius, center + b.sin() * radius),
                width,
                ACCENT.with_alpha((255.0 * fade * strength) as u8),
            );
        }
    }
    // Balayage : une ligne qui parcourt le visage de haut en bas, puis remonte.
    let inner = AVATAR / 2.0 - 4.0;
    let dy = (turn * TAU).cos() * (inner - 10.0);
    let half = (inner * inner - dy * dy).max(0.0).sqrt() - 4.0;
    let y = center + dy;
    canvas.line((center - half, y), (center + half, y), 7.0, ACCENT.with_alpha(70));
    canvas.line(
        (center - half, y),
        (center + half, y),
        2.0,
        Color::WHITE.with_alpha(200),
    );
}

/// Fond de l'écran en pixels BGRA : celui de la maquette « Fusion » (dégradé, lueurs et
/// anneaux), dans l'ambiance choisie.
pub(super) fn paint_lock_wall(width: u32, height: u32, dark: bool, wall: usize) -> Vec<u8> {
    let mood = LOCK_WALLS[wall.min(LOCK_WALLS.len() - 1)];
    let mut pixels = ferrite_ui::wallpaper::paint_lock(mood, dark, width, height).rgba;
    // RVBA vers BGRA.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    pixels
}

impl Canvas<'_> {
    /// Dégradé à 135° (du coin haut gauche au coin bas droit).
    pub(super) fn gradient(&mut self, rect: Rect, radius: f32, stops: &[(f32, u32)]) {
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
    pub(super) fn lock_wall(&self, width: u32, height: u32, lock: &LockView) -> MemoryRenderBuffer {
        let picture = lock.picture.as_ref().filter(|_| lock.wall == LOCK_WALLS.len());
        let key = (
            width,
            height,
            lock.dark,
            lock.wall,
            picture.map(|picture| std::sync::Arc::as_ptr(&picture.image) as usize),
            lock.blur,
        );
        let mut cache = self.lock_wall.borrow_mut();
        if let Some((known, buffer)) = cache.as_ref()
            && *known == key
        {
            return buffer.clone();
        }
        let pixels = match picture {
            Some(picture) => paint_lock_picture(width, height, lock.dark, lock.blur, picture),
            None => paint_lock_wall(width, height, lock.dark, lock.wall.min(LOCK_WALLS.len() - 1)),
        };
        let buffer = memory_buffer(&pixels, width, height);
        *cache = Some((key, buffer.clone()));
        buffer
    }

    /// Couches de l'écran de verrouillage, de la plus haute à la plus basse : heure, date,
    /// contenu (repos ou saisie), boutons, voile, fond. Chacune n'est dessinée qu'une fois ;
    /// le passage du repos à la saisie ne fait que déplacer, agrandir et fondre ces images,
    /// à la carte graphique (redessiner tout l'écran à chaque image prenait 20 ms).
    /// `content` est l'image du contenu, déjà dessinée.
    pub(super) fn lock_elements(
        &self,
        state: &VisualState,
        lock: &LockView,
        scale: f64,
        content: &MemoryRenderBuffer,
    ) -> Vec<LockElement> {
        let (width, height) = (state.width as f64, state.height as f64);
        let (pixels_w, pixels_h) = physical_size(state, scale);
        let ratio = lock_scale(width, height);
        let places = Places::new(width, height);
        let part = f64::from(lock.progress) / 1000.0;
        let full = Rect::new(0.0, 0.0, width, height);
        let mut elements = Vec::with_capacity(6);

        // Date et heure : l'image à la taille de repos, réduite pendant le passage ; la
        // taille de saisie a la sienne, nette, une fois arrivée.
        let clock = clock_places(part);
        let settled = if part >= 1.0 {
            Some(true)
        } else if part <= 0.0 {
            Some(false)
        } else {
            None
        };
        for (time, top, size) in [
            (true, clock.time_top, clock.time_size),
            (false, clock.date_top, clock.date_size),
        ] {
            let small = settled.unwrap_or(false);
            let base = match (time, small) {
                (true, false) => TIME_SIZE.0,
                (true, true) => TIME_SIZE.1,
                (false, false) => DATE_SIZE.0,
                (false, true) => DATE_SIZE.1,
            };
            let (buffer, pixels, band) = self.lock_clock(state, lock, scale, time, small);
            let grow = size / base;
            let (w, h) = (places.w * grow, band * grow);
            elements.push(LockElement {
                buffer,
                pixels,
                rect: Rect::new(
                    (places.w - w) / 2.0 * ratio,
                    (top - CLOCK_PAD * grow) * ratio,
                    w * ratio,
                    h * ratio,
                ),
                alpha: 1.0,
            });
        }

        // Anneau du visage autour de l'avatar : il tourne en passant d'une image déjà
        // dessinée à la suivante, sans redessiner l'écran.
        if lock.field && matches!(lock.face, LockFace::Scanning | LockFace::Recognized) {
            let frame = if lock.face == LockFace::Recognized {
                FACE_FRAMES
            } else {
                lock.face_tick % FACE_FRAMES
            };
            // Respiration de l'anneau, par son opacité.
            let breath = if lock.face == LockFace::Scanning {
                let wave = f64::from(lock.face_tick % 40) / 40.0 * std::f64::consts::TAU;
                0.82 + 0.18 * wave.cos()
            } else {
                1.0
            };
            let ring = places.face_ring();
            let side = (ring.w * ratio * scale).ceil().max(1.0) as u32;
            let buffer = self.face_ring(side, (scale * ratio) as f32, lock.dark, frame);
            elements.push(LockElement {
                buffer,
                pixels: (side, side),
                rect: Rect::new(ring.x * ratio, ring.y * ratio, ring.w * ratio, ring.h * ratio),
                alpha: (part * breath) as f32,
            });
        }

        // Le contenu de l'état visé apparaît en fondu.
        elements.push(LockElement {
            buffer: content.clone(),
            pixels: (pixels_w, pixels_h),
            rect: full,
            alpha: if lock.field { part } else { 1.0 - part } as f32,
        });

        let mut layers = self.lock_layers.borrow_mut();
        let key = lock_chrome_state(state);
        if !layers.chrome.as_ref().is_some_and(|(known, _)| *known == key) {
            let mut canvas = Canvas {
                pixels: vec![0; pixels_w as usize * pixels_h as usize * 4],
                width: pixels_w,
                height: pixels_h,
                scale: (scale * ratio) as f32,
                bold: &self.bold,
                icons: &self.icons,
            };
            self.draw_lock(&mut canvas, width, height, state, lock, LockLayer::Chrome);
            layers.chrome = Some((key, memory_buffer(&canvas.pixels, pixels_w, pixels_h)));
        }
        if let Some((_, chrome)) = &layers.chrome {
            elements.push(LockElement {
                buffer: chrome.clone(),
                pixels: (pixels_w, pixels_h),
                rect: full,
                alpha: 1.0,
            });
        }

        // Le voile s'épaissit en saisie : un seul pixel, étiré sur l'écran.
        let (rest, open) = tokens(lock.dark).veil;
        if !layers.veil.as_ref().is_some_and(|(known, _)| *known == open) {
            let pixel = [open.b, open.g, open.r, 255];
            layers.veil = Some((open, memory_buffer(&pixel, 1, 1)));
        }
        if let Some((_, veil)) = &layers.veil {
            elements.push(LockElement {
                buffer: veil.clone(),
                pixels: (1, 1),
                rect: full,
                alpha: (mix(f64::from(rest.a), f64::from(open.a), part) / 255.0) as f32,
            });
        }
        drop(layers);

        elements.push(LockElement {
            buffer: self.lock_wall(pixels_w, pixels_h, lock),
            pixels: (pixels_w, pixels_h),
            rect: full,
            alpha: 1.0,
        });
        elements
    }

    /// Image `frame` de l'anneau du visage (`FACE_FRAMES` : visage reconnu), gardée.
    fn face_ring(&self, side: u32, scale: f32, dark: bool, frame: u16) -> MemoryRenderBuffer {
        let mut layers = self.lock_layers.borrow_mut();
        let key = (side, dark);
        if !layers.face.as_ref().is_some_and(|(known, _)| *known == key) {
            layers.face = Some((key, vec![None; usize::from(FACE_FRAMES) + 1]));
        }
        let (_, frames) = layers.face.as_mut().expect("rempli juste avant");
        let slot = &mut frames[usize::from(frame.min(FACE_FRAMES))];
        if let Some(buffer) = slot {
            return buffer.clone();
        }
        let mut canvas = Canvas {
            pixels: vec![0; side as usize * side as usize * 4],
            width: side,
            height: side,
            scale,
            bold: &self.bold,
            icons: &self.icons,
        };
        draw_face_ring(&mut canvas, frame, tokens(dark).track);
        let buffer = memory_buffer(&canvas.pixels, side, side);
        *slot = Some(buffer.clone());
        buffer
    }

    /// Image de la date ou de l'heure, de toute la largeur, à la taille de repos ou de
    /// saisie ; et la hauteur de sa bande, en points de la toile.
    fn lock_clock(
        &self,
        state: &VisualState,
        lock: &LockView,
        scale: f64,
        time: bool,
        small: bool,
    ) -> (MemoryRenderBuffer, (u32, u32), f64) {
        let clock = state.clock;
        let text = if time {
            format!("{:02}:{:02}", clock.hour, clock.minute)
        } else {
            format!(
                "{} {} {}",
                DAYS[clock.weekday],
                clock.day,
                MONTHS[clock.month as usize - 1]
            )
        };
        let light = light_ink(lock, |picture| picture.clock_luma);
        let key = ClockKey {
            width: state.width,
            height: state.height,
            scale_milli: state.scale_milli,
            text,
            light,
            time,
            small,
        };
        let mut layers = self.lock_layers.borrow_mut();
        if let Some((_, buffer, pixels, band)) = layers.clocks.iter().find(|(known, ..)| *known == key) {
            return (buffer.clone(), *pixels, *band);
        }
        let (width, height) = (state.width as f64, state.height as f64);
        let ratio = lock_scale(width, height);
        let places = Places::new(width, height);
        let size = match (time, small) {
            (true, false) => TIME_SIZE.0,
            (true, true) => TIME_SIZE.1,
            (false, false) => DATE_SIZE.0,
            (false, true) => DATE_SIZE.1,
        };
        let band = size * 1.3 + 2.0 * CLOCK_PAD;
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
        let inks = tokens(light);
        let center = places.w / 2.0;
        if time {
            let width_time = canvas.tracked_width(&self.light, &key.text, size, -0.04);
            canvas.tracked(
                &self.light,
                &key.text,
                center - width_time / 2.0,
                CLOCK_PAD,
                size,
                -0.04,
                inks.fg,
            );
        } else {
            canvas.centered(&self.regular, &key.text, center, CLOCK_PAD, size, inks.fg2);
        }
        let buffer = memory_buffer(&canvas.pixels, pixels_w, pixels_h);
        // Les quatre images en usage (date et heure, aux deux tailles) ; une minute
        // nouvelle remplace les plus anciennes.
        if layers.clocks.len() >= 4 {
            layers.clocks.remove(0);
        }
        layers
            .clocks
            .push((key, buffer.clone(), (pixels_w, pixels_h), band));
        (buffer, (pixels_w, pixels_h), band)
    }

    /// Chevron et texte de l'invitation ; `baseline` est le bas de l'écran moins sa marge.
    fn draw_invitation(
        &self,
        canvas: &mut Canvas,
        lock: &LockView,
        center: f64,
        baseline: f64,
        tokens: &Tokens,
        level: f64,
    ) {
        let ink = tokens.fg2.with_alpha((level * f64::from(tokens.fg2.a)) as u8);
        let chevron = baseline - 33.0;
        canvas.line((center - 9.0, chevron + 8.0), (center, chevron), 2.4, ink);
        canvas.line((center, chevron), (center + 9.0, chevron + 8.0), 2.4, ink);
        canvas.centered(
            &self.regular,
            if lock.greeter {
                "Appuyez sur une touche ou cliquez pour vous connecter"
            } else {
                "Appuyez sur une touche ou cliquez pour déverrouiller"
            },
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
        let tokens = tokens(invitation_ink(lock));
        self.draw_invitation(
            &mut canvas,
            lock,
            places.w / 2.0,
            places.h - 46.0 - top,
            &tokens,
            1.0,
        );
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
        layer: LockLayer,
    ) {
        let places = Places::new(width, height);
        let tokens = tokens(lock.dark);

        if layer == LockLayer::Content {
            if lock.field {
                self.lock_entry(canvas, &places, lock, &tokens);
            } else {
                self.lock_rest(canvas, &places, lock, &tokens, clock_places(0.0).bottom);
            }
            return;
        }

        self.lock_status(canvas, &places, state, &tokens);

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

    /// En haut : la marque à gauche (thème « Fusion »), Wi-Fi et batterie à droite.
    fn lock_status(&self, canvas: &mut Canvas, places: &Places, state: &VisualState, tokens: &Tokens) {
        canvas.icon(
            "ferrite-symbol-white",
            (36.0, 30.0),
            16.0,
            Style::Symbolic(tokens.fg),
        );
        canvas.text(&self.bold, "Ferrite", 52.0, 22.0, 14.0, tokens.fg);
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
                svg::wifi_icon(true, state.view.wifi_signal),
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
            let inks = self::tokens(invitation_ink(lock));
            self.draw_invitation(
                canvas,
                lock,
                center,
                places.h - 46.0,
                &inks,
                pulse_level(lock.pulse),
            );
        }

        // Personnaliser : thème et fond de cet écran.
        let group = places.customize();
        let name = match LOCK_WALLS.get(lock.wall) {
            Some(mood) => mood.label(),
            None if lock.picture.is_some() => "Photo",
            None => LOCK_WALLS[0].label(),
        };
        let name_x = places.swatch(wall_count(lock) - 1).right() + 10.0;
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
        for (index, mood) in LOCK_WALLS.into_iter().enumerate() {
            let colors = mood.swatch(lock.dark);
            let swatch = places.swatch(index);
            if index == lock.wall {
                canvas.fill(swatch.expand(4.0), 19.0, ACCENT);
                canvas.fill(swatch.expand(2.0), 17.0, tokens.swatch_gap);
            }
            canvas.gradient(
                swatch,
                15.0,
                &[(0.0, colors[0]), (0.55, colors[1]), (1.0, colors[2])],
            );
        }
        if let Some(picture) = &lock.picture {
            let swatch = places.swatch(LOCK_WALLS.len());
            if lock.wall == LOCK_WALLS.len() {
                canvas.fill(swatch.expand(4.0), 19.0, ACCENT);
                canvas.fill(swatch.expand(2.0), 17.0, tokens.swatch_gap);
            }
            canvas.image_cover(swatch, &picture.thumb, 15.0);
        }
        canvas.text(&self.regular, name, name_x, group.y + 18.0, 12.0, tokens.fg2);
    }

    /// Saisie : avatar, nom, champ du mot de passe et message.
    fn lock_entry(&self, canvas: &mut Canvas, places: &Places, lock: &LockView, tokens: &Tokens) {
        let center = places.w / 2.0;
        let avatar = places.avatar();
        canvas.panel_shadow(avatar, 52.0);
        canvas.fill(avatar.expand(4.0), 56.0, tokens.ring);
        // Image de profil (Paramètres → Comptes), sinon l'initiale sur un dégradé.
        if let Some(picture) = &lock.avatar {
            canvas.image_cover(avatar, picture, AVATAR / 2.0);
        } else {
            canvas.gradient(avatar, 52.0, &[(0.0, 0xFF9466), (1.0, 0xB83A0A)]);
            canvas.centered(
                &self.bold,
                &lock.initial.to_string(),
                center,
                avatar.y + 27.0,
                42.0,
                Color::WHITE,
            );
        }
        canvas.centered(
            &self.bold,
            &lock.user,
            center,
            avatar.bottom() + 14.0,
            22.0,
            tokens.fg,
        );

        // Visage pas reconnu : l'anneau reste, à la couleur de l'erreur.
        if lock.face == LockFace::Failed {
            canvas.stroke(avatar.expand(11.0), 63.0, tokens.error.0, 4.0);
        }

        let field = places.field();
        if lock.checking || lock.welcome {
            let text = if lock.welcome && lock.face == LockFace::Recognized {
                "Visage reconnu, ouverture de la session…"
            } else if lock.welcome {
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
                    ACCENT,
                );
            }
            canvas.text(&self.regular, text, x + 30.0, field.y + 15.0, 14.0, tokens.fg2);
        } else {
            let shake = f64::from(lock.shake) / 10.0;
            let field = Rect::new(field.x + shake, field.y, field.w, field.h);
            let wrong = matches!(lock.message, LockMessage::Empty | LockMessage::Wrong);
            canvas.fill(field, 24.0, tokens.field);
            let edge = if wrong { tokens.error.0 } else { tokens.field_edge };
            canvas.stroke(field, 24.0, edge, 1.0);
            // Thème « Fusion » : anneau d'accent de 2 px, à 2 px du champ (champ actif).
            if !wrong {
                canvas.stroke(field.expand(3.0), 27.0, tokens.accent, 2.0);
            }
            let x = field.x + 16.0;
            let eye = places.eye();
            let eye = Rect::new(eye.x + shake, eye.y, eye.w, eye.h);
            // Place du texte, jusqu'à l'œil ; au-delà, la partie qui garde le curseur visible.
            let room = eye.x - 4.0 - x;
            let caret = if lock.length == 0 {
                // Clé live : le mot de passe de la session d'essai est rappelé dans le champ.
                let hint = if ferrite_system::catalog::is_live() {
                    "Mot de passe : ferrite"
                } else {
                    "Mot de passe"
                };
                canvas.text(&self.regular, hint, x + 6.0, field.y + 14.5, 15.0, tokens.fg3);
                x
            } else if let Some(text) = lock.shown.as_deref() {
                let chars: Vec<char> = text.chars().collect();
                let cursor = lock.cursor.min(chars.len());
                let width = |from: usize, to: usize| {
                    canvas.css_width(&self.regular, &chars[from..to].iter().collect::<String>(), 15.0)
                };
                let mut first = 0;
                while first < cursor && width(first, cursor) > room {
                    first += 1;
                }
                let mut last = chars.len();
                while last > cursor && width(first, last) > room {
                    last -= 1;
                }
                let caret = x + width(first, cursor);
                let visible: String = chars[first..last].iter().collect();
                canvas.text(&self.regular, &visible, x, field.y + 14.5, 15.0, tokens.fg);
                caret
            } else {
                let visible = ((room / 10.0) as usize).max(1);
                let cursor = lock.cursor.min(lock.length);
                let first = cursor
                    .saturating_sub(visible)
                    .min(lock.length.saturating_sub(visible));
                for index in 0..(lock.length - first).min(visible) {
                    let dot = Rect::new(x + index as f64 * 10.0, field.y + 20.0, 8.0, 8.0);
                    canvas.fill(dot, 4.0, tokens.fg);
                }
                x + (cursor - first) as f64 * 10.0 - 1.0
            };
            canvas.fill(Rect::new(caret, field.y + 14.0, 1.6, 20.0), 0.8, tokens.fg);
            if lock.length > 0 {
                let (name, ink) = if lock.shown.is_some() {
                    ("cc-eye-off", ACCENT)
                } else {
                    ("cc-eye", tokens.fg3)
                };
                canvas.icon(name, eye.center(), 18.0, Style::Symbolic(ink));
            }
            let submit = places.submit();
            let submit = Rect::new(submit.x + shake, submit.y, submit.w, submit.h);
            canvas.fill(submit, 18.0, tokens.accent);
            let (cx, cy) = submit.center();
            let ink = tokens.on_accent;
            canvas.line((cx - 5.0, cy), (cx + 5.0, cy), 2.0, ink);
            canvas.line((cx + 1.0, cy - 4.0), (cx + 5.0, cy), 2.0, ink);
            canvas.line((cx + 1.0, cy + 4.0), (cx + 5.0, cy), 2.0, ink);
            let (text, ink) = match (lock.message, lock.face) {
                (LockMessage::Empty, _) => ("Saisissez votre mot de passe", tokens.error.1),
                (LockMessage::Wrong, _) => ("Mot de passe incorrect", tokens.error.1),
                (_, LockFace::Scanning) => ("Regardez l’écran : reconnaissance du visage…", tokens.fg2),
                (_, LockFace::Failed) => (
                    "Visage non reconnu. Réessayez ou saisissez votre mot de passe",
                    tokens.error.1,
                ),
                (_, LockFace::Unavailable) => ("Caméra infrarouge indisponible", tokens.fg3),
                _ if lock.switch_user => (
                    "Entrée pour vous connecter · ↑ ↓ pour changer de compte",
                    tokens.fg3,
                ),
                _ if lock.greeter => ("Appuyez sur Entrée pour vous connecter", tokens.fg3),
                _ => ("Appuyez sur Entrée pour déverrouiller", tokens.fg3),
            };
            canvas.centered(&self.regular, text, center, field.bottom() + 14.0, 13.0, ink);
            if face_offered(lock) {
                let button = places.face_button();
                canvas.lock_button(button, tokens);
                let label = if matches!(lock.face, LockFace::Failed | LockFace::Unavailable) {
                    "Réessayer avec le visage"
                } else {
                    "Utiliser mon visage"
                };
                canvas.centered(
                    &self.regular,
                    label,
                    button.center().0,
                    button.y + 10.0,
                    13.0,
                    tokens.fg,
                );
            }
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
        // Écran de connexion : passe au compte suivant. Dans une session verrouillée, aucun
        // second compte ne s'ouvre d'ici : le bouton reste atténué.
        let switch = places.switch_user();
        canvas.lock_button(switch, tokens);
        canvas.centered(
            &self.regular,
            "Changer d’utilisateur",
            switch.center().0,
            switch.y + 12.0,
            13.0,
            if lock.switch_user {
                tokens.fg
            } else {
                tokens.fg.with_alpha(110)
            },
        );
    }
}
