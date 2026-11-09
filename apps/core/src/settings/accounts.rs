//! Paramètres → Comptes : les comptes de l'ordinateur, en ajouter, en supprimer, changer
//! un mot de passe, un nom complet ou le droit d'administrer.
//!
//! Tout passe par AccountsService (`org.freedesktop.Accounts`, bus système). Chaque
//! changement, sauf le nom complet et l'image de son propre compte, demande le mot de passe
//! administrateur à l'agent polkit du bureau ; Paramètres ne vérifie jamais lui-même un
//! mot de passe. Les mots de passe partent déjà chiffrés (`crypt`, méthode par défaut du
//! système) : jamais en clair sur le bus.
//!
//! Image de profil : choisie avec le sélecteur de fichiers (`ferrite-apps choisir`),
//! recadrée en carré de 256 px, puis confiée à AccountsService, qui en garde la copie
//! (`/var/lib/AccountsService/icons/IDENTIFIANT`).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::SystemTime,
};

use eframe::egui;
use zbus::{
    blocking::{Connection, Proxy},
    proxy::MethodFlags,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

use super::{
    Page, SettingsState,
    rows::{self, Group, Lead, Row, group},
};

const SERVICE: &str = "org.freedesktop.Accounts";
const ROOT: &str = "/org/freedesktop/Accounts";
const USER: &str = "org.freedesktop.Accounts.User";
/// Longueur minimale d'un mot de passe, en caractères.
const MIN_PASSWORD: usize = 6;

/// Un compte de l'ordinateur (sans les comptes du système).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    path: String,
    uid: u64,
    login: String,
    full: String,
    admin: bool,
    /// Image de profil (chemin donné par AccountsService ; peut ne pas exister).
    icon: String,
}

impl Account {
    fn name(&self) -> &str {
        if self.full.trim().is_empty() {
            &self.login
        } else {
            &self.full
        }
    }
}

/// Formulaire ouvert dans la page (un seul à la fois).
#[derive(Debug, Default)]
enum Form {
    #[default]
    None,
    Add {
        full: String,
        login: String,
        /// L'identifiant a été tapé : il ne suit plus le nom complet.
        login_typed: bool,
        password: String,
        confirm: String,
        admin: bool,
    },
    Password {
        path: String,
        login: String,
        own: bool,
        old: String,
        password: String,
        confirm: String,
    },
    Delete {
        uid: u64,
        name: String,
        remove_files: bool,
    },
}

#[derive(Default)]
pub struct AccountsState {
    accounts: Option<Result<Vec<Account>, String>>,
    loading: Option<Receiver<Result<Vec<Account>, String>>>,
    /// Changement en cours (le mot de passe administrateur peut être demandé).
    job: Option<Receiver<Result<String, String>>>,
    message: Option<Result<String, String>>,
    form: Form,
    /// Nom complet de son compte en cours de saisie.
    full_name: Option<String>,
    /// Ce qu'attend le changement en cours (sous la page, à côté de la roue).
    waiting: &'static str,
    /// Images de profil déjà chargées, par chemin, avec la date du fichier lu.
    pictures: HashMap<String, (Option<SystemTime>, Option<egui::TextureHandle>)>,
}

impl AccountsState {
    /// Pastille d'un compte : son image de profil, sinon le symbole du compte.
    fn lead(&mut self, ctx: &egui::Context, account: &Account) -> Lead {
        if account.icon.is_empty() {
            return Lead::Tile("glyph-account", Page::Accounts.color());
        }
        let modified = std::fs::metadata(&account.icon)
            .and_then(|metadata| metadata.modified())
            .ok();
        let fresh = self
            .pictures
            .get(&account.icon)
            .is_some_and(|(seen, _)| *seen == modified);
        if !fresh {
            let texture = modified
                .and_then(|_| load_picture(Path::new(&account.icon)))
                .map(|image| {
                    ctx.load_texture(
                        format!("profil-{}", account.login),
                        image,
                        egui::TextureOptions::LINEAR,
                    )
                });
            self.pictures.insert(account.icon.clone(), (modified, texture));
        }
        match self.pictures.get(&account.icon) {
            Some((_, Some(texture))) => Lead::Picture(texture.id()),
            _ => Lead::Tile("glyph-account", Page::Accounts.color()),
        }
    }

    fn has_picture(&self, account: &Account) -> bool {
        matches!(self.pictures.get(&account.icon), Some((_, Some(_))))
    }
}

/// Ce que la page demande, une fois dessinée.
enum Action {
    Rename(String, String),
    SetAdmin(String, bool),
    Create {
        full: String,
        login: String,
        admin: bool,
        password: String,
    },
    Password {
        path: String,
        own: bool,
        old: String,
        password: String,
    },
    Delete(u64, bool),
    /// Choisir une image de profil (chemin AccountsService, identifiant).
    Picture(String, String),
    NoPicture(String),
}

impl SettingsState {
    pub(super) fn accounts_page(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.accounts_receive(&ctx);
        let (_, me) = self.user.clone();
        let state = &mut self.accounts;
        let busy = state.job.is_some();
        let list = match &state.accounts {
            Some(Ok(list)) => list.clone(),
            Some(Err(error)) => {
                let error = error.clone();
                group(ui, &mut self.icons, "Comptes", |rows| {
                    rows.text(Row::new("Comptes illisibles").hint(&error));
                });
                return;
            }
            None => {
                ui.add(egui::Spinner::new().size(18.0));
                return;
            }
        };
        let admins = list.iter().filter(|account| account.admin).count();
        let mine = list.iter().find(|account| account.login == me).cloned();
        let others: Vec<Account> = list
            .iter()
            .filter(|account| account.login != me)
            .cloned()
            .collect();
        let mut action = None;

        let leads: Vec<Lead> = list.iter().map(|account| state.lead(&ctx, account)).collect();
        let lead_of = |account: &Account| {
            list.iter()
                .position(|other| other.path == account.path)
                .map_or(Lead::None, |index| leads[index])
        };
        let face = if mine.is_some() { self.face_summary() } else { "" };
        let mut open_face = false;
        let state = &mut self.accounts;
        if let Some(account) = &mine {
            let has_picture = state.has_picture(account);
            let kind = if account.admin {
                "Administrateur"
            } else {
                "Standard"
            };
            let hint = format!("{} · {kind}", account.login);
            let full_name = state.full_name.get_or_insert_with(|| account.full.clone());
            let form = &mut state.form;
            group(ui, &mut self.icons, "Mon compte", |rows| {
                rows.text(
                    Row::new(account.name())
                        .hint(&hint)
                        .bold(true)
                        .lead(lead_of(account)),
                );
                rows.row(
                    Row::new("Image de profil").hint("Une photo PNG ou JPEG, recadrée en carré"),
                    |ui| {
                        if ui.add_enabled(!busy, egui::Button::new("Choisir…")).clicked() {
                            action = Some(Action::Picture(account.path.clone(), account.login.clone()));
                        }
                        if has_picture && ui.add_enabled(!busy, egui::Button::new("Retirer")).clicked() {
                            action = Some(Action::NoPicture(account.path.clone()));
                        }
                    },
                );
                rows.row(Row::new("Nom complet"), |ui| {
                    let changed = full_name.trim() != account.full.trim() && !full_name.trim().is_empty();
                    if ui
                        .add_enabled(changed && !busy, egui::Button::new("Enregistrer"))
                        .clicked()
                    {
                        action = Some(Action::Rename(account.path.clone(), full_name.trim().to_owned()));
                    }
                    ui.add(egui::TextEdit::singleline(full_name).desired_width(220.0));
                });
                let open = matches!(form, Form::Password { own: true, .. });
                rows.row(
                    Row::new("Mot de passe").hint("Celui de la connexion et du verrouillage"),
                    |ui| {
                        if !open && ui.add_enabled(!busy, egui::Button::new("Changer…")).clicked() {
                            *form = Form::Password {
                                path: account.path.clone(),
                                login: account.login.clone(),
                                own: true,
                                old: String::new(),
                                password: String::new(),
                                confirm: String::new(),
                            };
                        }
                    },
                );
                if open {
                    password_form(rows, form, busy, &mut action);
                }
                rows.row(Row::new("Déverrouillage par visage").hint(face), |ui| {
                    if ui.button("Configurer…").clicked() {
                        open_face = true;
                    }
                });
            });
        }
        if open_face {
            self.go(Page::Privacy);
            return;
        }
        let state = &mut self.accounts;

        let form = &mut state.form;
        group(ui, &mut self.icons, "Autres comptes", |rows| {
            if others.is_empty() {
                rows.text(Row::new("Aucun autre compte").hint("Chacun garde ses fichiers et ses réglages"));
            }
            for account in &others {
                let kind = if account.admin {
                    "Administrateur"
                } else {
                    "Standard"
                };
                let hint = format!("{} · {kind}", account.login);
                rows.row(
                    Row::new(account.name()).hint(&hint).lead(lead_of(account)),
                    |ui| {
                        if ui.add_enabled(!busy, egui::Button::new("Supprimer…")).clicked() {
                            *form = Form::Delete {
                                uid: account.uid,
                                name: account.name().to_owned(),
                                remove_files: false,
                            };
                        }
                        if ui
                            .add_enabled(!busy, egui::Button::new("Image…"))
                            .on_hover_text("Choisir son image de profil")
                            .clicked()
                        {
                            action = Some(Action::Picture(account.path.clone(), account.login.clone()));
                        }
                        if ui
                            .add_enabled(!busy, egui::Button::new("Mot de passe…"))
                            .clicked()
                        {
                            *form = Form::Password {
                                path: account.path.clone(),
                                login: account.login.clone(),
                                own: false,
                                old: String::new(),
                                password: String::new(),
                                confirm: String::new(),
                            };
                        }
                    },
                );
                let mut admin = account.admin;
                // Le dernier administrateur le reste : sans lui, plus personne ne pourrait
                // rien changer.
                let last = account.admin && admins <= 1;
                let label = format!("Administrateur · {}", account.login);
                let (changed, _) = rows.row(
                    Row::new(&label).hint("Installe des logiciels, gère les comptes et le système"),
                    |ui| {
                        ui.add_enabled_ui(!busy && !last, |ui| rows::switch(ui, &mut admin))
                            .inner
                    },
                );
                if changed {
                    action = Some(Action::SetAdmin(account.path.clone(), admin));
                }
                match form {
                    Form::Password {
                        login, own: false, ..
                    } if *login == account.login => {
                        password_form(rows, form, busy, &mut action);
                    }
                    Form::Delete { uid, .. } if *uid == account.uid => {
                        delete_form(rows, form, busy, &mut action);
                    }
                    _ => {}
                }
            }
            if matches!(form, Form::Add { .. }) {
                add_form(rows, form, &list, busy, &mut action);
            } else {
                rows.row(Row::new(""), |ui| {
                    if ui
                        .add_enabled(!busy, egui::Button::new("Ajouter un compte…"))
                        .clicked()
                    {
                        *form = Form::Add {
                            full: String::new(),
                            login: String::new(),
                            login_typed: false,
                            password: String::new(),
                            confirm: String::new(),
                            admin: false,
                        };
                    }
                });
            }
        });

        if busy {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(state.waiting);
            });
        }
        match &state.message {
            Some(Ok(text)) => {
                ui.colored_label(crate::style::success(), text);
            }
            Some(Err(text)) => {
                ui.colored_label(crate::style::error(), text);
            }
            None => {}
        }
        ui.add_space(8.0);
        crate::kit::note(
            ui,
            "Ajouter, supprimer un compte ou changer un mot de passe demande le mot de passe \
             d’un administrateur. Chaque compte a son dossier personnel, ses réglages et ses \
             fichiers ; un compte standard ne peut pas modifier le système.",
        );

        if let Some(action) = action {
            self.accounts_run(&ctx, action);
        }
    }

    fn accounts_receive(&mut self, ctx: &egui::Context) {
        let state = &mut self.accounts;
        if state.accounts.is_none() && state.loading.is_none() {
            state.loading = Some(spawn(ctx, list));
        }
        if let Some(result) = state
            .loading
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            state.accounts = Some(result);
            state.loading = None;
        }
        if let Some(result) = state.job.as_ref().and_then(|receiver| receiver.try_recv().ok()) {
            state.job = None;
            if result.is_ok() {
                state.form = Form::None;
                state.full_name = None;
            }
            // Choix d'image annulé : rien à dire.
            state.message = Some(result).filter(|result| !matches!(result, Ok(text) if text.is_empty()));
            // Relecture : liste, et son propre nom complet.
            state.loading = Some(spawn(ctx, list));
            self.user = super::current_user();
        }
        if self.accounts.loading.is_some() || self.accounts.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    fn accounts_run(&mut self, ctx: &egui::Context, action: Action) {
        let state = &mut self.accounts;
        state.message = None;
        state.waiting = match action {
            Action::Picture(..) => "Choix de l’image…",
            Action::Rename(..) | Action::NoPicture(_) => "Enregistrement…",
            _ => "Mot de passe administrateur demandé…",
        };
        state.job = Some(spawn(ctx, move || match action {
            Action::Rename(path, full) => {
                set_real_name(&path, &full).map(|()| "Nom complet enregistré".to_owned())
            }
            Action::SetAdmin(path, admin) => set_admin(&path, admin).map(|()| {
                if admin {
                    "Le compte peut désormais administrer l’ordinateur".to_owned()
                } else {
                    "Le compte est désormais standard".to_owned()
                }
            }),
            Action::Create {
                full,
                login,
                admin,
                mut password,
            } => {
                let result = create(&full, &login, admin, &password);
                wipe(&mut password);
                result.map(|()| format!("Compte « {login} » ajouté"))
            }
            Action::Password {
                path,
                own,
                mut old,
                mut password,
            } => {
                let result = set_password(&path, &password);
                if result.is_ok() && own {
                    // Le trousseau suit le mot de passe de la session (il s'ouvre au
                    // déverrouillage avec lui).
                    if let Err(error) = change_keyring(&old, &password) {
                        eprintln!("trousseau non changé : {error}");
                    }
                }
                wipe(&mut old);
                wipe(&mut password);
                result.map(|()| "Mot de passe changé".to_owned())
            }
            Action::Delete(uid, remove_files) => {
                delete(uid, remove_files).map(|()| "Compte supprimé".to_owned())
            }
            Action::Picture(path, login) => choose_picture(&path, &login),
            Action::NoPicture(path) => set_icon(&path, "").map(|()| "Image de profil retirée".to_owned()),
        }));
    }
}

/// Champ d'un formulaire ; renvoie vrai quand Entrée y est tapée.
fn field(rows: &mut Group<'_>, label: &str, text: &mut String, secret: bool) -> bool {
    rows.row(Row::new(label), |ui| {
        let response = ui.add(
            egui::TextEdit::singleline(text)
                .password(secret)
                .desired_width(240.0),
        );
        response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
    })
    .0
}

/// Ligne des boutons d'un formulaire : (valider, annuler).
fn buttons(
    rows: &mut Group<'_>,
    problem: Option<&str>,
    label: &str,
    busy: bool,
    danger: bool,
) -> (bool, bool) {
    let hint = problem.unwrap_or("");
    rows.row(Row::new("").hint(hint), |ui| {
        let ready = problem.is_none() && !busy;
        let fill = if danger {
            crate::style::error()
        } else {
            crate::style::accent()
        };
        let button = if ready {
            egui::Button::new(egui::RichText::new(label).color(crate::style::accent_text())).fill(fill)
        } else {
            egui::Button::new(label)
        };
        let ok = ui.add_enabled(ready, button).clicked();
        let cancel = ui.add_enabled(!busy, egui::Button::new("Annuler")).clicked();
        (ok, cancel)
    })
    .0
}

fn password_form(rows: &mut Group<'_>, form: &mut Form, busy: bool, action: &mut Option<Action>) {
    let Form::Password {
        path,
        login,
        own,
        old,
        password,
        confirm,
    } = form
    else {
        return;
    };
    let mut enter = false;
    if *own {
        enter |= field(rows, "Mot de passe actuel", old, true);
    }
    let label = if *own {
        "Nouveau mot de passe".to_owned()
    } else {
        format!("Nouveau mot de passe de {login}")
    };
    enter |= field(rows, &label, password, true);
    enter |= field(rows, "Confirmer", confirm, true);
    let mut problem = check_password(password, confirm).err();
    if *own && old.is_empty() {
        problem = Some("Tapez le mot de passe actuel");
    }
    let (ok, cancel) = buttons(rows, problem, "Changer", busy, false);
    if (ok || enter) && problem.is_none() && !busy {
        *action = Some(Action::Password {
            path: path.clone(),
            own: *own,
            old: std::mem::take(old),
            password: std::mem::take(password),
        });
        confirm.clear();
    } else if cancel {
        *form = Form::None;
    }
}

fn delete_form(rows: &mut Group<'_>, form: &mut Form, busy: bool, action: &mut Option<Action>) {
    let Form::Delete {
        uid,
        name,
        remove_files,
    } = form
    else {
        return;
    };
    let hint = if *remove_files {
        "Son dossier personnel et tous ses fichiers seront effacés"
    } else {
        "Son dossier personnel reste sur le disque"
    };
    rows.row(Row::new("Effacer aussi ses fichiers").hint(hint), |ui| {
        rows::switch(ui, remove_files);
    });
    let label = format!("Supprimer {name}");
    let (ok, cancel) = buttons(rows, None, &label, busy, true);
    if ok {
        *action = Some(Action::Delete(*uid, *remove_files));
    } else if cancel {
        *form = Form::None;
    }
}

fn add_form(
    rows: &mut Group<'_>,
    form: &mut Form,
    list: &[Account],
    busy: bool,
    action: &mut Option<Action>,
) {
    let Form::Add {
        full,
        login,
        login_typed,
        password,
        confirm,
        admin,
    } = form
    else {
        return;
    };
    let mut enter = field(rows, "Nom complet", full, false);
    if !*login_typed {
        *login = suggest_login(full);
    }
    let before = login.clone();
    enter |= field(rows, "Identifiant", login, false);
    if *login != before {
        *login_typed = true;
    }
    enter |= field(rows, "Mot de passe", password, true);
    enter |= field(rows, "Confirmer", confirm, true);
    rows.row(
        Row::new("Administrateur").hint("Installe des logiciels, gère les comptes et le système"),
        |ui| rows::switch(ui, admin),
    );
    let problem = if full.trim().is_empty() {
        Some("Tapez un nom complet")
    } else {
        check_login(login, list)
            .err()
            .or_else(|| check_password(password, confirm).err())
    };
    let (ok, cancel) = buttons(rows, problem, "Ajouter", busy, false);
    if (ok || enter) && problem.is_none() && !busy {
        *action = Some(Action::Create {
            full: full.trim().to_owned(),
            login: login.clone(),
            admin: *admin,
            password: std::mem::take(password),
        });
        confirm.clear();
    } else if cancel {
        *form = Form::None;
    }
}

// ---- Règles des saisies -------------------------------------------------------------

/// Identifiant proposé d'après le nom complet : premier mot, sans accent, en minuscules.
fn suggest_login(full: &str) -> String {
    let word = full.split_whitespace().next().unwrap_or("");
    let mut login = String::new();
    for c in word.chars().flat_map(char::to_lowercase) {
        let plain = match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => "a",
            'é' | 'è' | 'ê' | 'ë' => "e",
            'î' | 'ï' | 'í' | 'ì' => "i",
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => "o",
            'ù' | 'û' | 'ü' | 'ú' => "u",
            'ç' => "c",
            'ñ' => "n",
            'ÿ' => "y",
            'œ' => "oe",
            'æ' => "ae",
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' => {
                login.push(c);
                continue;
            }
            _ => continue,
        };
        login.push_str(plain);
    }
    let start = login
        .find(|c: char| c.is_ascii_lowercase())
        .unwrap_or(login.len());
    login[start..].chars().take(32).collect()
}

fn check_login(login: &str, list: &[Account]) -> Result<(), &'static str> {
    let mut chars = login.chars();
    match chars.next() {
        None => return Err("Tapez un identifiant"),
        Some(first) if !first.is_ascii_lowercase() => {
            return Err("L’identifiant commence par une lettre minuscule");
        }
        _ => {}
    }
    if login.len() > 32 {
        return Err("Identifiant trop long (32 caractères au plus)");
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
        return Err("Identifiant : lettres minuscules sans accent, chiffres, - et _");
    }
    if list.iter().any(|account| account.login == login) || is_system_name(login) {
        return Err("Cet identifiant est déjà pris");
    }
    Ok(())
}

/// Nom déjà pris dans `/etc/passwd` ou `/etc/group` (comptes du système compris).
fn is_system_name(login: &str) -> bool {
    ["/etc/passwd", "/etc/group"].iter().any(|file| {
        std::fs::read_to_string(file)
            .is_ok_and(|text| text.lines().any(|line| line.split(':').next() == Some(login)))
    })
}

fn check_password(password: &str, confirm: &str) -> Result<(), &'static str> {
    if password.chars().count() < MIN_PASSWORD {
        return Err("Mot de passe : 6 caractères au moins");
    }
    if password != confirm {
        return Err("Les deux mots de passe diffèrent");
    }
    Ok(())
}

/// Écrase un mot de passe en mémoire avant de le rendre.
fn wipe(password: &mut String) {
    let zeros = "\0".repeat(password.len());
    password.replace_range(.., &zeros);
    password.clear();
}

// ---- AccountsService ----------------------------------------------------------------

fn spawn<T: Send + 'static>(ctx: &egui::Context, work: impl FnOnce() -> T + Send + 'static) -> Receiver<T> {
    let (sender, receiver) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = sender.send(work());
        ctx.request_repaint();
    });
    receiver
}

/// Message pour une erreur d'AccountsService.
fn explain(error: zbus::Error) -> String {
    if let zbus::Error::MethodError(name, text, _) = &error {
        let name = name.as_str();
        if name.ends_with("PermissionDenied") || name.ends_with("NotAuthorized") {
            return "Mot de passe administrateur non donné : rien n’a changé.".to_owned();
        }
        if name.ends_with("UserExists") {
            return "Cet identifiant est déjà pris.".to_owned();
        }
        if name.ends_with("UserDoesNotExist") {
            return "Ce compte n’existe plus.".to_owned();
        }
        if name.ends_with("ServiceUnknown") {
            return "Le service des comptes (accountsservice) est absent.".to_owned();
        }
        if let Some(text) = text {
            return format!("Refusé : {text}");
        }
    }
    format!("Le service des comptes ne répond pas : {error}")
}

fn connection() -> Result<Connection, String> {
    Connection::system().map_err(|error| format!("Bus système injoignable : {error}"))
}

fn proxy<'a>(connection: &Connection, path: &'a str, interface: &'a str) -> Result<Proxy<'a>, String> {
    Proxy::new(connection, SERVICE, path, interface).map_err(explain)
}

/// Appel qui peut demander le mot de passe administrateur (agent polkit du bureau).
macro_rules! call {
    ($proxy:expr, $method:expr, $body:expr) => {
        $proxy
            .call_with_flags($method, MethodFlags::AllowInteractiveAuth.into(), $body)
            .map_err(explain)
            .and_then(|reply| reply.ok_or_else(|| "Le service des comptes n’a pas répondu.".to_owned()))
    };
}

fn list() -> Result<Vec<Account>, String> {
    let connection = connection()?;
    let root = proxy(&connection, ROOT, SERVICE)?;
    let mut paths: Vec<OwnedObjectPath> = root.call("ListCachedUsers", &()).map_err(explain)?;
    // Le compte de la session, même si AccountsService ne l'a pas encore en cache.
    if let Ok(login) = std::env::var("USER")
        && let Ok(path) = root.call::<_, _, OwnedObjectPath>("FindUserByName", &(login,))
        && !paths.contains(&path)
    {
        paths.push(path);
    }
    let mut accounts = Vec::new();
    for path in paths {
        let user = proxy(&connection, path.as_str(), USER)?;
        let system: bool = user.get_property("SystemAccount").unwrap_or(false);
        if system {
            continue;
        }
        let kind: i32 = user.get_property("AccountType").unwrap_or(0);
        accounts.push(Account {
            path: path.as_str().to_owned(),
            uid: user.get_property("Uid").map_err(explain)?,
            login: user.get_property("UserName").map_err(explain)?,
            full: user.get_property("RealName").unwrap_or_default(),
            icon: user.get_property("IconFile").unwrap_or_default(),
            admin: kind == 1,
        });
    }
    accounts.sort_by_key(|account| account.name().to_lowercase());
    Ok(accounts)
}

fn set_real_name(path: &str, full: &str) -> Result<(), String> {
    let connection = connection()?;
    call!(proxy(&connection, path, USER)?, "SetRealName", &(full,))
}

fn set_admin(path: &str, admin: bool) -> Result<(), String> {
    let connection = connection()?;
    call!(
        proxy(&connection, path, USER)?,
        "SetAccountType",
        &(i32::from(admin),)
    )
}

fn set_password(path: &str, password: &str) -> Result<(), String> {
    let hashed = crypt::hash(password).ok_or("Chiffrement du mot de passe impossible.")?;
    let connection = connection()?;
    call!(
        proxy(&connection, path, USER)?,
        "SetPassword",
        &(hashed.as_str(), "")
    )
}

fn create(full: &str, login: &str, admin: bool, password: &str) -> Result<(), String> {
    let hashed = crypt::hash(password).ok_or("Chiffrement du mot de passe impossible.")?;
    let connection = connection()?;
    let root = proxy(&connection, ROOT, SERVICE)?;
    let path: OwnedObjectPath = call!(root, "CreateUser", &(login, full, i32::from(admin)))?;
    // Même autorisation que la création (gardée quelques minutes par polkit).
    call!(
        proxy(&connection, path.as_str(), USER)?,
        "SetPassword",
        &(hashed.as_str(), "")
    )
    .map_err(|error| format!("Compte ajouté, mais sans mot de passe : {error}"))
}

fn delete(uid: u64, remove_files: bool) -> Result<(), String> {
    let connection = connection()?;
    let root = proxy(&connection, ROOT, SERVICE)?;
    let uid = i64::try_from(uid).map_err(|_| "Compte inconnu.".to_owned())?;
    call!(root, "DeleteUser", &(uid, remove_files))
}

fn set_icon(path: &str, file: &str) -> Result<(), String> {
    let connection = connection()?;
    call!(proxy(&connection, path, USER)?, "SetIconFile", &(file,))
}

/// Côté de l'image de profil enregistrée, en pixels.
const PICTURE_SIDE: u32 = 256;

/// Ouvre le sélecteur de fichiers, prépare l'image choisie et la donne à AccountsService.
/// Rend une chaîne vide si l'on annule.
fn choose_picture(path: &str, login: &str) -> Result<String, String> {
    let exe = ferrite_system::command::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(exe);
    command.args([
        "choisir",
        "--titre",
        "Image de profil",
        "--bouton",
        "Choisir",
        "--filtre",
        "Images|*.png;*.jpg;*.jpeg;*.PNG;*.JPG;*.JPEG",
    ]);
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let pictures = home.join("Images");
        command
            .arg("--dans")
            .arg(if pictures.is_dir() { pictures } else { home });
    }
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("Sélecteur de fichiers introuvable : {error}"))?;
    let chosen = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    if chosen.is_empty() {
        return Ok(String::new());
    }
    let file = prepare_picture(Path::new(&chosen), login)?;
    set_icon(path, &file.to_string_lossy())?;
    Ok("Image de profil changée".to_owned())
}

/// Image choisie, recadrée au centre en carré de 256 px, écrite en PNG dans le cache de
/// l'utilisateur (AccountsService refuse les fichiers trop lourds).
fn prepare_picture(source: &Path, login: &str) -> Result<PathBuf, String> {
    let image =
        ferrite_ui::wallpaper::decode(source).map_err(|error| format!("Image illisible : {error}"))?;
    let image = crate::viewer::shrink(&square(&image), PICTURE_SIDE);
    let mut data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut data, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer
            .write_image_data(&image.rgba)
            .map_err(|error| error.to_string())?;
        writer.finish().map_err(|error| error.to_string())?;
    }
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or("Dossier personnel introuvable.")?
        .join("ferrite");
    std::fs::create_dir_all(&cache).map_err(|error| error.to_string())?;
    let file = cache.join(format!("profil-{login}.png"));
    std::fs::write(&file, data).map_err(|error| format!("Image non enregistrée : {error}"))?;
    Ok(file)
}

/// Plus grand carré au centre de l'image.
fn square(image: &ferrite_ui::wallpaper::Image) -> ferrite_ui::wallpaper::Image {
    let side = image.width.min(image.height);
    let (left, top) = ((image.width - side) / 2, (image.height - side) / 2);
    let mut rgba = Vec::with_capacity((side * side * 4) as usize);
    for y in top..top + side {
        let start = ((y * image.width + left) * 4) as usize;
        rgba.extend_from_slice(&image.rgba[start..start + (side * 4) as usize]);
    }
    ferrite_ui::wallpaper::Image {
        width: side,
        height: side,
        rgba,
    }
}

/// Image de profil réduite pour la page (pastille de 30 px, nette en HiDPI).
fn load_picture(path: &Path) -> Option<egui::ColorImage> {
    // Au plus quelques mégaoctets : une image de profil n'est jamais plus lourde.
    if std::fs::metadata(path).ok()?.len() > 8 * 1024 * 1024 {
        return None;
    }
    let image = ferrite_ui::wallpaper::decode(path).ok()?;
    let image = crate::viewer::shrink(&square(&image), 96);
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [image.width as usize, image.height as usize],
        &image.rgba,
    ))
}

/// Change le mot de passe du trousseau par défaut de gnome-keyring, qui s'ouvre au
/// déverrouillage avec celui de la session. Sans effet si l'ancien ne lui correspond pas.
fn change_keyring(old: &str, new: &str) -> zbus::Result<()> {
    const SECRETS: &str = "org.freedesktop.secrets";
    const PATH: &str = "/org/freedesktop/secrets";
    let connection = Connection::session()?;
    let service = Proxy::new(&connection, SECRETS, PATH, "org.freedesktop.Secret.Service")?;
    let collection: OwnedObjectPath = service.call("ReadAlias", &("default",))?;
    if collection.as_str() == "/" {
        return Ok(());
    }
    // Session « plain » : les secrets ne passent que par le bus de l'utilisateur.
    let (_, session): (OwnedValue, OwnedObjectPath) =
        service.call("OpenSession", &("plain", Value::from("")))?;
    let result = Proxy::new(
        &connection,
        SECRETS,
        PATH,
        "org.gnome.keyring.InternalUnsupportedGuiltRiddenInterface",
    )?
    .call::<_, _, ()>(
        "ChangeWithMasterPassword",
        &(
            &collection,
            (&session, Vec::<u8>::new(), old.as_bytes(), "text/plain"),
            (&session, Vec::<u8>::new(), new.as_bytes(), "text/plain"),
        ),
    );
    let _ = Proxy::new(
        &connection,
        SECRETS,
        session.as_str(),
        "org.freedesktop.Secret.Session",
    )?
    .call::<_, _, ()>("Close", &());
    result
}

/// Chiffrement des mots de passe par la bibliothèque du système (libxcrypt), avec sa
/// méthode par défaut (yescrypt sur Debian 13), comme `passwd`.
#[allow(unsafe_code)]
mod crypt {
    use std::ffi::{CStr, CString, c_char, c_int, c_ulong, c_void};

    #[link(name = "crypt")]
    unsafe extern "C" {
        fn crypt_gensalt_ra(
            prefix: *const c_char,
            count: c_ulong,
            rbytes: *const c_char,
            nrbytes: c_int,
        ) -> *mut c_char;
        fn crypt_ra(
            phrase: *const c_char,
            setting: *const c_char,
            data: *mut *mut c_void,
            size: *mut c_int,
        ) -> *mut c_char;
    }

    unsafe extern "C" {
        fn free(pointer: *mut c_void);
    }

    pub fn hash(password: &str) -> Option<String> {
        let phrase = CString::new(password).ok()?;
        // SAFETY : préfixe nul (méthode par défaut) et aléa tiré par libxcrypt elle-même ;
        // la chaîne rendue est allouée par malloc et libérée ici.
        let setting = unsafe { crypt_gensalt_ra(std::ptr::null(), 0, std::ptr::null(), 0) };
        if setting.is_null() {
            return None;
        }
        let mut data: *mut c_void = std::ptr::null_mut();
        let mut size: c_int = 0;
        // SAFETY : chaînes terminées par un zéro ; `data` est alloué par crypt_ra, effacé
        // puis libéré ici ; le résultat pointe dans `data`, copié avant la libération.
        let hashed = unsafe {
            let result = crypt_ra(phrase.as_ptr(), setting, &raw mut data, &raw mut size);
            let hashed = (!result.is_null()).then(|| CStr::from_ptr(result).to_string_lossy().into_owned());
            if !data.is_null() {
                std::ptr::write_bytes(data.cast::<u8>(), 0, usize::try_from(size).unwrap_or(0));
                free(data);
            }
            free(setting.cast());
            hashed
        };
        // Échec : libxcrypt rend une chaîne qui commence par « * ».
        hashed.filter(|hashed| hashed.starts_with('$'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(login: &str) -> Account {
        Account {
            path: String::new(),
            uid: 1000,
            login: login.to_owned(),
            full: String::new(),
            admin: false,
            icon: String::new(),
        }
    }

    #[test]
    fn identifiant_propose_depuis_le_nom() {
        assert_eq!(suggest_login("Hélène Dupont"), "helene");
        assert_eq!(suggest_login("  François"), "francois");
        assert_eq!(suggest_login("Œdipe"), "oedipe");
        assert_eq!(suggest_login("42 Zoé"), "");
        assert_eq!(suggest_login(""), "");
    }

    #[test]
    fn identifiant_verifie() {
        let list = [account("marie")];
        assert!(check_login("paul", &list).is_ok());
        assert!(check_login("paul-2_b", &list).is_ok());
        assert!(check_login("", &list).is_err());
        assert!(check_login("Paul", &list).is_err());
        assert!(check_login("2paul", &list).is_err());
        assert!(check_login("pa ul", &list).is_err());
        assert!(check_login("paulé", &list).is_err());
        assert_eq!(check_login("marie", &list), Err("Cet identifiant est déjà pris"));
        // Compte du système.
        assert!(check_login("root", &list).is_err());
    }

    #[test]
    fn mot_de_passe_verifie() {
        assert!(check_password("secret1", "secret1").is_ok());
        assert!(check_password("court", "court").is_err());
        assert!(check_password("secret1", "secret2").is_err());
    }

    #[test]
    fn chiffrement_du_systeme() {
        let hashed = crypt::hash("mot de passe").expect("libxcrypt");
        assert!(hashed.starts_with('$'));
        assert!(!hashed.contains("mot de passe"));
        // Deux chiffrements du même mot de passe diffèrent (sel aléatoire).
        assert_ne!(hashed, crypt::hash("mot de passe").unwrap());
    }

    #[test]
    fn image_recadree_au_centre() {
        // 4 × 2 : colonnes 0 à 3 de valeurs 0, 1, 2, 3.
        let rgba = (0..2).flat_map(|_| (0u8..4).flat_map(|x| [x; 4])).collect();
        let image = ferrite_ui::wallpaper::Image {
            width: 4,
            height: 2,
            rgba,
        };
        let cropped = square(&image);
        assert_eq!((cropped.width, cropped.height), (2, 2));
        assert_eq!(&cropped.rgba[..8], &[1, 1, 1, 1, 2, 2, 2, 2]);
    }

    #[test]
    fn effacement_en_memoire() {
        let mut password = "secret".to_owned();
        wipe(&mut password);
        assert!(password.is_empty());
    }
}
