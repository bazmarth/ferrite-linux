#!/bin/sh
# Bilan de fluidité du bureau : combien d'images ont dépassé leur budget (11 ms à 90 Hz),
# la pire, et pourquoi. Lit les relevés « profil du rendu » de ferrite-shell
# (shell/src/render/profile.rs), écrits tant que $XDG_RUNTIME_DIR/ferrite-profil existe.
#
#   tools/fluidite.sh [SECONDES]          sur le bureau en cours : mesure pendant SECONDES
#                                         (30 par défaut) pendant que tu t'en sers
#   tools/fluidite.sh --fenetre [BINAIRE] bureau de test en fenêtre, démarrage puis repos,
#                                         trois fois (BINAIRE : target/debug/ferrite-shell)
#
# Rien n'est modifié : le fichier de mesure est retiré à la fin.
# shellcheck shell=sh
set -eu

# shellcheck source=tools/lib.sh
. "$(dirname -- "$0")/lib.sh"

# Bilan des relevés lus sur l'entrée standard (journal de ferrite-shell).
bilan() {
    sed 's/\x1b\[[0-9;]*m//g' | grep 'profil du rendu' | awk '
        function champ(nom,    valeur) {
            if (!match($0, nom "=(\"[^\"]*\"|[^ ]*)")) return ""
            valeur = substr($0, RSTART + length(nom) + 1, RLENGTH - length(nom) - 1)
            gsub(/"/, "", valeur)
            return valeur
        }
        {
            releves++
            images += champ("images")
            hors += champ("hors_budget")
            gpu += champ("gpu_hors_budget")
            g = champ("gpu_max_ms") + 0
            if (g > gpu_max) gpu_max = g
            pire = champ("pire_image")
            if (pire != "" && pire + 0 > pire_ms + 0) { pire_ms = pire + 0; pire_texte = pire }
            n = split(champ("causes"), liste, ", ")
            for (i = 1; i <= n; i++) {
                c = liste[i]
                if (!match(c, / [0-9]+×$/)) continue
                fois = substr(c, RSTART + 1, RLENGTH - 3) + 0
                causes[substr(c, 1, RSTART - 1)] += fois
            }
        }
        END {
            if (releves == 0) {
                print "Aucun relevé : le bureau n'\''a rien dessiné, ou ferrite-shell est trop ancien."
                exit 1
            }
            printf "  relevés de 5 s          : %d\n", releves
            printf "  images préparées        : %d\n", images
            printf "  hors budget (> 11 ms)   : %d (%.1f %%)\n", hors, images ? 100 * hors / images : 0
            printf "  pire image              : %s\n", pire_texte == "" ? "aucune hors budget" : pire_texte
            texte = ""
            for (k = 0; k < 4; k++) {
                meilleur = ""
                for (c in causes) if (meilleur == "" || causes[c] > causes[meilleur]) meilleur = c
                if (meilleur == "") break
                texte = texte (texte == "" ? "" : ", ") meilleur " " causes[meilleur] "×"
                delete causes[meilleur]
            }
            printf "  causes principales      : %s\n", texte == "" ? "aucune" : texte
            printf "  rendus GPU > 11 ms      : %d (pire %.1f ms)\n", gpu, gpu_max
        }'
}

case "${1:-}" in
    --fenetre)
        binaire=${2:-"$REPO/target/debug/ferrite-shell"}
        [ -x "$binaire" ] || die "$binaire introuvable (cargo build -p ferrite-shell)"
        [ -n "${WAYLAND_DISPLAY:-}" ] || die "à lancer depuis un bureau Wayland"
        case "$WAYLAND_DISPLAY" in
            /*) affichage=$WAYLAND_DISPLAY ;;
            *) affichage="${XDG_RUNTIME_DIR:?}/$WAYLAND_DISPLAY" ;;
        esac
        for essai in 1 2 3; do
            # Dossier d'exécution à part (chemin court : limite des sockets à 108 caractères).
            dossier=$(mktemp -d /tmp/fr.XXXX)
            touch "$dossier/ferrite-profil"
            XDG_RUNTIME_DIR=$dossier WAYLAND_DISPLAY=$affichage timeout 60 \
                dbus-run-session -- "$binaire" --winit --capture "$dossier/capture.png" \
                --capture-delay 11 >"$dossier/journal" 2>&1 || true
            info "essai $essai (démarrage puis repos, 11 s)"
            bilan <"$dossier/journal" || true
            rm -rf "$dossier"
        done
        ;;
    ""|[0-9]*)
        secondes=${1:-30}
        journal="${XDG_RUNTIME_DIR:?}/ferrite-shell.log"
        [ -f "$journal" ] || die "$journal absent : ferrite-shell ne tourne pas sur ce bureau"
        drapeau="$XDG_RUNTIME_DIR/ferrite-profil"
        [ -e "$drapeau" ] && deja=1 || deja=0
        debut=$(wc -l <"$journal")
        touch "$drapeau"
        info "mesure pendant $secondes s : sers-toi du bureau normalement (menus, fenêtres, gestes)"
        sleep "$secondes"
        # Le dernier relevé tombe au plus 5 s après le début de sa période.
        sleep 6
        [ "$deja" = 1 ] || rm -f "$drapeau"
        info "bilan"
        tail -n "+$((debut + 1))" "$journal" | bilan
        ;;
    *) die "option inconnue : $1 (voir l'en-tête de $0)" ;;
esac
