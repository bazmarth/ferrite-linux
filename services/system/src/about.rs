//! Informations sur l'ordinateur (page « À propos »).

use std::fs;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct About {
    pub os: String,
    pub kernel: String,
    pub hostname: String,
    pub processor: String,
    pub cores: usize,
    pub memory_kib: u64,
    pub disk_total: u64,
    pub disk_free: u64,
    pub uptime_seconds: u64,
}

pub fn read() -> About {
    let os = fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| os_name(&text))
        .unwrap_or_else(|| "Linux".into());
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let (disk_total, disk_free) = disk_usage();
    About {
        os: format!("Ferrite OS — {os}"),
        kernel: fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|text| text.trim().to_owned())
            .unwrap_or_default(),
        hostname: fs::read_to_string("/proc/sys/kernel/hostname")
            .map(|text| text.trim().to_owned())
            .unwrap_or_default(),
        processor: processor(&cpuinfo).unwrap_or_else(|| "Inconnu".into()),
        cores: cpuinfo
            .lines()
            .filter(|line| line.starts_with("processor"))
            .count(),
        memory_kib: fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|text| meminfo_total(&text))
            .unwrap_or(0),
        disk_total,
        disk_free,
        uptime_seconds: fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
            .map_or(0, |seconds| seconds as u64),
    }
}

pub fn os_name(os_release: &str) -> Option<String> {
    os_release
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim_matches('"').to_owned())
}

pub fn processor(cpuinfo: &str) -> Option<String> {
    cpuinfo
        .lines()
        .find_map(|line| line.strip_prefix("model name"))
        .and_then(|rest| rest.split_once(':'))
        .map(|(_, name)| name.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn meminfo_total(meminfo: &str) -> Option<u64> {
    meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Taille et espace libre de la partition racine, en octets (via `df`).
fn disk_usage() -> (u64, u64) {
    let Ok(output) = crate::command::run("df", &["-B1", "--output=size,avail", "/"]) else {
        return (0, 0);
    };
    let mut numbers = output
        .lines()
        .nth(1)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|value| value.parse().ok());
    (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyse_des_fichiers_systeme() {
        assert_eq!(
            os_name("NAME=\"Debian\"\nPRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\n").as_deref(),
            Some("Debian GNU/Linux 13 (trixie)")
        );
        assert_eq!(
            processor("processor\t: 0\nmodel name\t: 12th Gen  Intel(R) Core(TM) i5\n").as_deref(),
            Some("12th Gen Intel(R) Core(TM) i5")
        );
        assert_eq!(meminfo_total("MemTotal:       16314152 kB\n"), Some(16_314_152));
    }
}
