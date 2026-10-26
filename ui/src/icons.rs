//! Icônes SVG : conversion en version « symbolique » monochrome.
//!
//! Les icônes Ferrite sont dessinées en couleur. Pour la barre du haut, les menus et les
//! barres latérales, elles sont converties à la volée : chaque couleur vive devient la
//! teinte demandée, et les parties claires et désaturées (barres Wi-Fi éteintes, rayons
//! inactifs…) restent visibles en estompé. Partagé par ferrite-shell et les applications.

use crate::theme::Color;

/// Opacité des parties « éteintes » d'une icône symbolique.
const DIM_OPACITY: f32 = 0.35;

/// Version monochrome d'une icône : chaque couleur devient `ink`, pleine ou estompée
/// selon qu'elle est vive (partie active) ou claire et grise (partie éteinte). Les
/// fonds argentés disparaissent pour ne garder que les contours.
pub fn symbolic(source: &str, ink: Color) -> String {
    let body_start = source.find("</defs>").map_or(0, |index| index + "</defs>".len());
    let (defs, body) = source.split_at(body_start);
    let full = format!("rgba({},{},{},1)", ink.r, ink.g, ink.b);
    let dim = format!("rgba({},{},{},{DIM_OPACITY})", ink.r, ink.g, ink.b);
    let body = body.replace("url(#silver)", "none");
    let mut out = String::with_capacity(source.len() + 256);
    out.push_str(defs);
    let mut rest = body.as_str();
    while let Some(index) = rest.find(['#', 'u']) {
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        if let Some(gradient) = rest.strip_prefix("url(#") {
            // Dégradé coloré : partie active.
            let end = gradient.find(')').map_or(gradient.len(), |end| end + 1);
            out.push_str(&full);
            rest = &gradient[end..];
        } else if let Some((color, length)) = rest.strip_prefix('#').and_then(hex_color) {
            out.push_str(if is_inactive(color) { &dim } else { &full });
            rest = &rest[1 + length..];
        } else {
            out.push_str(&rest[..1]);
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Couleur hexadécimale `RRGGBB` ou `RGB` en tête de `text`, avec sa longueur.
fn hex_color(text: &str) -> Option<(u32, usize)> {
    let length = text.bytes().take_while(u8::is_ascii_hexdigit).count();
    let value = u32::from_str_radix(&text[..length], 16).ok()?;
    match length {
        6 => Some((value, 6)),
        3 => {
            let [r, g, b] = [(value >> 8) & 0xF, (value >> 4) & 0xF, value & 0xF].map(|c| c * 0x11);
            Some(((r << 16) | (g << 8) | b, 3))
        }
        _ => None,
    }
}

/// Couleur claire et peu saturée : partie « éteinte » d'une icône d'état.
fn is_inactive(rgb: u32) -> bool {
    let [r, g, b] = [(rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF].map(|c| c as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    luminance > 0.72 && max - min < 0.2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toutes_les_couleurs_prennent_la_teinte() {
        let source = r##"<svg><defs><linearGradient id="silver"/></defs><g><path fill="#273544"/><path stroke="#CBD2DB"/><rect fill="url(#silver)"/><circle fill="#FFF"/></g></svg>"##;
        let out = symbolic(source, Color::rgb(0x102030));
        assert!(out.contains(r#"fill="rgba(16,32,48,1)""#));
        assert!(out.contains(r#"stroke="rgba(16,32,48,0.35)""#));
        assert!(out.contains(r#"fill="none""#));
        assert!(!out.contains("#273544"));
    }
}
