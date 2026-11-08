//! Images en ligne de l'aperçu Markdown (badges, captures hébergées) : téléchargées une
//! fois par `curl` dans un fil à part, puis gardées dans `~/.cache/ferrite/textedit-images`.
//! Le téléchargement n'a lieu que lorsque l'aperçu montre l'image.

use std::{
    collections::HashSet,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
};

use eframe::egui;

/// Taille maximale d'une image téléchargée.
const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Adresses en cours de téléchargement ou en échec (pas de nouvel essai avant le
/// prochain lancement).
static TRIED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub enum Remote {
    Ready(PathBuf),
    Loading,
    Failed,
}

pub fn is_remote(path: &str) -> bool {
    path.starts_with("https://") || path.starts_with("http://")
}

/// Fichier local d'une image en ligne ; lance le téléchargement s'il manque.
pub fn fetch(ctx: &egui::Context, url: &str) -> Remote {
    let Some(file) = cache_file(url) else {
        return Remote::Failed;
    };
    if file.is_file() {
        return Remote::Ready(file);
    }
    let mut tried = TRIED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let tried = tried.get_or_insert_with(HashSet::new);
    if !tried.insert(url.to_owned()) {
        return if failed_marker(&file).exists() {
            Remote::Failed
        } else {
            Remote::Loading
        };
    }
    let url = url.to_owned();
    let ctx = ctx.clone();
    thread::spawn(move || {
        if download(&url, &file).is_none() {
            let _ = fs::write(failed_marker(&file), "");
        }
        ctx.request_repaint();
    });
    Remote::Loading
}

fn download(url: &str, file: &Path) -> Option<()> {
    fs::create_dir_all(file.parent()?).ok()?;
    let partial = file.with_extension("part");
    let status = Command::new("curl")
        .args(["-fsSL", "--globoff", "--max-time", "20", "--max-filesize"])
        .arg(MAX_BYTES.to_string())
        .arg("-o")
        .arg(&partial)
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    if !status.success() {
        let _ = fs::remove_file(&partial);
        return None;
    }
    fs::rename(&partial, file).ok()
}

fn cache_file(url: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    Some(
        base.join("ferrite/textedit-images")
            .join(format!("{:016x}.img", hasher.finish())),
    )
}

/// Marque d'échec, distincte d'un téléchargement en cours (dans ce lancement seulement).
fn failed_marker(file: &Path) -> PathBuf {
    file.with_extension("echec")
}

/// Image PNG, JPEG ou SVG en RVBA, avec son nombre de pixels par point à l'écran (un SVG
/// est dessiné deux fois plus grand pour rester net). Rien si illisible ou démesurée.
pub fn decode(path: &Path) -> Option<(egui::ColorImage, f32)> {
    if fs::metadata(path).ok()?.len() > MAX_BYTES {
        return None;
    }
    let data = fs::read(path).ok()?;
    if is_svg(&data) {
        return decode_svg(&data);
    }
    let image = ferrite_ui::wallpaper::decode_bytes(data).ok()?;
    if image.width as usize * image.height as usize > 4096 * 4096 {
        return None;
    }
    let image =
        egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.rgba);
    Some((image, 1.0))
}

fn is_svg(data: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&data[..data.len().min(512)]).to_ascii_lowercase();
    head.contains("<svg")
}

/// Polices du système pour le texte des SVG (chargées une fois).
fn fonts() -> std::sync::Arc<resvg::usvg::fontdb::Database> {
    static FONTS: std::sync::OnceLock<std::sync::Arc<resvg::usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut database = resvg::usvg::fontdb::Database::new();
            database.load_system_fonts();
            std::sync::Arc::new(database)
        })
        .clone()
}

fn decode_svg(data: &[u8]) -> Option<(egui::ColorImage, f32)> {
    use resvg::{tiny_skia, usvg};
    let options = usvg::Options {
        fontdb: fonts(),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(data, &options).ok()?;
    let size = tree.size();
    let factor = 2.0_f32.min(4096.0 / size.width().max(size.height()));
    let (width, height) = (
        (size.width() * factor).round().max(1.0) as u32,
        (size.height() * factor).round().max(1.0) as u32,
    );
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(factor, factor),
        &mut pixmap.as_mut(),
    );
    let image = egui::ColorImage::from_rgba_premultiplied([width as usize, height as usize], pixmap.data());
    Some((image, factor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnait_un_svg() {
        assert!(is_svg(
            br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"/>"#
        ));
        assert!(!is_svg(b"\x89PNG\r\n"));
    }

    #[test]
    fn badge_svg_dessine() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="90" height="20"><rect width="90" height="20" fill="#4c1"/></svg>"##;
        let (image, factor) = decode_svg(svg).expect("SVG lisible");
        assert_eq!((image.size, factor), ([180, 40], 2.0));
    }
}
