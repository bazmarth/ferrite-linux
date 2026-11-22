//! Valeurs visuelles indépendantes des moteurs de rendu.
//!
//! Les adaptateurs egui et ferrite-shell traduisent ces mesures dans leurs propres API.

pub mod spacing {
    pub const X_SMALL: f32 = 4.0;
    pub const SMALL: f32 = 6.0;
    pub const MEDIUM: f32 = 8.0;
    pub const LARGE: f32 = 12.0;
    pub const X_LARGE: f32 = 16.0;
    pub const CONTROL_HORIZONTAL: f32 = 12.0;
    pub const CONTROL_VERTICAL: f32 = 5.0;
}

/// Couleurs de signal. L'accent par défaut est l'« Oxyde », l'orange de l'oxyde de fer dont
/// la ferrite est faite : foncé en clair (texte blanc dessus, 5,8:1), vif en sombre (encre
/// foncée dessus). Chaque couleur garde 4,5:1 comme texte sur les fonds de son thème.
pub mod colors {
    pub const ACCENT_LIGHT: u32 = 0xB83A0A;
    pub const ACCENT_DARK: u32 = 0xFF7A45;
    pub const DANGER_LIGHT: u32 = 0xC62828;
    pub const DANGER_DARK: u32 = 0xF87171;
    pub const SUCCESS_LIGHT: u32 = 0x137333;
    pub const SUCCESS_DARK: u32 = 0x4ADE80;
    pub const WARNING_LIGHT: u32 = 0x8F5200;
    pub const WARNING_DARK: u32 = 0xFBBF24;
}

/// Plus l'objet est grand, plus il est rond : contrôle 7, carte et menu 10, fenêtre 14,
/// panneau du bureau 20.
pub mod radius {
    pub const CONTROL: u8 = 7;
    pub const WINDOW: u32 = 14;
    pub const PANEL: u8 = 10;
    pub const POPOVER: u32 = 10;
}

pub mod border {
    pub const HAIRLINE: f32 = 1.0;
    pub const FOCUS: f32 = 2.0;
}

/// Ombres, partout les mêmes (bureau, fenêtres, Dock, applications). Notation CSS :
/// décalage vertical, flou, opacité du noir. Le flou suit CSS : écart type = flou / 2, et
/// l'ombre s'efface en douceur sur une fois et demie le flou de chaque côté du bord.
/// Tout ce qui flotte pose deux couches : [`CONTACT`], fine et nette, qui détache le bord,
/// puis l'ombre de son niveau, large et légère.
pub mod elevation {
    /// Une couche d'ombre.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Shadow {
        pub offset_y: f32,
        pub blur: f32,
        pub alpha: f32,
    }

    impl Shadow {
        /// Distance au-delà du bord où l'ombre devient nulle (décalage compris), en pixels.
        pub fn reach(self) -> f32 {
            self.offset_y.abs() + 1.5 * self.blur
        }

        /// Opacité à la distance signée `d` du bord de la forme décalée (négative dedans),
        /// avec `blur` déjà à l'échelle : flou gaussien d'un bord, ramené à exactement
        /// l'opacité pleine en deçà de -1,5 flou et à zéro au-delà de +1,5 flou.
        pub fn amount(self, d: f32, blur: f32) -> f32 {
            self.alpha * profile(d, blur)
        }
    }

    /// Part d'ombre (0 à 1) à la distance signée `d` d'un bord flouté sur `blur`.
    pub fn profile(d: f32, blur: f32) -> f32 {
        let sigma = (blur * 0.5).max(0.25);
        let reach = 3.0 * sigma;
        if d >= reach {
            return 0.0;
        }
        if d <= -reach {
            return 1.0;
        }
        // 0,5 × erfc(3 / √2) : ce qui reste à 3 écarts types, retiré pour finir à zéro.
        const TAIL: f32 = 0.001_349_9;
        let raw = 0.5 * (1.0 - erf(d / (sigma * std::f32::consts::SQRT_2)));
        ((raw - TAIL) / (1.0 - 2.0 * TAIL)).clamp(0.0, 1.0)
    }

    /// Fonction d'erreur (Abramowitz et Stegun 7.1.26, erreur < 1,5e-7).
    fn erf(x: f32) -> f32 {
        let sign = x.signum();
        let x = x.abs();
        let t = 1.0 / (1.0 + 0.327_591_1 * x);
        let poly = t
            * (0.254_829_6 + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152_1 + t * 1.061_405_4))));
        sign * (1.0 - poly * (-x * x).exp())
    }

    /// Couche de contact, sous tout ce qui flotte : 0 1 3, noir 10 %.
    pub const CONTACT: Shadow = Shadow {
        offset_y: 1.0,
        blur: 3.0,
        alpha: 0.10,
    };
    /// Petits éléments posés sur un fond (bouton d'un curseur, îlot) : 0 1 3, noir 24 %.
    pub const SMALL: Shadow = Shadow {
        offset_y: 1.0,
        blur: 3.0,
        alpha: 0.24,
    };
    /// Menus, bulles, popovers : 0 8 24, noir 16 %.
    pub const MENU: Shadow = Shadow {
        offset_y: 8.0,
        blur: 24.0,
        alpha: 0.16,
    };
    /// Panneaux du bureau (centre de contrôle, lanceur, Dock) et dialogues : 0 12 32,
    /// noir 20 %.
    pub const PANEL: Shadow = Shadow {
        offset_y: 12.0,
        blur: 32.0,
        alpha: 0.20,
    };
    /// Fenêtre active : 0 16 48, noir 28 %.
    pub const WINDOW_ACTIVE: Shadow = Shadow {
        offset_y: 16.0,
        blur: 48.0,
        alpha: 0.28,
    };
    /// Fenêtre inactive ou aimantée : 0 4 16, noir 14 %.
    pub const WINDOW_INACTIVE: Shadow = Shadow {
        offset_y: 4.0,
        blur: 16.0,
        alpha: 0.14,
    };
    /// Débord de l'ombre d'une fenêtre : assez pour l'ombre active entière.
    pub const WINDOW_MARGIN: i32 = 96;
}

/// Tailles de texte. Pas de demi-tailles : une taille entre deux prend la plus petite.
/// Base du système : 14 (`BODY`). Rien sous 12 hors des mentions serrées (`CAPTION`).
pub mod typography {
    /// Mentions serrées (heures d'un agenda, durées, étiquettes de pastille).
    pub const CAPTION: f32 = 11.0;
    /// Métadonnées, aide sous un champ, titre de groupe.
    pub const SMALL: f32 = 12.0;
    /// Libellés compacts des listes, menus, descriptions sous un libellé.
    pub const LABEL: f32 = 13.0;
    /// Taille de base de tout le système.
    pub const BODY: f32 = 14.0;
    /// Titre de dialogue, de carte, de notification.
    pub const HEADING: f32 = 16.0;
    /// Titre de section dans une page.
    pub const SECTION: f32 = 20.0;
    /// Titre de page (Réglages) et grands titres de panneau.
    pub const TITLE: f32 = 24.0;
    /// Écrans d'accueil (installation, premier démarrage) : une fois par écran.
    pub const DISPLAY: f32 = 32.0;
    pub const MONOSPACE: f32 = 13.0;
    /// Titre de fenêtre, en semi-gras.
    pub const WINDOW_TITLE: f32 = 13.0;
}

pub mod control {
    /// Hauteur des boutons, champs et listes déroulantes.
    pub const HEIGHT: f32 = 32.0;
    /// Boutons d'action des écrans plein écran (connexion, installation).
    pub const HEIGHT_LARGE: f32 = 40.0;
}

/// Durées des animations, communes au bureau et aux applications. La courbe est
/// `theme::Easing::EaseOut` (départ rapide, arrivée douce), sauf mention contraire.
pub mod motion {
    use std::time::Duration;

    /// Disparitions, survols, petits changements d'état.
    pub const QUICK: Duration = Duration::from_millis(120);
    /// Apparition d'un panneau ou d'un menu, fondus.
    pub const SHORT: Duration = Duration::from_millis(200);
    /// Ouverture d'une fenêtre, vue d'ensemble, glissement d'un contrôle.
    pub const NORMAL: Duration = Duration::from_millis(320);
    /// Grands mouvements : changement de bureau, verrouillage, îlot, retour d'un glisser.
    pub const LONG: Duration = Duration::from_millis(360);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn les_mesures_et_durees_sont_positives() {
        assert_eq!(spacing::MEDIUM, 8.0);
        assert_eq!(radius::WINDOW, 14);
        assert_eq!(border::HAIRLINE, 1.0);
        assert_eq!(control::HEIGHT, 32.0);
        assert_eq!(elevation::WINDOW_ACTIVE.blur, 48.0);
        assert_eq!(motion::SHORT, Duration::from_millis(200));
        assert_eq!(motion::NORMAL, Duration::from_millis(320));
    }

    #[test]
    fn les_ombres_s_effacent_en_douceur_et_tiennent_dans_leur_marge() {
        use elevation::*;
        assert_eq!(profile(-100.0, 32.0), 1.0);
        assert_eq!(profile(48.0, 32.0), 0.0);
        assert!((profile(0.0, 32.0) - 0.5).abs() < 1e-3);
        // Décroissante et sans marche : aucun saut de plus de 3 % par pixel.
        let mut last = 1.0;
        for step in -60..=60 {
            let value = profile(step as f32, 32.0);
            assert!(
                value <= last && last - value < 0.03,
                "à {step} : {last} puis {value}"
            );
            last = value;
        }
        assert!(WINDOW_ACTIVE.reach() <= WINDOW_MARGIN as f32);
    }

    #[test]
    fn les_couleurs_semantiques_sont_rgb() {
        for color in [
            colors::ACCENT_LIGHT,
            colors::ACCENT_DARK,
            colors::DANGER_LIGHT,
            colors::DANGER_DARK,
            colors::SUCCESS_LIGHT,
            colors::SUCCESS_DARK,
            colors::WARNING_LIGHT,
            colors::WARNING_DARK,
        ] {
            assert!(color <= 0xFFFFFF);
        }
    }
}
