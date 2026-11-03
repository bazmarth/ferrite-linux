# Ferrite — thème visuel de ferrite-linux

Icônes (freedesktop, SVG) + curseurs (Xcursor compilés) + palette.

## Installation
```sh
./install.sh                    # utilisateur
sudo ./install.sh --system      # tout le système
./install.sh --prefix "$ROOTFS" # intégration dans l'image ferrite-linux
```
`--prefix` pose aussi les réglages par défaut : `/etc/gtk-{3,4}.0/settings.ini`,
`/usr/share/icons/default/index.theme`, `/etc/profile.d/ferrite-theme.sh` et
un override gsettings.

## Compositeurs Wayland
- Hyprland : `env = XCURSOR_THEME,Ferrite-Cursors` / `exec-once = hyprctl setcursor Ferrite-Cursors 24`
- Sway : `seat seat0 xcursor_theme Ferrite-Cursors 24`
- KDE : Paramètres → Apparence → Curseurs / Icônes

## Contenu
- `icons/Ferrite/scalable/apps` — 16 apps + alias (Nautilus, Firefox, kitty, VLC…)
- `icons/Ferrite/scalable/places` — dossiers XDG, corbeille, logo `start-here`
- `icons/Ferrite/scalable/mimetypes` — texte, image, audio, vidéo, archive, PDF, script, exécutable
- `icons/Ferrite/scalable/devices` — disque
- `cursors/Ferrite-Cursors` — 24 curseurs (attente/progression animés), tailles 24→96, alias X11 + CSS
- `cursors/Ferrite-Cursors/src` — sources SVG 32×32
- `palette/` — jetons GTK et JSON

Les icônes absentes sont reprises d'Adwaita (héritage).
