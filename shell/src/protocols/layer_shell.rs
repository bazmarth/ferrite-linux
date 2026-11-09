//! `zwlr_layer_shell_v1` : barres, lanceurs, fonds d'écran et notifications d'autres
//! bureaux (waybar, fuzzel, swaybg, mako…). Leurs calques sont posés par écran : « fond »
//! et « bas » sous les fenêtres, « haut » au-dessus, « superposition » au-dessus de tout,
//! barre et Dock de Ferrite compris. Une zone exclusive (une barre) réduit la place des
//! fenêtres agrandies ou accrochées.

use smithay::{
    delegate_layer_shell,
    desktop::{LayerSurface, WindowSurfaceType, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface},
    utils::{Logical, Point},
    wayland::{
        compositor::with_states,
        shell::wlr_layer::{
            KeyboardInteractivity, Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData,
            WlrLayerShellHandler, WlrLayerShellState,
        },
    },
};
use tracing::warn;

use crate::state::Ferrite;

impl WlrLayerShellHandler for Ferrite {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        // Sans écran demandé : le premier.
        let output = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.space.outputs().next().cloned());
        let Some(output) = output else { return };
        let mut map = layer_map_for_output(&output);
        if let Err(err) = map.map_layer(&LayerSurface::new(surface, namespace)) {
            warn!(?err, "calque refusé");
        }
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let focused = self.keyboard_focus().as_ref() == Some(surface.wl_surface());
        for output in self.space.outputs() {
            let mut map = layer_map_for_output(output);
            let found = map
                .layers()
                .find(|layer| *layer.layer_surface() == surface)
                .cloned();
            if let Some(layer) = found {
                map.unmap_layer(&layer);
            }
        }
        // Le lanceur fermé rend le clavier à la fenêtre du dessus.
        if focused {
            let top = self.top_window();
            self.focus_window(top);
        }
        self.request_redraw();
    }
}

delegate_layer_shell!(Ferrite);

impl Ferrite {
    /// Validation d'une surface de calque : disposition, première configuration, et focus
    /// clavier pour un calque qui le veut en exclusivité (un lanceur).
    pub fn handle_layer_commit(&mut self, surface: &WlSurface) {
        let Some(output) = self
            .space
            .outputs()
            .find(|output| {
                layer_map_for_output(output)
                    .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                    .is_some()
            })
            .cloned()
        else {
            return;
        };
        let configured = with_states(surface, |states| {
            states
                .data_map
                .get::<LayerSurfaceData>()
                .is_some_and(|data| data.lock().is_ok_and(|data| data.initial_configure_sent))
        });
        let exclusive = {
            let mut map = layer_map_for_output(&output);
            map.arrange();
            let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL) else {
                return;
            };
            if !configured {
                layer.layer_surface().send_configure();
            }
            let state = layer.cached_state();
            state.keyboard_interactivity == KeyboardInteractivity::Exclusive
                && matches!(state.layer, Layer::Top | Layer::Overlay)
        };
        if exclusive && self.keyboard_focus().as_ref() != Some(surface) {
            self.set_keyboard_focus(Some(surface.clone()));
        }
    }

    /// Surface d'un calque des couches `layers` sous `pos`, avec la position de son origine.
    pub fn layer_surface_under(
        &self,
        pos: Point<f64, Logical>,
        layers: &[Layer],
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        for output in self.space.outputs() {
            let Some(output_geo) = self.space.output_geometry(output) else {
                continue;
            };
            let local = pos - output_geo.loc.to_f64();
            let map = layer_map_for_output(output);
            for layer in layers {
                let Some(found) = map.layer_under(*layer, local) else {
                    continue;
                };
                let Some(geometry) = map.layer_geometry(found) else {
                    continue;
                };
                if let Some((surface, location)) =
                    found.surface_under(local - geometry.loc.to_f64(), WindowSurfaceType::ALL)
                {
                    return Some((surface, (location + geometry.loc + output_geo.loc).to_f64()));
                }
            }
        }
        None
    }

    /// Un clic sur un calque qui accepte le clavier à la demande lui donne le focus.
    pub fn focus_layer_on_click(&mut self, surface: &WlSurface) {
        let wants_keyboard = self.space.outputs().any(|output| {
            layer_map_for_output(output)
                .layer_for_surface(surface, WindowSurfaceType::ALL)
                .is_some_and(LayerSurface::can_receive_keyboard_focus)
        });
        if wants_keyboard {
            self.set_keyboard_focus(Some(surface.clone()));
        }
    }
}
