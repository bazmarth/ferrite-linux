//! Mesure du rendu, pour chercher les saccades : tant que le fichier
//! `$XDG_RUNTIME_DIR/ferrite-profil` existe, le journal reçoit toutes les 5 secondes le
//! nombre d'images, le temps passé à les préparer et les panneaux redessinés au processeur.
//! Sans ce fichier, la mesure ne coûte qu'une lecture de l'horloge par image.
//!
//! Chaque image préparée en plus de [`BUDGET`] (une image à 90 Hz) est comptée « hors
//! budget », avec sa cause : l'étape la plus longue de l'image et, s'il y en a eu, le
//! panneau rastérisé le plus long (« panneaux › calque »). `tools/fluidite.sh` en fait un
//! bilan.

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
/// Temps d'une image sur l'écran du ThinkBook (90 Hz) : au-delà, l'image risque de manquer
/// son rendez-vous avec l'écran (saccade).
pub const BUDGET: Duration = Duration::from_micros(11_111);
/// Étapes qui englobent les autres : jamais retenues comme cause.
const OUTER: [&str; 2] = ["image", "tour de boucle"];

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
    /// Images préparées en plus de [`BUDGET`], la pire et leurs causes.
    over: u32,
    worst: Option<(Duration, String)>,
    causes: BTreeMap<String, u32>,
    /// Rendus à la carte graphique de plus de [`BUDGET`], et le plus long.
    gpu_over: u32,
    gpu_max: Duration,
    /// Étapes et rastérisations de l'image en cours.
    steps: Vec<(&'static str, Duration)>,
    rasters: Vec<(&'static str, Duration)>,
}

impl Stats {
    /// Relit le fichier de mesure au plus une fois par [`CHECK`] ; à l'activation, le
    /// bilan repart de zéro. Appelé à chaque étape, pour que la première image mesurée le
    /// soit en entier.
    fn refresh(&mut self, now: Instant) {
        if now.duration_since(self.checked) < CHECK {
            return;
        }
        self.checked = now;
        let enabled = flag().is_some_and(|flag| flag.exists());
        if enabled && !self.enabled {
            self.reset(now);
        }
        self.enabled = enabled;
    }

    /// Remet les compteurs du bilan à zéro.
    fn reset(&mut self, now: Instant) {
        self.since = now;
        self.frames = 0;
        self.frame_total = Duration::ZERO;
        self.frame_max = Duration::ZERO;
        self.raster.clear();
        self.sent = 0;
        self.commits.clear();
        self.over = 0;
        self.worst = None;
        self.causes.clear();
        self.gpu_over = 0;
        self.gpu_max = Duration::ZERO;
        self.steps.clear();
        self.rasters.clear();
    }
}

/// Cause d'une image lente : l'étape la plus longue (hors étapes englobantes) et le
/// panneau rastérisé le plus long, s'il y en a un.
fn cause(steps: &[(&'static str, Duration)], rasters: &[(&'static str, Duration)]) -> String {
    let longest = |list: &[(&'static str, Duration)]| {
        list.iter()
            .filter(|(name, _)| !OUTER.contains(name))
            .max_by_key(|(_, spent)| *spent)
            .map(|(name, _)| *name)
    };
    match (longest(steps), longest(rasters)) {
        (Some(step), Some(raster)) if step != raster => format!("{step} › {raster}"),
        (Some(name), _) | (None, Some(name)) => name.to_owned(),
        (None, None) => "inconnue".to_owned(),
    }
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
        over: 0,
        worst: None,
        causes: BTreeMap::new(),
        gpu_over: 0,
        gpu_max: Duration::ZERO,
        steps: Vec::new(),
        rasters: Vec::new(),
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
    STATS.with_borrow_mut(|stats| {
        stats.refresh(Instant::now());
        if !stats.enabled {
            return;
        }
        if name == "rendu GPU" {
            stats.gpu_max = stats.gpu_max.max(spent);
            if spent > BUDGET {
                stats.gpu_over += 1;
            }
        } else {
            stats.steps.push((name, spent));
        }
    });
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
        stats.refresh(now);
        if !stats.enabled {
            return;
        }
        stats.frames += 1;
        stats.frame_total += spent;
        stats.frame_max = stats.frame_max.max(spent);
        if spent > BUDGET {
            let cause = cause(&stats.steps, &stats.rasters);
            stats.over += 1;
            *stats.causes.entry(cause.clone()).or_default() += 1;
            if stats.worst.as_ref().is_none_or(|(worst, _)| spent > *worst) {
                stats.worst = Some((spent, cause));
            }
        }
        stats.steps.clear();
        stats.rasters.clear();
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
        let mut causes: Vec<(&String, &u32)> = stats.causes.iter().collect();
        causes.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        let causes: Vec<String> = causes
            .iter()
            .take(4)
            .map(|(cause, count)| format!("{cause} {count}×"))
            .collect();
        let worst = stats.worst.as_ref().map_or_else(String::new, |(spent, cause)| {
            format!("{:.1} ms ({cause})", spent.as_secs_f64() * 1000.0)
        });
        info!(
            envoyees_par_seconde = format!("{:.1}", f64::from(stats.sent) / seconds),
            soumises = commits.join(", "),
            images = stats.frames,
            images_par_seconde = format!("{per_second:.1}"),
            preparation_moyenne_ms = format!("{average:.2}"),
            preparation_max_ms = format!("{:.2}", stats.frame_max.as_secs_f64() * 1000.0),
            rasterisations = raster.join(", "),
            hors_budget = stats.over,
            pire_image = worst,
            causes = causes.join(", "),
            gpu_hors_budget = stats.gpu_over,
            gpu_max_ms = format!("{:.2}", stats.gpu_max.as_secs_f64() * 1000.0),
            "profil du rendu"
        );
        stats.reset(now);
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
            let spent = start.elapsed();
            let entry = stats.raster.entry(name).or_default();
            entry.0 += 1;
            entry.1 += spent;
            stats.rasters.push((name, spent));
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

/// Bilan d'un geste à plusieurs doigts : course totale, sens retenu, annulé ou non, durée
/// et, pour un changement de bureau, page de départ et d'arrivée.
pub fn gesture(
    total: (f64, f64),
    horizontal: Option<bool>,
    cancelled: bool,
    duration: Duration,
    page: Option<(usize, usize)>,
) {
    if !STATS.with_borrow(|stats| stats.enabled) {
        return;
    }
    let sens = match horizontal {
        Some(true) => "horizontal",
        Some(false) => "vertical",
        None => "trop court",
    };
    let bureau = page.map_or_else(String::new, |(from, to)| format!("{from} → {to}"));
    info!(
        course_x = format!("{:.0}", total.0),
        course_y = format!("{:.0}", total.1),
        sens,
        annule = cancelled,
        duree_ms = duration.as_millis() as u64,
        bureau,
        "geste"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_cause_d_une_image_lente_est_son_etape_la_plus_longue() {
        let ms = Duration::from_millis;
        let steps = [("image", ms(30)), ("vue du bureau", ms(2)), ("panneaux", ms(25))];
        assert_eq!(
            cause(&steps, &[("barre", ms(1)), ("calque", ms(22))]),
            "panneaux › calque"
        );
        assert_eq!(cause(&steps, &[]), "panneaux");
        assert_eq!(cause(&[("image", ms(30))], &[("calque", ms(22))]), "calque");
        assert_eq!(cause(&[("image", ms(30))], &[]), "inconnue");
        // Même nom des deux côtés : écrit une fois.
        assert_eq!(cause(&[("widgets", ms(20))], &[("widgets", ms(18))]), "widgets");
    }
}
