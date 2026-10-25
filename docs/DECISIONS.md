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
