//! Éléments de décoration d'une fenêtre : ombre, barre de titre, titre.
//!
//! Les éléments sont gardés d'une image à l'autre dans les données de la fenêtre : leur
//! identifiant reste le même et ils ne sont mis à jour que si quelque chose change.

use std::cell::RefCell;

use ferrite_ui::theme::{Palette, TRAFFIC_LIGHT_FIRST_X, TRAFFIC_LIGHT_STEP, WINDOW_RADIUS};
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            },
            gles::{GlesRenderer, Uniform, element::PixelShaderElement},
        },
    },
    desktop::Window,
    utils::{Logical, Point, Rectangle, Transform},
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};
use tracing::warn;

use super::{
    OutputElement,
    shaders::{Shaders, premultiplied},
    title::{TITLE_SIZE, TitleFont},
};
use crate::wm::decoration;

/// Ombre : débord autour du cadre, décalage vers le bas, flou et intensité.
const SHADOW_MARGIN: i32 = 48;
const SHADOW_OFFSET_Y: f32 = 10.0;
const SHADOW_BLUR: f32 = 26.0;
const SHADOW_ACTIVE: f32 = 0.42;
const SHADOW_INACTIVE: f32 = 0.22;

/// Ressources partagées des décorations.
#[derive(Debug)]
pub struct Decorations {
    pub shaders: Shaders,
    font: TitleFont,
}

/// État d'une fenêtre qui détermine l'apparence de sa décoration.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Look {
    active: bool,
    hover: bool,
    scale: f64,
}

/// Titre déjà dessiné, avec ce qui a servi à le dessiner.
#[derive(Debug)]
struct TitleCache {
    text: String,
    active: bool,
    scale: f64,
    max_width: u32,
    /// Image et taille en pixels physiques ; `None` si rien à afficher.
    image: Option<(MemoryRenderBuffer, (i32, i32))>,
}

#[derive(Debug, Default)]
struct Cache {
    titlebar: Option<(Look, Rectangle<i32, Logical>, PixelShaderElement)>,
    shadow: Option<(bool, Rectangle<i32, Logical>, PixelShaderElement)>,
    title: Option<TitleCache>,
}

impl Decorations {
    pub fn new(renderer: &mut GlesRenderer) -> Result<Decorations, String> {
        let shaders = Shaders::compile(renderer).map_err(|err| format!("shaders : {err:?}"))?;
        let font = TitleFont::new()?;
        Ok(Decorations { shaders, font })
    }

    /// Ajoute à `out`, du premier plan vers l'arrière, le titre, la barre de titre et
    /// l'ombre de `window`, dont le contenu occupe `content` (logique, repère de l'écran).
    pub fn push_elements(
        &self,
        renderer: &mut GlesRenderer,
        window: &Window,
        content: Rectangle<i32, Logical>,
        look: (bool, bool),
        scale: f64,
        out: &mut Vec<OutputElement>,
    ) {
        let (active, hover) = look;
        let look = Look { active, hover, scale };
        window
            .user_data()
            .insert_if_missing(|| RefCell::new(Cache::default()));
        let Some(cache) = window.user_data().get::<RefCell<Cache>>() else {
            return;
        };
        let mut cache = cache.borrow_mut();
        let palette = Palette::LIGHT;
        let bar = decoration::titlebar(content);

        // Titre, centré dans la barre, sans empiéter sur les feux.
        let lights_width = (TRAFFIC_LIGHT_FIRST_X + 2 * TRAFFIC_LIGHT_STEP) as i32 + 16;
        let max_width = ((bar.size.w - 2 * lights_width).max(0) as f64 * scale) as u32;
        let text = window_title(window);
        let stale = cache.title.as_ref().is_none_or(|t| {
            t.text != text || t.active != active || t.scale != scale || t.max_width != max_width
        });
        if stale {
            let color = if active {
                palette.text
            } else {
                palette.text_secondary
            };
            let image = self
                .font
                .render(&text, TITLE_SIZE * scale as f32, color, max_width)
                .map(|bitmap| {
                    let size = (bitmap.width as i32, bitmap.height as i32);
                    let buffer = MemoryRenderBuffer::from_slice(
                        &bitmap.pixels,
                        Fourcc::Argb8888,
                        size,
                        1,
                        Transform::Normal,
                        None,
                    );
                    (buffer, size)
                });
            cache.title = Some(TitleCache {
                text,
                active,
                scale,
                max_width,
                image,
            });
        }
        if let Some((buffer, (w, h))) = cache.title.as_ref().and_then(|t| t.image.as_ref()) {
            let bar_physical = bar.to_f64().to_physical(scale);
            let x = (bar_physical.loc.x + (bar_physical.size.w - *w as f64) / 2.0).round();
            let y = (bar_physical.loc.y + (bar_physical.size.h - *h as f64) / 2.0).round();
            // Le tampon est en pixels physiques : sa taille logique est divisée par l'échelle.
            let src = Rectangle::from_size((*w as f64, *h as f64).into());
            let size = (
                (*w as f64 / scale).round() as i32,
                (*h as f64 / scale).round() as i32,
            );
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                Point::from((x, y)),
                buffer,
                None,
                Some(src),
                Some(size.into()),
                Kind::Unspecified,
            ) {
                Ok(element) => out.push(OutputElement::Memory(element)),
                Err(err) => warn!("titre non importé : {err:?}"),
            }
        }

        // Barre de titre.
        let uniforms = || {
            vec![
                Uniform::new("scale", scale as f32),
                Uniform::new("background", premultiplied(palette.titlebar)),
                // Sans trait sous la barre : elle se prolonge dans l'en-tête des
                // applications Ferrite, de la même teinte.
                Uniform::new("separator", premultiplied(palette.titlebar)),
                Uniform::new("lit", if active { 1.0f32 } else { 0.0 }),
                Uniform::new("hover", if hover { 1.0f32 } else { 0.0 }),
            ]
        };
        match &mut cache.titlebar {
            Some((last_look, last_area, element)) => {
                if *last_look != look {
                    element.update_uniforms(uniforms());
                    *last_look = look;
                }
                if *last_area != bar {
                    element.resize(bar, None);
                    *last_area = bar;
                }
            }
            None => {
                let element = PixelShaderElement::new(
                    self.shaders.titlebar.clone(),
                    bar,
                    None,
                    1.0,
                    uniforms(),
                    Kind::Unspecified,
                );
                cache.titlebar = Some((look, bar, element));
            }
        }
        if let Some((_, _, element)) = &cache.titlebar {
            out.push(OutputElement::Shader(element.clone()));
        }

        // Ombre, autour du cadre entier.
        let frame = decoration::frame(content);
        let area = Rectangle::new(
            frame.loc - Point::from((SHADOW_MARGIN, SHADOW_MARGIN)),
            (frame.size.w + 2 * SHADOW_MARGIN, frame.size.h + 2 * SHADOW_MARGIN).into(),
        );
        let shadow_uniforms = || {
            let margin = SHADOW_MARGIN as f32;
            vec![
                Uniform::new(
                    "rect",
                    [
                        margin,
                        margin + SHADOW_OFFSET_Y,
                        frame.size.w as f32,
                        frame.size.h as f32,
                    ],
                ),
                Uniform::new("radius", WINDOW_RADIUS as f32),
                Uniform::new("blur", SHADOW_BLUR),
                Uniform::new("strength", if active { SHADOW_ACTIVE } else { SHADOW_INACTIVE }),
            ]
        };
        match &mut cache.shadow {
            Some((last_active, last_area, element)) => {
                if *last_active != active || last_area.size != area.size {
                    element.update_uniforms(shadow_uniforms());
                    *last_active = active;
                }
                if *last_area != area {
                    element.resize(area, None);
                    *last_area = area;
                }
            }
            None => {
                let element = PixelShaderElement::new(
                    self.shaders.shadow.clone(),
                    area,
                    None,
                    1.0,
                    shadow_uniforms(),
                    Kind::Unspecified,
                );
                cache.shadow = Some((active, area, element));
            }
        }
        if let Some((_, _, element)) = &cache.shadow {
            out.push(OutputElement::Shader(element.clone()));
        }
    }
}

/// Titre de la fenêtre (vide si le client n'en a pas donné).
fn window_title(window: &Window) -> String {
    let Some(toplevel) = window.toplevel() else {
        return String::new();
    };
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|data| data.lock().ok().and_then(|data| data.title.clone()))
            .unwrap_or_default()
    })
}
