#!/bin/sh
# Dépôt apt de Ferrite, signé : build/depot, prêt à être servi tel quel (GitHub Pages).
#
#   tools/depot.sh   range build/paquets/*.deb (tools/paquets.sh) et signe les index
#
# Structure : pool/main/*.deb, dists/trixie/main/binary-amd64/Packages(.gz), puis
# dists/trixie/Release, InRelease et Release.gpg, signés par la clé de tools/cle-depot.sh
# (FERRITE_CLE_DEPOT pour un autre dossier). Seuls les derniers paquets y sont : apt n'a
# besoin que de la version à installer.
#
# Source à ajouter sur un ordinateur (ferrite-desktop le fait) :
#   /etc/apt/sources.list.d/ferrite.sources, Suites: trixie, Components: main
set -eu
. "$(dirname -- "$0")/lib.sh"

command -v apt-ftparchive >/dev/null 2>&1 || die "apt-ftparchive absent (sudo apt install apt-utils)"
command -v gpg >/dev/null 2>&1 || die "gpg absent (sudo apt install gnupg)"
GNUPGHOME=${FERRITE_CLE_DEPOT:-$HOME/.local/share/ferrite/cle-depot}
export GNUPGHOME
gpg --list-secret-keys 2>/dev/null | grep -q . || die "pas de clé dans $GNUPGHOME (tools/cle-depot.sh)"

paquets="$BUILD/paquets"
depot=${FERRITE_DEPOT_DOSSIER:-$BUILD/depot}
ls "$paquets"/*.deb >/dev/null 2>&1 || die "aucun paquet dans $paquets (tools/paquets.sh)"

info "Dépôt dans $depot"
rm -rf "$depot"
mkdir -p "$depot/pool/main" "$depot/dists/trixie/main/binary-amd64"
cp "$paquets"/*.deb "$depot/pool/main/"
cd "$depot"
apt-ftparchive packages pool >dists/trixie/main/binary-amd64/Packages
gzip -9 -k -n dists/trixie/main/binary-amd64/Packages
apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=Ferrite \
    -o APT::FTPArchive::Release::Label="Ferrite OS" \
    -o APT::FTPArchive::Release::Suite=trixie \
    -o APT::FTPArchive::Release::Codename=trixie \
    -o APT::FTPArchive::Release::Architectures="amd64 all" \
    -o APT::FTPArchive::Release::Components=main \
    -o APT::FTPArchive::Release::Description="Bureau Ferrite OS pour Debian 13" \
    release dists/trixie >Release.nouveau
mv Release.nouveau dists/trixie/Release
gpg --batch --yes --clearsign -o dists/trixie/InRelease dists/trixie/Release
gpg --batch --yes -abs -o dists/trixie/Release.gpg dists/trixie/Release

# GitHub Pages : servir les fichiers tels quels (sans Jekyll), et une page d'accueil.
touch .nojekyll
{
    printf '<!doctype html><meta charset="utf-8"><title>Dépôt Ferrite OS</title>\n'
    printf '<h1>Dépôt apt de Ferrite OS</h1>\n'
    printf '<p>Mises à jour du bureau Ferrite pour Debian 13. Sur Ferrite, rien à faire :\n'
    printf 'le paquet <code>ferrite-desktop</code> branche ce dépôt.</p>\n<ul>\n'
    for deb in pool/main/*.deb; do
        printf '<li><a href="%s">%s</a></li>\n' "$deb" "$(basename "$deb")"
    done
    printf '</ul>\n'
} >index.html
ok "$(find pool -name '*.deb' | wc -l) paquets, index signés ($(du -sh . | cut -f1))"
