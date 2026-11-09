//! « Envoyer par Bluetooth » : choix d'un appareil appairé, puis envoi des fichiers par
//! OBEX (profil Object Push) à travers le service `obexd` de BlueZ (paquet
//! `bluez-obexd`, sur le bus de session).
//!
//! Le téléphone ou l'ordinateur qui reçoit doit accepter chaque fichier de son côté.

use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{self, FontId, RichText};
use ferrite_system::bluetooth::{self, Device};
use zbus::{
    blocking::{Connection, MessageIterator, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

use super::{Action, Dialog, FilesState, file_name, ops::Job, paint_path_icon};
use crate::style::{self, IconStyle};

const OBEX: &str = "org.bluez.obex";
/// Temps laissé à l'autre appareil pour accepter un fichier.
const ACCEPT: Duration = Duration::from_secs(90);
/// Silence maximal pendant un envoi en cours.
const STALL: Duration = Duration::from_secs(30);

/// État du Bluetooth lu pour le dialogue.
#[derive(Debug)]
pub(super) enum Scan {
    NoAdapter,
    Off,
    /// Appareils appairés capables de recevoir des fichiers.
    Ready(Vec<Device>),
}

#[derive(Debug)]
pub(super) struct SendDialog {
    paths: Vec<PathBuf>,
    scan: Option<Scan>,
    updates: Receiver<Scan>,
    /// Allumage en cours.
    powering: bool,
}

impl SendDialog {
    pub(super) fn new(paths: Vec<PathBuf>, ctx: &egui::Context) -> Self {
        Self {
            paths,
            scan: None,
            updates: read_state(ctx, false),
            powering: false,
        }
    }
}

/// Lit l'état du Bluetooth dans un fil à part (après l'avoir allumé si `power_on`).
fn read_state(ctx: &egui::Context, power_on: bool) -> Receiver<Scan> {
    let (sender, receiver) = mpsc::channel();
    let ctx = ctx.clone();
    thread::spawn(move || {
        if power_on {
            let _ = bluetooth::set_powered(true);
        }
        let scan = match bluetooth::powered() {
            None => Scan::NoAdapter,
            Some(false) => Scan::Off,
            Some(true) => Scan::Ready(receivers(bluetooth::devices().unwrap_or_default())),
        };
        let _ = sender.send(scan);
        ctx.request_repaint();
    });
    receiver
}

/// Appareils appairés qui peuvent recevoir un fichier : ni casque, ni enceinte, ni
/// clavier, ni souris. Téléphones puis ordinateurs en tête.
fn receivers(devices: Vec<Device>) -> Vec<Device> {
    let mut list: Vec<Device> = devices
        .into_iter()
        .filter(|device| {
            device.paired && !device.icon.starts_with("audio-") && !device.icon.starts_with("input-")
        })
        .collect();
    let rank = |device: &Device| match device.icon.as_str() {
        "phone" => 0,
        "computer" => 1,
        _ => 2,
    };
    list.sort_by_key(rank);
    list
}

/// Icône d'un appareil d'après le genre qu'il annonce.
fn device_icon(device: &Device) -> &'static str {
    match device.icon.as_str() {
        "phone" => "device-phone",
        "computer" => "settings-computer",
        _ => "settings-bluetooth",
    }
}

/// Seuls les fichiers s'envoient (pas les dossiers).
pub(super) fn sendable(paths: &[PathBuf]) -> bool {
    !paths.is_empty() && paths.iter().all(|path| path.is_file())
}

impl FilesState {
    pub(super) fn bluetooth_dialog(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(Dialog::Bluetooth(dialog)) = &mut self.dialog else {
            return;
        };
        if let Ok(scan) = dialog.updates.try_recv() {
            dialog.scan = Some(scan);
            dialog.powering = false;
        }
        let icons = &mut self.icons;
        let looks = &self.looks;
        let modal = egui::Modal::new(egui::Id::unique("fichiers-bluetooth")).show(ctx, |ui| {
            ui.set_width(380.0);
            // En-tête : icône du premier fichier, titre, nom ou nombre de fichiers.
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(40.0), egui::Sense::hover());
                if let Some(first) = dialog.paths.first() {
                    paint_path_icon(looks, ui.painter(), first, false, rect);
                }
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.add_space(1.0);
                    ui.label(
                        RichText::new("Envoyer par Bluetooth")
                            .font(FontId::new(17.0, style::strong_family()))
                            .color(style::heading_text()),
                    );
                    let what = match dialog.paths.as_slice() {
                        [one] => file_name(one),
                        many => format!("{} fichiers", many.len()),
                    };
                    ui.add(egui::Label::new(RichText::new(what).color(style::text_secondary())).truncate());
                });
            });
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);
            let secondary = |ui: &mut egui::Ui, text: &str| {
                ui.add(egui::Label::new(RichText::new(text).color(style::text_secondary())).wrap());
            };
            match &dialog.scan {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        secondary(ui, "Recherche des appareils appairés…");
                    });
                }
                Some(Scan::NoAdapter) => secondary(ui, "Aucun adaptateur Bluetooth sur cet ordinateur."),
                Some(Scan::Off) => {
                    secondary(ui, "Le Bluetooth est éteint.");
                    ui.add_space(8.0);
                    if dialog.powering {
                        ui.spinner();
                    } else if ui.button("Allumer le Bluetooth").clicked() {
                        dialog.powering = true;
                        dialog.updates = read_state(ctx, true);
                    }
                }
                Some(Scan::Ready(devices)) if devices.is_empty() => {
                    secondary(
                        ui,
                        "Aucun appareil appairé ne peut recevoir de fichiers. Appairez d’abord \
                         votre téléphone ou votre ordinateur dans Paramètres › Bluetooth.",
                    );
                    ui.add_space(8.0);
                    if ui.button("Ouvrir les réglages Bluetooth").clicked() {
                        actions.push(Action::BluetoothSettings);
                    }
                }
                Some(Scan::Ready(devices)) => {
                    style::sidebar_heading(ui, "Appareils appairés");
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for device in devices {
                                if device_row(ui, icons, device).clicked() {
                                    actions.push(Action::SendBluetooth(
                                        dialog.paths.clone(),
                                        device.address.clone(),
                                        device.name.clone(),
                                    ));
                                }
                            }
                        });
                    ui.add_space(4.0);
                    secondary(ui, "L’appareil choisi devra accepter le fichier.");
                }
            }
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Annuler").clicked() {
                    actions.push(Action::CloseDialog);
                }
                let ready = dialog.scan.is_some() && !dialog.powering;
                if ui.add_enabled(ready, egui::Button::new("Actualiser")).clicked() {
                    dialog.scan = None;
                    dialog.updates = read_state(ctx, false);
                }
            });
        });
        if modal.should_close() {
            actions.push(Action::CloseDialog);
        }
    }
}

/// Une ligne : icône du genre d'appareil, nom, fond au survol.
fn device_row(ui: &mut egui::Ui, icons: &mut style::Icons, device: &Device) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 8.0, style::tint(18));
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 8.0, style::tint(10));
    }
    let icon = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + 13.0, rect.center().y),
        egui::Vec2::splat(26.0),
    );
    icons.paint(ui, device_icon(device), icon, IconStyle::Color);
    let mut job = egui::text::LayoutJob::simple_singleline(
        device.name.clone(),
        FontId::proportional(14.0),
        style::primary_text(),
    );
    let left = icon.right() + 12.0;
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.right() - left - 8.0);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        style::primary_text(),
    );
    response
}

/// Message en français pour une erreur d'`obexd`.
fn explain(error: &zbus::Error) -> String {
    let text = error.to_string();
    if text.contains("ServiceUnknown") || text.contains("org.bluez.obex was not provided") {
        "Le service d’envoi Bluetooth est absent (paquet bluez-obexd)".into()
    } else if text.contains("service record") || text.contains("Unable to find") {
        "Cet appareil n’accepte pas de fichiers par Bluetooth".into()
    } else if text.contains("Host is down") || text.contains("timed out") || text.contains("Timeout") {
        "L’appareil ne répond pas : allumez-le, déverrouillez-le et rapprochez-le".into()
    } else if text.contains("refused") || text.contains("Forbidden") {
        "L’appareil a refusé la connexion".into()
    } else {
        format!(
            "Échec Bluetooth : {}",
            text.rsplit(": ").next().unwrap_or(&text).trim()
        )
    }
}

/// Changement d'état d'un transfert, lu dans un signal `PropertiesChanged`.
#[derive(Debug, Default, PartialEq)]
struct Change {
    path: String,
    status: Option<String>,
    transferred: Option<u64>,
}

/// Envoie `paths` à l'appareil `address`, un fichier après l'autre, en suivant
/// l'avancement dans `job`. Bloquant : à appeler dans un fil à part.
pub(super) fn send(paths: &[PathBuf], address: &str, job: &Job) -> Result<(), String> {
    let sizes: Vec<u64> = paths
        .iter()
        .map(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
        .collect();
    let total: u64 = sizes.iter().sum();
    job.update(|progress| {
        progress.total_bytes = total;
        progress.current = "connexion…".into();
    });
    // Connexion à part : la fermer arrête le fil qui relaie les signaux.
    let connection = Connection::session().map_err(|error| format!("Bus de session : {error}"))?;
    let client = Proxy::new(&connection, OBEX, "/org/bluez/obex", "org.bluez.obex.Client1")
        .map_err(|error| explain(&error))?;
    let target: HashMap<&str, Value> = HashMap::from([("Target", Value::from("opp"))]);
    let session: OwnedObjectPath = client
        .call("CreateSession", &(address, target))
        .map_err(|error| explain(&error))?;
    let result = push_all(&connection, &session, paths, &sizes, job);
    let _ = client.call::<_, _, ()>("RemoveSession", &(&session,));
    let _ = connection.close();
    result
}

fn push_all(
    connection: &Connection,
    session: &OwnedObjectPath,
    paths: &[PathBuf],
    sizes: &[u64],
    job: &Job,
) -> Result<(), String> {
    let changes = watch(connection, session.as_str())?;
    let push = Proxy::new(connection, OBEX, session.as_str(), "org.bluez.obex.ObjectPush1")
        .map_err(|error| explain(&error))?;
    let mut sent = 0;
    for (path, size) in paths.iter().zip(sizes) {
        let name = file_name(path);
        job.update(|progress| progress.current = format!("{name} (en attente d’acceptation)"));
        let (transfer, _): (OwnedObjectPath, HashMap<String, OwnedValue>) = push
            .call("SendFile", &(path.to_string_lossy().as_ref(),))
            .map_err(|error| explain(&error))?;
        follow(connection, &transfer, &changes, job, &name, sent)?;
        sent += size;
        job.update(|progress| progress.done_bytes = sent);
    }
    Ok(())
}

/// Relaie les signaux `PropertiesChanged` des transferts de la session.
fn watch(connection: &Connection, session: &str) -> Result<Receiver<Change>, String> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.DBus.Properties")
        .and_then(|rule| rule.member("PropertiesChanged"))
        .and_then(|rule| rule.path_namespace(session.to_owned()))
        .map_err(|error| error.to_string())?
        .build();
    let messages =
        MessageIterator::for_match_rule(rule, connection, None).map_err(|error| explain(&error))?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        // S'arrête quand la connexion est fermée (fin de l'envoi).
        for message in messages {
            let Ok(message) = message else { break };
            let path = message
                .header()
                .path()
                .map(|path| path.to_string())
                .unwrap_or_default();
            let Ok((_, changed, _)) = message
                .body()
                .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
            else {
                continue;
            };
            let change = Change {
                path,
                status: changed
                    .get("Status")
                    .and_then(|value| String::try_from(value.clone()).ok()),
                transferred: changed
                    .get("Transferred")
                    .and_then(|value| u64::try_from(value.clone()).ok()),
            };
            if sender.send(change).is_err() {
                break;
            }
        }
    });
    Ok(receiver)
}

/// Suit un transfert jusqu'à sa fin ; l'annule si on le demande ou s'il traîne.
fn follow(
    connection: &Connection,
    transfer: &OwnedObjectPath,
    changes: &Receiver<Change>,
    job: &Job,
    name: &str,
    before: u64,
) -> Result<(), String> {
    let cancel = || {
        if let Ok(proxy) = Proxy::new(connection, OBEX, transfer.as_str(), "org.bluez.obex.Transfer1") {
            let _ = proxy.call::<_, _, ()>("Cancel", &());
        }
    };
    let mut started = false;
    let mut last = Instant::now();
    loop {
        if job.cancelled() {
            cancel();
            return Err("Annulé".into());
        }
        let limit = if started { STALL } else { ACCEPT };
        if last.elapsed() > limit {
            cancel();
            return Err(if started {
                "L’envoi s’est interrompu".into()
            } else {
                "L’appareil n’a pas accepté le fichier à temps".into()
            });
        }
        let change = match changes.recv_timeout(Duration::from_millis(200)) {
            Ok(change) => change,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return Err("Service Bluetooth arrêté".into()),
        };
        if change.path != transfer.as_str() {
            continue;
        }
        last = Instant::now();
        if let Some(done) = change.transferred {
            job.update(|progress| progress.done_bytes = before + done);
        }
        match change.status.as_deref() {
            Some("active") if !started => {
                started = true;
                job.update(|progress| progress.current = name.to_owned());
            }
            Some("complete") => return Ok(()),
            Some("error") => {
                return Err(if started {
                    "L’envoi s’est interrompu".into()
                } else {
                    "L’appareil a refusé le fichier".into()
                });
            }
            _ => {}
        }
    }
}

/// Résumé affiché une fois l'envoi terminé.
pub(super) fn done_message(paths: &[PathBuf], device: &str) -> String {
    match paths {
        [one] => format!("« {} » envoyé à {device}", file_name(one)),
        many => format!("{} fichiers envoyés à {device}", many.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, icon: &str, paired: bool) -> Device {
        Device {
            address: format!("AA:{name}"),
            name: name.into(),
            paired,
            connected: false,
            icon: icon.into(),
            battery: None,
            trusted: false,
        }
    }

    #[test]
    fn seuls_les_appareils_appaires_qui_recoivent_des_fichiers() {
        let list = receivers(vec![
            device("Casque", "audio-headset", true),
            device("Portable", "computer", true),
            device("Souris", "input-mouse", true),
            device("Inconnu", "phone", false),
            device("Pixel", "phone", true),
            device("Montre", "", true),
        ]);
        let names: Vec<&str> = list.iter().map(|device| device.name.as_str()).collect();
        assert_eq!(names, ["Pixel", "Portable", "Montre"]);
        assert_eq!(device_icon(&list[0]), "device-phone");
        assert_eq!(device_icon(&list[2]), "settings-bluetooth");
    }

    #[test]
    fn message_de_fin() {
        let one = [PathBuf::from("/tmp/photo.jpg")];
        assert_eq!(done_message(&one, "Pixel"), "« photo.jpg » envoyé à Pixel");
        let two = [PathBuf::from("/a"), PathBuf::from("/b")];
        assert_eq!(done_message(&two, "Pixel"), "2 fichiers envoyés à Pixel");
        assert!(!sendable(&[]));
        assert!(!sendable(&[PathBuf::from("/tmp")]));
    }
}
