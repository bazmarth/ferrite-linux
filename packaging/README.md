# Paquets

Phase 9 de `docs/PLAN.md`. Les paquets `.deb` du bureau sont fabriqués par
`tools/paquets.sh` (dans `build/paquets`) ; l'ISO (`tools/usb.sh`) les installe, et les
ordinateurs Ferrite reçoivent les mises à jour par apt depuis
`https://bazmarth.github.io/ferrite-paquets` (GitHub Pages).

| Paquet | Contenu |
|---|---|
| `ferrite-shell` | bureau (compositeur), `ferrite-session`, `ferrite-greeter`, `ferrite-check` |
| `ferrite-apps` | applications et leurs entrées de menu (remplace `ferrite-textedit`) |
| `ferrite-visage` | déverrouillage par le visage |
| `ferrite-visage-modeles` | modèles YuNet et SFace, version fixe (pas retéléchargés à chaque mise à jour) |
| `xdg-desktop-portal-ferrite` | sélecteur de fichiers (portail) |
| `ferrite-desktop` | métapaquet : les autres, la source apt et la clé publique du dépôt |

Les programmes restent dans `/usr/local/bin` (voir `docs/DECISIONS.md`). Les réglages du
système (`/etc` : greetd, modprobe, zram…), l'installateur et le thème de démarrage restent
dans l'image (`image/config/includes.chroot`).

## Publier une mise à jour

```
tools/cle-depot.sh          # une seule fois : clé de signature
tools/publier-paquets.sh    # à chaque version, depuis un arbre commité
```

- `ferrite-depot.pgp` (ici) est la clé **publique** du dépôt. La clé **secrète** est dans
  `~/.local/share/ferrite/cle-depot`, jamais dans git : en garder une copie à part. Sans
  elle, plus aucune mise à jour ne peut être signée.
- Un Ferrite installé avant les paquets (par `tools/install-dev.sh`) se branche une fois
  avec `sudo apt install ./build/paquets/*.deb`.

## TextEdit seul

`tools/deb-textedit.sh` fabrique un paquet autonome de TextEdit
(`build/ferrite-textedit_VERSION_amd64.deb`) pour d'autres distributions (GNOME, Kali,
Ubuntu 24.04 ou plus récent). Installation : `sudo apt install ./ferrite-textedit_*.deb`.
