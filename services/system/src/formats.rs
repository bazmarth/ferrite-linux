//! Formats de fichiers et application de Ferrite qui ouvre chacun : la seule liste.
//! Fichiers (double-clic, « Ouvrir avec »), Paramètres → Applications par défaut, les
//! fichiers `.desktop` et le `mimeapps.list` de l'image la suivent ; des tests vérifient
//! qu'ils sont d'accord.

use std::path::Path;

/// Application de Ferrite capable d'ouvrir un fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    TextEdit,
    Viewer,
    Pdf,
    Video,
    Music,
}

impl Builtin {
    pub const ALL: [Self; 5] = [Self::TextEdit, Self::Viewer, Self::Pdf, Self::Video, Self::Music];

    /// Commande de `ferrite-apps` qui l'ouvre.
    pub fn command(self) -> &'static str {
        match self {
            Self::TextEdit => "textedit",
            Self::Viewer => "viewer",
            Self::Pdf => "pdf",
            Self::Video => "video",
            Self::Music => "music",
        }
    }

    /// Son fichier `.desktop`.
    pub fn desktop_id(self) -> &'static str {
        match self {
            Self::TextEdit => "ferrite-textedit.desktop",
            Self::Viewer => "ferrite-viewer.desktop",
            Self::Pdf => "ferrite-pdf.desktop",
            Self::Video => "ferrite-video.desktop",
            Self::Music => "ferrite-music.desktop",
        }
    }

    /// Nom affiché.
    pub fn name(self) -> &'static str {
        match self {
            Self::TextEdit => "TextEdit",
            Self::Viewer => "Visionneuse",
            Self::Pdf => "Lecteur PDF",
            Self::Video => "Lecteur vidéo",
            Self::Music => "Musique",
        }
    }

    /// Tous les types MIME qu'elle ouvre, sans doublon.
    pub fn mimes(self) -> Vec<&'static str> {
        let mut mimes = Vec::new();
        for format in FORMATS.iter().filter(|format| format.app == self) {
            for mime in format.mimes {
                if !mimes.contains(mime) {
                    mimes.push(*mime);
                }
            }
        }
        mimes
    }
}

/// Une extension, ses types MIME (le premier est le type de la base `shared-mime-info`
/// de Debian, les suivants ses anciens noms encore employés) et l'application qui l'ouvre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub extension: &'static str,
    pub mimes: &'static [&'static str],
    pub app: Builtin,
}

const fn format(extension: &'static str, mimes: &'static [&'static str], app: Builtin) -> Format {
    Format {
        extension,
        mimes,
        app,
    }
}

use Builtin::{Music, Pdf, TextEdit, Video, Viewer};

/// Les pages web (`.html`) restent au navigateur ; les archives sont ouvertes par
/// Fichiers lui-même. « .mp4 » va au Lecteur vidéo (presque toujours un film ou un
/// enregistrement d'écran), « .ts » aussi (flux vidéo, rarement une traduction Qt).
pub const FORMATS: &[Format] = &[
    // Textes et code.
    format("txt", &["text/plain"], TextEdit),
    format("md", &["text/markdown"], TextEdit),
    format("markdown", &["text/markdown"], TextEdit),
    format("log", &["text/x-log"], TextEdit),
    format("csv", &["text/csv"], TextEdit),
    format("conf", &["text/plain"], TextEdit),
    format("ini", &["text/plain"], TextEdit),
    format("rs", &["text/rust"], TextEdit),
    format("toml", &["application/toml"], TextEdit),
    format("json", &["application/json"], TextEdit),
    format("yaml", &["application/yaml"], TextEdit),
    format("yml", &["application/yaml"], TextEdit),
    format("sh", &["application/x-shellscript"], TextEdit),
    format("py", &["text/x-python3", "text/x-python"], TextEdit),
    format("js", &["text/javascript"], TextEdit),
    format("c", &["text/x-csrc"], TextEdit),
    format("h", &["text/x-chdr"], TextEdit),
    format("cpp", &["text/x-c++src"], TextEdit),
    format("hpp", &["text/x-c++hdr"], TextEdit),
    format("css", &["text/css"], TextEdit),
    format("xml", &["application/xml"], TextEdit),
    // Images.
    format("png", &["image/png"], Viewer),
    format("jpg", &["image/jpeg"], Viewer),
    format("jpeg", &["image/jpeg"], Viewer),
    // Documents.
    format("pdf", &["application/pdf"], Pdf),
    // Vidéos.
    format("mp4", &["video/mp4"], Video),
    format("m4v", &["video/mp4", "video/x-m4v"], Video),
    format("mkv", &["video/x-matroska"], Video),
    format("webm", &["video/webm"], Video),
    format("avi", &["video/vnd.avi", "video/x-msvideo"], Video),
    format("mov", &["video/quicktime"], Video),
    format("ogv", &["video/ogg"], Video),
    format("mpeg", &["video/mpeg"], Video),
    format("mpg", &["video/mpeg"], Video),
    format("wmv", &["video/x-ms-wmv"], Video),
    format("flv", &["video/x-flv"], Video),
    format("3gp", &["video/3gpp"], Video),
    format("ts", &["video/mp2t"], Video),
    // Musique.
    format("mp3", &["audio/mpeg"], Music),
    format("flac", &["audio/flac"], Music),
    format("ogg", &["audio/ogg", "audio/x-vorbis+ogg"], Music),
    format("oga", &["audio/ogg", "audio/x-vorbis+ogg"], Music),
    format("wav", &["audio/vnd.wave", "audio/x-wav"], Music),
    format("m4a", &["audio/mp4"], Music),
    format("aac", &["audio/aac"], Music),
    format("alac", &["audio/mp4"], Music),
];

/// Format connu de `path`, d'après son extension (sans tenir compte de la casse).
pub fn format_of(path: &Path) -> Option<&'static Format> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    FORMATS.iter().find(|format| format.extension == extension)
}

/// Application de Ferrite qui ouvre `path`, si Ferrite en a une pour ce format.
pub fn builtin_for(path: &Path) -> Option<Builtin> {
    format_of(path).map(|format| format.app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn image_file(relative: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../image/config/includes.chroot")
            .join(relative);
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    }

    #[test]
    fn chaque_extension_une_seule_fois() {
        for (index, format) in FORMATS.iter().enumerate() {
            assert_eq!(format.extension, format.extension.to_ascii_lowercase());
            assert!(
                !FORMATS[..index]
                    .iter()
                    .any(|other| other.extension == format.extension),
                "extension en double : {}",
                format.extension
            );
            assert!(!format.mimes.is_empty(), "{} sans type", format.extension);
        }
    }

    #[test]
    fn un_type_mime_une_seule_application() {
        for format in FORMATS {
            for mime in format.mimes {
                for other in FORMATS {
                    if other.mimes.contains(mime) {
                        assert_eq!(other.app, format.app, "{mime} partagé entre deux applications");
                    }
                }
            }
        }
    }

    #[test]
    fn application_d_un_fichier() {
        assert_eq!(builtin_for(Path::new("/x/Film.MKV")), Some(Video));
        assert_eq!(builtin_for(Path::new("/x/clip.mp4")), Some(Video));
        assert_eq!(builtin_for(Path::new("/x/chanson.m4a")), Some(Music));
        assert_eq!(builtin_for(Path::new("/x/guide.pdf")), Some(Pdf));
        assert_eq!(builtin_for(Path::new("/x/photo.JPEG")), Some(Viewer));
        assert_eq!(builtin_for(Path::new("/x/script.py")), Some(TextEdit));
        assert_eq!(builtin_for(Path::new("/x/page.html")), None);
        assert_eq!(builtin_for(Path::new("/x/archive.tar.gz")), None);
        assert_eq!(builtin_for(Path::new("/x/Makefile")), None);
    }

    /// Le `.desktop` de chaque application annonce tous les types qu'elle ouvre.
    #[test]
    fn fichiers_desktop_d_accord() {
        for app in Builtin::ALL {
            let text = image_file(&format!("usr/share/applications/{}", app.desktop_id()));
            assert!(
                text.contains(&format!("Exec=ferrite-apps {} %f", app.command())),
                "{} : mauvaise commande",
                app.desktop_id()
            );
            let declared: Vec<&str> = text
                .lines()
                .find_map(|line| line.strip_prefix("MimeType="))
                .unwrap_or_default()
                .split(';')
                .filter(|mime| !mime.is_empty())
                .collect();
            for mime in app.mimes() {
                assert!(
                    declared.contains(&mime),
                    "{} n'annonce pas {mime}",
                    app.desktop_id()
                );
            }
            for mime in declared {
                assert!(
                    app.mimes().contains(&mime),
                    "{} annonce {mime} en trop",
                    app.desktop_id()
                );
            }
        }
    }

    /// Le `mimeapps.list` de l'image donne chaque type à la bonne application.
    #[test]
    fn mimeapps_de_l_image_d_accord() {
        let text = image_file("etc/xdg/mimeapps.list");
        for app in Builtin::ALL {
            for mime in app.mimes() {
                assert_eq!(
                    crate::defaults::default_in(&text, mime).as_deref(),
                    Some(app.desktop_id()),
                    "{mime} dans /etc/xdg/mimeapps.list"
                );
            }
        }
    }
}
