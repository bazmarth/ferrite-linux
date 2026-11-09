# Dépendances

Chaque dépendance avec sa licence et la raison de sa présence. Les versions exactes sont
épinglées par `Cargo.lock`.

## Crates Rust

`ferrite-ui` n'utilise que la bibliothèque standard.

| Crate | Utilisée par | Licence | Raison |
|---|---|---|---|
| `smithay` 0.7 | ferrite-shell | MIT | compositeur Wayland (imposé par le plan) ; fonctions `backend_drm`, `backend_libinput`, `backend_winit`, `desktop`, `renderer_gl`, `wayland_frontend`, `xwayland` |
| `bitflags` 2 | ferrite-shell | MIT/Apache-2.0 | bords de redimensionnement |
| `fontdue` 0.9 | ferrite-shell | MIT/Apache-2.0/Zlib | rendu du titre des fenêtres (police Inter) ; prévu par le plan |
| `png` 0.18 | ferrite-shell | MIT/Apache-2.0 | écriture des captures (`--capture`) |
| `tracing`, `tracing-subscriber` | ferrite-shell | MIT | journal (celui de Smithay) ; niveau réglable par `RUST_LOG` |
| `resvg` 0.49 (sans fonctions par défaut) | ferrite-shell, ferrite-apps | Apache-2.0/MIT | rendu des icônes SVG de `ferrite-theme` et `icons/SVG` (Dock, barre du haut, menus, Fichiers) ; sans `text` dans ferrite-shell (les icônes n'utilisent que des tracés). ferrite-apps active `text` et `system-fonts` pour les badges SVG de l'aperçu Markdown (ajoute `fontdb`, `fontconfig-parser`, `roxmltree`, `unicode-bidi`, `unicode-script`, `unicode-vo`, `xmlwriter`, `base64`, tous MIT/Apache) |
| `ferrite-system` (interne) | ferrite-shell, ferrite-apps | MIT | services du système avec délai maximal ; aucune dépendance externe |
| `zbus` 5 (`blocking-api`) | ferrite-shell, ferrite-apps, xdg-desktop-portal-ferrite | MIT | serveur `org.freedesktop.Notifications`, logind (veille, bouton d'alimentation), agent polkit, MPRIS (îlot, Musique), portail `FileChooser`, envoi Bluetooth (`obexd`) ; imposé par le plan |
| `alacritty_terminal` 0.26 | ferrite-apps | Apache-2.0 | émulation du Terminal ; imposé par le plan |
| `eframe` 0.37 | ferrite-apps | MIT/Apache-2.0 | fenêtres natives Wayland de toutes les applications Ferrite ; rendu GLES et prise en charge clavier/souris |
| `epaint` 0.37 (déjà tiré par eframe) | ferrite-apps | MIT/Apache-2.0 | seulement pour activer `color_fonts` : emoji en couleur (police Noto Color Emoji du système) ; aucune bibliothèque nouvelle |
| `winit` 0.30.13 (copie dans `third_party/winit`) | ferrite-apps (par eframe) | Apache-2.0 | remplace la version de crates.io (`[patch.crates-io]`) pour transmettre le pincement du pavé tactile sous Wayland ; détails dans `third_party/winit/FERRITE.md` |
| `qrcode` 0.14 (sans fonctions par défaut, aucune dépendance) | ferrite-apps | MIT/Apache-2.0 | Paramètres › Wi-Fi › Détails : QR code pour partager le réseau |
| `miniz_oxide` 0.8 (déjà tirée par `png`) | ferrite-apps | MIT/Zlib/Apache-2.0 | Fichiers › Créer un PDF : images PNG compressées sans perte (zlib) |
| `async-channel` 2 | ferrite-shell | MIT/Apache-2.0 | réponses de l'agent polkit attendues sans bloquer la boucle |
| `libc` 0.2 | ferrite-shell | MIT/Apache-2.0 | `statvfs` : espace disque libre affiché par le bureau |
| `gstreamer`, `gstreamer-app`, `gstreamer-video` 0.25 | ferrite-apps, ferrite-visage | MIT/Apache-2.0 (GStreamer : LGPL, liaison dynamique) | Lecteur vidéo, vignettes des vidéos dans Fichiers, lecture de la caméra infrarouge |
| `symphonia` 0.5 (mp3, aac, isomp4, alac) | ferrite-apps | MPL-2.0 | Musique : décodage et lecture des métadonnées des morceaux |
| `tract-onnx` 0.21 | ferrite-visage | MIT/Apache-2.0 | exécution des modèles YuNet et SFace (visage) en Rust pur, sans OpenCV |
| `xcursor` 0.3 | ferrite-shell | MIT | lecture des thèmes de curseurs X (pack Ferrite embarqué, Adwaita…) ; déjà présent par winit |
| `blocking` 1 | xdg-desktop-portal-ferrite | MIT/Apache-2.0 | fenêtre du sélecteur (`ferrite-apps choisir`) attendue dans un fil à part, sans bloquer les autres demandes |

## Paquets de l'image live

Liste : `image/config/package-lists/ferrite.list.chroot`. Points notables :

| Paquet | Raison |
|---|---|
| `firmware-realtek` | firmware `rtw89/rtw8852b_fw*.bin` du Wi-Fi RTL8852BE |
| `firmware-intel-graphics`, `intel-media-va-driver` | Intel Iris Xe (i915/xe, décodage vidéo) |
| `firmware-sof-signed`, `alsa-ucm-conf` | son des portables Intel récents (Sound Open Firmware) |
| `dbus-user-session` | services utilisateur PipeWire/WirePlumber sans environnement de bureau |
| `libinput-tools`, `alsa-utils`, `pciutils`, `usbutils` | vérifications de la phase 1 (`ferrite-check`) |
| `xwayland`, `mesa-vulkan-drivers`, `libegl1`, `libgles2` | préparés pour le compositeur (phase 2) |
| `xdg-desktop-portal`, `xdg-desktop-portal-gtk` | dialogues de fichiers et ouverture de liens des applications (navigateurs, Flatpak) |
| `pulseaudio-utils` | `pactl` : balance et volume par application dans Paramètres |
| `curl` | relevé de la météo (widget Horloge, écran de verrouillage) |
| `libarchive-tools` | `bsdtar` : création ZIP/TAR.GZ et extraction ZIP/TAR/RAR dans Fichiers |
| `rar` | création d’archives RAR ; outil propriétaire fourni par les dépôts non libres |
| `wf-recorder` | enregistrement vidéo de l’écran (MP4) depuis la barre de capture ; lit les images par le protocole screencopy du compositeur, encode avec FFmpeg |
| `dbus-bin` | `dbus-update-activation-environment` : transmet `WAYLAND_DISPLAY` et `DISPLAY` aux services D-Bus |
| `foot` | terminal de secours ; le Terminal Ferrite ne l'utilise pas |
| `fish` | shell des terminaux du bureau (complétion, suggestions) |
| `power-profiles-daemon` | `powerprofilesctl` : tuile « Économie d'énergie » |
| `pkexec`, `polkitd`, `accountsservice` | mot de passe administrateur (agent polkit du bureau), « Ouvrir en administrateur », enregistrement du visage ; Paramètres → Comptes (ajout, suppression, mots de passe, images de profil) |
| `greetd` | écran de connexion : lance `ferrite-greeter` (`_greetd`), vérifie le mot de passe par PAM (`/etc/pam.d/greetd`, qui ouvre aussi gnome-keyring) et ouvre `ferrite-session --greetd` ; `agreety` sert de repli en mode texte |
| `libcrypt1` (toujours présent ; `libcrypt-dev` pour compiler, tiré par `libc6-dev`) | ferrite-apps : chiffrement des mots de passe (`crypt_ra`, méthode par défaut du système) avant de les donner à AccountsService |
| `gnome-keyring`, `libpam-gnome-keyring`, `gcr` | coffre à mots de passe (Secret Service) des navigateurs et d'Electron |
| `bluez-obexd` | `obexd` (bus de session) : « Envoyer par Bluetooth » dans Fichiers (profil Object Push) |
| `cups`, `cups-browsed`, `ipp-usb` | Paramètres › Imprimantes : service d’impression, imprimantes du réseau trouvées seules, imprimantes USB récentes sans pilote |
| `tesseract-ocr`, `tesseract-ocr-fra` | Capture › Texte : reconnaissance du texte d'une zone de l'écran (français et anglais), copié dans le presse-papiers |
| `ntfs-3g` | disques et clés formatés sous Windows |
| `libgstreamer1.0-0`, `gstreamer1.0-plugins-base`, `-good`, `gstreamer1.0-libav` | Lecteur vidéo, Musique, caméra infrarouge ; libav pour le H.264 |
| `libglib2.0-bin`, `gsettings-desktop-schemas`, `dconf-gsettings-backend` | thème clair/sombre et ordre des boutons annoncés aux applications GTK |
| `x11-xserver-utils` | `xhost` : programmes lancés par `pkexec` (GParted) affichés sur XWayland |
| `xsettingsd` | échelle ×2 des programmes GTK sous X11 |
| `plymouth-label` | écran de démarrage Ferrite (thème `ferrite`) |
| `poppler-utils` | Lecteur PDF : `pdfinfo` (pages), `pdftoppm` (rendu), `pdftotext` (recherche) |
| `fonts-jetbrains-mono` | police du terminal `foot` (`/etc/xdg/foot/foot.ini`) |

## Ressources

| Ressource | Licence | Fichier |
|---|---|---|
| Inter, InterDisplay | SIL OFL 1.1 | `assets/fonts/Inter-LICENSE.txt` |
| JetBrains Mono | SIL OFL 1.1 | `assets/fonts/JetBrainsMono-OFL.txt` |
| Source Serif 4 | SIL OFL 1.1 | `assets/fonts/SourceSerif4-LICENSE.txt` |
| Icônes Ferrite (`icons/SVG`), logo | fournies par l'utilisateur pour le projet | `icons/manifest.json` |
| Modèles YuNet et SFace (visage) | MIT, Apache-2.0 (OpenCV Zoo) | téléchargés par `tools/visage-modeles.sh`, hors git |
| Thème Ferrite (`ferrite-theme`) | fourni par l'utilisateur pour le projet ; licence à préciser | `ferrite-theme/README.md` |
