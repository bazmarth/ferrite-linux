//! Installer Ferrite (`ferrite-apps installer`), seulement sur la clé live : un assistant
//! en six écrans pour mettre Ferrite sur le disque, à côté de Windows, dans un espace libre,
//! sur tout le disque ou sur une partition choisie.
//!
//! - `disks` lit les disques et ce que la sonde y a trouvé ;
//! - `plan` décide des partitions (tout est calculé et testé ici) ;
//! - `job` lance `ferrite-installation` en administrateur, dans un fil.
//!
//! Rien n'est écrit sur un disque avant le bouton « Installer » de l'écran de vérification.

mod disks;
mod job;
pub(crate) mod plan;

use eframe::egui::{self, Color32, FontId, Frame, Margin, RichText, Sense, Stroke};
use ferrite_ui::tokens::typography;

use self::{
    disks::{Disk, GIB, Ntfs, Part, Region, size_text},
    plan::{Account, Choice, Firmware, KEYBOARDS, Locale, MIN_ROOT, Plan, TIMEZONES, Tone},
};
use crate::{kit, style};

/// Largeur de la colonne des écrans.
const COLUMN: f32 = 620.0;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Welcome,
    Region,
    Disk,
    Account,
    Review,
    Installing,
    Done,
    Failed(String),
}

impl Step {
    /// Numéro affiché (« Étape 2 sur 5 »), pour les écrans qu'on remplit.
    fn number(&self) -> Option<usize> {
        match self {
            Self::Region => Some(1),
            Self::Disk => Some(2),
            Self::Account => Some(3),
            Self::Review => Some(4),
            Self::Installing => Some(5),
            _ => None,
        }
    }
}

/// Façon d'installer, choisie sur l'écran « Où installer Ferrite ? ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Beside,
    Free,
    Erase,
    Manual,
}

pub struct InstallerState {
    context: egui::Context,
    step: Step,
    firmware: Firmware,
    locale: Locale,
    probe: Option<job::Probe>,
    disks: Option<Result<Vec<Disk>, String>>,
    disk: usize,
    mode: Option<Mode>,
    /// Place donnée à Ferrite à côté de Windows, en octets.
    ferrite: u64,
    /// Zone choisie en mode manuel.
    manual: Option<Region>,
    account: Account,
    confirm: String,
    user_edited: bool,
    host_edited: bool,
    /// « J'ai mis mes fichiers importants à l'abri » (effacement ou réduction).
    saved: bool,
    install: Option<job::Install>,
    status: String,
    progress: u8,
}

impl InstallerState {
    pub fn new(context: &egui::Context) -> Self {
        Self {
            context: context.clone(),
            step: Step::Welcome,
            firmware: Firmware::current(),
            locale: Locale::default(),
            probe: Some(job::Probe::start(context)),
            disks: None,
            disk: 0,
            mode: None,
            ferrite: 0,
            manual: None,
            account: Account::default(),
            confirm: String::new(),
            user_edited: false,
            host_edited: false,
            saved: false,
            install: None,
            status: String::new(),
            progress: 0,
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        self.poll();
        let step = self.step.clone();
        style::header(ui, |ui| {
            let text = match step.number() {
                Some(number) => format!("Installer Ferrite · étape {number} sur 5"),
                None => "Installer Ferrite".to_owned(),
            };
            ui.painter().text(
                ui.max_rect().center(),
                egui::Align2::CENTER_CENTER,
                text,
                FontId::new(13.0, style::strong_family()),
                style::text_secondary(),
            );
        });
        self.footer(ui);
        egui::CentralPanel::no_frame()
            .frame(
                Frame::new()
                    .fill(style::background())
                    .inner_margin(Margin::symmetric(24, 8)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    let width = ui.available_width().min(COLUMN);
                    let side = ((ui.available_width() - width) / 2.0).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(side);
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.add_space(12.0);
                            match step {
                                Step::Welcome => self.welcome(ui),
                                Step::Region => self.region(ui),
                                Step::Disk => self.where_to(ui),
                                Step::Account => self.account(ui),
                                Step::Review => self.review(ui),
                                Step::Installing => self.installing(ui),
                                Step::Done => done(ui),
                                Step::Failed(message) => failed(ui, &message),
                            }
                            ui.add_space(24.0);
                        });
                    });
                });
            });
    }

    /// Résultats de la sonde et nouvelles de l'installation.
    fn poll(&mut self) {
        if let Some(result) = self.probe.as_ref().and_then(job::Probe::poll) {
            self.probe = None;
            if let Ok(disks) = &result {
                // Premier disque où l'on peut installer : celui de Windows d'abord.
                self.disk = disks
                    .iter()
                    .position(|disk| !disk.live && disk.has_windows())
                    .or_else(|| disks.iter().position(|disk| !disk.live))
                    .unwrap_or(0);
            }
            self.disks = Some(result);
            self.mode = None;
            self.default_mode();
            if let Some(screen) = job::demo_screen() {
                self.demo(&screen);
            }
        }
        let Some(install) = &self.install else { return };
        for event in install.poll() {
            match event {
                job::Event::Step(text) => self.status = text,
                job::Event::Progress(value) => self.progress = value,
                job::Event::Done => {
                    self.progress = 100;
                    self.step = Step::Done;
                }
                job::Event::Failed(message) => self.step = Step::Failed(message),
            }
        }
        if self.step == Step::Installing {
            self.context
                .request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    /// Mode démo (captures) : `FERRITE_INSTALLATION_DEMO=disque|manuel|effacer|compte|
    /// verification|installation|fini|echec` ouvre directement cet écran, rempli.
    fn demo(&mut self, screen: &str) {
        self.account = Account {
            full_name: "Aïssata Ouédraogo".into(),
            user: "aissata".into(),
            host: "pc-aissata".into(),
            password: "demo".into(),
            autologin: false,
        };
        self.confirm = "demo".into();
        self.step = match screen {
            "clavier" => Step::Region,
            "disque" => Step::Disk,
            "manuel" => {
                self.mode = Some(Mode::Manual);
                self.manual = Some(Region::Part(5));
                Step::Disk
            }
            "effacer" => {
                self.mode = Some(Mode::Erase);
                Step::Disk
            }
            "compte" => Step::Account,
            "verification" => Step::Review,
            "installation" => {
                self.step = Step::Review;
                self.next();
                return;
            }
            "fini" => Step::Done,
            "echec" => Step::Failed("La partition Windows n'a pas pu être réduite.".into()),
            _ => Step::Welcome,
        };
    }

    fn disk(&self) -> Option<&Disk> {
        self.disks.as_ref()?.as_ref().ok()?.get(self.disk)
    }

    /// Partition Windows qu'on peut réduire sur le disque choisi (la plus grande).
    fn windows(&self) -> Option<(usize, u64, u64, u64)> {
        let disk = self.disk()?;
        disk.parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.is_windows_data())
            .filter_map(|(index, part)| {
                plan::beside_limits(disk, part, self.firmware)
                    .map(|(min, max, proposed)| (index, min, max, proposed))
            })
            .max_by_key(|(index, ..)| disk.parts[*index].sectors)
    }

    /// Choix proposé d'office : à côté de Windows, sinon l'espace libre, sinon rien (le
    /// choix d'effacer reste à faire soi-même).
    fn default_mode(&mut self) {
        if self.mode.is_some() {
            return;
        }
        if let Some((_, _, _, proposed)) = self.windows() {
            self.mode = Some(Mode::Beside);
            self.ferrite = proposed;
        } else if self.free_space().is_some() {
            self.mode = Some(Mode::Free);
        } else if self.disk().is_some_and(|disk| disk.parts.is_empty()) {
            self.mode = Some(Mode::Erase);
        }
    }

    /// Plus grand espace libre assez grand pour Ferrite.
    fn free_space(&self) -> Option<(u64, u64)> {
        let disk = self.disk()?;
        disk.largest_free()
            .filter(|(_, sectors)| sectors * disk.sector >= MIN_ROOT + 512 * disks::MIB)
    }

    fn choice(&self) -> Option<Choice> {
        Some(match self.mode? {
            Mode::Beside => Choice::Beside {
                part: self.windows()?.0,
                ferrite: self.ferrite,
            },
            Mode::Free => {
                let (start, sectors) = self.free_space()?;
                Choice::Free { start, sectors }
            }
            Mode::Erase => Choice::Erase,
            Mode::Manual => match self.manual? {
                Region::Part(part) => Choice::Replace { part },
                Region::Free { start, sectors } => Choice::Free { start, sectors },
            },
        })
    }

    fn plan(&self) -> Option<Result<Plan, String>> {
        let disk = self.disk()?;
        let choice = self.choice()?;
        Some(plan::build(disk, choice, self.firmware))
    }

    /// Ce qui empêche de passer à l'écran suivant.
    fn blocker(&self) -> Option<String> {
        match &self.step {
            Step::Disk => match self.plan() {
                None if self.probe.is_some() => Some("Recherche des disques…".into()),
                None => Some("Choisissez où installer Ferrite.".into()),
                Some(Err(message)) => Some(message),
                Some(Ok(_)) => None,
            },
            Step::Account => self.account.problem(&self.confirm).map(str::to_owned),
            Step::Review => {
                let risky = self.plan().and_then(Result::ok).is_some_and(|plan| {
                    plan.new_table
                        || plan.shrink.is_some()
                        || matches!(plan.root, plan::Root::Existing { .. })
                });
                (risky && !self.saved).then(|| "Cochez la case pour confirmer.".to_owned())
            }
            _ => None,
        }
    }

    /// Barre du bas : Retour et Continuer (ou Installer, Redémarrer).
    fn footer(&mut self, ui: &mut egui::Ui) {
        if self.step == Step::Installing {
            return;
        }
        let blocker = self.blocker();
        egui::Panel::bottom(ui.scope_id().with("installer-pied"))
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(style::header_color())
                    .inner_margin(Margin::symmetric(20, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let back = !matches!(self.step, Step::Welcome | Step::Done);
                    if back && kit::wide_button(ui, 110.0, "Retour", kit::Tone::Plain, true).clicked() {
                        self.back();
                    }
                    if let Some(text) = &blocker {
                        ui.add_space(8.0);
                        kit::note(ui, text);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (label, tone) = match &self.step {
                            Step::Welcome => ("Commencer", kit::Tone::Accent),
                            Step::Review => ("Installer", kit::Tone::Accent),
                            Step::Done => ("Redémarrer", kit::Tone::Accent),
                            Step::Failed(_) => ("Recommencer", kit::Tone::Plain),
                            _ => ("Continuer", kit::Tone::Accent),
                        };
                        if kit::wide_button(ui, 150.0, label, tone, blocker.is_none()).clicked() {
                            self.next();
                        }
                    });
                });
            });
    }

    fn back(&mut self) {
        self.step = match self.step {
            Step::Region => Step::Welcome,
            Step::Disk => Step::Region,
            Step::Account => Step::Disk,
            Step::Review | Step::Failed(_) => Step::Account,
            _ => return,
        };
    }

    fn next(&mut self) {
        self.step = match self.step {
            Step::Welcome => Step::Region,
            Step::Region => Step::Disk,
            Step::Disk => Step::Account,
            Step::Account => {
                self.saved = false;
                Step::Review
            }
            Step::Review => {
                let Some(Ok(plan)) = self.plan() else { return };
                let file = plan.to_file(&self.account, &self.locale);
                self.status = "Démarrage…".into();
                self.progress = 0;
                self.install = Some(job::Install::start(
                    &self.context,
                    file,
                    self.account.password.clone(),
                ));
                Step::Installing
            }
            Step::Done => {
                let _ = std::process::Command::new("systemctl").arg("reboot").spawn();
                return;
            }
            Step::Failed(_) => {
                self.probe = Some(job::Probe::start(&self.context));
                self.disks = None;
                Step::Disk
            }
            Step::Installing => return,
        };
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(28.0);
        title(ui, "Installer Ferrite sur cet ordinateur");
        lead(
            ui,
            "Quelques questions simples, puis Ferrite se copie sur le disque. Comptez 10 à 20 minutes.",
        );
        ui.add_space(16.0);
        kit::card(ui, |ui| {
            point(
                ui,
                "Branchez l'ordinateur sur le secteur.",
                "Une coupure pendant l'installation peut abîmer le disque.",
            );
            kit::separator(ui);
            point(
                ui,
                "Mettez vos fichiers importants à l'abri.",
                "Sur une clé USB ou un disque externe : l'installation modifie le disque, et une erreur reste possible.",
            );
            kit::separator(ui);
            point(
                ui,
                "Vous gardez Windows si vous le voulez.",
                "Ferrite peut s'installer à côté : à l'allumage, un menu vous laissera choisir.",
            );
        });
        ui.add_space(12.0);
        kit::note(
            ui,
            "Rien n'est modifié avant le dernier écran, où tout est résumé avant de confirmer.",
        );
    }

    fn region(&mut self, ui: &mut egui::Ui) {
        title(ui, "Clavier et heure");
        lead(ui, "Ferrite s'affiche en français.");
        kit::heading(ui, "Clavier");
        kit::card(ui, |ui| {
            for (index, keyboard) in KEYBOARDS.iter().enumerate() {
                if index > 0 {
                    kit::separator(ui);
                }
                if radio_line(ui, keyboard.name, None, self.locale.keyboard == keyboard, true) {
                    self.locale.keyboard = keyboard;
                }
            }
        });
        kit::heading(ui, "Fuseau horaire");
        kit::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Ville la plus proche").size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let current = TIMEZONES
                        .iter()
                        .find(|(_, zone)| *zone == self.locale.timezone)
                        .map_or("", |(name, _)| name);
                    egui::ComboBox::from_id_salt("fuseau")
                        .icon(crate::style::combo_chevron)
                        .width(220.0)
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (name, zone) in TIMEZONES {
                                ui.selectable_value(&mut self.locale.timezone, zone, *name);
                            }
                        });
                });
            });
        });
    }

    fn where_to(&mut self, ui: &mut egui::Ui) {
        title(ui, "Où installer Ferrite ?");
        let disks = match &self.disks {
            None => {
                lead(ui, "Recherche des disques et des systèmes déjà installés…");
                ui.add(egui::Spinner::new().size(22.0));
                return;
            }
            Some(Err(message)) => {
                warning(ui, message);
                return;
            }
            Some(Ok(disks)) => disks.clone(),
        };
        let usable: Vec<usize> = (0..disks.len()).filter(|index| !disks[*index].live).collect();
        if usable.is_empty() {
            warning(ui, "Aucun disque trouvé où installer Ferrite.");
            return;
        }
        if usable.len() > 1 {
            kit::heading(ui, "Disque");
            kit::card(ui, |ui| {
                for (row, index) in usable.iter().enumerate() {
                    if row > 0 {
                        kit::separator(ui);
                    }
                    let disk = &disks[*index];
                    let systems = disk.systems();
                    let detail = if systems.is_empty() {
                        "Aucun système trouvé".to_owned()
                    } else {
                        systems.join(", ")
                    };
                    if radio_line(ui, &disk.title(), Some(&detail), self.disk == *index, true)
                        && self.disk != *index
                    {
                        self.disk = *index;
                        self.mode = None;
                        self.manual = None;
                        self.default_mode();
                    }
                }
            });
        }
        let disk = disks[self.disk].clone();
        kit::heading(ui, "Aujourd'hui sur ce disque");
        disk_bar(ui, &disk, None);

        kit::heading(ui, "Votre choix");
        let windows = self.windows();
        let free = self.free_space();
        let others = disk.systems();
        let beside_name = others
            .iter()
            .find(|name| name.contains("Windows"))
            .cloned()
            .unwrap_or_else(|| "Windows".into());
        // Pourquoi Windows ne peut pas être réduit, s'il est là.
        let windows_reason = disk
            .parts
            .iter()
            .filter(|part| part.is_windows_data())
            .max_by_key(|part| part.sectors)
            .map(|part| match part.ntfs {
                Some(Ntfs::Hibernated) => "Windows est en veille (démarrage rapide). Dans Windows, cliquez sur Arrêter en tenant la touche Maj, puis revenez.",
                Some(Ntfs::BitLocker) => "Windows est chiffré par BitLocker : suspendez BitLocker dans Windows, puis revenez.",
                Some(Ntfs::Unreadable) | None => "Windows n'a pas été arrêté proprement : redémarrez-le une fois, puis revenez.",
                Some(Ntfs::Shrinkable(_)) => "Pas assez de place libre dans Windows pour Ferrite.",
            });
        kit::card(ui, |ui| {
            let mut first = true;
            let mut row = |ui: &mut egui::Ui, mode: Mode, name: &str, detail: &str, enabled: bool| {
                if !first {
                    kit::separator(ui);
                }
                first = false;
                if radio_line(ui, name, Some(detail), self.mode == Some(mode), enabled) {
                    self.mode = Some(mode);
                }
            };
            if windows.is_some() || windows_reason.is_some() {
                row(
                    ui,
                    Mode::Beside,
                    &format!("À côté de {beside_name} (conseillé)"),
                    windows_reason.filter(|_| windows.is_none()).unwrap_or(
                        "Windows et ses fichiers restent là ; vous choisissez la place de chacun.",
                    ),
                    windows.is_some(),
                );
            }
            if let Some((_, sectors)) = free {
                row(
                    ui,
                    Mode::Free,
                    &format!("Dans l'espace libre ({})", size_text(sectors * disk.sector)),
                    "Rien n'est effacé.",
                    true,
                );
            }
            row(
                ui,
                Mode::Erase,
                "Effacer tout le disque",
                if others.is_empty() {
                    "Tout ce qu'il contient sera supprimé."
                } else {
                    "Tout sera supprimé, y compris les systèmes déjà installés."
                },
                true,
            );
            row(
                ui,
                Mode::Manual,
                "Choisir moi-même la partition",
                "Pour les connaisseurs : la partition choisie sera effacée.",
                true,
            );
        });

        match self.mode {
            Some(Mode::Beside) => {
                if let Some((part, min, max, _)) = windows {
                    kit::heading(ui, "Partage de la place");
                    kit::note(
                        ui,
                        "Faites glisser la séparation pour choisir la place de chacun.",
                    );
                    ui.add_space(8.0);
                    self.ferrite = self.ferrite.clamp(min, max);
                    self.split_bar(ui, &disk, &disk.parts[part], min, max);
                }
            }
            Some(Mode::Manual) => self.manual_list(ui, &disk),
            Some(Mode::Erase) => {
                ui.add_space(12.0);
                warning(
                    ui,
                    &if others.is_empty() {
                        "Tous les fichiers de ce disque seront effacés.".to_owned()
                    } else {
                        format!(
                            "{} et tous les fichiers de ce disque seront effacés.",
                            others.join(", ")
                        )
                    },
                );
            }
            Some(Mode::Free) | None => {}
        }
        if let Some(Ok(plan)) = self.plan() {
            kit::heading(ui, "Après l'installation");
            disk_bar(ui, &disk, Some(&plan));
        }
    }

    /// Barre « Windows | Ferrite » à séparation déplaçable.
    fn split_bar(&mut self, ui: &mut egui::Ui, disk: &Disk, windows: &Part, min: u64, max: u64) {
        let total = windows.bytes(disk.sector)
            + (disk.next_start_after(windows) - windows.start - windows.sectors) * disk.sector;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 64.0), Sense::click_and_drag());
        let x_of = |ferrite: u64| rect.right() - rect.width() * (ferrite as f32 / total as f32);
        if let Some(pointer) = response.interact_pointer_pos() {
            let share = ((rect.right() - pointer.x) / rect.width()).clamp(0.0, 1.0);
            let bytes = (f64::from(share) * total as f64) as u64;
            self.ferrite = (bytes / GIB * GIB).clamp(min, max);
        }
        let split = x_of(self.ferrite);
        let painter = ui.painter();
        let left = egui::Rect::from_min_max(rect.min, egui::pos2(split - 3.0, rect.bottom()));
        let right = egui::Rect::from_min_max(egui::pos2(split + 3.0, rect.top()), rect.max);
        painter.rect_filled(left, 12.0, windows_color());
        painter.rect_filled(right, 12.0, style::accent());
        // Ce que Windows occupe déjà : la séparation ne peut pas aller plus à gauche.
        let used = x_of(max);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top()),
                egui::pos2(used.min(split - 3.0), rect.bottom()),
            ),
            12.0,
            style::tint(40),
        );
        let windows_bytes = total - self.ferrite;
        let font = FontId::new(14.0, style::strong_family());
        painter.text(
            left.left_center() + egui::vec2(14.0, -9.0),
            egui::Align2::LEFT_CENTER,
            "Windows",
            font.clone(),
            Color32::WHITE,
        );
        painter.text(
            left.left_center() + egui::vec2(14.0, 10.0),
            egui::Align2::LEFT_CENTER,
            size_text(windows_bytes),
            FontId::proportional(13.0),
            Color32::WHITE,
        );
        painter.text(
            right.right_center() + egui::vec2(-14.0, -9.0),
            egui::Align2::RIGHT_CENTER,
            "Ferrite",
            font,
            style::accent_text(),
        );
        painter.text(
            right.right_center() + egui::vec2(-14.0, 10.0),
            egui::Align2::RIGHT_CENTER,
            size_text(self.ferrite),
            FontId::proportional(13.0),
            style::accent_text(),
        );
        // Poignée.
        let handle = egui::Rect::from_center_size(egui::pos2(split, rect.center().y), egui::vec2(14.0, 40.0));
        style::knob_shadow(painter, handle.center(), 8.0);
        painter.rect_filled(handle, 7.0, Color32::WHITE);
        for dy in [-6.0, 0.0, 6.0] {
            painter.hline(
                handle.x_range().shrink(4.0),
                handle.center().y + dy,
                Stroke::new(1.0, style::tertiary_text()),
            );
        }
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        ui.add_space(6.0);
        kit::note(
            ui,
            &format!(
                "Ferrite : entre {} et {}. Windows garde au moins {} de libre pour ses mises à jour.",
                size_text(min),
                size_text(max),
                size_text(plan::WINDOWS_MARGIN)
            ),
        );
    }

    /// Mode manuel : chaque partition et chaque espace libre du disque.
    fn manual_list(&mut self, ui: &mut egui::Ui, disk: &Disk) {
        kit::heading(ui, "Partition pour Ferrite");
        kit::card(ui, |ui| {
            for (row, region) in disk.regions().into_iter().enumerate() {
                if row > 0 {
                    kit::separator(ui);
                }
                let (name, detail, enabled) = match region {
                    Region::Part(index) => {
                        let part = &disk.parts[index];
                        let size = size_text(part.bytes(disk.sector));
                        let device = part.path.trim_start_matches("/dev/");
                        match plan::replace_allowed(part, disk.sector) {
                            Ok(()) => (
                                format!("{} · {size}", part.title()),
                                format!("{device} · sera effacée"),
                                true,
                            ),
                            Err(reason) => (
                                format!("{} · {size}", part.title()),
                                format!("{device} · {reason}"),
                                false,
                            ),
                        }
                    }
                    Region::Free { sectors, .. } => {
                        let bytes = sectors * disk.sector;
                        let enough = bytes >= MIN_ROOT;
                        let detail = if enough {
                            "Rien n'est effacé.".to_owned()
                        } else {
                            format!("Trop petit : il faut au moins {}.", size_text(MIN_ROOT))
                        };
                        (format!("Espace libre · {}", size_text(bytes)), detail, enough)
                    }
                };
                if radio_line(ui, &name, Some(&detail), self.manual == Some(region), enabled) {
                    self.manual = Some(region);
                }
            }
        });
    }

    fn account(&mut self, ui: &mut egui::Ui) {
        title(ui, "Votre compte");
        lead(ui, "Le nom et le mot de passe pour ouvrir votre session.");
        ui.add_space(8.0);
        kit::card(ui, |ui| {
            if form_row(ui, "Votre nom", &mut self.account.full_name, false, "Prénom Nom").changed() {
                if !self.user_edited {
                    self.account.user = plan::user_from(&self.account.full_name);
                }
                if !self.host_edited {
                    self.account.host = plan::host_from(&self.account.user);
                }
            }
            kit::separator(ui);
            if form_row(ui, "Nom du compte", &mut self.account.user, false, "prenom").changed() {
                self.user_edited = true;
                if !self.host_edited {
                    self.account.host = plan::host_from(&self.account.user);
                }
            }
            kit::separator(ui);
            form_row(ui, "Mot de passe", &mut self.account.password, true, "");
            kit::separator(ui);
            form_row(ui, "Encore une fois", &mut self.confirm, true, "");
            kit::separator(ui);
            if form_row(ui, "Nom de l'ordinateur", &mut self.account.host, false, "pc").changed() {
                self.host_edited = true;
            }
        });
        ui.add_space(4.0);
        kit::note(
            ui,
            "Retenez bien le mot de passe : il sert aussi à installer des applications.",
        );
        kit::heading(ui, "Ouverture de session");
        kit::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Ouvrir ma session sans mot de passe").size(14.0));
                    kit::note(ui, "Pratique sur un ordinateur que vous êtes seul à utiliser.");
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    kit::switch(ui, &mut self.account.autologin);
                });
            });
        });
    }

    fn review(&mut self, ui: &mut egui::Ui) {
        title(ui, "Vérifiez avant d'installer");
        let (Some(disk), Some(Ok(plan))) = (self.disk().cloned(), self.plan()) else {
            warning(ui, "Le choix du disque n'est plus valable : revenez en arrière.");
            return;
        };
        lead(ui, &format!("Sur {}.", disk.title()));
        ui.add_space(8.0);
        kit::card(ui, |ui| {
            for (index, (tone, text)) in plan.summary(&disk).iter().enumerate() {
                if index > 0 {
                    kit::separator(ui);
                }
                let color = match tone {
                    Tone::Info => style::primary_text(),
                    Tone::Warning => style::warning(),
                    Tone::Danger => style::error(),
                };
                ui.add_space(6.0);
                ui.label(RichText::new(text).size(14.0).color(color));
                ui.add_space(6.0);
            }
        });
        let risky =
            plan.new_table || plan.shrink.is_some() || matches!(plan.root, plan::Root::Existing { .. });
        // Juste sous le résumé : visible sans faire défiler.
        if risky {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.saved, "");
                let text = if plan.shrink.is_some() {
                    "J'ai mis mes fichiers importants à l'abri, et je laisse Ferrite réduire Windows."
                } else {
                    "J'ai mis mes fichiers importants à l'abri, et j'accepte l'effacement."
                };
                if ui
                    .add(egui::Label::new(RichText::new(text).size(14.0)).sense(Sense::click()))
                    .clicked()
                {
                    self.saved = !self.saved;
                }
            });
        }
        ui.add_space(12.0);
        disk_bar(ui, &disk, Some(&plan));
        kit::heading(ui, "Votre ordinateur");
        kit::card(ui, |ui| {
            let timezone = TIMEZONES
                .iter()
                .find(|(_, zone)| *zone == self.locale.timezone)
                .map_or(self.locale.timezone, |(name, _)| name);
            let lines = [
                (
                    "Compte",
                    format!("{} ({})", self.account.full_name.trim(), self.account.user),
                ),
                ("Ordinateur", self.account.host.clone()),
                ("Clavier", self.locale.keyboard.name.to_owned()),
                ("Heure", timezone.to_owned()),
                (
                    "Session",
                    if self.account.autologin {
                        "ouverte toute seule".to_owned()
                    } else {
                        "avec mot de passe".to_owned()
                    },
                ),
            ];
            for (index, (key, value)) in lines.iter().enumerate() {
                if index > 0 {
                    kit::separator(ui);
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(*key).size(14.0).color(style::text_secondary()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(value).size(14.0));
                    });
                });
                ui.add_space(4.0);
            }
        });
    }

    fn installing(&mut self, ui: &mut egui::Ui) {
        ui.add_space(40.0);
        title(ui, "Installation de Ferrite");
        lead(
            ui,
            "Laissez l'ordinateur allumé et branché. Vous pouvez continuer à utiliser la clé pendant ce temps.",
        );
        ui.add_space(24.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 10.0), Sense::hover());
        let shown = kit::glide(
            ui,
            ui.scope_id().with("avancee"),
            f32::from(self.progress) / 100.0,
        );
        ui.painter().rect_filled(rect, 5.0, style::track());
        let done = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * shown, rect.height()));
        ui.painter().rect_filled(done, 5.0, style::accent());
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            kit::status(ui, &self.status, true);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                kit::note(ui, &format!("{} %", self.progress));
            });
        });
    }
}

fn done(ui: &mut egui::Ui) {
    ui.add_space(48.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 72.0), Sense::hover());
    let center = rect.center();
    ui.painter().circle_filled(center, 32.0, style::success());
    let ink = Stroke::new(5.0, style::ink_on(style::success()));
    ui.painter().line_segment(
        [center + egui::vec2(-13.0, 1.0), center + egui::vec2(-3.0, 11.0)],
        ink,
    );
    ui.painter().line_segment(
        [center + egui::vec2(-3.0, 11.0), center + egui::vec2(15.0, -9.0)],
        ink,
    );
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new("Ferrite est installé")
                .size(typography::DISPLAY)
                .family(style::strong_family()),
        );
        ui.add_space(8.0);
        kit::note(ui, "Redémarrez, puis retirez la clé USB quand l'écran s'éteint.");
        kit::note(ui, "À l'allumage, choisissez Ferrite dans le menu s'il apparaît.");
    });
}

fn failed(ui: &mut egui::Ui, message: &str) {
    ui.add_space(40.0);
    title(ui, "L'installation n'a pas abouti");
    warning(ui, message);
    ui.add_space(12.0);
    kit::note(
        ui,
        &format!(
            "Le détail est dans le journal {} : gardez-le pour obtenir de l'aide. Vous pouvez revenir en arrière et réessayer.",
            job::LOG
        ),
    );
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(typography::TITLE)
            .family(style::strong_family())
            .color(style::heading_text()),
    );
    ui.add_space(6.0);
}

fn lead(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(typography::BODY)
            .color(style::text_secondary()),
    );
}

fn warning(ui: &mut egui::Ui, text: &str) {
    Frame::new()
        .fill(style::error().gamma_multiply(0.12))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).size(14.0).color(style::error()));
        });
}

/// Point de la page d'accueil : phrase en gras et explication.
fn point(ui: &mut egui::Ui, text: &str, detail: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(14.0).family(style::strong_family()));
    kit::note(ui, detail);
    ui.add_space(6.0);
}

/// Ligne à bouton rond, cliquable sur toute sa largeur ; vrai si elle vient d'être choisie.
fn radio_line(ui: &mut egui::Ui, name: &str, detail: Option<&str>, on: bool, enabled: bool) -> bool {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                let radio = kit::radio(ui, on && enabled);
                ui.add_space(6.0);
                let ink = if enabled {
                    style::primary_text()
                } else {
                    style::tertiary_text()
                };
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    ui.label(RichText::new(name).size(14.0).color(ink));
                    if let Some(detail) = detail {
                        ui.label(
                            RichText::new(detail)
                                .size(typography::SMALL)
                                .color(style::text_secondary()),
                        );
                    }
                    ui.add_space(6.0);
                });
                radio
            })
        })
        .inner;
    let row = ui.interact(
        response.response.rect,
        response.response.id.with("ligne"),
        Sense::click(),
    );
    enabled && (row.clicked() || response.inner.clicked())
}

/// Champ d'un formulaire : libellé à gauche, champ à droite.
fn form_row(ui: &mut egui::Ui, label: &str, text: &mut String, secret: bool, hint: &str) -> egui::Response {
    ui.add_space(4.0);
    let response = ui
        .horizontal(|ui| {
            ui.label(RichText::new(label).size(14.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(text)
                        .password(secret)
                        .hint_text(hint)
                        .desired_width(260.0),
                )
            })
            .inner
        })
        .inner;
    ui.add_space(4.0);
    response
}

/// Couleur de Windows dans les barres de disque.
fn windows_color() -> Color32 {
    style::marker(4)
}

/// Couleur d'une partition dans une barre de disque.
fn part_color(part: &Part) -> Color32 {
    if part.system.as_deref() == Some("Windows") || part.is_windows_data() {
        windows_color()
    } else if part.is_technical() || part.fstype == "swap" {
        style::tertiary_text()
    } else if part.system.is_some() {
        style::marker(1)
    } else {
        style::marker(3)
    }
}

/// Barre d'un disque, avec sa légende ; avec un plan, l'état après l'installation.
fn disk_bar(ui: &mut egui::Ui, disk: &Disk, plan: Option<&Plan>) {
    // Morceaux : (début, secteurs, couleur, nom).
    let mut pieces: Vec<(u64, u64, Color32, String)> = Vec::new();
    let ferrite = |start: u64, sectors: u64| (start, sectors, style::accent(), "Ferrite".to_owned());
    match plan {
        Some(plan) if plan.new_table => {
            pieces.push(ferrite(0, disk.sectors));
        }
        _ => {
            for region in disk.regions() {
                match region {
                    Region::Part(index) => {
                        let part = &disk.parts[index];
                        let mut sectors = part.sectors;
                        if let Some(shrink) = plan.and_then(|plan| plan.shrink.as_ref())
                            && shrink.path == part.path
                        {
                            sectors = shrink.sectors;
                        }
                        let replaced = plan.is_some_and(|plan| {
                            matches!(&plan.root, plan::Root::Existing { path, .. } if *path == part.path)
                        });
                        if replaced {
                            pieces.push(ferrite(part.start, sectors));
                        } else {
                            pieces.push((part.start, sectors, part_color(part), part.title()));
                        }
                    }
                    Region::Free { start, sectors } => {
                        pieces.push((start, sectors, style::track(), "Espace libre".to_owned()));
                    }
                }
            }
            if let Some(plan) = plan
                && let plan::Root::New(span) = &plan.root
            {
                // La place de Ferrite remplace l'espace libre ou ce qu'a rendu Windows.
                pieces.retain(|(start, sectors, color, _)| {
                    !(*color == style::track()
                        && *start < span.start + span.sectors
                        && span.start < start + sectors)
                });
                pieces.push(ferrite(span.start, span.sectors));
                pieces.sort_by_key(|piece| piece.0);
            }
        }
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, style::track());
    let total = disk.sectors.max(1) as f32;
    // Chaque morceau a au moins 3 points de large : les petites partitions restent visibles.
    let mut x = rect.left();
    let widths: Vec<f32> = pieces
        .iter()
        .map(|(_, sectors, ..)| (*sectors as f32 / total * rect.width()).max(3.0))
        .collect();
    let scale = rect.width() / widths.iter().sum::<f32>().max(rect.width());
    for ((_, _, color, _), width) in pieces.iter().zip(&widths) {
        let width = width * scale;
        let piece = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(width, rect.height()));
        painter.rect_filled(piece.shrink2(egui::vec2(1.0, 0.0)), 4.0, *color);
        x += width;
    }
    ui.add_space(8.0);
    // Légende : les morceaux d'au moins 1 Go, sans doublon de nom.
    ui.horizontal_wrapped(|ui| {
        let mut seen: Vec<&str> = Vec::new();
        for (_, sectors, color, name) in &pieces {
            let bytes = sectors * disk.sector;
            if bytes < GIB || seen.contains(&name.as_str()) {
                continue;
            }
            seen.push(name);
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 5.0, *color);
            ui.label(
                RichText::new(format!("{name} · {}", size_text(bytes)))
                    .size(typography::SMALL)
                    .color(style::text_secondary()),
            );
            ui.add_space(10.0);
        }
    });
}
