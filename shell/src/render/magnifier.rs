//! Zoom de l'écran (loupe) : la scène est dessinée dans une image de la taille de l'écran,
//! dont on montre une partie, agrandie par le GPU, autour du pointeur.
//!
//! L'image est gardée d'une image à l'autre avec son propre suivi des dégâts : seules les
//! zones qui ont changé y sont redessinées, et l'écran n'est redessiné que si elle a changé
//! ou si la partie montrée a bougé. Au repos, rien n'est redessiné (la cadence de repos du
//! backend DRM suit les dégâts).

use std::{cell::RefCell, collections::HashMap};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Color32F, Offscreen,
            damage::OutputDamageTracker,
            element::{
                Kind,
                texture::{TextureRenderBuffer, TextureRenderElement},
            },
            gles::{GlesRenderer, GlesTexture},
        },
    },
    utils::{Buffer, Logical, Physical, Point, Rectangle, Size, Transform},
};
use tracing::warn;

use super::OutputElement;
use crate::desktop::magnifier;

/// Image de la scène d'un écran, et ce qui y a déjà été dessiné.
#[derive(Debug)]
struct SceneImage {
    size: Size<i32, Physical>,
    scale: f64,
    buffer: TextureRenderBuffer<GlesTexture>,
    damage: OutputDamageTracker,
    /// Une image a déjà été dessinée : seuls les changements sont à redessiner.
    drawn: bool,
}

/// Images de la scène, par écran.
#[derive(Debug, Default)]
pub struct Magnifiers {
    images: RefCell<HashMap<String, SceneImage>>,
}

impl Magnifiers {
    /// Remplace `scene` par sa partie grossie `zoom` fois autour de `pointer` (coordonnées de
    /// l'écran). Sans image possible, la scène reste telle quelle.
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &self,
        renderer: &mut GlesRenderer,
        output_name: &str,
        screen: Size<i32, Logical>,
        scale: f64,
        pointer: Point<f64, Logical>,
        zoom: f64,
        scene: Vec<OutputElement>,
        elements: &mut Vec<OutputElement>,
    ) {
        let physical = screen.to_f64().to_physical(scale);
        let size = Size::<i32, Physical>::from((physical.w.round() as i32, physical.h.round() as i32));
        let mut images = self.images.borrow_mut();
        let image = match images.get_mut(output_name) {
            Some(image) if image.size == size && image.scale == scale => image,
            _ => match SceneImage::new(renderer, size, scale) {
                Ok(image) => images
                    .entry(output_name.to_owned())
                    .insert_entry(image)
                    .into_mut(),
                Err(err) => {
                    warn!(?err, "image du zoom de l'écran indisponible");
                    elements.extend(scene);
                    return;
                }
            },
        };
        if let Err(err) = image.draw(renderer, &scene) {
            warn!(?err, "scène du zoom de l'écran non dessinée");
            elements.extend(scene);
            return;
        }
        // Partie montrée, en pixels de l'image (son échelle est 1).
        let area = magnifier::visible_area(screen.to_f64(), pointer, zoom);
        let src = Rectangle::<f64, Logical>::new(
            Point::from((area.loc.x * scale, area.loc.y * scale)),
            Size::from((area.size.w * scale, area.size.h * scale)),
        );
        elements.push(OutputElement::Texture(
            TextureRenderElement::from_texture_render_buffer(
                Point::from((0.0, 0.0)),
                &image.buffer,
                None,
                Some(src),
                Some(screen),
                Kind::Unspecified,
            ),
        ));
    }

    /// Oublie les images (loupe arrêtée) : la mémoire du GPU est rendue.
    pub fn clear(&self) {
        let mut images = self.images.borrow_mut();
        if !images.is_empty() {
            images.clear();
        }
    }
}

impl SceneImage {
    fn new(
        renderer: &mut GlesRenderer,
        size: Size<i32, Physical>,
        scale: f64,
    ) -> Result<SceneImage, smithay::backend::renderer::gles::GlesError> {
        let texture: GlesTexture = renderer.create_buffer(
            Fourcc::Abgr8888,
            Size::<i32, Buffer>::from((size.w.max(1), size.h.max(1))),
        )?;
        Ok(SceneImage {
            size,
            scale,
            buffer: TextureRenderBuffer::from_texture(renderer, texture, 1, Transform::Normal, None),
            damage: OutputDamageTracker::new(size, scale, Transform::Normal),
            drawn: false,
        })
    }

    /// Redessine dans l'image ce qui a changé dans la scène.
    fn draw(&mut self, renderer: &mut GlesRenderer, scene: &[OutputElement]) -> Result<(), String> {
        let age = usize::from(self.drawn);
        let damage = &mut self.damage;
        let mut context = self.buffer.render();
        context.draw(|texture| {
            let mut target = renderer.bind(texture).map_err(|err| format!("{err:?}"))?;
            let result = damage
                .render_output(renderer, &mut target, age, scene, Color32F::TRANSPARENT)
                .map_err(|err| format!("{err:?}"))?;
            Ok::<_, String>(
                result
                    .damage
                    .map(|rects| {
                        rects
                            .iter()
                            .map(|rect| {
                                Rectangle::<i32, Buffer>::new(
                                    Point::from((rect.loc.x, rect.loc.y)),
                                    Size::from((rect.size.w, rect.size.h)),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            )
        })?;
        drop(context);
        self.drawn = true;
        Ok(())
    }
}
