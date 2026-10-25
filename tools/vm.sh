#!/bin/sh
# Démarre une ISO dans QEMU en UEFI (OVMF), avec KVM si disponible.
#
#   tools/vm.sh [ISO]                         fenêtre QEMU (via WSLg)
#   tools/vm.sh --headless SECONDES [ISO]     sans fenêtre : console série dans
#                                             build/vm-serie.log et capture d'écran
#                                             build/vm-ecran.png après SECONDES
#
# ISO par défaut : build/ferrite-live.iso. Aucun disque de l'ordinateur n'est attaché.
set -eu
. "$(dirname -- "$0")/lib.sh"

headless=""
if [ "${1:-}" = "--headless" ]; then
    headless=${2:?durée en secondes attendue après --headless}
    shift 2
fi
iso=${1:-$BUILD/ferrite-live.iso}
[ -f "$iso" ] || die "ISO introuvable : $iso (construis-la avec « sudo tools/usb.sh »)"

code=/usr/share/OVMF/OVMF_CODE_4M.fd
vars_template=/usr/share/OVMF/OVMF_VARS_4M.fd
[ -f "$code" ] || die "OVMF absent (sudo tools/setup-wsl.sh)"
mkdir -p "$BUILD"
vars="$BUILD/vm-ovmf-vars.fd"
cp "$vars_template" "$vars"

if [ -w /dev/kvm ]; then
    accel="-accel kvm -cpu host"
else
    printf 'KVM indisponible : émulation logicielle, démarrage lent.\n' >&2
    accel="-accel tcg -cpu max"
fi

# shellcheck disable=SC2086 # $accel contient plusieurs options volontairement.
set -- qemu-system-x86_64 -machine q35 $accel -m 4G -smp 4 \
    -drive "if=pflash,format=raw,readonly=on,file=$code" \
    -drive "if=pflash,format=raw,file=$vars" \
    -drive "id=cd,if=none,media=cdrom,readonly=on,file=$iso" -device ide-cd,drive=cd,bootindex=0 \
    -device virtio-vga -device qemu-xhci -device usb-tablet \
    -nic user,model=virtio-net-pci \
    -audiodev none,id=son -device intel-hda -device hda-duplex,audiodev=son

if [ -z "$headless" ]; then
    exec "$@"
fi

log="$BUILD/vm-serie.log"
shot="$BUILD/vm-ecran.png"
# Socket hors de /mnt/d : le disque Windows ne gère pas les sockets Unix.
monitor="${TMPDIR:-/tmp}/ferrite-vm-$$.sock"
rm -f "$log" "$shot" "$monitor"
"$@" -display none -serial "file:$log" -monitor "unix:$monitor,server,nowait" &
qemu=$!
trap 'kill $qemu 2>/dev/null || true' EXIT INT TERM
sleep "$headless"
kill -0 "$qemu" 2>/dev/null || die "QEMU s'est arrêté ; voir $log"
printf 'screendump %s -f png\n' "$shot" | socat - "UNIX-CONNECT:$monitor" >/dev/null
sleep 1
printf 'quit\n' | socat - "UNIX-CONNECT:$monitor" >/dev/null || true
wait "$qemu" 2>/dev/null || true
ok "capture : $shot"
ok "console série : $log"
