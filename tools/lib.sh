# shellcheck shell=sh
# Fonctions communes aux scripts de tools/ (à sourcer, pas à exécuter).

REPO=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
# shellcheck disable=SC2034 # utilisé par les scripts qui sourcent ce fichier
BUILD="$REPO/build"

info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok() { printf '\033[1;32m ok\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merreur :\033[0m %s\n' "$*" >&2; exit 1; }

# Compiler sur /mnt/d (disque Windows) est lent : les produits de compilation vont dans le
# système de fichiers Linux, sauf si CARGO_TARGET_DIR est déjà défini.
case "$REPO" in
    /mnt/*) export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/ferrite-linux/target}" ;;
esac

# Cargo installé par rustup prime sur celui de Debian (trop ancien pour certaines crates).
if [ -d "$HOME/.cargo/bin" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
fi
