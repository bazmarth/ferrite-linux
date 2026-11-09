#!/bin/sh
# Installe la dernière compilation de Ferrite pour tous les comptes de l'ordinateur, comme
# tools/install-dev/inside.sh : binaires dans /usr/local/bin, modèles du visage, sélecteur
# de fichiers (portail) et fichiers .desktop des applications.
#
#   tools/dev-on.sh                    (compile, en utilisateur)
#   sudo tools/installer-systeme.sh
#
# tools/dev-on.sh ne sert qu'au compte qui le lance (dossier personnel) : les autres comptes
# ouvraient les binaires de /usr/local/bin, restés à une ancienne version.
# shellcheck shell=sh
set -eu

# shellcheck source=tools/lib.sh
. "$(dirname -- "$0")/lib.sh"

[ "$(id -u)" = 0 ] || die "à lancer avec sudo"
user=${SUDO_USER:-}
[ -n "$user" ] && [ "$user" != root ] || die "lancer avec sudo depuis son compte (SUDO_USER absent)"
includes="$REPO/image/config/includes.chroot"
home=$(getent passwd "$user" | cut -d: -f6)

# Les binaires les plus récents : ceux de tools/dev-on.sh, sinon la compilation release.
source="$home/.local/share/ferrite-dev"
[ -x "$source/ferrite-shell" ] || source="${CARGO_TARGET_DIR:-$REPO/target}/release"
names="ferrite-shell ferrite-apps ferrite-visage xdg-desktop-portal-ferrite"
for name in $names; do
    [ -x "$source/$name" ] || die "$source/$name absent : lancer d'abord tools/dev-on.sh"
done

info "Binaires de $source"
for name in $names; do
    # Copie puis renommage : un binaire en cours d'exécution n'est pas écrasé en place.
    install -m 0755 "$source/$name" "/usr/local/bin/.$name.nouveau"
    mv -f "/usr/local/bin/.$name.nouveau" "/usr/local/bin/$name"
done
for script in "$includes"/usr/local/bin/ferrite-*; do
    install -m 0755 "$script" /usr/local/bin/
done

if ls "$REPO"/assets/visage/*.onnx >/dev/null 2>&1; then
    info "Modèles du visage"
    install -d /usr/share/ferrite/visage
    install -m 0644 "$REPO"/assets/visage/*.onnx /usr/share/ferrite/visage/
fi

info "Sélecteur de fichiers et applications Ferrite"
for dir in dbus-1/services xdg-desktop-portal xdg-desktop-portal/portals applications; do
    install -d "/usr/share/$dir"
    for file in "$includes/usr/share/$dir"/*; do
        [ -f "$file" ] && install -m 0644 "$file" "/usr/share/$dir/"
    done
done
update-desktop-database /usr/share/applications 2>/dev/null || true
ok "Ferrite à jour pour tous les comptes (à la prochaine ouverture de leur session)."
