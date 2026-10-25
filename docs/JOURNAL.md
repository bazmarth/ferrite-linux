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

Vérifié :
- `tools/ci.sh` réussit dans Debian 13 WSL (Rust 1.99.1) : fmt, clippy `-D warnings`,
  4 tests, shellcheck des scripts → **critère 1 de la phase 0 rempli**.
- `sudo tools/usb.sh` produit `build/ferrite-live.iso` (1,1 Gio, noyau 6.12.113, Debian 13.7).
  Aucun environnement de bureau dans la liste des paquets (quelques bibliothèques GTK 3
  arrivent par dépendance, sans serveur X).
- `tools/vm.sh --headless 120` : démarrage UEFI (OVMF, KVM) → GRUB (5 s) → connexion
  automatique de `ferrite` sur tty1 → accueil Ferrite en français (capture
  `build/vm-ecran.png`) → **critère 2 de la phase 0 rempli** avec notre ISO au lieu d'une
  netinst.
- Corrigé en route : `-boot d` ne crée pas d'entrée de démarrage UEFI sur q35 (le lecteur
  est maintenant déclaré avec `bootindex=0`) ; les sockets Unix ne marchent pas sur `/mnt/d`.

Non vérifié :
- `ferrite-check` n'a pas été exécuté (ni en VM ni sur le matériel).
- Rien n'est encore essayé sur le ThinkBook.

Reste :
- Phase 1 : essai sur le ThinkBook réel (checklist `docs/TEST-THINKBOOK.md`), persistance
  documentée mais non essayée.
- L'utilisateur `baz` est dans le groupe `kvm` après un `wsl --shutdown` ; d'ici là,
  `tools/vm.sh` doit être lancé avec sudo pour profiter de KVM.

## 2026-10-25 — Phase 2 : squelette de ferrite-shell (mode fenêtre)

Fait :
- Crate `ferrite-shell` (Smithay 0.7.0), organisée comme le plan le demande : `backend/winit.rs`,
  `protocols/` (compositor + shm, xdg-shell, seat + data-device, output + xdg-output),
  `wm/` (placement en cascade sous la barre de menus, focus au clic, déplacement,
  redimensionnement, agrandir/restaurer), `render/` (fond d'écran en dégradé Ferrite,
  capture PNG), `input.rs`, `options.rs`. Base : l'exemple `smallvil` de Smithay.
- Clavier `fr` (xkbcommon), répétition 300 ms / 30 par seconde ; Ctrl+Alt+Retour arrière
  quitte le compositeur.
- Option `--scale` (échelle fractionnaire de l'écran), `--capture FICHIER` pour les
  vérifications automatiques, client lancé après `--`.
- `ferrite-ui` : dégradé du fond d'écran (`WALLPAPER_GRADIENT`, `gradient_at`).
- `tools/shell-dev.sh` : compile et lance ferrite-shell + foot dans WSLg.

Vérifié :
- `tools/ci.sh` réussit (clippy `-D warnings`, 14 tests).
- Dans WSLg : ferrite-shell s'ouvre, `foot` s'y connecte et s'affiche à la position de la
  cascade, sur le dégradé, à l'échelle 1 et 1,5 (captures `build/shell-foot.png`).

Non vérifié :
- Déplacement, redimensionnement, agrandissement et saisie au clavier n'ont pas été
  essayés à la main (code repris de smallvil, testé seulement par compilation).
- Critères de la phase 2 sur le ThinkBook : rien (pas encore de backend DRM).

Problème rencontré : le Weston de WSLg plante (signal 11) quand la fenêtre de
ferrite-shell s'ouvre en Wayland ; `tools/shell-dev.sh` la fait passer par X11.

Reste pour la phase 2 : décorations serveur (barre de titre Ferrite, feux tricolores),
coins arrondis et ombres, backend DRM/KMS + libinput, XWayland, protocoles restants
(dmabuf, fractional-scale, viewporter, xdg-activation, primary-selection…), session
`ferrite-session` qui lance le compositeur.
