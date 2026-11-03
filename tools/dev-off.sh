#!/bin/sh
# Quitte le mode développement sur le disque : retire les binaires copiés par
# tools/dev-on.sh. Au prochain Ctrl+Alt+Retour arrière, le bureau repart sur ceux
# de /usr/local/bin.
# shellcheck shell=sh
set -eu

# shellcheck source=tools/lib.sh
. "$(dirname -- "$0")/lib.sh"

dev="${XDG_DATA_HOME:-$HOME/.local/share}/ferrite-dev"
if [ ! -d "$dev" ]; then
    ok "Mode développement déjà inactif"
    exit 0
fi
# Ce dossier ne contient que des copies de target/release : pas besoin de corbeille.
rm -f "$dev/ferrite-shell" "$dev/ferrite-apps" "$dev/.ferrite-shell.nouveau" "$dev/.ferrite-apps.nouveau"
rmdir "$dev" 2>/dev/null || true
ok "Mode développement inactif : Ctrl+Alt+Retour arrière relance le bureau stable"
