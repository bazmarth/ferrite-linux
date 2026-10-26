# Journal de développement

Une entrée par session : ce qui est fait, ce qui est vérifié (et comment), ce qui reste.

## 2026-10-25 — Bureau inspiré de la maquette OrionOS

Fait :
- Recomposition du bureau autour d'une barre supérieure, d'un rail d'applications à gauche,
  d'un lanceur sombre, d'un Dock centré, d'un panneau de réglages et d'une zone de
  notifications translucides.
- Ajout d'un fond alpin au coucher du soleil, de la fenêtre Fichiers claire et de tailles
  de fenêtre adaptées aux applications Ferrite.
- Les zones du Dock, du lanceur, des panneaux réseau/audio/luminosité, de l'aide et de
  confirmation d'arrêt/redémarrage sont reliées aux actions du compositeur et aux commandes
  système disponibles. Le lancement des applications passe par leurs exécutables.
- À la fermeture du bureau, demande de fermeture envoyée aux clients Wayland avant l'arrêt
  du compositeur, avec un court délai pour leur laisser traiter la demande.
- Le script de session affiche désormais le journal si le bureau reste bloqué au démarrage.

Vérifié :
- `tools/ci.sh` : formatage, clippy sans avertissement, ShellCheck et **37 tests réussis**.
- `tools/usb.sh` : ISO live reconstruite avec les binaires optimisés actuels.
- `tools/vm.sh --headless 35` : démarrage UEFI dans QEMU; la capture `build/vm-ecran.png`
  montre le paysage, le Dock, le lanceur, le panneau système et Fichiers.
- Fermeture par Ctrl+Alt+Retour arrière dans QEMU : le bureau se ferme et rend la console
  à la session Ferrite; QEMU reste actif. Capture `build/vm-fermeture.png`.
- Capture WSLg `build/apercu-final3.png` utilisée pour vérifier le fond opaque clair de la
  fenêtre Fichiers.

Limites :
- L'illustration est un paysage vectoriel généré dans le compositeur, inspiré de la maquette;
  ce n'est pas une copie pixel par pixel de son fond photographique.
- Les clics passent par des tests de routage et les commandes sont branchées, mais Wi-Fi,
  Bluetooth, luminosité, volume et boutons d'alimentation n'ont pas été vérifiés sur le
  ThinkBook physique. QEMU ne reproduit pas ces périphériques réels.
- Les applications Ferrite restent rudimentaires; ce travail met en place leur présentation
  et leur lancement, sans ajouter toutes les fonctions de la maquette.

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

## 2026-10-25 — Phase 2 : décorations serveur Ferrite

Fait :
- Protocole `xdg-decoration` : ferrite-shell propose ses décorations ; un client qui veut
  dessiner les siennes est respecté (pas de barre Ferrite, pas d'ombre, pas d'arrondi).
- Barre de titre de 30 px dessinée par un shader GLES : coins du haut arrondis (11 px),
  séparateur d'un pixel physique, feux tricolores (gris si la fenêtre est inactive, symboles
  ×, −, + au survol), couleurs et mesures injectées depuis `ferrite_ui::theme`.
- Titre centré en Inter demi-gras 13 px (fontdue), raccourci par « … » si trop long.
- Coins du bas du contenu arrondis par un shader de texture (`render/rounded.rs`).
- Ombre douce sous tout le cadre (shader, rectangle arrondi flou, décalée vers le bas,
  plus légère pour les fenêtres inactives).
- Clics : glisser la barre de titre déplace, double-clic agrandit/restaure, bords de 6 px
  pour redimensionner, feu rouge ferme, jaune réduit, vert agrandit ; les feux agissent au
  relâchement. Ctrl+Alt+M remet la dernière fenêtre réduite (en attendant le Dock).
- La recherche sous le pointeur suit l'empilement : une barre de titre masque le contenu
  des fenêtres placées dessous.

Vérifié :
- `tools/ci.sh` réussit (21 tests, dont géométrie et zones de clic des décorations,
  rendu du titre, source des shaders).
- Captures dans WSLg à l'échelle 1 et 2 (`build/deco-foot.png`, `build/deco-foot-x2.png`) :
  barre, feux, titre net, coins arrondis anticrénelés, ombre.

Non vérifié :
- Survol, clics sur les feux, déplacement, double-clic et redimensionnement par les bords :
  testés seulement par les tests unitaires de géométrie, pas à la souris (la fenêtre WSLg
  n'a pas pu être pilotée).
- Rendu des menus contextuels et des sous-surfaces dans une fenêtre décorée.

## 2026-10-25 — Vérification du backend DRM/KMS dans l’image live

Fait et vérifié :
- Relecture de `docs/PLAN.md`, du code courant et du journal : le backend DRM/KMS était
  déjà implémenté dans `shell/src/backend/drm.rs`, mais son état et ses vérifications
  n’étaient pas consignés ici.
- `tools/ci.sh` dans Debian 13 WSL : formatage, clippy avec avertissements interdits,
  ShellCheck et **25 tests réussis**.
- Reconstruction de `build/ferrite-live.iso` (environ 1,1 Gio), incluant le binaire actuel
  de `ferrite-shell`.
- `tools/vm.sh --headless 90` : démarrage UEFI de l’image dans QEMU avec virtio-vga ; la
  capture `build/vm-ecran.png` montre le fond Ferrite, une fenêtre `foot` rendue par
  `ferrite-shell` via le backend DRM/KMS virtio, et ses décorations Ferrite.

Limites de cette vérification :
- QEMU confirme le chemin DRM/KMS sur une carte virtuelle, pas encore sur l’Intel Iris Xe
  du ThinkBook ; aucune écriture sur son SSD n’a été faite.
- La VM est en 1280×720 : le mode natif 2880×1800, l’échelle 2 et le Wi-Fi RTL8852BE
  restent à vérifier sur la machine réelle.
- La capture prouve l’affichage d’un client Wayland ; les gestes du pavé tactile, le
  déplacement/redimensionnement à la souris, XWayland, le presse-papiers et la fluidité
  à 60 images/s ne sont pas encore validés.
- Le journal série de la VM ne contient que la console ttyS0 ; le bureau est visible sur
  la sortie graphique QEMU. Les erreurs détaillées du compositeur sont écrites dans le
  journal de session utilisateur `/run/user/<uid>/ferrite-shell.log`.

État : le lancement graphique minimal de la phase 2 est maintenant vérifié en QEMU. La
phase 2 reste incomplète tant que les critères sur le ThinkBook et les fonctions ci-dessus
ne sont pas vérifiés.

## 2026-10-25 — Accrochage des fenêtres aux bords

Fait :
- Le relâchement d'une fenêtre à moins de 24 px d'un bord horizontal l'accroche à la
  moitié gauche ou droite de l'écran ; la barre de menus reste dégagée.
- `ferrite-shell` annonce l'état `tiled_left` ou `tiled_right` au client et demande une
  taille adaptée. Le double-clic sur la barre de titre restaure la géométrie antérieure.
- Les deux moitiés sont calculées sans perte de pixel sur les largeurs impaires ; des
  tests couvrent les côtés, les dimensions et le seuil des bords.

Vérifié :
- `tools/ci.sh` réussit après le changement : formatage, clippy sans avertissement,
  ShellCheck et **27 tests** (22 pour ferrite-shell, 5 pour ferrite-ui).
- ISO reconstruite et redémarrée dans QEMU UEFI : la capture `build/vm-ecran.png` montre
  toujours le fond Ferrite et la fenêtre `foot` décorée.

À vérifier : l'accrochage lui-même et sa restauration n'ont pas encore été actionnés à la
souris dans QEMU ou sur le ThinkBook ; les tests actuels valident la géométrie et la
détection des bords, pas le geste complet du client Wayland.

## 2026-10-25 — Dock et applications de base

Fait :
- Ajout d'un dock Ferrite avec des lanceurs pour Fichiers, TextEdit, Terminal et
  Calculatrice.
- Ajout de `ferrite-apps` : explorateur de fichiers, éditeur de texte UTF-8 avec
  ouverture/enregistrement et calculatrice avec opérations usuelles et parenthèses.
- Intégration de `ferrite-apps` dans la construction de l'image live.

Vérifié :
- `tools/ci.sh` passe : formatage, clippy sans avertissement, ShellCheck et **32 tests**.
- Les trois applications graphiques Fichiers, TextEdit et Calculatrice ont été lancées
  comme clients Wayland dans la session de développement WSLg et capturées.
- L'ISO a été reconstruite puis démarrée dans QEMU UEFI pendant 90 secondes ; la capture
  `build/vm-ecran.png` montre le bureau Ferrite, le terminal et les quatre icônes du dock.
- SHA-256 de `build/ferrite-live.iso` :
  `b8fa8398675e6abd8e00b74f7b2881221f50ca705ab8ae780713c23d9409ace2`.

Limites :
- Les clics sur les lanceurs dans QEMU n'ont pas été automatisés ; le lancement via le
  dock est relié aux exécutables, mais les tests ne valident pas encore le parcours complet
  clic → ouverture de chaque application dans la VM.
- Fichiers couvre la navigation et l'ouverture de texte ; il ne fournit pas encore toutes
  les fonctions d'un gestionnaire complet (copie, déplacement, corbeille, montage USB).
- TextEdit sauvegarde du texte UTF-8 ; calculatrice couvre les opérations arithmétiques.
  Terminal avancé, réglages système, lecteur multimédia et visionneuse PDF restent à faire.
- La vérification a été faite dans QEMU, pas sur le ThinkBook physique.

## 2026-10-26 — Bureau épuré et icônes SVG

Fait :
- Icônes du dossier `icons/SVG` rendues par `resvg` dans ferrite-shell et ferrite-apps.
  Le module `shell/src/desktop/svg.rs` remplace les icônes dessinées à la main.
- Icônes adaptées : `$` retiré du Terminal, Musique redessinée (tuile violette et double
  croche), panier au centre de la Boutique. Quatre icônes ajoutées dans le même style :
  `app-text-editor`, `app-calculator`, `file-text`, `file-generic`.
- Bureau façon elementary OS :
  - barre fine et translucide en haut : Applications à gauche, date et heure locales
    en français au centre, Bluetooth, Wi-Fi, volume, batterie et alimentation à droite,
    avec des icônes monochromes ;
  - menu Applications sous forme de grille d'icônes ;
  - réglages rapides : interrupteurs Wi-Fi et Bluetooth, curseurs du volume et de la
    luminosité ;
  - menu Session (Redémarrer, Éteindre), avec une boîte de confirmation ;
  - plus d'icônes sur le bureau ni de panneaux ouverts au démarrage.
- Dock clair et translucide avec les icônes SVG. Un point s'affiche sous chaque application
  ouverte, reconnue par son `app_id`.
- Fenêtres harmonisées :
  - la barre de titre se prolonge dans l'en-tête des applications, sans trait ;
  - Fichiers a une barre latérale d'emplacements, un chemin cliquable, Précédent et
    Suivant, une liste à colonnes avec des icônes par type et une relecture automatique
    du dossier ;
  - TextEdit a un en-tête, une barre d'état et un titre de fenêtre qui suit le document ;
  - la Calculatrice a un écran et un pavé de touches, et fonctionne aussi au clavier.
- Les fenêtres s'ouvrent au-dessus du Dock et sont recentrées selon leur taille réelle.
- Ajout de l'option `--menu applications|reglages|session` pour les captures.
- Ajout de la configuration de `foot` (`/etc/xdg/foot/foot.ini`) et du paquet
  `fonts-jetbrains-mono` à l'image.

Vérifié :
- `tools/ci.sh` passe : formatage, clippy sans avertissement, ShellCheck et **46 tests**
  (33 pour ferrite-shell, 8 pour ferrite-apps, 5 pour ferrite-ui).
- Captures dans WSLg (`tools/shell-dev.sh --capture`) :
  - bureau vide ;
  - chacun des trois menus ;
  - Fichiers, TextEdit, Calculatrice et `foot`.

À vérifier :
- l'ISO n'a pas été reconstruite ni démarrée dans QEMU après ces changements ;
- la police JetBrains Mono de `foot` n'a pas été vue à l'écran (absente de WSL) ;
- les clics dans les menus et le Dock ne sont couverts que par des tests de géométrie,
  pas par un parcours à la souris ;
- la boîte de confirmation d'arrêt n'a pas été capturée.

