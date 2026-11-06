#!/bin/sh
# Mode développement sur le disque (Ferrite installé par tools/install-dev.sh) : compile
# ferrite-shell, ferrite-apps et ferrite-visage, puis les copie là où ferrite-session les
# cherche (avec les modèles du visage, téléchargés une fois par tools/visage-modeles.sh).
# Ctrl+Alt+Retour arrière relance ensuite le bureau avec cette version.
#
#   tools/dev-on.sh                compile (release) puis copie
#   tools/dev-on.sh --sans-compiler  copie la dernière compilation telle quelle
#
# Si la version copiée plante deux fois de suite, la session repart seule sur les
# binaires stables de /usr/local/bin. tools/dev-off.sh revient à ces binaires.
# shellcheck shell=sh
set -eu

# shellcheck source=tools/lib.sh
. "$(dirname -- "$0")/lib.sh"
cd "$REPO"

build=1
case "${1:-}" in
    "") ;;
    --sans-compiler) build=0 ;;
    *) die "option inconnue : $1 (voir l'en-tête de $0)" ;;
esac

dev="${XDG_DATA_HOME:-$HOME/.local/share}/ferrite-dev"
target=${CARGO_TARGET_DIR:-"$REPO/target"}

if [ "$build" = 1 ]; then
    info "Compilation de ferrite-shell, ferrite-apps et ferrite-visage (release)"
    cargo build --locked --release -p ferrite-shell -p ferrite-apps -p ferrite-visage
fi

mkdir -p "$dev"
for name in ferrite-shell ferrite-apps ferrite-visage; do
    [ -x "$target/release/$name" ] || die "$target/release/$name absent : compiler d'abord"
    # Copie puis renommage : le binaire en cours d'exécution n'est pas écrasé en place.
    cp "$target/release/$name" "$dev/.$name.nouveau"
    mv -f "$dev/.$name.nouveau" "$dev/$name"
done
# Modèles du visage, à côté de ferrite-visage (voir services/visage/src/models.rs).
if [ -f "$REPO/assets/visage/face_recognition_sface_2021dec.onnx" ]; then
    mkdir -p "$dev/visage"
    cp "$REPO"/assets/visage/*.onnx "$dev/visage/"
else
    printf '  Modèles du visage absents : tools/visage-modeles.sh\n'
fi
ok "Binaires de développement dans $dev"

if ! grep -q disk_dev /usr/local/bin/ferrite-session 2>/dev/null; then
    printf '\n  /usr/local/bin/ferrite-session ne connaît pas encore ce mode. Une fois :\n'
    printf '    sudo install -m 0755 %s /usr/local/bin/\n' \
        "$REPO/image/config/includes.chroot/usr/local/bin/ferrite-session"
    printf '  puis redémarrer la session (ou le PC).\n'
else
    printf '\n  Bouton rouge → « Relancer le bureau » (ou Ctrl+Alt+Retour arrière) : bureau relancé avec cette version.\n'
    printf '  Les fenêtres ouvertes se ferment ; Claude retrouve la conversation dans la barre latérale.\n'
fi
