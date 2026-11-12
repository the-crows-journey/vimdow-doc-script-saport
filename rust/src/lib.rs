use godot::prelude::*;

mod docs;
mod neovim;
mod render;

struct Extension;

#[gdextension]
unsafe impl ExtensionLibrary for Extension {}
