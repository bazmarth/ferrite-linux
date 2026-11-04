#!/bin/sh
# Construit build/ferrite-live.iso depuis Fedora, où live-build n'existe pas : les binaires
# sont compilés dans Debian 13 (tools/vm-build.sh), puis tools/usb.sh tourne dans un
# conteneur Debian 13 privilégié.
#
#   tools/usb-fedora.sh            construit l'image (demande sudo), rapide si possible
#   tools/usb-fedora.sh --complet  reconstruit aussi le système de l'image
#   tools/usb-fedora.sh --final    image à distribuer (complète, compression xz)
#   tools/usb-fedora.sh --propre   complète, en effaçant aussi le cache des paquets
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

# Outils de live-build installés une fois dans une image locale (refaite seulement si
# cette recette change), au lieu d'un apt-get à chaque construction.
info "Image des outils live-build (demande sudo)"
sudo podman build --quiet --tag localhost/ferrite-live-build:trixie \
    -f "$REPO/tools/containers/live-build.Containerfile" "$REPO/tools/containers" >/dev/null

info "Construction de l'image dans un conteneur Debian 13"
sudo podman run --rm -it --privileged \
    --security-opt label=disable \
    -v "$REPO:/src" \
    -v ferrite-image:/var/tmp/ferrite-image:dev,suid,exec \
    -e FERRITE_SQUASHFS \
    localhost/ferrite-live-build:trixie \
    sh -eu -c '
        FERRITE_SHELL_BIN=/src/target/release/ferrite-shell \
        FERRITE_APPS_BIN=/src/target/release/ferrite-apps \
        /src/tools/usb.sh "$@"
    ' usb "$@"

# L'image sort appartenant à root ; sur un disque NTFS, le propriétaire est fixe.
sudo chown "$(id -u):$(id -g)" "$BUILD" "$BUILD/ferrite-live.iso" "$BUILD/ferrite-live.iso.sha256" 2>/dev/null || true
ok "$BUILD/ferrite-live.iso"
