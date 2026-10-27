//! Shaders GLES des décorations : barre de titre (avec feux tricolores), ombre, et coins
//! arrondis du contenu des fenêtres. Les couleurs et mesures viennent de la charte
//! (`ferrite_ui::theme`) et sont écrites dans la source GLSL à la compilation.
//!
//! Les distances sont calculées en pixels logiques ; `scale` (pixels physiques par pixel
//! logique) sert à l'anticrénelage, sur un pixel physique.

use ferrite_ui::theme::{
    Color, Palette, TITLEBAR_HEIGHT, TRAFFIC_LIGHT_FIRST_X, TRAFFIC_LIGHT_RADIUS, TRAFFIC_LIGHT_STEP,
    WINDOW_RADIUS,
};
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
uniform float lit;
uniform float hover;

float segment(vec2 p, vec2 a, vec2 b) {
    vec2 pa = p - a;
    vec2 ba = b - a;
    float h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

vec4 light(vec4 color, vec2 p, vec2 c, vec3 fill, vec3 edge, float kind) {
    float d = length(p - c);
    color = over(color, edge, coverage(d - LIGHT_RADIUS, scale));
    color = over(color, fill, coverage(d - (LIGHT_RADIUS - 0.6), scale));
    if (hover > 0.5) {
        // Symboles au survol : × fermer, − réduire, + plein écran.
        vec2 q = p - c;
        float s;
        if (kind < 0.5) {
            s = min(segment(q, vec2(-2.2, -2.2), vec2(2.2, 2.2)), segment(q, vec2(-2.2, 2.2), vec2(2.2, -2.2)));
        } else if (kind < 1.5) {
            s = segment(q, vec2(-3.0, 0.0), vec2(3.0, 0.0));
        } else {
            s = min(segment(q, vec2(-3.0, 0.0), vec2(3.0, 0.0)), segment(q, vec2(0.0, -3.0), vec2(0.0, 3.0)));
        }
        color = over(color, edge * 0.4, coverage(s - 0.55, scale));
    }
    return color;
}

void main() {
    vec2 p = v_coords * size;

    // Fond, coins du haut arrondis.
    float d = -1.0;
    vec2 left = vec2(RADIUS, RADIUS);
    vec2 right = vec2(size.x - RADIUS, RADIUS);
    if (p.y < RADIUS && p.x < left.x) {
        d = length(p - left) - RADIUS;
    } else if (p.y < RADIUS && p.x > right.x) {
        d = length(p - right) - RADIUS;
    }
    vec4 color = background * coverage(d, scale);

    // Séparateur d'un pixel physique en bas.
    if (p.y > size.y - 1.0 / scale) {
        color = separator;
    }

    // Feux tricolores : en couleur si la fenêtre est active ou survolée, gris sinon.
    float y = size.y * 0.5;
    if (lit > 0.5 || hover > 0.5) {
        color = light(color, p, vec2(LIGHT_X, y), CLOSE_FILL, CLOSE_EDGE, 0.0);
        color = light(color, p, vec2(LIGHT_X + LIGHT_STEP, y), MIN_FILL, MIN_EDGE, 1.0);
        color = light(color, p, vec2(LIGHT_X + 2.0 * LIGHT_STEP, y), ZOOM_FILL, ZOOM_EDGE, 2.0);
    } else {
        color = light(color, p, vec2(LIGHT_X, y), OFF_FILL, OFF_EDGE, 0.0);
        color = light(color, p, vec2(LIGHT_X + LIGHT_STEP, y), OFF_FILL, OFF_EDGE, 1.0);
        color = light(color, p, vec2(LIGHT_X + 2.0 * LIGHT_STEP, y), OFF_FILL, OFF_EDGE, 2.0);
    }

    gl_FragColor = finish(color);
}
"#;

const SHADOW: &str = r#"
uniform vec4 rect;
uniform float radius;
uniform float blur;
uniform float strength;

float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 p = v_coords * size;
    float d = rounded_box(p - (rect.xy + rect.zw * 0.5), rect.zw * 0.5, radius);
    float a = 1.0 - smoothstep(-blur * 0.5, blur, d);
    gl_FragColor = finish(vec4(0.0, 0.0, 0.0, strength * a * a));
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
    vec3 accent = vec3(0.212, 0.537, 0.902);
    vec4 color = over(vec4(0.0), accent, 0.20 * coverage(d, scale));
    color = over(color, accent, 0.75 * coverage(abs(d + 1.0) - 1.0, scale));
    gl_FragColor = finish(color);
}
"#;

/// Fond du Dock : verre clair aux coins arrondis, contour discret et reflet en haut.
const DOCK: &str = r#"
uniform float scale;

float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 p = v_coords * size;
    float d = rounded_box(p - size * 0.5, size * 0.5, 18.0);
    vec4 color = over(vec4(0.0), vec3(0.0), 0.157 * coverage(d, scale));
    color = over(color, vec3(0.957, 0.965, 0.980), 0.588 * coverage(d + 1.0, scale));
    if (p.y < 2.0 && p.x > 18.0 && p.x < size.x - 18.0) {
        color = over(color, vec3(1.0), 0.588 * coverage(d + 1.0, scale));
    }
    gl_FragColor = finish(color);
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

/// Programmes compilés, partagés par toutes les fenêtres.
#[derive(Debug, Clone)]
pub struct Shaders {
    pub titlebar: GlesPixelProgram,
    pub shadow: GlesPixelProgram,
    pub snap: GlesPixelProgram,
    pub dock: GlesPixelProgram,
    pub rounded: GlesTexProgram,
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
                float("lit"),
                float("hover"),
            ],
        )?;
        let shadow = renderer.compile_custom_pixel_shader(
            format!("{COMMON}{SHADOW}"),
            &[vec4("rect"), float("radius"), float("blur"), float("strength")],
        )?;
        let snap = renderer.compile_custom_pixel_shader(format!("{COMMON}{SNAP}"), &[float("scale")])?;
        let dock = renderer.compile_custom_pixel_shader(format!("{COMMON}{DOCK}"), &[float("scale")])?;
        let rounded = renderer.compile_custom_texture_shader(
            ROUNDED_TEXTURE,
            &[vec4("geo"), vec4("clip"), float("radius"), float("top")],
        )?;
        Ok(Shaders {
            titlebar,
            shadow,
            snap,
            dock,
            rounded,
        })
    }
}

/// Source du shader de barre de titre, constantes de la charte comprises.
fn titlebar_source() -> String {
    let palette = Palette::LIGHT;
    let [close, minimize, zoom] = palette.traffic_lights;
    let (off_fill, off_edge) = palette.traffic_light_inactive;
    let defines = [
        ("RADIUS", float(WINDOW_RADIUS as f32)),
        ("LIGHT_RADIUS", float(TRAFFIC_LIGHT_RADIUS as f32 + 0.25)),
        ("LIGHT_X", float(TRAFFIC_LIGHT_FIRST_X as f32)),
        ("LIGHT_STEP", float(TRAFFIC_LIGHT_STEP as f32)),
        ("CLOSE_FILL", vec3(close.0)),
        ("CLOSE_EDGE", vec3(close.1)),
        ("MIN_FILL", vec3(minimize.0)),
        ("MIN_EDGE", vec3(minimize.1)),
        ("ZOOM_FILL", vec3(zoom.0)),
        ("ZOOM_EDGE", vec3(zoom.1)),
        ("OFF_FILL", vec3(off_fill)),
        ("OFF_EDGE", vec3(off_edge)),
    ];
    let mut source = String::new();
    for (name, value) in defines {
        source.push_str(&format!("#define {name} {value}\n"));
    }
    debug_assert_eq!(
        TITLEBAR_HEIGHT, 30,
        "les feux sont centrés sur la hauteur de la barre"
    );
    source.push_str(COMMON);
    source.push_str(TITLEBAR);
    source
}

fn float(value: f32) -> String {
    format!("{value:.3}")
}

fn vec3(color: Color) -> String {
    let c = |v: u8| v as f32 / 255.0;
    format!("vec3({:.4}, {:.4}, {:.4})", c(color.r), c(color.g), c(color.b))
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
        assert!(source.starts_with("#define RADIUS 11.000\n"));
        assert!(source.contains("#define CLOSE_FILL vec3(1.0000, 0.3725, 0.3412)"));
        assert!(source.find("#define OFF_EDGE").unwrap() < source.find("void main").unwrap());
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
