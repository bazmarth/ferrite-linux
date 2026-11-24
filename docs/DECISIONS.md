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
  Complément du 2026-11-07 : les empreintes quittent le dossier personnel pour
  `/var/lib/ferrite/visage/<uid>` (root:groupe de l'utilisateur, 0640) ; ajouter ou
  effacer un visage passe par `pkexec` (mot de passe administrateur). Sinon, quiconque
  trouve la session ouverte pourrait ajouter son visage et rouvrir l'écran plus tard.
  `verifier` refuse un fichier qui n'appartient pas à root ou que d'autres peuvent écrire.
  Complément du 2026-11-07 (soir) : l'emplacement matériel de la caméra (lien sysfs) est
  retenu à l'enregistrement et seule cette caméra sert à `verifier` (une fausse caméra USB
  ne peut pas la remplacer) ; un ancien fichier sans caméra doit être refait. Limite
  connue et dite dans Paramètres : une photo imprimée sur papier peut tromper la caméra.

- **2026-11-06 — Programmes administrateur sur XWayland (xhost root local).** `pkexec`
  retire l'accès à Wayland : GParted lancé en administrateur ne trouvait aucun écran.
  Quand XWayland est prêt, ferrite-shell lance `xhost +SI:localuser:root`, comme le
  paquet GParted d'Ubuntu : seul le compte root local, qui peut déjà tout sur la machine,
  peut ouvrir des fenêtres X11 ; ni le réseau ni les autres comptes. Si `/tmp/.X11-unix`
  a disparu (constaté une fois, cause inconnue), ferrite-shell le recrée en 1777 avant de
  lancer XWayland, sinon aucune application X11 ne s'affiche.

- **2026-11-06 — Veille mémoire laissée au noyau, jamais `deep`.** Le 2026-11-06, une veille
  automatique est entrée en `deep` et le ThinkBook ne s'est jamais réveillé (redémarrage
  forcé). Le micrologiciel choisit lui-même `s2idle`, qui se réveille bien : le fichier
  `sleep.conf.d/60-ferrite.conf` qui forçait `deep` est retiré de l'image.

- **2026-11-06 — Une seule barre de titre, celle de Ferrite, par défaut.** Beaucoup
  d'applications egui ou winit demandent par `xdg-decoration` à dessiner leur cadre puis
  n'en dessinent pas (aucun bouton). La règle est inversée : toute fenêtre qui passe par ce
  protocole reçoit la barre Ferrite. Gardent la leur : les applications Ferrite et la liste
  `cadre_propre` (navigateurs Chromium, dont les onglets sont la barre). La règle est revue
  quand l'identifiant de la fenêtre arrive après la demande.

- **2026-11-06 — Sélecteur de fichiers Ferrite par le portail, GTK en repli.**
  `xdg-desktop-portal-ferrite` ne fournit que `FileChooser` et lance `ferrite-apps choisir`
  (une fenêtre par demande, processus à part : un plantage du sélecteur n'emporte ni le
  portail ni l'application). GTK reste l'implémentation de tout le reste et le repli
  (`FileChooser=ferrite;gtk`).

- **2026-11-06 — Applications par défaut dans les `mimeapps.list` standard.** Pas de fichier
  Ferrite : le choix est lu dans les `mimeapps.list` (utilisateur, `/etc/xdg`, système) et
  écrit dans `~/.config/mimeapps.list` sans toucher au reste, pour que `xdg-open` et les
  applications tierces suivent le même choix.

- **2026-11-07 — Vignettes vidéo dans un processus à part.** GStreamer dans Fichiers gardait
  la mémoire de chaque décodeur (690 Mo pour 9 vidéos). `ferrite-apps vignette-video`
  extrait une image et rend la mémoire en se terminant (98 Mo).

- **2026-11-07 — Copie de winit pour le pincement.** winit 0.30 ne lit pas le pincement du
  pavé tactile sous Wayland et eframe 0.37 en dépend. Plutôt qu'attendre une nouvelle
  version, une copie de winit 0.30.13 (`third_party/winit`, `[patch.crates-io]`) ajoute ce
  seul geste ; les écarts sont listés dans `third_party/winit/FERRITE.md`. À retirer quand
  une version d'eframe utilisera un winit qui le gère.

- **2026-11-07 — Veille sans fondu, verrouillage en fondu enchaîné zoomé.** Sur la vraie
  machine, le fondu au noir avant la veille se voyait mal : l'écran passe au noir d'un coup.
  Le verrouillage passe d'une scène à l'autre sans noir (photo du bureau qui recule, écran
  verrouillé qui se pose, 560 ms). C'est une texture plein écran par image pendant le
  passage seulement, exception assumée à la règle « rien de plein écran par image ».

- **2026-11-07 — Bouton d'alimentation : seul le bouton physique suit le réglage.** Les
  boutons dessinés (centre de contrôle, écran verrouillé) ouvrent toujours le dialogue
  d'arrêt, pour qu'un clic n'éteigne ou n'endorme jamais le PC par surprise. Le bouton
  physique : veille (par défaut), rien, ou dialogue.

- **2026-11-07 — Économie d'énergie lue dans `powerprofilesctl`.** La tuile montre le vrai
  profil (lu avec le reste de l'état système) au lieu d'un drapeau local. Le profil
  Performance compte comme « Désactivée » ; sans le démon, la tuile est « Indisponible » et
  sans effet (aucun bouton qui ment).

- **2026-11-07 — `PASSATION.md` devient `TECHNIQUE.md`.** Personne ne reprend le projet :
  Claude le développe à chaque séance. Le document reste la référence technique relue avant
  de toucher au code.

- **2026-11-07 — Curseurs : le bureau dessine tout, pack Ferrite embarqué.** Le bureau dessine
  déjà lui-même chaque pointeur (y compris sur les applications, d'après le nom de forme
  qu'elles demandent). Le thème choisi dans Paramètres est donc lu par le bureau seul, dans
  son fil système (`xcursor`), jamais au rendu. Le pack `Ferrite-Cursors` est compilé dans
  le binaire, comme les icônes du pack : rien à installer dans l'ISO ni sur le PC. Les
  formes vectorielles d'origine restent le choix par défaut. Taille : l'image du thème la
  plus proche au-dessus, réduite par le GPU à la taille voulue (24, 32 ou 48 points).

- **2026-11-07 — « Taille du texte » = zoom des applications, pas du bureau.** Les applis
  Ferrite fixent 130 tailles de police à la main : agrandir le seul texte casserait leurs
  mises en page. Le réglage agrandit tout le contenu de leurs fenêtres (`zoom_factor`
  d'egui), et passe aux applis GTK par `text-scaling-factor`. Barre, Dock et panneaux du
  bureau gardent leur taille (dessinés à des mesures fixes) ; la ligne de Paramètres le dit.

- **2026-11-07 — Écran éteint par le bureau, avant la veille.** Après `ecran_eteint` minutes
  sans entrée (10 par défaut, une minute sur l'écran verrouillé), le bureau coupe l'affichage
  par `DrmCompositor::clear()` (DPMS) et cesse de dessiner : la session continue (musique,
  téléchargements), seule la dalle s'éteint. Pas d'assombrissement avant, pour rester simple
  (une vidéo affichée empêche toujours l'extinction). La touche qui rallume l'écran est
  transmise à l'application, comme sous GNOME.

- **2026-11-07 — Sons système fabriqués par le code.** Aucun thème de sons n'est installé, et
  ceux de freedesktop ont des licences mêlées. Les sept sons de Ferrite (alertes Goutte,
  Cloche, Verre, Douce ; Corbeille, capture, démarrage) sont synthétisés par
  `ferrite_system::sounds` (sinusoïdes et bruit mis en forme), écrits une fois en WAV dans
  `$XDG_RUNTIME_DIR/ferrite-sons`, joués par `pw-play` dans un fil : rien à installer, rien
  qui bloque le bureau. Une notification sonne si elle est nouvelle, montrée en bulle (donc
  pas en « Ne pas déranger », sauf urgente) et sans `suppress-sound` ; les notifications du
  bureau lui-même ne sonnent pas (la capture a déjà son déclic). Le son de démarrage ne
  sonne qu'à l'ouverture de session (`FERRITE_LOCK=1`), pas à une relance du bureau.

- **2026-11-07 — Jauge du micro par GStreamer, seulement sur la page Son.** `pulsesrc ! level`
  (PipeWire répond par sa couche PulseAudio) : le micro est ouvert à l'affichage de la page
  Son et relâché dès qu'on la quitte.

- **2026-11-07 — Disques SATA sans économie d'énergie du lien.** Le SSD système du PC
  (iCoolax M.2 SATA, en bonne santé d'après SMART) se figeait 1 à 3 s plusieurs fois par
  minute avec le réglage par défaut de Debian (`min_power_with_partial`) : petites écritures
  bloquées jusqu'à 0,8 s, tout le système en attente du disque 15 % du temps, et le bureau
  figé quand il lisait un fichier à ce moment-là. En `max_performance`, l'attente tombe sous
  2 %. Règle udev `60-ferrite-sata-lpm.rules` (appliquée à chaque contrôleur SATA, sans
  dépendre de l'initramfs) ; un peu plus de consommation sur batterie.

- **2026-11-08 — Compilations en priorité basse.** Le dépôt est sur la partition NTFS
  « FICHIERS », sur le même SSD lent que le système : une compilation (target/ à 51 Go,
  24 Go écrits en 1 h 30 de travail) saturait le disque jusqu'à 71 % du temps et figeait le
  bureau. `priorite_basse` (`tools/lib.sh`, `renice 15` + `ionice -c 3`) est appelée par
  `ci.sh`, `dev-on.sh`, `vm-build.sh` et `usb.sh` : compilations un peu plus lentes, bureau
  fluide. Les scripts qui lancent le bureau (`shell-dev.sh`, `vm*.sh`) ne l'ont pas, pour ne
  pas ralentir ce qu'on regarde.
  Complément du 2026-11-08 : d'autres sessions lançaient `cargo build` directement, en
  priorité normale. `.cargo/config.toml` passe désormais chaque appel de rustc par
  `tools/rustc-bas.sh` (`build.rustc-wrapper`), qui le met en nice 15 et disque « idle » :
  toute compilation du dépôt est concernée, d'où qu'elle vienne (éditeur compris). Ne
  change ni les binaires ni ce qui est déjà compilé.

- **2026-11-08 — Échange zram, prioritaire sur le disque.** Pendant une compilation, 268 Mo
  de la mémoire de `ferrite-shell` étaient partis dans l'échange du disque (même SSD lent,
  déjà saturé) : une image a mis 35 s à se préparer. `systemd-zram-generator`
  (`zram-size = ram / 2`, zstd, priorité 100) et `vm.swappiness = 180`,
  `vm.page-cluster = 0` (comme Fedora et Pop!_OS). L'échange du disque reste en secours.
  Écarté : `mlockall` dans le bureau (limite `memlock` de 8 Mo pour l'utilisateur).

- **2026-11-08 — Calendrier : heure locale « flottante » et fichiers texte.** Les dates
  sont gardées en heure locale sans fuseau (un rendez-vous à 9 h reste à 9 h après un
  changement d'heure ou de fuseau), comme la plupart des agendas personnels ; l'heure
  actuelle vient de `date +%z`, relu chaque minute. Pas de nouvelle dépendance (ni chrono
  ni serde) : calcul des jours maison (Howard Hinnant) testé, et fichiers `clé=valeur` en
  français dans `~/.local/share/ferrite/calendrier/` (`agenda`, `rappels`), lisibles et
  écrits par renommage atomique. Écarté pour l'instant : import/export iCalendar (.ics).
- **2026-11-08 — Rappels du Calendrier par un service à part.** `ferrite-apps rappels`
  (sans fenêtre, un par dossier de données grâce à un verrou) est lancé par le bureau à
  l'ouverture de session et par le Calendrier ; il regarde chaque minute les rappels dus
  et les envoie au centre de notifications (`org.freedesktop.Notifications`) avec
  « Ouvrir », « Reporter 10 min » et « OK ». L'état (envoyé, reporté, acquitté) est dans
  le fichier `rappels`, écrit sous verrou par le service et par l'application : rien n'est
  annoncé deux fois, et un rappel manqué ordinateur éteint est rattrapé au retour s'il date
  de moins d'un jour et que l'événement n'est pas fini. Le service s'arrête avec le bus de
  session. Le Calendrier n'est pas épinglé au Dock d'origine (comme l'Horloge) : le Dock
  des utilisateurs ne change pas.
- **2026-11-09 — Le Calendrier, seule source des widgets et de la barre.** Le modèle de
  l'agenda (`date`, `model`, `store`) passe dans `ferrite_system::agenda`, lu par
  l'application et par le bureau. Le widget Tâches n'a plus de tâches à lui : ses
  feuilles sont les listes du Calendrier (plus une feuille « Sans liste » s'il y a des
  tâches sans liste), et chaque action relit l'agenda, le change et l'écrit aussitôt
  (l'application relit le fichier toutes les 2 s, le bureau aussi). Supprimer une
  feuille fait comme le Calendrier : ses tâches restent, sans liste. Les anciennes
  tâches des réglages (`tableau_liste_*`) sont reprises une fois dans l'agenda
  (`tableau_taches_calendrier = oui`), sans les tâches d'exemple. Le calendrier de la
  barre et le widget Calendrier marquent d'un point les jours avec un événement ou une
  tâche à faire ; un clic sur un jour ouvre le Calendrier sur ce jour, ailleurs sur le
  mois. Le bureau ne lance pas `date` pour l'heure de l'agenda : il reprend le décalage
  déjà lu par l'horloge de la barre.
- **2026-11-09 — Boutons de fenêtre tous à droite : réduire, agrandir, fermer.** Demande
  de l'utilisateur, après avoir vu PhotoCraft (barre à boutons à droite) à côté de la barre
  Ferrite (fermer à gauche). Même disposition partout : barre de titre du bureau,
  applications Ferrite (dans Ferrite comme ailleurs) et applications GTK
  (`button-layout` = `:minimize,maximize,close`). Le bouton réduire revient (comme dans
  PhotoCraft). Remplace l'ordre « fermer à gauche, agrandir à droite » du 2026-11-02.
- **2026-11-09 — Raccourcis sur le bureau des widgets.** À la demande, on revient en partie
  sur « le bureau n'a plus d'icônes » : applications, dossiers et fichiers peuvent avoir un
  raccourci, posé sur le bureau 0 (celui des widgets, où aucune fenêtre ne s'ouvre), en
  colonnes depuis le coin haut droit, sous les widgets. Un seul clic ouvre, comme le Dock.
  La liste est dans `~/.config/ferrite/raccourcis-bureau` (une ligne par raccourci,
  `application …` ou `chemin …`), écrite par renommage et relue par le fil système quand
  elle change : Fichiers y ajoute sans parler au bureau. Pas de place libre ni de glisser
  pour l'instant (ordre d'ajout) ; un raccourci recouvert par un widget n'est ni dessiné ni
  cliquable. Un raccourci dont l'application ou le chemin a disparu reste, nom estompé,
  avec seulement « Retirer du bureau ».
- **2026-11-09 — Écran de connexion par greetd.** Plusieurs comptes : la connexion
  automatique sur tty1 laisse place à greetd (console 7). L'écran de connexion est le bureau
  lui-même en mode `--greeter` (l'écran de verrouillage, avec « Changer d'utilisateur », ↑ ↓
  et Tab pour choisir le compte) ; le mot de passe va à greetd (PAM), jamais vérifié par
  Ferrite. Comptes : `/etc/passwd`, UID 1000 à 59999 avec un vrai shell, par UID ; le dernier
  ouvert est retenu dans le dossier de `_greetd`. Pas de visage à la connexion (le trousseau
  veut le mot de passe ; règle du visage inchangée). Système live : session ouverte seule
  (`[initial_session]`). Si l'écran plante : connexion texte `agreety`, jamais d'ordinateur
  fermé. Changer d'utilisateur sans fermer la session d'un autre demanderait un second
  greetd : plus tard.
- **2026-11-10 — Plus de verre dépoli sur les widgets.** Demande de l'utilisateur : le
  verre est gardé pour le Dock seulement. Les cartes ont un fond sombre uni à 84 %, net
  (sans flou du fond d'écran) ; leur dessin passe de 100 à 19 ms. Le verre du lanceur
  « Applications » reste.
- **2026-11-10 — Plus de réouverture des fenêtres au démarrage.** À la demande de
  l'utilisateur, les fenêtres ouvertes avant un arrêt, un redémarrage ou une déconnexion
  sont oubliées : le bureau repart vide. Le même jour, sur nouvelle demande, la
  mémorisation des fenêtres et le réglage « Rouvrir les fenêtres au démarrage » ont été
  **retirés entièrement** : plus rien n'est enregistré à l'arrêt ; l'ancien fichier
  `~/.local/state/ferrite/session` est effacé au démarrage s'il en reste un.
- **2026-11-10 — Pas de navigateur maison, Logithèque suffisante.** Décision de
  l'utilisateur : la phase 6 (port du navigateur de `D:\Os`) est abandonnée, Brave reste le
  navigateur de Ferrite. La Logithèque actuelle (catalogues Debian et Flathub,
  installation, mises à jour, `.deb`) couvre la phase 7 : plus de grand chantier dessus,
  seulement des corrections.
- **2026-11-10 — Pilotes graphiques AMD et NVIDIA téléchargés à la demande.** Leurs
  microprogrammes (15 et 40 Mo de paquets, environ 160 Mo une fois installés, recopiés
  dans l'initrd) ne sont plus dans l'ISO. Sans eux, ces cartes affichent l'image sans
  accélération (affichage simple du firmware). La Logithèque a une rubrique « Pilotes »,
  visible seulement sur un PC qui en a besoin (`/sys/bus/pci/devices`, classe `0x03`,
  fabricant `1002` ou `10de`, paquet absent de `/var/lib/dpkg/info`), qui installe le
  paquet par PackageKit. Le service des mises à jour l'annonce deux minutes après
  l'ouverture de session, une fois par semaine au plus. Pas sur le système live : il
  faut redémarrer pour utiliser le pilote, et ce qu'on installe sur la clé est perdu au
  redémarrage (bouton grisé, message). Les microprogrammes Wi-Fi restent dans l'ISO :
  sans réseau, rien ne se télécharge. Releases construites avec `tools/usb.sh --final`
  (compression xz).
- **2026-11-10 — Plusieurs écrans : dupliquer par défaut, étendre à droite.** Un écran
  branché (HDMI, DisplayPort) est détecté aussitôt (événement udev de la carte
  graphique). Réglage `ecrans_mode = dupliquer | etendre` (Paramètres › Écrans, Super+P).
  Dupliquer par défaut, comme Windows sur un portable : brancher une télévision ou un
  projecteur montre tout de suite le bureau. En « dupliquer », l'image de l'écran
  principal est recopiée, réduite ou agrandie, centrée, sans déformation (bandes noires
  si les proportions diffèrent). En « étendre », le second écran est placé à droite du
  principal ; il ne porte ni barre ni Dock (un seul exemplaire de l'interface, sur
  l'écran intégré). Une fenêtre agrandie ou accrochée remplit l'écran où elle se trouve.
  Les fenêtres laissées hors de tout écran (débranchement, retour à « dupliquer »)
  reviennent au centre du principal.
- **2026-11-10 — Panneaux découpés et fenêtres cachées ralenties.** L'image des menus et
  dialogues n'est plus envoyée en entier à la carte graphique : seul le rectangle
  dessiné part (à une échelle entière ; image entière à une échelle fractionnaire). Une
  fenêtre entièrement couverte par une autre ne reçoit plus qu'un rappel d'image par
  seconde (au lieu d'un par image) : elle ne redessine plus pour rien. Revers accepté :
  derrière une fenêtre translucide, son contenu ne bouge qu'une fois par seconde.
- **2026-11-10 — Ordonnanceur BFQ sur les disques SATA.** Mesure sur le ThinkBook : le
  bureau s'est figé 2,7 s pendant une compilation (`tools/ci.sh`, pourtant en `nice 15` et
  `ionice -c 3`). Le système et le dépôt sont sur le même SSD SATA (`sda`), dont
  l'ordonnanceur `mq-deadline` ignore `ionice` : les écritures de la compilation passaient
  devant les lectures du bureau. Règle udev `60-ferrite-disque-bfq.rules` (SATA non
  rotatifs seulement ; le NVMe garde `none`).
- **2026-11-10 — Lanceur sans voile sombre.** Demande de l'utilisateur : le lanceur
  s'ouvre en panneau blanc par-dessus le bureau, sans assombrir l'écran. Gain : seule la
  place du panneau est envoyée à la carte graphique.

- **2026-11-10 — Charte épurée (`docs/CHARTE.md`).** Maquette validée par l'utilisateur.
  Une seule couleur vive (l'accent, pour ce qui est allumé ou choisi) ; les surfaces se
  séparent par leur teinte, pas par des bordures. Colonnes : symboles gris d'une seule
  couleur au lieu de pastilles colorées (Réglages, Logithèque, Calendrier, sélecteur de
  fichiers). Centre de contrôle : deux grandes tuiles puis des bascules rondes sans texte
  d'état ; le chevron du mode nuit disparaît (ses réglages restent dans Réglages → Écrans).
  Barre du haut : symbole seul à la place du mot « Applications ». Bouton d'arrêt neutre :
  il demande déjà confirmation. Icônes d'applications inchangées.
- **2026-11-11 — Le Dock reste toujours en bas.** Demande de l'utilisateur : pas de Dock à
  gauche ou à droite. La ligne grisée « Position » de Paramètres › Bureau est retirée le
  même jour (demande de l'utilisateur) : ce n'est plus un objectif.
- **2026-11-11 — Bureaux : écran principal uniquement (oui par défaut).** Réglage
  `espaces_ecran_principal`. En mode « étendre », une fenêtre dont le centre est sur le
  second écran ne change pas de bureau : elle reste affichée, et ne compte pour aucun
  bureau (un bureau qui n'a qu'elle est vide). Désactivé, toutes les fenêtres suivent le
  bureau, comme avant. Oui par défaut, comme GNOME : le second écran sert souvent à garder
  une fenêtre sous les yeux (vidéo, documentation).
- **2026-11-11 — Fond d'écran et fond du lanceur dessinés dans un fil à part.** Seul le
  tout premier fond d'écran est dessiné sur place (rien n'anime encore l'écran). Ensuite
  (autre taille d'écran, autre fond), l'ancien fond reste affiché, étiré au besoin, pendant
  les 80 à 200 ms du dessin : un fond juste avec une image de retard vaut mieux qu'une
  saccade. Le fond du lanceur ouvert avant d'être prêt est dessiné sur place, comme avant.
- **2026-11-11 — Menus préparés d'avance au repos, sur place.** Centre de contrôle,
  notifications, presse-papiers et capture sont dessinés une fois, sans être affichés,
  1,5 s après la dernière entrée (un par image) : leur première ouverture passait de
  20–41 ms à 5–14 ms. Fait dans le fil du compositeur (les panneaux ne se partagent pas
  entre fils) : une image de retard au repos ne se voit pas, sauf pendant une vidéo,
  une seule fois par écran et par thème.
- **2026-11-11 — Fond du centre de contrôle dessiné par la carte graphique.** Ombre et
  verre du panneau passent du processeur (`Canvas::glass`, 18 ms par hauteur nouvelle,
  3 fois plus sur batterie) au shader `PANEL` (formule d'ombre identique à
  `elevation::profile`) ; le processeur ne dessine plus que le contenu, posé par-dessus
  avec le même agrandissement. Revers accepté : pendant le fondu d'ouverture (160 ms),
  fond et contenu s'estompent chacun de leur côté, au lieu d'une seule image.
- **2026-11-12 — Lecteur PDF : liens lus avec `lopdf`, pas avec Poppler.** `pdftohtml -xml`
  donne bien les liens, mais seulement par morceau de ligne et sans la hauteur visée :
  un clic sur [12] serait tombé en haut de la page de la bibliographie. `lopdf` (Rust pur,
  sans compilation de bibliothèque C) lit les vrais rectangles et destinations, dans un
  fil. Le rendu reste à Poppler. Seules les adresses `http`, `https` et `mailto` s'ouvrent
  (jamais `file:` ni lancement de programme) ; un lien vers un autre PDF l'ouvre en onglet.
- **2026-11-12 — Lecteur PDF : onglets plutôt que fenêtres.** Ouvrir un autre document
  ajoute un onglet (ou revient au sien) au lieu de remplacer le document lu ; la barre
  n'apparaît qu'à partir de deux. Les fenêtres séparées restent possibles par le réglage
  existant « plusieurs fenêtres » de Paramètres.
- **2026-11-12 — Premier plan : `focus_window` de winit branché sur `xdg_activation`.**
  winit 0.30 ne faisait rien sous Wayland ; une application à fenêtre unique qui recevait
  un fichier restait derrière. Retouche dans `third_party/winit` ; le compositeur gérait
  déjà la demande (sortie du Dock, changement de bureau).
- **2026-11-13 — Installateur maison (application Ferrite), pas Calamares.** Le plan
  (phase 9) le demande en application Ferrite : même charte, français simple, trois
  choix et un mode manuel au lieu d'un éditeur de partitions. Toute la décision
  (secteurs, alignement 1 Mio, bornes de Windows) est en Rust et testée
  (`installer/plan.rs`) ; le script `ferrite-installation` (root, par `sudo -n`, le compte
  live étant administrateur sans mot de passe) ne fait qu'exécuter le plan.
- **2026-11-13 — Copie du système de la clé (`unsquashfs`), pas `debootstrap`.**
  L'installation marche sans réseau et donne exactement le système essayé sur la clé ;
  les paquets du système live sont ensuite purgés. GRUB (UEFI signé ou BIOS) vient de
  paquets mis de côté dans l'image (hook `0130`), car `grub-efi-amd64` et `grub-pc` ne
  peuvent pas être installés ensemble.
- **2026-11-13 — À côté de Windows : seulement en réduisant NTFS, avec marge.** Windows
  garde ce qu'il occupe (mesuré par `ntfsresize`) plus 10 Go. Windows en veille
  prolongée (démarrage rapide) ou chiffré par BitLocker n'est pas touché : l'assistant
  explique quoi faire dans Windows. La partition EFI existante est gardée telle quelle.
  Pas de partition d'échange : zram suffit (déjà en place).
- **2026-11-22 — Nouvelle charte : accent « Oxyde », neutres sans teinte, thème GTK.**
  L'accent par défaut passe du bleu à l'Oxyde (`#B83A0A` en clair, `#FF7A45` en sombre),
  l'orange de l'oxyde de fer qui donne son nom à la ferrite ; le bleu reste au choix dans
  Paramètres (`accent = bleu`). Gris neutres (plus de bleuté), fenêtre `#F4F4F6` et cartes
  blanches en clair, `#141416` / `#1E1E22` en sombre ; chaque texte garde 4,5:1 (test
  `textes_lisibles_sur_chaque_fond_des_deux_themes`). Barre de titre 44 px ; boutons de
  fenêtre de 14 px, toujours à droite, « fermer » seul en accent au repos et symboles au
  survol ; contrôles de 32 px. Les applications tierces suivent par un thème GTK
  « Ferrite » / « Ferrite-dark » généré depuis la charte (`ferrite-theme/gtk/generer.py`),
  choisi par ferrite-shell avec le mode clair ou sombre. Dock et barre du haut inchangés.

- **2026-11-23 — Assistant de bienvenue : sept étapes après l'installation, quatre en live.**
  Après l'installation : langue et région, réseau, compte, confidentialité, apparence,
  applications, fin. Sur la clé : langue et région, réseau, apparence, fin (avec
  « Installer Ferrite »). Le compte se crée pendant l'installation, et une application
  installée dans l'essai disparaîtrait à l'arrêt. Il ne s'ouvre que sur la clé ou quand
  l'installation a écrit `bienvenue = a_faire` : un Ferrite déjà en service ne le voit pas
  surgir après une mise à jour. Fermer la fenêtre compte comme « fait ». Seul le français
  est proposé (les autres langues sont grisées « Traduction à venir ») : l'interface n'est
  pas traduite.
