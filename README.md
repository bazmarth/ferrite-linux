# Ferrite OS — base Linux

Ferrite OS sur le noyau Linux : Debian 13 « trixie » pour le matériel et les paquets
(`.deb`, Flatpak), avec **notre propre bureau** (compositeur Wayland `ferrite-shell`, écrit
en Rust avec Smithay) et **nos propres applications**.

Le plan complet, phase par phase, est dans [docs/PLAN.md](docs/PLAN.md). L'état d'avancement
est dans [docs/JOURNAL.md](docs/JOURNAL.md).

Le Ferrite d'origine (noyau maison, `D:\Os`) reste un projet séparé et intact.

## Organisation

| Dossier | Contenu |
|---|---|
| `ui/` | `ferrite-ui` : charte visuelle (mesures, couleurs, échelle, animations), puis widgets |
| `shell/` | `ferrite-shell` : compositeur Wayland et bureau (phase 2) |
| `apps/` | applications Ferrite (phase 5) |
| `services/` | services système : réglages, écran de connexion |
| `image/` | configuration `live-build` de l'image live |
| `packaging/` | paquets `.deb` et dépôt apt (phase 9) |
| `assets/` | polices, icônes, fonds d'écran, sons |
| `tools/` | scripts : installation, CI, image, machine virtuelle |
| `docs/` | plan, journal, décisions, dépendances, architecture, tests |

## Développer

Tout se fait dans **Debian 13 sous WSL2** (ou sous Fedora : voir [docs/FEDORA.md](docs/FEDORA.md)). Le dépôt est sur `D:\ferrite-linux`, donc
`/mnt/d/ferrite-linux` dans WSL.

```sh
cd /mnt/d/ferrite-linux
sudo tools/setup-wsl.sh    # paquets Debian, QEMU, live-build (une fois)
tools/setup-wsl.sh         # Rust stable via rustup (une fois)
tools/ci.sh                # format, lints, tests — avant chaque commit
```

## Lancer le bureau en fenêtre (WSLg)

```sh
tools/shell-dev.sh                 # ferrite-shell avec le terminal foot
tools/shell-dev.sh -- weston-terminal
```

Ctrl+Alt+Retour arrière quitte ferrite-shell ; Ctrl+Alt+M remet la dernière fenêtre
réduite. Options : `ferrite-shell --help`.

## Construire et essayer l'image live

```sh
tools/usb.sh                       # compile ferrite-shell puis → build/ferrite-live.iso (sudo)
tools/vm.sh                        # démarre l'ISO dans QEMU (UEFI), fenêtre via WSLg
tools/vm.sh --headless 90          # sans fenêtre : capture build/vm-ecran.png
```

Pour la clé USB et les vérifications sur le ThinkBook : [docs/TEST-THINKBOOK.md](docs/TEST-THINKBOOK.md).
L'image live ne monte ni ne modifie le SSD interne : Windows reste intact.
