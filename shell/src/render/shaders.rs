//! Shaders GLES des décorations et des animations. Les couleurs et mesures viennent de
//! la charte (`ferrite_ui::theme`) et sont écrites dans la source GLSL à la compilation.
//!
//! Les distances sont calculées en pixels logiques ; `scale` (pixels physiques par pixel
//! logique) sert à l'anticrénelage, sur un pixel physique.

use ferrite_ui::theme::{Color, WINDOW_BUTTON_GAP, WINDOW_BUTTON_INSET, WINDOW_BUTTON_RADIUS};
use ferrite_ui::tokens::elevation;
use smithay::backend::renderer::gles::{
    GlesError, GlesPixelProgram, GlesRenderer, GlesTexProgram, UniformName, UniformType,
};

/// Fonctions communes aux shaders de pixels.
const COMMON: &str = r#"
precision mediump float;
uniform vec2 size;
uniform float alpha;
varying vec2 v_coords;
#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

// Couverture d'un pixel à la distance signée d (négative à l'intérieur), en pixels logiques.
float coverage(float d, float scale) {
    return clamp(0.5 - d * scale, 0.0, 1.0);
}

// Composition « par-dessus » en alpha prémultiplié.
vec4 over(vec4 dst, vec3 rgb, float a) {
    return dst * (1.0 - a) + vec4(rgb * a, a);
}

// Part d'ombre (0 à 1) à la distance signée d d'un bord flouté sur `blur` (CSS : écart
// type = blur / 2), comme `ferrite_ui::tokens::elevation::profile`.
float shadow_profile(float d, float blur) {
    float sigma = max(blur * 0.5, 0.25);
    float x = d / (sigma * 1.41421356);
    // Fonction d'erreur (Abramowitz et Stegun 7.1.26).
    float ax = abs(x);
    float t = 1.0 / (1.0 + 0.3275911 * ax);
    float poly = t * (0.2548296 + t * (-0.28449674 + t * (1.4214137 + t * (-1.4531521 + t * 1.0614054))));
    float e = sign(x) * (1.0 - poly * exp(-ax * ax));
    return clamp((0.5 - 0.5 * e - 0.0013499) / 0.9973002, 0.0, 1.0);
}

vec4 finish(vec4 color) {
    color = color * alpha;
#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif
    return color;
}
"#;

const TITLEBAR: &str = r#"
uniform float scale;
uniform vec4 background;
uniform vec4 separator;
uniform vec4 button;
uniform vec4 glyph;
uniform vec4 danger;
uniform vec4 accent;
uniform float hover;
uniform float corner;

// Encre lisible sur un fond : presque noire sur un fond clair, blanche sinon (même seuil
// que `Color::ink_on`).
vec3 ink_on(vec3 c) {
    return dot(c, vec3(0.299, 0.587, 0.114)) > 0.57 ? vec3(0.078, 0.078, 0.086) : vec3(1.0);
}

float segment(vec2 p, vec2 a, vec2 b) {
    vec2 pa = p - a;
    vec2 ba = b - a;
    float h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

void main() {
    vec2 p = v_coords * size;

    // Fond, coins du haut arrondis (droits si la fenêtre est agrandie).
    float d = -1.0;
    vec2 left = vec2(corner, corner);
    vec2 right = vec2(size.x - corner, corner);
    if (p.y < corner && p.x < left.x) {
        d = length(p - left) - corner;
    } else if (p.y < corner && p.x > right.x) {
        d = length(p - right) - corner;
    }
    vec4 color = background * coverage(d, scale);

    // Filet d'un pixel physique en bas.
    if (p.y > size.y - 1.0 / scale) {
        color = separator;
    }

    // Boutons à droite, pastilles de 14 px. Au repos, « fermer » porte l'accent (`accent`
    // vaut la pastille neutre si la fenêtre est inactive) et les autres sont neutres ; les
    // symboles n'apparaissent qu'au survol du bouton, « fermer » passant alors au rouge.
    float y = size.y * 0.5;
    vec2 q = p - vec2(size.x - BUTTON_INSET, y);
    bool lit = hover > 0.5 && hover < 1.5;
    vec3 fill = lit ? danger.rgb : accent.rgb;
    color = over(color, fill, coverage(length(q) - BUTTON_RADIUS, scale));
    float s = min(segment(q, vec2(-2.5, -2.5), vec2(2.5, 2.5)), segment(q, vec2(2.5, -2.5), vec2(-2.5, 2.5)));
    if (lit) {
        color = over(color, ink_on(fill), coverage(s - 0.75, scale));
    }

    // Agrandir, à gauche de fermer : deux flèches opposées.
    q = p - vec2(size.x - BUTTON_INSET - BUTTON_GAP, y);
    lit = hover > 1.5 && hover < 2.5;
    color = over(color, button.rgb, coverage(length(q) - BUTTON_RADIUS, scale));
    s = min(segment(q, vec2(-2.5, 0.5), vec2(-2.5, 2.5)), segment(q, vec2(-2.5, 2.5), vec2(-0.5, 2.5)));
    s = min(s, segment(q, vec2(0.5, -2.5), vec2(2.5, -2.5)));
    s = min(s, segment(q, vec2(2.5, -2.5), vec2(2.5, -0.5)));
    s = min(s, segment(q, vec2(-2.5, 2.5), vec2(-0.5, 0.5)));
    s = min(s, segment(q, vec2(2.5, -2.5), vec2(0.5, -0.5)));
    if (lit) {
        color = over(color, glyph.rgb, coverage(s - 0.7, scale));
    }

    // Réduire, à gauche d'agrandir : un trait.
    q = p - vec2(size.x - BUTTON_INSET - 2.0 * BUTTON_GAP, y);
    lit = hover > 2.5;
    color = over(color, button.rgb, coverage(length(q) - BUTTON_RADIUS, scale));
    s = segment(q, vec2(-3.0, 0.0), vec2(3.0, 0.0));
    if (lit) {
        color = over(color, glyph.rgb, coverage(s - 0.75, scale));
    }

    gl_FragColor = finish(color);
}
"#;

const SHADOW: &str = r#"
uniform vec4 rect;
uniform float radius;
// Couches de la charte : (décalage vertical, flou, opacité, inutilisé).
uniform vec4 contact;
uniform vec4 level;
uniform vec4 ring;

float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

float layer(vec2 p, vec2 center, vec4 shadow) {
    float d = rounded_box(p - center - vec2(0.0, shadow.x), rect.zw * 0.5, radius);
    return shadow.z * shadow_profile(d, shadow.y);
}

void main() {
    vec2 p = v_coords * size;
    vec2 center = rect.xy + rect.zw * 0.5;
    float a = layer(p, center, contact);
    a = a + layer(p, center, level) * (1.0 - a);
    vec4 color = vec4(0.0, 0.0, 0.0, a);
    // Bordure d'un pixel logique, juste à l'extérieur du cadre.
    float edge = rounded_box(p - center, rect.zw * 0.5, radius);
    float line = clamp(edge + 0.5, 0.0, 1.0) * clamp(1.5 - edge, 0.0, 1.0);
    gl_FragColor = finish(color * (1.0 - ring.a * line) + ring * line);
}
"#;

/// Aperçu d'accrochage : zone bleutée translucide aux coins arrondis, bordée d'un trait.
const SNAP: &str = r#"
uniform float scale;

float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 p = v_coords * size;
    float d = rounded_box(p - size * 0.5, size * 0.5 - 6.0, 12.0);
    vec3 accent = vec3(0.722, 0.227, 0.039);
    vec4 color = over(vec4(0.0), accent, 0.20 * coverage(d, scale));
    color = over(color, accent, 0.75 * coverage(abs(d + 1.0) - 1.0, scale));
    gl_FragColor = finish(color);
}
"#;

/// Fond du Dock : gris moyen à 20 % puis blanc à 12 % (lisible sur un fond clair comme
/// sombre), filet blanc à 28 % bordé d'un filet noir à 14 %, rayon 24, ombre d'un panneau
/// de la charte (contact puis `elevation::PANEL`). La zone dessinée déborde du fond de `MARGIN` de chaque côté, pour l'ombre.
const DOCK: &str = r#"
uniform float scale;

float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 p = v_coords * size;
    vec2 half_plate = size * 0.5 - MARGIN;
    // Rayon de 22, jamais plus que la demi-hauteur (petites icônes du Dock).
    float radius = min(22.0, min(half_plate.x, half_plate.y));
    float d = rounded_box(p - size * 0.5, half_plate, radius);
    float near = rounded_box(p - size * 0.5 - vec2(0.0, CONTACT.x), half_plate, radius);
    float far = rounded_box(p - size * 0.5 - vec2(0.0, LEVEL.x), half_plate, radius);
    float a = CONTACT.z * shadow_profile(near, CONTACT.y);
    a = a + LEVEL.z * shadow_profile(far, LEVEL.y) * (1.0 - a);
    float outside = 1.0 - coverage(d, scale);
    vec4 color = vec4(0.0, 0.0, 0.0, a * outside);
    // Plateau sombre et discret (charte) : ardoise à 76 %, liseré clair très fin.
    color = over(color, vec3(0.11, 0.133, 0.172), 0.76 * coverage(d, scale));
    color = over(color, vec3(0.0), 0.16 * coverage(abs(d - 0.5) - 0.5, scale));
    color = over(color, vec3(1.0), 0.09 * coverage(abs(d + 0.5) - 0.5, scale));
    gl_FragColor = finish(color);
}
"#;

/// Fond d'un panneau du bureau (centre de contrôle) : ombre de la charte (contact puis
/// `elevation::PANEL`) puis verre de la couleur `fill`, comme `Canvas::glass` le dessinait
/// au processeur (18 ms par hauteur nouvelle, bien plus sur batterie). `rect` : le
/// panneau, en pixels logiques dans la zone dessinée ; `grow` : agrandissement de
/// l'animation d'ouverture (ombre comprise) ; `fade` : opacité.
const PANEL: &str = r#"
uniform float scale;
uniform vec4 rect;
uniform float radius;
uniform float grow;
uniform vec4 fill;
uniform float fade;

float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 p = v_coords * size;
    vec2 half_card = rect.zw * 0.5;
    vec2 center = rect.xy + half_card;
    float r = min(radius, min(half_card.x, half_card.y));
    float near = rounded_box(p - center - vec2(0.0, CONTACT.x * grow), half_card, r);
    float far = rounded_box(p - center - vec2(0.0, LEVEL.x * grow), half_card, r);
    float a = CONTACT.z * shadow_profile(near, CONTACT.y * grow);
    a = a + LEVEL.z * shadow_profile(far, LEVEL.y * grow) * (1.0 - a);
    vec4 color = vec4(0.0, 0.0, 0.0, a);
    float cover = coverage(rounded_box(p - center, half_card, r), scale);
    color = color * (1.0 - fill.a * cover) + fill * cover;
    gl_FragColor = finish(color * fade);
}
"#;

/// Shader de texture : arrondit les coins du bas du contenu d'une fenêtre (ceux du haut
/// sont sous la barre de titre). Coordonnées en pixels physiques, repère de l'écran.
const ROUNDED_TEXTURE: &str = r#"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision mediump float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

// Élément dessiné et contenu de la fenêtre : x, y, largeur, hauteur.
uniform vec4 geo;
uniform vec4 clip;
uniform float radius;
// 1 : arrondir aussi les coins du haut (fenêtre sans barre de titre).
uniform float top;

void main() {
    vec4 color = texture2D(tex, v_coords);
#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0);
#endif

    vec2 p = geo.xy + v_coords * geo.zw;
    float bottom = clip.y + clip.w - radius;
    float upper = clip.y + radius;
    float cover = 1.0;
    float y = 0.0;
    bool corner = false;
    if (p.y > bottom) {
        y = bottom;
        corner = true;
    } else if (top > 0.5 && p.y < upper) {
        y = upper;
        corner = true;
    }
    if (corner) {
        vec2 left = vec2(clip.x + radius, y);
        vec2 right = vec2(clip.x + clip.z - radius, y);
        if (p.x < left.x) {
            cover = clamp(radius - length(p - left) + 0.5, 0.0, 1.0);
        } else if (p.x > right.x) {
            cover = clamp(radius - length(p - right) + 0.5, 0.0, 1.0);
        }
    }
    color = color * alpha * cover;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif
    gl_FragColor = color;
}
"#;

/// Texture de la fenêtre déformée entre ses bords et l'icône du Dock.
const GENIE_TEXTURE: &str = r#"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision mediump float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;
uniform vec4 geo;
uniform vec4 win;
uniform vec4 icon;
uniform float top;
uniform float bottom;
uniform float squeeze;
uniform float radius;

void main() {
    vec2 p = geo.xy + v_coords * geo.zw;
    if (p.y < top || p.y > bottom) {
        gl_FragColor = vec4(0.0);
        return;
    }

    float neck = max(icon.y, win.y + 1.0);
    float t = clamp((p.y - win.y) / (neck - win.y), 0.0, 1.0);
    float amount = t * t * (3.0 - 2.0 * t) * squeeze;
    float left = mix(win.x, icon.x, amount);
    float right = mix(win.x + win.z, icon.x + icon.z, amount);
    if (p.x < left || p.x > right) {
        gl_FragColor = vec4(0.0);
        return;
    }

    // Toute l'image, étirée au cadre.
    float u = (p.x - left) / max(right - left, 1.0);
    float v = (p.y - top) / max(bottom - top, 1.0);
    vec4 color = texture2D(tex, vec2(u, v));
#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0);
#endif
    // Coins arrondis du cadre, pour le bord d'une image rognée.
    if (radius > 0.0) {
        vec2 hs = vec2(right - left, bottom - top) * 0.5;
        vec2 q = abs(p - vec2(left, top) - hs) - (hs - radius);
        float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
        color *= clamp(0.5 - d, 0.0, 1.0);
    }
    gl_FragColor = color * alpha;
}
"#;

/// Programmes compilés, partagés par toutes les fenêtres.
#[derive(Debug, Clone)]
pub struct Shaders {
    pub titlebar: GlesPixelProgram,
    pub shadow: GlesPixelProgram,
    pub snap: GlesPixelProgram,
    pub dock: GlesPixelProgram,
    pub panel: GlesPixelProgram,
    pub rounded: GlesTexProgram,
    pub genie: GlesTexProgram,
}

impl Shaders {
    pub fn compile(renderer: &mut GlesRenderer) -> Result<Shaders, GlesError> {
        let float = |name: &'static str| UniformName::new(name, UniformType::_1f);
        let vec4 = |name: &'static str| UniformName::new(name, UniformType::_4f);

        let titlebar = renderer.compile_custom_pixel_shader(
            titlebar_source(),
            &[
                float("scale"),
                vec4("background"),
                vec4("separator"),
                vec4("button"),
                vec4("glyph"),
                vec4("danger"),
                vec4("accent"),
                float("hover"),
                float("corner"),
            ],
        )?;
        let shadow = renderer.compile_custom_pixel_shader(
            format!("{COMMON}{SHADOW}"),
            &[
                vec4("rect"),
                float("radius"),
                vec4("contact"),
                vec4("level"),
                vec4("ring"),
            ],
        )?;
        let snap = renderer.compile_custom_pixel_shader(format!("{COMMON}{SNAP}"), &[float("scale")])?;
        let dock = renderer.compile_custom_pixel_shader(dock_source(), &[float("scale")])?;
        let panel = renderer.compile_custom_pixel_shader(
            panel_source(),
            &[
                float("scale"),
                vec4("rect"),
                float("radius"),
                float("grow"),
                vec4("fill"),
                float("fade"),
            ],
        )?;
        let rounded = renderer.compile_custom_texture_shader(
            ROUNDED_TEXTURE,
            &[vec4("geo"), vec4("clip"), float("radius"), float("top")],
        )?;
        let genie = renderer.compile_custom_texture_shader(
            GENIE_TEXTURE,
            &[
                vec4("geo"),
                vec4("win"),
                vec4("icon"),
                float("top"),
                float("bottom"),
                float("squeeze"),
                float("radius"),
            ],
        )?;
        Ok(Shaders {
            titlebar,
            shadow,
            snap,
            dock,
            panel,
            rounded,
            genie,
        })
    }
}

/// Source du shader de barre de titre, constantes de la charte comprises.
fn titlebar_source() -> String {
    format!(
        "#define BUTTON_RADIUS {}\n#define BUTTON_INSET {}\n#define BUTTON_GAP {}\n{COMMON}{TITLEBAR}",
        float(WINDOW_BUTTON_RADIUS as f32),
        float(WINDOW_BUTTON_INSET as f32),
        float(WINDOW_BUTTON_GAP as f32),
    )
}

/// Source du shader du Dock : marge et couches d'ombre de la charte.
fn dock_source() -> String {
    let layer = |shadow: elevation::Shadow| {
        format!(
            "vec3({}, {}, {})",
            float(shadow.offset_y),
            float(shadow.blur),
            float(shadow.alpha)
        )
    };
    format!(
        "#define MARGIN {}\n#define CONTACT {}\n#define LEVEL {}\n{COMMON}{DOCK}",
        float(crate::desktop::dock::PLATE_SHADOW_MARGIN as f32),
        layer(elevation::CONTACT),
        layer(elevation::PANEL),
    )
}

/// Source du shader des panneaux : couches d'ombre de la charte.
fn panel_source() -> String {
    let layer = |shadow: elevation::Shadow| {
        format!(
            "vec3({}, {}, {})",
            float(shadow.offset_y),
            float(shadow.blur),
            float(shadow.alpha)
        )
    };
    format!(
        "#define CONTACT {}\n#define LEVEL {}\n{COMMON}{PANEL}",
        layer(elevation::CONTACT),
        layer(elevation::PANEL),
    )
}

/// Couche d'ombre en `vec4` (décalage, flou, opacité, 0), pour les uniformes.
pub fn shadow_layer(shadow: elevation::Shadow) -> [f32; 4] {
    [shadow.offset_y, shadow.blur, shadow.alpha, 0.0]
}

fn float(value: f32) -> String {
    format!("{value:.3}")
}

/// Couleur en `vec4` prémultiplié, pour les uniformes.
pub fn premultiplied(color: Color) -> [f32; 4] {
    let a = color.a as f32 / 255.0;
    let c = |v: u8| v as f32 / 255.0 * a;
    [c(color.r), c(color.g), c(color.b), a]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_de_la_barre_de_titre() {
        let source = titlebar_source();
        // Les constantes sont définies avant d'être utilisées par le code.
        assert!(source.starts_with("#define BUTTON_RADIUS 7.000\n"));
        assert!(source.find("#define BUTTON_INSET 19.000").unwrap() < source.find("void main").unwrap());
    }

    #[test]
    fn couleur_premultipliee() {
        assert_eq!(premultiplied(Color::WHITE), [1.0, 1.0, 1.0, 1.0]);
        let half = premultiplied(Color::WHITE.with_alpha(51));
        assert!((half[0] - 0.2).abs() < 1e-6 && (half[3] - 0.2).abs() < 1e-6);
        let red = premultiplied(Color::rgb(0xFF0000).with_alpha(51));
        assert!((red[0] - 0.2).abs() < 1e-6 && red[1] == 0.0);
    }
}
