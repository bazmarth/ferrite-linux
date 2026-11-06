#!/bin/sh
# Refuse les couleurs écrites à la main dans les applications : elles prennent les rôles
# communs de apps/core/src/style.rs (fond, filet, scène, succès…), pour qu'une même chose
# ait la même teinte partout, en clair comme en sombre.
#
# Fichiers dont les couleurs sont voulues, et pourquoi :
#   style.rs, kit.rs          la charte elle-même
#   video.rs                  lecteur toujours sombre, comme la maquette
#   terminal/colors.rs        palette du terminal (couleurs ANSI)
#   editor/icons.rs           couleur de chaque type de fichier
#   editor/highlight.rs       coloration syntaxique
#   monitor/mod.rs            couleur de chaque courbe (processeur, mémoire, réseau…)
#   settings.rs               tuiles des rubriques et illustrations (écran, fonds)
#   settings/rows.rs          tuiles colorées des lignes de réglage
set -eu
cd "$(dirname -- "$0")/.."

allowed='style.rs|kit.rs|video.rs|terminal/colors.rs|editor/icons.rs|editor/highlight.rs|monitor/mod.rs|settings.rs|settings/rows.rs'
pattern='0x[0-9A-Fa-f]{6}\b|from_rgb\(\s*0x|from_rgba_unmultiplied\(\s*[0-9]|from_gray\(\s*[0-9]'

found=$(grep -rnE "$pattern" apps/core/src --include='*.rs' \
    | grep -vE "^apps/core/src/($allowed):" \
    | grep -vE '#\[cfg\(test\)\]|assert' || true)

if [ -n "$found" ]; then
    printf 'Couleurs écrites à la main (prendre un rôle de style.rs, ou en ajouter un) :\n%s\n' "$found" >&2
    exit 1
fi

# Bureau : ses panneaux lisent la même charte (ferrite_ui::theme::Palette). Les couleurs
# propres qui restent sont voulues (widgets et écran de verrouillage en verre sombre, îlot
# noir, barre de capture sombre, météo, voile de la barre) ; chaque fichier en garde au
# plus autant qu'aujourd'hui, pour qu'il n'en arrive pas de nouvelles.
shell_pattern='0x[0-9A-Fa-f]{6}\b|rgb\(\s*0x|from_rgb\(\s*0x|from_rgba_unmultiplied\(\s*[0-9]'
over=''
for limit in \
    shell/src/desktop/panels/lock_screen.rs:30 \
    shell/src/desktop/panels/cards.rs:29 \
    shell/src/desktop/panels.rs:28 \
    shell/src/desktop/panels/island.rs:7 \
    shell/src/desktop/panels/auth.rs:3 \
    shell/src/render/shaders.rs:1 \
    shell/src/render/ghost.rs:1 \
    shell/src/desktop/svg.rs:1; do
    file=${limit%%:*}
    max=${limit##*:}
    count=$(grep -cE "$shell_pattern" "$file" || true)
    if [ "$count" -gt "$max" ]; then
        over="$over\n  $file : $count couleurs propres (au plus $max)"
    fi
done
others=$(grep -rlE "$shell_pattern" shell/src --include='*.rs' \
    | grep -vE '^shell/src/(desktop/panels/(lock_screen|cards|island|auth)\.rs|desktop/panels\.rs|render/(shaders|ghost)\.rs|desktop/svg\.rs)$' || true)
for file in $others; do
    over="$over\n  $file : couleurs écrites à la main"
done
if [ -n "$over" ]; then
    printf 'Couleurs du bureau hors de la charte (prendre palette() ou un rôle de ferrite_ui) :%b\n' "$over" >&2
    exit 1
fi
printf 'couleurs : uniquement les rôles communs\n'
