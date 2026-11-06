#!/bin/sh
# Construit l'image live hybride BIOS/UEFI : build/ferrite-live.iso, avec ferrite-shell,
# ferrite-apps, ferrite-visage et les modèles du déverrouillage par visage.
#
#   tools/usb.sh            compile le compositeur et les applications (release), puis construit l'image
#   tools/usb.sh --complet  reconstruit aussi le système de l'image (paquets réinstallés)
#   tools/usb.sh --final    image à distribuer : reconstruction complète, compression xz
#   tools/usb.sh --propre   reconstruction complète, en effaçant aussi le cache des paquets
#
# Rapide par défaut : si ni la configuration live-build, ni les listes de paquets, ni les
# hooks n'ont changé depuis la dernière image, le système déjà installé est gardé ; seuls
# les fichiers de Ferrite (binaires, scripts de includes.chroot) y sont recopiés, puis
# l'image est refaite (compression zstd). Compter quelques minutes au lieu d'une demi-heure.
#
# live-build travaille dans le système de fichiers Linux (FERRITE_WORK), pas sur /mnt/d :
# il y crée des fichiers spéciaux et des liens que le disque Windows ne gère pas.
set -eu
. "$(dirname -- "$0")/lib.sh"

# Étape 1, en utilisateur normal : compilation de ferrite-shell (Rust est installé pour
# l'utilisateur, pas pour root), puis relance du script en root.
if [ "$(id -u)" -ne 0 ]; then
    info "Compilation de ferrite-shell, ferrite-apps et ferrite-visage (release)"
    (cd "$REPO" && cargo build --release --locked -p ferrite-shell -p ferrite-apps -p ferrite-visage)
    # Modèles du visage : téléchargés une fois (empreintes vérifiées), hors de git.
    "$REPO/tools/visage-modeles.sh"
    target="${CARGO_TARGET_DIR:-$REPO/target}/release"
    exec sudo FERRITE_SHELL_BIN="$target/ferrite-shell" FERRITE_APPS_BIN="$target/ferrite-apps" \
        FERRITE_VISAGE_BIN="$target/ferrite-visage" "$0" "$@"
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
visage_bin=${FERRITE_VISAGE_BIN:-}
if [ -z "$visage_bin" ] || [ ! -x "$visage_bin" ]; then
    die "binaire ferrite-visage introuvable (lance tools/usb.sh sans sudo)"
fi
models="$REPO/assets/visage"
for model in face_detection_yunet_2023mar.onnx face_recognition_sface_2021dec.onnx; do
    [ -f "$models/$model" ] || die "modèle $model absent (tools/visage-modeles.sh)"
done

work=${FERRITE_WORK:-/var/tmp/ferrite-image}
mkdir -p "$work"
cd "$work"
full=0
case "${1:-}" in
    --propre)
        info "Nettoyage complet de $work"
        lb clean --purge
        full=1
        ;;
    --complet) full=1 ;;
    --final)
        export FERRITE_SQUASHFS=xz
        full=1
        ;;
    "") ;;
    *) die "option inconnue : $1 (--complet, --final ou --propre)" ;;
esac

# Empreinte de ce qui façonne le système de l'image (tout sauf includes.chroot, recopié
# à chaque fois) et de la compression choisie.
stamp=$(
    cd "$REPO/image"
    {
        find auto config -type f ! -path 'config/includes.chroot/*' | LC_ALL=C sort | xargs sha256sum
        echo "squashfs=${FERRITE_SQUASHFS:-zstd}"
    } | sha256sum | cut -d' ' -f1
)
if [ "$full" = 0 ] && [ -d chroot/usr ] && [ "$(cat .ferrite-stamp 2>/dev/null)" = "$stamp" ]; then
    fast=1
else
    fast=0
fi

info "Copie de la configuration dans $work"
rm -rf auto config
cp -r "$REPO/image/auto" "$REPO/image/config" .
install -D -m 0755 "$bin" config/includes.chroot/usr/local/bin/ferrite-shell
install -D -m 0755 "$apps_bin" config/includes.chroot/usr/local/bin/ferrite-apps
install -D -m 0755 "$visage_bin" config/includes.chroot/usr/local/bin/ferrite-visage
# Modèles du visage, là où ferrite-visage les cherche une fois installé.
install -d config/includes.chroot/usr/share/ferrite/visage
install -m 0644 "$models"/*.onnx config/includes.chroot/usr/share/ferrite/visage/
chmod +x auto/config config/hooks/live/*.hook.chroot config/includes.chroot/usr/local/bin/*

if [ "$fast" = 1 ]; then
    info "Système de l'image inchangé : mise à jour des fichiers de Ferrite seulement"
    lb config
    cp -a config/includes.chroot/. chroot/
    lb clean --binary
    lb binary
else
    info "Construction complète (15 à 30 minutes)"
    rm -f .ferrite-stamp
    lb clean
    lb config
    lb build
    echo "$stamp" >.ferrite-stamp
fi

mkdir -p "$BUILD"
cp live-image-amd64.hybrid.iso "$BUILD/ferrite-live.iso"
sha256sum "$BUILD/ferrite-live.iso" | sed "s|$BUILD/||" >"$BUILD/ferrite-live.iso.sha256"
if [ -n "${SUDO_UID:-}" ]; then
    chown "$SUDO_UID:${SUDO_GID:-$SUDO_UID}" "$BUILD" "$BUILD/ferrite-live.iso" "$BUILD/ferrite-live.iso.sha256"
fi
ok "$BUILD/ferrite-live.iso ($(du -h "$BUILD/ferrite-live.iso" | cut -f1))"
