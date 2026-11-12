//! Icônes SVG de Ferrite (pack `ferrite-theme` et dossier `icons/SVG`), rasterisées avec resvg.
//!
//! Les icônes d'applications sont dessinées en couleur. Les icônes d'état, conçues en
//! couleur, sont aussi disponibles en version « symbolique » monochrome pour la barre
//! du haut et les menus : leurs tracés prennent une seule teinte, et les parties claires
//! et désaturées (barres Wi-Fi éteintes, rayons inactifs…) restent visibles en estompé.

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
};

use ferrite_ui::theme::Color;
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg::{Options, Tree},
};
use smithay::reexports::calloop::ping::Ping;

macro_rules! icons {
    ($($name:literal),* $(,)?; $($theme_name:literal => $theme_path:literal),* $(,)?) => {
        /// Icônes embarquées dans le binaire : (nom, source SVG). Le pack `ferrite-theme` fournit
        /// celles qui y existent, `icons/SVG` les autres (état, recherche).
        const SOURCES: &[(&str, &str)] = &[
            $(($name, include_str!(concat!("../../../icons/SVG/", $name, ".svg")))),*,
            $((
                $theme_name,
                include_str!(concat!("../../../ferrite-theme/icons/Ferrite/scalable/", $theme_path, ".svg")),
            )),*
        ];
    };
}

icons![
    "app-code",
    "app-files",
    "ferrite-logo",
    "ferrite-symbol",
    "ferrite-symbol-white",
    "app-search",
    "app-video",
    "status-battery-000",
    "status-battery-010",
    "status-battery-020",
    "status-battery-030",
    "status-battery-040",
    "status-battery-050",
    "status-battery-060",
    "status-battery-070",
    "status-battery-080",
    "status-battery-090",
    "status-battery-100",
    "status-battery-charging-000",
    "status-battery-charging-010",
    "status-battery-charging-020",
    "status-battery-charging-030",
    "status-battery-charging-040",
    "status-battery-charging-050",
    "status-battery-charging-060",
    "status-battery-charging-070",
    "status-battery-charging-080",
    "status-battery-charging-090",
    "status-battery-charging-100",
    "status-bluetooth-off",
    "status-bluetooth-on",
    "status-cloud",
    "status-download",
    "status-location",
    "status-network",
    "status-notifications",
    "status-power",
    "status-upload",
    "status-update",
    "status-volume-0",
    "status-volume-1",
    "status-volume-2",
    "status-volume-3",
    "status-wifi-0",
    "status-wifi-1",
    "status-wifi-2",
    "status-wifi-3",
    "status-wifi-off",
    "file-document",
    "file-spreadsheet",
    "file-presentation",
    ;
    "app-terminal" => "apps/utilities-terminal",
    "app-web" => "apps/web-browser",
    "app-settings" => "apps/preferences-system",
    "app-text-editor" => "apps/accessories-text-editor",
    "app-calculator" => "apps/accessories-calculator",
    "app-monitor" => "apps/utilities-system-monitor",
    "app-photos" => "apps/image-viewer",
    "app-pdf" => "mimetypes/application-pdf",
    "app-music" => "apps/multimedia-audio-player",
    "app-calendar" => "apps/x-office-calendar",
    "app-clock" => "apps/preferences-system-time",
    "app-store" => "apps/system-software-install",
    "file-generic" => "mimetypes/text-x-generic",
    "places-trash" => "places/user-trash",
    // Fichiers glissés depuis Fichiers (image sous le pointeur).
    "file-text" => "mimetypes/text-x-generic",
    "file-image" => "mimetypes/image-x-generic",
    "file-audio" => "mimetypes/audio-x-generic",
    "file-video" => "mimetypes/video-x-generic",
    "file-pdf" => "mimetypes/application-pdf",
    "file-archive" => "mimetypes/package-x-generic",
    "file-script" => "mimetypes/text-x-script",
    "file-executable" => "mimetypes/application-x-executable",
    "places-folder" => "places/folder",
    "places-desktop" => "places/user-desktop",
    "places-documents" => "places/folder-documents",
    "places-downloads" => "places/folder-download",
    "places-home" => "places/user-home",
    "places-music" => "places/folder-music",
    "places-pictures" => "places/folder-pictures",
    "places-public" => "places/folder-publicshare",
    "places-templates" => "places/folder-templates",
    "places-videos" => "places/folder-videos",
];

/// Nom d'icône embarquée correspondant à `name`, s'il en existe une.
pub fn known(name: &str) -> Option<&'static str> {
    SOURCES
        .iter()
        .map(|(known, _)| *known)
        .find(|known| *known == name)
}

/// Pictogrammes au trait des réglages rapides, dessinés en noir puis teintés comme les
/// icônes symboliques.
const GLYPHS: &[(&str, &str)] = &[
    (
        "cc-wifi",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 19L9.45 16.45A3.6 3.6 0 0 1 14.55 16.45Z M5.35 12.35A9.4 9.4 0 0 1 18.65 12.35L16.1 14.9A5.8 5.8 0 0 0 7.9 14.9Z M1.25 8.25A15.2 15.2 0 0 1 22.75 8.25L20.2 10.8A11.6 11.6 0 0 0 3.8 10.8Z" fill="#000" stroke="#000" stroke-width="0.8" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-bluetooth",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M7 7 L17 17 L12 21 V3 L17 7 L7 17" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-dark",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M20 14 A8 8 0 1 1 10 4 A6.5 6.5 0 0 0 20 14 Z" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-dnd",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3 a9 9 0 1 0 0.01 0 M8 12 H16" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-saver",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 8 H18 V16 H3 Z M20 11 V13 M9 9.5 L7 12.5 H11 L9 15" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-balanced",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3.5 17.5 A8.5 8.5 0 0 1 20.5 17.5 M12 17 V10.5" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><circle cx="12" cy="17" r="1.6" fill="#000"/></svg>"##,
    ),
    (
        "cc-performance",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3.5 17.5 A8.5 8.5 0 0 1 20.5 17.5 M12 17 L17 12.5" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><circle cx="12" cy="17" r="1.6" fill="#000"/></svg>"##,
    ),
    (
        "cc-screen",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 5 H21 V16 H3 Z M12 16 V20 M8 20 H16" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-keyboard",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 7 H21 V17 H3 Z M7 11 H7.01 M11 11 H11.01 M15 11 H15.01 M8 14 H16" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-keyboard-light",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 11 H21 V20 H3 Z M7 14.5 H7.01 M11 14.5 H11.01 M15 14.5 H15.01 M8 17.5 H16 M12 3 V6 M6.5 4.5 L8 7 M17.5 4.5 L16 7" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-volume",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M5.3 9.1H7.7L12.5 5.5V18.5L7.7 14.9H5.3Q3.6 14.9 3.6 13.2V10.8Q3.6 9.1 5.3 9.1Z M14.8 9.8A2.9 2.9 0 0 1 14.8 14.2 M16.5 7.8A5.5 5.5 0 0 1 16.5 16.2" fill="none" stroke="#000" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-mute",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M5.3 9.1H7.7L12.5 5.5V18.5L7.7 14.9H5.3Q3.6 14.9 3.6 13.2V10.8Q3.6 9.1 5.3 9.1Z M15.6 9.8L19.9 14.2M19.9 9.8L15.6 14.2" fill="none" stroke="#000" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-mic",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M9 6 a3 3 0 0 1 6 0 V12 a3 3 0 0 1 -6 0 Z M5.5 11.5 a6.5 6.5 0 0 0 13 0 M12 18 V21" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-mic-off",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M9 6 a3 3 0 0 1 6 0 V12 a3 3 0 0 1 -6 0 Z M5.5 11.5 a6.5 6.5 0 0 0 13 0 M12 18 V21 M4 4 L20 20" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-airplane",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3 a1.5 1.5 0 0 1 1.5 1.5 V9.5 L21 14 V16 L13.5 13.5 V18 L16 20 V21.5 L12 20.5 L8 21.5 V20 L10.5 18 V13.5 L3 16 V14 L10.5 9.5 V4.5 A1.5 1.5 0 0 1 12 3 Z" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-brightness",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 8 a4 4 0 1 0 0.01 0 M12 2.5 V4.5 M12 19.5 V21.5 M2.5 12 H4.5 M19.5 12 H21.5 M5.3 5.3 L6.7 6.7 M17.3 17.3 L18.7 18.7 M5.3 18.7 L6.7 17.3 M17.3 6.7 L18.7 5.3" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-settings",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 8.8 a3.2 3.2 0 1 0 0.01 0 M12 2.5 V5.5 M12 18.5 V21.5 M2.5 12 H5.5 M18.5 12 H21.5 M5.3 5.3 L7.4 7.4 M16.6 16.6 L18.7 18.7 M5.3 18.7 L7.4 16.6 M16.6 7.4 L18.7 5.3" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-lock",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 11 H18 V20 H6 Z M8.5 11 V8 a3.5 3.5 0 0 1 7 0 V11" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-account",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 4 a4 4 0 1 0 0.01 0 M4 21 a8 8 0 0 1 16 0" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-eye",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M2.5 12 C5 7 8.5 5 12 5 C15.5 5 19 7 21.5 12 C19 17 15.5 19 12 19 C8.5 19 5 17 2.5 12 Z M12 9 a3 3 0 1 0 0.01 0" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-eye-off",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M2.5 12 C5 7 8.5 5 12 5 C15.5 5 19 7 21.5 12 C19 17 15.5 19 12 19 C8.5 19 5 17 2.5 12 Z M12 9 a3 3 0 1 0 0.01 0 M4 4 L20 20" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-night",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M18 14.5 A7.5 7.5 0 1 1 9.5 6 A6 6 0 0 0 18 14.5 Z M17 3 V6 M15.5 4.5 H18.5 M20.5 8.5 V10.5 M19.5 9.5 H21.5" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-restart",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M20 12 a8 8 0 1 1 -2.3 -5.7 M20 4 V9 H15" fill="none" stroke="#000" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-logout",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M10 4 H5 V20 H10 M15 8 L19 12 L15 16 M19 12 H9" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-power",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3 V11 M7 6.5 a7.5 7.5 0 1 0 10 0" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-back",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M15 4 L7 12 L15 20" fill="none" stroke="#000" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-check",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 12.5 L9.5 18 L20 6.5" fill="none" stroke="#000" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-head",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 15 V12 a8 8 0 0 1 16 0 V15 M4 14 H7 V20 H4 Z M17 14 H20 V20 H17 Z" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-mouse",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 3 a6 6 0 0 1 6 6 V15 a6 6 0 0 1 -12 0 V9 a6 6 0 0 1 6 -6 Z M12 3 V9" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-phone",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M8 3 H16 V21 H8 Z M11 18 H13" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-speaker",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M7 3 H17 V21 H7 Z M12 15 a3 3 0 1 0 0.01 0 M12 7 V7.1" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-watch",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M8 7 H16 V17 H8 Z M9.5 7 L10 3 H14 L14.5 7 M9.5 17 L10 21 H14 L14.5 17" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-chevron",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M9 4 L17 12 L9 20" fill="none" stroke="#000" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "cc-search",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M10.3 3.8 a6.5 6.5 0 1 0 0.01 0 M15.5 15.5 L21 21" fill="none" stroke="#000" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-wifi",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 19L9.45 16.45A3.6 3.6 0 0 1 14.55 16.45Z M5.35 12.35A9.4 9.4 0 0 1 18.65 12.35L16.1 14.9A5.8 5.8 0 0 0 7.9 14.9Z M1.25 8.25A15.2 15.2 0 0 1 22.75 8.25L20.2 10.8A11.6 11.6 0 0 0 3.8 10.8Z" fill="#000" stroke="#000" stroke-width="0.8" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-bluetooth",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6.8 7.6 17 16.4l-5 4.6V3l5 4.6-10.2 8.8" fill="none" stroke="#000" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "glyph-airplane",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M21 16v-2l-8-5V3.5a1.5 1.5 0 0 0-3 0V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5z" fill="#000"/></svg>"##,
    ),
    (
        "glyph-sun",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4.2" fill="#000"/><g stroke="#000" stroke-width="2" stroke-linecap="round"><path d="M12 2.5v2.2M12 19.3v2.2M2.5 12h2.2M19.3 12h2.2M5.3 5.3l1.6 1.6M17.1 17.1l1.6 1.6M5.3 18.7l1.6-1.6M17.1 6.9l1.6-1.6"/></g></svg>"##,
    ),
    (
        "glyph-speaker",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 9h4l5-4.2v14.4L7 15H3z" fill="#000"/><path d="M15.4 9a4.2 4.2 0 0 1 0 6M18.2 6.2a8.2 8.2 0 0 1 0 11.6" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round"/></svg>"##,
    ),
    (
        "glyph-clipboard",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="5" y="4.2" width="14" height="17" rx="2.6" fill="none" stroke="#000" stroke-width="2"/><rect x="8.5" y="2.5" width="7" height="4" rx="1.3" fill="#000"/><path d="M8.6 11.2h6.8M8.6 15.2h4.6" stroke="#000" stroke-width="2" stroke-linecap="round"/></svg>"##,
    ),
    (
        "glyph-gear",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.6" fill="none" stroke="#000" stroke-width="2.8" stroke-dasharray="3.4 3.35"/><circle cx="12" cy="12" r="5.9" fill="none" stroke="#000" stroke-width="3.2"/></svg>"##,
    ),
    (
        "clip-code",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M8 8 L4 12 L8 16 M16 8 L20 12 L16 16 M13.5 6 L10.5 18" fill="none" stroke="#000" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"/></svg>"##,
    ),
    (
        "clip-link",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M10 14 a4 4 0 0 0 5.7 0 L19 10.7 a4 4 0 0 0 -5.7 -5.7 L12 6.3 M14 10 a4 4 0 0 0 -5.7 0 L5 13.3 a4 4 0 0 0 5.7 5.7 L12 17.7" fill="none" stroke="#000" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"/></svg>"##,
    ),
    (
        "clip-text",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M5 6 H19 M5 10 H19 M5 14 H19 M5 18 H13" fill="none" stroke="#000" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.9"/></svg>"##,
    ),
    (
        "clip-pin",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M9 3 H15 L14 9 L18 13 H6 L10 9 Z M12 13 V21" fill="none" stroke="#000" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8"/></svg>"##,
    ),
    (
        "clip-pin-on",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M9 3 H15 L14 9 L18 13 H6 L10 9 Z M12 13 V21" fill="#000" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "clip-close",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><path d="M2.5 2.5 L9.5 9.5 M9.5 2.5 L2.5 9.5" fill="none" stroke="#000" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.7"/></svg>"##,
    ),
    (
        "clip-sun",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4.5" fill="none" stroke="#000" stroke-width="2"/><path d="M12 2.5 V4.5 M12 19.5 V21.5 M2.5 12 H4.5 M19.5 12 H21.5 M5.3 5.3 L6.7 6.7 M17.3 17.3 L18.7 18.7 M5.3 18.7 L6.7 17.3 M17.3 6.7 L18.7 5.3" fill="none" stroke="#000" stroke-linecap="round" stroke-linejoin="round" stroke-width="2"/></svg>"##,
    ),
    (
        "clip-moon",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M20 14 A8 8 0 1 1 10 4 A6.5 6.5 0 0 0 20 14 Z" fill="none" stroke="#000" stroke-linecap="round" stroke-linejoin="round" stroke-width="2"/></svg>"##,
    ),
    (
        "clip-search",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14"><circle cx="6" cy="6" r="4.2" fill="none" stroke="#000" stroke-width="1.6"/><path d="M9.2 9.2 L12.5 12.5" fill="none" stroke="#000" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.6"/></svg>"##,
    ),
    (
        "clip-empty",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M8 4 H16 V7 H8 Z M6 5.5 H5 V21 H19 V5.5 H18" fill="none" stroke="#000" stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8"/></svg>"##,
    ),
];

/// Rendu d'une icône : couleurs d'origine ou teinte unique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style {
    Color,
    Symbolic(Color),
}

/// Clé du cache : nom, côté en pixels et style.
type CacheKey = (&'static str, u32, Style);

/// Clé du cache des fichiers : chemin et côté en pixels.
type FileKey = (PathBuf, u32);

/// Icônes déjà rasterisées.
#[derive(Debug, Default)]
pub struct IconCache {
    pixmaps: RefCell<HashMap<CacheKey, Option<Pixmap>>>,
    /// Icônes des applications installées, lues sur le disque.
    files: RefCell<HashMap<FileKey, Option<Pixmap>>>,
    /// Fil « icônes », démarré à la première demande en arrière-plan.
    background: RefCell<Option<Background>>,
    /// Relance le rendu quand une icône demandée en arrière-plan est prête.
    waker: RefCell<Option<Ping>>,
}

/// Icône à préparer hors du fil du compositeur.
#[derive(Debug)]
enum Job {
    Builtin(CacheKey),
    File(FileKey),
}

/// Icône préparée par le fil « icônes », pas encore rangée dans le cache.
#[derive(Debug)]
enum Done {
    Builtin(CacheKey, Option<Pixmap>),
    File(FileKey, Option<Pixmap>),
}

/// Liaison avec le fil « icônes » : lecture du disque et dessin du SVG s'y font sans
/// retarder l'image en cours.
#[derive(Debug)]
struct Background {
    jobs: mpsc::Sender<(Job, Option<Ping>)>,
    done: mpsc::Receiver<Done>,
}

impl Background {
    fn start() -> Option<Self> {
        let (jobs, job_receiver) = mpsc::channel::<(Job, Option<Ping>)>();
        let (done_sender, done) = mpsc::channel();
        std::thread::Builder::new()
            .name("icônes".to_owned())
            .spawn(move || {
                // S'arrête quand le cache disparaît (canal des demandes fermé).
                for (job, waker) in job_receiver {
                    let done = match job {
                        Job::Builtin(key @ (name, side, style)) => {
                            Done::Builtin(key, rasterize(name, side, style))
                        }
                        Job::File(key) => {
                            let pixmap = rasterize_file(&key.0, key.1);
                            Done::File(key, pixmap)
                        }
                    };
                    if done_sender.send(done).is_err() {
                        break;
                    }
                    if let Some(waker) = waker {
                        waker.ping();
                    }
                }
            })
            .inspect_err(|err| tracing::warn!(%err, "fil des icônes non démarré"))
            .ok()?;
        Some(Self { jobs, done })
    }
}

impl IconCache {
    /// Donne de quoi relancer le rendu quand une icône préparée en arrière-plan est prête.
    pub fn set_waker(&self, waker: Option<&Ping>) {
        let mut known = self.waker.borrow_mut();
        if known.is_none() {
            *known = waker.cloned();
        }
    }

    /// Demande l'icône `name` à `size` pixels au fil « icônes ». Rien n'est dessiné ici :
    /// [`IconCache::is_ready`] dit quand elle est prête. Sans fil, elle est préparée sur
    /// place.
    pub fn prepare_in_background(&self, name: &'static str, size: f32, style: Style) {
        let key = (name, side_of(size), style);
        if self.pixmaps.borrow().contains_key(&key) {
            return;
        }
        if !self.send(Job::Builtin(key)) {
            self.prepare(name, size, style);
        }
    }

    /// Comme [`IconCache::prepare_in_background`], pour un fichier d'icône du système.
    pub fn prepare_file_in_background(&self, path: &Path, size: f32) {
        let key = (path.to_path_buf(), side_of(size));
        if self.files.borrow().contains_key(&key) {
            return;
        }
        if !self.send(Job::File(key)) {
            self.prepare_file(path, size);
        }
    }

    /// Vrai si l'icône `name` est prête à `size` pixels (lisible ou non).
    pub fn is_ready(&self, name: &'static str, size: f32, style: Style) -> bool {
        self.collect();
        self.pixmaps.borrow().contains_key(&(name, side_of(size), style))
    }

    /// Comme [`IconCache::is_ready`], pour un fichier d'icône du système.
    pub fn is_file_ready(&self, path: &Path, size: f32) -> bool {
        self.collect();
        self.files
            .borrow()
            .contains_key(&(path.to_path_buf(), side_of(size)))
    }

    /// Envoie une demande au fil « icônes » (démarré au besoin). Faux s'il est indisponible.
    fn send(&self, job: Job) -> bool {
        let mut background = self.background.borrow_mut();
        if background.is_none() {
            *background = Background::start();
        }
        let waker = self.waker.borrow().clone();
        background
            .as_ref()
            .is_some_and(|background| background.jobs.send((job, waker)).is_ok())
    }

    /// Range dans le cache les icônes que le fil « icônes » a terminées. Une icône déjà
    /// dessinée sur place entre-temps garde sa version.
    fn collect(&self) {
        let background = self.background.borrow();
        let Some(background) = background.as_ref() else {
            return;
        };
        for done in background.done.try_iter() {
            match done {
                Done::Builtin(key, pixmap) => {
                    self.pixmaps.borrow_mut().entry(key).or_insert(pixmap);
                }
                Done::File(key, pixmap) => {
                    self.files.borrow_mut().entry(key).or_insert(pixmap);
                }
            }
        }
    }

    /// Dessine l'icône `name` dans un tampon BGRA prémultiplié (`Argb8888`), son coin
    /// haut-gauche en (`x`, `y`) et de côté `size`, en pixels physiques.
    #[allow(clippy::too_many_arguments)] // Tampon, nom, géométrie et style, sans état global.
    pub fn draw(
        &self,
        pixels: &mut [u8],
        width: u32,
        height: u32,
        name: &'static str,
        x: f32,
        y: f32,
        size: f32,
        style: Style,
    ) {
        self.collect();
        let side = side_of(size);
        let mut pixmaps = self.pixmaps.borrow_mut();
        let pixmap = pixmaps
            .entry((name, side, style))
            .or_insert_with(|| rasterize(name, side, style));
        if let Some(pixmap) = pixmap {
            composite(pixels, width, height, pixmap, x.round() as i32, y.round() as i32);
        }
    }

    /// Rastérise l'icône `name` à `size` pixels de côté sans la dessiner : la prochaine
    /// [`IconCache::draw`] à cette taille la trouvera prête.
    pub fn prepare(&self, name: &'static str, size: f32, style: Style) {
        self.collect();
        let side = side_of(size);
        self.pixmaps
            .borrow_mut()
            .entry((name, side, style))
            .or_insert_with(|| rasterize(name, side, style));
    }

    /// Comme [`IconCache::prepare`], pour un fichier d'icône du système.
    pub fn prepare_file(&self, path: &Path, size: f32) {
        self.collect();
        let side = side_of(size);
        self.files
            .borrow_mut()
            .entry((path.to_path_buf(), side))
            .or_insert_with(|| rasterize_file(path, side));
    }

    /// Comme [`IconCache::draw`], pour un fichier d'icône SVG ou PNG du système. Renvoie
    /// faux si le fichier est illisible (l'appelant dessine alors une icône par défaut).
    #[allow(clippy::too_many_arguments)] // Tampon, chemin et géométrie, sans état global.
    pub fn draw_file(
        &self,
        pixels: &mut [u8],
        width: u32,
        height: u32,
        path: &Path,
        x: f32,
        y: f32,
        size: f32,
    ) -> bool {
        self.collect();
        let side = side_of(size);
        let mut files = self.files.borrow_mut();
        let pixmap = files
            .entry((path.to_path_buf(), side))
            .or_insert_with(|| rasterize_file(path, side));
        match pixmap {
            Some(pixmap) => {
                composite(pixels, width, height, pixmap, x.round() as i32, y.round() as i32);
                true
            }
            None => false,
        }
    }
}

/// Côté en pixels entiers d'une icône de `size` pixels.
fn side_of(size: f32) -> u32 {
    size.round().max(1.0) as u32
}

/// Lit une icône du système (SVG ou PNG) et la met à `side` pixels de côté.
fn rasterize_file(path: &Path, side: u32) -> Option<Pixmap> {
    let data = fs::read(path).ok()?;
    let is_svg = path.extension().and_then(|ext| ext.to_str()) == Some("svg");
    let mut pixmap = Pixmap::new(side, side)?;
    if is_svg {
        let tree = Tree::from_data(&data, &Options::default()).ok()?;
        let view = tree.size();
        let factor = side as f32 / view.width().max(view.height());
        resvg::render(&tree, Transform::from_scale(factor, factor), &mut pixmap.as_mut());
        return Some(pixmap);
    }
    let (rgba, source_width, source_height) = decode_png(&data)?;
    // Réduction par moyenne de zone, en alpha prémultiplié (bords nets, sans halo).
    let target = pixmap.data_mut();
    for ty in 0..side {
        for tx in 0..side {
            let x0 = tx * source_width / side;
            let x1 = ((tx + 1) * source_width / side).max(x0 + 1).min(source_width);
            let y0 = ty * source_height / side;
            let y1 = ((ty + 1) * source_height / side).max(y0 + 1).min(source_height);
            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let offset = ((sy * source_width + sx) * 4) as usize;
                    let alpha = rgba[offset + 3] as u32;
                    for channel in 0..3 {
                        sum[channel] += rgba[offset + channel] as u32 * alpha / 255;
                    }
                    sum[3] += alpha;
                }
            }
            let count = (x1 - x0) * (y1 - y0);
            let offset = ((ty * side + tx) * 4) as usize;
            for channel in 0..4 {
                target[offset + channel] = (sum[channel] / count.max(1)) as u8;
            }
        }
    }
    Some(pixmap)
}

/// Décode un PNG en RVBA 8 bits (non prémultiplié).
fn decode_png(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let pixels = &buffer[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => pixels.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((rgba, info.width, info.height))
}

/// Rasterise l'icône `name` en un carré de `side` pixels.
fn rasterize(name: &str, side: u32, style: Style) -> Option<Pixmap> {
    let Some((_, source)) = SOURCES
        .iter()
        .chain(GLYPHS)
        .find(|(candidate, _)| *candidate == name)
    else {
        tracing::warn!(name, "icône inconnue");
        return None;
    };
    let source = match style {
        Style::Color => (*source).to_owned(),
        Style::Symbolic(ink) => ferrite_ui::icons::symbolic(source, ink),
    };
    let tree = Tree::from_str(&source, &Options::default())
        .inspect_err(|err| tracing::warn!(name, %err, "icône SVG illisible"))
        .ok()?;
    let mut pixmap = Pixmap::new(side, side)?;
    let view = tree.size();
    let factor = side as f32 / view.width().max(view.height());
    resvg::render(&tree, Transform::from_scale(factor, factor), &mut pixmap.as_mut());
    Some(pixmap)
}

/// Pose `pixmap` (RVBA prémultiplié) sur le tampon BGRA prémultiplié, par-dessus.
fn composite(pixels: &mut [u8], width: u32, height: u32, pixmap: &Pixmap, x: i32, y: i32) {
    let side_w = pixmap.width() as i32;
    let source = pixmap.data();
    for row in 0..pixmap.height() as i32 {
        let py = y + row;
        if py < 0 || py >= height as i32 {
            continue;
        }
        for column in 0..side_w {
            let px = x + column;
            if px < 0 || px >= width as i32 {
                continue;
            }
            let from = ((row * side_w + column) * 4) as usize;
            let alpha = source[from + 3] as u32;
            if alpha == 0 {
                continue;
            }
            let to = ((py as u32 * width + px as u32) * 4) as usize;
            let inverse = 255 - alpha;
            for (channel, offset) in [(2, 0), (1, 1), (0, 2), (3, 3)] {
                let below = pixels[to + offset] as u32;
                pixels[to + offset] = (source[from + channel] as u32 + below * inverse / 255).min(255) as u8;
            }
        }
    }
}

/// Nom de l'icône Wi-Fi : barrée radio coupée, ondes éteintes sans connexion, sinon
/// autant d'ondes allumées que le signal le permet.
pub fn wifi_icon(enabled: bool, signal: Option<u8>) -> &'static str {
    match (enabled, signal) {
        (false, _) => "status-wifi-off",
        (true, None) => "status-wifi-0",
        (true, Some(signal)) => wifi_level_icon(ferrite_system::network::signal_level(signal)),
    }
}

/// Icône Wi-Fi pour un niveau de 0 à 3 ondes.
pub fn wifi_level_icon(level: u8) -> &'static str {
    ["status-wifi-0", "status-wifi-1", "status-wifi-2", "status-wifi-3"][level.min(3) as usize]
}

/// Nom de l'icône du volume pour un niveau de 0 à 100 : coupé, puis 1 à 3 ondes allumées.
pub fn volume_icon(level: u8) -> &'static str {
    const NAMES: [&str; 4] = [
        "status-volume-0",
        "status-volume-1",
        "status-volume-2",
        "status-volume-3",
    ];
    NAMES[step(level, 3)]
}

/// Nom de l'icône de batterie, arrondie à la dizaine.
pub fn battery_icon(percent: u8, charging: bool) -> &'static str {
    const NAMES: [&str; 11] = [
        "status-battery-000",
        "status-battery-010",
        "status-battery-020",
        "status-battery-030",
        "status-battery-040",
        "status-battery-050",
        "status-battery-060",
        "status-battery-070",
        "status-battery-080",
        "status-battery-090",
        "status-battery-100",
    ];
    const CHARGING: [&str; 11] = [
        "status-battery-charging-000",
        "status-battery-charging-010",
        "status-battery-charging-020",
        "status-battery-charging-030",
        "status-battery-charging-040",
        "status-battery-charging-050",
        "status-battery-charging-060",
        "status-battery-charging-070",
        "status-battery-charging-080",
        "status-battery-charging-090",
        "status-battery-charging-100",
    ];
    let index = ((percent.min(100) as usize) + 5) / 10;
    if charging { CHARGING[index] } else { NAMES[index] }
}

/// Palier de 0 à `steps` pour un niveau de 0 à 100 ; 0 est réservé au niveau nul.
fn step(level: u8, steps: usize) -> usize {
    if level == 0 {
        0
    } else {
        (level.min(100) as usize * steps).div_ceil(100).clamp(1, steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(name: &'static str, style: Style) -> Vec<u8> {
        let mut pixels = vec![0u8; 48 * 48 * 4];
        IconCache::default().draw(&mut pixels, 48, 48, name, 0.0, 0.0, 48.0, style);
        pixels
    }

    #[test]
    fn une_icone_preparee_en_arriere_plan_est_identique() {
        let icons = IconCache::default();
        assert!(!icons.is_ready("app-files", 48.0, Style::Color));
        icons.prepare_in_background("app-files", 48.0, Style::Color);
        let start = std::time::Instant::now();
        while !icons.is_ready("app-files", 48.0, Style::Color) {
            assert!(
                start.elapsed() < std::time::Duration::from_secs(10),
                "icône jamais prête"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut pixels = vec![0u8; 48 * 48 * 4];
        icons.draw(&mut pixels, 48, 48, "app-files", 0.0, 0.0, 48.0, Style::Color);
        assert!(pixels.iter().any(|&value| value != 0));
        assert_eq!(pixels, render("app-files", Style::Color));
    }

    #[test]
    fn les_icones_du_theme_n_ont_pas_de_texte() {
        // resvg est compilé sans le texte : une balise <text> disparaîtrait au rendu.
        for (name, source) in SOURCES {
            assert!(!source.contains("<text"), "{name} contient du texte");
        }
    }

    #[test]
    fn toutes_les_icones_embarquees_sont_lisibles() {
        // Une icône entièrement « éteinte » (Wi-Fi coupé) reste visible, estompée.
        for (name, _) in SOURCES.iter().chain(GLYPHS) {
            for style in [Style::Color, Style::Symbolic(Color::WHITE)] {
                let pixmap = rasterize(name, 32, style).unwrap_or_else(|| panic!("{name} illisible"));
                assert!(
                    pixmap.data().chunks(4).any(|pixel| pixel[3] > 60),
                    "{name} ({style:?}) est vide"
                );
            }
        }
    }

    #[test]
    fn une_icone_symbolique_n_a_qu_une_teinte() {
        let ink = Color::rgb(0x102030);
        let pixels = render("status-wifi-2", Style::Symbolic(ink));
        let mut alphas = Vec::new();
        for pixel in pixels.chunks(4).filter(|pixel| pixel[3] == 255) {
            assert_eq!(&pixel[..3], &[0x30, 0x20, 0x10], "BGRA attendu");
        }
        for pixel in pixels.chunks(4).filter(|pixel| pixel[3] > 0) {
            alphas.push(pixel[3]);
        }
        // Barres actives opaques et barre éteinte estompée.
        assert!(alphas.contains(&255));
        assert!(alphas.iter().any(|&alpha| (70..110).contains(&alpha)));
    }

    #[test]
    fn les_paliers_suivent_le_niveau() {
        assert_eq!(volume_icon(0), "status-volume-0");
        assert_eq!(volume_icon(1), "status-volume-1");
        assert_eq!(volume_icon(33), "status-volume-1");
        assert_eq!(volume_icon(65), "status-volume-2");
        assert_eq!(volume_icon(100), "status-volume-3");
        assert_eq!(battery_icon(47, false), "status-battery-050");
        assert_eq!(battery_icon(3, true), "status-battery-charging-000");
        assert_eq!(wifi_icon(false, Some(90)), "status-wifi-off");
        assert_eq!(wifi_icon(true, None), "status-wifi-0");
        assert_eq!(wifi_icon(true, Some(20)), "status-wifi-1");
        assert_eq!(wifi_icon(true, Some(85)), "status-wifi-3");
    }
}
