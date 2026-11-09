//! Interface du bureau, avec l'aspect du Ferrite d'origine : une barre claire en haut (logo
//! et menu Applications à gauche, date et heure puis indicateurs à droite) et des menus
//! déroulants clairs qui s'ouvrent sous elle.
//!
//! La géométrie de chaque élément est calculée une seule fois par [`Layout`], qui sert
//! à la fois au dessin et à la détection des clics.

use std::{
    cell::{Cell, RefCell},
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ferrite_system::config::{Accent, ClockStyle};
use ferrite_ui::{
    theme::{Color, Palette},
    wallpaper::Image,
};
use fontdue::{Font, FontSettings};
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            },
            gles::GlesRenderer,
        },
    },
    output::Output,
    utils::{Logical, Point, Rectangle, Size},
};

mod auth;
mod cards;
mod island;
mod lock_screen;

pub use cards::{tasks_scroll_limit, widget_at, widget_edges_at, widgets_areas};
pub use lock_screen::{
    LOCK_BLUR_MAX, LOCK_WALLS, LockAction, LockPicture, avatar_path, load_avatar, lock_action_at,
};

use cards::dashboard_action_at;
pub use island::{IslandHit, IslandKind, IslandView, island_contains};

use crate::clipboard::{self, Filter as ClipFilter, Kind as ClipKind, Thumbnail};
use crate::desktop::svg::{self, IconCache, Style};
use crate::state::PowerAction;

const FONT_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Medium.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");
/// Grande heure de l'écran de verrouillage.
const FONT_LIGHT: &[u8] = include_bytes!("../../../assets/fonts/InterDisplay-Light.ttf");
/// Code, couleurs et chemins copiés, dans le presse-papiers.
const FONT_MONO: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");

/// Écart entre la barre (ou le bord de l'écran) et un menu déroulant.
const POPOVER_MARGIN: f64 = 8.0;
/// Icône de 18 px et écart de 16 px entre deux indicateurs.
const INDICATOR_WIDTH: f64 = 34.0;
/// Lanceur : icônes par ligne, case d'une application, marges et hauteurs.
const LAUNCHER_COLUMNS: usize = 6;
const LAUNCHER_CELL: (f64, f64) = (114.0, 118.0);
const LAUNCHER_PAD: f64 = 18.0;
/// Hauteur du champ de recherche, en haut du lanceur.
const LAUNCHER_HEADER: f64 = 76.0;
/// Hauteur du titre d'une rubrique, et espace entre deux rubriques.
const LAUNCHER_TITLE: f64 = 40.0;
const LAUNCHER_GAP: f64 = 14.0;
const LAUNCHER_MAX_HEIGHT: f64 = 560.0;
const LAUNCHER_RADIUS: f32 = 16.0;
/// Défilement d'un cran de molette, en pixels.
pub const LAUNCHER_SCROLL_STEP: f64 = 48.0;
const CONTROLS_WIDTH: f64 = 380.0;
/// Écart entre les modules des réglages rapides, et entre eux et le bord du panneau.
const CONTROLS_GAP: f64 = 14.0;
const CONTROLS_PAD: f64 = 16.0;
const CONTROLS_TILE: f64 = 64.0;
/// Rangées de tuiles du centre de contrôle, deux par rangée.
const CONTROLS_TILE_ROWS: f64 = 5.0;
const CONTROLS_ROW: f64 = 36.0;
/// Listes Wi-Fi et Bluetooth : haut (retour, titre), bas (état, lien), ligne, titre de groupe,
/// et place du mot de passe demandé sur une ligne.
const CONTROLS_LIST_TOP: f64 = 62.0;
const CONTROLS_LIST_BOTTOM: f64 = 46.0;
const CONTROLS_LIST_ROW: f64 = 54.0;
const CONTROLS_HEADING: f64 = 26.0;
const CONTROLS_PASSWORD: f64 = 118.0;
/// Panneau Super+V : largeur, hauteur au plus, en-tête (jusqu'à son filet), pied, et
/// marges de la liste en haut et en bas.
pub const CLIPBOARD_WIDTH: f64 = 440.0;
const CLIPBOARD_MAX_HEIGHT: f64 = 760.0;
const CLIPBOARD_HEAD: f64 = 152.0;
const CLIPBOARD_FOOT: f64 = 44.0;
const CLIPBOARD_LIST_PAD: (f64, f64) = (6.0, 10.0);
/// Largeur des pastilles de filtre, dans l'ordre de `Filter::ALL`, et du bouton « Tout
/// effacer ».
const CLIPBOARD_FILTERS: [f64; 5] = [48.0, 54.0, 52.0, 64.0, 70.0];
const CLIPBOARD_CLEAR: f64 = 98.0;

/// Hauteur du panneau Super+V sur un écran haut de `screen_h` points.
pub fn clipboard_height(screen_h: f64) -> f64 {
    (screen_h - 34.0 - 2.0 * POPOVER_MARGIN).clamp(380.0, CLIPBOARD_MAX_HEIGHT)
}

/// Hauteur visible de la liste du panneau Super+V.
pub fn clipboard_list_height(screen_h: f64) -> f64 {
    clipboard_height(screen_h) - CLIPBOARD_HEAD - CLIPBOARD_FOOT - CLIPBOARD_LIST_PAD.0 - CLIPBOARD_LIST_PAD.1
}
const DIALOG: (f64, f64) = (480.0, 344.0);
/// Fréquence de relecture de l'état de la batterie.
const BATTERY_REFRESH: Duration = Duration::from_secs(20);

/// Verre des réglages rapides : gris très clair presque opaque (sans flou derrière, un
/// verre plus transparent laisse lire les fenêtres au travers), liseré blanc et contour
/// sombre fins.
static DARK_THEME: AtomicBool = AtomicBool::new(false);
static ACCENT_RGB: AtomicU32 = AtomicU32::new(0);

fn palette() -> Palette {
    Palette::themed(DARK_THEME.load(Ordering::Relaxed))
}

fn themed<T>(light: T, dark: T) -> T {
    if DARK_THEME.load(Ordering::Relaxed) {
        dark
    } else {
        light
    }
}

/// Voile qui contraste avec le fond : noir en thème clair, blanc en thème sombre. Pour les
/// filets, les survols et les pastilles ; les ombres, elles, restent noires.
fn tint(alpha: u8) -> Color {
    themed(Color::BLACK, Color::WHITE).with_alpha(alpha)
}

/// Barre du haut : #0A0E14 à 55 % sur le fond d'écran, écriture blanche.
const BAR_VEIL: Color = Color::rgb(0x0A0E14).with_alpha(140);

/// Taille à donner à `Canvas::label` pour un texte de `css` pixels (la police y est agrandie
/// de 1,22 fois).
const fn px(css: f32) -> f32 {
    css / 1.22
}

/// Fond des panneaux système : #FAFBFC à 97 %, ou la surface sombre.
fn panel_fill() -> Color {
    themed(Color::rgb(0xFAFBFC), Color::rgb(0x2A2E35)).with_alpha(247)
}

/// Fond des boutons et des tuiles éteintes.
fn control_fill() -> Color {
    themed(Color::rgb(0xEEF0F4), Color::rgb(0x353A42))
}

fn primary_text() -> Color {
    palette().text
}

fn text_secondary() -> Color {
    palette().text_secondary
}

fn accent() -> Color {
    let rgb = ACCENT_RGB.load(Ordering::Relaxed);
    accent_for(palette(), (rgb != 0).then_some(rgb))
}

fn accent_for(palette: Palette, rgb: Option<u32>) -> Color {
    rgb.map_or(palette.accent, Color::rgb)
}

fn destructive() -> Color {
    palette().danger
}

fn text_strong() -> Color {
    palette().text
}

/// Menu déroulant ouvert depuis la barre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popover {
    Launcher,
    Controls,
    Screenshot,
    /// Historique du presse-papiers (Super+V).
    Clipboard,
    /// Menu d'une icône du Dock (clic droit).
    DockMenu,
    /// Menu du bureau (clic droit).
    DesktopMenu,
    /// Historique des notifications, ouvert depuis l'horloge ou Super+N.
    Notifications,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAction {
    TogglePopover(Popover),
    ClosePopover,
    /// Pastille rouge d'un enregistrement vidéo : l'arrête.
    StopRecording,
    /// Clic dans l'îlot du haut de l'écran.
    Island(IslandHit),
    /// Application du lanceur : position dans la liste (filtrée) des applications.
    LaunchTile(usize),
    ToggleWifi,
    ToggleBluetooth,
    SetVolume(u8),
    SetBrightness(u8),
    NotificationActivate(u32),
    /// Bouton d'action `index` d'une bannière.
    NotificationAction(u32, usize),
    NotificationsClear,
    ToggleDoNotDisturb,
    ToggleDarkMode,
    TogglePowerSaver,
    ToggleAirplane,
    /// Rétroéclairage du clavier : niveau suivant, puis éteint.
    CycleKeyboardLight,
    /// Mode nuit : allumer ou éteindre tout de suite, ou ouvrir ses réglages.
    ToggleNight,
    OpenNightSettings,
    /// Tuile Clavier : passe à l'autre disposition.
    ToggleKeyboardLayout,
    /// Chevron d'une tuile, ou retour aux tuiles.
    ControlsPage(ControlsPage),
    NetworkJoin(usize),
    /// L'œil d'un réseau enregistré : afficher ou cacher son mot de passe.
    NetworkPassword(usize),
    /// L'œil du champ de mot de passe.
    PasswordReveal,
    PasswordCancel,
    PasswordSubmit,
    DeviceAction(usize),
    /// Ouvre Paramètres : lien du centre de contrôle, ou sur une rubrique.
    OpenSettings,
    OpenSettingsPage(&'static str),
    Lock,
    /// Bouton rouge du centre de contrôle : toujours le dialogue d'arrêt, quel que soit le
    /// réglage du bouton physique.
    OpenPowerDialog,
    /// Dialogue d'arrêt : tuile choisie (Éteindre, Redémarrer, Veille).
    PowerChoose(PowerAction),
    ConfirmPower(PowerAction),
    CancelPower,
    /// Fenêtre d'authentification : autoriser (vérifier le mot de passe) ou annuler.
    AuthSubmit,
    AuthCancel,
    /// Historique du presse-papiers : indices dans la liste complète.
    ClipboardPaste(usize),
    ClipboardPin(usize),
    ClipboardDelete(usize),
    ClipboardClear,
    ClipboardFilter(ClipFilter),
    ClipboardUndo,
    /// Déplacement du panneau presse-papiers depuis sa barre ; décalage pointeur/carte.
    ClipboardDragStart(i32, i32),
    DashboardAddNote,
    DashboardEditNote(usize),
    DashboardAddTask,
    DashboardToggleTask(usize),
    DashboardCyclePriority(usize),
    DashboardDeleteTask(usize),
    DashboardMonth(i32),
    /// Ouvre le Calendrier sur un jour (année, mois, jour), ou sur le mois en cours :
    /// depuis le calendrier de la barre, et depuis le widget (qu'une fenêtre peut couvrir).
    OpenCalendar(Option<(i32, u32, u32)>),
    DashboardCalendar(Option<(i32, u32, u32)>),
    DashboardConsume,
    /// Corbeille de la note affichée.
    DashboardDeleteNote(usize),
    /// Onglet d'une feuille de tâches, son « + », et la croix de la feuille affichée.
    DashboardList(usize),
    DashboardAddList,
    DashboardDeleteList(usize),
    /// Application du mini-Dock (indice dans `DashboardView::apps`).
    DashboardApp(usize),
    /// Widget Musique : bouton du lecteur, ou ouvrir le lecteur qui joue (Musique sinon).
    DashboardMedia(crate::desktop::lock::MediaCommand),
    DashboardOpenMusic,
    /// Widgets Notes : montrer une note dans le widget Notes, cocher une ligne.
    DashboardShowNote(usize),
    DashboardToggleCheck(usize, usize),
    /// Note précédente ou suivante du widget Notes.
    DashboardNoteStep(i32),
    /// Mode d'édition des widgets.
    WidgetsDone,
    WidgetAdd(crate::desktop::widgets::WidgetKind),
    WidgetRemove(usize),
    WidgetsReset,
    /// Appui sur un widget : déplacement, ou redimensionnement par les bords donnés.
    WidgetDragStart(usize, crate::desktop::widgets::WidgetEdges),
    /// Marque de la barre : aller à ce bureau.
    GoToPage(usize),
    /// Choix `index` du menu d'une icône du Dock.
    DockMenu(usize),
    /// Choix `index` du menu du bureau.
    DesktopMenu(usize),
    /// Raccourci `index` du bureau.
    Shortcut(usize),
    /// Clic absorbé par un panneau, sans effet.
    Consume,
}

/// Ligne visible du panneau Super+V.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipRow {
    /// Position dans l'historique complet.
    pub index: usize,
    pub kind: crate::clipboard::Kind,
    pub label: String,
    /// Application d'origine et âge (« Terminal · il y a 2 min »).
    pub sub: String,
    /// Chiffre qui colle l'élément (les neuf premiers).
    pub key: Option<usize>,
    pub pinned: bool,
    pub selected: bool,
    /// Ordonnée depuis le haut de la liste, défilement compris, et hauteur.
    pub y: f64,
    pub h: f64,
    pub thumbnail: Option<Thumbnail>,
    /// Icône du fichier copié ; teinte d'une couleur copiée.
    pub file_icon: &'static str,
    pub swatch: Option<Color>,
}

/// Menu d'une icône du Dock, posé au-dessus d'elle.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DockMenuView {
    /// Centre de l'icône, et haut du Dock, en pixels logiques de l'écran.
    pub x: f64,
    pub bottom: f64,
    pub entries: Vec<String>,
    /// Ouvert par un clic droit hors du Dock (menu Applications) : le menu part du
    /// pointeur vers le bas, comme un menu contextuel, au lieu de monter au-dessus.
    pub at_pointer: bool,
}

impl Eq for DockMenuView {}
impl Eq for ClipRow {}
impl Eq for ClipboardView {}

/// Panneau Super+V, d'après la planche « Presse-papiers ».
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClipboardView {
    /// Lignes visibles et titres de section, aux ordonnées de la liste.
    pub rows: Vec<ClipRow>,
    pub sections: Vec<(&'static str, f64)>,
    /// Éléments de l'historique, filtrés ou non.
    pub count: usize,
    pub query: String,
    pub filter: ClipFilter,
    /// Liste vide : titre et explication.
    pub empty: Option<(&'static str, &'static str)>,
    /// Bandeau d'une suppression, qui propose « Annuler ».
    pub toast: Option<&'static str>,
    /// Position personnalisée de la carte, en pixels logiques sur l'écran.
    pub position: Option<(i32, i32)>,
}

/// Icône d'une case du menu Applications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileIcon {
    /// Icône Ferrite embarquée.
    Builtin(&'static str),
    /// Fichier d'icône d'une application installée.
    File(PathBuf),
    /// Pas d'icône trouvée : icône générique.
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub name: String,
    pub icon: TileIcon,
    /// Position dans la liste (filtrée) des applications.
    pub index: usize,
}

/// Rubrique du lanceur : titre, applications, et lien éventuel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherSection {
    pub title: String,
    pub tiles: Vec<Tile>,
}

/// Contenu du lanceur : recherche, rubriques et défilement.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LauncherView {
    pub query: String,
    pub sections: Vec<LauncherSection>,
    /// Défilement demandé, en pixels logiques (borné à l'affichage).
    pub scroll: u32,
    /// Fond d'écran flouté, vu à travers le verre du lanceur.
    pub backdrop: Option<Arc<Image>>,
}

/// État du bureau à afficher, fourni par le compositeur. `None` : matériel ou service
/// absent, le réglage est alors grisé.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DashboardTaskView {
    pub text: String,
    pub done: bool,
    pub priority: crate::desktop::widgets::Priority,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DashboardNoteView {
    pub title: String,
    pub preview: String,
    pub updated: u64,
    /// Toutes les lignes (titre compris), pour la liste à cocher de la Note épinglée.
    pub lines: Vec<String>,
}

/// Habillage d'un aperçu de la vue d'ensemble ; `frame` : x, y, largeur, hauteur logiques.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewTile {
    pub frame: (i32, i32, i32, i32),
    pub title: String,
    pub icon: crate::desktop::apps::AppIcon,
    pub hovered: bool,
    pub close_hovered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OverviewView {
    pub tiles: Vec<OverviewTile>,
    pub spaces: Vec<SpaceThumb>,
    /// Nombre fixe de bureaux : pas de vignette « nouvel espace » au bout.
    pub fixed: bool,
}

/// Ce que montre le centre de contrôle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlsPage {
    #[default]
    Main,
    Wifi,
    Bluetooth,
}

/// Réseau de la liste Wi-Fi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRow {
    pub index: usize,
    pub ssid: String,
    pub level: u8,
    pub secured: bool,
    pub connected: bool,
    /// Enregistré : son mot de passe peut s'afficher.
    pub known: bool,
    pub hint: String,
    /// Son mot de passe est affiché en clair à la place de l'indication.
    pub revealed: bool,
}

/// Mot de passe demandé sur la ligne d'un réseau : seule sa longueur arrive au dessin, sauf
/// si l'œil le montre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordRow {
    pub ssid: String,
    pub length: usize,
    pub shown: Option<String>,
}

/// Appareil de la liste Bluetooth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRow {
    pub index: usize,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    pub hint: String,
    pub glyph: &'static str,
    /// Une connexion est en cours : les boutons attendent.
    pub busy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ControlsView {
    pub page: ControlsPage,
    pub networks: Vec<NetworkRow>,
    pub devices: Vec<DeviceRow>,
    pub password: Option<PasswordRow>,
    pub scanning: bool,
    pub current_network: Option<String>,
    pub connected_devices: usize,
}

/// Une notification à l'écran ; sans icône, une cloche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationView {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub icon: Option<crate::desktop::apps::AppIcon>,
    /// « maintenant », « il y a 5 min », « 18:30 ».
    pub when: String,
    /// Libellés des boutons d'action.
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotificationsView {
    pub bubbles: Vec<NotificationView>,
    pub history: Vec<NotificationView>,
    pub do_not_disturb: bool,
}

/// Écran de verrouillage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockView {
    pub user: String,
    pub initial: char,
    /// Image de profil du compte, à la place de l'initiale.
    pub avatar: Option<Thumbnail>,
    /// Nombre de caractères tapés : le mot de passe n'arrive jusqu'au dessin que tant
    /// que l'œil est tenu (`shown`).
    pub length: usize,
    /// Place du curseur, en caractères.
    pub cursor: usize,
    pub shown: Option<String>,
    pub field: bool,
    /// Part du passage à la saisie, de 0 à 1000.
    pub progress: u16,
    pub checking: bool,
    pub welcome: bool,
    pub message: LockMessage,
    /// Déverrouillage par visage : où il en est, et s'il peut être (re)lancé.
    pub face: LockFace,
    pub face_ready: bool,
    /// Écran de connexion avec plusieurs comptes : « Changer d'utilisateur » passe au suivant.
    pub switch_user: bool,
    /// Écran de connexion (`--greeter`) plutôt que session verrouillée.
    pub greeter: bool,
    /// Pas de l'animation du visage (anneau qui tourne autour de l'avatar).
    pub face_tick: u16,
    /// Secousse du champ après un refus, en dixièmes de point.
    pub shake: i16,
    pub pulse: u8,
    pub spin: u8,
    pub dark: bool,
    /// Fond choisi : un des fonds dessinés, ou `LOCK_WALLS.len()` pour la photo.
    pub wall: usize,
    pub picture: Option<lock_screen::LockPicture>,
    /// Flou de la photo (0 : nette).
    pub blur: u8,
    pub weather: Option<ferrite_system::weather::Weather>,
    pub place: String,
    pub notice: Option<LockNotice>,
    pub media: Option<crate::desktop::lock::Media>,
}

/// Fenêtre d'authentification : ce que demande l'application, pour quel compte, et le
/// nombre de caractères tapés (jamais le mot de passe lui-même).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthView {
    pub message: String,
    pub user: String,
    pub length: usize,
    pub checking: bool,
    pub refused: bool,
}

/// Message sous le champ du mot de passe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMessage {
    Hint,
    Empty,
    Wrong,
}

/// Déverrouillage par visage, sur l'écran verrouillé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LockFace {
    /// Rien en cours.
    #[default]
    Idle,
    /// La caméra infrarouge cherche le visage : l'anneau tourne autour de l'avatar.
    Scanning,
    /// Visage reconnu : l'anneau se ferme, l'écran s'ouvre.
    Recognized,
    /// Visage vu mais pas reconnu (ou pas vu) : on peut réessayer.
    Failed,
    /// Caméra infrarouge ou modèles indisponibles.
    Unavailable,
}

/// Notifications reçues : l'application et leur nombre, jamais leur contenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockNotice {
    pub app: String,
    pub icon: Option<crate::desktop::apps::AppIcon>,
    pub count: usize,
    pub when: String,
}

/// Vignette d'un espace de travail dans la vue d'ensemble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceThumb {
    pub page: usize,
    pub windows: usize,
    pub current: bool,
}

/// Sélecteur Alt+Tab : une icône par fenêtre.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwitcherView {
    pub icons: Vec<crate::desktop::apps::AppIcon>,
    pub selected: usize,
    pub title: String,
}

/// Sélection d'une zone à capturer ; `rect` : x, y, largeur, hauteur logiques.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RegionView {
    pub rect: Option<(i32, i32, i32, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DashboardView {
    pub notes: Vec<DashboardNoteView>,
    /// Tâches de la feuille affichée.
    pub tasks: Vec<DashboardTaskView>,
    /// Noms des feuilles de tâches, et celle affichée.
    pub lists: Vec<String>,
    pub list_shown: usize,
    /// La feuille affichée est celle des tâches sans liste, qu'on ne supprime pas.
    pub loose_list: bool,
    /// Jours occupés (événement ou tâche à faire) du mois du widget Calendrier et du mois
    /// en cours (calendrier de la barre) : un bit par jour, bit 0 pour le 1er.
    pub busy_month: u32,
    pub busy_today: u32,
    pub draft: String,
    pub editing: Option<crate::desktop::widgets::DashboardEdit>,
    pub month_offset: i32,
    pub weather: Option<ferrite_system::weather::Weather>,
    pub place: String,
    /// Fond d'écran à peine flouté, vu au travers des cartes.
    pub backdrop: Option<Arc<Image>>,
    pub layout: Vec<crate::desktop::widgets::WidgetSlot>,
    pub note_shown: usize,
    pub tasks_scroll: usize,
    /// Widgets affichés et cliquables (pas dans la vue d'ensemble).
    pub shown: bool,
    /// Ramenés devant les fenêtres par Super+W.
    pub front: bool,
    /// Mode d'édition, et widget en train d'être déplacé.
    pub edit: bool,
    pub drag: Option<usize>,
    /// Case de la grille où ira le widget déplacé, montrée en creux.
    pub target: Option<crate::desktop::widgets::WidgetSlot>,
    /// Mini-Dock : applications récemment utilisées.
    pub apps: Vec<DashboardApp>,
    /// Raccourcis du bureau (applications, dossiers, fichiers).
    pub shortcuts: Vec<crate::desktop::shortcuts::ShortcutView>,
    /// Raccourcis affichés, sous les fenêtres : ni dans la vue d'ensemble ni sur l'écran
    /// verrouillé. Ils s'effacent sur le bureau des widgets, où ils ne prennent pas le clic
    /// (`shown`).
    pub shortcuts_shown: bool,
    /// Taille des icônes des raccourcis, en pixels logiques.
    pub icon_size: u32,
    /// Raccourci déplacé à la souris, et son décalage depuis sa case.
    pub shortcut_drag: Option<(usize, (i32, i32))>,
    /// Lecture en cours, pour le widget Musique.
    pub media: Option<crate::desktop::lock::Media>,
}

/// Application du mini-Dock des widgets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardApp {
    pub icon: crate::desktop::apps::AppIcon,
    pub name: String,
    pub running: bool,
}

/// Bulle affichée un instant après une touche de fonction (son, luminosité, micro, mode
/// avion) : pictogramme, texte et, pour un niveau, une jauge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Osd {
    pub icon: &'static str,
    pub label: &'static str,
    pub level: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DesktopView {
    pub dark_theme: bool,
    pub overview: Option<OverviewView>,
    pub switcher: Option<SwitcherView>,
    pub lock: Option<LockView>,
    pub notifications: NotificationsView,
    pub region: Option<RegionView>,
    /// Fenêtre survolée pendant la sélection d'une fenêtre à capturer (x, y, l, h).
    pub window_select: Option<(i32, i32, i32, i32)>,
    /// Zone déjà tracée, en cours d'ajustement avant la prise (x, y, l, h).
    pub region_adjust: Option<(i32, i32, i32, i32)>,
    pub popover: Option<Popover>,
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
    pub volume: Option<u8>,
    pub brightness: Option<u8>,
    pub airplane: Option<bool>,
    /// Rétroéclairage du clavier : niveau et maximum.
    pub keyboard_light: Option<(u8, u8)>,
    /// Économie d'énergie active (`None` : power-profiles-daemon absent).
    pub power_saver: Option<bool>,
    /// Mode nuit : teinte active, et l'état écrit sur sa tuile.
    pub night: (bool, String),
    pub power: Option<PowerAction>,
    /// Mot de passe administrateur demandé (polkit).
    pub auth: Option<AuthView>,
    /// Dialogue d'arrêt : secondes avant l'action automatique, applications ouvertes.
    pub power_left: u32,
    pub power_apps: usize,
    /// Dernier réglage refusé par le système.
    pub error: Option<String>,
    /// Barre du haut affichée (masquée quand une application est en plein écran).
    pub bar_visible: bool,
    /// Une fenêtre agrandie touche la barre : elle devient opaque.
    pub bar_opaque: bool,
    /// Réglages rapides qui ne viennent pas du fil système.
    pub controls: ControlsView,
    pub battery_percent: bool,
    pub bluetooth_icon: bool,
    pub clock_style: ClockStyle,
    pub keyboard: String,
    /// Hauteur configurable de la barre du haut.
    pub bar_height: u32,
    /// Page affichée (0 : widgets) et nombre de pages ; 0 page : pas de points dans la barre.
    pub page: usize,
    pub page_count: usize,
    /// Bande du bas occupée par le Dock, que les widgets évitent.
    pub dock_height: i32,
    pub launcher: LauncherView,
    pub clipboard: ClipboardView,
    pub dashboard: DashboardView,
    pub dock_menu: Option<DockMenuView>,
    /// Coin du menu du bureau, et ses lignes.
    pub desktop_menu: Option<(i32, i32)>,
    pub desktop_menu_entries: Vec<String>,
    /// Pointeur, connu seulement quand un menu est ouvert : il en surligne la ligne.
    pub menu_pointer: Option<(i32, i32)>,
    pub osd: Option<Osd>,
    pub capture: CaptureView,
    /// Enregistrement vidéo en cours : durée écoulée (« 0:07 »).
    pub recording: Option<String>,
    /// Îlot du haut de l'écran.
    pub island: IslandView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaptureView {
    pub mode: crate::render::capture::Mode,
    pub cursor: bool,
    /// Délai avant la prise, en secondes (0 : immédiate).
    pub delay: u64,
    /// Au moins une fenêtre est affichée : le mode Fenêtre est proposé.
    pub windows: bool,
    /// Vidéo choisie plutôt que photo.
    pub video: bool,
}

/// Ce qui est sous le pointeur dans la barre d'outils de capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureHit {
    Mode(crate::render::capture::Mode),
    Photo,
    Video,
    Shutter,
    Delay,
    Pointer,
    Close,
    /// Ailleurs sur la barre : le clic ne touche pas la zone dessous.
    Toolbar,
}

/// Modes de capture, dans l'ordre de la barre d'outils.
const CAPTURE_MODES: [(crate::render::capture::Mode, &str); 3] = [
    (crate::render::capture::Mode::Area, "Sélection"),
    (crate::render::capture::Mode::Screen, "Écran"),
    (crate::render::capture::Mode::Window, "Fenêtre"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct VisualState {
    width: u32,
    height: u32,
    scale_milli: u32,
    accent: Color,
    view: DesktopView,
    battery: Option<Battery>,
    clock: ClockSnapshot,
    system: SystemMetrics,
}

/// Taille en pixels physiques de l'écran décrit par `state`.
fn physical_size(state: &VisualState, scale: f64) -> (u32, u32) {
    (
        (state.width as f64 * scale).round().max(1.0) as u32,
        (state.height as f64 * scale).round().max(1.0) as u32,
    )
}

/// Ce dont dépend l'image des raccourcis du bureau : elle n'est redessinée que si cela
/// change, pas à chaque image.
#[derive(Debug, PartialEq)]
struct ShortcutsKey {
    size: (u32, u32, u32),
    icon: u32,
    shortcuts: Vec<crate::desktop::shortcuts::ShortcutView>,
    /// Case de chaque raccourci à l'écran.
    cells: Vec<(usize, Rect)>,
    /// Raccourci en cours de déplacement, dessiné à part.
    skip: Option<usize>,
}

/// Images des raccourcis : tous (sauf celui qu'on déplace), l'icône saisie, la case visée.
#[derive(Default)]
struct ShortcutLayers {
    rest: Option<(ShortcutsKey, Layer)>,
    lifted: Option<((usize, u32, u32, crate::desktop::shortcuts::ShortcutView), Layer)>,
    target: Option<((i32, i32, u32), Layer)>,
}

impl std::fmt::Debug for ShortcutLayers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShortcutLayers").finish_non_exhaustive()
    }
}

/// Widgets gardés d'une image à l'autre. Hors édition, seules les cartes dont les données
/// vivantes changent (heure, mesures du système, lecture en cours) sont redessinées, sur
/// leur verre, et seule leur zone est renvoyée à la carte graphique : redessiner tout
/// l'écran au processeur pour un pourcentage du processeur faisait sauter une image.
#[derive(Debug)]
struct DashboardCache {
    /// État sans ses parties vivantes : s'il change, tout est redessiné.
    base: VisualState,
    live: Vec<cards::CardLive>,
    pixels: Vec<u8>,
    buffer: MemoryRenderBuffer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ClockSnapshot {
    year: i64,
    month: u32,
    day: u32,
    weekday: usize,
    hour: u32,
    minute: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct SystemMetrics {
    cpu: u8,
    memory: u8,
    disk: u8,
    uptime_minutes: u64,
    download_kib: u64,
    upload_kib: u64,
    network_history: Vec<(u64, u64)>,
}

#[derive(Debug, Default)]
struct SystemMonitor {
    read_at: Option<Instant>,
    previous_cpu: Option<(u64, u64)>,
    previous_network: Option<(u64, u64)>,
    network_history: Vec<(u64, u64)>,
    metrics: SystemMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Battery {
    percent: u8,
    charging: bool,
}

/// Largeur et hauteur d'une image, en pixels.
type PixelSize = (u32, u32);
/// Toile (largeur, hauteur, échelle), panneau et teinte du verre du menu Applications.
type LauncherBackdropKey = (u32, u32, u32, Rect, Color);
/// Invitation de l'écran de verrouillage : écran (largeur, hauteur, échelle, thème
/// sombre), image, et place en logique (haut, hauteur).
type InvitationImage = ((u32, u32, u32, bool), MemoryRenderBuffer, (f64, f64));

#[derive(Debug)]
pub struct Panels {
    regular: Font,
    bold: Font,
    light: Font,
    mono: Font,
    icons: IconCache,
    system: RefCell<SystemMonitor>,
    cached: RefCell<Option<(VisualState, MemoryRenderBuffer, Option<PopoverLayer>)>>,
    /// Barre du haut, dessinée à part : elle change (opaque quand une fenêtre est
    /// agrandie, heure) sans qu'il faille redessiner tout l'écran.
    /// Image et sa taille en pixels.
    bar: RefCell<Option<(VisualState, MemoryRenderBuffer, PixelSize)>>,
    /// Invitation de l'écran de verrouillage, qui pulse par son opacité.
    invitation: RefCell<Option<InvitationImage>>,
    /// Widgets, dessinés à part : ils passent sous les fenêtres.
    dashboard: RefCell<Option<DashboardCache>>,
    /// Raccourcis du bureau, dessinés à part pour rester sur tous les bureaux.
    shortcuts: RefCell<ShortcutLayers>,
    /// Verre des widgets, gardé d'un dessin à l'autre (voir `cards::GlassKey`).
    glass: RefCell<Option<(cards::GlassKey, Vec<u8>)>>,
    /// Images du glisser d'un widget (voir `push_dragged_dashboard`).
    drag_layers: RefCell<DragLayers>,
    /// Dernier panneau animé affiché (centre de contrôle, session), gardé pour s'effacer
    /// après sa fermeture, et sa part d'apparition (0 à 1).
    layer: RefCell<Option<PopoverLayer>>,
    layer_shown: Cell<(f64, Option<Instant>)>,
    /// Îlot du haut de l'écran, et son mouvement.
    island: RefCell<island::IslandMotion>,
    /// Verre du lanceur, dernier calculé.
    frost: RefCell<Option<(FrostKey, Arc<Frost>)>>,
    /// Fond du menu Applications (voile, ombre et verre), gardé d'un dessin à l'autre.
    launcher_backdrop: RefCell<Option<(LauncherBackdropKey, Vec<u8>)>>,
    /// Fond de l'écran de verrouillage.
    lock_wall: RefCell<Option<(LockWallKey, MemoryRenderBuffer)>>,
    /// Autres couches de l'écran de verrouillage (voir `lock_elements`).
    lock_layers: RefCell<lock_screen::LockLayers>,
}

/// Durées de l'apparition et de la disparition d'un panneau, et sa taille de départ.
/// Choix du menu du bureau.
/// Marques des bureaux dans la barre.
const PAGE_MARK: f64 = 6.0;
const PAGE_MARK_ACTIVE: f64 = 16.0;
const PAGE_MARK_GAP: f64 = 6.0;
const LAYER_IN: Duration = ferrite_ui::tokens::motion::SHORT;
const LAYER_OUT: Duration = ferrite_ui::tokens::motion::QUICK;
const LAYER_SCALE: f64 = 0.94;

/// Panneau dessiné à part pour pouvoir s'animer : son rectangle (ombre comprise, en
/// pixels logiques de l'écran) et ses pixels.
#[derive(Debug, Clone)]
struct PopoverLayer {
    rect: Rect,
    buffer: MemoryRenderBuffer,
}

/// Panneaux qui apparaissent et disparaissent en fondu, grandissant depuis leur coin
/// haut droit (sous leur bouton de la barre), comme sur macOS.
fn animated(popover: Option<Popover>) -> bool {
    matches!(popover, Some(Popover::Controls))
}

/// Taille (pixels physiques), thème et choix du fond de l'écran de verrouillage, la
/// photo affichée (par son adresse) et son flou.
type LockWallKey = (u32, u32, bool, usize, Option<usize>, u8);
/// Place (pixels physiques), échelle et fond d'écran d'un verre calculé.
type FrostKey = ([u32; 4], u32, Option<usize>);

/// Verre dépoli prêt à poser : pixels BGRA prémultipliés du rectangle `left`, `top`,
/// `width` × `height` (pixels physiques de l'écran).
#[derive(Debug)]
struct Frost {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Panels {
    pub fn set_theme(dark: bool, accent: Accent) {
        DARK_THEME.store(dark, Ordering::Relaxed);
        ACCENT_RGB.store(accent.rgb().unwrap_or_default(), Ordering::Relaxed);
    }

    pub fn new() -> Result<Self, String> {
        let regular = Font::from_bytes(FONT_REGULAR, FontSettings::default()).map_err(|e| e.to_string())?;
        let bold = Font::from_bytes(FONT_BOLD, FontSettings::default()).map_err(|e| e.to_string())?;
        let light = Font::from_bytes(FONT_LIGHT, FontSettings::default()).map_err(|e| e.to_string())?;
        let mono = Font::from_bytes(FONT_MONO, FontSettings::default()).map_err(|e| e.to_string())?;
        Ok(Self {
            regular,
            bold,
            light,
            mono,
            icons: IconCache::default(),
            system: RefCell::new(SystemMonitor::default()),
            cached: RefCell::new(None),
            bar: RefCell::new(None),
            invitation: RefCell::new(None),
            dashboard: RefCell::new(None),
            shortcuts: RefCell::new(ShortcutLayers::default()),
            glass: RefCell::new(None),
            drag_layers: RefCell::new(DragLayers::default()),
            layer: RefCell::new(None),
            layer_shown: Cell::new((0.0, None)),
            island: RefCell::new(island::IslandMotion::default()),
            frost: RefCell::new(None),
            launcher_backdrop: RefCell::new(None),
            lock_wall: RefCell::new(None),
            lock_layers: RefCell::new(lock_screen::LockLayers::default()),
        })
    }

    pub fn push_element(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        output_geo: Rectangle<i32, Logical>,
        scale: f64,
        view: &DesktopView,
        out: &mut Vec<super::super::render::OutputElement>,
    ) {
        // Les widgets et l'îlot ont leurs propres images : les retirer évite de tout
        // redessiner pour eux. L'îlot montre aussi les bulles de notification, le son et
        // l'enregistrement ; seul l'écran verrouillé dessine encore la bulle du son.
        let state = VisualState {
            width: output_geo.size.w.max(1) as u32,
            height: output_geo.size.h.max(1) as u32,
            scale_milli: (scale * 1000.0).round() as u32,
            accent: accent(),
            view: DesktopView {
                dark_theme: view.dark_theme,
                // Seuls les jours occupés du mois en cours servent ici (calendrier de la barre).
                dashboard: DashboardView {
                    busy_today: view.dashboard.busy_today,
                    ..DashboardView::default()
                },
                island: IslandView::default(),
                recording: None,
                osd: view.osd.filter(|_| view.lock.is_some()),
                notifications: NotificationsView {
                    bubbles: Vec::new(),
                    ..view.notifications.clone()
                },
                ..view.clone()
            },
            battery: self.battery(),
            clock: clock_snapshot(SystemTime::now()),
            system: SystemMetrics::default(),
        };
        // La barre a sa propre image ; le reste ne dépend ni de son opacité ni de sa
        // présence, qui changent quand une fenêtre est agrandie.
        let bar_visible = state.view.bar_visible;
        // L'invitation de l'écran de verrouillage pulse par l'opacité de sa propre image.
        let pulse = state.view.lock.as_ref().map(|lock| lock.pulse);
        // Le passage du repos à la saisie ne redessine rien : le contenu est celui de l'état
        // visé, ses couches bougent à la carte graphique.
        let lock_now = state.view.lock.clone();
        let rest = VisualState {
            view: DesktopView {
                bar_opaque: false,
                bar_visible: true,
                lock: state.view.lock.clone().map(|lock| LockView {
                    pulse: lock_screen::PULSE_APART,
                    // L'anneau du visage a ses propres images : il tourne sans tout redessiner.
                    face_tick: 0,
                    progress: if lock.field { 1000 } else { 0 },
                    ..lock
                }),
                ..state.view.clone()
            },
            ..state.clone()
        };
        let invitation = state.view.lock.as_ref().map(|lock| {
            let key = (
                state.width,
                state.height,
                state.scale_milli,
                lock_screen::invitation_ink(lock),
            );
            let mut cache = self.invitation.borrow_mut();
            if !cache.as_ref().is_some_and(|(known, _, _)| *known == key) {
                let (buffer, place) = self.rasterize_invitation(&state, lock, scale);
                *cache = Some((key, buffer, place));
            }
            let (_, buffer, place) = cache.as_ref().expect("rempli juste avant");
            (buffer.clone(), *place)
        });
        let mut bar_cache = self.bar.borrow_mut();
        if state.view.bar_visible && !bar_cache.as_ref().is_some_and(|(key, _, _)| key == &state) {
            let (buffer, size) =
                crate::render::profile::raster("barre", || self.rasterize_bar(&state, scale));
            *bar_cache = Some((state, buffer, size));
        }
        let bar = bar_cache
            .as_ref()
            .filter(|_| bar_visible)
            .map(|(_, buffer, size)| (buffer.clone(), *size));
        drop(bar_cache);
        let state = rest;
        let mut cache = self.cached.borrow_mut();
        if !cache.as_ref().is_some_and(|(key, _, _)| key == &state) {
            if let Some((known, _, _)) = cache.as_ref() {
                crate::render::profile::changed("panneaux", known, &state);
            }
            let buffer = crate::render::profile::raster("panneaux", || self.rasterize(&state, scale));
            let layer = crate::render::profile::raster("calque", || self.rasterize_layer(&state, scale));
            *cache = Some((state, buffer, layer));
        }
        let Some((state, buffer, open)) = cache.as_ref() else {
            return;
        };

        // Panneau animé, au-dessus du reste.
        if let Some(open) = open {
            *self.layer.borrow_mut() = Some(open.clone());
        }
        let (target, duration) = if open.is_some() {
            (1.0, LAYER_IN)
        } else {
            (0.0, LAYER_OUT)
        };
        let shown = super::dock::ease(super::dock::DockMotion::step(&self.layer_shown, target, duration));
        if shown > 0.0
            && let Some(layer) = self.layer.borrow().as_ref()
        {
            let rect = layer.rect;
            let grow = LAYER_SCALE + (1.0 - LAYER_SCALE) * shown;
            let (w, h) = (rect.w * grow, rect.h * grow);
            let origin = Point::<f64, Logical>::from((rect.x + rect.w - w, rect.y));
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                origin.to_physical(scale),
                &layer.buffer,
                Some(shown as f32),
                // Comme pour le reste du bureau : pixels du tampon, posés sur `w` × `h`.
                Some(Rectangle::from_size((rect.w * scale, rect.h * scale).into())),
                Some(Size::from((w.round().max(1.0) as i32, h.round().max(1.0) as i32))),
                Kind::Unspecified,
            ) {
                Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
                Err(err) => tracing::warn!(?err, output = output.name(), "panneau animé non importé"),
            }
        }

        // Îlot, devant le bureau et la barre.
        let island_layout = Layout::new(output_geo.size.w as f64, output_geo.size.h as f64, view);
        self.push_island(renderer, output, &island_layout, scale, view, out);

        // L'invitation s'efface pendant la saisie.
        let entry = lock_now
            .as_ref()
            .map_or(0.0, |lock| f64::from(lock.progress) / 1000.0);
        if let (Some((image, (top, height))), Some(pulse)) = (invitation, pulse)
            && entry < 1.0
        {
            let size = (output_geo.size.w as f64 * scale, (height * scale).round());
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                Point::<f64, Logical>::from((0.0, top)).to_physical(scale),
                &image,
                Some((lock_screen::pulse_level(pulse) * (1.0 - entry)) as f32),
                Some(Rectangle::<f64, Logical>::from_size(size.into())),
                Some(Size::from((output_geo.size.w, height.round() as i32))),
                Kind::Unspecified,
            ) {
                Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
                Err(err) => tracing::warn!(?err, output = output.name(), "invitation non importée"),
            }
        }

        if let Some(lock) = &lock_now {
            for layer in self.lock_elements(state, lock, scale, buffer) {
                let rect = layer.rect;
                let source = (f64::from(layer.pixels.0), f64::from(layer.pixels.1));
                match MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    Point::<f64, Logical>::from((rect.x, rect.y)).to_physical(scale),
                    &layer.buffer,
                    (layer.alpha < 1.0).then_some(layer.alpha),
                    Some(Rectangle::<f64, Logical>::from_size(source.into())),
                    Some(Size::from((
                        rect.w.round().max(1.0) as i32,
                        rect.h.round().max(1.0) as i32,
                    ))),
                    Kind::Unspecified,
                ) {
                    Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
                    Err(err) => tracing::warn!(
                        ?err,
                        output = output.name(),
                        "couche de verrouillage non importée"
                    ),
                }
            }
        } else {
            let physical_size = (output_geo.size.w as f64 * scale, output_geo.size.h as f64 * scale);
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                Point::from((0.0, 0.0)),
                buffer,
                None,
                Some(Rectangle::<f64, Logical>::from_size(physical_size.into())),
                Some(output_geo.size),
                Kind::Unspecified,
            ) {
                Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
                Err(err) => tracing::warn!(?err, output = output.name(), "panneau du bureau non importé"),
            }
        }

        // Barre du haut, sous les menus.
        if let Some((bar, (width, height))) = bar {
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                Point::from((0.0, 0.0)),
                &bar,
                None,
                Some(Rectangle::<f64, Logical>::from_size(
                    (width as f64, height as f64).into(),
                )),
                Some(Size::from((
                    output_geo.size.w,
                    (height as f64 / scale).round() as i32,
                ))),
                Kind::Unspecified,
            ) {
                Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
                Err(err) => tracing::warn!(?err, output = output.name(), "barre du haut non importée"),
            }
        }
    }

    /// Widgets, avec l'opacité `alpha` : ils s'estompent quand une fenêtre les recouvre.
    #[allow(clippy::too_many_arguments)] // Rendu, écran, état et opacité.
    pub fn push_dashboard(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        output_geo: Rectangle<i32, Logical>,
        scale: f64,
        view: &DesktopView,
        alpha: f32,
        out: &mut Vec<super::super::render::OutputElement>,
    ) {
        let state = VisualState {
            width: output_geo.size.w.max(1) as u32,
            height: output_geo.size.h.max(1) as u32,
            scale_milli: (scale * 1000.0).round() as u32,
            accent: accent(),
            view: DesktopView {
                dark_theme: view.dark_theme,
                dashboard: view.dashboard.clone(),
                bar_height: view.bar_height,
                dock_height: view.dock_height,
                ..DesktopView::default()
            },
            battery: self.battery(),
            clock: clock_snapshot(SystemTime::now()),
            system: self.system_metrics(),
        };
        let dashboard = &state.view.dashboard;
        if let Some(index) = dashboard
            .drag
            .filter(|&index| dashboard.edit && index < dashboard.layout.len())
        {
            self.push_dragged_dashboard(renderer, output, scale, &state, index, alpha, out);
            return;
        }
        *self.drag_layers.borrow_mut() = DragLayers::default();
        let mut cache = self.dashboard.borrow_mut();
        let base = cards::base_key(&state);
        let live: Vec<cards::CardLive> = state
            .view
            .dashboard
            .layout
            .iter()
            .map(|slot| cards::card_live(slot.kind, &state))
            .collect();
        let incremental = cache.as_ref().is_some_and(|known| known.base == base);
        let redrawn = match cache.as_mut().filter(|_| incremental) {
            // En édition, les cartes peuvent se chevaucher et leur verre n'est pas en cache :
            // l'image est gardée telle quelle (heure et mesures figées, comme pendant un
            // glisser) au lieu d'être redessinée en entier à chaque mouvement de la souris.
            Some(_) if state.view.dashboard.edit => true,
            Some(known) => {
                let changed: Vec<usize> = (0..live.len())
                    .filter(|&index| known.live.get(index) != Some(&live[index]))
                    .collect();
                let done = changed.is_empty()
                    || crate::render::profile::raster("widgets:cartes-vivantes", || {
                        self.redraw_cards(known, &state, scale, &changed)
                    });
                known.live = live.clone();
                done
            }
            None => false,
        };
        if !redrawn {
            let pixels =
                crate::render::profile::raster("widgets", || self.dashboard_pixels(&state, scale, None));
            let (width, height) = physical_size(&state, scale);
            let buffer = MemoryRenderBuffer::from_slice(
                &pixels,
                Fourcc::Argb8888,
                (width as i32, height as i32),
                1,
                smithay::utils::Transform::Normal,
                None,
            );
            *cache = Some(DashboardCache {
                base,
                live,
                pixels,
                buffer,
            });
        }
        let Some(DashboardCache { buffer, .. }) = cache.as_ref() else {
            return;
        };
        let physical_size = (output_geo.size.w as f64 * scale, output_geo.size.h as f64 * scale);
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::from((0.0, 0.0)),
            buffer,
            Some(alpha),
            Some(Rectangle::<f64, Logical>::from_size(physical_size.into())),
            Some(output_geo.size),
            Kind::Unspecified,
        ) {
            Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
            Err(err) => tracing::warn!(?err, output = output.name(), "widgets non importés"),
        }
    }

    /// Raccourcis du bureau, sous les fenêtres, sur tous les bureaux sauf celui des
    /// widgets ; `alpha` : ils s'effacent à mesure qu'on approche de ce dernier. L'image est
    /// gardée tant que la liste, l'écran et la taille ne changent pas. Pendant un glisser,
    /// l'icône saisie et la case visée sont deux petites images posées par-dessus.
    #[allow(clippy::too_many_arguments)] // Rendu, écran, état et opacité.
    pub fn push_shortcuts(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        output_geo: Rectangle<i32, Logical>,
        scale: f64,
        view: &DesktopView,
        alpha: f32,
        out: &mut Vec<super::super::render::OutputElement>,
    ) {
        let dashboard = &view.dashboard;
        if !dashboard.shortcuts_shown || dashboard.shortcuts.is_empty() || alpha <= 0.0 {
            return;
        }
        let (w, h) = (output_geo.size.w.max(1) as u32, output_geo.size.h.max(1) as u32);
        let layout = Layout::new(f64::from(w), f64::from(h), view);
        let scale_milli = (scale * 1000.0).round() as u32;
        let cells = cards::visible_shortcuts(&layout, dashboard);
        let drag = dashboard.shortcut_drag.and_then(|(index, (dx, dy))| {
            let cell = cells.iter().find(|(known, _)| *known == index)?.1;
            Some((index, (f64::from(dx), f64::from(dy)), cell))
        });
        let key = ShortcutsKey {
            size: (w, h, scale_milli),
            icon: dashboard.icon_size,
            shortcuts: dashboard.shortcuts.clone(),
            cells: cells.clone(),
            skip: drag.map(|(index, ..)| index),
        };
        let mut layers = self.shortcuts.borrow_mut();
        if layers.rest.as_ref().is_none_or(|(known, _)| *known != key) {
            let width = (f64::from(w) * scale).round().max(1.0) as u32;
            let height = (f64::from(h) * scale).round().max(1.0) as u32;
            let mut canvas = Canvas {
                pixels: vec![0; width as usize * height as usize * 4],
                width,
                height,
                scale: scale as f32,
                bold: &self.bold,
                icons: &self.icons,
            };
            crate::render::profile::raster("raccourcis", || {
                self.draw_shortcuts(&mut canvas, &layout, dashboard, key.skip)
            });
            layers.rest = Some((key, canvas_buffer(canvas)));
        }
        if let Some((index, offset, cell)) = drag {
            let lifted_key = (
                index,
                dashboard.icon_size,
                scale_milli,
                dashboard.shortcuts[index].clone(),
            );
            if layers
                .lifted
                .as_ref()
                .is_none_or(|(known, _)| *known != lifted_key)
            {
                let lifted = Rect::new(LIFT_MARGIN, LIFT_MARGIN, cell.w, cell.h);
                let mut canvas = Canvas {
                    pixels: Vec::new(),
                    width: ((cell.w + 2.0 * LIFT_MARGIN) * scale).ceil().max(1.0) as u32,
                    height: ((cell.h + 2.0 * LIFT_MARGIN) * scale).ceil().max(1.0) as u32,
                    scale: scale as f32,
                    bold: &self.bold,
                    icons: &self.icons,
                };
                canvas.pixels = vec![0; canvas.width as usize * canvas.height as usize * 4];
                self.draw_shortcut(
                    &mut canvas,
                    &dashboard.shortcuts[index],
                    lifted,
                    dashboard.icon_size,
                );
                layers.lifted = Some((lifted_key, canvas_buffer(canvas)));
            }
            let target = cards::shortcut_target(&layout, dashboard, index, offset)
                .map(|target| layout.shortcut_grid(dashboard.icon_size).rect(target));
            if let Some(target) = target {
                let target_key = (target.w as i32, target.h as i32, scale_milli);
                if !layers
                    .target
                    .as_ref()
                    .is_some_and(|(known, ..)| *known == target_key)
                {
                    layers.target = Some((target_key, self.rasterize_target(target, scale)));
                }
            }
            // Du dessus vers le dessous : icône saisie, case visée.
            if let Some((_, (buffer, pixels))) = &layers.lifted {
                let at = (cell.x + offset.0 - LIFT_MARGIN, cell.y + offset.1 - LIFT_MARGIN);
                push_layer(renderer, output, buffer, *pixels, at, scale, alpha, out);
            }
            if let (Some(target), Some((_, (buffer, pixels)))) = (target, &layers.target) {
                let at = (target.x - TARGET_MARGIN, target.y - TARGET_MARGIN);
                push_layer(renderer, output, buffer, *pixels, at, scale, alpha, out);
            }
        }
        if let Some((_, (buffer, pixels))) = &layers.rest {
            push_layer(renderer, output, buffer, *pixels, (0.0, 0.0), scale, alpha, out);
        }
    }

    /// Widget déplacé ou redimensionné : tout redessiner à chaque image prendrait plus de
    /// 100 ms. Le reste du tableau de bord est figé (heure et mesures comprises) jusqu'au
    /// lâcher ; la case visée et le widget saisi sont deux petites images posées par-dessus,
    /// que le glisser ne fait que déplacer. Le verre du widget saisi montre le fond vu à
    /// l'endroit où il a été pris.
    #[allow(clippy::too_many_arguments)] // Rendu, écran, état et opacité.
    fn push_dragged_dashboard(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        scale: f64,
        state: &VisualState,
        index: usize,
        alpha: f32,
        out: &mut Vec<super::super::render::OutputElement>,
    ) {
        let layout = Layout::new(state.width as f64, state.height as f64, &state.view);
        let card = layout.dashboard_cards(&state.view.dashboard.layout)[index];
        let target = state.view.dashboard.target.map(|target| {
            let slot = target.clamped(layout.w as i32, layout.h as i32, layout.bar as i32);
            Rect::new(
                f64::from(slot.x),
                f64::from(slot.y),
                f64::from(slot.w),
                f64::from(slot.h),
            )
        });
        // Ce dont dépendent les images : tout, sauf la place du widget saisi et la case visée.
        let mut key = state.clone();
        key.clock = ClockSnapshot::default();
        key.system = SystemMetrics::default();
        key.view.dashboard.target = None;
        let slot = &mut key.view.dashboard.layout[index];
        let size = (slot.w, slot.h);
        (slot.x, slot.y, slot.w, slot.h) = (0, 0, 0, 0);

        let mut layers = self.drag_layers.borrow_mut();
        if !layers.rest.as_ref().is_some_and(|(known, _)| *known == key) {
            let mut rest = state.clone();
            rest.view.dashboard.target = None;
            layers.rest = Some((
                key.clone(),
                crate::render::profile::raster("widgets-dessous", || {
                    self.rasterize_dashboard(&rest, scale, Some(index))
                }),
            ));
        }
        if !layers
            .card
            .as_ref()
            .is_some_and(|(known, known_size, ..)| *known == key && *known_size == size)
        {
            let lifted = crate::render::profile::raster("widget-saisi", || {
                self.rasterize_lifted(state, index, card, scale)
            });
            layers.card = Some((key, size, lifted));
        }
        if let Some(target) = target {
            let target_key = (target.w as i32, target.h as i32, state.scale_milli);
            if !layers
                .target
                .as_ref()
                .is_some_and(|(known, ..)| *known == target_key)
            {
                layers.target = Some((target_key, self.rasterize_target(target, scale)));
            }
        }

        // Du dessus vers le dessous : widget saisi, case visée, reste.
        if let Some((.., (buffer, pixels))) = &layers.card {
            let at = (card.x - LIFT_MARGIN, card.y - LIFT_MARGIN);
            push_layer(renderer, output, buffer, *pixels, at, scale, alpha, out);
        }
        if let (Some(target), Some((_, (buffer, pixels)))) = (target, &layers.target) {
            let at = (target.x - TARGET_MARGIN, target.y - TARGET_MARGIN);
            push_layer(renderer, output, buffer, *pixels, at, scale, alpha, out);
        }
        if let Some((_, buffer)) = &layers.rest {
            let pixels = (
                (state.width as f64 * scale).round() as i32,
                (state.height as f64 * scale).round() as i32,
            );
            push_layer(renderer, output, buffer, pixels, (0.0, 0.0), scale, alpha, out);
        }
    }

    /// Widget `index` seul, dans une image à peine plus grande que lui (son badge de retrait
    /// déborde) ; et la taille de l'image en pixels.
    fn rasterize_lifted(&self, state: &VisualState, index: usize, card: Rect, scale: f64) -> Layer {
        let width = ((card.w + 2.0 * LIFT_MARGIN) * scale).ceil().max(1.0) as u32;
        let height = ((card.h + 2.0 * LIFT_MARGIN) * scale).ceil().max(1.0) as u32;
        let mut canvas = Canvas {
            pixels: vec![0; width as usize * height as usize * 4],
            width,
            height,
            scale: scale as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        let placement = cards::Placement {
            x: card.x - LIFT_MARGIN,
            y: card.y - LIFT_MARGIN,
            screen: (
                (state.width as f64 * scale).round().max(1.0),
                (state.height as f64 * scale).round().max(1.0),
            ),
        };
        let local = Rect::new(LIFT_MARGIN, LIFT_MARGIN, card.w, card.h);
        self.draw_card(&mut canvas, state, index, local, true, Some(placement));
        canvas_buffer(canvas)
    }

    /// Case visée par le widget saisi, de la taille de `target`.
    fn rasterize_target(&self, target: Rect, scale: f64) -> Layer {
        let width = ((target.w + 2.0 * TARGET_MARGIN) * scale).ceil().max(1.0) as u32;
        let height = ((target.h + 2.0 * TARGET_MARGIN) * scale).ceil().max(1.0) as u32;
        let mut canvas = Canvas {
            pixels: vec![0; width as usize * height as usize * 4],
            width,
            height,
            scale: scale as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        let local = Rect::new(TARGET_MARGIN, TARGET_MARGIN, target.w, target.h);
        cards::target_hollow(&mut canvas, local);
        canvas_buffer(canvas)
    }

    fn rasterize_dashboard(
        &self,
        state: &VisualState,
        scale: f64,
        skip: Option<usize>,
    ) -> MemoryRenderBuffer {
        let pixels = self.dashboard_pixels(state, scale, skip);
        let (width, height) = physical_size(state, scale);
        MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Argb8888,
            (width as i32, height as i32),
            1,
            smithay::utils::Transform::Normal,
            None,
        )
    }

    /// Pixels de tous les widgets (verre compris), sauf la carte `skip`.
    fn dashboard_pixels(&self, state: &VisualState, scale: f64, skip: Option<usize>) -> Vec<u8> {
        let (width, height) = physical_size(state, scale);
        let layout = Layout::new(state.width as f64, state.height as f64, &state.view);
        // Hors édition, les cartes ne se chevauchent pas : leur verre peut être posé
        // d'abord, d'un bloc, depuis le cache.
        let glass =
            (!state.view.dashboard.edit).then(|| self.glass_pixels(&layout, state, width, height, scale));
        let glass_drawn = glass.is_some();
        let mut canvas = Canvas {
            pixels: glass.unwrap_or_else(|| vec![0; width as usize * height as usize * 4]),
            width,
            height,
            scale: scale as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        self.draw_dashboard(&mut canvas, &layout, state, glass_drawn, skip);
        canvas.pixels
    }

    /// Redessine les cartes `changed` sur leur verre et ne renvoie que leur zone. Faux si le
    /// verre n'est plus en cache (tout sera alors redessiné).
    fn redraw_cards(
        &self,
        known: &mut DashboardCache,
        state: &VisualState,
        scale: f64,
        changed: &[usize],
    ) -> bool {
        let glass = self.glass.borrow();
        let Some((_, glass)) = glass.as_ref() else {
            return false;
        };
        let (width, height) = physical_size(state, scale);
        if glass.len() != known.pixels.len() || known.pixels.len() != width as usize * height as usize * 4 {
            return false;
        }
        let layout = Layout::new(state.width as f64, state.height as f64, &state.view);
        let cards = layout.dashboard_cards(&state.view.dashboard.layout);
        let mut canvas = Canvas {
            pixels: std::mem::take(&mut known.pixels),
            width,
            height,
            scale: scale as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        let mut damage: Vec<Rectangle<i32, smithay::utils::Buffer>> = Vec::new();
        for &index in changed {
            let Some(card) = cards.get(index).copied() else {
                continue;
            };
            // La carte, un peu élargie pour les bords adoucis.
            let x0 = ((card.x * scale).floor() as i64 - 2).clamp(0, i64::from(width)) as usize;
            let y0 = ((card.y * scale).floor() as i64 - 2).clamp(0, i64::from(height)) as usize;
            let x1 = ((card.right() * scale).ceil() as i64 + 2).clamp(0, i64::from(width)) as usize;
            let y1 = ((card.bottom() * scale).ceil() as i64 + 2).clamp(0, i64::from(height)) as usize;
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            for y in y0..y1 {
                let row = (y * width as usize + x0) * 4..(y * width as usize + x1) * 4;
                canvas.pixels[row.clone()].copy_from_slice(&glass[row]);
            }
            self.draw_card(&mut canvas, state, index, card, false, None);
            damage.push(Rectangle::new(
                (x0 as i32, y0 as i32).into(),
                ((x1 - x0) as i32, (y1 - y0) as i32).into(),
            ));
        }
        known.pixels = canvas.pixels;
        let pixels = &known.pixels;
        let row_bytes = width as usize * 4;
        let copied: Result<(), std::convert::Infallible> = known.buffer.render().draw(|memory| {
            for rect in &damage {
                for y in rect.loc.y..rect.loc.y + rect.size.h {
                    let start = y as usize * row_bytes + rect.loc.x as usize * 4;
                    let end = start + rect.size.w as usize * 4;
                    memory[start..end].copy_from_slice(&pixels[start..end]);
                }
            }
            Ok(damage.clone())
        });
        copied.is_ok()
    }

    /// État de la batterie, relu toutes les [`BATTERY_REFRESH`] par [`latest_battery`].
    fn battery(&self) -> Option<Battery> {
        latest_battery()
    }

    fn system_metrics(&self) -> SystemMetrics {
        let mut monitor = self.system.borrow_mut();
        if monitor
            .read_at
            .is_none_or(|read| read.elapsed() >= Duration::from_secs(2))
        {
            let now = Instant::now();
            let elapsed = monitor
                .read_at
                .map_or(0.0, |read| now.duration_since(read).as_secs_f64());
            monitor.read_at = Some(Instant::now());
            if let Some((total, idle)) = cpu_counters() {
                if let Some((previous_total, previous_idle)) = monitor.previous_cpu {
                    let elapsed = total.saturating_sub(previous_total);
                    let busy = elapsed.saturating_sub(idle.saturating_sub(previous_idle));
                    monitor.metrics.cpu = busy
                        .saturating_mul(100)
                        .checked_div(elapsed)
                        .map_or(0, |percent| percent.min(100) as u8);
                }
                monitor.previous_cpu = Some((total, idle));
            }
            if let Some(memory) = memory_usage() {
                monitor.metrics.memory = memory;
            }
            if let Some(disk) = disk_usage() {
                monitor.metrics.disk = disk;
            }
            monitor.metrics.uptime_minutes = fs::read_to_string("/proc/uptime")
                .ok()
                .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
                .map_or(0, |seconds| seconds as u64 / 60);
            if let Some((received, sent)) = network_counters() {
                if let Some((previous_received, previous_sent)) = monitor.previous_network
                    && elapsed > 0.0
                {
                    monitor.metrics.download_kib =
                        (received.saturating_sub(previous_received) as f64 / elapsed / 1024.0) as u64;
                    monitor.metrics.upload_kib =
                        (sent.saturating_sub(previous_sent) as f64 / elapsed / 1024.0) as u64;
                    let sample = (monitor.metrics.download_kib, monitor.metrics.upload_kib);
                    monitor.network_history.push(sample);
                    if monitor.network_history.len() > 32 {
                        monitor.network_history.remove(0);
                    }
                    monitor.metrics.network_history = monitor.network_history.clone();
                }
                monitor.previous_network = Some((received, sent));
            }
        }
        monitor.metrics.clone()
    }

    /// Barre du haut seule, dans une image de sa hauteur.
    fn rasterize_bar(&self, state: &VisualState, scale: f64) -> (MemoryRenderBuffer, PixelSize) {
        let layout = Layout::new(state.width as f64, state.height as f64, &state.view);
        let width = (state.width as f64 * scale).round().max(1.0) as u32;
        let height = (layout.bar * scale).ceil().max(1.0) as u32;
        let mut canvas = Canvas {
            pixels: vec![0; width as usize * height as usize * 4],
            width,
            height,
            scale: scale as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        self.draw_bar(&mut canvas, &layout, state);
        let buffer = MemoryRenderBuffer::from_slice(
            &canvas.pixels,
            Fourcc::Argb8888,
            (width as i32, height as i32),
            1,
            smithay::utils::Transform::Normal,
            None,
        );
        (buffer, (width, height))
    }

    fn rasterize(&self, state: &VisualState, scale: f64) -> MemoryRenderBuffer {
        let width = (state.width as f64 * scale).round().max(1.0) as u32;
        let height = (state.height as f64 * scale).round().max(1.0) as u32;
        let mut canvas = Canvas {
            pixels: vec![0; width as usize * height as usize * 4],
            width,
            height,
            scale: scale as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        let view = &state.view;
        let layout = Layout::new(state.width as f64, state.height as f64, view);

        // L'écran de verrouillage ne laisse rien voir d'autre ; ici, son contenu seul (le
        // reste est dans ses autres couches, voir `lock_elements`).
        if let Some(lock) = &view.lock {
            canvas.scale = (scale * lock_screen::lock_scale(state.width as f64, state.height as f64)) as f32;
            self.draw_lock(
                &mut canvas,
                state.width as f64,
                state.height as f64,
                state,
                lock,
                lock_screen::LockLayer::Content,
            );
            canvas.scale = scale as f32;
            if let Some(osd) = view.osd {
                self.draw_osd(&mut canvas, &layout, osd, 0);
            }
            // Arrêt demandé depuis l'écran verrouillé : la confirmation passe devant.
            if let Some(power) = view.power {
                self.draw_dialog(&mut canvas, &layout, power, view);
            }
            return MemoryRenderBuffer::from_slice(
                &canvas.pixels,
                Fourcc::Argb8888,
                (width as i32, height as i32),
                1,
                smithay::utils::Transform::Normal,
                None,
            );
        }

        // Barre du haut : dessinée à part, par `rasterize_bar`.

        match view.popover {
            Some(Popover::Launcher) => self.draw_launcher(&mut canvas, &layout, &view.launcher),
            // Dessinée plus bas, au-dessus du voile de la zone ou de la fenêtre.
            Some(Popover::Screenshot) => {}
            // Dessinés à part, par `rasterize_layer`.
            Some(Popover::Controls) => {}
            Some(Popover::Clipboard) => self.draw_clipboard(&mut canvas, &layout, &view.clipboard),
            Some(Popover::Notifications) => {
                self.draw_notification_center(
                    &mut canvas,
                    &layout,
                    &view.notifications,
                    view.dashboard.busy_today,
                );
            }
            Some(Popover::DockMenu) => {
                if let Some(menu) = &view.dock_menu {
                    self.draw_dock_menu(&mut canvas, &layout, menu, view.menu_pointer);
                }
            }
            Some(Popover::DesktopMenu) => {
                if let Some(anchor) = view.desktop_menu {
                    self.draw_desktop_menu(
                        &mut canvas,
                        &layout,
                        anchor,
                        &view.desktop_menu_entries,
                        view.menu_pointer,
                    );
                }
            }
            None => {}
        }

        // Notifications, son et enregistrement : dans l'îlot (voir `push_island`).

        if let Some(overview) = &view.overview {
            self.draw_overview(&mut canvas, &layout, overview);
        }
        if let Some(switcher) = &view.switcher {
            self.draw_switcher(&mut canvas, &layout, switcher);
        }
        if let Some(region) = view.region {
            self.draw_region(&mut canvas, &layout, region);
        }
        if let Some(window) = view.window_select {
            self.draw_window_select(&mut canvas, &layout, window);
        }
        if let Some(rect) = view.region_adjust {
            self.draw_region_adjust(&mut canvas, &layout, rect);
        }
        if view.popover == Some(Popover::Screenshot) {
            self.draw_capture(&mut canvas, &layout, view.capture);
        }

        // Confirmation explicite, pour éviter un arrêt accidentel.
        if let Some(power) = view.power {
            self.draw_dialog(&mut canvas, &layout, power, view);
        }
        // Mot de passe administrateur : par-dessus tout le reste.
        if let Some(auth) = &view.auth {
            self.draw_auth(&mut canvas, &layout, auth);
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

    /// Panneau animé ouvert, seul, découpé à sa place (ombre comprise).
    fn rasterize_layer(&self, state: &VisualState, scale: f64) -> Option<PopoverLayer> {
        let view = &state.view;
        if !animated(view.popover) {
            return None;
        }
        let width = (state.width as f64 * scale).round().max(1.0) as u32;
        let height = (state.height as f64 * scale).round().max(1.0) as u32;
        let mut canvas = Canvas {
            pixels: vec![0; width as usize * height as usize * 4],
            width,
            height,
            scale: scale as f32,
            bold: &self.bold,
            icons: &self.icons,
        };
        let layout = Layout::new(state.width as f64, state.height as f64, view);
        self.draw_controls(&mut canvas, &layout, state);
        let card = layout.controls();
        // Marge pour l'ombre, en pixels physiques entiers.
        let left = ((card.x - 16.0) * scale).floor().max(0.0) as u32;
        let top = ((card.y - 16.0) * scale).floor().max(0.0) as u32;
        let right = (((card.x + card.w + 16.0) * scale).ceil() as u32).min(width);
        let bottom = (((card.y + card.h + 20.0) * scale).ceil() as u32).min(height);
        if right <= left || bottom <= top {
            return None;
        }
        let (w, h) = (right - left, bottom - top);
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
        for row in top..bottom {
            let start = (row as usize * width as usize + left as usize) * 4;
            pixels.extend_from_slice(&canvas.pixels[start..start + w as usize * 4]);
        }
        Some(PopoverLayer {
            rect: Rect::new(
                left as f64 / scale,
                top as f64 / scale,
                w as f64 / scale,
                h as f64 / scale,
            ),
            buffer: MemoryRenderBuffer::from_slice(
                &pixels,
                Fourcc::Argb8888,
                (w as i32, h as i32),
                1,
                smithay::utils::Transform::Normal,
                None,
            ),
        })
    }

    /// Soleil : rayons proportionnels au disque, comme sur la maquette.
    fn sun(&self, canvas: &mut Canvas, center: (f64, f64), radius: f64, color: Color) {
        canvas.fill(
            Rect::new(center.0 - radius, center.1 - radius, radius * 2.0, radius * 2.0),
            radius as f32,
            color,
        );
        for angle in 0..8 {
            let (sin, cos) = (std::f64::consts::TAU * f64::from(angle) / 8.0).sin_cos();
            canvas.line(
                (center.0 + cos * radius * 1.5, center.1 + sin * radius * 1.5),
                (center.0 + cos * radius * 2.0, center.1 + sin * radius * 2.0),
                (radius * 0.3).max(1.0),
                color,
            );
        }
    }

    /// Symbole du temps ; `k` : son échelle (1 : celle des prévisions).
    fn weather_symbol(
        &self,
        canvas: &mut Canvas,
        center: (f64, f64),
        sky: ferrite_system::weather::Sky,
        k: f64,
    ) {
        use ferrite_system::weather::Sky;
        let sun = Color::rgb(0xFFC53D);
        match sky {
            Sky::Clear => self.sun(canvas, center, 4.8 * k, sun),
            Sky::Partly => {
                self.sun(canvas, (center.0 - 4.0 * k, center.1 - 2.0 * k), 4.0 * k, sun);
                canvas.icon(
                    "status-cloud",
                    (center.0 + 3.0 * k, center.1 + 4.0 * k),
                    21.0 * k,
                    Style::Color,
                );
            }
            Sky::Cloudy | Sky::Fog => {
                canvas.icon("status-cloud", center, 22.0 * k, Style::Color);
            }
            Sky::Rain | Sky::Storm | Sky::Snow => {
                canvas.icon("status-cloud", center, 21.0 * k, Style::Color);
                let precipitation = if sky == Sky::Snow {
                    Color::rgb(0x74AFFF)
                } else {
                    Color::rgb(0x3089F2)
                };
                for offset in [-5.0, 3.0] {
                    canvas.line(
                        (center.0 + offset * k, center.1 + 6.0 * k),
                        (center.0 + (offset - 2.0) * k, center.1 + 11.0 * k),
                        1.8,
                        precipitation,
                    );
                }
            }
        }
    }

    fn draw_bar(&self, canvas: &mut Canvas, layout: &Layout, state: &VisualState) {
        let view = &state.view;
        let (veil, ink, pill) = if view.bar_opaque {
            (palette().titlebar, primary_text(), tint(26))
        } else {
            (BAR_VEIL, Color::WHITE, Color::WHITE.with_alpha(46))
        };
        canvas.fill(Rect::new(0.0, 0.0, layout.w, layout.bar), 0.0, veil);
        let applications = layout.applications_button();
        // Pastille sous l'élément de la barre dont le panneau est ouvert.
        let open = view.popover.and_then(|popover| match popover {
            Popover::Launcher => Some(applications),
            Popover::Controls => Some(layout.indicators_group()),
            Popover::Notifications => Some(layout.clock_button()),
            Popover::Clipboard | Popover::DockMenu | Popover::DesktopMenu | Popover::Screenshot => None,
        });
        if let Some(rect) = open {
            canvas.fill(Rect::new(rect.x, 4.0, rect.w, layout.bar - 8.0), 8.0, pill);
        }
        // Symbole de Ferrite OS sans tuile, de la teinte du texte de la barre : blanc sur le
        // voile sombre ou la barre opaque du thème sombre, sombre sur la barre opaque claire.
        let symbol = if ink.r > 128 {
            "ferrite-symbol-white"
        } else {
            "ferrite-symbol"
        };
        canvas.icon(
            symbol,
            (applications.x + 20.0, layout.bar / 2.0),
            20.0,
            Style::Color,
        );
        canvas.bar_label(
            &self.bold,
            "Applications",
            applications.x + 40.0,
            px(14.0),
            ink,
            layout.bar,
        );
        // Une marque par bureau : une pilule pour celui qui est affiché.
        for page in 0..view.page_count {
            let slot = layout.page_dot(page);
            let (center_x, center_y) = slot.center();
            let (width, color) = if page == view.page {
                (PAGE_MARK_ACTIVE, ink)
            } else {
                (PAGE_MARK, ink.with_alpha(102))
            };
            canvas.fill(
                Rect::new(center_x - width / 2.0, center_y - 3.0, width, 6.0),
                3.0,
                color,
            );
        }
        let clock = clock_text(view.clock_style, clock_label(SystemTime::now(), local_offset()));
        let clock_width = canvas.text_width(&self.bold, &clock, px(14.0));
        canvas.bar_label(
            &self.bold,
            &clock,
            (layout.w - clock_width) / 2.0,
            px(14.0),
            ink,
            layout.bar,
        );
        for (indicator, rect) in layout.indicators() {
            let name = match indicator {
                Indicator::Bluetooth if view.bluetooth == Some(true) => "status-bluetooth-on",
                Indicator::Bluetooth => "status-bluetooth-off",
                Indicator::Network => match view.wifi {
                    Some(enabled) => svg::wifi_icon(enabled),
                    None => "status-network",
                },
                Indicator::Volume => svg::volume_icon(view.volume.unwrap_or(0)),
                Indicator::Battery => match state.battery {
                    Some(battery) => svg::battery_icon(battery.percent, battery.charging),
                    None => continue,
                },
            };
            // Les tracés du Wi-Fi remplissent leur dessin : plus petits, ils gardent le même
            // poids visuel que le son et la batterie.
            let size = match indicator {
                Indicator::Bluetooth => 19.0,
                Indicator::Network if view.wifi.is_some() => 21.0,
                _ => 26.0,
            };
            canvas.icon(name, rect.center(), size, Style::Symbolic(ink));
        }
        if let (true, Some(battery)) = (view.battery_percent, state.battery) {
            let rect = layout.indicator(Indicator::Battery);
            let text = format!("{} %", battery.percent);
            canvas.bar_label(
                &self.regular,
                &text,
                rect.right() - 2.0,
                px(12.0),
                ink,
                layout.bar,
            );
        }
    }

    /// Lanceur d'applications, au centre de l'écran comme celui de macOS : verre blanc
    /// dépoli, champ de recherche en haut, puis les rubriques (une ligne d'icônes chacune,
    /// « Plus » pour tout voir).
    fn draw_launcher(&self, canvas: &mut Canvas, layout: &Layout, launcher: &LauncherView) {
        let card = layout.launcher();
        let content = layout.launcher_content();
        let frost = self.frost(canvas, card);
        // Voile de 40 % sur tout le bureau, ombre et verre : ils ne changent qu'avec
        // l'écran, alors que le reste est redessiné à chaque touche de la recherche et à
        // chaque pas de défilement. Le menu est dessiné le premier, sur une toile vide.
        let key = (
            canvas.width,
            canvas.height,
            canvas.scale.to_bits(),
            card,
            panel_fill(),
        );
        let mut backdrop = self.launcher_backdrop.borrow_mut();
        match backdrop.as_ref() {
            Some((known, pixels)) if *known == key => canvas.pixels.copy_from_slice(pixels),
            _ => {
                canvas.fill(
                    Rect::new(0.0, 0.0, layout.w, layout.h),
                    0.0,
                    Color::BLACK.with_alpha(102),
                );
                canvas.panel_shadow(card, LAUNCHER_RADIUS);
                canvas.blit_frost(&frost, None);
                *backdrop = Some((key, canvas.pixels.clone()));
            }
        }
        drop(backdrop);

        // Ce qui défile peut déborder du panneau : l'écran autour est gardé puis remis.
        let margin = LAUNCHER_CELL.1 + LAUNCHER_TITLE + LAUNCHER_GAP;
        let above = canvas.save_rows(card.y - margin, card.y);
        let below = canvas.save_rows(card.bottom(), card.bottom() + margin);
        for item in layout.launcher_items(launcher) {
            match item {
                _ if !item.visible_in(content) => {}
                LauncherItem::Title { section, rect } => {
                    let title = &launcher.sections[section].title;
                    canvas.label(
                        &self.bold,
                        &title.to_uppercase(),
                        rect.x,
                        rect.y + 15.0,
                        px(11.0),
                        text_secondary(),
                    );
                }
                LauncherItem::Tile { section, slot, rect } => {
                    self.draw_tile(canvas, &launcher.sections[section].tiles[slot], rect);
                }
            }
        }

        // Ce qui déborde du contenu (défilement) est recouvert par le verre, puis le champ
        // de recherche est dessiné par-dessus.
        canvas.restore_rows(above);
        canvas.restore_rows(below);
        canvas.blit_frost(&frost, Some((card.y, content.y)));
        canvas.blit_frost(&frost, Some((content.bottom(), card.bottom())));
        // Champ de recherche de 48 px.
        let field = Rect::new(card.x + 16.0, card.y + 16.0, card.w - 32.0, 48.0);
        canvas.fill(field, 10.0, control_fill());
        canvas.icon(
            "cc-search",
            (field.x + 24.0, field.y + 24.0),
            18.0,
            Style::Symbolic(text_secondary()),
        );
        let text_x = field.x + 44.0;
        let text_y = field.y + (field.h - 15.0 * 1.0 - 4.0) / 2.0;
        let caret = if launcher.query.is_empty() {
            canvas.label(
                &self.regular,
                "Rechercher des apps, fichiers, réglages",
                text_x + 4.0,
                text_y,
                px(15.0),
                text_secondary(),
            );
            text_x
        } else {
            canvas.label(
                &self.regular,
                &launcher.query,
                text_x,
                text_y,
                px(15.0),
                text_strong(),
            );
            text_x + 2.0 + canvas.text_width(&self.regular, &launcher.query, px(15.0))
        };
        canvas.fill(Rect::new(caret, field.y + 13.0, 2.0, 22.0), 1.0, accent());
    }

    /// Barre d'outils de capture, toujours sombre : modes, photo ou vidéo, déclencheur,
    /// délai et pointeur, et la croix pour fermer.
    fn draw_capture(&self, canvas: &mut Canvas, layout: &Layout, capture: CaptureView) {
        let bar = layout.capture_toolbar();
        let background = Color::rgb(0x2c2c2e);
        let white = Color::WHITE;
        let dim = Color::WHITE.with_alpha(90);
        canvas.panel_shadow(bar, 24.0);
        canvas.fill(bar, 24.0, background.with_alpha(245));

        for (index, (mode, label)) in CAPTURE_MODES.into_iter().enumerate() {
            let rect = layout.capture_mode(index);
            let enabled = mode != crate::render::capture::Mode::Window || capture.windows;
            let selected = capture.mode == mode;
            let fill = if selected {
                Color::rgb(0x48484a)
            } else {
                background
            };
            if selected {
                canvas.fill(rect, 14.0, fill);
            }
            let ink = if enabled { white } else { dim };
            let center = (rect.x + rect.w / 2.0, rect.y + 25.0);
            match mode {
                crate::render::capture::Mode::Area => capture_icon_selection(canvas, center, ink),
                crate::render::capture::Mode::Screen => capture_icon_screen(canvas, center, ink),
                crate::render::capture::Mode::Window => capture_icon_window(canvas, center, ink, fill),
            }
            let width = canvas.text_width(&self.regular, label, px(12.0));
            canvas.label(
                &self.regular,
                label,
                rect.x + (rect.w - width) / 2.0,
                rect.y + 44.0,
                px(12.0),
                ink,
            );
        }

        // Photo ou vidéo : la moitié choisie est blanche.
        let photo = layout.capture_kind(false);
        let video = layout.capture_kind(true);
        canvas.fill(
            Rect::new(photo.x, photo.y, photo.w + video.w, photo.h),
            17.0,
            Color::WHITE.with_alpha(22),
        );
        let (chosen, photo_ink, video_ink) = if capture.video {
            (video, white, background)
        } else {
            (photo, background, white)
        };
        canvas.fill(chosen, 17.0, white);
        capture_icon_camera(canvas, photo.center(), photo_ink);
        capture_icon_video(canvas, video.center(), video_ink);

        // Déclencheur : anneau blanc, disque blanc (photo) ou rouge (vidéo).
        let shutter = layout.capture_shutter();
        canvas.stroke(shutter, (shutter.w / 2.0) as f32, white, 3.0);
        let disc = shutter.expand(-7.0);
        canvas.fill(
            disc,
            (disc.w / 2.0) as f32,
            if capture.video {
                Color::rgb(0xe5_3935)
            } else {
                white
            },
        );

        let delay = layout.capture_delay();
        if capture.delay > 0 {
            canvas.fill(delay, 17.0, Color::WHITE.with_alpha(40));
            let text = format!("{} s", capture.delay);
            let width = canvas.text_width(&self.bold, &text, px(12.0));
            canvas.label(
                &self.bold,
                &text,
                delay.x + (delay.w - width) / 2.0,
                delay.y + 9.0,
                px(12.0),
                white,
            );
        } else {
            capture_icon_timer(canvas, delay.center(), white);
        }

        let pointer = layout.capture_pointer();
        if capture.cursor {
            canvas.fill(pointer, 17.0, Color::WHITE.with_alpha(40));
        }
        capture_icon_pointer(canvas, pointer.center(), if capture.cursor { white } else { dim });

        let close = layout.capture_close();
        canvas.fill(close, (close.w / 2.0) as f32, Color::rgb(0x3a3a3c));
        let (cx, cy) = close.center();
        canvas.line((cx - 5.0, cy - 5.0), (cx + 5.0, cy + 5.0), 2.0, white);
        canvas.line((cx - 5.0, cy + 5.0), (cx + 5.0, cy - 5.0), 2.0, white);
    }

    /// Case d'une application : grande icône, nom sur deux lignes au plus, centré.
    fn draw_tile(&self, canvas: &mut Canvas, tile: &Tile, rect: Rect) {
        let center = (rect.x + rect.w / 2.0, rect.y + 42.0);
        let drawn = match &tile.icon {
            TileIcon::Builtin(name) => {
                canvas.icon(name, center, 72.0, Style::Color);
                true
            }
            TileIcon::File(path) => canvas.icon_file(path, center, 72.0),
            TileIcon::Missing => false,
        };
        if !drawn {
            canvas.icon("file-generic", center, 72.0, Style::Color);
        }
        let lines = wrap_two_lines(canvas, &self.regular, &tile.name, px(12.0), rect.w - 8.0);
        for (index, line) in lines.iter().enumerate() {
            let width = canvas.text_width(&self.regular, line, px(12.0));
            canvas.label(
                &self.regular,
                line,
                rect.x + (rect.w - width) / 2.0,
                rect.y + 82.0 + index as f64 * 15.0,
                px(12.0),
                primary_text(),
            );
        }
    }

    /// Fond du lanceur dans le rectangle arrondi `card`, gardé pour recouvrir ce qui déborde
    /// au défilement, tant que la place, l'échelle et le thème ne changent pas.
    fn frost(&self, canvas: &Canvas, card: Rect) -> Arc<Frost> {
        let s = canvas.scale as f64;
        let left = (card.x * s).floor().max(0.0) as u32;
        let top = (card.y * s).floor().max(0.0) as u32;
        let right = ((card.right() * s).ceil() as u32).min(canvas.width);
        let bottom = ((card.bottom() * s).ceil() as u32).min(canvas.height);
        let fill = panel_fill();
        let key = (
            [left, top, right, bottom],
            canvas.scale.to_bits(),
            Some(u32::from(fill.r) as usize),
        );
        let mut cache = self.frost.borrow_mut();
        if let Some((known, frost)) = cache.as_ref()
            && *known == key
        {
            return frost.clone();
        }
        let (width, height) = (right.saturating_sub(left), bottom.saturating_sub(top));
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let radius = LAUNCHER_RADIUS * canvas.scale;
        let (x, y, w, h) = (
            card.x as f32 * canvas.scale,
            card.y as f32 * canvas.scale,
            card.w as f32 * canvas.scale,
            card.h as f32 * canvas.scale,
        );
        let alpha = f32::from(fill.a) / 255.0;
        for py in 0..height {
            for px in 0..width {
                let (sx, sy) = ((left + px) as f32 + 0.5, (top + py) as f32 + 0.5);
                let qx = (sx - (x + w * 0.5)).abs() - (w * 0.5 - radius);
                let qy = (sy - (y + h * 0.5)).abs() - (h * 0.5 - radius);
                let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                let coverage = (0.5 - distance).clamp(0.0, 1.0) * alpha;
                if coverage == 0.0 {
                    continue;
                }
                let offset = (py as usize * width as usize + px as usize) * 4;
                let channel = |value: u8| (f32::from(value) * coverage).round() as u8;
                pixels[offset..offset + 4].copy_from_slice(&[
                    channel(fill.b),
                    channel(fill.g),
                    channel(fill.r),
                    (255.0 * coverage).round() as u8,
                ]);
            }
        }
        let frost = Arc::new(Frost {
            left,
            top,
            width,
            height,
            pixels,
        });
        *cache = Some((key, frost.clone()));
        frost
    }

    /// Réglages rapides, à la manière du Centre de contrôle de macOS : modules clairs
    /// posés sur un panneau gris, interrupteurs ronds et curseurs épais.
    /// Centre de contrôle : six tuiles, curseurs du son et de la luminosité, sortie du son,
    /// batterie et boutons de session.
    fn draw_controls(&self, canvas: &mut Canvas, layout: &Layout, state: &VisualState) {
        let view = &state.view;
        canvas.glass(layout.controls(), 16.0);
        match view.controls.page {
            ControlsPage::Main => self.draw_controls_main(canvas, layout, state),
            ControlsPage::Wifi => self.draw_controls_wifi(canvas, layout, view),
            ControlsPage::Bluetooth => self.draw_controls_bluetooth(canvas, layout, view),
        }
    }

    fn draw_controls_main(&self, canvas: &mut Canvas, layout: &Layout, state: &VisualState) {
        let view = &state.view;
        let on_off = |on: bool| if on { "Activé" } else { "Désactivé" };
        let plural = if view.controls.connected_devices > 1 {
            "s"
        } else {
            ""
        };
        let devices = format!("{} appareil{plural}", view.controls.connected_devices);
        let wifi_state = match (view.wifi, &view.controls.current_network) {
            (None, _) => "Aucun adaptateur",
            (Some(true), Some(name)) => name.as_str(),
            (Some(on), _) => on_off(on),
        };
        let bluetooth_state = match view.bluetooth {
            None => "Aucun adaptateur",
            Some(true) if view.controls.connected_devices > 0 => devices.as_str(),
            Some(on) => on_off(on),
        };
        let dark = if view.dark_theme { "Activé" } else { "Désactivé" };
        let dnd = on_off(view.notifications.do_not_disturb);
        let saver = match view.power_saver {
            None => "Indisponible",
            Some(true) => "Activée",
            Some(false) => "Désactivée",
        };
        let airplane = match view.airplane {
            None => "Aucune radio",
            Some(on) => on_off(on),
        };
        let keyboard_light = match view.keyboard_light {
            None => "Absent",
            Some((0, _)) => "Éteint",
            Some((level, max)) if level >= max => "Fort",
            Some(_) => "Faible",
        };
        let tiles: [(Control, &str, &str, &str, bool); 9] = [
            (
                Control::Night,
                "cc-night",
                "Mode nuit",
                view.night.1.as_str(),
                view.night.0,
            ),
            (
                Control::Wifi,
                "cc-wifi",
                "Wi-Fi",
                wifi_state,
                view.wifi == Some(true),
            ),
            (
                Control::Bluetooth,
                "cc-bluetooth",
                "Bluetooth",
                bluetooth_state,
                view.bluetooth == Some(true),
            ),
            (Control::Dark, "cc-dark", "Mode sombre", dark, view.dark_theme),
            (
                Control::DoNotDisturb,
                "cc-dnd",
                "Ne pas déranger",
                dnd,
                view.notifications.do_not_disturb,
            ),
            (
                Control::Saver,
                "cc-saver",
                "Économie d’énergie",
                saver,
                view.power_saver == Some(true),
            ),
            (Control::Keyboard, "cc-keyboard", "Clavier", &view.keyboard, false),
            (
                Control::Airplane,
                "cc-airplane",
                "Mode avion",
                airplane,
                view.airplane == Some(true),
            ),
            (
                Control::KeyboardLight,
                "cc-keyboard-light",
                "Rétroéclairage",
                keyboard_light,
                view.keyboard_light.is_some_and(|(level, _)| level > 0),
            ),
        ];
        for (control, glyph, name, status, on) in tiles {
            let tile = layout.control_row(control);
            let (fill, disc, ink, title, sub) = if on {
                let white = Color::WHITE;
                (accent(), white.with_alpha(56), white, white, Color::rgb(0xE4EEFB))
            } else {
                (
                    control_fill(),
                    palette().window_background,
                    palette().heading,
                    primary_text(),
                    text_secondary(),
                )
            };
            canvas.fill(tile, 16.0, fill);
            let center = (tile.x + 29.0, tile.y + tile.h / 2.0);
            canvas.fill(
                Rect::new(center.0 - 17.0, center.1 - 17.0, 34.0, 34.0),
                17.0,
                disc,
            );
            canvas.icon(glyph, center, 18.0, Style::Symbolic(ink));
            // Wi-Fi et Bluetooth : la tuile active, le chevron ouvre la liste.
            let chevron = match control {
                Control::Wifi => Some(layout.control_row(Control::WifiMore)),
                Control::Bluetooth => Some(layout.control_row(Control::BluetoothMore)),
                Control::Night => Some(layout.control_row(Control::NightMore)),
                _ => None,
            };
            let mut room = tile.w - 56.0 - 8.0;
            if let Some(more) = chevron {
                room -= more.w;
                let line = if on { Color::WHITE.with_alpha(51) } else { tint(26) };
                canvas.fill(Rect::new(more.x, tile.y, 1.0, tile.h), 0.0, line);
                canvas.icon("cc-chevron", more.center(), 14.0, Style::Symbolic(ink));
            }
            let name = canvas.fit_text(&self.bold, name, px(13.0), room);
            canvas.label(&self.bold, &name, tile.x + 56.0, tile.y + 14.0, px(13.0), title);
            let status = canvas.fit_text(&self.regular, status, px(12.0), room);
            canvas.label(
                &self.regular,
                &status,
                tile.x + 56.0,
                tile.y + 33.0,
                px(12.0),
                sub,
            );
        }

        let sliders = [
            (Control::Volume, "cc-volume", view.volume),
            (Control::Brightness, "cc-brightness", view.brightness),
        ];
        for (control, glyph, value) in sliders {
            // Pas de curseur pour un réglage que la machine n'offre pas.
            let Some(value) = value else { continue };
            let row = layout.control_row(control);
            let ink = palette().heading;
            canvas.icon(
                glyph,
                (row.x + 10.0, row.y + row.h / 2.0),
                20.0,
                Style::Symbolic(ink),
            );
            canvas.slider(layout.slider_track(control), value);
            let text = format!("{value} %");
            let width = canvas.text_width(&self.regular, &text, px(12.0));
            canvas.label(
                &self.regular,
                &text,
                row.right() - width,
                row.y + 11.0,
                px(12.0),
                text_secondary(),
            );
        }

        let footer = layout.controls_footer();
        canvas.fill(
            Rect::new(footer.x, footer.y, footer.w, 1.0),
            0.0,
            palette().separator,
        );
        let battery = state.battery.map_or(String::new(), |battery| {
            let status = if battery.charging { " · en charge" } else { "" };
            format!("{} %{status}", battery.percent)
        });
        canvas.label(
            &self.regular,
            &battery,
            footer.x,
            footer.y + 23.0,
            px(13.0),
            palette().heading,
        );
        // Boutons ronds, de gauche à droite : compte, Paramètres, verrouiller, éteindre.
        let round = [
            (Control::Account, "cc-account", control_fill()),
            (Control::Settings, "glyph-gear", control_fill()),
            (Control::Lock, "cc-lock", control_fill()),
            (Control::Power, "cc-power", destructive()),
        ];
        for (control, glyph, fill) in round {
            let button = layout.control_row(control);
            canvas.fill(button, button.h as f32 / 2.0, fill);
            let ink = if control == Control::Power {
                Color::WHITE
            } else {
                palette().heading
            };
            canvas.icon(glyph, button.center(), 16.0, Style::Symbolic(ink));
        }

        if let Some(error) = &view.error {
            let line = layout.controls_error();
            canvas.label(
                &self.regular,
                &ellipsize(error, 52),
                line.x,
                line.y,
                px(12.0),
                destructive(),
            );
        }
    }

    /// Haut d'une liste : retour, titre et interrupteur.
    fn draw_controls_header(&self, canvas: &mut Canvas, layout: &Layout, title: &str, on: bool) {
        let back = layout.controls_back();
        canvas.fill(back, 17.0, control_fill());
        canvas.icon("cc-back", back.center(), 16.0, Style::Symbolic(primary_text()));
        canvas.label(
            &self.bold,
            title,
            back.right() + 12.0,
            back.y + 6.0,
            px(18.0),
            primary_text(),
        );
        let switch = layout.controls_switch();
        let track = if on { accent() } else { palette().track };
        canvas.fill(switch, (switch.h / 2.0) as f32, track);
        let knob = switch.h - 4.0;
        let x = if on {
            switch.right() - knob - 2.0
        } else {
            switch.x + 2.0
        };
        canvas.fill(
            Rect::new(x, switch.y + 2.0, knob, knob),
            (knob / 2.0) as f32,
            Color::WHITE,
        );
    }

    /// Bas d'une liste : état à gauche, lien vers Paramètres à droite.
    fn draw_controls_bottom(
        &self,
        canvas: &mut Canvas,
        layout: &Layout,
        status: &str,
        link: &str,
        spin: bool,
    ) {
        let bottom = layout.controls_bottom();
        let mut x = bottom.x;
        if spin {
            canvas.stroke(Rect::new(x, bottom.y + 9.0, 14.0, 14.0), 7.0, accent(), 2.0);
            x += 22.0;
        }
        canvas.label(
            &self.regular,
            status,
            x,
            bottom.y + 7.0,
            px(13.0),
            text_secondary(),
        );
        let width = canvas.text_width(&self.bold, link, px(13.0));
        canvas.label(
            &self.bold,
            link,
            bottom.right() - width,
            bottom.y + 7.0,
            px(13.0),
            accent(),
        );
    }

    fn draw_controls_wifi(&self, canvas: &mut Canvas, layout: &Layout, view: &DesktopView) {
        let controls = &view.controls;
        self.draw_controls_header(canvas, layout, "Wi-Fi", view.wifi == Some(true));
        let list = layout.wifi_list();
        canvas.fill(list, 14.0, palette().window_background);
        if controls.networks.is_empty() {
            let text = match view.wifi {
                Some(true) => "Analyse en cours…",
                Some(false) => "Wi-Fi désactivé",
                None => "Aucune carte Wi-Fi détectée",
            };
            canvas.label(
                &self.regular,
                text,
                list.x + 16.0,
                list.y + 16.0,
                px(13.0),
                text_secondary(),
            );
        }
        for (slot, network) in controls.networks.iter().enumerate() {
            let row = layout.wifi_row(slot);
            if slot > 0 {
                canvas.fill(Rect::new(row.x, row.y, row.w, 1.0), 0.0, palette().separator);
            }
            let center = row.y + CONTROLS_LIST_ROW / 2.0;
            for (index, height) in [4.0, 7.0, 10.0, 13.0].into_iter().enumerate() {
                let bar = Rect::new(
                    row.x + 16.0 + index as f64 * 6.0,
                    center + 7.0 - height,
                    4.0,
                    height,
                );
                let color = if (index as u8) < network.level {
                    primary_text()
                } else {
                    palette().track
                };
                canvas.fill(bar, 1.0, color);
            }
            let x = row.x + 48.0;
            let name = canvas.fit_text(&self.bold, &network.ssid, px(14.0), row.w - 48.0 - 64.0);
            canvas.label(&self.bold, &name, x, row.y + 8.0, px(14.0), primary_text());
            let hint = if network.connected {
                palette().success_text
            } else {
                text_secondary()
            };
            canvas.label(&self.regular, &network.hint, x, row.y + 28.0, px(12.0), hint);
            let mut right = row.right() - 26.0;
            if network.connected {
                canvas.icon("cc-check", (right, center), 16.0, Style::Symbolic(accent()));
                right -= 28.0;
            }
            if network.secured {
                canvas.icon(
                    "cc-lock",
                    (right, center),
                    14.0,
                    Style::Symbolic(text_secondary()),
                );
            }
            if network.known && network.secured {
                let eye = layout.wifi_eye(slot, network.connected);
                let (name, ink) = if network.revealed {
                    ("cc-eye-off", accent())
                } else {
                    ("cc-eye", text_secondary())
                };
                canvas.icon(
                    name,
                    (eye.x + eye.w / 2.0, eye.y + eye.h / 2.0),
                    16.0,
                    Style::Symbolic(ink),
                );
            }
            if controls
                .password
                .as_ref()
                .is_some_and(|entry| entry.ssid == network.ssid)
            {
                self.draw_password(canvas, layout, controls);
            }
        }
        self.draw_controls_bottom(
            canvas,
            layout,
            "Analyse auto toutes les 10 s",
            "Paramètres Wi-Fi…",
            false,
        );
    }

    fn draw_password(&self, canvas: &mut Canvas, layout: &Layout, controls: &ControlsView) {
        let Some(entry) = &controls.password else {
            return;
        };
        let field = layout.password_field();
        let label = format!("Mot de passe de « {} »", entry.ssid);
        let label = canvas.fit_text(&self.regular, &label, px(12.0), field.w);
        canvas.label(
            &self.regular,
            &label,
            field.x,
            field.y - 20.0,
            px(12.0),
            text_secondary(),
        );
        canvas.fill(field, 10.0, palette().window_background);
        canvas.stroke(field, 10.0, accent(), 2.0);
        let eye = layout.password_eye();
        let (name, ink) = if entry.shown.is_some() {
            ("cc-eye-off", accent())
        } else {
            ("cc-eye", text_secondary())
        };
        canvas.icon(
            name,
            (eye.x + eye.w / 2.0, eye.y + eye.h / 2.0),
            18.0,
            Style::Symbolic(ink),
        );
        if let Some(text) = entry.shown.as_deref().filter(|text| !text.is_empty()) {
            let text = canvas.fit_text(&self.regular, text, px(14.0), field.w - 24.0 - eye.w);
            canvas.label(
                &self.regular,
                &text,
                field.x + 14.0,
                field.y + 11.0,
                px(14.0),
                primary_text(),
            );
        } else if entry.length == 0 {
            canvas.fill(
                Rect::new(field.x + 12.0, field.y + 10.0, 2.0, 20.0),
                1.0,
                accent(),
            );
            canvas.label(
                &self.regular,
                "Mot de passe",
                field.x + 18.0,
                field.y + 11.0,
                px(13.0),
                text_secondary(),
            );
        } else {
            let room = ((field.w - 28.0 - eye.w) / 9.0).max(1.0) as usize;
            for index in 0..entry.length.min(room) {
                let dot = Rect::new(field.x + 14.0 + index as f64 * 9.0, field.y + 17.0, 6.0, 6.0);
                canvas.fill(dot, 3.0, primary_text());
            }
        }
        canvas.button(
            layout.password_cancel(),
            "Annuler",
            control_fill(),
            primary_text(),
        );
        canvas.button(layout.password_submit(), "Se connecter", accent(), Color::WHITE);
    }

    fn draw_controls_bluetooth(&self, canvas: &mut Canvas, layout: &Layout, view: &DesktopView) {
        let controls = &view.controls;
        self.draw_controls_header(canvas, layout, "Bluetooth", view.bluetooth == Some(true));
        for (title, paired) in [("Mes appareils", true), ("Appareils à proximité", false)] {
            let Some((heading, list)) = layout.bluetooth_group(paired) else {
                continue;
            };
            canvas.label(
                &self.bold,
                title,
                heading.x,
                heading.y,
                px(13.0),
                text_secondary(),
            );
            canvas.fill(list, 14.0, palette().window_background);
        }
        for (slot, device) in controls.devices.iter().enumerate() {
            let row = layout.bluetooth_row(slot);
            let first = slot == 0 || controls.devices[slot - 1].paired != device.paired;
            if !first {
                canvas.fill(Rect::new(row.x, row.y, row.w, 1.0), 0.0, palette().separator);
            }
            let center = (row.x + 33.0, row.y + CONTROLS_LIST_ROW / 2.0);
            let (disc, ink) = if device.connected {
                (accent(), Color::WHITE)
            } else {
                (control_fill(), primary_text())
            };
            canvas.fill(
                Rect::new(center.0 - 17.0, center.1 - 17.0, 34.0, 34.0),
                17.0,
                disc,
            );
            canvas.icon(device.glyph, center, 18.0, Style::Symbolic(ink));
            let button = layout.bluetooth_button(slot);
            let name = canvas.fit_text(&self.bold, &device.name, px(14.0), button.x - row.x - 66.0);
            canvas.label(
                &self.bold,
                &name,
                row.x + 60.0,
                row.y + 8.0,
                px(14.0),
                primary_text(),
            );
            let hint = if device.connected {
                palette().success_text
            } else {
                text_secondary()
            };
            canvas.label(
                &self.regular,
                &device.hint,
                row.x + 60.0,
                row.y + 28.0,
                px(12.0),
                hint,
            );
            let (text, fill, text_ink) = match (device.connected, device.paired) {
                (true, _) => ("Déconnecter", control_fill(), primary_text()),
                (false, true) => ("Connecter", accent(), Color::WHITE),
                (false, false) => ("Associer", accent(), Color::WHITE),
            };
            let (fill, text_ink) = if device.busy {
                (control_fill(), text_secondary())
            } else {
                (fill, text_ink)
            };
            canvas.button(button, text, fill, text_ink);
        }
        let status = if view.bluetooth != Some(true) {
            "Bluetooth désactivé"
        } else if controls.scanning {
            "Recherche d’appareils…"
        } else {
            "Recherche terminée"
        };
        self.draw_controls_bottom(canvas, layout, status, "Paramètres Bluetooth…", controls.scanning);
    }

    fn draw_desktop_menu(
        &self,
        canvas: &mut Canvas,
        layout: &Layout,
        anchor: (i32, i32),
        entries: &[String],
        pointer: Option<(i32, i32)>,
    ) {
        canvas.glass(layout.desktop_menu(anchor, entries.len()), 10.0);
        for (index, entry) in entries.iter().enumerate() {
            self.menu_row(
                canvas,
                layout.desktop_menu_row(anchor, entries.len(), index),
                entry,
                pointer,
            );
        }
    }

    fn draw_dock_menu(
        &self,
        canvas: &mut Canvas,
        layout: &Layout,
        menu: &DockMenuView,
        pointer: Option<(i32, i32)>,
    ) {
        canvas.glass(layout.dock_menu(menu), 10.0);
        for (index, entry) in menu.entries.iter().enumerate() {
            self.menu_row(canvas, layout.dock_menu_row(menu, index), entry, pointer);
        }
    }

    /// Ligne de menu de 32 : surlignée sous le pointeur.
    fn menu_row(&self, canvas: &mut Canvas, row: Rect, text: &str, pointer: Option<(i32, i32)>) {
        let hovered = pointer.is_some_and(|(x, y)| row.contains(Point::from((f64::from(x), f64::from(y)))));
        if hovered {
            canvas.fill(row, 6.0, accent());
        }
        let ink = if hovered { Color::WHITE } else { primary_text() };
        canvas.label(&self.regular, text, row.x + 10.0, row.y + 8.0, px(13.0), ink);
    }

    /// Historique du presse-papiers (planche « Presse-papiers ») : en-tête avec recherche
    /// et filtres, épinglés puis récents, pied des raccourcis, bandeau « Annuler ».
    fn draw_clipboard(&self, canvas: &mut Canvas, layout: &Layout, view: &ClipboardView) {
        let t = clip_tokens();
        let card = layout.clipboard();
        canvas.panel_shadow(card, 18.0);
        canvas.fill(card, 18.0, t.panel);
        canvas.stroke(card, 18.0, t.ring, 1.0);

        // En-tête : titre et nombre, thème et « Tout effacer ».
        canvas.text(
            &self.bold,
            "Presse-papiers",
            card.x + 18.0,
            card.y + 24.0,
            17.0,
            t.fg,
        );
        let count = format!("{} élément{}", view.count, if view.count > 1 { "s" } else { "" });
        let title_w = canvas.css_width(&self.bold, "Presse-papiers", 17.0);
        canvas.text(
            &self.regular,
            &count,
            card.x + 18.0 + title_w + 8.0,
            card.y + 28.0,
            13.0,
            t.fg3,
        );
        let theme = layout.clipboard_theme();
        canvas.fill(theme, 9.0, t.chip);
        let glyph = themed("clip-moon", "clip-sun");
        canvas.icon(glyph, theme.center(), 15.0, Style::Symbolic(t.icon));
        let clear = layout.clipboard_clear();
        canvas.fill(clear, 9.0, t.chip);
        centered_text(canvas, &self.regular, "Tout effacer", clear, 13.0, t.fg2);

        // Recherche, toujours active : le clavier y écrit.
        let search = layout.clipboard_search();
        canvas.fill(search, 10.0, t.field);
        canvas.stroke(search, 10.0, t.field_ring, 1.0);
        canvas.icon(
            "clip-search",
            (search.x + 19.0, search.center().1),
            14.0,
            Style::Symbolic(t.fg3),
        );
        let text_x = search.x + 34.0;
        let text_top = search.center().1 - 8.5;
        if view.query.is_empty() {
            canvas.text(
                &self.regular,
                "Rechercher dans l’historique",
                text_x,
                text_top,
                14.0,
                t.fg3,
            );
        }
        let query = ellipsize_to(canvas, &self.regular, &view.query, px(14.0), search.w - 48.0);
        canvas.text(&self.regular, &query, text_x, text_top, 14.0, t.fg);
        let caret = text_x + canvas.css_width(&self.regular, &query, 14.0) + 1.0;
        canvas.fill(
            Rect::new(caret, search.center().1 - 9.0, 1.5, 18.0),
            0.75,
            accent(),
        );

        for (slot, filter) in ClipFilter::ALL.into_iter().enumerate() {
            let pill = layout.clipboard_filter(slot);
            let on = view.filter == filter;
            canvas.fill(pill, 14.0, if on { t.filter_on } else { t.chip });
            let (font, ink) = if on {
                (&self.bold, t.filter_on_fg)
            } else {
                (&self.regular, t.fg2)
            };
            centered_text(canvas, font, filter.label(), pill, 12.0, ink);
        }
        canvas.fill(
            Rect::new(card.x, card.y + CLIPBOARD_HEAD - 1.0, card.w, 1.0),
            0.0,
            t.line,
        );

        // Liste, coupée entre le filet de l'en-tête et le pied.
        let clip = layout.clipboard_clip();
        let above = canvas.save_rows(card.y - 30.0, clip.y);
        let below = canvas.save_rows(clip.bottom(), card.bottom() + clipboard::ROW_TALL + 30.0);
        let list = layout.clipboard_list();
        for (title, y) in &view.sections {
            canvas.tracked(
                &self.bold,
                &title.to_uppercase(),
                list.x + 8.0,
                list.y + y + 12.0,
                11.0,
                0.06,
                t.fg3,
            );
        }
        for row in &view.rows {
            self.clipboard_row(canvas, layout.clipboard_row(row), row, &t);
        }
        canvas.restore_rows(above);
        canvas.restore_rows(below);
        if let Some((title, hint)) = view.empty {
            let (cx, cy) = clip.center();
            let tile = Rect::new(cx - 28.0, cy - 64.0, 56.0, 56.0);
            canvas.fill(tile, 16.0, t.chip);
            canvas.icon("clip-empty", tile.center(), 26.0, Style::Symbolic(t.fg3));
            let line = Rect::new(clip.x + 30.0, tile.bottom() + 10.0, clip.w - 60.0, 22.0);
            centered_text(canvas, &self.bold, title, line, 15.0, t.fg);
            let lines = wrap_two_lines(canvas, &self.regular, hint, px(13.0), clip.w - 60.0);
            for (number, text) in lines.iter().enumerate() {
                let rect = Rect::new(line.x, line.bottom() + 6.0 + number as f64 * 19.5, line.w, 19.5);
                centered_text(canvas, &self.regular, text, rect, 13.0, t.fg2);
            }
        }

        // Pied : raccourcis.
        let foot = Rect::new(card.x, card.bottom() - CLIPBOARD_FOOT, card.w, CLIPBOARD_FOOT);
        canvas.fill(Rect::new(foot.x, foot.y, foot.w, 1.0), 0.0, t.line);
        let mut x = foot.x + 16.0;
        for (keys, what) in [("↑↓", "naviguer"), ("Entrée", "coller")] {
            x = self.kbd(canvas, keys, x, foot.center().1, &t) + 6.0;
            canvas.text(&self.regular, what, x, foot.center().1 - 7.5, 12.0, t.fg3);
            x += canvas.css_width(&self.regular, what, 12.0) + 14.0;
        }
        let shortcut = "Super + V";
        let width = canvas.css_width(&self.bold, shortcut, 11.0) + 12.0;
        self.kbd(canvas, shortcut, foot.right() - 16.0 - width, foot.center().1, &t);

        // Bandeau d'une suppression, avec « Annuler ».
        if let Some(text) = view.toast {
            let toast = layout.clipboard_toast();
            canvas.panel_shadow(toast, 12.0);
            canvas.fill(toast, 12.0, t.toast_bg);
            let fit = ellipsize_to(canvas, &self.regular, text, px(13.0), toast.w - 120.0);
            canvas.text(
                &self.regular,
                &fit,
                toast.x + 16.0,
                toast.center().1 - 8.0,
                13.0,
                t.toast_fg,
            );
            let undo = layout.clipboard_undo();
            canvas.fill(undo, 8.0, Color::WHITE.with_alpha(36));
            centered_text(canvas, &self.bold, "Annuler", undo, 13.0, t.toast_action);
        }
    }

    /// Une ligne : tuile du type, texte et origine, chiffre, épingle et croix.
    fn clipboard_row(&self, canvas: &mut Canvas, rect: Rect, row: &ClipRow, t: &ClipTokens) {
        if row.selected {
            canvas.fill(rect, 12.0, accent().with_alpha(themed(26, 36)));
            canvas.stroke(rect, 12.0, accent(), 1.5);
        }
        let cy = rect.center().1;
        let tile = Rect::new(rect.x + 10.0, cy - 22.0, 44.0, 44.0);
        match (&row.thumbnail, row.kind) {
            (Some(thumbnail), _) => canvas.image_cover(tile, thumbnail, 10.0),
            (None, ClipKind::Files) => {
                canvas.fill(tile, 10.0, t.tile);
                canvas.icon(row.file_icon, tile.center(), 34.0, Style::Color);
            }
            (None, ClipKind::Color) => {
                canvas.fill(tile, 10.0, t.tile);
                let swatch = Rect::new(tile.x + 8.0, tile.y + 8.0, 28.0, 28.0);
                canvas.fill(swatch, 8.0, row.swatch.unwrap_or(t.fg3));
                canvas.stroke(swatch, 8.0, Color::BLACK.with_alpha(31), 1.0);
            }
            (None, kind) => {
                canvas.fill(tile, 10.0, t.tile);
                let glyph = match kind {
                    ClipKind::Code => "clip-code",
                    ClipKind::Link => "clip-link",
                    _ => "clip-text",
                };
                canvas.icon(glyph, tile.center(), 20.0, Style::Symbolic(t.icon));
            }
        }
        let delete = clipboard_delete(rect);
        canvas.icon("clip-close", delete.center(), 12.0, Style::Symbolic(t.fg3));
        let pin = clipboard_pin(rect);
        let (glyph, ink) = if row.pinned {
            ("clip-pin-on", accent())
        } else {
            ("clip-pin", t.fg3)
        };
        canvas.icon(glyph, pin.center(), 16.0, Style::Symbolic(ink));
        let mut right = pin.x;
        if let Some(key) = row.key {
            let kbd = Rect::new(pin.x - 4.0 - 22.0, cy - 11.0, 22.0, 22.0);
            canvas.stroke(kbd, 6.0, t.kbd_ring, 1.0);
            centered_text(canvas, &self.bold, &key.to_string(), kbd, 11.0, t.fg3);
            right = kbd.x;
        }
        let x = tile.right() + 12.0;
        let width = right - 12.0 - x;
        let (font, size) = match row.kind {
            ClipKind::Code | ClipKind::Color | ClipKind::Files => (&self.mono, 13.0),
            _ => (&self.regular, 14.0),
        };
        let lines = if row.h > clipboard::ROW {
            wrap_two_lines(canvas, font, &row.label, px(size as f32), width)
        } else {
            vec![ellipsize_to(canvas, font, &row.label, px(size as f32), width)]
        };
        let line_h = size * 1.4;
        let block = lines.len() as f64 * line_h + 3.0 + 15.0;
        let top = cy - block / 2.0;
        for (number, line) in lines.iter().enumerate() {
            let y = top + number as f64 * line_h + (line_h - size * 1.21) / 2.0;
            canvas.text(font, line, x, y, size, t.fg);
        }
        let sub = ellipsize_to(canvas, &self.regular, &row.sub, px(12.0), width);
        canvas.text(
            &self.regular,
            &sub,
            x,
            top + lines.len() as f64 * line_h + 3.0,
            12.0,
            t.fg3,
        );
    }

    /// Touche dessinée en `x`, centrée sur `cy` ; rend son bord droit.
    fn kbd(&self, canvas: &mut Canvas, keys: &str, x: f64, cy: f64, t: &ClipTokens) -> f64 {
        let width = canvas.css_width(&self.bold, keys, 11.0) + 12.0;
        let rect = Rect::new(x, cy - 10.0, width, 20.0);
        canvas.stroke(rect, 5.0, t.kbd_ring, 1.0);
        centered_text(canvas, &self.bold, keys, rect, 11.0, t.fg3);
        rect.right()
    }

    /// Sur chaque aperçu : l'icône de l'application à cheval sur le bas ; au survol, un
    /// contour, le titre et le bouton de fermeture.
    fn draw_overview(&self, canvas: &mut Canvas, layout: &Layout, overview: &OverviewView) {
        const ICON: f64 = 56.0;
        // Le fond suit le thème (voir `RenderAssets::overview_overlay`) : écritures et
        // miniatures aussi.
        let text = themed(Color::rgb(0x1C1C1E), Color::WHITE);
        let soft = themed(Color::rgb(0x4A5563), Color::rgb(0xC9D4E3));
        let thumb_fill = themed(Color::rgb(0xCDD5DF), Color::rgb(0x16202D));
        let ring = themed(Color::rgb(0x2F7FDB), Color::rgb(0x6CB8FF));
        // Bande des espaces de travail ; en mode dynamique, le dernier, vide, est le
        // « nouvel espace ».
        for (index, space) in overview.spaces.iter().enumerate() {
            let thumb = overview_space_rect(layout.w, layout.bar, overview.spaces.len(), index);
            if !overview.fixed && space.windows == 0 && index + 1 == overview.spaces.len() {
                canvas.stroke(thumb, 8.0, tint(90), 1.5);
                let center = thumb.center();
                canvas.line((center.0 - 6.0, center.1), (center.0 + 6.0, center.1), 2.0, soft);
                canvas.line((center.0, center.1 - 6.0), (center.0, center.1 + 6.0), 2.0, soft);
                continue;
            }
            canvas.fill(thumb, 8.0, thumb_fill);
            if space.current {
                canvas.stroke(thumb.expand(2.0), 10.0, ring, 2.0);
            }
            let shown = space.windows.min(3);
            for slot in 0..shown {
                let w = (thumb.w - 8.0 * (shown as f64 + 1.0)) / shown as f64;
                let window = Rect::new(
                    thumb.x + 8.0 + slot as f64 * (w + 8.0),
                    thumb.y + 9.0,
                    w,
                    thumb.h - 18.0,
                );
                canvas.fill(window, 3.0, Color::WHITE.with_alpha(230));
            }
            let name = format!("Espace {}", space.page + 1);
            let width = canvas.text_width(&self.regular, &name, px(11.0));
            canvas.label(
                &self.regular,
                &name,
                thumb.x + (thumb.w - width) / 2.0,
                thumb.bottom() + 4.0,
                px(11.0),
                soft,
            );
        }
        if overview.tiles.is_empty() {
            let message = "Aucune fenêtre ouverte sur ce bureau";
            let width = canvas.text_width(&self.regular, message, 15.0);
            canvas.label(
                &self.regular,
                message,
                (layout.w - width) / 2.0,
                layout.h / 2.0 - 10.0,
                15.0,
                text,
            );
            return;
        }
        for tile in &overview.tiles {
            let (x, y, w, h) = tile.frame;
            let frame = Rect::new(x as f64, y as f64, w as f64, h as f64);
            let center = (frame.x + frame.w / 2.0, frame.bottom() - ICON / 6.0);
            if tile.hovered {
                canvas.stroke(frame.expand(3.0), 11.0, ring, 3.0);
            }
            canvas.app_icon(&tile.icon, center, ICON);
            if !tile.title.is_empty() {
                let title = canvas.fit_text(&self.bold, &tile.title, px(12.0), frame.w.max(120.0));
                let width = canvas.text_width(&self.bold, &title, px(12.0));
                canvas.label(
                    &self.bold,
                    &title,
                    center.0 - width / 2.0,
                    center.1 + ICON / 2.0 + 8.0,
                    px(12.0),
                    text,
                );
            }
            if !tile.hovered {
                continue;
            }
            let close = crate::wm::workspaces::overview_close_button(Rectangle::new(
                (frame.x, frame.y).into(),
                (frame.w, frame.h).into(),
            ));
            let close = Rect::new(close.loc.x, close.loc.y, close.size.w, close.size.h);
            let disc = if tile.close_hovered {
                destructive()
            } else {
                Color::rgb(0x2C2C2E).with_alpha(235)
            };
            canvas.fill(close, (close.w / 2.0) as f32, disc);
            let (cx, cy, arm) = (close.x + close.w / 2.0, close.y + close.h / 2.0, 5.0);
            canvas.line((cx - arm, cy - arm), (cx + arm, cy + arm), 1.8, Color::WHITE);
            canvas.line((cx - arm, cy + arm), (cx + arm, cy - arm), 1.8, Color::WHITE);
        }
    }

    /// Contenu d'une notification dans `rect` : icône, application, titre, deux lignes, croix.
    fn draw_notification(&self, canvas: &mut Canvas, rect: Rect, notification: &NotificationView) {
        let center = (rect.x + 14.0 + 16.0, rect.y + 14.0 + 16.0);
        match &notification.icon {
            Some(icon) => canvas.app_icon(icon, center, 32.0),
            None => canvas.icon(
                "status-notifications",
                center,
                32.0,
                Style::Symbolic(primary_text()),
            ),
        }
        let x = rect.x + 58.0;
        let right = rect.right() - 14.0;
        let when = &notification.when;
        let when_width = canvas.text_width(&self.regular, when, px(11.0));
        canvas.label(
            &self.regular,
            when,
            right - when_width,
            rect.y + 14.0,
            px(11.0),
            text_secondary(),
        );
        let title = if notification.summary.is_empty() {
            &notification.app
        } else {
            &notification.summary
        };
        let title = canvas.fit_text(&self.bold, title, px(13.0), right - when_width - 8.0 - x);
        canvas.label(&self.bold, &title, x, rect.y + 13.0, px(13.0), primary_text());
        let ink = palette().heading;
        for (index, line) in wrap_two_lines(canvas, &self.regular, &notification.body, px(13.0), right - x)
            .iter()
            .enumerate()
        {
            canvas.label(
                &self.regular,
                line,
                x,
                rect.y + 33.0 + index as f64 * 17.0,
                px(13.0),
                ink,
            );
        }
    }

    /// Historique des notifications, avec « Ne pas déranger » et « Tout effacer ».
    /// Calendrier du mois (points sous les jours occupés du Calendrier, `busy`), puis
    /// historique des notifications.
    fn draw_notification_center(
        &self,
        canvas: &mut Canvas,
        layout: &Layout,
        view: &NotificationsView,
        busy: u32,
    ) {
        let card = layout.calendar(view.history.len());
        canvas.glass(card, 16.0);
        let clock = clock_snapshot(SystemTime::now());
        const MONTHS: [&str; 12] = [
            "Janvier",
            "Février",
            "Mars",
            "Avril",
            "Mai",
            "Juin",
            "Juillet",
            "Août",
            "Septembre",
            "Octobre",
            "Novembre",
            "Décembre",
        ];
        const DAYS: [&str; 7] = [
            "Dimanche", "Lundi", "Mardi", "Mercredi", "Jeudi", "Vendredi", "Samedi",
        ];
        let title = format!("{} {}", MONTHS[clock.month as usize - 1], clock.year);
        canvas.label(
            &self.bold,
            &title,
            card.x + 18.0,
            card.y + 16.0,
            px(18.0),
            primary_text(),
        );
        let today = format!("{} {}", DAYS[clock.weekday], clock.day);
        let width = canvas.text_width(&self.regular, &today, px(13.0));
        canvas.label(
            &self.regular,
            &today,
            card.right() - 18.0 - width,
            card.y + 20.0,
            px(13.0),
            text_secondary(),
        );
        let cell = (card.w - 36.0) / 7.0;
        let grid = card.y + 52.0;
        for (index, name) in ["L", "M", "M", "J", "V", "S", "D"].into_iter().enumerate() {
            let width = canvas.text_width(&self.bold, name, px(11.0));
            canvas.label(
                &self.bold,
                name,
                card.x + 18.0 + cell * (index as f64 + 0.5) - width / 2.0,
                grid,
                px(11.0),
                text_secondary(),
            );
        }
        for day in 1..=days_in_month(clock.year, clock.month) {
            let center = calendar_cell(card, clock.year, clock.month, day).center();
            let text = day.to_string();
            let current = day == clock.day;
            if current {
                canvas.fill(
                    Rect::new(center.0 - 16.0, center.1 - 16.0, 32.0, 32.0),
                    16.0,
                    accent(),
                );
            }
            let (font, ink) = if current {
                (&self.bold, Color::WHITE)
            } else {
                (&self.regular, primary_text())
            };
            let width = canvas.text_width(font, &text, px(13.0));
            canvas.label(font, &text, center.0 - width / 2.0, center.1 - 9.0, px(13.0), ink);
            if busy & (1 << (day - 1)) != 0 {
                let dot = if current { Color::WHITE } else { accent() };
                canvas.fill(Rect::new(center.0 - 2.0, center.1 + 9.0, 4.0, 4.0), 2.0, dot);
            }
        }

        let rule = layout.calendar_rule(view.history.len());
        canvas.fill(Rect::new(rule.x, rule.y, rule.w, 1.0), 0.0, palette().separator);
        canvas.label(
            &self.bold,
            "Notifications",
            rule.x,
            rule.y + 20.0,
            px(13.0),
            text_secondary(),
        );
        if view.history.is_empty() {
            canvas.label(
                &self.regular,
                "Aucune notification",
                rule.x,
                rule.y + 50.0,
                px(13.0),
                text_secondary(),
            );
            return;
        }
        canvas.button(
            layout.notifications_clear(view.history.len()),
            "Tout effacer",
            control_fill(),
            primary_text(),
        );
        for (slot, notification) in view.history.iter().take(layout.notification_rows()).enumerate() {
            let row = layout.notification_row(view.history.len(), slot);
            canvas.fill(row, 12.0, palette().window_background);
            canvas.stroke(row, 12.0, palette().separator, 1.0);
            self.draw_notification(canvas, row, notification);
        }
    }

    /// Sélecteur Alt+Tab, au centre de l'écran : les icônes et le titre de la fenêtre choisie.
    fn draw_switcher(&self, canvas: &mut Canvas, layout: &Layout, switcher: &SwitcherView) {
        let (card, tile) = switcher_card(layout.w, layout.h, switcher.icons.len());
        canvas.fill(card, 20.0, Color::rgb(0x16181C).with_alpha(224));
        for (index, icon) in switcher.icons.iter().enumerate() {
            let cell = Rect::new(
                card.x + SWITCHER_PADDING + index as f64 * (tile + SWITCHER_GAP),
                card.y + SWITCHER_PADDING,
                tile,
                tile,
            );
            if index == switcher.selected {
                canvas.fill(cell, 14.0, Color::WHITE.with_alpha(46));
            }
            canvas.app_icon(icon, cell.center(), tile * 0.8);
        }
        let text = canvas.fit_text(
            &self.bold,
            &switcher.title,
            px(13.0),
            card.w - 2.0 * SWITCHER_PADDING,
        );
        let width = canvas.text_width(&self.bold, &text, px(13.0));
        canvas.label(
            &self.bold,
            &text,
            card.x + (card.w - width) / 2.0,
            card.y + SWITCHER_PADDING + tile + 10.0,
            px(13.0),
            Color::WHITE,
        );
    }

    /// Voile sur l'écran, sauf sur la zone choisie, pendant la sélection d'une capture.
    /// Bulle d'une touche de fonction : centrée, au-dessus du Dock.
    fn draw_osd(&self, canvas: &mut Canvas, layout: &Layout, osd: Osd, dock_height: i32) {
        let (w, h) = (300.0, 60.0);
        let bubble = Rect::new(
            ((layout.w - w) / 2.0).round(),
            (layout.h - f64::from(dock_height.max(0)) - 32.0 - h).round(),
            w,
            h,
        );
        canvas.glass(bubble, 16.0);
        canvas.icon(
            osd.icon,
            (bubble.x + 34.0, bubble.y + h / 2.0),
            24.0,
            Style::Symbolic(primary_text()),
        );
        let text_x = bubble.x + 62.0;
        let Some(level) = osd.level else {
            canvas.label(
                &self.bold,
                osd.label,
                text_x,
                bubble.y + 21.0,
                px(14.0),
                primary_text(),
            );
            return;
        };
        canvas.label(
            &self.bold,
            osd.label,
            text_x,
            bubble.y + 11.0,
            px(13.0),
            primary_text(),
        );
        let track = Rect::new(text_x, bubble.y + 37.0, bubble.right() - 22.0 - text_x, 6.0);
        canvas.fill(track, 3.0, control_fill());
        let filled = Rect::new(
            track.x,
            track.y,
            track.w * f64::from(level.min(100)) / 100.0,
            track.h,
        );
        if filled.w > 0.0 {
            canvas.fill(filled, 3.0, accent());
        }
    }

    fn draw_region(&self, canvas: &mut Canvas, layout: &Layout, region: RegionView) {
        let veil = Color::BLACK.with_alpha(90);
        let Some((x, y, w, h)) = region.rect else {
            canvas.fill(Rect::new(0.0, 0.0, layout.w, layout.h), 0.0, veil);
            let text = "Glissez pour choisir la zone à capturer — Échap pour annuler";
            let width = canvas.text_width(&self.bold, text, 13.0);
            let pill = Rect::new(
                (layout.w - width) / 2.0 - 16.0,
                layout.bar + 20.0,
                width + 32.0,
                34.0,
            );
            canvas.fill(pill, 17.0, Color::BLACK.with_alpha(205));
            canvas.label(&self.bold, text, pill.x + 16.0, pill.y + 9.0, 13.0, Color::WHITE);
            return;
        };
        let zone = Rect::new(x as f64, y as f64, w as f64, h as f64);
        canvas.fill(Rect::new(0.0, 0.0, layout.w, zone.y), 0.0, veil);
        canvas.fill(
            Rect::new(0.0, zone.bottom(), layout.w, layout.h - zone.bottom()),
            0.0,
            veil,
        );
        canvas.fill(Rect::new(0.0, zone.y, zone.x, zone.h), 0.0, veil);
        canvas.fill(
            Rect::new(zone.right(), zone.y, layout.w - zone.right(), zone.h),
            0.0,
            veil,
        );
        canvas.stroke(zone, 0.0, Color::WHITE, 1.5);
        let text = format!("{w} × {h}");
        let width = canvas.text_width(&self.bold, &text, 12.0);
        let pill = Rect::new(
            zone.x,
            (zone.bottom() + 8.0).min(layout.h - 34.0),
            width + 20.0,
            26.0,
        );
        canvas.fill(pill, 13.0, Color::BLACK.with_alpha(205));
        canvas.label(&self.bold, &text, pill.x + 10.0, pill.y + 5.0, 12.0, Color::WHITE);
    }

    /// Fenêtre choisie pour la capture : le reste de l'écran est voilé.
    fn draw_window_select(&self, canvas: &mut Canvas, layout: &Layout, frame: (i32, i32, i32, i32)) {
        let (x, y, w, h) = frame;
        let zone = Rect::new(x as f64, y as f64, w as f64, h as f64);
        veil_around(canvas, layout, zone);
        canvas.stroke(zone.expand(1.0), 10.0, Color::WHITE, 2.0);
    }

    /// Zone sélectionnée, ajustable : voile autour, contour et poignées rondes aux coins.
    fn draw_region_adjust(&self, canvas: &mut Canvas, layout: &Layout, rect: (i32, i32, i32, i32)) {
        let (x, y, w, h) = rect;
        let zone = Rect::new(x as f64, y as f64, w as f64, h as f64);
        veil_around(canvas, layout, zone);
        canvas.stroke(zone, 0.0, Color::WHITE, 1.5);
        let grip = 22.0;
        for (cx, cy) in [
            (zone.x, zone.y),
            (zone.right(), zone.y),
            (zone.x, zone.bottom()),
            (zone.right(), zone.bottom()),
        ] {
            let grip_rect = Rect::new(cx - grip / 2.0, cy - grip / 2.0, grip, grip);
            canvas.fill(grip_rect.expand(1.0), 12.0, Color::BLACK.with_alpha(50));
            canvas.fill(grip_rect, 11.0, Color::WHITE);
        }
    }

    /// Dialogue système : 420 px, centré, voile de 40 % sur tout l'écran.
    /// Dialogue d'arrêt (planches « Dialogue d’arrêt ») : trois tuiles (Éteindre,
    /// Redémarrer, Veille), compte à rebours, Annuler et l'action choisie, puis la session
    /// (Se déconnecter, Verrouiller, Relancer le bureau).
    fn draw_dialog(&self, canvas: &mut Canvas, layout: &Layout, power: PowerAction, view: &DesktopView) {
        let dark = palette().text.r > 128;
        let pick = |light: u32, dark_value: u32| Color::rgb(if dark { dark_value } else { light });
        let (fg, fg2, fg3) = (
            palette().text,
            palette().text_secondary,
            palette().window_title_inactive,
        );
        let (panel, line, chip) = (
            palette().window_background,
            palette().separator,
            palette().control_hover,
        );
        let (tile, dot) = (
            palette().header_background,
            themed(Color::WHITE, palette().separator),
        );
        let tile_on = if dark {
            Color::rgb(0x6CB8FF).with_alpha(31)
        } else {
            Color::rgb(0xEAF2FD)
        };
        let red = pick(0xD92D20, 0xE5372B);
        let red_soft = if dark {
            Color::rgb(0xE5372B).with_alpha(46)
        } else {
            Color::rgb(0xFDE7E5)
        };
        let red_icon = pick(0xD92D20, 0xFF8A80);
        let blue = accent();
        let scrim = if dark {
            Color::BLACK.with_alpha(128)
        } else {
            Color::rgb(0x0F172A).with_alpha(82)
        };
        canvas.fill(Rect::new(0.0, 0.0, layout.w, layout.h), 0.0, scrim);

        let dialog = layout.dialog();
        canvas.panel_shadow(dialog, 20.0);
        canvas.fill(dialog, 20.0, panel);
        let chosen = match power {
            PowerAction::Restart | PowerAction::Sleep => power,
            _ => PowerAction::PowerOff,
        };
        let (title, verb, noun) = match chosen {
            PowerAction::Restart => ("Redémarrer l’ordinateur ?", "Redémarrer", "Redémarrage"),
            PowerAction::Sleep => ("Mettre en veille ?", "Mettre en veille", "Mise en veille"),
            _ => ("Éteindre l’ordinateur ?", "Éteindre", "Arrêt"),
        };
        let x = dialog.x + 24.0;
        canvas.text(&self.bold, title, x, dialog.y + 24.0, 20.0, fg);
        let opened = match (chosen, view.power_apps) {
            (PowerAction::Sleep, _) => "Vos applications restent ouvertes.".to_owned(),
            (_, 0) => "Aucune application ouverte.".to_owned(),
            (_, 1) => "1 application ouverte sera fermée.".to_owned(),
            (_, count) => format!("{count} applications ouvertes seront fermées."),
        };
        let description = format!("{opened} {noun} automatique dans {} s.", view.power_left);
        let lines = wrap_dashboard_text(&self.regular, &description, px(14.0), dialog.w - 48.0);
        for (index, text) in lines.iter().take(2).enumerate() {
            canvas.text(
                &self.regular,
                text,
                x,
                dialog.y + 58.0 + index as f64 * 21.0,
                14.0,
                fg2,
            );
        }

        // Tuiles : l'action choisie est entourée (rouge pour Éteindre, bleu sinon).
        let tiles = [
            (PowerAction::PowerOff, "Éteindre", "cc-power"),
            (PowerAction::Restart, "Redémarrer", "cc-restart"),
            (PowerAction::Sleep, "Veille", "cc-dark"),
        ];
        for (index, (action, label, glyph)) in tiles.into_iter().enumerate() {
            let rect = layout.dialog_choice(index);
            let on = action == chosen;
            let danger = action == PowerAction::PowerOff;
            let (fill, ring, disc, ink) = match (on, danger) {
                (true, true) => (red_soft, red, red, Color::WHITE),
                (true, false) => (tile_on, blue, blue, Color::WHITE),
                (false, true) => (tile, line, dot, red_icon),
                (false, false) => (tile, line, dot, fg2),
            };
            canvas.fill(rect, 14.0, fill);
            let inset = if on { 1.0 } else { 0.5 };
            let edge = Rect::new(
                rect.x + inset,
                rect.y + inset,
                rect.w - 2.0 * inset,
                rect.h - 2.0 * inset,
            );
            canvas.stroke(edge, 13.0, ring, if on { 2.0 } else { 1.0 });
            let center = (rect.x + rect.w / 2.0, rect.y + 32.0);
            canvas.fill(
                Rect::new(center.0 - 20.0, center.1 - 20.0, 40.0, 40.0),
                20.0,
                disc,
            );
            canvas.icon(glyph, center, 20.0, Style::Symbolic(ink));
            let width = canvas.css_width(&self.bold, label, 13.0);
            canvas.text(
                &self.bold,
                label,
                rect.x + (rect.w - width) / 2.0,
                rect.y + 60.0,
                13.0,
                fg,
            );
        }

        let separator = layout.dialog_cancel().y - 14.0;
        canvas.fill(Rect::new(dialog.x, separator, dialog.w, 1.0), 0.0, line);
        let cancel = layout.dialog_cancel();
        canvas.fill(cancel, 11.0, chip);
        let width = canvas.css_width(&self.bold, "Annuler", 14.0);
        canvas.text(
            &self.bold,
            "Annuler",
            cancel.x + (cancel.w - width) / 2.0,
            cancel.y + 11.0,
            14.0,
            fg,
        );
        let confirm = layout.dialog_confirm();
        canvas.fill(
            confirm,
            11.0,
            if chosen == PowerAction::PowerOff {
                red
            } else {
                blue
            },
        );
        let width = canvas.css_width(&self.bold, verb, 14.0);
        canvas.text(
            &self.bold,
            verb,
            confirm.x + (confirm.w - width) / 2.0,
            confirm.y + 11.0,
            14.0,
            Color::WHITE,
        );

        // Session : petits boutons discrets.
        for (rect, label, glyph, ink) in [
            (layout.dialog_logout(), "Se déconnecter", Some("cc-logout"), fg2),
            (layout.dialog_lock(), "Verrouiller", Some("cc-lock"), fg2),
            (layout.dialog_relaunch(), "Relancer le bureau", None, fg3),
        ] {
            let mut text_x = rect.x + 10.0;
            if let Some(glyph) = glyph {
                canvas.icon(
                    glyph,
                    (rect.x + 17.0, rect.y + rect.h / 2.0),
                    14.0,
                    Style::Symbolic(ink),
                );
                text_x += 20.0;
            }
            canvas.text(&self.regular, label, text_x, rect.y + 7.0, 13.0, ink);
        }
    }
}

/// Partie de la barre d'outils de capture sous `pos`, s'il y en a une.
pub fn capture_toolbar_at(
    output: Rectangle<i32, Logical>,
    pos: Point<f64, Logical>,
    windows: bool,
) -> Option<CaptureHit> {
    let layout = Layout::new(
        output.size.w as f64,
        output.size.h as f64,
        &DesktopView::default(),
    );
    if layout.capture_close().contains(pos) {
        return Some(CaptureHit::Close);
    }
    if !layout.capture_toolbar().contains(pos) {
        return None;
    }
    let mode = CAPTURE_MODES
        .iter()
        .enumerate()
        .find(|(index, _)| layout.capture_mode(*index).contains(pos))
        .map(|(_, (mode, _))| *mode)
        .filter(|mode| *mode != crate::render::capture::Mode::Window || windows);
    Some(if let Some(mode) = mode {
        CaptureHit::Mode(mode)
    } else if layout.capture_kind(false).contains(pos) {
        CaptureHit::Photo
    } else if layout.capture_kind(true).contains(pos) {
        CaptureHit::Video
    } else if layout.capture_shutter().contains(pos) {
        CaptureHit::Shutter
    } else if layout.capture_delay().contains(pos) {
        CaptureHit::Delay
    } else if layout.capture_pointer().contains(pos) {
        CaptureHit::Pointer
    } else {
        CaptureHit::Toolbar
    })
}

/// Voile sombre sur tout l'écran sauf `zone`.
fn veil_around(canvas: &mut Canvas, layout: &Layout, zone: Rect) {
    let veil = Color::BLACK.with_alpha(140);
    canvas.fill(Rect::new(0.0, 0.0, layout.w, zone.y.max(0.0)), 0.0, veil);
    canvas.fill(
        Rect::new(0.0, zone.bottom(), layout.w, (layout.h - zone.bottom()).max(0.0)),
        0.0,
        veil,
    );
    canvas.fill(Rect::new(0.0, zone.y, zone.x.max(0.0), zone.h), 0.0, veil);
    canvas.fill(
        Rect::new(zone.right(), zone.y, (layout.w - zone.right()).max(0.0), zone.h),
        0.0,
        veil,
    );
}

/// Rectangle en pointillés, coins marqués : le mode Sélection.
fn capture_icon_selection(canvas: &mut Canvas, (cx, cy): (f64, f64), ink: Color) {
    let (left, top, right, bottom) = (cx - 11.0, cy - 9.0, cx + 11.0, cy + 9.0);
    for (start, end) in [
        ((left, top), (right, top)),
        ((left, bottom), (right, bottom)),
        ((left, top), (left, bottom)),
        ((right, top), (right, bottom)),
    ] {
        let length = (end.0 - start.0).hypot(end.1 - start.1);
        let mut at = 4.5;
        while at + 3.0 <= length - 4.0 {
            let a = at / length;
            let b = (at + 3.0) / length;
            canvas.line(
                (start.0 + (end.0 - start.0) * a, start.1 + (end.1 - start.1) * a),
                (start.0 + (end.0 - start.0) * b, start.1 + (end.1 - start.1) * b),
                1.5,
                ink,
            );
            at += 5.0;
        }
    }
    for (x, y) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
        canvas.stroke(Rect::new(x - 2.5, y - 2.5, 5.0, 5.0), 2.5, ink, 1.4);
    }
}

/// Écran sur son pied : le mode Écran.
fn capture_icon_screen(canvas: &mut Canvas, (cx, cy): (f64, f64), ink: Color) {
    canvas.stroke(Rect::new(cx - 12.0, cy - 10.0, 24.0, 16.0), 2.0, ink, 1.6);
    canvas.line((cx, cy + 6.0), (cx, cy + 10.0), 1.6, ink);
    canvas.line((cx - 6.0, cy + 10.5), (cx + 6.0, cy + 10.5), 1.6, ink);
}

/// Deux fenêtres superposées : le mode Fenêtre.
fn capture_icon_window(canvas: &mut Canvas, (cx, cy): (f64, f64), ink: Color, background: Color) {
    canvas.stroke(Rect::new(cx - 12.0, cy - 10.0, 17.0, 13.0), 2.0, ink, 1.6);
    let front = Rect::new(cx - 5.0, cy - 3.0, 17.0, 13.0);
    canvas.fill(front, 2.0, background);
    canvas.stroke(front, 2.0, ink, 1.6);
}

fn capture_icon_camera(canvas: &mut Canvas, (cx, cy): (f64, f64), ink: Color) {
    canvas.stroke(Rect::new(cx - 8.0, cy - 5.0, 16.0, 11.0), 2.5, ink, 1.6);
    canvas.fill(Rect::new(cx - 3.5, cy - 7.5, 7.0, 3.0), 1.0, ink);
    canvas.stroke(Rect::new(cx - 3.0, cy - 2.5, 6.0, 6.0), 3.0, ink, 1.5);
}

fn capture_icon_video(canvas: &mut Canvas, (cx, cy): (f64, f64), ink: Color) {
    canvas.stroke(Rect::new(cx - 8.0, cy - 5.0, 11.0, 10.0), 2.0, ink, 1.6);
    canvas.polygon(
        &[
            (cx + 4.0, cy - 1.5),
            (cx + 8.5, cy - 4.5),
            (cx + 8.5, cy + 4.5),
            (cx + 4.0, cy + 1.5),
        ],
        ink,
    );
}

fn capture_icon_timer(canvas: &mut Canvas, (cx, cy): (f64, f64), ink: Color) {
    canvas.stroke(Rect::new(cx - 7.5, cy - 6.0, 15.0, 15.0), 7.5, ink, 1.6);
    canvas.line((cx, cy + 1.5), (cx, cy - 2.5), 1.6, ink);
    canvas.line((cx, cy + 1.5), (cx + 3.0, cy + 3.0), 1.6, ink);
    canvas.line((cx - 2.5, cy - 9.0), (cx + 2.5, cy - 9.0), 1.6, ink);
}

fn capture_icon_pointer(canvas: &mut Canvas, (cx, cy): (f64, f64), ink: Color) {
    canvas.polygon(
        &[
            (cx - 4.5, cy - 8.0),
            (cx - 4.5, cy + 6.0),
            (cx - 1.0, cy + 2.8),
            (cx + 1.5, cy + 8.0),
            (cx + 3.8, cy + 7.0),
            (cx + 1.3, cy + 2.0),
            (cx + 5.5, cy + 2.0),
        ],
        ink,
    );
}

/// Case de chaque raccourci du bureau sur l'écran `output` (aucune : grille pleine).
pub fn shortcut_cells(
    output: Rectangle<i32, Logical>,
    view: &DesktopView,
) -> Vec<Option<ferrite_system::shortcuts::Cell>> {
    let layout = Layout::new(output.size.w as f64, output.size.h as f64, view);
    cards::shortcut_cells(&layout, &view.dashboard)
}

/// Case où se pose le raccourci `index` lâché après un déplacement de `offset`.
pub fn shortcut_drop_cell(
    output: Rectangle<i32, Logical>,
    view: &DesktopView,
    index: usize,
    offset: Option<(f64, f64)>,
) -> Option<ferrite_system::shortcuts::Cell> {
    let layout = Layout::new(output.size.w as f64, output.size.h as f64, view);
    cards::shortcut_target(&layout, &view.dashboard, index, offset?)
}

/// Hit-test du bureau ; les panneaux interceptent leurs propres surfaces pour que le clic
/// ne tombe jamais sur une fenêtre située derrière eux.
pub fn action_at(
    output: Rectangle<i32, Logical>,
    pos: Point<f64, Logical>,
    view: &DesktopView,
) -> Option<DesktopAction> {
    let layout = Layout::new(output.size.w as f64, output.size.h as f64, view);
    // Authentification demandée : la fenêtre est modale, rien d'autre ne reçoit le clic.
    if view.auth.is_some() {
        let pos = Point::from((pos.x - output.loc.x as f64, pos.y - output.loc.y as f64));
        return Some(auth::action_at(&layout, pos));
    }
    let (popover, pending_power) = (view.popover, view.power);
    if !view.bar_visible && pending_power.is_none() && popover.is_none() && pos.y < view.bar_height as f64 {
        // Barre masquée (plein écran) : le clic va à l'application.
        return None;
    }
    let pos = Point::from((pos.x - output.loc.x as f64, pos.y - output.loc.y as f64));

    if pending_power.is_none()
        && let Some(hit) = island::island_at(&layout, &view.island, pos)
    {
        return Some(match (&view.island.kind, hit) {
            (Some(IslandKind::Recording(_)), IslandHit::Side) => DesktopAction::StopRecording,
            (Some(IslandKind::Notification { notification, .. }), IslandHit::Button(index)) => {
                DesktopAction::NotificationAction(notification.id, index)
            }
            (Some(IslandKind::Notification { notification, .. }), _) => {
                DesktopAction::NotificationActivate(notification.id)
            }
            _ => DesktopAction::Island(hit),
        });
    }

    if popover.is_none()
        && pending_power.is_none()
        && view.dashboard.shown
        && let Some(action) = dashboard_action_at(&layout, view, pos)
    {
        return Some(action);
    }

    if popover.is_none()
        && pending_power.is_none()
        && let Some(action) = cards::shortcut_at(&layout, view, pos)
    {
        return Some(action);
    }

    if let Some(power) = pending_power {
        let choices = [PowerAction::PowerOff, PowerAction::Restart, PowerAction::Sleep];
        let chosen = match power {
            PowerAction::Restart | PowerAction::Sleep => power,
            _ => PowerAction::PowerOff,
        };
        if let Some(index) = (0..3).find(|&index| layout.dialog_choice(index).contains(pos)) {
            return Some(DesktopAction::PowerChoose(choices[index]));
        }
        return Some(if layout.dialog_cancel().contains(pos) {
            DesktopAction::CancelPower
        } else if layout.dialog_confirm().contains(pos) {
            DesktopAction::ConfirmPower(chosen)
        } else if layout.dialog_logout().contains(pos) {
            DesktopAction::ConfirmPower(PowerAction::LogOut)
        } else if layout.dialog_lock().contains(pos) {
            DesktopAction::Lock
        } else if layout.dialog_relaunch().contains(pos) {
            DesktopAction::ConfirmPower(PowerAction::RelaunchDesktop)
        } else if layout.dialog().contains(pos) {
            DesktopAction::Consume
        } else {
            DesktopAction::CancelPower
        });
    }

    if pos.y < view.bar_height as f64 {
        if layout.applications_button().contains(pos) {
            return Some(DesktopAction::TogglePopover(Popover::Launcher));
        }
        if let Some(page) = (0..view.page_count).find(|&page| layout.page_dot(page).contains(pos)) {
            return Some(DesktopAction::GoToPage(page));
        }
        if layout.clock_button().contains(pos) {
            return Some(DesktopAction::TogglePopover(Popover::Notifications));
        }
        if layout.indicators_group().contains(pos) {
            return Some(DesktopAction::TogglePopover(Popover::Controls));
        }
        return Some(if popover.is_some() {
            DesktopAction::ClosePopover
        } else {
            DesktopAction::Consume
        });
    }

    let popover = popover?;
    let action = match popover {
        Popover::DockMenu => match &view.dock_menu {
            Some(menu) if layout.dock_menu(menu).contains(pos) => (0..menu.entries.len())
                .find(|&index| layout.dock_menu_row(menu, index).contains(pos))
                .map_or(DesktopAction::Consume, DesktopAction::DockMenu),
            _ => DesktopAction::ClosePopover,
        },
        Popover::DesktopMenu => match view.desktop_menu {
            Some(anchor)
                if layout
                    .desktop_menu(anchor, view.desktop_menu_entries.len())
                    .contains(pos) =>
            {
                (0..view.desktop_menu_entries.len())
                    .find(|&index| {
                        layout
                            .desktop_menu_row(anchor, view.desktop_menu_entries.len(), index)
                            .contains(pos)
                    })
                    .map_or(DesktopAction::Consume, DesktopAction::DesktopMenu)
            }
            _ => DesktopAction::ClosePopover,
        },
        Popover::Launcher if layout.launcher().contains(pos) => {
            let launcher = &view.launcher;
            if !layout.launcher_content().contains(pos) {
                DesktopAction::Consume
            } else {
                layout
                    .launcher_items(launcher)
                    .into_iter()
                    .find_map(|item| match item {
                        LauncherItem::Tile { section, slot, rect } if rect.contains(pos) => Some(
                            DesktopAction::LaunchTile(launcher.sections[section].tiles[slot].index),
                        ),
                        _ => None,
                    })
                    .unwrap_or(DesktopAction::Consume)
            }
        }
        Popover::Controls if layout.controls().contains(pos) => match view.controls.page {
            ControlsPage::Main => {
                let on = |control| layout.control_row(control).contains(pos);
                let percent = |control| slider_percent(layout.slider_track(control), pos.x);
                if on(Control::NightMore) {
                    DesktopAction::OpenNightSettings
                } else if on(Control::Night) {
                    DesktopAction::ToggleNight
                } else if on(Control::WifiMore) && view.wifi.is_some() {
                    DesktopAction::ControlsPage(ControlsPage::Wifi)
                } else if on(Control::BluetoothMore) && view.bluetooth.is_some() {
                    DesktopAction::ControlsPage(ControlsPage::Bluetooth)
                } else if on(Control::Wifi) && view.wifi.is_some() {
                    DesktopAction::ToggleWifi
                } else if on(Control::Bluetooth) && view.bluetooth.is_some() {
                    DesktopAction::ToggleBluetooth
                } else if on(Control::Dark) {
                    DesktopAction::ToggleDarkMode
                } else if on(Control::DoNotDisturb) {
                    DesktopAction::ToggleDoNotDisturb
                } else if on(Control::Saver) && view.power_saver.is_some() {
                    DesktopAction::TogglePowerSaver
                } else if on(Control::Keyboard) {
                    DesktopAction::ToggleKeyboardLayout
                } else if on(Control::Airplane) && view.airplane.is_some() {
                    DesktopAction::ToggleAirplane
                } else if on(Control::KeyboardLight) && view.keyboard_light.is_some() {
                    DesktopAction::CycleKeyboardLight
                } else if on(Control::Account) {
                    DesktopAction::OpenSettingsPage("comptes")
                } else if on(Control::Settings) {
                    DesktopAction::OpenSettings
                } else if on(Control::Lock) {
                    DesktopAction::Lock
                } else if on(Control::Power) {
                    DesktopAction::OpenPowerDialog
                } else if on(Control::Volume) && view.volume.is_some() {
                    DesktopAction::SetVolume(percent(Control::Volume))
                } else if on(Control::Brightness) && view.brightness.is_some() {
                    DesktopAction::SetBrightness(percent(Control::Brightness))
                } else {
                    DesktopAction::Consume
                }
            }
            ControlsPage::Wifi => {
                let networks = &view.controls.networks;
                if layout.controls_back().contains(pos) {
                    DesktopAction::ControlsPage(ControlsPage::Main)
                } else if layout.controls_switch().contains(pos) && view.wifi.is_some() {
                    DesktopAction::ToggleWifi
                } else if view.controls.password.is_some() && layout.password_eye().contains(pos) {
                    DesktopAction::PasswordReveal
                } else if view.controls.password.is_some() && layout.password_cancel().contains(pos) {
                    DesktopAction::PasswordCancel
                } else if view.controls.password.is_some() && layout.password_submit().contains(pos) {
                    DesktopAction::PasswordSubmit
                } else if layout.controls_bottom().contains(pos) {
                    DesktopAction::OpenSettingsPage("wifi")
                } else if let Some(network) = networks
                    .iter()
                    .enumerate()
                    .find(|(slot, network)| {
                        network.known
                            && network.secured
                            && layout.wifi_eye(*slot, network.connected).contains(pos)
                    })
                    .map(|(_, network)| network)
                {
                    DesktopAction::NetworkPassword(network.index)
                } else if let Some(network) = networks
                    .iter()
                    .enumerate()
                    .find(|(slot, _)| layout.wifi_row(*slot).contains(pos))
                    .map(|(_, network)| network)
                {
                    DesktopAction::NetworkJoin(network.index)
                } else {
                    DesktopAction::Consume
                }
            }
            ControlsPage::Bluetooth => {
                let devices = &view.controls.devices;
                if layout.controls_back().contains(pos) {
                    DesktopAction::ControlsPage(ControlsPage::Main)
                } else if layout.controls_switch().contains(pos) && view.bluetooth.is_some() {
                    DesktopAction::ToggleBluetooth
                } else if layout.controls_bottom().contains(pos) {
                    DesktopAction::OpenSettingsPage("bluetooth")
                } else if let Some(device) = devices
                    .iter()
                    .enumerate()
                    .find(|(slot, _)| layout.bluetooth_button(*slot).contains(pos))
                    .map(|(_, device)| device)
                {
                    if device.busy {
                        DesktopAction::Consume
                    } else {
                        DesktopAction::DeviceAction(device.index)
                    }
                } else {
                    DesktopAction::Consume
                }
            }
        },
        Popover::Screenshot => DesktopAction::Consume,
        Popover::Clipboard if layout.clipboard().contains(pos) => {
            let clipboard = &view.clipboard;
            let filter = ClipFilter::ALL
                .into_iter()
                .enumerate()
                .find(|(slot, _)| layout.clipboard_filter(*slot).contains(pos));
            if clipboard.toast.is_some() && layout.clipboard_undo().contains(pos) {
                DesktopAction::ClipboardUndo
            } else if layout.clipboard_theme().contains(pos) {
                DesktopAction::ToggleDarkMode
            } else if layout.clipboard_clear().contains(pos) {
                DesktopAction::ClipboardClear
            } else if let Some((_, filter)) = filter {
                DesktopAction::ClipboardFilter(filter)
            } else if layout.clipboard_header().contains(pos) {
                let card = layout.clipboard();
                DesktopAction::ClipboardDragStart(
                    (pos.x - card.x).round() as i32,
                    (pos.y - card.y).round() as i32,
                )
            } else if layout.clipboard_clip().contains(pos)
                && let Some(row) = clipboard
                    .rows
                    .iter()
                    .find(|row| layout.clipboard_row(row).contains(pos))
            {
                let rect = layout.clipboard_row(row);
                if clipboard_pin(rect).contains(pos) {
                    DesktopAction::ClipboardPin(row.index)
                } else if clipboard_delete(rect).contains(pos) {
                    DesktopAction::ClipboardDelete(row.index)
                } else {
                    DesktopAction::ClipboardPaste(row.index)
                }
            } else {
                DesktopAction::Consume
            }
        }
        Popover::Notifications if layout.calendar(view.notifications.history.len()).contains(pos) => {
            let history = &view.notifications.history;
            let card = layout.calendar(history.len());
            let ((year, month), _) = calendar_months(0);
            let day = (1..=days_in_month(i64::from(year), month))
                .find(|day| calendar_cell(card, i64::from(year), month, *day).contains(pos));
            let title = Rect::new(card.x, card.y, card.w, 52.0);
            let row = history
                .iter()
                .take(layout.notification_rows())
                .enumerate()
                .find(|(slot, _)| layout.notification_row(history.len(), *slot).contains(pos));
            if let Some(day) = day {
                DesktopAction::OpenCalendar(Some((year, month, day)))
            } else if title.contains(pos) {
                DesktopAction::OpenCalendar(None)
            } else if !history.is_empty() && layout.notifications_clear(history.len()).contains(pos) {
                DesktopAction::NotificationsClear
            } else if let Some((_, notification)) = row {
                DesktopAction::NotificationActivate(notification.id)
            } else {
                DesktopAction::Consume
            }
        }
        _ => DesktopAction::ClosePopover,
    };
    Some(action)
}

/// Élément du lanceur, placé par [`launcher_flow`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum LauncherItem {
    Title {
        section: usize,
        rect: Rect,
    },
    /// Case `slot` de la rubrique `section`.
    Tile {
        section: usize,
        slot: usize,
        rect: Rect,
    },
}

impl LauncherItem {
    /// Au moins en partie dans la zone visible `content` (seuls ceux-là sont dessinés).
    fn visible_in(&self, content: Rect) -> bool {
        let (LauncherItem::Title { rect, .. } | LauncherItem::Tile { rect, .. }) = *self;
        rect.bottom() > content.y && rect.y < content.bottom()
    }
}

/// Rubriques les unes sous les autres, relatives au coin du contenu (marge comprise) ;
/// renvoie aussi la hauteur totale.
fn launcher_flow(launcher: &LauncherView) -> (Vec<LauncherItem>, f64) {
    let width = LAUNCHER_COLUMNS as f64 * LAUNCHER_CELL.0;
    let mut items = Vec::new();
    let mut y = 4.0;
    for (index, section) in launcher.sections.iter().enumerate() {
        if !section.title.is_empty() {
            items.push(LauncherItem::Title {
                section: index,
                rect: Rect::new(0.0, y, width, LAUNCHER_TITLE),
            });
            y += LAUNCHER_TITLE;
        }
        for slot in 0..section.tiles.len() {
            let (column, row) = (slot % LAUNCHER_COLUMNS, slot / LAUNCHER_COLUMNS);
            items.push(LauncherItem::Tile {
                section: index,
                slot,
                rect: Rect::new(
                    column as f64 * LAUNCHER_CELL.0,
                    y + row as f64 * LAUNCHER_CELL.1,
                    LAUNCHER_CELL.0,
                    LAUNCHER_CELL.1,
                ),
            });
        }
        y += section.tiles.len().div_ceil(LAUNCHER_COLUMNS) as f64 * LAUNCHER_CELL.1;
    }
    (items, y + 8.0)
}

/// Défilement maximal du lanceur, en pixels logiques (pour borner la molette).
pub fn launcher_scroll_limit(output: Rectangle<i32, Logical>, view: &DesktopView) -> u32 {
    let layout = Layout::new(output.size.w as f64, output.size.h as f64, view);
    let (_, height) = launcher_flow(&view.launcher);
    (height - layout.launcher_content().h).max(0.0).ceil() as u32
}

/// Marges autour du widget saisi (son badge de retrait déborde de 6 points) et de la case
/// visée (son liseré).
const LIFT_MARGIN: f64 = 10.0;
const TARGET_MARGIN: f64 = 2.0;

/// Images du glisser d'un widget, chacune avec ce dont elle dépend : le reste du tableau
/// de bord, le widget saisi (et sa taille) et la case visée (taille et échelle).
type DragKey = VisualState;
/// Image prête et sa taille en pixels.
type Layer = (MemoryRenderBuffer, (i32, i32));
#[derive(Default)]
struct DragLayers {
    rest: Option<(DragKey, MemoryRenderBuffer)>,
    card: Option<(DragKey, (i32, i32), Layer)>,
    target: Option<((i32, i32, u32), Layer)>,
}

impl std::fmt::Debug for DragLayers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragLayers").finish_non_exhaustive()
    }
}

/// Tampon d'un canevas, et sa taille en pixels.
fn canvas_buffer(canvas: Canvas) -> Layer {
    let pixels = (canvas.width as i32, canvas.height as i32);
    let buffer = MemoryRenderBuffer::from_slice(
        &canvas.pixels,
        Fourcc::Argb8888,
        pixels,
        1,
        smithay::utils::Transform::Normal,
        None,
    );
    (buffer, pixels)
}

/// Pose `buffer` (`pixels` de côté) au point logique `at`, pixel exact.
#[allow(clippy::too_many_arguments)] // Rendu, image, place et opacité.
fn push_layer(
    renderer: &mut GlesRenderer,
    output: &Output,
    buffer: &MemoryRenderBuffer,
    pixels: (i32, i32),
    at: (f64, f64),
    scale: f64,
    alpha: f32,
    out: &mut Vec<super::super::render::OutputElement>,
) {
    let position =
        Point::<f64, smithay::utils::Physical>::from(((at.0 * scale).round(), (at.1 * scale).round()));
    let size = Size::<i32, Logical>::from((
        (f64::from(pixels.0) / scale).round() as i32,
        (f64::from(pixels.1) / scale).round() as i32,
    ));
    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        position,
        buffer,
        Some(alpha),
        Some(Rectangle::<f64, Logical>::from_size(
            (f64::from(pixels.0), f64::from(pixels.1)).into(),
        )),
        Some(size),
        Kind::Unspecified,
    ) {
        Ok(element) => out.push(super::super::render::OutputElement::Memory(element)),
        Err(err) => tracing::warn!(?err, output = output.name(), "widget déplacé non importé"),
    }
}

/// Couleur du fond flouté au point (`u`, `v`) de l'écran (0 à 1), interpolée.
fn sample(image: &Image, u: f64, v: f64) -> [f32; 3] {
    let x = (u * image.width as f64 - 0.5).clamp(0.0, image.width as f64 - 1.0);
    let y = (v * image.height as f64 - 0.5).clamp(0.0, image.height as f64 - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(image.width - 1), (y0 + 1).min(image.height - 1));
    let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
    let at =
        |x: u32, y: u32, channel: usize| image.rgba[((y * image.width + x) * 4) as usize + channel] as f32;
    [0, 1, 2].map(|c| {
        let top = at(x0, y0, c) + (at(x1, y0, c) - at(x0, y0, c)) * fx;
        let bottom = at(x0, y1, c) + (at(x1, y1, c) - at(x0, y1, c)) * fx;
        top + (bottom - top) * fy
    })
}

/// Nom d'une application sur deux lignes au plus, coupé entre les mots ; la seconde
/// ligne se termine par « … » si elle déborde.
fn wrap_two_lines(canvas: &Canvas, font: &Font, text: &str, size: f32, width: f64) -> Vec<String> {
    if canvas.text_width(font, text, size) <= width {
        return vec![text.to_owned()];
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut first = String::new();
    let mut rest = 0;
    for (index, word) in words.iter().enumerate() {
        let candidate = if first.is_empty() {
            (*word).to_owned()
        } else {
            format!("{first} {word}")
        };
        if canvas.text_width(font, &candidate, size) > width && !first.is_empty() {
            break;
        }
        first = candidate;
        rest = index + 1;
    }
    let first = ellipsize_to(canvas, font, &first, size, width);
    let second = words[rest..].join(" ");
    if second.is_empty() {
        vec![first]
    } else {
        vec![first, ellipsize_to(canvas, font, &second, size, width)]
    }
}

/// Curseur des réglages rapides que l'on peut faire glisser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slider {
    Volume,
    Brightness,
}

/// Valeur (0 à 100) d'un curseur des réglages rapides pour le pointeur en `pos`, même
/// hors du curseur pendant un glissement.
pub fn slider_value(
    output: Rectangle<i32, Logical>,
    view: &DesktopView,
    slider: Slider,
    pos: Point<f64, Logical>,
) -> u8 {
    let layout = Layout::new(output.size.w as f64, output.size.h as f64, view);
    let control = match slider {
        Slider::Volume => Control::Volume,
        Slider::Brightness => Control::Brightness,
    };
    slider_percent(layout.slider_track(control), pos.x - output.loc.x as f64)
}

/// Valeur d'un curseur pour l'abscisse `x` : le bouton reste entier dans la piste, d'où
/// la course réduite d'un demi-bouton de chaque côté.
fn slider_percent(track: Rect, x: f64) -> u8 {
    let knob = track.h;
    (((x - track.x - knob / 2.0) / (track.w - knob)).clamp(0.0, 1.0) * 100.0).round() as u8
}

/// Indicateurs de la barre, de gauche à droite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Indicator {
    Bluetooth,
    /// Wi-Fi s'il y a une carte, réseau filaire sinon : toujours présent.
    Network,
    Volume,
    Battery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Wifi,
    Bluetooth,
    Dark,
    DoNotDisturb,
    Saver,
    Keyboard,
    Airplane,
    KeyboardLight,
    /// Tuile large du mode nuit, et son chevron (réglages).
    Night,
    NightMore,
    Volume,
    Brightness,
    /// Chevrons des tuiles Wi-Fi et Bluetooth.
    WifiMore,
    BluetoothMore,
    /// Pied : compte (Paramètres → Comptes), Paramètres, verrouiller, éteindre.
    Account,
    Settings,
    Lock,
    Power,
}

/// Géométrie du bureau, en pixels logiques relatifs à l'écran.
struct Layout {
    w: f64,
    h: f64,
    bar: f64,
    dock_height: f64,
    page: usize,
    clipboard_position: Option<(i32, i32)>,
    controls_view: ControlsView,
    battery: bool,
    battery_percent: bool,
    bluetooth: bool,
    volume: bool,
    brightness: bool,
    error: bool,
}

impl Layout {
    /// Barre d'outils de capture, centrée en bas de l'écran.
    fn capture_toolbar(&self) -> Rect {
        let (w, h) = (300.0, 168.0);
        Rect::new(
            ((self.w - w) / 2.0).max(8.0),
            (self.h - h - 40.0).max(self.bar + 12.0),
            w,
            h,
        )
    }

    fn capture_mode(&self, index: usize) -> Rect {
        let bar = self.capture_toolbar();
        Rect::new(bar.x + 18.0 + index as f64 * 90.0, bar.y + 16.0, 84.0, 66.0)
    }

    /// Moitié photo (`false`) ou vidéo (`true`) du sélecteur.
    fn capture_kind(&self, video: bool) -> Rect {
        let bar = self.capture_toolbar();
        Rect::new(
            bar.x + 18.0 + if video { 40.0 } else { 0.0 },
            bar.y + 109.0,
            40.0,
            34.0,
        )
    }

    fn capture_shutter(&self) -> Rect {
        let bar = self.capture_toolbar();
        Rect::new(bar.x + bar.w / 2.0 - 28.0, bar.y + 98.0, 56.0, 56.0)
    }

    fn capture_delay(&self) -> Rect {
        let bar = self.capture_toolbar();
        Rect::new(bar.right() - 92.0, bar.y + 109.0, 34.0, 34.0)
    }

    fn capture_pointer(&self) -> Rect {
        let bar = self.capture_toolbar();
        Rect::new(bar.right() - 52.0, bar.y + 109.0, 34.0, 34.0)
    }

    /// Croix ronde à cheval sur le coin haut droit de la barre.
    fn capture_close(&self) -> Rect {
        let bar = self.capture_toolbar();
        Rect::new(bar.right() - 24.0, bar.y - 14.0, 34.0, 34.0)
    }

    fn applications_button(&self) -> Rect {
        // Logo de 20 px, « Applications » en demi-gras, et une marge de chaque côté.
        Rect::new(6.0, 0.0, 138.0, self.bar)
    }

    /// Indicateurs présents, de gauche à droite.
    fn present(&self) -> Vec<Indicator> {
        let mut list = Vec::new();
        if self.bluetooth {
            list.push(Indicator::Bluetooth);
        }
        list.push(Indicator::Network);
        if self.volume {
            list.push(Indicator::Volume);
        }
        if self.battery {
            list.push(Indicator::Battery);
        }
        list
    }

    /// Case d'un indicateur ; vide (largeur nulle) s'il est absent.
    fn indicator(&self, indicator: Indicator) -> Rect {
        let present = self.present();
        let Some(index) = present.iter().position(|known| *known == indicator) else {
            return Rect::new(self.w, 0.0, 0.0, 0.0);
        };
        let from_right = (present.len() - index) as f64;
        // Le dernier s'arrête à 16 px du bord, la case dépassant son icône de 8 px.
        Rect::new(
            self.w - 8.0 - self.percent_width() - from_right * INDICATOR_WIDTH,
            0.0,
            INDICATOR_WIDTH,
            self.bar,
        )
    }

    /// Place du pourcentage de batterie.
    fn percent_width(&self) -> f64 {
        if self.battery && self.battery_percent {
            40.0
        } else {
            0.0
        }
    }

    fn indicators(&self) -> Vec<(Indicator, Rect)> {
        self.present()
            .into_iter()
            .map(|indicator| (indicator, self.indicator(indicator)))
            .collect()
    }

    /// Indicateurs, qui ouvrent le centre de contrôle.
    fn indicators_group(&self) -> Rect {
        let first = self.indicator(self.present()[0]);
        Rect::new(first.x, 0.0, self.w - 8.0 - first.x, self.bar)
    }

    /// Horloge au centre de la barre, qui ouvre le calendrier et les notifications.
    fn clock_button(&self) -> Rect {
        Rect::new((self.w - CLOCK_WIDTH) / 2.0, 0.0, CLOCK_WIDTH, self.bar)
    }

    /// Lignes de notifications qui tiennent sous le calendrier.
    fn notification_rows(&self) -> usize {
        let room = self.h - self.bar - POPOVER_MARGIN - 12.0 - CALENDAR_BODY - NOTIFICATION_HEADER - 18.0;
        ((room / (NOTIFICATION_HEIGHT + 8.0)).floor() as usize).max(1)
    }

    /// Panneau du calendrier et des notifications, centré sous la barre.
    fn calendar(&self, count: usize) -> Rect {
        let rows = count.min(self.notification_rows());
        let list = if rows == 0 {
            44.0
        } else {
            rows as f64 * (NOTIFICATION_HEIGHT + 8.0)
        };
        Rect::new(
            (self.w - CALENDAR_WIDTH) / 2.0,
            self.bar + POPOVER_MARGIN,
            CALENDAR_WIDTH,
            CALENDAR_BODY + NOTIFICATION_HEADER + list + 10.0,
        )
    }

    /// Filet au-dessus des notifications.
    fn calendar_rule(&self, count: usize) -> Rect {
        let card = self.calendar(count);
        Rect::new(card.x + 18.0, card.y + CALENDAR_BODY, card.w - 36.0, 1.0)
    }

    fn notifications_clear(&self, count: usize) -> Rect {
        let rule = self.calendar_rule(count);
        Rect::new(rule.right() - 92.0, rule.y + 12.0, 92.0, 26.0)
    }

    fn notification_row(&self, count: usize, slot: usize) -> Rect {
        let rule = self.calendar_rule(count);
        Rect::new(
            rule.x,
            rule.y + NOTIFICATION_HEADER + slot as f64 * (NOTIFICATION_HEIGHT + 8.0),
            rule.w,
            NOTIFICATION_HEIGHT,
        )
    }

    /// Centre de contrôle, ancré en haut à droite, 8 px sous la barre.
    fn controls(&self) -> Rect {
        let h = match self.controls_view.page {
            ControlsPage::Main => {
                let sliders = [self.volume, self.brightness]
                    .into_iter()
                    .filter(|&present| present)
                    .count() as f64;
                let error = if self.error { 22.0 } else { 0.0 };
                2.0 * CONTROLS_PAD
                    + CONTROLS_TILE_ROWS * CONTROLS_TILE
                    + (CONTROLS_TILE_ROWS - 1.0) * 10.0
                    + CONTROLS_GAP
                    + sliders * (CONTROLS_ROW + 6.0)
                    + CONTROLS_GAP
                    + 13.0
                    + CONTROLS_ROW
                    + error
            }
            ControlsPage::Wifi => CONTROLS_LIST_TOP + self.wifi_list_height() + CONTROLS_LIST_BOTTOM,
            ControlsPage::Bluetooth => {
                let groups = self.bluetooth_groups();
                CONTROLS_LIST_TOP
                    + groups
                        .iter()
                        .map(|count| {
                            if *count == 0 {
                                0.0
                            } else {
                                CONTROLS_HEADING + *count as f64 * CONTROLS_LIST_ROW + 10.0
                            }
                        })
                        .sum::<f64>()
                    + CONTROLS_LIST_BOTTOM
            }
        };
        Rect::new(
            (self.w - 16.0 - CONTROLS_WIDTH).max(POPOVER_MARGIN),
            self.bar + POPOVER_MARGIN,
            CONTROLS_WIDTH,
            h,
        )
    }

    /// Retour aux tuiles, en haut à gauche d'une liste.
    fn controls_back(&self) -> Rect {
        let card = self.controls();
        Rect::new(card.x + CONTROLS_PAD, card.y + CONTROLS_PAD, 34.0, 34.0)
    }

    /// Interrupteur, en haut à droite d'une liste.
    fn controls_switch(&self) -> Rect {
        let card = self.controls();
        Rect::new(
            card.right() - CONTROLS_PAD - 46.0,
            card.y + CONTROLS_PAD + 3.0,
            46.0,
            28.0,
        )
    }

    /// Bas d'une liste : état et lien vers Paramètres.
    fn controls_bottom(&self) -> Rect {
        let card = self.controls();
        Rect::new(
            card.x + CONTROLS_PAD,
            card.bottom() - CONTROLS_LIST_BOTTOM + 4.0,
            card.w - 2.0 * CONTROLS_PAD,
            32.0,
        )
    }

    fn wifi_list_height(&self) -> f64 {
        let rows = self.controls_view.networks.len().max(1) as f64;
        let password = if self.controls_view.password.is_some() {
            CONTROLS_PASSWORD
        } else {
            0.0
        };
        rows * CONTROLS_LIST_ROW + password
    }

    fn wifi_list(&self) -> Rect {
        let card = self.controls();
        Rect::new(
            card.x + CONTROLS_PAD,
            card.y + CONTROLS_LIST_TOP,
            card.w - 2.0 * CONTROLS_PAD,
            self.wifi_list_height(),
        )
    }

    /// Place du réseau dont on demande le mot de passe.
    fn password_slot(&self) -> Option<usize> {
        let entry = self.controls_view.password.as_ref()?;
        self.controls_view
            .networks
            .iter()
            .position(|network| network.ssid == entry.ssid)
    }

    /// Ligne d'un réseau ; celles sous le mot de passe demandé descendent.
    fn wifi_row(&self, slot: usize) -> Rect {
        let list = self.wifi_list();
        let below = self.password_slot().is_some_and(|open| slot > open);
        let extra = if below { CONTROLS_PASSWORD } else { 0.0 };
        Rect::new(
            list.x,
            list.y + slot as f64 * CONTROLS_LIST_ROW + extra,
            list.w,
            CONTROLS_LIST_ROW,
        )
    }

    fn password_field(&self) -> Rect {
        let row = self.wifi_row(self.password_slot().unwrap_or_default());
        Rect::new(row.x + 48.0, row.bottom() + 24.0, row.w - 48.0 - 14.0, 40.0)
    }

    /// L'œil, au bout du champ de mot de passe.
    fn password_eye(&self) -> Rect {
        let field = self.password_field();
        Rect::new(field.right() - 40.0, field.y, 40.0, field.h)
    }

    /// L'œil d'un réseau enregistré, à gauche de son cadenas (et de sa coche).
    fn wifi_eye(&self, slot: usize, connected: bool) -> Rect {
        let row = self.wifi_row(slot);
        let lock = row.right() - 26.0 - if connected { 28.0 } else { 0.0 };
        let center = (lock - 28.0, row.y + CONTROLS_LIST_ROW / 2.0);
        Rect::new(center.0 - 14.0, center.1 - 14.0, 28.0, 28.0)
    }

    fn password_submit(&self) -> Rect {
        let field = self.password_field();
        Rect::new(field.right() - 108.0, field.bottom() + 10.0, 108.0, 32.0)
    }

    fn password_cancel(&self) -> Rect {
        let submit = self.password_submit();
        Rect::new(submit.x - 8.0 - 76.0, submit.y, 76.0, submit.h)
    }

    /// Nombre d'appareils associés, puis à proximité.
    fn bluetooth_groups(&self) -> [usize; 2] {
        let devices = &self.controls_view.devices;
        let paired = devices.iter().filter(|device| device.paired).count();
        [paired, devices.len() - paired]
    }

    /// Titre et fond du groupe des appareils associés, ou de ceux à proximité.
    fn bluetooth_group(&self, paired: bool) -> Option<(Rect, Rect)> {
        let card = self.controls();
        let [mine, nearby] = self.bluetooth_groups();
        let (count, before) = if paired { (mine, 0) } else { (nearby, mine) };
        if count == 0 {
            return None;
        }
        let skipped = if before == 0 {
            0.0
        } else {
            CONTROLS_HEADING + before as f64 * CONTROLS_LIST_ROW + 10.0
        };
        let top = card.y + CONTROLS_LIST_TOP + skipped;
        let width = card.w - 2.0 * CONTROLS_PAD;
        Some((
            Rect::new(card.x + CONTROLS_PAD + 4.0, top, width, CONTROLS_HEADING),
            Rect::new(
                card.x + CONTROLS_PAD,
                top + CONTROLS_HEADING,
                width,
                count as f64 * CONTROLS_LIST_ROW,
            ),
        ))
    }

    /// Ligne d'un appareil ; la liste est triée, les associés d'abord.
    fn bluetooth_row(&self, slot: usize) -> Rect {
        let [mine, _] = self.bluetooth_groups();
        let paired = slot < mine;
        let index = if paired { slot } else { slot - mine };
        let list = self
            .bluetooth_group(paired)
            .map_or(self.controls(), |(_, list)| list);
        Rect::new(
            list.x,
            list.y + index as f64 * CONTROLS_LIST_ROW,
            list.w,
            CONTROLS_LIST_ROW,
        )
    }

    fn bluetooth_button(&self, slot: usize) -> Rect {
        let row = self.bluetooth_row(slot);
        Rect::new(
            row.right() - 12.0 - 96.0,
            row.y + (row.h - 30.0) / 2.0,
            96.0,
            30.0,
        )
    }

    /// Case d'un réglage ; vide si le curseur est absent.
    fn control_row(&self, control: Control) -> Rect {
        let card = self.controls();
        let inner = card.w - 2.0 * CONTROLS_PAD;
        let (left, top) = (card.x + CONTROLS_PAD, card.y + CONTROLS_PAD);
        let tile_w = (inner - 10.0) / 2.0;
        let tile = |column: f64, row: f64| {
            Rect::new(
                left + column * (tile_w + 10.0),
                top + row * (CONTROLS_TILE + 10.0),
                tile_w,
                CONTROLS_TILE,
            )
        };
        let sliders_top =
            top + CONTROLS_TILE_ROWS * CONTROLS_TILE + (CONTROLS_TILE_ROWS - 1.0) * 10.0 + CONTROLS_GAP;
        let row = |index: f64| {
            Rect::new(
                left,
                sliders_top + index * (CONTROLS_ROW + 6.0),
                inner,
                CONTROLS_ROW,
            )
        };
        let footer = self.controls_footer();
        let round = |from_right: f64| {
            Rect::new(
                footer.right() - from_right * 40.0 + 8.0,
                footer.y + 15.0,
                32.0,
                32.0,
            )
        };
        let chevron = |tile: Rect| Rect::new(tile.right() - 36.0, tile.y, 36.0, tile.h);
        match control {
            Control::Wifi => tile(0.0, 0.0),
            Control::Bluetooth => tile(1.0, 0.0),
            Control::Dark => tile(0.0, 1.0),
            Control::DoNotDisturb => tile(1.0, 1.0),
            Control::Saver => tile(0.0, 2.0),
            Control::Keyboard => tile(1.0, 2.0),
            Control::Airplane => tile(0.0, 3.0),
            Control::KeyboardLight => tile(1.0, 3.0),
            // Une rangée entière : le mode nuit a besoin de place pour son horaire.
            Control::Night => {
                let left = tile(0.0, 4.0);
                Rect::new(left.x, left.y, inner, left.h)
            }
            Control::NightMore => {
                let left = tile(0.0, 4.0);
                chevron(Rect::new(left.x, left.y, inner, left.h))
            }
            Control::Volume if self.volume => row(0.0),
            Control::Brightness if self.brightness => row(if self.volume { 1.0 } else { 0.0 }),
            Control::Volume | Control::Brightness => Rect::new(card.x, card.y, 0.0, 0.0),
            Control::WifiMore => chevron(tile(0.0, 0.0)),
            Control::BluetoothMore => chevron(tile(1.0, 0.0)),
            Control::Account => round(4.0),
            Control::Settings => round(3.0),
            Control::Lock => round(2.0),
            Control::Power => round(1.0),
        }
    }

    /// Bas du centre de contrôle : filet, batterie et boutons de session.
    fn controls_footer(&self) -> Rect {
        let card = self.controls();
        let error = if self.error { 22.0 } else { 0.0 };
        Rect::new(
            card.x + CONTROLS_PAD,
            card.bottom() - CONTROLS_PAD - error - CONTROLS_ROW - 13.0,
            card.w - 2.0 * CONTROLS_PAD,
            CONTROLS_ROW + 13.0,
        )
    }

    /// Ligne du message d'erreur, tout en bas du centre de contrôle.
    fn controls_error(&self) -> Rect {
        let card = self.controls();
        Rect::new(
            card.x + CONTROLS_PAD,
            card.bottom() - CONTROLS_PAD - 16.0,
            card.w - 2.0 * CONTROLS_PAD,
            16.0,
        )
    }

    /// Piste d'un curseur : le bouton de 20 px y circule, entre l'icône et le pourcentage.
    fn slider_track(&self, control: Control) -> Rect {
        let row = self.control_row(control);
        Rect::new(
            row.x + 32.0,
            row.y + (row.h - 20.0) / 2.0,
            row.w - 32.0 - 46.0,
            20.0,
        )
    }

    /// Dialogue d'arrêt : 480 px, centré.
    fn dialog(&self) -> Rect {
        Rect::new(
            ((self.w - DIALOG.0) / 2.0).max(8.0),
            ((self.h - DIALOG.1) / 2.0).max(self.bar + 8.0),
            DIALOG.0,
            DIALOG.1,
        )
    }

    /// Tuile `index` (0 : Éteindre, 1 : Redémarrer, 2 : Veille).
    fn dialog_choice(&self, index: usize) -> Rect {
        let dialog = self.dialog();
        let width = (dialog.w - 48.0 - 20.0) / 3.0;
        Rect::new(
            dialog.x + 24.0 + index as f64 * (width + 10.0),
            dialog.y + 116.0,
            width,
            92.0,
        )
    }

    fn dialog_confirm(&self) -> Rect {
        let dialog = self.dialog();
        Rect::new(dialog.right() - 24.0 - 170.0, dialog.y + 242.0, 170.0, 40.0)
    }

    fn dialog_cancel(&self) -> Rect {
        let confirm = self.dialog_confirm();
        Rect::new(confirm.x - 10.0 - 104.0, confirm.y, 104.0, confirm.h)
    }

    /// Rangée de la session, sous les boutons de l'ordinateur.
    fn dialog_logout(&self) -> Rect {
        let dialog = self.dialog();
        Rect::new(dialog.x + 24.0, dialog.y + 296.0, 136.0, 30.0)
    }

    fn dialog_lock(&self) -> Rect {
        let logout = self.dialog_logout();
        Rect::new(logout.right() + 4.0, logout.y, 112.0, logout.h)
    }

    fn dialog_relaunch(&self) -> Rect {
        let dialog = self.dialog();
        let logout = self.dialog_logout();
        Rect::new(dialog.right() - 24.0 - 140.0, logout.y, 140.0, logout.h)
    }

    fn new(w: f64, h: f64, view: &DesktopView) -> Self {
        Self {
            w,
            h,
            page: view.page,
            dock_height: view.dock_height as f64,
            bar: view.bar_height as f64,
            clipboard_position: view.clipboard.position,
            controls_view: view.controls.clone(),
            battery: battery_dir().is_some(),
            battery_percent: view.battery_percent,
            bluetooth: view.bluetooth_icon && view.bluetooth.is_some(),
            volume: view.volume.is_some(),
            brightness: view.brightness.is_some(),
            error: view.error.is_some(),
        }
    }

    /// Menu du bureau : 260 de large, des lignes de 32, marge de 6, jamais hors de l'écran.
    fn desktop_menu(&self, anchor: (i32, i32), rows: usize) -> Rect {
        let (w, h) = (260.0, rows as f64 * 32.0 + 12.0);
        Rect::new(
            f64::from(anchor.0)
                .min(self.w - w - POPOVER_MARGIN)
                .max(POPOVER_MARGIN),
            f64::from(anchor.1)
                .min(self.h - h - POPOVER_MARGIN)
                .max(self.bar + POPOVER_MARGIN),
            w,
            h,
        )
    }

    fn desktop_menu_row(&self, anchor: (i32, i32), rows: usize, index: usize) -> Rect {
        let menu = self.desktop_menu(anchor, rows);
        Rect::new(
            menu.x + 6.0,
            menu.y + 6.0 + index as f64 * 32.0,
            menu.w - 12.0,
            32.0,
        )
    }

    /// Marque du bureau `page` : la pilule du bureau affiché prend plus de place.
    fn page_dot(&self, page: usize) -> Rect {
        let width = |index: usize| {
            if index == self.page {
                PAGE_MARK_ACTIVE
            } else {
                PAGE_MARK
            }
        };
        let before: f64 = (0..page).map(|index| width(index) + PAGE_MARK_GAP).sum();
        // La zone du clic déborde dans les écarts.
        Rect::new(
            150.0 + before - PAGE_MARK_GAP / 2.0,
            0.0,
            width(page) + PAGE_MARK_GAP,
            self.bar,
        )
    }

    /// Lanceur, centré dans l'écran (un peu au-dessus du milieu, comme sur macOS).
    fn launcher(&self) -> Rect {
        let w = 2.0 * LAUNCHER_PAD + LAUNCHER_COLUMNS as f64 * LAUNCHER_CELL.0;
        let h = LAUNCHER_MAX_HEIGHT
            .min(self.h - self.bar - 120.0)
            .max(LAUNCHER_HEADER + 160.0);
        Rect::new(
            ((self.w - w) / 2.0).max(POPOVER_MARGIN),
            self.bar + ((self.h - self.bar - h) * 0.42).max(POPOVER_MARGIN),
            w,
            h,
        )
    }

    /// Partie qui défile, sous le champ de recherche.
    fn launcher_content(&self) -> Rect {
        let card = self.launcher();
        Rect::new(
            card.x,
            card.y + LAUNCHER_HEADER,
            card.w,
            card.h - LAUNCHER_HEADER - 10.0,
        )
    }

    /// Titres, liens, traits et cases des rubriques, à leur place (défilement compris).
    fn launcher_items(&self, launcher: &LauncherView) -> Vec<LauncherItem> {
        let (mut items, height) = launcher_flow(launcher);
        let content = self.launcher_content();
        let scroll = (launcher.scroll as f64).min((height - content.h).max(0.0));
        let (dx, dy) = (content.x + LAUNCHER_PAD, content.y - scroll);
        for item in &mut items {
            let rect = match item {
                LauncherItem::Title { rect, .. } | LauncherItem::Tile { rect, .. } => rect,
            };
            rect.x += dx;
            rect.y += dy;
        }
        items
    }

    /// Menu d'une icône du Dock, au-dessus d'elle (sans sortir de l'écran).
    fn dock_menu(&self, menu: &DockMenuView) -> Rect {
        let (w, h) = (210.0, 12.0 + menu.entries.len() as f64 * 32.0);
        if menu.at_pointer {
            let anchor = (menu.x.round() as i32, menu.bottom.round() as i32);
            return Rect {
                w,
                ..self.desktop_menu(anchor, menu.entries.len())
            };
        }
        let x = (menu.x - w / 2.0).clamp(POPOVER_MARGIN, (self.w - w - POPOVER_MARGIN).max(POPOVER_MARGIN));
        Rect::new(x, (menu.bottom - h - 10.0).max(self.bar + POPOVER_MARGIN), w, h)
    }

    fn dock_menu_row(&self, menu: &DockMenuView, index: usize) -> Rect {
        let card = self.dock_menu(menu);
        Rect::new(
            card.x + 6.0,
            card.y + 6.0 + index as f64 * 32.0,
            card.w - 12.0,
            32.0,
        )
    }

    /// Panneau Super+V : position personnalisée ou emplacement par défaut à droite, sous la
    /// barre.
    fn clipboard(&self) -> Rect {
        let h = clipboard_height(self.h);
        if let Some((x, y)) = self.clipboard_position {
            return Rect::new(x as f64, y as f64, CLIPBOARD_WIDTH, h);
        }
        Rect::new(
            (self.w - POPOVER_MARGIN - 10.0 - CLIPBOARD_WIDTH).max(POPOVER_MARGIN),
            self.bar + POPOVER_MARGIN + 8.0,
            CLIPBOARD_WIDTH,
            h,
        )
    }

    /// Haut du panneau, par où on le déplace.
    fn clipboard_header(&self) -> Rect {
        let card = self.clipboard();
        Rect::new(card.x, card.y, card.w, 58.0)
    }

    fn clipboard_theme(&self) -> Rect {
        let clear = self.clipboard_clear();
        Rect::new(clear.x - 6.0 - 32.0, clear.y, 32.0, 32.0)
    }

    fn clipboard_clear(&self) -> Rect {
        let card = self.clipboard();
        Rect::new(
            card.right() - 18.0 - CLIPBOARD_CLEAR,
            card.y + 18.0,
            CLIPBOARD_CLEAR,
            32.0,
        )
    }

    fn clipboard_search(&self) -> Rect {
        let card = self.clipboard();
        Rect::new(card.x + 18.0, card.y + 62.0, card.w - 36.0, 38.0)
    }

    fn clipboard_filter(&self, slot: usize) -> Rect {
        let card = self.clipboard();
        let x = card.x + 18.0 + CLIPBOARD_FILTERS[..slot].iter().map(|w| w + 6.0).sum::<f64>();
        Rect::new(x, card.y + 112.0, CLIPBOARD_FILTERS[slot], 28.0)
    }

    /// Zone où la liste défile, entre le filet de l'en-tête et le pied.
    fn clipboard_clip(&self) -> Rect {
        let card = self.clipboard();
        Rect::new(
            card.x,
            card.y + CLIPBOARD_HEAD,
            card.w,
            card.h - CLIPBOARD_HEAD - CLIPBOARD_FOOT,
        )
    }

    /// Haut de la liste (ordonnée 0 des lignes) et sa largeur.
    fn clipboard_list(&self) -> Rect {
        let clip = self.clipboard_clip();
        Rect::new(
            clip.x + 10.0,
            clip.y + CLIPBOARD_LIST_PAD.0,
            clip.w - 20.0,
            clip.h - CLIPBOARD_LIST_PAD.0 - CLIPBOARD_LIST_PAD.1,
        )
    }

    fn clipboard_row(&self, row: &ClipRow) -> Rect {
        let list = self.clipboard_list();
        Rect::new(list.x, list.y + row.y, list.w, row.h)
    }

    fn clipboard_toast(&self) -> Rect {
        let card = self.clipboard();
        Rect::new(card.x + 20.0, card.bottom() - 60.0 - 48.0, card.w - 40.0, 48.0)
    }

    fn clipboard_undo(&self) -> Rect {
        let toast = self.clipboard_toast();
        Rect::new(toast.right() - 10.0 - 80.0, toast.y + 10.0, 80.0, 28.0)
    }
}

/// Épingle et croix d'une ligne du presse-papiers, à sa droite.
fn clipboard_delete(row: Rect) -> Rect {
    Rect::new(row.right() - 8.0 - 30.0, row.center().1 - 15.0, 30.0, 30.0)
}

fn clipboard_pin(row: Rect) -> Rect {
    let delete = clipboard_delete(row);
    Rect::new(delete.x - 2.0 - 30.0, delete.y, 30.0, 30.0)
}

/// Couleurs de la planche « Presse-papiers », claires ou sombres.
struct ClipTokens {
    fg: Color,
    fg2: Color,
    fg3: Color,
    icon: Color,
    panel: Color,
    line: Color,
    chip: Color,
    field: Color,
    field_ring: Color,
    ring: Color,
    tile: Color,
    kbd_ring: Color,
    toast_bg: Color,
    toast_fg: Color,
    toast_action: Color,
    filter_on: Color,
    filter_on_fg: Color,
}

fn clip_tokens() -> ClipTokens {
    ClipTokens {
        fg: palette().text,
        fg2: palette().text_secondary,
        fg3: palette().window_title_inactive,
        icon: palette().window_title_active,
        panel: palette().window_background,
        line: palette().separator,
        chip: palette().control_hover,
        field: themed(palette().header_background, palette().sidebar_background),
        field_ring: palette().separator,
        ring: themed(Color::BLACK.with_alpha(20), Color::WHITE.with_alpha(15)),
        tile: themed(palette().sidebar_background, palette().header_background),
        kbd_ring: palette().track,
        toast_bg: palette().inverse,
        toast_fg: palette().inverse_text,
        toast_action: palette().inverse_action,
        filter_on: palette().inverse,
        filter_on_fg: palette().inverse_text,
    }
}

/// Texte de `css` pixels centré dans `rect`.
fn centered_text(canvas: &mut Canvas, font: &Font, text: &str, rect: Rect, css: f64, color: Color) {
    let width = canvas.css_width(font, text, css);
    let (cx, cy) = rect.center();
    canvas.text(font, text, cx - width / 2.0, cy - css * 0.6, css, color);
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    fn expand(self, by: f64) -> Self {
        Self::new(self.x - by, self.y - by, self.w + 2.0 * by, self.h + 2.0 * by)
    }

    fn right(self) -> f64 {
        self.x + self.w
    }

    fn bottom(self) -> f64 {
        self.y + self.h
    }

    fn center(self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    fn contains(self, p: Point<f64, Logical>) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }
}

/// Tampon de dessin BGRA prémultiplié ; les coordonnées des méthodes sont logiques.
struct Canvas<'a> {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    scale: f32,
    bold: &'a Font,
    icons: &'a IconCache,
}

impl Canvas<'_> {
    fn fill(&mut self, rect: Rect, radius: f32, color: Color) {
        let s = self.scale;
        let (x, y, w, h) = (
            rect.x as f32 * s,
            rect.y as f32 * s,
            rect.w as f32 * s,
            rect.h as f32 * s,
        );
        let radius = (radius * s).clamp(0.0, w.min(h) * 0.5);
        let left = (x - 1.0).floor().max(0.0) as u32;
        let top = (y - 1.0).floor().max(0.0) as u32;
        let right = (x + w + 1.0).ceil().clamp(0.0, self.width as f32) as u32;
        let bottom = (y + h + 1.0).ceil().clamp(0.0, self.height as f32) as u32;
        if radius <= 0.0 {
            // Rectangle droit (voiles plein écran) : la part de chaque pixel recouverte
            // suffit, sans calcul de distance à des coins arrondis.
            let cover = |p: u32, start: f32, end: f32| {
                ((p as f32 + 1.0).min(end) - (p as f32).max(start)).clamp(0.0, 1.0)
            };
            // Intérieur entièrement couvert : une boucle serrée, sans calcul par pixel.
            let inner = (
                x.ceil().max(0.0) as u32,
                ((x + w).floor().max(0.0) as u32).min(self.width),
            );
            let alpha = u32::from(color.a);
            let inverse = 255 - alpha;
            let source = [color.b, color.g, color.r].map(|channel| u32::from(channel) * alpha / 255);
            for py in top..bottom {
                let row = cover(py, y, y + h);
                if row <= 0.0 {
                    continue;
                }
                if row < 1.0 || inner.0 >= inner.1 {
                    for px in left..right {
                        self.blend(px, py, color, row * cover(px, x, x + w));
                    }
                    continue;
                }
                for px in (left..inner.0).chain(inner.1..right) {
                    self.blend(px, py, color, cover(px, x, x + w));
                }
                let start = ((py * self.width + inner.0) * 4) as usize;
                let end = ((py * self.width + inner.1) * 4) as usize;
                for pixel in self.pixels[start..end].chunks_exact_mut(4) {
                    for (channel, value) in source.iter().enumerate() {
                        pixel[channel] = (value + u32::from(pixel[channel]) * inverse / 255).min(255) as u8;
                    }
                    pixel[3] = (alpha + u32::from(pixel[3]) * inverse / 255).min(255) as u8;
                }
            }
            return;
        }
        for py in top..bottom {
            for px in left..right {
                let qx = (px as f32 + 0.5 - (x + w * 0.5)).abs() - (w * 0.5 - radius);
                let qy = (py as f32 + 0.5 - (y + h * 0.5)).abs() - (h * 0.5 - radius);
                let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                self.blend(px, py, color, (0.5 - distance).clamp(0.0, 1.0));
            }
        }
    }

    fn stroke(&mut self, rect: Rect, radius: f32, color: Color, width: f64) {
        let s = self.scale as f64;
        let half = width * 0.5;
        let left = ((rect.x - half) * s).floor().max(0.0) as u32;
        let top = ((rect.y - half) * s).floor().max(0.0) as u32;
        let right = ((rect.right() + half) * s).ceil().min(self.width as f64) as u32;
        let bottom = ((rect.bottom() + half) * s).ceil().min(self.height as f64) as u32;
        let radius = (radius as f64 * s).clamp(0.0, rect.w.min(rect.h) * s * 0.5);
        for y in top..bottom {
            for x in left..right {
                let px = x as f64 + 0.5;
                let py = y as f64 + 0.5;
                let qx = (px - (rect.x + rect.w * 0.5) * s).abs() - (rect.w * s * 0.5 - radius);
                let qy = (py - (rect.y + rect.h * 0.5) * s).abs() - (rect.h * s * 0.5 - radius);
                let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                let coverage = (half * s + 0.5 - distance.abs()).clamp(0.0, 1.0) as f32;
                if coverage > 0.0 {
                    self.blend(x, y, color, coverage);
                }
            }
        }
    }

    fn line(&mut self, start: (f64, f64), end: (f64, f64), width: f64, color: Color) {
        let s = self.scale as f64;
        let (x1, y1) = (start.0 * s, start.1 * s);
        let (x2, y2) = (end.0 * s, end.1 * s);
        let radius = width * s * 0.5;
        let left = (x1.min(x2) - radius - 1.0).floor().max(0.0) as u32;
        let top = (y1.min(y2) - radius - 1.0).floor().max(0.0) as u32;
        let right = (x1.max(x2) + radius + 1.0).ceil().min(self.width as f64) as u32;
        let bottom = (y1.max(y2) + radius + 1.0).ceil().min(self.height as f64) as u32;
        let (dx, dy) = (x2 - x1, y2 - y1);
        let length_squared = dx * dx + dy * dy;
        for y in top..bottom {
            for x in left..right {
                let px = x as f64 + 0.5 - x1;
                let py = y as f64 + 0.5 - y1;
                let t = if length_squared == 0.0 {
                    0.0
                } else {
                    ((px * dx + py * dy) / length_squared).clamp(0.0, 1.0)
                };
                let distance = (px - dx * t).hypot(py - dy * t);
                let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0) as f32;
                if coverage > 0.0 {
                    self.blend(x, y, color, coverage);
                }
            }
        }
    }

    fn polygon(&mut self, points: &[(f64, f64)], color: Color) {
        if points.len() < 3 {
            return;
        }
        let s = self.scale as f64;
        let min_y = points.iter().map(|point| point.1).fold(f64::INFINITY, f64::min);
        let max_y = points
            .iter()
            .map(|point| point.1)
            .fold(f64::NEG_INFINITY, f64::max);
        let top = (min_y * s).floor().max(0.0) as u32;
        let bottom = (max_y * s).ceil().min(self.height as f64) as u32;
        for y in top..bottom {
            let scan = y as f64 + 0.5;
            let mut intersections = Vec::new();
            for index in 0..points.len() {
                let (x1, y1) = points[index];
                let (x2, y2) = points[(index + 1) % points.len()];
                if (y1 <= scan / s && y2 > scan / s) || (y2 <= scan / s && y1 > scan / s) {
                    let y_logical = scan / s;
                    intersections.push((x1 + (y_logical - y1) * (x2 - x1) / (y2 - y1)) * s);
                }
            }
            intersections.sort_by(f64::total_cmp);
            for pair in intersections.chunks_exact(2) {
                let left = pair[0].ceil().max(0.0) as u32;
                let right = pair[1].ceil().min(self.width as f64) as u32;
                for x in left..right {
                    self.blend(x, y, color, 1.0);
                }
            }
        }
    }

    /// Débit réseau des dernières mesures : pas fixe, les plus récentes à droite, aire
    /// douce sous la courbe ; une ligne de base discrète tant qu'il y a peu de mesures.
    fn network_chart(&mut self, rect: Rect, samples: &[(u64, u64)]) {
        const CAPACITY: usize = 32;
        // En dessous de 64 Kio/s, l'échelle ne zoome pas : un réseau au repos reste plat.
        const FLOOR: u64 = 64;
        self.line(
            (rect.x, rect.bottom()),
            (rect.right(), rect.bottom()),
            1.0,
            Color::WHITE.with_alpha(30),
        );
        let samples = &samples[samples.len().saturating_sub(CAPACITY)..];
        if samples.len() < 2 {
            return;
        }
        let max = samples
            .iter()
            .flat_map(|(download, upload)| [*download, *upload])
            .max()
            .unwrap_or(0)
            .max(FLOOR) as f64;
        let step = rect.w / (CAPACITY - 1) as f64;
        let first = CAPACITY - samples.len();
        let point = |index: usize, value: u64| {
            let x = rect.x + step * (first + index) as f64;
            let y = rect.bottom() - (value as f64 / max * (rect.h - 4.0)).clamp(0.0, rect.h - 2.0);
            (x, y)
        };
        for (pick, color) in [(0_usize, Color::rgb(0x6CB8FF)), (1_usize, Color::rgb(0x3CC7D1))] {
            let value = |sample: &(u64, u64)| if pick == 0 { sample.0 } else { sample.1 };
            let mut area: Vec<(f64, f64)> = samples
                .iter()
                .enumerate()
                .map(|(index, sample)| point(index, value(sample)))
                .collect();
            let (last_x, first_x) = (area[area.len() - 1].0, area[0].0);
            area.push((last_x, rect.bottom()));
            area.push((first_x, rect.bottom()));
            self.polygon(&area, color.with_alpha(40));
            for index in 1..samples.len() {
                self.line(
                    point(index - 1, value(&samples[index - 1])),
                    point(index, value(&samples[index])),
                    1.6,
                    color,
                );
            }
        }
    }

    fn fit_text(&self, font: &Font, text: &str, size: f32, max_width: f64) -> String {
        if self.text_width(font, text, size) <= max_width {
            return text.to_owned();
        }
        let mut output = String::new();
        for character in text.chars() {
            let mut candidate = output.clone();
            candidate.push(character);
            candidate.push('…');
            if self.text_width(font, &candidate, size) > max_width {
                break;
            }
            output.push(character);
        }
        output.push('…');
        output
    }

    fn blend(&mut self, x: u32, y: u32, color: Color, coverage: f32) {
        let alpha = (color.a as f32 * coverage).round().clamp(0.0, 255.0) as u32;
        if alpha == 0 {
            return;
        }
        let inverse = 255 - alpha;
        let offset = ((y * self.width + x) * 4) as usize;
        let source = [color.b, color.g, color.r].map(|channel| channel as u32 * alpha / 255);
        let pixel = &mut self.pixels[offset..offset + 4];
        for (index, channel) in source.into_iter().enumerate() {
            pixel[index] = (channel + pixel[index] as u32 * inverse / 255).min(255) as u8;
        }
        pixel[3] = (alpha + pixel[3] as u32 * inverse / 255).min(255) as u8;
    }

    /// Verre liquide posé sur le fond d'écran `backdrop` : limpide au milieu, il courbe le
    /// fond comme une lentille sur ses bords, avive les couleurs, et accroche la lumière
    /// sur un liseré. À réserver à ce qui n'a que le fond d'écran derrière lui.
    /// Copie des lignes de pixels entre les ordonnées logiques `top` et `bottom`, à
    /// remettre avec [`Canvas::restore_rows`].
    fn save_rows(&self, top: f64, bottom: f64) -> (usize, Vec<u8>) {
        let s = self.scale as f64;
        let row = self.width as usize * 4;
        let first = ((top * s).floor().max(0.0) as usize).min(self.height as usize);
        let last = ((bottom * s).ceil().max(0.0) as usize)
            .min(self.height as usize)
            .max(first);
        (first * row, self.pixels[first * row..last * row].to_vec())
    }

    fn restore_rows(&mut self, (offset, saved): (usize, Vec<u8>)) {
        self.pixels[offset..offset + saved.len()].copy_from_slice(&saved);
    }

    /// Pose le verre `frost` : en entier par-dessus ce qui est dessous (`rows` vide), ou
    /// seulement entre les ordonnées logiques `rows`, en remplaçant ce qui y était dessiné
    /// (sauf sur le bord arrondi, mélangé).
    fn blit_frost(&mut self, frost: &Frost, rows: Option<(f64, f64)>) {
        let s = self.scale as f64;
        let (from, to) = match rows {
            Some((top, bottom)) => (
                ((top * s).floor().max(frost.top as f64) as u32).min(frost.top + frost.height),
                ((bottom * s).ceil().max(0.0) as u32).min(frost.top + frost.height),
            ),
            None => (frost.top, frost.top + frost.height),
        };
        for y in from..to {
            for x in 0..frost.width {
                let source = ((y - frost.top) * frost.width + x) as usize * 4;
                let pixel = &frost.pixels[source..source + 4];
                let alpha = pixel[3] as u32;
                if alpha == 0 {
                    continue;
                }
                let target = ((y * self.width + frost.left + x) * 4) as usize;
                let below = &mut self.pixels[target..target + 4];
                if alpha == 255 {
                    below.copy_from_slice(pixel);
                } else {
                    for channel in 0..4 {
                        below[channel] = (pixel[channel] as u32 + below[channel] as u32 * (255 - alpha) / 255)
                            .min(255) as u8;
                    }
                }
            }
        }
    }

    fn icon(&mut self, name: &'static str, center: (f64, f64), size: f64, style: Style) {
        let s = self.scale;
        let side = size as f32 * s;
        self.icons.draw(
            &mut self.pixels,
            self.width,
            self.height,
            name,
            center.0 as f32 * s - side / 2.0,
            center.1 as f32 * s - side / 2.0,
            side,
            style,
        );
    }

    /// Interrupteur ; `None` : grisé, indisponible.
    /// Icône d'une application installée ; faux si le fichier est illisible.
    /// Icône d'une application, ou une icône neutre si la sienne manque ou est illisible.
    fn app_icon(&mut self, icon: &crate::desktop::apps::AppIcon, center: (f64, f64), size: f64) {
        let drawn = match icon {
            crate::desktop::apps::AppIcon::Builtin(name) => {
                self.icon(name, center, size, Style::Color);
                true
            }
            crate::desktop::apps::AppIcon::File(path) => self.icon_file(path, center, size),
            crate::desktop::apps::AppIcon::Missing => false,
        };
        if !drawn {
            self.icon("app-search", center, size, Style::Color);
        }
    }

    fn icon_file(&mut self, path: &std::path::Path, center: (f64, f64), size: f64) -> bool {
        let s = self.scale;
        let side = size as f32 * s;
        self.icons.draw_file(
            &mut self.pixels,
            self.width,
            self.height,
            path,
            center.0 as f32 * s - side / 2.0,
            center.1 as f32 * s - side / 2.0,
            side,
        )
    }

    /// Vignette qui couvre `rect` (recadrée au centre), coins arrondis de `radius`.
    fn image_cover(&mut self, rect: Rect, thumbnail: &Thumbnail, radius: f64) {
        let s = self.scale as f64;
        let (w, h) = (rect.w * s, rect.h * s);
        let ratio = (w / thumbnail.width as f64).max(h / thumbnail.height as f64);
        let (offset_x, offset_y) = (
            (thumbnail.width as f64 * ratio - w) / 2.0,
            (thumbnail.height as f64 * ratio - h) / 2.0,
        );
        let r = radius * s;
        let (x0, y0) = ((rect.x * s).round() as i64, (rect.y * s).round() as i64);
        for y in 0..h.round() as i64 {
            for x in 0..w.round() as i64 {
                let (px, py) = (x0 + x, y0 + y);
                if px < 0 || py < 0 || px >= self.width as i64 || py >= self.height as i64 {
                    continue;
                }
                // Coin arrondi : couverture d'après la distance au coin.
                let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
                let qx = (cx - w / 2.0).abs() - (w / 2.0 - r);
                let qy = (cy - h / 2.0).abs() - (h / 2.0 - r);
                let distance = qx.max(0.0).hypot(qy.max(0.0)) - r;
                let coverage = (0.5 - distance).clamp(0.0, 1.0);
                if coverage == 0.0 {
                    continue;
                }
                let sx = ((cx + offset_x) / ratio) as u32;
                let sy = ((cy + offset_y) / ratio) as u32;
                let i = ((sy.min(thumbnail.height - 1) * thumbnail.width + sx.min(thumbnail.width - 1)) * 4)
                    as usize;
                let source = &thumbnail.pixels[i..i + 4];
                let offset = ((py as u32 * self.width + px as u32) * 4) as usize;
                let alpha = (source[3] as f64 * coverage) as u32;
                let inverse = 255 - alpha;
                let pixel = &mut self.pixels[offset..offset + 4];
                for channel in 0..4 {
                    let value = (source[channel] as f64 * coverage) as u32;
                    pixel[channel] = (value + pixel[channel] as u32 * inverse / 255).min(255) as u8;
                }
            }
        }
    }

    /// Panneau de verre liquide comme le Dock : intérieur translucide éclairé par le haut,
    /// liseré qui accroche la lumière, contour fin et ombre douce tout autour. Des fenêtres
    /// peuvent être dessous : il ne réfracte rien et reste presque opaque pour rester lisible.
    /// Panneau système : fond presque opaque et ombre flottante.
    fn glass(&mut self, rect: Rect, radius: f32) {
        self.panel_shadow(rect, radius);
        self.fill(rect, radius, panel_fill());
    }

    /// Ombre flottante 0 12 32, noir 30 %, autour de `rect`.
    fn panel_shadow(&mut self, rect: Rect, radius: f32) {
        const OFFSET_Y: f32 = 12.0;
        const BLUR: f32 = 32.0;
        const ALPHA: f32 = 0.30;
        let s = self.scale;
        let (x, y, w, h) = (
            rect.x as f32 * s,
            (rect.y as f32 + OFFSET_Y) * s,
            rect.w as f32 * s,
            rect.h as f32 * s,
        );
        let r = (radius * s).min(w.min(h) * 0.5);
        let spread = BLUR * s;
        let left = (x - spread).floor().max(0.0) as u32;
        let top = (y - spread).floor().max(0.0) as u32;
        let right = (x + w + spread).ceil().clamp(0.0, self.width as f32) as u32;
        let bottom = (y + h + spread).ceil().clamp(0.0, self.height as f32) as u32;
        for py in top..bottom {
            for px in left..right {
                let qx = (px as f32 + 0.5 - (x + w * 0.5)).abs() - (w * 0.5 - r);
                let qy = (py as f32 + 0.5 - (y + h * 0.5)).abs() - (h * 0.5 - r);
                let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
                let t = ((distance + spread * 0.5) / spread).clamp(0.0, 1.0);
                let amount = (1.0 - t * t * (3.0 - 2.0 * t)) * ALPHA;
                if amount > 0.0 {
                    self.blend(px, py, Color::BLACK, amount);
                }
            }
        }
    }

    /// Curseur épais : partie réglée blanche, bouton rond à son extrémité et pictogramme
    /// au début de la piste.
    /// Curseur : piste de 6 px, partie réglée dans la couleur d'accent, bouton blanc de 20 px.
    fn slider(&mut self, track: Rect, value: u8) {
        let center_y = track.y + track.h / 2.0;
        let rail = Rect::new(track.x, center_y - 3.0, track.w, 6.0);
        self.fill(rail, 3.0, palette().track);
        let knob = track.h;
        let x = track.x + (track.w - knob) * f64::from(value) / 100.0;
        self.fill(
            Rect::new(rail.x, rail.y, x + knob / 2.0 - rail.x, rail.h),
            3.0,
            accent(),
        );
        let shadow = Rect::new(x, track.y + 1.0, knob, knob);
        self.fill(shadow, (knob / 2.0) as f32, Color::BLACK.with_alpha(60));
        self.fill(
            Rect::new(x, track.y, knob, knob),
            (knob / 2.0) as f32,
            Color::WHITE,
        );
    }

    fn button(&mut self, rect: Rect, text: &str, background: Color, ink: Color) {
        self.fill(rect, 8.0, background);
        let font = self.bold;
        let size = px(13.0);
        let width = self.text_width(font, text, size);
        let y = rect.y + (rect.h - 13.0) / 2.0 - 2.0;
        self.label(font, text, rect.x + (rect.w - width) / 2.0, y, size, ink);
    }

    /// Texte de la barre, de la couleur `ink`.
    fn bar_label(&mut self, font: &Font, text: &str, x: f64, size: f32, ink: Color, bar: f64) {
        let y = (bar - size as f64 * 1.22) / 2.0 - 1.0;
        self.label(font, text, x, y, size, ink);
    }

    /// Largeur d'un texte de `css` pixels.
    fn css_width(&self, font: &Font, text: &str, css: f64) -> f64 {
        self.text_width(font, text, px(css as f32))
    }

    /// Texte de `css` pixels dont la ligne, de hauteur normale, commence en `top`.
    fn text(&mut self, font: &Font, text: &str, x: f64, top: f64, css: f64, color: Color) {
        self.label(font, text, x, top - css * 0.03, px(css as f32), color);
    }

    fn tracked_width(&self, font: &Font, text: &str, css: f64, tracking: f64) -> f64 {
        self.css_width(font, text, css) + text.chars().count() as f64 * tracking * css
    }

    /// Texte espacé (ou resserré) de `tracking` fois sa taille.
    #[allow(clippy::too_many_arguments)] // Police, texte, position, taille, espacement, encre.
    fn tracked(&mut self, font: &Font, text: &str, x: f64, top: f64, css: f64, tracking: f64, color: Color) {
        let mut cursor = x;
        let mut glyph = [0_u8; 4];
        for character in text.chars() {
            let piece: &str = character.encode_utf8(&mut glyph);
            self.text(font, piece, cursor, top, css, color);
            cursor += self.css_width(font, piece, css) + tracking * css;
        }
    }

    fn text_width(&self, font: &Font, text: &str, size: f32) -> f64 {
        text.chars()
            .map(|character| font.metrics(character, size * 1.22).advance_width as f64)
            .sum()
    }

    /// Écrit `text` dont la boîte commence en (`x`, `y`) ; `size` est la taille de base.
    fn label(&mut self, font: &Font, text: &str, x: f64, y: f64, size: f32, color: Color) {
        let s = self.scale;
        let mut cursor = x as f32 * s;
        let font_size = size * s * 1.22;
        let baseline = ((y as f32 + size * 1.22) * s).round() as i32;
        for character in text.chars() {
            let (metrics, coverage) = font.rasterize(character, font_size);
            for cy in 0..metrics.height {
                for cx in 0..metrics.width {
                    let px = cursor.round() as i32 + metrics.xmin + cx as i32;
                    let py = baseline - metrics.ymin - metrics.height as i32 + cy as i32;
                    if px >= 0 && py >= 0 && px < self.width as i32 && py < self.height as i32 {
                        let value = coverage[cy * metrics.width + cx] as f32 / 255.0;
                        self.blend(px as u32, py as u32, color, value);
                    }
                }
            }
            cursor += metrics.advance_width;
        }
    }
}

fn wrap_dashboard_text(font: &Font, text: &str, size: f32, max_width: f64) -> Vec<String> {
    let width = |text: &str| {
        text.chars()
            .map(|character| font.metrics(character, size * 1.22).advance_width as f64)
            .sum::<f64>()
    };
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if width(&candidate) <= max_width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            for character in word.chars() {
                let mut candidate = line.clone();
                candidate.push(character);
                if !line.is_empty() && width(&candidate) > max_width {
                    lines.push(std::mem::take(&mut line));
                }
                line.push(character);
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

/// Date et heure locales, à la française : « lun. 26 oct.  18:05 ».
/// Horloge selon le réglage, à partir du texte complet (« lun. 2 nov.  17:24 »).
fn clock_text(style: ClockStyle, full: String) -> String {
    match style {
        ClockStyle::Full => full,
        ClockStyle::Date => full
            .split_once(' ')
            .map_or(full.clone(), |(_, rest)| rest.to_owned()),
        ClockStyle::Time => full.rsplit(' ').next().unwrap_or_default().to_owned(),
    }
}

fn clock_label(now: SystemTime, offset: i64) -> String {
    const DAYS: [&str; 7] = ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."];
    const MONTHS: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc.",
    ];
    let seconds = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64 + offset;
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (_, month, day) = civil_from_days(days);
    let weekday = (days + 4).rem_euclid(7) as usize;
    format!(
        "{} {} {}  {:02}:{:02}",
        DAYS[weekday],
        day,
        MONTHS[month as usize - 1],
        time / 3600,
        (time / 60) % 60
    )
}

/// Heure locale : « 18:30 ».
pub fn hour_label(at: SystemTime) -> String {
    let seconds = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64 + local_offset();
    let time = seconds.rem_euclid(86_400);
    format!("{:02}:{:02}", time / 3600, (time / 60) % 60)
}

/// Date et heure locales pour un nom de fichier : « 2026-11-02 à 17.34.12 ».
pub fn file_timestamp(now: SystemTime) -> String {
    format_file_timestamp(now, local_offset())
}

fn format_file_timestamp(now: SystemTime, offset: i64) -> String {
    let seconds = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64 + offset;
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let time = seconds.rem_euclid(86_400);
    format!(
        "{year}-{month:02}-{day:02} à {:02}.{:02}.{:02}",
        time / 3600,
        (time / 60) % 60,
        time % 60
    )
}

/// Mesures des notifications, du calendrier et de la zone cliquable de l'horloge.
const NOTIFICATION_HEIGHT: f64 = 84.0;
const NOTIFICATION_HEADER: f64 = 48.0;
const CLOCK_WIDTH: f64 = 170.0;
const CALENDAR_WIDTH: f64 = 400.0;
const CALENDAR_ROW: f64 = 36.0;
/// Haut du panneau jusqu'au filet des notifications : titre, jours, six semaines.
const CALENDAR_BODY: f64 = 52.0 + 22.0 + 6.0 * CALENDAR_ROW + 12.0;

/// Marge intérieure du sélecteur Alt+Tab, écart et taille maximale d'une case (icône de 96 px).
const SWITCHER_PADDING: f64 = 18.0;
const SWITCHER_GAP: f64 = 8.0;
const SWITCHER_TILE: f64 = 120.0;

/// Vignettes des espaces dans la vue d'ensemble : taille, écart et marge sous la barre.
const SPACE_THUMB: (f64, f64) = (110.0, 68.0);
const SPACE_GAP: f64 = 12.0;

/// Vignette d'un espace : la rangée est centrée, sous la barre.
fn overview_space_rect(width: f64, bar: f64, count: usize, index: usize) -> Rect {
    let total = count as f64 * SPACE_THUMB.0 + count.saturating_sub(1) as f64 * SPACE_GAP;
    Rect::new(
        (width - total) / 2.0 + index as f64 * (SPACE_THUMB.0 + SPACE_GAP),
        bar + 22.0,
        SPACE_THUMB.0,
        SPACE_THUMB.1,
    )
}

/// Espace de travail sous le pointeur, dans la bande de la vue d'ensemble.
pub fn overview_space_at(
    output: Rectangle<i32, Logical>,
    view: &DesktopView,
    pos: Point<f64, Logical>,
) -> Option<usize> {
    let spaces = &view.overview.as_ref()?.spaces;
    let pos = Point::from((pos.x - output.loc.x as f64, pos.y - output.loc.y as f64));
    spaces
        .iter()
        .enumerate()
        .find(|(index, _)| {
            overview_space_rect(output.size.w as f64, view.bar_height as f64, spaces.len(), *index)
                .contains(pos)
        })
        .map(|(_, space)| space.page)
}

/// Carte du sélecteur et côté d'une case : les cases rétrécissent pour tenir dans l'écran.
fn switcher_card(w: f64, h: f64, count: usize) -> (Rect, f64) {
    let count = count.max(1) as f64;
    let gaps = (count - 1.0) * SWITCHER_GAP;
    let tile = SWITCHER_TILE.min((w - 40.0 - 2.0 * SWITCHER_PADDING - gaps) / count);
    let card_w = tile * count + gaps + 2.0 * SWITCHER_PADDING;
    let card_h = tile + 2.0 * SWITCHER_PADDING + 30.0;
    (
        Rect::new((w - card_w) / 2.0, (h - card_h) / 2.0, card_w, card_h),
        tile,
    )
}

fn clock_snapshot(now: SystemTime) -> ClockSnapshot {
    let seconds = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64 + local_offset();
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let time = seconds.rem_euclid(86_400);
    ClockSnapshot {
        year,
        month,
        day,
        weekday: (days + 4).rem_euclid(7) as usize,
        hour: (time / 3600) as u32,
        minute: ((time / 60) % 60) as u32,
    }
}

fn shift_month(year: i64, month: u32, offset: i32) -> (i64, u32) {
    let index = year * 12 + i64::from(month) - 1 + i64::from(offset);
    (index.div_euclid(12), index.rem_euclid(12) as u32 + 1)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn weekday_for(year: i64, month: u32, day: u32) -> &'static str {
    ["Dim.", "Lun.", "Mar.", "Mer.", "Jeu.", "Ven.", "Sam."]
        [(days_from_civil(year, month, day) + 4).rem_euclid(7) as usize]
}

fn note_timestamp(updated: u64, now: ClockSnapshot) -> String {
    if updated == 0 {
        return "—".to_owned();
    }
    let local_seconds = updated as i64 + local_offset();
    let note_days = local_seconds.div_euclid(86_400);
    let now_days = days_from_civil(now.year, now.month, now.day);
    let (year, month, day) = civil_from_days(note_days);
    match now_days - note_days {
        0 => format!(
            "{:02}:{:02}",
            local_seconds.rem_euclid(86_400) / 3600,
            (local_seconds / 60).rem_euclid(60)
        ),
        1 => "Hier".to_owned(),
        2..=6 => weekday_for(year, month, day).to_owned(),
        _ => {
            let months = [
                "jan.", "fév.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
                "déc.",
            ];
            format!("{day} {}", months[month.saturating_sub(1) as usize])
        }
    }
}

fn network_rate(kib_per_second: u64) -> (String, &'static str) {
    if kib_per_second >= 1024 {
        (format!("{:.1}", kib_per_second as f64 / 1024.0), "Mo")
    } else {
        (kib_per_second.to_string(), "Ko")
    }
}

fn cpu_counters() -> Option<(u64, u64)> {
    let line = fs::read_to_string("/proc/stat").ok()?.lines().next()?.to_owned();
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let values: Vec<u64> = fields.map(str::parse).collect::<Result<_, _>>().ok()?;
    let total = values.iter().sum();
    let idle = values.get(3)?.saturating_add(*values.get(4).unwrap_or(&0));
    Some((total, idle))
}

fn network_counters() -> Option<(u64, u64)> {
    let text = fs::read_to_string("/proc/net/dev").ok()?;
    text.lines()
        .skip(2)
        .filter_map(|line| {
            let (interface, counters) = line.split_once(':')?;
            if interface.trim() == "lo" {
                return None;
            }
            let counters: Vec<u64> = counters
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            Some((*counters.first()?, *counters.get(8)?))
        })
        .fold(None, |totals, (received, sent)| {
            Some(totals.map_or((received, sent), |(rx, tx)| (rx + received, tx + sent)))
        })
}

fn memory_usage() -> Option<u8> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    let value = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
    };
    let total = value("MemTotal:")?;
    let available = value("MemAvailable:").or_else(|| value("MemFree:"))?;
    Some((total.saturating_sub(available).saturating_mul(100) / total.max(1)).min(100) as u8)
}

#[allow(unsafe_code)]
fn disk_usage() -> Option<u8> {
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `stats` points to writable storage for a `statvfs` result and `c"/"`
    // is a valid, NUL-terminated path.
    if unsafe { libc::statvfs(c"/".as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: `statvfs` returned success and initialized the whole output structure.
    let stats = unsafe { stats.assume_init() };
    let total = stats.f_blocks.checked_mul(stats.f_frsize)?;
    let available = stats.f_bavail.checked_mul(stats.f_frsize)?;
    if total == 0 {
        return None;
    }
    Some((total.saturating_sub(available).saturating_mul(100) / total).min(100) as u8)
}

/// Date (année, mois, jour) du jour numéro `days` depuis le 1er janvier 1970.
/// Algorithme de H. Hinnant (calendrier grégorien proleptique).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Décalage du fuseau local en secondes, lu une fois au démarrage avec `date +%z`
/// (un passage à l'heure d'été n'est pris en compte qu'au prochain démarrage).
/// Case du jour `day` dans le calendrier de la barre (lundi en première colonne).
fn calendar_cell(card: Rect, year: i64, month: u32, day: u32) -> Rect {
    let cell = (card.w - 36.0) / 7.0;
    let first = (days_from_civil(year, month, 1) + 3).rem_euclid(7);
    let slot = first + i64::from(day) - 1;
    Rect::new(
        card.x + 18.0 + cell * (slot % 7) as f64,
        card.y + 52.0 + 22.0 + CALENDAR_ROW * (slot / 7) as f64,
        cell,
        CALENDAR_ROW,
    )
}

/// Année et mois du jour, et ceux du widget Calendrier (`offset` mois plus loin).
pub(crate) fn calendar_months(offset: i32) -> ((i32, u32), (i32, u32)) {
    let clock = clock_snapshot(SystemTime::now());
    let (year, month) = shift_month(clock.year, clock.month, offset);
    ((clock.year as i32, clock.month), (year as i32, month))
}

/// Minutes écoulées depuis 1970, en heure locale (pour l'agenda du Calendrier).
pub(crate) fn local_minutes() -> Option<i64> {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    Some((seconds + local_offset()).div_euclid(60))
}

fn local_offset() -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|output| parse_offset(String::from_utf8_lossy(&output.stdout).trim()))
            .unwrap_or(0)
    })
}

/// « +0130 » → 5400 secondes.
fn parse_offset(text: &str) -> Option<i64> {
    let (sign, digits) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = digits[..2].parse().ok()?;
    let minutes: i64 = digits[2..].parse().ok()?;
    Some(sign * (hours * 3600 + minutes * 60))
}

/// Dossier sysfs de la première batterie, s'il y en a une.
fn battery_dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let mut batteries: Vec<_> = fs::read_dir("/sys/class/power_supply")
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("BAT"))
            })
            .collect();
        batteries.sort();
        batteries.into_iter().next()
    })
    .as_ref()
}

/// Dernier état lu de la batterie. Lire `capacity` interroge le contrôleur de la carte mère
/// et prend parfois 0,2 s : un fil s'en charge toutes les [`BATTERY_REFRESH`], pour que le
/// dessin de l'écran ne l'attende jamais. Seule la première lecture, au démarrage, est
/// faite sur place.
fn latest_battery() -> Option<Battery> {
    static LATEST: OnceLock<Arc<Mutex<Option<Battery>>>> = OnceLock::new();
    let latest = LATEST.get_or_init(|| {
        let latest = Arc::new(Mutex::new(read_battery()));
        if battery_dir().is_some() {
            let shared = Arc::clone(&latest);
            let _ = std::thread::Builder::new()
                .name("batterie".to_owned())
                .spawn(move || {
                    loop {
                        std::thread::sleep(BATTERY_REFRESH);
                        let battery = read_battery();
                        *shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = battery;
                    }
                });
        }
        latest
    });
    *latest.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn read_battery() -> Option<Battery> {
    let dir = battery_dir()?;
    let percent = fs::read_to_string(dir.join("capacity"))
        .ok()?
        .trim()
        .parse::<u8>()
        .ok()?;
    let status = fs::read_to_string(dir.join("status")).unwrap_or_default();
    Some(Battery {
        percent: percent.min(100),
        charging: matches!(status.trim(), "Charging" | "Full"),
    })
}

/// Coupe `text` avec « … » pour qu'il tienne dans `width` points.
fn ellipsize_to(canvas: &Canvas, font: &Font, text: &str, size: f32, width: f64) -> String {
    if canvas.text_width(font, text, size) <= width {
        return text.to_owned();
    }
    let mut short: String = text.to_owned();
    while !short.is_empty() && canvas.text_width(font, &format!("{short}…"), size) > width {
        short.pop();
    }
    format!("{}…", short.trim_end())
}

/// Coupe un texte trop long avec « … ».
fn ellipsize(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut short: String = text.chars().take(max.saturating_sub(1)).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accentuation_personnalisee_remplace_l_accent_de_la_palette_du_shell() {
        assert_eq!(
            accent_for(Palette::DARK, Accent::Purple.rgb()),
            Color::rgb(0x7C4DE8)
        );
        assert_eq!(accent_for(Palette::DARK, None), Palette::DARK.accent);
    }

    #[test]
    fn les_notes_du_tableau_de_bord_reviennent_a_la_ligne() {
        let font = Font::from_bytes(FONT_REGULAR, FontSettings::default()).expect("police régulière");
        let lines = wrap_dashboard_text(
            &font,
            "Mes notes\n\nClique ici pour écrire… Soiuyuyfchdxthjgvklbjnk;ojjgufvtszeusduyk",
            16.0,
            180.0,
        );
        assert!(lines.len() > 3);
        assert_eq!(lines[0], "Mes notes");
        assert_eq!(lines[1], "");
        assert!(lines.iter().all(|line| {
            line.chars()
                .map(|character| font.metrics(character, 16.0 * 1.22).advance_width as f64)
                .sum::<f64>()
                <= 180.0
        }));
    }

    #[test]
    fn les_mois_du_calendrier_gere_le_changement_d_annee_et_bissextiles() {
        assert_eq!(shift_month(2026, 1, -1), (2025, 12));
        assert_eq!(shift_month(2026, 12, 1), (2027, 1));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(civil_from_days(days_from_civil(2026, 10, 30)), (2026, 10, 30));
    }

    #[test]
    fn le_tableau_de_bord_repond_aux_clics_de_ses_modules() {
        let output = output();
        let mut desktop = view(None);
        desktop.bar_height = 34;
        desktop.dashboard.shown = true;
        desktop.dashboard.layout = default_layout();
        desktop.dashboard.tasks = vec![DashboardTaskView {
            text: "Rapport".to_owned(),
            done: false,
            priority: crate::desktop::widgets::Priority::High,
        }];
        desktop.dashboard.notes = vec![DashboardNoteView {
            title: "Projet web".to_owned(),
            preview: "Finir l'interface".to_owned(),
            updated: 0,
            lines: Vec::new(),
        }];
        let layout = Layout::new(output.size.w as f64, output.size.h as f64, &desktop);
        let cards = layout.dashboard_cards(&desktop.dashboard.layout);
        let (tasks, notes) = (cards[3], cards[4]);
        let click = |x: f64, y: f64| action_at(output, point(x, y), &desktop);
        assert_eq!(
            click(notes.x + 30.0, notes.y + 70.0),
            Some(DesktopAction::DashboardEditNote(0))
        );
        assert_eq!(
            click(notes.right() - 30.0, notes.y + 28.0),
            Some(DesktopAction::DashboardAddNote)
        );
        assert_eq!(
            click(notes.right() - 60.0, notes.y + 28.0),
            Some(DesktopAction::DashboardDeleteNote(0))
        );
        // Les onglets des feuilles, puis les tâches de la feuille affichée.
        assert_eq!(
            click(tasks.x + 30.0, tasks.y + 58.0),
            Some(DesktopAction::DashboardList(0))
        );
        assert_eq!(
            click(tasks.x + 18.0 + 124.0 + 6.0 + 14.0, tasks.y + 58.0),
            Some(DesktopAction::DashboardAddList)
        );
        assert_eq!(
            click(tasks.x + 25.0, tasks.y + 99.0),
            Some(DesktopAction::DashboardToggleTask(0))
        );
        assert_eq!(
            click(tasks.right() - 52.0, tasks.y + 99.0),
            Some(DesktopAction::DashboardCyclePriority(0))
        );
        assert_eq!(
            click(tasks.right() - 28.0, tasks.y + 99.0),
            Some(DesktopAction::DashboardDeleteTask(0))
        );
        assert_eq!(
            click(tasks.right() - 30.0, tasks.y + 28.0),
            Some(DesktopAction::DashboardAddTask)
        );
    }

    use crate::desktop::widgets::{WidgetEdges, default_layout};

    #[test]
    fn les_raccourcis_se_rangent_en_colonnes_a_droite_et_prennent_le_clic() {
        let shortcut = |name: &str| crate::desktop::shortcuts::ShortcutView {
            icon: crate::desktop::apps::AppIcon::Builtin("places-folder"),
            name: name.to_owned(),
            missing: false,
            cell: None,
        };
        let mut desktop = view(None);
        desktop.bar_height = 34;
        desktop.dock_height = 80;
        desktop.dashboard.shortcuts_shown = true;
        desktop.dashboard.icon_size = ferrite_system::shortcuts::ICON_SIZE;
        desktop.dashboard.shortcuts = (0..8)
            .map(|index| shortcut(&format!("Dossier {index}")))
            .collect();
        let layout = Layout::new(1280.0, 720.0, &desktop);
        let cells: Vec<Rect> = cards::visible_shortcuts(&layout, &desktop.dashboard)
            .into_iter()
            .map(|(_, cell)| cell)
            .collect();
        // Colonne de droite d'abord, de haut en bas, puis la suivante à sa gauche.
        assert_eq!(cells[0].right(), 1280.0 - 16.0);
        assert!(cells[1].y > cells[0].y);
        assert!(cells.iter().all(|cell| cell.bottom() <= 720.0 - 80.0));
        assert!(cells[7].x < cells[0].x);
        let (x, y) = cells[2].center();
        assert_eq!(
            action_at(output(), point(x, y), &desktop),
            Some(DesktopAction::Shortcut(2))
        );
        // Sur le bureau des widgets, pas de raccourcis.
        desktop.dashboard.shown = true;
        assert_eq!(action_at(output(), point(x, y), &desktop), None);
        // Vue d'ensemble ou écran verrouillé non plus.
        desktop.dashboard.shown = false;
        desktop.dashboard.shortcuts_shown = false;
        assert_eq!(action_at(output(), point(x, y), &desktop), None);
        desktop.dashboard.shortcuts_shown = true;

        // Une icône posée ailleurs y reste ; plus grandes, les cases s'agrandissent.
        let far = ferrite_system::shortcuts::Cell { column: 5, row: 2 };
        desktop.dashboard.shortcuts[0].cell = Some(far);
        let grid = layout.shortcut_grid(desktop.dashboard.icon_size);
        let (x, y) = grid.rect(far).center();
        assert_eq!(
            action_at(output(), point(x, y), &desktop),
            Some(DesktopAction::Shortcut(0))
        );
        // La grille couvre toute la largeur : la dernière colonne touche la marge gauche.
        let last = grid.rect(ferrite_system::shortcuts::Cell {
            column: grid.columns - 1,
            row: 0,
        });
        assert!((last.x - 16.0).abs() < 0.01);
        let bigger = layout.shortcut_grid(96);
        assert!(bigger.cell.0 > grid.cell.0 && bigger.columns < grid.columns);
        // Glissée d'une case vers la gauche, elle vise la case voisine.
        let target = cards::shortcut_target(&layout, &desktop.dashboard, 0, (-grid.cell.0, 0.0));
        assert_eq!(
            target,
            Some(ferrite_system::shortcuts::Cell { column: 6, row: 2 })
        );
    }

    #[test]
    fn grille_des_raccourcis_cases_libres_et_hors_ecran() {
        use ferrite_system::shortcuts::Cell;
        let layout = Layout::new(1280.0, 720.0, &view(None));
        let grid = layout.shortcut_grid(52);
        let far = Cell {
            column: grid.columns + 3,
            row: 0,
        };
        let placed = grid.place(&[
            None,
            Some(Cell { column: 0, row: 0 }),
            Some(Cell { column: 0, row: 0 }),
            Some(far),
        ]);
        // La case choisie passe avant le rangement automatique ; un doublon ou une case
        // hors de l'écran prend la première case libre.
        assert_eq!(placed[1], Some(Cell { column: 0, row: 0 }));
        assert_eq!(placed[0], Some(Cell { column: 0, row: 1 }));
        assert_eq!(placed[2], Some(Cell { column: 0, row: 2 }));
        assert_eq!(placed[3], Some(Cell { column: 0, row: 3 }));
        // Lâchée loin de la grille, une icône va dans la case la plus proche.
        assert_eq!(
            grid.nearest((-500.0, 5000.0)),
            Cell {
                column: grid.columns - 1,
                row: grid.rows - 1
            }
        );
    }

    #[test]
    fn un_widget_devant_prend_le_clic_et_les_taches_suivent_la_hauteur() {
        let output = output();
        let mut desktop = view(None);
        desktop.dashboard.shown = true;
        desktop.dashboard.layout = default_layout();
        // Le calendrier posé sur la citation, en dernier donc devant.
        let mut calendar = desktop.dashboard.layout.remove(1);
        calendar.x = 40;
        calendar.y = 440;
        desktop.dashboard.layout.push(calendar);
        assert_eq!(widget_at(output, &desktop, point(60.0, 460.0)), Some(6));
        // Plus haute, la carte des tâches en montre plus.
        desktop.dashboard.tasks = (0..8)
            .map(|index| DashboardTaskView {
                text: format!("Tâche {index}"),
                done: false,
                priority: crate::desktop::widgets::Priority::Low,
            })
            .collect();
        let short = Rect::new(0.0, 0.0, 356.0, 170.0);
        let tall = Rect::new(0.0, 0.0, 356.0, 360.0);
        assert_eq!(cards::shown_tasks(&desktop.dashboard, short).len(), 2);
        assert_eq!(cards::shown_tasks(&desktop.dashboard, tall).len(), 7);
    }

    #[test]
    fn une_mesure_du_processeur_ne_redessine_que_la_carte_systeme() {
        use crate::desktop::widgets::WidgetKind;
        let state = VisualState {
            width: 1280,
            height: 720,
            scale_milli: 1000,
            accent: Color::WHITE,
            view: view(None),
            battery: None,
            clock: ClockSnapshot::default(),
            system: SystemMetrics::default(),
        };
        let mut busier = state.clone();
        busier.system.cpu = 42;
        busier.clock.minute = 7;
        // Le reste de l'état n'a pas bougé : pas de redessin complet.
        assert_eq!(cards::base_key(&state), cards::base_key(&busier));
        assert_ne!(
            cards::card_live(WidgetKind::System, &state),
            cards::card_live(WidgetKind::System, &busier)
        );
        assert_eq!(
            cards::card_live(WidgetKind::Tasks, &state),
            cards::card_live(WidgetKind::Tasks, &busier)
        );
        // L'horloge suit la minute.
        assert_ne!(
            cards::card_live(WidgetKind::Clock, &state),
            cards::card_live(WidgetKind::Clock, &busier)
        );
    }

    #[test]
    fn le_widget_musique_pilote_le_lecteur() {
        use crate::desktop::{
            lock::{Media, MediaCommand},
            widgets::{WidgetKind, WidgetSlot},
        };
        let mut desktop = view(None);
        desktop.dashboard.shown = true;
        desktop.dashboard.layout = vec![WidgetSlot {
            kind: WidgetKind::Music,
            x: 32,
            y: 244,
            w: 356,
            h: 170,
        }];
        let layout = Layout::new(1280.0, 720.0, &desktop);
        let card = layout.dashboard_cards(&desktop.dashboard.layout)[0];
        let [previous, play, next] = cards::music_buttons(card);
        let at = |rect: Rect, desktop: &DesktopView| {
            let (x, y) = rect.center();
            action_at(output(), point(x, y), desktop)
        };
        // Sans lecteur : la carte ouvre Musique, même sur la place des boutons.
        assert_eq!(at(play, &desktop), Some(DesktopAction::DashboardOpenMusic));
        desktop.dashboard.media = Some(Media {
            detail: "Nuit Claire · Ciel étoilé".into(),
            progress: 30,
            playing: true,
            title: "Ciel étoilé".into(),
            artist: "Nuit Claire".into(),
            cover: None,
            entry: "ferrite-music".into(),
            player: "org.mpris.MediaPlayer2.ferrite-music".into(),
            position: Duration::from_secs(30),
            read_at: Instant::now(),
            length: Duration::from_secs(100),
        });
        assert_eq!(
            at(previous, &desktop),
            Some(DesktopAction::DashboardMedia(MediaCommand::Previous))
        );
        assert_eq!(
            at(play, &desktop),
            Some(DesktopAction::DashboardMedia(MediaCommand::PlayPause))
        );
        assert_eq!(
            at(next, &desktop),
            Some(DesktopAction::DashboardMedia(MediaCommand::Next))
        );
        let title = Rect::new(card.x + 180.0, card.y + 40.0, 10.0, 10.0);
        assert_eq!(at(title, &desktop), Some(DesktopAction::DashboardOpenMusic));
    }

    fn point(x: f64, y: f64) -> Point<f64, Logical> {
        Point::from((x, y))
    }

    fn output() -> Rectangle<i32, Logical> {
        Rectangle::new((0, 0).into(), (1280, 720).into())
    }

    /// Ordinateur complet : Wi-Fi, Bluetooth, son et rétroéclairage présents.
    fn view(popover: Option<Popover>) -> DesktopView {
        DesktopView {
            popover,
            wifi: Some(true),
            bluetooth: Some(false),
            volume: Some(60),
            brightness: Some(70),
            airplane: Some(false),
            keyboard_light: Some((1, 2)),
            power_saver: Some(false),
            bar_visible: true,
            bar_height: 34,
            ..DesktopView::default()
        }
    }

    fn layout() -> Layout {
        Layout::new(1280.0, 720.0, &view(None))
    }

    fn click(pos: Point<f64, Logical>, popover: Option<Popover>) -> Option<DesktopAction> {
        action_at(output(), pos, &view(popover))
    }

    #[test]
    fn les_widgets_masques_ne_captent_pas_les_clics() {
        let mut desktop = view(None);
        desktop.dashboard.layout = default_layout();
        desktop.dashboard.shown = true;
        let layout = Layout::new(1280.0, 720.0, &desktop);
        let quote = layout.dashboard_cards(&desktop.dashboard.layout)[5];
        let inside = point(quote.x + 30.0, quote.y + 40.0);
        assert_eq!(
            action_at(output(), inside, &desktop),
            Some(DesktopAction::DashboardConsume)
        );
        desktop.dashboard.shown = false;
        assert_eq!(action_at(output(), inside, &desktop), None);
    }

    #[test]
    fn l_editeur_deplace_redimensionne_retire_et_termine() {
        let mut desktop = view(None);
        desktop.dashboard.layout = default_layout();
        desktop.dashboard.shown = true;
        desktop.dashboard.edit = true;
        desktop.dashboard.layout.remove(1);
        let layout = Layout::new(1280.0, 720.0, &desktop);
        let card = layout.dashboard_cards(&desktop.dashboard.layout)[0];
        let click = |x: f64, y: f64| action_at(output(), point(x, y), &desktop);
        assert_eq!(
            click(card.x + 2.0, card.y + 2.0),
            Some(DesktopAction::WidgetRemove(0))
        );
        assert_eq!(
            click(card.x + 100.0, card.y + 80.0),
            Some(DesktopAction::WidgetDragStart(0, WidgetEdges::default()))
        );
        assert_eq!(
            click(card.right() - 3.0, card.bottom() - 3.0),
            Some(DesktopAction::WidgetDragStart(
                0,
                WidgetEdges {
                    right: true,
                    bottom: true,
                    ..WidgetEdges::default()
                }
            ))
        );
        assert_eq!(
            click(card.x + 3.0, card.y + 80.0),
            Some(DesktopAction::WidgetDragStart(
                0,
                WidgetEdges {
                    left: true,
                    ..WidgetEdges::default()
                }
            ))
        );
        let bar = layout.editor_bar();
        assert_eq!(
            click(bar.x + 40.0, bar.y + 22.0),
            Some(DesktopAction::WidgetAdd(
                crate::desktop::widgets::WidgetKind::Calendar
            ))
        );
        assert_eq!(
            click(bar.right() - 40.0, bar.y + 22.0),
            Some(DesktopAction::WidgetsDone)
        );
        assert_eq!(
            click(bar.right() - 150.0, bar.y + 22.0),
            Some(DesktopAction::WidgetsReset)
        );
    }

    #[test]
    fn les_marques_de_la_barre_menent_a_leur_page() {
        let mut desktop = view(None);
        desktop.page = 1;
        desktop.page_count = 3;
        let layout = Layout::new(1280.0, 720.0, &desktop);
        for page in 0..3 {
            let (x, y) = layout.page_dot(page).center();
            assert_eq!(
                action_at(output(), point(x, y), &desktop),
                Some(DesktopAction::GoToPage(page))
            );
            assert!(!layout.applications_button().contains(point(x, y)));
        }
    }

    #[test]
    fn les_widgets_gardent_la_taille_de_la_maquette_et_restent_dans_l_ecran() {
        let mut desktop = view(None);
        desktop.bar_height = 34;
        desktop.dashboard.layout = default_layout();
        let cards = Layout::new(1600.0, 1000.0, &desktop).dashboard_cards(&desktop.dashboard.layout);
        assert_eq!(
            (cards[0].x, cards[0].y, cards[0].w, cards[0].h),
            (32.0, 58.0, 356.0, 170.0)
        );
        desktop.dashboard.layout[0].x = 9000;
        let cards = Layout::new(1280.0, 720.0, &desktop).dashboard_cards(&desktop.dashboard.layout);
        assert!(cards[0].right() <= 1280.0);
    }

    #[test]
    fn le_clic_droit_du_bureau_offre_ses_choix() {
        let mut desktop = view(Some(Popover::DesktopMenu));
        desktop.desktop_menu = Some((400, 300));
        desktop.desktop_menu_entries = vec!["Modifier les widgets".to_owned(), "Ouvrir".to_owned()];
        let row = Layout::new(1280.0, 720.0, &desktop).desktop_menu_row((400, 300), 2, 0);
        let (x, y) = row.center();
        assert_eq!(
            action_at(output(), point(x, y), &desktop),
            Some(DesktopAction::DesktopMenu(0))
        );
        assert_eq!(
            action_at(output(), point(10.0, 300.0), &desktop),
            Some(DesktopAction::ClosePopover)
        );
    }

    fn lock_view() -> DesktopView {
        let mut desktop = view(None);
        desktop.lock = Some(LockView {
            user: "Utilisateur".to_owned(),
            initial: 'U',
            avatar: None,
            length: 0,
            cursor: 0,
            shown: None,
            field: false,
            progress: 0,
            checking: false,
            welcome: false,
            message: LockMessage::Hint,
            face: LockFace::Idle,
            face_ready: false,
            switch_user: false,
            greeter: false,
            face_tick: 0,
            shake: 0,
            pulse: 0,
            spin: 0,
            dark: true,
            wall: 0,
            picture: None,
            blur: 0,
            weather: None,
            place: String::new(),
            notice: None,
            media: None,
        });
        desktop
    }

    #[test]
    fn la_photo_de_verrouillage_a_sa_pastille() {
        let mut desktop = lock_view();
        let area = Rectangle::from_size((1600, 1000).into());
        // Pastilles tous les 40 points depuis 115 : la cinquième (la photo) est à 275.
        let fifth = point(275.0, 944.0);
        assert_ne!(lock_action_at(area, &desktop, fifth), LockAction::Wall(4));
        if let Some(lock) = desktop.lock.as_mut() {
            lock.picture = Some(LockPicture {
                path: "/home/a/photo.png".into(),
                image: Arc::new(Image {
                    width: 1,
                    height: 1,
                    rgba: vec![0; 4],
                }),
                thumb: crate::clipboard::Thumbnail {
                    width: 1,
                    height: 1,
                    pixels: Arc::new(vec![0; 4]),
                },
                clock_luma: 0.0,
                invitation_luma: 0.0,
            });
        }
        assert_eq!(lock_action_at(area, &desktop, fifth), LockAction::Wall(4));
    }

    #[test]
    fn au_repos_un_clic_reveille_et_les_boutons_repondent() {
        let desktop = lock_view();
        let area = Rectangle::from_size((1600, 1000).into());
        assert_eq!(
            lock_action_at(area, &desktop, point(800.0, 500.0)),
            LockAction::Reveal
        );
        assert_eq!(
            lock_action_at(area, &desktop, point(1546.0, 948.0)),
            LockAction::Power
        );
        assert_eq!(
            lock_action_at(area, &desktop, point(1486.0, 948.0)),
            LockAction::Keyboard
        );
        assert_eq!(
            lock_action_at(area, &desktop, point(58.0, 944.0)),
            LockAction::ToggleTheme
        );
        assert_eq!(
            lock_action_at(area, &desktop, point(115.0, 944.0)),
            LockAction::Wall(0)
        );
    }

    #[test]
    fn en_saisie_on_annule_ou_on_valide() {
        let mut desktop = lock_view();
        if let Some(lock) = desktop.lock.as_mut() {
            lock.field = true;
        }
        let area = Rectangle::from_size((1600, 1000).into());
        assert_eq!(
            lock_action_at(area, &desktop, point(75.0, 950.0)),
            LockAction::Cancel
        );
        assert_eq!(
            lock_action_at(area, &desktop, point(800.0 + 130.0, 513.0)),
            LockAction::Submit
        );
        // Rien de ce qui est au repos ne reste cliquable.
        assert_eq!(
            lock_action_at(area, &desktop, point(800.0, 800.0)),
            LockAction::Consume
        );
    }

    #[test]
    fn le_fond_de_verrouillage_est_opaque_et_suit_le_theme() {
        let dark = lock_screen::paint_lock_wall(64, 40, true, 0);
        let light = lock_screen::paint_lock_wall(64, 40, false, 0);
        assert_eq!(dark.len(), 64 * 40 * 4);
        assert!(dark.chunks(4).all(|pixel| pixel[3] == 255));
        let luma = |pixels: &[u8]| pixels.iter().map(|value| u64::from(*value)).sum::<u64>();
        assert!(luma(&light) > luma(&dark));
    }

    #[test]
    fn la_barre_ouvre_chaque_menu() {
        assert_eq!(
            click(point(40.0, 15.0), None),
            Some(DesktopAction::TogglePopover(Popover::Launcher))
        );
        let network = layout().indicator(Indicator::Network).center();
        assert_eq!(
            click(point(network.0, network.1), None),
            Some(DesktopAction::TogglePopover(Popover::Controls))
        );
        assert_eq!(
            click(point(640.0, 15.0), None),
            Some(DesktopAction::TogglePopover(Popover::Notifications))
        );
        // Le reste de la barre absorbe le clic, ou ferme le menu ouvert.
        assert_eq!(click(point(400.0, 15.0), None), Some(DesktopAction::Consume));
    }

    #[test]
    fn la_barre_de_capture_repere_ses_boutons() {
        let layout = Layout::new(1280.0, 720.0, &view(Some(Popover::Screenshot)));
        let at = |rect: Rect, windows| {
            let (x, y) = rect.center();
            capture_toolbar_at(output(), point(x, y), windows)
        };
        for (index, (mode, _)) in CAPTURE_MODES.into_iter().enumerate() {
            assert_eq!(at(layout.capture_mode(index), true), Some(CaptureHit::Mode(mode)));
        }
        // Sans fenêtre, le mode Fenêtre n'est pas proposé.
        assert_eq!(at(layout.capture_mode(2), false), Some(CaptureHit::Toolbar));
        assert_eq!(at(layout.capture_shutter(), true), Some(CaptureHit::Shutter));
        assert_eq!(at(layout.capture_kind(true), true), Some(CaptureHit::Video));
        assert_eq!(at(layout.capture_delay(), true), Some(CaptureHit::Delay));
        assert_eq!(at(layout.capture_pointer(), true), Some(CaptureHit::Pointer));
        assert_eq!(at(layout.capture_close(), true), Some(CaptureHit::Close));
        assert_eq!(capture_toolbar_at(output(), point(100.0, 100.0), true), None);
        assert_eq!(
            click(point(400.0, 15.0), Some(Popover::Launcher)),
            Some(DesktopAction::ClosePopover)
        );
    }

    #[test]
    fn sans_materiel_les_indicateurs_absents_disparaissent() {
        // Machine virtuelle : ni Wi-Fi, ni Bluetooth, ni son.
        let bare = DesktopView {
            bar_visible: true,
            bar_height: 34,
            ..DesktopView::default()
        };
        let layout = Layout::new(1280.0, 720.0, &bare);
        assert_eq!(layout.indicator(Indicator::Bluetooth).w, 0.0);
        assert_eq!(layout.indicator(Indicator::Volume).w, 0.0);
        // L'indicateur réseau reste : les réglages rapides restent accessibles.
        let network = layout.indicator(Indicator::Network).center();
        assert_eq!(
            action_at(output(), point(network.0, network.1), &bare),
            Some(DesktopAction::TogglePopover(Popover::Controls))
        );
        // Les réglages indisponibles ne réagissent pas.
        let open = DesktopView {
            popover: Some(Popover::Controls),
            ..bare
        };
        let bluetooth = layout.control_row(Control::Bluetooth).center();
        assert_eq!(
            action_at(output(), point(bluetooth.0, bluetooth.1), &open),
            Some(DesktopAction::Consume)
        );
    }

    fn launcher(count: usize, scroll: u32) -> DesktopView {
        let tiles = (0..count)
            .map(|index| Tile {
                name: format!("App {index}"),
                icon: TileIcon::Missing,
                index,
            })
            .collect();
        DesktopView {
            launcher: LauncherView {
                sections: vec![LauncherSection {
                    title: String::new(),
                    tiles,
                }],
                scroll,
                ..LauncherView::default()
            },
            ..view(Some(Popover::Launcher))
        }
    }

    fn tile_rect(view: &DesktopView, index: usize) -> Rect {
        layout()
            .launcher_items(&view.launcher)
            .into_iter()
            .find_map(|item| match item {
                LauncherItem::Tile { section, slot, rect }
                    if view.launcher.sections[section].tiles[slot].index == index =>
                {
                    Some(rect)
                }
                _ => None,
            })
            .expect("case présente")
    }

    #[test]
    fn le_lanceur_est_centre_et_lance_chaque_application() {
        let card = layout().launcher();
        assert!((card.x + card.w / 2.0 - 640.0).abs() < 1.0);
        let open = launcher(10, 0);
        let at = |x: f64, y: f64| action_at(output(), point(x, y), &open);
        for index in [0, 4, 5, 9] {
            let (x, y) = tile_rect(&open, index).center();
            assert_eq!(at(x, y), Some(DesktopAction::LaunchTile(index)));
        }
        // Six applications par ligne, à la suite, sans rubriques.
        assert_eq!(tile_rect(&open, 5).y, tile_rect(&open, 0).y);
        assert_eq!(tile_rect(&open, 6).x, tile_rect(&open, 0).x);
        assert_eq!(tile_rect(&open, 6).y, tile_rect(&open, 0).bottom());
        // Le champ de recherche absorbe le clic ; dehors, le lanceur se ferme.
        assert_eq!(at(card.center().0, card.y + 20.0), Some(DesktopAction::Consume));
        assert_eq!(at(20.0, 600.0), Some(DesktopAction::ClosePopover));
    }

    #[test]
    fn le_lanceur_defile_sans_depasser_la_fin() {
        let long = launcher(40, 0);
        let limit = launcher_scroll_limit(output(), &long);
        assert!(limit > 0);
        // Défilé jusqu'au bout : la dernière case est visible, juste au-dessus du bas.
        let end = launcher(40, limit + 500);
        let content = layout().launcher_content();
        let last = tile_rect(&end, 39);
        assert!(last.bottom() <= content.bottom() + 0.5 && last.bottom() > content.bottom() - 20.0);
        // Une case passée sous le champ de recherche ne se clique plus.
        let first = tile_rect(&end, 0);
        assert!(first.bottom() < content.y);
        // Peu d'applications : rien à faire défiler.
        assert_eq!(launcher_scroll_limit(output(), &launcher(5, 0)), 0);
    }

    #[test]
    fn les_reglages_rapides_pilotent_wifi_bluetooth_son_et_luminosite() {
        let layout = layout();
        let open = Some(Popover::Controls);
        let at = |control| {
            let (x, y) = layout.control_row(control).center();
            click(point(x, y), open)
        };
        assert_eq!(at(Control::Wifi), Some(DesktopAction::ToggleWifi));
        assert_eq!(at(Control::Bluetooth), Some(DesktopAction::ToggleBluetooth));
        assert_eq!(at(Control::Dark), Some(DesktopAction::ToggleDarkMode));
        assert_eq!(at(Control::DoNotDisturb), Some(DesktopAction::ToggleDoNotDisturb));
        assert_eq!(at(Control::Saver), Some(DesktopAction::TogglePowerSaver));
        assert_eq!(at(Control::Keyboard), Some(DesktopAction::ToggleKeyboardLayout));
        assert_eq!(at(Control::Airplane), Some(DesktopAction::ToggleAirplane));
        assert_eq!(
            at(Control::KeyboardLight),
            Some(DesktopAction::CycleKeyboardLight)
        );
        // La quatrième rangée de tuiles reste au-dessus des curseurs.
        assert!(layout.control_row(Control::Airplane).bottom() < layout.control_row(Control::Volume).y);
        assert_eq!(
            at(Control::WifiMore),
            Some(DesktopAction::ControlsPage(ControlsPage::Wifi))
        );
        assert_eq!(
            at(Control::BluetoothMore),
            Some(DesktopAction::ControlsPage(ControlsPage::Bluetooth))
        );
        assert_eq!(
            at(Control::Account),
            Some(DesktopAction::OpenSettingsPage("comptes"))
        );
        assert_eq!(at(Control::Settings), Some(DesktopAction::OpenSettings));
        assert_eq!(at(Control::Lock), Some(DesktopAction::Lock));
        assert_eq!(at(Control::Power), Some(DesktopAction::OpenPowerDialog));
        // Les quatre boutons ronds du pied ne se touchent pas et restent dans le panneau.
        let footer = [Control::Account, Control::Settings, Control::Lock, Control::Power]
            .map(|control| layout.control_row(control));
        for pair in footer.windows(2) {
            assert!(pair[0].right() < pair[1].x);
        }
        assert!(footer[3].right() <= layout.controls_footer().right());
        // Le bouton reste dans la piste : sa course s'arrête à un demi-bouton des bords.
        let volume = layout.slider_track(Control::Volume);
        let quarter = volume.x + volume.h / 2.0 + (volume.w - volume.h) / 4.0;
        assert_eq!(
            click(point(quarter, volume.y + 5.0), open),
            Some(DesktopAction::SetVolume(25))
        );
        let brightness = layout.slider_track(Control::Brightness);
        assert_eq!(
            click(point(brightness.right() - 2.0, brightness.y + 5.0), open),
            Some(DesktopAction::SetBrightness(100))
        );
        assert_eq!(
            click(point(brightness.x + 2.0, brightness.y + 5.0), open),
            Some(DesktopAction::SetBrightness(0))
        );
    }

    #[test]
    fn les_yeux_du_wifi_montrent_les_mots_de_passe() {
        let row = |index: usize, ssid: &str, connected: bool, known: bool| NetworkRow {
            index,
            ssid: ssid.to_owned(),
            level: 3,
            secured: true,
            connected,
            known,
            hint: String::new(),
            revealed: false,
        };
        let mut desktop = view(Some(Popover::Controls));
        desktop.controls.page = ControlsPage::Wifi;
        desktop.controls.networks = vec![
            row(0, "Maison", true, true),
            row(1, "Atelier", false, true),
            row(2, "Voisin", false, false),
        ];
        desktop.controls.password = Some(PasswordRow {
            ssid: "Voisin".to_owned(),
            length: 4,
            shown: None,
        });
        let layout = Layout::new(1280.0, 720.0, &desktop);
        let at = |rect: Rect| {
            action_at(
                output(),
                point(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
                &desktop,
            )
        };
        assert_eq!(
            at(layout.wifi_eye(0, true)),
            Some(DesktopAction::NetworkPassword(0))
        );
        assert_eq!(
            at(layout.wifi_eye(1, false)),
            Some(DesktopAction::NetworkPassword(1))
        );
        // Réseau inconnu : pas d'œil, la ligne demande le mot de passe.
        assert_eq!(at(layout.wifi_eye(2, false)), Some(DesktopAction::NetworkJoin(2)));
        assert_eq!(at(layout.password_eye()), Some(DesktopAction::PasswordReveal));
        assert_eq!(at(layout.password_submit()), Some(DesktopAction::PasswordSubmit));
    }

    #[test]
    fn sans_retroeclairage_le_module_moniteur_disparait() {
        let desktop = DesktopView {
            brightness: None,
            ..view(Some(Popover::Controls))
        };
        let layout = Layout::new(1280.0, 720.0, &desktop);
        assert_eq!(layout.control_row(Control::Brightness).w, 0.0);
        // La rangée de sortie du son remonte d'autant.
        assert!(layout.controls().bottom() < self::layout().controls().bottom());
    }

    #[test]
    fn l_arret_demande_toujours_une_confirmation() {
        let layout = layout();
        let mut pending = DesktopView {
            power: Some(PowerAction::PowerOff),
            power_left: 60,
            power_apps: 3,
            ..view(None)
        };
        let at = |x: f64, y: f64, view: &DesktopView| action_at(output(), point(x, y), view);
        let click = |rect: Rect, view: &DesktopView| {
            let (x, y) = rect.center();
            at(x, y, view)
        };
        assert_eq!(
            click(layout.dialog_confirm(), &pending),
            Some(DesktopAction::ConfirmPower(PowerAction::PowerOff))
        );
        assert_eq!(
            click(layout.dialog_cancel(), &pending),
            Some(DesktopAction::CancelPower)
        );
        assert_eq!(
            click(layout.dialog_choice(2), &pending),
            Some(DesktopAction::PowerChoose(PowerAction::Sleep))
        );
        // La tuile choisie décide de ce que fait le bouton principal.
        pending.power = Some(PowerAction::Restart);
        assert_eq!(
            click(layout.dialog_confirm(), &pending),
            Some(DesktopAction::ConfirmPower(PowerAction::Restart))
        );
        assert_eq!(
            click(layout.dialog_logout(), &pending),
            Some(DesktopAction::ConfirmPower(PowerAction::LogOut))
        );
        assert_eq!(click(layout.dialog_lock(), &pending), Some(DesktopAction::Lock));
        assert_eq!(
            click(layout.dialog_relaunch(), &pending),
            Some(DesktopAction::ConfirmPower(PowerAction::RelaunchDesktop))
        );
        // Tout tient dans le dialogue, sans chevauchement.
        let dialog = layout.dialog();
        assert!(layout.dialog_choice(2).right() <= dialog.right() - 24.0 + 0.01);
        assert!(layout.dialog_choice(0).bottom() < layout.dialog_cancel().y);
        assert!(layout.dialog_cancel().bottom() < layout.dialog_logout().y);
        assert!(layout.dialog_lock().right() < layout.dialog_relaunch().x);
        assert!(layout.dialog_logout().bottom() <= dialog.bottom());
        assert_eq!(
            at(dialog.x + 10.0, dialog.y + 10.0, &pending),
            Some(DesktopAction::Consume)
        );
        assert_eq!(at(5.0, 700.0, &pending), Some(DesktopAction::CancelPower));
    }

    #[test]
    fn clics_sur_les_notifications() {
        let output = Rectangle::from_size((1440, 900).into());
        let notification = NotificationView {
            id: 7,
            app: "Essai".to_owned(),
            summary: "Titre".to_owned(),
            body: String::new(),
            icon: None,
            when: "maintenant".to_owned(),
            actions: vec!["Installer".to_owned()],
        };
        let mut view = DesktopView {
            bar_visible: true,
            bar_height: 34,
            notifications: NotificationsView {
                bubbles: vec![notification.clone()],
                ..NotificationsView::default()
            },
            ..DesktopView::default()
        };
        view.island = IslandView {
            kind: Some(IslandKind::Notification {
                notification: notification.clone(),
                more: 0,
            }),
            others: Vec::new(),
        };
        let layout = Layout::new(1440.0, 900.0, &view);
        let bubble = layout.island(&view.island).expect("îlot affiché");
        let at = |rect: Rect| Point::from(rect.center());
        assert_eq!(
            action_at(output, Point::from((bubble.x + 10.0, bubble.y + 10.0)), &view),
            Some(DesktopAction::NotificationActivate(7))
        );
        assert_eq!(
            action_at(output, at(layout.island_buttons(&view.island)[0]), &view),
            Some(DesktopAction::NotificationAction(7, 0))
        );
        view.island = IslandView::default();
        assert_eq!(
            action_at(output, at(layout.clock_button()), &view),
            Some(DesktopAction::TogglePopover(Popover::Notifications))
        );

        view.notifications.bubbles.clear();
        view.notifications.history = vec![notification];
        view.popover = Some(Popover::Notifications);
        assert_eq!(
            action_at(output, at(layout.notifications_clear(1)), &view),
            Some(DesktopAction::NotificationsClear)
        );
        assert_eq!(
            action_at(output, at(layout.notification_row(1, 0)), &view),
            Some(DesktopAction::NotificationActivate(7))
        );
    }

    #[test]
    fn un_jour_des_calendriers_du_bureau_ouvre_le_calendrier() {
        let center = |rect: Rect| point(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        // Calendrier de la barre : un jour, ou le titre du mois.
        let desktop = view(Some(Popover::Notifications));
        let card = layout().calendar(0);
        let ((year, month), _) = calendar_months(0);
        assert_eq!(
            action_at(
                output(),
                center(calendar_cell(card, i64::from(year), month, 12)),
                &desktop
            ),
            Some(DesktopAction::OpenCalendar(Some((year, month, 12))))
        );
        assert_eq!(
            action_at(output(), point(card.x + 40.0, card.y + 20.0), &desktop),
            Some(DesktopAction::OpenCalendar(None))
        );
        // Widget Calendrier : un jour du mois montré.
        let mut desktop = view(None);
        desktop.dashboard.layout = default_layout();
        desktop.dashboard.shown = true;
        desktop.dashboard.month_offset = 1;
        let index = desktop
            .dashboard
            .layout
            .iter()
            .position(|slot| slot.kind == crate::desktop::widgets::WidgetKind::Calendar)
            .unwrap();
        let widget = Layout::new(1280.0, 720.0, &desktop).dashboard_cards(&desktop.dashboard.layout)[index];
        let (_, (year, month)) = calendar_months(1);
        assert_eq!(
            action_at(
                output(),
                center(cards::calendar_day(widget, i64::from(year), month, 3)),
                &desktop
            ),
            Some(DesktopAction::DashboardCalendar(Some((year, month, 3))))
        );
    }

    #[test]
    fn horloge_en_francais_et_fuseau() {
        // Lundi 26 octobre 2026, 17:57 UTC.
        let time = UNIX_EPOCH + Duration::from_secs(20_752 * 86_400 + 17 * 3600 + 57 * 60);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_752), (2026, 10, 26));
        assert_eq!(clock_label(time, 0), "lun. 26 oct.  17:57");
        assert_eq!(clock_label(time, 3600), "lun. 26 oct.  18:57");
        assert_eq!(clock_label(time, 7 * 3600), "mar. 27 oct.  00:57");
        assert_eq!(parse_offset("+0100"), Some(3600));
        assert_eq!(parse_offset("-0230"), Some(-9000));
        assert_eq!(parse_offset("UTC"), None);
    }
}
