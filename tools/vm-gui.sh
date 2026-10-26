#!/bin/sh
# Ferrite VM : outil graphique pour démarrer et tester l'ISO dans QEMU (via WSLg).
# Depuis Windows : double-clic sur tools\ferrite-vm.cmd.
set -eu
. "$(dirname -- "$0")/lib.sh"
cd "$REPO"
[ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ] || die "aucun affichage (WSLg)"
cargo build --release -p ferrite-vm
# Comme ferrite-shell en développement : la fenêtre passe par X11 sous WSLg.
if [ -d /mnt/wslg ] && [ -n "${DISPLAY:-}" ]; then
    unset WAYLAND_DISPLAY
fi
FERRITE_REPO="$REPO" exec "${CARGO_TARGET_DIR:-$REPO/target}/release/ferrite-vm"
