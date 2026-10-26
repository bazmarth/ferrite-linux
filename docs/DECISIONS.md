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
- **2026-10-26 — Redémarrage automatique du bureau** par `ferrite-session` après un arrêt
  sur erreur, au plus 3 fois en 2 minutes ; le journal précédent est gardé
  (`ferrite-shell.log.precedent`).

