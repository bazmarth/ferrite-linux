//! Écran de connexion (`ferrite-shell --greeter`), lancé par greetd avec l'utilisateur
//! `_greetd` : l'écran de verrouillage, avec le choix du compte. Le mot de passe est confié
//! à greetd (PAM) par sa prise `GREETD_SOCK` ; une fois accepté, greetd ouvre la session
//! `ferrite-session --greetd` du compte et le bureau de connexion se ferme.
//!
//! Protocole de greetd (`man 7 greetd-ipc`) : chaque message est une longueur sur 32 bits
//! (ordre de la machine) suivie d'un objet JSON.

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
};

use crate::clipboard::Thumbnail;

/// Commande de la session ouverte après la connexion.
const SESSION: &str = "/usr/local/bin/ferrite-session";
/// Fichier où l'écran de connexion retient le dernier compte ouvert (dossier de `_greetd`).
const LAST_USER: &str = "dernier-compte";

/// Un compte proposé à la connexion.
#[derive(Clone, Debug)]
pub struct GreeterUser {
    pub login: String,
    /// Nom complet, sinon l'identifiant avec une majuscule.
    pub name: String,
    pub avatar: Option<Thumbnail>,
}

/// Comptes proposés et compte choisi.
#[derive(Debug)]
pub struct Greeter {
    pub users: Vec<GreeterUser>,
    pub selected: usize,
}

impl Greeter {
    /// Comptes de l'ordinateur, dans l'ordre de leur création (le compte principal
    /// d'abord) ; le dernier ouvert est le premier choix.
    pub fn load() -> Self {
        let text = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let users: Vec<GreeterUser> = parse_passwd(&text)
            .into_iter()
            .map(|(login, name)| {
                let icon = Path::new("/var/lib/AccountsService/icons").join(&login);
                GreeterUser {
                    avatar: crate::desktop::panels::load_avatar(&icon),
                    login,
                    name,
                }
            })
            .collect();
        let last = last_user_file()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .map(|text| text.trim().to_owned());
        let selected = last
            .and_then(|last| users.iter().position(|user| user.login == last))
            .unwrap_or(0);
        Self { users, selected }
    }

    pub fn current(&self) -> Option<&GreeterUser> {
        self.users.get(self.selected)
    }

    /// Compte suivant (`step` 1) ou précédent (-1), en boucle.
    pub fn step(&mut self, step: isize) {
        let count = self.users.len();
        if count > 1 {
            self.selected = (self.selected as isize + step).rem_euclid(count as isize) as usize;
        }
    }
}

/// Comptes des personnes dans `/etc/passwd` : UID de 1000 à 59999 et un vrai shell, par
/// UID croissant. Rend (identifiant, nom affiché).
fn parse_passwd(text: &str) -> Vec<(String, String)> {
    let mut users: Vec<(u32, String, String)> = text
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            let [login, _, uid, _, gecos, _, shell] = fields.as_slice() else {
                return None;
            };
            let uid: u32 = uid.parse().ok()?;
            let person = (1000..60000).contains(&uid)
                && !shell.ends_with("nologin")
                && !shell.ends_with("/false")
                && !login.is_empty();
            if !person {
                return None;
            }
            let full = gecos.split(',').next().unwrap_or("").trim();
            let name = if full.is_empty() {
                let mut letters = login.chars();
                letters
                    .next()
                    .map(|first| first.to_uppercase().chain(letters).collect())
                    .unwrap_or_default()
            } else {
                full.to_owned()
            };
            Some((uid, (*login).to_owned(), name))
        })
        .collect();
    users.sort_by_key(|(uid, _, _)| *uid);
    users.into_iter().map(|(_, login, name)| (login, name)).collect()
}

fn last_user_file() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(LAST_USER))
}

/// Ouvre la session de `login` avec `password` : `Ok(true)` si greetd l'a acceptée et va
/// la lancer, `Ok(false)` si le mot de passe est refusé. Bloquant : dans un fil à part.
pub fn login(login: &str, password: &str) -> Result<bool, String> {
    let socket = std::env::var_os("GREETD_SOCK").ok_or("greetd absent (GREETD_SOCK)")?;
    let mut stream = UnixStream::connect(socket).map_err(|error| format!("greetd injoignable : {error}"))?;
    let accepted = authenticate(&mut stream, login, password)?;
    if !accepted {
        return Ok(false);
    }
    let start = format!(
        r#"{{"type":"start_session","cmd":["{}","--greetd"],"env":["XDG_SESSION_TYPE=wayland","XDG_SESSION_DESKTOP=ferrite","XDG_CURRENT_DESKTOP=Ferrite"]}}"#,
        escape(SESSION)
    );
    send(&mut stream, &start)?;
    let reply = receive(&mut stream)?;
    if field(&reply, "type").as_deref() != Some("success") {
        return Err(field(&reply, "description").unwrap_or(reply));
    }
    if let Some(file) = last_user_file() {
        let _ = std::fs::write(file, login);
    }
    Ok(true)
}

/// Création de la session puis réponses aux questions de PAM.
fn authenticate(stream: &mut UnixStream, login: &str, password: &str) -> Result<bool, String> {
    send(
        stream,
        &format!(r#"{{"type":"create_session","username":"{}"}}"#, escape(login)),
    )?;
    loop {
        let reply = receive(stream)?;
        match field(&reply, "type").as_deref() {
            Some("success") => return Ok(true),
            Some("auth_message") => {
                let answer = match field(&reply, "auth_message_type").as_deref() {
                    // Mot de passe (et toute autre question cachée).
                    Some("secret") => {
                        let mut message = format!(
                            r#"{{"type":"post_auth_message_response","response":"{}"}}"#,
                            escape(password)
                        );
                        let result = send(stream, &message);
                        wipe(&mut message);
                        result?;
                        continue;
                    }
                    // Question visible : seul l'identifiant peut être demandé.
                    Some("visible") => format!(
                        r#"{{"type":"post_auth_message_response","response":"{}"}}"#,
                        escape(login)
                    ),
                    // Information ou erreur de PAM : simple accusé de réception.
                    _ => r#"{"type":"post_auth_message_response"}"#.to_owned(),
                };
                send(stream, &answer)?;
            }
            _ => {
                // Refus : la session commencée est abandonnée, pour pouvoir réessayer.
                let _ = send(stream, r#"{"type":"cancel_session"}"#);
                let _ = receive(stream);
                if field(&reply, "error_type").as_deref() == Some("auth_error") {
                    return Ok(false);
                }
                return Err(field(&reply, "description").unwrap_or(reply));
            }
        }
    }
}

fn send(stream: &mut UnixStream, message: &str) -> Result<(), String> {
    let length = u32::try_from(message.len()).map_err(|_| "message trop long".to_owned())?;
    stream
        .write_all(&length.to_ne_bytes())
        .and_then(|()| stream.write_all(message.as_bytes()))
        .map_err(|error| format!("greetd : {error}"))
}

fn receive(stream: &mut UnixStream) -> Result<String, String> {
    let mut length = [0u8; 4];
    stream
        .read_exact(&mut length)
        .map_err(|error| format!("greetd : {error}"))?;
    let length = u32::from_ne_bytes(length) as usize;
    if length > 1 << 20 {
        return Err("greetd : réponse trop longue".to_owned());
    }
    let mut data = vec![0u8; length];
    stream
        .read_exact(&mut data)
        .map_err(|error| format!("greetd : {error}"))?;
    String::from_utf8(data).map_err(|_| "greetd : réponse illisible".to_owned())
}

/// Chaîne JSON (sans les guillemets).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Valeur de la clé `key` (chaîne) dans un objet JSON à un seul niveau, comme ceux de
/// greetd.
fn field(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"");
    let mut rest = &json[json.find(&pattern)? + pattern.len()..];
    rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let mut chars = rest.strip_prefix('"')?.chars();
    let mut value = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(value),
            '\\' => match chars.next()? {
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                'u' => {
                    let code: String = chars.by_ref().take(4).collect();
                    value.push(char::from_u32(u32::from_str_radix(&code, 16).ok()?).unwrap_or('?'));
                }
                other => value.push(other),
            },
            c => value.push(c),
        }
    }
    None
}

/// Efface un texte de la mémoire.
fn wipe(text: &mut String) {
    let zeros = "\0".repeat(text.len());
    text.replace_range(.., &zeros);
    text.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comptes_des_personnes_seulement() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                      _greetd:x:110:115::/var/lib/greetd:/usr/sbin/nologin\n\
                      bazoum:x:1000:1000:Marthy Bazoum,,,:/home/bazoum:/bin/bash\n\
                      baz:x:1001:1001::/home/baz:/bin/bash\n\
                      nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n\
                      ancien:x:1002:1002::/home/ancien:/bin/false";
        assert_eq!(
            parse_passwd(passwd),
            [
                ("bazoum".to_owned(), "Marthy Bazoum".to_owned()),
                ("baz".to_owned(), "Baz".to_owned()),
            ]
        );
    }

    #[test]
    fn choix_du_compte_en_boucle() {
        let user = |login: &str| GreeterUser {
            login: login.to_owned(),
            name: login.to_owned(),
            avatar: None,
        };
        let mut greeter = Greeter {
            users: vec![user("a"), user("b"), user("c")],
            selected: 0,
        };
        greeter.step(-1);
        assert_eq!(greeter.selected, 2);
        greeter.step(1);
        assert_eq!(greeter.selected, 0);
    }

    #[test]
    fn json_de_greetd() {
        let reply =
            r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Mot de passe : "}"#;
        assert_eq!(field(reply, "type").as_deref(), Some("auth_message"));
        assert_eq!(field(reply, "auth_message_type").as_deref(), Some("secret"));
        assert_eq!(field(reply, "absent"), None);
        let error = r#"{ "type" : "error", "error_type":"auth_error", "description":"pam \"refus\"\n" }"#;
        assert_eq!(field(error, "error_type").as_deref(), Some("auth_error"));
        assert_eq!(field(error, "description").as_deref(), Some("pam \"refus\"\n"));
        assert_eq!(escape("a\"b\\c\u{1}"), "a\\\"b\\\\c\\u0001");
    }

    /// Faux greetd : vérifie l'échange complet, mot de passe juste puis faux.
    #[test]
    fn echange_avec_greetd() {
        fn serve(stream: &mut UnixStream, good: bool) {
            let create = receive(stream).unwrap();
            assert!(create.contains(r#""username":"baz""#));
            send(
                stream,
                r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#,
            )
            .unwrap();
            let answer = receive(stream).unwrap();
            assert_eq!(field(&answer, "response").as_deref(), Some("s3cr\"et"));
            if good {
                send(stream, r#"{"type":"success"}"#).unwrap();
            } else {
                send(
                    stream,
                    r#"{"type":"error","error_type":"auth_error","description":"refusé"}"#,
                )
                .unwrap();
                assert!(receive(stream).unwrap().contains("cancel_session"));
                send(stream, r#"{"type":"success"}"#).unwrap();
            }
        }
        for good in [true, false] {
            let (mut client, mut server) = UnixStream::pair().unwrap();
            let thread = std::thread::spawn(move || serve(&mut server, good));
            assert_eq!(authenticate(&mut client, "baz", "s3cr\"et"), Ok(good));
            thread.join().unwrap();
        }
    }
}
