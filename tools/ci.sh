#!/bin/sh
# Intégration continue locale : format, lints, tests, vérification des scripts.
#
#   tools/ci.sh            vérifications rapides (à lancer avant chaque commit)
#   tools/ci.sh --image    puis construction de l'image live (demande sudo, plusieurs minutes)
set -eu
. "$(dirname -- "$0")/lib.sh"
priorite_basse
cd "$REPO"

info "cargo fmt"
cargo fmt --all --check

info "cargo clippy"
cargo clippy --workspace --all-targets --locked -- -D warnings

info "cargo test"
cargo test --workspace --locked

info "couleurs"
"$REPO/tools/check-colors.sh"

if command -v shellcheck >/dev/null 2>&1; then
    info "shellcheck"
    shellcheck -x -e SC1091 tools/*.sh image/auto/config image/config/hooks/live/*.hook.chroot \
        image/config/includes.chroot/usr/local/bin/*
else
    printf 'shellcheck absent : scripts non vérifiés (sudo tools/setup-wsl.sh)\n'
fi

if [ "${1:-}" = "--image" ]; then
    info "image live"
    "$REPO/tools/usb.sh"
fi
ok "CI réussie"
