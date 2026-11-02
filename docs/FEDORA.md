# Développer Ferrite Linux sous Fedora

Le bureau (`ferrite-shell`) et les applications (`ferrite-apps`) se compilent et se lancent
directement sous Fedora. Seule l'image live, qui reste une Debian 13, se construit dans un
conteneur Debian.

## 1. Récupérer le dépôt

```sh
git clone https://github.com/bazmarth/ferrite-linux.git
cd ferrite-linux
```

## 2. Installer les dépendances

```sh
sudo tools/setup-fedora.sh   # paquets Fedora, ajout au groupe kvm
tools/setup-fedora.sh        # Rust stable (rustup), rustfmt et clippy
```

Puis se déconnecter et se reconnecter (groupe `kvm`, `PATH` de Rust).

Paquets installés par le script :

| Usage | Paquets Fedora |
|---|---|
| Compilation | `gcc gcc-c++ make pkgconf-pkg-config git curl` |
| Compositeur (Smithay) | `systemd-devel libinput-devel mesa-libgbm-devel libdrm-devel libseat-devel libxkbcommon-devel wayland-devel wayland-protocols-devel mesa-libEGL-devel mesa-libGLES-devel libglvnd-devel pixman-devel` |
| Services (son, D-Bus, médias) | `dbus-devel pipewire-devel gstreamer1-devel gstreamer1-plugins-base-devel` |
| Polices | `fontconfig-devel freetype-devel` |
| Applications egui sous X11 | `libxkbcommon-x11 libxcb libXcursor libXrandr libXi` |
| Image et machine virtuelle | `xorriso squashfs-tools mtools dosfstools qemu-system-x86 qemu-img edk2-ovmf socat podman` |
| Essais et vérifications | `weston foot tmux ShellCheck` |
| Rust | `rustup` : toolchain stable avec `rustfmt` et `clippy` (voir `rust-toolchain.toml`) |

## 3. Compiler, vérifier, lancer

```sh
tools/ci.sh                        # format, lints, tests
tools/shell-dev.sh                 # ferrite-shell dans une fenêtre, avec foot
```

## Image live et QEMU : différences avec Debian

- **live-build n'existe pas dans Fedora.** `tools/usb.sh` doit tourner dans un conteneur
  Debian 13, par exemple :

  ```sh
  podman run --rm -it --privileged -v "$PWD":/src:Z debian:trixie bash
  # dans le conteneur :
  apt-get update && apt-get install -y sudo && cd /src && tools/setup-wsl.sh  # en root
  ```

- **OVMF n'est pas au même endroit.** `tools/vm.sh` et `tools/vm-test.sh` cherchent
  `/usr/share/OVMF/OVMF_CODE_4M.fd` (Debian) ; sous Fedora, le fichier est dans
  `/usr/share/edk2/ovmf/` (`OVMF_CODE.fd`, `OVMF_VARS.fd`). Ces deux scripts sont à
  adapter avant de lancer l'ISO dans QEMU.
