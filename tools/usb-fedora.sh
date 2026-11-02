#!/bin/sh
# Construit build/ferrite-live.iso depuis Fedora, où live-build n'existe pas : les binaires
# sont compilés dans Debian 13 (tools/vm-build.sh), puis tools/usb.sh tourne dans un
# conteneur Debian 13 privilégié.
#
#   tools/usb-fedora.sh            construit l'image (demande sudo)
#   tools/usb-fedora.sh --propre   idem, en effaçant aussi le cache des paquets
#
# Le travail de live-build reste dans le volume Podman ferrite-image (système de fichiers
# Linux, conservé d'une construction à l'autre) : le dépôt peut être sur un disque NTFS.
# Le volume est monté avec dev,suid,exec : Podman le monterait sinon en nodev, et le chroot
# de live-build ne pourrait pas ouvrir /dev/null.
set -eu
. "$(dirname -- "$0")/lib.sh"

command -v podman >/dev/null 2>&1 || die "Podman absent (sudo dnf install podman)"

info "Compilation de ferrite-shell et ferrite-apps pour Debian 13"
"$REPO/tools/vm-build.sh"

info "Construction de l'image dans un conteneur Debian 13 (demande sudo)"
sudo podman run --rm -it --privileged \
    --security-opt label=disable \
    -v "$REPO:/src" \
    -v ferrite-image:/var/tmp/ferrite-image:dev,suid,exec \
    docker.io/library/debian:trixie \
    sh -eu -c '
        export DEBIAN_FRONTEND=noninteractive
        apt-get update
        apt-get install -y --no-install-recommends \
            live-build debootstrap xorriso squashfs-tools mtools dosfstools \
            grub-pc-bin grub-efi-amd64-bin ca-certificates
        FERRITE_SHELL_BIN=/src/target/release/ferrite-shell \
        FERRITE_APPS_BIN=/src/target/release/ferrite-apps \
        /src/tools/usb.sh "$@"
    ' usb "$@"

# L'image sort appartenant à root ; sur un disque NTFS, le propriétaire est fixe.
sudo chown "$(id -u):$(id -g)" "$BUILD" "$BUILD/ferrite-live.iso" "$BUILD/ferrite-live.iso.sha256" 2>/dev/null || true
ok "$BUILD/ferrite-live.iso"
