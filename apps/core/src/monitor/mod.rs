//! Moniteur d'activité : processeur, mémoire, disques, réseau et processus.
//!
//! Un fil à part mesure le système chaque seconde (`sample::Sampler`) ; l'interface
//! garde une minute d'historique pour les graphiques. Terminer un processus envoie
//! SIGTERM (fermeture propre), « Forcer l'arrêt » envoie SIGKILL.

mod sample;

use std::{
    collections::VecDeque,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use eframe::egui::{self, Color32, CornerRadius, Frame, Key, Margin, RichText, Sense, Stroke};
use ferrite_system::command;

use self::sample::{Process, Sampler, Snapshot};
use crate::style;

/// Points gardés dans les graphiques : une minute, une mesure par seconde.
const HISTORY: usize = 60;
const ROW: f32 = 28.0;
const CPU_COLOR: Color32 = Color32::from_rgb(0x36, 0x89, 0xE6);
const MEMORY_COLOR: Color32 = Color32::from_rgb(0x9B, 0x59, 0xD0);
const READ_COLOR: Color32 = Color32::from_rgb(0x2E, 0xA0, 0x5A);
const WRITE_COLOR: Color32 = Color32::from_rgb(0xE5, 0x7A, 0x1F);
const DOWN_COLOR: Color32 = Color32::from_rgb(0x1A, 0xA3, 0xB8);
const UP_COLOR: Color32 = Color32::from_rgb(0xD9, 0x41, 0x50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Processes,
    Resources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Name,
    User,
    Cpu,
    Memory,
    Threads,
    Pid,
}

#[derive(Debug, Default)]
struct History {
    cpu: VecDeque<f32>,
    memory: VecDeque<f32>,
    read: VecDeque<f32>,
    write: VecDeque<f32>,
    down: VecDeque<f32>,
    up: VecDeque<f32>,
}

impl History {
    fn push(&mut self, snapshot: &Snapshot) {
        let memory = if snapshot.memory.total > 0 {
            snapshot.memory.used as f32 / snapshot.memory.total as f32
        } else {
            0.0
        };
        for (series, value) in [
            (&mut self.cpu, snapshot.cpu),
            (&mut self.memory, memory),
            (&mut self.read, snapshot.disk_read as f32),
            (&mut self.write, snapshot.disk_write as f32),
            (&mut self.down, snapshot.net_down as f32),
            (&mut self.up, snapshot.net_up as f32),
        ] {
            series.push_back(value);
            if series.len() > HISTORY {
                series.pop_front();
            }
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
    snapshot: Option<Snapshot>,
    history: History,
    tab: Tab,
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
        f.debug_struct("MonitorState").field("tab", &self.tab).finish()
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
            self.snapshot = Some(snapshot);
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
            if ui.input(|input| input.key_pressed(Key::Delete)) {
                self.ask_terminate();
            }
            if ui.input(|input| input.key_pressed(Key::Escape)) {
                self.selected = None;
            }
        }

        style::header(ui, |ui| self.header(ui));
        self.status_bar(ui);
        egui::CentralPanel::no_frame()
            .frame(
                Frame::new()
                    .fill(style::background())
                    .inner_margin(Margin::same(14)),
            )
            .show(ui, |ui| {
                if self.snapshot.is_none() {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Mesure en cours…").color(style::text_secondary()));
                    });
                    return;
                }
                match self.tab {
                    Tab::Processes => {
                        self.cards(ui);
                        ui.add_space(12.0);
                        self.process_list(ui);
                    }
                    Tab::Resources => self.resources(ui),
                }
            });
        self.confirm_dialog(&ctx);
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        // Onglets en contrôle segmenté, d'une hauteur fixe dans la barre d'outils.
        let (pill, _) = ui.allocate_exact_size(egui::vec2(2.0 * 96.0 + 6.0, 30.0), Sense::hover());
        ui.painter().rect_filled(pill, 8.0, Color32::from_black_alpha(14));
        let mut tabs = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(pill.shrink(3.0))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        tabs.spacing_mut().item_spacing.x = 0.0;
        for (tab, text) in [(Tab::Processes, "Processus"), (Tab::Resources, "Ressources")] {
            let selected = self.tab == tab;
            let button = egui::Button::new(RichText::new(text).color(style::primary_text()))
                .fill(if selected {
                    style::background()
                } else {
                    Color32::TRANSPARENT
                })
                .stroke(Stroke::NONE)
                .corner_radius(6.0)
                .min_size(egui::vec2(96.0, 24.0));
            if tabs.add(button).clicked() {
                self.tab = tab;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if self.tab == Tab::Processes {
                let enabled = self.selected.is_some();
                let button = egui::Button::new(RichText::new("Terminer…").color(if enabled {
                    style::accent_text()
                } else {
                    style::text_secondary()
                }))
                .fill(if enabled {
                    style::error()
                } else {
                    Color32::from_black_alpha(10)
                });
                if ui
                    .add_enabled(enabled, button)
                    .on_hover_text("Terminer le processus choisi (Suppr)")
                    .clicked()
                {
                    self.ask_terminate();
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Rechercher un processus")
                        .desired_width(200.0),
                );
                ui.checkbox(&mut self.only_mine, "Mes processus");
            }
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        let Some(snapshot) = &self.snapshot else { return };
        let count = snapshot.processes.len();
        let threads: u32 = snapshot.processes.iter().map(|process| process.threads).sum();
        let message = self.message.clone();
        let (uptime, load) = (snapshot.uptime, snapshot.load);
        style::status_bar(ui, |ui| {
            ui.label(format!("{count} processus · {threads} fils"));
            ui.label("·");
            ui.label(format!("Allumé depuis {}", format_duration(uptime)));
            ui.label("·");
            ui.label(format!("Charge {:.2} {:.2} {:.2}", load[0], load[1], load[2]))
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

    /// Quatre cartes de synthèse, chacune avec son graphique de la dernière minute.
    fn cards(&mut self, ui: &mut egui::Ui) {
        let Some(snapshot) = &self.snapshot else { return };
        let memory = &snapshot.memory;
        let history = &self.history;
        let spacing = 10.0;
        let width = ((ui.available_width() - 3.0 * spacing) / 4.0).floor().max(120.0);
        let mut open_resources = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = spacing;
            let cards = [
                (
                    "Processeur",
                    format!("{:.0} %", snapshot.cpu * 100.0),
                    format!("{} cœurs", snapshot.cores.len()),
                    vec![(&history.cpu, CPU_COLOR)],
                    Some(1.0),
                ),
                (
                    "Mémoire",
                    format_bytes(memory.used),
                    format!("sur {}", format_bytes(memory.total)),
                    vec![(&history.memory, MEMORY_COLOR)],
                    Some(1.0),
                ),
                (
                    "Disque",
                    format!("{}/s", format_bytes(snapshot.disk_read as u64)),
                    format!("lus · {}/s écrits", format_bytes(snapshot.disk_write as u64)),
                    vec![(&history.read, READ_COLOR), (&history.write, WRITE_COLOR)],
                    None,
                ),
                (
                    "Réseau",
                    format!("{}/s", format_bytes(snapshot.net_down as u64)),
                    format!("reçus · {}/s envoyés", format_bytes(snapshot.net_up as u64)),
                    vec![(&history.down, DOWN_COLOR), (&history.up, UP_COLOR)],
                    None,
                ),
            ];
            for (title, value, detail, series, max) in cards {
                let response = card(ui, width, title, &value, &detail, &series, max);
                if response.on_hover_text("Détails dans Ressources").clicked() {
                    open_resources = true;
                }
            }
        });
        if open_resources {
            self.tab = Tab::Resources;
        }
    }

    fn process_list(&mut self, ui: &mut egui::Ui) {
        let processes = self.visible();
        // Place laissée à la barre de défilement de la liste.
        let width = ui.available_width() - 10.0;
        let fixed = [110.0, 86.0, 90.0, 56.0, 74.0];
        let name_width = (width - fixed.iter().sum::<f32>()).max(160.0);
        let columns = [
            (Column::Name, "Nom", name_width, false),
            (Column::User, "Utilisateur", fixed[0], false),
            (Column::Cpu, "% proc.", fixed[1], true),
            (Column::Memory, "Mémoire", fixed[2], true),
            (Column::Threads, "Fils", fixed[3], true),
            (Column::Pid, "PID", fixed[4], true),
        ];
        Frame::new()
            .fill(style::background())
            .stroke(Stroke::new(1.0, style::separator()))
            .corner_radius(10.0)
            .show(ui, |ui| {
                ui.set_width(width);
                // En-têtes cliquables : tri (deuxième clic : ordre inverse).
                let (header, _) = ui.allocate_exact_size(egui::vec2(width, 30.0), Sense::hover());
                let mut x = header.left() + 12.0;
                for (column, title, column_width, right) in columns {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(x, header.top()),
                        egui::vec2(column_width, 30.0),
                    );
                    let response = ui.interact(rect, ui.scope_id().with(("colonne", title)), Sense::click());
                    let active = self.sort == column;
                    let arrow = match (active, self.descending) {
                        (true, true) => " ▼",
                        (true, false) => " ▲",
                        _ => "",
                    };
                    paint_cell(
                        ui,
                        rect,
                        &format!("{title}{arrow}"),
                        right,
                        if active {
                            style::primary_text()
                        } else {
                            style::text_secondary()
                        },
                        11.5,
                    );
                    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        if active {
                            self.descending = !self.descending;
                        } else {
                            self.sort = column;
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
                            let fill = if selected {
                                Some(style::accent())
                            } else if response.hovered() {
                                Some(style::control_hover())
                            } else if index % 2 == 1 {
                                Some(style::content_gray())
                            } else {
                                None
                            };
                            if let Some(fill) = fill {
                                ui.painter()
                                    .rect_filled(rect.shrink2(egui::vec2(4.0, 1.0)), 6.0, fill);
                            }
                            let ink = if selected {
                                style::accent_text()
                            } else {
                                style::primary_text()
                            };
                            let secondary = if selected {
                                Color32::from_white_alpha(220)
                            } else {
                                style::text_secondary()
                            };
                            let cells = [
                                (process.name.clone(), ink),
                                (process.user.clone(), secondary),
                                (format!("{:.1}", process.cpu * 100.0), ink),
                                (format_bytes(process.memory), ink),
                                (process.threads.to_string(), secondary),
                                (process.pid.to_string(), secondary),
                            ];
                            let mut x = rect.left() + 12.0;
                            for ((_, _, column_width, right), (text, color)) in columns.iter().zip(cells) {
                                let cell = egui::Rect::from_min_size(
                                    egui::pos2(x, rect.top()),
                                    egui::vec2(*column_width, ROW),
                                );
                                paint_cell(ui, cell, &text, *right, color, 13.0);
                                x += column_width;
                            }
                            let command = if process.command.is_empty() {
                                format!("{} (processus du noyau)", process.name)
                            } else {
                                process.command.clone()
                            };
                            let response = response.on_hover_text_at_pointer(format!(
                                "{command}
Lancé il y a {}",
                                format_duration(process.age)
                            ));
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
                            });
                        }
                    },
                );
            });
    }

    fn resources(&self, ui: &mut egui::Ui) {
        let Some(snapshot) = &self.snapshot else { return };
        let history = &self.history;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            // Place laissée à la barre de défilement.
            let width = ui.available_width() - 14.0;

            section(ui, "Processeur", &format!("{:.0} %", snapshot.cpu * 100.0));
            panel(ui, width, |ui| {
                graph(
                    ui,
                    egui::vec2(width - 28.0, 110.0),
                    &[(&history.cpu, CPU_COLOR)],
                    Some(1.0),
                );
                ui.add_space(10.0);
                // Un petit indicateur par cœur.
                ui.horizontal_wrapped(|ui| {
                    for (index, core) in snapshot.cores.iter().enumerate() {
                        meter(ui, &format!("Cœur {}", index + 1), *core, CPU_COLOR, 120.0);
                    }
                });
            });

            let memory = &snapshot.memory;
            section(
                ui,
                "Mémoire",
                &format!("{} sur {}", format_bytes(memory.used), format_bytes(memory.total)),
            );
            panel(ui, width, |ui| {
                graph(
                    ui,
                    egui::vec2(width - 28.0, 90.0),
                    &[(&history.memory, MEMORY_COLOR)],
                    Some(1.0),
                );
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    legend(ui, MEMORY_COLOR, "Utilisée", &format_bytes(memory.used));
                    legend(
                        ui,
                        Color32::from_gray(0xB8),
                        "En cache",
                        &format_bytes(memory.cached),
                    );
                    if memory.swap_total > 0 {
                        legend(
                            ui,
                            WRITE_COLOR,
                            "Échange",
                            &format!(
                                "{} sur {}",
                                format_bytes(memory.swap_used),
                                format_bytes(memory.swap_total)
                            ),
                        );
                    } else {
                        legend(ui, WRITE_COLOR, "Échange", "aucun");
                    }
                });
            });

            section(ui, "Disques", "");
            panel(ui, width, |ui| {
                graph(
                    ui,
                    egui::vec2(width - 28.0, 80.0),
                    &[(&history.read, READ_COLOR), (&history.write, WRITE_COLOR)],
                    None,
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    legend(
                        ui,
                        READ_COLOR,
                        "Lecture",
                        &format!("{}/s", format_bytes(snapshot.disk_read as u64)),
                    );
                    legend(
                        ui,
                        WRITE_COLOR,
                        "Écriture",
                        &format!("{}/s", format_bytes(snapshot.disk_write as u64)),
                    );
                });
                ui.add_space(10.0);
                if snapshot.volumes.is_empty() {
                    ui.label(RichText::new("Aucun disque monté").color(style::text_secondary()));
                }
                for volume in &snapshot.volumes {
                    let fraction = volume.used as f32 / volume.size.max(1) as f32;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&volume.mount).family(style::strong_family()));
                        ui.label(
                            RichText::new(&volume.device)
                                .small()
                                .color(style::text_secondary()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(format!(
                                "{} libres sur {}",
                                format_bytes(volume.size.saturating_sub(volume.used)),
                                format_bytes(volume.size)
                            ));
                        });
                    });
                    let color = if fraction > 0.9 {
                        style::error()
                    } else {
                        READ_COLOR
                    };
                    bar(ui, width - 28.0, fraction, color);
                    ui.add_space(8.0);
                }
            });

            section(ui, "Réseau", "");
            panel(ui, width, |ui| {
                graph(
                    ui,
                    egui::vec2(width - 28.0, 80.0),
                    &[(&history.down, DOWN_COLOR), (&history.up, UP_COLOR)],
                    None,
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    legend(
                        ui,
                        DOWN_COLOR,
                        "Réception",
                        &format!("{}/s", format_bytes(snapshot.net_down as u64)),
                    );
                    legend(
                        ui,
                        UP_COLOR,
                        "Envoi",
                        &format!("{}/s", format_bytes(snapshot.net_up as u64)),
                    );
                    legend(
                        ui,
                        Color32::from_gray(0xB8),
                        "Depuis le démarrage",
                        &format!(
                            "{} reçus · {} envoyés",
                            format_bytes(snapshot.net_down_total),
                            format_bytes(snapshot.net_up_total)
                        ),
                    );
                });
            });
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

/// Carte de synthèse cliquable.
fn card(
    ui: &mut egui::Ui,
    width: f32,
    title: &str,
    value: &str,
    detail: &str,
    series: &[(&VecDeque<f32>, Color32)],
    max: Option<f32>,
) -> egui::Response {
    let response = Frame::new()
        .fill(style::background())
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(10.0)
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            // La rangée de cartes est horizontale : le contenu de chacune s'empile.
            ui.vertical(|ui| {
                ui.set_width(width - 26.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(title).small().color(style::text_secondary()));
                ui.label(RichText::new(value).size(20.0).family(style::strong_family()));
                ui.add(
                    egui::Label::new(RichText::new(detail).small().color(style::text_secondary())).truncate(),
                );
                ui.add_space(6.0);
                graph(ui, egui::vec2(width - 26.0, 44.0), series, max);
            });
        })
        .response;
    ui.interact(
        response.rect,
        ui.scope_id().with(("carte", title)),
        Sense::click(),
    )
}

fn section(ui: &mut egui::Ui, title: &str, value: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(15.0).family(style::strong_family()));
        ui.label(RichText::new(value).color(style::text_secondary()));
    });
    ui.add_space(4.0);
}

fn panel(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(style::background())
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(10.0)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            add(ui);
        });
    ui.add_space(10.0);
}

/// Graphique en aires de la dernière minute ; sans `max`, l'échelle suit la plus grande
/// valeur (débits).
fn graph(ui: &mut egui::Ui, size: egui::Vec2, series: &[(&VecDeque<f32>, Color32)], max: Option<f32>) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, style::content_gray());
    for fraction in [0.25, 0.5, 0.75] {
        let y = rect.bottom() - rect.height() * fraction;
        painter.hline(rect.x_range(), y, Stroke::new(1.0, style::separator()));
    }
    let scale = max.unwrap_or_else(|| {
        series
            .iter()
            .flat_map(|(values, _)| values.iter().copied())
            .fold(1024.0_f32, f32::max)
            * 1.15
    });
    let step = rect.width() / (HISTORY - 1) as f32;
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
                let y = rect.bottom() - (value / scale).clamp(0.0, 1.0) * (rect.height() - 2.0);
                egui::pos2(x, y)
            })
            .collect();
        let mut mesh = egui::Mesh::default();
        let fill = color.gamma_multiply(0.18);
        for pair in points.windows(2) {
            let base = mesh.vertices.len() as u32;
            for pos in [
                pair[0],
                pair[1],
                egui::pos2(pair[1].x, rect.bottom()),
                egui::pos2(pair[0].x, rect.bottom()),
            ] {
                mesh.colored_vertex(pos, fill);
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        painter.add(egui::Shape::mesh(mesh));
        painter.line(points, Stroke::new(1.6, *color));
    }
}

fn meter(ui: &mut egui::Ui, label: &str, value: f32, color: Color32, width: f32) {
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).small().color(style::text_secondary()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{:.0} %", value * 100.0)).small());
            });
        });
        bar(ui, width, value, color);
    });
}

fn bar(ui: &mut egui::Ui, width: f32, fraction: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, style::control_background());
    let filled =
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction.clamp(0.0, 1.0), 6.0));
    ui.painter().rect_filled(filled, CornerRadius::same(3), color);
}

fn legend(ui: &mut egui::Ui, color: Color32, label: &str, value: &str) {
    let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(dot.center(), 4.0, color);
    ui.label(RichText::new(label).color(style::text_secondary()));
    ui.label(value);
    ui.add_space(14.0);
}

/// Texte d'une cellule, coupé avec « … » s'il dépasse.
fn paint_cell(ui: &egui::Ui, rect: egui::Rect, text: &str, right: bool, color: Color32, size: f32) {
    let font = egui::FontId::proportional(size);
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
        assert_eq!(format_duration(125.0), "2 min");
        assert_eq!(format_duration(3.0 * 3600.0 + 300.0), "3 h 05");
        assert_eq!(format_duration(2.0 * 86400.0 + 7200.0), "2 j 2 h");
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
            age: 0.0,
        };
        let mut monitor = MonitorState {
            snapshot: Some(Snapshot {
                processes: vec![
                    process(1, "systemd", 0.01, false),
                    process(200, "brave", 0.30, true),
                    process(300, "foot", 0.05, true),
                ],
                ..Snapshot::default()
            }),
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
