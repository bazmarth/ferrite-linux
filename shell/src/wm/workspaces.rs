//! Espaces de travail, comme sur macOS : des bureaux côte à côte, chacun avec ses fenêtres. On passe de l'un à l'autre en glissant trois doigts vers la
//! gauche ou la droite ; les pages suivent les doigts.
//!
//! Seules les fenêtres de la page affichée sont dans le `Space` : celles des autres bureaux
//! attendent ici, avec leur position. En mode dynamique (par défaut), il reste toujours un
//! bureau vide au bout, et un bureau vide qu'on quitte disparaît : il n'y a rien à créer ni
//! à supprimer à la main. En nombre fixe (Paramètres › Bureau et dock), les bureaux
//! ordinaires sont toujours là, vides ou non.

use std::time::{Duration, Instant};

use smithay::{
    desktop::Window,
    input::pointer::MotionEvent,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Size},
    wayland::seat::WaylandFocus,
};

use crate::state::Ferrite;

/// Durée du glissement jusqu'à la page d'arrivée.
const GLIDE: Duration = ferrite_ui::tokens::motion::LONG;
/// Course des doigts pour une page entière, en unités de libinput non accélérées : la page
/// suit les doigts quelle que soit leur vitesse. Réglé sur le pavé du ThinkBook, où un geste
/// franc fait 200 à 380 unités (mesuré avec `ferrite-profil`) : 300 font une demi-page.
const SWIPE_PAGE: f64 = 600.0;
/// Course (environ 0,8 mm) à partir de laquelle on décide du sens du geste : plus tôt, le
/// tremblement des doigts qui se posent en faisait souvent un geste vertical sans effet.
const SWIPE_LOCK: f64 = 30.0;
/// Un geste n'est vertical que s'il l'est nettement : changer de bureau est le plus courant.
const SWIPE_VERTICAL_RATIO: f64 = 1.6;
/// Part de l'élan ajoutée à la position au lever des doigts (ms) : un geste vif et court
/// suffit à changer de page.
const SWIPE_MOMENTUM: f64 = 180.0;
/// Sans mouvement depuis ce délai, les doigts levés n'ont plus d'élan.
const SWIPE_STILL: Duration = Duration::from_millis(100);
/// Course minimale d'un balayage vertical à trois doigts pour ouvrir ou fermer la vue
/// d'ensemble (les gestes francs mesurés font 76 à 380, les frôlements moins de 10).
const OVERVIEW_SWIPE: f64 = 70.0;
/// Marge totale d'un aperçu dans sa case, et côté de son bouton de fermeture.
const OVERVIEW_PADDING: i32 = 24;
pub const OVERVIEW_CLOSE: f64 = 28.0;
/// Au-delà de la première ou de la dernière page, les pages résistent.
const RUBBER_BAND: f64 = 0.2;
const RUBBER_LIMIT: f64 = 0.5;

/// Cases de la vue d'ensemble, disposées en grille et centrées dans la zone utile.
pub fn overview_tiles(output: Rectangle<i32, Logical>, count: usize) -> Vec<Rectangle<i32, Logical>> {
    if count == 0 {
        return Vec::new();
    }
    // Côte à côte tant que l'écran large s'y prête.
    let columns = ((count as f64 * output.size.w as f64 / output.size.h.max(1) as f64)
        .sqrt()
        .ceil() as usize)
        .clamp(1, count);
    let rows = count.div_ceil(columns);
    let margin_x = output.size.w / 10;
    // La bande des espaces occupe le haut ; le Dock, le bas.
    let margin_top = (output.size.h / 7).max(150);
    let margin_bottom = (output.size.h / 7).max(120);
    let gap_x = output.size.w / 40;
    let gap_y = output.size.h / 35;
    let area_width = output.size.w - 2 * margin_x - gap_x * (columns as i32 - 1);
    let area_height = output.size.h - margin_top - margin_bottom - gap_y * (rows as i32 - 1);
    let tile_width = (area_width / columns as i32).max(1);
    let tile_height = (area_height / rows as i32).max(1);

    (0..count)
        .map(|index| {
            let column = index % columns;
            let row = index / columns;
            Rectangle::new(
                output.loc
                    + Point::from((
                        margin_x + column as i32 * (tile_width + gap_x),
                        margin_top + row as i32 * (tile_height + gap_y),
                    )),
                Size::from((tile_width, tile_height)),
            )
        })
        .collect()
}

/// Cadre d'une fenêtre dans sa case : réduit pour y tenir, jamais agrandi, et centré.
pub fn overview_fit(tile: Rectangle<i32, Logical>, frame: Size<i32, Logical>) -> Rectangle<f64, Logical> {
    let factor = ((tile.size.w - OVERVIEW_PADDING).max(1) as f64 / frame.w.max(1) as f64)
        .min((tile.size.h - OVERVIEW_PADDING).max(1) as f64 / frame.h.max(1) as f64)
        .min(1.0);
    let (w, h) = (frame.w as f64 * factor, frame.h as f64 * factor);
    Rectangle::new(
        Point::from((
            tile.loc.x as f64 + (tile.size.w as f64 - w) / 2.0,
            tile.loc.y as f64 + (tile.size.h as f64 - h) / 2.0,
        )),
        Size::from((w, h)),
    )
}

/// Bouton de fermeture d'un aperçu, à cheval sur son coin haut droit.
pub fn overview_close_button(frame: Rectangle<f64, Logical>) -> Rectangle<f64, Logical> {
    let side = OVERVIEW_CLOSE;
    Rectangle::new(
        Point::from((frame.loc.x + frame.size.w - side / 2.0, frame.loc.y - side / 2.0)),
        Size::from((side, side)),
    )
}

/// Ce qui est sous le pointeur dans la vue d'ensemble : l'index de l'aperçu, et s'il s'agit
/// de son bouton de fermeture.
pub fn overview_hit_index(
    frames: &[Rectangle<f64, Logical>],
    pos: Point<f64, Logical>,
) -> Option<(usize, bool)> {
    frames
        .iter()
        .position(|frame| overview_close_button(*frame).contains(pos))
        .map(|index| (index, true))
        .or_else(|| {
            frames
                .iter()
                .position(|frame| frame.contains(pos))
                .map(|index| (index, false))
        })
}

/// Fenêtre d'un bureau qui n'est pas affiché.
#[derive(Debug, Clone)]
pub struct Parked {
    pub window: Window,
    pub location: Point<i32, Logical>,
}

impl Parked {
    /// Contenu de la fenêtre, à sa place sur son bureau.
    pub fn content(&self) -> Rectangle<i32, Logical> {
        Rectangle::new(self.location, self.window.geometry().size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Motion {
    Rest,
    /// Les doigts sont posés : `raw` est leur position en pages, sans résistance, et
    /// `velocity` leur vitesse en pages par milliseconde.
    Drag {
        raw: f64,
        velocity: f64,
        last: Instant,
    },
    /// Glissement depuis `from` vers la page courante.
    Glide {
        from: f64,
        started: Instant,
    },
}

/// Pages et fenêtres mises de côté (`T` : [`Parked`] ; les tests s'en passent).
#[derive(Debug)]
pub struct Workspaces<T> {
    /// Une entrée par bureau, de gauche à droite ; dans chacune, du dessous vers le dessus.
    /// L'entrée du bureau affiché reste vide : ses fenêtres sont dans le `Space`.
    desktops: Vec<Vec<T>>,
    current: usize,
    motion: Motion,
    /// Position imposée pour les captures (`--demo-page`).
    frozen: Option<f64>,
    /// Nombre fixe de bureaux ordinaires (sans compter celui des widgets) ; `None` :
    /// bureaux dynamiques.
    fixed: Option<usize>,
}

impl<T> Default for Workspaces<T> {
    fn default() -> Self {
        Self {
            desktops: vec![Vec::new()],
            current: 0,
            motion: Motion::Rest,
            frozen: None,
            fixed: None,
        }
    }
}

impl<T> Workspaces<T> {
    /// Au démarrage de la session : le bureau des widgets à gauche, et l'on arrive sur le
    /// premier bureau ordinaire (le bureau 1), là où sont les icônes et où s'ouvrent les
    /// fenêtres.
    pub fn at_start() -> Self {
        Self {
            desktops: vec![Vec::new(), Vec::new()],
            current: 1,
            ..Self::default()
        }
    }

    pub fn current(&self) -> usize {
        self.current
    }

    pub fn page_count(&self) -> usize {
        self.desktops.len()
    }

    pub fn is_dragging(&self) -> bool {
        matches!(self.motion, Motion::Drag { .. })
    }

    /// Met une fenêtre de côté sur `page`, au-dessus de celles qui y sont déjà.
    pub fn park(&mut self, page: usize, item: T) {
        if let Some(desktop) = self.desktops.get_mut(page) {
            desktop.push(item);
        }
    }

    /// Fenêtres mises de côté sur `page`.
    pub fn parked(&self, page: usize) -> &[T] {
        self.desktops.get(page).map_or(&[], Vec::as_slice)
    }

    /// Toutes les fenêtres mises de côté.
    pub fn all_parked(&self) -> impl Iterator<Item = &T> {
        self.desktops.iter().flatten()
    }

    /// Page qui garde une fenêtre répondant à `matches`.
    pub fn page_of(&self, mut matches: impl FnMut(&T) -> bool) -> Option<usize> {
        self.desktops
            .iter()
            .position(|desktop| desktop.iter().any(&mut matches))
    }

    /// Oublie les fenêtres mises de côté qui ne répondent pas à `keep`.
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        for desktop in &mut self.desktops {
            desktop.retain(&mut keep);
        }
    }

    /// Change de page : range `outgoing` sur la page quittée et rend les fenêtres de `page`.
    fn exchange(&mut self, page: usize, outgoing: Vec<T>) -> Vec<T> {
        if let Some(desktop) = self.desktops.get_mut(self.current) {
            *desktop = outgoing;
        }
        self.current = page;
        self.desktops
            .get_mut(page)
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Position affichée, en pages (entière au repos).
    pub fn position(&self, now: Instant) -> f64 {
        if let Some(frozen) = self.frozen {
            return frozen;
        }
        let target = self.current as f64;
        match self.motion {
            Motion::Rest => target,
            Motion::Drag { raw, .. } => self.resisted(raw),
            Motion::Glide { from, started } => {
                let t = now.duration_since(started).as_secs_f64() / GLIDE.as_secs_f64();
                let eased = 1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3);
                from + (target - from) * eased
            }
        }
    }

    /// Pages à dessiner et leur décalage, en largeurs d'écran.
    pub fn visible_pages(&self, now: Instant) -> Vec<(usize, f64)> {
        let position = self.position(now);
        let last = self.page_count() - 1;
        let left = (position.floor().max(0.0) as usize).min(last);
        let right = (position.ceil().max(0.0) as usize).min(last);
        let mut pages = vec![(left, left as f64 - position)];
        if right != left {
            pages.push((right, right as f64 - position));
        }
        pages
    }

    /// Le glissement est-il terminé ? Revient alors au repos.
    fn settle(&mut self, now: Instant) -> bool {
        match self.motion {
            Motion::Rest => true,
            Motion::Drag { .. } => false,
            Motion::Glide { started, .. } => {
                let over = now.duration_since(started) >= GLIDE;
                if over {
                    self.motion = Motion::Rest;
                }
                over
            }
        }
    }

    /// Glisse d'où l'on est jusqu'à la page courante, celle-ci venant d'être choisie ou non.
    fn glide_from(&mut self, from: f64, now: Instant) {
        self.motion = Motion::Glide { from, started: now };
    }

    /// Page limite d'un geste : une seule page à la fois.
    fn drag_bounds(&self) -> (f64, f64) {
        let last = self.page_count().saturating_sub(1);
        (
            self.current.saturating_sub(1) as f64,
            (self.current + 1).min(last) as f64,
        )
    }

    /// Position à l'écran : au-delà des limites, les pages résistent.
    fn resisted(&self, raw: f64) -> f64 {
        let (low, high) = self.drag_bounds();
        if raw < low {
            low + (raw - low) * RUBBER_BAND
        } else if raw > high {
            high + (raw - high) * RUBBER_BAND
        } else {
            raw
        }
    }

    /// Les doigts ont bougé de `pages` (positif : vers la page de droite).
    fn drag_by(&mut self, pages: f64, now: Instant) {
        let (raw, velocity, last) = match self.motion {
            Motion::Drag { raw, velocity, last } => (raw, velocity, last),
            _ => (self.position(now), 0.0, now),
        };
        let (low, high) = self.drag_bounds();
        let raw = (raw + pages).clamp(low - RUBBER_LIMIT, high + RUBBER_LIMIT);
        let elapsed = now.duration_since(last).as_secs_f64() * 1000.0;
        let velocity = if elapsed > 0.0 {
            0.6 * pages / elapsed + 0.4 * velocity
        } else {
            velocity
        };
        self.motion = Motion::Drag {
            raw,
            velocity,
            last: now,
        };
    }

    /// Doigts levés : commence le glissement et renvoie la page d'arrivée, à donner ensuite
    /// comme page courante.
    fn drag_end(&mut self, cancelled: bool, now: Instant) -> usize {
        let Motion::Drag { raw, velocity, last } = self.motion else {
            return self.current;
        };
        let from = self.resisted(raw);
        let velocity = if now.duration_since(last) > SWIPE_STILL {
            0.0
        } else {
            velocity
        };
        let moved = from + velocity * SWIPE_MOMENTUM - self.current as f64;
        let (low, high) = self.drag_bounds();
        let target = if cancelled || moved.abs() < 0.5 {
            self.current as f64
        } else if moved > 0.0 {
            high
        } else {
            low
        };
        self.glide_from(from, now);
        target as usize
    }

    /// Nombre fixe de bureaux ordinaires, ou `None` pour des bureaux dynamiques.
    pub fn set_fixed(&mut self, fixed: Option<usize>) {
        self.fixed = fixed.map(|count| count.max(1));
    }

    /// Retire les bureaux vides qu'on a quittés et garde un bureau vide au bout.
    /// `occupied` : le bureau affiché a des fenêtres. Rien ne bouge pendant un glissement.
    fn sync(&mut self, occupied: bool, now: Instant) {
        if !self.settle(now) {
            return;
        }
        if let Some(count) = self.fixed {
            self.sync_fixed(count);
            return;
        }
        let mut index = 0;
        while index < self.desktops.len() {
            let page = index;
            // Le bureau 0, celui des widgets, ne disparaît jamais.
            if page == 0
                || page == self.current
                || index + 1 == self.desktops.len()
                || !self.desktops[index].is_empty()
            {
                index += 1;
                continue;
            }
            self.desktops.remove(index);
            if self.current > page {
                self.current -= 1;
            }
        }
        let count = self.desktops.len();
        let last_is_free =
            self.desktops.last().is_some_and(Vec::is_empty) && !(self.current + 1 == count && occupied);
        if !last_is_free {
            self.desktops.push(Vec::new());
        }
    }

    /// Nombre fixe : les `count` bureaux ordinaires existent toujours. Au-delà (nombre
    /// réduit, fenêtre envoyée plus loin), seuls les bureaux vides et quittés disparaissent,
    /// en partant de la fin : aucune fenêtre n'est perdue.
    fn sync_fixed(&mut self, count: usize) {
        let pages = count + 1;
        self.ensure_page(pages - 1);
        while self.desktops.len() > pages
            && self.current + 1 < self.desktops.len()
            && self.desktops.last().is_some_and(Vec::is_empty)
        {
            self.desktops.pop();
        }
    }

    /// Crée les bureaux vides qui manquent jusqu'à `page`.
    pub fn ensure_page(&mut self, page: usize) {
        while self.desktops.len() <= page {
            self.desktops.push(Vec::new());
        }
    }

    /// Pour les captures : page courante et position figée.
    pub fn freeze(&mut self, position: f64) {
        self.frozen = Some(position);
        self.current = position.ceil().max(0.0) as usize;
        while self.page_count() <= self.current {
            self.desktops.push(Vec::new());
        }
    }
}

/// Geste à trois doigts en cours : on attend de savoir s'il est horizontal.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Swipe {
    total: (f64, f64),
    horizontal: Option<bool>,
}

impl Swipe {
    /// Ajoute un mouvement ; renvoie la course sur l'axe reconnu, le cas échéant.
    pub fn update(&mut self, dx: f64, dy: f64) -> Option<(f64, f64)> {
        self.total.0 += dx;
        self.total.1 += dy;
        match self.horizontal {
            Some(true) => Some((dx, 0.0)),
            Some(false) => None,
            None if self.total.0.hypot(self.total.1) < SWIPE_LOCK => None,
            None => {
                let horizontal = self.total.1.abs() < self.total.0.abs() * SWIPE_VERTICAL_RATIO;
                self.horizontal = Some(horizontal);
                Some(if horizontal {
                    (self.total.0, 0.0)
                } else {
                    (0.0, self.total.1)
                })
            }
        }
    }

    pub fn is_horizontal(&self) -> bool {
        self.horizontal == Some(true)
    }

    pub fn is_vertical(&self) -> bool {
        self.horizontal == Some(false)
    }

    pub fn vertical_distance(&self) -> f64 {
        self.total.1
    }
}

impl Ferrite {
    /// Fenêtre mise de côté ou réduite dont la surface principale est `surface` : elle
    /// peut toujours se redessiner, mais ses demandes (agrandissement…) attendent.
    pub fn hidden_window(
        &self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) -> Option<Window> {
        self.workspaces
            .all_parked()
            .map(|parked| &parked.window)
            .chain(self.minimized.iter().map(|minimized| &minimized.window))
            .find(|window| window.wl_surface().is_some_and(|own| *own == *surface))
            .cloned()
    }

    /// Page qui garde la fenêtre de surface `surface`.
    pub fn parked_page(
        &self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) -> Option<usize> {
        self.workspaces
            .page_of(|parked| parked.window.wl_surface().is_some_and(|own| *own == *surface))
    }

    /// Change de page avec le glissement (raccourci, clic sur la barre, Dock). Un déplacement
    /// de fenêtre en cours ou un geste en cours l'emportent.
    pub fn go_to_page(&mut self, page: usize) {
        let page = page.min(self.workspaces.page_count() - 1);
        let grabbed = self
            .seat
            .get_pointer()
            .is_some_and(|pointer| pointer.is_grabbed());
        if page == self.workspaces.current() || grabbed || self.workspaces.is_dragging() {
            return;
        }
        let now = Instant::now();
        let from = self.workspaces.position(now);
        self.workspaces.glide_from(from, now);
        self.swap_windows(page);
    }

    /// Envoie `window`, affichée, sur la page `page` ; on y reste.
    pub fn move_window_to_page(&mut self, window: &Window, page: usize) {
        let page = page.min(self.workspaces.page_count().saturating_sub(1));
        if page == self.workspaces.current() {
            return;
        }
        let Some(location) = self.space.element_location(window) else {
            return;
        };
        self.space.unmap_elem(window);
        window.set_activated(false);
        if let Some(toplevel) = window.toplevel() {
            toplevel.send_pending_configure();
        }
        self.workspaces.park(
            page,
            Parked {
                window: window.clone(),
                location,
            },
        );
        let next = self.top_window();
        self.focus_window(next);
    }

    /// Page voisine (`step` : -1 à gauche, 1 à droite).
    pub fn go_to_neighbor_page(&mut self, step: i32) {
        let page = self.workspaces.current() as i32 + step;
        if page >= 0 {
            self.go_to_page(page as usize);
        }
    }

    pub fn swipe_begin(&mut self, fingers: u32) {
        let free = self.lock.is_none()
            && self
                .seat
                .get_pointer()
                .is_some_and(|pointer| !pointer.is_grabbed());
        self.swipe = (matches!(fingers, 3 | 4) && free && self.pending_power.is_none()).then(Swipe::default);
    }

    pub fn swipe_update(&mut self, dx: f64, dy: f64) {
        if let Some((horizontal, _)) = self.swipe.as_mut().and_then(|swipe| swipe.update(dx, dy)) {
            if !self.overview && self.swipe.as_ref().is_some_and(Swipe::is_horizontal) {
                // Comme sur macOS, les pages suivent les doigts : vers la gauche, la page de droite.
                self.workspaces.drag_by(-horizontal / SWIPE_PAGE, Instant::now());
            }
        }
    }

    pub fn swipe_end(&mut self, cancelled: bool) {
        if let Some(swipe) = self.swipe.as_ref() {
            crate::render::profile::gesture(swipe.total, swipe.horizontal, cancelled);
        }
        if let Some(swipe) = self.swipe.take() {
            if swipe.is_horizontal() {
                if self.overview {
                    return;
                }
                let target = self.workspaces.drag_end(cancelled, Instant::now());
                if target != self.workspaces.current() {
                    self.swap_windows(target);
                }
            } else if swipe.is_vertical() && !cancelled {
                let distance = swipe.vertical_distance();
                if distance < -OVERVIEW_SWIPE {
                    self.open_overview();
                } else if distance > OVERVIEW_SWIPE {
                    self.close_overview();
                }
            }
        }
    }

    /// Ouvre la vue d'ensemble du bureau courant.
    pub fn open_overview(&mut self) {
        if self.overview {
            return;
        }
        self.popover = None;
        self.overview = true;
        self.overview_order = self.space.elements().rev().cloned().collect();
        self.refresh_pointer_focus();
    }

    pub fn close_overview(&mut self) {
        if self.overview {
            self.overview = false;
            self.refresh_pointer_focus();
        }
    }

    /// Aperçus de la vue d'ensemble : chaque fenêtre et son cadre de destination, dans
    /// l'ordre de l'ouverture, les fenêtres apparues depuis à la fin.
    pub fn overview_layout(&self, output: Rectangle<i32, Logical>) -> Vec<(Window, Rectangle<f64, Logical>)> {
        let in_space = |window: &&Window| self.space.elements().any(|other| other == *window);
        let added = self
            .space
            .elements()
            .rev()
            .filter(|window| !self.overview_order.contains(window));
        let windows: Vec<&Window> = self.overview_order.iter().filter(in_space).chain(added).collect();
        let tiles = overview_tiles(output, windows.len());
        windows
            .into_iter()
            .zip(tiles)
            .filter_map(|(window, tile)| {
                let frame = self.window_frame(window)?;
                Some((window.clone(), overview_fit(tile, frame.size)))
            })
            .collect()
    }

    /// Cadre d'une fenêtre à sa place sur le bureau, barre de titre comprise.
    pub fn window_frame(&self, window: &Window) -> Option<Rectangle<i32, Logical>> {
        let content = self.space.element_geometry(window)?;
        Some(if crate::wm::is_server_decorated(window) {
            crate::wm::decoration::frame(content)
        } else {
            content
        })
    }

    /// Aperçu sous le pointeur, et s'il s'agit de son bouton de fermeture.
    pub fn overview_hit(&self, pos: Point<f64, Logical>) -> Option<(Window, bool)> {
        let output = self.space.outputs().next()?;
        let layout = self.overview_layout(self.space.output_geometry(output)?);
        let frames: Vec<_> = layout.iter().map(|(_, frame)| *frame).collect();
        let (index, close) = overview_hit_index(&frames, pos)?;
        Some((layout[index].0.clone(), close))
    }

    /// Range les fenêtres de la page affichée, sort celles de `page` et leur rend le clavier.
    fn swap_windows(&mut self, page: usize) {
        let windows: Vec<Window> = self.space.elements().cloned().collect();
        let mut outgoing = Vec::with_capacity(windows.len());
        for window in windows {
            let location = self.space.element_location(&window);
            self.space.unmap_elem(&window);
            window.set_activated(false);
            if let Some(toplevel) = window.toplevel() {
                toplevel.send_pending_configure();
            }
            // Les menus et bulles X11 ne suivent pas : ils se referment avec leur fenêtre.
            let transient = window.x11_surface().is_some_and(|x11| x11.is_override_redirect());
            if let (Some(location), false) = (location, transient) {
                outgoing.push(Parked { window, location });
            }
        }
        // Une fenêtre rangée n'est plus dessinée par son animation.
        self.window_animations.borrow_mut().retain(|animation| {
            !outgoing
                .iter()
                .any(|parked: &Parked| parked.window == animation.window)
        });
        for parked in self.workspaces.exchange(page, outgoing) {
            self.space.map_element(parked.window, parked.location, false);
        }
        self.popover = None;
        self.dock_menu = None;
        self.window_menu = None;
        self.dashboard_editing = None;
        self.pressed_light = None;
        let next = self
            .space
            .elements()
            .rev()
            .find(|window| !window.x11_surface().is_some_and(|x11| x11.is_override_redirect()))
            .cloned();
        self.focus_window(next);
        self.refresh_pointer_focus();
    }

    /// La fenêtre sous le pointeur a changé sans qu'il bouge.
    pub fn refresh_pointer_focus(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let location = pointer.current_location();
        let under = self.surface_under(location);
        let motion = MotionEvent {
            location,
            serial: SERIAL_COUNTER.next_serial(),
            time: self.start_time.elapsed().as_millis() as u32,
        };
        self.forget_cursor_on_leave(&pointer, under.as_ref());
        pointer.motion(self, under, &motion);
        pointer.frame(self);
    }

    /// Le pointeur change de surface : la forme demandée par l'ancienne (barre de texte…)
    /// est oubliée ; la nouvelle redonne la sienne en recevant le pointeur.
    pub fn forget_cursor_on_leave(
        &mut self,
        pointer: &smithay::input::pointer::PointerHandle<Self>,
        under: Option<&(
            smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
            Point<f64, Logical>,
        )>,
    ) {
        if pointer.current_focus().as_ref() != under.map(|(surface, _)| surface) {
            self.cursor_status = smithay::input::pointer::CursorImageStatus::default_named();
        }
    }

    /// À chaque tour de boucle : met à jour la liste des bureaux.
    pub fn sync_desktops(&mut self) {
        self.keep_widgets_page_free();
        self.workspaces.set_fixed(self.dock_config.fixed_workspaces);
        let occupied = self
            .space
            .elements()
            .any(|window| !window.x11_surface().is_some_and(|x11| x11.is_override_redirect()));
        self.workspaces.sync(occupied, Instant::now());
    }

    /// Une fenêtre va s'ouvrir alors qu'on est sur le bureau des widgets : on passe d'abord
    /// au bureau 1, où elle s'ouvrira normalement.
    pub fn leave_widgets_page(&mut self) {
        if self.workspaces.current() == 0 && !self.overview {
            self.workspaces.ensure_page(1);
            self.go_to_page(1);
        }
    }

    /// Le bureau 0 est réservé aux widgets : une fenêtre qui s'y ouvre (ou y revient) part sur
    /// le bureau 1, et l'affichage la suit. Rien ne bouge pendant un déplacement à la souris.
    fn keep_widgets_page_free(&mut self) {
        if self.workspaces.current() != 0 || self.overview || self.workspaces.is_dragging() {
            return;
        }
        let grabbed = self
            .seat
            .get_pointer()
            .is_some_and(|pointer| pointer.is_grabbed());
        let windows: Vec<Window> = self
            .space
            .elements()
            .filter(|window| !window.x11_surface().is_some_and(|x11| x11.is_override_redirect()))
            // Une fenêtre pas encore configurée ne se montrerait jamais : elle attend son tour.
            .filter(|window| !crate::protocols::xdg_shell::awaiting_placement(window))
            .cloned()
            .collect();
        if windows.is_empty() || grabbed {
            return;
        }
        self.workspaces.ensure_page(1);
        for window in &windows {
            self.move_window_to_page(window, 1);
        }
        self.go_to_page(1);
        if let Some(window) = windows.last() {
            self.focus_window(Some(window.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Instant {
        // Un instant de référence commun à chaque appel du test.
        thread_local!(static START: Instant = Instant::now());
        START.with(|start| *start + Duration::from_millis(ms))
    }

    /// Quatre bureaux, le dernier vide ; on est sur le deuxième.
    fn three_desktops() -> Workspaces<u32> {
        Workspaces {
            desktops: vec![vec![0], vec![1], vec![2], Vec::new()],
            current: 1,
            ..Workspaces::default()
        }
    }

    #[test]
    fn on_commence_sur_le_premier_bureau() {
        let pages = Workspaces::<u32>::default();
        assert_eq!(pages.current(), 0);
        assert_eq!(pages.page_count(), 1);
        assert!(pages.parked(0).is_empty());
    }

    #[test]
    fn la_session_s_ouvre_sur_le_bureau_1() {
        let mut pages = Workspaces::<u32>::at_start();
        assert_eq!(pages.current(), 1);
        // Sans fenêtre, ni bureau en plus ni bureau en moins.
        pages.sync(false, at(0));
        assert_eq!(pages.page_count(), 2);
        assert_eq!(pages.current(), 1);
    }

    #[test]
    fn chaque_bureau_garde_ses_fenetres() {
        let mut pages = Workspaces::<u32> {
            desktops: vec![Vec::new(), Vec::new(), vec![7, 8], Vec::new()],
            current: 1,
            ..Workspaces::default()
        };
        // Le bureau 1 affiche la fenêtre 5 ; on passe au bureau 2.
        let incoming = pages.exchange(2, vec![5]);
        assert_eq!(incoming, vec![7, 8]);
        assert_eq!(pages.current(), 2);
        assert_eq!(pages.parked(1), &[5]);
        assert!(pages.parked(2).is_empty());
        assert_eq!(pages.page_of(|window| *window == 5), Some(1));
        // Et retour : la fenêtre 5 revient, les 7 et 8 restent sur leur bureau.
        let incoming = pages.exchange(1, vec![7, 8]);
        assert_eq!(incoming, vec![5]);
        assert_eq!(pages.parked(2), &[7, 8]);
    }

    #[test]
    fn le_bureau_des_widgets_ne_disparait_jamais() {
        let now = at(0);
        // Bureau 0 vide, quitté pour le bureau 1 qui a une fenêtre.
        let mut pages = Workspaces::<u32> {
            desktops: vec![Vec::new(), Vec::new(), Vec::new()],
            current: 1,
            ..Workspaces::default()
        };
        pages.sync(true, now);
        assert_eq!(pages.page_count(), 3);
        assert_eq!(pages.current(), 1);
        pages.ensure_page(4);
        assert_eq!(pages.page_count(), 5);
    }

    #[test]
    fn un_bureau_vide_reste_toujours_au_bout() {
        let now = at(0);
        let mut pages = Workspaces::<u32>::default();
        // Premier bureau vide : pas de bureau de plus.
        pages.sync(false, now);
        assert_eq!(pages.page_count(), 1);
        // Une fenêtre s'y ouvre : un nouveau bureau vide apparaît.
        pages.sync(true, now);
        assert_eq!(pages.page_count(), 2);
        pages.sync(true, now);
        assert_eq!(pages.page_count(), 2);
    }

    #[test]
    fn un_bureau_vide_quitte_disparait() {
        let now = at(0);
        let mut pages = Workspaces::<u32> {
            desktops: vec![vec![0], vec![1], Vec::new(), vec![3], Vec::new()],
            current: 4,
            ..Workspaces::default()
        };
        pages.sync(false, now);
        assert_eq!(pages.page_count(), 4);
        assert_eq!(pages.current(), 3);
        assert_eq!(pages.parked(2), &[3]);
        // Le bureau affiché reste, même vide et au milieu.
        let mut pages = Workspaces::<u32> {
            desktops: vec![vec![0], vec![1], Vec::new(), vec![3], Vec::new()],
            current: 2,
            ..Workspaces::default()
        };
        pages.sync(false, now);
        assert_eq!(pages.current(), 2);
        assert_eq!(pages.page_count(), 5);
    }

    #[test]
    fn en_nombre_fixe_les_bureaux_restent() {
        let mut pages: Workspaces<u32> = Workspaces::at_start();
        pages.set_fixed(Some(4));
        pages.sync(false, at(0));
        // Le bureau des widgets et quatre bureaux ordinaires, même vides.
        assert_eq!(pages.page_count(), 5);
        // Une fenêtre sur le dernier : pas de bureau de plus.
        pages.current = 4;
        pages.sync(true, at(0));
        assert_eq!(pages.page_count(), 5);
        // On quitte un bureau vide du milieu : il reste.
        pages.current = 1;
        pages.sync(false, at(0));
        assert_eq!(pages.page_count(), 5);
    }

    #[test]
    fn nombre_fixe_reduit_sans_perdre_de_fenetre() {
        let mut pages: Workspaces<u32> = Workspaces::at_start();
        pages.ensure_page(6);
        pages.park(5, 7);
        pages.set_fixed(Some(2));
        pages.sync(false, at(0));
        // Le bureau 6, vide, disparaît ; le 5 garde sa fenêtre.
        assert_eq!(pages.page_count(), 6);
        assert_eq!(pages.parked(5), &[7]);
        // Repassé en dynamique : les bureaux vides quittés disparaissent, un seul vide au bout.
        pages.set_fixed(None);
        pages.sync(false, at(0));
        assert_eq!(pages.page_count(), 4);
        assert_eq!(pages.parked(2), &[7]);
    }

    #[test]
    fn rien_ne_disparait_pendant_un_glissement() {
        let mut pages = three_desktops();
        pages.current = 3;
        pages.desktops[1].clear();
        pages.glide_from(1.0, at(0));
        pages.sync(false, at(10));
        assert_eq!(pages.page_count(), 4);
        pages.sync(false, at(1000));
        assert_eq!(pages.page_count(), 3);
    }

    #[test]
    fn le_glissement_arrive_sur_la_page_courante() {
        let mut pages = three_desktops();
        pages.glide_from(1.0, at(0));
        pages.exchange(2, vec![]);
        assert_eq!(pages.position(at(0)), 1.0);
        let middle = pages.position(at(100));
        assert!(middle > 1.0 && middle < 2.0);
        assert_eq!(pages.position(at(GLIDE.as_millis() as u64)), 2.0);
    }

    #[test]
    fn deux_pages_visibles_pendant_le_geste() {
        let mut pages = three_desktops();
        pages.drag_by(-0.25, at(0));
        assert_eq!(pages.visible_pages(at(0)), vec![(0, -0.75), (1, 0.25)]);
        pages.motion = Motion::Rest;
        assert_eq!(pages.visible_pages(at(0)), vec![(1, 0.0)]);
    }

    #[test]
    fn un_geste_lent_et_court_revient_en_place() {
        let mut pages = three_desktops();
        pages.drag_by(0.1, at(0));
        pages.drag_by(0.1, at(100));
        assert_eq!(pages.drag_end(false, at(100)), 1);
    }

    #[test]
    fn un_geste_ample_ou_vif_change_de_page() {
        let mut pages = three_desktops();
        for step in 0..7 {
            pages.drag_by(0.1, at(step * 40));
        }
        assert_eq!(pages.drag_end(false, at(240)), 2);
        // Vif : la moitié de la course n'est pas atteinte, mais l'élan y mène.
        let mut pages = three_desktops();
        pages.drag_by(-0.1, at(0));
        pages.drag_by(-0.1, at(8));
        assert_eq!(pages.drag_end(false, at(10)), 0);
    }

    #[test]
    fn un_geste_annule_ou_arrete_ne_change_pas_de_page() {
        let mut pages = three_desktops();
        pages.drag_by(0.9, at(0));
        assert_eq!(pages.drag_end(true, at(1)), 1);
        // Arrêté avant de lever les doigts, sous la moitié : pas d'élan.
        let mut pages = three_desktops();
        pages.drag_by(0.1, at(0));
        pages.drag_by(0.2, at(8));
        assert_eq!(pages.drag_end(false, at(500)), 1);
    }

    #[test]
    fn un_geste_ne_saute_qu_une_page_et_resiste_aux_extremites() {
        let mut pages = three_desktops();
        pages.drag_by(5.0, at(0));
        assert!(pages.position(at(0)) <= 2.0 + RUBBER_LIMIT * RUBBER_BAND);
        assert_eq!(pages.drag_end(false, at(1)), 2);
        // Vers la gauche depuis le premier bureau : rien, les pages résistent puis reviennent.
        let mut pages = three_desktops();
        pages.current = 0;
        pages.drag_by(-0.4, at(0));
        let position = pages.position(at(0));
        assert!(position < 0.0 && position > -0.4 * RUBBER_BAND - 1e-9);
        assert_eq!(pages.drag_end(false, at(1)), 0);
    }

    #[test]
    fn seul_un_geste_horizontal_compte() {
        let mut swipe = Swipe::default();
        assert_eq!(swipe.update(10.0, 4.0), None);
        // Le seuil franchi, toute la course déjà faite est appliquée.
        assert_eq!(swipe.update(20.0, 4.0), Some((30.0, 0.0)));
        assert_eq!(swipe.update(-2.0, 50.0), Some((-2.0, 0.0)));
        assert!(swipe.is_horizontal());
        assert!(!swipe.is_vertical());

        // Un peu plus vertical qu'horizontal : encore un changement de bureau.
        let mut swipe = Swipe::default();
        assert_eq!(swipe.update(20.0, 25.0), Some((20.0, 0.0)));
        assert!(swipe.is_horizontal());

        // Nettement vertical : la vue d'ensemble.
        let mut swipe = Swipe::default();
        assert_eq!(swipe.update(3.0, 30.0), Some((0.0, 30.0)));
        assert_eq!(swipe.update(100.0, 0.0), None);
        assert!(!swipe.is_horizontal());
        assert!(swipe.is_vertical());
        assert_eq!(swipe.vertical_distance(), 30.0);
    }

    #[test]
    fn les_apercus_de_fenetres_sont_dans_une_grille_centree() {
        let output = Rectangle::new((10, 20).into(), (1440, 900).into());
        assert!(overview_tiles(output, 0).is_empty());
        let tiles = overview_tiles(output, 6);
        assert_eq!(tiles.len(), 6);
        assert!(tiles.iter().all(|tile| {
            tile.loc.x >= output.loc.x
                && tile.loc.y >= output.loc.y
                && tile.loc.x + tile.size.w <= output.loc.x + output.size.w
                && tile.loc.y + tile.size.h <= output.loc.y + output.size.h
        }));
    }

    #[test]
    fn un_apercu_tient_dans_sa_case_sans_etre_agrandi() {
        let tile = Rectangle::new(Point::from((100, 100)), Size::from((424, 324)));
        let large = overview_fit(tile, Size::from((800, 600)));
        assert_eq!(large.size, Size::from((400.0, 300.0)));
        assert_eq!(large.loc, Point::from((112.0, 112.0)));
        let small = overview_fit(tile, Size::from((100, 50)));
        assert_eq!(small.size, Size::from((100.0, 50.0)));
    }

    #[test]
    fn le_bouton_de_fermeture_passe_avant_l_apercu() {
        let frames = [
            Rectangle::new(Point::from((0.0, 0.0)), Size::from((100.0, 100.0))),
            Rectangle::new(Point::from((200.0, 0.0)), Size::from((100.0, 100.0))),
        ];
        assert_eq!(
            overview_hit_index(&frames, Point::from((250.0, 50.0))),
            Some((1, false))
        );
        // Au coin, et même juste au-dehors : c'est le bouton.
        assert_eq!(
            overview_hit_index(&frames, Point::from((99.0, 1.0))),
            Some((0, true))
        );
        assert_eq!(
            overview_hit_index(&frames, Point::from((110.0, -10.0))),
            Some((0, true))
        );
        assert_eq!(overview_hit_index(&frames, Point::from((150.0, 50.0))), None);
    }

    #[test]
    fn une_capture_peut_figer_la_position() {
        let mut pages = Workspaces::<u32>::default();
        pages.freeze(2.5);
        assert_eq!(pages.current(), 3);
        assert_eq!(pages.position(at(0)), 2.5);
        assert_eq!(pages.visible_pages(at(0)), vec![(2, -0.5), (3, 0.5)]);
    }
}
