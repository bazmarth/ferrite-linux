//! Mesures du système, lues dans `/proc` et `/sys` (et `df`, `ip`, `nvidia-smi`).
//!
//! Les compteurs du noyau (temps processeur, secteurs lus, octets reçus) ne font que
//! croître : `Sampler` garde la mesure précédente et en tire des taux.

use std::{collections::HashMap, fs, sync::Arc, time::Instant};

use ferrite_system::command;

use super::hardware::{
    self, Battery, DiskInfo, DrmClients, Gpu, Gpus, Info, NetDetails, NetKind, Power, SensorGroup,
};

/// Taille d'une page mémoire (x86_64 et arm64 sous Debian).
const PAGE_SIZE: u64 = 4096;
/// Taille d'un secteur dans `/proc/diskstats`, quel que soit le disque.
const SECTOR: u64 = 512;
/// Fréquence des « jiffies » de `/proc/[pid]/stat` (USER_HZ, toujours 100 sous Linux).
const USER_HZ: f64 = 100.0;

#[derive(Debug, Clone, Default)]
pub struct Memory {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub free: u64,
    pub cached: u64,
    pub buffers: u64,
    pub shared: u64,
    pub dirty: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// zram : données d'origine et place occupée une fois compressées.
    pub zram: Option<(u64, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub device: String,
    pub fstype: String,
    pub mount: String,
    pub size: u64,
    pub used: u64,
    /// Disque physique qui le porte (« nvme0n1 »).
    pub disk: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Disk {
    pub info: DiskInfo,
    pub temperature: Option<f32>,
    /// Octets par seconde.
    pub read: f64,
    pub write: f64,
    /// Part du temps où le disque travaille (0 à 1).
    pub busy: f32,
    /// Temps de réponse moyen d'une opération (ms).
    pub latency: Option<f32>,
    pub read_total: u64,
    pub write_total: u64,
    pub volumes: Vec<Volume>,
}

#[derive(Debug, Clone, Default)]
pub struct Interface {
    pub name: String,
    pub kind: NetKind,
    pub details: NetDetails,
    /// Octets par seconde.
    pub down: f64,
    pub up: f64,
    pub received: u64,
    pub sent: u64,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    /// Réseau Wi-Fi ou connexion (NetworkManager).
    pub network: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    pub user: String,
    pub mine: bool,
    /// Part du processeur entier (0 à 1).
    pub cpu: f32,
    pub memory: u64,
    pub threads: u32,
    pub command: String,
    /// Durée depuis le lancement, en secondes.
    pub age: f64,
    /// Lecture et écriture sur disque (octets par seconde ; processus de l'utilisateur).
    pub disk: f64,
    /// Activité graphique (0 à 1), mémoire graphique et carte (adresse PCI).
    pub gpu: f32,
    pub gpu_memory: u64,
    pub gpu_slot: String,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub info: Arc<Info>,
    /// Charge du processeur (0 à 1), totale puis par cœur.
    pub cpu: f32,
    pub cores: Vec<f32>,
    /// Part du temps utilisateur, noyau et attente des disques.
    pub cpu_user: f32,
    pub cpu_system: f32,
    pub cpu_wait: f32,
    /// Fréquence de chaque cœur (kHz).
    pub frequencies: Vec<u64>,
    pub cpu_temperature: Option<f32>,
    /// Par seconde.
    pub context_switches: f64,
    pub interrupts: f64,
    pub governor: Option<String>,
    pub energy_preference: Option<String>,
    pub open_files: u64,
    pub memory: Memory,
    pub volumes: Vec<Volume>,
    pub disks: Vec<Disk>,
    /// Débits en octets par seconde, tous disques et interfaces confondus.
    pub disk_read: f64,
    pub disk_write: f64,
    pub interfaces: Vec<Interface>,
    pub net_down: f64,
    pub net_up: f64,
    /// Octets reçus et envoyés depuis le démarrage.
    pub net_down_total: u64,
    pub net_up_total: u64,
    pub gpus: Vec<Gpu>,
    pub power: Power,
    pub sensors: Vec<SensorGroup>,
    pub processes: Vec<Process>,
    pub uptime: f64,
    pub load: [f32; 3],
}

impl Snapshot {
    pub fn battery(&self) -> Option<&Battery> {
        self.power.batteries.first()
    }
}

/// Temps processeur cumulés d'une ligne `cpu` de `/proc/stat`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CpuTimes {
    busy: u64,
    user: u64,
    system: u64,
    wait: u64,
    total: u64,
}

/// Compteurs d'un disque dans `/proc/diskstats`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DiskCounters {
    read: u64,
    write: u64,
    operations: u64,
    /// Temps passé dans les opérations, et temps où le disque a travaillé (ms).
    operation_ms: u64,
    busy_ms: u64,
}

#[derive(Debug, Default)]
pub struct Sampler {
    info: Arc<Info>,
    cpu: Vec<CpuTimes>,
    /// Temps processeur (jiffies) de chaque processus à la mesure précédente.
    ticks: HashMap<u32, u64>,
    /// Octets lus et écrits par chaque processus.
    io: HashMap<u32, u64>,
    disks: HashMap<String, DiskCounters>,
    net: HashMap<String, (u64, u64)>,
    events: (u64, u64),
    at: Option<Instant>,
    users: HashMap<u32, String>,
    my_uid: Option<u32>,
    volumes: Vec<Volume>,
    disk_info: Vec<DiskInfo>,
    addresses: HashMap<String, (Vec<String>, Vec<String>)>,
    networks: HashMap<String, String>,
    gpus: Gpus,
    drm: DrmClients,
    /// Temps processeur écoulé entre deux mesures, tous cœurs confondus (jiffies).
    total_ticks: u64,
    count: u64,
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            info: Arc::new(Info::read()),
            users: fs::read_to_string("/etc/passwd")
                .map(|text| parse_passwd(&text))
                .unwrap_or_default(),
            my_uid: fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|text| status_uid(&text)),
            gpus: Gpus::discover(),
            ..Self::default()
        }
    }

    pub fn sample(&mut self) -> Snapshot {
        let now = Instant::now();
        let elapsed = self
            .at
            .map(|at| now.duration_since(at).as_secs_f64())
            .unwrap_or(0.0);
        self.at = Some(now);

        let mut snapshot = Snapshot {
            info: self.info.clone(),
            ..Snapshot::default()
        };
        self.sample_cpu(&mut snapshot, elapsed);

        snapshot.memory = fs::read_to_string("/proc/meminfo")
            .map(|text| parse_meminfo(&text))
            .unwrap_or_default();
        snapshot.memory.zram = zram();

        // Description des disques et adresses : relues de temps en temps (branchements).
        if self.count.is_multiple_of(10) {
            self.disk_info = hardware::disks();
        }
        if self.count.is_multiple_of(5) {
            self.volumes = command::run(
                "df",
                &[
                    "-B1",
                    "--output=source,fstype,size,used,target",
                    "-x",
                    "tmpfs",
                    "-x",
                    "devtmpfs",
                    "-x",
                    "squashfs",
                    "-x",
                    "efivarfs",
                    "-x",
                    "overlay",
                ],
            )
            .map(|text| parse_df(&text))
            .unwrap_or_default();
            for volume in &mut self.volumes {
                volume.disk = hardware::disk_of(&volume.device);
            }
            self.addresses = command::run("ip", &["-o", "addr", "show"])
                .map(|text| hardware::parse_ip_addresses(&text))
                .unwrap_or_default();
        }
        snapshot.volumes = self.volumes.clone();
        self.sample_disks(&mut snapshot, elapsed);
        self.sample_network(&mut snapshot, elapsed);

        let uptime = fs::read_to_string("/proc/uptime").unwrap_or_default();
        snapshot.uptime = uptime
            .split_whitespace()
            .next()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0);
        let loadavg = fs::read_to_string("/proc/loadavg").unwrap_or_default();
        for (slot, value) in snapshot.load.iter_mut().zip(loadavg.split_whitespace()) {
            *slot = value.parse().unwrap_or(0.0);
        }
        snapshot.open_files = hardware::read("/proc/sys/fs/file-nr")
            .and_then(|text| text.split_whitespace().next()?.parse().ok())
            .unwrap_or(0);

        snapshot.sensors = hardware::sensors();
        snapshot.cpu_temperature = hardware::cpu_temperature(&snapshot.sensors);
        snapshot.power = hardware::power();

        let total_ticks = self.total_ticks;
        snapshot.processes = self.processes(total_ticks, snapshot.uptime, elapsed);
        let mine: Vec<u32> = snapshot
            .processes
            .iter()
            .filter(|process| process.mine)
            .map(|process| process.pid)
            .collect();
        let drm = self.drm.sample(&mine, elapsed);
        for process in &mut snapshot.processes {
            if let Some((usage, memory, slot)) = drm.processes.get(&process.pid) {
                process.gpu = *usage;
                process.gpu_memory = *memory;
                process.gpu_slot.clone_from(slot);
            }
        }
        snapshot.gpus = self.gpus.sample(&drm.devices);
        // Le pilote NVIDIA n'a pas de capteur dans hwmon : sa température vient de
        // nvidia-smi.
        for gpu in &snapshot.gpus {
            if let Some(celsius) = gpu.temperature
                && gpu.driver == "nvidia"
            {
                snapshot.sensors.push(SensorGroup {
                    driver: "nvidia".into(),
                    name: gpu.name.clone(),
                    temperatures: vec![hardware::Temperature {
                        label: "Carte graphique".into(),
                        celsius,
                        ..hardware::Temperature::default()
                    }],
                    fans: Vec::new(),
                });
            }
        }

        self.count += 1;
        snapshot
    }

    fn sample_cpu(&mut self, snapshot: &mut Snapshot, elapsed: f64) {
        let stat = fs::read_to_string("/proc/stat").unwrap_or_default();
        let cpu = parse_cpu_lines(&stat);
        let loads: Vec<f32> = cpu
            .iter()
            .zip(self.cpu.iter().chain(std::iter::repeat(&CpuTimes::default())))
            .map(|(now, before)| load(*before, *now))
            .collect();
        // Temps total écoulé, tous cœurs confondus : base du pourcentage par processus.
        self.total_ticks = match (cpu.first(), self.cpu.first()) {
            (Some(now), Some(before)) => now.total.saturating_sub(before.total),
            _ => 0,
        };
        if let (Some(now), Some(before)) = (cpu.first(), self.cpu.first()) {
            let total = now.total.saturating_sub(before.total).max(1) as f32;
            snapshot.cpu_user = now.user.saturating_sub(before.user) as f32 / total;
            snapshot.cpu_system = now.system.saturating_sub(before.system) as f32 / total;
            snapshot.cpu_wait = now.wait.saturating_sub(before.wait) as f32 / total;
        }
        self.cpu = cpu;
        snapshot.cpu = loads.first().copied().unwrap_or(0.0);
        snapshot.cores = loads.into_iter().skip(1).collect();

        let events = parse_events(&stat);
        if elapsed > 0.0 && self.events != (0, 0) {
            snapshot.context_switches = events.0.saturating_sub(self.events.0) as f64 / elapsed;
            snapshot.interrupts = events.1.saturating_sub(self.events.1) as f64 / elapsed;
        }
        self.events = events;

        // Fréquences : cpufreq, sinon /proc/cpuinfo (machines virtuelles).
        let mut frequencies: Vec<u64> = (0..snapshot.cores.len())
            .map(|cpu| {
                hardware::read_u64(format!(
                    "/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq"
                ))
                .unwrap_or(0)
            })
            .collect();
        if frequencies.iter().all(|value| *value == 0) {
            let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
            let mhz: Vec<u64> = cpuinfo
                .lines()
                .filter(|line| line.starts_with("cpu MHz"))
                .filter_map(|line| line.split(':').nth(1)?.trim().parse::<f64>().ok())
                .map(|mhz| (mhz * 1000.0) as u64)
                .collect();
            if !mhz.is_empty() {
                frequencies = mhz;
            }
        }
        snapshot.frequencies = frequencies;
        let cpufreq = "/sys/devices/system/cpu/cpu0/cpufreq";
        snapshot.governor = hardware::read(format!("{cpufreq}/scaling_governor"));
        snapshot.energy_preference = hardware::read(format!("{cpufreq}/energy_performance_preference"));
    }

    fn sample_disks(&mut self, snapshot: &mut Snapshot, elapsed: f64) {
        let counters = fs::read_to_string("/proc/diskstats")
            .map(|text| parse_diskstats(&text))
            .unwrap_or_default();
        for info in &self.disk_info {
            let Some(now) = counters.get(&info.name).copied() else {
                continue;
            };
            let mut disk = Disk {
                info: info.clone(),
                temperature: info.temperature(),
                read_total: now.read,
                write_total: now.write,
                volumes: self
                    .volumes
                    .iter()
                    .filter(|volume| volume.disk.as_deref() == Some(info.name.as_str()))
                    .cloned()
                    .collect(),
                ..Disk::default()
            };
            if let Some(before) = self.disks.get(&info.name)
                && elapsed > 0.0
            {
                disk.read = now.read.saturating_sub(before.read) as f64 / elapsed;
                disk.write = now.write.saturating_sub(before.write) as f64 / elapsed;
                disk.busy = (now.busy_ms.saturating_sub(before.busy_ms) as f64 / (elapsed * 1000.0))
                    .clamp(0.0, 1.0) as f32;
                let operations = now.operations.saturating_sub(before.operations);
                disk.latency = (operations > 0)
                    .then(|| now.operation_ms.saturating_sub(before.operation_ms) as f32 / operations as f32);
            }
            snapshot.disk_read += disk.read;
            snapshot.disk_write += disk.write;
            snapshot.disks.push(disk);
        }
        self.disks = counters;
    }

    fn sample_network(&mut self, snapshot: &mut Snapshot, elapsed: f64) {
        let counters = fs::read_to_string("/proc/net/dev")
            .map(|text| parse_net_dev(&text))
            .unwrap_or_default();
        let wireless = fs::read_to_string("/proc/net/wireless").unwrap_or_default();
        let mut names: Vec<&String> = counters.keys().collect();
        names.sort();
        for name in names {
            let Some(kind) = hardware::net_kind(name) else {
                continue;
            };
            let (received, sent) = counters[name];
            // Les comptes de toutes les interfaces visibles, sauf la boucle locale.
            snapshot.net_down_total += received;
            snapshot.net_up_total += sent;
            let (mut down, mut up) = (0.0, 0.0);
            if let Some(before) = self.net.get(name)
                && elapsed > 0.0
            {
                down = received.saturating_sub(before.0) as f64 / elapsed;
                up = sent.saturating_sub(before.1) as f64 / elapsed;
            }
            snapshot.net_down += down;
            snapshot.net_up += up;
            let details = hardware::net_details(name, &wireless);
            if kind == NetKind::Wifi && self.count.is_multiple_of(5) {
                let network = command::run(
                    "nmcli",
                    &["-t", "-g", "GENERAL.CONNECTION", "device", "show", name],
                )
                .ok()
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty() && details.up);
                match network {
                    Some(network) => self.networks.insert(name.clone(), network),
                    None => self.networks.remove(name),
                };
            }
            let (ipv4, ipv6) = self.addresses.get(name).cloned().unwrap_or_default();
            snapshot.interfaces.push(Interface {
                name: name.clone(),
                kind,
                network: self.networks.get(name).cloned(),
                details,
                down,
                up,
                received,
                sent,
                ipv4,
                ipv6,
            });
        }
        // Interfaces physiques d'abord : Ethernet, Wi-Fi, mobile, puis virtuelles.
        snapshot
            .interfaces
            .sort_by_key(|interface| (interface.kind == NetKind::Virtual, !interface.details.up));
        self.net = counters;
    }

    fn processes(&mut self, total_ticks: u64, uptime: f64, elapsed: f64) -> Vec<Process> {
        let mut ticks = HashMap::new();
        let mut io = HashMap::new();
        let mut list = Vec::new();
        let Ok(entries) = fs::read_dir("/proc") else {
            return list;
        };
        for entry in entries.filter_map(Result::ok) {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let dir = entry.path();
            let Some(stat) = fs::read_to_string(dir.join("stat"))
                .ok()
                .and_then(|text| parse_pid_stat(&text))
            else {
                continue;
            };
            let uid = fs::read_to_string(dir.join("status"))
                .ok()
                .and_then(|text| status_uid(&text));
            let command = fs::read(dir.join("cmdline"))
                .map(|bytes| {
                    bytes
                        .split(|&byte| byte == 0)
                        .filter(|part| !part.is_empty())
                        .map(String::from_utf8_lossy)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let used = stat.ticks;
            let before = self.ticks.get(&pid).copied();
            ticks.insert(pid, used);
            let cpu = match before {
                Some(before) if total_ticks > 0 => used.saturating_sub(before) as f32 / total_ticks as f32,
                _ => 0.0,
            };
            let mine = uid.is_some() && uid == self.my_uid;
            // `/proc/PID/io` n'est lisible que pour ses propres processus.
            let mut disk = 0.0;
            if mine
                && let Some(bytes) = fs::read_to_string(dir.join("io"))
                    .ok()
                    .and_then(|text| parse_pid_io(&text))
            {
                if let Some(before) = self.io.get(&pid)
                    && elapsed > 0.0
                {
                    disk = bytes.saturating_sub(*before) as f64 / elapsed;
                }
                io.insert(pid, bytes);
            }
            list.push(Process {
                pid,
                name: process_name(&stat.comm, &command),
                user: uid
                    .and_then(|uid| self.users.get(&uid).cloned())
                    .or_else(|| uid.map(|uid| uid.to_string()))
                    .unwrap_or_default(),
                mine,
                cpu: cpu.clamp(0.0, 1.0),
                memory: stat.rss_pages * PAGE_SIZE,
                threads: stat.threads,
                command,
                age: (uptime - stat.start_ticks as f64 / USER_HZ).max(0.0),
                disk,
                ..Process::default()
            });
        }
        self.ticks = ticks;
        self.io = io;
        list
    }
}

fn load(before: CpuTimes, now: CpuTimes) -> f32 {
    let total = now.total.saturating_sub(before.total);
    if total == 0 {
        return 0.0;
    }
    (now.busy.saturating_sub(before.busy) as f32 / total as f32).clamp(0.0, 1.0)
}

/// Lignes `cpu` (total) puis `cpu0`, `cpu1`… de `/proc/stat`.
fn parse_cpu_lines(text: &str) -> Vec<CpuTimes> {
    text.lines()
        .filter(|line| line.starts_with("cpu"))
        .map(|line| {
            let values: Vec<u64> = line
                .split_whitespace()
                .skip(1)
                .filter_map(|value| value.parse().ok())
                .collect();
            let value = |index: usize| values.get(index).copied().unwrap_or(0);
            // user nice system idle iowait irq softirq steal (guest est déjà compté dans user).
            let total: u64 = values.iter().take(8).sum();
            let idle = value(3) + value(4);
            CpuTimes {
                busy: total.saturating_sub(idle),
                user: value(0) + value(1),
                system: value(2) + value(5) + value(6),
                wait: value(4),
                total,
            }
        })
        .collect()
}

/// Changements de contexte et interruptions depuis le démarrage (`/proc/stat`).
fn parse_events(text: &str) -> (u64, u64) {
    let value = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next()?.parse().ok())
            .unwrap_or(0)
    };
    (value("ctxt "), value("intr "))
}

fn parse_meminfo(text: &str) -> Memory {
    let field = |name: &str| -> u64 {
        text.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
            .map(|kib| kib * 1024)
            .unwrap_or(0)
    };
    let total = field("MemTotal");
    let available = field("MemAvailable");
    Memory {
        total,
        used: total.saturating_sub(available),
        available,
        free: field("MemFree"),
        cached: field("Cached") + field("SReclaimable"),
        buffers: field("Buffers"),
        shared: field("Shmem"),
        dirty: field("Dirty"),
        swap_total: field("SwapTotal"),
        swap_used: field("SwapTotal").saturating_sub(field("SwapFree")),
        zram: None,
    }
}

/// zram de tous les périphériques : (données d'origine, place compressée).
fn zram() -> Option<(u64, u64)> {
    let entries = fs::read_dir("/sys/block").ok()?;
    let mut total: Option<(u64, u64)> = None;
    for entry in entries.filter_map(Result::ok) {
        if !entry.file_name().to_string_lossy().starts_with("zram") {
            continue;
        }
        let Some(stat) = hardware::read(entry.path().join("mm_stat")) else {
            continue;
        };
        let values: Vec<u64> = stat
            .split_whitespace()
            .filter_map(|value| value.parse().ok())
            .collect();
        if let (Some(original), Some(compressed)) = (values.first(), values.get(2)) {
            let sum = total.get_or_insert((0, 0));
            sum.0 += original;
            sum.1 += compressed;
        }
    }
    total.filter(|(original, _)| *original > 0)
}

/// Compteurs de chaque périphérique de `/proc/diskstats`.
fn parse_diskstats(text: &str) -> HashMap<String, DiskCounters> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(name) = fields.get(2) else { continue };
        let value = |index: usize| {
            fields
                .get(index)
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0)
        };
        map.insert(
            (*name).to_owned(),
            DiskCounters {
                read: value(5) * SECTOR,
                write: value(9) * SECTOR,
                operations: value(3) + value(7),
                operation_ms: value(6) + value(10),
                busy_ms: value(12),
            },
        );
    }
    map
}

/// Octets reçus et envoyés par interface.
fn parse_net_dev(text: &str) -> HashMap<String, (u64, u64)> {
    let mut map = HashMap::new();
    for line in text.lines().skip(2) {
        let Some((name, values)) = line.split_once(':') else {
            continue;
        };
        let values: Vec<u64> = values.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        map.insert(
            name.trim().to_owned(),
            (
                values.first().copied().unwrap_or(0),
                values.get(8).copied().unwrap_or(0),
            ),
        );
    }
    map
}

fn parse_df(text: &str) -> Vec<Volume> {
    let mut volumes: Vec<Volume> = text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let device = fields.next()?.to_owned();
            let fstype = fields.next()?.to_owned();
            let size = fields.next()?.parse().ok()?;
            let used = fields.next()?.parse().ok()?;
            let mount = fields.collect::<Vec<_>>().join(" ");
            (size > 0 && device.starts_with('/')).then_some(Volume {
                device,
                fstype,
                mount,
                size,
                used,
                disk: None,
            })
        })
        .collect();
    // Un même périphérique monté deux fois n'apparaît qu'une fois.
    volumes.dedup_by(|a, b| a.device == b.device);
    volumes
}

#[derive(Debug, PartialEq, Eq)]
struct PidStat {
    comm: String,
    ticks: u64,
    threads: u32,
    start_ticks: u64,
    rss_pages: u64,
}

/// `/proc/[pid]/stat` : le nom est entre parenthèses et peut lui-même en contenir.
fn parse_pid_stat(text: &str) -> Option<PidStat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let comm = text[open + 1..close].to_owned();
    // Champs après le nom, numérotés à partir de 3 (l'état).
    let rest: Vec<&str> = text[close + 1..].split_whitespace().collect();
    let field = |number: usize| rest.get(number - 3).and_then(|value| value.parse::<u64>().ok());
    Some(PidStat {
        comm,
        ticks: field(14)? + field(15)?,
        threads: field(20)? as u32,
        start_ticks: field(22)?,
        rss_pages: field(24)?,
    })
}

/// Octets réellement lus et écrits sur disque (`/proc/[pid]/io`).
fn parse_pid_io(text: &str) -> Option<u64> {
    let field = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.trim().parse::<u64>().ok())
    };
    Some(field("read_bytes:")? + field("write_bytes:")?)
}

fn status_uid(text: &str) -> Option<u32> {
    text.lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().next()?.parse().ok())
}

fn parse_passwd(text: &str) -> HashMap<u32, String> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let uid = fields.nth(1)?.parse().ok()?;
            Some((uid, name.to_owned()))
        })
        .collect()
}

/// Nom lisible : le nom du noyau est coupé à 15 caractères, on prend alors celui de
/// l'exécutable.
fn process_name(comm: &str, command: &str) -> String {
    if comm.len() >= 15
        && let Some(program) = command.split_whitespace().next()
    {
        let base = program.rsplit('/').next().unwrap_or(program);
        if base.starts_with(comm) {
            return base.to_owned();
        }
    }
    comm.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charge_du_processeur() {
        let before = parse_cpu_lines("cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0 0 0\n");
        let after = parse_cpu_lines("cpu  200 0 150 850 0 0 0 0 0 0\ncpu0 100 0 75 425 0 0 0 0 0 0\n");
        assert_eq!(before.len(), 2);
        assert!((load(before[0], after[0]) - 0.75).abs() < 1e-6);
        assert_eq!(after[0].user - before[0].user, 100);
        assert_eq!(after[0].system - before[0].system, 50);
        assert_eq!(
            parse_events("cpu 1 2 3\nintr 5000 1 2 3\nctxt 1200\nbtime 1\n"),
            (1200, 5000)
        );
    }

    #[test]
    fn memoire_disques_et_reseau() {
        let memory = parse_meminfo(
            "MemTotal:       8000000 kB\nMemFree:  1000000 kB\nMemAvailable:   6000000 kB\nBuffers: 100 kB\nCached: 900 kB\nSwapTotal: 2000 kB\nSwapFree: 1500 kB\nShmem: 50 kB\n",
        );
        assert_eq!(memory.total, 8_000_000 * 1024);
        assert_eq!(memory.used, 2_000_000 * 1024);
        assert_eq!(memory.available, 6_000_000 * 1024);
        assert_eq!(memory.cached, 900 * 1024);
        assert_eq!(memory.buffers, 100 * 1024);
        assert_eq!(memory.shared, 50 * 1024);
        assert_eq!(memory.swap_used, 500 * 1024);

        let disks = parse_diskstats(
            "   8       0 sda 10 0 100 30 20 0 200 70 0 400 0\n 259 0 nvme0n1 1 0 4 0 2 0 8 0 0 0 0\n",
        );
        let sda = disks["sda"];
        assert_eq!((sda.read, sda.write), (100 * 512, 200 * 512));
        assert_eq!((sda.operations, sda.operation_ms, sda.busy_ms), (30, 100, 400));
        assert_eq!(disks["nvme0n1"].read, 4 * 512);

        let net = parse_net_dev(
            "Inter-|   Receive\n face |bytes packets\n    lo: 500 5 0 0 0 0 0 0 500 5 0 0 0 0 0 0\n wlan0: 1000 10 0 0 0 0 0 0 300 3 0 0 0 0 0 0\n",
        );
        assert_eq!(net["wlan0"], (1000, 300));
        assert_eq!(net["lo"], (500, 500));

        let volumes = parse_df(
            "Filesystem Type Size Used Mounted on\n/dev/sdb1 vfat 1000 400 /media/ferrite/MA CLE\n/dev/sdb1 vfat 1000 400 /mnt/autre\nnone tmpfs 5 1 /run\n",
        );
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].mount, "/media/ferrite/MA CLE");
        assert_eq!(volumes[0].fstype, "vfat");
    }

    #[test]
    fn lecture_d_un_processus() {
        let stat = parse_pid_stat(
            "1234 (Web Content (x)) S 1 1234 1234 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 7 0 5000 123456 300 18446744073709551615",
        )
        .unwrap();
        assert_eq!(stat.comm, "Web Content (x)");
        assert_eq!(stat.ticks, 300);
        assert_eq!(stat.threads, 7);
        assert_eq!(stat.start_ticks, 5000);
        assert_eq!(stat.rss_pages, 300);
        assert_eq!(
            status_uid("Name:\tbash\nUid:\t1000\t1000\t1000\t1000\n"),
            Some(1000)
        );
        assert_eq!(
            parse_passwd("root:x:0:0::/root:/bin/sh\nferrite:x:1000:1000::/home/ferrite:/bin/bash\n")[&1000],
            "ferrite"
        );
        assert_eq!(
            process_name("xdg-desktop-por", "/usr/libexec/xdg-desktop-portal-gtk"),
            "xdg-desktop-portal-gtk"
        );
        assert_eq!(process_name("bash", "bash -l"), "bash");
        assert_eq!(
            parse_pid_io("rchar: 10\nwchar: 20\nread_bytes: 4096\nwrite_bytes: 8192\n"),
            Some(12288)
        );
    }

    #[test]
    fn mesure_reelle() {
        let mut sampler = Sampler::new();
        sampler.sample();
        let snapshot = sampler.sample();
        assert!(snapshot.memory.total > 0);
        assert!(!snapshot.processes.is_empty());
        assert!(!snapshot.info.cpu.kinds.is_empty());
        assert!(
            snapshot
                .processes
                .iter()
                .any(|process| process.pid == std::process::id())
        );
    }
}
