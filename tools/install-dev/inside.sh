#!/bin/sh
# Partie de tools/install-dev.sh qui tourne dans le conteneur Debian 13 : installe Debian et
# le bureau Ferrite dans /target (la partition montée), puis le configure.
#
# Variables : FERRITE_USER, FERRITE_UID, FERRITE_ROOT_UUID (la partition cible),
# FERRITE_FICHIERS_UUID (partition du projet, à
# monter dans /media/FICHIERS ; vide : rien). Le dépôt est en lecture seule dans /src.
set -eu

target=/target
user=${FERRITE_USER:?}
uid=${FERRITE_UID:?}
say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
in_target() { chroot "$target" /usr/bin/env DEBIAN_FRONTEND=noninteractive LC_ALL=C "$@"; }

if [ ! -x "$target/usr/bin/apt-get" ]; then
    say "Debian 13 de base (debootstrap, quelques minutes)"
    debootstrap --arch amd64 --components=main,contrib,non-free,non-free-firmware \
        trixie "$target" http://deb.debian.org/debian
fi

cat >"$target/etc/apt/sources.list" <<'EOF'
deb http://deb.debian.org/debian trixie main contrib non-free non-free-firmware
deb http://deb.debian.org/debian trixie-updates main contrib non-free non-free-firmware
deb http://security.debian.org/debian-security trixie-security main contrib non-free non-free-firmware
EOF

# Systèmes de fichiers du noyau et réseau pour le chroot ; démontés à la sortie.
for dir in dev dev/pts proc sys run; do
    mkdir -p "$target/$dir"
done
mount --bind /dev "$target/dev"
mount --bind /dev/pts "$target/dev/pts"
mount -t proc proc "$target/proc"
mount -t sysfs sys "$target/sys"
mount -t tmpfs tmpfs "$target/run"
cleanup() {
    for dir in run sys proc dev/pts dev; do
        umount -l "$target/$dir" 2>/dev/null || true
    done
}
trap cleanup EXIT
cp /etc/resolv.conf "$target/etc/resolv.conf"

# Les liens /vmlinuz et /initrd.img suivent chaque mise à jour du noyau : l'entrée du GRUB
# de Fedora pointe sur eux.
printf 'do_symlinks = yes\nlink_in_boot = no\n' >"$target/etc/kernel-img.conf"

say "Paquets de l'image Ferrite et outils de développement"
# Ceux de l'image, sans le système live (live-boot démarrerait sur une clé, pas un disque).
packages=$(sed -e 's/#.*//' /src/image/config/package-lists/*.list.chroot \
    | tr -s ' \t' '\n' \
    | grep -v -e '^$' -e '^live-' -e '^user-setup$')
# shellcheck disable=SC2086 # une liste de paquets, à découper.
in_target apt-get update
# shellcheck disable=SC2086
in_target apt-get install -y --no-install-recommends $packages \
    initramfs-tools fish sudo git curl ca-certificates build-essential pkg-config \
    bash-completion man-db ntfs-3g \
    libudev-dev libinput-dev libgbm-dev libdrm-dev libseat-dev libxkbcommon-dev \
    libwayland-dev wayland-protocols libegl-dev libgles-dev libpixman-1-dev \
    libdbus-1-dev libpipewire-0.3-dev libspa-0.2-dev \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    libfontconfig-dev libfreetype-dev \
    libxkbcommon-x11-0 libx11-xcb1 libxcursor1 libxrandr2 libxi6

say "Langue, clavier, fuseau horaire, nom de la machine"
echo 'fr_FR.UTF-8 UTF-8' >"$target/etc/locale.gen"
in_target locale-gen
echo 'LANG=fr_FR.UTF-8' >"$target/etc/default/locale"
cat >"$target/etc/default/keyboard" <<'EOF'
XKBMODEL="pc105"
XKBLAYOUT="fr"
XKBVARIANT=""
XKBOPTIONS=""
BACKSPACE="guess"
EOF
ln -sf /usr/share/zoneinfo/Africa/Ouagadougou "$target/etc/localtime"
echo 'Africa/Ouagadougou' >"$target/etc/timezone"
echo ferrite >"$target/etc/hostname"
cat >"$target/etc/hosts" <<'EOF'
127.0.0.1	localhost
127.0.1.1	ferrite
::1	localhost ip6-localhost ip6-loopback
EOF

say "Montages (/etc/fstab)"
root_uuid=${FERRITE_ROOT_UUID:?}
{
    echo "UUID=$root_uuid / ext4 errors=remount-ro 0 1"
    if [ -n "${FERRITE_FICHIERS_UUID:-}" ]; then
        # Le disque du projet, partagé avec Fedora et Windows ; nofail : le démarrage
        # continue s'il est absent ou mal démonté par Windows.
        echo "UUID=$FERRITE_FICHIERS_UUID /media/FICHIERS ntfs3 uid=$uid,gid=$uid,nofail,x-systemd.device-timeout=5 0 0"
    fi
} >"$target/etc/fstab"
mkdir -p "$target/media/FICHIERS"

say "Fichiers du bureau Ferrite"
# Pas de `cp -a` : le dépôt est souvent sur le disque NTFS, où tout appartient à
# l'utilisateur avec les droits 777 ; `cp -a` les recopiait sur /, /etc et /usr du système.
# Ici, les fichiers appartiennent à root (644, dossiers 755) et les dossiers déjà là
# gardent leurs droits ; seuls les scripts de usr/local/bin sont exécutables.
cp -r --no-preserve=mode,ownership /src/image/config/includes.chroot/. "$target/"
chmod 0755 "$target"/usr/local/bin/ferrite-*
install -m 0755 /src/target/release/ferrite-shell "$target/usr/local/bin/ferrite-shell"
install -m 0755 /src/target/release/ferrite-apps "$target/usr/local/bin/ferrite-apps"
install -m 0755 /src/target/release/ferrite-visage "$target/usr/local/bin/ferrite-visage"
install -m 0755 /src/target/release/xdg-desktop-portal-ferrite "$target/usr/local/bin/xdg-desktop-portal-ferrite"
install -d "$target/usr/share/ferrite/visage"
install -m 0644 /src/assets/visage/*.onnx "$target/usr/share/ferrite/visage/"
# Écran de connexion au démarrage : pas d'ouverture automatique comme sur le système live
# (bloc [initial_session], le dernier du fichier).
sed -i '/^# Système live/,$d' "$target/etc/greetd/config.toml"
rm -f "$target/etc/systemd/system/getty@tty1.service.d/autologin.conf"
for hook in /src/image/config/hooks/live/*.hook.chroot; do
    cp "$hook" "$target/tmp/hook"
    in_target sh /tmp/hook
done
rm -f "$target/tmp/hook"

say "Utilisateur $user"
if ! in_target id "$user" >/dev/null 2>&1; then
    in_target useradd --create-home --uid "$uid" --shell /bin/bash "$user"
fi
for group in sudo audio video render input netdev plugdev bluetooth lpadmin; do
    if in_target getent group "$group" >/dev/null; then
        in_target usermod -aG "$group" "$user"
    fi
done
echo "Mot de passe de $user dans Ferrite (pour sudo) :"
until chroot "$target" passwd "$user"; do
    echo "Encore une fois."
done

say "Rust pour $user (rustup)"
in_target su - "$user" -c \
    'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal -c rustfmt,clippy'

say "Noyau et démarrage"
in_target update-initramfs -u -k all
# Les liens sont normalement posés par le paquet du noyau (kernel-img.conf) ; sinon, ici.
version=$(find "$target/boot" -name 'vmlinuz-*' -printf '%f\n' | sed 's/^vmlinuz-//' | sort -V | tail -1)
[ -n "$version" ] || { echo "aucun noyau installé" >&2; exit 1; }
[ -e "$target/vmlinuz" ] || ln -sf "boot/vmlinuz-$version" "$target/vmlinuz"
[ -e "$target/initrd.img" ] || ln -sf "boot/initrd.img-$version" "$target/initrd.img"
rm -f "$target/etc/resolv.conf"
say "Ferrite installé"
