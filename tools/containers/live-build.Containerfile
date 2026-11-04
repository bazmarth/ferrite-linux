# Outils de construction de l'image et d'installation (live-build, debootstrap), sous
# Debian 13. Utilisé par tools/usb-fedora.sh et tools/install-dev.sh.
FROM docker.io/library/debian:trixie
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        live-build debootstrap xorriso squashfs-tools mtools dosfstools zstd \
        grub-pc-bin grub-efi-amd64-bin ca-certificates \
    && rm -rf /var/lib/apt/lists/*
