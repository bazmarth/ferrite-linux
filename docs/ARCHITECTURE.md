# Architecture

Résumé de `docs/PLAN.md` §3, tenu à jour avec ce qui existe réellement.

```
Applications Ferrite (Rust, ferrite-ui)  +  applications Linux (.deb, Flatpak, XWayland)
ferrite-ui        boîte à outils graphique, client Wayland
ferrite-shell     compositeur Wayland (Smithay) : fenêtres, effets, barre de menus, Dock…
Services D-Bus    NetworkManager, PipeWire/WirePlumber, BlueZ, UPower, udisks2, PackageKit, logind
Debian 13         noyau Linux, Mesa, micrologiciels
```

## État actuel

| Composant | État |
|---|---|
| `ferrite-ui` | charte visuelle (`theme.rs`) : mesures, palettes, échelle, animations |
| `ferrite-shell` | squelette en fenêtre (winit) : protocoles de base, cascade, déplacement, redimensionnement, agrandissement, fond d'écran, capture |
| Image live | Debian 13 console, utilisateur `ferrite`, connexion automatique, `ferrite-session` (shell), `ferrite-check` |

## Démarrage de l'image live

1. GRUB (UEFI via shim signé, ou BIOS) → noyau avec `boot=live` et la langue, le clavier et
   le fuseau horaire en paramètres.
2. live-config crée l'utilisateur `ferrite` (sudo sans mot de passe).
3. `getty@tty1` connecte `ferrite` automatiquement.
4. `/etc/profile.d/90-ferrite.sh` exécute `/usr/local/bin/ferrite-session` sur tty1.
5. Phase 1 : `ferrite-session` affiche l'accueil puis un shell. Phase 2 : il lancera
   `dbus-run-session ferrite-shell`.

## Unités

Les interfaces sont décrites en pixels logiques. `ferrite_ui::Scale` convertit vers les
pixels physiques ; l'échelle est en 120es comme dans `wp_fractional_scale_v1`
(240 = échelle 2, celle du ThinkBook).
