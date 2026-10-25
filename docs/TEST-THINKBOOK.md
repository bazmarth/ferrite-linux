# Essai sur le ThinkBook (phase 1)

L'image live démarre depuis une clé USB sans rien écrire sur le SSD : Windows n'est pas
touché. Il suffit de retirer la clé et de redémarrer pour retrouver Windows.

## 1. Préparer la clé USB (depuis Windows)

1. Récupérer `D:\ferrite-linux\build\ferrite-live.iso` (≈ 1 Gio) et vérifier son empreinte
   avec `build\ferrite-live.iso.sha256` :
   `Get-FileHash D:\ferrite-linux\build\ferrite-live.iso -Algorithm SHA256`
2. Ouvrir **Rufus**, choisir la clé (**toutes ses données seront effacées**), puis l'ISO.
3. Schéma de partition **GPT**, système de destination **UEFI (non CSM)**.
4. Si Rufus demande le mode d'écriture : **mode image DD**.
5. Démarrer.

## 2. Démarrer sur la clé

1. Brancher la clé, éteindre complètement le portable.
2. Allumer en appuyant sur **F12** (menu de démarrage Lenovo ; sinon le petit bouton
   Novo à côté de la prise d'alimentation, puis « Boot Menu »).
3. Choisir la clé USB (entrée UEFI), puis « Live system » dans GRUB.
4. Le Secure Boot peut rester activé (chargeur signé Debian).

L'utilisateur `ferrite` est connecté automatiquement et l'accueil Ferrite s'affiche.

## 3. Vérifications

Lancer :

```sh
ferrite-check
```

Puis faire à la main ce que le script ne peut pas faire :

| # | Vérification | Commande | Attendu |
|---|---|---|---|
| 1 | Temps de démarrage | (chronomètre, du menu GRUB à l'accueil) | moins de 60 s |
| 2 | Réseaux Wi-Fi visibles | `nmcli dev wifi list` | la liste des réseaux |
| 3 | Connexion Wi-Fi | `nmcli dev wifi connect "NOM" --ask` puis `ping -c3 debian.org` | réponses |
| 4 | Son audible | `speaker-test -c 2 -t wav -l 1` | « Front left », « Front right » |
| 5 | Clavier AZERTY | taper `azerty é è à ç ù` | caractères corrects |
| 6 | Pavé tactile | `sudo libinput list-devices` | une entrée « Touchpad » |
| 7 | Batterie | `cat /sys/class/power_supply/BAT*/capacity` | un pourcentage |
| 8 | SSD intact | `lsblk -o NAME,SIZE,FSTYPE,MOUNTPOINTS` | aucune partition `nvme…` montée |

Le rapport de `ferrite-check` est dans `/tmp/ferrite-check.txt`. Pour le récupérer, le
plus simple est de photographier l'écran (le système live n'écrit rien sur le disque).

En cas d'échec du Wi-Fi :

```sh
sudo dmesg | grep -i rtw89
ls /lib/firmware/rtw89/
```

## 4. Persistance (facultatif)

Sans persistance, tout est oublié à l'extinction. Pour garder ses fichiers sur la clé :

1. Sous Linux, dans l'espace libre de la clé, créer une partition ext4 nommée
   `persistence` contenant un fichier `persistence.conf` avec la ligne `/ union`.
2. Au menu GRUB, appuyer sur `e`, ajouter ` persistence persistence-media=removable-usb`
   à la fin de la ligne `linux`, puis F10.

`persistence-media=removable-usb` limite la recherche aux clés USB : le SSD n'est pas lu.
