#!/bin/sh
# Disque dur d'essai pour l'installation en machine virtuelle : 48 Go (fichier creux),
# table GPT et une partition NTFS de 40 Go qui joue Windows, avec quelques fichiers
# (« Documents/lettre.txt »), le reste libre. Aucun droit d'administrateur. Rangé dans
# ~/.cache/ferrite-vm (FERRITE_VM_DIR) : le disque NTFS du projet refuse les fichiers creux.
#
#   tools/disque-essai.sh                 crée ~/.cache/ferrite-vm/disque-essai.img (l'écrase)
#   FERRITE_VM_DISQUE=~/.cache/ferrite-vm/disque-essai.img tools/vm-test.sh tools/scenarios/installation.txt
set -eu
. "$(dirname -- "$0")/lib.sh"

[ -x /usr/sbin/mkntfs ] || die "mkntfs absent (paquet ntfs-3g)"
dir=${FERRITE_VM_DIR:-$HOME/.cache/ferrite-vm}
disk="$dir/disque-essai.img"
ntfs="$dir/disque-essai-ntfs.img"
mkdir -p "$dir"
rm -f "$disk" "$ntfs" "$dir/vm-ovmf-vars-disque.fd"

info "Disque de 48 Go et table GPT"
truncate -s 48G "$disk"
/usr/sbin/sfdisk --quiet "$disk" <<'TABLE'
label: gpt
start=2048, size=83886080, type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name="Basic data partition"
TABLE

info "Partition NTFS « Windows » de 40 Go"
truncate -s 40G "$ntfs"
/usr/sbin/mkntfs --quiet --fast --force --label Windows \
    --sectors-per-track 63 --heads 255 --partition-start 2048 "$ntfs"
if command -v ntfs-3g >/dev/null 2>&1 && command -v fusermount >/dev/null 2>&1; then
    mnt=$(mktemp -d)
    if ntfs-3g "$ntfs" "$mnt" 2>/dev/null; then
        mkdir -p "$mnt/Documents"
        echo "Lettre de Windows : elle doit survivre à l'installation de Ferrite." >"$mnt/Documents/lettre.txt"
        fusermount -u "$mnt"
    fi
    rmdir "$mnt"
fi
dd if="$ntfs" of="$disk" bs=1M seek=1 conv=notrunc,sparse status=none
rm -f "$ntfs"
ok "Disque d'essai : $disk"
