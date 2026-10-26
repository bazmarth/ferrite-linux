#!/bin/sh
# Construit l'image live hybride BIOS/UEFI : build/ferrite-live.iso, avec ferrite-shell.
#
#   tools/usb.sh            compile le compositeur et les applications (release), puis construit l'image
#   tools/usb.sh --propre   idem, en effaçant aussi le cache des paquets
#
# live-build travaille dans le système de fichiers Linux (FERRITE_WORK), pas sur /mnt/d :
# il y crée des fichiers spéciaux et des liens que le disque Windows ne gère pas.
set -eu
. "$(dirname -- "$0")/lib.sh"

# Étape 1, en utilisateur normal : compilation de ferrite-shell (Rust est installé pour
# l'utilisateur, pas pour root), puis relance du script en root.
if [ "$(id -u)" -ne 0 ]; then
    info "Compilation de ferrite-shell et ferrite-apps (release)"
    (cd "$REPO" && cargo build --release --locked -p ferrite-shell -p ferrite-apps)
    target="${CARGO_TARGET_DIR:-$REPO/target}/release"
    exec sudo FERRITE_SHELL_BIN="$target/ferrite-shell" FERRITE_APPS_BIN="$target/ferrite-apps" "$0" "$@"
fi

# Étape 2, en root : construction de l'image.
command -v lb >/dev/null 2>&1 || die "live-build absent (sudo tools/setup-wsl.sh)"
bin=${FERRITE_SHELL_BIN:-}
apps_bin=${FERRITE_APPS_BIN:-}
if [ -z "$bin" ] || [ ! -x "$bin" ]; then
    die "binaire ferrite-shell introuvable (lance tools/usb.sh sans sudo)"
fi
if [ -z "$apps_bin" ] || [ ! -x "$apps_bin" ]; then
    die "binaire ferrite-apps introuvable (lance tools/usb.sh sans sudo)"
fi

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
install -D -m 0755 "$bin" config/includes.chroot/usr/local/bin/ferrite-shell
install -D -m 0755 "$apps_bin" config/includes.chroot/usr/local/bin/ferrite-apps
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
