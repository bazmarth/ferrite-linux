#!/bin/sh
# Installe Ferrite (Debian 13 et le bureau Ferrite) sur une partition, pour développer
# depuis Ferrite lui-même. Lancé depuis Fedora ; le GRUB de Fedora reçoit une entrée
# « Ferrite OS ». Rien d'autre que la partition donnée n'est écrit, à part cette entrée.
#
#   tools/install-dev.sh /dev/sda5
#
# La partition est EFFACÉE. Le script refuse une partition montée, utilisée par Fedora,
# Windows (NTFS), EFI ou swap, et demande de retaper son nom avant de formater.
set -eu
. "$(dirname -- "$0")/lib.sh"

part=${1:-}
[ -n "$part" ] || die "usage : tools/install-dev.sh /dev/PARTITION"
[ -b "$part" ] || die "$part n'est pas une partition"
command -v podman >/dev/null 2>&1 || die "Podman absent (sudo dnf install podman)"
[ "$(lsblk -no TYPE "$part")" = part ] || die "$part n'est pas une partition"

# --- Garde-fous -------------------------------------------------------------------------
if findmnt -rno TARGET -S "$part" >/dev/null 2>&1; then
    die "$part est montée ($(findmnt -rno TARGET -S "$part" | head -1)) : refus"
fi
uuid_before=$(lsblk -no UUID "$part")
if [ -n "$uuid_before" ] && grep -q "$uuid_before" /etc/fstab; then
    die "$part figure dans /etc/fstab de Fedora : refus"
fi
if grep -q "^${part}[[:space:]]" /proc/swaps; then die "$part sert d'espace d'échange : refus"; fi
fstype=$(lsblk -no FSTYPE "$part")
case "$fstype" in
    "" | ext2 | ext3 | ext4 | btrfs | xfs) ;;
    *) die "$part est en $fstype (Windows, EFI, swap ou chiffré ?) : refus" ;;
esac
size=$(lsblk -no SIZE "$part" | tr -d ' ')

# Ce qu'il y a dessus, lu en lecture seule, pour que l'effacement soit fait en connaissance.
info "Partition $part ($size, $fstype)"
peek=$(mktemp -d)
system="inconnu ou vide"
if [ -n "$fstype" ] && sudo mount -o ro "$part" "$peek" 2>/dev/null; then
    if [ -r "$peek/etc/os-release" ]; then
        system=$(sed -n 's/^PRETTY_NAME=//p' "$peek/etc/os-release" | tr -d '"')
    fi
    printf '  contenu : '
    sudo ls "$peek" | head -12 | tr '\n' ' '
    echo
    sudo umount "$peek"
fi
rmdir "$peek"
printf '  système : %s\n\n' "$system"

# --- Binaires Ferrite compatibles Debian 13 --------------------------------------------
info "Compilation de Ferrite pour Debian 13"
"$REPO/tools/vm-build.sh"

printf '\033[1;31mTOUT LE CONTENU DE %s (%s, %s) VA ÊTRE EFFACÉ.\033[0m\n' "$part" "$size" "$system"
printf 'Retape « %s » pour continuer : ' "$part"
read -r answer
[ "$answer" = "$part" ] || die "abandon, rien n'a été modifié"

# --- Formatage et installation ----------------------------------------------------------
info "Formatage de $part en ext4 (FERRITE)"
sudo mkfs.ext4 -F -L FERRITE "$part"
root_uuid=$(sudo blkid -s UUID -o value "$part")
mnt=/mnt/ferrite-install
sudo mkdir -p "$mnt"
sudo mount "$part" "$mnt"
trap 'sudo umount -R "$mnt" 2>/dev/null || true' EXIT

# Le disque du projet (FICHIERS), monté aussi dans Ferrite pour y continuer le travail.
fichiers_uuid=$(findmnt -rno UUID -T "$REPO" 2>/dev/null || true)
case "$(findmnt -rno FSTYPE -T "$REPO" 2>/dev/null)" in
    ntfs* | fuseblk) ;;
    *) fichiers_uuid="" ;;
esac

info "Outils d'installation (conteneur Debian 13)"
sudo podman build --quiet --tag localhost/ferrite-live-build:trixie \
    -f "$REPO/tools/containers/live-build.Containerfile" "$REPO/tools/containers" >/dev/null

info "Installation de Debian 13 et du bureau Ferrite (15 à 30 minutes)"
sudo podman run --rm -it --privileged \
    --security-opt label=disable \
    -v "$REPO:/src:ro" \
    -v "$mnt:/target" \
    -e FERRITE_USER="$(id -un)" \
    -e FERRITE_UID="$(id -u)" \
    -e FERRITE_ROOT_UUID="$root_uuid" \
    -e FERRITE_FICHIERS_UUID="$fichiers_uuid" \
    localhost/ferrite-live-build:trixie \
    sh -eu /src/tools/install-dev/inside.sh

sudo umount -R "$mnt"
trap - EXIT

# --- Entrée dans le GRUB de Fedora ------------------------------------------------------
info "Entrée « Ferrite OS » dans le menu de démarrage de Fedora"
sudo tee /etc/grub.d/41_ferrite >/dev/null <<EOF
#!/bin/sh
# Ferrite OS, installé par tools/install-dev.sh sur $part.
cat <<'MENU'
menuentry 'Ferrite OS' --class debian --class gnu-linux {
    insmod part_gpt
    insmod ext2
    search --no-floppy --fs-uuid --set=root $root_uuid
    linux /vmlinuz root=UUID=$root_uuid ro quiet
    initrd /initrd.img
}
MENU
EOF
sudo chmod 0755 /etc/grub.d/41_ferrite
sudo grub2-mkconfig -o /boot/grub2/grub.cfg
# Fedora cache le menu quand il n'a qu'un système : il s'affichera à chaque démarrage.
sudo grub2-editenv - unset menu_auto_hide

ok "Ferrite est installé sur $part. Redémarre et choisis « Ferrite OS » dans le menu."
