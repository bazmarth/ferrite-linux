#!/bin/sh
# Écran de connexion (greetd + ferrite-greeter) sur un Ferrite déjà installé par
# tools/install-dev.sh, qui ouvrait la session sans rien demander (connexion automatique
# sur tty1). À lancer en administrateur, après une compilation release :
#
#   tools/dev-on.sh                       (compile, en utilisateur)
#   sudo tools/ecran-connexion.sh         installe greetd et l'écran de connexion
#   sudo tools/ecran-connexion.sh --retirer   revient à la connexion automatique
#
# Rien ne change avant le prochain démarrage. Si l'écran de connexion ne s'affiche pas,
# une connexion en mode texte le remplace ; Ctrl+Alt+F2 donne aussi une console, d'où
# « sudo tools/ecran-connexion.sh --retirer » rétablit l'ancien démarrage.
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
autologin=/etc/systemd/system/getty@tty1.service.d/autologin.conf

case "${1:-}" in
    --retirer)
        info "Retour à la connexion automatique de $user"
        systemctl disable greetd.service 2>/dev/null || true
        mkdir -p "$(dirname "$autologin")"
        cat >"$autologin" <<EOF
# Connexion automatique sur tty1 ; /etc/profile.d/90-ferrite.sh lance ensuite la session
# Ferrite (rétablie par tools/ecran-connexion.sh --retirer).
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin $user --noclear %I \$TERM
EOF
        systemctl daemon-reload
        ok "Au prochain démarrage, la session de $user s'ouvre seule."
        exit 0
        ;;
    "") ;;
    *) die "option inconnue : $1 (voir l'en-tête de $0)" ;;
esac

# Le binaire le plus récent : celui de tools/dev-on.sh, sinon la compilation release.
home=$(getent passwd "$user" | cut -d: -f6)
shell="$home/.local/share/ferrite-dev/ferrite-shell"
[ -x "$shell" ] || shell="${CARGO_TARGET_DIR:-$REPO/target}/release/ferrite-shell"
[ -x "$shell" ] || die "ferrite-shell compilé introuvable : lancer d'abord tools/dev-on.sh"
"$shell" --help | grep -q -- --greeter || die "$shell est trop ancien (pas d'option --greeter) : relancer tools/dev-on.sh"

info "Paquet greetd"
# Le paquet démarre greetd dès l'installation (connexion en mode texte sur la console 7,
# qui prendrait l'écran) : interdit le temps de l'installation.
made=0
if [ ! -e /usr/sbin/policy-rc.d ]; then
    printf '#!/bin/sh\nexit 101\n' >/usr/sbin/policy-rc.d
    chmod 0755 /usr/sbin/policy-rc.d
    made=1
    trap 'rm -f /usr/sbin/policy-rc.d' EXIT
fi
DEBIAN_FRONTEND=noninteractive apt-get install -y greetd
if [ "$made" = 1 ]; then
    rm -f /usr/sbin/policy-rc.d
    trap - EXIT
fi

info "Écran de connexion et session"
install -m 0755 "$shell" /usr/local/bin/.ferrite-shell.nouveau
mv -f /usr/local/bin/.ferrite-shell.nouveau /usr/local/bin/ferrite-shell
install -m 0755 "$includes/usr/local/bin/ferrite-greeter" /usr/local/bin/ferrite-greeter
install -m 0755 "$includes/usr/local/bin/ferrite-session" /usr/local/bin/ferrite-session
# Sans le bloc [initial_session] du système live : le mot de passe est demandé.
install -d /etc/greetd
sed '/^# Système live/,$d' "$includes/etc/greetd/config.toml" >/etc/greetd/config.toml
chmod 0644 /etc/greetd/config.toml

info "Démarrage sur l'écran de connexion"
rm -f "$autologin"
rmdir "$(dirname "$autologin")" 2>/dev/null || true
systemctl daemon-reload
systemctl enable greetd.service
ok "Au prochain démarrage, l'écran de connexion propose les comptes de l'ordinateur."
