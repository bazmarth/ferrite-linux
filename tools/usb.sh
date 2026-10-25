#!/bin/sh
# Construit l'image live hybride BIOS/UEFI : build/ferrite-live.iso.
#
#   sudo tools/usb.sh           construction (réutilise le cache des paquets)
#   sudo tools/usb.sh --propre  efface aussi le cache et repart de zéro
#
# live-build travaille dans le système de fichiers Linux (FERRITE_WORK), pas sur /mnt/d :
# il y crée des fichiers spéciaux et des liens que le disque Windows ne gère pas.
set -eu
. "$(dirname -- "$0")/lib.sh"

[ "$(id -u)" -eq 0 ] || die "lance ce script avec sudo"
command -v lb >/dev/null 2>&1 || die "live-build absent (sudo tools/setup-wsl.sh)"

work=${FERRITE_WORK:-/var/tmp/ferrite-image}
mkdir -p "$work"
cd "$work"
if [ "${1:-}" = "--propre" ]; then
    info "Nettoyage complet de $work"
    lb clean --purge
fi

info "Copie de la configuration dans $work"
rm -rf auto config
cp -r "$REPO/image/auto" "$REPO/image/config" .
chmod +x auto/config config/hooks/live/*.hook.chroot config/includes.chroot/usr/local/bin/*

info "Construction (première fois : 15 à 30 minutes)"
lb clean
lb config
lb build

mkdir -p "$BUILD"
cp live-image-amd64.hybrid.iso "$BUILD/ferrite-live.iso"
sha256sum "$BUILD/ferrite-live.iso" | sed "s|$BUILD/||" >"$BUILD/ferrite-live.iso.sha256"
if [ -n "${SUDO_UID:-}" ]; then
    chown "$SUDO_UID:${SUDO_GID:-$SUDO_UID}" "$BUILD" "$BUILD/ferrite-live.iso" "$BUILD/ferrite-live.iso.sha256"
fi
ok "$BUILD/ferrite-live.iso ($(du -h "$BUILD/ferrite-live.iso" | cut -f1))"
