//! Éléments de décoration d'une fenêtre : ombre, barre de titre, titre.
//!
//! Les éléments sont gardés d'une image à l'autre dans les données de la fenêtre : leur
//! identifiant reste le même et ils ne sont mis à jour que si quelque chose change.

use std::cell::RefCell;

use ferrite_ui::{
    theme::Palette,
    tokens::{elevation, radius::WINDOW as WINDOW_RADIUS},
};
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

/// Débord de l'ombre autour du cadre.
const SHADOW_MARGIN: i32 = elevation::WINDOW_MARGIN;

/// Ombre d'une fenêtre selon son état.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shadow {
    offset_y: f32,
    blur: f32,
    alpha: f32,
}

const SHADOW_ACTIVE: Shadow = Shadow {
    offset_y: elevation::ACTIVE_OFFSET_Y,
    blur: elevation::ACTIVE_BLUR,
    alpha: elevation::ACTIVE_ALPHA,
};
const SHADOW_INACTIVE: Shadow = Shadow {
    offset_y: elevation::INACTIVE_OFFSET_Y,
    blur: elevation::INACTIVE_BLUR,
    alpha: elevation::INACTIVE_ALPHA,
};

/// Ombre déjà prête : (active, thème sombre), zone et élément.
type ShadowCache = ((bool, bool), Rectangle<i32, Logical>, PixelShaderElement);

/// Fenêtre active, et bouton de fenêtre survolé.
pub type WindowLook = (bool, Option<decoration::WindowButton>);

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
    /// Bouton survolé : il prend sa couleur d'action.
    hover: Option<decoration::WindowButton>,
    /// Agrandie : coins droits, ni ombre ni bordure.
    maximized: bool,
    dark_theme: bool,
    scale: f64,
}

/// Titre déjà dessiné, avec ce qui a servi à le dessiner.
#[derive(Debug)]
struct TitleCache {
    text: String,
    active: bool,
    dark_theme: bool,
    scale: f64,
    max_width: u32,
    /// Image et taille en pixels physiques ; `None` si rien à afficher.
    image: Option<(MemoryRenderBuffer, (i32, i32))>,
}

#[derive(Debug, Default)]
struct Cache {
    titlebar: Option<(Look, Rectangle<i32, Logical>, PixelShaderElement)>,
    shadow: Option<ShadowCache>,
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
    /// Sans `titlebar` (la fenêtre dessine son haut), seulement l'ombre.
    #[allow(clippy::too_many_arguments)] // Rendu, fenêtre, géométrie et apparence.
    pub fn push_elements(
        &self,
        renderer: &mut GlesRenderer,
        window: &Window,
        content: Rectangle<i32, Logical>,
        look: WindowLook,
        titlebar: bool,
        dark_theme: bool,
        scale: f64,
        out: &mut Vec<OutputElement>,
    ) {
        let (active, hover) = look;
        let maximized = crate::wm::is_maximized(window);
        let look = Look {
            active,
            hover,
            maximized,
            dark_theme,
            scale,
        };
        window
            .user_data()
            .insert_if_missing(|| RefCell::new(Cache::default()));
        let Some(cache) = window.user_data().get::<RefCell<Cache>>() else {
            return;
        };
        let mut cache = cache.borrow_mut();
        // Fenêtre sans barre de titre (elle dessine la sienne) : seulement l'ombre.
        if titlebar {
            self.push_titlebar(renderer, window, content, look, scale, &mut cache, out);
        }
        if !maximized {
            let shadow = active && !crate::wm::is_tiled(window);
            self.push_shadow(content, titlebar, (shadow, dark_theme), &mut cache, out);
        }
    }

    #[allow(clippy::too_many_arguments)] // Rendu, fenêtre, géométrie et apparence.
    fn push_titlebar(
        &self,
        renderer: &mut GlesRenderer,
        window: &Window,
        content: Rectangle<i32, Logical>,
        look: Look,
        scale: f64,
        cache: &mut Cache,
        out: &mut Vec<OutputElement>,
    ) {
        let Look {
            active,
            hover,
            maximized,
            dark_theme,
            ..
        } = look;
        let palette = Palette::themed(dark_theme);
        let bar = decoration::titlebar(content);

        // Titre, centré dans la barre, sans empiéter sur les boutons.
        let buttons_width = decoration::BUTTONS_WIDTH;
        let max_width = ((bar.size.w - 2 * buttons_width).max(0) as f64 * scale) as u32;
        let text = window_title(window);
        let stale = cache.title.as_ref().is_none_or(|t| {
            t.text != text
                || t.active != active
                || t.dark_theme != dark_theme
                || t.scale != scale
                || t.max_width != max_width
        });
        if stale {
            let color = if active {
                palette.window_title_active
            } else {
                palette.window_title_inactive
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
                dark_theme,
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

        // Barre de titre, unie, avec son filet du bas.
        let (background, line, button) = if active {
            (palette.titlebar, palette.titlebar_line, palette.window_button)
        } else {
            (
                palette.titlebar_inactive,
                palette.titlebar_line_inactive,
                palette.window_button_inactive,
            )
        };
        let uniforms = || {
            vec![
                Uniform::new("scale", scale as f32),
                Uniform::new("background", premultiplied(background)),
                Uniform::new("separator", premultiplied(line)),
                Uniform::new("button", premultiplied(button)),
                Uniform::new("glyph", premultiplied(palette.window_glyph)),
                Uniform::new("danger", premultiplied(palette.danger)),
                Uniform::new("accent", premultiplied(palette.accent)),
                Uniform::new(
                    "hover",
                    match hover {
                        None => 0.0f32,
                        Some(decoration::WindowButton::Close) => 1.0,
                        Some(decoration::WindowButton::Zoom) => 2.0,
                        Some(decoration::WindowButton::Minimize) => 3.0,
                    },
                ),
                Uniform::new("corner", if maximized { 0.0f32 } else { WINDOW_RADIUS as f32 }),
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
    }

    /// Ombre, autour du cadre entier.
    fn push_shadow(
        &self,
        content: Rectangle<i32, Logical>,
        titlebar: bool,
        (active, dark_theme): (bool, bool),
        cache: &mut Cache,
        out: &mut Vec<OutputElement>,
    ) {
        let frame = if titlebar {
            decoration::frame(content)
        } else {
            content
        };
        let area = Rectangle::new(
            frame.loc - Point::from((SHADOW_MARGIN, SHADOW_MARGIN)),
            (frame.size.w + 2 * SHADOW_MARGIN, frame.size.h + 2 * SHADOW_MARGIN).into(),
        );
        let shadow = if active { SHADOW_ACTIVE } else { SHADOW_INACTIVE };
        let ring = Palette::themed(dark_theme).window_ring;
        let shadow_uniforms = || {
            let margin = SHADOW_MARGIN as f32;
            vec![
                Uniform::new("rect", [margin, margin, frame.size.w as f32, frame.size.h as f32]),
                Uniform::new("offset_y", shadow.offset_y),
                Uniform::new("radius", WINDOW_RADIUS as f32),
                Uniform::new("blur", shadow.blur),
                Uniform::new("strength", shadow.alpha),
                Uniform::new("ring", premultiplied(ring)),
            ]
        };
        let key = (active, dark_theme);
        match &mut cache.shadow {
            Some((last_key, last_area, element)) => {
                if *last_key != key || last_area.size != area.size {
                    element.update_uniforms(shadow_uniforms());
                    *last_key = key;
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
                cache.shadow = Some((key, area, element));
            }
        }
        if let Some((_, _, element)) = &cache.shadow {
            out.push(OutputElement::Shader(element.clone()));
        }
    }
}

/// Titre de la fenêtre (vide si le client n'en a pas donné).
pub(crate) fn window_title(window: &Window) -> String {
    if let Some(x11) = window.x11_surface() {
        return x11.title();
    }
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
