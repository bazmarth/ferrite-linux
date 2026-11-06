//! Lien avec l'îlot du haut de l'écran (ferrite-shell, `org.ferrite.Ilot`) : l'Horloge y
//! annonce son minuteur à chaque changement de phase, et en reçoit Pause, Reprendre et
//! Arrêter. Sans bureau Ferrite, les appels échouent sans bruit.

use std::sync::mpsc::{self, Receiver};

use eframe::egui;

const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/ferrite/Ilot";
const INTERFACE: &str = "org.ferrite.Ilot";

/// Demande venue de l'îlot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Pause,
    Resume,
    Stop,
}

#[derive(Debug)]
pub struct IslandLink {
    connection: Option<zbus::blocking::Connection>,
    pub commands: Receiver<Command>,
    /// Dernière annonce : phase et reste en millisecondes.
    reported: Option<(&'static str, u64)>,
}

impl IslandLink {
    pub fn start(ctx: egui::Context) -> Self {
        let (sender, commands) = mpsc::channel();
        let connection = zbus::blocking::Connection::session().ok();
        if let Some(connection) = connection.clone() {
            let spawned = std::thread::Builder::new()
                .name("ilot".to_owned())
                .spawn(move || {
                    if let Err(error) = listen(&connection, &sender, &ctx) {
                        eprintln!("îlot non suivi : {error}");
                    }
                });
            if let Err(error) = spawned {
                eprintln!("îlot non suivi : {error}");
            }
        }
        Self {
            connection,
            commands,
            reported: None,
        }
    }

    /// Annonce le minuteur s'il a changé de phase : « marche », « pause », « fini »,
    /// « arret ».
    pub fn report(&mut self, phase: &'static str, remaining_ms: u64, total_ms: u64) {
        if self.reported.is_none() && phase == "arret" {
            return;
        }
        if self.reported.is_some_and(|(known, _)| known == phase) {
            return;
        }
        self.reported = Some((phase, remaining_ms));
        if let Some(connection) = &self.connection {
            let _ = connection.call_method(
                Some(SERVICE),
                PATH,
                Some(INTERFACE),
                "Minuteur",
                &(phase, remaining_ms, total_ms),
            );
        }
    }
}

impl Drop for IslandLink {
    /// L'Horloge se ferme : son minuteur quitte l'îlot.
    fn drop(&mut self) {
        self.report("arret", 0, 0);
    }
}

fn listen(
    connection: &zbus::blocking::Connection,
    sender: &mpsc::Sender<Command>,
    ctx: &egui::Context,
) -> zbus::Result<()> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(PATH)?
        .interface(INTERFACE)?
        .member("Commande")?
        .build();
    let messages = zbus::blocking::MessageIterator::for_match_rule(rule, connection, None)?;
    for message in messages {
        let Ok(message) = message else {
            continue;
        };
        let Ok((command,)) = message.body().deserialize::<(String,)>() else {
            continue;
        };
        let command = match command.as_str() {
            "pause" => Command::Pause,
            "reprendre" => Command::Resume,
            "arreter" => Command::Stop,
            _ => continue,
        };
        if sender.send(command).is_err() {
            return Ok(());
        }
        ctx.request_repaint();
    }
    Ok(())
}
