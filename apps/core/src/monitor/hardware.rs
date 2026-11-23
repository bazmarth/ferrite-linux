//! Matériel lu dans `/sys` : description fixe (processeur, machine, système) et mesures
//! qui ne viennent pas de `/proc` : cartes graphiques, disques, interfaces réseau,
//! batteries et capteurs (`/sys/class/hwmon`).
//!
//! Tout est facultatif : un PC de bureau n'a pas de batterie, une machine virtuelle n'a
//! souvent ni capteur ni fréquence, un pilote graphique ne dit pas toujours son activité.
//! Une valeur absente reste `None` et l'interface l'omet.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use ferrite_system::command;

/// Texte d'un fichier de `/sys`, sans le saut de ligne.
pub fn read(path: impl AsRef<Path>) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

pub fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read(path)?.parse().ok()
}

fn read_i64(path: impl AsRef<Path>) -> Option<i64> {
    read(path)?.parse().ok()
}

/// Sous-dossiers `hwmonN` d'un périphérique (`device/hwmon/hwmonN` ou `device/hwmonN`).
fn hwmon_dirs(device: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in [device.join("hwmon"), device.to_path_buf()] {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.filter_map(Result::ok) {
                if entry.file_name().to_string_lossy().starts_with("hwmon")
                    && entry.path().join("name").exists()
                {
                    found.push(entry.path());
                }
            }
        }
    }
    found.sort();
    found
}

/// Liste de processeurs du noyau (« 0-7,16,18-19 »).
fn parse_cpu_list(text: &str) -> Vec<usize> {
    let mut list = Vec::new();
    for part in text.trim().split(',').filter(|part| !part.is_empty()) {
        match part.split_once('-') {
            Some((start, end)) => {
                if let (Ok(start), Ok(end)) = (start.parse::<usize>(), end.parse::<usize>()) {
                    list.extend(start..=end);
                }
            }
            None => list.extend(part.parse::<usize>().ok()),
        }
    }
    list
}

/// Taille d'un cache (« 48K », « 18432K », « 2M »).
fn parse_cache_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len()));
    let value: u64 = number.parse().ok()?;
    Some(match unit.trim() {
        "K" | "KB" | "KiB" => value * 1024,
        "M" | "MB" | "MiB" => value * 1024 * 1024,
        _ => value,
    })
}

// ---------------------------------------------------------------------------------------
// Description fixe
// ---------------------------------------------------------------------------------------

/// Type de cœur des processeurs hybrides (Intel 12e génération et suivants, ARM big.LITTLE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoreKind {
    #[default]
    Standard,
    Performance,
    Efficient,
}

#[derive(Debug, Clone, Default)]
pub struct CpuInfo {
    pub model: String,
    /// Type de chaque processeur logique.
    pub kinds: Vec<CoreKind>,
    /// Cœurs physiques, dont cœurs de performance et cœurs efficaces (0 sans hybride).
    pub cores: usize,
    pub performance_cores: usize,
    pub efficient_cores: usize,
    pub sockets: usize,
    /// Caches cumulés de tous les cœurs : (« L1 », octets)…
    pub caches: Vec<(String, u64)>,
    /// Fréquence maximale (kHz), la plus haute des cœurs.
    pub max_frequency: Option<u64>,
    pub virtualization: Option<&'static str>,
}

#[derive(Debug, Clone, Default)]
pub struct Info {
    pub cpu: CpuInfo,
    pub machine: Option<String>,
    pub os: String,
    pub kernel: String,
    pub hostname: String,
}

impl Info {
    pub fn read() -> Self {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        Self {
            cpu: read_cpu(&cpuinfo),
            machine: machine_name(),
            os: fs::read_to_string("/etc/os-release")
                .ok()
                .and_then(|text| os_name(&text))
                .unwrap_or_else(|| "Linux".into()),
            kernel: read("/proc/sys/kernel/osrelease").unwrap_or_default(),
            hostname: read("/proc/sys/kernel/hostname").unwrap_or_default(),
        }
    }
}

fn os_name(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim_matches('"').to_owned())
}

fn read_cpu(cpuinfo: &str) -> CpuInfo {
    let field = |name: &str| {
        cpuinfo.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == name).then(|| value.trim().to_owned())
        })
    };
    let model = field("model name")
        .or_else(|| field("Model"))
        .or_else(|| field("Hardware"))
        .map(|name| clean_cpu_name(&name))
        .unwrap_or_else(|| "Processeur".into());
    let flags = field("flags").unwrap_or_default();
    let virtualization = if flags.split_whitespace().any(|flag| flag == "vmx") {
        Some("Intel VT-x")
    } else if flags.split_whitespace().any(|flag| flag == "svm") {
        Some("AMD-V")
    } else {
        None
    };

    let base = Path::new("/sys/devices/system/cpu");
    let count = read(base.join("possible"))
        .or_else(|| read(base.join("present")))
        .map(|text| parse_cpu_list(&text).len())
        .filter(|count| *count > 0)
        .unwrap_or_else(|| {
            cpuinfo
                .lines()
                .filter(|line| line.starts_with("processor"))
                .count()
        });
    let count = count.max(1);

    // Hybride Intel : `cpu_core` (performance) et `cpu_atom` (efficaces). ARM : la
    // capacité de chaque cœur, la plus grande étant celle des cœurs de performance.
    let mut kinds = vec![CoreKind::Standard; count];
    let performance = read("/sys/devices/cpu_core/cpus").map(|text| parse_cpu_list(&text));
    let efficient = read("/sys/devices/cpu_atom/cpus").map(|text| parse_cpu_list(&text));
    if let (Some(performance), Some(efficient)) = (&performance, &efficient) {
        for &cpu in performance {
            if let Some(kind) = kinds.get_mut(cpu) {
                *kind = CoreKind::Performance;
            }
        }
        for &cpu in efficient {
            if let Some(kind) = kinds.get_mut(cpu) {
                *kind = CoreKind::Efficient;
            }
        }
    } else {
        let capacities: Vec<Option<u64>> = (0..count)
            .map(|cpu| read_u64(base.join(format!("cpu{cpu}/cpu_capacity"))))
            .collect();
        let max = capacities.iter().flatten().copied().max();
        let min = capacities.iter().flatten().copied().min();
        if let (Some(max), Some(min)) = (max, min)
            && max != min
        {
            for (kind, capacity) in kinds.iter_mut().zip(&capacities) {
                *kind = if *capacity == Some(max) {
                    CoreKind::Performance
                } else {
                    CoreKind::Efficient
                };
            }
        }
    }

    // Cœurs physiques : couples (boîtier, cœur) distincts.
    let mut cores = HashSet::new();
    let mut performance_cores = HashSet::new();
    let mut efficient_cores = HashSet::new();
    let mut sockets = HashSet::new();
    let mut max_frequency = None;
    for (cpu, kind) in kinds.iter().enumerate() {
        let topology = base.join(format!("cpu{cpu}/topology"));
        let package = read_u64(topology.join("physical_package_id")).unwrap_or(0);
        let core = read_u64(topology.join("core_id")).unwrap_or(cpu as u64);
        sockets.insert(package);
        cores.insert((package, core));
        match kind {
            CoreKind::Performance => {
                performance_cores.insert((package, core));
            }
            CoreKind::Efficient => {
                efficient_cores.insert((package, core));
            }
            CoreKind::Standard => {}
        }
        let frequency = read_u64(base.join(format!("cpu{cpu}/cpufreq/cpuinfo_max_freq")));
        max_frequency = max_frequency.max(frequency);
    }

    CpuInfo {
        model,
        cores: cores.len().max(1),
        performance_cores: performance_cores.len(),
        efficient_cores: efficient_cores.len(),
        sockets: sockets.len().max(1),
        caches: read_caches(base, count),
        max_frequency,
        virtualization,
        kinds,
    }
}

/// Caches de tous les cœurs, chaque cache partagé compté une fois.
fn read_caches(base: &Path, count: usize) -> Vec<(String, u64)> {
    let mut seen = HashSet::new();
    let mut totals: HashMap<u64, u64> = HashMap::new();
    for cpu in 0..count {
        let Ok(entries) = fs::read_dir(base.join(format!("cpu{cpu}/cache"))) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let dir = entry.path();
            let (Some(level), Some(size)) = (
                read_u64(dir.join("level")),
                read(dir.join("size")).and_then(|text| parse_cache_size(&text)),
            ) else {
                continue;
            };
            let kind = read(dir.join("type")).unwrap_or_default();
            let shared = read(dir.join("shared_cpu_list")).unwrap_or_else(|| cpu.to_string());
            if seen.insert((level, kind, shared)) {
                *totals.entry(level).or_default() += size;
            }
        }
    }
    let mut caches: Vec<(String, u64)> = totals
        .into_iter()
        .map(|(level, size)| (format!("L{level}"), size))
        .collect();
    caches.sort();
    caches
}

/// « 13th Gen Intel(R) Core(TM) i9-14900HX » → « Intel Core i9-14900HX ».
pub fn clean_cpu_name(name: &str) -> String {
    let mut name = name
        .replace("(R)", "")
        .replace("(r)", "")
        .replace("(TM)", "")
        .replace("(tm)", "")
        .replace(" CPU ", " ");
    if let Some(index) = name.find(" Gen ")
        && name[..index].ends_with(|c: char| c.is_ascii_alphabetic())
        && name[..index].starts_with(|c: char| c.is_ascii_digit())
    {
        name = name[index + 5..].to_owned();
    }
    if let Some(index) = name.find(" @ ") {
        name.truncate(index);
    }
    // « AMD Ryzen 9 5900X 12-Core Processor » : le nombre de cœurs est dit ailleurs.
    let words: Vec<&str> = name.split_whitespace().collect();
    let mut kept: Vec<&str> = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let core_count = word.ends_with("-Core") && words.get(index + 1) == Some(&"Processor");
        if core_count || (*word == "Processor" && index > 0 && words[index - 1].ends_with("-Core")) {
            continue;
        }
        kept.push(word);
    }
    kept.join(" ")
}

fn machine_name() -> Option<String> {
    let dmi = Path::new("/sys/class/dmi/id");
    let useless = |text: &String| {
        let lower = text.to_lowercase();
        lower.is_empty()
            || lower.contains("to be filled")
            || lower.contains("system product name")
            || lower.contains("default string")
            || lower.contains("not applicable")
            || lower == "none"
    };
    let vendor = read(dmi.join("sys_vendor")).filter(|text| !useless(text));
    let product = read(dmi.join("product_name")).filter(|text| !useless(text));
    let version = read(dmi.join("product_version")).filter(|text| !useless(text));
    let vendor = vendor.map(|vendor| pretty_vendor(&vendor));
    // Lenovo met un code (« 21CX ») dans le nom et le vrai nom dans la version.
    let product = match (&vendor, product, version) {
        (Some(vendor), _, Some(version)) if vendor == "Lenovo" => Some(version),
        (_, Some(product), _) => Some(product),
        (_, None, version) => version,
    };
    match (vendor, product) {
        (Some(vendor), Some(product)) if product.starts_with(&vendor) => Some(product),
        (Some(vendor), Some(product)) => Some(format!("{vendor} {product}")),
        (vendor, product) => vendor.or(product),
    }
}

fn pretty_vendor(vendor: &str) -> String {
    let lower = vendor.to_lowercase();
    let known = [
        ("lenovo", "Lenovo"),
        ("asus", "ASUS"),
        ("micro-star", "MSI"),
        ("dell", "Dell"),
        ("hewlett", "HP"),
        ("hp", "HP"),
        ("acer", "Acer"),
        ("gigabyte", "Gigabyte"),
        ("razer", "Razer"),
        ("samsung", "Samsung"),
        ("microsoft", "Microsoft"),
        ("framework", "Framework"),
        ("tuxedo", "TUXEDO"),
        ("schenker", "XMG"),
        ("clevo", "Clevo"),
        ("apple", "Apple"),
        ("qemu", "QEMU"),
        ("innotek", "VirtualBox"),
        ("vmware", "VMware"),
    ];
    known
        .iter()
        .find(|(key, _)| lower.starts_with(key))
        .map(|(_, name)| (*name).to_owned())
        .unwrap_or_else(|| vendor.to_owned())
}

// ---------------------------------------------------------------------------------------
// Cartes graphiques
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Vendor {
    Intel,
    Amd,
    Nvidia,
    #[default]
    Other,
}

impl Vendor {
    fn from_id(id: &str) -> Self {
        match id {
            "0x8086" => Self::Intel,
            "0x1002" | "0x1022" => Self::Amd,
            "0x10de" => Self::Nvidia,
            _ => Self::Other,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Intel => "Intel",
            Self::Amd => "AMD",
            Self::Nvidia => "NVIDIA",
            Self::Other => "",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Gpu {
    /// Adresse PCI (« 0000:01:00.0 »), clé de la carte.
    pub slot: String,
    pub name: String,
    pub driver: String,
    pub driver_version: Option<String>,
    pub integrated: bool,
    /// Carte dédiée d'un portable mise en veille par le noyau : on ne la réveille pas.
    pub asleep: bool,
    /// Activité (0 à 1).
    pub usage: Option<f32>,
    /// L'activité vient des compteurs des applications de l'utilisateur.
    pub usage_from_clients: bool,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    pub temperature: Option<f32>,
    /// Watts.
    pub power: Option<f32>,
    pub power_limit: Option<f32>,
    /// MHz.
    pub clock: Option<u32>,
    pub clock_max: Option<u32>,
    pub fan: Option<f32>,
    pub fan_rpm: Option<u32>,
    /// NVIDIA sans `nvidia-smi` : le pilote propriétaire ne dit rien d'autre.
    pub missing_tool: bool,
}

#[derive(Debug, Clone)]
struct GpuDevice {
    slot: String,
    vendor: Vendor,
    driver: String,
    name: String,
    dir: PathBuf,
}

/// Une ligne de `nvidia-smi`.
#[derive(Debug, Clone, Default, PartialEq)]
struct NvidiaRow {
    bus: String,
    name: String,
    usage: Option<f32>,
    memory_used: Option<u64>,
    memory_total: Option<u64>,
    temperature: Option<f32>,
    power: Option<f32>,
    power_limit: Option<f32>,
    clock: Option<u32>,
    clock_max: Option<u32>,
    fan: Option<f32>,
    version: Option<String>,
}

const NVIDIA_FULL: &str = "pci.bus_id,name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,power.limit,clocks.gr,clocks.max.gr,fan.speed,driver_version";
const NVIDIA_BASIC: &str = "pci.bus_id,name,utilization.gpu,memory.used,memory.total,temperature.gpu";

#[derive(Debug, Default)]
pub struct Gpus {
    devices: Vec<GpuDevice>,
    /// Champs acceptés par `nvidia-smi` (les anciens pilotes en refusent).
    nvidia_query: Option<&'static str>,
    nvidia_missing: bool,
    nvidia_names: HashMap<String, String>,
}

impl Gpus {
    /// Cartes vues sur le bus PCI (classe 0x03 : affichage), avec ou sans pilote DRM.
    pub fn discover() -> Self {
        let mut devices = Vec::new();
        if let Ok(entries) = fs::read_dir("/sys/bus/pci/devices") {
            for entry in entries.filter_map(Result::ok) {
                let dir = entry.path();
                if !read(dir.join("class")).is_some_and(|class| class.starts_with("0x03")) {
                    continue;
                }
                let slot = entry.file_name().to_string_lossy().into_owned();
                let vendor = Vendor::from_id(&read(dir.join("vendor")).unwrap_or_default());
                let driver = fs::read_link(dir.join("driver"))
                    .ok()
                    .and_then(|link| link.file_name().map(|name| name.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                let name = command::run("lspci", &["-mm", "-s", &slot])
                    .ok()
                    .and_then(|text| lspci_name(&text, vendor))
                    .unwrap_or_else(|| match vendor {
                        Vendor::Other => "Carte graphique".into(),
                        vendor => format!("Carte graphique {}", vendor.name()),
                    });
                devices.push(GpuDevice {
                    slot,
                    vendor,
                    driver,
                    name,
                    dir,
                });
            }
        }
        // Carte intégrée d'abord, comme dans les autres moniteurs.
        devices.sort_by(|a, b| a.slot.cmp(&b.slot));
        Self {
            devices,
            nvidia_query: Some(NVIDIA_FULL),
            ..Self::default()
        }
    }

    /// Mesure de chaque carte ; `clients` : activité et mémoire tirées des applications
    /// (fdinfo), par adresse PCI.
    pub fn sample(&mut self, clients: &HashMap<String, (f32, u64)>) -> Vec<Gpu> {
        let nvidia_awake = self.devices.iter().any(|device| {
            device.vendor == Vendor::Nvidia && device.driver == "nvidia" && !asleep(&device.dir)
        });
        let rows = if nvidia_awake {
            self.nvidia_rows()
        } else {
            Vec::new()
        };

        self.devices
            .iter()
            .map(|device| {
                let mut gpu = Gpu {
                    slot: device.slot.clone(),
                    name: self
                        .nvidia_names
                        .get(&device.slot)
                        .cloned()
                        .unwrap_or_else(|| device.name.clone()),
                    driver: device.driver.clone(),
                    integrated: device.vendor == Vendor::Intel,
                    asleep: asleep(&device.dir),
                    ..Gpu::default()
                };
                if device.driver == "nvidia" {
                    gpu.driver_version = read("/sys/module/nvidia/version");
                }
                if gpu.asleep {
                    return gpu;
                }
                match device.driver.as_str() {
                    "amdgpu" => {
                        gpu.usage =
                            read_u64(device.dir.join("gpu_busy_percent")).map(|value| value as f32 / 100.0);
                        gpu.memory_used = read_u64(device.dir.join("mem_info_vram_used"));
                        gpu.memory_total = read_u64(device.dir.join("mem_info_vram_total"));
                        // Une APU n'a qu'une petite réserve de mémoire vidéo.
                        gpu.integrated = gpu.memory_total.is_some_and(|total| total <= 4 << 30);
                    }
                    "i915" | "xe" => {
                        (gpu.clock, gpu.clock_max) = intel_clock(&device.dir);
                    }
                    "nvidia" => {
                        if let Some(row) = rows.iter().find(|row| same_bus(&row.bus, &device.slot)) {
                            gpu.usage = row.usage;
                            gpu.memory_used = row.memory_used;
                            gpu.memory_total = row.memory_total;
                            gpu.temperature = row.temperature;
                            gpu.power = row.power;
                            gpu.power_limit = row.power_limit;
                            gpu.clock = row.clock;
                            gpu.clock_max = row.clock_max;
                            gpu.fan = row.fan;
                            gpu.driver_version = row.version.clone().or(gpu.driver_version.take());
                            if !row.name.is_empty() {
                                gpu.name = row.name.clone();
                            }
                        }
                        gpu.missing_tool = self.nvidia_missing;
                    }
                    _ => {}
                }
                // Capteurs du pilote libre (amdgpu, nouveau, radeon…).
                for hwmon in hwmon_dirs(&device.dir) {
                    let celsius = |name: &str| read_i64(hwmon.join(name)).map(|value| value as f32 / 1000.0);
                    gpu.temperature = gpu.temperature.or_else(|| celsius("temp1_input"));
                    let watts =
                        |name: &str| read_u64(hwmon.join(name)).map(|value| value as f32 / 1_000_000.0);
                    gpu.power = gpu
                        .power
                        .or_else(|| watts("power1_average"))
                        .or_else(|| watts("power1_input"));
                    gpu.power_limit = gpu.power_limit.or_else(|| watts("power1_cap"));
                    gpu.clock = gpu
                        .clock
                        .or_else(|| read_u64(hwmon.join("freq1_input")).map(|hz| (hz / 1_000_000) as u32));
                    gpu.fan_rpm = gpu
                        .fan_rpm
                        .or_else(|| read_u64(hwmon.join("fan1_input")).map(|rpm| rpm as u32));
                    if gpu.fan.is_none()
                        && let Some(pwm) = read_u64(hwmon.join("pwm1"))
                    {
                        let max = read_u64(hwmon.join("pwm1_max")).unwrap_or(255).max(1);
                        gpu.fan = Some(pwm as f32 / max as f32);
                    }
                }
                if let Some((usage, memory)) = clients.get(&device.slot) {
                    if gpu.usage.is_none() {
                        gpu.usage = Some(*usage);
                        gpu.usage_from_clients = true;
                    }
                    if gpu.memory_used.is_none() && *memory > 0 {
                        gpu.memory_used = Some(*memory);
                    }
                } else if gpu.usage.is_none() && matches!(device.driver.as_str(), "i915" | "xe") {
                    // Pilote Intel sans application ouverte dessus : la carte est au repos.
                    gpu.usage = Some(0.0);
                    gpu.usage_from_clients = true;
                }
                gpu
            })
            .collect()
    }

    fn nvidia_rows(&mut self) -> Vec<NvidiaRow> {
        while let Some(query) = self.nvidia_query {
            let fields = format!("--query-gpu={query}");
            match command::run_with_timeout(
                "nvidia-smi",
                &[&fields, "--format=csv,noheader,nounits"],
                Duration::from_secs(3),
            ) {
                Ok(text) => {
                    let rows = parse_nvidia(&text);
                    for row in &rows {
                        if let Some(device) = self
                            .devices
                            .iter()
                            .find(|device| same_bus(&row.bus, &device.slot))
                            && !row.name.is_empty()
                        {
                            self.nvidia_names.insert(device.slot.clone(), row.name.clone());
                        }
                    }
                    return rows;
                }
                Err(error) if error.contains("indisponible") => {
                    self.nvidia_missing = true;
                    self.nvidia_query = None;
                }
                // Champ refusé par un vieux pilote : on réessaie avec les champs de base.
                Err(_) if query == NVIDIA_FULL => self.nvidia_query = Some(NVIDIA_BASIC),
                Err(_) => return Vec::new(),
            }
        }
        Vec::new()
    }
}

fn asleep(device: &Path) -> bool {
    read(device.join("power/runtime_status")).as_deref() == Some("suspended")
}

/// « 00000000:01:00.0 » (nvidia-smi) et « 0000:01:00.0 » (sysfs).
fn same_bus(nvidia: &str, slot: &str) -> bool {
    let tail = |text: &str| {
        text.rsplit(':')
            .take(2)
            .collect::<Vec<_>>()
            .join(":")
            .to_lowercase()
    };
    !nvidia.is_empty() && tail(nvidia) == tail(slot)
}

/// Nom lisible tiré de `lspci -mm -s ADRESSE`.
fn lspci_name(text: &str, vendor: Vendor) -> Option<String> {
    let line = text.lines().next()?;
    let fields: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
    let device = fields.get(2)?.trim();
    if device.is_empty() || device.starts_with("Device ") {
        return None;
    }
    // « Alder Lake-P GT2 [Iris Xe Graphics] » : le nom commercial est entre crochets.
    let commercial = match (device.find('['), device.rfind(']')) {
        (Some(open), Some(close)) if close > open => &device[open + 1..close],
        _ => device,
    };
    let prefix = vendor.name();
    Some(if prefix.is_empty() || commercial.starts_with(prefix) {
        commercial.to_owned()
    } else {
        format!("{prefix} {commercial}")
    })
}

fn parse_nvidia(text: &str) -> Vec<NvidiaRow> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split(',').map(str::trim).collect();
            let number = |index: usize| -> Option<f32> { fields.get(index)?.parse::<f32>().ok() };
            NvidiaRow {
                bus: fields.first().copied().unwrap_or_default().to_owned(),
                name: fields.get(1).copied().unwrap_or_default().to_owned(),
                usage: number(2).map(|value| value / 100.0),
                memory_used: number(3).map(|mib| (mib as u64) << 20),
                memory_total: number(4).map(|mib| (mib as u64) << 20),
                temperature: number(5),
                power: number(6),
                power_limit: number(7),
                clock: number(8).map(|mhz| mhz as u32),
                clock_max: number(9).map(|mhz| mhz as u32),
                fan: number(10).map(|value| value / 100.0),
                version: fields
                    .get(11)
                    .filter(|value| !value.is_empty() && !value.starts_with('['))
                    .map(|value| (*value).to_owned()),
            }
        })
        .collect()
}

/// Fréquence actuelle et maximale d'une carte Intel (MHz).
fn intel_clock(device: &Path) -> (Option<u32>, Option<u32>) {
    let mhz = |path: PathBuf| read_u64(path).map(|value| value as u32);
    // i915 : `drm/cardN/gt_act_freq_mhz`.
    if let Ok(entries) = fs::read_dir(device.join("drm")) {
        for entry in entries.filter_map(Result::ok) {
            let card = entry.path();
            if card.join("gt_act_freq_mhz").exists() {
                return (
                    mhz(card.join("gt_act_freq_mhz")),
                    mhz(card.join("gt_RP0_freq_mhz")).or_else(|| mhz(card.join("gt_max_freq_mhz"))),
                );
            }
        }
    }
    // xe : `tile0/gt0/freq0`.
    let freq = device.join("tile0/gt0/freq0");
    (mhz(freq.join("act_freq")), mhz(freq.join("max_freq")))
}

// ---------------------------------------------------------------------------------------
// Activité graphique par application (`/proc/PID/fdinfo`, norme DRM du noyau)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone, PartialEq)]
struct FdInfo {
    pdev: String,
    client: u64,
    /// Temps passé par moteur (ns) depuis l'ouverture.
    engines: Vec<(String, u64)>,
    capacity: HashMap<String, u64>,
    /// Pilote `xe` : cycles du moteur et cycles écoulés.
    cycles: Vec<(String, u64, u64)>,
    memory: u64,
}

/// Valeur « 338392 KiB », « 240 MiB », « 0 » en octets.
fn parse_amount(text: &str) -> u64 {
    let mut parts = text.split_whitespace();
    let value: u64 = parts.next().and_then(|value| value.parse().ok()).unwrap_or(0);
    match parts.next() {
        Some("KiB") => value << 10,
        Some("MiB") => value << 20,
        Some("GiB") => value << 30,
        _ => value,
    }
}

fn parse_fdinfo(text: &str) -> Option<FdInfo> {
    let mut info = FdInfo::default();
    let mut resident = 0;
    let mut legacy = 0;
    let mut has_resident = false;
    let mut total_cycles: HashMap<String, u64> = HashMap::new();
    let mut cycles: Vec<(String, u64)> = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if key == "drm-pdev" {
            info.pdev = value.to_owned();
        } else if key == "drm-client-id" {
            info.client = value.parse().ok()?;
        } else if let Some(engine) = key.strip_prefix("drm-engine-capacity-") {
            info.capacity
                .insert(engine.to_owned(), value.parse().unwrap_or(1));
        } else if let Some(engine) = key.strip_prefix("drm-engine-") {
            let ns = value.trim_end_matches("ns").trim().parse().unwrap_or(0);
            info.engines.push((engine.to_owned(), ns));
        } else if let Some(engine) = key.strip_prefix("drm-total-cycles-") {
            total_cycles.insert(engine.to_owned(), value.parse().unwrap_or(0));
        } else if let Some(engine) = key.strip_prefix("drm-cycles-") {
            cycles.push((engine.to_owned(), value.parse().unwrap_or(0)));
        } else if key.starts_with("drm-resident-") {
            has_resident = true;
            resident += parse_amount(value);
        } else if key.starts_with("drm-memory-") {
            legacy += parse_amount(value);
        }
    }
    if info.pdev.is_empty() {
        return None;
    }
    info.memory = if has_resident { resident } else { legacy };
    info.cycles = cycles
        .into_iter()
        .filter_map(|(engine, used)| {
            let total = *total_cycles.get(&engine)?;
            Some((engine, used, total))
        })
        .collect();
    Some(info)
}

/// Activité graphique des applications de l'utilisateur.
#[derive(Debug, Default)]
pub struct DrmClients {
    /// Descripteurs ouverts sur `/dev/dri` de chaque processus, relus toutes les 5 mesures.
    fds: HashMap<u32, Vec<String>>,
    /// Compteurs précédents, par (carte, client, moteur) : (utilisé, écoulé).
    previous: HashMap<(String, u64, String), (u64, u64)>,
    count: u64,
}

#[derive(Debug, Default)]
pub struct DrmUsage {
    /// Par processus : activité (0 à 1), mémoire graphique, carte la plus utilisée.
    pub processes: HashMap<u32, (f32, u64, String)>,
    /// Par carte : activité et mémoire.
    pub devices: HashMap<String, (f32, u64)>,
}

impl DrmClients {
    pub fn sample(&mut self, pids: &[u32], elapsed: f64) -> DrmUsage {
        if self.count.is_multiple_of(5) {
            self.fds = pids
                .iter()
                .filter_map(|&pid| {
                    let entries = fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
                    let fds: Vec<String> = entries
                        .filter_map(Result::ok)
                        .filter(|entry| {
                            fs::read_link(entry.path()).is_ok_and(|target| target.starts_with("/dev/dri/"))
                        })
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                        .collect();
                    (!fds.is_empty()).then_some((pid, fds))
                })
                .collect();
        }
        self.count += 1;

        let elapsed_ns = (elapsed * 1e9) as u64;
        let mut usage = DrmUsage::default();
        let mut seen = HashSet::new();
        let mut previous = HashMap::new();
        // Activité par moteur de chaque carte, sommée sur les clients.
        let mut device_engines: HashMap<(String, String), f32> = HashMap::new();
        for (&pid, fds) in &self.fds {
            let mut engines: HashMap<(String, String), f32> = HashMap::new();
            let mut memory = 0;
            for fd in fds {
                let Some(info) = fs::read_to_string(format!("/proc/{pid}/fdinfo/{fd}"))
                    .ok()
                    .and_then(|text| parse_fdinfo(&text))
                else {
                    continue;
                };
                // Un même client peut être ouvert par plusieurs descripteurs ou processus.
                if !seen.insert((info.pdev.clone(), info.client)) {
                    continue;
                }
                memory += info.memory;
                usage.devices.entry(info.pdev.clone()).or_default().1 += info.memory;
                let mut add = |engine: &str, fraction: f32| {
                    let key = (info.pdev.clone(), engine.to_owned());
                    *engines.entry(key.clone()).or_default() += fraction;
                    *device_engines.entry(key).or_default() += fraction;
                };
                for (engine, ns) in &info.engines {
                    let key = (info.pdev.clone(), info.client, engine.clone());
                    if let Some((before, _)) = self.previous.get(&key)
                        && elapsed_ns > 0
                    {
                        let capacity = info.capacity.get(engine).copied().unwrap_or(1).max(1);
                        let fraction = ns.saturating_sub(*before) as f32 / (elapsed_ns * capacity) as f32;
                        add(engine, fraction);
                    }
                    previous.insert(key, (*ns, 0));
                }
                for (engine, used, total) in &info.cycles {
                    let key = (info.pdev.clone(), info.client, engine.clone());
                    if let Some((before_used, before_total)) = self.previous.get(&key) {
                        let span = total.saturating_sub(*before_total);
                        if span > 0 {
                            add(engine, used.saturating_sub(*before_used) as f32 / span as f32);
                        }
                    }
                    previous.insert(key, (*used, *total));
                }
            }
            // Activité d'un processus : son moteur le plus occupé.
            if let Some(((pdev, _), busiest)) = engines.into_iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
                usage
                    .processes
                    .insert(pid, (busiest.clamp(0.0, 1.0), memory, pdev));
            } else if memory > 0 {
                usage.processes.insert(pid, (0.0, memory, String::new()));
            }
        }
        for ((pdev, _), fraction) in device_engines {
            let entry = usage.devices.entry(pdev).or_default();
            entry.0 = entry.0.max(fraction.clamp(0.0, 1.0));
        }
        self.previous = previous;
        usage
    }
}

// ---------------------------------------------------------------------------------------
// Disques
// ---------------------------------------------------------------------------------------

/// Description fixe d'un disque physique de `/sys/block`.
#[derive(Debug, Clone, Default)]
pub struct DiskInfo {
    pub name: String,
    pub model: String,
    pub kind: &'static str,
    pub size: u64,
    pub removable: bool,
    hwmon: Option<PathBuf>,
}

impl DiskInfo {
    pub fn temperature(&self) -> Option<f32> {
        let hwmon = self.hwmon.as_ref()?;
        read_i64(hwmon.join("temp1_input")).map(|value| value as f32 / 1000.0)
    }
}

/// Disques physiques : ceux qui ont un périphérique (`/sys/block/X/device`), donc ni
/// `loop`, ni `zram`, ni `dm`.
pub fn disks() -> Vec<DiskInfo> {
    let mut list = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/block") else {
        return list;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let dir = entry.path();
        let device = dir.join("device");
        let size = read_u64(dir.join("size")).unwrap_or(0) * 512;
        if !device.exists() || size == 0 || name.starts_with("sr") {
            continue;
        }
        let path = fs::canonicalize(&dir).unwrap_or_default();
        let usb = path.to_string_lossy().contains("/usb");
        let removable = usb || read_u64(dir.join("removable")) == Some(1);
        let rotational = read_u64(dir.join("queue/rotational")) == Some(1);
        let kind = if name.starts_with("nvme") {
            "SSD NVMe"
        } else if name.starts_with("mmcblk") {
            "Carte mémoire"
        } else if usb {
            "Disque USB"
        } else if rotational {
            "Disque dur"
        } else {
            "SSD"
        };
        let model = read(device.join("model"))
            .map(|model| model.split_whitespace().collect::<Vec<_>>().join(" "))
            .or_else(|| read(device.join("name")))
            .unwrap_or_else(|| name.clone());
        list.push(DiskInfo {
            hwmon: hwmon_dirs(&device).into_iter().next(),
            name,
            model,
            kind,
            size,
            removable,
        });
    }
    list.sort_by(|a, b| (a.removable, &a.name).cmp(&(b.removable, &b.name)));
    list
}

/// Disque physique qui porte `device` (« /dev/nvme0n1p2 », « /dev/mapper/racine »).
pub fn disk_of(device: &str) -> Option<String> {
    let real = fs::canonicalize(device).ok()?;
    let mut name = real.file_name()?.to_string_lossy().into_owned();
    // Volume chiffré ou logique : on descend vers la partition qui le porte.
    for _ in 0..4 {
        let slaves = Path::new("/sys/class/block").join(&name).join("slaves");
        match fs::read_dir(&slaves)
            .ok()
            .and_then(|mut entries| entries.next())
            .and_then(Result::ok)
        {
            Some(entry) => name = entry.file_name().to_string_lossy().into_owned(),
            None => break,
        }
    }
    let class = Path::new("/sys/class/block").join(&name);
    if class.join("partition").exists() {
        let parent = fs::canonicalize(&class).ok()?;
        return parent
            .parent()?
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
    }
    Some(name)
}

// ---------------------------------------------------------------------------------------
// Réseau
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetKind {
    Wifi,
    #[default]
    Ethernet,
    Mobile,
    Virtual,
}

impl NetKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Wifi => "Wi-Fi",
            Self::Ethernet => "Ethernet",
            Self::Mobile => "Réseau mobile",
            Self::Virtual => "Réseau virtuel",
        }
    }
}

/// Type d'une interface, ou `None` pour celles à cacher (boucle locale, liens de
/// conteneurs).
pub fn net_kind(name: &str) -> Option<NetKind> {
    if name == "lo" || name.starts_with("veth") {
        return None;
    }
    let dir = Path::new("/sys/class/net").join(name);
    if dir.join("wireless").exists() || dir.join("phy80211").exists() {
        return Some(NetKind::Wifi);
    }
    if name.starts_with("wwan") || name.starts_with("ww") {
        return Some(NetKind::Mobile);
    }
    if dir.join("device").exists() {
        return Some(NetKind::Ethernet);
    }
    // Interfaces virtuelles (VPN, ponts) : seulement quand elles sont en service.
    let up = read(dir.join("operstate")).is_some_and(|state| state == "up" || state == "unknown");
    let noisy = ["docker", "br-", "virbr", "vnet", "lxc", "cni", "flannel"];
    (up && !noisy.iter().any(|prefix| name.starts_with(prefix))).then_some(NetKind::Virtual)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetDetails {
    pub up: bool,
    pub mac: Option<String>,
    /// Mb/s.
    pub speed: Option<u32>,
    /// Qualité (0 à 1) et niveau (dBm) du signal Wi-Fi.
    pub signal: Option<(f32, i32)>,
}

pub fn net_details(name: &str, wireless: &str) -> NetDetails {
    let dir = Path::new("/sys/class/net").join(name);
    let up = read(dir.join("operstate")).is_some_and(|state| state == "up");
    NetDetails {
        up,
        mac: read(dir.join("address")).filter(|mac| mac != "00:00:00:00:00:00"),
        speed: if up {
            read_i64(dir.join("speed"))
                .filter(|speed| *speed > 0)
                .map(|speed| speed as u32)
        } else {
            None
        },
        signal: wireless_signal(wireless, name),
    }
}

/// Signal d'une interface dans `/proc/net/wireless` (qualité sur 70, niveau en dBm).
fn wireless_signal(text: &str, name: &str) -> Option<(f32, i32)> {
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{name}:")))?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    let quality: f32 = fields.get(2)?.trim_end_matches('.').parse().ok()?;
    let level: f32 = fields.get(3)?.trim_end_matches('.').parse().ok()?;
    Some(((quality / 70.0).clamp(0.0, 1.0), level as i32))
}

/// Adresses de `ip -o addr show` : interface → (IPv4, IPv6). Les adresses de lien IPv6
/// (`fe80::`) passent après les autres.
pub fn parse_ip_addresses(text: &str) -> HashMap<String, (Vec<String>, Vec<String>)> {
    let mut map: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (Some(name), Some(family), Some(address)) = (fields.get(1), fields.get(2), fields.get(3)) else {
            continue;
        };
        let address = address.split('/').next().unwrap_or(address).to_owned();
        let entry = map.entry((*name).to_owned()).or_default();
        match *family {
            "inet" => entry.0.push(address),
            "inet6" if address.starts_with("fe80") => entry.1.push(address),
            "inet6" => entry.1.insert(0, address),
            _ => {}
        }
    }
    map
}

// ---------------------------------------------------------------------------------------
// Batteries et capteurs
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Battery {
    pub name: String,
    pub model: Option<String>,
    pub technology: Option<String>,
    pub status: String,
    pub charging: bool,
    pub discharging: bool,
    /// 0 à 1.
    pub level: f32,
    /// Watts (toujours positif).
    pub power: Option<f32>,
    /// Wh.
    pub energy_now: Option<f32>,
    pub energy_full: Option<f32>,
    pub energy_design: Option<f32>,
    pub cycles: Option<u32>,
    pub voltage: Option<f32>,
    /// Secondes avant vide (sur batterie) ou avant pleine charge (en charge).
    pub time_left: Option<f64>,
}

impl Battery {
    /// Santé : capacité actuelle sur capacité d'origine.
    pub fn health(&self) -> Option<f32> {
        let (full, design) = (self.energy_full?, self.energy_design?);
        (design > 0.0).then(|| (full / design).clamp(0.0, 1.2))
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Power {
    pub batteries: Vec<Battery>,
    pub on_mains: Option<bool>,
}

pub fn power() -> Power {
    let mut power = Power::default();
    let Ok(entries) = fs::read_dir("/sys/class/power_supply") else {
        return power;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let dir = entry.path();
        match read(dir.join("type")).as_deref() {
            Some("Mains") => {
                let online = read_u64(dir.join("online")) == Some(1);
                power.on_mains = Some(power.on_mains.unwrap_or(false) || online);
            }
            // Batteries de souris et de casques : `scope` vaut « Device ».
            Some("Battery") if read(dir.join("scope")).as_deref() != Some("Device") => {
                if let Some(battery) = battery(&dir, &entry.file_name().to_string_lossy()) {
                    power.batteries.push(battery);
                }
            }
            _ => {}
        }
    }
    power
}

fn battery(dir: &Path, name: &str) -> Option<Battery> {
    if read_u64(dir.join("present")) == Some(0) {
        return None;
    }
    let micro = |file: &str| read_i64(dir.join(file)).map(|value| value.unsigned_abs() as f32 / 1_000_000.0);
    let voltage = micro("voltage_now");
    let design_voltage = micro("voltage_min_design").or(voltage);
    // Énergie en µWh, ou charge en µAh à multiplier par la tension.
    let energy = |name: &str| {
        micro(&format!("energy_{name}")).or_else(|| Some(micro(&format!("charge_{name}"))? * design_voltage?))
    };
    let energy_now = energy("now");
    let energy_full = energy("full");
    let power = micro("power_now")
        .or_else(|| Some(micro("current_now")? * voltage?))
        .filter(|watts| *watts > 0.05);
    let level = read_u64(dir.join("capacity"))
        .map(|value| value as f32 / 100.0)
        .or_else(|| Some(energy_now? / energy_full?))?
        .clamp(0.0, 1.0);
    let status = read(dir.join("status")).unwrap_or_default();
    let charging = status == "Charging";
    let discharging = status == "Discharging";
    let time_left = match (power, energy_now, energy_full) {
        (Some(watts), Some(now), _) if discharging => Some(f64::from(now / watts) * 3600.0),
        (Some(watts), Some(now), Some(full)) if charging => {
            Some(f64::from((full - now).max(0.0) / watts) * 3600.0)
        }
        _ => None,
    };
    Some(Battery {
        name: name.to_owned(),
        model: [read(dir.join("manufacturer")), read(dir.join("model_name"))]
            .into_iter()
            .flatten()
            .reduce(|a, b| format!("{a} {b}")),
        technology: read(dir.join("technology")).filter(|text| text != "Unknown"),
        status: match status.as_str() {
            "Charging" => "En charge",
            "Discharging" => "Sur batterie",
            "Full" => "Chargée",
            "Not charging" => "Branchée, sans charger",
            _ => "État inconnu",
        }
        .to_owned(),
        charging,
        discharging,
        level,
        power,
        energy_now,
        energy_full,
        energy_design: energy("full_design"),
        cycles: read_u64(dir.join("cycle_count"))
            .filter(|cycles| *cycles > 0)
            .map(|cycles| cycles as u32),
        voltage,
        time_left,
    })
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Temperature {
    pub label: String,
    pub celsius: f32,
    /// Seuil haut (`max`) et seuil critique (`crit`) donnés par le matériel.
    pub high: Option<f32>,
    pub critical: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fan {
    pub label: String,
    pub rpm: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SensorGroup {
    /// Nom du pilote (`coretemp`, `nvme`…).
    pub driver: String,
    pub name: String,
    pub temperatures: Vec<Temperature>,
    pub fans: Vec<Fan>,
}

/// Capteurs de `/sys/class/hwmon`, sauf ceux d'un périphérique en veille (lire le
/// réveillerait).
pub fn sensors() -> Vec<SensorGroup> {
    let mut groups = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/hwmon") else {
        return groups;
    };
    let mut dirs: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).collect();
    dirs.sort_by_key(|dir| {
        let name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
        name.trim_start_matches("hwmon").parse::<u32>().unwrap_or(0)
    });
    for dir in dirs {
        if asleep(&dir.join("device")) {
            continue;
        }
        let driver = read(dir.join("name")).unwrap_or_default();
        let mut group = SensorGroup {
            name: sensor_group_name(&driver),
            driver,
            ..SensorGroup::default()
        };
        let Ok(files) = fs::read_dir(&dir) else { continue };
        let mut names: Vec<String> = files
            .filter_map(Result::ok)
            .map(|file| file.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort_by_key(|name| {
            let digits: String = name.chars().filter(char::is_ascii_digit).collect();
            (
                name.chars().take(3).collect::<String>(),
                digits.parse::<u32>().unwrap_or(0),
            )
        });
        for name in names {
            if let Some(index) = name
                .strip_prefix("temp")
                .and_then(|rest| rest.strip_suffix("_input"))
            {
                let Some(milli) = read_i64(dir.join(&name)) else {
                    continue;
                };
                let celsius = milli as f32 / 1000.0;
                // Valeurs absurdes : capteur absent ou débranché.
                if !(1.0..=150.0).contains(&celsius) {
                    continue;
                }
                let limit = |suffix: &str| {
                    read_i64(dir.join(format!("temp{index}_{suffix}")))
                        .map(|value| value as f32 / 1000.0)
                        .filter(|value| (20.0..=200.0).contains(value))
                };
                group.temperatures.push(Temperature {
                    label: read(dir.join(format!("temp{index}_label")))
                        .unwrap_or_else(|| format!("Capteur {index}")),
                    celsius,
                    high: limit("max"),
                    critical: limit("crit"),
                });
            } else if let Some(index) = name
                .strip_prefix("fan")
                .and_then(|rest| rest.strip_suffix("_input"))
            {
                let Some(rpm) = read_u64(dir.join(&name)) else {
                    continue;
                };
                group.fans.push(Fan {
                    label: read(dir.join(format!("fan{index}_label")))
                        .unwrap_or_else(|| format!("Ventilateur {index}")),
                    rpm: rpm as u32,
                });
            }
        }
        if !group.temperatures.is_empty() || !group.fans.is_empty() {
            groups.push(group);
        }
    }
    groups
}

fn sensor_group_name(driver: &str) -> String {
    let name = match driver {
        "coretemp" | "k10temp" | "zenpower" | "cpu_thermal" => "Processeur",
        "nvme" => "SSD NVMe",
        "drivetemp" => "Disque",
        "amdgpu" | "radeon" => "Carte graphique AMD",
        "nouveau" => "Carte graphique NVIDIA",
        "acpitz" => "Carte mère (ACPI)",
        "thinkpad" => "ThinkPad",
        "dell_smm" => "Dell",
        "asus" | "asus_wmi_sensors" | "asus-nb-wmi" => "ASUS",
        "hp" => "HP",
        "spd5118" | "jc42" => "Mémoire vive",
        "iwlwifi_1" | "iwlwifi" | "mt7921_phy0" | "ath11k_hwmon" => "Wi-Fi",
        "BAT0" | "BAT1" => "Batterie",
        "ADP1" | "AC" => "Alimentation",
        other if other.starts_with("pch_") => "Puce de la carte mère",
        other if other.starts_with("iwlwifi") || other.starts_with("mt79") => "Wi-Fi",
        other => return other.to_owned(),
    };
    name.to_owned()
}

/// Température du processeur : boîtier (Intel), Tdie/Tctl (AMD), ou premier capteur du
/// groupe processeur.
pub fn cpu_temperature(groups: &[SensorGroup]) -> Option<f32> {
    let cpu = groups.iter().find(|group| {
        matches!(
            group.driver.as_str(),
            "coretemp" | "k10temp" | "zenpower" | "cpu_thermal"
        )
    })?;
    ["Package id 0", "Tdie", "Tctl"]
        .iter()
        .find_map(|label| cpu.temperatures.iter().find(|temp| temp.label == *label))
        .or_else(|| cpu.temperatures.first())
        .map(|temp| temp.celsius)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms_et_listes() {
        assert_eq!(parse_cpu_list("0-3,8,10-11"), [0, 1, 2, 3, 8, 10, 11]);
        assert_eq!(parse_cache_size("48K"), Some(48 * 1024));
        assert_eq!(parse_cache_size("2M"), Some(2 << 20));
        assert_eq!(
            clean_cpu_name("13th Gen Intel(R) Core(TM) i9-14900HX"),
            "Intel Core i9-14900HX"
        );
        assert_eq!(
            clean_cpu_name("AMD Ryzen 9 5900X 12-Core Processor"),
            "AMD Ryzen 9 5900X"
        );
        assert_eq!(
            clean_cpu_name("Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz"),
            "Intel Core i7-8550U"
        );
        assert_eq!(
            lspci_name(
                "00:02.0 \"VGA compatible controller\" \"Intel Corporation\" \"Alder Lake-P GT2 [Iris Xe Graphics]\" -r0c \"Lenovo\" \"Device 3f96\"",
                Vendor::Intel
            )
            .as_deref(),
            Some("Intel Iris Xe Graphics")
        );
        assert_eq!(
            lspci_name(
                "01:00.0 \"3D controller\" \"NVIDIA Corporation\" \"Device 2d58\" -ra1",
                Vendor::Nvidia
            ),
            None
        );
        assert!(same_bus("00000000:01:00.0", "0000:01:00.0"));
        assert!(!same_bus("00000000:02:00.0", "0000:01:00.0"));
        assert_eq!(
            os_name("NAME=\"Debian\"\nPRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\n").as_deref(),
            Some("Debian GNU/Linux 13 (trixie)")
        );
    }

    #[test]
    fn lecture_de_nvidia_smi() {
        let rows = parse_nvidia(
            "00000000:01:00.0, NVIDIA GeForce RTX 5070 Laptop GPU, 37, 2048, 8151, 61, 54.20, 115.00, 2100, 3090, [N/A], 580.95.05\n",
        );
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.name, "NVIDIA GeForce RTX 5070 Laptop GPU");
        assert_eq!(row.usage, Some(0.37));
        assert_eq!(row.memory_total, Some(8151 << 20));
        assert_eq!(row.temperature, Some(61.0));
        assert_eq!(row.clock_max, Some(3090));
        assert_eq!(row.fan, None);
        assert_eq!(row.version.as_deref(), Some("580.95.05"));
    }

    #[test]
    fn lecture_de_fdinfo() {
        let info = parse_fdinfo(
            "pos:\t0\ndrm-driver:\ti915\ndrm-client-id:\t26\ndrm-pdev:\t0000:00:02.0\ndrm-total-system0:\t338392 KiB\ndrm-resident-system0:\t1 MiB\ndrm-resident-stolen-system0:\t0\ndrm-engine-render:\t5907333328 ns\ndrm-engine-video:\t0 ns\ndrm-engine-capacity-video:\t2\n",
        )
        .unwrap();
        assert_eq!(info.pdev, "0000:00:02.0");
        assert_eq!(info.client, 26);
        assert_eq!(info.memory, 1 << 20);
        assert_eq!(info.engines[0], ("render".into(), 5_907_333_328));
        assert_eq!(info.capacity["video"], 2);
        let xe = parse_fdinfo(
            "drm-client-id:\t3\ndrm-pdev:\t0000:00:02.0\ndrm-cycles-rcs:\t500\ndrm-total-cycles-rcs:\t1000\n",
        )
        .unwrap();
        assert_eq!(xe.cycles, [("rcs".into(), 500, 1000)]);
        assert!(parse_fdinfo("pos: 0\nflags: 02\n").is_none());
    }

    #[test]
    fn reseau() {
        let wireless = "Inter-| sta-|   Quality        |\n face | tus | link level noise |\nwlp44s0: 0000   64.  -46.  -256        0      0      0      1     60        0\n";
        assert_eq!(wireless_signal(wireless, "wlp44s0"), Some((64.0 / 70.0, -46)));
        assert_eq!(wireless_signal(wireless, "wlan0"), None);
        let addresses = parse_ip_addresses(
            "3: wlp44s0    inet 192.168.11.108/24 brd 192.168.11.255 scope global wlp44s0\n3: wlp44s0    inet6 fe80::1/64 scope link\n3: wlp44s0    inet6 2a01::5/64 scope global\n",
        );
        let (v4, v6) = &addresses["wlp44s0"];
        assert_eq!(v4, &["192.168.11.108"]);
        assert_eq!(v6, &["2a01::5", "fe80::1"]);
    }

    #[test]
    fn capteurs_du_processeur() {
        let groups = vec![SensorGroup {
            driver: "coretemp".into(),
            name: "Processeur".into(),
            temperatures: vec![
                Temperature {
                    label: "Core 0".into(),
                    celsius: 50.0,
                    ..Temperature::default()
                },
                Temperature {
                    label: "Package id 0".into(),
                    celsius: 61.0,
                    ..Temperature::default()
                },
            ],
            fans: Vec::new(),
        }];
        assert_eq!(cpu_temperature(&groups), Some(61.0));
    }

    #[test]
    fn materiel_reel() {
        // Ne doit jamais échouer, quel que soit le PC (machine virtuelle comprise).
        let info = Info::read();
        assert!(!info.cpu.kinds.is_empty());
        assert!(info.cpu.cores >= 1);
        let _ = sensors();
        let _ = power();
        let _ = disks();
    }
}
