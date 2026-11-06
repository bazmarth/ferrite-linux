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

pub const SHORTCUT_CTRL: u8 = 1;
pub const SHORTCUT_ALT: u8 = 2;
pub const SHORTCUT_SHIFT: u8 = 4;
pub const SHORTCUT_SUPER: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardShortcutBinding {
    pub key_code: u32,
    pub modifiers: u8,
}

impl KeyboardShortcutBinding {
    pub const fn new(key_code: u32, modifiers: u8) -> Self {
        Self { key_code, modifiers }
    }

    pub fn label(self) -> String {
        let mut parts = Vec::new();
        if self.modifiers & SHORTCUT_CTRL != 0 {
            parts.push("Ctrl");
        }
        if self.modifiers & SHORTCUT_ALT != 0 {
            parts.push("Alt");
        }
        if self.modifiers & SHORTCUT_SHIFT != 0 {
            parts.push("Maj");
        }
        if self.modifiers & SHORTCUT_SUPER != 0 {
            parts.push("Super");
        }
        parts.push(keyboard_shortcut_key_label(self.key_code));
        parts.join(" + ")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KeyboardShortcutDefinition {
    pub id: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub defaults: &'static [KeyboardShortcutBinding],
}

const CTRL_ALT: u8 = SHORTCUT_CTRL | SHORTCUT_ALT;
const SUPER: u8 = SHORTCUT_SUPER;
const SUPER_CTRL: u8 = SHORTCUT_SUPER | SHORTCUT_CTRL;
const SUPER_SHIFT: u8 = SHORTCUT_SUPER | SHORTCUT_SHIFT;
const SUPER_ALT: u8 = SHORTCUT_SUPER | SHORTCUT_ALT;

const CLOSE_WINDOW: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(24, SUPER)];
const MINIMIZE_WINDOW: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(58, SUPER)];
const HIDE_WINDOW: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(43, SUPER)];
const MAXIMIZE_WINDOW: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(111, SUPER)];
const RESTORE_MINIMIZE_WINDOW: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(116, SUPER)];
const SNAP_LEFT: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(113, SUPER)];
const SNAP_RIGHT: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(114, SUPER)];
const SNAP_CENTER: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(54, SUPER)];
const TOGGLE_TILING: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(28, SUPER)];
const SUPER_ENTER: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(36, SUPER)];
const SWITCHER_NEXT: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(23, SHORTCUT_ALT)];
const SWITCHER_PREVIOUS: &[KeyboardShortcutBinding] =
    &[KeyboardShortcutBinding::new(23, SHORTCUT_ALT | SHORTCUT_SHIFT)];
const SWITCHER_SAME_APP_NEXT: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(49, SHORTCUT_ALT)];
const SWITCHER_SAME_APP_PREVIOUS: &[KeyboardShortcutBinding] =
    &[KeyboardShortcutBinding::new(49, SHORTCUT_ALT | SHORTCUT_SHIFT)];
const SWITCH_PAGE_PREVIOUS: &[KeyboardShortcutBinding] = &[
    KeyboardShortcutBinding::new(113, CTRL_ALT),
    KeyboardShortcutBinding::new(113, SUPER_CTRL),
];
const SWITCH_PAGE_NEXT: &[KeyboardShortcutBinding] = &[
    KeyboardShortcutBinding::new(114, CTRL_ALT),
    KeyboardShortcutBinding::new(114, SUPER_CTRL),
];
const OVERVIEW_OPEN: &[KeyboardShortcutBinding] = &[
    KeyboardShortcutBinding::new(111, CTRL_ALT),
    KeyboardShortcutBinding::new(111, SUPER_CTRL),
];
const OVERVIEW_TOGGLE: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(39, SUPER)];
const OVERVIEW_CLOSE: &[KeyboardShortcutBinding] = &[
    KeyboardShortcutBinding::new(116, CTRL_ALT),
    KeyboardShortcutBinding::new(116, SUPER_CTRL),
];
const TOGGLE_CONTROLS: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(38, SUPER)];
const TOGGLE_NOTIFICATIONS: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(57, SUPER)];
const TOGGLE_CLIPBOARD: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(55, SUPER)];
const TOGGLE_WIDGETS: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(25, SUPER)];
const TOGGLE_KEYBOARD_LAYOUT: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(65, SUPER)];
const SHOW_DESKTOP: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(40, SUPER)];
const LOCK_SCREEN: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(46, SUPER)];
const POWER_DIALOG: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(9, SUPER)];
const OPEN_TERMINAL: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(28, CTRL_ALT)];
const QUIT_SHELL: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(22, CTRL_ALT)];
const RESTORE_MINIMIZED: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(58, CTRL_ALT)];
const SCREENSHOT: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(107, 0)];
const SCREENSHOT_REGION: &[KeyboardShortcutBinding] = &[KeyboardShortcutBinding::new(107, SHORTCUT_SHIFT)];

pub const KEYBOARD_SHORTCUTS: &[KeyboardShortcutDefinition] = &[
    KeyboardShortcutDefinition {
        id: "close_window",
        group: "Fenêtres",
        label: "Fermer la fenêtre",
        defaults: CLOSE_WINDOW,
    },
    KeyboardShortcutDefinition {
        id: "minimize_window",
        group: "Fenêtres",
        label: "Réduire la fenêtre",
        defaults: MINIMIZE_WINDOW,
    },
    KeyboardShortcutDefinition {
        id: "hide_window",
        group: "Fenêtres",
        label: "Masquer l’application",
        defaults: HIDE_WINDOW,
    },
    KeyboardShortcutDefinition {
        id: "maximize_window",
        group: "Fenêtres",
        label: "Agrandir ou restaurer",
        defaults: MAXIMIZE_WINDOW,
    },
    KeyboardShortcutDefinition {
        id: "restore_minimize_window",
        group: "Fenêtres",
        label: "Restaurer ou réduire",
        defaults: RESTORE_MINIMIZE_WINDOW,
    },
    KeyboardShortcutDefinition {
        id: "snap_left",
        group: "Fenêtres",
        label: "Accrocher à gauche",
        defaults: SNAP_LEFT,
    },
    KeyboardShortcutDefinition {
        id: "snap_right",
        group: "Fenêtres",
        label: "Accrocher à droite",
        defaults: SNAP_RIGHT,
    },
    KeyboardShortcutDefinition {
        id: "snap_center",
        group: "Fenêtres",
        label: "Centrer la fenêtre",
        defaults: SNAP_CENTER,
    },
    KeyboardShortcutDefinition {
        id: "toggle_tiling",
        group: "Fenêtres",
        label: "Activer ou arrêter le tuilage",
        defaults: TOGGLE_TILING,
    },
    KeyboardShortcutDefinition {
        id: "super_enter",
        group: "Fenêtres",
        label: "Échanger avec la fenêtre principale",
        defaults: SUPER_ENTER,
    },
    KeyboardShortcutDefinition {
        id: "switcher_next",
        group: "Fenêtres",
        label: "Fenêtre suivante",
        defaults: SWITCHER_NEXT,
    },
    KeyboardShortcutDefinition {
        id: "switcher_previous",
        group: "Fenêtres",
        label: "Fenêtre précédente",
        defaults: SWITCHER_PREVIOUS,
    },
    KeyboardShortcutDefinition {
        id: "switcher_same_app_next",
        group: "Fenêtres",
        label: "Fenêtre suivante de l’application",
        defaults: SWITCHER_SAME_APP_NEXT,
    },
    KeyboardShortcutDefinition {
        id: "switcher_same_app_previous",
        group: "Fenêtres",
        label: "Fenêtre précédente de l’application",
        defaults: SWITCHER_SAME_APP_PREVIOUS,
    },
    KeyboardShortcutDefinition {
        id: "switch_page_previous",
        group: "Espaces de travail",
        label: "Espace précédent",
        defaults: SWITCH_PAGE_PREVIOUS,
    },
    KeyboardShortcutDefinition {
        id: "switch_page_next",
        group: "Espaces de travail",
        label: "Espace suivant",
        defaults: SWITCH_PAGE_NEXT,
    },
    KeyboardShortcutDefinition {
        id: "overview_open",
        group: "Espaces de travail",
        label: "Ouvrir la vue d’ensemble",
        defaults: OVERVIEW_OPEN,
    },
    KeyboardShortcutDefinition {
        id: "overview_toggle",
        group: "Espaces de travail",
        label: "Afficher ou masquer la vue d’ensemble",
        defaults: OVERVIEW_TOGGLE,
    },
    KeyboardShortcutDefinition {
        id: "overview_close",
        group: "Espaces de travail",
        label: "Fermer la vue d’ensemble",
        defaults: OVERVIEW_CLOSE,
    },
    KeyboardShortcutDefinition {
        id: "send_space_1",
        group: "Espaces de travail",
        label: "Envoyer la fenêtre sur l’espace 1",
        defaults: &[KeyboardShortcutBinding::new(10, SUPER_SHIFT)],
    },
    KeyboardShortcutDefinition {
        id: "send_space_2",
        group: "Espaces de travail",
        label: "Envoyer la fenêtre sur l’espace 2",
        defaults: &[KeyboardShortcutBinding::new(11, SUPER_SHIFT)],
    },
    KeyboardShortcutDefinition {
        id: "send_space_3",
        group: "Espaces de travail",
        label: "Envoyer la fenêtre sur l’espace 3",
        defaults: &[KeyboardShortcutBinding::new(12, SUPER_SHIFT)],
    },
    KeyboardShortcutDefinition {
        id: "send_space_4",
        group: "Espaces de travail",
        label: "Envoyer la fenêtre sur l’espace 4",
        defaults: &[KeyboardShortcutBinding::new(13, SUPER_SHIFT)],
    },
    KeyboardShortcutDefinition {
        id: "send_space_5",
        group: "Espaces de travail",
        label: "Envoyer la fenêtre sur l’espace 5",
        defaults: &[KeyboardShortcutBinding::new(14, SUPER_SHIFT)],
    },
    KeyboardShortcutDefinition {
        id: "send_space_6",
        group: "Espaces de travail",
        label: "Envoyer la fenêtre sur l’espace 6",
        defaults: &[KeyboardShortcutBinding::new(15, SUPER_SHIFT)],
    },
    KeyboardShortcutDefinition {
        id: "send_space_7",
        group: "Espaces de travail",
        label: "Envoyer la fenêtre sur l’espace 7",
        defaults: &[KeyboardShortcutBinding::new(16, SUPER_SHIFT)],
    },
    KeyboardShortcutDefinition {
        id: "send_space_8",
        group: "Espaces de travail",
        label: "Envoyer la fenêtre sur l’espace 8",
        defaults: &[KeyboardShortcutBinding::new(17, SUPER_SHIFT)],
    },
    KeyboardShortcutDefinition {
        id: "send_space_9",
        group: "Espaces de travail",
        label: "Envoyer la fenêtre sur l’espace 9",
        defaults: &[KeyboardShortcutBinding::new(18, SUPER_SHIFT)],
    },
    KeyboardShortcutDefinition {
        id: "snap_third_1",
        group: "Fenêtres",
        label: "Accrocher au premier tiers",
        defaults: &[KeyboardShortcutBinding::new(10, SUPER_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "snap_third_2",
        group: "Fenêtres",
        label: "Accrocher au deuxième tiers",
        defaults: &[KeyboardShortcutBinding::new(11, SUPER_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "snap_third_3",
        group: "Fenêtres",
        label: "Accrocher au troisième tiers",
        defaults: &[KeyboardShortcutBinding::new(12, SUPER_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "toggle_controls",
        group: "Bureau",
        label: "Centre de contrôle",
        defaults: TOGGLE_CONTROLS,
    },
    KeyboardShortcutDefinition {
        id: "toggle_notifications",
        group: "Bureau",
        label: "Calendrier et notifications",
        defaults: TOGGLE_NOTIFICATIONS,
    },
    KeyboardShortcutDefinition {
        id: "toggle_clipboard",
        group: "Bureau",
        label: "Historique du presse-papiers",
        defaults: TOGGLE_CLIPBOARD,
    },
    KeyboardShortcutDefinition {
        id: "toggle_widgets",
        group: "Bureau",
        label: "Afficher ou masquer les widgets",
        defaults: TOGGLE_WIDGETS,
    },
    KeyboardShortcutDefinition {
        id: "toggle_keyboard_layout",
        group: "Bureau",
        label: "Changer de disposition clavier",
        defaults: TOGGLE_KEYBOARD_LAYOUT,
    },
    KeyboardShortcutDefinition {
        id: "show_desktop",
        group: "Bureau",
        label: "Afficher le bureau",
        defaults: SHOW_DESKTOP,
    },
    KeyboardShortcutDefinition {
        id: "lock_screen",
        group: "Bureau",
        label: "Verrouiller la session",
        defaults: LOCK_SCREEN,
    },
    KeyboardShortcutDefinition {
        id: "power_dialog",
        group: "Bureau",
        label: "Dialogue d’arrêt",
        defaults: POWER_DIALOG,
    },
    KeyboardShortcutDefinition {
        id: "open_terminal",
        group: "Bureau",
        label: "Ouvrir un terminal",
        defaults: OPEN_TERMINAL,
    },
    KeyboardShortcutDefinition {
        id: "quit_shell",
        group: "Bureau",
        label: "Quitter Ferrite",
        defaults: QUIT_SHELL,
    },
    KeyboardShortcutDefinition {
        id: "restore_minimized",
        group: "Fenêtres",
        label: "Restaurer la fenêtre réduite",
        defaults: RESTORE_MINIMIZED,
    },
    KeyboardShortcutDefinition {
        id: "screenshot",
        group: "Bureau",
        label: "Capture de tout l’écran",
        defaults: SCREENSHOT,
    },
    KeyboardShortcutDefinition {
        id: "screenshot_region",
        group: "Bureau",
        label: "Capture d’une zone",
        defaults: SCREENSHOT_REGION,
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_1",
        group: "Bureau",
        label: "Terminal virtuel 1",
        defaults: &[KeyboardShortcutBinding::new(67, CTRL_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_2",
        group: "Bureau",
        label: "Terminal virtuel 2",
        defaults: &[KeyboardShortcutBinding::new(68, CTRL_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_3",
        group: "Bureau",
        label: "Terminal virtuel 3",
        defaults: &[KeyboardShortcutBinding::new(69, CTRL_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_4",
        group: "Bureau",
        label: "Terminal virtuel 4",
        defaults: &[KeyboardShortcutBinding::new(70, CTRL_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_5",
        group: "Bureau",
        label: "Terminal virtuel 5",
        defaults: &[KeyboardShortcutBinding::new(71, CTRL_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_6",
        group: "Bureau",
        label: "Terminal virtuel 6",
        defaults: &[KeyboardShortcutBinding::new(72, CTRL_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_7",
        group: "Bureau",
        label: "Terminal virtuel 7",
        defaults: &[KeyboardShortcutBinding::new(73, CTRL_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_8",
        group: "Bureau",
        label: "Terminal virtuel 8",
        defaults: &[KeyboardShortcutBinding::new(74, CTRL_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_9",
        group: "Bureau",
        label: "Terminal virtuel 9",
        defaults: &[KeyboardShortcutBinding::new(75, CTRL_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_10",
        group: "Bureau",
        label: "Terminal virtuel 10",
        defaults: &[KeyboardShortcutBinding::new(76, CTRL_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_11",
        group: "Bureau",
        label: "Terminal virtuel 11",
        defaults: &[KeyboardShortcutBinding::new(95, CTRL_ALT)],
    },
    KeyboardShortcutDefinition {
        id: "switch_vt_12",
        group: "Bureau",
        label: "Terminal virtuel 12",
        defaults: &[KeyboardShortcutBinding::new(96, CTRL_ALT)],
    },
];

pub const KEYBOARD_SHORTCUT_KEYS: &[(u32, &str)] = &[
    (9, "Échap"),
    (10, "1"),
    (11, "2"),
    (12, "3"),
    (13, "4"),
    (14, "5"),
    (15, "6"),
    (16, "7"),
    (17, "8"),
    (18, "9"),
    (19, "0"),
    (22, "Retour arrière"),
    (23, "Tab"),
    (24, "Q"),
    (25, "W"),
    (26, "E"),
    (27, "R"),
    (28, "T"),
    (29, "Y"),
    (30, "U"),
    (31, "I"),
    (32, "O"),
    (33, "P"),
    (36, "Entrée"),
    (38, "A"),
    (39, "S"),
    (40, "D"),
    (41, "F"),
    (42, "G"),
    (43, "H"),
    (44, "J"),
    (45, "K"),
    (46, "L"),
    (49, "Accent grave"),
    (52, "Z"),
    (53, "X"),
    (54, "C"),
    (55, "V"),
    (56, "B"),
    (57, "N"),
    (58, "M"),
    (65, "Espace"),
    (67, "F1"),
    (68, "F2"),
    (69, "F3"),
    (70, "F4"),
    (71, "F5"),
    (72, "F6"),
    (73, "F7"),
    (74, "F8"),
    (75, "F9"),
    (76, "F10"),
    (95, "F11"),
    (96, "F12"),
    (107, "Impr. écran"),
    (111, "↑"),
    (113, "←"),
    (114, "→"),
    (116, "↓"),
];

pub fn keyboard_shortcut_key_label(key_code: u32) -> &'static str {
    KEYBOARD_SHORTCUT_KEYS
        .iter()
        .find(|(code, _)| *code == key_code)
        .map_or("Touche inconnue", |(_, label)| label)
}

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

    pub fn keyboard_shortcut_bindings(&self, id: &str) -> Vec<KeyboardShortcutBinding> {
        let Some(definition) = KEYBOARD_SHORTCUTS.iter().find(|shortcut| shortcut.id == id) else {
            return Vec::new();
        };
        let Some(value) = self.get(&format!("raccourci_{id}")) else {
            return definition.defaults.to_vec();
        };
        value
            .split(',')
            .filter_map(|binding| {
                let (key_code, modifiers) = binding.split_once(':')?;
                Some(KeyboardShortcutBinding::new(
                    key_code.parse().ok()?,
                    modifiers.parse().ok()?,
                ))
            })
            .collect()
    }

    pub fn set_keyboard_shortcut(&mut self, id: &str, binding: KeyboardShortcutBinding) {
        if KEYBOARD_SHORTCUTS.iter().any(|shortcut| shortcut.id == id) {
            self.set(
                &format!("raccourci_{id}"),
                &format!("{}:{}", binding.key_code, binding.modifiers),
            );
        }
    }

    pub fn reset_keyboard_shortcut(&mut self, id: &str) {
        self.values.remove(&format!("raccourci_{id}"));
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
        // L'ancien Terminal (foot) cède sa place au Terminal Ferrite.
        self.get("dock_epingles").map(|list| {
            split_list(list)
                .into_iter()
                .map(|app| {
                    if app == "foot" {
                        "ferrite-terminal".to_owned()
                    } else {
                        app
                    }
                })
                .collect()
        })
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
    /// Réglage oui/non ; absent, sa valeur par défaut.
    pub fn flag(&self, key: &str, default: bool) -> bool {
        match self.get(key) {
            Some("oui") => true,
            Some("non") => false,
            _ => default,
        }
    }

    pub fn set_flag(&mut self, key: &str, value: bool) {
        self.set(key, if value { "oui" } else { "non" });
    }

    /// Côté des icônes du Dock, en pixels logiques.
    pub fn dock_icon(&self) -> u32 {
        parse_height(self.get("dock_icones"), self.dock_size().icon() as u32, 40, 80)
    }

    pub fn clock_style(&self) -> ClockStyle {
        ClockStyle::parse(self.get("horloge").unwrap_or_default())
    }

    /// Action d'un coin actif : `hg`, `hd`, `bg` ou `bd`.
    pub fn hot_corner(&self, corner: &str) -> CornerAction {
        let default = match corner {
            "hg" => CornerAction::Overview,
            "bd" => CornerAction::ShowDesktop,
            _ => CornerAction::None,
        };
        self.get(&format!("coin_{corner}"))
            .map_or(default, CornerAction::parse)
    }

    pub fn double_click(&self) -> DoubleClick {
        DoubleClick::parse(self.get("double_clic").unwrap_or_default())
    }

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
        parse_height(self.get("dock_hauteur"), 89, 68, 120)
    }

    /// Hauteur de la barre des menus, en pixels logiques.
    pub fn bar_height(&self) -> u32 {
        parse_height(self.get("barre_hauteur"), 34, 24, 60)
    }

    pub fn scroll_bars(&self) -> ScrollBars {
        ScrollBars::parse(self.get("barres_defilement").unwrap_or_default())
    }

    /// Inversion de la molette ; activée par défaut pour conserver le comportement historique.
    pub fn scroll_inverted(&self) -> bool {
        self.get("defilement_inverse") != Some("non")
    }

    /// Fond d'écran : nom d'un fond de Ferrite, ou chemin absolu d'une image (vide : le
    /// fond Ferrite).
    pub fn wallpaper(&self) -> &str {
        self.get("fond_ecran").unwrap_or_default()
    }

    /// Comportement du bouton d’alimentation : veille par défaut, ou dialogue au choix.
    pub fn power_button_action(&self) -> PowerButtonAction {
        PowerButtonAction::parse(self.get("bouton_alimentation").unwrap_or_default())
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
            DockSize::Small => 48,
            DockSize::Medium => 58,
            DockSize::Large => 72,
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
    /// Le bleu Ferrite, plus clair en thème sombre.
    #[default]
    Blue,
    Teal,
    Green,
    Yellow,
    Orange,
    Red,
    Pink,
    Purple,
    Graphite,
}

impl Accent {
    pub const ALL: [Accent; 9] = [
        Accent::Blue,
        Accent::Teal,
        Accent::Green,
        Accent::Yellow,
        Accent::Orange,
        Accent::Red,
        Accent::Pink,
        Accent::Purple,
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
            Accent::Blue => "bleu",
            Accent::Teal => "turquoise",
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
            Accent::Blue => "Bleu",
            Accent::Teal => "Turquoise",
            Accent::Purple => "Violet",
            Accent::Pink => "Rose",
            Accent::Red => "Rouge",
            Accent::Orange => "Orange",
            Accent::Yellow => "Jaune",
            Accent::Green => "Vert",
            Accent::Graphite => "Graphite",
        }
    }

    /// Couleur `0xRRGGBB` ; `None` pour le bleu Ferrite, qui dépend du thème.
    pub fn rgb(self) -> Option<u32> {
        match self {
            Accent::Blue => None,
            Accent::Teal => Some(0x0E9AA7),
            Accent::Green => Some(0x1F9D63),
            Accent::Yellow => Some(0xE3A008),
            Accent::Orange => Some(0xE5571A),
            Accent::Red => Some(0xE5372B),
            Accent::Pink => Some(0xDB2C57),
            Accent::Purple => Some(0x7C4DE8),
            Accent::Graphite => Some(0x4B5563),
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

/// Texte de l'horloge de la barre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClockStyle {
    /// « lun. 2 nov.  17:24 »
    #[default]
    Full,
    /// « 2 nov.  17:24 »
    Date,
    /// « 17:24 »
    Time,
}

impl ClockStyle {
    pub const ALL: [ClockStyle; 3] = [ClockStyle::Full, ClockStyle::Date, ClockStyle::Time];

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|style| style.key() == value)
            .unwrap_or_default()
    }

    pub fn key(self) -> &'static str {
        match self {
            ClockStyle::Full => "complet",
            ClockStyle::Date => "date",
            ClockStyle::Time => "heure",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ClockStyle::Full => "Jour, date et heure",
            ClockStyle::Date => "Date et heure",
            ClockStyle::Time => "Heure seule",
        }
    }
}

/// Action d'un coin actif de l'écran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CornerAction {
    #[default]
    None,
    Overview,
    ShowDesktop,
    Launcher,
    ControlCenter,
}

impl CornerAction {
    pub const ALL: [CornerAction; 5] = [
        CornerAction::None,
        CornerAction::Overview,
        CornerAction::ShowDesktop,
        CornerAction::Launcher,
        CornerAction::ControlCenter,
    ];

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|action| action.key() == value)
            .unwrap_or_default()
    }

    pub fn key(self) -> &'static str {
        match self {
            CornerAction::None => "aucune",
            CornerAction::Overview => "vue",
            CornerAction::ShowDesktop => "bureau",
            CornerAction::Launcher => "lanceur",
            CornerAction::ControlCenter => "centre",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CornerAction::None => "Aucune action",
            CornerAction::Overview => "Vue d’ensemble",
            CornerAction::ShowDesktop => "Afficher le bureau",
            CornerAction::Launcher => "Lanceur",
            CornerAction::ControlCenter => "Centre de contrôle",
        }
    }
}

/// Double-clic sur la barre de titre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DoubleClick {
    #[default]
    Maximize,
    Minimize,
    Nothing,
}

impl DoubleClick {
    pub const ALL: [DoubleClick; 3] = [DoubleClick::Maximize, DoubleClick::Minimize, DoubleClick::Nothing];

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|action| action.key() == value)
            .unwrap_or_default()
    }

    pub fn key(self) -> &'static str {
        match self {
            DoubleClick::Maximize => "agrandir",
            DoubleClick::Minimize => "reduire",
            DoubleClick::Nothing => "rien",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DoubleClick::Maximize => "Agrandir",
            DoubleClick::Minimize => "Réduire",
            DoubleClick::Nothing => "Rien",
        }
    }
}

/// Action du bouton d’alimentation dans le bureau.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PowerButtonAction {
    #[default]
    Sleep,
    Dialog,
}

impl PowerButtonAction {
    pub const ALL: [PowerButtonAction; 2] = [PowerButtonAction::Sleep, PowerButtonAction::Dialog];

    fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|action| action.key() == value)
            .unwrap_or_default()
    }

    pub fn key(self) -> &'static str {
        match self {
            PowerButtonAction::Sleep => "veille",
            PowerButtonAction::Dialog => "dialogue",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PowerButtonAction::Sleep => "Mettre en veille",
            PowerButtonAction::Dialog => "Afficher le dialogue",
        }
    }
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
        assert_eq!(defaults.accent(), Accent::Blue);
        assert_eq!(defaults.highlight(), None);
        assert_eq!(defaults.sidebar_icons(), DockSize::Medium);
        assert_eq!(defaults.scroll_bars(), ScrollBars::Auto);
        assert!(defaults.scroll_inverted());
        let chosen = Settings::parse(
            "apparence = sombre\naccent = violet\nsurlignage = vert\nbarre_laterale_icones = petit\nbarres_defilement = toujours\ndefilement_inverse = non\n",
        );
        assert_eq!(chosen.appearance(), Appearance::Dark);
        assert_eq!(chosen.accent(), Accent::Purple);
        assert_eq!(chosen.highlight(), Some(Accent::Green));
        assert_eq!(chosen.sidebar_icons(), DockSize::Small);
        assert_eq!(chosen.scroll_bars(), ScrollBars::Always);
        assert!(!chosen.scroll_inverted());
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

    #[test]
    fn bouton_alimentation_en_veille_par_defaut_ou_dialogue_sur_demande() {
        assert_eq!(
            Settings::default().power_button_action(),
            PowerButtonAction::Sleep
        );
        assert_eq!(
            Settings::parse("bouton_alimentation = dialogue\n").power_button_action(),
            PowerButtonAction::Dialog
        );
        for action in PowerButtonAction::ALL {
            assert_eq!(PowerButtonAction::parse(action.key()), action);
        }
    }

    #[test]
    fn raccourcis_clavier_sont_modifiables_et_reinitialisables() {
        let defaults = Settings::default();
        assert_eq!(
            defaults.keyboard_shortcut_bindings("toggle_keyboard_layout"),
            [KeyboardShortcutBinding::new(65, SHORTCUT_SUPER)]
        );

        let mut settings = Settings::default();
        let binding = KeyboardShortcutBinding::new(24, SHORTCUT_CTRL | SHORTCUT_ALT);
        settings.set_keyboard_shortcut("toggle_keyboard_layout", binding);
        let parsed = Settings::parse(&format!(
            "raccourci_toggle_keyboard_layout = {}:{}",
            binding.key_code, binding.modifiers
        ));
        assert_eq!(
            parsed.keyboard_shortcut_bindings("toggle_keyboard_layout"),
            [binding]
        );
        settings.reset_keyboard_shortcut("toggle_keyboard_layout");
        assert_eq!(
            settings.keyboard_shortcut_bindings("toggle_keyboard_layout"),
            defaults.keyboard_shortcut_bindings("toggle_keyboard_layout")
        );
    }

    #[test]
    fn raccourcis_par_defaut_sans_conflit() {
        for (index, shortcut) in KEYBOARD_SHORTCUTS.iter().enumerate() {
            for other in &KEYBOARD_SHORTCUTS[index + 1..] {
                assert!(
                    shortcut
                        .defaults
                        .iter()
                        .all(|binding| !other.defaults.contains(binding)),
                    "conflit entre {} et {}",
                    shortcut.id,
                    other.id
                );
            }
        }
    }
}
