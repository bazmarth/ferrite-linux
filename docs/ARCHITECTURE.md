# Architecture

Résumé de `docs/PLAN.md` §3, tenu à jour avec ce qui existe réellement (détails :
`docs/TECHNIQUE.md` §1).

```
Applications Ferrite (ferrite-apps, egui)  +  applications Linux (Wayland, XWayland, Flatpak, .deb)
ferrite-shell      compositeur Wayland (Smithay) et tout le bureau dessiné par lui
Programmes Ferrite ferrite-visage (visage), xdg-desktop-portal-ferrite (sélecteur de fichiers)
ferrite-system     appels système partagés, réglages, catalogue des applications
Services système   NetworkManager, PipeWire/WirePlumber, BlueZ, UPower, udisks2, logind,
                   polkit, power-profiles-daemon, gnome-keyring, portails
Debian 13          noyau Linux, Mesa, micrologiciels
```

## État actuel (2026-11-07)

| Composant | État |
|---|---|
| `ferrite-ui` | mesures, palettes et rôles de couleur, échelle, durées et courbes d'animation, fonds d'écran, icônes |
| `ferrite-shell` | écran réel (DRM/KMS, libinput, session logind) et fenêtre de dev (winit). Fenêtres : décorations Ferrite, aimantation, tuilage, vue d'ensemble, espaces de travail, effet génie, XWayland net à l'échelle 2. Bureau : barre du haut, Dock, lanceur, centre de contrôle, îlot, notifications, widgets (espace 0), écran de verrouillage (mot de passe, visage), agent polkit, capture et enregistrement de l'écran, mode nuit, veille, presse-papiers |
| `ferrite-apps` | Fichiers, Paramètres, TextEdit, Terminal, Musique, Lecteur vidéo, Lecteur PDF, Visionneuse, Horloge, Calculatrice, Moniteur, sélecteur de fichiers, galerie des pièces |
| `ferrite-visage` | enregistrement et vérification d'un visage (YuNet + SFace, caméra infrarouge) |
| `xdg-desktop-portal-ferrite` | `OpenFile`, `SaveFile`, `SaveFiles` |
| Image ISO | Debian 13 live, connexion automatique, `ferrite-session`, GRUB et Plymouth Ferrite ; installation sur une partition par `tools/install-dev.sh` |
| Pas encore fait | Logithèque, écran de connexion, paquets `.deb` et dépôt apt, installateur graphique, navigateur maison |

## Démarrage

1. GRUB (UEFI via shim signé) puis l'écran de démarrage Plymouth `ferrite`.
2. Image live : live-config crée l'utilisateur `ferrite` (sudo sans mot de passe).
   Machine installée : l'utilisateur de l'installation.
3. `getty@tty1` connecte l'utilisateur automatiquement.
4. `/etc/profile.d/90-ferrite.sh` exécute `/usr/local/bin/ferrite-session` sur tty1.
5. `ferrite-session` lance `ferrite-shell --drm` sur le bus D-Bus de systemd (`--user`),
   où tournent déjà le coffre à mots de passe, le son et les portails
   (`dbus-run-session` seulement si ce bus manque), écran verrouillé (sauf sur le live).
   Il préfère la version de développement (`~/.local/share/ferrite-dev`) et la relance
   selon son code de sortie.

## Unités

Les interfaces sont décrites en pixels logiques. `ferrite_ui::Scale` convertit vers les
pixels physiques ; l'échelle est en 120es comme dans `wp_fractional_scale_v1`
(240 = échelle 2, celle du ThinkBook).
