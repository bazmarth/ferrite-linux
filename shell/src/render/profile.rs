//! Mesure du rendu, pour chercher les saccades : tant que le fichier
//! `$XDG_RUNTIME_DIR/ferrite-profil` existe, le journal reçoit toutes les 5 secondes le
//! nombre d'images, le temps passé à les préparer et les panneaux redessinés au processeur.
//! Sans ce fichier, la mesure ne coûte qu'une lecture de l'horloge par image.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use tracing::info;

const REPORT: Duration = Duration::from_secs(5);
/// Au-delà, une étape du fil principal est notée dans le journal, même sans profil : à
/// 90 images par seconde, une image dure 11 ms ; 50 ms se voient déjà comme une saccade.
const SLOW: Duration = Duration::from_millis(50);
const CHECK: Duration = Duration::from_secs(1);

struct Stats {
    enabled: bool,
    checked: Instant,
    since: Instant,
    frames: u32,
    frame_total: Duration,
    frame_max: Duration,
    /// Par panneau : nombre de rastérisations et temps total.
    raster: BTreeMap<&'static str, (u32, Duration)>,
    /// Images vraiment envoyées à l'écran, et nouvelles images soumises par application.
    sent: u32,
    commits: BTreeMap<String, u32>,
}

thread_local! {
    static STATS: RefCell<Stats> = RefCell::new(Stats {
        enabled: false,
        checked: Instant::now() - CHECK,
        since: Instant::now(),
        frames: 0,
        frame_total: Duration::ZERO,
        frame_max: Duration::ZERO,
        raster: BTreeMap::new(),
        sent: 0,
        commits: BTreeMap::new(),
    });
}

fn flag() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join("ferrite-profil"))
}

/// Exécute une étape du fil principal et la note dans le journal si elle dure plus de
/// [`SLOW`] (pour retrouver la cause d'une saccade sans profileur).
pub fn timed<T>(name: &'static str, step: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = step();
    let spent = start.elapsed();
    if spent >= SLOW {
        tracing::warn!(
            etape = name,
            ms = spent.as_millis() as u64,
            "étape lente du bureau"
        );
    }
    result
}

/// Une image préparée en `spent`.
pub fn frame(spent: Duration) {
    STATS.with_borrow_mut(|stats| {
        let now = Instant::now();
        if now.duration_since(stats.checked) >= CHECK {
            stats.checked = now;
            let enabled = flag().is_some_and(|flag| flag.exists());
            if enabled && !stats.enabled {
                stats.since = now;
                stats.frames = 0;
                stats.frame_total = Duration::ZERO;
                stats.frame_max = Duration::ZERO;
                stats.raster.clear();
                stats.sent = 0;
                stats.commits.clear();
            }
            stats.enabled = enabled;
        }
        if !stats.enabled {
            return;
        }
        stats.frames += 1;
        stats.frame_total += spent;
        stats.frame_max = stats.frame_max.max(spent);
        let elapsed = now.duration_since(stats.since);
        if elapsed < REPORT {
            return;
        }
        let per_second = f64::from(stats.frames) / elapsed.as_secs_f64();
        let average = stats.frame_total.as_secs_f64() * 1000.0 / f64::from(stats.frames.max(1));
        let raster: Vec<String> = stats
            .raster
            .iter()
            .map(|(name, (count, total))| {
                format!(
                    "{name} {count}× {:.1} ms",
                    total.as_secs_f64() * 1000.0 / f64::from((*count).max(1))
                )
            })
            .collect();
        let seconds = elapsed.as_secs_f64();
        let mut commits: Vec<(&String, &u32)> = stats.commits.iter().collect();
        commits.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        let commits: Vec<String> = commits
            .iter()
            .take(6)
            .map(|(app, count)| format!("{app} {:.0}/s", f64::from(**count) / seconds))
            .collect();
        info!(
            envoyees_par_seconde = format!("{:.1}", f64::from(stats.sent) / seconds),
            soumises = commits.join(", "),
            images_par_seconde = format!("{per_second:.1}"),
            preparation_moyenne_ms = format!("{average:.2}"),
            preparation_max_ms = format!("{:.2}", stats.frame_max.as_secs_f64() * 1000.0),
            rasterisations = raster.join(", "),
            "profil du rendu"
        );
        stats.since = now;
        stats.frames = 0;
        stats.frame_total = Duration::ZERO;
        stats.frame_max = Duration::ZERO;
        stats.raster.clear();
        stats.sent = 0;
        stats.commits.clear();
    });
}

/// Une image a été envoyée à l'écran.
pub fn sent() {
    STATS.with_borrow_mut(|stats| {
        if stats.enabled {
            stats.sent += 1;
        }
    });
}

/// Une application (ou une surface du bureau) a soumis une nouvelle image.
pub fn commit(app: impl FnOnce() -> String) {
    STATS.with_borrow_mut(|stats| {
        if stats.enabled {
            *stats.commits.entry(app()).or_default() += 1;
        }
    });
}

/// Mesure `work`, une rastérisation du panneau `name`.
pub fn raster<T>(name: &'static str, work: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = work();
    STATS.with_borrow_mut(|stats| {
        if stats.enabled {
            let entry = stats.raster.entry(name).or_default();
            entry.0 += 1;
            entry.1 += start.elapsed();
        }
    });
    result
}

/// Ce qui a changé entre deux états d'un panneau redessiné (le premier écart de leur
/// description), pour trouver ce qui le fait redessiner.
pub fn changed(name: &'static str, before: &impl std::fmt::Debug, after: &impl std::fmt::Debug) {
    if !STATS.with_borrow(|stats| stats.enabled) {
        return;
    }
    let (before, after) = (format!("{before:?}"), format!("{after:?}"));
    let at = before
        .char_indices()
        .zip(after.chars())
        .find(|((_, a), b)| a != b)
        .map_or(before.len().min(after.len()), |((index, _), _)| index);
    let start = before[..at]
        .char_indices()
        .rev()
        .nth(80)
        .map_or(0, |(index, _)| index);
    let excerpt = |text: &str| {
        text.get(start..)
            .unwrap_or("")
            .chars()
            .take(160)
            .collect::<String>()
    };
    info!(
        panneau = name,
        avant = excerpt(&before),
        apres = excerpt(&after),
        "panneau redessiné"
    );
}

/// Bilan d'un geste à plusieurs doigts : course totale, sens retenu, annulé ou non.
pub fn gesture(total: (f64, f64), horizontal: Option<bool>, cancelled: bool) {
    if !STATS.with_borrow(|stats| stats.enabled) {
        return;
    }
    let sens = match horizontal {
        Some(true) => "horizontal",
        Some(false) => "vertical",
        None => "trop court",
    };
    info!(
        course_x = format!("{:.0}", total.0),
        course_y = format!("{:.0}", total.1),
        sens,
        annule = cancelled,
        "geste"
    );
}
