#!/bin/sh
# Compile et lance ferrite-shell dans une fenêtre (WSLg), avec un client.
#
#   tools/shell-dev.sh                       ferrite-shell + terminal foot
#   tools/shell-dev.sh -- weston-terminal    autre client
#   tools/shell-dev.sh --capture build/a.png --capture-delay 4 -- foot
#
# Les options sont transmises à ferrite-shell (voir « ferrite-shell --help ») ; sans
# commande après « -- », foot est lancé.
set -eu
. "$(dirname -- "$0")/lib.sh"
cd "$REPO"

[ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ] || die "aucun affichage (WSLg) : WAYLAND_DISPLAY et DISPLAY sont vides"

cargo build -p ferrite-shell
target=${CARGO_TARGET_DIR:-$REPO/target}

# Sous WSLg, le Weston de Windows plante (signal 11) quand la fenêtre de ferrite-shell
# s'ouvre en Wayland : on la fait passer par X11 (XWayland). Les clients lancés dans
# ferrite-shell restent en Wayland, sur notre propre socket.
if [ -d /mnt/wslg ] && [ -n "${DISPLAY:-}" ]; then
    unset WAYLAND_DISPLAY
fi

case " $* " in
    *" -- "*) exec "$target/debug/ferrite-shell" "$@" ;;
    *) exec "$target/debug/ferrite-shell" "$@" -- foot ;;
esac
