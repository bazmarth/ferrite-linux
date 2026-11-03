#!/bin/sh
set -e
DEST="${XDG_DATA_HOME:-$HOME/.local/share}/icons"
[ "$1" = "--system" ] && DEST=/usr/share/icons
rm -rf "$DEST/Ferrite" "$DEST/Ferrite-Cursors"
echo "Ferrite retiré de $DEST"
