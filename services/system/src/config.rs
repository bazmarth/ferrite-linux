//! Réglages de l'utilisateur partagés entre le compositeur et Paramètres, dans
//! `$XDG_CONFIG_HOME/ferrite/reglages` (une ligne `clé = valeur` par réglage).

use std::{
    collections::BTreeMap,
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

/// Disposition du clavier par défaut (voir `docs/PLAN.md` §1.2).
pub const DEFAULT_KEYBOARD_LAYOUT: &str = "us";

/// Dispositions proposées dans Paramètres : (code XKB, nom).
pub const KEYBOARD_LAYOUTS: &[(&str, &str)] = &[
    ("us", "Anglais (États-Unis, QWERTY)"),
    ("gb", "Anglais (Royaume-Uni, QWERTY)"),
    ("fr", "Français (AZERTY)"),
    ("be", "Belge (AZERTY)"),
    ("ch(fr)", "Suisse romand (QWERTZ)"),
    ("ca(fr)", "Canadien français"),
    ("de", "Allemand (QWERTZ)"),
    ("es", "Espagnol"),
];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Settings {
    values: BTreeMap<String, String>,
}

impl Settings {
    pub fn path() -> PathBuf {
        let base = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        base.join("ferrite").join("reglages")
    }

    /// Date de dernière modification du fichier (pour détecter un changement).
    pub fn modified() -> Option<SystemTime> {
        fs::metadata(Self::path())
            .and_then(|metadata| metadata.modified())
            .ok()
    }

    pub fn load() -> Settings {
        fs::read_to_string(Self::path())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn parse(text: &str) -> Settings {
        let values = text
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
            .filter(|(key, _)| !key.is_empty())
            .collect();
        Settings { values }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_owned(), value.to_owned());
    }

    pub fn keyboard_layout(&self) -> &str {
        self.get("clavier")
            .filter(|layout| !layout.is_empty())
            .unwrap_or(DEFAULT_KEYBOARD_LAYOUT)
    }

    pub fn dock_size(&self) -> DockSize {
        DockSize::parse(self.get("dock_taille").unwrap_or_default())
    }

    pub fn dock_hiding(&self) -> DockHiding {
        DockHiding::parse(self.get("dock_masquage").unwrap_or_default())
    }

    /// Applications épinglées dans le Dock (identifiants : `app_id` des applications
    /// Ferrite, nom du fichier `.desktop` des autres) ; `None` : le Dock par défaut.
    pub fn dock_pinned(&self) -> Option<Vec<String>> {
        self.get("dock_epingles").map(split_list)
    }

    pub fn set_dock_pinned(&mut self, apps: &[String]) {
        self.set("dock_epingles", &apps.join(","));
    }

    /// Applications qui peuvent ouvrir plusieurs fenêtres ; les autres n'en ont qu'une,
    /// ramenée au premier plan quand on les relance. Fichiers par défaut.
    pub fn multi_window_apps(&self) -> Vec<String> {
        self.get("fenetres_multiples")
            .map(split_list)
            .unwrap_or_else(|| vec!["ferrite-files".to_owned()])
    }

    pub fn set_multi_window_apps(&mut self, apps: &[String]) {
        self.set("fenetres_multiples", &apps.join(","));
    }

    /// Agrandissement des icônes du Dock sous la souris, comme sur macOS (oui par défaut).
    pub fn dock_magnify(&self) -> bool {
        self.get("dock_agrandissement") != Some("non")
    }

    /// Masquage de la barre du haut ; par défaut, elle ne se cache qu'en plein écran.
    pub fn bar_hiding(&self) -> DockHiding {
        match self.get("barre_masquage") {
            Some(value) if !value.is_empty() => DockHiding::parse(value),
            _ => DockHiding::Never,
        }
    }

    /// Mode d'apparence choisi dans Paramètres (clair par défaut).
    pub fn appearance(&self) -> Appearance {
        Appearance::parse(self.get("apparence").unwrap_or_default())
    }

    /// Couleur d'accentuation (celle de Ferrite par défaut).
    pub fn accent(&self) -> Accent {
        Accent::parse(self.get("accent").unwrap_or_default())
    }

    /// Couleur de surlignage du texte ; `None` : celle d'accentuation.
    pub fn highlight(&self) -> Option<Accent> {
        self.get("surlignage")
            .filter(|value| !value.is_empty() && *value != "accent")
            .map(Accent::parse)
    }

    /// Taille des icônes des barres latérales.
    pub fn sidebar_icons(&self) -> DockSize {
        DockSize::parse(self.get("barre_laterale_icones").unwrap_or_default())
    }

    /// Hauteur totale du Dock, en pixels logiques.
    pub fn dock_height(&self) -> u32 {
        parse_height(self.get("dock_hauteur"), 68, 68, 120)
    }

    /// Hauteur de la barre des menus, en pixels logiques.
    pub fn bar_height(&self) -> u32 {
        parse_height(self.get("barre_hauteur"), 34, 24, 60)
    }

    pub fn scroll_bars(&self) -> ScrollBars {
        ScrollBars::parse(self.get("barres_defilement").unwrap_or_default())
    }

    /// Fond d'écran : nom d'un fond de Ferrite, ou chemin absolu d'une image (vide : le
    /// fond Ferrite).
    pub fn wallpaper(&self) -> &str {
        self.get("fond_ecran").unwrap_or_default()
    }

    /// Écrit le fichier entier (via un fichier temporaire, pour ne jamais le laisser à moitié).
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut text = String::from("# Réglages Ferrite (modifiés par Paramètres)\n");
        for (key, value) in &self.values {
            text.push_str(&format!("{key} = {value}\n"));
        }
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, text)?;
        fs::rename(temporary, path)
    }
}

/// Taille des icônes du Dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl DockSize {
    pub const ALL: [DockSize; 3] = [DockSize::Small, DockSize::Medium, DockSize::Large];

    fn parse(value: &str) -> Self {
        match value {
            "petit" => DockSize::Small,
            "grand" => DockSize::Large,
            _ => DockSize::Medium,
        }
    }

    /// Valeur écrite dans le fichier de réglages.
    pub fn key(self) -> &'static str {
        match self {
            DockSize::Small => "petit",
            DockSize::Medium => "moyen",
            DockSize::Large => "grand",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DockSize::Small => "Petit",
            DockSize::Medium => "Moyen",
            DockSize::Large => "Grand",
        }
    }

    /// Côté des icônes, en pixels logiques.
    pub fn icon(self) -> i32 {
        match self {
            DockSize::Small => 42,
            DockSize::Medium => 52,
            DockSize::Large => 64,
        }
    }
}

/// Quand le Dock se cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockHiding {
    /// Caché quand une fenêtre le recouvre ou en plein écran.
    #[default]
    Smart,
    /// Toujours caché, sauf quand la souris touche le bas de l'écran.
    Always,
    /// Toujours visible ; les fenêtres s'arrêtent au-dessus de lui.
    Never,
}

impl DockHiding {
    pub const ALL: [DockHiding; 3] = [DockHiding::Smart, DockHiding::Always, DockHiding::Never];

    fn parse(value: &str) -> Self {
        match value {
            "toujours" => DockHiding::Always,
            "jamais" => DockHiding::Never,
            _ => DockHiding::Smart,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            DockHiding::Smart => "intelligent",
            DockHiding::Always => "toujours",
            DockHiding::Never => "jamais",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DockHiding::Smart => "Quand une fenêtre le recouvre",
            DockHiding::Always => "Toujours",
            DockHiding::Never => "Jamais",
        }
    }

    /// Libellé pour la barre du haut (féminin).
    pub fn bar_label(self) -> &'static str {
        match self {
            DockHiding::Smart => "Quand une fenêtre la recouvre",
            DockHiding::Always => "Toujours",
            DockHiding::Never => "Jamais (sauf en plein écran)",
        }
    }
}

/// Apparence des fenêtres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    Light,
    Dark,
    /// Clair le jour, sombre la nuit.
    Auto,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::Light, Appearance::Dark, Appearance::Auto];

    fn parse(value: &str) -> Self {
        match value {
            "sombre" => Appearance::Dark,
            "auto" => Appearance::Auto,
            _ => Appearance::Light,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Appearance::Light => "clair",
            Appearance::Dark => "sombre",
            Appearance::Auto => "auto",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Appearance::Light => "Clair",
            Appearance::Dark => "Sombre",
            Appearance::Auto => "Auto",
        }
    }

    /// Résout le mode automatique selon l'heure UTC : sombre entre 19 h et 7 h.
    pub fn is_dark(self) -> bool {
        match self {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::Auto => {
                let seconds = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                Self::is_dark_at_hour((seconds / 3600 % 24) as u8)
            }
        }
    }

    fn is_dark_at_hour(hour: u8) -> bool {
        !(7..19).contains(&hour)
    }
}

/// Couleur d'accentuation, comme dans les Réglages de macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Accent {
    /// Chaque application garde sa couleur (le bleu Ferrite pour les nôtres).
    #[default]
    Multicolor,
    Blue,
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
    Graphite,
}

impl Accent {
    pub const ALL: [Accent; 9] = [
        Accent::Multicolor,
        Accent::Blue,
        Accent::Purple,
        Accent::Pink,
        Accent::Red,
        Accent::Orange,
        Accent::Yellow,
        Accent::Green,
        Accent::Graphite,
    ];

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|accent| accent.key() == value)
            .unwrap_or_default()
    }

    pub fn key(self) -> &'static str {
        match self {
            Accent::Multicolor => "multicolore",
            Accent::Blue => "bleu",
            Accent::Purple => "violet",
            Accent::Pink => "rose",
            Accent::Red => "rouge",
            Accent::Orange => "orange",
            Accent::Yellow => "jaune",
            Accent::Green => "vert",
            Accent::Graphite => "graphite",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Accent::Multicolor => "Multicolore",
            Accent::Blue => "Bleu",
            Accent::Purple => "Violet",
            Accent::Pink => "Rose",
            Accent::Red => "Rouge",
            Accent::Orange => "Orange",
            Accent::Yellow => "Jaune",
            Accent::Green => "Vert",
            Accent::Graphite => "Graphite",
        }
    }

    /// Couleur `0xRRGGBB` ; `None` pour « Multicolore ».
    pub fn rgb(self) -> Option<u32> {
        match self {
            Accent::Multicolor => None,
            Accent::Blue => Some(0x007AFF),
            Accent::Purple => Some(0x953D96),
            Accent::Pink => Some(0xF74F9E),
            Accent::Red => Some(0xE0383E),
            Accent::Orange => Some(0xF7821B),
            Accent::Yellow => Some(0xFFC600),
            Accent::Green => Some(0x62BA46),
            Accent::Graphite => Some(0x8C8C8C),
        }
    }
}

/// Affichage des barres de défilement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollBars {
    /// Fines et toujours visibles avec une souris, comme sur un PC.
    #[default]
    Auto,
    /// Seulement quand la souris survole la zone qui défile.
    WhenScrolling,
    Always,
}

impl ScrollBars {
    pub const ALL: [ScrollBars; 3] = [ScrollBars::Auto, ScrollBars::WhenScrolling, ScrollBars::Always];

    fn parse(value: &str) -> Self {
        match value {
            "defilement" => ScrollBars::WhenScrolling,
            "toujours" => ScrollBars::Always,
            _ => ScrollBars::Auto,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ScrollBars::Auto => "auto",
            ScrollBars::WhenScrolling => "defilement",
            ScrollBars::Always => "toujours",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ScrollBars::Auto => "Automatiquement selon la souris ou le pavé tactile",
            ScrollBars::WhenScrolling => "Pendant le défilement",
            ScrollBars::Always => "Toujours",
        }
    }
}

/// Liste séparée par des virgules, sans éléments vides.
fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_height(value: Option<&str>, default: u32, min: u32, max: u32) -> u32 {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
        .clamp(min, max)
}

/// Sépare « ch(fr) » en disposition « ch » et variante « fr » pour XKB.
pub fn split_layout(layout: &str) -> (String, String) {
    match layout.split_once('(') {
        Some((base, variant)) => (base.to_owned(), variant.trim_end_matches(')').to_owned()),
        None => (layout.to_owned(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reglages_d_apparence() {
        let defaults = Settings::default();
        assert_eq!(defaults.appearance(), Appearance::Light);
        assert_eq!(defaults.accent(), Accent::Multicolor);
        assert_eq!(defaults.highlight(), None);
        assert_eq!(defaults.sidebar_icons(), DockSize::Medium);
        assert_eq!(defaults.scroll_bars(), ScrollBars::Auto);
        let chosen = Settings::parse(
            "apparence = sombre\naccent = violet\nsurlignage = vert\nbarre_laterale_icones = petit\nbarres_defilement = toujours\n",
        );
        assert_eq!(chosen.appearance(), Appearance::Dark);
        assert_eq!(chosen.accent(), Accent::Purple);
        assert_eq!(chosen.highlight(), Some(Accent::Green));
        assert_eq!(chosen.sidebar_icons(), DockSize::Small);
        assert_eq!(chosen.scroll_bars(), ScrollBars::Always);
        assert_eq!(Settings::parse("surlignage = accent\n").highlight(), None);
        for accent in Accent::ALL {
            assert_eq!(Accent::parse(accent.key()), accent);
        }
    }

    #[test]
    fn apparence_auto_suit_une_plage_nocturne() {
        assert!(Appearance::is_dark_at_hour(0));
        assert!(Appearance::is_dark_at_hour(6));
        assert!(!Appearance::is_dark_at_hour(7));
        assert!(!Appearance::is_dark_at_hour(18));
        assert!(Appearance::is_dark_at_hour(19));
        assert!(Appearance::is_dark_at_hour(23));
    }

    #[test]
    fn lecture_des_reglages() {
        let settings = Settings::parse("# commentaire\nclavier = fr\n\nmal formé\n = vide\n");
        assert_eq!(settings.keyboard_layout(), "fr");
        assert_eq!(Settings::default().keyboard_layout(), DEFAULT_KEYBOARD_LAYOUT);
        assert_eq!(split_layout("ch(fr)"), ("ch".into(), "fr".into()));
        assert_eq!(split_layout("us"), ("us".into(), String::new()));
        let dock = Settings::parse(
            "dock_taille = grand
dock_masquage = jamais
",
        );
        assert_eq!(dock.dock_size(), DockSize::Large);
        assert_eq!(dock.dock_hiding(), DockHiding::Never);
        assert_eq!(Settings::default().dock_hiding(), DockHiding::Smart);
        assert_eq!(Settings::default().bar_hiding(), DockHiding::Never);
        assert_eq!(Settings::default().dock_pinned(), None);
        assert_eq!(Settings::default().multi_window_apps(), ["ferrite-files"]);
        assert!(Settings::default().dock_magnify());
        let mut custom = Settings::default();
        custom.set_dock_pinned(&["ferrite-files".into(), "vlc".into()]);
        custom.set_multi_window_apps(&[]);
        let custom = Settings::parse(&format!(
            "dock_epingles = {}
fenetres_multiples = {}
dock_agrandissement = non
",
            custom.get("dock_epingles").unwrap(),
            custom.get("fenetres_multiples").unwrap()
        ));
        assert_eq!(
            custom.dock_pinned(),
            Some(vec!["ferrite-files".into(), "vlc".into()])
        );
        assert!(custom.multi_window_apps().is_empty());
        assert!(!custom.dock_magnify());
        assert_eq!(
            Settings::parse("barre_masquage = toujours\n").bar_hiding(),
            DockHiding::Always
        );
        assert_eq!(DockSize::parse(DockSize::Small.key()), DockSize::Small);
    }
}
