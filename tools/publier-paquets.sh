#!/bin/sh
# Publie les paquets de Ferrite sur GitHub Pages : https://bazmarth.github.io/ferrite-paquets
#
#   tools/publier-paquets.sh
#
# 1. Refuse un arbre non commité (ferrite-theme/ mis à part, hors des paquets) : ce qui est
#    publié doit correspondre à un commit.
# 2. Fabrique les paquets (tools/paquets.sh) et le dépôt signé (tools/depot.sh).
# 3. Crée le dépôt GitHub public « ferrite-paquets » et active Pages, la première fois.
# 4. Remplace son contenu par un seul commit (pas d'historique : les anciens paquets ne
#    s'accumulent pas dans git). apt n'a besoin que de la dernière version.
#
# Le code source de Ferrite reste dans le dépôt privé ; seuls les paquets sont publics.
set -eu
. "$(dirname -- "$0")/lib.sh"
cd "$REPO"

compte=${FERRITE_GITHUB_COMPTE:-bazmarth}
nom=ferrite-paquets
command -v gh >/dev/null 2>&1 || die "gh absent (sudo apt install gh)"

if [ -n "$(git status --porcelain -- . ':!ferrite-theme')" ]; then
    git status --short -- . ':!ferrite-theme' | head -20
    die "des changements ne sont pas commités : commiter avant de publier"
fi
commit=$(git rev-parse --short HEAD)

"$REPO/tools/paquets.sh"
"$REPO/tools/depot.sh"
version=$(dpkg-deb -f "$BUILD"/paquets/ferrite-desktop_*.deb Version)

if ! gh repo view "$compte/$nom" >/dev/null 2>&1; then
    info "Création du dépôt GitHub public $compte/$nom"
    gh repo create "$compte/$nom" --public \
        --description "Dépôt apt du bureau Ferrite OS (paquets compilés)"
    nouveau=1
else
    nouveau=0
fi

info "Envoi de la version $version (commit $commit)"
depot="$BUILD/depot"
rm -rf "$depot/.git"
git -C "$depot" init --quiet --initial-branch=main
git -C "$depot" add --all
git -C "$depot" -c user.name="Ferrite OS" -c user.email="noreply@ferrite.invalid" \
    commit --quiet -m "Paquets $version (ferrite-linux $commit)"
git -C "$depot" push --quiet --force "https://github.com/$compte/$nom.git" main
rm -rf "$depot/.git"

if [ "$nouveau" = 1 ] || ! gh api "repos/$compte/$nom/pages" >/dev/null 2>&1; then
    info "Activation de GitHub Pages"
    gh api -X POST "repos/$compte/$nom/pages" -f "source[branch]=main" -f "source[path]=/" >/dev/null
fi
ok "Publié : https://$compte.github.io/$nom (en ligne d'ici une à deux minutes)"
