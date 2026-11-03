#!/bin/sh
# Compile ferrite-shell et ferrite-apps dans un Debian 13 compatible avec l'ISO QEMU.
#
#   tools/vm-build.sh
#   FERRITE_VM_DEV=1 tools/vm.sh
set -eu
. "$(dirname -- "$0")/lib.sh"

command -v podman >/dev/null 2>&1 || die "Podman absent (Fedora : sudo dnf install podman)"

podman run --rm \
    -v "$REPO:/src:rw,Z" \
    -v ferrite-cargo-cache:/root/.cargo \
    -v "$REPO/target:/src/target:rw,Z" \
    -w /src \
    docker.io/library/debian:trixie \
    sh -eu -c '
        export DEBIAN_FRONTEND=noninteractive
        apt-get update
        apt-get install -y --no-install-recommends \
            build-essential pkg-config git curl ca-certificates \
            libudev-dev libinput-dev libgbm-dev libdrm-dev libseat-dev libxkbcommon-dev \
            libwayland-dev wayland-protocols libegl-dev libgles-dev libpixman-1-dev \
            libdbus-1-dev libpipewire-0.3-dev libspa-0.2-dev \
            libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
            libfontconfig-dev libfreetype-dev \
            libxkbcommon-x11-0 libx11-xcb1 libxcursor1 libxrandr2 libxi6
        if ! command -v cargo >/dev/null 2>&1; then
            curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
            export PATH=/root/.cargo/bin:$PATH
        fi
        rustup toolchain install stable --profile minimal
        export CARGO_TARGET_DIR=/tmp/ferrite-vm-target
        cargo build --release --locked -p ferrite-shell -p ferrite-apps --bins --example faux-lecteur
        install -m 0755 /tmp/ferrite-vm-target/release/ferrite-shell /src/target/release/ferrite-shell
        install -m 0755 /tmp/ferrite-vm-target/release/ferrite-apps /src/target/release/ferrite-apps
        install -m 0755 /tmp/ferrite-vm-target/release/examples/faux-lecteur /src/target/release/faux-lecteur
    '

ok "Binaires Debian compatibles recompilés dans target/release"
