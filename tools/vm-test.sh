#!/bin/sh
# Rejoue un scénario de clics dans l'ISO sous QEMU, sans fenêtre, et capture l'écran.
#
#   tools/vm-test.sh tools/scenarios/stabilite.txt [ISO]
#   FERRITE_VM_DEV=1 tools/vm-test.sh …   avec les binaires de target/release
#   FERRITE_VM_DISQUE=~/.cache/ferrite-vm/disque-essai.img tools/vm-test.sh …   avec un disque dur
#       (tools/disque-essai.sh), gardé d'un essai à l'autre comme la mémoire UEFI
#       (vm-ovmf-vars-disque.fd à côté du disque : l'entrée de démarrage créée par l'installation)
#   FERRITE_VM_SANS_CD=1 (avec FERRITE_VM_DISQUE) : démarre sur le disque, sans la clé
#
# Une ligne par étape : « sleep SECONDES », « click X Y » et « rclick X Y » (pixels de l'écran 1280×800),
# « move X Y » (déplacer le pointeur), « serial LIGNE » (tapée sur la console série, sortie dans build/vm-serie.log),
# « drag X1 Y1 X2 Y2 » (glisser avec le bouton gauche), « key TOUCHES » (syntaxe sendkey de QEMU, ex. ctrl-alt-backspace), « keydown TOUCHE » et « keyup TOUCHE » (touche maintenue), « type TEXTE »
# (frappe au clavier QWERTY américain, disposition par défaut de Ferrite), « serve FICHIER »
# (le fichier est servi à la machine virtuelle sur 10.0.2.2:8765, à lire par exemple avec
# « bash -c "$(cat </dev/tcp/10.0.2.2/8765)" ») ou « shot NOM » (capture dans build/NOM.png). Les lignes vides et celles commençant par # sont ignorées.
set -eu
. "$(dirname -- "$0")/lib.sh"

scenario=${1:?scénario attendu (voir tools/scenarios)}
iso=${2:-$BUILD/ferrite-live.iso}
[ -f "$scenario" ] || die "scénario introuvable : $scenario"
[ -f "$iso" ] || [ "${FERRITE_VM_SANS_CD:-0}" = 1 ] || die "ISO introuvable : $iso (construis-la avec tools/usb.sh)"
command -v socat >/dev/null 2>&1 || die "socat absent (sudo apt install socat)"

set_ovmf_files
code=$OVMF_CODE
vars="$BUILD/vm-ovmf-vars.fd"
disk=${FERRITE_VM_DISQUE:-}
if [ -n "$disk" ]; then
    [ -f "$disk" ] || die "disque introuvable : $disk (tools/disque-essai.sh)"
    vars="$(dirname -- "$disk")/vm-ovmf-vars-disque.fd"
    [ -f "$vars" ] || cp "$OVMF_VARS_TEMPLATE" "$vars"
else
    cp "$OVMF_VARS_TEMPLATE" "$vars"
fi
if [ -w /dev/kvm ]; then
    accel="-accel kvm -cpu host"
else
    accel="-accel tcg -cpu max"
fi
monitor="${TMPDIR:-/tmp}/ferrite-test-$$.sock"
qmp="${TMPDIR:-/tmp}/ferrite-test-$$.qmp"
serial="${TMPDIR:-/tmp}/ferrite-test-$$.serie"
rm -f "$monitor" "$qmp" "$serial" "$BUILD/vm-serie.log"

set --
if [ -n "$disk" ]; then
    set -- -drive "id=hd,if=none,format=raw,file=$disk" -device virtio-blk-pci,drive=hd,bootindex=1
fi
cd_drive="-drive id=cd,if=none,media=cdrom,readonly=on,file=$iso -device ide-cd,drive=cd,bootindex=0"
[ "${FERRITE_VM_SANS_CD:-0}" = 1 ] && cd_drive=""
# Mode développement : les binaires compilés (cargo build --release) sont partagés avec
# la machine virtuelle, qui les utilise à la place de ceux de l'ISO.
if [ "${FERRITE_VM_DEV:-0}" = 1 ]; then
    release="${CARGO_TARGET_DIR:-$REPO/target}/release"
    [ -x "$release/ferrite-shell" ] || die "binaires absents : cargo build --release -p ferrite-shell -p ferrite-apps"
    set -- "$@" -virtfs "local,path=$release,mount_tag=ferrite-dev,security_model=none,readonly=on"
fi
# shellcheck disable=SC2086 # $accel et $cd_drive contiennent plusieurs options volontairement.
qemu-system-x86_64 "$@" -machine q35 $accel -m 4G -smp 4 \
    -drive "if=pflash,format=raw,readonly=on,file=$code" -drive "if=pflash,format=raw,file=$vars" \
    $cd_drive \
    -device virtio-vga -device qemu-xhci -device usb-tablet -nic user,model=virtio-net-pci \
    -display none \
    -chardev "socket,id=serie,path=$serial,server=on,wait=off,logfile=$BUILD/vm-serie.log" \
    -serial chardev:serie \
    -monitor "unix:$monitor,server,nowait" -qmp "unix:$qmp,server,nowait" &
qemu=$!
server=""
trap 'kill $qemu $server 2>/dev/null || true; rm -f "$monitor" "$qmp" "$serial"' EXIT INT TERM
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

# Place le pointeur de la tablette USB en (X, Y), en pixels de l'écran 1280×800.
move_abs() {
    input "{\"type\":\"abs\",\"data\":{\"axis\":\"x\",\"value\":$(($1 * 32767 / 1280))}},{\"type\":\"abs\",\"data\":{\"axis\":\"y\",\"value\":$(($2 * 32767 / 800))}}"
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
        serial)
            # Ligne tapée sur la console série (ttyS0) ; la sortie va dans vm-serie.log.
            printf '%s\r' "$rest" | socat -t1 - "UNIX-CONNECT:$serial" >/dev/null
            sleep 1
            ;;
        serve)
            [ -f "$a" ] || die "fichier à servir introuvable : $a"
            socat TCP-LISTEN:8765,bind=127.0.0.1,reuseaddr,fork "SYSTEM:cat $a" &
            server=$!
            ;;
        key) keys "$a" ;;
        keydown | keyup)
            # Touche maintenue : « keydown alt », puis « key tab », puis « keyup alt ».
            [ "$command" = keydown ] && down=true || down=false
            input "{\"type\":\"key\",\"data\":{\"down\":$down,\"key\":{\"type\":\"qcode\",\"data\":\"$a\"}}}"
            ;;
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
        move)
            # move X Y : déplace le pointeur sans cliquer.
            move_abs "$a" "$b"
            ;;
        drag)
            # drag X1 Y1 X2 Y2 : appui en (X1, Y1), glissement en 20 pas, relâchement.
            # shellcheck disable=SC2086 # coordonnées séparées volontairement.
            set -- $rest
            move_abs "$1" "$2"
            sleep 0.3
            input '{"type":"btn","data":{"down":true,"button":"left"}}'
            step=1
            while [ "$step" -le 20 ]; do
                move_abs $(($1 + ($3 - $1) * step / 20)) $(($2 + ($4 - $2) * step / 20))
                sleep 0.05
                step=$((step + 1))
            done
            input '{"type":"btn","data":{"down":false,"button":"left"}}'
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
        rclick)
            # Clic droit en (X, Y), en pixels de l'écran 1280×800.
            move_abs "$a" "$b"
            sleep 0.5
            input '{"type":"btn","data":{"down":true,"button":"right"}}'
            sleep 0.15
            input '{"type":"btn","data":{"down":false,"button":"right"}}'
            ;;
        *) die "étape inconnue : $command" ;;
    esac
    kill -0 "$qemu" 2>/dev/null || die "QEMU s'est arrêté ; voir $BUILD/vm-serie.log"
done <"$scenario"
hmp quit || true
