# Dépendances

Chaque dépendance avec sa licence et la raison de sa présence. Les versions exactes sont
épinglées par `Cargo.lock`.

## Crates Rust

`ferrite-ui` n'utilise que la bibliothèque standard.

| Crate | Utilisée par | Licence | Raison |
|---|---|---|---|
| `smithay` 0.7 | ferrite-shell | MIT | compositeur Wayland (imposé par le plan) ; fonctions `backend_winit`, `desktop`, `renderer_gl`, `wayland_frontend` |
| `bitflags` 2 | ferrite-shell | MIT/Apache-2.0 | bords de redimensionnement |
| `fontdue` 0.9 | ferrite-shell | MIT/Apache-2.0/Zlib | rendu du titre des fenêtres (police Inter) ; prévu par le plan |
| `png` 0.18 | ferrite-shell | MIT/Apache-2.0 | écriture des captures (`--capture`) |
| `tracing`, `tracing-subscriber` | ferrite-shell | MIT | journal (celui de Smithay) ; niveau réglable par `RUST_LOG` |

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

## Ressources

| Ressource | Licence | Fichier |
|---|---|---|
| Inter, InterDisplay | SIL OFL 1.1 | `assets/fonts/Inter-LICENSE.txt` |
| JetBrains Mono | SIL OFL 1.1 | `assets/fonts/JetBrainsMono-OFL.txt` |
| Source Serif 4 | SIL OFL 1.1 | `assets/fonts/SourceSerif4-LICENSE.txt` |
