#!/bin/sh
# Mode développement sur le disque (Ferrite installé par tools/install-dev.sh) : compile
# ferrite-shell, ferrite-apps et ferrite-visage, puis les copie là où ferrite-session les
# cherche (avec les modèles du visage, téléchargés une fois par tools/visage-modeles.sh).
# Ctrl+Alt+Retour arrière relance ensuite le bureau avec cette version.
#
#   tools/dev-on.sh                compile (release) puis copie
#   tools/dev-on.sh --sans-compiler  copie la dernière compilation telle quelle
#
# Si la version copiée plante deux fois de suite, la session repart seule sur les
# binaires stables de /usr/local/bin. tools/dev-off.sh revient à ces binaires.
# shellcheck shell=sh
set -eu

# shellcheck source=tools/lib.sh
. "$(dirname -- "$0")/lib.sh"
priorite_basse
cd "$REPO"

build=1
case "${1:-}" in
    "") ;;
    --sans-compiler) build=0 ;;
    *) die "option inconnue : $1 (voir l'en-tête de $0)" ;;
esac

dev="${XDG_DATA_HOME:-$HOME/.local/share}/ferrite-dev"
target=${CARGO_TARGET_DIR:-"$REPO/target"}

if [ "$build" = 1 ]; then
    info "Compilation de ferrite-shell, ferrite-apps, ferrite-visage et du portail (release)"
    cargo build --locked --release -p ferrite-shell -p ferrite-apps -p ferrite-visage \
        -p xdg-desktop-portal-ferrite
fi

mkdir -p "$dev"
for name in ferrite-shell ferrite-apps ferrite-visage xdg-desktop-portal-ferrite; do
    [ -x "$target/release/$name" ] || die "$target/release/$name absent : compiler d'abord"
    # Copie puis renommage : le binaire en cours d'exécution n'est pas écrasé en place.
    cp "$target/release/$name" "$dev/.$name.nouveau"
    mv -f "$dev/.$name.nouveau" "$dev/$name"
done
# Modèles du visage, à côté de ferrite-visage (voir services/visage/src/models.rs).
if [ -f "$REPO/assets/visage/face_recognition_sface_2021dec.onnx" ]; then
    mkdir -p "$dev/visage"
    cp "$REPO"/assets/visage/*.onnx "$dev/visage/"
else
    printf '  Modèles du visage absents : tools/visage-modeles.sh\n'
fi
# Sélecteur de fichiers de Ferrite pour les applications (portail) : service D-Bus et
# préférence dans le dossier personnel ; seul le fichier .portal doit être dans /usr/share.
services="${XDG_DATA_HOME:-$HOME/.local/share}/dbus-1/services"
mkdir -p "$services" "${XDG_CONFIG_HOME:-$HOME/.config}/xdg-desktop-portal"
printf '[D-BUS Service]\nName=org.freedesktop.impl.portal.desktop.ferrite\nExec=%s\n' \
    "$dev/xdg-desktop-portal-ferrite" >"$services/org.freedesktop.impl.portal.desktop.ferrite.service"
cp "$REPO/image/config/includes.chroot/usr/share/xdg-desktop-portal/ferrite-portals.conf" \
    "${XDG_CONFIG_HOME:-$HOME/.config}/xdg-desktop-portal/ferrite-portals.conf"
portal=/usr/share/xdg-desktop-portal/portals/ferrite.portal
if [ ! -f "$portal" ]; then
    printf '  Sélecteur de fichiers Ferrite : une fois, avec le mot de passe :\n'
    printf '    sudo install -m 0644 %s /usr/share/xdg-desktop-portal/portals/\n' \
        "$REPO/image/config/includes.chroot$portal"
fi
# Applications Ferrite connues du système (Applications par défaut, xdg-open) : leurs
# fichiers .desktop, pointant vers les binaires de développement, cachés du menu.
apps_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$apps_dir"
for desktop in "$REPO"/image/config/includes.chroot/usr/share/applications/ferrite-*.desktop; do
    sed "s|^Exec=ferrite-apps |Exec=$dev/ferrite-apps |" "$desktop" >"$apps_dir/$(basename "$desktop")"
done
update-desktop-database "$apps_dir" 2>/dev/null || true
# Le portail relit sa configuration au démarrage : relancé pour prendre le nouveau service.
pkill -f xdg-desktop-portal-ferrite 2>/dev/null || true
systemctl --user try-restart xdg-desktop-portal.service 2>/dev/null || true
ok "Binaires de développement dans $dev"

if ! grep -q disk_dev /usr/local/bin/ferrite-session 2>/dev/null; then
    printf '\n  /usr/local/bin/ferrite-session ne connaît pas encore ce mode. Une fois :\n'
    printf '    sudo install -m 0755 %s /usr/local/bin/\n' \
        "$REPO/image/config/includes.chroot/usr/local/bin/ferrite-session"
    printf '  puis redémarrer la session (ou le PC).\n'
else
    printf '\n  Bouton rouge → « Relancer le bureau » (ou Ctrl+Alt+Retour arrière) : bureau relancé avec cette version.\n'
    printf '  Les fenêtres ouvertes se ferment ; Claude retrouve la conversation dans la barre latérale.\n'
fi
