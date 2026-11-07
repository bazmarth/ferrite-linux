//! Vignettes des images (PNG, JPEG et SVG) et des vidéos (une image prise vers 10 % de la
//! durée, par GStreamer comme le Lecteur vidéo), décodées dans un fil à part puis gardées en
//! texture : la grille et le panneau d'aperçu les montrent à la place de l'icône du fichier.

use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use gst::prelude::*;
use gstreamer as gst;

use eframe::egui::{self, TextureHandle, TextureOptions};

/// Côté maximal d'une vignette, en pixels.
const SIDE: u32 = 360;
/// Au-delà, l'image n'est pas décodée (vignette trop longue à produire).
const MAX_BYTES: u64 = 40 * 1024 * 1024;

enum Slot {
    Pending,
    Ready(TextureHandle),
    Failed,
}

/// Fils qui produisent les vignettes. Peu nombreux et toujours les mêmes : un fil par
/// vignette gardait chacun sa réserve de mémoire (des centaines de mégaoctets sur un
/// dossier de vidéos).
const WORKERS: usize = 2;

type Done = (PathBuf, Option<egui::ColorImage>);

pub struct Thumbnails {
    slots: HashMap<PathBuf, Slot>,
    /// Demandes aux fils, créés à la première vignette.
    requests: Option<Sender<(PathBuf, egui::Context)>>,
    sender: Sender<Done>,
    receiver: Receiver<Done>,
}

impl Default for Thumbnails {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            slots: HashMap::new(),
            requests: None,
            sender,
            receiver,
        }
    }
}

/// Démarre les fils de travail ; ils s'arrêtent quand `Thumbnails` disparaît.
fn start_workers(done: &Sender<Done>) -> Sender<(PathBuf, egui::Context)> {
    let (requests, queue) = mpsc::channel::<(PathBuf, egui::Context)>();
    let queue = Arc::new(Mutex::new(queue));
    for _ in 0..WORKERS {
        let queue = Arc::clone(&queue);
        let done = done.clone();
        thread::spawn(move || {
            loop {
                let next = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .recv();
                let Ok((path, ctx)) = next else { return };
                let image = decode(&path);
                if done.send((path, image)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
    }
    requests
}

/// L'extension a-t-elle une vignette ?
pub fn supported(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "svg")
    ) || is_video(path)
}

fn is_video(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "ogv")
    )
}

/// Une vidéo à la fois : chaque lecture ouvre un décodeur complet.
static VIDEO_TURN: Mutex<()> = Mutex::new(());
/// Attente maximale d'une image (fichier abîmé, codec manquant).
const VIDEO_TIMEOUT: Duration = Duration::from_secs(8);

/// Vignette d'une vidéo, extraite par un processus à part (`ferrite-apps vignette-video`) :
/// GStreamer y ouvre ses décodeurs et ses fils, et toute leur mémoire est rendue quand il se
/// termine. Dans Fichiers même, un dossier de vidéos gardait des centaines de mégaoctets.
fn decode_video(path: &Path) -> Option<egui::ColorImage> {
    let _turn = VIDEO_TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut child = Command::new(ferrite_system::command::current_exe().ok()?)
        .arg("vignette-video")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });
    let bytes = receiver.recv_timeout(VIDEO_TIMEOUT + Duration::from_secs(2));
    if bytes.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let bytes = bytes.ok()?;
    let (width, height, rgba) = parse_frame(&bytes)?;
    Some(egui::ColorImage::from_rgba_unmultiplied([width, height], rgba))
}

/// Lit « largeur hauteur\n » puis les pixels RVBA.
fn parse_frame(bytes: &[u8]) -> Option<(usize, usize, &[u8])> {
    let newline = bytes.iter().position(|byte| *byte == b'\n')?;
    let header = std::str::from_utf8(&bytes[..newline]).ok()?;
    let (width, height) = header.split_once(' ')?;
    let (width, height): (usize, usize) = (width.parse().ok()?, height.parse().ok()?);
    let rgba = &bytes[newline + 1..];
    (width > 0 && height > 0 && rgba.len() == width.checked_mul(height)?.checked_mul(4)?)
        .then_some((width, height, rgba))
}

/// Commande `ferrite-apps vignette-video <fichier>` : écrit l'image sur la sortie
/// (« largeur hauteur\n » puis RVBA). Faux si la vidéo n'a pas pu être lue.
pub fn print_video_frame(path: &Path) -> bool {
    let Some(image) = video_frame(path) else {
        return false;
    };
    let mut out = std::io::stdout().lock();
    out.write_all(format!("{} {}\n", image.width, image.height).as_bytes())
        .and_then(|()| out.write_all(&image.rgba))
        .and_then(|()| out.flush())
        .is_ok()
}

/// Image de la vidéo vers 10 % de sa durée, réduite à `SIDE`.
fn video_frame(path: &Path) -> Option<ferrite_ui::wallpaper::Image> {
    gst::init().ok()?;
    // L'image est réduite par GStreamer (largeur `SIDE`, proportions gardées) : décodée en
    // pleine taille, une vidéo 4K coûterait des centaines de mégaoctets.
    let sink = gstreamer_app::AppSink::builder()
        .caps(
            &gst::Caps::builder("video/x-raw")
                .field("format", "RGBA")
                .field("width", SIDE as i32)
                .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
                .build(),
        )
        .max_buffers(1)
        .build();
    let scale = gst::ElementFactory::make("videoscale").build().ok()?;
    let convert = gst::ElementFactory::make("videoconvert").build().ok()?;
    let bin = gst::Bin::new();
    bin.add_many([&convert, &scale, sink.upcast_ref()]).ok()?;
    gst::Element::link_many([&convert, &scale, sink.upcast_ref()]).ok()?;
    let pad = convert.static_pad("sink")?;
    bin.add_pad(&gst::GhostPad::with_target(&pad).ok()?).ok()?;
    let pipeline = gst::ElementFactory::make("playbin").build().ok()?;
    pipeline.set_property("uri", crate::video::file_uri(path).ok()?);
    pipeline.set_property("video-sink", &bin);
    pipeline.set_property("audio-sink", gst::ElementFactory::make("fakesink").build().ok()?);
    let frame = (|| {
        pipeline.set_state(gst::State::Paused).ok()?;
        let timeout = gst::ClockTime::from_mseconds(VIDEO_TIMEOUT.as_millis() as u64);
        let (result, _, _) = pipeline.state(timeout);
        result.ok()?;
        // Le tout début est souvent noir : on avance un peu.
        if let Some(duration) = pipeline.query_duration::<gst::ClockTime>() {
            let target = gst::ClockTime::from_nseconds(duration.nseconds() / 10);
            if pipeline
                .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, target)
                .is_ok()
            {
                let _ = pipeline.state(timeout);
            }
        }
        let sample = sink.try_pull_preroll(timeout)?;
        let info = gstreamer_video::VideoInfo::from_caps(sample.caps()?).ok()?;
        let buffer = sample.buffer()?;
        let mapped = buffer.map_readable().ok()?;
        let (width, height) = (info.width() as usize, info.height() as usize);
        let stride = usize::try_from(info.stride()[0]).ok()?;
        let row_bytes = width.checked_mul(4)?;
        let mut rgba = Vec::with_capacity(row_bytes.checked_mul(height)?);
        for row in 0..height {
            let start = row.checked_mul(stride)?;
            rgba.extend_from_slice(mapped.as_slice().get(start..start.checked_add(row_bytes)?)?);
        }
        let image = ferrite_ui::wallpaper::Image {
            width: u32::try_from(width).ok()?,
            height: u32::try_from(height).ok()?,
            rgba,
        };
        Some(crate::viewer::shrink(&image, SIDE))
    })();
    let _ = pipeline.set_state(gst::State::Null);
    frame
}

impl Thumbnails {
    /// Vignette de `path`, ou rien tant qu'elle n'est pas prête (elle est alors demandée).
    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<TextureHandle> {
        while let Ok((done, image)) = self.receiver.try_recv() {
            let slot = match image {
                Some(image) => {
                    Slot::Ready(ctx.load_texture(done.to_string_lossy(), image, TextureOptions::LINEAR))
                }
                None => Slot::Failed,
            };
            self.slots.insert(done, slot);
        }
        if !supported(path) {
            return None;
        }
        match self.slots.get(path) {
            Some(Slot::Ready(texture)) => Some(texture.clone()),
            Some(_) => None,
            None => {
                self.slots.insert(path.to_path_buf(), Slot::Pending);
                let requests = self.requests.get_or_insert_with(|| start_workers(&self.sender));
                let _ = requests.send((path.to_path_buf(), ctx.clone()));
                None
            }
        }
    }

    /// Oublie les vignettes d'un dossier relu (fichiers modifiés entre-temps).
    pub fn forget(&mut self) {
        self.slots.retain(|_, slot| matches!(slot, Slot::Pending));
    }
}

pub(super) fn decode(path: &Path) -> Option<egui::ColorImage> {
    // Une vidéo n'est pas lue en entier : sa taille ne compte pas.
    if is_video(path) {
        return decode_video(path);
    }
    if fs::metadata(path).ok()?.len() > MAX_BYTES {
        return None;
    }
    let data = fs::read(path).ok()?;
    let is_svg = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    if is_svg {
        decode_svg(&data)
    } else if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "jpg" | "jpeg"))
    {
        let image = ferrite_ui::wallpaper::decode_bytes(data).ok()?;
        let image = crate::viewer::shrink(&image, SIDE);
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [image.width as usize, image.height as usize],
            &image.rgba,
        ))
    } else {
        decode_png(&data)
    }
}

fn decode_svg(data: &[u8]) -> Option<egui::ColorImage> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let factor = SIDE as f32 / size.width().max(size.height());
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
    Some(egui::ColorImage::from_rgba_premultiplied(
        [width as usize, height as usize],
        pixmap.data(),
    ))
}

fn decode_png(data: &[u8]) -> Option<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let (width, height) = (info.width as usize, info.height as usize);
    if width * height > 8192 * 8192 {
        return None;
    }
    let pixels = &buffer[..info.buffer_size()];
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    // Réduction par échantillonnage : la vignette tient dans `SIDE` × `SIDE`.
    let factor = (SIDE as f32 / width.max(height) as f32).min(1.0);
    let (out_w, out_h) = (
        ((width as f32 * factor).round() as usize).max(1),
        ((height as f32 * factor).round() as usize).max(1),
    );
    let mut rgba = Vec::with_capacity(out_w * out_h * 4);
    for y in 0..out_h {
        let source_y = (y * height / out_h).min(height - 1);
        for x in 0..out_w {
            let source_x = (x * width / out_w).min(width - 1);
            let p = &pixels[(source_y * width + source_x) * channels..][..channels];
            rgba.extend_from_slice(&match channels {
                4 => [p[0], p[1], p[2], p[3]],
                3 => [p[0], p[1], p[2], 255],
                2 => [p[0], p[0], p[0], p[1]],
                _ => [p[0], p[0], p[0], 255],
            });
        }
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([out_w, out_h], &rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vignettes_les_images_jpeg_sans_tenir_compte_de_la_casse() {
        assert!(supported(Path::new("photo.jpg")));
        assert!(supported(Path::new("photo.jpeg")));
        assert!(supported(Path::new("PHOTO.JPG")));
        assert!(supported(Path::new("photo.png")));
        assert!(supported(Path::new("dessin.svg")));
        assert!(!supported(Path::new("photo.gif")));
        assert!(supported(Path::new("film.MP4")));
        assert!(supported(Path::new("clip.webm")));
    }

    #[test]
    fn image_renvoyee_par_le_processus_de_vignette() {
        let mut bytes = b"2 1\n".to_vec();
        bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(parse_frame(&bytes), Some((2, 1, &bytes[4..])));
        // Pixels manquants, en-tête abîmé : pas de vignette.
        assert_eq!(parse_frame(&bytes[..10]), None);
        assert_eq!(parse_frame(b"deux 1\n"), None);
    }
}
