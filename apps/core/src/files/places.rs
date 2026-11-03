//! Emplacements de la barre latérale qui ne sont pas des dossiers : fichiers récents
//! (liste partagée `recently-used.xbel` des applications), étiquettes, et espace des disques.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use ferrite_system::command;

/// Étiquettes proposées, comme sur la planche.
pub const TAGS: [&str; 3] = ["Travail", "Personnel", "Important"];

/// Nombre maximal de fichiers récents affichés.
const RECENT_LIMIT: usize = 40;

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Fichiers récemment ouverts qui existent encore, du plus récent au plus ancien.
pub fn recent() -> Vec<PathBuf> {
    let Ok(text) = fs::read_to_string(data_home().join("recently-used.xbel")) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = parse_recent(&text)
        .into_iter()
        .filter(|(_, path)| path.exists())
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.dedup_by(|a, b| a.1 == b.1);
    found
        .into_iter()
        .take(RECENT_LIMIT)
        .map(|(_, path)| path)
        .collect()
}

/// (date de dernière visite, chemin) des signets `file://` d'un `recently-used.xbel`.
pub fn parse_recent(text: &str) -> Vec<(String, PathBuf)> {
    text.split("<bookmark ")
        .skip(1)
        .filter_map(|bookmark| {
            let attribute = |name: &str| {
                let start = bookmark.find(&format!("{name}=\""))? + name.len() + 2;
                let end = bookmark[start..].find('"')? + start;
                Some(&bookmark[start..end])
            };
            let href = attribute("href")?.strip_prefix("file://")?;
            let when = attribute("visited")
                .or_else(|| attribute("modified"))
                .unwrap_or_default();
            Some((when.to_owned(), PathBuf::from(unescape(href))))
        })
        .collect()
}

/// Décode les `%XX` d'une adresse et les entités XML courantes.
fn unescape(text: &str) -> String {
    let text = text
        .replace("&amp;", "&")
        .replace("&apos;", "'")
        .replace("&quot;", "\"");
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(value) = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(value);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Étiquettes des fichiers, gardées dans `~/.config/ferrite/etiquettes` : une ligne
/// « étiquette<TAB>chemin » par fichier étiqueté.
#[derive(Debug, Default)]
pub struct Tags {
    by_path: BTreeMap<PathBuf, Vec<String>>,
}

fn tags_path() -> PathBuf {
    ferrite_system::config::Settings::path().with_file_name("etiquettes")
}

impl Tags {
    pub fn load() -> Self {
        let mut tags = Tags::default();
        if let Ok(text) = fs::read_to_string(tags_path()) {
            for line in text.lines() {
                if let Some((tag, path)) = line.split_once('\t') {
                    tags.by_path
                        .entry(PathBuf::from(path))
                        .or_default()
                        .push(tag.to_owned());
                }
            }
        }
        tags
    }

    fn save(&self) {
        let text: String = self
            .by_path
            .iter()
            .flat_map(|(path, tags)| tags.iter().map(move |tag| format!("{tag}\t{}\n", path.display())))
            .collect();
        let path = tags_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, text);
    }

    pub fn of(&self, path: &Path) -> &[String] {
        self.by_path.get(path).map_or(&[], Vec::as_slice)
    }

    pub fn toggle(&mut self, path: &Path, tag: &str) {
        let tags = self.by_path.entry(path.to_path_buf()).or_default();
        match tags.iter().position(|known| known == tag) {
            Some(index) => {
                tags.remove(index);
            }
            None => tags.push(tag.to_owned()),
        }
        self.by_path.retain(|_, tags| !tags.is_empty());
        self.save();
    }

    /// Fichiers qui portent `tag` et existent encore.
    pub fn tagged(&self, tag: &str) -> Vec<PathBuf> {
        self.by_path
            .iter()
            .filter(|(path, tags)| tags.iter().any(|known| known == tag) && path.exists())
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Un fichier déplacé ou renommé garde ses étiquettes.
    pub fn moved(&mut self, from: &Path, to: &Path) {
        if let Some(tags) = self.by_path.remove(from) {
            self.by_path.insert(to.to_path_buf(), tags);
            self.save();
        }
    }
}

/// Espace libre et taille du système de fichiers qui contient `path`, en octets.
pub fn disk_usage(path: &Path) -> Option<(u64, u64)> {
    let output = command::run("df", &["-B1", "--output=avail,size", &path.to_string_lossy()]).ok()?;
    let mut fields = output.lines().nth(1)?.split_whitespace();
    let free = fields.next()?.parse().ok()?;
    let total = fields.next()?.parse().ok()?;
    Some((free, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fichiers_recents_du_xbel() {
        let text = r#"<xbel>
  <bookmark href="file:///home/moi/Documents/rapport%20final.pdf" added="2026-11-01T10:00:00Z" modified="2026-11-01T10:00:00Z" visited="2026-11-02T09:12:00Z">
  </bookmark>
  <bookmark href="https://example.org" visited="2026-11-03T00:00:00Z"></bookmark>
  <bookmark href="file:///tmp/caf%C3%A9.txt" visited="2026-11-03T08:00:00Z"></bookmark>
</xbel>"#;
        let found = parse_recent(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].1, PathBuf::from("/home/moi/Documents/rapport final.pdf"));
        assert_eq!(found[1].1, PathBuf::from("/tmp/café.txt"));
        assert_eq!(found[1].0, "2026-11-03T08:00:00Z");
    }
}
