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

## 2026-10-26 — Stabilité du bureau et Paramètres

Problème constaté dans l'image live (QEMU) : un clic sur Bluetooth figeait tout le
bureau. `bluetoothctl power off` attend indéfiniment quand il n'y a ni adaptateur ni
démon, et le compositeur l'attendait dans sa boucle principale.

Fait :
- Nouvelle crate `ferrite-system` (`services/system`) : appels de `nmcli`,
  `bluetoothctl`, `wpctl`, `brightnessctl` et `timedatectl` avec un délai maximal
  (processus tué au-delà), lecture de l'état réel et réglages de l'utilisateur
  (`~/.config/ferrite/reglages`).
- Les réglages du compositeur passent par un fil d'exécution à part
  (`shell/src/system.rs`), qui relit l'état du système toutes les 8 s.
- Le matériel absent n'est plus proposé : l'indicateur disparaît de la barre et le
  réglage est grisé (« Aucun adaptateur », « Luminosité non réglable »). Un refus du
  système s'affiche dans les réglages rapides.
- Les applications lancées sont attendues par un fil, pour ne plus laisser de processus
  zombies.
- Raccourcis clavier : Échap ferme un menu ou la confirmation, Super ouvre le menu
  Applications.
- `ferrite-session` relance le bureau après un arrêt sur erreur, au plus 3 fois en
  2 minutes.
- Nouvelle application **Paramètres** (`ferrite-apps settings`), accessible depuis le
  Dock, le menu Applications et les réglages rapides :
  - Wi-Fi et réseau : liste des réseaux, connexion avec mot de passe ;
  - Bluetooth : recherche, appairage et connexion des appareils ;
  - Son : volume, muet, choix de la sortie ;
  - Écran : luminosité ;
  - Clavier : disposition appliquée aussitôt, QWERTY américain par défaut ;
  - Date et heure : fuseau, mise à l'heure automatique ;
  - À propos.
- Nouvel outil `tools/vm-test.sh` : il rejoue un scénario de clics dans l'ISO sous QEMU,
  avec des captures (`tools/scenarios/stabilite.txt`).

Vérifié :
- `tools/ci.sh` passe : formatage, clippy sans avertissement, ShellCheck et **56 tests**
  (35 pour ferrite-shell, 9 pour ferrite-apps, 7 pour ferrite-system, 5 pour ferrite-ui).
- ISO reconstruite, puis `tools/vm-test.sh tools/scenarios/stabilite.txt` dans QEMU UEFI :
  - le Terminal s'ouvre depuis le Dock ;
  - le clic sur Bluetooth sans adaptateur n'a aucun effet, et le bureau reste réactif
    (un clic extérieur ferme le menu) ;
  - Paramètres s'ouvre depuis le Dock.
- Paramètres et les réglages rapides ont aussi été capturés dans WSLg, sans matériel.

À vérifier :
- sur le ThinkBook : Wi-Fi réel (connexion avec mot de passe), Bluetooth réel,
  luminosité et changement de fuseau (`timedatectl` passe par `sudo -n` faute d'agent
  polkit) ;
- le redémarrage automatique du bureau n'a pas été provoqué par un vrai plantage ;
- dans cette ISO, sans carte Wi-Fi, la page Réseau affichait encore une erreur de liste.
  Le correctif qui la masque est postérieur à la construction de l'ISO.

## 2026-10-26 — Compatibilité avec les applications Linux

Fait :
- XWayland lancé par ferrite-shell, qui gère les fenêtres X11 :
  - placement, focus, décorations Ferrite si l'application ne dessine pas les siennes ;
  - déplacement, redimensionnement, agrandissement, plein écran, réduction ;
  - menus X11 posés à la position demandée ;
  - boîtes de dialogue centrées sur leur fenêtre parente et gardées au-dessus d'elle ;
  - taille limitée à la zone utile.
- `DISPLAY` est transmis aux applications lancées et à D-Bus.
- Correctifs trouvés en ajoutant X11 :
  - un `unwrap()` du redimensionnement aurait planté sur toute fenêtre non Wayland ;
  - les fenêtres X11 décorées n'étaient pas dessinées du tout.
- Protocoles ajoutés : `linux-dmabuf` (avec retour du périphérique de rendu),
  viewporter, mise à l'échelle fractionnaire, activation, sélection primaire et pointeur
  relatif.
- Menu Applications alimenté par les fichiers `.desktop` (Debian, Flatpak, utilisateur) :
  - icônes SVG et PNG du thème hicolor ;
  - mise à jour dès qu'une application est installée ;
  - recherche au clavier, pages de 24, molette ;
  - Entrée lance le premier résultat.
- Portails GTK (`xdg-desktop-portal-gtk`) dans l'image.
- `tools/vm-test.sh` :
  - frappe fiable par QMP (étape `type`) ;
  - service d'un script à la machine virtuelle (étape `serve`) ;
  - scénario `tools/scenarios/applications.txt`.

Vérifié :
- `tools/ci.sh` passe : **60 tests** (36 pour ferrite-shell, 10 pour ferrite-system, 9 pour
  ferrite-apps, 5 pour ferrite-ui).
- Dans QEMU, avec l'ISO reconstruite (scénario `applications.txt`) :
  - VLC (Debian) et Brave (dépôt officiel) s'installent depuis le Terminal ;
  - les deux apparaissent dans le menu avec leur icône, sans redémarrer ;
  - la recherche « vlc » puis Entrée ouvre VLC, avec la barre de titre Ferrite ;
  - la recherche « brave » ouvre Brave, en français, avec son propre cadre.
- Le scénario de stabilité passe toujours.

À vérifier :
- les deux derniers correctifs ne sont pas encore dans l'ISO testée :
  - dialogue de VLC passé derrière sa fenêtre ;
  - Brave qui se redimensionne sous le Dock ;
- lecture vidéo, son et accélération GPU (`linux-dmabuf`) sur le ThinkBook ; QEMU n'a pas
  de GPU ;
- copier-coller entre applications X11 et Wayland : non relié pour l'instant ;
- fenêtres X11 à l'échelle 2 du ThinkBook : elles seront agrandies, donc moins nettes ;
- dans une machine virtuelle très chargée, le relâchement des touches peut arriver en
  retard et le terminal répète la lettre.

## 2026-10-26 — Gel au déplacement des fenêtres, luminosité, test rapide en VM

Problèmes signalés sur le ThinkBook : le Wi-Fi se connecte, mais la luminosité ne change
pas et le système plante quand on déplace une fenêtre.

Fait :
- **Gel au déplacement** : reproduit dans QEMU (scénario `fenetres.txt`, nouvelle étape
  `drag`), puis diagnostiqué par la console série. Une pile `gdb` lisible a été obtenue
  grâce au mode développement ci-dessous. Deux interblocages, figeant tout le compositeur :
  - au relâchement, l'accrochage lisait la position par `PointerHandle`, dont le verrou
    est déjà tenu pendant le déplacement ; il passe désormais par `handle` ;
  - `snap_window_to_side` lisait la taille de la fenêtre en tenant déjà le verrou des
    données de sa surface.
- **Luminosité** : tous les rétroéclairages sont réglés, et non plus seulement le premier
  par ordre alphabétique (souvent `acpi_video0`, sans effet sur Intel). Trois méthodes
  sont essayées : écriture dans `/sys`, `SetBrightness` de logind, `brightnessctl`. Les
  curseurs des réglages rapides se font maintenant glisser.
- **Mode développement** :
  - `ferrite-dev.service` monte en 9p le partage `ferrite-dev` fourni par QEMU ;
  - `ferrite-session` utilise alors ces binaires, et relance le bureau quand on le quitte ;
  - `FERRITE_VM_DEV=1 tools/vm.sh` ou `tools/vm-test.sh` partage `target/release` ;
  - après une modification : `cargo build --release` (environ 20 s), puis
    Ctrl+Alt+Retour arrière dans la VM, sans reconstruire l'ISO.
- **Ferrite VM** (`tools/vm-manager`, `tools/vm-gui.sh`, `toolserrite-vm.cmd` sous
  Windows) : outil graphique pour :
  - choisir l'ISO, la mémoire, les processeurs, le réseau et le son ;
  - démarrer, redémarrer, éteindre la VM, prendre des captures ;
  - « Recompiler et relancer le bureau » ;
  - lancer les scénarios de test, avec journal et aperçu de la dernière capture.
- `tools/vm-test.sh` : étapes `drag` et `serial` (console série dans
  `build/vm-serie.log`).

Vérifié :
- `tools/ci.sh` passe : **62 tests**.
- Scénario `fenetres.txt` en mode développement, dans QEMU :
  - déplacements, accrochage à droite et à gauche, fenêtres hors de l'écran ;
  - Terminal déplacé dans tous les sens ;
  - le bureau répond toujours à la fin.
  Avant le correctif, le même scénario figeait le bureau dès le premier relâchement.
- Ferrite VM s'ouvre et s'affiche correctement (capture dans le bureau de développement).

À vérifier :
- la luminosité sur le ThinkBook (QEMU n'a pas de rétroéclairage) ;
- les déplacements de fenêtres sur le ThinkBook ;
- l'ISO actuelle contient le premier correctif du gel mais pas le second (accrochage) :
  elle est à reconstruire avant de refaire la clé USB.

## 2026-10-26 — Dock masquable, plein écran, finitions des applications

Fait :
- Barres latérales de Fichiers et Paramètres : un composant commun dessine l'icône et le
  texte, si bien que toute la ligne est cliquable. Avant, le texte captait la souris et se
  laissait sélectionner. La sélection du texte des libellés est coupée partout.
  - Fichiers : groupes « Favoris » et « Appareils », icônes monochromes dans la couleur
    d'accent.
  - Paramètres : pastilles colorées, nouvelle rubrique « Bureau et Dock ».
- Conversion « symbolique » des icônes déplacée dans `ferrite-ui`, partagée avec les
  applications.
- Icônes redessinées : Fichiers (proportions carrées), Wi-Fi (arcs épais), réseau filaire
  (prise Ethernet).
- Dock :
  - taille (petit, moyen, grand) et masquage (quand une fenêtre le recouvre, toujours,
    jamais) réglables ; appliqués en une seconde ;
  - glissement animé ;
  - retour quand la souris touche le bas de l'écran, ou avec la touche Super ;
  - la bande réservée en bas n'existe plus qu'en mode « jamais ».
- Plein écran :
  - géré pour les applications Wayland ;
  - la barre du haut et le Dock s'effacent et reviennent quand la souris touche le bord ;
  - F11 dans les applications Ferrite.
- Jauges du volume et de la luminosité : une valeur réglée à la main n'est plus écrasée
  pendant 2 s par une relecture plus ancienne du système. C'est ce qui faisait trembler
  le curseur et l'icône.
- XWayland n'est plus lancé sous WSLg, où `/tmp/.X11-unix` est en lecture seule.
- `tools/vm-test.sh` : étape `move` ; scénario `tools/scenarios/dock.txt`.

Vérifié (QEMU en mode développement, sans reconstruire l'ISO) :
- `tools/ci.sh` passe : **64 tests**.
- la rubrique « Bureau et Dock » s'affiche ;
- TextEdit en plein écran (F11) : barre du haut et Dock masqués ;
- la souris en haut rappelle la barre, en bas le Dock ;
- le Dock se masque quand le Terminal le recouvre.

À vérifier :
- le changement de taille du Dock depuis Paramètres, testé seulement par les tests
  unitaires ;
- les jauges sur le ThinkBook (QEMU n'a ni rétroéclairage ni glissement de curseur
  réaliste) ;
- l'ISO n'est pas reconstruite.


## 2026-10-26 — Fichiers complet, TextEdit moderne

Fichiers (`apps/core/src/files/`) :
- vues liste (colonnes triables, tri naturel « image2 » avant « image10 ») et grille ;
  vue, tri et fichiers cachés gardés dans `~/.config/ferrite/fichiers` ;
- chemin cliquable ou saisi (Ctrl+L), précédent/suivant/parent, recherche ;
- sélection multiple (Ctrl, Maj, Ctrl+A), menus contextuels, raccourcis habituels ;
- copier, couper, coller, dupliquer en arrière-plan avec progression et annulation ;
  renommer (F2), nouveau dossier (Ctrl+Maj+N) ;
- Corbeille au format freedesktop (restaurer, vider), suppression définitive confirmée ;
- ouvrir avec…, propriétés (taille des dossiers calculée à part), terminal ici ;
- clés USB : montage par `udisksctl` et éjection ;
- `ferrite-apps files CHEMIN` ouvre un dossier ou montre un fichier ; `xdg-open` d'un
  dossier ouvre Fichiers (`ferrite-files.desktop`, `/etc/xdg/mimeapps.list`) ;
- mode sélecteur (Ouvrir, Enregistrer sous, confirmation avant de remplacer) ;
- icônes « emplacements » au trait (`icons/SVG/places-*.svg`), faites pour être teintées.

TextEdit (`apps/core/src/editor.rs`) :
- onglets, glisser-déposer, fichier déjà ouvert = retour à son onglet ;
- Ouvrir et Enregistrer sous affichent le sélecteur de Fichiers en feuille ;
- numéros de ligne, ligne courante, retour à la ligne au choix, taille du texte
  (Ctrl+plus, Ctrl+moins, Ctrl+0) ;
- rechercher (Ctrl+F, F3, occurrences surlignées) et remplacer (Ctrl+H) ;
- demande avant de fermer un onglet ou de quitter avec des modifications ;
- barre d'état : ligne, colonne, mots, caractères, fins de ligne.

Vérifié :
- clippy sans avertissement, tous les tests passent (20 pour `ferrite-apps`) ;
- captures dans `tools/shell-dev.sh` : Fichiers en liste et en grille, TextEdit,
  feuille « Enregistrer sous », recherche et remplacement.

À vérifier :
- les opérations à la souris et au clavier dans QEMU (scénario
  `tools/scenarios/fichiers.txt` écrit, pas encore lancé) ;
- l'ISO n'est pas reconstruite.

## 2026-10-27 — Barre du haut masquable

- Paramètres > Bureau et Dock > « Barre du haut » : jamais (sauf en plein écran, comme
  avant, par défaut), quand une fenêtre la recouvre, ou toujours (clé `barre_masquage`).
- Masquable, elle ne réserve plus de bande : les fenêtres agrandies montent jusqu'en haut.
- Masquée, elle revient quand la souris touche le bord du haut (2 px), puis reste tant que
  la souris est dessus : la barre de titre d'une fenêtre agrandie reste accessible. Même
  règle en plein écran.

Vérifié : clippy, tous les tests ; capture du bureau en mode « toujours » (barre absente).
À vérifier : le mode « quand une fenêtre la recouvre » avec une fenêtre agrandie, et le
retour de la barre à la souris, dans QEMU.

## 2026-10-27 — Moniteur d'activité, accrochage des fenêtres, historique du presse-papiers

- **Moniteur d'activité** (`ferrite-apps monitor`, menu Applications) : processeur (total
  et par cœur), mémoire, disques (débits et occupation), réseau, sur une minute ; liste des
  processus triable, recherche, « Mes processus », terminer ou forcer l'arrêt (confirmé).
- **Fenêtres** : zone de redimensionnement élargie (10 px autour du cadre, coins de 20 px),
  curseurs à double flèche ; accrochage comme sous Windows : bord gauche ou droit (moitié),
  coins (quart), bord du haut (tout l'écran), avec aperçu ; une fenêtre accrochée qu'on
  tire reprend sa taille sous le pointeur.
- **Historique du presse-papiers (Super+V)**, comme Windows + V : 25 éléments (texte et
  images PNG), épingler (gardé après redémarrage), supprimer, tout effacer ; flèches et
  Entrée, ou clic, collent l'élément dans l'application active. La touche Super seule ouvre
  désormais le menu Applications au relâcher.
- **Presse-papiers partagé** : les applications Ferrite étaient compilées sans le
  presse-papiers du système (option `clipboard` d'eframe) ; le pont X11 ↔ Wayland
  manquait aussi (copier dans Brave, coller dans TextEdit). Les deux sont faits.
- Fichiers : Ctrl+C/Ctrl+V fonctionnent sous Wayland, Ctrl+L remplace le chemin ;
  TextEdit place le curseur dans le texte dès l'ouverture.

Vérifié dans QEMU (mode développement) : scénarios `accrochage.txt` (moitié, quart, plein
écran, décrochage), `fichiers.txt` (copie « essai (copie) », chemin saisi),
`presse-papiers.txt` (deux copies, choix au clavier, collage). Clippy et tous les tests.
À vérifier : copier-coller avec une application X11 (Brave) et une image, sur le PC.

## 2026-10-27 — Dock dynamique, une fenêtre par application, nouveau cadre des applications

- **Dock** : applications épinglées puis, après un trait, les applications ouvertes non
  épinglées ; clic droit : garder / retirer du Dock, nouvelle fenêtre (si permise),
  quitter (`dock_epingles`). Clic sur une application ouverte : sa fenêtre revient, même
  réduite ; déjà devant, on passe à sa fenêtre suivante.
- **Agrandissement** des icônes sous la souris (+22 %, sur place ; le fond garde sa
  taille), désactivable (`dock_agrandissement`).
- **Réduction animée** vers l'icône du Dock (rétrécit et s'estompe, 320 ms) et retour.
- **Une seule fenêtre par application** : le menu et le Dock ramènent la fenêtre ouverte ;
  un deuxième lancement d'une application Ferrite (TextEdit ouvert depuis Fichiers) confie
  son fichier à la première instance (`$XDG_RUNTIME_DIR/<app_id>.sock`). Exceptions dans
  Paramètres > Bureau et Dock > Fenêtres (`fenetres_multiples`, Fichiers par défaut).
- **Cadre des applications**, dans l'esprit de macOS : colonne colorée sur toute la
  hauteur avec les boutons de fenêtre en haut, barre d'outils et contenu clairs à droite.
  Les applications Ferrite dessinent leur haut (`with_decorations(false)`) ; ferrite-shell
  garde coins arrondis (les quatre), ombre et bords de redimensionnement.
- **Paramètres** comme les Réglages de macOS : recherche, carte de l'utilisateur,
  rubriques groupées en tuiles colorées, choix en bleu, précédent / suivant et titre.
- Boutons de fenêtre plus grands (rayon 7, pas 23), dans le compositeur et les applications.

Vérifié dans QEMU (mode développement, scénario `dock-dynamique.txt`) : une seule fenêtre
de TextEdit, Moniteur après le trait, agrandissement, réduction par le bouton jaune de
l'application et retour par le Dock. Clippy et tous les tests. Captures du nouveau cadre
(Paramètres, Fichiers, TextEdit, Moniteur, Calculatrice).
À vérifier : menu du clic droit sur le Dock (pas d'étape « clic droit » dans les
scénarios), déplacement d'une fenêtre par sa barre d'outils, sur le PC.

## 2026-10-28 — Aspect du Ferrite d'origine

Le bureau et les applications reprennent l'apparence du Ferrite d'origine (`D:\Os`) :

- **Fond d'écran** : dégradé bleu nuit → violet → corail, halo en haut à droite et quatre
  collines ondulées translucides (`paint_wallpaper` d'origine), à la résolution de l'écran.
- **Barre du haut** claire (presque blanche), texte sombre, logo en losange devant
  « Applications », date et heure à droite avant les indicateurs ; le menu ouvert est
  marqué par une pastille grise.
- **Dock** : verre blanc translucide (rayon 20), liseré blanc, point sombre sous les
  applications ouvertes ; icônes refaites en carrés arrondis à dégradé (Fichiers,
  TextEdit, Terminal, Calculatrice, Paramètres, Moniteur, Horloge).
- **Barres de titre** des autres applications : dégradé F0EEF1 → E4E2E5, reflet blanc,
  trait dessous, titre gris foncé (clair si inactive).
- **Applications Ferrite** : panneau latéral gris flottant aux coins arrondis (Fichiers,
  Paramètres), barre d'outils en dégradé sans panneau (TextEdit, Moniteur, Calculatrice),
  groupes de boutons en pilule, contrôle segmenté grille / liste, accent 0A7AFF.

Vérifié : clippy, tous les tests ; captures dans QEMU (mode développement, scénario
`aspect-origine.txt`) : bureau, Fichiers, TextEdit, terminal foot, menu Applications.
À vérifier : rendu sur l'écran du ThinkBook (échelle 2).

## 2026-10-30 — Tokens visuels partagés

`ferrite-ui` devient la source commune des mesures et des rôles de couleur : espacements,
rayons, bordures, élévation, typographie, dimensions des contrôles, mouvement et couleurs
sémantiques. `theme::Palette` définit les variantes claire et sombre sans dépendre d'un moteur
de rendu. L'adaptateur egui utilise ces valeurs pour les surfaces, la typographie, les
contrôles et les bordures ; `ferrite-shell` les utilise pour les titres, le rayon et l'ombre
des fenêtres. Les deux moteurs restent découplés.

Vérifié : `cargo test -p ferrite-ui -p ferrite-apps -p ferrite-shell` (12 tests UI, 40
applications, 88 shell), `cargo clippy -p ferrite-ui -p ferrite-apps --all-targets -- -D
warnings`, `cargo fmt --all --check` et `git diff --check`.

La hauteur de barre locale de 34 px est conservée ; le test ThinkBook vérifie désormais ses
68 px physiques à l'échelle 2. La migration des vues propres aux applications continuera à
utiliser ces rôles au fil de leur harmonisation, sans importer egui ou Smithay dans
`ferrite-ui`.

## 2026-11-02 — Thème de la spécification, raccourcis, notifications, verrouillage

Fait :
- Bureau et applications refaits d'après la « Spécification des fenêtres et dispositions » et
  ses planches : palette et accents, deux boutons de fenêtre, barre de titre de 46 px, ombres,
  fond hexagonal, barre du haut (horloge centrée), Dock avec corbeille, centre de contrôle,
  calendrier et notifications, lanceur, Alt+Tab, vue d'ensemble avec bande des espaces,
  dialogues, écran de verrouillage ; Fichiers en grille, Terminal sombre, Paramètres par
  catégories avec page Apparence. Super+V, animations du Dock, génie et widgets conservés.
- Alt+Tab et Alt+`, tous les raccourcis Super (voir `RACCOURCIS.md`), aimantation à 12 px
  d'écart (moitiés, deux tiers, tiers, centrée), tuilage automatique, afficher le bureau,
  envoi sur un espace, coin actif, captures d'écran (écran et zone).
- Serveur de notifications sur le bus de session, verrouillage (Super+L et avant la veille).

Vérifié :
- `cargo fmt`, clippy et les tests de tout l'espace de travail ; rendu en fenêtre.
- Dans QEMU (`tools/scenarios/theme.txt`, mode développement), avec de vraies touches sur le
  backend DRM : Alt+Tab, aimantation, agrandissement, tuilage, vue d'ensemble, centre de
  contrôle, calendrier, captures écrites dans `~/Images`, refus puis acceptation du mot de
  passe par `unix_chkpwd`, Super+D, Super+Q.
- Notifications : `notify-send` et `gdbus` sur un bus isolé, bannières, urgence, fin d'une
  bannière et historique.

Non vérifié : ThinkBook réel (échelle 2, fluidité, pavé tactile), veille réelle.

Reste de la spécification : Mail, Musique, Photos, Vidéos, Navigateur et Logithèque ;
rubriques manquantes de Paramètres et réglages Taille du texte, Barre translucide, Réduire les
animations ; OSD et touches de volume ; menu de fenêtre, appui long sur agrandir, épinglage ;
glisser une fenêtre vers un espace ; recherche de fichiers, réglages et calculs dans le lanceur ; notifications groupées ; thème de curseurs Ferrite ; thème GTK ; écran de
connexion ; accessibilité ; surcouche de débogage.

Puis, à la demande : lanceur en une seule liste de A à Z, fenêtres toujours centrées à
l'ouverture, icônes du Dock qui ne passent plus devant leur menu. Vérifié : CI complète, rendu
en fenêtre pour la liste et le centrage ; le menu du Dock n'a pas été revu à l'écran.

Puis, d'après les nouvelles planches et les captures reçues : Paramètres à 19 rubriques (les 18
de la maquette, plus Date et heure), pages Wi-Fi, Bluetooth, Son, Bureau et dock et Apparence en
groupes de lignes ; centre de contrôle avec listes Wi-Fi et Bluetooth dans le panneau, mot de
passe sur place ; tuile Clavier qui bascule entre français et américain ; icônes centrées dans
le Dock ; agrandissement figé sous le menu d'une icône. Les lignes sans fonction réelle sont
grisées : Position du Dock, Nombre fixe d'espaces, Écran principal uniquement, réception de
fichiers Bluetooth, niveau d'entrée, sons système, thème et taille de curseur, taille du texte.
Rubriques vides, annoncées comme telles : Notifications, Énergie, Imprimantes, Confidentialité,
Accessibilité, Mises à jour.

Puis, d'après les planches « Widgets — refonte », « Verrouillage » et « Surfaces système » :
widgets en grille de 4 colonnes de 170 px sur le côté gauche de chaque bureau, en verre sombre,
sous les fenêtres ; estompe quand une fenêtre les recouvre, Super+W pour les ramener devant ; clic
droit sur le bureau, puis retirer, ajouter, déplacer et redimensionner librement (à la demande, sans grille : la grille reste la disposition d'origine). La page des widgets
disparaît : le premier bureau est la page 0. Écran de verrouillage au repos et en saisie, thème et
quatre fonds propres, notifications sans contenu, lecture en cours (MPRIS), secousse au refus.
Menus contextuels de 32 px et popover « Trier » (tri par type ajouté) dans Fichiers, survol
accentué dans les menus du bureau et du Dock. `curl` manquait à l'ISO : la météo n'y arrivait pas.

## 2026-11-03 — Développer depuis Ferrite ; Wi-Fi stable

Fait :
- `tools/dev-on.sh` (option `--sans-compiler`) et `tools/dev-off.sh` : mode développement
  sur le disque, voir DECISIONS. `ferrite-session` choisit ses binaires à chaque relance
  (QEMU, puis `~/.local/share/ferrite-dev`, puis `/usr/local/bin`), met le dossier choisi en
  tête du PATH pour les applications, et écarte une version de développement après deux
  plantages de suite.
- Wi-Fi : économie d'énergie du pilote rtw89 et de NetworkManager désactivée dans l'image ;
  `ferrite-check` le vérifie.

Vérifié :
- Wi-Fi sur le ThinkBook, réglage posé à la main puis redémarrage : 80 pings vers la box,
  0 % de pertes, 1,3 ms en moyenne (avant : 1,25 % de pertes, pointes à 110 ms).
- `ferrite-session` simulé avec de faux binaires : la version de développement qui plante est
  lancée deux fois puis écartée, la stable prend le relais ; après `dev-off.sh`, Ctrl+Alt+Retour
  arrière relance la stable. `dev-on.sh --sans-compiler` et `dev-off.sh` dans un dossier
  temporaire. `sh -n` et `dash -n` (shellcheck absent de Ferrite).

Reste : installer la nouvelle `ferrite-session` dans `/usr/local/bin` (sudo) et faire un
vrai relancement du bureau avec `dev-on.sh`.
