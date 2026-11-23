//! Moniteur d'activité : processus, et performances de chaque appareil (processeur,
//! mémoire, cartes graphiques, disques, réseaux, batterie, capteurs).
//!
//! Un fil à part mesure le système chaque seconde (`sample::Sampler`) ; l'interface
//! garde une minute d'historique pour les graphiques. Terminer un processus envoie
//! SIGTERM (fermeture propre), « Forcer l'arrêt » envoie SIGKILL.
//!
//! Tout s'adapte à la machine : nombre de cœurs (grille des cœurs), cœurs hybrides
//! (performance / efficaces), une ou plusieurs cartes graphiques (Intel, AMD, NVIDIA),
//! disques et interfaces branchés, batterie présente ou non ; et à la largeur de la
//! fenêtre (colonnes de processus, colonne des appareils repliée en bandeau).

mod hardware;
mod perf;
mod sample;

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    thread,
    time::Duration,
};

use eframe::egui::{self, Color32, CornerRadius, FontId, Frame, Key, Margin, RichText, Sense, Stroke};
use ferrite_system::command;
use ferrite_ui::tokens::typography;

use self::sample::{Process, Sampler, Snapshot};
use crate::{kit, style};

/// Points gardés dans les graphiques : une minute, une mesure par seconde.
const HISTORY: usize = 60;
const ROW: f32 = 30.0;
/// Couleur de chaque courbe : la même dans la liste des processus, les cartes et les
/// pages de performances.
const CPU_COLOR: Color32 = Color32::from_rgb(0x36, 0x89, 0xE6);
const CPU_EFFICIENT_COLOR: Color32 = Color32::from_rgb(0x2B, 0xB3, 0xC0);
const MEMORY_COLOR: Color32 = Color32::from_rgb(0x9B, 0x59, 0xD0);
const GPU_COLOR: Color32 = Color32::from_rgb(0xE0, 0x9B, 0x10);
const READ_COLOR: Color32 = Color32::from_rgb(0x2E, 0xA0, 0x5A);
const WRITE_COLOR: Color32 = Color32::from_rgb(0xE5, 0x6A, 0x1F);
const DOWN_COLOR: Color32 = Color32::from_rgb(0x1A, 0x9B, 0xD8);
const UP_COLOR: Color32 = Color32::from_rgb(0xD9, 0x41, 0x6A);
const BATTERY_COLOR: Color32 = Color32::from_rgb(0x34, 0xA8, 0x53);
const SENSOR_COLOR: Color32 = Color32::from_rgb(0xE0, 0x4E, 0x39);
const SYSTEM_COLOR: Color32 = Color32::from_rgb(0x7A, 0x86, 0x99);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Processes,
    Performance,
}

/// Page de l'onglet Performances.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Page {
    Cpu,
    Memory,
    Gpu(String),
    Disk(String),
    Network(String),
    Battery,
    Sensors,
    System,
}

impl Page {
    /// `ferrite-apps monitor performances|processeur|coeurs|memoire|gpu|disque|reseau|batterie|capteurs|systeme`.
    fn from_argument(argument: &str) -> Option<(Tab, Option<Self>)> {
        Some(match argument {
            "processus" => (Tab::Processes, None),
            "performances" | "processeur" | "coeurs" => (Tab::Performance, Some(Self::Cpu)),
            "memoire" => (Tab::Performance, Some(Self::Memory)),
            "gpu" => (Tab::Performance, Some(Self::Gpu(String::new()))),
            "disque" => (Tab::Performance, Some(Self::Disk(String::new()))),
            "reseau" => (Tab::Performance, Some(Self::Network(String::new()))),
            "batterie" => (Tab::Performance, Some(Self::Battery)),
            "capteurs" => (Tab::Performance, Some(Self::Sensors)),
            "systeme" => (Tab::Performance, Some(Self::System)),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Name,
    User,
    Cpu,
    Memory,
    Disk,
    Gpu,
    Threads,
    Pid,
}

/// Une minute de chaque mesure, par nom (« cpu », « cpu/3 », « gpu/0000:01:00.0 »…).
#[derive(Debug, Default)]
struct History {
    series: HashMap<String, VecDeque<f32>>,
}

impl History {
    fn add(&mut self, key: String, value: f32) {
        let series = self.series.entry(key).or_default();
        series.push_back(value);
        if series.len() > HISTORY {
            series.pop_front();
        }
    }

    fn get(&self, key: &str) -> &VecDeque<f32> {
        static EMPTY: VecDeque<f32> = VecDeque::new();
        self.series.get(key).unwrap_or(&EMPTY)
    }

    fn push(&mut self, snapshot: &Snapshot) {
        let fraction = |used: u64, total: u64| {
            if total > 0 {
                used as f32 / total as f32
            } else {
                0.0
            }
        };
        let memory = &snapshot.memory;
        self.add("cpu".into(), snapshot.cpu);
        self.add("cpu-noyau".into(), snapshot.cpu_system);
        for (index, core) in snapshot.cores.iter().enumerate() {
            self.add(format!("cpu/{index}"), *core);
        }
        self.add("memoire".into(), fraction(memory.used, memory.total));
        self.add("echange".into(), fraction(memory.swap_used, memory.swap_total));
        self.add("lecture".into(), snapshot.disk_read as f32);
        self.add("ecriture".into(), snapshot.disk_write as f32);
        self.add("reception".into(), snapshot.net_down as f32);
        self.add("envoi".into(), snapshot.net_up as f32);
        for gpu in &snapshot.gpus {
            self.add(format!("gpu/{}", gpu.slot), gpu.usage.unwrap_or(0.0));
            if let (Some(used), Some(total)) = (gpu.memory_used, gpu.memory_total) {
                self.add(format!("vram/{}", gpu.slot), fraction(used, total));
            }
        }
        for disk in &snapshot.disks {
            let name = &disk.info.name;
            self.add(format!("lecture/{name}"), disk.read as f32);
            self.add(format!("ecriture/{name}"), disk.write as f32);
            self.add(format!("activite/{name}"), disk.busy);
        }
        for interface in &snapshot.interfaces {
            let name = &interface.name;
            self.add(format!("reception/{name}"), interface.down as f32);
            self.add(format!("envoi/{name}"), interface.up as f32);
        }
        if let Some(battery) = snapshot.battery() {
            self.add("puissance".into(), battery.power.unwrap_or(0.0));
            self.add("batterie".into(), battery.level);
        }
        if let Some(celsius) = snapshot.cpu_temperature {
            self.add("temperature".into(), celsius);
        }
    }
}

/// Processus à terminer, en attente de confirmation.
#[derive(Debug, Clone)]
struct Confirm {
    pid: u32,
    name: String,
}

pub struct MonitorState {
    receiver: Receiver<Snapshot>,
    snapshot: Option<Arc<Snapshot>>,
    history: History,
    tab: Tab,
    page: Page,
    /// Page Processeur : un graphique par cœur au lieu du graphique global.
    per_core: bool,
    search: String,
    sort: Column,
    descending: bool,
    only_mine: bool,
    selected: Option<u32>,
    confirm: Option<Confirm>,
    message: Option<(String, bool)>,
    started: bool,
}

impl std::fmt::Debug for MonitorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonitorState")
            .field("tab", &self.tab)
            .field("page", &self.page)
            .finish()
    }
}

impl Default for MonitorState {
    fn default() -> Self {
        let (_, receiver) = mpsc::channel();
        Self {
            receiver,
            snapshot: None,
            history: History::default(),
            tab: Tab::Processes,
            page: Page::Cpu,
            per_core: false,
            search: String::new(),
            sort: Column::Cpu,
            descending: true,
            only_mine: false,
            selected: None,
            confirm: None,
            message: None,
            started: false,
        }
    }
}

impl MonitorState {
    /// `ferrite-apps monitor performances` (ou `gpu`, `capteurs`…) ouvre directement cette
    /// page.
    pub fn with_page(argument: Option<&str>) -> Self {
        let mut state = Self::default();
        if let Some((tab, page)) = argument.and_then(Page::from_argument) {
            state.tab = tab;
            if let Some(page) = page {
                state.page = page;
            }
        }
        // `coeurs` : page Processeur, un graphique par cœur.
        state.per_core = argument == Some("coeurs");
        state
    }

    /// Démarre le fil de mesure ; il s'arrête avec la fenêtre (canal fermé).
    fn start(&mut self, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let mut sampler = Sampler::new();
            loop {
                if sender.send(sampler.sample()).is_err() {
                    return;
                }
                ctx.request_repaint();
                thread::sleep(Duration::from_secs(1));
            }
        });
        self.receiver = receiver;
        self.started = true;
    }

    fn poll(&mut self) {
        while let Ok(snapshot) = self.receiver.try_recv() {
            self.history.push(&snapshot);
            if self
                .selected
                .is_some_and(|pid| !snapshot.processes.iter().any(|process| process.pid == pid))
            {
                self.selected = None;
            }
            self.snapshot = Some(Arc::new(snapshot));
        }
    }

    /// Processus affichés : filtre, recherche et tri appliqués.
    fn visible(&self) -> Vec<Process> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        let query = self.search.trim().to_lowercase();
        let mut list: Vec<Process> = snapshot
            .processes
            .iter()
            .filter(|process| !self.only_mine || process.mine)
            .filter(|process| {
                query.is_empty()
                    || process.name.to_lowercase().contains(&query)
                    || process.command.to_lowercase().contains(&query)
                    || process.pid.to_string() == query
            })
            .cloned()
            .collect();
        list.sort_by(|a, b| {
            let order = match self.sort {
                Column::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Column::User => a.user.cmp(&b.user),
                Column::Cpu => a.cpu.total_cmp(&b.cpu),
                Column::Memory => a.memory.cmp(&b.memory),
                Column::Disk => a.disk.total_cmp(&b.disk),
                Column::Gpu => a.gpu.total_cmp(&b.gpu),
                Column::Threads => a.threads.cmp(&b.threads),
                Column::Pid => a.pid.cmp(&b.pid),
            };
            let order = if self.descending { order.reverse() } else { order };
            order.then(a.pid.cmp(&b.pid))
        });
        list
    }

    fn selected_process(&self) -> Option<Process> {
        let pid = self.selected?;
        self.snapshot
            .as_ref()?
            .processes
            .iter()
            .find(|process| process.pid == pid)
            .cloned()
    }

    fn ask_terminate(&mut self) {
        if let Some(process) = self.selected_process() {
            self.confirm = Some(Confirm {
                pid: process.pid,
                name: process.name,
            });
        }
    }

    fn terminate(&mut self, confirm: &Confirm, force: bool) {
        let signal = if force { "-KILL" } else { "-TERM" };
        let pid = confirm.pid.to_string();
        self.message = Some(match command::run("kill", &[signal, &pid]) {
            Ok(_) if force => (format!("« {} » a été arrêté", confirm.name), false),
            Ok(_) => (format!("« {} » a été invité à se fermer", confirm.name), false),
            Err(error) if error.contains("not permitted") => (
                format!("« {} » appartient à un autre utilisateur", confirm.name),
                true,
            ),
            Err(error) => (
                format!("Impossible de terminer « {} » : {error}", confirm.name),
                true,
            ),
        });
    }

    pub fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !self.started {
            self.start(&ctx);
        }
        self.poll();

        if self.confirm.is_none() && !ctx.memory(|memory| memory.focused().is_some()) {
            if self.tab == Tab::Processes && ui.input(|input| input.key_pressed(Key::Delete)) {
                self.ask_terminate();
            }
            if ui.input(|input| input.key_pressed(Key::Escape)) {
                self.selected = None;
            }
        }

        style::header(ui, |ui| self.header(ui));
        // Thème « Fusion » : barre d'outils sur le chrome, contenu dans la carte encastrée.
        style::inset_panel(ui, true, false, |ui| {
            self.status_bar(ui);
            egui::CentralPanel::no_frame()
                .frame(
                    Frame::new()
                        .fill(style::background())
                        .inner_margin(Margin::same(14)),
                )
                .show(ui, |ui| {
                    let Some(snapshot) = self.snapshot.clone() else {
                        ui.centered_and_justified(|ui| {
                            ui.label(RichText::new("Mesure en cours…").color(style::text_secondary()));
                        });
                        return;
                    };
                    match self.tab {
                        Tab::Processes => {
                            self.cards(ui, &snapshot);
                            ui.add_space(12.0);
                            self.process_list(ui, &snapshot);
                        }
                        Tab::Performance => self.performance(ui, &snapshot),
                    }
                });
        });
        self.confirm_dialog(&ctx);
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let tab = match self.tab {
            Tab::Processes => 0,
            Tab::Performance => 1,
        };
        if let Some(index) = kit::segments(ui, &["Processus", "Performances"], tab) {
            self.tab = if index == 0 {
                Tab::Processes
            } else {
                Tab::Performance
            };
        }
        if self.tab != Tab::Processes {
            return;
        }
        let narrow = ui.available_width() < 520.0;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let enabled = self.selected.is_some();
            let button = egui::Button::new(RichText::new("Terminer…").color(if enabled {
                style::accent_text()
            } else {
                style::text_secondary()
            }))
            .fill(if enabled { style::error() } else { style::tint(10) })
            .corner_radius(f32::from(ferrite_ui::tokens::radius::CONTROL))
            .min_size(egui::vec2(0.0, 30.0));
            if ui
                .add_enabled(enabled, button)
                .on_hover_text("Terminer le processus choisi (Suppr)")
                .clicked()
            {
                self.ask_terminate();
            }
            let (field, _) = ui.allocate_exact_size(
                egui::vec2(if narrow { 150.0 } else { 220.0 }, 30.0),
                Sense::hover(),
            );
            kit::search_field(ui, field, &mut self.search, "Rechercher");
            ui.checkbox(
                &mut self.only_mine,
                if narrow { "Les miens" } else { "Mes processus" },
            );
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        let Some(snapshot) = &self.snapshot else { return };
        let count = snapshot.processes.len();
        let threads: u32 = snapshot.processes.iter().map(|process| process.threads).sum();
        let message = self.message.clone();
        let (uptime, load) = (snapshot.uptime, snapshot.load);
        style::status_bar(ui, |ui| {
            ui.label(format!(
                "{} processus · {} fils",
                format_count(count as u64),
                format_count(u64::from(threads))
            ));
            ui.label("·");
            ui.label(format!("Allumé depuis {}", format_duration(uptime)));
            ui.label("·");
            ui.label(format!(
                "Charge {} {} {}",
                format_decimal(load[0], 2),
                format_decimal(load[1], 2),
                format_decimal(load[2], 2)
            ))
            .on_hover_text("Processus en attente du processeur, en moyenne sur 1, 5 et 15 minutes");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some((text, error)) = &message {
                    if *error {
                        ui.colored_label(style::error(), text);
                    } else {
                        ui.label(text);
                    }
                }
            });
        });
    }

    /// Cartes de synthèse, chacune avec son graphique de la dernière minute ; compactes
    /// quand la fenêtre est étroite ou basse.
    fn cards(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let memory = &snapshot.memory;
        let history = &self.history;
        let mut cards: Vec<Card> = vec![
            Card::new(
                Page::Cpu,
                "Processeur",
                format_percent(snapshot.cpu),
                match (average_frequency(snapshot), snapshot.cpu_temperature) {
                    (Some(khz), Some(celsius)) => {
                        format!("{} · {}", format_ghz(khz), format_celsius(celsius))
                    }
                    (Some(khz), None) => format_ghz(khz),
                    _ => format!("{} cœurs", snapshot.cores.len()),
                },
                vec![(history.get("cpu"), CPU_COLOR)],
                Some(1.0),
            ),
            Card::new(
                Page::Memory,
                "Mémoire",
                format_bytes(memory.used),
                format!("sur {}", format_bytes(memory.total)),
                vec![(history.get("memoire"), MEMORY_COLOR)],
                Some(1.0),
            ),
        ];
        // Carte graphique : la plus occupée, sinon la dédiée.
        if let Some(gpu) = snapshot.gpus.iter().max_by(|a, b| {
            let busy = |gpu: &hardware::Gpu| (gpu.usage.unwrap_or(0.0), !gpu.integrated);
            busy(a).partial_cmp(&busy(b)).unwrap_or(std::cmp::Ordering::Equal)
        }) {
            let key = format!("gpu/{}", gpu.slot);
            cards.push(Card::new(
                Page::Gpu(gpu.slot.clone()),
                "Carte graphique",
                if gpu.asleep {
                    "En veille".into()
                } else {
                    gpu.usage.map(format_percent).unwrap_or_else(|| "—".into())
                },
                gpu.name.clone(),
                vec![(history.get(&key), GPU_COLOR)],
                Some(1.0),
            ));
        }
        cards.push(Card::new(
            Page::Disk(String::new()),
            "Disques",
            format_rate(snapshot.disk_read + snapshot.disk_write),
            format!(
                "{} lus · {} écrits",
                format_rate(snapshot.disk_read),
                format_rate(snapshot.disk_write)
            ),
            vec![
                (history.get("lecture"), READ_COLOR),
                (history.get("ecriture"), WRITE_COLOR),
            ],
            None,
        ));
        cards.push(Card::new(
            Page::Network(String::new()),
            "Réseau",
            format_rate(snapshot.net_down + snapshot.net_up),
            format!(
                "↓ {} · ↑ {}",
                format_rate(snapshot.net_down),
                format_rate(snapshot.net_up)
            ),
            vec![
                (history.get("reception"), DOWN_COLOR),
                (history.get("envoi"), UP_COLOR),
            ],
            None,
        ));

        let spacing = 10.0;
        let available = ui.available_width();
        let count = cards.len();
        // Toutes les cartes tiennent sur une rangée avec leur graphique ; sinon (fenêtre
        // étroite ou basse) elles passent en version compacte, sans graphique, pour
        // laisser la place à la liste.
        let full = available + spacing >= count as f32 * (170.0 + spacing) && ui.available_height() > 520.0;
        let columns = if full {
            count
        } else {
            (((available + spacing) / (112.0 + spacing)).floor() as usize).clamp(2, count)
        };
        let width = ((available - spacing * (columns - 1) as f32) / columns as f32).floor();
        let mut open = None;
        for row in cards.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = spacing;
                for entry in row {
                    let response = card(ui, width, entry, full);
                    if response.on_hover_text("Détails dans Performances").clicked() {
                        open = Some(entry.page.clone());
                    }
                }
            });
            ui.add_space(spacing);
        }
        if let Some(page) = open {
            self.open_page(page, snapshot);
        }
    }

    /// Ouvre une page de Performances ; « Disques » et « Réseau » sans nom ouvrent le
    /// plus actif.
    fn open_page(&mut self, page: Page, snapshot: &Snapshot) {
        self.page = match page {
            Page::Disk(name) if name.is_empty() => snapshot
                .disks
                .iter()
                .max_by(|a, b| (a.read + a.write).total_cmp(&(b.read + b.write)))
                .map(|disk| Page::Disk(disk.info.name.clone()))
                .unwrap_or(Page::Cpu),
            Page::Network(name) if name.is_empty() => snapshot
                .interfaces
                .iter()
                .max_by(|a, b| (a.down + a.up).total_cmp(&(b.down + b.up)))
                .map(|interface| Page::Network(interface.name.clone()))
                .unwrap_or(Page::Cpu),
            page => page,
        };
        self.tab = Tab::Performance;
    }

    fn process_list(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let processes = self.visible();
        // Place laissée à la barre de défilement de la liste.
        let width = ui.available_width() - 10.0;
        let gpu_known = snapshot.gpus.iter().any(|gpu| gpu.usage.is_some())
            || snapshot.processes.iter().any(|process| process.gpu_memory > 0);
        let gpu_total = snapshot
            .gpus
            .iter()
            .filter_map(|gpu| gpu.usage)
            .fold(0.0_f32, f32::max);
        let memory_fraction = snapshot.memory.used as f32 / snapshot.memory.total.max(1) as f32;
        // Colonnes ôtées une à une quand la fenêtre rétrécit (la moins utile d'abord).
        let mut columns: Vec<(Column, &str, Option<String>, f32, bool)> = vec![
            (Column::Name, "Nom", None, 0.0, false),
            (Column::User, "Utilisateur", None, 104.0, false),
            (
                Column::Cpu,
                "Processeur",
                Some(format_percent(snapshot.cpu)),
                104.0,
                true,
            ),
            (
                Column::Memory,
                "Mémoire",
                Some(format_percent(memory_fraction)),
                96.0,
                true,
            ),
            (
                Column::Disk,
                "Disque",
                Some(format_rate(snapshot.disk_read + snapshot.disk_write)),
                96.0,
                true,
            ),
        ];
        if gpu_known {
            columns.push((Column::Gpu, "GPU", Some(format_percent(gpu_total)), 76.0, true));
        }
        columns.push((Column::Threads, "Fils", None, 56.0, true));
        columns.push((Column::Pid, "PID", None, 72.0, true));
        for column in [
            Column::Threads,
            Column::User,
            Column::Pid,
            Column::Disk,
            Column::Gpu,
        ] {
            let fixed: f32 = columns.iter().map(|column| column.3).sum();
            if width - fixed >= 200.0 {
                break;
            }
            columns.retain(|entry| entry.0 != column);
        }
        let fixed: f32 = columns.iter().map(|column| column.3).sum();
        if let Some(name) = columns.first_mut() {
            name.3 = (width - fixed - 12.0).max(140.0);
        }
        // Le tri ne reste pas sur une colonne cachée.
        if !columns.iter().any(|column| column.0 == self.sort) {
            self.sort = Column::Cpu;
        }
        let header_height = 42.0;
        Frame::new()
            .fill(style::background())
            .stroke(Stroke::new(1.0, style::separator()))
            .corner_radius(10.0)
            .show(ui, |ui| {
                ui.set_width(width);
                // En-têtes cliquables : tri (deuxième clic : ordre inverse). Au-dessus du
                // titre, le total de la colonne (comme le Gestionnaire des tâches).
                let (header, _) = ui.allocate_exact_size(egui::vec2(width, header_height), Sense::hover());
                let mut x = header.left() + 12.0;
                for (column, title, total, column_width, right) in &columns {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(x, header.top()),
                        egui::vec2(*column_width, header_height),
                    );
                    let response = ui.interact(rect, ui.scope_id().with(("colonne", *title)), Sense::click());
                    let active = self.sort == *column;
                    if response.hovered() {
                        ui.painter().rect_filled(
                            rect.shrink2(egui::vec2(2.0, 4.0)),
                            6.0,
                            style::control_hover(),
                        );
                    }
                    let arrow = match (active, self.descending) {
                        (true, true) => " ▼",
                        (true, false) => " ▲",
                        _ => "",
                    };
                    let bottom = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.center().y), rect.max);
                    paint_cell(
                        ui,
                        bottom,
                        &format!("{title}{arrow}"),
                        *right,
                        if active {
                            style::accent_ink()
                        } else {
                            style::text_secondary()
                        },
                        FontId::new(
                            11.5,
                            if active {
                                style::strong_family()
                            } else {
                                egui::FontFamily::Proportional
                            },
                        ),
                    );
                    if let Some(total) = total {
                        let top = egui::Rect::from_min_max(
                            rect.min,
                            egui::pos2(rect.right(), rect.center().y + 2.0),
                        );
                        paint_cell(
                            ui,
                            top,
                            total,
                            *right,
                            style::primary_text(),
                            FontId::new(14.0, style::strong_family()),
                        );
                    }
                    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        if active {
                            self.descending = !self.descending;
                        } else {
                            self.sort = *column;
                            self.descending = !matches!(column, Column::Name | Column::User | Column::Pid);
                        }
                    }
                    x += column_width;
                }
                ui.painter().hline(
                    header.x_range(),
                    header.bottom(),
                    Stroke::new(1.0, style::separator()),
                );

                if processes.is_empty() {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("Aucun processus ne correspond").color(style::text_secondary()),
                        );
                    });
                    ui.add_space(24.0);
                    return;
                }
                let memory_total = snapshot.memory.total.max(1) as f32;
                egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                    ui,
                    ROW,
                    processes.len(),
                    |ui, range| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for index in range {
                            let process = &processes[index];
                            let (rect, response) =
                                ui.allocate_exact_size(egui::vec2(width, ROW), Sense::click());
                            let selected = self.selected == Some(process.pid);
                            if selected {
                                ui.painter().rect_filled(
                                    rect.shrink2(egui::vec2(4.0, 1.0)),
                                    7.0,
                                    style::accent(),
                                );
                            } else if response.hovered() {
                                ui.painter().rect_filled(
                                    rect.shrink2(egui::vec2(4.0, 1.0)),
                                    7.0,
                                    style::control_hover(),
                                );
                            }
                            let ink = if selected {
                                style::accent_text()
                            } else {
                                style::primary_text()
                            };
                            let secondary = if selected {
                                style::accent_text().gamma_multiply(0.85)
                            } else {
                                style::text_secondary()
                            };
                            let mut x = rect.left() + 12.0;
                            for (column, _, _, column_width, right) in &columns {
                                let cell = egui::Rect::from_min_size(
                                    egui::pos2(x, rect.top()),
                                    egui::vec2(*column_width, ROW),
                                );
                                x += column_width;
                                // Chaleur : fond de la couleur de la mesure, d'autant plus
                                // franc que le processus en consomme.
                                let heat = match column {
                                    Column::Cpu => Some((process.cpu * 4.0, CPU_COLOR)),
                                    Column::Memory => {
                                        Some((process.memory as f32 / memory_total * 8.0, MEMORY_COLOR))
                                    }
                                    Column::Disk => {
                                        Some(((process.disk / (16.0 * 1024.0 * 1024.0)) as f32, READ_COLOR))
                                    }
                                    Column::Gpu => Some((process.gpu * 2.0, GPU_COLOR)),
                                    _ => None,
                                };
                                if let Some((level, color)) = heat
                                    && !selected
                                    && level > 0.02
                                {
                                    let alpha = 0.08 + 0.32 * level.clamp(0.0, 1.0);
                                    ui.painter().rect_filled(
                                        cell.shrink2(egui::vec2(1.0, 2.0)),
                                        4.0,
                                        color.gamma_multiply(alpha),
                                    );
                                }
                                let (text, color) = match column {
                                    Column::Name => (process.name.clone(), ink),
                                    Column::User => (process.user.clone(), secondary),
                                    Column::Cpu => (format_decimal(process.cpu * 100.0, 1), ink),
                                    Column::Memory => (format_bytes(process.memory), ink),
                                    Column::Disk if process.mine => (format_rate(process.disk), ink),
                                    Column::Disk => ("—".into(), secondary),
                                    Column::Gpu if process.gpu > 0.0005 || process.gpu_memory > 0 => {
                                        (format_decimal(process.gpu * 100.0, 1), ink)
                                    }
                                    Column::Gpu => ("—".into(), secondary),
                                    Column::Threads => (process.threads.to_string(), secondary),
                                    Column::Pid => (process.pid.to_string(), secondary),
                                };
                                paint_cell(ui, cell, &text, *right, color, FontId::proportional(13.0));
                            }
                            let command = if process.command.is_empty() {
                                format!("{} (processus du noyau)", process.name)
                            } else {
                                process.command.clone()
                            };
                            let mut hover = format!(
                                "{command}\nLancé il y a {} · {} fils",
                                format_duration(process.age),
                                process.threads
                            );
                            if process.gpu_memory > 0 {
                                hover.push_str(&format!(
                                    "\nMémoire graphique : {}",
                                    format_bytes(process.gpu_memory)
                                ));
                            }
                            let response = response.on_hover_text_at_pointer(hover);
                            if response.clicked() || response.secondary_clicked() {
                                self.selected = Some(process.pid);
                            }
                            if response.double_clicked() {
                                self.ask_terminate();
                            }
                            response.context_menu(|ui| {
                                if ui.button("Terminer…").clicked() {
                                    self.selected = Some(process.pid);
                                    self.ask_terminate();
                                    ui.close();
                                }
                                if ui.button("Copier la commande").clicked() {
                                    ui.ctx().copy_text(command.clone());
                                    ui.close();
                                }
                            });
                        }
                    },
                );
            });
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.confirm.clone() else {
            return;
        };
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("moniteur-terminer")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.heading(format!("Terminer « {} » ?", confirm.name));
            ui.label(
                RichText::new(
                    "Les données non enregistrées de ce programme peuvent être perdues. \
                     « Forcer l'arrêt » sert quand il ne répond plus.",
                )
                .color(style::text_secondary()),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.button("Forcer l'arrêt").clicked() {
                    answer = Some(Some(true));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let button = egui::Button::new(RichText::new("Terminer").color(style::accent_text()))
                        .fill(style::error());
                    if ui.add(button).clicked() {
                        answer = Some(Some(false));
                    }
                    if ui.button("Annuler").clicked() {
                        answer = Some(None);
                    }
                });
            });
        });
        if modal.should_close() {
            answer = Some(None);
        }
        if let Some(answer) = answer {
            self.confirm = None;
            if let Some(force) = answer {
                self.terminate(&confirm, force);
            }
        }
    }
}

/// Fréquence moyenne des cœurs (kHz).
fn average_frequency(snapshot: &Snapshot) -> Option<u64> {
    let known: Vec<u64> = snapshot
        .frequencies
        .iter()
        .copied()
        .filter(|khz| *khz > 0)
        .collect();
    (!known.is_empty()).then(|| known.iter().sum::<u64>() / known.len() as u64)
}

/// Carte de synthèse de l'onglet Processus.
struct Card<'a> {
    page: Page,
    title: &'static str,
    value: String,
    detail: String,
    series: Vec<(&'a VecDeque<f32>, Color32)>,
    max: Option<f32>,
}

impl<'a> Card<'a> {
    fn new(
        page: Page,
        title: &'static str,
        value: String,
        detail: String,
        series: Vec<(&'a VecDeque<f32>, Color32)>,
        max: Option<f32>,
    ) -> Self {
        Self {
            page,
            title,
            value,
            detail,
            series,
            max,
        }
    }
}

/// Carte de synthèse cliquable ; sans `with_graph`, version compacte.
fn card(ui: &mut egui::Ui, width: f32, entry: &Card, with_graph: bool) -> egui::Response {
    let Card {
        title,
        value,
        detail,
        series,
        max,
        ..
    } = entry;
    let (title, max) = (*title, *max);
    let id = ui.scope_id().with(("carte", title));
    let hovered = ui
        .ctx()
        .read_response(id)
        .is_some_and(|response| response.hovered());
    let response = Frame::new()
        .fill(if hovered {
            style::surface_hover()
        } else {
            style::background()
        })
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(12.0)
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            // La rangée de cartes est horizontale : le contenu de chacune s'empile.
            ui.vertical(|ui| {
                ui.set_width(width - 26.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 14.0), Sense::hover());
                    if let Some((_, color)) = series.first() {
                        ui.painter().circle_filled(dot.center(), 3.5, *color);
                    }
                    ui.label(
                        RichText::new(title)
                            .size(typography::SMALL)
                            .color(style::text_secondary()),
                    );
                });
                ui.label(
                    RichText::new(value)
                        .size(if with_graph { 22.0 } else { 17.0 })
                        .family(style::strong_family()),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(detail)
                            .size(typography::SMALL)
                            .color(style::text_secondary()),
                    )
                    .truncate(),
                );
                if with_graph {
                    ui.add_space(6.0);
                    graph(ui, egui::vec2(width - 26.0, 42.0), series, max);
                }
            });
        })
        .response;
    ui.interact(response.rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Graphique en aires de la dernière minute, au dégradé vertical ; sans `max`, l'échelle
/// suit la plus grande valeur (débits). Renvoie l'échelle retenue.
fn graph(ui: &mut egui::Ui, size: egui::Vec2, series: &[(&VecDeque<f32>, Color32)], max: Option<f32>) -> f32 {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, style::content_gray());
    let grid = Stroke::new(1.0, style::separator().gamma_multiply(0.7));
    for fraction in [0.25, 0.5, 0.75] {
        let y = rect.bottom() - rect.height() * fraction;
        painter.hline(rect.x_range().shrink(1.0), y, grid);
    }
    // Un trait toutes les 10 secondes, sur les grands graphiques.
    if rect.height() > 80.0 {
        for tick in 1..6 {
            let x = rect.left() + rect.width() * tick as f32 / 6.0;
            painter.vline(x, rect.y_range().shrink(1.0), grid);
        }
    }
    let scale =
        max.unwrap_or_else(|| nice_scale(series.iter().flat_map(|(values, _)| values.iter().copied())));
    let step = rect.width() / (HISTORY - 1) as f32;
    let inner = rect.shrink2(egui::vec2(0.0, 1.0));
    for (values, color) in series {
        if values.len() < 2 {
            continue;
        }
        // Les points les plus récents à droite.
        let start = HISTORY - values.len();
        let points: Vec<egui::Pos2> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let x = rect.left() + (start + index) as f32 * step;
                let y = inner.bottom() - (value / scale).clamp(0.0, 1.0) * (inner.height() - 2.0);
                egui::pos2(x, y)
            })
            .collect();
        let mut mesh = egui::Mesh::default();
        let top = color.gamma_multiply(0.34);
        let bottom = color.gamma_multiply(0.04);
        // Dégradé : chaque point tient la teinte de sa hauteur, le bas reste pâle.
        for pair in points.windows(2) {
            let base = mesh.vertices.len() as u32;
            let shade = |y: f32| {
                let t = ((inner.bottom() - y) / inner.height()).clamp(0.0, 1.0);
                lerp_color(bottom, top, 0.35 + 0.65 * t)
            };
            mesh.colored_vertex(pair[0], shade(pair[0].y));
            mesh.colored_vertex(pair[1], shade(pair[1].y));
            mesh.colored_vertex(egui::pos2(pair[1].x, inner.bottom()), bottom);
            mesh.colored_vertex(egui::pos2(pair[0].x, inner.bottom()), bottom);
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        painter.add(egui::Shape::mesh(mesh));
        let last = *points.last().unwrap_or(&rect.right_bottom());
        painter.line(points, Stroke::new(1.8, *color));
        if rect.height() > 60.0 {
            painter.circle_filled(last, 3.0, *color);
        }
    }
    scale
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// Échelle « ronde » au-dessus de la plus grande valeur (1, 2 ou 5 × 1024ⁿ), au moins
/// 64 Kio/s : un graphique de débit au repos reste plat.
fn nice_scale(values: impl Iterator<Item = f32>) -> f32 {
    let peak = values.fold(0.0_f32, f32::max).max(64.0 * 1024.0) * 1.1;
    let mut unit = 1.0_f32;
    while unit * 1024.0 <= peak {
        unit *= 1024.0;
    }
    [
        1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 1024.0,
    ]
    .iter()
    .map(|step| step * unit)
    .find(|value| *value >= peak)
    .unwrap_or(peak)
}

fn paint_bar(painter: &egui::Painter, rect: egui::Rect, fraction: f32, color: Color32) {
    let radius = CornerRadius::same((rect.height() / 2.0) as u8);
    painter.rect_filled(rect, radius, style::track());
    let filled = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
    );
    if filled.width() > 0.5 {
        painter.rect_filled(filled, radius, color);
    }
}

/// Texte d'une cellule, coupé avec « … » s'il dépasse.
fn paint_cell(ui: &egui::Ui, rect: egui::Rect, text: &str, right: bool, color: Color32, font: FontId) {
    let max = rect.width() - 14.0;
    let mut shown = text.to_owned();
    let mut galley = ui.painter().layout_no_wrap(shown.clone(), font.clone(), color);
    while galley.size().x > max && shown.chars().count() > 1 {
        shown.pop();
        galley = ui
            .painter()
            .layout_no_wrap(format!("{}…", shown.trim_end()), font.clone(), color);
    }
    let y = rect.center().y - galley.size().y / 2.0;
    let x = if right {
        rect.right() - 14.0 - galley.size().x
    } else {
        rect.left()
    };
    ui.painter().galley(egui::pos2(x, y), galley, color);
}

/// Nombre à virgule, à la française.
fn format_decimal(value: f32, digits: usize) -> String {
    format!("{value:.digits$}").replace('.', ",")
}

fn format_percent(fraction: f32) -> String {
    let percent = fraction * 100.0;
    if percent > 0.0 && percent < 10.0 {
        format!("{} %", format_decimal(percent, 1))
    } else {
        format!("{percent:.0} %")
    }
}

/// Entier en groupes de trois chiffres (« 12 345 »), espace fine insécable.
fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push('\u{202F}');
        }
        out.push(digit);
    }
    out
}

fn format_ghz(khz: u64) -> String {
    format!("{} GHz", format_decimal(khz as f32 / 1_000_000.0, 2))
}

fn format_celsius(celsius: f32) -> String {
    format!("{celsius:.0} °C")
}

/// Taille lisible en unités binaires (Kio, Mio, Gio), comme les autres moniteurs Linux.
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["o", "Kio", "Mio", "Gio", "Tio"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} o"),
        _ if value >= 100.0 => format!("{value:.0} {}", UNITS[unit]),
        _ => format!("{value:.1} {}", UNITS[unit]).replace('.', ","),
    }
}

fn format_rate(bytes_per_second: f64) -> String {
    format!("{}/s", format_bytes(bytes_per_second.max(0.0) as u64))
}

fn format_duration(seconds: f64) -> String {
    let minutes = (seconds / 60.0) as u64;
    let (days, hours, minutes) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    match (days, hours) {
        (0, 0) => format!("{minutes} min"),
        (0, _) => format!("{hours} h {minutes:02}"),
        _ => format!("{days} j {hours} h"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailles_et_durees() {
        assert_eq!(format_bytes(512), "512 o");
        assert_eq!(format_bytes(1536), "1,5 Kio");
        assert_eq!(format_bytes(8 * 1024 * 1024 * 1024), "8,0 Gio");
        assert_eq!(format_bytes(300 * 1024 * 1024), "300 Mio");
        assert_eq!(format_rate(2048.0), "2,0 Kio/s");
        assert_eq!(format_duration(125.0), "2 min");
        assert_eq!(format_duration(3.0 * 3600.0 + 300.0), "3 h 05");
        assert_eq!(format_duration(2.0 * 86400.0 + 7200.0), "2 j 2 h");
        assert_eq!(format_percent(0.234), "23 %");
        assert_eq!(format_percent(0.052), "5,2 %");
        assert_eq!(format_percent(0.0), "0 %");
        assert_eq!(format_ghz(3_120_000), "3,12 GHz");
        assert_eq!(format_count(1234567), "1\u{202F}234\u{202F}567");
        assert_eq!(format_count(999), "999");
    }

    #[test]
    fn echelle_des_debits() {
        assert_eq!(nice_scale([0.0].into_iter()), 100.0 * 1024.0);
        assert_eq!(
            nice_scale([3.0 * 1024.0 * 1024.0].into_iter()),
            5.0 * 1024.0 * 1024.0
        );
    }

    #[test]
    fn page_demandee_au_lancement() {
        let monitor = MonitorState::with_page(Some("capteurs"));
        assert_eq!((monitor.tab, monitor.page), (Tab::Performance, Page::Sensors));
        let monitor = MonitorState::with_page(Some("inconnu"));
        assert_eq!(monitor.tab, Tab::Processes);
    }

    #[test]
    fn tri_et_filtre_des_processus() {
        let process = |pid, name: &str, cpu, mine| Process {
            pid,
            name: name.into(),
            user: if mine { "ferrite" } else { "root" }.into(),
            mine,
            cpu,
            memory: u64::from(pid) * 1000,
            threads: 1,
            command: format!("/usr/bin/{name}"),
            ..Process::default()
        };
        let mut monitor = MonitorState {
            snapshot: Some(Arc::new(Snapshot {
                processes: vec![
                    process(1, "systemd", 0.01, false),
                    process(200, "brave", 0.30, true),
                    process(300, "foot", 0.05, true),
                ],
                ..Snapshot::default()
            })),
            ..MonitorState::default()
        };
        let names = |monitor: &MonitorState| -> Vec<String> {
            monitor
                .visible()
                .into_iter()
                .map(|process| process.name)
                .collect()
        };
        assert_eq!(names(&monitor), ["brave", "foot", "systemd"]);
        monitor.only_mine = true;
        assert_eq!(names(&monitor), ["brave", "foot"]);
        monitor.search = "FOO".into();
        assert_eq!(names(&monitor), ["foot"]);
        monitor.search.clear();
        monitor.sort = Column::Memory;
        monitor.descending = false;
        assert_eq!(names(&monitor), ["brave", "foot"]);
    }
}
