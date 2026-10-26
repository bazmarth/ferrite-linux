#!/bin/sh
# Démarre une ISO dans QEMU en UEFI (OVMF), avec KVM si disponible.
#
#   tools/vm.sh [ISO]                         fenêtre QEMU (via WSLg)
#   tools/vm.sh --headless SECONDES [ISO]     sans fenêtre : console série dans
#                                             build/vm-serie.log et capture d'écran
#                                             build/vm-ecran.png après SECONDES
#   FERRITE_VM_CLOSE_AFTER=SECONDES tools/vm.sh --headless DÉLAI [ISO]
#                                             envoie Ctrl+Alt+Retour arrière avant la capture
#
# Réglages facultatifs (utilisés par l'outil graphique tools/vm-gui.sh) :
#   FERRITE_VM_MEMORY=Go (4)  FERRITE_VM_CPUS=N (4)  FERRITE_VM_NETWORK=0 (sans réseau)
#   FERRITE_VM_SOUND=1 (son vers WSLg)  FERRITE_VM_MONITOR=SOCKET (moniteur QEMU)
#   FERRITE_VM_DEV=1 (binaires de target/release au lieu de ceux de l'ISO : pas besoin
#                     de reconstruire l'ISO après une modification du bureau)
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

memory=${FERRITE_VM_MEMORY:-4}
cpus=${FERRITE_VM_CPUS:-4}
network="-nic user,model=virtio-net-pci"
[ "${FERRITE_VM_NETWORK:-1}" = 0 ] && network="-nic none"
sound="none"
[ "${FERRITE_VM_SOUND:-0}" = 1 ] && sound="pa"

# shellcheck disable=SC2086 # $accel et $network contiennent plusieurs options volontairement.
set -- qemu-system-x86_64 -machine q35 $accel -m "${memory}G" -smp "$cpus" \
    -name "Ferrite OS" \
    -drive "if=pflash,format=raw,readonly=on,file=$code" \
    -drive "if=pflash,format=raw,file=$vars" \
    -drive "id=cd,if=none,media=cdrom,readonly=on,file=$iso" -device ide-cd,drive=cd,bootindex=0 \
    -device virtio-vga -device qemu-xhci -device usb-tablet \
    $network \
    -audiodev "$sound,id=son" -device intel-hda -device hda-duplex,audiodev=son
# Mode développement : les binaires compilés (cargo build --release) sont partagés avec
# la machine virtuelle, qui les utilise à la place de ceux de l'ISO.
if [ "${FERRITE_VM_DEV:-0}" = 1 ]; then
    release="${CARGO_TARGET_DIR:-$REPO/target}/release"
    [ -x "$release/ferrite-shell" ] || die "binaires absents : cargo build --release -p ferrite-shell -p ferrite-apps"
    set -- "$@" -virtfs "local,path=$release,mount_tag=ferrite-dev,security_model=none,readonly=on"
fi
if [ -n "${FERRITE_VM_MONITOR:-}" ]; then
    set -- "$@" -monitor "unix:$FERRITE_VM_MONITOR,server,nowait"
fi

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
sleep 3
# Dans le mode sans écran, GRUB ne reçoit pas toujours son propre délai d'attente
# virtuel. Confirmer l'entrée live rend le test reproductible comme un appui Entrée.
printf 'sendkey ret\n' | socat - "UNIX-CONNECT:$monitor" >/dev/null
# Test facultatif du chemin de fermeture du compositeur ; QEMU reste en marche
# afin de capturer le retour à la console après l'arrêt demandé.
if [ -n "${FERRITE_VM_CLOSE_AFTER:-}" ]; then
    sleep "$FERRITE_VM_CLOSE_AFTER"
    printf 'sendkey ctrl-alt-backspace\n' | socat - "UNIX-CONNECT:$monitor" >/dev/null
    sleep 2
else
    sleep "$headless"
fi
kill -0 "$qemu" 2>/dev/null || die "QEMU s'est arrêté ; voir $log"
printf 'screendump %s -f png\n' "$shot" | socat - "UNIX-CONNECT:$monitor" >/dev/null
sleep 1
printf 'quit\n' | socat - "UNIX-CONNECT:$monitor" >/dev/null || true
wait "$qemu" 2>/dev/null || true
ok "capture : $shot"
ok "console série : $log"
