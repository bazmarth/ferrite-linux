# Services

- `system` (`ferrite-system`) : accès au réseau, au Bluetooth, au son, à l'écran, à
  l'énergie (`powerprofilesctl`), à l'heure, aux applications par défaut et aux réglages
  de l'utilisateur, avec un délai maximal par commande. Contient aussi le catalogue unique
  des applications Ferrite. Utilisée par ferrite-shell et ferrite-apps.
- `visage` (`ferrite-visage`) : déverrouillage par visage avec la caméra infrarouge
  (`enregistrer`, `verifier`, `effacer`, `etat`, `essai`).
- `portal` (`xdg-desktop-portal-ferrite`) : portail `FileChooser` ; ouvre le sélecteur de
  fichiers Ferrite (`ferrite-apps choisir`) pour toutes les applications.

Prévu (phase 8 de `docs/PLAN.md`) : `ferrite-greeter`, écran de connexion (greetd). Le
service de réglages `ferrite-settingsd` du plan n'existe pas : les réglages sont un
fichier texte lu directement (`docs/TECHNIQUE.md` §1.4).
