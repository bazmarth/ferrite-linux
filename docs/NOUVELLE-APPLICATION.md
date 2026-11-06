# Créer une application Ferrite

Toutes les applications Ferrite vivent dans `ferrite-apps` (`apps/core`, eframe/egui).
Elles partagent la même fenêtre, le même thème suivi en direct, les mêmes pièces et les
mêmes couleurs. Une nouvelle application ne redessine rien de tout cela.

## Les quatre étapes

1. **Le catalogue** : une entrée dans `services/system/src/catalog.rs` (commande, `app_id`,
   nom, icône de `icons/SVG`, tailles, épinglée au Dock ou non). Le bureau la propose
   aussitôt dans le menu Applications et reconnaît ses fenêtres dans le Dock.
2. **Le module** : `apps/core/src/mon_app.rs`, déclaré par `mod mon_app;` dans `main.rs`.
   Un état et une fonction `update` :

   ```rust
   use eframe::egui;
   use crate::{kit, style};

   #[derive(Debug, Default)]
   pub struct MonAppState { /* … */ }

   impl MonAppState {
       pub fn update(&mut self, ui: &mut egui::Ui) {
           style::header(ui, |ui| { /* barre du haut : titre, onglets, boutons */ });
           egui::CentralPanel::no_frame().show(ui, |ui| {
               kit::heading(ui, "Réglages");
               kit::card(ui, |ui| { /* … */ });
           });
       }
   }
   ```

3. **La vue** : dans `main.rs`, `impl AppView for MonAppState` (dessiner, et `open` s'il
   reçoit des fichiers), puis une ligne dans `create` :
   `"mon-app" => Box::new(MonAppState::default()),`.
4. **La vérification** : `tools/ci.sh`, puis une capture en clair et en sombre
   (`docs/PASSATION.md`, procédure des captures).

Le modèle le plus court à lire est la galerie : `apps/core/src/gallery.rs`
(`ferrite-apps galerie`, hors du menu Applications).

## Les règles qui gardent l'harmonie

- **Fenêtre** : `style::header` (ou `style::toolbar`, `style::sidebar`) pour le haut ; la
  fenêtre n'a pas de décoration système, le bureau garde coins, ombre et bords.
- **Pièces** : `kit` avant tout dessin à la main : `card`, `soft_card`, `heading`, `note`,
  `caption`, `separator`, `button`, `wide_button`, `switch`, `slider`, `radio`,
  `segments`, `segments_at`, `status`, `tabular_text`. Une pièce qui manque s'ajoute à
  `kit`, pas dans l'application.
- **Couleurs** : uniquement les rôles de `style` (`background`, `line`, `stage`,
  `primary_text`, `success`, `inverse`…, tous montrés dans la page « Couleurs » de la
  galerie). `tools/check-colors.sh`, lancé par `tools/ci.sh`, refuse une couleur écrite à
  la main. Une couleur propre à l'application (types de fichiers, courbes) se justifie dans
  la liste en tête de ce script.
- **Animations** : les durées de `ferrite_ui::tokens::motion` (`QUICK`, `SHORT`, `NORMAL`,
  `LONG`) et la courbe `Easing::EaseOut`, les mêmes que le bureau. Dans une application :
  `kit::glide`, qui respecte « Réduire les animations ».
- **Chiffres qui changent** (compteurs, durées) : `kit::tabular_text`, sinon ils tremblent.
- **Textes** : en français, avec les espaces insécables avant `:` `;` `?` `!` et `%`.
