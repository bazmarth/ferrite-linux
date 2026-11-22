//! Emplacements de la barre latérale qui ne sont pas des dossiers : récents (fichiers
//! ouverts, d'après la liste partagée `recently-used.xbel` des applications, et fichiers ou
//! dossiers créés depuis peu dans le dossier perso), étiquettes, et espace des disques.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use ferrite_system::{command, config::Accent};

use super::file_art::{self, Emblem};

/// Étiquettes proposées tant que l'utilisateur n'a rien changé, comme sur la planche.
const DEFAULT_TAGS: [(&str, Accent); 3] = [
    ("Travail", Accent::Blue),
    ("Personnel", Accent::Green),
    ("Important", Accent::Red),
];

/// Nombre maximal de fichiers récents affichés.
const RECENT_LIMIT: usize = 40;

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Ancienneté maximale d'un élément de « Récents ».
const RECENT_DAYS: u64 = 30;
/// Nombre maximal d'éléments de « Récents ».
const RECENT_ITEMS: usize = 80;
/// Profondeur et nombre d'entrées au plus lus dans le dossier perso.
const SCAN_DEPTH: usize = 4;
const SCAN_LIMIT: usize = 20_000;
/// Dossiers habituels du dossier perso : jamais montrés eux-mêmes, mais fouillés.
const STANDARD_DIRS: [&str; 16] = [
    "Bureau",
    "Desktop",
    "Documents",
    "Téléchargements",
    "Downloads",
    "Images",
    "Pictures",
    "Musique",
    "Music",
    "Vidéos",
    "Videos",
    "Public",
    "Modèles",
    "Templates",
    "snap",
    "go",
];
/// Dossiers techniques (dépendances, compilation, environnements) : ni montrés ni fouillés.
const TECHNICAL_DIRS: [&str; 10] = [
    "node_modules",
    "target",
    "__pycache__",
    "venv",
    "env",
    "build",
    "dist",
    "vendor",
    "site-packages",
    "Steam",
];
/// Fichiers de passage : téléchargements en cours, sauvegardes d'éditeur, verrous.
const TEMPORARY_ENDINGS: [&str; 8] = [
    "~",
    ".part",
    ".crdownload",
    ".tmp",
    ".swp",
    ".lock",
    ".partial",
    ".download",
];

/// Récents : fichiers ouverts récemment, et fichiers ou dossiers créés depuis moins de
/// 30 jours dans le dossier perso, du plus récent au plus ancien. Ni le dossier perso ni ses
/// dossiers habituels (Documents, Téléchargements…), ni les dossiers cachés ou techniques ;
/// un dossier nouveau est montré seul, sans tout son contenu.
pub fn recent_items(home: &Path) -> Vec<PathBuf> {
    let since = SystemTime::now()
        .checked_sub(Duration::from_secs(RECENT_DAYS * 86_400))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut found: BTreeMap<PathBuf, SystemTime> = BTreeMap::new();
    let mut keep = |path: PathBuf, when: SystemTime| {
        let date = found.entry(path).or_insert(when);
        *date = (*date).max(when);
    };
    if let Ok(text) = fs::read_to_string(data_home().join("recently-used.xbel")) {
        for (when, path) in parse_recent(&text) {
            let when = parse_iso_date(&when).unwrap_or(SystemTime::UNIX_EPOCH);
            if when >= since && path.exists() && !is_home_folder(&path, home) && !hidden_under(&path, home) {
                keep(path, when);
            }
        }
    }
    let mut budget = SCAN_LIMIT;
    scan_created(home, 0, since, &mut budget, &mut keep);
    let mut list: Vec<(SystemTime, PathBuf)> = found.into_iter().map(|(path, when)| (when, path)).collect();
    list.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    list.into_iter()
        .take(RECENT_ITEMS)
        .map(|(_, path)| path)
        .collect()
}

/// Le dossier perso ou l'un de ses dossiers habituels.
fn is_home_folder(path: &Path, home: &Path) -> bool {
    path == home
        || (path.parent() == Some(home)
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| STANDARD_DIRS.contains(&name)))
}

/// Sous un dossier caché du dossier perso (`.config`, `.cache`…).
fn hidden_under(path: &Path, home: &Path) -> bool {
    path.strip_prefix(home).is_ok_and(|rest| {
        rest.components()
            .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
    })
}

/// Parcourt `dir` à la recherche de ce qui a été créé depuis `since`.
fn scan_created(
    dir: &Path,
    depth: usize,
    since: SystemTime,
    budget: &mut usize,
    keep: &mut impl FnMut(PathBuf, SystemTime),
) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(metadata) = entry.metadata() else { continue };
        if metadata.file_type().is_symlink() {
            continue;
        }
        let when = metadata.created().or_else(|_| metadata.modified()).ok();
        let path = entry.path();
        if metadata.is_dir() {
            if TECHNICAL_DIRS.contains(&name.as_str()) {
                continue;
            }
            let standard = depth == 0 && STANDARD_DIRS.contains(&name.as_str());
            match when {
                Some(when) if !standard && when >= since => keep(path, when),
                _ if depth < SCAN_DEPTH => scan_created(&path, depth + 1, since, budget, keep),
                _ => {}
            }
        } else if let Some(when) = when.filter(|when| *when >= since)
            && !TEMPORARY_ENDINGS.iter().any(|ending| name.ends_with(ending))
        {
            keep(path, when);
        }
    }
}

/// « 2026-11-09T03:10:00.123456Z » (dates de `recently-used.xbel`, en UTC).
pub fn parse_iso_date(text: &str) -> Option<SystemTime> {
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Jours depuis le 1er janvier 1970 (algorithme « days from civil »).
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    u64::try_from(seconds)
        .ok()
        .map(|seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
}

/// Fichiers récemment ouverts qui existent encore, du plus récent au plus ancien.
pub fn recent() -> Vec<PathBuf> {
    let Ok(text) = fs::read_to_string(data_home().join("recently-used.xbel")) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = parse_recent(&text)
        .into_iter()
        .filter(|(_, path)| path.exists())
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.dedup_by(|a, b| a.1 == b.1);
    found
        .into_iter()
        .take(RECENT_LIMIT)
        .map(|(_, path)| path)
        .collect()
}

/// (date de dernière visite, chemin) des signets `file://` d'un `recently-used.xbel`.
pub fn parse_recent(text: &str) -> Vec<(String, PathBuf)> {
    text.split("<bookmark ")
        .skip(1)
        .filter_map(|bookmark| {
            let attribute = |name: &str| {
                let start = bookmark.find(&format!("{name}=\""))? + name.len() + 2;
                let end = bookmark[start..].find('"')? + start;
                Some(&bookmark[start..end])
            };
            let href = attribute("href")?.strip_prefix("file://")?;
            let when = attribute("visited")
                .or_else(|| attribute("modified"))
                .unwrap_or_default();
            Some((when.to_owned(), PathBuf::from(unescape(href))))
        })
        .collect()
}

/// Décode les `%XX` d'une adresse et les entités XML courantes.
fn unescape(text: &str) -> String {
    let text = text
        .replace("&amp;", "&")
        .replace("&apos;", "'")
        .replace("&quot;", "\"");
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(value) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(value);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Une étiquette : son nom et sa couleur, prise dans la palette des accentuations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagDef {
    pub name: String,
    pub color: Accent,
}

/// Étiquettes des fichiers, gardées dans `~/.config/ferrite/etiquettes` : une ligne
/// « étiquette<TAB>chemin » par fichier étiqueté. La liste des étiquettes, dans l'ordre de
/// la barre latérale, est dans `~/.config/ferrite/etiquettes-liste` : « nom<TAB>couleur ».
#[derive(Debug, Default)]
pub struct Tags {
    by_path: BTreeMap<PathBuf, Vec<String>>,
    defs: Vec<TagDef>,
}

fn tags_path() -> PathBuf {
    ferrite_system::config::Settings::path().with_file_name("etiquettes")
}

fn defs_path() -> PathBuf {
    ferrite_system::config::Settings::path().with_file_name("etiquettes-liste")
}

/// Couleur d'une étiquette ; bleue si elle n'en a pas (une étiquette n'est pas l'accent).
fn accent_of(key: &str) -> Accent {
    Accent::ALL
        .into_iter()
        .find(|accent| accent.key() == key)
        .unwrap_or(Accent::Blue)
}

/// Lit la liste « nom<TAB>couleur » ; `None` si le fichier n'existe pas encore.
pub fn parse_defs(text: &str) -> Vec<TagDef> {
    let mut defs: Vec<TagDef> = Vec::new();
    for line in text.lines() {
        let (name, color) = line.split_once('\t').unwrap_or((line, ""));
        let name = name.trim();
        if !name.is_empty() && !defs.iter().any(|def| def.name == name) {
            defs.push(TagDef {
                name: name.to_owned(),
                color: accent_of(color.trim()),
            });
        }
    }
    defs
}

impl Tags {
    pub fn load() -> Self {
        let mut tags = Tags::default();
        if let Ok(text) = fs::read_to_string(tags_path()) {
            for line in text.lines() {
                if let Some((tag, path)) = line.split_once('\t') {
                    tags.by_path
                        .entry(PathBuf::from(path))
                        .or_default()
                        .push(tag.to_owned());
                }
            }
        }
        tags.defs = match fs::read_to_string(defs_path()) {
            Ok(text) => parse_defs(&text),
            Err(_) => DEFAULT_TAGS
                .iter()
                .map(|(name, color)| TagDef {
                    name: (*name).to_owned(),
                    color: *color,
                })
                .collect(),
        };
        tags.adopt_orphans();
        tags
    }

    /// Une étiquette portée par un fichier mais absente de la liste y est ajoutée (en
    /// bleu) : rien ne disparaît sans qu'on l'ait demandé.
    fn adopt_orphans(&mut self) {
        let used: Vec<String> = self.by_path.values().flatten().cloned().collect();
        for name in used {
            if !self.defs.iter().any(|def| def.name == name) {
                self.defs.push(TagDef {
                    name,
                    color: Accent::Blue,
                });
            }
        }
    }

    fn save(&self) {
        let text: String = self
            .by_path
            .iter()
            .flat_map(|(path, tags)| tags.iter().map(move |tag| format!("{tag}\t{}\n", path.display())))
            .collect();
        write_config(&tags_path(), &text);
    }

    fn save_defs(&self) {
        let text: String = self
            .defs
            .iter()
            .map(|def| format!("{}\t{}\n", def.name, def.color.key()))
            .collect();
        write_config(&defs_path(), &text);
    }

    /// Étiquettes, dans l'ordre de la barre latérale.
    pub fn list(&self) -> &[TagDef] {
        &self.defs
    }

    pub fn color_of(&self, name: &str) -> Accent {
        self.defs
            .iter()
            .find(|def| def.name == name)
            .map(|def| def.color)
            .unwrap_or_default()
    }

    /// Nombre de fichiers qui portent `name`.
    pub fn usage(&self, name: &str) -> usize {
        self.by_path
            .values()
            .filter(|tags| tags.iter().any(|tag| tag == name))
            .count()
    }

    /// Nom accepté pour une étiquette (nouvelle, ou `current` renommée) : non vide, sur
    /// une ligne, pas déjà pris (sans tenir compte de la casse).
    pub fn check_name(&self, name: &str, current: Option<&str>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Donnez un nom à l’étiquette.".into());
        }
        if name.contains(['\t', '\n', '\r']) {
            return Err("Le nom tient sur une ligne.".into());
        }
        if name.chars().count() > 40 {
            return Err("Nom trop long (40 caractères au plus).".into());
        }
        let taken = self
            .defs
            .iter()
            .any(|def| Some(def.name.as_str()) != current && def.name.to_lowercase() == name.to_lowercase());
        if taken {
            return Err(format!("L’étiquette « {name} » existe déjà."));
        }
        Ok(name.to_owned())
    }

    /// Crée l'étiquette, ou renomme et recolore `current` (les fichiers la gardent).
    pub fn save_tag(&mut self, current: Option<&str>, name: &str, color: Accent) -> Result<String, String> {
        let name = self.check_name(name, current)?;
        match current.and_then(|current| self.defs.iter_mut().find(|def| def.name == current)) {
            Some(def) => {
                let old = std::mem::replace(&mut def.name, name.clone());
                def.color = color;
                if old != name {
                    for tags in self.by_path.values_mut() {
                        for tag in tags.iter_mut().filter(|tag| **tag == old) {
                            tag.clone_from(&name);
                        }
                    }
                    self.save();
                }
            }
            None => self.defs.push(TagDef {
                name: name.clone(),
                color,
            }),
        }
        self.save_defs();
        Ok(name)
    }

    /// Supprime l'étiquette et la retire des fichiers (les fichiers restent).
    pub fn delete(&mut self, name: &str) {
        self.defs.retain(|def| def.name != name);
        for tags in self.by_path.values_mut() {
            tags.retain(|tag| tag != name);
        }
        self.by_path.retain(|_, tags| !tags.is_empty());
        self.save();
        self.save_defs();
    }

    pub fn of(&self, path: &Path) -> &[String] {
        self.by_path.get(path).map_or(&[], Vec::as_slice)
    }

    pub fn toggle(&mut self, path: &Path, tag: &str) {
        let tags = self.by_path.entry(path.to_path_buf()).or_default();
        match tags.iter().position(|known| known == tag) {
            Some(index) => {
                tags.remove(index);
            }
            None => tags.push(tag.to_owned()),
        }
        self.by_path.retain(|_, tags| !tags.is_empty());
        self.save();
    }

    /// Fichiers qui portent `tag` et existent encore.
    pub fn tagged(&self, tag: &str) -> Vec<PathBuf> {
        self.by_path
            .iter()
            .filter(|(path, tags)| tags.iter().any(|known| known == tag) && path.exists())
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Un fichier déplacé ou renommé garde ses étiquettes.
    pub fn moved(&mut self, from: &Path, to: &Path) {
        if let Some(tags) = self.by_path.remove(from) {
            self.by_path.insert(to.to_path_buf(), tags);
            self.save();
        }
    }
}

/// Couleur et emblème choisis pour un dossier ou un fichier (`None` : ceux d'origine).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Custom {
    /// Indice dans `file_art::SWATCHES`.
    pub color: Option<usize>,
    pub emblem: Option<Emblem>,
}

/// Icônes personnalisées, gardées dans `~/.config/ferrite/icones` : une ligne
/// « couleur<TAB>emblème<TAB>chemin » par élément (champ vide : celui d'origine).
#[derive(Debug, Default)]
pub struct Looks {
    by_path: BTreeMap<PathBuf, Custom>,
}

fn looks_path() -> PathBuf {
    ferrite_system::config::Settings::path().with_file_name("icones")
}

/// Lit le fichier des icônes personnalisées.
pub fn parse_looks(text: &str) -> BTreeMap<PathBuf, Custom> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let mut fields = line.splitn(3, '\t');
        let (Some(color), Some(emblem), Some(path)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        let custom = Custom {
            color: file_art::swatch(color),
            emblem: Emblem::from_key(emblem),
        };
        if !path.is_empty() && custom != Custom::default() {
            map.insert(PathBuf::from(path), custom);
        }
    }
    map
}

impl Looks {
    pub fn load() -> Self {
        Looks {
            by_path: fs::read_to_string(looks_path())
                .map(|text| parse_looks(&text))
                .unwrap_or_default(),
        }
    }

    fn save(&self) {
        let text: String = self
            .by_path
            .iter()
            .map(|(path, custom)| {
                format!(
                    "{}\t{}\t{}\n",
                    custom.color.map_or("", |index| file_art::SWATCHES[index].0),
                    custom.emblem.map_or("", Emblem::key),
                    path.display()
                )
            })
            .collect();
        write_config(&looks_path(), &text);
    }

    pub fn get(&self, path: &Path) -> Option<&Custom> {
        self.by_path.get(path)
    }

    /// Couleur et emblème affichés pour le dossier `path`.
    pub fn folder(&self, path: &Path, home: &Path) -> (usize, Emblem) {
        let (color, emblem) = file_art::folder_default(path, home);
        let custom = self.get(path).copied().unwrap_or_default();
        (custom.color.unwrap_or(color), custom.emblem.unwrap_or(emblem))
    }

    /// Couleur choisie pour l'étiquette du fichier `path`.
    pub fn file_color(&self, path: &Path) -> Option<eframe::egui::Color32> {
        self.get(path)?.color.map(|index| file_art::SWATCHES[index].2)
    }

    /// Change la couleur ou l'emblème de `path` ; `None` pour les deux rétablit l'origine.
    pub fn set(&mut self, path: &Path, color: Option<usize>, emblem: Option<Emblem>) {
        let mut custom = self.get(path).copied().unwrap_or_default();
        if color.is_some() {
            custom.color = color;
        }
        if emblem.is_some() {
            custom.emblem = emblem;
        }
        if color.is_none() && emblem.is_none() {
            custom = Custom::default();
        }
        if custom == Custom::default() {
            self.by_path.remove(path);
        } else {
            self.by_path.insert(path.to_path_buf(), custom);
        }
        self.save();
    }

    /// Un élément déplacé ou renommé garde son icône.
    pub fn moved(&mut self, from: &Path, to: &Path) {
        if let Some(custom) = self.by_path.remove(from) {
            self.by_path.insert(to.to_path_buf(), custom);
            self.save();
        }
    }
}

fn write_config(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, text);
}

pub fn disk_usage(path: &Path) -> Option<(u64, u64)> {
    let output = command::run("df", &["-B1", "--output=avail,size", &path.to_string_lossy()]).ok()?;
    let mut fields = output.lines().nth(1)?.split_whitespace();
    let free = fields.next()?.parse().ok()?;
    let total = fields.next()?.parse().ok()?;
    Some((free, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_iso() {
        let date = parse_iso_date("2026-11-09T03:10:05.123Z").unwrap();
        let seconds = date.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
        assert_eq!(seconds, 1_794_193_805);
        assert_eq!(
            parse_iso_date("1970-01-01T00:00:00Z"),
            Some(SystemTime::UNIX_EPOCH)
        );
        assert_eq!(parse_iso_date("pas une date"), None);
    }

    #[test]
    fn recents_sans_les_dossiers_du_perso() {
        let home = std::env::temp_dir().join(format!("ferrite-recents-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        for dir in [
            "Documents/Cours",
            "Téléchargements",
            ".cache/vieux",
            "projet/src",
            "code/target/debug",
        ] {
            fs::create_dir_all(home.join(dir)).unwrap();
        }
        for file in [
            "Documents/rapport.pdf",
            "Téléchargements/photo.zip",
            "Téléchargements/film.mp4.part",
            ".cache/vieux/x.bin",
            "projet/src/main.rs",
            "code/target/debug/app",
            "notes.txt~",
        ] {
            fs::write(home.join(file), "x").unwrap();
        }
        let since = SystemTime::now() - Duration::from_secs(3_600);
        let mut found = Vec::new();
        let mut budget = SCAN_LIMIT;
        scan_created(&home, 0, since, &mut budget, &mut |path: PathBuf, _| {
            found.push(path.strip_prefix(&home).unwrap().to_path_buf());
        });
        found.sort();
        let _ = fs::remove_dir_all(&home);
        let expected: Vec<PathBuf> = [
            "Documents/Cours",
            "Documents/rapport.pdf",
            "Téléchargements/photo.zip",
            "code",
            "projet",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(found, expected);
        assert!(is_home_folder(&home.join("Documents"), &home));
        assert!(hidden_under(&home.join(".config/x"), &home));
    }

    #[test]
    fn icones_personnalisees() {
        let map = parse_looks("vert\t\t/a/Projets\n\tetoile\t/a/b c\n\t\t/vide\nmauvais\n");
        assert_eq!(map.len(), 2);
        assert_eq!(map[Path::new("/a/Projets")].color, file_art::swatch("vert"));
        assert_eq!(map[Path::new("/a/b c")].emblem, Some(Emblem::Etoile));
    }

    #[test]
    fn liste_des_etiquettes() {
        let defs = parse_defs("Travail\tbleu\nFamille\trose\n\nFamille\tvert\nSans couleur\n");
        assert_eq!(
            defs,
            [
                TagDef {
                    name: "Travail".into(),
                    color: Accent::Blue
                },
                TagDef {
                    name: "Famille".into(),
                    color: Accent::Pink
                },
                TagDef {
                    name: "Sans couleur".into(),
                    color: Accent::Blue
                },
            ]
        );
    }

    #[test]
    fn noms_d_etiquettes() {
        let tags = Tags {
            defs: parse_defs("Travail\tbleu\nFamille\trose\n"),
            ..Tags::default()
        };
        assert!(tags.check_name("  ", None).is_err());
        assert!(tags.check_name("travail", None).is_err());
        assert_eq!(tags.check_name(" Vacances ", None), Ok("Vacances".into()));
        // Garder son propre nom en changeant la couleur est permis.
        assert_eq!(tags.check_name("Travail", Some("Travail")), Ok("Travail".into()));
        assert!(tags.check_name("Famille", Some("Travail")).is_err());
    }

    #[test]
    fn fichiers_recents_du_xbel() {
        let text = r#"<xbel>
  <bookmark href="file:///home/moi/Documents/rapport%20final.pdf" added="2026-11-01T10:00:00Z" modified="2026-11-01T10:00:00Z" visited="2026-11-02T09:12:00Z">
  </bookmark>
  <bookmark href="https://example.org" visited="2026-11-03T00:00:00Z"></bookmark>
  <bookmark href="file:///tmp/caf%C3%A9.txt" visited="2026-11-03T08:00:00Z"></bookmark>
</xbel>"#;
        let found = parse_recent(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].1, PathBuf::from("/home/moi/Documents/rapport final.pdf"));
        assert_eq!(found[1].1, PathBuf::from("/tmp/café.txt"));
        assert_eq!(found[1].0, "2026-11-03T08:00:00Z");
    }
}
