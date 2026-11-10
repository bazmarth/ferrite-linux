//! Logithèque (`ferrite-apps logitheque`) : trouver, installer, désinstaller et mettre à
//! jour des applications, des dépôts de Debian (PackageKit) ou de Flathub (Flatpak).
//!
//! Le catalogue vient d'AppStream (`appstream.rs`) : Debian et Flathub publient eux-mêmes
//! la liste de leurs vraies applications, avec nom, description, catégories, icône et
//! captures. Tout ce qui attend (lecture du catalogue, installation, réseau) se fait dans un
//! fil à part ; l'interface reçoit des messages.

mod appstream;
pub mod drivers;
mod flatpak;
mod media;
pub mod notifier;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

use eframe::egui::{self, FontId, Frame, Margin, RichText, Sense};
use ferrite_system::config::Accent;

use self::appstream::{App, Block, Component, Origin};
use crate::{kit, packagekit, style};

/// Largeur de la colonne, et d'une carte d'application dans la grille.
const SIDEBAR: f32 = 220.0;
const CARD_MIN: f32 = 280.0;
const CARD_HEIGHT: f32 = 98.0;
const GAP: f32 = 12.0;

/// Applications mises en avant sur l'accueil, si le catalogue les connaît.
const SELECTION: &[&str] = &[
    "org.videolan.VLC",
    "org.mozilla.firefox",
    "org.libreoffice.LibreOffice",
    "org.gimp.GIMP",
    "com.spotify.Client",
    "com.visualstudio.code",
    "org.inkscape.Inkscape",
    "org.kde.krita",
    "com.obsproject.Studio",
    "org.audacityteam.Audacity",
    "org.blender.Blender",
    "org.kde.kdenlive",
    "org.mozilla.Thunderbird",
    "org.telegram.desktop",
    "com.discordapp.Discord",
    "com.valvesoftware.Steam",
    "org.signal.Signal",
    "com.github.tchx84.Flatseal",
];

/// Rubriques : catégorie freedesktop, nom, symbole, couleur de la pastille.
const CATEGORIES: [(&str, &str, &str, Accent); 10] = [
    ("AudioVideo", "Son et vidéo", "glyph-play", Accent::Red),
    ("Graphics", "Graphisme et photo", "glyph-brush", Accent::Purple),
    ("Office", "Bureautique", "glyph-doc", Accent::Blue),
    ("Network", "Internet", "glyph-globe", Accent::Teal),
    ("Game", "Jeux", "glyph-game", Accent::Green),
    ("Education", "Éducation", "glyph-book", Accent::Orange),
    ("Development", "Développement", "glyph-code", Accent::Graphite),
    ("Science", "Sciences", "glyph-flask", Accent::Pink),
    ("Utility", "Accessoires", "glyph-grid", Accent::Yellow),
    ("System", "Système", "glyph-screen", Accent::Graphite),
];

fn in_category(component: &Component, key: &str) -> bool {
    component.categories.iter().any(|category| {
        category == key || (key == "AudioVideo" && matches!(category.as_str(), "Audio" | "Video"))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Discover,
    Category(usize),
    Installed,
    Updates,
    Drivers,
}

/// Opération en cours (une seule à la fois : PackageKit les fait de toute façon à la file).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    App(String, Origin),
    Deb(PathBuf),
    /// Pilote graphique (nom du paquet).
    Driver(&'static str),
    Updates,
    DebianCatalog,
    Flathub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    Install,
    Remove,
    Update,
}

#[derive(Debug, Clone)]
struct Operation {
    target: Target,
    verb: Verb,
    /// Nom affiché (« VLC »).
    name: String,
    percent: Option<u32>,
}

impl Operation {
    fn label(&self) -> String {
        let action = match self.verb {
            Verb::Install => "Installation",
            Verb::Remove => "Désinstallation",
            Verb::Update => "Mise à jour",
        };
        match self.percent {
            Some(percent) => format!("{action} de {}… {percent}\u{a0}%", self.name),
            None => format!("{action} de {}…", self.name),
        }
    }
}

/// Fichier `.deb` ouvert depuis Fichiers.
#[derive(Debug, Clone, Default)]
struct DebFile {
    path: PathBuf,
    package: String,
    version: String,
    summary: String,
    description: String,
    maintainer: String,
    homepage: String,
    /// Taille installée, en kio (champ `Installed-Size`).
    installed_kib: Option<u64>,
    error: Option<String>,
}

/// Mises à jour connues.
#[derive(Debug, Clone)]
struct Updates {
    debian: Result<Vec<packagekit::Package>, String>,
    flatpak: Result<Vec<String>, String>,
}

impl Updates {
    fn count(&self) -> usize {
        self.debian.as_ref().map_or(0, Vec::len) + self.flatpak.as_ref().map_or(0, Vec::len)
    }
}

enum Msg {
    Catalog {
        apps: Vec<App>,
        debian: bool,
        flathub: bool,
    },
    Installed {
        debian: HashSet<String>,
        flatpak: Vec<flatpak::Installed>,
    },
    Size(String, Origin, Option<String>),
    Deb(DebFile),
    Updates(Updates),
    Progress(u32),
    /// Fin de l'opération : message à afficher, ou erreur.
    Done(Result<String, String>),
}

/// Fiche ouverte.
#[derive(Debug, Clone)]
struct Detail {
    id: String,
    origin: Origin,
    shot: usize,
}

pub struct StoreState {
    ctx: egui::Context,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    apps: Arc<Vec<App>>,
    loading: bool,
    has_debian: bool,
    has_flathub: bool,
    installed_debian: HashSet<String>,
    installed_flatpak: Vec<flatpak::Installed>,
    page: Page,
    search: String,
    detail: Option<Detail>,
    deb: Option<DebFile>,
    sizes: HashMap<(String, Origin), Option<String>>,
    updates: Option<Updates>,
    checking_updates: bool,
    operation: Option<Operation>,
    /// Pilotes graphiques utiles et absents (`drivers.rs`).
    drivers: Vec<drivers::Driver>,
    /// Un pilote vient d'être installé : il faut redémarrer.
    restart_needed: bool,
    /// Système live : un pilote installé serait perdu au redémarrage.
    live: bool,
    /// Dernier message (réussite ou erreur).
    notice: Option<Result<String, String>>,
    media: media::Media,
    icons: style::Icons,
}

impl std::fmt::Debug for StoreState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreState")
            .field("apps", &self.apps.len())
            .field("page", &self.page)
            .field("operation", &self.operation)
            .finish_non_exhaustive()
    }
}

impl StoreState {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Self {
            ctx: ctx.clone(),
            tx,
            rx,
            apps: Arc::new(Vec::new()),
            loading: true,
            has_debian: true,
            has_flathub: true,
            installed_debian: HashSet::new(),
            installed_flatpak: Vec::new(),
            page: Page::Discover,
            search: String::new(),
            detail: None,
            deb: None,
            sizes: HashMap::new(),
            updates: None,
            checking_updates: false,
            operation: None,
            drivers: drivers::missing(),
            restart_needed: false,
            live: drivers::is_live(),
            notice: None,
            media: media::Media::new(ctx),
            icons: style::Icons::default(),
        };
        state.spawn(load_catalog);
        state.spawn(load_installed);
        state
    }

    /// Rubrique ou fichier demandé : `mises-a-jour`, `installees`, `recherche:mots`, un
    /// identifiant d'application ou un fichier `.deb`.
    pub fn open(&mut self, target: PathBuf) {
        match target.to_str() {
            Some("mises-a-jour") => self.show_updates(false),
            Some("installees") => self.page = Page::Installed,
            Some("pilotes") => self.page = Page::Drivers,
            Some(text) if text.starts_with("recherche:") => {
                self.search = text["recherche:".len()..].to_owned();
            }
            _ if target
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("deb")) =>
            {
                self.detail = None;
                self.deb = Some(DebFile {
                    path: target.clone(),
                    ..DebFile::default()
                });
                self.spawn(move || Msg::Deb(read_deb(&target)));
            }
            _ => {
                // Identifiant d'application (`ferrite-apps logitheque org.videolan.VLC`).
                let id = target.to_string_lossy().into_owned();
                self.detail = Some(Detail {
                    id,
                    origin: Origin::Debian,
                    shot: 0,
                });
            }
        }
    }

    fn spawn(&self, job: impl FnOnce() -> Msg + Send + 'static) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
    }

    /// Lance une opération longue ; `progress` envoie l'avancement à l'interface.
    fn start(
        &mut self,
        target: Target,
        verb: Verb,
        name: &str,
        job: impl FnOnce(&dyn Fn(u32)) -> Result<String, String> + Send + 'static,
    ) {
        if self.operation.is_some() {
            return;
        }
        self.notice = None;
        self.operation = Some(Operation {
            target,
            verb,
            name: name.to_owned(),
            percent: None,
        });
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            let progress = |percent: u32| {
                let _ = tx.send(Msg::Progress(percent));
                ctx.request_repaint();
            };
            let result = job(&progress);
            let _ = tx.send(Msg::Done(result));
            ctx.request_repaint();
        });
    }

    fn receive(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Msg::Catalog {
                    apps,
                    debian,
                    flathub,
                } => {
                    self.apps = Arc::new(apps);
                    self.loading = false;
                    self.has_debian = debian;
                    self.has_flathub = flathub;
                    self.fix_detail_origin();
                }
                Msg::Installed { debian, flatpak } => {
                    self.installed_debian = debian;
                    self.installed_flatpak = flatpak;
                }
                Msg::Size(id, origin, size) => {
                    self.sizes.insert((id, origin), size);
                }
                Msg::Deb(deb) => self.deb = Some(deb),
                Msg::Updates(updates) => {
                    self.updates = Some(updates);
                    self.checking_updates = false;
                }
                Msg::Progress(percent) => {
                    if let Some(operation) = self.operation.as_mut() {
                        operation.percent = Some(percent);
                    }
                }
                Msg::Done(result) => {
                    let finished = self.operation.take();
                    self.notice = Some(result.clone());
                    self.spawn(load_installed);
                    if let Some(finished) = finished
                        && result.is_ok()
                    {
                        match finished.target {
                            Target::Updates => self.show_updates(false),
                            Target::DebianCatalog | Target::Flathub => {
                                self.loading = true;
                                self.spawn(load_catalog);
                            }
                            Target::Driver(_) => {
                                self.drivers = drivers::missing();
                                self.restart_needed = true;
                            }
                            Target::App(..) | Target::Deb(_) => {
                                // Les mises à jour connues peuvent avoir changé.
                                if self.updates.is_some() {
                                    self.show_updates(false);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Une fiche ouverte par identifiant avant la fin du chargement : prendre l'origine
    /// qui existe vraiment.
    fn fix_detail_origin(&mut self) {
        let apps = Arc::clone(&self.apps);
        if let Some(detail) = self.detail.as_mut()
            && let Some(app) = find(&apps, &detail.id)
            && app.variant(detail.origin).is_none()
        {
            detail.origin = app.main().origin();
        }
    }

    fn show_updates(&mut self, refresh: bool) {
        self.page = Page::Updates;
        self.detail = None;
        self.deb = None;
        if self.checking_updates {
            return;
        }
        self.checking_updates = true;
        self.spawn(move || {
            Msg::Updates(Updates {
                debian: packagekit::check(refresh),
                flatpak: flatpak::updates(),
            })
        });
    }

    fn is_installed(&self, component: &Component) -> bool {
        match component.origin() {
            Origin::Debian => self.installed_debian.contains(&component.package),
            Origin::Flathub => self.installed_flatpak.iter().any(|app| app.id == component.id),
        }
    }

    fn app_installed(&self, app: &App) -> bool {
        app.variants.iter().any(|variant| self.is_installed(variant))
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        self.receive();
        self.media.poll(&self.ctx);
        style::sidebar(
            ui,
            "logitheque-colonne",
            SIDEBAR,
            style::sidebar_color(),
            true,
            |ui| {
                self.sidebar(ui);
            },
        );
        style::toolbar(ui, true, |ui| self.toolbar(ui));
        if let Some(operation) = self.operation.clone() {
            style::status_bar(ui, |ui| {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(RichText::new(operation.label()).size(13.0));
                if let Some(percent) = operation.percent {
                    ui.add(egui::ProgressBar::new(percent as f32 / 100.0).desired_width(160.0));
                }
            });
        } else if let Some(notice) = self.notice.clone() {
            style::status_bar(ui, |ui| {
                match &notice {
                    Ok(text) => ui.colored_label(style::success(), text),
                    Err(text) => ui.colored_label(style::error(), text),
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("OK").clicked() {
                        self.notice = None;
                    }
                });
            });
        }
        egui::CentralPanel::no_frame()
            .frame(
                Frame::new()
                    .fill(style::background())
                    .inner_margin(Margin::symmetric(28, 0)),
            )
            .show(ui, |ui| {
                if self.deb.is_some() {
                    self.deb_page(ui);
                } else if let Some(detail) = self.detail.clone() {
                    self.detail_page(ui, &detail);
                } else if !self.search.trim().is_empty() {
                    self.search_page(ui);
                } else {
                    match self.page {
                        Page::Discover => self.discover_page(ui),
                        Page::Category(index) => self.category_page(ui, index),
                        Page::Installed => self.installed_page(ui),
                        Page::Updates => self.updates_page(ui),
                        Page::Drivers => self.drivers_page(ui),
                    }
                }
            });
    }

    // ———————————————————————————— Cadre ————————————————————————————

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let browsing = self.detail.is_none() && self.deb.is_none() && self.search.trim().is_empty();
            let mut go = None;
            if style::sidebar_tile_item(
                ui,
                &mut self.icons,
                "glyph-star",
                style::accent_color(Accent::Blue),
                "Découvrir",
                browsing && self.page == Page::Discover,
            )
            .clicked()
            {
                go = Some(Page::Discover);
            }
            ui.add_space(6.0);
            style::sidebar_heading(ui, "Catégories");
            for (index, (_, label, glyph, color)) in CATEGORIES.iter().enumerate() {
                if style::sidebar_tile_item(
                    ui,
                    &mut self.icons,
                    glyph,
                    style::accent_color(*color),
                    label,
                    browsing && self.page == Page::Category(index),
                )
                .clicked()
                {
                    go = Some(Page::Category(index));
                }
            }
            ui.add_space(6.0);
            style::sidebar_heading(ui, "Mes applications");
            if style::sidebar_tile_item(
                ui,
                &mut self.icons,
                "glyph-installed",
                style::accent_color(Accent::Green),
                "Installées",
                browsing && self.page == Page::Installed,
            )
            .clicked()
            {
                go = Some(Page::Installed);
            }
            let count = self.updates.as_ref().map_or(0, Updates::count);
            let label = if count > 0 {
                format!("Mises à jour ({count})")
            } else {
                "Mises à jour".to_owned()
            };
            if style::sidebar_tile_item(
                ui,
                &mut self.icons,
                "glyph-update",
                style::accent_color(Accent::Orange),
                &label,
                browsing && self.page == Page::Updates,
            )
            .clicked()
            {
                self.search.clear();
                self.show_updates(false);
            }
            // Seulement sur un PC qui en a besoin (ou juste après une installation).
            if !self.drivers.is_empty() || self.restart_needed || self.page == Page::Drivers {
                let label = if self.drivers.is_empty() {
                    "Pilotes".to_owned()
                } else {
                    format!("Pilotes ({})", self.drivers.len())
                };
                if style::sidebar_tile_item(
                    ui,
                    &mut self.icons,
                    "glyph-screen",
                    style::accent_color(Accent::Purple),
                    &label,
                    browsing && self.page == Page::Drivers,
                )
                .clicked()
                {
                    go = Some(Page::Drivers);
                }
            }
            if let Some(page) = go {
                self.page = page;
                self.detail = None;
                self.deb = None;
                self.search.clear();
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let bar = ui.max_rect();
        if self.detail.is_some() || self.deb.is_some() {
            let back = kit::tool_button(ui, kit::TOOL, false, "Retour", |painter, rect, ink| {
                let c = rect.center();
                painter.line_segment(
                    [egui::pos2(c.x + 3.0, c.y - 6.0), egui::pos2(c.x - 3.0, c.y)],
                    egui::Stroke::new(2.0, ink),
                );
                painter.line_segment(
                    [egui::pos2(c.x - 3.0, c.y), egui::pos2(c.x + 3.0, c.y + 6.0)],
                    egui::Stroke::new(2.0, ink),
                );
            });
            if back.clicked()
                || ui.input(|input| input.modifiers.alt && input.key_pressed(egui::Key::ArrowLeft))
            {
                self.detail = None;
                self.deb = None;
            }
        }
        // Champ de recherche au milieu de la barre.
        let width = 320.0_f32.min(bar.width() - 80.0).max(120.0);
        let field = egui::Rect::from_center_size(bar.center(), egui::vec2(width, 28.0));
        ui.painter().rect_filled(field, 8.0, style::control_background());
        let inner = field.shrink2(egui::vec2(10.0, 0.0));
        let response = ui.put(
            inner,
            egui::TextEdit::singleline(&mut self.search)
                .id(egui::Id::unique("logitheque-recherche"))
                .hint_text(RichText::new("Rechercher des applications").size(14.0))
                .font(FontId::proportional(14.0))
                .frame(Frame::NONE)
                .align(egui::Align2::LEFT_CENTER)
                .desired_width(inner.width()),
        );
        if response.changed() {
            self.detail = None;
            self.deb = None;
        }
        if ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::F)) {
            response.request_focus();
        }
        if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.search.clear();
        }
    }

    // ———————————————————————————— Pages ————————————————————————————

    fn loading(&self, ui: &mut egui::Ui) -> bool {
        if self.loading {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(28.0));
                ui.add_space(8.0);
                kit::note(ui, "Lecture du catalogue des applications…");
            });
        }
        self.loading
    }

    fn discover_page(&mut self, ui: &mut egui::Ui) {
        if self.loading(ui) {
            return;
        }
        let apps = Arc::clone(&self.apps);
        egui::ScrollArea::vertical().id_salt("decouvrir").show(ui, |ui| {
            ui.add_space(8.0);
            self.sources_cards(ui);
            let selection: Vec<&App> = SELECTION.iter().filter_map(|id| find(&apps, id)).collect();
            if !selection.is_empty() {
                kit::heading(ui, "Sélection");
                self.grid(ui, &selection);
            }
            for (index, (key, label, _, _)) in CATEGORIES.iter().enumerate().take(6) {
                let mut picks: Vec<&App> = apps
                    .iter()
                    .filter(|app| in_category(app.main(), key) && app.main().icon.is_some())
                    .filter(|app| !selection.iter().any(|picked| picked.id == app.id))
                    .collect();
                // Les plus complètes d'abord : captures et description.
                picks.sort_by_key(|app| {
                    std::cmp::Reverse((
                        app.variants.len(),
                        app.main().screenshots.len().min(3),
                        app.main().description.len().min(4),
                    ))
                });
                picks.truncate(6);
                if picks.is_empty() {
                    continue;
                }
                heading_row(ui, label, |ui| {
                    if ui
                        .link(RichText::new("Tout voir").size(13.0).color(style::accent()))
                        .clicked()
                    {
                        self.page = Page::Category(index);
                    }
                });
                self.grid(ui, &picks);
            }
            ui.add_space(24.0);
        });
    }

    /// Cartes « catalogue Debian manquant » et « Flathub non activé ».
    fn sources_cards(&mut self, ui: &mut egui::Ui) {
        let busy = self.operation.is_some();
        if !self.has_debian {
            kit::card(ui, |ui| {
                ui.label(
                    RichText::new("Catalogue Debian à télécharger")
                        .size(15.0)
                        .strong(),
                );
                kit::note(
                    ui,
                    "Pour présenter les applications de Debian, la Logithèque a besoin de leur \
                     catalogue (paquet « appstream »). Le mot de passe administrateur sera demandé.",
                );
                ui.add_space(6.0);
                if ui
                    .add_enabled(!busy, egui::Button::new("Télécharger le catalogue"))
                    .clicked()
                {
                    self.start(
                        Target::DebianCatalog,
                        Verb::Install,
                        "catalogue Debian",
                        |progress| install_debian_catalog(progress),
                    );
                }
            });
            ui.add_space(10.0);
        }
        if !self.has_flathub {
            kit::card(ui, |ui| {
                ui.label(RichText::new("Activer Flathub ?").size(15.0).strong());
                kit::note(
                    ui,
                    "Flathub propose des milliers d'applications récentes (Spotify, Discord, \
                     Steam…), installées pour vous seul, sans mot de passe. Elles viennent de leurs \
                     éditeurs et non de Debian.",
                );
                ui.add_space(6.0);
                if ui
                    .add_enabled(!busy, egui::Button::new("Activer Flathub"))
                    .clicked()
                {
                    self.start(Target::Flathub, Verb::Install, "Flathub", |_| {
                        flatpak::add_flathub().map(|()| "Flathub est activé.".to_owned())
                    });
                }
            });
            ui.add_space(10.0);
        }
    }

    fn category_page(&mut self, ui: &mut egui::Ui, index: usize) {
        let (key, label, _, _) = CATEGORIES[index];
        ui.add_space(4.0);
        kit::heading(ui, label);
        if self.loading(ui) {
            return;
        }
        let apps = Arc::clone(&self.apps);
        let list: Vec<&App> = apps.iter().filter(|app| in_category(app.main(), key)).collect();
        self.long_grid(ui, &list, "categorie");
    }

    fn search_page(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        kit::heading(ui, "Résultats");
        if self.loading(ui) {
            return;
        }
        let apps = Arc::clone(&self.apps);
        let query = self.search.trim().to_owned();
        let mut found: Vec<(u32, &App)> = apps
            .iter()
            .map(|app| (appstream::score(app, &query), app))
            .filter(|(score, _)| *score > 0)
            .collect();
        found.sort_by_key(|(score, app)| (std::cmp::Reverse(*score), std::cmp::Reverse(app.variants.len())));
        let list: Vec<&App> = found.into_iter().map(|(_, app)| app).collect();
        if list.is_empty() {
            kit::note(ui, &format!("Aucune application ne correspond à « {query} »."));
            return;
        }
        self.long_grid(ui, &list, "recherche");
    }

    fn installed_page(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        kit::heading(ui, "Installées");
        if self.loading(ui) {
            return;
        }
        let apps = Arc::clone(&self.apps);
        let list: Vec<&App> = apps.iter().filter(|app| self.app_installed(app)).collect();
        // Applications Flatpak installées que le catalogue de Flathub ne connaît pas
        // (autres dépôts) : on peut au moins les désinstaller.
        let others: Vec<flatpak::Installed> = self
            .installed_flatpak
            .iter()
            .filter(|installed| {
                !apps
                    .iter()
                    .any(|app| app.variant(Origin::Flathub).is_some_and(|v| v.id == installed.id))
            })
            .cloned()
            .collect();
        egui::ScrollArea::vertical().id_salt("installees").show(ui, |ui| {
            if list.is_empty() && others.is_empty() {
                kit::note(
                    ui,
                    "Aucune application installée depuis la Logithèque pour l'instant.",
                );
            }
            self.grid(ui, &list);
            if !others.is_empty() {
                kit::heading(ui, "Autres applications Flatpak");
                kit::card(ui, |ui| {
                    for (row, installed) in others.iter().enumerate() {
                        if row > 0 {
                            kit::separator(ui);
                        }
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&installed.id).size(14.0));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let enabled = self.operation.is_none();
                                if ui
                                    .add_enabled(enabled, egui::Button::new("Désinstaller"))
                                    .clicked()
                                {
                                    let (id, system) = (installed.id.clone(), installed.system);
                                    self.start(
                                        Target::App(id.clone(), Origin::Flathub),
                                        Verb::Remove,
                                        &id.clone(),
                                        move |progress| {
                                            flatpak::uninstall(&id, system, progress)
                                                .map(|()| format!("{id} est désinstallée."))
                                        },
                                    );
                                }
                                if ui.button("Ouvrir").clicked()
                                    && let Err(error) = flatpak::launch(&installed.id)
                                {
                                    self.notice = Some(Err(error));
                                }
                            });
                        });
                    }
                });
            }
            ui.add_space(24.0);
        });
    }

    fn updates_page(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        heading_row(ui, "Mises à jour", |ui| {
            let idle = self.operation.is_none() && !self.checking_updates;
            let count = self.updates.as_ref().map_or(0, Updates::count);
            let update_all =
                egui::Button::new(RichText::new("Tout mettre à jour").color(style::accent_text()))
                    .fill(style::accent());
            if count > 0 && ui.add_enabled(idle, update_all).clicked() {
                self.update_all();
            }
            if ui.add_enabled(idle, egui::Button::new("Rechercher")).clicked() {
                self.show_updates(true);
            }
        });
        let Some(updates) = self.updates.clone() else {
            kit::status(ui, "Recherche des mises à jour…", true);
            return;
        };
        if self.checking_updates {
            kit::status(ui, "Recherche des mises à jour…", true);
        }
        let apps = Arc::clone(&self.apps);
        egui::ScrollArea::vertical()
            .id_salt("mises-a-jour")
            .show(ui, |ui| {
                match &updates.debian {
                    Err(error) => {
                        kit::heading(ui, "Système (Debian)");
                        ui.colored_label(style::error(), error);
                    }
                    Ok(list) => {
                        kit::heading(ui, &format!("Système (Debian) — {}", packagekit::label(list)));
                        if !list.is_empty() {
                            kit::card(ui, |ui| {
                                for (row, package) in list.iter().take(200).enumerate() {
                                    if row > 0 {
                                        kit::separator(ui);
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&package.name).size(14.0).strong());
                                        ui.label(
                                            RichText::new(&package.version)
                                                .size(12.0)
                                                .color(style::text_secondary()),
                                        );
                                        if package.security {
                                            ui.colored_label(
                                                style::warning(),
                                                RichText::new("sécurité").size(12.0),
                                            );
                                        }
                                    });
                                    if !package.summary.is_empty() {
                                        kit::note(ui, &package.summary);
                                    }
                                }
                                if list.len() > 200 {
                                    kit::note(ui, &format!("… et {} autres.", list.len() - 200));
                                }
                            });
                        }
                    }
                }
                match &updates.flatpak {
                    Err(error) => {
                        kit::heading(ui, "Applications Flathub");
                        ui.colored_label(style::error(), error);
                    }
                    Ok(ids) => {
                        let title = match ids.len() {
                            0 => "Applications Flathub — à jour".to_owned(),
                            1 => "Applications Flathub — 1 mise à jour".to_owned(),
                            count => format!("Applications Flathub — {count} mises à jour"),
                        };
                        kit::heading(ui, &title);
                        let list: Vec<&App> = ids.iter().filter_map(|id| find(&apps, id)).collect();
                        self.grid(ui, &list);
                    }
                }
                ui.add_space(24.0);
            });
    }

    fn drivers_page(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        kit::heading(ui, "Pilotes graphiques");
        if self.restart_needed {
            kit::card(ui, |ui| {
                ui.label(RichText::new("Redémarrez l'ordinateur").size(14.0).strong());
                kit::note(ui, "Le pilote installé sera utilisé au prochain démarrage.");
            });
            ui.add_space(12.0);
        }
        if self.drivers.is_empty() {
            if !self.restart_needed {
                kit::note(
                    ui,
                    "Aucun pilote à télécharger : votre carte graphique a tout ce qu'il lui faut.",
                );
            }
            return;
        }
        if self.live {
            kit::card(ui, |ui| {
                ui.label(
                    RichText::new("Vous utilisez le système live (clé USB).")
                        .size(14.0)
                        .color(style::warning()),
                );
                kit::note(
                    ui,
                    "Un pilote installé ici serait perdu au redémarrage, alors qu'il faut \
                        redémarrer pour l'utiliser. Installez d'abord Ferrite sur le disque : le \
                        pilote vous sera proposé ensuite.",
                );
            });
            ui.add_space(12.0);
        }
        for driver in self.drivers.clone() {
            let target = Target::Driver(driver.package);
            kit::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width((ui.available_width() - 190.0).max(160.0));
                        ui.label(
                            RichText::new(format!("Carte graphique {}", driver.brand))
                                .size(15.0)
                                .family(style::strong_family()),
                        );
                        ui.add_space(2.0);
                        kit::note(
                            ui,
                            &format!(
                                "Microprogramme de la carte (paquet {}), environ {} Mo à \
                                    télécharger. Sans lui, l'écran marche sans accélération : \
                                    vidéos, jeux et animations sont plus lents.",
                                driver.package, driver.download_mb
                            ),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(operation) = self
                            .operation
                            .as_ref()
                            .filter(|operation| operation.target == target)
                        {
                            if let Some(percent) = operation.percent {
                                ui.add(
                                    egui::ProgressBar::new(percent as f32 / 100.0)
                                        .desired_width(160.0)
                                        .show_percentage(),
                                );
                            } else {
                                ui.add(egui::Spinner::new().size(20.0));
                            }
                        } else {
                            let enabled = self.operation.is_none() && !self.live;
                            if kit::wide_button(ui, 160.0, "Télécharger", kit::Tone::Accent, enabled)
                                .clicked()
                            {
                                let name = format!("pilote {}", driver.brand);
                                self.start(target.clone(), Verb::Install, &name, move |progress| {
                                    install_driver(driver, progress)
                                });
                            }
                        }
                    });
                });
            });
            ui.add_space(10.0);
        }
        kit::note(
            ui,
            "Mot de passe administrateur demandé. Il faut être connecté à Internet.",
        );
    }

    fn update_all(&mut self) {
        let Some(updates) = self.updates.clone() else {
            return;
        };
        let ids: Vec<String> = updates
            .debian
            .as_ref()
            .map(|list| list.iter().map(|package| package.id.clone()).collect())
            .unwrap_or_default();
        let flatpak_count = updates.flatpak.as_ref().map_or(0, Vec::len);
        self.start(Target::Updates, Verb::Update, "votre système", move |progress| {
            if !ids.is_empty() {
                packagekit::install(&ids, progress)?;
            }
            if flatpak_count > 0 {
                flatpak::update_all(progress)?;
            }
            Ok("Tout est à jour.".to_owned())
        });
    }

    // ———————————————————————————— Grilles ————————————————————————————

    fn columns(width: f32) -> usize {
        (((width + GAP) / (CARD_MIN + GAP)).floor() as usize).max(1)
    }

    /// Grille courte (accueil) : tout est dessiné.
    fn grid(&mut self, ui: &mut egui::Ui, list: &[&App]) {
        let columns = Self::columns(ui.available_width());
        for row in list.chunks(columns) {
            self.grid_row(ui, row, columns);
            ui.add_space(GAP);
        }
    }

    /// Grille longue (catégorie, recherche) : seules les rangées visibles sont dessinées.
    fn long_grid(&mut self, ui: &mut egui::Ui, list: &[&App], salt: &str) {
        let columns = Self::columns(ui.available_width());
        let rows = list.len().div_ceil(columns);
        egui::ScrollArea::vertical()
            .id_salt(salt)
            .show_rows(ui, CARD_HEIGHT + GAP, rows, |ui, range| {
                for row in range {
                    let start = row * columns;
                    let end = (start + columns).min(list.len());
                    self.grid_row(ui, &list[start..end], columns);
                    ui.add_space(GAP);
                }
            });
    }

    fn grid_row(&mut self, ui: &mut egui::Ui, row: &[&App], columns: usize) {
        let width = ui.available_width();
        let card_width = (width - GAP * (columns as f32 - 1.0)) / columns as f32;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, CARD_HEIGHT), Sense::hover());
        for (index, app) in row.iter().enumerate() {
            let left = rect.left() + index as f32 * (card_width + GAP);
            let card =
                egui::Rect::from_min_size(egui::pos2(left, rect.top()), egui::vec2(card_width, CARD_HEIGHT));
            if self.card(ui, card, app).clicked() {
                self.detail = Some(Detail {
                    id: app.id.clone(),
                    origin: app.main().origin(),
                    shot: 0,
                });
            }
        }
    }

    fn card(&mut self, ui: &mut egui::Ui, rect: egui::Rect, app: &App) -> egui::Response {
        let response = ui
            .interact(rect, ui.scope_id().with(("carte", &app.id)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let painter = ui.painter_at(rect);
        let fill = if response.hovered() {
            style::control_hover()
        } else {
            style::sidebar_color()
        };
        painter.rect_filled(rect, 14.0, fill);
        let main = app.main();
        let icon = egui::Rect::from_min_size(rect.min + egui::vec2(14.0, 14.0), egui::Vec2::splat(56.0));
        self.paint_icon(ui, main, icon);
        let text_left = icon.right() + 12.0;
        let text_width = rect.right() - 14.0 - text_left;
        let mut job = egui::text::LayoutJob::simple(
            main.name.clone(),
            FontId::new(15.0, style::strong_family()),
            style::primary_text(),
            text_width,
        );
        job.wrap.max_rows = 1;
        let name = painter.layout_job(job);
        let name_height = name.size().y;
        painter.galley(
            egui::pos2(text_left, rect.top() + 14.0),
            name,
            style::primary_text(),
        );
        let mut job = egui::text::LayoutJob::simple(
            main.summary.clone(),
            FontId::proportional(12.5),
            style::text_secondary(),
            text_width,
        );
        job.wrap.max_rows = 2;
        let summary = painter.layout_job(job);
        painter.galley(
            egui::pos2(text_left, rect.top() + 16.0 + name_height),
            summary,
            style::text_secondary(),
        );
        // Origine(s) et « installée » en bas.
        let mut tags: Vec<String> = app
            .variants
            .iter()
            .map(|variant| variant.origin().label().to_owned())
            .collect();
        if self.app_installed(app) {
            tags.insert(0, "Installée".to_owned());
        }
        painter.text(
            egui::pos2(text_left, rect.bottom() - 10.0),
            egui::Align2::LEFT_BOTTOM,
            tags.join(" · "),
            FontId::proportional(11.0),
            style::tertiary_text(),
        );
        response
    }

    fn paint_icon(&mut self, ui: &egui::Ui, component: &Component, rect: egui::Rect) {
        let texture = component.icon.as_deref().and_then(|path| self.media.icon(path));
        match texture {
            Some(texture) => {
                // Garder les proportions dans le carré.
                let size = texture.size_vec2();
                let scale = (rect.width() / size.x).min(rect.height() / size.y);
                let shown = egui::Rect::from_center_size(rect.center(), size * scale);
                ui.painter().image(
                    texture.id(),
                    shown,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => self.icons.paint(
                ui,
                "app-store",
                rect.shrink(rect.width() * 0.08),
                style::IconStyle::Color,
            ),
        }
    }

    // ———————————————————————————— Fiche ————————————————————————————

    fn detail_page(&mut self, ui: &mut egui::Ui, detail: &Detail) {
        if self.loading(ui) {
            return;
        }
        let apps = Arc::clone(&self.apps);
        let Some(app) = find(&apps, &detail.id) else {
            ui.add_space(24.0);
            kit::note(
                ui,
                &format!("Application introuvable dans le catalogue : {}", detail.id),
            );
            return;
        };
        let component = app.variant(detail.origin).unwrap_or_else(|| app.main());
        let origin = component.origin();
        let key = (component.id.clone(), origin);
        if !self.sizes.contains_key(&key) {
            self.sizes.insert(key.clone(), None);
            let (id, package) = (component.id.clone(), component.package.clone());
            self.spawn(move || Msg::Size(id, origin, size_of(origin, &package)));
        }
        egui::ScrollArea::vertical().id_salt(("fiche", &app.id)).show(ui, |ui| {
            ui.add_space(20.0);
            // En-tête : icône, nom, éditeur, résumé, bouton.
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(egui::Vec2::splat(96.0), Sense::hover());
                self.paint_icon(ui, component, icon);
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 190.0).max(160.0));
                    ui.label(RichText::new(&component.name).size(24.0).family(style::strong_family()));
                    let mut by = component.developer.clone();
                    if component.verified {
                        by = if by.is_empty() { "Éditeur vérifié ✓".to_owned() } else { format!("{by} ✓") };
                    }
                    if !by.is_empty() {
                        ui.label(RichText::new(by).size(13.0).color(style::text_secondary()));
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new(&component.summary).size(14.0));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(170.0, 140.0),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| self.actions(ui, component),
                    );
                });
            });
            // Choix de l'origine quand l'application existe des deux côtés.
            if app.variants.len() > 1 {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Source").size(13.0).color(style::text_secondary()));
                    let labels: Vec<&str> = app.variants.iter().map(|variant| variant.origin().label()).collect();
                    let selected = app.variants.iter().position(|variant| variant.origin() == origin).unwrap_or(0);
                    if let Some(choice) = kit::segments(ui, &labels, selected)
                        && let Some(detail) = self.detail.as_mut()
                    {
                        detail.origin = app.variants[choice].origin();
                        detail.shot = 0;
                    }
                });
                kit::note(
                    ui,
                    match origin {
                        Origin::Debian => "Version de Debian : vérifiée par Debian, mise à jour avec le système.",
                        Origin::Flathub => "Version de Flathub : souvent plus récente, isolée du système, installée pour vous seul.",
                    },
                );
            }
            // Captures.
            if !component.screenshots.is_empty() {
                ui.add_space(18.0);
                self.screenshots(ui, component, detail.shot);
            }
            // Description.
            if !component.description.is_empty() {
                ui.add_space(10.0);
                kit::heading(ui, "Description");
                description(ui, &component.description);
            }
            // Informations.
            kit::heading(ui, "Informations");
            kit::card(ui, |ui| {
                let size = self.sizes.get(&key).cloned().flatten();
                let mut rows: Vec<(&str, String)> = vec![("Source", origin.label().to_owned())];
                rows.push((
                    match origin {
                        Origin::Debian => "Paquet",
                        Origin::Flathub => "Identifiant",
                    },
                    match origin {
                        Origin::Debian => component.package.clone(),
                        Origin::Flathub => component.id.clone(),
                    },
                ));
                if let Some(version) = &component.version {
                    rows.push(("Version", version.clone()));
                }
                rows.push(("Taille", size.unwrap_or_else(|| "—".to_owned())));
                if let Some(license) = &component.license {
                    rows.push(("Licence", license_label(license)));
                }
                for (index, (label, value)) in rows.iter().enumerate() {
                    if index > 0 {
                        kit::separator(ui);
                    }
                    info_row(ui, label, value);
                }
                if let Some(homepage) = component.homepage.clone() {
                    kit::separator(ui);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Site web").size(13.0).color(style::text_secondary()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.link(RichText::new(&homepage).size(13.0).color(style::accent())).clicked() {
                                open_link(&homepage);
                            }
                        });
                    });
                }
            });
            ui.add_space(28.0);
        });
    }

    /// Boutons de la fiche : Installer, ou Ouvrir et Désinstaller ; barre d'avancement.
    fn actions(&mut self, ui: &mut egui::Ui, component: &Component) {
        let origin = component.origin();
        let target = Target::App(component.id.clone(), origin);
        if let Some(operation) = self
            .operation
            .as_ref()
            .filter(|operation| operation.target == target)
        {
            ui.add(egui::Spinner::new().size(20.0));
            let text = match operation.verb {
                Verb::Install => "Installation…",
                Verb::Remove => "Désinstallation…",
                Verb::Update => "Mise à jour…",
            };
            ui.label(RichText::new(text).size(13.0).color(style::text_secondary()));
            if let Some(percent) = operation.percent {
                ui.add(
                    egui::ProgressBar::new(percent as f32 / 100.0)
                        .desired_width(160.0)
                        .show_percentage(),
                );
            }
            return;
        }
        let busy = self.operation.is_some();
        let name = component.name.clone();
        if self.is_installed(component) {
            let launchable = self.launcher(component).is_some();
            if kit::wide_button(ui, 160.0, "Ouvrir", kit::Tone::Accent, launchable).clicked() {
                self.launch(component);
            }
            ui.add_space(6.0);
            if kit::wide_button(ui, 160.0, "Désinstaller", kit::Tone::Plain, !busy).clicked() {
                let component = component.clone();
                let system = self
                    .installed_flatpak
                    .iter()
                    .find(|installed| installed.id == component.id)
                    .is_some_and(|installed| installed.system);
                self.start(target, Verb::Remove, &name, move |progress| {
                    remove(&component, system, progress)
                        .map(|()| format!("{} est désinstallée.", component.name))
                });
            }
        } else {
            if kit::wide_button(ui, 160.0, "Installer", kit::Tone::Accent, !busy).clicked() {
                let component = component.clone();
                self.start(target, Verb::Install, &name, move |progress| {
                    install(&component, progress).map(|()| format!("{} est installée.", component.name))
                });
            }
            ui.add_space(4.0);
            let hint = match origin {
                Origin::Debian => "Mot de passe administrateur demandé",
                Origin::Flathub => "Pour vous seul, sans mot de passe",
            };
            ui.label(RichText::new(hint).size(11.5).color(style::tertiary_text()));
        }
    }

    /// Commande qui lance une application installée.
    fn launcher(&self, component: &Component) -> Option<Command> {
        match component.origin() {
            Origin::Flathub => {
                let mut command = Command::new("flatpak");
                command.args(["run", &component.id]);
                Some(command)
            }
            Origin::Debian => {
                let wanted = component
                    .desktop_id
                    .as_deref()
                    .map(appstream::normalize_id)
                    .unwrap_or_else(|| component.id.clone());
                let entry = ferrite_system::applications::installed()
                    .into_iter()
                    .find(|entry| entry.id == wanted || entry.id == component.id)?;
                let (program, args) = entry.exec.split_first()?;
                let mut command = Command::new(program);
                command.args(args);
                Some(command)
            }
        }
    }

    fn launch(&mut self, component: &Component) {
        match self.launcher(component) {
            Some(command) => {
                if let Err(error) = ferrite_system::command::spawn_detached(command) {
                    self.notice = Some(Err(format!("Impossible d'ouvrir {} : {error}", component.name)));
                }
            }
            None => self.notice = Some(Err(format!("{} n'a pas de raccourci à ouvrir.", component.name))),
        }
    }

    fn screenshots(&mut self, ui: &mut egui::Ui, component: &Component, current: usize) {
        let count = component.screenshots.len();
        let current = current.min(count - 1);
        let shot = &component.screenshots[current];
        let width = ui.available_width();
        let height = (width * 0.56).clamp(220.0, 420.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
        ui.painter().rect_filled(rect, 14.0, style::sidebar_color());
        match self.media.shot(&shot.thumbnail) {
            media::Shot::Ready(texture) => {
                let size = texture.size_vec2();
                let scale = ((rect.width() - 24.0) / size.x)
                    .min((rect.height() - 24.0) / size.y)
                    .min(1.0);
                let shown = egui::Rect::from_center_size(rect.center(), size * scale);
                ui.painter().image(
                    texture.id(),
                    shown,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            media::Shot::Loading => {
                ui.put(
                    egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(24.0)),
                    egui::Spinner::new(),
                );
            }
            media::Shot::Failed => {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Capture indisponible (pas de connexion ?)",
                    FontId::proportional(13.0),
                    style::text_secondary(),
                );
            }
        }
        let mut next = None;
        if count > 1 {
            for (direction, x) in [(-1i32, rect.left() + 26.0), (1, rect.right() - 26.0)] {
                let button = egui::Rect::from_center_size(
                    egui::pos2(x, rect.center().y),
                    egui::Vec2::splat(kit::TOOL + 4.0),
                );
                let hint = if direction < 0 {
                    "Capture précédente"
                } else {
                    "Capture suivante"
                };
                let response = kit::tool_button_at(ui, button, false, hint, |painter, area, ink| {
                    let c = area.center();
                    let d = direction as f32 * 3.0;
                    painter.line_segment(
                        [egui::pos2(c.x - d, c.y - 6.0), egui::pos2(c.x + d, c.y)],
                        egui::Stroke::new(2.0, ink),
                    );
                    painter.line_segment(
                        [egui::pos2(c.x + d, c.y), egui::pos2(c.x - d, c.y + 6.0)],
                        egui::Stroke::new(2.0, ink),
                    );
                });
                if response.clicked() {
                    next = Some((current as i32 + direction).rem_euclid(count as i32) as usize);
                }
            }
            // Points : une capture par point, la courante pleine.
            let dots = egui::pos2(rect.center().x - (count as f32 - 1.0) * 6.0, rect.bottom() - 12.0);
            for index in 0..count {
                let color = if index == current {
                    style::primary_text()
                } else {
                    style::tertiary_text()
                };
                ui.painter()
                    .circle_filled(dots + egui::vec2(index as f32 * 12.0, 0.0), 3.0, color);
            }
        }
        if !shot.caption.is_empty() {
            ui.add_space(4.0);
            ui.vertical_centered(|ui| kit::note(ui, &shot.caption));
        }
        // Préparer la suivante pour qu'elle s'affiche sans attente.
        if count > 1 {
            let _ = self
                .media
                .shot(&component.screenshots[(current + 1) % count].thumbnail);
        }
        if let Some(next) = next
            && let Some(detail) = self.detail.as_mut()
        {
            detail.shot = next;
        }
    }

    // ———————————————————————————— Fichier .deb ————————————————————————————

    fn deb_page(&mut self, ui: &mut egui::Ui) {
        let Some(deb) = self.deb.clone() else { return };
        ui.add_space(20.0);
        let file_name = deb
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(error) = &deb.error {
            kit::heading(ui, &file_name);
            ui.colored_label(style::error(), error);
            return;
        }
        if deb.package.is_empty() {
            kit::status(ui, &format!("Lecture de {file_name}…"), true);
            return;
        }
        let installed = self.installed_debian.contains(&deb.package);
        let target = Target::Deb(deb.path.clone());
        ui.horizontal(|ui| {
            let (icon, _) = ui.allocate_exact_size(egui::Vec2::splat(96.0), Sense::hover());
            self.icons.paint(ui, "app-store", icon, style::IconStyle::Color);
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 190.0).max(160.0));
                ui.label(
                    RichText::new(&deb.package)
                        .size(24.0)
                        .family(style::strong_family()),
                );
                if !deb.maintainer.is_empty() {
                    ui.label(
                        RichText::new(&deb.maintainer)
                            .size(13.0)
                            .color(style::text_secondary()),
                    );
                }
                ui.add_space(4.0);
                ui.label(RichText::new(&deb.summary).size(14.0));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(170.0, 140.0),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        if let Some(operation) = self
                            .operation
                            .as_ref()
                            .filter(|operation| operation.target == target)
                        {
                            ui.add(egui::Spinner::new().size(20.0));
                            ui.label(
                                RichText::new("Installation…")
                                    .size(13.0)
                                    .color(style::text_secondary()),
                            );
                            if let Some(percent) = operation.percent {
                                ui.add(
                                    egui::ProgressBar::new(percent as f32 / 100.0)
                                        .desired_width(160.0)
                                        .show_percentage(),
                                );
                            }
                        } else {
                            let label = if installed { "Réinstaller" } else { "Installer" };
                            let busy = self.operation.is_some();
                            if kit::wide_button(ui, 160.0, label, kit::Tone::Accent, !busy).clicked() {
                                let path = deb.path.clone();
                                let name = deb.package.clone();
                                self.start(target.clone(), Verb::Install, &deb.package, move |progress| {
                                    packagekit::install_file(&path.to_string_lossy(), progress)
                                        .map(|()| format!("{name} est installé."))
                                });
                            }
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new("Mot de passe administrateur demandé")
                                    .size(11.5)
                                    .color(style::tertiary_text()),
                            );
                        }
                    },
                );
            });
        });
        ui.add_space(14.0);
        kit::card(ui, |ui| {
            ui.label(
                RichText::new("Ce fichier ne vient pas des dépôts de Debian.")
                    .size(14.0)
                    .color(style::warning()),
            );
            kit::note(
                ui,
                "Installez-le seulement si vous faites confiance au site d'où vous l'avez \
                 téléchargé : il aura accès à tout le système.",
            );
        });
        if !deb.description.is_empty() {
            kit::heading(ui, "Description");
            ui.label(RichText::new(&deb.description).size(14.0));
        }
        kit::heading(ui, "Informations");
        kit::card(ui, |ui| {
            let mut rows = vec![("Fichier", file_name.clone()), ("Version", deb.version.clone())];
            if let Some(kib) = deb.installed_kib {
                rows.push(("Taille installée", crate::files::format_size(kib * 1024)));
            }
            if installed {
                rows.push(("État", "Déjà installé".to_owned()));
            }
            for (index, (label, value)) in rows.iter().enumerate() {
                if index > 0 {
                    kit::separator(ui);
                }
                info_row(ui, label, value);
            }
            if !deb.homepage.is_empty() {
                kit::separator(ui);
                info_row(ui, "Site web", &deb.homepage);
            }
        });
    }
}

// ———————————————————————————— Morceaux ————————————————————————————

fn find<'a>(apps: &'a [App], id: &str) -> Option<&'a App> {
    let id = appstream::normalize_id(id);
    apps.iter().find(|app| app.id.eq_ignore_ascii_case(&id))
}

/// Titre de partie avec, à droite sur la même ligne, des boutons ou un lien.
/// Télécharge et installe un pilote graphique depuis les dépôts de Debian.
fn install_driver(driver: drivers::Driver, progress: &dyn Fn(u32)) -> Result<String, String> {
    let package = packagekit::resolve(driver.package)?
        .ok_or_else(|| format!("Le paquet {} est introuvable dans les dépôts.", driver.package))?;
    packagekit::install_packages(&[package.id], progress)?;
    Ok(format!(
        "Pilote {} installé. Redémarrez l'ordinateur pour l'utiliser.",
        driver.brand
    ))
}

fn heading_row(ui: &mut egui::Ui, text: &str, right: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(20.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(text).size(17.0).family(style::strong_family()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(8.0);
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(13.0).color(style::text_secondary()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(13.0));
        });
    });
}

fn description(ui: &mut egui::Ui, blocks: &[Block]) {
    for block in blocks {
        match block {
            Block::Paragraph(text) => {
                ui.label(RichText::new(text).size(14.0));
                ui.add_space(6.0);
            }
            Block::Item(text) => {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new("•").size(14.0));
                    ui.label(RichText::new(text).size(14.0));
                });
            }
        }
    }
}

/// Licences courantes en clair.
fn license_label(license: &str) -> String {
    if license.starts_with("LicenseRef-proprietary") {
        "Propriétaire".to_owned()
    } else {
        license.to_owned()
    }
}

fn open_link(url: &str) {
    if url.starts_with("https://") || url.starts_with("http://") {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        let _ = ferrite_system::command::spawn_detached(command);
    }
}

// ———————————————————————————— Travaux en arrière-plan ————————————————————————————

fn load_catalog() -> Msg {
    let (debian, flathub) = thread::scope(|scope| {
        let debian = scope.spawn(|| {
            let files = appstream::debian_files();
            let components: Vec<Component> = files
                .iter()
                .flat_map(|file| appstream::load_debian(file))
                .collect();
            (components, !files.is_empty())
        });
        let flathub = scope.spawn(|| {
            let dir = appstream::flathub_dir();
            let components = dir.as_deref().map(appstream::load_flathub).unwrap_or_default();
            // Flathub configuré mais catalogue pas encore là : il arrive à la prochaine
            // mise à jour ; on le demande tout de suite.
            if dir.is_none() && flatpak::has_flathub() && flatpak::refresh_catalog().is_ok() {
                let dir = appstream::flathub_dir();
                let components = dir.as_deref().map(appstream::load_flathub).unwrap_or_default();
                return (components, true);
            }
            (components, dir.is_some() || flatpak::has_flathub())
        });
        (
            debian.join().unwrap_or_default(),
            flathub.join().unwrap_or_default(),
        )
    });
    let (mut components, has_debian) = debian;
    let (flathub_components, has_flathub) = flathub;
    components.extend(flathub_components);
    Msg::Catalog {
        apps: appstream::merge(components),
        debian: has_debian,
        flathub: has_flathub,
    }
}

fn load_installed() -> Msg {
    Msg::Installed {
        debian: dpkg_installed(),
        flatpak: flatpak::installed().unwrap_or_default(),
    }
}

/// Paquets Debian installés (`dpkg-query`, instantané, sans droits).
fn dpkg_installed() -> HashSet<String> {
    let output = Command::new("dpkg-query")
        .args(["-W", "-f", "${db:Status-Abbrev} ${Package}\\n"])
        .output();
    let Ok(output) = output else {
        return HashSet::new();
    };
    parse_dpkg(&String::from_utf8_lossy(&output.stdout))
}

fn parse_dpkg(text: &str) -> HashSet<String> {
    text.lines()
        .filter_map(|line| {
            let (status, name) = line.split_once(' ')?;
            status
                .starts_with("ii")
                .then(|| name.trim().split(':').next().unwrap_or_default().to_owned())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

fn size_of(origin: Origin, package: &str) -> Option<String> {
    match origin {
        Origin::Debian => {
            let package = packagekit::resolve(package).ok()??;
            let bytes = packagekit::size(&package.id).ok()??;
            Some(crate::files::format_size(bytes))
        }
        Origin::Flathub => {
            let (download, installed) = flatpak::sizes(package).ok()?;
            match (download, installed) {
                (Some(download), Some(installed)) => Some(format!("{installed} ({download} à télécharger)")),
                (download, installed) => installed.or(download),
            }
        }
    }
}

fn install(component: &Component, progress: &dyn Fn(u32)) -> Result<(), String> {
    match component.origin() {
        Origin::Debian => {
            let package = packagekit::resolve(&component.package)?
                .ok_or_else(|| format!("Le paquet « {} » n'est pas dans les dépôts.", component.package))?;
            packagekit::install_packages(&[package.id], progress)
        }
        Origin::Flathub => flatpak::install(&component.package, progress),
    }
}

fn remove(component: &Component, system: bool, progress: &dyn Fn(u32)) -> Result<(), String> {
    match component.origin() {
        Origin::Debian => {
            let package = packagekit::resolve(&component.package)?
                .filter(|package| package.installed)
                .ok_or_else(|| format!("Le paquet « {} » n'est pas installé.", component.package))?;
            packagekit::remove_packages(&[package.id], progress)
        }
        Origin::Flathub => flatpak::uninstall(&component.id, system, progress),
    }
}

/// Installe le paquet `appstream` (catalogue des applications de Debian), puis relit les
/// dépôts pour le télécharger.
fn install_debian_catalog(progress: &dyn Fn(u32)) -> Result<String, String> {
    let mut ids = Vec::new();
    for name in ["appstream", "apt-config-icons-hidpi"] {
        if let Some(package) = packagekit::resolve(name)?
            && !package.installed
        {
            ids.push(package.id);
        }
    }
    if !ids.is_empty() {
        packagekit::install_packages(&ids, progress)?;
    }
    packagekit::check(true)?;
    Ok("Le catalogue Debian est téléchargé.".to_owned())
}

/// Lit l'en-tête d'un `.deb` (`dpkg-deb -f`).
fn read_deb(path: &Path) -> DebFile {
    let mut deb = DebFile {
        path: path.to_path_buf(),
        ..DebFile::default()
    };
    let output = Command::new("dpkg-deb").arg("-f").arg(path).output();
    match output {
        Ok(output) if output.status.success() => {
            parse_control(&String::from_utf8_lossy(&output.stdout), &mut deb);
            if deb.package.is_empty() {
                deb.error = Some("Ce fichier n'est pas un paquet Debian valide.".to_owned());
            }
        }
        _ => deb.error = Some("Ce fichier n'est pas un paquet Debian lisible.".to_owned()),
    }
    deb
}

/// Champs d'un fichier `control` : `Clé: valeur`, suite sur les lignes qui commencent par
/// une espace (« . » seul : ligne vide).
fn parse_control(text: &str, deb: &mut DebFile) {
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some((_, value)) = fields.last_mut() {
                let part = line.trim();
                value.push('\n');
                if part != "." {
                    value.push_str(part);
                }
            }
        } else if let Some((key, value)) = line.split_once(':') {
            fields.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    for (key, value) in fields {
        match key.as_str() {
            "Package" => deb.package = value,
            "Version" => deb.version = value,
            "Maintainer" => {
                // « Nom <adresse> » : le nom seul.
                deb.maintainer = value.split('<').next().unwrap_or_default().trim().to_owned();
            }
            "Homepage" => deb.homepage = value,
            "Installed-Size" => deb.installed_kib = value.parse().ok(),
            "Description" => {
                let (summary, rest) = value.split_once('\n').unwrap_or((value.as_str(), ""));
                deb.summary = summary.trim().to_owned();
                deb.description = rest.trim().to_owned();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paquets_installes_de_dpkg() {
        let installed = parse_dpkg("ii  vlc\nrc  ancien\nii  libc6:amd64\nun  rien\n");
        assert!(installed.contains("vlc"));
        assert!(installed.contains("libc6"));
        assert!(!installed.contains("ancien"));
        assert_eq!(installed.len(), 2);
    }

    #[test]
    fn entete_d_un_deb() {
        let mut deb = DebFile::default();
        parse_control(
            "Package: code\nVersion: 1.95.0-1\nMaintainer: Microsoft Corporation <vscode@microsoft.com>\n\
             Installed-Size: 412345\nHomepage: https://code.visualstudio.com/\n\
             Description: Code editing. Redefined.\n Visual Studio Code is a new choice.\n .\n See docs.\n",
            &mut deb,
        );
        assert_eq!(deb.package, "code");
        assert_eq!(deb.version, "1.95.0-1");
        assert_eq!(deb.maintainer, "Microsoft Corporation");
        assert_eq!(deb.installed_kib, Some(412_345));
        assert_eq!(deb.summary, "Code editing. Redefined.");
        assert_eq!(
            deb.description,
            "Visual Studio Code is a new choice.\n\nSee docs."
        );
        assert_eq!(deb.homepage, "https://code.visualstudio.com/");
    }

    #[test]
    fn rubriques_son_et_video() {
        let component = Component {
            categories: vec!["Audio".into(), "Player".into()],
            ..Component::default()
        };
        assert!(in_category(&component, "AudioVideo"));
        assert!(!in_category(&component, "Game"));
    }

    #[test]
    fn colonnes_de_la_grille() {
        assert_eq!(StoreState::columns(200.0), 1);
        assert_eq!(StoreState::columns(CARD_MIN * 2.0 + GAP), 2);
        assert_eq!(StoreState::columns(1000.0), 3);
    }
}
