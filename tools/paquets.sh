#!/bin/sh
# Paquets .deb du bureau Ferrite (phase 9) : build/paquets/*.deb.
#
#   tools/paquets.sh                 compile en release puis fabrique les paquets
#   tools/paquets.sh --sans-compiler reprend la dernière compilation release
#
# Paquets :
#   ferrite-shell               bureau (compositeur), session, écran de connexion
#   ferrite-apps                applications (Fichiers, Paramètres, TextEdit…) et leurs entrées de menu
#   ferrite-visage              déverrouillage par le visage
#   ferrite-visage-modeles      ses modèles (38 Mo) : version fixe, jamais retéléchargés
#   xdg-desktop-portal-ferrite  sélecteur de fichiers pour les applications (portail)
#   ferrite-desktop             métapaquet : installe les quatre et branche le dépôt apt Ferrite
#
# Les programmes restent dans /usr/local/bin, comme avant les paquets (voir DECISIONS) : dpkg
# remplace sans rien demander les copies faites à la main par tools/installer-systeme.sh.
# Les réglages du système (/etc : greetd, modprobe, zram…) restent dans l'image.
#
# Version : celle de Cargo.toml suivie de la date de fabrication (0.1.0+202611281030),
# toujours croissante : apt voit chaque nouvelle fabrication comme une mise à jour.
#
# Dépôt apt : ferrite-desktop contient la clé publique packaging/ferrite-depot.pgp
# (fabriquée par tools/cle-depot.sh) ; FERRITE_DEPOT_CLE_PUBLIQUE en choisit une autre
# (essais). Sans clé, ferrite-desktop est fabriqué sans le dépôt.
set -eu
. "$(dirname -- "$0")/lib.sh"
priorite_basse
cd "$REPO"

command -v dpkg-deb >/dev/null 2>&1 || die "dpkg-deb absent (paquet dpkg)"
command -v dpkg-shlibdeps >/dev/null 2>&1 || die "dpkg-shlibdeps absent (sudo apt install dpkg-dev)"

url=${FERRITE_DEPOT_URL:-https://bazmarth.github.io/ferrite-paquets}
cle=${FERRITE_DEPOT_CLE_PUBLIQUE:-$REPO/packaging/ferrite-depot.pgp}
includes="$REPO/image/config/includes.chroot"
models="$REPO/assets/visage"
release="${CARGO_TARGET_DIR:-$REPO/target}/release"

base=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
[ -n "$base" ] || die "version introuvable dans Cargo.toml"
version=${FERRITE_VERSION:-$base+$(date -u +%Y%m%d%H%M)}
models_version=2023.3

if [ "${1:-}" != --sans-compiler ]; then
    info "Compilation (release)"
    cargo build --release --locked -p ferrite-shell -p ferrite-apps -p ferrite-visage \
        -p xdg-desktop-portal-ferrite
fi
for name in ferrite-shell ferrite-apps ferrite-visage xdg-desktop-portal-ferrite; do
    [ -x "$release/$name" ] || die "$release/$name absent (lancer sans --sans-compiler)"
done
"$REPO/tools/visage-modeles.sh" >/dev/null

out="$BUILD/paquets"
work="$BUILD/paquets-racines"
rm -rf "$work" "$out"
mkdir -p "$work" "$out"

# Dépendances des bibliothèques liées au binaire, calculées par Debian (dpkg-shlibdeps).
shlibs() {
    dir="$work/.shlibs"
    mkdir -p "$dir/debian"
    printf 'Source: ferrite\n\nPackage: ferrite\nArchitecture: amd64\n' >"$dir/debian/control"
    (cd "$dir" && dpkg-shlibdeps -O "$@" 2>/dev/null) | sed -n 's/^shlibs:Depends=//p'
}

# Arborescence vide du paquet $1.
root() {
    mkdir -p "$work/$1/DEBIAN" "$work/$1/usr/share/doc/$1"
    cat >"$work/$1/usr/share/doc/$1/copyright" <<'EOF'
Ferrite OS — bureau Wayland en Rust pour Debian. Licence MIT.
Polices intégrées : Inter et JetBrains Mono (SIL Open Font License 1.1).
EOF
    printf '%s\n' "$work/$1"
}

# Écrit DEBIAN/control puis fabrique build/paquets/$1_VERSION_$2.deb.
# $1 paquet, $2 architecture, $3 section, $4 champs en plus (Depends…), $5 description,
# $6 version si elle n'est pas celle de la fabrication.
build() {
    dir="$work/$1"
    pkg_version=${6:-$version}
    find "$dir" -type d -exec chmod 755 {} +
    find "$dir/usr/share" -type f -exec chmod 644 {} +
    size=$(du -sk --exclude=DEBIAN "$dir" | cut -f1)
    {
        printf 'Package: %s\nVersion: %s\nSection: %s\nPriority: optional\n' "$1" "$pkg_version" "$3"
        printf 'Architecture: %s\nMaintainer: Ferrite OS <noreply@ferrite.invalid>\n' "$2"
        printf 'Installed-Size: %s\nHomepage: %s\n' "$size" "$url"
        printf '%s\n' "$4"
        printf 'Description: %s\n' "$5"
    } | sed '/^$/d' >"$dir/DEBIAN/control"
    deb="$out/${1}_${pkg_version}_$2.deb"
    dpkg-deb --root-owner-group -Zxz --build "$dir" "$deb" >/dev/null
    ok "$(basename "$deb") ($(du -h "$deb" | cut -f1))"
}

info "Paquets $version"

# --- ferrite-shell ----------------------------------------------------------------------
dir=$(root ferrite-shell)
mkdir -p "$dir/usr/local/bin"
install -m 755 "$release/ferrite-shell" "$dir/usr/local/bin/"
for script in ferrite-session ferrite-greeter ferrite-check; do
    install -m 755 "$includes/usr/local/bin/$script" "$dir/usr/local/bin/"
done
# libEGL et libGLES sont ouvertes au démarrage (dlopen), invisibles pour dpkg-shlibdeps.
build ferrite-shell amd64 x11 \
    "Depends: $(shlibs -e"$release/ferrite-shell"), libegl1, libgles2, libgl1-mesa-dri, dbus-user-session
Recommends: greetd, xwayland, foot, fish, network-manager, pipewire, wireplumber, ferrite-apps
Suggests: ferrite-visage" \
    "bureau Ferrite (compositeur Wayland)
 Le bureau de Ferrite OS : compositeur Wayland, barre, Dock, centre de contrôle,
 écran de verrouillage et écran de connexion (greetd), avec la session qui le
 lance et le relance après un plantage."

# --- ferrite-apps -----------------------------------------------------------------------
dir=$(root ferrite-apps)
mkdir -p "$dir/usr/local/bin" "$dir/usr/share/applications"
install -m 755 "$release/ferrite-apps" "$dir/usr/local/bin/"
install -m 644 "$includes"/usr/share/applications/ferrite-*.desktop "$dir/usr/share/applications/"
# TextEdit seul (tools/deb-textedit.sh) a la même entrée de menu : l'un remplace l'autre.
build ferrite-apps amd64 x11 \
    "Depends: $(shlibs -e"$release/ferrite-apps"), libegl1, libgl1, libwayland-client0, libwayland-egl1, libxkbcommon0
Recommends: xdg-utils, flatpak, fonts-noto-color-emoji
Conflicts: ferrite-textedit
Replaces: ferrite-textedit" \
    "applications de Ferrite OS
 Fichiers, Paramètres, Logithèque, TextEdit, Musique, Vidéos, Visionneuse,
 PDF, Bienvenue et l'installateur, réunis dans le programme ferrite-apps."

# --- ferrite-visage ---------------------------------------------------------------------
dir=$(root ferrite-visage)
mkdir -p "$dir/usr/local/bin"
install -m 755 "$release/ferrite-visage" "$dir/usr/local/bin/"
build ferrite-visage amd64 x11 \
    "Depends: $(shlibs -e"$release/ferrite-visage"), ferrite-visage-modeles (>= $models_version)" \
    "déverrouillage de Ferrite par le visage
 Reconnaissance du visage par la caméra infrarouge pour l'écran de
 verrouillage."

# Modèles : à changer seulement avec les modèles (tools/visage-modeles.sh), pour qu'une
# mise à jour du bureau ne retélécharge pas leurs 38 Mo.
dir=$(root ferrite-visage-modeles)
mkdir -p "$dir/usr/share/ferrite/visage"
install -m 644 "$models"/*.onnx "$dir/usr/share/ferrite/visage/"
build ferrite-visage-modeles all x11 "" \
    "modèles du déverrouillage par le visage
 Modèles YuNet (détection) et SFace (reconnaissance) d'OpenCV Zoo, licence
 Apache 2.0, utilisés par ferrite-visage." "$models_version"

# --- xdg-desktop-portal-ferrite ---------------------------------------------------------
dir=$(root xdg-desktop-portal-ferrite)
mkdir -p "$dir/usr/local/bin" "$dir/usr/share/dbus-1/services" \
    "$dir/usr/share/xdg-desktop-portal/portals"
install -m 755 "$release/xdg-desktop-portal-ferrite" "$dir/usr/local/bin/"
install -m 644 "$includes"/usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.ferrite.service \
    "$dir/usr/share/dbus-1/services/"
install -m 644 "$includes/usr/share/xdg-desktop-portal/portals/ferrite.portal" \
    "$dir/usr/share/xdg-desktop-portal/portals/"
install -m 644 "$includes/usr/share/xdg-desktop-portal/ferrite-portals.conf" \
    "$dir/usr/share/xdg-desktop-portal/"
build xdg-desktop-portal-ferrite amd64 x11 \
    "Depends: $(shlibs -e"$release/xdg-desktop-portal-ferrite"), xdg-desktop-portal
Recommends: xdg-desktop-portal-gtk" \
    "sélecteur de fichiers de Ferrite pour les applications
 Portail xdg-desktop-portal du bureau Ferrite : les applications (Flatpak,
 navigateurs…) ouvrent et enregistrent leurs fichiers avec le sélecteur Ferrite."

# --- ferrite-desktop --------------------------------------------------------------------
dir=$(root ferrite-desktop)
same="(= $version)"
if [ -f "$cle" ]; then
    mkdir -p "$dir/etc/apt/sources.list.d" "$dir/usr/share/keyrings"
    install -m 644 "$cle" "$dir/usr/share/keyrings/ferrite-depot.pgp"
    cat >"$dir/etc/apt/sources.list.d/ferrite.sources" <<EOF
# Dépôt du bureau Ferrite : mises à jour de Ferrite par apt et la Logithèque.
Types: deb
URIs: $url
Suites: trixie
Components: main
Signed-By: /usr/share/keyrings/ferrite-depot.pgp
EOF
    chmod 644 "$dir/etc/apt/sources.list.d/ferrite.sources"
    echo /etc/apt/sources.list.d/ferrite.sources >"$dir/DEBIAN/conffiles"
else
    printf '  (pas de clé %s : ferrite-desktop sans le dépôt apt)\n' "$cle"
fi
build ferrite-desktop all metapackages \
    "Depends: ferrite-shell $same, ferrite-apps $same, xdg-desktop-portal-ferrite $same
Recommends: ferrite-visage $same" \
    "bureau Ferrite OS complet
 Installe le bureau Ferrite, ses applications et son portail, et branche
 le dépôt apt de Ferrite pour en recevoir les mises à jour."

rm -rf "$work"
ok "Paquets dans $out"
