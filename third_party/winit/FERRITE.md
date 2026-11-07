# winit 0.30.13, retouché pour Ferrite OS

Copie de winit 0.30.13 (crates.io), branchée par `[patch.crates-io]` dans le `Cargo.toml`
du dépôt. Une seule retouche : sous Wayland, le pincement du pavé tactile
(`zwp_pointer_gestures_v1`) devient `WindowEvent::PinchGesture`, que winit 0.30 ne
fournissait que sur macOS et iOS. egui le lit comme un zoom.

Fichiers touchés :
- `src/platform_impl/linux/wayland/seat/pointer/pointer_gestures.rs` (nouveau) ;
- `src/platform_impl/linux/wayland/seat/pointer/mod.rs`, `seat/mod.rs`, `state.rs`.

Le dossier `docs/` du paquet d'origine a été retiré (inutile à la compilation).
À retirer quand une version de winit gérera le pincement sous Wayland.
