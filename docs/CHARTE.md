# Charte visuelle de Ferrite OS

Ce document fixe l'apparence de tout le système : bureau, panneaux et applications.
Les valeurs exactes vivent dans le code (`ui/src/tokens.rs`, `ui/src/theme.rs`) ; ce
document dit **comment s'en servir**. En cas de doute, il prime sur une habitude du code.

Maquette validée le 10 novembre 2026 : bureau et centre de contrôle, lanceur, Réglages
(page Écrans en clair, page Apparence en sombre).

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
| `window_background` | fond des fenêtres et des pages | blanc | #22262D |
| `sidebar_background` | colonne de gauche | #F2F3F6 | #1E2127 |
| `surface` | cartes de groupe, sans bordure | #F4F5F7 | #2B2F36 |
| `sidebar_pill` | élément choisi dans une colonne | blanc | #343840 |
| `accent` | allumé, choisi, lien, curseur de texte | réglage utilisateur | idem |
| `text` / `text_secondary` | texte / indications, titres de groupe | | |
| `separator` | filet entre deux lignes d'une même carte | | |

Bureau : `panel_fill()` (gris très clair à 97 %, ou ardoise en sombre) pour les panneaux ;
`raised_fill()` pour ce qui est posé dessus (bascule éteinte, curseur, bouton rond,
champ de recherche).

Texte posé sur l'accent : toujours `Color::ink_on` (blanc ou encre foncée selon la clarté
de l'accent).

## 3. Texte

Police Inter. Tailles de `tokens::typography`, jamais de demi-taille :

| Taille | Usage |
|---|---|
| 24 `TITLE` | titre de page (Réglages), en semi-gras, dans le contenu |
| 16 `HEADING` | titre de dialogue |
| 14 `BODY` | libellé d'une ligne de réglage |
| 13 `LABEL` | ligne de colonne, tuile du centre de contrôle |
| 12 `SMALL` | titre de groupe (semi-gras, gris), indication sous un libellé |
| 11 `CAPTION` | titre de section de colonne, libellé sous une bascule ronde |

**Pas de capitales forcées** (ni `to_uppercase`, ni lettres espacées) pour les titres de
section : « Connexions », pas « CONNEXIONS ».

## 4. Formes et mesures

- Rayons : contrôle 8, carte 12, panneau du bureau 20, fenêtre 14, pastille = hauteur / 2.
- Espacements : multiples de 4 (4, 8, 12, 16, 24…).
- Ombres : uniquement les niveaux de `tokens::elevation` (CONTACT, SMALL, MENU, PANEL,
  WINDOW_*). Une carte de groupe n'a pas d'ombre ; l'élément choisi d'une colonne a
  l'ombre CONTACT.
- Cibles cliquables : 32 px de haut au moins (lignes de colonne), 36 px pour les boutons
  ronds, 46 px pour les bascules du centre de contrôle.

## 5. Composants

### Fenêtre d'application à colonne (Réglages, Logithèque, Calendrier…)

- Colonne `sidebar_background`, pleine hauteur, compte et recherche en tête.
- Ligne de colonne (`style::sidebar_item`) : symbole au trait de 17 px **gris**, libellé
  13. Choisie : pastille `sidebar_pill` + ombre de contact, symbole en accent, libellé en
  semi-gras. Plus de pastilles de couleur derrière les symboles.
- Titre de section (`style::sidebar_heading`) : 11 semi-gras gris, sans capitales.
- Contenu : bande du haut **sans titre ni filet**, du fond de la page (elle porte les
  boutons de fenêtre et sert à déplacer) ; le titre de la page est dans le contenu, en
  24 semi-gras.

### Groupes de réglages (`settings/rows.rs`)

- Titre de groupe 12 semi-gras gris, au-dessus de la carte.
- Carte `surface`, rayon 12, **sans bordure** ; lignes de 48 px séparées par un filet
  `separator` décalé de 16 px.
- Valeur à droite en 13 gris ; commandes à droite (interrupteur, liste, segments).

### Centre de contrôle

- Panneau de 364 px, rayon 20, marge 14.
- En tête, deux grandes tuiles côte à côte (Wi-Fi, Bluetooth) : nom, état sur une ligne,
  chevron qui ouvre la liste. Allumée = pleine d'accent.
- Dessous, une grille de bascules rondes (4 par rangée) : disque de 46 px, libellé court
  dessous en 11 (« Silence », « Économie », « Clavier US »). Allumée = disque d'accent.
  Indisponible sur la machine = grisée, jamais absente sans raison.
- Curseurs épais de 40 px (volume, luminosité) : pilule `track`, part réglée claire,
  pictogramme dedans à gauche, **sans pourcentage**.
- Pied : batterie en 12 gris à gauche ; à droite, boutons ronds neutres (compte,
  Réglages, verrouiller, éteindre). L'arrêt n'est pas rouge : il ouvre une confirmation.

### Barre du haut

- Voile léger sur le fond d'écran (le fond se devine), écriture blanche.
- À gauche : le symbole Ferrite seul (ouvre le lanceur), puis les marques des bureaux.
- Au centre : la date et l'heure.
- À droite : les indicateurs groupés dans une pastille toujours visible (ouvre le centre
  de contrôle).

### Dock

- Plateau ardoise translucide, rayon 22, liseré clair très fin, marges réduites.
- Point blanc sous les applications ouvertes ; trait fin avant la corbeille.

### Lanceur

- Panneau `panel_fill`, rayon 20, champ de recherche `raised_fill` de 48 px en tête.
- Titres de rubrique en 12 semi-gras gris, sans capitales.

## 6. Mouvement

Durées de `tokens::motion` (QUICK 150, SHORT 200, NORMAL 280, LONG 360 ms), courbe
`EaseOut`. Toujours respecter « réduire les animations ».

## 7. À vérifier avant de livrer un écran

1. Aucune couleur à la main (`tools/check-colors.sh`).
2. Capture en clair **et** en sombre.
3. Aucun texte coupé par « … » dans la configuration par défaut.
4. Pas de capitales forcées, pas de bordure autour d'une carte de groupe.
5. Aucun bouton qui fait semblant : grisé s'il ne fait rien.
