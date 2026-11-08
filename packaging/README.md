# Paquets

Pas encore fait (phase 9 de `docs/PLAN.md`) : configuration `cargo-deb` de chaque
composant, métapaquet `ferrite-desktop` et dépôt apt signé. Aujourd'hui, les binaires sont
copiés dans `/usr/local/bin` par `tools/usb.sh` (ISO) et `tools/install-dev.sh`. La clé de
signature ne devra jamais être dans le dépôt.

Exception : `tools/deb-textedit.sh` fabrique un paquet autonome de TextEdit
(`build/ferrite-textedit_VERSION_amd64.deb`) pour d'autres distributions (GNOME, Kali,
Ubuntu 24.04 ou plus récent). Installation : `sudo apt install ./ferrite-textedit_*.deb`.
