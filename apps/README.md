# Applications Ferrite

## `ferrite-apps` (`apps/core`)

Applications du premier ensemble du bureau, en Rust avec eframe/egui :

- **Fichiers** : parcourir les dossiers, remonter, saisir un chemin, trier dossiers et
  fichiers, ouvrir les fichiers texte dans TextEdit, les images/PDF dans Visionneuse,
  les vidéos dans le Lecteur vidéo et déléguer les autres à `xdg-open`.
- **TextEdit** : onglets, fichiers UTF-8, enregistrement et annulation/rétablissement ;
  les fichiers Markdown ont les modes Éditer, Partagé et Aperçu avec rendu des titres,
  listes, tableaux, citations et blocs de code.
- **Calculatrice** : opérations `+`, `-`, `*`, `/`, nombres décimaux et parenthèses.
- **Visionneuse** : images PNG/JPEG et documents PDF, avec navigation par page et zoom.
  Le rendu PDF s'appuie sur Poppler (`poppler-utils`).
- **Lecteur vidéo** : lecture des vidéos prises en charge par GStreamer, avec son,
  recherche, pause, muet et plein écran (`ferrite-apps video [fichier]`).
- **Terminal** : `alacritty_terminal`, onglets, plusieurs fenêtres, défilement, sélection,
  recherche, zoom, souris dans les programmes (`ferrite-apps terminal [dossier] [-e commande]`).
- **Paramètres** : Wi-Fi et réseau (connexion avec mot de passe), Bluetooth (recherche,
  appairage, connexion), son (volume, muet, sortie), écran (luminosité), clavier
  (disposition appliquée aussitôt), date et heure (fuseau, mise à l'heure automatique)
  et à propos. Les commandes système tournent en arrière-plan (`ferrite-system`).

Lancement : `ferrite-apps files`, `ferrite-apps viewer [image ou PDF]`,
`ferrite-apps video [fichier vidéo]`,
`ferrite-apps textedit [fichier]`, `ferrite-apps calculator` ou `ferrite-apps settings`.

Les applications sont des étapes utilisables, pas encore le périmètre complet de la phase
5 : corbeille, opérations de fichiers avec progression, profils du terminal, coloration
syntaxique et Réglages restent à faire.
