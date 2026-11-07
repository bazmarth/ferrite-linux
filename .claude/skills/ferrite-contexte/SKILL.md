---
name: ferrite-contexte
description: Charge le contexte d'architecture et les procédures de test de Ferrite OS. À invoquer en début de session quand on attaque un nouveau chantier (compositeur, panneaux, applications, image ISO, réglages).
---

# Contexte Ferrite OS

## Architecture en bref

- `shell/` : compositeur Wayland (Smithay 0.7), binaire `ferrite-shell`, backends DRM/KMS et winit.
- `apps/core/` : applications egui, un binaire `ferrite-apps <commande>`. Chaque appli est une vue (`AppView`) créée dans `main.rs::create` ; pièces communes dans `kit.rs`, thème et rôles de couleur dans `style.rs`. Nouvelle appli : `docs/NOUVELLE-APPLICATION.md`, modèle `gallery.rs`.
- Liste des applications : `services/system/src/catalog.rs` (la seule ; le bureau et `ferrite-apps` la lisent).
- `services/system/` : crate `ferrite_system`, appels système partagés (nmcli, wpctl, bluetoothctl, powerprofilesctl…), réglages (`config.rs`), applications par défaut (`defaults.rs`).
- `services/visage/` : `ferrite-visage` (déverrouillage par visage) ; `services/portal/` : `xdg-desktop-portal-ferrite` (sélecteur de fichiers → `ferrite-apps choisir`).
- `third_party/winit/` : copie de winit avec le pincement Wayland (`[patch.crates-io]`).
- `ui/` : tokens, thème, polices, icônes.
- `tools/vm-manager/` : outil de gestion de la machine virtuelle.
- Réglages utilisateur : `~/.config/ferrite/reglages` (clé=valeur, français).
- Image : `image/` (live-build Debian 13).

## Procédure de test

1. Modifications de code : `tools/ci.sh` (fmt, clippy `--all-targets -D warnings`, tests, shellcheck).
2. Test visuel : `target/debug/ferrite-shell --winit --capture /tmp/x.png --capture-delay 5` (options `--demo-*` pour figer un état : verrouillage, visage, centre de contrôle, îlot… ; liste par `--help`). Réglages isolés par `XDG_CONFIG_HOME` (`apparence = sombre` pour le thème sombre) ; si l'appli est déjà ouverte sur le vrai bureau, bus et `XDG_RUNTIME_DIR` à part (`docs/TECHNIQUE.md` §6). Galerie : `ferrite-apps galerie`.
3. Test ISO : `tools/vm-test.sh tools/scenarios/NOM.txt` (scénarios dans `tools/scenarios/`).
4. Bureau réel : `tools/dev-on.sh` puis Ctrl+Alt+Retour arrière (ou « Relancer le bureau ») ; repli automatique si plantage. Les clics sur le vrai bureau, c'est l'utilisateur qui les fait : lui donner une liste d'essais numérotés.
5. Shellcheck n'est pas installé sur le PC : `tools/ci.sh` le saute, le dire.

## Règles du compositeur

- Aucune commande bloquante dans la boucle du compositeur : passer par `ferrite_system::command` dans un fil à part.
- Pas de rendu plein écran sur le processeur à chaque image : animer des images déjà dessinées, ne pas redessiner (`docs/TECHNIQUE.md` §3.3).
- Ne pas appeler `update_uniforms` à chaque image sans changement.
- Ne jamais déclencher de vraie mise en veille en essai ; ne pas forcer la veille `deep` (le ThinkBook ne s'en réveille pas).

## Charte UI

- Couleurs et mesures : `ui/src/tokens.rs` et `ui/src/theme.rs`. Rôles de couleur dans `Palette` : `style::nom()` côté applis, `palette().nom` côté bureau. Aucune couleur à la main : `tools/check-colors.sh` la refuse.
- Durées : `tokens::motion` (QUICK, SHORT, NORMAL, LONG), courbe `Easing::EaseOut` ; `kit::glide` dans les applis.
- Respecter le réglage « réduire les animations ».
- Aucun bouton qui fait semblant : griser plutôt qu'un bouton inactif.

## Pointeurs

- Référence technique : `docs/TECHNIQUE.md` (ancien `PASSATION.md` ; lire les sections 2, 3 et 4 avant de modifier le shell, la section 5 pour l'état et la suite).
- Raccourcis : `docs/RACCOURCIS.md` (à compléter à chaque nouveau raccourci) ; dépendances : `docs/DEPENDANCES.md` (à compléter à chaque crate ou paquet ajouté).
- Décisions : `docs/DECISIONS.md` (prime sur le plan).
- Journal : fin de `docs/JOURNAL.md`.
