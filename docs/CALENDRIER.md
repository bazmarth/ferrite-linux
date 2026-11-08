# Calendrier

`ferrite-apps calendar` — événements, tâches et rappels. Code : `apps/core/src/calendar/`.

## Ce qu'il fait

- **Vues** : Mois, Semaine, Jour, Agenda ; Aujourd'hui, À venir (14 jours), En retard,
  Tâches. Recherche dans les titres, lieux, descriptions et notes (Ctrl + F).
- **Événements** : journée entière ou heures, sur plusieurs jours, catégorie (couleur
  d'accentuation de Ferrite), lieu, description, répétition (jour, semaine et jours choisis,
  mois, an, intervalle, fin à une date ou après N fois), plusieurs rappels. Une occurrence
  d'une série peut être modifiée ou supprimée seule.
- **Tâches** : listes personnalisées, échéance (heure facultative), priorité, terminée,
  événement associé (« Tâche liée » depuis un événement), rappels.
- **Rappels** : annoncés par le centre de notifications de Ferrite, même Calendrier fermé,
  avec « Ouvrir », « Reporter 10 min » et « OK ». Les rappels non acquittés restent dans un
  bandeau de l'application.

## Fichiers

`$XDG_DATA_HOME/ferrite/calendrier/` (par défaut `~/.local/share/ferrite/calendrier/`) :

| Fichier | Contenu | Écrit par |
|---|---|---|
| `agenda` | catégories, listes, événements, tâches (`clé=valeur`) | l'application |
| `rappels` | état de chaque rappel : envoyé, reporté, acquitté | l'application et le service |
| `rappels.verrou`, `service.verrou` | verrous | — |

## Modules

| Module | Rôle |
|---|---|
| `date.rs` | jours civils, heures, saisies françaises (« 9/11 », « demain », « 9h30 ») |
| `model.rs` | événements, récurrences, occurrences, tâches, recherche, progression |
| `store.rs` | lecture tolérante et écriture atomique de `agenda` |
| `reminders.rs` | rappels dus, état, service `ferrite-apps rappels` (D-Bus) |
| `views.rs`, `lists.rs` | vues du calendrier et de productivité |
| `editor.rs`, `widgets.rs` | dialogues et pièces propres (petit calendrier, champs) |

## Essayer

```sh
XDG_DATA_HOME=/tmp/cal target/debug/ferrite-apps calendar semaine
R=$(mktemp -d /tmp/fr.XXXX); XDG_DATA_HOME=/tmp/cal XDG_RUNTIME_DIR=$R \
  WAYLAND_DISPLAY=/run/user/1000/wayland-1 dbus-run-session -- \
  target/debug/ferrite-shell --winit --capture /tmp/x.png -- ferrite-apps calendar mois
```

## Limites connues

- Pas d'import ni d'export iCalendar (.ics), pas de synchronisation (CalDAV).
- Pas de glisser-déposer des événements dans la grille ; pas de tâches récurrentes.
- Modifier « toute la série » depuis une occurrence décale la série entière du même écart.
