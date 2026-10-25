//! Surface d'un client dessinée avec les coins du bas arrondis (shader `rounded` de
//! `shaders.rs`).

use smithay::{
    backend::renderer::{
        element::{
            Element, Id, Kind, RenderElement, UnderlyingStorage, surface::WaylandSurfaceRenderElement,
        },
        gles::{GlesError, GlesFrame, GlesRenderer, GlesTexProgram, Uniform},
        utils::{CommitCounter, DamageSet, OpaqueRegions},
    },
    utils::{Buffer, Physical, Point, Rectangle, Scale, Transform},
};

#[derive(Debug)]
pub struct RoundedSurface {
    inner: WaylandSurfaceRenderElement<GlesRenderer>,
    program: GlesTexProgram,
    /// Contenu de la fenêtre, en pixels physiques (repère de l'écran).
    clip: Rectangle<i32, Physical>,
    /// Rayon des coins, en pixels physiques.
    radius: f32,
}

impl RoundedSurface {
    pub fn new(
        inner: WaylandSurfaceRenderElement<GlesRenderer>,
        program: GlesTexProgram,
        clip: Rectangle<i32, Physical>,
        radius: f32,
    ) -> Self {
        RoundedSurface {
            inner,
            program,
            clip,
            radius,
        }
    }
}

impl Element for RoundedSurface {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn damage_since(&self, scale: Scale<f64>, commit: Option<CommitCounter>) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    // Les coins sont transparents : aucune zone n'est déclarée opaque, sinon ce qui est
    // derrière (ombre, fond d'écran) ne serait pas dessiné.
    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::default()
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl RenderElement<GlesRenderer> for RoundedSurface {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        let rect =
            |r: Rectangle<i32, Physical>| [r.loc.x as f32, r.loc.y as f32, r.size.w as f32, r.size.h as f32];
        frame.override_default_tex_program(
            self.program.clone(),
            vec![
                Uniform::new("geo", rect(dst)),
                Uniform::new("clip", rect(self.clip)),
                Uniform::new("radius", self.radius),
            ],
        );
        // Sans zones opaques : le mélange alpha reste actif partout, coins compris.
        let result = self.inner.draw(frame, src, dst, damage, &[]);
        frame.clear_tex_program_override();
        result
    }

    fn underlying_storage(&self, _renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        None
    }
}
