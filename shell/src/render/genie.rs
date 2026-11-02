//! Rendu de l'animation « génie » (voir `wm::animation`) : la fenêtre est d'abord
//! dessinée une fois dans une texture (barre de titre et coins arrondis compris, sans
//! l'ombre), puis cette image est posée ligne par ligne entre les bords de l'entonnoir
//! par le shader `genie` de `shaders.rs`.

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Color32F, Frame, Offscreen, Renderer, Texture,
            element::{Element, Id, Kind, RenderElement, UnderlyingStorage},
            gles::{GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform},
            utils::{CommitCounter, draw_render_elements},
        },
    },
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform},
};

use super::OutputElement;
use crate::wm::animation::Genie;

/// Dessine `elements` (placés à partir de l'origine) dans une nouvelle texture de
/// `size` pixels physiques, sur fond transparent.
pub fn snapshot(
    renderer: &mut GlesRenderer,
    elements: &[OutputElement],
    size: Size<i32, Physical>,
    scale: f64,
) -> Result<GlesTexture, GlesError> {
    let buffer: Size<i32, Buffer> = (size.w.max(1), size.h.max(1)).into();
    let mut texture: GlesTexture = renderer.create_buffer(Fourcc::Abgr8888, buffer)?;
    {
        let mut target = renderer.bind(&mut texture)?;
        let full = [Rectangle::from_size(size)];
        let mut frame = renderer.render(&mut target, size, Transform::Normal)?;
        frame.clear(Color32F::TRANSPARENT, &full)?;
        draw_render_elements::<GlesRenderer, _, _>(&mut frame, scale, elements, &full)?;
        let _ = frame.finish()?;
    }
    Ok(texture)
}

/// Image de la fenêtre déformée, sur l'écran.
#[derive(Debug)]
pub struct GenieElement {
    id: Id,
    texture: GlesTexture,
    program: GlesTexProgram,
    /// Rectangle qui contient la fenêtre déformée, en pixels physiques de l'écran.
    geometry: Rectangle<i32, Physical>,
    uniforms: Vec<Uniform<'static>>,
    alpha: f32,
}

impl GenieElement {
    /// `genie` est en coordonnées logiques relatives à l'écran.
    pub fn new(texture: GlesTexture, program: GlesTexProgram, genie: &Genie, scale: f64) -> Self {
        let physical = |rect: Rectangle<f64, Logical>| {
            let rect = rect.to_physical(scale);
            [
                rect.loc.x as f32,
                rect.loc.y as f32,
                rect.size.w as f32,
                rect.size.h as f32,
            ]
        };
        let bounds = genie.bounds().to_physical(scale);
        // Arrondi vers l'extérieur, pour ne rien couper des bords.
        let loc = Point::<i32, Physical>::from((bounds.loc.x.floor() as i32, bounds.loc.y.floor() as i32));
        let end = (
            (bounds.loc.x + bounds.size.w).ceil() as i32,
            (bounds.loc.y + bounds.size.h).ceil() as i32,
        );
        let geometry = Rectangle::new(loc, (end.0 - loc.x, end.1 - loc.y).into());
        let uniforms = vec![
            Uniform::new(
                "geo",
                [
                    loc.x as f32,
                    loc.y as f32,
                    geometry.size.w as f32,
                    geometry.size.h as f32,
                ],
            ),
            Uniform::new("win", physical(genie.window)),
            Uniform::new("icon", physical(genie.icon)),
            Uniform::new("top", (genie.top * scale) as f32),
            Uniform::new("bottom", (genie.bottom * scale) as f32),
            Uniform::new("squeeze", genie.squeeze as f32),
        ];
        GenieElement {
            // Nouvel identifiant à chaque image : la forme change, tout est à redessiner.
            id: Id::new(),
            texture,
            program,
            geometry,
            uniforms,
            alpha: genie.alpha as f32,
        }
    }
}

impl Element for GenieElement {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        CommitCounter::default()
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        Rectangle::from_size(self.texture.size().to_f64())
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry
    }

    fn alpha(&self) -> f32 {
        self.alpha
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl RenderElement<GlesRenderer> for GenieElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        frame.render_texture_from_to(
            &self.texture,
            src,
            dst,
            damage,
            &[],
            Transform::Normal,
            self.alpha,
            Some(&self.program),
            &self.uniforms,
        )
    }

    fn underlying_storage(&self, _renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        None
    }
}
