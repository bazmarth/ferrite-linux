#!/bin/sh
# Installe la dernière compilation de Ferrite pour tous les comptes de l'ordinateur, comme
# tools/install-dev/inside.sh : binaires dans /usr/local/bin, modèles du visage, sélecteur
# de fichiers (portail) et fichiers .desktop des applications. L'écran de connexion (qui
# utilise ces binaires) reprend aussi les choix de l'écran de verrouillage du compte.
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
if [ -z "$user" ] || [ "$user" = root ]; then
    die "lancer avec sudo depuis son compte (SUDO_USER absent)"
fi
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

# L'écran de connexion tourne sous le compte _greetd, qui ne peut pas lire le dossier
# personnel : il reçoit ici une copie des choix de l'écran de verrouillage du compte
# (fond, thème, couleur d'accent, accessibilité, clavier).
greeter_home=$(getent passwd _greetd | cut -d: -f6)
user_settings="$home/.config/ferrite/reglages"
if [ -n "$greeter_home" ] && [ -d "$greeter_home" ] && [ -f "$user_settings" ]; then
    info "Écran de connexion : réglages du verrouillage de $user"
    greeter_dir="$greeter_home/.config/ferrite"
    install -d -o _greetd -g _greetd "$greeter_dir"
    keys="verrouillage_fond verrouillage_theme accent contraste_eleve texte_taille \
reduire_animations clavier curseur_taille curseur_theme"
    copied=$(mktemp)
    for key in $keys; do
        line=$(grep -m 1 "^$key = " "$user_settings" || true)
        [ -n "$line" ] || continue
        if [ "$key" = verrouillage_fond ]; then
            # Une photo personnelle : copiée à côté, le dossier personnel restant fermé.
            photo=${line#verrouillage_fond = }
            case "$photo" in
                /*)
                    if [ -f "$photo" ]; then
                        install -m 0644 -o _greetd -g _greetd "$photo" "$greeter_dir/fond-verrouillage"
                        line="verrouillage_fond = $greeter_dir/fond-verrouillage"
                    else
                        continue
                    fi
                    ;;
            esac
        fi
        printf '%s\n' "$line" >>"$copied"
    done
    greeter_settings="$greeter_dir/reglages"
    merged=$(mktemp)
    # Les autres réglages de l'écran de connexion (widgets, dernier compte…) sont gardés.
    if [ -f "$greeter_settings" ]; then
        awk -F ' = ' 'FILENAME == ARGV[1] { copied[$1] = 1; next } !($1 in copied)' \
            "$copied" "$greeter_settings" >"$merged"
    fi
    cat "$copied" >>"$merged"
    install -m 0664 -o _greetd -g _greetd "$merged" "$greeter_settings"
    rm -f "$copied" "$merged"
fi
ok "Ferrite à jour pour tous les comptes (à la prochaine ouverture de leur session)."
