//! Pour les scénarios de test : un faux lecteur MPRIS, et trois notifications, sans
//! dépendre d'une application installée dans la machine virtuelle.

use std::collections::HashMap;

use zbus::{
    blocking::{Connection, connection::Builder},
    interface,
    zvariant::{OwnedValue, Str, Value},
};

struct Player {
    playing: bool,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        if self.playing { "Playing" } else { "Paused" }.to_owned()
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        38_000_000
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let value = |value: Value<'_>| OwnedValue::try_from(value).expect("valeur simple");
        HashMap::from([
            ("xesam:title".to_owned(), value(Str::from("Concentration").into())),
            ("xesam:artist".to_owned(), value(vec!["Musique"].into())),
            ("mpris:length".to_owned(), value(100_000_000_i64.into())),
        ])
    }

    fn play_pause(&mut self) {
        self.playing = !self.playing;
    }

    fn previous(&self) {}

    fn next(&self) {}
}

fn main() -> zbus::Result<()> {
    let notifier = Connection::session()?;
    for index in 0..3 {
        let hints: HashMap<&str, Value<'_>> = HashMap::new();
        notifier.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "Mail",
                0_u32,
                "",
                format!("Message secret {index}"),
                "Ce contenu ne doit jamais paraître sur l'écran verrouillé",
                Vec::<&str>::new(),
                hints,
                -1_i32,
            ),
        )?;
    }
    let _player = Builder::session()?
        .name("org.mpris.MediaPlayer2.fauxlecteur")?
        .serve_at("/org/mpris/MediaPlayer2", Player { playing: true })?
        .build()?;
    loop {
        std::thread::park();
    }
}
