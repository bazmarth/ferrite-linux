#!/bin/sh
# Installe le thème Ferrite (icônes + curseurs).
#   ./install.sh                 -> utilisateur courant (~/.local/share/icons)
#   ./install.sh --system        -> /usr/share/icons (root)
#   ./install.sh --prefix ROOTFS -> dans l'image de ferrite-linux (ROOTFS/usr/share/icons + défauts)
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
MODE=user; PREFIX=""
case "$1" in
  --system) MODE=system ;;
  --prefix) MODE=prefix; PREFIX="${2:?chemin du rootfs manquant}" ;;
  "") ;;
  *) echo "usage: $0 [--system | --prefix ROOTFS]"; exit 1 ;;
esac

case "$MODE" in
  user)   DEST="${XDG_DATA_HOME:-$HOME/.local/share}/icons" ;;
  system) DEST=/usr/share/icons ;;
  prefix) DEST="$PREFIX/usr/share/icons" ;;
esac

mkdir -p "$DEST"
rm -rf "$DEST/Ferrite" "$DEST/Ferrite-Cursors"
cp -a "$HERE/icons/Ferrite" "$DEST/"
cp -a "$HERE/cursors/Ferrite-Cursors" "$DEST/"
rm -f "$DEST/Ferrite-Cursors/cursor.theme"
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -f -t "$DEST/Ferrite" || true

# Thème de curseur par défaut (X11, Xwayland, toolkits sans gsettings)
if [ "$MODE" = user ]; then
  mkdir -p "$HOME/.icons/default"
  printf '[Icon Theme]\nInherits=Ferrite-Cursors\n' > "$HOME/.icons/default/index.theme"
  if command -v gsettings >/dev/null 2>&1; then
    gsettings set org.gnome.desktop.interface icon-theme 'Ferrite' 2>/dev/null || true
    gsettings set org.gnome.desktop.interface cursor-theme 'Ferrite-Cursors' 2>/dev/null || true
  fi
else
  ROOT="${PREFIX}"
  mkdir -p "$ROOT/usr/share/icons/default"
  printf '[Icon Theme]\nInherits=Ferrite-Cursors\n' > "$ROOT/usr/share/icons/default/index.theme"
  mkdir -p "$ROOT/etc/gtk-3.0" "$ROOT/etc/gtk-4.0"
  for v in 3.0 4.0; do
    printf '[Settings]\ngtk-icon-theme-name=Ferrite\ngtk-cursor-theme-name=Ferrite-Cursors\ngtk-cursor-theme-size=24\n' > "$ROOT/etc/gtk-$v/settings.ini"
  done
  mkdir -p "$ROOT/etc/profile.d"
  printf 'export XCURSOR_THEME=Ferrite-Cursors\nexport XCURSOR_SIZE=24\n' > "$ROOT/etc/profile.d/ferrite-theme.sh"
  mkdir -p "$ROOT/usr/share/glib-2.0/schemas"
  printf "[org.gnome.desktop.interface]\nicon-theme='Ferrite'\ncursor-theme='Ferrite-Cursors'\n" > "$ROOT/usr/share/glib-2.0/schemas/90_ferrite-theme.gschema.override"
  command -v glib-compile-schemas >/dev/null 2>&1 && glib-compile-schemas "$ROOT/usr/share/glib-2.0/schemas" 2>/dev/null || true
fi
echo "Ferrite installé dans $DEST"
