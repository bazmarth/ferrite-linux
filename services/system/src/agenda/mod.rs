//! Agenda du Calendrier, partagé par l'application (`ferrite-apps calendar`) et le bureau
//! (widgets Calendrier et Tâches, calendrier de la barre) : une seule source de données.
//!
//! - `date` : jours, heures, saisies et mise en forme en français ;
//! - `model` : événements, récurrences, tâches, catégories, listes et requêtes ;
//! - `store` : fichiers de `~/.local/share/ferrite/calendrier/`.

pub mod date;
pub mod model;
pub mod store;
