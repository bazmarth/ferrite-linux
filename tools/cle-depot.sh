#!/bin/sh
# Crée la clé qui signe le dépôt apt de Ferrite (à faire une seule fois).
#
#   tools/cle-depot.sh
#
# - Clé secrète : ~/.local/share/ferrite/cle-depot (FERRITE_CLE_DEPOT pour un autre
#   dossier), HORS du dépôt git. Sans elle, plus aucune mise à jour ne peut être publiée :
#   en garder une copie (clé USB à part, gestionnaire de mots de passe).
# - Clé publique : packaging/ferrite-depot.pgp, dans le dépôt git ; ferrite-desktop
#   l'installe pour qu'apt vérifie les paquets.
set -eu
. "$(dirname -- "$0")/lib.sh"

command -v gpg >/dev/null 2>&1 || die "gpg absent (sudo apt install gnupg)"
GNUPGHOME=${FERRITE_CLE_DEPOT:-$HOME/.local/share/ferrite/cle-depot}
export GNUPGHOME
public=${FERRITE_DEPOT_CLE_PUBLIQUE:-$REPO/packaging/ferrite-depot.pgp}

if gpg --list-secret-keys 2>/dev/null | grep -q .; then
    die "une clé existe déjà dans $GNUPGHOME : rien n'est remplacé"
fi
mkdir -p "$GNUPGHOME"
chmod 700 "$GNUPGHOME"

info "Création de la clé de signature du dépôt (ed25519, sans date de fin)"
gpg --batch --quiet --passphrase '' --quick-generate-key \
    "Dépôt Ferrite OS <noreply@ferrite.invalid>" ed25519 sign never
gpg --export >"$public"
ok "Clé secrète : $GNUPGHOME (à sauvegarder, jamais dans git)"
ok "Clé publique : $public"
printf '  empreinte : %s\n' "$(gpg --with-colons --fingerprint | awk -F: '/^fpr/ { print $10; exit }')"
