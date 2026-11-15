//! Écran de verrouillage : Super+L, bouton du centre de contrôle, ou avant une mise en veille.
//!
//! Tant qu'il est là, aucune fenêtre n'est dessinée ni ne reçoit d'entrée. Le mot de passe
//! est vérifié sur un autre fil par `unix_chkpwd`, l'auxiliaire de PAM prévu pour ça.

use std::{
    io::Write,
    process::{Command, Stdio},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use smithay::reexports::calloop::{
    LoopHandle, channel,
    timer::{TimeoutAction, Timer},
};

pub use crate::desktop::media::{Media, MediaCommand, MediaLink};
use crate::{
    desktop::panels::{LOCK_BLUR_MAX, LOCK_WALLS, LockFace, LockMessage},
    state::Ferrite,
};

/// Auxiliaire de `pam_unix` : il ne vérifie que le compte qui l'appelle.
const HELPERS: [&str; 2] = ["/usr/sbin/unix_chkpwd", "/sbin/unix_chkpwd"];
/// Temps du passage du repos à la saisie, et de la secousse d'un refus.
const TRANSITION: Duration = ferrite_ui::tokens::motion::LONG;
/// Fondu enchaîné zoomé entre le bureau et l'écran verrouillé.
const SWAP: Duration = Duration::from_millis(560);
/// Zoom du bureau quand il est derrière l'écran verrouillé (il recule).
const DESKTOP_ZOOM: f32 = 0.95;
/// Zoom de l'écran verrouillé quand il est hors de l'écran (devant, tout proche).
const LOCK_ZOOM: f32 = 1.08;
/// Attente maximale de la photo de l'écran avant le passage.
const CAPTURE_WAIT: Duration = Duration::from_millis(120);
const SCREEN_FADE_STEP: Duration = Duration::from_millis(16);
const SHAKE: Duration = Duration::from_millis(380);
/// Message d'accueil, une fois le mot de passe accepté.
const WELCOME: Duration = Duration::from_millis(600);
/// Période de l'invitation qui pulse.
const PULSE: Duration = Duration::from_millis(2400);

/// État de l'écran de verrouillage.
#[derive(Debug)]
pub struct Lock {
    password: String,
    /// Place du curseur dans le mot de passe, en caractères.
    cursor: usize,
    /// L'œil est tenu enfoncé : le mot de passe s'affiche en clair.
    peek: bool,
    /// Le champ n'apparaît qu'après une touche ou un clic.
    pub field_visible: bool,
    /// Dernier passage entre le repos et la saisie.
    field_changed: Option<Instant>,
    pub checking: bool,
    /// Mot de passe accepté : l'écran part à cette heure.
    welcome_until: Option<Instant>,
    pub message: LockMessage,
    /// Déverrouillage par visage : où il en est, depuis quand, et s'il peut être lancé.
    pub face: LockFace,
    pub face_since: Instant,
    pub face_ready: bool,
    refused_at: Option<Instant>,
    opened_at: Instant,
    pub dark: bool,
    pub wall: usize,
    /// Flou de la photo, de 0 (aucun, par défaut) à [`LOCK_BLUR_MAX`].
    pub blur: u8,
    pub media: Option<Media>,
}

impl Lock {
    fn new() -> Self {
        let settings = ferrite_system::config::Settings::load();
        // Clair par défaut, quel que soit le thème du bureau.
        let dark = settings
            .get("verrouillage_theme")
            .is_some_and(|theme| theme == "sombre");
        let blur = settings
            .get("verrouillage_flou")
            .and_then(|level| level.parse::<u8>().ok())
            .unwrap_or(0)
            .min(LOCK_BLUR_MAX);
        // Un chemin : la photo choisie dans la Visionneuse (après les fonds dessinés).
        let wall = settings
            .get("verrouillage_fond")
            .and_then(|name| {
                if name.starts_with('/') {
                    Some(LOCK_WALLS.len())
                } else {
                    LOCK_WALLS.iter().position(|wall| wall.0 == name)
                }
            })
            .unwrap_or(0);
        Self {
            password: String::new(),
            cursor: 0,
            peek: false,
            field_visible: false,
            field_changed: None,
            checking: false,
            welcome_until: None,
            message: LockMessage::Hint,
            face: LockFace::Idle,
            face_since: Instant::now(),
            face_ready: false,
            refused_at: None,
            opened_at: Instant::now(),
            dark,
            wall,
            blur,
            media: None,
        }
    }

    /// Mot de passe accepté ou visage reconnu : l'écran part après l'accueil.
    pub fn accept(&mut self) {
        self.forget_password();
        self.welcome_until = Some(Instant::now() + WELCOME);
    }

    /// L'écran est en train de partir.
    pub fn unlocking(&self) -> bool {
        self.welcome_until.is_some()
    }

    pub fn password_length(&self) -> usize {
        self.password.chars().count()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Le mot de passe en clair, seulement tant que l'œil est tenu.
    pub fn peeked(&self) -> Option<String> {
        self.peek.then(|| self.password.clone())
    }

    /// Position en octets du caractère `index` (la fin s'il n'existe pas).
    fn byte_at(&self, index: usize) -> usize {
        self.password
            .char_indices()
            .nth(index)
            .map_or(self.password.len(), |(at, _)| at)
    }

    fn forget_password(&mut self) {
        wipe(&mut self.password);
        self.cursor = 0;
        self.peek = false;
    }

    pub fn is_welcoming(&self) -> bool {
        self.welcome_until.is_some()
    }

    /// Part du passage à la saisie, de 0 à 1000 (`instant` : sans animation).
    pub fn progress(&self, instant: bool) -> u16 {
        let shown = self.field_changed.filter(|_| !instant).map_or(1.0, |changed| {
            (changed.elapsed().as_secs_f64() / TRANSITION.as_secs_f64()).min(1.0)
        });
        // Démarre vite et se pose en douceur.
        let eased = 1.0 - (1.0 - shown).powi(3);
        let part = if self.field_visible { eased } else { 1.0 - eased };
        (part * 1000.0).round() as u16
    }

    /// Décalage du champ après un refus, en dixièmes de point.
    pub fn shake(&self) -> i16 {
        let Some(refused) = self.refused_at else {
            return 0;
        };
        shake_offset(refused.elapsed().as_secs_f64() / SHAKE.as_secs_f64())
    }

    /// Niveau de l'invitation qui pulse, de 0 à 8.
    pub fn pulse(&self) -> u8 {
        let cycle = self.opened_at.elapsed().as_secs_f64() / PULSE.as_secs_f64();
        let wave = (cycle.fract() * 2.0 - 1.0).abs();
        (wave * 8.0).round() as u8
    }

    /// Pas de l'anneau du visage (un toutes les 40 ms) ; figé si les animations sont
    /// réduites.
    pub fn face_tick(&self, reduce_motion: bool) -> u16 {
        if self.face != LockFace::Scanning || reduce_motion {
            return 0;
        }
        (self.face_since.elapsed().as_millis() / 40 % 1200) as u16
    }

    /// Angle de l'indicateur d'attente, en 36 pas.
    pub fn spin(&self) -> u8 {
        if self.checking || self.is_welcoming() {
            (self.opened_at.elapsed().as_millis() / 25 % 36) as u8
        } else {
            0
        }
    }
}

/// Position de la secousse à l'instant `t` (de 0 à 1), en dixièmes de point.
fn shake_offset(t: f64) -> i16 {
    const KEYS: [f64; 6] = [0.0, -100.0, 90.0, -60.0, 40.0, 0.0];
    if !(0.0..1.0).contains(&t) {
        return 0;
    }
    let at = t * 5.0;
    let index = at.floor() as usize;
    (KEYS[index] + (KEYS[index + 1] - KEYS[index]) * at.fract()).round() as i16
}

/// Efface un mot de passe de la mémoire avant de le rendre.
fn wipe(password: &mut String) {
    // Même longueur : les octets sont écrasés sur place, sans nouvelle allocation.
    let zeros = "\0".repeat(password.len());
    password.replace_range(.., &zeros);
    password.clear();
}

/// Message vers la boucle du compositeur.
#[derive(Debug)]
pub enum Event {
    Lock,
    Resumed,
    Checked(Result<bool, String>),
    /// Réponse de `ferrite-visage verifier`.
    Face(crate::desktop::face::FaceResult),
    /// Relevé des lecteurs multimédia (`desktop/media.rs`).
    Media(Vec<Media>),
}

/// Vérifie le mot de passe du compte courant avec `helper`.
fn check_with(helper: &str, user: &str, password: &str) -> Result<bool, String> {
    let mut child = Command::new(helper)
        .args([user, "nonull"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        // L'auxiliaire attend le mot de passe terminé par un zéro.
        // Un auxiliaire qui s'arrête sans lire (erreur de sa part) ferme l'entrée : c'est
        // alors son code de sortie qui décide, jamais un déverrouillage par défaut.
        let written = stdin
            .write_all(password.as_bytes())
            .and_then(|()| stdin.write_all(&[0]));
        if let Err(err) = written
            && err.kind() != std::io::ErrorKind::BrokenPipe
        {
            return Err(err.to_string());
        }
    }
    let status = child.wait().map_err(|err| err.to_string())?;
    Ok(status.success())
}

fn check(user: &str, password: &str) -> Result<bool, String> {
    let helper = HELPERS
        .into_iter()
        .find(|path| std::path::Path::new(path).exists())
        .ok_or("unix_chkpwd introuvable")?;
    check_with(helper, user, password)
}

/// Ouvre le trousseau par défaut de gnome-keyring avec le mot de passe de la session.
///
/// La connexion sur tty1 est automatique : PAM n'a jamais le mot de passe, et Brave
/// demandait le trousseau à chaque démarrage. Le mot de passe que l'écran de verrouillage
/// vient d'accepter sert donc aussi ici, quand le trousseau a le même (sinon gnome-keyring
/// le refuse et l'application le redemande, comme avant).
fn unlock_keyring(password: &str) -> zbus::Result<()> {
    use zbus::{
        blocking::Proxy,
        zvariant::{OwnedObjectPath, OwnedValue, Value},
    };

    const SERVICE: &str = "org.freedesktop.secrets";
    const PATH: &str = "/org/freedesktop/secrets";
    let connection = zbus::blocking::Connection::session()?;
    let service = Proxy::new(&connection, SERVICE, PATH, "org.freedesktop.Secret.Service")?;
    let collection: OwnedObjectPath = service.call("ReadAlias", &("default",))?;
    if collection.as_str() == "/" {
        return Ok(());
    }
    let locked: bool = Proxy::new(
        &connection,
        SERVICE,
        collection.as_str(),
        "org.freedesktop.Secret.Collection",
    )?
    .get_property("Locked")?;
    if !locked {
        return Ok(());
    }
    // Session « plain » : le secret ne passe que par le bus de l'utilisateur.
    let (_, session): (OwnedValue, OwnedObjectPath) =
        service.call("OpenSession", &("plain", Value::from("")))?;
    let unlocked = Proxy::new(
        &connection,
        SERVICE,
        PATH,
        "org.gnome.keyring.InternalUnsupportedGuiltRiddenInterface",
    )?
    .call::<_, _, ()>(
        "UnlockWithMasterPassword",
        &(
            &collection,
            (&session, Vec::<u8>::new(), password.as_bytes(), "text/plain"),
        ),
    );
    let _ = Proxy::new(
        &connection,
        SERVICE,
        session.as_str(),
        "org.freedesktop.Secret.Session",
    )?
    .call::<_, _, ()>("Close", &());
    unlocked
}

/// Nom du compte courant.
pub fn user_name() -> String {
    std::env::var("USER").unwrap_or_default()
}

/// Branche le canal sur la boucle et verrouille avant chaque mise en veille (logind).
pub fn start(handle: &LoopHandle<'static, Ferrite>) -> Result<(channel::Sender<Event>, MediaLink), String> {
    let (events, receiver) = channel::channel::<Event>();
    handle
        .insert_source(receiver, |event, _, state| {
            if let channel::Event::Msg(event) = event {
                state.on_lock_event(event);
                state.request_redraw();
            }
        })
        .map_err(|err| err.to_string())?;
    let sleep_events = events.clone();
    std::thread::Builder::new()
        .name("mise-en-veille".to_owned())
        .spawn(move || {
            if let Err(err) = watch_sleep(&sleep_events) {
                tracing::warn!(%err, "mise en veille non surveillée");
            }
        })
        .map_err(|err| err.to_string())?;
    let media = crate::desktop::media::start(events.clone())?;
    Ok((events, media))
}

/// Attend le signal `PrepareForSleep` de logind.
fn watch_sleep(events: &channel::Sender<Event>) -> zbus::Result<()> {
    let connection = zbus::blocking::Connection::system()?;
    let manager = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    let mut previous_keyboard_light = None;
    let mut delay_inhibitor = acquire_sleep_delay_inhibitor(&manager);
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.login1")?
        .interface("org.freedesktop.login1.Manager")?
        .member("PrepareForSleep")?
        .build();
    for message in zbus::blocking::MessageIterator::for_match_rule(rule, &connection, None)? {
        let going_to_sleep: bool = message?.body().deserialize()?;
        if going_to_sleep {
            previous_keyboard_light = ferrite_system::display::keyboard_light().map(|(level, _)| level);
            if previous_keyboard_light.is_some_and(|level| level > 0)
                && let Err(error) = ferrite_system::display::set_keyboard_light(0)
            {
                tracing::warn!(%error, "impossible d'éteindre le rétroéclairage avant la veille");
            }
            if events.send(Event::Lock).is_err() {
                break;
            }
            drop(delay_inhibitor.take());
        } else {
            if let Some(level) = previous_keyboard_light.take()
                && let Err(error) = ferrite_system::display::set_keyboard_light(level)
            {
                tracing::warn!(%error, "impossible de rétablir le rétroéclairage après la veille");
            }
            if events.send(Event::Resumed).is_err() {
                break;
            }
            delay_inhibitor = acquire_sleep_delay_inhibitor(&manager);
        }
    }
    Ok(())
}

/// Prend en charge le bouton d'alimentation physique à la place de logind (qui éteindrait).
/// Le verrou dure tant que le descripteur rendu est gardé ; si le bureau s'arrête ou plante,
/// il se ferme et logind reprend son comportement habituel.
pub fn take_power_key() -> Option<zbus::zvariant::OwnedFd> {
    let result = (|| -> zbus::Result<zbus::zvariant::OwnedFd> {
        let connection = zbus::blocking::Connection::system()?;
        let manager = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )?;
        manager.call(
            "Inhibit",
            &(
                "handle-power-key",
                "Ferrite",
                "Le bureau gère le bouton d'alimentation",
                "block",
            ),
        )
    })();
    match result {
        Ok(inhibitor) => Some(inhibitor),
        Err(error) => {
            tracing::warn!(%error, "bouton d'alimentation laissé à logind");
            None
        }
    }
}

fn acquire_sleep_delay_inhibitor(manager: &zbus::blocking::Proxy<'_>) -> Option<zbus::zvariant::OwnedFd> {
    // Pris dès qu'un clavier rétroéclairé existe, même éteint : il peut être rallumé avant la
    // veille, et sans ce délai il resterait allumé pendant la veille (s2idle).
    ferrite_system::display::keyboard_light()?;
    match manager.call(
        "Inhibit",
        &(
            "sleep",
            "Ferrite",
            "Extinction du rétroéclairage du clavier",
            "delay",
        ),
    ) {
        Ok(inhibitor) => Some(inhibitor),
        Err(error) => {
            tracing::warn!(%error, "impossible de retarder la veille pour éteindre le clavier");
            None
        }
    }
}

/// Scène qui remplace l'écran présent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    /// L'écran verrouillé se pose sur le bureau.
    Lock,
    /// Le bureau revient sous l'écran verrouillé.
    Unlock,
}

impl Scene {
    /// Zoom de l'image qui part, à la fin du passage : le bureau recule, l'écran verrouillé
    /// s'approche et s'envole.
    fn leaving_zoom(self) -> f32 {
        match self {
            Scene::Lock => DESKTOP_ZOOM,
            Scene::Unlock => LOCK_ZOOM,
        }
    }

    /// Zoom de la scène qui arrive, au début du passage ; elle se pose à 1.
    fn arriving_zoom(self) -> f32 {
        match self {
            Scene::Lock => LOCK_ZOOM,
            Scene::Unlock => DESKTOP_ZOOM,
        }
    }
}

/// Image d'un passage, à un instant : zoom et opacité de chaque scène.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwapFrame {
    pub leaving_zoom: f32,
    pub leaving_alpha: f32,
    pub arriving_zoom: f32,
    pub arriving_alpha: f32,
    /// L'image qui part est dessinée par-dessus celle qui arrive.
    pub leaving_on_top: bool,
}

/// Passages de l'écran : verrouillage, déverrouillage, mise en veille et réveil.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScreenFade {
    /// L'écran présent est photographié (au prochain rendu), puis la scène change.
    Capture { since: Instant, to: Scene },
    /// Fondu enchaîné : la photo de l'ancien écran s'efface en zoomant, la nouvelle scène,
    /// vivante, se pose en dézoomant.
    Swap {
        started: Instant,
        duration: Duration,
        to: Scene,
    },
    /// Noir complet jusqu'au réveil.
    Dark,
}

impl ScreenFade {
    fn swap(to: Scene, reduce_motion: bool) -> Self {
        ScreenFade::Swap {
            started: Instant::now(),
            duration: if reduce_motion { Duration::ZERO } else { SWAP },
            to,
        }
    }

    fn part(started: Instant, duration: Duration) -> f32 {
        if duration.is_zero() {
            return 1.0;
        }
        (started.elapsed().as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// Zoom et opacités d'un passage arrivé à `t` (de 0 à 1).
    ///
    /// Le bureau, plus petit que l'écran pendant le passage, est toujours dessiné
    /// par-dessus l'écran verrouillé, qui reste plein et couvre tout l'écran : autour du
    /// bureau qui recule on voit le fond d'écran verrouillé, jamais un cadre noir.
    pub fn swap_frame(to: Scene, t: f32) -> SwapFrame {
        use ferrite_ui::theme::Easing;
        // Le zoom file vite puis ralentit : le mouvement se pose sans à-coup.
        let motion = Easing::EaseOut.apply(t);
        // Le bureau s'efface (ou apparaît) dans les deux premiers tiers : son bord ne
        // traîne pas à l'écran.
        let shown = (Easing::EaseInOut.apply(t) * 1.6).min(1.0);
        let (leaving_alpha, arriving_alpha) = match to {
            Scene::Lock => (1.0 - shown, 1.0),
            Scene::Unlock => (1.0, shown),
        };
        SwapFrame {
            leaving_zoom: 1.0 + (to.leaving_zoom() - 1.0) * motion,
            leaving_alpha,
            arriving_zoom: to.arriving_zoom() + (1.0 - to.arriving_zoom()) * motion,
            arriving_alpha,
            leaving_on_top: to == Scene::Lock,
        }
    }

    /// Image du passage en cours, s'il y en a un à dessiner.
    pub fn frame(&self) -> Option<SwapFrame> {
        match *self {
            ScreenFade::Swap {
                started,
                duration,
                to,
            } => Some(Self::swap_frame(to, Self::part(started, duration))),
            _ => None,
        }
    }

    /// Pendant la photo et le noir, clavier et souris sont ignorés ; pendant le fondu, la
    /// nouvelle scène les reçoit déjà (on peut taper son mot de passe).
    pub fn blocks_input(&self) -> bool {
        !matches!(self, ScreenFade::Swap { .. })
    }

    fn finished(&self) -> bool {
        match *self {
            ScreenFade::Swap {
                started, duration, ..
            } => Self::part(started, duration) >= 1.0,
            ScreenFade::Capture { .. } | ScreenFade::Dark => false,
        }
    }
}

impl Lock {
    pub(crate) fn show_field(&mut self) {
        if !self.field_visible {
            self.field_visible = true;
            self.field_changed = Some(Instant::now());
        }
    }
}

impl Ferrite {
    /// Verrouille : le bureau recule et s'efface, l'écran verrouillé se pose par-dessus.
    pub fn lock_session(&mut self) {
        if let Some(lock) = self.lock.as_mut() {
            // Mise en veille pendant l'accueil : on repart verrouillé.
            lock.welcome_until = None;
            lock.checking = false;
            if matches!(
                self.screen_fade,
                Some(ScreenFade::Capture {
                    to: Scene::Unlock,
                    ..
                })
            ) {
                self.screen_fade = None;
                self.screen_fade_shots.borrow_mut().clear();
                self.request_redraw();
            }
            return;
        }
        self.begin_swap(Scene::Lock);
    }

    /// Verrouille puis met l'ordinateur en veille : noir immédiat jusqu'au réveil (un fondu
    /// se voyait mal sur la vraie machine).
    pub fn lock_and_sleep(&mut self) {
        if self.screen_fade == Some(ScreenFade::Dark) {
            return;
        }
        self.super_held = false;
        self.super_used = true;
        self.lock_now();
        self.screen_fade = Some(ScreenFade::Dark);
        self.screen_fade_shots.borrow_mut().clear();
        self.request_redraw();
        self.request_system_change(crate::system::Change::Power(crate::state::PowerAction::Sleep));
    }

    fn begin_swap(&mut self, to: Scene) {
        match self.screen_fade {
            // Déjà en route, ou écran éteint : rien à refaire.
            Some(ScreenFade::Capture { .. } | ScreenFade::Dark) => return,
            Some(ScreenFade::Swap { .. }) | None => {}
        }
        self.super_held = false;
        self.super_used = true;
        self.screen_fade_shots.borrow_mut().clear();
        if self.dock_config.reduce_motion {
            // « Réduire les animations » : la scène change d'un coup.
            self.screen_fade = None;
            self.change_scene(to);
            self.request_redraw();
            return;
        }
        self.screen_fade = Some(ScreenFade::Capture {
            since: Instant::now(),
            to,
        });
        self.drive_screen_fade();
    }

    fn change_scene(&mut self, to: Scene) {
        match to {
            Scene::Lock => self.lock_now(),
            Scene::Unlock => self.unlock_now(),
        }
    }

    /// Réveil ou veille refusée : l'écran présent revient d'un coup, sans fondu.
    pub fn reveal_screen(&mut self) {
        if self.screen_fade == Some(ScreenFade::Dark) {
            self.screen_fade = None;
            self.request_redraw();
        }
    }

    /// Fait avancer le passage image après image, et enchaîne les étapes.
    fn drive_screen_fade(&mut self) {
        self.request_redraw();
        let timer = Timer::from_duration(SCREEN_FADE_STEP);
        let inserted = self.loop_handle.insert_source(timer, |_, _, state| {
            state.request_redraw();
            if state.step_screen_fade() {
                TimeoutAction::ToDuration(SCREEN_FADE_STEP)
            } else {
                TimeoutAction::Drop
            }
        });
        if let Err(error) = inserted {
            tracing::warn!(%error, "passage animé de l'écran indisponible");
            // Sans minuterie, on va droit au résultat.
            if let Some(ScreenFade::Capture { to, .. } | ScreenFade::Swap { to, .. }) = self.screen_fade {
                if matches!(self.screen_fade, Some(ScreenFade::Capture { .. })) {
                    self.change_scene(to);
                }
                self.screen_fade = None;
                self.screen_fade_shots.borrow_mut().clear();
            }
        }
    }

    /// Une étape du passage ; `false` quand il n'y a plus rien à animer.
    fn step_screen_fade(&mut self) -> bool {
        match self.screen_fade {
            Some(ScreenFade::Capture { since, to }) => {
                // La photo est prise au rendu ; sans rendu (écran éteint), on n'attend pas.
                if self.screen_fade_shots.borrow().is_empty() && since.elapsed() < CAPTURE_WAIT {
                    return true;
                }
                self.change_scene(to);
                self.screen_fade = Some(ScreenFade::swap(to, self.dock_config.reduce_motion));
                true
            }
            Some(fade @ ScreenFade::Swap { .. }) => {
                if !fade.finished() {
                    return true;
                }
                self.screen_fade = None;
                self.screen_fade_shots.borrow_mut().clear();
                false
            }
            Some(ScreenFade::Dark) | None => false,
        }
    }

    /// Verrouille la session : tout ce qui est ouvert au-dessus du bureau se referme.
    fn lock_now(&mut self) {
        if let Some(lock) = self.lock.as_mut() {
            lock.welcome_until = None;
            lock.checking = false;
            return;
        }
        self.popover = None;
        self.pending_power = None;
        self.switcher = None;
        self.region_select = None;
        self.close_overview();
        self.stop_widgets_overlay();
        self.lock = Some(Lock::new());
        self.refresh_face_ready();
        if let (Some(face), Some(lock)) = (self.demo_face, self.lock.as_mut()) {
            lock.face = face;
            lock.face_ready = true;
            lock.show_field();
        }
        self.set_media_watching(true);
        if let Some(lock) = self.lock.as_mut() {
            lock.media = self.media.current(Instant::now()).cloned();
        }
        self.focus_window(None);
        self.refresh_pointer_focus();
    }

    pub fn on_lock_event(&mut self, event: Event) {
        match event {
            // L'ordinateur s'endort déjà (capot fermé…) : verrouillé derrière le noir, pour
            // que le réveil dévoile l'écran verrouillé.
            Event::Lock => {
                self.lock_now();
                self.screen_fade = Some(ScreenFade::Dark);
            }
            Event::Resumed => {
                // Réveil (capot ouvert…) sans entrée : l'écran doit se rallumer.
                self.idle.activity();
                self.reveal_screen();
                // Retour de veille : la personne est devant l'écran.
                self.try_face_unlock();
            }
            Event::Face(result) => self.on_face_checked(result),
            Event::Checked(result) => {
                let accepted = match result {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        tracing::warn!(%error, "mot de passe non vérifié");
                        false
                    }
                };
                let Some(lock) = self.lock.as_mut() else {
                    return;
                };
                lock.forget_password();
                if accepted {
                    lock.welcome_until = Some(Instant::now() + WELCOME);
                    self.face_since_password = true;
                } else {
                    lock.checking = false;
                    lock.message = LockMessage::Wrong;
                    lock.refused_at = Some(Instant::now());
                }
            }
            Event::Media(list) => self.on_media(list),
        }
    }

    /// Caractère tapé. Au repos, la touche ne fait qu'afficher le champ : elle n'est pas
    /// écrite.
    pub fn lock_type(&mut self, character: char) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            if !lock.field_visible {
                lock.show_field();
                self.try_face_unlock();
                return;
            }
            lock.message = LockMessage::Hint;
            let at = lock.byte_at(lock.cursor);
            lock.password.insert(at, character);
            lock.cursor += 1;
        }
    }

    /// Retour arrière : le caractère avant le curseur.
    pub fn lock_erase(&mut self) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            lock.show_field();
            if lock.cursor > 0 {
                lock.cursor -= 1;
                let at = lock.byte_at(lock.cursor);
                lock.password.remove(at);
            }
        }
    }

    /// Suppr : le caractère après le curseur.
    pub fn lock_delete(&mut self) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            lock.show_field();
            if lock.cursor < lock.password_length() {
                let at = lock.byte_at(lock.cursor);
                lock.password.remove(at);
            }
        }
    }

    /// ← et → (`step` de -1 ou 1), Début et Fin (`step` de `isize::MIN` ou `isize::MAX`).
    pub fn lock_move_cursor(&mut self, step: isize) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            lock.show_field();
            let length = lock.password_length();
            lock.cursor = lock.cursor.saturating_add_signed(step).min(length);
        }
    }

    /// L'œil du champ, tenu enfoncé (`true`) puis relâché.
    pub fn lock_peek(&mut self, held: bool) {
        if let Some(lock) = self.lock.as_mut() {
            lock.peek = held && !lock.checking;
        }
    }

    /// Toute autre touche, ou un clic, fait apparaître le champ (et essaie le visage).
    pub fn lock_reveal(&mut self) {
        if let Some(lock) = self.lock.as_mut() {
            lock.show_field();
        }
        self.try_face_unlock();
    }

    /// Échap ou Annuler : retour au repos, le mot de passe commencé est oublié.
    pub fn lock_cancel(&mut self) {
        if let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) {
            lock.forget_password();
            lock.message = LockMessage::Hint;
            lock.refused_at = None;
            if matches!(lock.face, LockFace::Failed | LockFace::Unavailable) {
                lock.face = LockFace::Idle;
            }
            if lock.field_visible {
                lock.field_visible = false;
                lock.field_changed = Some(Instant::now());
            }
        }
    }

    /// Écran de connexion : verrouillé d'emblée (aucun bureau à quitter), le champ du mot
    /// de passe déjà ouvert.
    pub fn open_greeter(&mut self, greeter: crate::desktop::greeter::Greeter) {
        self.greeter = Some(greeter);
        self.lock_now();
        if let Some(lock) = self.lock.as_mut() {
            lock.show_field();
        }
    }

    /// Écran de connexion : compte suivant (1) ou précédent (-1). Le mot de passe commencé
    /// est oublié.
    pub fn greeter_step(&mut self, step: isize) {
        let Some(greeter) = self.greeter.as_mut() else {
            return;
        };
        if let Some(lock) = self
            .lock
            .as_mut()
            .filter(|lock| !lock.checking && !lock.unlocking())
        {
            greeter.step(step);
            lock.forget_password();
            lock.message = LockMessage::Hint;
            lock.refused_at = None;
            lock.show_field();
        }
    }

    /// Thème de cet écran, indépendant de celui de la session.
    pub fn lock_toggle_theme(&mut self) {
        if let Some(lock) = self.lock.as_mut() {
            lock.dark = !lock.dark;
            let mut settings = ferrite_system::config::Settings::load();
            settings.set("verrouillage_theme", if lock.dark { "sombre" } else { "clair" });
            if let Err(error) = settings.save() {
                tracing::warn!(%error, "thème de l'écran de verrouillage non enregistré");
            }
        }
    }

    pub fn lock_set_wall(&mut self, wall: usize) {
        let name = match LOCK_WALLS.get(wall) {
            Some((name, _, _)) => Some((*name).to_owned()),
            None => self
                .lock_picture
                .as_ref()
                .filter(|_| wall == LOCK_WALLS.len())
                .map(|picture| picture.path.to_string_lossy().into_owned()),
        };
        if let Some(lock) = self.lock.as_mut()
            && let Some(name) = name
        {
            lock.wall = wall;
            let mut settings = ferrite_system::config::Settings::load();
            settings.set("verrouillage_fond", &name);
            if let Err(error) = settings.save() {
                tracing::warn!(%error, "fond de l'écran de verrouillage non enregistré");
            }
        }
    }

    /// Le lecteur est toujours suivi : l'îlot du haut de l'écran montre la musique en
    /// cours (une lecture des propriétés MPRIS toutes les deux secondes).
    pub fn refresh_media_watching(&self) {
        self.set_media_watching(true);
    }

    pub fn set_media_watching(&self, watching: bool) {
        if let Some(link) = &self.media_link {
            link.watching.store(watching, Ordering::Relaxed);
        }
    }

    /// À chaque tour de boucle : la session s'ouvre une fois l'accueil passé.
    pub fn lock_tick(&mut self) {
        let ready = self
            .lock
            .as_ref()
            .and_then(|lock| lock.welcome_until)
            .is_some_and(|until| Instant::now() >= until);
        if ready && self.greeter.is_some() {
            // Connexion acceptée : greetd ouvre la session dès que cet écran se ferme.
            if !self.expected_exit {
                self.exit_code = 0;
                self.request_exit();
                self.loop_signal.stop();
            }
            return;
        }
        if ready && !matches!(self.screen_fade, Some(ScreenFade::Capture { .. })) {
            self.begin_swap(Scene::Unlock);
        }
    }

    /// Ouvre la session, juste avant que le bureau reparaisse sous l'écran verrouillé.
    fn unlock_now(&mut self) {
        if !self.lock.as_ref().is_some_and(Lock::unlocking) {
            return;
        }
        self.lock = None;
        self.refresh_media_watching();
        let top = self.top_window();
        self.focus_window(top);
        self.refresh_pointer_focus();
    }

    /// Entrée : la vérification se fait à côté, sans figer le bureau.
    pub fn lock_submit(&mut self) {
        let Some(events) = self.lock_events.clone() else {
            return;
        };
        let Some(lock) = self.lock.as_mut().filter(|lock| !lock.checking) else {
            return;
        };
        lock.show_field();
        if lock.password.is_empty() {
            lock.message = LockMessage::Empty;
            lock.refused_at = Some(Instant::now());
            return;
        }
        lock.message = LockMessage::Hint;
        lock.checking = true;
        let mut password = std::mem::take(&mut lock.password);
        lock.cursor = 0;
        lock.peek = false;
        if let Some(user) = self.greeter.as_ref().and_then(|greeter| greeter.current()) {
            let login = user.login.clone();
            std::thread::spawn(move || {
                let result = crate::desktop::greeter::login(&login, &password);
                wipe(&mut password);
                let _ = events.send(Event::Checked(result));
            });
            return;
        }
        std::thread::spawn(move || {
            let result = check(&user_name(), &password);
            if result == Ok(true)
                && let Err(err) = unlock_keyring(&password)
            {
                tracing::info!(%err, "trousseau non ouvert");
            }
            wipe(&mut password);
            let _ = events.send(Event::Checked(result));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_curseur_compte_en_caracteres() {
        let mut lock = Lock::new();
        lock.password = "éa€".to_owned();
        assert_eq!(lock.byte_at(0), 0);
        assert_eq!(lock.byte_at(1), 2);
        assert_eq!(lock.byte_at(2), 3);
        assert_eq!(lock.byte_at(3), 6);
        lock.forget_password();
        assert!(lock.password.is_empty() && lock.cursor == 0 && lock.peeked().is_none());
    }

    #[test]
    fn la_secousse_part_et_revient_au_centre() {
        assert_eq!(shake_offset(0.0), 0);
        assert_eq!(shake_offset(0.2), -100);
        assert_eq!(shake_offset(0.4), 90);
        assert_eq!(shake_offset(1.0), 0);
        assert_eq!(shake_offset(3.0), 0);
    }

    #[test]
    fn le_passage_part_et_arrive_sans_saut() {
        for to in [Scene::Lock, Scene::Unlock] {
            // Opacité de l'image du dessus (le bureau) et de celle du dessous (l'écran
            // verrouillé), et zoom de l'image du dessous.
            let layers = |t: f32| {
                let f = ScreenFade::swap_frame(to, t);
                if f.leaving_on_top {
                    (f.leaving_alpha, f.arriving_alpha, f.arriving_zoom)
                } else {
                    (f.arriving_alpha, f.leaving_alpha, f.leaving_zoom)
                }
            };
            let start = ScreenFade::swap_frame(to, 0.0);
            assert_eq!(start.leaving_zoom, 1.0);
            let end = ScreenFade::swap_frame(to, 1.0);
            assert_eq!(end.arriving_zoom, 1.0);
            // Seule l'image qui part se voit au début, seule celle qui arrive à la fin.
            let (top, _, _) = layers(0.0);
            assert_eq!(top, if to == Scene::Lock { 1.0 } else { 0.0 });
            let (top, _, _) = layers(1.0);
            assert_eq!(top, if to == Scene::Lock { 0.0 } else { 1.0 });
            for i in 0..=20 {
                let (_, below, zoom) = layers(i as f32 / 20.0);
                // Dessous, l'écran verrouillé reste plein et couvre tout l'écran : jamais
                // de cadre noir autour du bureau qui recule.
                assert_eq!(below, 1.0);
                assert!(zoom >= 1.0);
            }
        }
        // Le bureau recule en partant et revient de derrière.
        assert!(ScreenFade::swap_frame(Scene::Lock, 0.5).leaving_zoom < 1.0);
        assert!(ScreenFade::swap_frame(Scene::Unlock, 0.5).arriving_zoom < 1.0);
        // L'écran verrouillé arrive de devant et s'y envole en partant.
        assert!(ScreenFade::swap_frame(Scene::Lock, 0.5).arriving_zoom > 1.0);
        assert!(ScreenFade::swap_frame(Scene::Unlock, 0.5).leaving_zoom > 1.0);

        assert!(ScreenFade::swap(Scene::Lock, false).frame().is_some());
        assert!(!ScreenFade::swap(Scene::Lock, false).blocks_input());
        assert!(ScreenFade::Dark.blocks_input() && ScreenFade::Dark.frame().is_none());
    }

    #[test]
    fn reduire_les_animations_coupe_le_passage() {
        let swap = ScreenFade::swap(Scene::Lock, true);
        assert!(swap.finished());
        assert_eq!(swap.frame().map(|frame| frame.arriving_alpha), Some(1.0));
    }

    #[test]
    fn un_auxiliaire_absent_ne_deverrouille_pas() {
        assert!(check_with("/nexiste/pas", "moi", "secret").is_err());
    }

    #[test]
    fn la_reponse_de_l_auxiliaire_decide() {
        assert_eq!(check_with("/bin/true", "moi", "secret"), Ok(true));
        assert_eq!(check_with("/bin/false", "moi", "secret"), Ok(false));
    }

    #[test]
    fn le_mot_de_passe_est_efface() {
        let mut password = "secret".to_owned();
        wipe(&mut password);
        assert!(password.is_empty());
    }
}
