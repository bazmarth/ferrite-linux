# Charte visuelle de Ferrite OS

Ce document fixe l'apparence de tout le système : bureau, panneaux et applications.
Les valeurs exactes vivent dans le code (`ui/src/tokens.rs`, `ui/src/theme.rs`) ; ce
document dit **comment s'en servir**. En cas de doute, il prime sur une habitude du code.

Maquette validée le 10 novembre 2026 : bureau et centre de contrôle, lanceur, Réglages
(page Écrans en clair, page Apparence en sombre). Revue le 22 novembre 2026 : accent
« Oxyde », neutres sans teinte, barre de titre de 44 px, thème GTK pour les applications
tierces ; puis refonte « Forge » (maquette `docs/maquettes/refonte-forge.html`) : icônes au
trait, barre en îlots, centre de contrôle en tuiles, sélection en accent léger.

## 1. Principes

1. **Calme d'abord.** Une seule couleur vive à l'écran : l'accent, réservé à ce qui est
   allumé ou choisi. Le reste est en gris neutres.
2. **La forme dit l'état, pas le texte.** Une bascule allumée est pleine d'accent ; on
   n'écrit pas « Activé » ou « Désactivé » dessous.
3. **Des surfaces, pas des traits.** On sépare les zones par une différence de teinte
   (fond, carte, colonne), presque jamais par une bordure.
4. **Une seule façon de faire chaque chose.** Même ligne de colonne, même carte, même
   curseur partout.
5. **Les icônes d'applications ne changent pas.** La charte concerne les symboles de
   l'interface, pas les icônes des applications.

## 2. Couleurs (rôles de `Palette`)

Jamais de couleur écrite à la main : `tools/check-colors.sh` la refuse dans les
applications et limite le bureau. On prend un rôle.

| Rôle | Usage | Clair | Sombre |
|---|---|---|---|
| `window_background` | fond des fenêtres et des pages | #F4F4F6 | #141416 |
| `content_background` | contenu, champs de saisie, boutons | blanc | #1E1E22 |
| `sidebar_background` | colonne de gauche, barre de titre | #EBEBEF | #19191C |
| `surface` | cartes de groupe, sans bordure | blanc | #1E1E22 |
| `sidebar_pill` | élément choisi dans une colonne | blanc | #2A2A2F |
| `accent` | allumé, choisi, lien, curseur de texte | réglage utilisateur, Oxyde #B83A0A par défaut | Oxyde #FF7A45 |
| `text` / `text_secondary` | texte / indications, titres de groupe | #1B1B1F / #5A5A64 | #F2F2F5 / #A6A6B0 |
| `separator` | filet entre deux lignes d'une même carte | #E2E2E7 | #2C2C31 |
| `border_hover` | bord d'un champ ou d'une case (3:1) | #8A8A94 | #6E6E78 |
| `danger` / `success` / `warning` | signaux, toujours avec un mot ou une icône | #C62828 / #137333 / #8F5200 | #F87171 / #4ADE80 / #FBBF24 |

Chaque texte garde au moins 4,5:1 sur les fonds de son thème : le test
`textes_lisibles_sur_chaque_fond_des_deux_themes` (`ui/src/theme.rs`) le vérifie. Une
nouvelle couleur d'accent doit garder 4,5:1 comme texte sur `window_background`.

Bureau : `panel_fill()` (gris très clair à 97 %, ou ardoise en sombre) pour les panneaux ;
`raised_fill()` pour ce qui est posé dessus (bascule éteinte, curseur, bouton rond,
champ de recherche).

Texte posé sur l'accent : toujours `Color::ink_on` (blanc ou encre foncée selon la clarté
de l'accent).

## 3. Texte

Police Inter. Tailles de `tokens::typography`, jamais de demi-taille :

| Taille | Usage |
|---|---|
| 32 `DISPLAY` | écran d'accueil (installation, premier démarrage), une fois par écran |
| 24 `TITLE` | titre de page (Réglages), en semi-gras, dans le contenu |
| 20 `SECTION` | titre de section dans une page |
| 16 `HEADING` | titre de dialogue, de carte, de notification |
| 14 `BODY` | **taille de base** : libellé d'une ligne de réglage, boutons, champs |
| 13 `LABEL` | ligne de colonne, menus, tuile du centre de contrôle, titre de fenêtre (semi-gras) |
| 12 `SMALL` | titre de groupe (semi-gras, gris), indication sous un libellé |
| 11 `CAPTION` | titre de section de colonne, libellé sous une bascule ronde |

**Pas de capitales forcées** (ni `to_uppercase`, ni lettres espacées) pour les titres de
section : « Connexions », pas « CONNEXIONS ».

## 4. Formes et mesures

- Rayons : contrôle 7, carte et menu 10 à 12, panneau du bureau 20, fenêtre 14,
  pastille = hauteur / 2.
- Hauteurs : contrôle 32 (`control::HEIGHT`), 40 sur les écrans plein écran
  (`HEIGHT_LARGE`), barre de titre 44 (`TITLEBAR_HEIGHT`), ligne de menu 28.
- Espacements : multiples de 4 (4, 8, 12, 16, 24…).
- Ombres : uniquement les niveaux de `tokens::elevation` (CONTACT, SMALL, MENU, PANEL,
  WINDOW_*). Une carte de groupe n'a pas d'ombre ; l'élément choisi d'une colonne a
  l'ombre CONTACT.
- Cibles cliquables : 32 px de haut au moins (lignes de colonne), 36 px pour les boutons
  ronds, 46 px pour les bascules du centre de contrôle.

## 5. Composants

### Icônes système

- Une seule famille au trait, dessinée par `tools/icones.py` : grille de 24, trait de
  1,75, bouts et angles arrondis. Noir pour ce qui est allumé, gris clair `#D4D4D4` pour
  ce qui est éteint ; la teinte vient de `ferrite_ui::icons::symbolic`.
- L'état se lit à la forme : arcs Wi-Fi estompés selon le signal, trait oblique pour
  « éteint », deux points pour Bluetooth connecté, jauge dans la batterie.
- Pictogrammes partagés : `ferrite_ui::glyphs::GLYPHS` (bureau et applis). Ne pas
  dessiner de tracé à la main dans le code : ajouter le dessin au script et le relancer.
- Tailles : 16 dans la barre du haut, 17 dans les colonnes, 20 dans le centre de contrôle.

### Boutons de fenêtre

- Toujours à droite : réduire, agrandir, fermer (au bord). Pastilles de 14 px, 22 px
  entre deux centres, cible de clic de 22 px.
- Au repos, « fermer » est seul en accent (fenêtre active) ; les autres sont neutres et
  sans symbole. Au survol, le symbole apparaît et « fermer » passe au rouge. Fenêtre
  inactive : les trois neutres.

### Applications tierces

- Thème GTK « Ferrite » (clair) et « Ferrite-dark », générés par
  `ferrite-theme/gtk/generer.py` depuis cette charte : à relancer après tout changement
  de couleur. ferrite-shell choisit l'un ou l'autre avec le mode clair ou sombre.
- GTK 4 et libadwaita lisent `~/.config/gtk-4.0/gtk.css` (posé par `install.sh`, ou
  `/etc/skel` dans l'image).

### Fenêtre d'application à colonne (Réglages, Logithèque, Calendrier…)

- Colonne `sidebar_background`, pleine hauteur, compte et recherche en tête.
- Ligne de colonne (`style::sidebar_item`) : symbole au trait de 17 px **gris**, libellé
  13. Choisie : fond en accent léger (`style::selection_pill`, sans ombre), symbole et
  libellé en accent, libellé en semi-gras. Même règle dans Fichiers.
- Titre de section (`style::sidebar_heading`) : 11 semi-gras gris, sans capitales.
- Contenu : bande du haut **sans titre ni filet**, du fond de la page (elle porte les
  boutons de fenêtre et sert à déplacer) ; tête de page dans le contenu : symbole de la
  page dans un carré de 40 px en accent léger (`rows::accent_tile`), puis le titre en 24
  semi-gras.

### Groupes de réglages (`settings/rows.rs`)

- Titre de groupe 12 semi-gras gris, au-dessus de la carte.
- Carte `surface`, rayon 12, **sans bordure** ; lignes de 48 px séparées par un filet
  `separator` décalé de 16 px.
- Valeur à droite en 13 gris ; commandes à droite (interrupteur, liste, segments).
- Pastille de ligne (appareil, compte, imprimante…) : carré de 30 px en accent léger,
  symbole en accent. Plus de pastilles de couleurs différentes selon la rubrique.
- État « bon » (connecté…) : point vert devant l'indication, texte gris (jamais toute la
  ligne en vert).
- Grande ligne de tête (Wi-Fi, Bluetooth) : titre, état et interrupteur, sans symbole
  (il est dans la tête de page).

### Fichiers

- Dossiers d'un seul ton (`style::folder_front`/`folder_back`), emblème en accent ; la
  couleur ne vient que d'un choix de l'utilisateur (Personnaliser) ou d'une étiquette.
- Pied fondu dans le contenu : même fond, sans filet.

### Centre de contrôle

- Panneau de 372 px, rayon 20, marge 14, gris neutre (sans teinte bleue).
- Tête : batterie (pictogramme, pourcentage, « en charge ») à gauche ; à droite, boutons
  ronds neutres (compte, Réglages, verrouiller, éteindre). L'arrêt n'est pas rouge : il
  ouvre une confirmation.
- Six tuiles de même taille, deux par rangée (Wi-Fi, Bluetooth, Mode sombre, Silence,
  Énergie, Mode avion) : disque, nom, état sur une ligne. Allumée = pleine d'accent ; le
  chevron (Wi-Fi, Bluetooth) ouvre la liste. Indisponible sur la machine = grisée, jamais
  absente sans raison.
- Curseurs épais de 40 px (volume, luminosité) : piste `track`, part réglée pleine
  d'accent, pictogramme dedans à gauche, **sans pourcentage**.
- Pastilles du bas (Mode nuit, Éclairage du clavier, Clavier, second écran), trois par
  rangée : accent léger quand le réglage est mis.

### Barre du haut

- En îlots sur le fond d'écran : verre sombre écrit en blanc, le fond d'écran visible
  entre eux. Une fenêtre agrandie rend la barre opaque : les îlots deviennent des teintes.
- À gauche : le symbole Ferrite (ouvre le lanceur) et les marques des bureaux (celui qui
  est affiché en pilule d'accent).
- Au centre : la date et l'heure (calendrier et notifications).
- À droite : les indicateurs et le pourcentage de batterie (ouvrent le centre de
  contrôle), puis la cloche des notifications, avec un point d'accent s'il y en a.

### Dock

- Plateau ardoise translucide, rayon 22, liseré clair très fin, marges réduites (9 px
  autour des icônes, 12 px au-dessus du bord de l'écran).
- Point d'accent sous les applications ouvertes ; trait fin avant la corbeille.

### Lanceur

- Panneau `panel_fill`, rayon 20, champ de recherche `raised_fill` de 48 px en tête.
- Titres de rubrique en 12 semi-gras gris, sans capitales.
- Sans recherche : « Applications Ferrite », puis les autres applications rangées par
  rubrique (Internet, Bureautique, Multimédia, Graphisme, Développement, Jeux,
  Éducation, Outils et système, Autres), d'après leurs catégories freedesktop. Pendant
  une recherche : une seule liste. Entrée lance la première case affichée.

## 6. Mouvement

Durées de `tokens::motion` (QUICK 120, SHORT 200, NORMAL 320, LONG 360 ms), courbe
`EaseOut`. Toujours respecter « réduire les animations ».

## 7. À vérifier avant de livrer un écran

1. Aucune couleur à la main (`tools/check-colors.sh`).
2. Capture en clair **et** en sombre.
3. Aucun texte coupé par « … » dans la configuration par défaut.
4. Pas de capitales forcées, pas de bordure autour d'une carte de groupe.
5. Aucun bouton qui fait semblant : grisé s'il ne fait rien.
