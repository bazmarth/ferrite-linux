#!/bin/sh
# Rejoue un scénario de clics dans l'ISO sous QEMU, sans fenêtre, et capture l'écran.
#
#   tools/vm-test.sh tools/scenarios/stabilite.txt [ISO]
#
# Une ligne par étape : « sleep SECONDES », « click X Y » (pixels de l'écran 1280×800),
# « key TOUCHES » (syntaxe sendkey de QEMU, ex. ctrl-alt-backspace) ou « shot NOM »
# (capture dans build/NOM.png). Les lignes vides et celles commençant par # sont ignorées.
set -eu
. "$(dirname -- "$0")/lib.sh"

scenario=${1:?scénario attendu (voir tools/scenarios)}
iso=${2:-$BUILD/ferrite-live.iso}
[ -f "$scenario" ] || die "scénario introuvable : $scenario"
[ -f "$iso" ] || die "ISO introuvable : $iso (construis-la avec tools/usb.sh)"
command -v socat >/dev/null 2>&1 || die "socat absent (sudo apt install socat)"

code=/usr/share/OVMF/OVMF_CODE_4M.fd
vars="$BUILD/vm-ovmf-vars.fd"
cp /usr/share/OVMF/OVMF_VARS_4M.fd "$vars"
if [ -w /dev/kvm ]; then
    accel="-accel kvm -cpu host"
else
    accel="-accel tcg -cpu max"
fi
monitor="${TMPDIR:-/tmp}/ferrite-test-$$.sock"
qmp="${TMPDIR:-/tmp}/ferrite-test-$$.qmp"
rm -f "$monitor" "$qmp"

# shellcheck disable=SC2086 # $accel contient plusieurs options volontairement.
qemu-system-x86_64 -machine q35 $accel -m 4G -smp 4 \
    -drive "if=pflash,format=raw,readonly=on,file=$code" -drive "if=pflash,format=raw,file=$vars" \
    -drive "id=cd,if=none,media=cdrom,readonly=on,file=$iso" -device ide-cd,drive=cd,bootindex=0 \
    -device virtio-vga -device qemu-xhci -device usb-tablet -nic user,model=virtio-net-pci \
    -display none -serial "file:$BUILD/vm-serie.log" \
    -monitor "unix:$monitor,server,nowait" -qmp "unix:$qmp,server,nowait" &
qemu=$!
trap 'kill $qemu 2>/dev/null || true; rm -f "$monitor" "$qmp"' EXIT INT TERM
sleep 3

hmp() { printf '%s\n' "$*" | socat - "UNIX-CONNECT:$monitor" >/dev/null; }
input() {
    printf '{"execute":"qmp_capabilities"}\n{"execute":"input-send-event","arguments":{"events":[%s]}}\n' "$1" |
        socat -t1 - "UNIX-CONNECT:$qmp" >/dev/null
}
# Entrée live de GRUB.
hmp sendkey ret

while read -r command a b; do
    case "$command" in
        '' | '#'*) ;;
        sleep) sleep "$a" ;;
        key) hmp sendkey "$a" ;;
        shot)
            hmp screendump "$BUILD/$a.png" -f png
            sleep 1
            ok "capture : $BUILD/$a.png"
            ;;
        click)
            # Coordonnées absolues de la tablette USB : de 0 à 32767 sur chaque axe.
            x=$((a * 32767 / 1280))
            y=$((b * 32767 / 800))
            input "{\"type\":\"abs\",\"data\":{\"axis\":\"x\",\"value\":$x}},{\"type\":\"abs\",\"data\":{\"axis\":\"y\",\"value\":$y}}"
            sleep 0.3
            input '{"type":"btn","data":{"down":true,"button":"left"}}'
            sleep 0.15
            input '{"type":"btn","data":{"down":false,"button":"left"}}'
            ;;
        *) die "étape inconnue : $command" ;;
    esac
    kill -0 "$qemu" 2>/dev/null || die "QEMU s'est arrêté ; voir $BUILD/vm-serie.log"
done <"$scenario"
hmp quit || true
