# Ferrite OS — Document de passation technique

Destinataire : l'IA qui reprend le développement et la maintenance (contexte système).
État décrit : branche `main`, commit `a70bb5b` (release `v2026.11.05`), 5 novembre 2026.
Langue du projet : **français partout** (code commenté, interface, messages de commit, docs).

> Ferrite n'est pas une personnalisation de bureau existant (pas de dotfiles GNOME/KDE/Hyprland).
> C'est un **environnement de bureau complet écrit en Rust** : compositeur Wayland maison,
> bureau dessiné par le compositeur, applications maison, image ISO Debian. Toute la
> configuration utilisateur passe par un seul fichier de réglages (§1.4).

Documents à lire avant toute modification, dans cet ordre :
`docs/PLAN.md` (cahier des charges), `docs/DECISIONS.md` (choix et raisons — prime sur le
plan quand ils divergent), `docs/ARCHITECTURE.md`, `docs/DEPENDANCES.md`,
`docs/RACCOURCIS.md`, la fin de `docs/JOURNAL.md` (dernières sessions).

---

## 0. Règles de travail (obligatoires)

1. **Vérifier avant chaque commit** : `tools/ci.sh` = `cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets --locked -- -D warnings`,
   `cargo test --workspace --locked`, `shellcheck` des scripts. Clippy sans `--all-targets`
   laisse passer des erreurs dans les tests : toujours `--all-targets`.
2. **Ne commiter que sur demande explicite de l'utilisateur.** Commits directement sur
   `main`, auteur `Bazteck <marthybazoum@gmail.com>`, message en français : titre
   « Domaine : résumé », puis liste à puces.
3. **`ferrite-theme/` contient 208 liens symboliques** (curseurs, alias d'icônes). Ils
   s'abîmaient en raccourcis NTFS (`IntxLNK…`) tant que `core.symlinks` valait `false` ;
   réparés le 2026-11-06 par `git config core.symlinks true` puis `git checkout`. Garder ce
   réglage. `git add` toujours par chemins explicites, jamais `git add -A` / `git add .`.
4. **Ne jamais saisir ni demander un mot de passe** : l'utilisateur tape lui-même ses `sudo`
   dans un terminal. Les commandes root se lancent dans un terminal visible par lui.
5. **Tester sur le vrai bureau par le mode dev** (§2.2) : l'utilisateur travaille *dans*
   Ferrite ; on remplace le bureau qui l'héberge. Un bureau qui plante le coupe.
6. **Vérifier visuellement** : `--capture` (bureau imbriqué) ou `tools/vm-test.sh` (ISO dans
   QEMU) ; lire l'image produite avant d'annoncer qu'un écran est correct.
7. Noter chaque session dans `docs/JOURNAL.md`, chaque choix non évident dans
   `docs/DECISIONS.md` (format : date — choix — raison).
8. L'utilisateur n'est pas développeur : réponses en français simple, pas de jargon inutile,
   dire clairement ce qui est vérifié et ce qui ne l'est pas.
9. Une autre session (autre IA) peut modifier le dépôt en parallèle : avant de commiter,
   `git status` / `git diff --stat` et ne pas embarquer un travail inachevé sans le dire.

---

## 1. Structure globale du système

### 1.1 Pile

| Couche | Choix |
|---|---|
| Distribution | Debian 13 « trixie », amd64, `main contrib non-free non-free-firmware` |
| Démarrage | GRUB + shim signé (Secure Boot), systemd, connexion automatique sur tty1 |
| Affichage | `ferrite-shell` : compositeur Wayland **Smithay 0.7**, backends DRM/KMS+GBM+libinput (`--drm`, session libseat/logind) et winit (`--winit`, fenêtre de dev) |
| Rendu | `GlesRenderer` (OpenGL ES) uniquement ; shaders maison (ombres, coins, décorations) ; panneaux du bureau rastérisés sur le **CPU** (`Canvas`, BGRA prémultiplié) puis envoyés en `MemoryRenderBuffer` |
| Applications Ferrite | `ferrite-apps` (un seul binaire, **egui**), sous-commande par appli |
| Applications tierces | Wayland natif, XWayland (Smithay `xwayland`), Flatpak, .deb |
| Services | NetworkManager (`nmcli`), PipeWire/WirePlumber (`wpctl`), BlueZ (`bluetoothctl`), UPower/sysfs, udisks2 (`udisksctl`), logind, polkit, gnome-keyring |
| D-Bus | `zbus` 5 (API bloquante, fils dédiés) |
| Machine cible | Lenovo ThinkBook 14 G4+ : 2880×1800 à 90 Hz, **échelle 2** (1440×900 logiques), Wi-Fi RTL8852BE (rtw89), pavé tactile, double démarrage Windows (NVMe) et Fedora |

Rust stable via rustup (pas le paquet Debian), édition 2024, `rust-toolchain.toml`.
Profil dev en `opt-level = 1` (et 2 pour les dépendances) : le rendu CPU est inutilisable
sans optimisation.

### 1.2 Arborescence du dépôt

```
ferrite-linux/
├── Cargo.toml              espace de travail : apps/core, services/system, shell, ui, tools/vm-manager
├── shell/                  ferrite-shell (compositeur + tout le bureau)
│   └── src/
│       ├── main.rs         démarrage : services, options --demo-*, FERRITE_LOCK, agent polkit
│       ├── options.rs      ligne de commande (--drm, --winit, --scale, --capture, --demo-*)
│       ├── state.rs        struct Ferrite : tout l'état du compositeur
│       ├── input.rs        clavier (filtre XKB), pointeur, gestes, raccourcis, actions
│       ├── system.rs       fil « ferrite-systeme » : réglages, relevés système, Event/Change
│       ├── clipboard.rs    historique du presse-papiers (Super+V), vignettes
│       ├── backend/        drm.rs (écran réel, gamma du mode nuit), winit.rs
│       ├── render/         mod.rs (composition d'une image), profile.rs, capture.rs,
│       │                   decorations.rs, shaders.rs, title.rs, genie (effet de réduction)
│       ├── wm/             fenêtres, espaces de travail (workspaces.rs), aimantation, tuilage, animations
│       ├── protocols/      un module par protocole Wayland (xdg_shell, idle, session_lock, xwayland…)
│       └── desktop/
│           ├── panels.rs   ~6 000 lignes : barre, menus, centre de contrôle, lanceur, dialogue
│           │               d'arrêt, OSD, notifications ; Canvas ; hit-test `action_at`
│           ├── panels/     lock_screen.rs (écran de verrouillage en couches), auth.rs
│           │               (fenêtre polkit), cards.rs (widgets), island.rs (îlot)
│           ├── dock.rs     Dock (layout, agrandissement, masquage, rebond)
│           ├── lock.rs     état du verrouillage, vérification unix_chkpwd, MPRIS
│           ├── polkit.rs   agent d'authentification polkit
│           ├── idle.rs     verrouillage / veille après inactivité
│           ├── night.rs    mode nuit (table gamma DRM)
│           ├── notifications.rs  serveur org.freedesktop.Notifications (+ org.ferrite.Ilot)
│           ├── island.rs   îlot du haut : ce qu'il montre, minuteur de l'Horloge, clics
│           │               (dessin : panels/island.rs)
│           ├── recording.rs  enregistrement vidéo de l'écran (wf-recorder)
│           ├── widgets.rs  espace 0 (widgets), météo
│           ├── apps.rs, launch.rs, controls.rs, function_keys.rs, file_drag.rs, svg.rs
├── apps/core/              ferrite-apps (egui) : files/ (Fichiers), terminal/, editor/,
│                           settings.rs + settings/ (Paramètres), monitor/, music/,
│                           viewer.rs (Visionneuse), pdf_viewer.rs, video.rs, clock.rs,
│                           calculator.rs, gallery.rs (galerie des pièces), details.rs ;
│                           main.rs (vues `AppView` créées par `create`), style.rs (thème,
│                           rôles de couleur), kit.rs (pièces communes), island.rs (lien
│                           de l'Horloge avec l'îlot). Nouvelle appli : docs/NOUVELLE-APPLICATION.md
├── services/system/        ferrite-system : appels système partagés (nmcli, wpctl, bluetoothctl,
│                           brightnessctl, timedatectl, rfkill, udisks…), config.rs (réglages),
│                           night.rs, idle.rs, weather.rs, command.rs (délais, kill),
│                           catalog.rs (liste unique des applications Ferrite)
├── ui/                     ferrite-ui : tokens.rs, theme.rs (palette, Scale, Easing), wallpaper.rs, icons.rs
├── icons/SVG, assets/fonts embarqués à la compilation ; ferrite-theme/ = pack d'icônes (voir §0.3)
├── image/                  live-build : auto/config, config/package-lists/ferrite.list.chroot,
│                           config/hooks/live/*.hook.chroot, config/includes.chroot/ (fichiers système)
├── tools/                  scripts (§2)
├── packaging/              (prévu : cargo-deb, dépôt apt — pas encore fait)
└── docs/
```

### 1.3 Fichiers système livrés par l'image (`image/config/includes.chroot/`)

| Fichier | Rôle |
|---|---|
| `etc/systemd/system/getty@tty1.service.d/autologin.conf` | connexion automatique de `ferrite` sur tty1 |
| `etc/profile.d/90-ferrite.sh` | lance `ferrite-session` une fois, sur tty1, shell interactif |
| `usr/local/bin/ferrite-session` | boucle de session (§2.1) |
| `usr/local/bin/ferrite-check` | vérifications matérielles de la phase 1 |
| `etc/modprobe.d/ferrite-rtw89.conf` | économie d'énergie Wi-Fi rtw89 désactivée |
| `etc/NetworkManager/conf.d/ferrite-wifi-powersave.conf` | `wifi.powersave = 2` |
| `etc/systemd/system/ferrite-wifi-unblock.service` | débloque rfkill au démarrage |
| `etc/systemd/system/ferrite-dev.service` | montage du partage QEMU `ferrite-dev` (développement en VM) |
| `etc/xdg/mimeapps.list` | applications par défaut (Fichiers, Visionneuse, Musique…) |
| `usr/share/applications/ferrite-*.desktop` | entrées des applications Ferrite |
| `usr/share/xdg-desktop-portal/ferrite-portals.conf` | portails : implémentation GTK pour `XDG_CURRENT_DESKTOP=Ferrite` |
| `etc/xdg/foot/foot.ini` | terminal de secours |

Hooks : `0100-enable-ferrite-wifi` (active les deux services Ferrite), `0110-enable-audio`
(sockets PipeWire globaux, bluetooth). Binaires `ferrite-shell` et `ferrite-apps` copiés
dans `/usr/local/bin` par `tools/usb.sh` au moment de la construction.

### 1.4 Réglages utilisateur

Un seul fichier texte `clé=valeur` : `~/.config/ferrite/reglages` (`$XDG_CONFIG_HOME`),
lu par `ferrite_system::config::Settings`. Paramètres l'écrit ; le fil système du compositeur
surveille sa date de modification et envoie des `Event` à la boucle (application sans
redémarrage). Clés en français, valeurs en français. Clés actuelles :

`accent`, `aimantation`, `apparence` (clair/sombre/auto), `barre_bluetooth`,
`barre_hauteur`, `barre_laterale_icones`, `barre_masquage`, `barre_pourcentage`,
`barre_translucide`, `barres_defilement`, `clavier`, `defilement_inverse`,
`dock_agrandissement`, `dock_epingles`, `dock_hauteur`, `dock_icones`, `dock_masquage`,
`dock_points`, `dock_recentes`, `dock_taille`, `double_clic`, `fenetres_multiples`,
`fond_ecran`, `horloge`, `meteo_ville`, `mode_nuit`, `mode_nuit_debut`, `mode_nuit_fin`,
`mode_nuit_temperature`, `reduire_animations`, `surlignage`, `tableau_*` (widgets Notes et
Tâches), `tuilage`, `veille_batterie`, `veille_secteur`, `verrouillage_auto`,
`verrouillage_flou`, `verrouillage_fond` (nom d'un fond dessiné ou chemin absolu d'une
photo), `verrouillage_theme`, `widgets_applications`, `widgets_disposition`, `widgets_nombre`,
et les raccourcis (`open_terminal`, `lock_screen`, `toggle_widgets`…).

Ajouter un réglage = une valeur par défaut dans la crate `ferrite-system` (comme
`IdleSettings::default`, `NightSettings::default`), lecture dans le fil système
(`shell/src/system.rs`, boucle des réglages), un `Event` vers la boucle, une ligne dans
Paramètres. Ne jamais écrire le fichier quand l'utilisateur garde les valeurs d'origine.

---

## 2. Logique des scripts

### 2.1 `ferrite-session` (`image/config/includes.chroot/usr/local/bin/`)

But : lancer le bureau sur tty1 et le relancer selon son code de sortie.

- Exporte `FERRITE_SESSION=1`, `XDG_CURRENT_DESKTOP=Ferrite`, `XDG_SESSION_TYPE=wayland` ;
  `SHELL=/usr/bin/fish` pour les terminaux du bureau.
- **Bus D-Bus** : utilise le bus de systemd `--user` (`$XDG_RUNTIME_DIR/bus`) s'il existe,
  sinon `dbus-run-session`. Raison : gnome-keyring, PipeWire et les portails vivent sur ce
  bus ; sur un bus privé, `org.freedesktop.secrets` ne répondait jamais et Brave/Claude
  attendaient 120 s au démarrage. **Ne pas revenir à `dbus-run-session` systématique.**
- **Mode dev** : préfère `/run/ferrite-dev` (partage QEMU) puis
  `~/.local/share/ferrite-dev` (rempli par `tools/dev-on.sh`) à `/usr/local/bin`. Une copie
  dev qui plante **deux fois de suite** est écartée (chemin + date du binaire dans
  `rejected`) et la session repart sur les binaires stables.
- **Verrouillage au démarrage** : `FERRITE_LOCK=1` à l'ouverture, après une déconnexion et
  après un plantage (un plantage ne doit jamais ouvrir la session) ; `0` après « Relancer
  le bureau » (code 3) et Ctrl+Alt+Retour arrière en dev (code 0). Jamais sur le système
  live (`boot=live` dans `/proc/cmdline`) : son utilisateur n'a pas de mot de passe connu.
- **Codes de sortie** de `ferrite-shell` (`PowerAction::exit_code`) : 0 = Ctrl+Alt+Retour
  arrière (console, ou relance en dev), 1 = plantage, **3 = relancer le bureau**,
  **4 = se déconnecter** (« Session fermée », Entrée rouvre, `console` reste en console).
- Plantages : relance automatique, au plus 3 en 2 minutes, puis console avec les
  15 dernières lignes du journal. Journal : `$XDG_RUNTIME_DIR/ferrite-shell.log`
  (le précédent en `.precedent`).
- N'ouvre **aucune application** au démarrage : on arrive sur l'espace 0 (widgets).
- Modifier ce script ne prend effet sur la machine installée qu'après
  `sudo install -m 0755 image/config/includes.chroot/usr/local/bin/ferrite-session /usr/local/bin/`
  (commande tapée par l'utilisateur) puis redémarrage ou déconnexion.

### 2.2 Outils de développement (`tools/`)

| Script | But et garde-fous |
|---|---|
| `lib.sh` | `REPO`, `BUILD`, `info/ok/die`, fichiers OVMF ; sourcé par tous |
| `ci.sh` | fmt + clippy `-D warnings` + tests + shellcheck ; `--image` construit l'ISO |
| `dev-on.sh` | compile en release et **copie** (pas de lien) vers `~/.local/share/ferrite-dev` : une compilation ratée ou en cours n'est jamais prise. Puis l'utilisateur fait « Relancer le bureau ». `--sans-compiler` copie la dernière compilation |
| `dev-off.sh` | retire la copie dev : retour aux binaires de `/usr/local/bin` |
| `usb.sh` | construit `build/ferrite-live.iso` (+ `.sha256`). Étape 1 en utilisateur (cargo release), puis se relance en `sudo`. live-build travaille dans `/var/tmp/ferrite-image` (le disque NTFS ne gère pas les fichiers spéciaux). **Incrémental** : empreinte `.ferrite-stamp` de `image/auto`, listes de paquets, hooks et compression ; si inchangés, recopie seulement `includes.chroot` et refait `lb binary`. `--complet`, `--final` (xz, image à distribuer), `--propre` (`lb clean --purge`) |
| `vm-test.sh SCENARIO [ISO]` | démarre l'ISO dans QEMU sans fenêtre (KVM si accessible), rejoue un scénario (`sleep`, `click X Y`, `type`, `key`, `drag`, `shot NOM` → `build/NOM.png`, `serial`, `serve`) sur un écran 1280×800. Scénarios dans `tools/scenarios/` |
| `vm.sh`, `vm-gui.sh`, `vm-manager/` | VM interactive |
| `install-dev.sh /dev/PARTITION` | installe Debian 13 + Ferrite sur **une** partition (depuis Fedora). Refuse : partition montée, présente dans `/etc/fstab`, swap, NTFS/EFI/chiffrée ; montre le contenu en lecture seule puis exige de **retaper le nom** avant `mkfs`. N'écrit rien d'autre qu'une entrée GRUB Fedora |
| `vm-build.sh` | compile dans un conteneur Debian 13 (Podman) pour des binaires compatibles ISO |
| `setup-wsl.sh`, `setup-fedora.sh`, `shell-dev.sh`, `usb-fedora.sh` | environnements historiques (WSL, Fedora) |

Publication : `gh release create vAAAA.MM.JJ` avec `ferrite-os-AAAA.MM.JJ.iso` et son
`.sha256`, tag annoté sur le commit exact de construction ; reconstruire l'ISO **depuis un
arbre commité** pour que la release corresponde au code. Dépôt
`github.com/bazmarth/ferrite-linux`, **privé** (ne pas changer sans accord explicite).

### 2.3 Sécurité et robustesse dans le code (cas limites gérés)

- **Commandes système hors de la boucle Wayland** : tout `nmcli`/`bluetoothctl`/`wpctl`… passe
  par `ferrite_system::command::run` (délai maximal, processus tué au-delà) dans des fils
  dédiés. Jamais d'appel bloquant dans la boucle calloop : un `bluetoothctl` sans démon
  figeait tout le bureau.
- **Mot de passe de l'écran de verrouillage** : vérifié par `unix_chkpwd` (setuid, PAM local)
  dans un fil ; la chaîne est **écrasée par des zéros** (`wipe`) puis libérée ; la vue de
  rendu ne reçoit que la longueur et le curseur, le texte clair seulement tant que l'œil est
  tenu. La touche qui fait apparaître le champ n'est pas écrite.
- **Agent polkit** (`desktop/polkit.rs`) : inscrit sur le bus système pour la session
  `XDG_SESSION_ID` (sinon `unix-process` pid + start-time), locale `fr_FR.UTF-8`.
  `BeginAuthentication` ne répond qu'après l'utilisateur (canal `async_channel`) ; le mot de
  passe est passé à `polkit-agent-helper-1 <utilisateur>` (cookie puis mot de passe sur
  l'entrée standard, attente de `SUCCESS`/`FAILURE`) puis effacé. Pas d'agent dans un
  bureau imbriqué (un seul agent par session). Modale : clavier et clics capturés.
- **Super coincée** : le relâcher de Super pendant le verrouillage remettait jamais
  `super_held` à faux → toutes les touches devenaient des raccourcis. Super retombe au
  relâcher même verrouillé, et dès qu'une touche est frappée sans le modificateur logo.
- **Disques dans Fichiers** : clés montées sans mot de passe (`--no-user-interaction`) ;
  partitions **internes** montées en **lecture seule** (`-o ro`) avec mot de passe, pour ne
  jamais abîmer Windows/Fedora ; partitions techniques (EFI, réservée, récupération, swap) et
  < 4 Gio cachées ; montage dans un fil (la fenêtre ne gèle pas pendant le mot de passe).
- **Ouvrir en administrateur** : `pkexec env WAYLAND_DISPLAY=… XDG_RUNTIME_DIR=… ferrite-apps files DIR`.
- **Inactivité** : vidéo (`zwp_idle_inhibit_v1`) = présence ; son en cours (`wpctl status`,
  flux `[active]`, casques Bluetooth compris ; ALSA en secours) reporte la veille ;
  `Instant` monotone (ne compte pas la durée de veille → pas de re-veille immédiate au réveil).
- **Rétroéclairage clavier en veille** : le signal `PrepareForSleep` de logind permet de
  mémoriser le niveau, de l'éteindre avant la suspension (inhibiteur de délai) et de le
  rétablir au réveil.
- **Rétroéclairage clavier au repos** : après une minute sans frappe, le compositeur l'éteint
  dans son fil système ; la prochaine frappe rétablit le dernier niveau. Les mouvements de
  souris ne réinitialisent pas ce délai.
- **Écran pendant la veille** : le voile noir reste au-dessus de l'écran verrouillé jusqu'au
  signal de reprise de logind ; si la commande de veille échoue, il est retiré immédiatement.
- **Mode de veille mémoire** : celui du noyau (`s2idle` sur le ThinkBook). Ne pas forcer
  `deep` : le 2026-11-06, la veille automatique est entrée en `deep` et l'ordinateur ne
  s'est jamais réveillé (redémarrage forcé). systemd ne peut pas se replier si le réveil
  échoue après l'entrée en veille.
- **Photo de verrouillage** décodée dans le fil système, une fois par choix, jamais au rendu.
- **Fenêtres sur l'espace 0** (réservé aux widgets) : une fenêtre qui s'ouvre y est
  déplacée vers l'espace 1 **avant** son premier configure, sinon elle ne s'affiche jamais.

---

## 3. Choix d'interface (UI/UX)

La référence visuelle est la **maquette claude.ai** (artifact Design
`https://claude.ai/artifact/B9G1X56BWAEhcmbTvnmjd3`, planches `*.dc.html` : Arret,
Musique, NotesWidgets, Visionneuse…) et la « Spécification des fenêtres et dispositions ».
Elle prime sur la charte §3.4 de `PLAN.md` (voir `DECISIONS.md`, 2026-11-02).

### 3.1 Mesures et couleurs (`ui/src/tokens.rs`, `ui/src/theme.rs`)

- Pixels **logiques** ; échelle en 120es (`Scale`, 240 = ×2).
- Barre du haut 34 px (horloge centrée, menu Applications à gauche, indicateurs à droite) ;
  barre de titre 46 px ; rayon des fenêtres 14 px ; deux boutons de fenêtre (fermer à
  gauche, agrandir à droite) de rayon 11 ; Dock : icônes 58 px, écart 10, marge 12.
- Accent `#2779E0` (clair) / `#6CB8FF` (sombre) ; danger `#E5372B` ; succès `#1F9D63` ;
  attention `#FF9F0A`. Fond uni `#16202D` à hexagones par défaut.
- Ombres : active `0 28 70` noir 38 % ; inactive `0 6 14` noir 22 %.
- Rayons : contrôles 8, panneaux/menus 10, dialogues 20.
- Polices embarquées : Inter Medium/SemiBold (interface), InterDisplay Light (grande horloge),
  JetBrains Mono (terminal, code). Tailles : 12 / 14 (corps) / 16 / 13 mono.
- **Rôles de couleur** dans `Palette` (fond, filet, rail, titre, scène, inverse, succès…) :
  les applications les lisent par `style::nom()`, le bureau par `palette().nom`. Une couleur
  écrite à la main est refusée par `tools/check-colors.sh` (lancé par `tools/ci.sh`), sauf
  les exceptions listées en tête du script. La galerie (`ferrite-apps galerie`) les montre.
- Mode sombre sans couleur figée : `themed` (valeur par thème) et `tint` ; la barre du haut
  suit la luminosité du fond d'écran ; le thème est annoncé aux applis tierces par la clé
  GNOME `color-scheme` (portail GTK).

### 3.2 Animations

- Échelle commune `tokens::motion` : QUICK 150 ms (disparitions, survols), SHORT 200
  (panneaux, fondus, Dock), NORMAL 280 (fenêtres, vue d'ensemble, contrôles), LONG 360
  (bureaux, verrouillage, îlot, retour d'un glisser). `EaseOut` cubique pour les
  apparitions, `EaseInOut` entre deux états. Applications : `kit::glide`. Exceptions
  voulues : génie 520 ms, rebond du Dock, pulsation de l'invitation.
- `reduire_animations` : état final immédiat (`Easing::progress`). Toujours le respecter.
- Effet génie à la réduction, rebond des icônes du Dock au lancement, agrandissement au survol.

### 3.3 Performance du rendu (règle d'or : rien de plein écran par image sur le CPU)

- Les panneaux sont rastérisés sur le CPU **seulement quand leur état change** (caches par
  clé `VisualState`/clés dédiées). Une animation se fait en **déplaçant, réduisant ou
  fondant** des images déjà dessinées (alpha et géométrie de `MemoryRenderBufferRenderElement`),
  pas en redessinant. Exemple : l'écran de verrouillage est en couches (fond, voile d'un pixel
  étiré, boutons, contenu, date, heure) ; avant, 21 ms par image dont 10 ms pour un voile
  plein écran.
- Ne jamais appeler `update_uniforms` d'un `PixelShaderElement` à chaque image s'il ne change
  pas (incrémente le compteur de commit → dommage plein écran permanent).
- Widgets : redessin incrémental par carte (`DashboardCache`), pas de l'espace entier.
- Mesure : créer `$XDG_RUNTIME_DIR/ferrite-profil` active les relevés
  (`shell/src/render/profile.rs` : images, rastérisations, causes de redessin, gestes) dans le
  journal ; supprimer le fichier pour arrêter.
- Le « verre liquide » (réfraction) n'existe que sur les widgets de l'espace 0, calculé une
  fois par fond d'écran ; pas de flou GPU par image.

### 3.4 Comportements UX établis (à conserver)

- Espace 0 = widgets uniquement ; on démarre dessus. Super+W y va et revient.
- Gestes à trois doigts : seuils calibrés sur le ThinkBook (`SWIPE_PAGE 600`,
  `OVERVIEW_SWIPE 70`, `SWIPE_LOCK 30`, `SWIPE_VERTICAL_RATIO 1.6`, deltas non accélérés).
- Dock masqué : se cache **dès** que la souris le quitte ; revient sur une descente rapide.
- Écran de verrouillage : clair par défaut, photo nette par défaut, flou réglable, encres de
  la date/heure/invitation choisies selon la luminosité de la photo, curseur ←/→/Début/Fin,
  Suppr, œil maintenu.
- Mode nuit : teinte par la table gamma DRM, programmable (soleil ou heures).
- Aucun bouton qui ment : une fonction absente est **grisée**, jamais simulée.
- Lanceur : une seule liste A→Z (tri sans casse ni accent). Fenêtres ouvertes au centre.

---

## 4. Points de vigilance (ne pas modifier sans raison démontrée)

| Élément | Pourquoi |
|---|---|
| `getty@tty1` autologin + `90-ferrite.sh` + `ferrite-session` | chaîne de démarrage : une erreur = plus de bureau. La sécurité repose sur le **verrouillage au démarrage** (§2.1) ; ne pas le retirer ni l'appliquer au système live |
| Bus D-Bus de systemd dans `ferrite-session` | sinon gnome-keyring injoignable, 2 min d'attente des navigateurs |
| `--systemd` dans `dbus-update-activation-environment` (`state.rs::publish_environment`) | les services démarrés par systemd (portails) doivent trouver l'écran |
| `ferrite-rtw89.conf` + `wifi.powersave = 2` | sans eux, coupures Wi-Fi et pics à 110 ms sur le ThinkBook |
| `unix_chkpwd` (paquet `libpam-modules-bin`) | seule vérification du mot de passe de verrouillage ; ne pas lier libpam ni écrire de service PAM sans décision |
| `polkit-agent-helper-1`, `polkitd`, `pkexec` | l'agent polkit et « Ouvrir en administrateur » en dépendent ; ne jamais vérifier un mot de passe admin autrement |
| `--apt-recommends false` (`image/auto/config`) | sinon un bureau complet (GNOME…) arrive par dépendances |
| `--firmware-chroot/--firmware-binary false` | sinon `firmware-b43-installer` télécharge et casse la construction ; les micrologiciels du ThinkBook sont listés à la main |
| Montage interne en lecture seule | protège Windows (démarrage rapide) et Fedora |
| `install-dev.sh` et ses refus | le PC contient Windows et Fedora : ne jamais écrire hors de la partition donnée |
| Pas de `persistence` par défaut sur l'ISO | live-boot fouillerait tous les disques |
| Sécurité de l'image live | l'utilisateur `ferrite` a `sudo` sans mot de passe (live-config) : normal pour un live, à revoir pour l'installateur |
| Pare-feu | **aucun n'est configuré** : l'image n'installe pas de pare-feu ; sur la machine installée, `nftables` est présent mais désactivé. Aucun service Ferrite n'écoute sur le réseau. Ne pas en ajouter un qui bloque DHCP, mDNS ou Bluetooth sans le tester sur la machine |
| `ferrite-theme/` | liens symboliques : garder `git config core.symlinks true`, sinon ils redeviennent des raccourcis NTFS abîmés |

Ne jamais : publier une release depuis un arbre non commité ; rendre le dépôt public sans
accord ; supprimer une release ou un tag sans demande ; lancer `sudo` à la place de
l'utilisateur ; pousser de force (`--force`) sur `main`.

---

## 5. Feuille de route

### 5.1 État au 2026-11-06 (tout est commité)

Faits et poussés : lecteur vidéo, lecteur PDF, Horloge (minuteur, chronomètre), capture
d'écran façon GNOME et enregistrement vidéo (wf-recorder), îlot du haut, bouton
d'alimentation géré par le bureau, socle commun des applications (catalogue, `kit`, rôles
de couleur, durées communes, galerie, guide). Le détail est dans `docs/JOURNAL.md`.

### 5.2 À vérifier sur la vraie machine (codé, pas encore confirmé par l'utilisateur)

- Agent polkit : fenêtre « Authentification requise » sur `pkexec true`, montage de la
  partition Windows, « Ouvrir en administrateur » sur `/root`.
- Parcours des archives ZIP/TAR/RAR dans Fichiers : couvert par des tests, reste à essayer
  visuellement sur la vraie machine.
- Veille légère (`s2idle`, choix du noyau) : un cycle veille → réveil reste à confirmer
  sur le ThinkBook. Le mode `deep` ne se réveille pas (incident du 2026-11-06) : ne pas le
  forcer. Le ventilateur reste géré par le firmware.
- Îlot du haut : animation à l'œil, minuteur de l'Horloge (annonce et boutons), musique,
  clics réels. Enregistrement vidéo : vérifié par l'utilisateur (vidéo du 2026-11-06).
- Premier démarrage après le passage au bus systemd (notifications, son, portails, coffre ;
  une appli peut redemander sa connexion une fois).

### 5.3 Prochaines étapes proposées (ordre recommandé)

1. Thème de curseurs réel (aujourd'hui grisé dans Paramètres) : les liens de
   `ferrite-theme/` sont réparés depuis le 2026-11-06.
2. Finir la phase 5 : réglages encore grisés (taille du texte, taille du curseur, pavé
   tactile).
3. « Éteindre l'écran après… » (DPMS DRM) avant la veille.
4. Applications par défaut et « Ouvrir avec… » partout (MIME).
5. Logithèque (phase 7) : PackageKit + Flatpak, notification des mises à jour (polkit est prêt).
6. Batterie et fiabilité (phase 10) : moins d'images au repos (l'appli Claude soumet 40 images/s
   en attente), démarrage < 20 s, tests automatiques des écrans par captures de référence.
7. Phases 8–9 : écran de connexion (greetd), `xdg-desktop-portal-ferrite`, paquets `.deb`,
   dépôt apt signé, installateur graphique (tests en VM d'abord, Windows intact).
8. Phase 6 en dernier : navigateur maison (Brave couvre le besoin).

### 5.4 Défauts connus

- Le widget Musique peut chevaucher le widget Système dans la disposition de l'utilisateur.
- Ouverture du centre de contrôle ~31 ms (une rastérisation complète).
- `--demo-auth` : le voile sombre du fond ne couvre pas les widgets de l'espace 0.
- Une résolution DNS de 51 s a été vue une fois, non reproduite.
- XWayland en bureau imbriqué : « Could not find a free socket » (sans conséquence en session réelle).

---

## 6. Recettes rapides

```sh
tools/ci.sh                                   # avant tout commit
tools/dev-on.sh                               # puis « Relancer le bureau » (dialogue d'arrêt)
target/debug/ferrite-shell --winit --capture /tmp/x.png --capture-delay 5 [--demo-lock|--demo-auth|--demo-power] [-- ferrite-apps settings energie]
XDG_CONFIG_HOME=/tmp/cfg target/debug/ferrite-shell --winit …   # réglages isolés pour un essai
# Appli déjà ouverte sur le vrai bureau : sinon l'instance unique lui passe la main.
# Bus et dossier d'exécution à part (chemin court : limite des sockets à 108 caractères).
R=$(mktemp -d /tmp/fr.XXXX); XDG_RUNTIME_DIR=$R WAYLAND_DISPLAY=/run/user/1000/wayland-1 \
  dbus-run-session -- target/debug/ferrite-shell --winit --capture /tmp/x.png -- ferrite-apps clock
ferrite-apps galerie                          # toutes les pièces et couleurs communes
tools/usb.sh --final                          # ISO à distribuer (sudo, ~30 min la première fois)
                                              # (ferrite-shell, ferrite-apps, ferrite-visage et
                                              # les modèles du visage, téléchargés une fois)
tools/vm-test.sh tools/scenarios/stabilite.txt  # ISO dans QEMU + captures dans build/
```

Outil de travail : Claude Code. Consignes du dépôt : `CLAUDE.md` ; contexte détaillé dans
la skill `.claude/skills/ferrite-contexte`.

Un bureau imbriqué (`--winit`) ne lance ni l'agent polkit ni le verrouillage `FERRITE_LOCK`,
et ne doit jamais déclencher de vraie mise en veille : mettre `veille_batterie=0` et
`veille_secteur=0` dans des réglages isolés pour tester l'inactivité.
