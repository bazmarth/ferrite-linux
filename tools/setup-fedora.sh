#!/bin/sh
# Prépare Fedora pour développer Ferrite Linux (équivalent de setup-wsl.sh sous Debian).
#
#   sudo tools/setup-fedora.sh   paquets Fedora (compilation, QEMU, essais), groupe kvm
#   tools/setup-fedora.sh        Rust stable via rustup, pour l'utilisateur courant
#
# L'image live reste une Debian 13 : live-build n'existe pas dans Fedora, elle se construit
# dans un conteneur Debian (voir docs/FEDORA.md).
set -eu
. "$(dirname -- "$0")/lib.sh"

if [ "$(id -u)" -eq 0 ]; then
    info "Installation des paquets Fedora"
    dnf install -y \
        gcc gcc-c++ make pkgconf-pkg-config git curl ca-certificates \
        systemd-devel libinput-devel mesa-libgbm-devel libdrm-devel libseat-devel \
        libxkbcommon-devel wayland-devel wayland-protocols-devel \
        mesa-libEGL-devel mesa-libGLES-devel libglvnd-devel pixman-devel \
        dbus-devel pipewire-devel \
        gstreamer1-devel gstreamer1-plugins-base-devel \
        fontconfig-devel freetype-devel \
        libxkbcommon-x11 libxcb libXcursor libXrandr libXi \
        xorriso squashfs-tools mtools dosfstools \
        qemu-system-x86 qemu-img edk2-ovmf socat \
        weston foot ShellCheck podman
    user=${SUDO_USER:-}
    if [ -n "$user" ] && [ "$user" != root ]; then
        usermod -aG kvm "$user"
        ok "$user ajouté au groupe kvm (effectif après reconnexion)"
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
if ! grep -q '.cargo/bin' "$HOME/.bashrc" 2>/dev/null; then
    # shellcheck disable=SC2016 # $HOME et $PATH doivent rester littéraux dans .bashrc.
    printf '\n# Rust (rustup)\nPATH="$HOME/.cargo/bin:$PATH"\n' >>"$HOME/.bashrc"
fi
ok "$(rustc --version)"
