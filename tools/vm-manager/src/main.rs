//! Ferrite VM : démarrer et tester l'ISO Ferrite dans QEMU sans ligne de commande.
//!
//! L'outil s'appuie sur les scripts du dépôt (`tools/vm.sh` pour la machine virtuelle
//! avec fenêtre, `tools/vm-test.sh` pour les scénarios) : la configuration de QEMU reste
//! à un seul endroit. Il parle au moniteur QEMU par une socket Unix pour l'arrêt, le
//! redémarrage, les touches spéciales et les captures d'écran.
//!
//! En mode développement, la machine virtuelle utilise les binaires de `target/release`
//! (partage 9p) : « Recompiler et relancer le bureau » suffit après une modification,
//! sans reconstruire l'ISO.

use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime},
};

use eframe::egui::{self, Color32, RichText};

const MONITOR: &str = "/tmp/ferrite-vm-gui.sock";
const ACCENT: Color32 = Color32::from_rgb(0x36, 0x89, 0xE6);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_app_id("ferrite-vm")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([760.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Ferrite VM",
        options,
        Box::new(|context| Ok(Box::new(VmManager::new(context)))),
    )
}

/// Processus suivi (machine virtuelle ou scénario), avec sa sortie au fil de l'eau.
struct Job {
    child: Child,
    title: String,
}

struct VmManager {
    repo: PathBuf,
    iso: String,
    memory: u8,
    cpus: u8,
    network: bool,
    sound: bool,
    dev: bool,
    vm: Option<Job>,
    scenario: Option<Job>,
    build: Option<Job>,
    /// Relancer le bureau de la machine virtuelle à la fin de la compilation.
    relaunch_after_build: bool,
    log: Arc<Mutex<Vec<String>>>,
    capture: Option<(PathBuf, SystemTime, egui::TextureHandle)>,
    message: String,
}

impl VmManager {
    fn new(context: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(inter) = fs::read(repo().join("assets/fonts/Inter-Regular.ttf")) {
            fonts
                .font_data
                .insert("inter".into(), Arc::new(egui::FontData::from_owned(inter)));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "inter".into());
        }
        context.egui_ctx.set_fonts(fonts);
        context.egui_ctx.set_theme(egui::Theme::Light);
        context.egui_ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 6.0);
            style.visuals.selection.bg_fill = ACCENT;
        });
        let repo = repo();
        Self {
            iso: repo.join("build/ferrite-live.iso").display().to_string(),
            repo,
            memory: 4,
            cpus: 4,
            network: true,
            sound: false,
            dev: true,
            vm: None,
            scenario: None,
            build: None,
            relaunch_after_build: false,
            log: Arc::new(Mutex::new(Vec::new())),
            capture: None,
            message: String::new(),
        }
    }

    fn push_log(&self, line: impl Into<String>) {
        if let Ok(mut log) = self.log.lock() {
            log.push(line.into());
            let excess = log.len().saturating_sub(400);
            log.drain(..excess);
        }
    }

    /// Lance un script du dépôt et recopie sa sortie dans le journal.
    fn spawn(&mut self, mut command: Command, title: &str) -> Option<Job> {
        command
            .current_dir(&self.repo)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match command.spawn() {
            Ok(mut child) => {
                for pipe in [
                    child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
                    child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
                ]
                .into_iter()
                .flatten()
                {
                    let log = self.log.clone();
                    thread::spawn(move || {
                        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                            if let Ok(mut log) = log.lock() {
                                log.push(strip_ansi(&line));
                            }
                        }
                    });
                }
                self.push_log(format!("▶ {title}"));
                Some(Job {
                    child,
                    title: title.to_owned(),
                })
            }
            Err(error) => {
                self.message = format!("Lancement impossible : {error}");
                None
            }
        }
    }

    fn start_vm(&mut self) {
        if !Path::new(&self.iso).is_file() {
            self.message = "ISO introuvable : construis-la avec tools/usb.sh".into();
            return;
        }
        let _ = fs::remove_file(MONITOR);
        let mut command = Command::new(self.repo.join("tools/vm.sh"));
        command
            .arg(&self.iso)
            .env("FERRITE_VM_MEMORY", self.memory.to_string())
            .env("FERRITE_VM_CPUS", self.cpus.to_string())
            .env("FERRITE_VM_NETWORK", if self.network { "1" } else { "0" })
            .env("FERRITE_VM_SOUND", if self.sound { "1" } else { "0" })
            .env("FERRITE_VM_MONITOR", MONITOR)
            .env("FERRITE_VM_DEV", if self.dev { "1" } else { "0" });
        self.vm = self.spawn(command, "Machine virtuelle");
        self.message.clear();
    }

    /// Compile le bureau et les applications en mode release (binaires partagés).
    fn start_build(&mut self, relaunch: bool) {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(". tools/lib.sh && cargo build --release -p ferrite-shell -p ferrite-apps");
        self.build = self.spawn(command, "Compilation");
        self.relaunch_after_build = relaunch;
    }

    /// Envoie une commande au moniteur QEMU et renvoie sa réponse.
    fn monitor(&mut self, command: &str) -> Option<String> {
        let mut stream = match UnixStream::connect(MONITOR) {
            Ok(stream) => stream,
            Err(error) => {
                self.message = format!("Moniteur QEMU injoignable : {error}");
                return None;
            }
        };
        let _ = stream.set_read_timeout(Some(Duration::from_millis(400)));
        let _ = writeln!(stream, "{command}");
        thread::sleep(Duration::from_millis(250));
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        Some(answer)
    }

    fn take_capture(&mut self) {
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |time| time.as_secs());
        let path = self.repo.join(format!("build/vm-capture-{stamp}.png"));
        if self
            .monitor(&format!("screendump {} -f png", path.display()))
            .is_some()
        {
            thread::sleep(Duration::from_millis(500));
            self.message = format!("Capture : {}", path.display());
        }
    }

    /// Montre la capture PNG la plus récente du dossier `build`.
    fn refresh_capture(&mut self, ctx: &egui::Context) {
        let newest = fs::read_dir(self.repo.join("build"))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
            .filter_map(|path| Some((fs::metadata(&path).ok()?.modified().ok()?, path)))
            .max_by_key(|(modified, _)| *modified);
        let Some((modified, path)) = newest else { return };
        if self
            .capture
            .as_ref()
            .is_some_and(|(known, time, _)| *known == path && *time == modified)
        {
            return;
        }
        if let Some(image) = load_png(&path) {
            let texture = ctx.load_texture("capture", image, egui::TextureOptions::LINEAR);
            self.capture = Some((path, modified, texture));
        }
    }

    fn scenarios(&self) -> Vec<PathBuf> {
        let mut list: Vec<PathBuf> = fs::read_dir(self.repo.join("tools/scenarios"))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"))
            .collect();
        list.sort();
        list
    }

    /// Met à jour l'état des processus terminés.
    fn reap(&mut self) {
        if let Some(job) = &mut self.build
            && let Ok(Some(status)) = job.child.try_wait()
        {
            self.build = None;
            if status.success() {
                self.push_log("■ Compilation réussie");
                if self.relaunch_after_build && self.vm.is_some() {
                    // Quitter le bureau : en mode développement, la session le relance
                    // aussitôt avec les nouveaux binaires.
                    self.monitor("sendkey ctrl-alt-backspace");
                    self.message = "Bureau relancé avec la nouvelle compilation".into();
                }
            } else {
                self.push_log(format!("■ Compilation échouée ({status})"));
                self.message = "La compilation a échoué : voir le journal".into();
            }
        }
        for slot in [&mut self.vm, &mut self.scenario] {
            if let Some(job) = slot
                && let Ok(Some(status)) = job.child.try_wait()
            {
                let line = format!("■ {} terminé ({status})", job.title);
                *slot = None;
                if let Ok(mut log) = self.log.lock() {
                    log.push(line);
                }
            }
        }
    }
}

impl eframe::App for VmManager {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.reap();
        self.refresh_capture(&ctx);
        ctx.request_repaint_after(Duration::from_millis(500));
        let running = self.vm.is_some();

        egui::Panel::left("reglages")
            .resizable(false)
            .exact_size(300.0)
            .frame(egui::Frame::new().fill(Color32::from_rgb(0xF2, 0xF2, 0xF4)).inner_margin(16))
            .show(ui, |ui| {
                ui.heading("Ferrite VM");
                ui.label(RichText::new("Tester l'ISO dans QEMU").color(Color32::GRAY));
                ui.add_space(12.0);

                ui.label("Image ISO");
                ui.add_enabled(!running, egui::TextEdit::singleline(&mut self.iso).desired_width(f32::INFINITY));
                match fs::metadata(&self.iso) {
                    Ok(meta) => {
                        let age = meta
                            .modified()
                            .ok()
                            .and_then(|time| SystemTime::now().duration_since(time).ok())
                            .map_or(String::new(), |age| format!(", construite il y a {}", human(age)));
                        ui.label(
                            RichText::new(format!("{:.2} Go{age}", meta.len() as f64 / 1e9))
                                .small()
                                .color(Color32::GRAY),
                        );
                    }
                    Err(_) => {
                        ui.label(RichText::new("Introuvable : construis-la avec tools/usb.sh").small().color(Color32::RED));
                    }
                }
                ui.add_space(8.0);
                ui.add_enabled_ui(!running, |ui| {
                    egui::Grid::new("materiel").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label("Mémoire");
                        egui::ComboBox::from_id_salt("memoire")
                            .selected_text(format!("{} Go", self.memory))
                            .show_ui(ui, |ui| {
                                for value in [2, 4, 6, 8] {
                                    ui.selectable_value(&mut self.memory, value, format!("{value} Go"));
                                }
                            });
                        ui.end_row();
                        ui.label("Processeurs");
                        egui::ComboBox::from_id_salt("cpus")
                            .selected_text(self.cpus.to_string())
                            .show_ui(ui, |ui| {
                                for value in [1, 2, 4, 6, 8] {
                                    ui.selectable_value(&mut self.cpus, value, value.to_string());
                                }
                            });
                        ui.end_row();
                    });
                    ui.checkbox(&mut self.network, "Accès à Internet");
                    ui.checkbox(&mut self.sound, "Son (vers Windows)");
                    ui.checkbox(&mut self.dev, "Binaires de développement")
                        .on_hover_text("Utilise target/release au lieu des binaires de l'ISO : pas besoin de reconstruire l'ISO");
                });
                ui.add_space(12.0);

                if running {
                    ui.label(RichText::new("● Machine virtuelle en marche").color(Color32::from_rgb(0x2E, 0x9E, 0x4F)));
                    if self.dev {
                        let label = if self.build.is_some() {
                            "Compilation en cours…"
                        } else {
                            "Recompiler et relancer le bureau"
                        };
                        let button = egui::Button::new(RichText::new(label).color(Color32::WHITE))
                            .fill(ACCENT)
                            .min_size(egui::vec2(ui.available_width(), 32.0));
                        if ui.add_enabled(self.build.is_none(), button).clicked() {
                            self.start_build(true);
                        }
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Capture d'écran").clicked() {
                            self.take_capture();
                        }
                        if ui.button("Redémarrer").clicked() {
                            self.monitor("system_reset");
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Ctrl+Alt+Retour arrière").on_hover_text("Quitte le bureau Ferrite").clicked() {
                            self.monitor("sendkey ctrl-alt-backspace");
                        }
                        if ui.button("Éteindre").clicked() {
                            self.monitor("system_powerdown");
                        }
                        if ui.button("Forcer l'arrêt").clicked() {
                            self.monitor("quit");
                        }
                    });
                } else {
                    let start = egui::Button::new(RichText::new("Démarrer la machine virtuelle").color(Color32::WHITE))
                        .fill(ACCENT)
                        .min_size(egui::vec2(ui.available_width(), 36.0));
                    if ui.add_enabled(self.scenario.is_none() && self.build.is_none(), start).clicked() {
                        self.start_vm();
                    }
                    if self.dev && ui.add_enabled(self.build.is_none(), egui::Button::new("Compiler le bureau")).clicked() {
                        self.start_build(false);
                    }
                }

                ui.add_space(16.0);
                ui.separator();
                ui.label(RichText::new("Scénarios de test").strong());
                ui.label(
                    RichText::new("Sans fenêtre ; captures dans build/")
                        .small()
                        .color(Color32::GRAY),
                );
                for scenario in self.scenarios() {
                    let name = scenario
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label(&name);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let free = self.scenario.is_none() && !running;
                            if ui.add_enabled(free, egui::Button::new("Lancer")).clicked() {
                                let mut command = Command::new(self.repo.join("tools/vm-test.sh"));
                                command
                                    .arg(&scenario)
                                    .arg(&self.iso)
                                    .env("FERRITE_VM_DEV", if self.dev { "1" } else { "0" });
                                self.scenario = self.spawn(command, &format!("Scénario {name}"));
                            }
                        });
                    });
                }
                if let Some(job) = &mut self.scenario {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label(format!("{} en cours…", job.title));
                    });
                    if ui.button("Arrêter le scénario").clicked() {
                        let _ = job.child.kill();
                    }
                }
                if !self.message.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(&self.message).small());
                }
            });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.label(RichText::new("Dernière capture").strong());
            if let Some((path, _, texture)) = &self.capture {
                ui.label(
                    RichText::new(path.display().to_string())
                        .small()
                        .color(Color32::GRAY),
                );
                let available = ui.available_width();
                let size = texture.size_vec2();
                let scale = (available / size.x).min(1.0);
                ui.add(egui::Image::new((texture.id(), size * scale)));
            } else {
                ui.label(RichText::new("Aucune capture pour l'instant.").color(Color32::GRAY));
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Journal").strong());
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink(false)
                .show(ui, |ui| {
                    if let Ok(log) = self.log.lock() {
                        for line in log.iter() {
                            ui.label(RichText::new(line).monospace().size(11.5));
                        }
                    }
                });
        });
    }
}

impl Drop for VmManager {
    fn drop(&mut self) {
        // Fermer l'outil arrête aussi la machine virtuelle et le scénario en cours.
        for job in [self.vm.as_mut(), self.scenario.as_mut(), self.build.as_mut()]
            .into_iter()
            .flatten()
        {
            let _ = job.child.kill();
        }
    }
}

/// Racine du dépôt : `FERRITE_REPO`, ou le dossier courant.
fn repo() -> PathBuf {
    env::var_os("FERRITE_REPO")
        .map(PathBuf::from)
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn load_png(path: &Path) -> Option<egui::ColorImage> {
    let decoder = png::Decoder::new(BufReader::new(fs::File::open(path).ok()?));
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let size = [info.width as usize, info.height as usize];
    let pixels = &buffer[..info.buffer_size()];
    match info.color_type {
        png::ColorType::Rgba => Some(egui::ColorImage::from_rgba_unmultiplied(size, pixels)),
        png::ColorType::Rgb => Some(egui::ColorImage::from_rgb(size, pixels)),
        _ => None,
    }
}

/// Durée lisible : « 5 min », « 2 h », « 3 j ».
fn human(age: Duration) -> String {
    let minutes = age.as_secs() / 60;
    match minutes {
        0 => "moins d'une minute".into(),
        1..=59 => format!("{minutes} min"),
        60..=1439 => format!("{} h", minutes / 60),
        _ => format!("{} j", minutes / 1440),
    }
}

/// Retire les codes de couleur du terminal (ESC [ … m).
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(character);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utilitaires() {
        assert_eq!(strip_ansi("\u{1b}[1;32m ok\u{1b}[0m capture"), " ok capture");
        assert_eq!(human(Duration::from_secs(150)), "2 min");
        assert_eq!(human(Duration::from_secs(3 * 3600)), "3 h");
    }
}
