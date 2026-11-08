#!/bin/sh
# Relais de rustc (`build.rustc-wrapper` dans .cargo/config.toml) : toute compilation du
# dépôt passe en priorité basse, pour le processeur (nice 15) comme pour le disque (classe
# « idle »), même lancée par `cargo` directement, par un éditeur ou par une autre session.
# Le bureau garde la main pendant ce temps (voir docs/DECISIONS.md, 2026-11-08).
# Le relais change la priorité de son propre processus puis laisse la place à rustc :
# les enfants (éditeur de liens, clippy) en héritent. Sans effet sur les résultats.
if [ "$(nice 2>/dev/null || echo 0)" -lt 15 ]; then
    renice -n 15 -p $$ >/dev/null 2>&1 || true
fi
if command -v ionice >/dev/null 2>&1; then
    ionice -c 3 -p $$ >/dev/null 2>&1 || true
fi
exec "$@"
