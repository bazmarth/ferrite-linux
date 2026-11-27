//! Agent d'authentification polkit : quand une application demande les droits
//! administrateur (monter un disque interne, installer un logiciel, `pkexec`…), polkit
//! s'adresse à l'agent inscrit pour la session, qui affiche une fenêtre « Authentification
//! requise ». Le mot de passe est vérifié par `polkit-agent-helper-1` (programme de polkit,
//! setuid) puis effacé ; le bureau ne le garde jamais.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use smithay::reexports::calloop::{LoopHandle, channel};
use zbus::{interface, zvariant::OwnedValue};

use crate::{desktop::text_field, state::Ferrite};

const AGENT_PATH: &str = "/org/ferrite/PolicyKit1/AuthenticationAgent";
const HELPERS: [&str; 2] = [
    "/usr/lib/polkit-1/polkit-agent-helper-1",
    "/usr/libexec/polkit-agent-helper-1",
];

/// Message vers la boucle du compositeur.
#[derive(Debug)]
pub enum Event {
    /// Polkit demande une authentification ; la réponse part sur `reply`.
    Begin {
        cookie: String,
        message: String,
        user: String,
        reply: async_channel::Sender<bool>,
    },
    Cancel(String),
    /// Le mot de passe a été vérifié (`true` : accepté).
    Checked {
        cookie: String,
        accepted: bool,
    },
}

/// Demande affichée (ou en attente derrière une autre).
#[derive(Debug)]
pub struct Auth {
    pub message: String,
    pub user: String,
    cookie: String,
    pub password: String,
    /// Place du curseur dans le mot de passe, en caractères.
    pub cursor: usize,
    pub checking: bool,
    pub refused: bool,
    reply: async_channel::Sender<bool>,
}

impl Auth {
    /// Demande fictive, pour les captures (`--demo-auth`) : elle ne mène nulle part.
    pub fn demo() -> Self {
        Self {
            message: "Une authentification est nécessaire pour monter Disque 291,6G \
                      (lecture seule)."
                .to_owned(),
            user: std::env::var("USER").unwrap_or_else(|_| "ferrite".to_owned()),
            cookie: String::new(),
            password: "secret".to_owned(),
            cursor: 6,
            checking: false,
            refused: false,
            reply: async_channel::bounded(1).0,
        }
    }
}

/// Lien avec le fil de l'agent.
#[derive(Debug)]
pub struct Link {
    events: channel::Sender<Event>,
}

struct Agent {
    events: channel::Sender<Event>,
}

#[interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl Agent {
    /// Ne répond qu'une fois l'utilisateur passé : polkit attend cette réponse.
    async fn begin_authentication(
        &self,
        _action_id: String,
        message: String,
        _icon_name: String,
        _details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedValue>)>,
    ) -> zbus::fdo::Result<()> {
        let user = choose_user(&identities)
            .ok_or_else(|| zbus::fdo::Error::Failed("aucun compte à authentifier".to_owned()))?;
        let (reply, answer) = async_channel::bounded(1);
        self.events
            .send(Event::Begin {
                cookie,
                message,
                user,
                reply,
            })
            .map_err(|_| zbus::fdo::Error::Failed("le bureau ne répond plus".to_owned()))?;
        match answer.recv().await {
            Ok(true) => Ok(()),
            _ => Err(zbus::fdo::Error::Failed("authentification annulée".to_owned())),
        }
    }

    fn cancel_authentication(&self, cookie: String) {
        let _ = self.events.send(Event::Cancel(cookie));
    }
}

/// Le compte à authentifier : celui de la session s'il figure parmi ceux que polkit
/// accepte (administrateurs), sinon le premier.
fn choose_user(identities: &[(String, HashMap<String, OwnedValue>)]) -> Option<String> {
    let uids: Vec<u32> = identities
        .iter()
        .filter(|(kind, _)| kind == "unix-user")
        .filter_map(|(_, details)| details.get("uid")?.downcast_ref::<u32>().ok())
        .collect();
    let own = own_uid()?;
    let uid = if uids.contains(&own) { own } else { *uids.first()? };
    user_name(uid)
}

/// Compte qui fait tourner le bureau (propriétaire de son dossier `/proc`).
fn own_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self")
        .ok()
        .map(|metadata| metadata.uid())
}

fn user_name(uid: u32) -> Option<String> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        let name = fields.next()?;
        (fields.nth(1)?.parse::<u32>().ok()? == uid).then(|| name.to_owned())
    })
}

/// Branche le canal sur la boucle et inscrit l'agent auprès de polkit (bus système).
pub fn start(handle: &LoopHandle<'static, Ferrite>) -> Result<Link, String> {
    let (events, receiver) = channel::channel::<Event>();
    handle
        .insert_source(receiver, |event, _, state| {
            if let channel::Event::Msg(event) = event {
                state.on_polkit_event(event);
                state.request_redraw();
            }
        })
        .map_err(|err| err.to_string())?;
    let agent = Agent {
        events: events.clone(),
    };
    std::thread::Builder::new()
        .name("polkit".to_owned())
        .spawn(move || {
            if let Err(err) = serve(agent) {
                tracing::warn!(%err, "agent polkit non inscrit : les demandes administrateur échoueront");
            }
        })
        .map_err(|err| err.to_string())?;
    Ok(Link { events })
}

fn serve(agent: Agent) -> zbus::Result<()> {
    let connection = zbus::blocking::connection::Builder::system()?
        .serve_at(AGENT_PATH, agent)?
        .build()?;
    let subject = subject();
    connection.call_method(
        Some("org.freedesktop.PolicyKit1"),
        "/org/freedesktop/PolicyKit1/Authority",
        Some("org.freedesktop.PolicyKit1.Authority"),
        "RegisterAuthenticationAgent",
        &(subject, "fr_FR.UTF-8", AGENT_PATH),
    )?;
    tracing::info!("agent polkit inscrit");
    // La connexion sert l'agent tant que le bureau vit.
    loop {
        std::thread::park();
    }
}

/// La session de connexion (logind), ou à défaut ce processus.
fn subject() -> (String, HashMap<String, zbus::zvariant::Value<'static>>) {
    use zbus::zvariant::Value;
    if let Ok(session) = std::env::var("XDG_SESSION_ID") {
        return (
            "unix-session".to_owned(),
            HashMap::from([("session-id".to_owned(), Value::from(session))]),
        );
    }
    let start_time = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|stat| {
            // Le nom du programme peut contenir des espaces : on compte après « ) ».
            let rest = &stat[stat.rfind(')')? + 2..];
            rest.split_whitespace().nth(19)?.parse::<u64>().ok()
        })
        .unwrap_or(0);
    (
        "unix-process".to_owned(),
        HashMap::from([
            ("pid".to_owned(), Value::from(std::process::id())),
            ("start-time".to_owned(), Value::from(start_time)),
        ]),
    )
}

/// Vérifie le mot de passe par l'auxiliaire de polkit, qui confirme lui-même la demande
/// (`cookie`) auprès de polkit s'il est bon.
fn check(user: &str, cookie: &str, password: &str) -> bool {
    let Some(helper) = HELPERS.iter().find(|path| std::path::Path::new(path).exists()) else {
        tracing::warn!("polkit-agent-helper-1 introuvable");
        return false;
    };
    let Ok(mut child) = Command::new(helper)
        .arg(user)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let (Some(mut input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
        return false;
    };
    // Les versions récentes lisent la demande sur l'entrée, les anciennes en argument.
    let _ = writeln!(input, "{cookie}");
    let mut accepted = false;
    for line in BufReader::new(output).lines().map_while(Result::ok) {
        if line.starts_with("PAM_PROMPT_ECHO_OFF") || line.starts_with("PAM_PROMPT_ECHO_ON") {
            if writeln!(input, "{password}").is_err() {
                break;
            }
        } else if line == "SUCCESS" {
            accepted = true;
            break;
        } else if line == "FAILURE" {
            break;
        }
    }
    drop(input);
    let _ = child.wait();
    accepted
}

/// Efface le mot de passe sur place avant de le libérer.
fn wipe(password: &mut String) {
    let zeros = "\0".repeat(password.len());
    password.replace_range(.., &zeros);
    password.clear();
}

impl Ferrite {
    fn on_polkit_event(&mut self, event: Event) {
        match event {
            Event::Begin {
                cookie,
                message,
                user,
                reply,
            } => {
                tracing::info!(%user, "authentification administrateur demandée");
                self.auth.push_back(Auth {
                    message,
                    user,
                    cookie,
                    password: String::new(),
                    cursor: 0,
                    checking: false,
                    refused: false,
                    reply,
                });
                self.restart_caret();
            }
            Event::Cancel(cookie) => {
                if let Some(index) = self.auth.iter().position(|auth| auth.cookie == cookie)
                    && let Some(mut auth) = self.auth.remove(index)
                {
                    wipe(&mut auth.password);
                    let _ = auth.reply.try_send(false);
                }
            }
            Event::Checked { cookie, accepted } => {
                let Some(index) = self.auth.iter().position(|auth| auth.cookie == cookie) else {
                    return;
                };
                if accepted {
                    if let Some(auth) = self.auth.remove(index) {
                        let _ = auth.reply.try_send(true);
                    }
                } else if let Some(auth) = self.auth.get_mut(index) {
                    auth.checking = false;
                    auth.refused = true;
                    self.restart_caret();
                }
            }
        }
    }

    /// Change le mot de passe demandé (pas pendant sa vérification), puis remet le curseur
    /// visible.
    fn auth_edit(&mut self, change: impl FnOnce(&mut String, &mut usize)) {
        if let Some(auth) = self.auth.front_mut().filter(|auth| !auth.checking) {
            change(&mut auth.password, &mut auth.cursor);
            self.restart_caret();
        }
    }

    pub fn auth_type(&mut self, character: char) {
        self.auth_edit(|password, cursor| text_field::insert(password, cursor, character));
        if let Some(auth) = self.auth.front_mut().filter(|auth| !auth.checking) {
            auth.refused = false;
        }
    }

    /// Retour arrière : le caractère avant le curseur.
    pub fn auth_erase(&mut self) {
        self.auth_edit(text_field::erase);
    }

    /// Suppr : le caractère après le curseur.
    pub fn auth_delete(&mut self) {
        self.auth_edit(text_field::delete);
    }

    /// ← → Début Fin.
    pub fn auth_move(&mut self, step: isize) {
        self.auth_edit(|password, cursor| text_field::step(password, cursor, step));
    }

    pub fn auth_submit(&mut self) {
        let Some(link) = &self.polkit_link else {
            return;
        };
        let events = link.events.clone();
        let Some(auth) = self.auth.front_mut().filter(|auth| !auth.checking) else {
            return;
        };
        if auth.password.is_empty() {
            auth.refused = true;
            return;
        }
        auth.checking = true;
        let mut password = std::mem::take(&mut auth.password);
        let (user, cookie) = (auth.user.clone(), auth.cookie.clone());
        std::thread::spawn(move || {
            let accepted = check(&user, &cookie, &password);
            wipe(&mut password);
            let _ = events.send(Event::Checked { cookie, accepted });
        });
    }

    pub fn auth_cancel(&mut self) {
        if self.auth.front().is_some_and(|auth| auth.checking) {
            return;
        }
        if let Some(mut auth) = self.auth.pop_front() {
            wipe(&mut auth.password);
            let _ = auth.reply.try_send(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compte_de_la_session_d_abord() {
        let uid = |uid: u32| {
            (
                "unix-user".to_owned(),
                HashMap::from([("uid".to_owned(), OwnedValue::from(uid))]),
            )
        };
        let own = own_uid().unwrap();
        let other = if own == 0 { 1 } else { 0 };
        let identities = vec![uid(other), uid(own)];
        assert_eq!(choose_user(&identities), user_name(own));
        assert_eq!(choose_user(&[]), None);
    }
}
