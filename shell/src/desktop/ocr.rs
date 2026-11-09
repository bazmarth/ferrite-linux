//! Capture → mode « Texte » : le texte d'une zone de l'écran, reconnu par tesseract
//! (paquets `tesseract-ocr`, `tesseract-ocr-fra`), puis copié dans le presse-papiers.
//! Toujours dans un fil à part : la reconnaissance prend une à quelques secondes.

use std::time::Duration;

/// Délai maximal de la reconnaissance.
const TIMEOUT: Duration = Duration::from_secs(30);
/// En dessous de ce nombre de pixels, l'image est agrandie deux fois : tesseract lit mal
/// le texte d'écran, petit, à sa taille réelle.
const UPSCALE_BELOW: u64 = 4_000_000;

/// Échec de la reconnaissance, déjà rédigé pour une notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrError {
    pub summary: &'static str,
    pub body: String,
}

/// Reconnaît le texte de l'image RGBA `width × height`. Bloquant.
pub fn recognize(rgba: &[u8], width: u32, height: u32) -> Result<String, OcrError> {
    let missing = || OcrError {
        summary: "Reconnaissance du texte absente",
        body: "Installez les paquets tesseract-ocr et tesseract-ocr-fra.".to_owned(),
    };
    let list = ferrite_system::command::run("tesseract", &["--list-langs"]).map_err(|_| missing())?;
    let languages = languages(&list).ok_or_else(missing)?;
    let (pixels, width, height) = if u64::from(width) * u64::from(height) < UPSCALE_BELOW {
        (upscale2(rgba, width, height), width * 2, height * 2)
    } else {
        (rgba.to_vec(), width, height)
    };
    let png = crate::render::capture::encode_png(width, height, &pixels).map_err(|error| OcrError {
        summary: "Copie du texte impossible",
        body: error,
    })?;
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    let file = dir.join(format!("ferrite-texte-{}.png", std::process::id()));
    std::fs::write(&file, png).map_err(|error| OcrError {
        summary: "Copie du texte impossible",
        body: error.to_string(),
    })?;
    let path = file.to_string_lossy().into_owned();
    let result = ferrite_system::command::run_with_timeout(
        "tesseract",
        &[&path, "stdout", "-l", &languages, "--psm", "3"],
        TIMEOUT,
    );
    let _ = std::fs::remove_file(&file);
    let text = result.map_err(|error| OcrError {
        summary: "Copie du texte impossible",
        body: error,
    })?;
    Ok(tidy(&text))
}

/// Langues à demander à tesseract parmi celles installées : français et anglais.
fn languages(list: &str) -> Option<String> {
    let installed: Vec<&str> = list.lines().map(str::trim).collect();
    let wanted: Vec<&str> = ["fra", "eng"]
        .into_iter()
        .filter(|language| installed.contains(language))
        .collect();
    (!wanted.is_empty()).then(|| wanted.join("+"))
}

/// Lignes nettoyées : espaces de fin retirés, pas plus d'une ligne vide de suite, ni au
/// début ni à la fin.
fn tidy(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines().map(str::trim_end) {
        if line.trim().is_empty() && out.last().is_none_or(|last| last.is_empty()) {
            continue;
        }
        out.push(if line.trim().is_empty() { "" } else { line });
    }
    while out.last().is_some_and(|last| last.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

/// Image agrandie deux fois (chaque pixel devient un carré de quatre).
fn upscale2(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (width, height) = (width as usize, height as usize);
    let mut out = Vec::with_capacity(width * height * 16);
    for row in rgba.chunks_exact(width * 4).take(height) {
        let mut doubled = Vec::with_capacity(width * 8);
        for pixel in row.chunks_exact(4) {
            doubled.extend_from_slice(pixel);
            doubled.extend_from_slice(pixel);
        }
        out.extend_from_slice(&doubled);
        out.extend_from_slice(&doubled);
    }
    out
}

/// Début du texte, pour la notification.
pub fn preview(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 90 {
        format!("{}…", flat.chars().take(90).collect::<String>())
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn langues_installees() {
        let list =
            "List of available languages in \"/usr/share/tesseract-ocr/5/tessdata/\" (3):\neng\nfra\nosd\n";
        assert_eq!(languages(list).as_deref(), Some("fra+eng"));
        assert_eq!(languages("List…\neng\nosd\n").as_deref(), Some("eng"));
        assert_eq!(languages("List…\nosd\n"), None);
    }

    #[test]
    fn texte_nettoye() {
        assert_eq!(tidy("\n\nBonjour  \n\n\n\nà tous\n\n"), "Bonjour\n\nà tous");
        assert_eq!(tidy("  \n"), "");
    }

    #[test]
    fn agrandissement() {
        let rgba = [1, 1, 1, 1, 2, 2, 2, 2];
        let big = upscale2(&rgba, 2, 1);
        assert_eq!(big.len(), 4 * 4 * 2);
        assert_eq!(&big[..16], &[1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2]);
        assert_eq!(&big[..16], &big[16..]);
    }

    #[test]
    fn apercu_court() {
        assert_eq!(preview("a\n  b"), "a b");
        assert!(preview(&"x".repeat(200)).ends_with('…'));
    }
}
