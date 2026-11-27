#!/bin/sh
# Mesure si la veille (s2idle) atteint le sommeil profond du processeur Intel (S0ix), en
# levant un à un les blocages trouvés le 2026-11-26 : réglage actuel ; latence du port Sud B
# (Wi-Fi et lecteur de cartes) ignorée ; plus l'économie d'énergie du lien SATA ; plus la
# gestion d'énergie automatique de tous les périphériques PCI. Met le PC en veille quatre
# fois 40 secondes (réveil automatique), puis écrit le rapport dans
# build/veille-diagnostic.txt. Tout est remis comme avant, sauf la latence ignorée du port
# Sud B, qui le reste jusqu'au prochain redémarrage.
#
#   sudo tools/veille-diagnostic.sh
#
# Fermer les applications qui écrivent sur le disque avant ; ne pas toucher au clavier
# pendant les veilles (un appui réveille le PC et fausse la mesure).
# shellcheck shell=sh
set -eu

# shellcheck source=tools/lib.sh
. "$(dirname -- "$0")/lib.sh"

[ "$(id -u)" -eq 0 ] || die "lancer avec sudo : sudo tools/veille-diagnostic.sh"
command -v rtcwake >/dev/null 2>&1 || die "rtcwake absent (paquet util-linux)"
pmc=/sys/kernel/debug/pmc_core
[ -r "$pmc/slp_s0_residency_usec" ] || die "compteurs Intel absents ($pmc)"

out="$BUILD/veille-diagnostic.txt"
mkdir -p "$BUILD"
duree=40

hosts=$(ls -d /sys/class/scsi_host/host* 2>/dev/null || true)
lpm_get() {
    for h in $hosts; do cat "$h/link_power_management_policy"; done | sort -u | tr '\n' ' '
}
lpm_set() {
    for h in $hosts; do echo "$1" >"$h/link_power_management_policy" 2>/dev/null || true; done
}
# Valeur de départ de chaque contrôleur, remise à la fin (même en cas d'erreur).
sauvegarde=$(mktemp)
for h in $hosts; do echo "$h $(cat "$h/link_power_management_policy")"; done >"$sauvegarde"
lpm_restore() {
    while read -r h valeur; do
        echo "$valeur" >"$h/link_power_management_policy" 2>/dev/null || true
    done <"$sauvegarde"
}

# Gestion d'énergie à l'exécution des périphériques PCI, remise à la fin.
pci_sauvegarde=$(mktemp)
for d in /sys/bus/pci/devices/*; do echo "$d $(cat "$d/power/control")"; done >"$pci_sauvegarde"
pci_restore() {
    while read -r d valeur; do
        echo "$valeur" >"$d/power/control" 2>/dev/null || true
    done <"$pci_sauvegarde"
}

# Exigences du sommeil profond non remplies lors de la dernière veille.
bloquants() {
    awk -F'|' 'NR > 1 && $3 ~ /Required/ && $4 !~ /Yes/ { gsub(/ +/, " ", $1); printf "   %s\n", $1 }' \
        "$pmc/substate_requirements" 2>/dev/null | grep -v Int_Timer || true
}

# Une veille de $duree s ; écrit dans le rapport les secondes passées en S0ix.
veille() {
    avant=$(cat "$pmc/slp_s0_residency_usec")
    sync
    rtcwake -m freeze -s "$duree" >/dev/null 2>&1 || true
    apres=$(cat "$pmc/slp_s0_residency_usec")
    s0ix=$(((apres - avant) / 1000000))
    echo "  S0ix : $s0ix s sur $duree s (last_hw_sleep : $(cat /sys/power/suspend_stats/last_hw_sleep) µs)"
    echo "  Encore bloquant :"
    bloquants
    sleep 5
}

origine=$(lpm_get)
trap 'lpm_restore; pci_restore; rm -f "$sauvegarde" "$pci_sauvegarde"' EXIT

{
    echo "Diagnostic de la veille — $(date '+%Y-%m-%d %H:%M') — noyau $(uname -r)"
    echo "Mode de veille : $(cat /sys/power/mem_sleep)"
    echo "Profil d'énergie : $(powerprofilesctl get 2>/dev/null || echo ?)"
    echo
    echo "1. Réglages actuels (SATA : $origine)"
    veille
    # Index 1 de ltr_show : SOUTHPORT_B (ports 5 à 8 : Wi-Fi, lecteur de cartes).
    echo 1 >"$pmc/ltr_ignore"
    echo "2. Latence du port Sud B ignorée"
    veille
    lpm_set med_power_with_dipm
    echo "3. + SATA en économie ($(lpm_get))"
    veille
    for d in /sys/bus/pci/devices/*; do echo auto >"$d/power/control" 2>/dev/null || true; done
    echo "4. + gestion d'énergie automatique de tous les périphériques PCI"
    veille
    lpm_restore
    pci_restore
    echo
    echo "== Blocs qui empêchent le sommeil profond (substate_requirements) =="
    cat "$pmc/substate_requirements" 2>/dev/null || echo "(absent)"
    echo
    echo "== Tolérance de latence annoncée par chaque bloc (ltr_show) =="
    cat "$pmc/ltr_show" 2>/dev/null || echo "(absent)"
    echo
    echo "== États des paquets du processeur =="
    cat "$pmc/package_cstate_show" 2>/dev/null || echo "(absent)"
    echo
    echo "== Périphériques PCI : gestion d'énergie à l'exécution, état =="
    for d in /sys/bus/pci/devices/*; do
        printf '%s %s %s\n' "${d##*/}" "$(cat "$d/power/control" 2>/dev/null)" \
            "$(cat "$d/power_state" 2>/dev/null)"
    done
    echo
    echo "== ASPM (politique) =="
    cat /sys/module/pcie_aspm/parameters/policy
} >"$out" 2>&1

user=${SUDO_USER:-}
[ -z "$user" ] || chown "$user" "$out"
info "Rapport : $out"
grep "S0ix" "$out"
