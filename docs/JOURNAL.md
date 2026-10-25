# Journal de développement

Une entrée par session : ce qui est fait, ce qui est vérifié (et comment), ce qui reste.

## 2026-10-25 — Phase 0 (fondations) et début de la phase 1

Fait :
- Dépôt `D:\ferrite-linux` (git, branche `main`) avec l'arborescence du plan (§3.3).
- Espace de travail Cargo (édition 2024, résolveur 3, lints communs), `rust-toolchain.toml`,
  `rustfmt.toml`, fins de ligne LF imposées par `.gitattributes`.
- Crate `ferrite-ui` : charte visuelle en code (`ui/src/theme.rs`) — mesures reprises du
  bureau Ferrite d'origine, palettes clair/sombre, échelle en 120es (protocole
  `wp_fractional_scale`), courbes d'animation avec « réduire les animations ». Tests unitaires.
- Scripts : `tools/setup-wsl.sh`, `tools/ci.sh`, `tools/vm.sh` (UEFI, mode sans fenêtre
  avec capture), `tools/usb.sh` (image live).
- Polices Inter, InterDisplay, JetBrains Mono, Source Serif 4 et leurs licences OFL dans
  `assets/fonts`.
- Image live (phase 1, tâches 1 à 3 et 5) : configuration reprise du prototype
  `D:\Os\ferrite-linux\image`, nettoyée (plus de Weston), paquets du plan, utilisateur
  `ferrite`, connexion automatique sur tty1, session console, outil `ferrite-check`.

Vérifié : voir la section « Vérifications » ajoutée en fin de session.

Reste :
- Phase 1 : essai sur le ThinkBook réel (checklist `docs/TEST-THINKBOOK.md`), persistance
  documentée mais non essayée.
