# Écran de démarrage

Sources des images du démarrage de l'ISO, tirées du logo de Ferrite OS (`icons/SVG/ferrite-*.svg`).
Les PNG sont rendus depuis ces SVG (resvg) et rangés dans l'image :

| Source | Image rendue | Taille |
|---|---|---|
| `grub-fond.svg` | `image/config/bootloaders/grub-pc/splash.png` (fond du menu GRUB) | 800 × 600 |
| `plymouth-logo.svg` | `image/config/includes.chroot/usr/share/plymouth/themes/ferrite/logo.png` | 160 × 160 |
| `plymouth-point.svg` | `…/plymouth/themes/ferrite/point.png` (points qui s'allument pendant le chargement) | 12 × 12 |

Après une modification d'un SVG, rendre à nouveau le PNG à la même taille.
