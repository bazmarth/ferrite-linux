# Décisions

Choix non imposés par le plan, avec leur raison. Format : date — choix — raison.

- **2026-10-25 — Dépôt dans `D:\ferrite-linux`, séparé de `D:\Os`.** Le plan demande un dépôt
  distinct. Le prototype `D:\Os\ferrite-linux` (bureau egui dans Weston) n'est pas repris :
  il ne suit pas l'architecture (compositeur Smithay). Seule sa configuration d'image l'est.
- **2026-10-25 — Sources sur le disque Windows, compilation dans WSL.** Les sources restent
  visibles depuis Windows ; `tools/lib.sh` place `target/` dans
  `~/.cache/ferrite-linux/target` (ext4) car compiler sur `/mnt/d` est lent.
- **2026-10-25 — live-build travaille dans `/var/tmp/ferrite-image`.** Le disque Windows ne
  gère pas les fichiers spéciaux du chroot ; l'ISO finale est copiée dans `build/`.
- **2026-10-25 — Rust stable via rustup, pas le paquet Debian.** Debian 13 fournit Rust 1.85 ;
  Smithay et ses dépendances suivent des versions plus récentes.
- **2026-10-25 — Licence MIT** pour le code Ferrite, comme le champ du prototype.
- **2026-10-25 — Échelle stockée en 120es** (`ferrite_ui::Scale`), l'unité du protocole
  `wp_fractional_scale_v1`, pour éviter les erreurs d'arrondi des flottants.
- **2026-10-25 — Persistance désactivée par défaut.** L'option `persistence` ferait chercher
  à live-boot une partition sur tous les disques, SSD compris ; on préfère l'activer à la main
  au démarrage (procédure dans `docs/TEST-THINKBOOK.md`).
- **2026-10-25 — Modèles GRUB de live-build copiés dans `image/config/bootloaders/grub-pc`.**
  Seul `config.cfg` est modifié (délai de 5 s, plus de bip) : sans délai, l'image attendait
  une touche au menu. Le thème graphique Ferrite de GRUB viendra plus tard.
- **2026-10-25 — ferrite-shell part de l'exemple `smallvil` de Smithay 0.7.** C'est la base
  minimale maintenue avec la bibliothèque ; on la découpe selon l'organisation du plan.
- **2026-10-25 — Fenêtre de ferrite-shell en X11 sous WSLg.** Le Weston de WSLg plante
  (signal 11) à l'ouverture de la fenêtre winit en Wayland. `tools/shell-dev.sh` retire
  `WAYLAND_DISPLAY` pour ferrite-shell seul ; les clients restent en Wayland.
- **2026-10-25 — Rendu écrit pour `GlesRenderer` uniquement**, pas générique : c'est le
  moteur de rendu des deux backends prévus (winit et DRM). On généralisera si besoin.
- **2026-10-25 — Capture intégrée (`--capture`)** par lecture du tampon OpenGL : permet de
  vérifier le rendu sans intervention, en attendant `screencopy` (`tools/screenshot.sh`).
- **2026-10-25 — Décorations dessinées par shaders GLES**, pas par des images : nettes à
  toute échelle et sans texture à recalculer quand la fenêtre change de taille. Seul le
  titre est une image (fontdue), recalculée quand le titre, l'échelle ou l'état changent.
- **2026-10-25 — Choix du client respecté pour les décorations** : ferrite-shell propose
  les siennes, mais un client qui demande le mode client (GTK) dessine sa propre barre.
- **2026-10-25 — Bouton jaune : réduction sans Dock.** La fenêtre disparaît et revient par
  Ctrl+Alt+M ; le Dock (phase 3) remplacera ce raccourci.
- **2026-10-25 — Bouton vert : agrandir**, pas plein écran : le protocole plein écran et la
  sortie du plein écran viendront avec les raccourcis globaux.
- **2026-10-25 — `--apt-recommends false`** dans l'image : seuls les paquets listés sont
  installés, pour éviter qu'un environnement de bureau complet n'arrive par dépendance.
- **2026-10-26 — Bureau dans l'esprit d'elementary OS**, à la demande de l'utilisateur : une
  barre fine (menu Applications, date et heure au centre, indicateurs à droite) remplace la
  barre de menus façon macOS ; les menus s'ouvrent sous la barre et se ferment au clic
  extérieur. Le bureau n'a plus d'icônes. Les feux tricolores des fenêtres sont gardés.
- **2026-10-26 — Icônes SVG rendues par resvg**, à partir du dossier `icons/SVG` embarqué
  dans les binaires. Les icônes d'état, dessinées en couleur, sont converties à la volée en
  version monochrome pour la barre et les menus : couleurs vives en teinte pleine, gris
  clairs (parties « éteintes ») en teinte estompée.
- **2026-10-26 — Zone du Dock réservée** : les fenêtres placées ou agrandies s'arrêtent
  au-dessus du Dock, qui ne se masque pas encore automatiquement. Une nouvelle fenêtre est
  recentrée à son premier tampon, quand sa taille réelle est connue.
- **2026-10-26 — Barre de titre sans trait de séparation** : elle se prolonge dans
  l'en-tête des applications Ferrite, peint de la même teinte (`Palette::LIGHT.titlebar`).
- **2026-10-26 — Commandes système hors de la boucle du compositeur.** `bluetoothctl`
  attend indéfiniment un démon absent, ce qui figeait tout le bureau. Les réglages passent
  désormais par un fil d'exécution à part (`shell/src/system.rs`) ; chaque commande a un
  délai maximal et le processus est tué au-delà (`ferrite-system`).
- **2026-10-26 — Crate `ferrite-system`** (`services/system`), partagée par le compositeur
  et Paramètres : appels de `nmcli`, `bluetoothctl`, `wpctl`, `brightnessctl` et
  `timedatectl`, avec des analyses de sortie testées sans le matériel. Le D-Bus direct
  (`ferrite-settingsd`) viendra plus tard.
- **2026-10-26 — Réglages absents grisés** : sans carte Wi-Fi, adaptateur Bluetooth, sortie
  audio ou rétroéclairage, l'indicateur disparaît de la barre et le réglage est grisé, au
  lieu d'échouer au clic.
- **2026-10-26 — Clavier QWERTY américain par défaut** (`docs/PLAN.md` §1.2), modifiable
  dans Paramètres. Le choix est écrit dans `~/.config/ferrite/reglages` ; le compositeur
  le relit et l'applique sans redémarrer.
- **2026-11-02 — Sens de défilement configurable** dans Paramètres → Souris et pavé tactile ;
  le choix est appliqué sans redémarrer et n'affecte pas le défilement naturel du pavé tactile.
- **2026-10-26 — Redémarrage automatique du bureau** par `ferrite-session` après un arrêt
  sur erreur, au plus 3 fois en 2 minutes ; le journal précédent est gardé
  (`ferrite-shell.log.precedent`).
- **2026-10-26 — XWayland lancé par ferrite-shell** (fonction `xwayland` de Smithay) : les
  applications X11 (Brave et Chromium par défaut, VLC, jeux) s'affichent comme les autres.
  Leurs fenêtres reçoivent les décorations Ferrite si elles ne dessinent pas les leurs.
  Les menus X11 sont posés tels quels. `DISPLAY` est transmis aux applications lancées et
  à D-Bus (`dbus-update-activation-environment`).
- **2026-10-26 — `linux-dmabuf` avec retour du périphérique de rendu**, pour que Mesa
  utilise le GPU dans les clients. L'import réel est fait au rendu : un tampon refusé
  n'empêche pas les autres fenêtres de s'afficher.
- **2026-10-26 — Protocoles d'appoint** : viewporter, mise à l'échelle fractionnaire,
  activation, sélection primaire et pointeur relatif. Les contraintes du pointeur (jeux)
  et `presentation-time` attendront un vrai besoin, plutôt que d'être annoncés sans être
  respectés.
- **2026-10-26 — Menu Applications alimenté par les fichiers `.desktop`** (Debian, Flatpak
  et utilisateur), relus quand un dossier change : une application installée apparaît
  sans redémarrer. Recherche au clavier dès l'ouverture du menu, pages de 24.
- **2026-10-26 — Portails GTK** (`xdg-desktop-portal-gtk`) pour les dialogues de fichiers
  et l'ouverture de liens des navigateurs et des Flatpak.
- **2026-11-02 — Mode sombre sans couleur figée.** Bureau et applications passent par
  `themed` (valeur par thème) et `tint` (voile noir en clair, blanc en sombre) ; seules les
  ombres restent noires. La barre du haut suit le fond d'écran, pas le thème. Le thème est
  annoncé aux applications tierces par la clé GNOME `color-scheme` (lue par le portail
  GTK), sauf quand le bureau tourne dans une fenêtre d'un autre bureau.
- **2026-11-02 — Verre liquide, vue d'ensemble à la GNOME.** Les cartes de l'espace 0
  réfractent le fond d'écran sur leurs bords ; c'est calculé une fois par fond et par thème,
  sans flou GPU à chaque image, car seul le fond d'écran passe dessous. Le Dock et les menus,
  au-dessus des fenêtres, n'ont que les reflets : réfracter exigerait de recopier l'image
  de l'écran à chaque trame. La vue d'ensemble dessine
  chaque fenêtre vivante dans une texture à chaque image, seulement tant qu'elle est ouverte.
- **2026-11-02 — Terminal Ferrite natif, à la place de foot + tmux.** `ferrite-apps terminal`
  repose sur `alacritty_terminal` : chaque fenêtre est un processus avec ses onglets, et le
  Terminal peut toujours ouvrir plusieurs fenêtres. L'ancienne session `tmux -A` partagée
  montrait le même contenu dans chaque fenêtre. Sessions persistantes : lancer `tmux` soi-même.
- **2026-11-02 — Le pack `ferrite-theme` fournit les icônes.** Bureau et applications lisent
  directement ses SVG à la compilation (pas de copie) ; `icons/SVG` ne garde que l'état, les
  réglages et ce que le pack n'a pas. Le logo carré `start-here` remplace le losange. Le texte
  des icônes du pack est converti en tracés : resvg est compilé sans le texte.
- **2026-11-02 — Le thème suit la « Spécification des fenêtres et dispositions » et ses
  planches, à la place de la charte du §3.4 du plan.** Deux boutons de fenêtre (fermer à
  gauche, agrandir à droite) au lieu des feux, barre de titre de 46 px, coins de 14 px, horloge
  centrée, fond uni #16202D à hexagones, palette et accents du document. Conservés car absents
  de la spécification : Super+V, agrandissement et rebond du Dock, effet génie, page des
  widgets et son verre liquide. Le verre liquide disparaît des panneaux système et du Dock,
  que la maquette veut presque opaques (panneaux) ou simplement translucides (Dock).
- **2026-11-02 — Réponses aux questions ouvertes de la spécification.** Décorations dessinées
  par le compositeur, sauf les applications Ferrite qui ont leur barre d'en-tête. Shell maison,
  dans le compositeur (pas de clients layer-shell ni de Waybar).
- **2026-11-02 — Super+Ctrl+← et → changent d'espace.** La spécification les donne aussi pour
  les quarts ; les quarts restent au glisser vers un coin. Super+Ctrl+↑ et ↓ gardent la vue
  d'ensemble.
- **2026-11-02 — Verrouillage dans le compositeur, mot de passe vérifié par `unix_chkpwd`.**
  Pas d'`ext-session-lock` : le compositeur n'a pas besoin d'un client pour dessiner son écran.
  `unix_chkpwd` (paquet `libpam-modules-bin`) évite de lier libpam et d'écrire un service PAM ;
  il n'accepte que les mots de passe locaux (pas d'empreinte). Limite : si le compositeur
  plante, `ferrite-session` le relance sans verrou.
- **2026-11-02 — Notifications servies par `zbus`, sur un fil à part.** Le service reste
  facultatif : sans bus de session, ou sous un autre bureau qui tient déjà le nom, seules les
  notifications internes (captures) s'affichent.
- **2026-11-02 — Écarts demandés à la spécification.** Le lanceur montre une seule liste de A à
  Z (tri sans casse ni accent), sans rubriques ni « Plus » ni catégories à gauche. Toutes les
  fenêtres s'ouvrent au centre de la zone de travail, sans décalage de 32 px pour la deuxième
  fenêtre d'une application, ni position mémorisée.
- **2026-11-02 — Boutons Verrouiller et Éteindre gardés dans le centre de contrôle.** La dernière
  maquette n'y montre que le pourcentage et « Paramètres… », mais aucun autre endroit ne permet
  d'éteindre ou de verrouiller à la souris. Une ligne de Paramètres sans fonction réelle reste
  affichée, grisée, plutôt que de simuler un réglage.
- **2026-11-02 — Widgets sur le bureau, plus de page à part.** D'après les règles de la maquette,
  les widgets occupent le côté gauche de chaque bureau, sous les fenêtres, et s'estompent quand
  l'une d'elles les recouvre. Le premier bureau devient donc la page 0 et peut recevoir des
  fenêtres. Super+W, qui fermait aussi la fenêtre, ramène désormais les widgets devant ; Super+Q
  ferme, comme dans le tableau des raccourcis de la maquette.
- **2026-11-02 — Widgets libres.** À la demande, chaque widget a sa propre place et sa propre
  taille, sans grille : en mode d'édition, on le glisse où on veut et on tire ses bords ou ses
  coins. La grille de la maquette reste la disposition d'origine (bouton Rétablir).
- **2026-11-02 — Écran de verrouillage : pas de bouton qui ment.** Les notifications ne montrent
  que l'application et leur nombre. Le lecteur d'empreintes n'est pas proposé (aucune
  authentification par fprintd) ; « Changer d'utilisateur » et Accessibilité sont visibles mais
  atténués, sans action. Thème et fond de cet écran sont propres à lui et enregistrés.
