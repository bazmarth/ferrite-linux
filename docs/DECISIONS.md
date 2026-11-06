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
- **2026-11-03 — Micrologiciels de l'image choisis à la main.** `--firmware-chroot` et
  `--firmware-binary` sont désactivés dans `image/auto/config` : sinon live-build installe tous
  les paquets de micrologiciels des dépôts, dont `firmware-b43-installer` (contrib), qui
  télécharge pendant son installation et fait échouer la construction. Ceux du ThinkBook sont
  listés dans `ferrite.list.chroot`.
- **2026-11-03 — Construction de l'ISO incrémentale.** Par défaut, `tools/usb.sh` garde le
  système de l'image (chroot) tant que `image/auto`, les listes de paquets et les hooks n'ont
  pas changé (empreinte `.ferrite-stamp`) : il y recopie `includes.chroot` (binaires, scripts)
  et ne refait que l'étape `lb binary`. `--complet`, `--final` ou `--propre` reconstruisent
  tout. Compression squashfs zstd en développement (plusieurs fois plus rapide que xz, image un
  peu plus grosse) ; xz pour `--final`. Les conteneurs de compilation et de live-build sont des
  images locales (`localhost/ferrite-build`, `localhost/ferrite-live-build`) ; registre Cargo
  et produits de compilation dans des volumes Podman, pour compiler seulement ce qui change.
- **2026-11-03 — Mode développement sur le disque, avec repli.** Depuis que Ferrite est installé
  sur le disque et qu'on y développe, `tools/dev-on.sh` copie la compilation release dans
  `~/.local/share/ferrite-dev`, que `ferrite-session` préfère à `/usr/local/bin` (après le
  partage QEMU). Copie plutôt que lien vers `target/release` : une compilation en cours ou
  ratée n'est jamais prise au relancement, seul ce qu'on a choisi de copier l'est. Comme on
  remplace le bureau dans lequel on travaille (Claude compris), une version copiée qui plante
  deux fois de suite est écartée (chemin et date du binaire) et la session repart sur les
  binaires stables ; une nouvelle copie est retentée. `/usr/local/bin` ne change que par sudo.
- **2026-11-03 — Économie d'énergie Wi-Fi désactivée.** Sur le ThinkBook (RTL8852BE, pilote
  rtw89), elle provoquait des coupures et des pics de latence jusqu'à 110 ms vers la box
  (0 % de pertes et ~1 ms une fois coupée). L'image la désactive dans le pilote
  (`modprobe.d/ferrite-rtw89.conf`) et dans NetworkManager ; un peu plus de consommation.
- **2026-11-03 — Déconnexion et relance du bureau par code de sortie.** Le dialogue d'arrêt
  propose « Se déconnecter » et « Relancer le bureau » au-dessus des boutons de l'ordinateur.
  ferrite-shell se ferme avec le code 4 ou 3, que `ferrite-session` comprend : 3 relance le
  bureau (avec la dernière copie de `tools/dev-on.sh`), 4 affiche « Session fermée » et
  rouvre la session à l'appui sur Entrée (« console » pour rester sur la console). 0 reste
  Ctrl+Alt+Retour arrière et 1 un plantage. Pas de gestionnaire de connexion avant la phase 8 :
  la session tourne sur tty1 en connexion automatique, un code de sortie suffit.
- **2026-11-03 — Bureau 0 réservé aux widgets.** À la demande, on revient sur « widgets sur
  chaque bureau » : le bureau 0 ne porte que les widgets et ne disparaît jamais. Une fenêtre
  qui s'ouvre pendant qu'on y est fait d'abord passer au bureau 1, où elle s'ouvre ; une
  fenêtre qui y arriverait autrement (restaurée, X11) y est renvoyée, une fois configurée.
  Les widgets s'effacent à mesure qu'on quitte le bureau 0 et ne reçoivent de clics que sur
  lui. Super+W va au bureau des widgets puis revient au bureau quitté ; Super+Maj+1…9
  visent les bureaux 1 à 9.
- **2026-11-05 — Lecteur PDF : Poppler appelé dans un fil, une image par page.** Le rendu
  s'appuie sur `pdftoppm` (image) et `pdfinfo` (nombre de pages) plutôt que sur une
  bibliothèque liée : moins de code, le rendu est celui de Poppler, et la compilation ne
  dépend pas de développement de poppler-glib. Chaque page est rendue à la demande dans un
  fil (`ferrite_system` non requis, la commande est ponctuelle) et son PNG revient par un
  canal ; la boucle du bureau ne bloque jamais. Un numéro de demande croissant fait jeter
  la réponse d'une page qu'on a déjà quittée, et la texture est réutilisée (`TextureHandle::set`)
  pour ne pas en créer une par page.
- **2026-11-05 — Deux qualités de rendu, pas de flou qui traîne.** Une page est d'abord
  rendue à 2400 px de côté (lecture entière), puis rendue à 3600 px quand on zoome au-delà
  de 1,5 ou qu'on ajuste à la largeur. Pendant ce second rendu, la page déjà affichée reste
  à l'écran et la barre montre l'attente : pas de clignotement, et pas de texte flou
  agrandi à l'écran. Un rendu refusé au-delà de 24 Mpx protège la mémoire.
- **2026-11-05 — Feuille centrée à la main plutôt que dans un cadre de mise en page.** Un
  `egui::Frame` posé directement dans un agencement vertical s'étire à toute la largeur
  disponible : la « page » devenait un fond blanc plein écran, invisible sur un panneau
  blanc (constaté en capture). Le cadre est désormais dans un agencement horizontal, avec la
  marge de centrage calculée, et la scène autour reprend les couleurs de la Visionneuse
  (0xEDEFF2 clair, 0x121418 sombre) pour que la feuille se détache.
- **2026-11-05 — Horloge sans bibliothèque de temps.** L'heure locale est l'heure Unix
  (`SystemTime`) plus le décalage lu par `date +%z` (mis en cache et relu chaque minute), et
  la date civile est calculée par l'algorithme de Howard Hinnant (jour ↔ année/mois/jour) :
  pas de dépendance `chrono`/`time` pour une application simple. Les horloges du monde
  gardent des décalages fixes (heure d'hiver) ; l'heure d'été n'y est pas appliquée pour
  l'instant, l'heure locale restant exacte grâce au décalage réel du système.

- **2026-11-06 — Restaurer les fenêtres après arrêt ou redémarrage.** La session Ferrite
  mémorise les applications, leurs dimensions et leur espace de travail avant un arrêt ou un
  redémarrage déclenché depuis Ferrite, puis relance les applications et replace leurs
  fenêtres au prochain démarrage. L'état interne des documents reste à la charge de chaque
  application ; les fenêtres d'applications absentes ou impossibles à relancer sont ignorées.

- **2026-11-06 — Déverrouillage par visage, intégré et limité.** Plutôt que Howdy (module
  PAM, absent de Debian 13, dépendances Python lourdes), un programme Ferrite :
  `ferrite-visage` (`services/visage`) lit la seule caméra infrarouge (GRAY8 : une photo
  sur un écran n'émet pas d'infrarouge), trouve le visage avec YuNet, en calcule
  l'empreinte avec SFace (modèles OpenCV, MIT et Apache 2.0, exécutés en Rust pur par
  `tract`) et la compare aux empreintes enregistrées (cosinus ≥ 0,6 sur deux images en
  4 s). Seules des empreintes sont gardées, jamais d'image, dans un fichier 0600 de
  l'utilisateur. Le mot de passe (`unix_chkpwd`) reste la seule vérification hors écran
  verrouillé : le visage ne sert jamais aux droits administrateur (polkit), ni avant le
  premier mot de passe de la session (il ouvre aussi le trousseau), et la vérification ne
  part que sur un geste (touche, clic, réveil), jamais au moment où l'écran se verrouille.
  Réglage `visage_deverrouillage` (désactivé par défaut). Les modèles (38 Mo) ne sont pas
  dans git : `tools/visage-modeles.sh` les télécharge et vérifie leur SHA-256.

- **2026-11-06 — Programmes administrateur sur XWayland (xhost root local).** `pkexec`
  retire l'accès à Wayland : GParted lancé en administrateur ne trouvait aucun écran.
  Quand XWayland est prêt, ferrite-shell lance `xhost +SI:localuser:root`, comme le
  paquet GParted d'Ubuntu : seul le compte root local, qui peut déjà tout sur la machine,
  peut ouvrir des fenêtres X11 ; ni le réseau ni les autres comptes. Si `/tmp/.X11-unix`
  a disparu (constaté une fois, cause inconnue), ferrite-shell le recrée en 1777 avant de
  lancer XWayland, sinon aucune application X11 ne s'affiche.
