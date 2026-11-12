# winit 0.30.13, retouché pour Ferrite OS

Copie de winit 0.30.13 (crates.io), branchée par `[patch.crates-io]` dans le `Cargo.toml`
du dépôt. Deux retouches sous Wayland :
- le pincement du pavé tactile (`zwp_pointer_gestures_v1`) devient
  `WindowEvent::PinchGesture`, que winit 0.30 ne fournissait que sur macOS et iOS. egui le
  lit comme un zoom ;
- `Window::focus_window` (commande `ViewportCommand::Focus` d'egui) demande le premier plan
  par `xdg_activation` au lieu de ne rien faire : une application à fenêtre unique qui
  reçoit un fichier d'un second lancement revient ainsi devant.

Fichiers touchés :
- `src/platform_impl/linux/wayland/seat/pointer/pointer_gestures.rs` (nouveau) ;
- `src/platform_impl/linux/wayland/seat/pointer/mod.rs`, `seat/mod.rs`, `state.rs` ;
- `src/platform_impl/linux/wayland/window/mod.rs` (`focus_window`).

Le dossier `docs/` du paquet d'origine a été retiré (inutile à la compilation).
À retirer quand une version de winit gérera le pincement et le premier plan sous Wayland.
