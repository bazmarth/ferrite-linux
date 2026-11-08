#!/bin/sh
# Compile ferrite-shell, ferrite-apps et ferrite-visage dans un Debian 13 compatible avec
# l'ISO et QEMU.
#
#   tools/vm-build.sh
#   FERRITE_VM_DEV=1 tools/vm.sh
#
# Tout ce qui coûte est gardé d'une fois sur l'autre : l'image de compilation
# (localhost/ferrite-build, paquets de développement et Rust déjà installés), le registre
# Cargo et les produits de compilation (volumes Podman). Seul ce qui a changé est recompilé.
set -eu
. "$(dirname -- "$0")/lib.sh"
priorite_basse

command -v podman >/dev/null 2>&1 || die "Podman absent (Fedora : sudo dnf install podman)"

# Image de compilation : refaite par Podman seulement si cette recette change.
info "Image de compilation Debian 13"
podman build --quiet --tag localhost/ferrite-build:trixie -f - "$REPO/tools" <<'CONTAINERFILE' >/dev/null
FROM docker.io/library/debian:trixie
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config git curl ca-certificates \
        libudev-dev libinput-dev libgbm-dev libdrm-dev libseat-dev libxkbcommon-dev \
        libwayland-dev wayland-protocols libegl-dev libgles-dev libpixman-1-dev \
        libdbus-1-dev libpipewire-0.3-dev libspa-0.2-dev \
        libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
        libfontconfig-dev libfreetype-dev \
        libxkbcommon-x11-0 libx11-xcb1 libxcursor1 libxrandr2 libxi6 \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --default-toolchain stable \
    && rustup component add rustfmt clippy
CONTAINERFILE

mkdir -p "$REPO/target/release"
# Pas de :Z : le dépôt peut être sur un disque NTFS, qui refuse les étiquettes SELinux.
# Les produits de compilation restent dans un volume (système de fichiers Linux, rapide),
# pas sur le disque du dépôt.
info "Compilation (incrémentale)"
podman run --rm \
    --security-opt label=disable \
    -v "$REPO:/src:rw" \
    -v ferrite-cargo-registry:/opt/cargo/registry \
    -v ferrite-vm-target:/target \
    -e CARGO_TARGET_DIR=/target \
    -w /src \
    localhost/ferrite-build:trixie \
    sh -eu -c '
        cargo build --release --locked -p ferrite-shell -p ferrite-apps -p ferrite-visage -p xdg-desktop-portal-ferrite --bins --example faux-lecteur
        install -m 0755 /target/release/ferrite-shell /src/target/release/ferrite-shell
        install -m 0755 /target/release/ferrite-apps /src/target/release/ferrite-apps
        install -m 0755 /target/release/ferrite-visage /src/target/release/ferrite-visage
        install -m 0755 /target/release/xdg-desktop-portal-ferrite /src/target/release/xdg-desktop-portal-ferrite
        install -m 0755 /target/release/examples/faux-lecteur /src/target/release/faux-lecteur
    '

ok "Binaires Debian compatibles dans target/release"
