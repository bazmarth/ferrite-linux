#!/bin/sh
# Rejoue un scénario de clics dans l'ISO sous QEMU, sans fenêtre, et capture l'écran.
#
#   tools/vm-test.sh tools/scenarios/stabilite.txt [ISO]
#
# Une ligne par étape : « sleep SECONDES », « click X Y » (pixels de l'écran 1280×800),
# « key TOUCHES » (syntaxe sendkey de QEMU, ex. ctrl-alt-backspace), « type TEXTE »
# (frappe au clavier QWERTY américain, disposition par défaut de Ferrite), « serve FICHIER »
# (le fichier est servi à la machine virtuelle sur 10.0.2.2:8765, à lire par exemple avec
# « bash -c "$(cat </dev/tcp/10.0.2.2/8765)" ») ou « shot NOM » (capture dans build/NOM.png). Les lignes vides et celles commençant par # sont ignorées.
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
server=""
trap 'kill $qemu $server 2>/dev/null || true; rm -f "$monitor" "$qmp"' EXIT INT TERM
sleep 3

hmp() { printf '%s\n' "$*" | socat - "UNIX-CONNECT:$monitor" >/dev/null; }
input() {
    printf '{"execute":"qmp_capabilities"}\n{"execute":"input-send-event","arguments":{"events":[%s]}}\n' "$1" |
        socat -t1 - "UNIX-CONNECT:$qmp" >/dev/null
}
# Envoie des touches par QMP, une combinaison par argument (ex. « shift-a ret »), avec
# un appui bref pour ne pas déclencher la répétition automatique.
keys() {
    {
        printf '{"execute":"qmp_capabilities"}\n'
        for combo in "$@"; do
            list=""
            rest_combo=$combo
            while [ -n "$rest_combo" ]; do
                part=${rest_combo%%-*}
                if [ "$part" = "$rest_combo" ]; then rest_combo=""; else rest_combo=${rest_combo#*-}; fi
                list="$list${list:+,}{\"type\":\"qcode\",\"data\":\"$part\"}"
            done
            printf '{"execute":"send-key","arguments":{"keys":[%s],"hold-time":30}}\n' "$list"
        done
    } | socat -t 2 - "UNIX-CONNECT:$qmp" >/dev/null
    # Laisse QEMU rejouer les touches (environ 60 ms chacune) avant l'étape suivante.
    sleep "$(($# / 12 + 1))"
}

# Nom QMP (qcode) d'un caractère (clavier QWERTY américain), « shift-x » pour les majuscules.
keyname() {
    case "$1" in
        [a-z0-9]) printf '%s' "$1" ;;
        [A-Z]) printf 'shift-%s' "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" ;;
        ' ') printf spc ;;
        -) printf minus ;;
        _) printf shift-minus ;;
        .) printf dot ;;
        ,) printf comma ;;
        /) printf slash ;;
        :) printf shift-semicolon ;;
        ';') printf semicolon ;;
        =) printf equal ;;
        +) printf shift-equal ;;
        '"') printf shift-apostrophe ;;
        "'") printf apostrophe ;;
        '|') printf shift-backslash ;;
        '&') printf shift-7 ;;
        '>') printf shift-dot ;;
        '<') printf shift-comma ;;
        '[') printf bracket_left ;;
        ']') printf bracket_right ;;
        '~') printf shift-grave_accent ;;
        '*') printf shift-8 ;;
        '$') printf shift-4 ;;
        '#') printf shift-3 ;;
        @) printf shift-2 ;;
        '(') printf shift-9 ;;
        ')') printf shift-0 ;;
        '!') printf shift-1 ;;
        %) printf shift-5 ;;
        *) die "caractère non pris en charge par « type » : $1" ;;
    esac
}

# Entrée live de GRUB.
hmp sendkey ret

while read -r command a b; do
    rest="$a${b:+ $b}"
    case "$command" in
        '' | '#'*) ;;
        sleep) sleep "$a" ;;
        serve)
            [ -f "$a" ] || die "fichier à servir introuvable : $a"
            socat TCP-LISTEN:8765,bind=127.0.0.1,reuseaddr,fork "SYSTEM:cat $a" &
            server=$!
            ;;
        key) keys "$a" ;;
        type)
            # Toute la frappe passe par une seule connexion QMP : une connexion par touche
            # perdait des caractères.
            text=$rest
            names=""
            while [ -n "$text" ]; do
                character=${text%"${text#?}"}
                text=${text#?}
                names="$names $(keyname "$character")"
            done
            # shellcheck disable=SC2086 # une touche (ou combinaison) par mot.
            keys $names
            ;;
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
