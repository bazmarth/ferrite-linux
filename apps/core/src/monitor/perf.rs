//! Onglet Performances : une colonne d'appareils (processeur, mémoire, chaque carte
//! graphique, chaque disque, chaque interface réseau, batterie, capteurs, système), et
//! la page détaillée de l'appareil choisi. Fenêtre étroite : la colonne devient un
//! bandeau défilant au-dessus de la page.

use eframe::egui::{self, Color32, FontId, Frame, Margin, RichText, Sense, Stroke};
use ferrite_ui::tokens::typography;

use super::{
    BATTERY_COLOR, CPU_COLOR, CPU_EFFICIENT_COLOR, DOWN_COLOR, GPU_COLOR, History, MEMORY_COLOR,
    MonitorState, Page, READ_COLOR, SENSOR_COLOR, SYSTEM_COLOR, UP_COLOR, WRITE_COLOR, average_frequency,
    format_bytes, format_celsius, format_count, format_decimal, format_duration, format_ghz, format_percent,
    format_rate, graph,
    hardware::{CoreKind, Gpu, NetKind},
    paint_bar, paint_cell,
    sample::{Disk, Interface, Process, Snapshot},
};
use crate::{kit, style};

/// Largeur de la colonne des appareils ; en dessous de `WIDE`, bandeau.
const COLUMN: f32 = 238.0;
const WIDE: f32 = 780.0;
const ENTRY_HEIGHT: f32 = 66.0;
const GAP: f32 = 10.0;

/// Ligne de la colonne des appareils.
struct Entry {
    page: Page,
    title: String,
    line: String,
    detail: String,
    series: Vec<(String, Color32)>,
    max: Option<f32>,
}

fn entries(snapshot: &Snapshot) -> Vec<Entry> {
    let mut list = Vec::new();
    let mut cpu_detail = format_percent(snapshot.cpu);
    if let Some(khz) = average_frequency(snapshot) {
        cpu_detail.push_str(&format!(" · {}", format_ghz(khz)));
    }
    if let Some(celsius) = snapshot.cpu_temperature {
        cpu_detail.push_str(&format!(" · {}", format_celsius(celsius)));
    }
    list.push(Entry {
        page: Page::Cpu,
        title: "Processeur".into(),
        line: snapshot.info.cpu.model.clone(),
        detail: cpu_detail,
        series: vec![("cpu".into(), CPU_COLOR)],
        max: Some(1.0),
    });
    let memory = &snapshot.memory;
    list.push(Entry {
        page: Page::Memory,
        title: "Mémoire".into(),
        line: format!("{} sur {}", format_bytes(memory.used), format_bytes(memory.total)),
        detail: format_percent(memory.used as f32 / memory.total.max(1) as f32),
        series: vec![("memoire".into(), MEMORY_COLOR)],
        max: Some(1.0),
    });
    let several = snapshot.gpus.len() > 1;
    for (index, gpu) in snapshot.gpus.iter().enumerate() {
        list.push(Entry {
            page: Page::Gpu(gpu.slot.clone()),
            title: if several {
                format!("Carte graphique {index}")
            } else {
                "Carte graphique".into()
            },
            line: gpu.name.clone(),
            detail: gpu_status(gpu),
            series: vec![(format!("gpu/{}", gpu.slot), GPU_COLOR)],
            max: Some(1.0),
        });
    }
    for (index, disk) in snapshot.disks.iter().enumerate() {
        let name = &disk.info.name;
        list.push(Entry {
            page: Page::Disk(name.clone()),
            title: format!("Disque {index} · {}", disk.info.kind),
            line: disk.info.model.clone(),
            detail: format!(
                "{} · {}",
                format_percent(disk.busy),
                format_rate(disk.read + disk.write)
            ),
            series: vec![
                (format!("lecture/{name}"), READ_COLOR),
                (format!("ecriture/{name}"), WRITE_COLOR),
            ],
            max: None,
        });
    }
    for interface in &snapshot.interfaces {
        let name = &interface.name;
        list.push(Entry {
            page: Page::Network(name.clone()),
            title: interface.kind.name().into(),
            line: interface
                .network
                .as_ref()
                .map(|network| format!("{network} · {name}"))
                .unwrap_or_else(|| name.clone()),
            detail: if interface.details.up || interface.kind == NetKind::Virtual {
                format!(
                    "↓ {} · ↑ {}",
                    format_rate(interface.down),
                    format_rate(interface.up)
                )
            } else {
                "Déconnecté".into()
            },
            series: vec![
                (format!("reception/{name}"), DOWN_COLOR),
                (format!("envoi/{name}"), UP_COLOR),
            ],
            max: None,
        });
    }
    if let Some(battery) = snapshot.battery() {
        let mut detail = format_percent(battery.level);
        if let Some(watts) = battery.power {
            detail.push_str(&format!(" · {} W", format_decimal(watts, 1)));
        }
        list.push(Entry {
            page: Page::Battery,
            title: "Batterie".into(),
            line: battery.status.clone(),
            detail,
            series: vec![("batterie".into(), BATTERY_COLOR)],
            max: Some(1.0),
        });
    }
    if !snapshot.sensors.is_empty() {
        let hottest = snapshot
            .sensors
            .iter()
            .flat_map(|group| group.temperatures.iter())
            .map(|temp| temp.celsius)
            .fold(None, |max: Option<f32>, value| {
                Some(max.map_or(value, |max| max.max(value)))
            });
        let fans = snapshot
            .sensors
            .iter()
            .map(|group| group.fans.len())
            .sum::<usize>();
        list.push(Entry {
            page: Page::Sensors,
            title: "Capteurs".into(),
            line: match hottest {
                Some(celsius) => format!("Le plus chaud : {}", format_celsius(celsius)),
                None => "Ventilateurs".into(),
            },
            detail: match fans {
                0 => format!(
                    "{} températures",
                    snapshot
                        .sensors
                        .iter()
                        .map(|group| group.temperatures.len())
                        .sum::<usize>()
                ),
                1 => "1 ventilateur".into(),
                count => format!("{count} ventilateurs"),
            },
            series: vec![("temperature".into(), SENSOR_COLOR)],
            max: Some(100.0),
        });
    }
    list.push(Entry {
        page: Page::System,
        title: "Système".into(),
        line: snapshot
            .info
            .machine
            .clone()
            .unwrap_or_else(|| snapshot.info.hostname.clone()),
        detail: snapshot.info.os.clone(),
        series: Vec::new(),
        max: None,
    });
    list
}

fn gpu_status(gpu: &Gpu) -> String {
    if gpu.asleep {
        return "En veille".into();
    }
    let mut parts = Vec::new();
    if let Some(usage) = gpu.usage {
        parts.push(format_percent(usage));
    }
    if let Some(celsius) = gpu.temperature {
        parts.push(format_celsius(celsius));
    }
    if let (Some(used), Some(total)) = (gpu.memory_used, gpu.memory_total) {
        parts.push(format!("{} / {}", format_bytes(used), format_bytes(total)));
    }
    if parts.is_empty() {
        "Active".into()
    } else {
        parts.join(" · ")
    }
}

/// Page réelle : une page demandée sans nom (« gpu ») prend le premier appareil du
/// genre ; un appareil disparu (clé débranchée) renvoie au processeur.
fn resolve(page: &Page, list: &[Entry]) -> Page {
    let same_kind = |entry: &Entry| {
        matches!(
            (page, &entry.page),
            (Page::Gpu(_), Page::Gpu(_))
                | (Page::Disk(_), Page::Disk(_))
                | (Page::Network(_), Page::Network(_))
        )
    };
    if list.iter().any(|entry| entry.page == *page) {
        return page.clone();
    }
    let unnamed = matches!(page, Page::Gpu(name) | Page::Disk(name) | Page::Network(name) if name.is_empty());
    if unnamed && let Some(entry) = list.iter().find(|entry| same_kind(entry)) {
        return entry.page.clone();
    }
    Page::Cpu
}

impl MonitorState {
    pub(super) fn performance(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let list = entries(snapshot);
        self.page = resolve(&self.page, &list);
        let full = ui.available_rect_before_wrap();
        let mut chosen = None;
        if full.width() >= WIDE {
            let side = egui::Rect::from_min_size(full.min, egui::vec2(COLUMN, full.height()));
            let main = egui::Rect::from_min_max(egui::pos2(side.right() + 16.0, full.top()), full.max);
            let mut side_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(side)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            egui::ScrollArea::vertical()
                .id_salt("moniteur-appareils")
                .auto_shrink(false)
                .show(&mut side_ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for entry in &list {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), ENTRY_HEIGHT),
                            Sense::hover(),
                        );
                        if device_entry(ui, rect, entry, &self.history, entry.page == self.page).clicked() {
                            chosen = Some(entry.page.clone());
                        }
                    }
                });
            // Filet entre la colonne et la page.
            ui.painter().vline(
                side.right() + 8.0,
                side.y_range(),
                Stroke::new(1.0, style::separator()),
            );
            let mut main_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(main)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            self.page_content(&mut main_ui, snapshot);
            ui.allocate_rect(full, Sense::hover());
        } else {
            egui::ScrollArea::horizontal()
                .id_salt("moniteur-bandeau")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for entry in &list {
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(196.0, ENTRY_HEIGHT), Sense::hover());
                            if device_entry(ui, rect, entry, &self.history, entry.page == self.page).clicked()
                            {
                                chosen = Some(entry.page.clone());
                            }
                        }
                    });
                });
            ui.add_space(8.0);
            self.page_content(ui, snapshot);
        }
        if let Some(page) = chosen {
            self.page = page;
        }
    }

    fn page_content(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let page = self.page.clone();
        egui::ScrollArea::vertical()
            .id_salt(("moniteur-page", format!("{page:?}")))
            .auto_shrink(false)
            .show(ui, |ui| {
                // Place laissée à la barre de défilement.
                let width = (ui.available_width() - 12.0).max(200.0);
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                let history = &self.history;
                match &page {
                    Page::Cpu => cpu_page(ui, width, snapshot, history, &mut self.per_core),
                    Page::Memory => memory_page(ui, width, snapshot, history),
                    Page::Gpu(slot) => {
                        if let Some(gpu) = snapshot.gpus.iter().find(|gpu| &gpu.slot == slot) {
                            gpu_page(ui, width, snapshot, gpu, history);
                        }
                    }
                    Page::Disk(name) => {
                        if let Some(disk) = snapshot.disks.iter().find(|disk| &disk.info.name == name) {
                            disk_page(ui, width, disk, history);
                        }
                    }
                    Page::Network(name) => {
                        if let Some(interface) = snapshot
                            .interfaces
                            .iter()
                            .find(|interface| &interface.name == name)
                        {
                            network_page(ui, width, interface, history);
                        }
                    }
                    Page::Battery => battery_page(ui, width, snapshot, history),
                    Page::Sensors => sensors_page(ui, width, snapshot, history),
                    Page::System => system_page(ui, width, snapshot),
                }
                ui.add_space(16.0);
            });
    }
}

/// Ligne de la colonne (ou du bandeau) : petit graphique, titre, nom et valeur.
fn device_entry(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    entry: &Entry,
    history: &History,
    selected: bool,
) -> egui::Response {
    let id = ui.scope_id().with(("appareil", format!("{:?}", entry.page)));
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected {
        style::selection_pill(ui, rect);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 8.0, style::surface_hover());
    }
    let spark = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 10.0, rect.center().y - 21.0),
        egui::vec2(58.0, 42.0),
    );
    let painter = ui.painter();
    if entry.series.is_empty() {
        // Système : pastille sans graphique.
        painter.rect_filled(spark, 8.0, SYSTEM_COLOR.gamma_multiply(0.18));
        painter.text(
            spark.center(),
            egui::Align2::CENTER_CENTER,
            "i",
            FontId::new(20.0, style::strong_family()),
            SYSTEM_COLOR,
        );
    } else {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(spark));
        let series: Vec<(&std::collections::VecDeque<f32>, Color32)> = entry
            .series
            .iter()
            .map(|(key, color)| (history.get(key), *color))
            .collect();
        graph(&mut child, spark.size(), &series, entry.max);
        child.painter().rect_stroke(
            spark,
            8.0,
            Stroke::new(1.0, style::separator()),
            egui::StrokeKind::Inside,
        );
    }
    let text = egui::Rect::from_min_max(
        egui::pos2(spark.right() + 10.0, rect.top() + 6.0),
        egui::pos2(rect.right() - 4.0, rect.bottom() - 6.0),
    );
    let third = text.height() / 3.0;
    let line = |index: usize| {
        egui::Rect::from_min_size(
            egui::pos2(text.left(), text.top() + third * index as f32),
            egui::vec2(text.width() + 14.0, third),
        )
    };
    let title_ink = if selected {
        style::accent_ink()
    } else {
        style::primary_text()
    };
    paint_cell(
        ui,
        line(0),
        &entry.title,
        false,
        title_ink,
        FontId::new(typography::LABEL, style::strong_family()),
    );
    paint_cell(
        ui,
        line(1),
        &entry.line,
        false,
        style::text_secondary(),
        FontId::proportional(typography::SMALL),
    );
    paint_cell(
        ui,
        line(2),
        &entry.detail,
        false,
        style::text_secondary(),
        FontId::proportional(typography::SMALL),
    );
    response
}

// ---------------------------------------------------------------------------------------
// Pièces communes des pages
// ---------------------------------------------------------------------------------------

/// Titre de page : nom de l'appareil, description, et valeur du moment à droite.
fn page_title(ui: &mut egui::Ui, width: f32, title: &str, subtitle: &str, value: Option<(String, Color32)>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 52.0), Sense::hover());
    let painter = ui.painter();
    let value_width = value.as_ref().map_or(0.0, |(text, color)| {
        painter
            .text(
                egui::pos2(rect.right(), rect.center().y),
                egui::Align2::RIGHT_CENTER,
                text,
                FontId::new(28.0, style::strong_family()),
                *color,
            )
            .width()
            + 16.0
    });
    let left = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right() - value_width, rect.bottom()));
    paint_cell(
        ui,
        egui::Rect::from_min_max(left.min, egui::pos2(left.right() + 14.0, left.center().y + 2.0)),
        title,
        false,
        style::heading_text(),
        FontId::new(typography::SECTION, style::strong_family()),
    );
    paint_cell(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(left.left(), left.center().y + 2.0),
            egui::pos2(left.right() + 14.0, left.bottom()),
        ),
        subtitle,
        false,
        style::text_secondary(),
        FontId::proportional(typography::LABEL),
    );
    ui.add_space(10.0);
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(18.0);
    ui.label(
        RichText::new(text)
            .size(typography::HEADING - 1.0)
            .family(style::strong_family())
            .color(style::heading_text()),
    );
    ui.add_space(8.0);
}

/// Grand graphique : légende au-dessus, échelle en haut à gauche, durée en dessous.
fn big_graph(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    legend: &[(Color32, &str, String)],
    series: &[(&std::collections::VecDeque<f32>, Color32)],
    max: Option<f32>,
    scale_text: impl Fn(f32) -> String,
) {
    if !legend.is_empty() {
        legend_row(ui, legend);
        ui.add_space(6.0);
    }
    let top = ui.cursor().min;
    let scale = graph(ui, egui::vec2(width, height), series, max);
    ui.painter().text(
        top + egui::vec2(8.0, 6.0),
        egui::Align2::LEFT_TOP,
        scale_text(scale),
        FontId::proportional(typography::CAPTION),
        style::text_secondary(),
    );
    let (caption, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), Sense::hover());
    for (align, text) in [
        (egui::Align2::LEFT_BOTTOM, "Il y a 60 s"),
        (egui::Align2::RIGHT_BOTTOM, "Maintenant"),
    ] {
        let x = if align == egui::Align2::LEFT_BOTTOM {
            caption.left()
        } else {
            caption.right()
        };
        ui.painter().text(
            egui::pos2(x, caption.bottom()),
            align,
            text,
            FontId::proportional(typography::CAPTION),
            style::tertiary_text(),
        );
    }
}

fn legend_row(ui: &mut egui::Ui, items: &[(Color32, &str, String)]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
        for (color, label, value) in items {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, *color);
            ui.label(
                RichText::new(*label)
                    .size(typography::SMALL)
                    .color(style::text_secondary()),
            );
            ui.label(
                RichText::new(value)
                    .size(typography::SMALL)
                    .family(style::strong_family()),
            );
            ui.add_space(12.0);
        }
    });
}

/// Grille de valeurs : autant de colonnes que la largeur le permet (150 px au moins).
fn stat_grid(ui: &mut egui::Ui, width: f32, stats: &[(&str, String)]) {
    if stats.is_empty() {
        return;
    }
    let columns = (((width + GAP) / (158.0 + GAP)).floor() as usize).clamp(1, 6);
    let tile_width = (width - GAP * (columns - 1) as f32) / columns as f32;
    let tile_height = 58.0;
    for row in stats.chunks(columns) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, tile_height), Sense::hover());
        for (index, (label, value)) in row.iter().enumerate() {
            let tile = egui::Rect::from_min_size(
                egui::pos2(rect.left() + index as f32 * (tile_width + GAP), rect.top()),
                egui::vec2(tile_width, tile_height),
            );
            ui.painter().rect_filled(tile, 10.0, style::content_gray());
            let inner = tile.shrink2(egui::vec2(12.0, 9.0));
            paint_cell(
                ui,
                egui::Rect::from_min_size(inner.min, egui::vec2(inner.width() + 14.0, 16.0)),
                label,
                false,
                style::text_secondary(),
                FontId::proportional(typography::SMALL),
            );
            paint_cell(
                ui,
                egui::Rect::from_min_max(
                    egui::pos2(inner.left(), inner.bottom() - 22.0),
                    egui::pos2(inner.right() + 14.0, inner.bottom()),
                ),
                value,
                false,
                style::primary_text(),
                FontId::new(typography::BODY + 1.0, style::strong_family()),
            );
            // Valeur longue (adresse IPv6, modèle) : en entier au survol.
            if value.chars().count() > 16 {
                ui.interact(tile, ui.scope_id().with(("valeur", *label)), Sense::hover())
                    .on_hover_text(value);
            }
        }
        ui.add_space(GAP);
    }
}

/// Lignes « nom : valeur » dans une carte bordée (page Système).
fn key_values(ui: &mut egui::Ui, width: f32, rows: &[(&str, String)]) {
    Frame::new()
        .fill(style::background())
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(12.0)
        .inner_margin(Margin::symmetric(16, 6))
        .show(ui, |ui| {
            ui.set_width(width - 32.0);
            let label_width = if width > 560.0 { 200.0 } else { 130.0 };
            for (index, (label, value)) in rows.iter().enumerate() {
                if index > 0 {
                    kit::separator(ui);
                }
                ui.horizontal(|ui| {
                    ui.set_min_height(36.0);
                    ui.add_sized(
                        egui::vec2(label_width, 20.0),
                        egui::Label::new(RichText::new(*label).color(style::text_secondary())).truncate(),
                    );
                    ui.add(egui::Label::new(RichText::new(value)).wrap());
                });
            }
        });
}

/// Note discrète (explication, donnée absente).
fn note(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text)
            .size(typography::SMALL)
            .color(style::text_secondary()),
    );
    ui.add_space(6.0);
}

/// Les processus qui consomment le plus de `value`.
fn top_processes(
    ui: &mut egui::Ui,
    width: f32,
    title: &str,
    processes: &[&Process],
    color: Color32,
    value: impl Fn(&Process) -> (f32, String),
) {
    if processes.is_empty() {
        return;
    }
    section_title(ui, title);
    Frame::new()
        .fill(style::background())
        .stroke(Stroke::new(1.0, style::separator()))
        .corner_radius(12.0)
        .inner_margin(Margin::symmetric(14, 6))
        .show(ui, |ui| {
            let inner = width - 28.0;
            ui.set_width(inner);
            for process in processes {
                let (fraction, text) = value(process);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(inner, 34.0), Sense::hover());
                let name_width = (inner * 0.42).max(120.0);
                paint_cell(
                    ui,
                    egui::Rect::from_min_size(rect.min, egui::vec2(name_width, rect.height())),
                    &process.name,
                    false,
                    style::primary_text(),
                    FontId::proportional(typography::LABEL),
                );
                let value_width = 92.0;
                let track = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + name_width, rect.center().y - 3.0),
                    egui::pos2(rect.right() - value_width, rect.center().y + 3.0),
                );
                if track.width() > 20.0 {
                    paint_bar(ui.painter(), track, fraction, color);
                }
                paint_cell(
                    ui,
                    egui::Rect::from_min_max(
                        egui::pos2(rect.right() - value_width, rect.top()),
                        rect.max + egui::vec2(14.0, 0.0),
                    ),
                    &text,
                    true,
                    style::primary_text(),
                    FontId::new(typography::LABEL, style::strong_family()),
                );
            }
        });
}

/// Couleur d'une température : succès, attention, puis erreur près du seuil critique.
fn heat_color(celsius: f32, high: Option<f32>, critical: Option<f32>) -> Color32 {
    let warn = high
        .or(critical.map(|value| value - 15.0))
        .unwrap_or(80.0)
        .min(90.0);
    let danger = critical.unwrap_or(warn + 10.0).min(100.0);
    if celsius >= danger - 5.0 {
        style::error()
    } else if celsius >= warn - 10.0 {
        style::warning()
    } else {
        style::success()
    }
}

// ---------------------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------------------

fn cpu_page(ui: &mut egui::Ui, width: f32, snapshot: &Snapshot, history: &History, per_core: &mut bool) {
    let cpu = &snapshot.info.cpu;
    let hybrid = cpu.performance_cores > 0 && cpu.efficient_cores > 0;
    page_title(
        ui,
        width,
        "Processeur",
        &cpu.model,
        Some((format_percent(snapshot.cpu), CPU_COLOR)),
    );

    // Choix de vue, à droite au-dessus du graphique.
    ui.horizontal(|ui| {
        ui.set_width(width);
        legend_row(
            ui,
            &[
                (CPU_COLOR, "Total", format_percent(snapshot.cpu)),
                (
                    CPU_COLOR.gamma_multiply(0.5),
                    "Noyau",
                    format_percent(snapshot.cpu_system),
                ),
            ],
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(index) = kit::segments(ui, &["Global", "Par cœur"], usize::from(*per_core)) {
                *per_core = index == 1;
            }
        });
    });
    ui.add_space(8.0);
    if *per_core {
        core_graphs(ui, width, snapshot, history);
    } else {
        big_graph(
            ui,
            width,
            190.0,
            &[],
            &[
                (history.get("cpu"), CPU_COLOR),
                (history.get("cpu-noyau"), CPU_COLOR.gamma_multiply(0.5)),
            ],
            Some(1.0),
            |_| "100 %".into(),
        );
    }

    ui.add_space(14.0);
    let threads: u32 = snapshot.processes.iter().map(|process| process.threads).sum();
    let mut stats: Vec<(&str, String)> = vec![("Utilisation", format_percent(snapshot.cpu))];
    if let Some(khz) = average_frequency(snapshot) {
        stats.push(("Vitesse moyenne", format_ghz(khz)));
    }
    if let Some(khz) = cpu.max_frequency {
        stats.push(("Vitesse maximale", format_ghz(khz)));
    }
    if let Some(celsius) = snapshot.cpu_temperature {
        stats.push(("Température", format_celsius(celsius)));
    }
    stats.push((
        "Cœurs",
        if hybrid {
            format!(
                "{} ({} P + {} E)",
                cpu.cores, cpu.performance_cores, cpu.efficient_cores
            )
        } else {
            cpu.cores.to_string()
        },
    ));
    stats.push(("Processeurs logiques", snapshot.cores.len().to_string()));
    if cpu.sockets > 1 {
        stats.push(("Supports", cpu.sockets.to_string()));
    }
    stats.push(("Processus", format_count(snapshot.processes.len() as u64)));
    stats.push(("Fils", format_count(u64::from(threads))));
    stats.push(("Descripteurs ouverts", format_count(snapshot.open_files)));
    stats.push((
        "Changements de contexte",
        format!("{} /s", format_count(snapshot.context_switches as u64)),
    ));
    stats.push((
        "Interruptions",
        format!("{} /s", format_count(snapshot.interrupts as u64)),
    ));
    stats.push((
        "Utilisateur · noyau · attente",
        format!(
            "{} · {} · {}",
            format_percent(snapshot.cpu_user),
            format_percent(snapshot.cpu_system),
            format_percent(snapshot.cpu_wait)
        ),
    ));
    stats.push((
        "Charge moyenne",
        format!(
            "{} · {} · {}",
            format_decimal(snapshot.load[0], 2),
            format_decimal(snapshot.load[1], 2),
            format_decimal(snapshot.load[2], 2)
        ),
    ));
    for (level, size) in &cpu.caches {
        stats.push((
            match level.as_str() {
                "L1" => "Cache L1",
                "L2" => "Cache L2",
                "L3" => "Cache L3",
                _ => "Cache",
            },
            format_bytes(*size),
        ));
    }
    if let Some(profile) = snapshot
        .energy_preference
        .as_deref()
        .map(energy_preference)
        .or_else(|| snapshot.governor.as_deref().map(governor))
    {
        stats.push(("Profil d'énergie", profile));
    }
    if let Some(virtualization) = cpu.virtualization {
        stats.push(("Virtualisation", virtualization.into()));
    }
    stats.push(("Allumé depuis", format_duration(snapshot.uptime)));
    stat_grid(ui, width, &stats);

    if !*per_core {
        section_title(ui, "Cœurs");
        if hybrid {
            legend_row(
                ui,
                &[
                    (CPU_COLOR, "P", "cœurs de performance".into()),
                    (CPU_EFFICIENT_COLOR, "E", "cœurs efficaces".into()),
                ],
            );
            ui.add_space(6.0);
        }
        core_tiles(ui, width, snapshot);
    }

    let mut busiest: Vec<&Process> = snapshot.processes.iter().collect();
    busiest.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    busiest.truncate(5);
    busiest.retain(|process| process.cpu > 0.0005);
    top_processes(ui, width, "Les plus gourmands", &busiest, CPU_COLOR, |process| {
        (
            process.cpu,
            format!("{} %", format_decimal(process.cpu * 100.0, 1)),
        )
    });
}

/// Nom et couleur d'un processeur logique (« P 0 », « E 16 », « 3 »).
fn core_label(snapshot: &Snapshot, index: usize) -> (String, Color32) {
    match snapshot.info.cpu.kinds.get(index) {
        Some(CoreKind::Performance) => (format!("P {index}"), CPU_COLOR),
        Some(CoreKind::Efficient) => (format!("E {index}"), CPU_EFFICIENT_COLOR),
        _ => (format!("Cœur {index}"), CPU_COLOR),
    }
}

/// Une tuile par processeur logique : niveau de charge, pourcentage et fréquence. La
/// grille prend autant de colonnes que la place permet (2 cœurs comme 128).
fn core_tiles(ui: &mut egui::Ui, width: f32, snapshot: &Snapshot) {
    let gap = 6.0;
    let columns = (((width + gap) / (96.0 + gap)).floor() as usize).max(2);
    let tile_width = (width - gap * (columns - 1) as f32) / columns as f32;
    let tile_height = 54.0;
    let indices: Vec<usize> = (0..snapshot.cores.len()).collect();
    for row in indices.chunks(columns) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, tile_height), Sense::hover());
        for (position, &index) in row.iter().enumerate() {
            let load = snapshot.cores[index];
            let tile = egui::Rect::from_min_size(
                egui::pos2(rect.left() + position as f32 * (tile_width + gap), rect.top()),
                egui::vec2(tile_width, tile_height),
            );
            let (label, color) = core_label(snapshot, index);
            let painter = ui.painter();
            painter.rect_filled(tile, 8.0, style::content_gray());
            // Niveau qui monte du bas de la tuile.
            let level = egui::Rect::from_min_max(
                egui::pos2(tile.left(), tile.bottom() - tile.height() * load.clamp(0.0, 1.0)),
                tile.max,
            );
            if level.height() > 0.5 {
                painter.rect_filled(
                    level,
                    egui::CornerRadius {
                        nw: if level.height() > tile.height() - 8.0 {
                            8
                        } else {
                            2
                        },
                        ne: if level.height() > tile.height() - 8.0 {
                            8
                        } else {
                            2
                        },
                        sw: 8,
                        se: 8,
                    },
                    color.gamma_multiply(0.30),
                );
            }
            painter.text(
                tile.left_top() + egui::vec2(9.0, 7.0),
                egui::Align2::LEFT_TOP,
                &label,
                FontId::proportional(typography::CAPTION),
                style::text_secondary(),
            );
            painter.text(
                tile.right_top() + egui::vec2(-9.0, 6.0),
                egui::Align2::RIGHT_TOP,
                format!("{:.0} %", load * 100.0),
                FontId::new(typography::LABEL, style::strong_family()),
                style::primary_text(),
            );
            if let Some(khz) = snapshot.frequencies.get(index).filter(|khz| **khz > 0) {
                painter.text(
                    tile.left_bottom() + egui::vec2(9.0, -7.0),
                    egui::Align2::LEFT_BOTTOM,
                    format_ghz(*khz),
                    FontId::proportional(typography::CAPTION),
                    style::text_secondary(),
                );
            }
        }
        ui.add_space(gap);
    }
}

/// Un petit graphique par processeur logique.
fn core_graphs(ui: &mut egui::Ui, width: f32, snapshot: &Snapshot, history: &History) {
    let gap = 8.0;
    let columns = (((width + gap) / (124.0 + gap)).floor() as usize).max(2);
    let tile_width = (width - gap * (columns - 1) as f32) / columns as f32;
    let indices: Vec<usize> = (0..snapshot.cores.len()).collect();
    for row in indices.chunks(columns) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(gap, 2.0);
            for &index in row {
                ui.vertical(|ui| {
                    ui.set_width(tile_width);
                    let (label, color) = core_label(snapshot, index);
                    let (line, _) = ui.allocate_exact_size(egui::vec2(tile_width, 18.0), Sense::hover());
                    ui.painter().text(
                        line.left_center(),
                        egui::Align2::LEFT_CENTER,
                        label,
                        FontId::proportional(typography::CAPTION),
                        style::text_secondary(),
                    );
                    ui.painter().text(
                        line.right_center(),
                        egui::Align2::RIGHT_CENTER,
                        format!("{:.0} %", snapshot.cores[index] * 100.0),
                        FontId::new(typography::CAPTION, style::strong_family()),
                        style::primary_text(),
                    );
                    graph(
                        ui,
                        egui::vec2(tile_width, 58.0),
                        &[(history.get(&format!("cpu/{index}")), color)],
                        Some(1.0),
                    );
                });
            }
        });
        ui.add_space(gap);
    }
}

fn governor(name: &str) -> String {
    match name {
        "powersave" => "Économie".into(),
        "performance" => "Performance".into(),
        "schedutil" => "Adaptatif".into(),
        "ondemand" => "À la demande".into(),
        "conservative" => "Prudent".into(),
        other => other.into(),
    }
}

fn energy_preference(name: &str) -> String {
    match name {
        "performance" => "Performance".into(),
        "balance_performance" => "Équilibré, côté performance".into(),
        "balance_power" => "Équilibré, côté économie".into(),
        "power" => "Économie".into(),
        "default" => "Par défaut".into(),
        other => other.into(),
    }
}

fn memory_page(ui: &mut egui::Ui, width: f32, snapshot: &Snapshot, history: &History) {
    let memory = &snapshot.memory;
    let fraction = memory.used as f32 / memory.total.max(1) as f32;
    page_title(
        ui,
        width,
        "Mémoire",
        &format!("{} installés", format_bytes(memory.total)),
        Some((format_percent(fraction), MEMORY_COLOR)),
    );
    let swap = MEMORY_COLOR.gamma_multiply(0.45);
    let mut legend = vec![(MEMORY_COLOR, "Utilisée", format_bytes(memory.used))];
    let mut series = vec![(history.get("memoire"), MEMORY_COLOR)];
    if memory.swap_total > 0 {
        legend.push((
            swap,
            "Échange",
            format_percent(memory.swap_used as f32 / memory.swap_total as f32),
        ));
        series.push((history.get("echange"), swap));
    }
    big_graph(ui, width, 170.0, &legend, &series, Some(1.0), |_| "100 %".into());

    // Composition : utilisée, cache (rendu aux applications au besoin), libre.
    section_title(ui, "Composition");
    let total = memory.total.max(1) as f32;
    let cache = (memory.cached + memory.buffers).min(memory.total.saturating_sub(memory.used));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 16.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, style::track());
    let used_width = rect.width() * memory.used as f32 / total;
    let cache_width = rect.width() * cache as f32 / total;
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(used_width + cache_width, rect.height())),
        8.0,
        MEMORY_COLOR.gamma_multiply(0.35),
    );
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(used_width, rect.height())),
        8.0,
        MEMORY_COLOR,
    );
    ui.add_space(8.0);
    legend_row(
        ui,
        &[
            (MEMORY_COLOR, "Utilisée", format_bytes(memory.used)),
            (MEMORY_COLOR.gamma_multiply(0.35), "En cache", format_bytes(cache)),
            (style::track(), "Libre", format_bytes(memory.free)),
        ],
    );
    ui.add_space(12.0);

    let mut stats: Vec<(&str, String)> = vec![
        ("Utilisée", format_bytes(memory.used)),
        ("Disponible", format_bytes(memory.available)),
        ("En cache", format_bytes(memory.cached)),
        ("Tampons", format_bytes(memory.buffers)),
        ("Partagée", format_bytes(memory.shared)),
        ("En attente d'écriture", format_bytes(memory.dirty)),
        ("Libre", format_bytes(memory.free)),
        (
            "Échange",
            if memory.swap_total > 0 {
                format!(
                    "{} sur {}",
                    format_bytes(memory.swap_used),
                    format_bytes(memory.swap_total)
                )
            } else {
                "Aucun".into()
            },
        ),
    ];
    if let Some((original, compressed)) = memory.zram {
        stats.push((
            "zram (place occupée)",
            format!(
                "{} (× {})",
                format_bytes(compressed),
                format_decimal(original as f32 / compressed.max(1) as f32, 1)
            ),
        ));
    }
    stat_grid(ui, width, &stats);

    let mut biggest: Vec<&Process> = snapshot.processes.iter().collect();
    biggest.sort_by_key(|process| std::cmp::Reverse(process.memory));
    biggest.truncate(6);
    top_processes(
        ui,
        width,
        "Les plus gourmands",
        &biggest,
        MEMORY_COLOR,
        |process| (process.memory as f32 / total, format_bytes(process.memory)),
    );
}

fn gpu_page(ui: &mut egui::Ui, width: f32, snapshot: &Snapshot, gpu: &Gpu, history: &History) {
    page_title(
        ui,
        width,
        &gpu.name,
        &format!(
            "{} · pilote {}",
            if gpu.integrated {
                "Carte intégrée"
            } else {
                "Carte dédiée"
            },
            if gpu.driver.is_empty() {
                "absent"
            } else {
                &gpu.driver
            }
        ),
        if gpu.asleep {
            Some(("En veille".into(), style::text_secondary()))
        } else {
            gpu.usage.map(|usage| (format_percent(usage), GPU_COLOR))
        },
    );
    if gpu.asleep {
        Frame::new()
            .fill(style::accent_soft())
            .corner_radius(12.0)
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(width - 28.0);
                ui.label(
                    RichText::new("La carte dort pour économiser l'énergie")
                        .family(style::strong_family())
                        .color(style::accent_ink()),
                );
                ui.label(
                    RichText::new(
                        "Elle se réveille dès qu'un jeu ou une application lui confie du travail. \
                         Le moniteur ne la réveille pas pour la mesurer.",
                    )
                    .size(typography::SMALL)
                    .color(style::primary_text()),
                );
            });
        ui.add_space(12.0);
    }
    let key = format!("gpu/{}", gpu.slot);
    if gpu.usage.is_some() || gpu.asleep {
        big_graph(
            ui,
            width,
            170.0,
            &[(
                GPU_COLOR,
                "Activité",
                gpu.usage.map(format_percent).unwrap_or_else(|| "0 %".into()),
            )],
            &[(history.get(&key), GPU_COLOR)],
            Some(1.0),
            |_| "100 %".into(),
        );
    } else if gpu.missing_tool {
        note(
            ui,
            "Installez le pilote propriétaire NVIDIA (il fournit nvidia-smi) pour suivre l'activité, \
             la mémoire vidéo et la température de cette carte.",
        );
    } else {
        note(ui, "Ce pilote ne donne pas l'activité de la carte.");
    }
    if gpu.usage_from_clients {
        note(
            ui,
            "Activité mesurée d'après vos applications (le pilote ne la donne pas pour toute la carte).",
        );
    }
    if let (Some(used), Some(total)) = (gpu.memory_used, gpu.memory_total) {
        section_title(ui, "Mémoire vidéo");
        big_graph(
            ui,
            width,
            110.0,
            &[(
                GPU_COLOR.gamma_multiply(0.6),
                "Utilisée",
                format!("{} sur {}", format_bytes(used), format_bytes(total)),
            )],
            &[(
                history.get(&format!("vram/{}", gpu.slot)),
                GPU_COLOR.gamma_multiply(0.6),
            )],
            Some(1.0),
            |_| format_bytes(total),
        );
    }
    ui.add_space(14.0);

    let mut stats: Vec<(&str, String)> = Vec::new();
    if let Some(usage) = gpu.usage {
        stats.push(("Activité", format_percent(usage)));
    }
    match (gpu.memory_used, gpu.memory_total) {
        (Some(used), Some(total)) => stats.push((
            "Mémoire vidéo",
            format!("{} sur {}", format_bytes(used), format_bytes(total)),
        )),
        (Some(used), None) => stats.push(("Mémoire partagée utilisée", format_bytes(used))),
        _ => {}
    }
    if let Some(celsius) = gpu.temperature {
        stats.push(("Température", format_celsius(celsius)));
    }
    match (gpu.power, gpu.power_limit) {
        (Some(power), Some(limit)) => stats.push((
            "Consommation",
            format!(
                "{} W sur {} W",
                format_decimal(power, 1),
                format_decimal(limit, 0)
            ),
        )),
        (Some(power), None) => stats.push(("Consommation", format!("{} W", format_decimal(power, 1)))),
        _ => {}
    }
    match (gpu.clock, gpu.clock_max) {
        // Carte Intel au repos (veille RC6) : elle ne tourne pas du tout.
        (Some(0), _) => stats.push(("Fréquence", "Au repos".into())),
        (Some(clock), Some(max)) => stats.push((
            "Fréquence",
            format!(
                "{} MHz sur {}",
                format_count(u64::from(clock)),
                format_count(u64::from(max))
            ),
        )),
        (Some(clock), None) => stats.push(("Fréquence", format!("{} MHz", format_count(u64::from(clock))))),
        _ => {}
    }
    match (gpu.fan, gpu.fan_rpm) {
        (Some(fan), Some(rpm)) => stats.push((
            "Ventilateur",
            format!(
                "{} · {} tr/min",
                format_percent(fan),
                format_count(u64::from(rpm))
            ),
        )),
        (Some(fan), None) => stats.push(("Ventilateur", format_percent(fan))),
        (None, Some(rpm)) => stats.push(("Ventilateur", format!("{} tr/min", format_count(u64::from(rpm))))),
        _ => {}
    }
    stats.push((
        "Pilote",
        match &gpu.driver_version {
            Some(version) => format!("{} {version}", gpu.driver),
            None if gpu.driver.is_empty() => "Aucun".into(),
            None => gpu.driver.clone(),
        },
    ));
    stats.push(("Type", if gpu.integrated { "Intégrée" } else { "Dédiée" }.into()));
    stats.push(("Emplacement PCI", gpu.slot.clone()));
    stat_grid(ui, width, &stats);

    let mut users: Vec<&Process> = snapshot
        .processes
        .iter()
        .filter(|process| process.gpu_slot == gpu.slot && (process.gpu > 0.0 || process.gpu_memory > 0))
        .collect();
    users.sort_by(|a, b| b.gpu.total_cmp(&a.gpu).then(b.gpu_memory.cmp(&a.gpu_memory)));
    users.truncate(8);
    top_processes(
        ui,
        width,
        "Applications sur cette carte",
        &users,
        GPU_COLOR,
        |process| {
            (
                process.gpu,
                if process.gpu > 0.0005 {
                    format!("{} %", format_decimal(process.gpu * 100.0, 1))
                } else {
                    format_bytes(process.gpu_memory)
                },
            )
        },
    );
}

fn disk_page(ui: &mut egui::Ui, width: f32, disk: &Disk, history: &History) {
    let info = &disk.info;
    page_title(
        ui,
        width,
        &info.model,
        &format!("{} · {} · {}", info.kind, format_bytes(info.size), info.name),
        Some((format_percent(disk.busy), READ_COLOR)),
    );
    let name = &info.name;
    big_graph(
        ui,
        width,
        120.0,
        &[(READ_COLOR, "Activité", format_percent(disk.busy))],
        &[(history.get(&format!("activite/{name}")), READ_COLOR)],
        Some(1.0),
        |_| "100 %".into(),
    );
    section_title(ui, "Débit");
    big_graph(
        ui,
        width,
        120.0,
        &[
            (READ_COLOR, "Lecture", format_rate(disk.read)),
            (WRITE_COLOR, "Écriture", format_rate(disk.write)),
        ],
        &[
            (history.get(&format!("lecture/{name}")), READ_COLOR),
            (history.get(&format!("ecriture/{name}")), WRITE_COLOR),
        ],
        None,
        |scale| format_rate(f64::from(scale)),
    );
    ui.add_space(14.0);
    let mut stats: Vec<(&str, String)> = vec![
        ("Activité", format_percent(disk.busy)),
        ("Lecture", format_rate(disk.read)),
        ("Écriture", format_rate(disk.write)),
        (
            "Temps de réponse",
            disk.latency
                .map(|ms| format!("{} ms", format_decimal(ms, 1)))
                .unwrap_or_else(|| "—".into()),
        ),
        ("Lu depuis le démarrage", format_bytes(disk.read_total)),
        ("Écrit depuis le démarrage", format_bytes(disk.write_total)),
        ("Capacité", format_bytes(info.size)),
        ("Type", info.kind.into()),
    ];
    if let Some(celsius) = disk.temperature {
        stats.push(("Température", format_celsius(celsius)));
    }
    if info.removable {
        stats.push(("Amovible", "Oui".into()));
    }
    stat_grid(ui, width, &stats);

    section_title(ui, "Partitions montées");
    if disk.volumes.is_empty() {
        note(ui, "Aucune partition de ce disque n'est montée.");
        return;
    }
    for volume in &disk.volumes {
        let fraction = volume.used as f32 / volume.size.max(1) as f32;
        Frame::new()
            .fill(style::background())
            .stroke(Stroke::new(1.0, style::separator()))
            .corner_radius(12.0)
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(width - 28.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&volume.mount).family(style::strong_family()));
                    ui.label(
                        RichText::new(format!("{} · {}", volume.device, volume.fstype))
                            .size(typography::SMALL)
                            .color(style::text_secondary()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} libres sur {}",
                                format_bytes(volume.size.saturating_sub(volume.used)),
                                format_bytes(volume.size)
                            ))
                            .size(typography::SMALL),
                        );
                    });
                });
                let color = if fraction > 0.9 {
                    style::error()
                } else if fraction > 0.75 {
                    style::warning()
                } else {
                    READ_COLOR
                };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width - 28.0, 8.0), Sense::hover());
                paint_bar(ui.painter(), rect, fraction, color);
            });
        ui.add_space(8.0);
    }
}

fn network_page(ui: &mut egui::Ui, width: f32, interface: &Interface, history: &History) {
    let details = &interface.details;
    let mut subtitle = interface.name.clone();
    if let Some(network) = &interface.network {
        subtitle = format!("{network} · {subtitle}");
    }
    page_title(
        ui,
        width,
        interface.kind.name(),
        &subtitle,
        Some((format!("↓ {}", format_rate(interface.down)), DOWN_COLOR)),
    );
    let name = &interface.name;
    big_graph(
        ui,
        width,
        190.0,
        &[
            (DOWN_COLOR, "Réception", format_rate(interface.down)),
            (UP_COLOR, "Envoi", format_rate(interface.up)),
        ],
        &[
            (history.get(&format!("reception/{name}")), DOWN_COLOR),
            (history.get(&format!("envoi/{name}")), UP_COLOR),
        ],
        None,
        |scale| format_rate(f64::from(scale)),
    );
    ui.add_space(14.0);
    let mut stats: Vec<(&str, String)> = vec![
        ("Réception", format_rate(interface.down)),
        ("Envoi", format_rate(interface.up)),
        ("Reçu depuis le démarrage", format_bytes(interface.received)),
        ("Envoyé depuis le démarrage", format_bytes(interface.sent)),
        (
            "État",
            if details.up || interface.kind == NetKind::Virtual {
                "Connecté"
            } else {
                "Déconnecté"
            }
            .into(),
        ),
    ];
    if let Some(network) = &interface.network {
        stats.push(("Réseau", network.clone()));
    }
    if let Some((quality, dbm)) = details.signal {
        stats.push(("Signal", format!("{} ({dbm} dBm)", format_percent(quality))));
    }
    if let Some(speed) = details.speed {
        stats.push((
            "Débit du lien",
            if speed >= 1000 {
                format!("{} Gb/s", format_decimal(speed as f32 / 1000.0, 1))
            } else {
                format!("{speed} Mb/s")
            },
        ));
    }
    if let Some(address) = interface.ipv4.first() {
        stats.push(("Adresse IPv4", address.clone()));
    }
    if let Some(address) = interface.ipv6.first() {
        stats.push(("Adresse IPv6", address.clone()));
    }
    if let Some(mac) = &details.mac {
        stats.push(("Adresse matérielle", mac.to_uppercase()));
    }
    stat_grid(ui, width, &stats);
}

fn battery_page(ui: &mut egui::Ui, width: f32, snapshot: &Snapshot, history: &History) {
    let Some(battery) = snapshot.battery() else {
        return;
    };
    page_title(
        ui,
        width,
        "Batterie",
        battery.model.as_deref().unwrap_or(&battery.status),
        Some((format_percent(battery.level), battery_color(battery))),
    );
    // Jauge : la charge, en couleur d'alerte quand elle est basse ; l'état au-dessus.
    let color = battery_color(battery);
    let mut label = battery.status.clone();
    if let Some(seconds) = battery.time_left {
        label.push_str(&format!(
            " · {} {}",
            format_duration(seconds),
            if battery.charging {
                "avant la charge complète"
            } else {
                "restantes"
            }
        ));
    }
    ui.label(
        RichText::new(label)
            .size(typography::LABEL)
            .family(style::strong_family()),
    );
    ui.add_space(8.0);
    let gauge_width = width.min(520.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(gauge_width, 28.0), Sense::hover());
    let body = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right() - 7.0, rect.bottom()));
    let painter = ui.painter();
    painter.rect_filled(body, 9.0, style::track());
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(body.right() + 4.0, body.center().y),
            egui::vec2(4.0, 12.0),
        ),
        2.0,
        style::track(),
    );
    let fill = egui::Rect::from_min_size(
        body.min,
        egui::vec2((body.width() * battery.level).max(12.0), body.height()),
    );
    painter.rect_filled(fill.shrink(3.0), 6.0, color);
    ui.add_space(16.0);

    let peak = history.get("puissance").iter().copied().fold(0.0_f32, f32::max);
    let scale = (peak * 1.25).max(10.0).ceil();
    big_graph(
        ui,
        width,
        140.0,
        &[(
            BATTERY_COLOR,
            if battery.charging {
                "Charge"
            } else {
                "Consommation"
            },
            battery
                .power
                .map(|watts| format!("{} W", format_decimal(watts, 1)))
                .unwrap_or_else(|| "—".into()),
        )],
        &[(history.get("puissance"), BATTERY_COLOR)],
        Some(scale),
        |scale| format!("{scale:.0} W"),
    );
    ui.add_space(14.0);

    for (index, battery) in snapshot.power.batteries.iter().enumerate() {
        if index > 0 {
            section_title(ui, &format!("Batterie {}", battery.name));
        }
        let mut stats: Vec<(&str, String)> = vec![
            ("Charge", format_percent(battery.level)),
            ("État", battery.status.clone()),
        ];
        if let Some(watts) = battery.power {
            stats.push((
                if battery.charging {
                    "Puissance de charge"
                } else {
                    "Consommation"
                },
                format!("{} W", format_decimal(watts, 1)),
            ));
        }
        if let Some(seconds) = battery.time_left {
            stats.push((
                if battery.charging {
                    "Charge complète dans"
                } else {
                    "Autonomie restante"
                },
                format_duration(seconds),
            ));
        }
        if let Some(health) = battery.health() {
            stats.push(("Santé", format_percent(health)));
        }
        match (battery.energy_full, battery.energy_design) {
            (Some(full), Some(design)) => stats.push((
                "Capacité",
                format!(
                    "{} Wh sur {} Wh",
                    format_decimal(full, 1),
                    format_decimal(design, 1)
                ),
            )),
            (Some(full), None) => stats.push(("Capacité", format!("{} Wh", format_decimal(full, 1)))),
            _ => {}
        }
        if let Some(cycles) = battery.cycles {
            stats.push(("Cycles de charge", format_count(u64::from(cycles))));
        }
        if let Some(voltage) = battery.voltage {
            stats.push(("Tension", format!("{} V", format_decimal(voltage, 2))));
        }
        if let Some(technology) = &battery.technology {
            stats.push(("Technologie", technology.clone()));
        }
        if let Some(model) = &battery.model {
            stats.push(("Modèle", model.clone()));
        }
        if index == 0
            && let Some(mains) = snapshot.power.on_mains
        {
            stats.push(("Secteur", if mains { "Branché" } else { "Débranché" }.into()));
        }
        stat_grid(ui, width, &stats);
    }
}

/// Vert, puis attention sous 20 %, erreur sous 10 % (sauf en charge).
fn battery_color(battery: &super::hardware::Battery) -> Color32 {
    if battery.charging || battery.level > 0.2 {
        BATTERY_COLOR
    } else if battery.level > 0.1 {
        style::warning()
    } else {
        style::error()
    }
}

fn sensors_page(ui: &mut egui::Ui, width: f32, snapshot: &Snapshot, history: &History) {
    let hottest = snapshot
        .sensors
        .iter()
        .flat_map(|group| group.temperatures.iter())
        .map(|temp| temp.celsius)
        .fold(None, |max: Option<f32>, value| {
            Some(max.map_or(value, |max| max.max(value)))
        });
    page_title(
        ui,
        width,
        "Capteurs",
        "Températures et ventilateurs",
        hottest.map(|celsius| (format_celsius(celsius), SENSOR_COLOR)),
    );
    if let Some(celsius) = snapshot.cpu_temperature {
        big_graph(
            ui,
            width,
            120.0,
            &[(SENSOR_COLOR, "Processeur", format_celsius(celsius))],
            &[(history.get("temperature"), SENSOR_COLOR)],
            Some(100.0),
            |_| "100 °C".into(),
        );
    }
    if snapshot.sensors.is_empty() {
        note(
            ui,
            "Aucun capteur lisible sur cette machine (c'est courant dans une machine virtuelle).",
        );
        return;
    }
    for group in &snapshot.sensors {
        section_title(ui, &group.name);
        Frame::new()
            .fill(style::background())
            .stroke(Stroke::new(1.0, style::separator()))
            .corner_radius(12.0)
            .inner_margin(Margin::symmetric(14, 4))
            .show(ui, |ui| {
                let inner = width - 28.0;
                ui.set_width(inner);
                let label_width = (inner * 0.36).clamp(110.0, 240.0);
                for temp in &group.temperatures {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(inner, 32.0), Sense::hover());
                    paint_cell(
                        ui,
                        egui::Rect::from_min_size(rect.min, egui::vec2(label_width, rect.height())),
                        &temp.label,
                        false,
                        style::primary_text(),
                        FontId::proportional(typography::LABEL),
                    );
                    let color = heat_color(temp.celsius, temp.high, temp.critical);
                    let ceiling = temp.critical.unwrap_or(100.0).max(temp.celsius);
                    let track = egui::Rect::from_min_max(
                        egui::pos2(rect.left() + label_width, rect.center().y - 3.0),
                        egui::pos2(rect.right() - 80.0, rect.center().y + 3.0),
                    );
                    if track.width() > 20.0 {
                        paint_bar(ui.painter(), track, temp.celsius / ceiling, color);
                    }
                    ui.painter().text(
                        rect.right_center(),
                        egui::Align2::RIGHT_CENTER,
                        format_celsius(temp.celsius),
                        FontId::new(typography::LABEL, style::strong_family()),
                        color,
                    );
                }
                for fan in &group.fans {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(inner, 32.0), Sense::hover());
                    paint_cell(
                        ui,
                        egui::Rect::from_min_size(rect.min, egui::vec2(label_width, rect.height())),
                        &fan.label,
                        false,
                        style::primary_text(),
                        FontId::proportional(typography::LABEL),
                    );
                    ui.painter().text(
                        rect.right_center(),
                        egui::Align2::RIGHT_CENTER,
                        if fan.rpm == 0 {
                            "À l'arrêt".to_owned()
                        } else {
                            format!("{} tr/min", format_count(u64::from(fan.rpm)))
                        },
                        FontId::new(typography::LABEL, style::strong_family()),
                        style::primary_text(),
                    );
                }
            });
    }
}

fn system_page(ui: &mut egui::Ui, width: f32, snapshot: &Snapshot) {
    let info = &snapshot.info;
    page_title(ui, width, "Système", &info.hostname, None);
    let cpu = &info.cpu;
    let mut rows: Vec<(&str, String)> = Vec::new();
    if let Some(machine) = &info.machine {
        rows.push(("Ordinateur", machine.clone()));
    }
    rows.push(("Système", info.os.clone()));
    rows.push(("Bureau", "Ferrite (Wayland)".into()));
    rows.push(("Noyau", format!("Linux {}", info.kernel)));
    rows.push(("Nom de l'ordinateur", info.hostname.clone()));
    rows.push(("Allumé depuis", format_duration(snapshot.uptime)));
    rows.push((
        "Processeur",
        format!(
            "{} — {} cœurs, {} processeurs logiques",
            cpu.model,
            cpu.cores,
            snapshot.cores.len()
        ),
    ));
    rows.push(("Mémoire", format_bytes(snapshot.memory.total)));
    for gpu in &snapshot.gpus {
        rows.push((
            "Carte graphique",
            match gpu.memory_total {
                Some(total) if !gpu.integrated => format!("{} ({})", gpu.name, format_bytes(total)),
                _ => gpu.name.clone(),
            },
        ));
    }
    for disk in &snapshot.disks {
        rows.push((
            "Disque",
            format!(
                "{} — {}, {}",
                disk.info.model,
                disk.info.kind,
                format_bytes(disk.info.size)
            ),
        ));
    }
    key_values(ui, width, &rows);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::hardware::{Battery, Power};

    #[test]
    fn appareils_listes_selon_la_machine() {
        let mut snapshot = Snapshot::default();
        let pages = |snapshot: &Snapshot| -> Vec<Page> {
            entries(snapshot).into_iter().map(|entry| entry.page).collect()
        };
        // Machine virtuelle : ni carte, ni batterie, ni capteur.
        assert_eq!(pages(&snapshot), [Page::Cpu, Page::Memory, Page::System]);
        snapshot.gpus = vec![
            Gpu {
                slot: "0000:00:02.0".into(),
                integrated: true,
                ..Gpu::default()
            },
            Gpu {
                slot: "0000:01:00.0".into(),
                asleep: true,
                ..Gpu::default()
            },
        ];
        snapshot.power = Power {
            batteries: vec![Battery::default()],
            on_mains: Some(true),
        };
        let list = entries(&snapshot);
        assert_eq!(list[2].title, "Carte graphique 0");
        assert_eq!(list[3].detail, "En veille");
        assert!(list.iter().any(|entry| entry.page == Page::Battery));
        // « gpu » sans adresse : la première carte ; un disque disparu : le processeur.
        assert_eq!(
            resolve(&Page::Gpu(String::new()), &list),
            Page::Gpu("0000:00:02.0".into())
        );
        assert_eq!(resolve(&Page::Disk("sdz".into()), &list), Page::Cpu);
    }

    #[test]
    fn couleur_des_temperatures() {
        assert_eq!(heat_color(45.0, None, Some(100.0)), style::success());
        assert_eq!(heat_color(97.0, None, Some(100.0)), style::error());
        assert_eq!(heat_color(78.0, None, None), style::warning());
    }
}
