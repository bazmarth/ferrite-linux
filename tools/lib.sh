# shellcheck shell=sh
# Fonctions communes aux scripts de tools/ (à sourcer, pas à exécuter).

REPO=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
# shellcheck disable=SC2034 # utilisé par les scripts qui sourcent ce fichier
BUILD="$REPO/build"

info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok() { printf '\033[1;32m ok\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merreur :\033[0m %s\n' "$*" >&2; exit 1; }

# Trouve le micrologiciel UEFI empaqueté selon la distribution.
set_ovmf_files() {
    if [ -f /usr/share/edk2/ovmf/OVMF_CODE.fd ] && [ -f /usr/share/edk2/ovmf/OVMF_VARS.fd ]; then
        OVMF_CODE=/usr/share/edk2/ovmf/OVMF_CODE.fd
        OVMF_VARS_TEMPLATE=/usr/share/edk2/ovmf/OVMF_VARS.fd
    elif [ -f /usr/share/edk2/x64/OVMF_CODE.fd ] && [ -f /usr/share/edk2/x64/OVMF_VARS.fd ]; then
        OVMF_CODE=/usr/share/edk2/x64/OVMF_CODE.fd
        OVMF_VARS_TEMPLATE=/usr/share/edk2/x64/OVMF_VARS.fd
    elif [ -f /usr/share/OVMF/OVMF_CODE_4M.fd ] && [ -f /usr/share/OVMF/OVMF_VARS_4M.fd ]; then
        OVMF_CODE=/usr/share/OVMF/OVMF_CODE_4M.fd
        OVMF_VARS_TEMPLATE=/usr/share/OVMF/OVMF_VARS_4M.fd
    elif [ -f /usr/share/OVMF/OVMF_CODE.fd ] && [ -f /usr/share/OVMF/OVMF_VARS.fd ]; then
        OVMF_CODE=/usr/share/OVMF/OVMF_CODE.fd
        OVMF_VARS_TEMPLATE=/usr/share/OVMF/OVMF_VARS.fd
    else
        die "OVMF absent (Fedora : sudo dnf install edk2-ovmf ; Debian/WSL : sudo tools/setup-wsl.sh)"
    fi
}

# Compiler sur /mnt/d (disque Windows) est lent : les produits de compilation vont dans le
# système de fichiers Linux, sauf si CARGO_TARGET_DIR est déjà défini.
case "$REPO" in
    /mnt/*) export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/ferrite-linux/target}" ;;
esac

# Cargo installé par rustup prime sur celui de Debian (trop ancien pour certaines crates).
if [ -d "$HOME/.cargo/bin" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
fi
