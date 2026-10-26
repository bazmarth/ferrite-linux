# Applications Ferrite

## `ferrite-apps` (`apps/core`)

Applications du premier ensemble du bureau, en Rust avec eframe/egui :

- **Fichiers** : parcourir les dossiers, remonter, saisir un chemin, trier dossiers et
  fichiers, ouvrir les fichiers texte dans TextEdit et déléguer les autres à `xdg-open`.
- **TextEdit** : ouvrir ou créer un fichier UTF-8, modifier le texte, enregistrer et
  utiliser Ctrl+S.
- **Calculatrice** : opérations `+`, `-`, `*`, `/`, nombres décimaux et parenthèses.
- **Terminal** : `foot`, lancé par le Dock.
- **Paramètres** : Wi-Fi et réseau (connexion avec mot de passe), Bluetooth (recherche,
  appairage, connexion), son (volume, muet, sortie), écran (luminosité), clavier
  (disposition appliquée aussitôt), date et heure (fuseau, mise à l'heure automatique)
  et à propos. Les commandes système tournent en arrière-plan (`ferrite-system`).

Lancement : `ferrite-apps files`, `ferrite-apps textedit [fichier]` ou
`ferrite-apps calculator` ou `ferrite-apps settings`.

Les applications sont des étapes utilisables, pas encore le périmètre complet de la phase
5 : corbeille, opérations de fichiers avec progression, onglets du terminal, coloration
syntaxique, Réglages, visionneuse PDF et lecteur multimédia restent à faire.
