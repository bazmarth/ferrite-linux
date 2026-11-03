//! Bibliothèque musicale : les morceaux du dossier Musique, avec leurs étiquettes (titre,
//! artiste, album, année, numéro) et la pochette de chaque album, lus par symphonia.

use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    path::{Path, PathBuf},
};

use symphonia::core::{
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::{MetadataOptions, MetadataRevision, StandardTagKey},
    probe::Hint,
};

/// Extensions lues (les formats décodés par symphonia avec nos options).
const EXTENSIONS: [&str; 9] = ["mp3", "flac", "ogg", "oga", "wav", "m4a", "aac", "mp4", "alac"];

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: Option<u32>,
    pub number: Option<u32>,
    /// Durée en secondes (0 si inconnue).
    pub duration: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Album {
    pub title: String,
    pub artist: String,
    pub year: Option<u32>,
    /// Indices dans [`Library::tracks`], dans l'ordre des numéros.
    pub tracks: Vec<usize>,
    /// Pochette : image intégrée à un morceau, sinon `cover.jpg` (ou autre) du dossier.
    pub cover: Option<Cover>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cover {
    Embedded(PathBuf),
    File(PathBuf),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Library {
    pub folder: PathBuf,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
}

/// Dossier Musique de l'utilisateur (`~/Musique`, sinon `~/Music`, sinon `~/Musique`).
pub fn music_dir() -> PathBuf {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    ["Musique", "Music"]
        .into_iter()
        .map(|name| home.join(name))
        .find(|path| path.is_dir())
        .unwrap_or_else(|| home.join("Musique"))
}

pub fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
}

/// Parcourt `folder` et ses sous-dossiers (fichiers cachés exclus).
fn audio_files(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            audio_files(&path, found);
        } else if supported(&path) {
            found.push(path);
        }
    }
}

/// Étiquettes lues dans un fichier.
#[derive(Debug, Default)]
struct Tags {
    title: Option<String>,
    artist: Option<String>,
    album_artist: Option<String>,
    album: Option<String>,
    year: Option<u32>,
    number: Option<u32>,
    cover: bool,
}

impl Tags {
    fn read(&mut self, revision: &MetadataRevision) {
        for tag in revision.tags() {
            // Les étiquettes RIFF (WAV) finissent par un octet nul, que D-Bus refuserait.
            let value = tag
                .value
                .to_string()
                .trim_matches(|c: char| c == '\0' || c.is_whitespace())
                .to_owned();
            if value.is_empty() {
                continue;
            }
            let slot = match tag.std_key {
                Some(StandardTagKey::TrackTitle) => &mut self.title,
                Some(StandardTagKey::Artist) => &mut self.artist,
                Some(StandardTagKey::AlbumArtist) => &mut self.album_artist,
                Some(StandardTagKey::Album) => &mut self.album,
                Some(StandardTagKey::Date | StandardTagKey::OriginalDate) => {
                    self.year = self.year.or_else(|| leading_number(&value));
                    continue;
                }
                Some(StandardTagKey::TrackNumber) => {
                    self.number = self.number.or_else(|| leading_number(&value));
                    continue;
                }
                _ => continue,
            };
            slot.get_or_insert(value);
        }
        self.cover |= !revision.visuals().is_empty();
    }
}

/// « 2024-05-01 » → 2024, « 3/12 » → 3.
fn leading_number(text: &str) -> Option<u32> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Ouvre `path` avec symphonia ; renvoie le lecteur de conteneur et les métadonnées sondées.
pub fn probe(path: &Path) -> Result<symphonia::core::probe::ProbeResult, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }
    symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| format!("format non reconnu : {error}"))
}

fn read_track(path: &Path) -> Option<(Track, Option<String>, bool)> {
    let mut probed = probe(path).ok()?;
    let mut tags = Tags::default();
    if let Some(metadata) = probed.metadata.get()
        && let Some(revision) = metadata.current()
    {
        tags.read(revision);
    }
    if let Some(revision) = probed.format.metadata().current() {
        tags.read(revision);
    }
    let duration = probed
        .format
        .default_track()
        .map(|track| &track.codec_params)
        .and_then(
            |params| match (params.n_frames, params.time_base, params.sample_rate) {
                (Some(frames), Some(base), _) => {
                    let time = base.calc_time(frames);
                    Some(time.seconds as f64 + time.frac)
                }
                (Some(frames), None, Some(rate)) => Some(frames as f64 / rate as f64),
                _ => None,
            },
        )
        .unwrap_or(0.0);
    let folder_name = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned());
    let title = tags.title.unwrap_or_else(|| {
        path.file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
    });
    let track = Track {
        path: path.to_path_buf(),
        title,
        artist: tags
            .artist
            .clone()
            .unwrap_or_else(|| "Artiste inconnu".to_owned()),
        album: tags
            .album
            .or(folder_name)
            .unwrap_or_else(|| "Album inconnu".to_owned()),
        year: tags.year,
        number: tags.number,
        duration,
    };
    Some((track, tags.album_artist, tags.cover))
}

/// Image du dossier qui sert de pochette (`cover.jpg`, `folder.png`…).
fn folder_cover(folder: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(folder).ok()?;
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let extension = path
                .extension()
                .map(|extension| extension.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            matches!(stem.as_str(), "cover" | "folder" | "front" | "album" | "pochette")
                && matches!(extension.as_str(), "jpg" | "jpeg" | "png")
        })
}

/// Album en cours de constitution : année, morceaux, morceau qui porte une pochette.
type Group = (Option<u32>, Vec<usize>, Option<PathBuf>);

/// Lit toute la bibliothèque de `folder` (bloquant : à lancer dans un fil à part).
pub fn scan(folder: &Path) -> Library {
    let mut files = Vec::new();
    audio_files(folder, &mut files);
    files.sort();
    let mut tracks = Vec::new();
    // Album : (titre, artiste de l'album) → (année, morceaux, pochette intégrée).
    let mut groups: BTreeMap<(String, String), Group> = BTreeMap::new();
    for path in files {
        let Some((track, album_artist, has_cover)) = read_track(&path) else {
            continue;
        };
        let key = (
            track.album.clone(),
            album_artist.unwrap_or_else(|| track.artist.clone()),
        );
        let group = groups.entry(key).or_default();
        group.0 = group.0.or(track.year);
        group.1.push(tracks.len());
        if has_cover && group.2.is_none() {
            group.2 = Some(path.clone());
        }
        tracks.push(track);
    }
    let mut albums: Vec<Album> = groups
        .into_iter()
        .map(|((title, artist), (year, mut members, embedded))| {
            members.sort_by_key(|&index| {
                let track = &tracks[index];
                (track.number.unwrap_or(u32::MAX), track.path.clone())
            });
            let cover = embedded.map(Cover::Embedded).or_else(|| {
                tracks[members[0]]
                    .path
                    .parent()
                    .and_then(folder_cover)
                    .map(Cover::File)
            });
            Album {
                title,
                artist,
                year,
                tracks: members,
                cover,
            }
        })
        .collect();
    albums.sort_by_key(|album| (album.artist.to_lowercase(), album.title.to_lowercase()));
    Library {
        folder: folder.to_path_buf(),
        tracks,
        albums,
    }
}

/// Octets de la pochette intégrée à `path`, s'il y en a une.
pub fn embedded_cover(path: &Path) -> Option<Vec<u8>> {
    let mut probed = probe(path).ok()?;
    if let Some(metadata) = probed.metadata.get()
        && let Some(visual) = metadata.current().and_then(|revision| revision.visuals().first())
    {
        return Some(visual.data.to_vec());
    }
    probed
        .format
        .metadata()
        .current()
        .and_then(|revision| revision.visuals().first())
        .map(|visual| visual.data.to_vec())
}

/// « 3:07 », « 1:02:45 ».
pub fn format_duration(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_en_tete() {
        assert_eq!(leading_number("2024-05-01"), Some(2024));
        assert_eq!(leading_number("3/12"), Some(3));
        assert_eq!(leading_number("inconnu"), None);
    }

    #[test]
    fn durees() {
        assert_eq!(format_duration(187.4), "3:07");
        assert_eq!(format_duration(3765.0), "1:02:45");
        assert_eq!(format_duration(-1.0), "0:00");
    }

    #[test]
    fn formats_reconnus() {
        assert!(supported(Path::new("a/Morceau.FLAC")));
        assert!(supported(Path::new("b.mp3")));
        assert!(!supported(Path::new("pochette.jpg")));
    }
}
