---
name: ferrite-contexte
description: Charge le contexte d'architecture et les procédures de test de Ferrite OS. À invoquer en début de session quand on attaque un nouveau chantier (compositeur, panneaux, applications, image ISO, réglages).
---

# Contexte Ferrite OS

## Architecture en bref

- `shell/` : compositeur Wayland (Smithay 0.7), binaire `ferrite-shell`, backends DRM/KMS et winit.
- `apps/core/` : applications egui, un binaire sous-commande par appli.
- `services/system/` : crate `ferrite_system`, appels système partagés (nmcli, wpctl, bluetoothctl…).
- `ui/` : tokens, thème, polices, icônes.
- `tools/vm-manager/` : outil de gestion de la machine virtuelle.
- Réglages utilisateur : `~/.config/ferrite/reglages` (clé=valeur, français).
- Image : `image/` (live-build Debian 13).

## Procédure de test

1. Modifications de code : `tools/ci.sh` (fmt, clippy `--all-targets -D warnings`, tests, shellcheck).
2. Test visuel : `target/debug/ferrite-shell --winit --capture /tmp/x.png --capture-delay 5`.
3. Test ISO : `tools/vm-test.sh tools/scenarios/NOM.txt` (scénarios dans `tools/scenarios/`).
4. Bureau réel : `tools/dev-on.sh` puis « Relancer le bureau » ; repli automatique si plantage.

## Règles du compositeur

- Aucune commande bloquante dans la boucle du compositeur : passer par `ferrite_system::command` dans un fil à part.
- Pas de rendu plein écran sur le processeur à chaque image : animer des images déjà dessinées, ne pas redessiner (`docs/PASSATION.md` §3.3).
- Ne pas appeler `update_uniforms` à chaque image sans changement.

## Charte UI

- Couleurs et mesures : `ui/src/tokens.rs` et `ui/src/theme.rs`.
- Respecter le réglage « réduire les animations ».
- Aucun bouton qui fait semblant : griser plutôt qu'un bouton inactif.

## Pointeurs

- Détails complets : `docs/PASSATION.md` (lire les sections 2, 3 et 4 avant de modifier le shell).
- Décisions : `docs/DECISIONS.md` (prime sur le plan).
- Journal : fin de `docs/JOURNAL.md`.
