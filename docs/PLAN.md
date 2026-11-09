# Ferrite OS (base Linux) — Cahier des charges et plan de développement pour IA

> Ce document est destiné à une IA de développement (agent de code). Il décrit le contexte,
> les objectifs, l'architecture, les règles de travail et chaque phase avec ses tâches et ses
> critères de réussite. Lis-le entièrement avant de commencer, puis travaille phase par phase.
> Langue du projet : **français** (interface, commentaires du code, documentation, messages de commit).

> **Avancement au 2026-11-07** (le plan d'origine n'est pas réécrit ; les écarts voulus sont
> dans `docs/DECISIONS.md`, l'état détaillé dans `docs/TECHNIQUE.md` §5) :
> phases 0 à 3 faites ; phases 4 et 5 en grande partie (restent des réglages grisés dans
> Paramètres et quelques rubriques en maquette) ; phase 7 (Logithèque) commencée le
> 2026-11-09 (catalogues Debian et Flathub, installation, mises à jour, `.deb`) ; phase 8 :
> écran de connexion greetd, portail Ferrite (sélecteur de fichiers) et agent polkit faits ;
> phase 9 (paquets, installateur graphique) pas commencée ; phase 6 (navigateur) laissée pour la
> fin, Brave couvre le besoin ; phase 10 en continu. Le développement se fait sur le
> ThinkBook, dans Ferrite installé sur sa propre partition, et non plus sous WSL.

---

## 0. Instructions générales pour l'IA

1. **Travaille par phases, dans l'ordre.** Ne commence pas une phase tant que les critères de
   réussite de la précédente ne sont pas vérifiés (et notés dans `docs/JOURNAL.md`).
2. **Chaque phase se termine par quelque chose qui démarre et s'utilise.** Pas de code « pour
   plus tard » sans test.
3. **Ne touche jamais au disque interne du PC** (partition Windows) avant la phase 9, et jamais
   sans confirmation explicite de l'utilisateur. Les essais se font en machine virtuelle puis sur
   clé USB live.
4. **Vérifie toujours ce que tu affirmes** : compile, lance, fais une capture d'écran ou un test
   automatique. Si quelque chose n'a pas été vérifié, dis-le.
5. **Commits petits et fréquents**, un sujet par commit, message en français à l'impératif
   (« Ajoute le panneau Wi-Fi au centre de contrôle »).
6. **Code lisible** : Rust idiomatique (édition 2024), `cargo fmt`, `cargo clippy -- -D warnings`
   propres, pas de `unsafe` sans commentaire qui le justifie, commentaires en français.
7. **Dépendances** : bibliothèques libres (MIT, Apache-2.0, BSD, MPL, LGPL en liaison dynamique).
   Utilise la dernière version stable publiée au moment où tu commences, épingle-la via
   `Cargo.lock`, et note la raison de chaque dépendance dans `docs/DEPENDANCES.md`.
8. **Quand une décision n'est pas couverte par ce document**, choisis l'option la plus simple qui
   respecte l'architecture, note-la dans `docs/DECISIONS.md` (date, choix, raison), et continue.
   Ne pose une question à l'utilisateur que si le choix est irréversible ou touche ses données.
9. **Tiens à jour** `docs/JOURNAL.md` (ce qui est fait, vérifié, restant) à la fin de chaque session.

---

## 1. Contexte

### 1.1 L'utilisateur
- Développe « Ferrite OS », d'abord un système écrit de zéro (assembleur x86 + Rust : noyau,
  FAT32, serveur de fenêtres, navigateur web). Ce projet existe dans `D:\Os` et **reste intact**.
- Veut maintenant un système **utilisable au quotidien sur son PC**, avec **sa propre interface
  et ses propres applications**, en s'appuyant sur le **noyau Linux** pour les pilotes.
- Veut pouvoir **installer les paquets `.deb`** et les applications Linux existantes.
- Préférences : interface en **français**, style **macOS** (barre de menus, dock, fenêtres
  translucides et floutées, coins arrondis, ombres douces, feux tricolores), navigateur aux
  fonctions « type Brave » (onglets, bloqueur de publicités, favoris, téléchargements, PDF).

### 1.2 Le matériel cible
| Élément | Détail |
|---|---|
| Machine | Lenovo ThinkBook (portable) |
| Écran | 2880×1800 ; sous Windows, zoom 200 % → **échelle 2** (ou 1,75 en option) |
| Carte graphique | Intel Iris Xe (pilote Mesa `iris`, noyau `i915`/`xe`) |
| Wi-Fi | Realtek **RTL8852BE**, PCI `10EC:B852` → pilote noyau **rtw89**, firmware `firmware-realtek` |
| Stockage | SSD NVMe (contient Windows 11 — **à préserver**) |
| Clavier | **QWERTY anglais** ; pavé tactile |
| Fuseau horaire | Africa/Ouagadougou |
| OS actuel | Windows 11 (développement via WSL2) |

### 1.3 Code existant réutilisable (dans `D:\Os`)
| Chemin | Contenu | Réutilisation |
|---|---|---|
| `apps/browser/` | Navigateur : HTML, CSS (cascade, flex, grid via Taffy), mise en page, peinture, images PNG/JPEG/GIF, TLS 1.3 maison, HTTP, cookies, bloqueur de publicités, onglets, favoris, téléchargements | **Port** vers Linux (phase 6) |
| `apps/browser/src/js/` | Moteur JavaScript maison (≈14 000 lignes) : analyseur, interpréteur, bibliothèque standard, DOM, événements, fetch, modules ES, Proxy | **Conservé tel quel** |
| `user/src/gui.rs` | Bibliothèque graphique (Canvas à l'échelle, polices `.fnt`, widgets) | Base de `ferrite-ui` (phase 4) |
| `kernel/src/gui.rs`, `kernel/src/wm.rs`, `kernel/src/gfx.rs` | Bureau Ferrite : menubar, dock, fenêtres givrées, ombres, curseur vectoriel, rendu des icônes | **Référence visuelle** et code à porter dans `ferrite-shell` (phases 2-3) |
| `apps/files`, `apps/textedit`, … | Applications | Port (phase 5) |
| `xtask/fonts/` | Polices Inter, InterDisplay, JetBrains Mono, Source Serif 4 (OFL) | À reprendre |
| `live-linux/` | Essai Debian live (live-build) qui lance Ferrite dans QEMU | Point de départ de l'image (phase 1) |

Le nouveau projet ne dépend pas du noyau Ferrite : on remplace les appels système Ferrite par
ceux de Linux (bibliothèque standard Rust `std`).

---

## 2. Objectif final

Un système nommé **Ferrite OS** :
- démarre depuis une clé USB live puis s'installe sur le SSD **à côté de Windows** (double démarrage) ;
- utilise Debian 13 « trixie » et son noyau pour tout le matériel du ThinkBook ;
- affiche **son propre bureau** (serveur d'affichage Wayland maison) au style Ferrite/macOS ;
- fournit ses propres applications (Navigateur, Fichiers, Terminal, TextEdit, Réglages,
  Logithèque, Visionneuse, Lecteur, Moniteur, Horloge, Calculatrice) ;
- exécute aussi les applications Linux existantes (`.deb`, Flatpak) : Firefox, VLC, VS Code,
  LibreOffice, GIMP… y compris les anciennes applications X11 (XWayland) ;
- se met à jour via `apt` (dépôt de paquets Ferrite).

---

## 3. Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ Applications Ferrite (Rust, ferrite-ui)                      │
│ + applications Linux (.deb, Flatpak) via Wayland / XWayland  │
├──────────────────────────────────────────────────────────────┤
│ ferrite-ui : boîte à outils graphique (client Wayland)       │
├──────────────────────────────────────────────────────────────┤
│ ferrite-shell : serveur d'affichage Wayland (Smithay)        │
│   gestion des fenêtres, effets, menubar, dock, centre de     │
│   contrôle, notifications, Spotlight, verrouillage           │
├──────────────────────────────────────────────────────────────┤
│ Services système (D-Bus) : NetworkManager, PipeWire +        │
│ WirePlumber, BlueZ, UPower, udisks2, PackageKit, logind,     │
│ polkit, AccountsService                                      │
├──────────────────────────────────────────────────────────────┤
│ Debian 13 + noyau Linux + Mesa + firmware                    │
└──────────────────────────────────────────────────────────────┘
```

### 3.1 Choix techniques (imposés)
| Domaine | Choix |
|---|---|
| Distribution de base | Debian 13 « trixie », amd64, sections `main contrib non-free non-free-firmware` |
| Démarrage | GRUB + shim signé (Secure Boot compatible), systemd |
| Langage | Rust stable, édition 2024, espace de travail Cargo unique |
| Serveur d'affichage | **Smithay** (bibliothèque Rust de compositeur Wayland) |
| Sorties graphiques | DRM/KMS + GBM + libinput (session via `libseat`/logind) ; mode fenêtre (backend winit) pour le développement |
| Rendu | OpenGL ES 2/3 via le moteur de rendu GLES de Smithay ; shaders maison pour flou, ombres, coins arrondis |
| Clients Wayland (nos applis) | `smithay-client-toolkit` + rendu logiciel (`tiny-skia`) dans un premier temps, GPU ensuite si nécessaire |
| Texte | `fontdue` pour la rasterisation, `cosmic-text` (ou `rustybuzz`) pour la mise en forme et les langues complexes |
| D-Bus | `zbus` |
| Clavier | `xkbcommon` (disposition `fr`, variante par défaut ; changement dans Réglages) |
| Son | PipeWire (applis : `pipewire` crate ou passage par PulseAudio-compat) |
| Terminal | `alacritty_terminal` (émulation VT) + PTY via `rustix`/`nix` |
| PDF | PDFium (liaison dynamique) |
| Média | GStreamer (`gstreamer-rs`) |
| TLS (navigateur, Linux) | `rustls` + certificats système (`rustls-native-certs`), remplace le TLS maison |
| Paquets | `.deb` construits avec `cargo-deb` ; dépôt apt signé (reprepro ou aptly) |

### 3.2 Protocoles Wayland à implémenter dans ferrite-shell
Obligatoires : `wl_compositor`, `wl_subcompositor`, `wl_shm`, `linux-dmabuf`, `wl_seat`
(clavier, pointeur, tactile), `wl_output` + `xdg-output`, `xdg-shell`, `xdg-decoration`
(décorations côté serveur par défaut), `wp_viewporter`, `wp_fractional_scale`,
`wl_data_device` (presse-papiers, glisser-déposer), `zwp_primary_selection`,
`xdg-activation`, `wp_presentation_time`, `zwlr_layer_shell` (barres, dock, notifications si
on les fait en clients séparés), `ext-session-lock` (verrouillage), `zwlr_screencopy` ou
`ext-image-copy-capture` (captures), `wp_cursor_shape`, `zwp_text_input_v3`
(saisie de texte), `idle-inhibit`, `ext-idle-notify`, XWayland (`xwayland` de Smithay).

### 3.3 Organisation du dépôt (nouveau dépôt `ferrite-linux`)
```
ferrite-linux/
├── Cargo.toml                 espace de travail
├── shell/                     ferrite-shell (compositeur + bureau)
│   ├── src/backend/           drm.rs, winit.rs
│   ├── src/render/            effets : blur.rs, shadow.rs, rounded.rs, shaders/
│   ├── src/wm/                fenêtres, focus, snap, animations
│   ├── src/desktop/           menubar.rs, dock.rs, control_center.rs,
│   │                          notifications.rs, spotlight.rs, lock.rs, wallpaper.rs
│   └── src/protocols/         un module par protocole
├── ui/                        ferrite-ui (boîte à outils, thèmes, widgets)
├── apps/
│   ├── browser/ files/ terminal/ textedit/ settings/ store/
│   ├── viewer/ player/ monitor/ clock/ calculator/
├── services/
│   ├── ferrite-settingsd/     réglages (stockage, D-Bus)
│   └── ferrite-greeter/       écran de connexion
├── assets/                    polices, icônes, fonds d'écran, sons
├── image/                     construction des images (live-build) + scripts
├── packaging/                 configuration cargo-deb, dépôt apt, clés
├── tools/                     vm.sh, usb.sh, screenshot.sh, ci.sh
└── docs/                      ce plan, JOURNAL.md, DECISIONS.md, DEPENDANCES.md,
                               ARCHITECTURE.md, guide de style visuel
```

### 3.4 Charte visuelle (à respecter partout)
- Pixels **logiques** ; échelle 2 sur l'écran 2880×1800 (1440×900 logiques).
- Barre de menus : 25 px, translucide floutée, logo Ferrite à gauche, menus de l'appli active,
  à droite : Wi-Fi, son, batterie, centre de contrôle, date et heure.
- Dock : centré en bas, translucide flouté, icônes 54 px (squircles à dégradé), point sous les
  applis ouvertes, grossissement au survol (optionnel, réglable).
- Fenêtres : barre de titre 30 px, rayon des coins 11 px, ombre douce large, feux tricolores
  (rouge fermer, jaune réduire, vert plein écran) avec symbole au survol.
- Polices : Inter (interface), InterDisplay (titres), JetBrains Mono (terminal, code).
- Couleur d'accent : bleu macOS par défaut ; thème clair et sombre ; accent réglable.
- Animations : 200-300 ms, courbes d'accélération douces ; respecter « réduire les animations ».
- Référence : le bureau actuel de Ferrite (`D:\Os\kernel\src\gui.rs`) — reproduire son rendu.

---

## 4. Environnement de développement

- **Poste** : Windows 11 + **WSL2 Debian 13** (compilation, outils `live-build`, `debootstrap`,
  `xorriso`, `squashfs-tools`, `qemu-system-x86`, `ovmf`).
- **Itération rapide** : `ferrite-shell --winit` dans une fenêtre via **WSLg**, applications
  lancées dedans avec `WAYLAND_DISPLAY`.
- **Test complet** : image dans QEMU (`tools/vm.sh`, UEFI OVMF, `-m 4G`, virtio-gpu ou
  `-vga virtio`, KVM si disponible, sinon WHPX depuis Windows).
- **Test réel** : clé USB live (`tools/usb.sh` produit l'ISO ; l'utilisateur l'écrit avec
  Rufus/balenaEtcher) sur le ThinkBook, entrée UEFI du menu de démarrage.
- **Captures automatiques** : `tools/screenshot.sh` (via screencopy) pour vérifier le rendu.
- **Depuis Ferrite lui-même** (installé sur le disque par `tools/install-dev.sh`) :
  applications recompilées et lancées à côté des anciennes (`target/release/ferrite-apps NOM`) ;
  bureau essayé dans une fenêtre (`tools/shell-dev.sh`) ; puis en vrai avec `tools/dev-on.sh`
  et Ctrl+Alt+Retour arrière (repli automatique sur `/usr/local/bin` si la version plante deux
  fois ; `tools/dev-off.sh` pour revenir). Ctrl+Alt+F2 donne une console, Fedora reste dans
  GRUB. Démarrage, partitions et installateur : toujours en QEMU.
- **CI locale** : `tools/ci.sh` = `cargo fmt --check`, `cargo clippy`, `cargo test`, construction
  des `.deb`, construction de l'image.

---

## 5. Phases de développement

Format de chaque phase : objectif, tâches numérotées, livrables, **critères de réussite**
(à vérifier et consigner), pièges connus.

### Phase 0 — Fondations (≈ 2 jours)
**Objectif** : dépôt, outils et boucle de test en place.

Tâches :
1. Créer le dépôt `ferrite-linux` avec l'arborescence du §3.3 et l'espace de travail Cargo.
2. Créer `docs/JOURNAL.md`, `docs/DECISIONS.md`, `docs/DEPENDANCES.md`, `docs/ARCHITECTURE.md`.
3. Script `tools/setup-wsl.sh` : installe toutes les dépendances de compilation (Rust via
   rustup, `libudev-dev`, `libinput-dev`, `libgbm-dev`, `libseat-dev`, `libxkbcommon-dev`,
   `libegl-dev`, `libgles-dev`, `libpixman-1-dev`, `libdbus-1-dev`, `libpipewire-0.3-dev`,
   `libgstreamer1.0-dev`, `live-build`, `qemu-system-x86`, `ovmf`…).
4. `tools/ci.sh` et `tools/vm.sh` (lance une ISO dans QEMU UEFI).
5. Copier polices et éléments graphiques dans `assets/` (vérifier les licences).

Critères de réussite :
- `tools/ci.sh` passe sur un espace de travail vide (crate de démonstration).
- `tools/vm.sh` démarre une ISO Debian netinst de test dans QEMU.

### Phase 1 — Image système de base (≈ 1 semaine)
**Objectif** : une clé USB live Debian minimale qui démarre sur le ThinkBook avec tout le matériel.

Tâches :
1. Configuration `live-build` dans `image/` (reprendre `D:\Os\live-linux\auto\config`) :
   trixie, amd64, `grub-efi` + `grub-pc`, Secure Boot `auto`, locale `fr_FR.UTF-8`,
   clavier `fr`, fuseau `Africa/Ouagadougou`.
2. Paquets : `linux-image-amd64`, `firmware-realtek`, `firmware-misc-nonfree`,
   `firmware-intel-graphics` (si disponible) / `firmware-linux-nonfree`, `intel-media-va-driver`,
   `mesa-vulkan-drivers`, `libgl1-mesa-dri`, `network-manager`, `wpasupplicant`,
   `pipewire`, `pipewire-pulse`, `wireplumber`, `bluez`, `upower`, `udisks2`, `packagekit`,
   `flatpak`, `polkitd`, `accountsservice`, `xwayland`, `fonts-noto-core`, `fonts-noto-color-emoji`,
   `sudo`, `ca-certificates`.
   **Aucun** environnement de bureau (pas de GNOME, KDE, Xfce, X11 complet).
3. Utilisateur live `ferrite` (sudo), connexion automatique sur tty1 (systemd override de
   `getty@tty1`) qui lancera la session Ferrite (pour l'instant : un shell).
4. Persistance : partition `persistence` facultative sur la clé (documenter la création).
5. Script `tools/usb.sh` → `build/ferrite-live.iso` (hybride BIOS/UEFI).

Critères de réussite (sur le ThinkBook réel, consignés avec photos/captures) :
- démarrage UEFI jusqu'à la console en moins de 60 s ;
- `nmcli dev wifi list` voit les réseaux et `nmcli dev wifi connect` fonctionne (rtw89) ;
- `wpctl status` montre la sortie son ; un son de test est audible ;
- clavier AZERTY correct, pavé tactile détecté (`libinput list-devices`) ;
- `cat /sys/class/power_supply/BAT*/capacity` donne la batterie ;
- le SSD et Windows **ne sont pas modifiés** (vérifier `lsblk` : rien de monté en écriture).

Piège : si rtw89 échoue, vérifier la version du noyau (≥ 6.2 pour RTL8852BE ; Debian 13 a 6.12)
et la présence de `rtw89/rtw8852b_fw*.bin` ; consigner `dmesg | grep rtw89`.

### Phase 2 — ferrite-shell : le cœur du compositeur (≈ 4 semaines)
**Objectif** : notre serveur d'affichage affiche des fenêtres d'applications Linux, sur le vrai écran.

Tâches :
1. Squelette Smithay : boucle `calloop`, état global, `Display` Wayland, socket `wayland-1`.
2. Backend **winit** (développement dans WSLg) et backend **DRM/KMS** (libseat, udev, GBM,
   gestion du branchement d'écrans, mode natif 2880×1800, page flip sans déchirement).
3. Entrées : libinput (clavier, souris, pavé tactile : clic par tape, défilement à deux doigts
   naturel, accélération), xkbcommon (`fr`), répétition des touches, raccourcis globaux.
4. Protocoles du §3.2, en commençant par : compositor, shm, dmabuf, seat, output, xdg-shell,
   xdg-decoration, fractional-scale, viewporter, data-device.
5. Gestion des fenêtres : placement en cascade, déplacement par la barre de titre,
   redimensionnement par les bords, agrandir (double-clic barre de titre), réduire vers le dock,
   plein écran, **snap** gauche/droite par glisser au bord, Alt+Tab (sélecteur visuel),
   Cmd(Super)+Q / W / M / H.
6. Décorations serveur : barre de titre 30 px, titre centré, feux tricolores (survol =
   symboles ×, −, +), coins arrondis 11 px, ombre.
7. Rendu : shaders GLES pour coins arrondis (masque SDF), ombres (flou gaussien précalculé ou
   SDF), **flou d'arrière-plan** (Kawase double, 2-4 passes, rendu à demi-résolution) ;
   dommages (damage tracking) pour ne redessiner que ce qui change.
8. HiDPI : échelle 2 par défaut, fractionnaire (1,75 / 1,5) prise en charge, configuration
   dans `~/.config/ferrite/display.toml`.
9. Curseur : thème vectoriel Ferrite (reprendre le curseur de `kernel/src/gui.rs`), tailles
   selon l'échelle, `cursor-shape-v1`.
10. XWayland : lancement à la demande, fenêtres X11 gérées comme les autres.
11. Session : script `ferrite-session` lancé depuis tty1 (phase 1) qui démarre
    `dbus-run-session ferrite-shell`, exporte `XDG_CURRENT_DESKTOP=Ferrite`,
    `XDG_SESSION_TYPE=wayland`, lance `pipewire`, `wireplumber`, agent polkit.

Critères de réussite :
- dans WSLg : `ferrite-shell --winit` s'ouvre ; `foot` ou `weston-terminal` s'y affiche ;
- sur le ThinkBook : le bureau s'affiche en 2880×1800 net (échelle 2) ; Firefox (`.deb`
  Debian) s'ouvre, se déplace, se redimensionne, passe en plein écran, la vidéo défile sans
  déchirement ; une appli X11 (`xeyes` ou `gimp`) fonctionne via XWayland ;
- 60 images/s mesurées lors du déplacement d'une fenêtre avec flou actif ;
- copier-coller de texte entre Firefox et une appli X11.

Pièges : tester tôt `linux-dmabuf` avec Mesa iris ; prévoir un repli si GLES échoue (rendu
pixman) ; le flou coûte cher → le désactiver automatiquement sur batterie faible (réglage).

### Phase 3 — Le bureau (≈ 3 semaines)
**Objectif** : utiliser le système sans terminal.

Tâches :
1. **Fond d'écran** (images dans `assets/wallpapers`, choix dans Réglages).
2. **Barre de menus** (dessinée dans le compositeur ou client layer-shell — choisir et noter) :
   menu Ferrite (À propos, Réglages, Logithèque, Mettre en veille, Redémarrer, Éteindre,
   Fermer la session), menus de l'application active (protocole maison `ferrite-menu-v1` pour nos
   applis ; menus génériques Fichier/Fenêtre pour les autres), zone d'état, horloge.
3. **Dock** : applis épinglées (fichiers `.desktop` standard), applis ouvertes, point
   d'activité, clic = lancer/activer, clic droit = menu (Garder dans le Dock, Quitter),
   animation de rebond au lancement, grossissement optionnel, glisser pour réordonner.
4. **Centre de contrôle** : Wi-Fi (liste, connexion, mot de passe — NetworkManager D-Bus),
   Bluetooth (BlueZ), volume + choix de sortie (PipeWire), luminosité
   (`/sys/class/backlight` via logind `SetBrightness`), batterie (UPower), mode avion (rfkill),
   mode sombre, Ne pas déranger.
5. **Notifications** : serveur `org.freedesktop.Notifications`, bulles en haut à droite,
   historique dans un panneau.
6. **Spotlight** (Super+Espace) : recherche d'applications (`.desktop`), de fichiers
   (index simple), calculs, réglages.
7. **Verrouillage** (`ext-session-lock`, authentification PAM), mise en veille au rabat
   (logind), verrouillage au réveil, extinction/redémarrage via logind.
8. **Captures d'écran** (Cmd+Maj+3/4) vers `~/Images`.
9. Raccourcis clavier documentés dans `docs/RACCOURCIS.md`.

Critères de réussite (sur le ThinkBook) :
- connexion à un réseau Wi-Fi protégé uniquement par l'interface ;
- volume, luminosité, mode sombre réglés depuis le centre de contrôle ;
- une notification de Firefox apparaît dans le style Ferrite ;
- rabattre l'écran met en veille, l'ouvrir affiche l'écran de verrouillage, le mot de passe déverrouille ;
- autonomie mesurée sur 1 h d'usage léger, consignée.

### Phase 4 — ferrite-ui : boîte à outils (≈ 2 semaines, en parallèle de la phase 3)
**Objectif** : une bibliothèque pour écrire nos applications facilement.

Tâches :
1. Porter `D:\Os\user\src\gui.rs` : Canvas à l'échelle, primitives (rectangles arrondis,
   dégradés, ombres, cercles anti-crénelés), polices.
2. Fenêtre Wayland (`smithay-client-toolkit`), double tampon, mise à l'échelle fractionnaire,
   gestion du focus, curseurs, presse-papiers, glisser-déposer, saisie (`text-input-v3`).
3. Modèle d'application simple : état + fonction de dessin + gestion d'événements
   (style « mode immédiat » comme Ferrite, ou arbre de widgets retenu — choisir et noter).
4. Widgets : bouton (normal, principal, icône), champ de texte (sélection à la souris et
   au clavier, copier/couper/coller, annuler), zone de texte multiligne, case à cocher,
   interrupteur, curseur, liste déroulante, liste/tableau avec défilement inertiel,
   barre latérale, onglets, barre d'outils, menu contextuel, boîte de dialogue, barre de
   progression, info-bulle.
5. Thèmes clair/sombre, couleur d'accent, lecture des réglages via `ferrite-settingsd`.
6. Dialogue d'ouverture/enregistrement de fichiers commun.
7. Application `ui-demo` montrant tous les widgets.

Critères de réussite : `ui-demo` fonctionne dans WSLg et sur le ThinkBook, texte net en
échelle 2, saisie d'accents (é, è, à, ç, ê) correcte, copier-coller avec Firefox.

### Phase 5 — Applications de base (≈ 4 semaines)
Pour chaque appli : fichier `.desktop`, icône, menus (protocole `ferrite-menu-v1`),
raccourcis standards, thème clair/sombre, paquet `.deb`.

1. **Terminal** : `alacritty_terminal`, PTY, couleurs 256/vraies couleurs, onglets,
   défilement, sélection/copie, police JetBrains Mono, profils.
2. **Fichiers** : barre latérale (Accueil, Bureau, Documents, Téléchargements, Images,
   clés USB via udisks2), vues icônes/liste, aperçu, renommer, copier/déplacer avec
   progression, corbeille (spécification freedesktop), ouvrir avec (MIME + `.desktop`).
3. **TextEdit** : ouvrir/enregistrer, encodage UTF-8, rechercher/remplacer, numéros de
   ligne, coloration syntaxique (optionnelle, `syntect`).
4. **Réglages** : Écran (résolution, échelle, luminosité, veille), Apparence (thème, accent,
   fond, animations), Réseau, Bluetooth, Son, Clavier (disposition, répétition, raccourcis),
   Pavé tactile, Utilisateurs (AccountsService), Date et heure, Mises à jour, À propos.
5. **Moniteur d'activité** : processeur, mémoire, disque, réseau, liste des processus
   (`/proc`), terminer un processus.
6. **Visionneuse** : images (zoom, rotation, diaporama), PDF (PDFium : pages, zoom,
   recherche, sélection de texte).
7. **Lecteur** : audio et vidéo via GStreamer (lecture, playlist, plein écran).
8. **Horloge** (heure mondiale, minuteur, chronomètre), **Calculatrice**.

Critère de réussite : une journée de travail réelle (fichiers, texte, terminal, PDF,
musique) sans quitter les applications Ferrite ; aucun plantage consigné.

### Phase 6 — Navigateur (≈ 4 semaines)
**Objectif** : porter le navigateur Ferrite sur Linux et l'améliorer.

Tâches :
1. Copier `D:\Os\apps\browser` dans `apps/browser` ; remplacer `ferrite::*` (fichiers,
   réseau, horloge, aléatoire, fenêtre) par `std` + `ferrite-ui`.
2. Réseau : connexions TCP réelles, **plusieurs connexions en parallèle**, réutilisation
   (keep-alive), HTTP/1.1 (HTTP/2 ensuite), DNS système, proxy système.
3. TLS : remplacer le TLS maison par **rustls** avec **vérification des certificats**
   (magasin système) ; cadenas et détails du certificat dans l'interface ; page d'erreur
   claire en cas de certificat invalide.
4. Garder et tester : moteur HTML/CSS, mise en page, JavaScript (`src/js/`), bloqueur de
   publicités, onglets, favoris, historique, téléchargements dans `~/Téléchargements`.
5. Ajouter : PDF intégré (PDFium), son des pages (`<audio>`, `<video>` via GStreamer),
   images SVG et WebP, mise en page incrémentale (ne recalculer que ce qui change),
   exécution JavaScript qui ne fige pas l'interface (fil séparé pour réseau et décodage).
6. Intégration : navigateur par défaut (`x-scheme-handler/http(s)`), ouverture depuis
   les autres applis.
7. Option documentée : utiliser **Servo** comme moteur alternatif dans l'interface Ferrite
   pour la compatibilité totale (YouTube, Gmail) — à évaluer, pas obligatoire.

Critères de réussite : Wikipédia (fr), un site d'actualité, DuckDuckGo et GitHub
s'affichent correctement avec HTTPS vérifié ; un certificat invalide (badssl.com) est
refusé ; un PDF s'ouvre dans un onglet ; 20 onglets ouverts sans dépasser 1,5 Gio.

### Phase 7 — Logithèque (≈ 2 semaines)
Tâches :
1. Backend **PackageKit** (D-Bus) pour apt : recherche, détails, installation,
   désinstallation, mises à jour, progression, authentification polkit.
2. Backend **Flatpak** (Flathub activé par défaut, avec accord de l'utilisateur au premier lancement).
3. Interface : accueil (sélection), catégories, recherche, fiche (captures, description,
   taille, source .deb/Flatpak), onglet Mises à jour, onglet Installées.
4. Double-clic sur un `.deb` téléchargé → fiche d'installation dans la Logithèque.
5. Notification quotidienne des mises à jour disponibles.

Critères de réussite : installer VLC (deb), VS Code (deb téléchargé) et Spotify (Flatpak)
en deux clics chacun ; les désinstaller ; appliquer une mise à jour.

### Phase 8 — Intégration système (≈ 2 semaines)
1. **Écran de connexion** `ferrite-greeter` (greetd + notre client) : utilisateurs avec
   avatar, mot de passe, choix de session.
2. `xdg-desktop-portal-ferrite` : sélecteur de fichiers (notre dialogue), captures,
   partage d'écran (PipeWire), paramètres — pour Flatpak et Firefox.
3. Trousseau : `gnome-keyring` ou `oo7` pour les mots de passe Wi-Fi et navigateur.
4. Applications par défaut et associations de types (MIME), ouverture des liens.
5. Impression (CUPS) — facultatif.
6. Accessibilité de base : taille du texte, contraste élevé, zoom de l'écran.

Critère de réussite : redémarrage complet → écran de connexion Ferrite → session ;
le sélecteur de fichiers Ferrite s'ouvre depuis Firefox Flatpak.

### Phase 9 — Installation et distribution (≈ 2 semaines)
⚠️ Phase sensible : **le Windows de l'utilisateur doit rester intact.**
1. Paquets `.deb` pour chaque composant (`cargo-deb`), métapaquet `ferrite-desktop`.
2. Dépôt apt Ferrite signé (GPG) ; `apt upgrade` met à jour le bureau.
3. **Installateur graphique** (application Ferrite, s'appuyant sur `debootstrap` ou sur la copie
   du système live) :
   - détection de Windows, **redimensionnement de la partition NTFS uniquement avec accord
     explicite et après rappel de sauvegarde** (et BitLocker suspendu au préalable) ;
   - création des partitions Ferrite (racine ext4 ou btrfs, échange ou fichier d'échange),
     réutilisation de la partition EFI existante sans l'effacer ;
   - chiffrement LUKS optionnel ;
   - GRUB avec entrée Windows (os-prober) ; Secure Boot via shim signé Debian ;
   - utilisateur, mot de passe, fuseau, clavier.
4. Testé d'abord **uniquement en machine virtuelle** avec un Windows de test en double
   démarrage, puis seulement ensuite sur le ThinkBook, après confirmation de l'utilisateur.
5. Image ISO officielle versionnée (`ferrite-os-AAAA.MM.iso`) et somme SHA-256.

Critère de réussite : en VM, installation à côté de Windows → les deux systèmes démarrent ;
mise à jour d'un composant via `apt`. Puis, avec accord, même résultat sur le ThinkBook.

### Phase 10 — Finitions (continu)
- Performances : 60 images/s constantes, démarrage < 20 s jusqu'au bureau, consommation
  mémoire du bureau au repos < 300 Mio.
- Batterie : flou désactivable, fréquence d'images réduite au repos, mise en veille fiable.
- Tests automatiques : tests unitaires (ui, navigateur, moteur JS), tests d'intégration du
  compositeur (clients de test), captures de référence comparées.
- Traductions (fichiers Fluent), documentation utilisateur, pages d'aide.

---

## 6. Définition de « terminé » (pour chaque tâche)
- Le code compile sans avertissement, `cargo fmt` et `clippy` propres.
- Un test automatique ou une vérification manuelle documentée existe.
- Le comportement a été vu fonctionner (capture ou journal) dans l'environnement indiqué
  (WSLg, QEMU ou ThinkBook).
- `docs/JOURNAL.md` est mis à jour ; les décisions sont dans `docs/DECISIONS.md`.

## 7. Règles de sécurité et de données
- Ne jamais écrire sur le SSD du ThinkBook avant la phase 9 et sans accord explicite.
- Ne jamais committer de mots de passe, clés privées ou jetons ; la clé de signature du
  dépôt apt reste hors du dépôt.
- Toute action qui supprime des fichiers de l'utilisateur passe par la corbeille.
- Les mises à jour automatiques ne redémarrent jamais sans accord.

## 8. Ordre de priorité si le temps manque
1. Phases 0-2 (le système démarre avec notre bureau et Firefox) ;
2. Phase 3 (Wi-Fi, son, batterie sans terminal) ;
3. Phases 4-5 (nos applications) ;
4. Phase 7 (installer des logiciels facilement) ;
5. Phase 6 (notre navigateur — Firefox couvre le besoin entre-temps) ;
6. Phases 8-9 (installation sur le disque).

## 9. Première session de travail attendue
1. Lire ce document et `D:\Os\README.md`.
2. Réaliser la phase 0 complète.
3. Commencer la phase 1 : configuration live-build, construire l'ISO, la démarrer dans QEMU.
4. Fournir à l'utilisateur l'ISO et la procédure pour la tester sur clé USB (Rufus, mode GPT/UEFI),
   avec la liste des vérifications du critère de réussite de la phase 1 à faire sur le ThinkBook.
