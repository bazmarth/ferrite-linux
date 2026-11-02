#!/bin/sh
# Prépare Debian 13 (WSL2) pour développer Ferrite Linux.
#
#   sudo tools/setup-wsl.sh   paquets Debian (compilation, image live, QEMU), groupe kvm
#   tools/setup-wsl.sh        Rust stable via rustup, pour l'utilisateur courant
set -eu
. "$(dirname -- "$0")/lib.sh"

if [ "$(id -u)" -eq 0 ]; then
    info "Installation des paquets Debian"
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends \
        build-essential pkg-config git curl ca-certificates \
        libudev-dev libinput-dev libgbm-dev libdrm-dev libseat-dev libxkbcommon-dev \
        libwayland-dev wayland-protocols libegl-dev libgles-dev libpixman-1-dev \
        libdbus-1-dev libpipewire-0.3-dev libspa-0.2-dev \
        libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
        libfontconfig-dev libfreetype-dev \
        libxkbcommon-x11-0 libx11-xcb1 libxcursor1 libxrandr2 libxi6 \
        live-build debootstrap xorriso squashfs-tools mtools dosfstools \
        grub-pc-bin grub-efi-amd64-bin \
        qemu-system-x86 qemu-utils ovmf socat \
        weston foot tmux shellcheck
    user=${SUDO_USER:-}
    if [ -n "$user" ] && [ "$user" != root ]; then
        usermod -aG kvm "$user"
        ok "$user ajouté au groupe kvm (effectif après « wsl --shutdown »)"
    fi
    ok "paquets installés ; relance ce script sans sudo pour installer Rust"
    exit 0
fi

if ! command -v rustup >/dev/null 2>&1; then
    info "Installation de rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path
    PATH="$HOME/.cargo/bin:$PATH"
fi
info "Mise à jour de Rust stable"
rustup toolchain install stable --profile minimal --component rustfmt,clippy
rustup default stable
if ! grep -q '.cargo/bin' "$HOME/.profile" 2>/dev/null; then
    # shellcheck disable=SC2016 # $HOME et $PATH doivent rester littéraux dans .profile.
    printf '\n# Rust (rustup)\nPATH="$HOME/.cargo/bin:$PATH"\n' >>"$HOME/.profile"
fi
ok "$(rustc --version)"
