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

## 2026-11-03 — Protocoles Wayland manquants

Fait :
- `wlr-layer-shell` (v4) : calques par écran (fond et bas sous les fenêtres, haut au-dessus,
  superposition au-dessus de tout), leurs menus, zone exclusive retirée de la place des
  fenêtres agrandies ou accrochées, clavier exclusif (lanceurs) ou à la demande (au clic).
- `ext-session-lock-v1` : un verrouilleur externe (swaylock…) remplace l'écran de
  verrouillage ; seules ses surfaces sont dessinées et reçoivent clavier et pointeur, les
  raccourcis du bureau sont coupés (sauf le changement de terminal virtuel).
- `wlr-screencopy-v1` (v3), écrit à la main (Smithay ne l'a pas) : écran entier ou zone,
  avec ou sans curseur, copiés dans un tampon `XRGB8888` du client à l'image suivante.
- `text-input-v3` et `input-method-v2` (fenêtres de candidats posées près de la fenêtre),
  `cursor-shape-v1` avec quatre nouvelles formes du curseur Ferrite (texte, main,
  déplacement, viseur), `idle-inhibit-v1` et `ext-idle-notify-v1` (toute entrée est une
  activité).

Vérifié :
- `cargo fmt`, clippy et les tests (134 pour le shell).
- Dans le shell en fenêtre, avec un client de test écrit pour l'occasion : les six globaux
  sont annoncés ; une capture par screencopy est juste (sens, couleurs, curseur) ; un calque
  rouge en haut à droite est configuré puis dessiné à sa place ; un verrou est confirmé, sa
  surface couvre l'écran seule, puis le déverrouillage rend la main.

Reste :
- Essayer de vrais clients : waybar, fuzzel, swaybg, swaylock, grim, fcitx5, swayidle.
- Les applications qui envoient leur propre image de curseur (et non un nom) ont la flèche.
- La barre de Ferrite reste dessinée au-dessus des calques « haut ».

## 2026-11-03 — Terminal : Tab et fish ; clics à travers les fenêtres

Fait :
- Terminal : Tab servait aussi à la navigation d'egui, qui donnait le focus aux boutons des
  onglets ; le clavier n'allait plus au shell, et Espace ou Entrée « cliquaient » le bouton
  (jusqu'à fermer l'onglet, donc la fenêtre). La grille garde désormais le clavier (Tab,
  flèches, Échap), sauf quand la recherche ou un menu est ouvert.
- Image : `fish` installé ; `ferrite-session` en fait le `$SHELL` des terminaux du bureau
  (la connexion sur tty1 reste en sh, qui lit `/etc/profile.d`).
- Bureau : un clic sur une fenêtre posée sur le widget « Applications récentes » lançait
  l'application dessous (`DashboardApp` manquait au filtre des clics sous une fenêtre).

Vérifié :
- Test de bout en bout du terminal : trois Tab puis Espace et Entrée, le shell reçoit la
  ligne et l'onglet reste ouvert (le test échoue sans la correction).

## 2026-11-03 — Bluetooth, son et mot de passe Wi-Fi

Fait :
- Bluetooth (`ferrite-system`, donc bureau et Paramètres) : la connexion passe par une
  seule session `bluetoothctl` pilotée, avec un agent `NoInputNoOutput` enregistré pendant
  tout l'échange (en ligne de commande, `bluetoothctl pair` n'en avait pas), `pairable on`,
  et une recherche active jusqu'à voir l'appareil avant de l'appairer. Si l'appareil a
  oublié l'appairage (`br-connection-key-missing`, cas du casque soundcore sur le PC), il
  est oublié puis appairé de nouveau. Erreurs de BlueZ traduites en français au lieu de
  « Connexion refusée ». Appareils sans nom masqués. Un casque connecté devient la sortie
  par défaut (`pactl set-default-sink bluez_output.…`).
- Panneau Bluetooth : « Connexion… » sur la ligne, boutons inactifs pendant l'opération.
- Image : `libspa-0.2-bluetooth` (sans lui, PipeWire ne crée aucune sortie Bluetooth),
  `rtkit`, et un hook qui active PipeWire, pipewire-pulse, WirePlumber et bluetooth.
- Panneau Wi-Fi : un œil sur les réseaux enregistrés affiche leur mot de passe (`nmcli -s`),
  et un œil dans le champ de saisie montre le texte tapé.

Vérifié :
- Tests unitaires (sortie de `bluetoothctl`, erreurs, sorties `bluez_output`, mots de passe
  échappés par `nmcli`, clics sur les yeux) ; session réelle sur le PC : appairage détecté,
  `key-missing` reconnu.

Reste :
- Ré-appairer réellement le casque (le mettre en mode appairage) et vérifier le son ;
  essayer l'ISO reconstruite sur le ThinkBook.

## 2026-11-03 — Écran entier, touches maintenues, feuilles de notes et de tâches

Fait :
- Quand la zone utile change (fenêtre hôte ou écran redimensionnés, Dock ou barre réglés),
  le tuilage est recalculé et les fenêtres agrandies ou en plein écran reprennent toute la
  place (`refit_to_area`, à chaque tour de boucle) ; l'ordre d'empilement est conservé.
- Touche maintenue dans les champs dessinés par le shell (notes, tâches, recherche du
  lanceur, mot de passe Wi-Fi, verrouillage, presse-papiers) : répétée avec le délai et la
  cadence annoncés aux applications (300 ms, 30/s). Avant, Retour arrière maintenu
  n'effaçait qu'un caractère.
- Widget Tâches : plusieurs feuilles en onglets (« + » en crée une, nommée à la saisie ;
  croix sur l'onglet affiché pour la supprimer), chaque feuille garde ses tâches ; une
  croix au bout de chaque tâche la supprime. Anciennes tâches reprises dans « Tâches ».
- Widget Notes : corbeille de la note affichée, et position « Notes 2/3 ».

Vérifié :
- Tests (136 pour le shell) : feuilles, notes, clics sur onglets, croix et corbeille.
- Capture du bureau en fenêtre avec une configuration vide.
- Correctif : la zone utile était croisée avec la zone libre des calques (`layer-shell`),
  qui n'est recalculée que par `LayerMap::arrange`. Elle gardait la taille de l'écran au
  moment du dernier calcul : fenêtre hôte agrandie (ou écran changé), les fenêtres agrandies
  s'arrêtaient à ~88 % de la largeur. `usable_area` et le redimensionnement en fenêtre
  appellent maintenant `arrange` avant de la lire.

## 2026-11-03 — Dock : corbeille en double, icônes manquantes, curseur

Fait :
- Corbeille en double : un clic sur la corbeille ouvre Fichiers, mais le rebond attendait
  une fenêtre « corbeille » qui n'arrive jamais ; la corbeille passait pour une application
  ouverte et revenait une deuxième fois, en rebondissant 12 s. Le rebond attend maintenant
  Fichiers, et la corbeille garde sa seule case, sans point.
- Fenêtres sans `.desktop` visible (foot, caché du menu ; programmes lancés à la main) :
  icône du thème au nom de leur identifiant, au lieu de la loupe (cherchée une fois).
- Pointeur sur le Dock ou la barre du haut : l'application dessous ne le reçoit plus et le
  curseur reste une flèche (un champ de texte derrière le Dock imposait le curseur texte).

Vérifié :
- Tests (136) et clippy ; capture en fenêtre avec foot : son icône apparaît dans le Dock.

## 2026-11-03 — Construction de l'ISO plus rapide

Fait : voir DECISIONS (construction incrémentale). `tools/vm-build.sh` : image locale
`ferrite-build`, volumes `ferrite-cargo-registry` et `ferrite-vm-target`. `tools/usb.sh` :
chemin rapide (`lb binary` seul) si la configuration de l'image n'a pas changé, options
`--complet`, `--final` (xz) et `--propre`. `tools/usb-fedora.sh` : image locale
`ferrite-live-build`.

Vérifié : `tools/vm-build.sh` — première fois 5 min (image de compilation et tout le code),
ensuite 0,6 s sans changement. Le chemin rapide de live-build demande sudo : pas encore
essayé.

## 2026-11-03 — Installation de développement sur disque

Fait : `tools/install-dev.sh /dev/PARTITION`, lancé depuis Fedora. Garde-fous : refuse une
partition montée, présente dans le fstab de Fedora, en swap, NTFS, EFI ou chiffrée ; montre
le système qu'elle contient et demande de retaper son nom. Puis : ext4 « FERRITE »,
debootstrap de Debian 13 et paquets de l'image (sans live-boot) dans le conteneur
`localhost/ferrite-live-build` (`tools/install-dev/inside.sh`), outils de développement et
rustup, utilisateur de Fedora (même nom et UID) en connexion automatique, disque FICHIERS
monté dans `/media/FICHIERS`, entrée « Ferrite OS » dans le GRUB de Fedora
(`/etc/grub.d/41_ferrite`, menu toujours affiché). Secure Boot est désactivé sur ce PC : le
GRUB de Fedora peut lancer le noyau Debian.

Vérifié : shellcheck ; la liste des paquets se résout dans Debian 13 (`apt-get install -s`).
Pas encore lancé (demande sudo) ; partition prévue : `sda5` (elementary OS, accord donné).

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

## 2026-11-03 — Se déconnecter et relancer le bureau sans commande

Fait :
- Dialogue d'arrêt (bouton rouge du centre de contrôle, écran de verrouillage) : rangée
  « Se déconnecter » et « Relancer le bureau », voir DECISIONS. `PowerAction::LogOut` et
  `RelaunchDesktop` ne passent pas par systemd ; `run` renvoie le code de sortie choisi.
- `--demo-power` ouvre ce dialogue au démarrage, pour les captures.

Vérifié :
- Tests (136, dont les clics sur les deux nouveaux boutons), clippy, fmt.
- Capture en fenêtre avec `--demo-power` : deux rangées alignées, sans chevauchement.
- `ferrite-session` simulé : code 3 → relance ; code 4 puis Entrée → session rouverte
  (Fichiers ouvert comme au démarrage) ; code 4 puis « console » → console.

Reste : essai réel sur le ThinkBook (nouvelle `ferrite-session` à installer avec sudo).

## 2026-11-03 — Dock réordonnable ; touches de fonction

Fait :
- Dock : une icône épinglée tenue puis déplacée change de place en direct ; l'ordre est
  enregistré au relâchement. Sans mouvement (moins de 6 px), c'est un clic, qui ouvre
  désormais l'application au relâchement. La corbeille et les applications ouvertes non
  épinglées ne se déplacent pas.
- Touches de fonction (`desktop/function_keys.rs`) : volume ±5 %, muet, micro coupé,
  luminosité ±5 % (5 % au moins), mode avion, calculatrice, recherche (menu Applications),
  lecture/suivant/précédent (MPRIS), capture d'une zone (touche Lenovo). Volume et
  luminosité se répètent quand la touche est maintenue ; son, micro, luminosité et mode
  avion marchent aussi sur l'écran verrouillé. Bulle au-dessus du Dock pendant 1,5 s.
- Mode avion : `ferrite_system::rfkill` (lecture de `/sys/class/rfkill`, écriture d'un
  événement dans `/dev/rfkill`). Le noyau peut basculer lui-même les radios sur cette touche
  (`rfkill-input`) : le bureau regarde l'état 300 ms après et ne bascule que s'il n'a pas changé.
- `--demo-osd` affiche la bulle pour les captures.

Vérifié :
- Tests (139 pour le shell, 31 pour les services) : place visée dans le Dock, touches du
  ThinkBook 14 G4+ (codes lus dans `/proc/bus/input/devices`), bulle du son, mode avion sur
  un faux `/sys/class/rfkill`. Clippy, fmt. Capture de la bulle du volume.

- Sur le ThinkBook, après « Relancer le bureau » : touches et glisser dans le Dock essayés,
  tout marche.

Reste : F7 (écran externe), la touche du pavé tactile et la touche étoile ne font encore rien.

## 2026-11-03 — Mode avion et rétroéclairage du clavier dans le centre de contrôle

Fait :
- Constat : après F8, le Bluetooth restait bloqué par rfkill (`bluetoothctl power on` →
  `org.bluez.Error.Failed`) alors que le Wi-Fi était revenu. `rfkill-input` bascule chaque
  sorte de radio à part : un état mélangé le reste. La touche impose désormais, 300 ms
  après, l'état voulu à toutes les radios ; allumer le Wi-Fi ou le Bluetooth lève d'abord
  leur blocage rfkill (`rfkill::set_blocked`).
- Centre de contrôle : quatrième rangée, « Mode avion » et « Rétroéclairage » (Éteint,
  Faible, Fort, puis Éteint). Le clavier passe par `SetBrightness` de logind (sous-système
  `leds`), sans droits de root ; son niveau est relu avec l'état du système (Fn+Espace
  reste géré par le micrologiciel).

Vérifié :
- Bluetooth débloqué puis allumé à la main avec le même événement rfkill.
- `SetBrightness leds platform::kbd_backlight` 1 puis 0 depuis la session : appliqué.
- Tests (139 shell, 31 services) dont les clics sur les deux tuiles ; clippy, fmt ;
  capture du centre de contrôle.

Reste : essai réel des tuiles et de F8 avec un état mélangé.

## 2026-11-03 — Visionneuse d'images

Fait (d'après les planches « Visionneuse d'images » clair et sombre de la maquette) :
- `ferrite-apps viewer [image ou dossier]` (`apps/core/src/viewer.rs`), `app_id`
  `ferrite-viewer` : image PNG ou JPEG sur une scène (thème du système), images du dossier
  en bandeau de vignettes, barre flottante (zoom −/+, « Ajusté » ou pourcentage, pivoter à
  gauche et à droite, corbeille avec « Annuler »), boutons précédent/suivant sur les côtés,
  panneau d'informations (dimensions, taille, format, date, dossier, cinq couleurs
  dominantes, « Définir comme fond d'écran »), diaporama plein écran (4 s par image).
- Molette ou pincement : zoom autour du pointeur ; glisser : déplacer l'image zoomée ;
  double-clic : 100 % ou ajustée. Clavier : ← →, Début/Fin, + − 0 1, R et Maj+R, Suppr, I,
  Espace (diaporama, pause), Échap.
- Décodage dans des fils à part (image affichée réduite au maximum des textures, vignettes
  une à une). Sans fichier, elle ouvre `~/Images`. Le panneau se cache si la fenêtre est
  trop étroite.
- Fichiers ouvre les PNG et JPEG dans la Visionneuse ; menu Applications (icône
  `image-viewer`) ; `ferrite-viewer.desktop` et `mimeapps.list` dans l'image.
- `files::trash::trash` renvoie la place de l'élément dans la Corbeille (pour annuler).
- Pas repris de la maquette : « Modifier », « Partager » et Favoris (rien derrière),
  bouton de thème (les applications suivent le thème du système).

Vérifié :
- Tests (4 nouveaux : extensions, réduction, couleurs dominantes, tri du dossier), clippy,
  fmt ; captures dans le bureau en fenêtre, dont `ferrite-aurore.png` (3840 × 2160).
- Le test `terminal::tests::saisie_et_onglets_de_bout_en_bout` échoue (Tab de fish dans le
  terminal) : il porte sur `terminal/mod.rs`, modifié avant cette session, pas sur la
  Visionneuse.
- Sur le ThinkBook, ouverte depuis Fichiers : tout marche.

## 2026-11-03 — Musique et widget Musique

Fait :
- `ferrite-apps music [dossier ou morceau]` (`apps/core/src/music/`), d'après les planches
  « Musique » : bibliothèque de `~/Musique` (étiquettes et pochettes lues par symphonia),
  Écouter maintenant, Albums, Titres, Artistes, recherche, page d'album (Lire, Aléatoire),
  file « À suivre », lecteur (aléatoire, précédent, lecture, suivant, répéter, progression,
  volume). Son décodé par symphonia et joué par `pw-cat` (PipeWire), sans en-têtes ALSA.
- Serveur MPRIS `org.mpris.MediaPlayer2.ferrite_musique` : touches multimédia, écran
  verrouillé et widget le pilotent. Les octets nuls des étiquettes WAV sont retirés (D-Bus
  coupait la connexion).
- Widget Musique (optionnel, « Modifier les widgets ») : pochette, titre, artiste,
  progression, précédent/lecture/suivant ; sans lecteur, un clic ouvre Musique. Le lecteur
  est suivi tant que le widget est posé. Fichiers ouvre les morceaux dans Musique ; entrée
  du menu Applications, `ferrite-music.desktop` et types MIME audio.

Vérifié : tests (6 pour Musique, 140 pour le bureau), clippy ; avec un album de test
silencieux : lecture par pw-cat, métadonnées MPRIS, commande Next par D-Bus, captures de
l'application et du widget.

Reste : widgets Notes de la maquette (Note rapide, Notes récentes, Note épinglée) ; essai
réel avec de vrais morceaux ; « Titres aimés » et playlists.

## 2026-11-03 — Bureau des widgets ; widgets Notes

Fait :
- Bureau 0 réservé aux widgets (voir DECISIONS) : les fenêtres s'ouvrent sur le bureau 1 et
  suivantes, les widgets s'effacent en quittant le bureau 0, Super+W y va et en revient,
  Super+Maj+1…9 visent les bureaux 1 à 9.
- Widgets Notes de la maquette, optionnels : Note rapide (bouton « Écrire »), Notes
  récentes (titre et heure, un clic montre la note dans le widget Notes), Note épinglée
  (lignes « - [ ] » de la note la plus récente qui en a, cochables depuis le bureau, avec
  « 2/4 faites » et sa barre).

Vérifié : tests (142 pour le shell, dont le bureau 0 jamais supprimé et la liste à cocher),
clippy ; captures : Fichiers ouvert au démarrage arrive sur le bureau 1 sans widgets, et
les trois widgets Notes sur une copie des réglages.

- Sur le ThinkBook, après « Relancer le bureau » : bureau des widgets, Super+W et widgets
  Notes essayés, tout marche.

## 2026-11-03 — Saccades : widgets redessinés en plein écran

Constat : machine au repos (95 % de processeur libre, 11 Gio libres, pas de swap), mais
ferrite-shell à ~12 % d'un cœur et une image perdue toutes les ~2 s. Mesure ajoutée
(`render/profile.rs`, activée par le fichier `$XDG_RUNTIME_DIR/ferrite-profil`) : les
widgets étaient redessinés au processeur en plein écran (2560 × 1600) à chaque mesure du
widget Système ou avancée du widget Musique, 12 à 14 ms (dessin des cartes 7-10 ms, copie
du verre 3,5 ms, copie de l'image 1 ms), d'où des images de 20 à 45 ms.

Fait : `DashboardCache` garde les pixels des widgets ; hors édition, seules les cartes dont
les données vivantes changent (`cards::card_live` : heure, mesures, lecture) sont
redessinées sur leur verre, et seule leur zone est renvoyée à la carte graphique
(`MemoryRenderBuffer::render().draw` avec dommages). Tout autre changement redessine tout.
La mesure note aussi ce qui a changé quand les panneaux sont redessinés.

Vérifié : bureau en fenêtre, version optimisée : mise à jour des widgets 2-3 ms (au lieu
de 12-14), pire image au repos 4-10 ms (au lieu de 20-45). Session réelle (DRM) : pire
image au repos sous 1 ms. Tests (143, dont celui des clés vivantes), clippy.

Reste : ouvrir le centre de contrôle coûte encore ~31 ms (une image) ; ~8 % d'un cœur au
repos (essais de rendu toutes les 16 ms, applications ouvertes).

## 2026-11-03 — Dialogue d'arrêt de la maquette

Fait (planches « Dialogue d’arrêt » clair et sombre) : dialogue de 480 px, titre et texte
selon l'action choisie (nombre d'applications ouvertes, compte à rebours de 60 s puis
action automatique), trois tuiles Éteindre (rouge), Redémarrer, Veille ; Annuler et le
bouton de l'action ; Se déconnecter, Verrouiller, Relancer le bureau. Clavier : Super+Échap
ouvre, Entrée confirme, Échap annule, ← → changent d'action. Veille : `systemctl suspend`,
le bureau reste ouvert (seuls l'arrêt et le redémarrage le ferment). Le bloc « Travail non
enregistré » de la maquette n'est pas repris : le bureau ne sait pas ce qui est enregistré.

Vérifié : tests (143, dont les clics du nouveau dialogue), clippy, capture.

Vu en passant : dans les réglages réels, le widget Musique chevauche le widget Système.

## 2026-11-03 — Gestes plus fiables, repos plus léger, mode nuit

Fait :
- Gestes à trois doigts : déplacements non accélérés de libinput (la page suit les doigts
  quelle que soit leur vitesse, ~28 mm par page) ; sens décidé après ~0,8 mm au lieu de
  ~0,2 mm et en faveur de l'horizontal (un geste n'est vertical que s'il l'est 1,6 fois
  plus) : le tremblement des doigts qui se posent en faisait souvent un geste vertical
  sans effet. Bilan de chaque geste dans le journal quand la mesure est activée.
- Repos : après une demi-seconde sans rien à redessiner, les essais de rendu passent de
  16 à 100 ms ; événements système, notifications, verrouillage, météo, glisser de
  fichiers et répétition des touches déclenchent désormais un rendu immédiat.
- Mode nuit (`ferrite_system::night`, `shell/src/desktop/night.rs`) : désactivé,
  toujours, du coucher au lever du soleil (NOAA, ville de la météo) ou heures choisies ;
  température de 2700 à 5500 K ; fondu de 20 min aux bascules ; appliqué par la table de
  gamma de l'écran (DRM). Tuile large dans le centre de contrôle (clic : forcer jusqu'à la
  prochaine bascule ; chevron : Paramètres › Écrans), section Mode nuit dans Paramètres.

Vérifié : tests (145 shell, 35 services : soleil à Ouagadougou, fondu, gains, réglages,
table de gamma, sens des gestes), clippy ; captures de la tuile et de Paramètres.

Gestes recalés sur le pavé du ThinkBook (mesures `ferrite-profil`) : les gestes francs font
76 à 380 unités brutes ; vue d'ensemble dès 70 (au lieu de 400), page entière pour 600 (au
lieu de 1100).

Essayé sur le ThinkBook : gestes à trois doigts dans les quatre sens et mode nuit (teinte
DRM), tout marche.

Images en continu : la mesure compte désormais les images
envoyées à l'écran et celles soumises par chaque application. Le fond du Dock voyait ses
paramètres réécrits à chaque image (`update_uniforms`), ce qui le marquait modifié et
faisait redessiner sans cesse : réécrits seulement si l'échelle change. Après correction,
au repos : préparation 0,3 ms, plus aucun panneau redessiné ; les images restantes (60 à
73/s) suivent l'application Claude, qui en soumet 37 à 58/s même en attente. Écran :
2880 × 1800 à 90 Hz, échelle 200 %.

Écran de verrouillage : une photo peut lui servir de fond (Visionneuse, « Définir comme
fond de verrouillage » ; réglage `verrouillage_fond` en chemin), cinquième pastille
« Photo » dans la personnalisation. Le bureau démarre désormais verrouillé :
`ferrite-session` passe `FERRITE_LOCK=1` à l'ouverture, après une déconnexion et après un
plantage, mais pas pour « Relancer le bureau » (la connexion sur tty1 étant automatique,
c'est le mot de passe de l'écran de verrouillage qui protège la session).

L'écran de verrouillage est clair par défaut (réglage `verrouillage_theme`, plus le thème
du bureau) et la photo s'affiche nette, sans voile. Paramètres > Bureau, groupe « Écran de
verrouillage » : thème, et flou de la photo (`verrouillage_flou`, Aucun à Fort ; calculé à
mi-taille, avec un voile proportionnel pour la lisibilité).

Super restait « tenue » après Super+L : son relâcher arrivait sur l'écran verrouillé, qui
garde les touches, et `super_held` ne retombait jamais ; au retour, chaque touche devenait
un raccourci (L reverrouillait). Il retombe désormais au relâcher même verrouillé, et dès
qu'une touche est frappée sans Super enfoncée. Sur une photo, la date et l'heure prennent
l'encre claire ou foncée selon la luminosité de la zone derrière elles (voile compris),
comme l'invitation du bas (« Appuyez sur une touche… »).

Mot de passe de l'écran de verrouillage : curseur (←, →, Début, Fin), Suppr et Retour
arrière autour de lui, et un œil qui affiche le mot de passe en clair tant qu'il est tenu
(il ne rejoint le dessin que pendant ce temps). Passage du repos à la saisie fluide :
chaque image redessinait tout l'écran (21 ms, dont 10 ms pour le seul voile plein écran,
plus l'envoi de 20 Mo). L'écran est maintenant fait de couches dessinées une fois (fond,
voile d'un pixel étiré, boutons, contenu du repos ou de la saisie, date, heure) que la
carte graphique fond, déplace et réduit ; l'invitation du bas s'efface en saisie.

Au repos, la touche qui fait apparaître le champ du mot de passe n'y est plus écrite.

La session ne lance plus Fichiers au démarrage : le bureau s'ouvre sur l'espace des widgets.

Navigateur lent après le démarrage : Brave (et Claude) demandaient le coffre à mots de
passe `org.freedesktop.secrets` sur le bus privé de `dbus-run-session`. Le gnome-keyring
démarré par systemd vit sur le bus de systemd ; celui que lance le bus privé le trouve, s'efface
sans prendre le nom, et l'activation échoue au bout de 120 s. La session utilise désormais
le bus de systemd (`dbus-run-session` en secours), et l'environnement est aussi transmis à
systemd (`--systemd`). Une résolution DNS de 51 s vue une fois n'a pas été reproduite
(serveurs testés : réponses en 30 à 40 ms, aucune perte sur 60 requêtes).

Trousseau demandé au démarrage : la connexion automatique sur tty1 ne donne jamais le mot de
passe à PAM, et le « Default Keyring » restait fermé jusqu'à ce que Brave le réclame. L'écran
de verrouillage, qui vérifie déjà le mot de passe, ouvre maintenant le trousseau par défaut
avec (interface interne de gnome-keyring `UnlockWithMasterPassword`). Il faut que le
trousseau ait le même mot de passe que la session ; sinon rien ne change.

Image à distribuer alignée sur le système installé : coffre à mots de passe (gnome-keyring,
libpam-gnome-keyring, gcr pour la fenêtre de déverrouillage) et ntfs-3g ajoutés aux
paquets. Sur le système live de l'ISO (`boot=live`), le bureau ne démarre pas verrouillé :
son utilisateur n'a pas de mot de passe connu. Les fichiers de includes.chroot étaient déjà
plus récents que ceux du système installé (commentaires, associations Visionneuse et
Musique, vérification Wi-Fi de ferrite-check).

Verrouillage et mise en veille automatiques (`ferrite_system::idle`, `desktop/idle.rs`) :
réglages `verrouillage_auto`, `veille_batterie`, `veille_secteur` en minutes (5, 15 et 30
d'origine, 0 : jamais), page Énergie de Paramètres. Le bureau compte depuis la dernière
entrée, vérifie toutes les 5 s par une minuterie ; une vidéo (`zwp_idle_inhibit_v1`) vaut
une présence, du son en cours (flux actif dans `wpctl status`, casques Bluetooth compris ;
ALSA en secours) reporte la veille. Essayé en bureau imbriqué : verrouillé après 60 s.

Dock masqué automatiquement : il se cache dès que la souris le quitte (plus de délai de 1,5 s, `REVEAL_HOLD` retiré).

Fichiers montre aussi les partitions internes de données (FICHIERS, la partition de
Windows) : montées sous /media ou /mnt, ou pas montées et d'au moins 4 Gio ; les
partitions techniques (EFI, réservée, récupération, échange) restent cachées. Une
partition interne s'ouvre en lecture seule, après le mot de passe administrateur, dans un
fil à part. Agent polkit dans ferrite-shell (`desktop/polkit.rs`, fenêtre
`panels/auth.rs`) : inscrit pour la session de connexion sur le bus système, il affiche
« Authentification requise » et fait vérifier le mot de passe par
`polkit-agent-helper-1` ; pas d'agent dans un bureau imbriqué (`--demo-auth` pour les
captures). Un dossier illisible propose « Ouvrir en administrateur » (`pkexec`, ajouté à
l'image).

## 2026-11-05 — Lecteur PDF (Poppler, zoom, ajustement à la page ou à la largeur)

Fait :
- `apps/core/src/pdf_viewer.rs` : véritable lecteur PDF. Les pages sont rendues par
  Poppler (`pdftoppm -scale-to`), décodées en `ColorImage` (PNG lu par la caisse `png` ;
  RGB, gris et gris+alpha convertis en RGBA) puis envoyées une seule fois à la carte
  graphique. Un rendu par demande, dans un fil, résultat renvoyé par un canal : rien de
  lourd dans la boucle du compositeur. Un numéro de demande fait ignorer la réponse d'une
  page qu'on a déjà quittée.
- Barre d'outils : page précédente/suivante, numéro de page à saisir, « sur N »,
  ajustement à la page ou à la largeur, « Ouvrir… », indicateur d'attente, nom du
  document à droite, zoom −/+. Touches : ←/→, Page↑/Page↓, Début/Fin, +/−, 0 (page
  entière), W (largeur), Ctrl+molette, Ctrl+O (sélecteur de Fichiers).
- Deux qualités de rendu : 2400 px pour lire, 3600 px dès qu'on zoome au-delà de 1,5 ou
  qu'on ajuste à la largeur ; la page déjà affichée reste à l'écran pendant ce nouveau
  rendu, l'attente se voit dans la barre.
- Fenêtre sans document : icône PDF, « Aucun document ouvert » et bouton « Choisir un
  document », avec le même sélecteur de fichiers que le lecteur vidéo.
- Scène grise autour de la feuille, comme la Visionneuse : sur fond blanc, la page ne se
  distinguait pas du panneau.

Vérifié :
- `tools/ci.sh` réussie : formatage, clippy `--all-targets -D warnings`, tests (dont six
  nouveaux tests du lecteur), shellcheck absent sur cette machine.
- Bureau imbriqué `--winit --capture` sur un PDF de trois pages fabriqué pour l'essai
  (`build/essai-pdf/rapport.pdf`) : page blanche de 962 × 1350 px, centrée, rapport 0,713
  (0,707 en A4), 39 lignes de texte, barre remplie, aucune panique au journal. Sans
  document : icône PDF, textes et bouton bien dessinés.
- Ces contrôles ont été mesurés sur les captures PNG par un petit script Python jetable
  (couleurs dominantes, rectangles, lignes de texte), l'image n'étant pas regardable
  directement ici. Le premier essai montrait une feuille étirée à toute la largeur : le
  cadre était posé dans un agencement vertical, qui l'étire ; il est maintenant dans un
  agencement horizontal, avec la marge de centrage calculée.

Limites :
- Navigation, zoom, Ctrl+O et le sélecteur n'ont pas été essayés à la souris : seuls le
  premier affichage et l'écran vide ont été vus en capture. À essayer sur le bureau.
- Thème sombre non vérifié en capture (session en thème clair) ; la scène et la barre
  suivent `style::themed`.
- Le rendu dépend de Poppler (`pdftoppm`, `pdfinfo`) : s'ils manquent, le lecteur affiche
  « Poppler n'est pas installé ».
- Une page paysage n'a pas été essayée ; l'ajustement prend le plus petit des deux
  rapports, elle doit donc tenir.
- `shell/src/input.rs` (modifié par une autre session, non commité) ne passait plus
  clippy (priorité d'opérateurs sur `ctrl * … | alt * …`) : parenthèses ajoutées.

## 2026-11-05 — Lecteur vidéo (GStreamer, intégré à la fenêtre)

Codé avec le lecteur PDF dans les mêmes fichiers (`main.rs`, `Cargo.toml`) :
`ferrite-apps video [fichier]` (`apps/core/src/video.rs`). Lecture par GStreamer
(`gstreamer`, `gstreamer-app`, `gstreamer-video`), image intégrée à la fenêtre egui (une
texture réutilisée, actualisée toutes les 100 ms) et son par la sortie système. Barre de
lecture : lecture/pause, muet, position et durée, recherche par pas de 5 s, et sélecteur de
fichiers (`FilesState`). Les vidéos du même dossier s'enchaînent dans une file ; types
reconnus par extension (mp4, mkv, webm, avi, mov…).

Vérifié : compilation et clippy `-D warnings` propres, tests (mise en forme du temps,
encodage des chemins en URI, extensions reconnues). Pas encore vu en capture : la lecture
d'une vraie vidéo reste à essayer sur le bureau.

## 2026-11-05 — Horloge (heure locale et mondiale, minuteur, chronomètre)

Fait :
- `apps/core/src/clock.rs` : application Horloge (`ferrite-apps clock`), trois onglets.
  L'Horloge affiche l'heure locale (chiffres larges, date en toutes lettres en français)
  et sept horloges du monde (Paris, Londres, New York, Los Angeles, Tokyo, Sydney, UTC)
  avec leur décalage et l'indication « hier » / « demain » quand le jour diffère. Le
  Minuteur règle heures/minutes/secondes, compte à rebours avec barre de progression,
  pause, reprise et message « Temps écoulé » à zéro. Le Chronomètre mesure au centième,
  avec tours cumulés et durée de chaque tour.
- L'heure locale vient de l'heure Unix (`SystemTime`) plus le décalage lu par `date +%z`,
  mis en cache et relu toutes les minutes (bascule d'heure d'été). La date civile est
  calculée par l'algorithme de Howard Hinnant (jour → année/mois/jour), sans dépendance.
- Ajoutée au menu Applications du bureau (`App::Clock`, icône `app-clock`, lancement
  `ferrite-apps clock`) ; fenêtre 440 × 560, instance unique comme les autres.

Vérifié :
- `cargo clippy --all-targets -D warnings` propre, sept tests du module (décalage, dates
  civiles, aller-retour jour↔date, formatages, minuteur qui s'arrête à zéro, pause/reprise,
  tours).
- Capture en bureau imbriqué `--winit --capture` : barre de titre, trois onglets (Horloge
  choisi), heure en grands chiffres, date, puis la liste des horloges du monde. Mesuré sur
  la capture PNG par un petit script (emplacement des chiffres et des lignes de villes).

Limites :
- Les horloges du monde utilisent des décalages fixes (heure d'hiver) : l'heure d'été n'y
  est pas appliquée. L'heure locale, elle, est exacte (décalage réel du système).
- Les onglets Minuteur et Chronomètre n'ont pas été vus en capture (changement d'onglet à la
  souris) : leur logique est couverte par les tests, à essayer sur le bureau.
- Le minuteur signale la fin par un message visuel, sans son.

## 2026-11-06 — Écran noir maintenu pendant la veille

Le fondu de mise en veille masquait le bureau, mais son échéance retirait aussi le voile noir
juste après l'affichage de l'écran verrouillé. Le voile reste maintenant actif jusqu'au signal
de reprise de logind ; il est également retiré si la commande de veille échoue ou si les
services système ne sont pas disponibles. Le réveil du clavier reste inchangé.

## 2026-11-06 — Veille directe au bouton d’alimentation

Le bouton d’alimentation met maintenant directement l’ordinateur en veille. Dans Paramètres →
Énergie, le comportement peut être remplacé par l’ouverture du dialogue d’alimentation. Ce
réglage s’applique au bouton du centre de contrôle et à celui de l’écran verrouillé ; le
raccourci clavier dédié continue d’ouvrir directement le dialogue.

## 2026-11-06 — Rétroéclairage du clavier après une minute sans frappe

Le compositeur éteint le rétroéclairage du clavier après une minute sans frappe, sans tenir
compte des mouvements de souris. La première frappe suivante rétablit le dernier niveau
utilisé. Le réglage manuel du rétroéclairage remplace le niveau mémorisé.

## 2026-11-06 — Retrouver les applications et espaces après redémarrage

Avant « Éteindre » ou « Redémarrer » depuis Ferrite, le bureau enregistre les applications,
la position, la taille et l'espace de travail de leurs fenêtres, y compris les fenêtres
réduites. Au lancement de la vraie session, les applications reconnues sont relancées et
leurs fenêtres replacées après leur première configuration. Les documents ne sont pas
enregistrés par Ferrite : leur contenu restauré dépend de chaque application. Les fenêtres
d'applications absentes ou impossibles à lancer ne sont pas restaurées. Un arrêt effectué
hors du dialogue d'alimentation Ferrite n'enregistre pas l'état.

## 2026-11-06 — Voyant Caps Lock du clavier

Le backend DRM actualise les voyants de tous les claviers libinput selon l’état Caps Lock
géré par XKB. Le témoin physique s’allume quand le verrouillage des majuscules est actif.

## 2026-11-06 — Dépôt remis au propre

- Les 208 liens symboliques de `ferrite-theme/` (curseurs, alias d'icônes) étaient devenus
  des raccourcis NTFS illisibles parce que Git était réglé sans liens (`core.symlinks=false`).
  Le disque FICHIERS (ntfs3) les accepte : réglage passé à `true` et fichiers restaurés depuis
  Git. Aucun lien cassé ne reste.
- Mis à la corbeille (récupérables) : `FerriteOs.zip`, `CLAUDE.md.ancien`, `.continue/`.
- Passation mise à jour (liens réparés, mentions de DeepSeek retirées) ;
  `.claude/settings.local.json` ignoré par Git.

## 2026-11-06 — Bouton d'alimentation physique géré par le bureau

Sur la vraie machine, le bouton physique éteignait le PC : seuls les boutons dessinés
(centre de contrôle, écran verrouillé) suivaient le réglage. En session DRM, le bureau prend
maintenant le verrou logind `handle-power-key` (mode `block`) et traite la touche
`XF86PowerOff` comme ces boutons : veille directe ou dialogue d'arrêt selon Paramètres →
Énergie. Si le bureau s'arrête ou plante, le verrou se ferme et logind reprend la main
(extinction). Pas pris en bureau imbriqué, pour ne pas priver le bureau hôte du bouton.

## 2026-11-06 — Écran de capture d'un seul tenant

La capture passait par une boîte de dialogue, puis par une sélection séparée. Impr. écran
ouvre maintenant un écran unique : une barre d'outils sombre en bas (Sélection, Écran,
Fenêtre ; photo ou vidéo ; déclencheur rond ; délai 0/3/5/10 s ; pointeur inclus ou non ;
croix pour fermer) au-dessus de ce qui sera capturé.
- Sélection : la dernière zone est reproposée (au début, la moitié de l'écran, centrée) ;
  poignées rondes aux coins, glisser l'intérieur la déplace, glisser ailleurs en trace une
  nouvelle.
- Fenêtre : la fenêtre sous le pointeur (sinon celle du dessus) est proposée, un clic en
  choisit une autre ; mode grisé s'il n'y a aucune fenêtre.
- Entrée ou Espace capture, Échap ferme. Le Dock se cache pendant la capture.
- Enregistrement dans Images et copie dans le presse-papiers : toujours les deux.

Vérifié : `tools/ci.sh`, test des boutons de la barre, captures `--demo-region` (zone et
barre). Pas vu en image : le mode Fenêtre (il faut cliquer), à essayer sur la machine.

## 2026-11-06 — Capture : bords de la zone et touche Lenovo sans Fn

- Zone de capture : coins attrapables dans un carré de 36 px (poignée dessinée : 22 px),
  les quatre bords redimensionnent dans un seul sens, et le curseur annonce coin, bord,
  déplacement ou nouveau tracé.
- La touche de capture du ThinkBook envoie Super+Maj+S sans Fn (relevé sur la machine :
  Super, puis Maj et S) et Impr. écran avec Fn. Super+Maj+S ouvre maintenant la capture en
  mode Sélection, comme Maj+Impr. écran.

## 2026-11-06 — Enregistrement vidéo de l'écran

- La barre de capture bascule entre photo et vidéo ; en vidéo, le déclencheur est rouge.
  Il lance `wf-recorder` (nouveau paquet de l'image) sur l'écran, la zone ou la fenêtre
  choisie ; le fichier MP4 va dans Vidéos (`XDG_VIDEOS_DIR`), nommé « Enregistrement du … ».
- Pendant l'enregistrement, une pastille rouge sous la barre affiche la durée et
  « Arrêter » ; la touche de capture l'arrête aussi. Arrêt par SIGINT pour que le MP4 soit
  bien fermé ; une notification annonce la fin (ou l'interruption si `wf-recorder` s'arrête
  seul, vérifié chaque seconde).
- `wf-recorder` lit les images par le protocole screencopy déjà servi par le compositeur.
- Option `--demo-video` : barre de capture en mode vidéo, pour les captures.

Vérifié : `tools/ci.sh`, capture `--demo-video` (bouton vidéo choisi, déclencheur rouge).
Pas vérifié : un vrai enregistrement (`wf-recorder` n'est pas encore installé sur la
machine), la pastille d'arrêt en image, la fluidité à 2880×1800 (chaque image est
redessinée puis relue par le processeur).
Limites : pas de son ; le pointeur et le délai ne s'appliquent qu'aux photos ; en mode
Écran, la pastille rouge apparaît dans la vidéo.

## 2026-11-06 — Lecteur vidéo, Horloge et widgets selon les maquettes

Maquettes « Lecteur vidéo » et « Widgets — refonte » du projet de design Ferrite (pas de
maquette pour l'Horloge : même langage visuel).
- Lecteur vidéo : image plein cadre sur fond noir ; en haut, dégradé avec fermer, Ouvrir…,
  titre et fichier centrés, liste et agrandir ; en bas, panneau de verre (temps, barre de
  progression à pastille, ±10 s, grand bouton lecture blanc, suivante, son et réglette,
  vitesse 1×/1,25×/1,5×/2×/0,75×, plein écran) ; grand bouton lecture au centre en pause ;
  bulle centrale (« + 10 s », « Volume 70 % ») ; colonne « À suivre » (vignette de la
  vidéo en cours, progression, « En lecture »/« En pause ») et interrupteur « Lecture
  automatique » (vidéo suivante à la fin). Les commandes se masquent après 2,5 s sans
  mouvement pendant la lecture. Touches : Espace/K, ←/→ ou J/L, ↑/↓, M, N, F, Échap,
  Ctrl+O. Fenêtre par défaut 1100 × 660 (l'ancienne dépassait l'écran).
- Horloge : onglets en segments dans la barre ; heure en Inter Display Light avec les
  secondes à côté, date, titres en petites capitales ; horloges du monde en carte
  (« Aujourd'hui, +1 h ») ; minuteur avec anneau de progression, réglages −/+ et durées
  toutes prêtes ; chronomètre avec tours en carte (le plus rapide en vert, le plus lent en
  rouge). `ferrite-apps clock minuteur|chronometre` ouvre l'onglet. Léger : un dessin par
  seconde pour l'heure, rien quand le chronomètre tourne sur un autre onglet.
- Widgets : verre plus sobre (teinte 66 %, couleurs du fond à peine ravivées, flou plus
  large) pour rester propre sur un fond très coloré ; prévisions en colonnes (maximum
  au-dessus du minimum) placées après l'heure, sans chevauchement ; graphique réseau à pas
  fixe calé à droite, aire douce, échelle minimale de 64 Kio/s.

Vérifié : `tools/ci.sh`, captures du lecteur (lecture, commandes, liste), des trois onglets
de l'Horloge, et des widgets avec les réglages réels (fond « fall foliage »).
Pas vérifié : les clics réels (vitesse, volume, glisser la progression), le thème sombre
de l'Horloge, le graphique réseau rempli.

## 2026-11-06 — Îlot du haut de l'écran, chiffres stables de l'Horloge

- Îlot (`shell/src/desktop/island.rs`, dessin dans `panels/island.rs`) : pastille noire
  autour de l'heure de la barre. Repliée, elle montre l'activité de part et d'autre de
  l'heure : enregistrement vidéo (point rouge et durée, clic : arrêter), minuteur de
  l'Horloge (anneau et reste en orange), musique (icône et égaliseur). Un clic sur un côté
  la déplie 6 s avec ses boutons (minuteur : pause/reprendre, arrêter ; musique :
  précédent, lecture, suivant) ; le milieu reste l'heure et ouvre le calendrier.
  Dépliée un instant : son, luminosité, micro et mode avion (à la place de la bulle du
  bas), notifications (à la place des bulles en haut à droite, boutons d'action compris,
  « +2 » s'il en attend d'autres), fin du minuteur (« Temps écoulé », Arrêter).
  Elle change de forme avec un léger ressort (380 ms) ; forme et contenu ont leurs propres
  petites images, le reste du bureau n'est pas redessiné. Elle s'efface quand un panneau
  est ouvert, à l'écran verrouillé, et (repliée seulement) en plein écran.
- Minuteur : l'Horloge l'annonce à chaque changement de phase par `org.ferrite.Ilot`
  (méthode `Minuteur`, servie avec les notifications) ; l'îlot compte les secondes
  lui-même et renvoie ses boutons par le signal `Commande` (`apps/core/src/island.rs`).
- Le lecteur multimédia est désormais suivi en permanence (MPRIS lu toutes les 2 s) pour
  que l'îlot montre la musique.
- Horloge : les chiffres ont tous la même largeur (comme des chiffres tabulaires) ; le
  chronomètre centré tremblait à chaque centième parce qu'un « 1 » est plus étroit qu'un
  « 0 » (vidéo de l'utilisateur).
- `--demo-island` garde l'îlot déplié pour les captures.

Vérifié : `tools/ci.sh` ; captures sur un bus isolé (`dbus-run-session`, appels `gdbus`)
du minuteur replié, déplié en marche et en pause, de la fin du minuteur, d'une
notification avec bouton et du volume ; capture du chronomètre aux chiffres fixes.
Pas vérifié : l'animation à l'œil, la musique (aucun lecteur sur le bus d'essai),
l'enregistrement dans l'îlot (wf-recorder absent), les clics réels, le lien avec la vraie
Horloge (annonce et boutons). Pour une capture avec une application déjà ouverte sur le
vrai bureau : `XDG_RUNTIME_DIR` court et à part, sinon l'instance unique lui passe la main.

## 2026-11-06 — Socle commun des applications

- Catalogue unique des applications (`services/system/src/catalog.rs`) : commande,
  `app_id`, nom, icône, tailles, Dock, menu. `ferrite-apps` et `ferrite-shell` le lisent ;
  le bureau n'a plus sa propre liste (`App` y devient un nom de commande).
- `ferrite-apps` : chaque application est une vue (`AppView` : `update`, `open`) créée à un
  seul endroit (`create`) ; le reste (thème en direct, fond, F11, instance unique) est
  commun. Ajouter une application : `docs/NOUVELLE-APPLICATION.md`.
- Pièces communes (`apps/core/src/kit.rs`) : cartes, titres, boutons, interrupteur,
  curseur, radio, segments (le choix glisse), état, chiffres stables. Les Paramètres et
  l'Horloge les utilisent au lieu de leurs copies.
- Rôles de couleur dans `style` (scène, filet, rail, inverse, succès, attention, voile,
  ombre…) ; Visionneuse, PDF, Musique, Fichiers, détails, TextEdit, Horloge et Paramètres
  les prennent au lieu de leurs teintes propres (les gris sombres variaient d'une
  application à l'autre). `tools/check-colors.sh` (dans `tools/ci.sh`) refuse toute
  nouvelle couleur écrite à la main ; les exceptions voulues sont listées en tête.
- Animations : échelle commune `tokens::motion` (QUICK 150, SHORT 200, NORMAL 280,
  LONG 360 ms) pour le bureau (panneaux, Dock, vue d'ensemble, fenêtres, bureaux,
  verrouillage, îlot, glisser) ; `kit::glide` pour les applications, même courbe.
- Galerie Ferrite (`ferrite-apps galerie`, hors du menu) : toutes les pièces et les rôles
  de couleur, en clair et en sombre.
- Corrigé en chemin : la carte douce était blanche sur blanc en thème clair (horloges du
  monde de l'Horloge), le segment choisi invisible en sombre.

Vérifié : `tools/ci.sh` ; captures de la galerie (deux pages, clair et sombre), de
l'Horloge (sombre, chronomètre clair), des Paramètres (Son clair, Apparence sombre) et de
Fichiers (clair et sombre).
Pas vérifié : la glissade des segments à l'œil.

Suite du même jour :
- Les rôles de couleur sont dans la charte partagée (`ferrite_ui::theme::Palette` : rail,
  titre, texte de réussite, scène, inverse et son texte et son action) ; `style` et les
  panneaux du bureau les lisent tous deux. Panneaux du bureau : titres, fonds de liste,
  rails, vert « Connecté », menus et dialogue d'arrêt (accent choisi au lieu du bleu fixe),
  presse-papiers et fenêtre de mot de passe passent par la palette.
- `tools/check-colors.sh` surveille aussi le bureau : les couleurs propres qui restent
  (widgets et écran de verrouillage en verre sombre, îlot, barre de capture, météo) sont
  plafonnées fichier par fichier, aucune ailleurs.
- Visionneuse : un chemin long dans les informations garde sa fin (« …/Photos ») au lieu
  de déborder sur le libellé.

Vérifié : `tools/ci.sh` ; captures de Musique (clair), du Moniteur (clair et sombre), du
lecteur PDF (sombre), de la Visionneuse (chemin long), du dialogue d'arrêt (clair et
sombre), de la fenêtre de mot de passe et du centre de contrôle (sombre).

## 2026-11-06 — Veille profonde retirée (l'ordinateur ne se réveillait pas)

Redémarrage non voulu vers 17h55 : à 17h47, la veille automatique (15 min sur batterie)
est entrée en mode `deep` (`PM: suspend entry (deep)` dans le journal système) et rien
n'a suivi ; il a fallu forcer le redémarrage. Le micrologiciel du ThinkBook choisit
lui-même la veille légère (« Low-power S0 idle used by default »), qui se réveillait bien.
`image/config/includes.chroot/etc/systemd/sleep.conf.d/60-ferrite.conf` est retiré de
l'image ; sur la machine, le même fichier est à supprimer avec sudo. Voir PASSATION.

## 2026-11-06 — Boutons de fenêtre des applications tierces

Les applications GTK (et Firefox, Chromium, Brave, qui suivent GTK) mettaient « fermer »
à droite : la clé GNOME `org.gnome.desktop.wm.preferences button-layout` valait
`appmenu:close`. ferrite-shell la publie maintenant au démarrage (`close:maximize`, hors
bureau imbriqué), comme `color-scheme` : fermer à gauche, agrandir à droite, comme les
fenêtres Ferrite. Les applications qui laissent le bureau dessiner leur barre (Qt, foot,
X11) avaient déjà les boutons Ferrite.
Vérifié : capture de Brave (profil temporaire) avec fermer à gauche et agrandir à droite.
Reste à part : les applications Electron à barre de titre maison (VS Code, Claude,
ChatGPT) dessinent leurs boutons elles-mêmes ; VS Code peut prendre la barre du bureau
(`"window.titleBarStyle": "native"`).

## 2026-11-06 — L'îlot se replie au premier clic ailleurs

Un clic hors de l'îlot déplié le replie aussitôt, sans attendre ses 6 s : minuteur ou
musique ouverts, son et luminosité, notification (elle file dans l'historique, avec
`Center::hide_bubbles`). La fin d'un minuteur reste affichée jusqu'à « Arrêter ». Le clic
poursuit ensuite son chemin (fenêtre, bureau). Testé : `island_contains` et
`hide_bubbles` ; pas encore essayé à la souris sur le bureau.

## 2026-11-06 — Audit du socle et de la fluidité, l'îlot ouvre l'application

- Socle : un seul bouton d'outil (`kit::tool_button`, `tool_button_at`) pour la
  Visionneuse, TextEdit et Fichiers ; les cinq copies avaient chacune leur survol (voile
  9 à 14), leurs coins (8 ou 10) et leurs couleurs d'état actif.
- Fluidité mesurée sur le vrai bureau (`ferrite-profil`, écran 90 Hz, 11 ms par image) :
  au repos 0,2 ms par image (0,9 au pire), aucun panneau redessiné ; les images viennent
  de l'application Claude (53/s) et de Brave (24/s). En mouvement : 10,3 ms au lancement
  d'une application, 16,5 ms (image sautée) à l'arrivée d'une notification.
- Cause de l'image sautée : l'îlot, les bulles, le son et l'enregistrement faisaient
  partie de la clé des images du bureau et de la barre ; chaque changement de l'îlot (et
  chaque seconde d'un minuteur ou d'un enregistrement) redessinait tout l'écran et la
  barre au processeur. Ils en sont retirés (`push_element`) ; l'îlot a ses propres images,
  maintenant comptées par la mesure (« îlot », 1 à 2 ms). Vérifié en bureau d'essai : une
  notification ne redessine plus que l'îlot.
- Îlot : un clic ouvre l'application concernée : l'Horloge pour le minuteur (et
  « Temps écoulé » l'arrête), le lecteur qui joue pour la musique (`DesktopEntry` de
  MPRIS), l'expéditeur d'une notification s'il est ouvert. Clic droit : ses boutons.
  L'enregistrement s'arrête toujours d'un clic.

Correction après essai (« impossible d'ouvrir l'îlot ») : la musique venait de Brave, qui
ne donne pas son `DesktopEntry` ; sa fenêtre est maintenant retrouvée par le nom du
lecteur (« brave » → `brave-browser`). Et un clic gauche ouvre l'application **et** déplie
l'îlot avec ses boutons, pour qu'il réagisse toujours, même si l'application est déjà
devant.

Pas vérifié : nouvelle mesure en mouvement sur le vrai bureau, clics réels sur l'îlot
depuis cette correction.

## 2026-11-06 — Îlot : le premier clic le déplie

À la demande de l'utilisateur : un premier clic sur la pastille repliée (minuteur,
musique) déplie l'îlot avec ses boutons ; c'est un clic sur l'îlot déplié, hors boutons,
qui ouvre l'application (Horloge, lecteur) et le replie. Un clic ailleurs le replie
toujours aussitôt. Pas encore essayé à la souris.
Suite : le clic sur l'îlot déplié passait par `activate_app`, qui réduit l'application
quand elle est déjà au premier plan (comportement du Dock). L'îlot et les notifications
passent maintenant par `show_app` : l'application vient au premier plan, sort du Dock si
elle était réduite, est lancée si elle est fermée, et n'est jamais réduite.

## 2026-11-06 — Un clic sur une notification ouvre ce qu'elle annonce

- Les notifications du bureau gardent le fichier qu'elles annoncent (`Notice::file`,
  `file_notice`) : un clic sur « Capture enregistrée » l'ouvre dans la Visionneuse, sur
  « Enregistrement terminé » dans le Lecteur vidéo (`App::for_file` : images, vidéos,
  PDF, sinon Fichiers). Si l'application est déjà ouverte, elle reçoit le fichier et
  passe devant (instance unique).
- Les autres notifications ouvrent l'application qui les a envoyées : au premier plan si
  elle est ouverte ou réduite, lancée si elle est fermée (`show_app`, jamais réduite).
- Même règle depuis l'îlot et depuis l'historique sous l'horloge.

Testé : `App::for_file`. Pas encore essayé à la souris.

## 2026-11-06 — Îlot : plusieurs activités en même temps

L'îlot ne montrait qu'une activité (enregistrement, sinon minuteur, sinon musique) ; les
autres étaient cachées. Toutes les activités en cours sont maintenant visibles, comme sur
l'iPhone : la plus pressante dans la pastille, chacune des autres dans une bulle ronde à
droite (point rouge, anneau du minuteur, barres de la musique), qui apparaît en fondu.
Un clic sur une bulle déplie son activité (`IslandFocus`) ; sur la bulle de
l'enregistrement, il l'arrête. Notifications, son et fin du minuteur passent toujours
devant, bulles masquées le temps de l'annonce.
Vérifié : capture d'un minuteur et d'une musique (lecteur MPRIS simulé) ; test des bulles
(`une_bulle_par_autre_activite`). Pas encore essayé à la souris.

## 2026-11-06 — Déverrouillage par visage (caméra infrarouge)

- Caméra : `/dev/video2` du ThinkBook donne du GRAY8 640 × 360 (infrarouge) ; sa lampe
  s'allume seule environ une seconde après l'ouverture (images de test effacées).
- `ferrite-visage` (`services/visage`, nouveau membre de l'espace de travail) :
  `enregistrer` (8 empreintes), `verifier`, `effacer`, `etat`, `essai` (détection seule,
  rien de gardé). YuNet (entrée 640 × 640) 124 ms, SFace 45 ms : environ 0,18 s par image
  sur le processeur. La version int8 de SFace n'est pas prise en charge par `tract`
  (QLinearMul) : modèle fp32. Tests : décodage de YuNet, alignement, cosinus, fichier
  d'empreintes.
- Bureau (`shell/src/desktop/face.rs`) : sur l'écran verrouillé, une touche, un clic ou le
  réveil lance `ferrite-visage verifier` (au plus une fois toutes les 6 s), message
  « Reconnaissance du visage… » sous le champ ; reconnu, l'écran s'ouvre comme après le
  mot de passe. Garde-fous : voir DECISIONS.
- Paramètres → Confidentialité : enregistrer son visage (progression, « Je ne vois pas
  votre visage »), recommencer, effacer, interrupteur « Déverrouiller l'écran avec mon
  visage ». `ferrite-apps settings visage` ouvre la page.
- `tools/visage-modeles.sh` télécharge les modèles (SHA-256 vérifiés) dans
  `assets/visage/` (hors git) ; `tools/dev-on.sh` installe `ferrite-visage` et les modèles.

Vérifié : `essai` sur la vraie caméra (visage trouvé, confiance 0,84 à 0,89) ; puis par
l'utilisateur : enregistrement et trois déverrouillages réels, ressemblance 0,90 à 0,94.
Le seuil passe de 0,5 à 0,6 (plus strict envers les autres visages, large marge pour le
sien) ; empreintes en 0600 dans un dossier 0700. Reste à faire : l'image ISO
(installer `ferrite-visage` et télécharger les modèles pendant sa construction).

## 2026-11-06 — Tout le travail du jour dans l'ISO et l'installation

- `tools/usb.sh` (et `usb-fedora.sh`, `vm-build.sh`, `install-dev.sh`) compilent et
  installent aussi `ferrite-visage` dans `/usr/local/bin`, et les modèles du visage dans
  `/usr/share/ferrite/visage` (téléchargés une fois par `tools/visage-modeles.sh`,
  empreintes vérifiées ; la construction s'arrête s'ils manquent).
- Paquets listés explicitement, qui n'arrivaient jusqu'ici que par les dépendances
  d'autres paquets : GStreamer (lecteur vidéo, Musique, caméra infrarouge), et
  `gsettings`, ses schémas et dconf (thème et boutons de fenêtre des applis tierces).
  Ajouté : `gstreamer1.0-libav`, sans lequel le lecteur vidéo de l'ISO ne lisait pas les
  vidéos H.264 (dont les enregistrements d'écran).
- Le reste du jour est dans les binaires (îlot, capture, socle, Horloge…) ou déjà dans
  l'image (wf-recorder, veille profonde retirée, mimeapps).

Vérifié : syntaxe des scripts (`sh -n`), présence des bibliothèques et commandes dans le
système de la dernière image (4 nov.), `tools/ci.sh`.
Pas vérifié : une construction complète de l'ISO (sudo, environ 30 minutes : la liste des
paquets a changé, `tools/usb.sh` refera tout le système) et un démarrage de l'ISO.

## 2026-11-06 — Une seule barre de titre pour toutes les fenêtres

Les applications ArtCraft n'avaient aucun bouton de fenêtre : comme beaucoup
d'applications egui ou winit sans cadre, elles demandent par `xdg-decoration` à dessiner
leur cadre, puis n'en dessinent pas. La règle est inversée (`protocols/decoration.rs`) :
toute fenêtre qui passe par ce protocole reçoit la barre de Ferrite (fermer à gauche,
agrandir à droite, titre au centre), même si elle demande son propre cadre. Gardent le
leur : les applications Ferrite (barre unifiée au même style) et `cadre_propre`
(navigateurs Chromium : Brave, Chrome, Edge, Vivaldi, Opera, dont les onglets sont la
barre et qui suivent déjà l'ordre des boutons de Ferrite). L'identifiant de la fenêtre
arrivant souvent après la demande, la règle est revue à son arrivée (`app_id_changed`).
Chaque demande est notée dans le journal du bureau (« décorations demandées »).
Applications GTK : cadre maison avec l'ordre des boutons de Ferrite ; X11 : cadre Ferrite.
Vérifié en bureau d'essai : Brave garde une seule barre (la sienne), foot reçoit celle de
Ferrite ; test `seuls_ferrite_et_les_navigateurs_gardent_leur_cadre`. Pas vérifié :
ArtCraft (une seule instance à la fois, à rouvrir sur le vrai bureau).
Corrigé en chemin : le test `la_reponse_de_l_auxiliaire_decide` échouait par moments
(`/bin/true` s'arrête avant de lire, l'envoi du mot de passe rencontrait « Broken pipe »).
`check_with` laisse alors décider le code de sortie de l'auxiliaire (jamais un
déverrouillage par défaut) et attend toujours sa fin.

## 2026-11-06 — GParted ne démarrait pas (XWayland absent)

GParted demandait bien le mot de passe, puis s'arrêtait : « cannot open display ».
Deux causes : `pkexec` retire l'accès à Wayland (le programme passe donc par XWayland),
et XWayland ne démarrait plus, `/tmp/.X11-unix` ayant disparu (règle de `x11-common`
pourtant présente ; cause inconnue, journal système non lu). ferrite-shell recrée le
dossier s'il manque, puis autorise root local sur XWayland (`xhost`, voir DECISIONS) ;
`x11-xserver-utils` ajouté à l'image. Vérifié : compilation, `tools/ci.sh`. Pas vérifié :
GParted réel (après relance du bureau).

## 2026-11-06 — Applications X11 nettes sur l'écran ×2

GParted et VLC passent par XWayland ; ils étaient dessinés à la moitié de la résolution
puis agrandis (flous). Le client XWayland reçoit maintenant l'échelle de l'écran
(`set_client_scale`, Smithay convertit positions et tailles) : XWayland voit l'écran à sa
vraie résolution. Les programmes X11 s'agrandissent eux-mêmes : `Xft.dpi` (xrdb) et
`QT_ENABLE_HIGHDPI_SCALING=1` pour Qt (VLC), `xsettingsd` (Gdk/WindowScalingFactor) pour
GTK sous X11 (GParted, y compris lancé en administrateur : XSETTINGS passe par le
serveur X). `xsettingsd` ajouté à l'image ; sur la machine : `sudo apt install xsettingsd`.
Vérifié : VLC en X11 dans un bureau d'essai à l'échelle 2, texte net et taille juste.
Pas vérifié : GParted (xsettingsd absent de la machine), changement d'échelle à chaud.
