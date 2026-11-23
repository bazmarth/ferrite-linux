//! Pictogrammes au trait de l'interface (charte « Forge », `docs/CHARTE.md`).
//!
//! Une seule famille pour le bureau et les applications : grille de 24, trait de 1,75,
//! bouts arrondis. Les dessins sont dans `icons/SVG/symboles/`, produits par
//! `tools/icones.py` ; ils sont tracés en noir et prennent leur teinte par
//! [`crate::icons::symbolic`]. Plusieurs noms peuvent désigner le même dessin : les
//! anciens noms (`cc-…`, `glyph-…`, `clip-…`) restent valables.

macro_rules! glyphs {
    ($($name:literal => $file:literal),* $(,)?) => {
        /// Pictogrammes embarqués : (nom, source SVG).
        pub const GLYPHS: &[(&str, &str)] = &[
            $(($name, include_str!(concat!("../../icons/SVG/symboles/", $file, ".svg")))),*
        ];
    };
}

glyphs![
    // Réglages rapides et menus du bureau.
    "cc-wifi" => "wifi",
    "cc-bluetooth" => "bluetooth",
    "cc-dark" => "lune",
    "cc-dnd" => "silence",
    "cc-saver" => "economie",
    "cc-balanced" => "equilibre",
    "cc-performance" => "performance",
    "cc-screen" => "ecran",
    "cc-keyboard" => "clavier",
    "cc-keyboard-light" => "clavier-eclaire",
    "cc-volume" => "volume",
    "cc-mute" => "coupe",
    "cc-mic" => "micro",
    "cc-mic-off" => "micro-coupe",
    "cc-airplane" => "avion",
    "cc-brightness" => "luminosite",
    "cc-settings" => "reglages",
    "cc-lock" => "cadenas",
    "cc-account" => "compte",
    "cc-eye" => "oeil",
    "cc-eye-off" => "oeil-barre",
    "cc-night" => "nuit",
    "cc-restart" => "redemarrer",
    "cc-logout" => "deconnexion",
    "cc-power" => "eteindre",
    "cc-sleep" => "veille",
    "cc-back" => "retour",
    "cc-check" => "valider",
    "cc-head" => "casque",
    "cc-mouse" => "souris",
    "cc-phone" => "telephone",
    "cc-speaker" => "enceinte",
    "cc-watch" => "montre",
    "cc-chevron" => "chevron",
    "cc-search" => "rechercher",
    "cc-bell" => "cloche",
    "cc-play" => "lire",
    "cc-pause" => "pause",
    "cc-previous" => "precedent",
    "cc-next" => "suivant",
    "cc-music" => "musique",
    // Presse-papiers.
    "clip-code" => "code",
    "clip-link" => "lien",
    "clip-text" => "texte",
    "clip-pin" => "epingle",
    "clip-pin-on" => "epingle-pleine",
    "clip-close" => "fermer",
    "clip-sun" => "luminosite",
    "clip-moon" => "lune",
    "clip-search" => "rechercher",
    "clip-empty" => "presse-papiers-vide",
    // Colonnes des applications (Réglages, Calendrier, Logithèque, Fichiers).
    "glyph-wifi" => "wifi",
    "glyph-bluetooth" => "bluetooth",
    "glyph-airplane" => "avion",
    "glyph-sun" => "luminosite",
    "glyph-clipboard" => "presse-papiers",
    "glyph-gear" => "reglages",
    "glyph-calendar" => "date",
    "glyph-today" => "aujourdhui",
    "glyph-upcoming" => "a-venir",
    "glyph-overdue" => "en-retard",
    "glyph-tasks" => "taches",
    "glyph-network" => "reseau",
    "glyph-ethernet" => "cable",
    "glyph-appearance" => "apparence",
    "glyph-dock" => "bureau",
    "glyph-windows" => "fenetres",
    "glyph-bell" => "cloche",
    "glyph-sound" => "volume-moyen",
    "glyph-sound-full" => "volume",
    "glyph-energy" => "energie",
    "glyph-screen" => "ecran",
    "glyph-keyboard" => "clavier",
    "glyph-mouse" => "souris",
    "glyph-printer" => "imprimante",
    "glyph-account" => "compte",
    "glyph-accounts" => "comptes",
    "glyph-lock" => "cadenas",
    "glyph-privacy" => "confidentialite",
    "glyph-access" => "accessibilite",
    "glyph-update" => "mise-a-jour",
    "glyph-about" => "a-propos",
    "glyph-clock" => "horloge",
    "glyph-location" => "position",
    "glyph-head" => "casque",
    "glyph-phone" => "telephone",
    "glyph-speaker" => "enceinte",
    "glyph-watch" => "montre",
    "glyph-laptop" => "portable",
    "glyph-mic" => "micro",
    "glyph-camera" => "camera",
    "glyph-app" => "applications",
    "glyph-star" => "etoile",
    "glyph-installed" => "installe",
    "glyph-play" => "lecture",
    "glyph-brush" => "pinceau",
    "glyph-globe" => "reseau",
    "glyph-doc" => "document",
    "glyph-code" => "code",
    "glyph-game" => "jeu",
    "glyph-book" => "livre",
    "glyph-flask" => "science",
    "glyph-grid" => "grille",
    "glyph-list" => "liste",
    "glyph-panel" => "panneau",
    "glyph-more" => "plus-d-actions",
    "glyph-add" => "ajouter",
    "glyph-refresh" => "actualiser",
    "glyph-search" => "rechercher",
    "glyph-share" => "partage",
    "glyph-screenshot" => "capture",
    "glyph-home" => "dossier-perso",
    "glyph-folder" => "dossier",
    "glyph-documents" => "document",
    "glyph-downloads" => "telechargements",
    "glyph-pictures" => "images",
    "glyph-music" => "musique",
    "glyph-videos" => "videos",
    "glyph-trash" => "corbeille",
    "glyph-recent" => "recents",
    "glyph-drive" => "disque",
    "glyph-usb" => "usb",
    "glyph-analysis" => "analyse",
    "glyph-templates" => "modeles",
    "glyph-upload" => "envoi",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaque_nom_est_unique() {
        let mut names: Vec<_> = GLYPHS.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len());
    }

    #[test]
    fn chaque_dessin_est_au_trait_et_en_noir_seulement() {
        for (name, source) in GLYPHS {
            assert!(source.contains(r#"viewBox="0 0 24 24""#), "{name}");
            assert!(source.contains(r#"stroke-width="1.75""#), "{name}");
            // Seuls le noir (allumé) et le gris clair (éteint) : la teinte vient de `symbolic`.
            for color in source.split('#').skip(1) {
                let hex: String = color.chars().take_while(char::is_ascii_hexdigit).collect();
                assert!(
                    matches!(hex.as_str(), "000" | "D4D4D4" | "fff"),
                    "{name} : #{hex}"
                );
            }
        }
    }

    #[test]
    fn la_teinte_remplace_le_noir() {
        let (_, wifi) = GLYPHS.iter().find(|(name, _)| *name == "cc-wifi").unwrap();
        let tinted = crate::icons::symbolic(wifi, crate::Color::rgb(0xB83A0A));
        assert!(!tinted.contains("#000"));
        assert!(tinted.contains("rgba(184,58,10,1)"));
    }
}
