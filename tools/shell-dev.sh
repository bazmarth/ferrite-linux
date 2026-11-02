#!/bin/sh
# Compile ferrite-shell et ferrite-apps, puis lance le bureau dans une fenêtre.
#
#   tools/shell-dev.sh
#   tools/shell-dev.sh -- weston-terminal
#   tools/shell-dev.sh --capture build/bureau.png --capture-delay 4 -- foot
#
# Les options avant « -- » sont transmises à ferrite-shell. Sans séparateur,
# foot est lancé par défaut comme client.
# shellcheck shell=sh
set -eu

# shellcheck source=tools/lib.sh
. "$(dirname -- "$0")/lib.sh"
cd "$REPO"

if [ -z "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]; then
    die "aucun affichage : WAYLAND_DISPLAY et DISPLAY sont vides"
fi

cargo build --locked -p ferrite-shell -p ferrite-apps
target=${CARGO_TARGET_DIR:-"$REPO/target"}

# Le Weston de WSLg peut planter à l'ouverture d'une fenêtre Wayland native.
# Utiliser XWayland pour ferrite-shell tout en gardant son socket Wayland pour les clients.
if [ -d /mnt/wslg ] && [ -n "${DISPLAY:-}" ]; then
    unset WAYLAND_DISPLAY
fi

has_separator=0
for arg do
    if [ "$arg" = "--" ]; then
        has_separator=1
        break
    fi
done

if [ "$has_separator" -eq 0 ]; then
    set -- "$@" -- foot
fi

exec "$target/debug/ferrite-shell" "$@"
