//! Asset loading — textures, models, resource packs.
use std::path::{Path, PathBuf};

pub mod index;
pub mod model;
pub mod resolver;
pub mod resource_pack;
pub mod skin;
pub mod sound;
pub mod texture;

/// Resolve a runtime asset path without assuming that the process was started
/// from the workspace root.
///
/// Normal game startup selects a runtime directory containing the assets.
/// Cargo test binaries instead run from `crates/game`, so missing relative
/// paths fall back to the workspace root. Existing paths always win, preserving
/// custom runtime directories and packaged installations.
pub(crate) fn resolve_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() || path.exists() {
        return path.to_path_buf();
    }

    let workspace_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    if workspace_path.exists() {
        workspace_path
    } else {
        path.to_path_buf()
    }
}
