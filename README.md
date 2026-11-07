# Ferrite OS — base Linux

Ferrite OS sur le noyau Linux : Debian 13 « trixie » pour le matériel et les paquets
(`.deb`, Flatpak), avec **notre propre bureau** (compositeur Wayland `ferrite-shell`, écrit
en Rust avec Smithay) et **nos propres applications** (`ferrite-apps`, egui).

Ce n'est pas une personnalisation d'un bureau existant : barre du haut, Dock, centre de
contrôle, îlot, écran de verrouillage, widgets, Fichiers, Paramètres, TextEdit, Musique,
Lecteur vidéo, Lecteur PDF, Visionneuse, Horloge, Terminal… sont écrits ici.

Le Ferrite d'origine (noyau maison) reste un projet séparé et intact.

## Documents

| Document | Contenu |
|---|---|
| [docs/TECHNIQUE.md](docs/TECHNIQUE.md) | référence technique : structure, règles, pièges, état et suite du projet |
| [docs/PLAN.md](docs/PLAN.md) | cahier des charges d'origine, phase par phase |
| [docs/DECISIONS.md](docs/DECISIONS.md) | choix non évidents et leurs raisons (priment sur le plan) |
| [docs/JOURNAL.md](docs/JOURNAL.md) | journal des séances : ce qui est fait, vérifié ou non |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | vue d'ensemble des couches et du démarrage |
| [docs/RACCOURCIS.md](docs/RACCOURCIS.md) | raccourcis clavier et gestes |
| [docs/DEPENDANCES.md](docs/DEPENDANCES.md) | bibliothèques et paquets, avec leur raison |
| [docs/NOUVELLE-APPLICATION.md](docs/NOUVELLE-APPLICATION.md) | ajouter une application Ferrite |
| [docs/TEST-THINKBOOK.md](docs/TEST-THINKBOOK.md) | essayer l'ISO sur clé USB |
| [docs/FEDORA.md](docs/FEDORA.md) | développer depuis Fedora (historique) |

## Organisation

| Dossier | Contenu |
|---|---|
| `shell/` | `ferrite-shell` : compositeur Wayland et tout le bureau |
| `apps/core/` | `ferrite-apps` : toutes les applications Ferrite, une sous-commande par application |
| `services/system/` | `ferrite-system` : appels système partagés, réglages, catalogue des applications |
| `services/visage/` | `ferrite-visage` : déverrouillage par visage (caméra infrarouge) |
| `services/portal/` | `xdg-desktop-portal-ferrite` : sélecteur de fichiers Ferrite pour toutes les applications |
| `ui/` | `ferrite-ui` : mesures, couleurs, échelle, animations |
| `image/` | configuration `live-build` de l'image ISO |
| `icons/`, `assets/` | icônes et logo, polices, fonds d'écran, sons, écran de démarrage |
| `third_party/winit/` | copie retouchée de winit (pincement du pavé tactile) |
| `tools/` | scripts : CI, mode dev, image, machine virtuelle, installation |
| `packaging/` | paquets `.deb` et dépôt apt (pas encore fait) |
| `docs/` | documentation |

## Développer

Le développement se fait **dans Ferrite lui-même**, sur le ThinkBook : Debian 13 + Ferrite
installés sur leur propre partition (`tools/install-dev.sh`), dépôt sur la partition de
données. Le bureau en cours d'usage est remplacé par la version compilée :

```sh
tools/ci.sh        # format, clippy, tests, couleurs, shellcheck — avant chaque commit
tools/dev-on.sh    # compile et copie le bureau et les applis, puis Ctrl+Alt+Retour arrière
tools/dev-off.sh   # revenir aux binaires stables de /usr/local/bin
```

Une version copiée qui plante deux fois de suite est écartée et la session repart sur les
binaires stables.

Essai dans une fenêtre, sans toucher au vrai bureau, avec capture d'écran :

```sh
target/debug/ferrite-shell --winit --capture /tmp/x.png --capture-delay 5 -- ferrite-apps files
```

## Construire et essayer l'image ISO

```sh
tools/usb.sh                                   # build/ferrite-live.iso (sudo, tapé par l'utilisateur)
tools/vm-test.sh tools/scenarios/stabilite.txt # démarre l'ISO dans QEMU et prend des captures
```

Clé USB et vérifications sur le ThinkBook : [docs/TEST-THINKBOOK.md](docs/TEST-THINKBOOK.md).
L'image live ne monte ni ne modifie le SSD interne : Windows reste intact.
