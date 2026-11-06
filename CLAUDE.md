# Ferrite OS — Règles de travail

Projet : bureau Wayland complet en Rust sur Debian 13. Langue : français partout
(code, interface, commits, docs).

## Non négociable

- Avant commit : `tools/ci.sh` doit passer (fmt + clippy `--all-targets -D warnings` + tests + shellcheck).
- Ne commiter QUE sur demande explicite de l'utilisateur, sur `main`, avec `git add` sur des chemins précis.
- Jamais `git add -A` ni `git add .`, et jamais `ferrite-theme/` (fichiers NTFS abîmés).
- Message de commit : « Domaine : résumé », puis une liste à puces.
- Ne jamais taper `sudo` ni demander un mot de passe : l'utilisateur tape lui-même.
- Ne jamais mettre de clé API ni de secret dans le dépôt ; ne pas lire `~/.ssh`, `~/.config/gh`, `~/.continue`.
- Ne jamais publier de release depuis un arbre non commité, ni rendre le dépôt public, ni supprimer une release ou un tag sans demande.
- Ne jamais écrire sur le disque interne du PC (Windows/Fedora) sans accord explicite.

## Contexte

- L'utilisateur n'est pas développeur : réponses en français simple, dire ce qui est vérifié et ce qui ne l'est pas.
- Vérifier visuellement avant d'annoncer qu'un écran est correct (`--capture` ou `tools/vm-test.sh`).
- Une autre session peut modifier le dépôt : `git status` / `git diff --stat` avant de commiter.

## Où trouver le reste

- Architecture, procédures de test, règles du compositeur, charte UI : la skill `ferrite-contexte` (invoque-la en début de chantier) et `docs/`.
- Journal des sessions : `docs/JOURNAL.md` (lire la fin avant de commencer). Choix non évidents : `docs/DECISIONS.md`.
