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
| `resvg` 0.49 (sans fonctions par défaut) | ferrite-shell, ferrite-apps | Apache-2.0/MIT | rendu des icônes SVG de `ferrite-theme` et `icons/SVG` (Dock, barre du haut, menus, Fichiers) ; sans `text` : les icônes n'utilisent que des tracés |
| `ferrite-system` (interne) | ferrite-shell, ferrite-apps | MIT | services du système avec délai maximal ; aucune dépendance externe |
| `zbus` 5 (`blocking-api`) | ferrite-shell | MIT | serveur `org.freedesktop.Notifications`, signal de mise en veille de logind ; imposé par le plan |
| `alacritty_terminal` 0.26 | ferrite-apps | Apache-2.0 | émulation du Terminal ; imposé par le plan |
| `eframe` 0.37 | ferrite-apps | MIT/Apache-2.0 | fenêtres natives Wayland pour Fichiers, TextEdit et Calculatrice ; rendu GLES et prise en charge clavier/souris |

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
| `foot`, `tmux` | terminal de secours ; le Terminal Ferrite ne les utilise pas |
| `fonts-jetbrains-mono` | police du terminal `foot` (`/etc/xdg/foot/foot.ini`) |

## Ressources

| Ressource | Licence | Fichier |
|---|---|---|
| Inter, InterDisplay | SIL OFL 1.1 | `assets/fonts/Inter-LICENSE.txt` |
| JetBrains Mono | SIL OFL 1.1 | `assets/fonts/JetBrainsMono-OFL.txt` |
| Source Serif 4 | SIL OFL 1.1 | `assets/fonts/SourceSerif4-LICENSE.txt` |
| Icônes Ferrite (`icons/SVG`) | fournies par l'utilisateur pour le projet | `icons/manifest.json` |
| Thème Ferrite (`ferrite-theme`) | fourni par l'utilisateur pour le projet ; licence à préciser | `ferrite-theme/README.md` |
