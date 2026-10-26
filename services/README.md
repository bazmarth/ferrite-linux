# Services

- `system` (`ferrite-system`) : accès au réseau, au Bluetooth, au son, à l'écran, à
  l'heure et aux réglages de l'utilisateur, avec un délai maximal par commande. Utilisée
  par ferrite-shell et Paramètres.

- `ferrite-settingsd` : stockage des réglages, exposé sur D-Bus.
- `ferrite-greeter` : écran de connexion (greetd).

Phases 4 et 8 de `docs/PLAN.md`.
