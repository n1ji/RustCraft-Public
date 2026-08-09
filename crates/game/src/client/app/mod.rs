mod assets;
mod backend;
mod block_interaction;
mod book;
mod bootstrap;
mod frame;
mod input;
mod inventory_interaction;
mod lifecycle;
mod menu_actions;
mod script_bridge;
mod state;
mod tasks;

pub use assets::ResourcePackInfo;
pub use lifecycle::run;
pub use state::App;

pub(super) const WINDOW_TITLE: &str = concat!("RustCraft v", env!("CARGO_PKG_VERSION"));

use backend::RenderBackend;

pub(super) use assets::MC_PACK_FORMAT;
pub(super) use assets::{
    apply_audio_config, load_local_cape_pixels, load_local_skin, scan_resource_packs,
};
pub(super) use state::{
    AppFrameProfile, PendingShaderPackLoad, PlayerSkinProfileCache, ShaderPackLoadReason,
};
