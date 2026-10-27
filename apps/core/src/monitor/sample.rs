//! Mesures du système, lues dans `/proc` (et `df` pour l'occupation des disques).
//!
//! Les compteurs du noyau (temps processeur, secteurs lus, octets reçus) ne font que
//! croître : `Sampler` garde la mesure précédente et en tire des taux.

use std::{collections::HashMap, fs, time::Instant};

use ferrite_system::command;

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
    pub cached: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub device: String,
    pub mount: String,
    pub size: u64,
    pub used: u64,
}

#[derive(Debug, Clone)]
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
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Charge du processeur (0 à 1), totale puis par cœur.
    pub cpu: f32,
    pub cores: Vec<f32>,
    pub memory: Memory,
    pub volumes: Vec<Volume>,
    /// Débits en octets par seconde.
    pub disk_read: f64,
    pub disk_write: f64,
    pub net_down: f64,
    pub net_up: f64,
    /// Octets reçus et envoyés depuis le démarrage.
    pub net_down_total: u64,
    pub net_up_total: u64,
    pub processes: Vec<Process>,
    pub uptime: f64,
    pub load: [f32; 3],
}

/// Temps processeur cumulés d'une ligne `cpu` de `/proc/stat` : (actif, total).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CpuTimes {
    busy: u64,
    total: u64,
}

#[derive(Debug, Default)]
pub struct Sampler {
    cpu: Vec<CpuTimes>,
    /// Temps processeur (jiffies) de chaque processus à la mesure précédente.
    ticks: HashMap<u32, u64>,
    disk: (u64, u64),
    net: (u64, u64),
    at: Option<Instant>,
    users: HashMap<u32, String>,
    my_uid: Option<u32>,
    volumes: Vec<Volume>,
    count: u64,
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            users: fs::read_to_string("/etc/passwd")
                .map(|text| parse_passwd(&text))
                .unwrap_or_default(),
            my_uid: fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|text| status_uid(&text)),
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

        let mut snapshot = Snapshot::default();
        let stat = fs::read_to_string("/proc/stat").unwrap_or_default();
        let cpu = parse_cpu_lines(&stat);
        let loads: Vec<f32> = cpu
            .iter()
            .zip(self.cpu.iter().chain(std::iter::repeat(&CpuTimes::default())))
            .map(|(now, before)| load(*before, *now))
            .collect();
        // Temps total écoulé, tous cœurs confondus : base du pourcentage par processus.
        let total_ticks = match (cpu.first(), self.cpu.first()) {
            (Some(now), Some(before)) => now.total.saturating_sub(before.total),
            _ => 0,
        };
        self.cpu = cpu;
        snapshot.cpu = loads.first().copied().unwrap_or(0.0);
        snapshot.cores = loads.into_iter().skip(1).collect();

        snapshot.memory = fs::read_to_string("/proc/meminfo")
            .map(|text| parse_meminfo(&text))
            .unwrap_or_default();

        let disk = fs::read_to_string("/proc/diskstats")
            .map(|text| parse_diskstats(&text))
            .unwrap_or_default();
        let net = fs::read_to_string("/proc/net/dev")
            .map(|text| parse_net_dev(&text))
            .unwrap_or_default();
        if elapsed > 0.0 {
            snapshot.disk_read = disk.0.saturating_sub(self.disk.0) as f64 / elapsed;
            snapshot.disk_write = disk.1.saturating_sub(self.disk.1) as f64 / elapsed;
            snapshot.net_down = net.0.saturating_sub(self.net.0) as f64 / elapsed;
            snapshot.net_up = net.1.saturating_sub(self.net.1) as f64 / elapsed;
        }
        (self.disk, self.net) = (disk, net);
        (snapshot.net_down_total, snapshot.net_up_total) = net;

        // L'occupation des disques change lentement : `df` toutes les 5 mesures.
        if self.count.is_multiple_of(5) {
            self.volumes = command::run(
                "df",
                &[
                    "-B1",
                    "--output=source,size,used,target",
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
        }
        self.count += 1;
        snapshot.volumes = self.volumes.clone();

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

        snapshot.processes = self.processes(total_ticks, snapshot.uptime);
        snapshot
    }

    fn processes(&mut self, total_ticks: u64, uptime: f64) -> Vec<Process> {
        let mut ticks = HashMap::new();
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
            list.push(Process {
                pid,
                name: process_name(&stat.comm, &command),
                user: uid
                    .and_then(|uid| self.users.get(&uid).cloned())
                    .or_else(|| uid.map(|uid| uid.to_string()))
                    .unwrap_or_default(),
                mine: uid.is_some() && uid == self.my_uid,
                cpu: cpu.clamp(0.0, 1.0),
                memory: stat.rss_pages * PAGE_SIZE,
                threads: stat.threads,
                command,
                age: (uptime - stat.start_ticks as f64 / USER_HZ).max(0.0),
            });
        }
        self.ticks = ticks;
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
            // user nice system idle iowait irq softirq steal (guest est déjà compté dans user).
            let total: u64 = values.iter().take(8).sum();
            let idle = values.get(3).copied().unwrap_or(0) + values.get(4).copied().unwrap_or(0);
            CpuTimes {
                busy: total.saturating_sub(idle),
                total,
            }
        })
        .collect()
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
        cached: field("Cached") + field("Buffers"),
        swap_total: field("SwapTotal"),
        swap_used: field("SwapTotal").saturating_sub(field("SwapFree")),
    }
}

/// Octets lus et écrits sur les disques entiers (pas les partitions, déjà comptées dans
/// leur disque, ni les disques virtuels `loop` et `ram`).
fn parse_diskstats(text: &str) -> (u64, u64) {
    let mut total = (0, 0);
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(name) = fields.get(2) else { continue };
        if !is_whole_disk(name) {
            continue;
        }
        let sectors = |index: usize| {
            fields
                .get(index)
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0)
        };
        total.0 += sectors(5) * SECTOR;
        total.1 += sectors(9) * SECTOR;
    }
    total
}

fn is_whole_disk(name: &str) -> bool {
    let digits_end = name.ends_with(|c: char| c.is_ascii_digit());
    if name.starts_with("nvme") || name.starts_with("mmcblk") {
        // nvme0n1 (disque) et nvme0n1p1 (partition).
        return !name.contains('p') || name.rfind('p') < name.rfind('n');
    }
    (name.starts_with("sd") || name.starts_with("vd") || name.starts_with("hd") || name.starts_with("xvd"))
        && !digits_end
}

/// Octets reçus et envoyés par toutes les interfaces sauf `lo`.
fn parse_net_dev(text: &str) -> (u64, u64) {
    let mut total = (0, 0);
    for line in text.lines().skip(2) {
        let Some((name, values)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" {
            continue;
        }
        let values: Vec<u64> = values.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        total.0 += values.first().copied().unwrap_or(0);
        total.1 += values.get(8).copied().unwrap_or(0);
    }
    total
}

fn parse_df(text: &str) -> Vec<Volume> {
    let mut volumes: Vec<Volume> = text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let device = fields.next()?.to_owned();
            let size = fields.next()?.parse().ok()?;
            let used = fields.next()?.parse().ok()?;
            let mount = fields.collect::<Vec<_>>().join(" ");
            (size > 0 && device.starts_with('/')).then_some(Volume {
                device,
                mount,
                size,
                used,
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
    }

    #[test]
    fn memoire_disques_et_reseau() {
        let memory = parse_meminfo(
            "MemTotal:       8000000 kB\nMemFree:  1000000 kB\nMemAvailable:   6000000 kB\nBuffers: 100 kB\nCached: 900 kB\nSwapTotal: 2000 kB\nSwapFree: 1500 kB\n",
        );
        assert_eq!(memory.total, 8_000_000 * 1024);
        assert_eq!(memory.used, 2_000_000 * 1024);
        assert_eq!(memory.cached, 1000 * 1024);
        assert_eq!(memory.swap_used, 500 * 1024);

        let disks = parse_diskstats(
            "   8       0 sda 10 0 100 0 20 0 200 0 0 0 0\n   8       1 sda1 10 0 100 0 20 0 200 0 0 0 0\n 259 0 nvme0n1 1 0 4 0 2 0 8 0 0 0 0\n 259 1 nvme0n1p1 1 0 4 0 2 0 8 0 0 0 0\n   7 0 loop0 1 0 999 0 0 0 0 0 0 0 0\n",
        );
        assert_eq!(disks, (104 * 512, 208 * 512));

        let net = parse_net_dev(
            "Inter-|   Receive\n face |bytes packets\n    lo: 500 5 0 0 0 0 0 0 500 5 0 0 0 0 0 0\n wlan0: 1000 10 0 0 0 0 0 0 300 3 0 0 0 0 0 0\n",
        );
        assert_eq!(net, (1000, 300));

        let volumes = parse_df(
            "Filesystem Size Used Mounted on\n/dev/sdb1 1000 400 /media/ferrite/MA CLE\n/dev/sdb1 1000 400 /mnt/autre\nnone 5 1 /run\n",
        );
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].mount, "/media/ferrite/MA CLE");
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
    }

    #[test]
    fn mesure_reelle() {
        let mut sampler = Sampler::new();
        sampler.sample();
        let snapshot = sampler.sample();
        assert!(snapshot.memory.total > 0);
        assert!(!snapshot.processes.is_empty());
        assert!(
            snapshot
                .processes
                .iter()
                .any(|process| process.pid == std::process::id())
        );
    }
}
