//! World management — chunks, blocks, terrain generation.

pub mod block;
pub mod block_models;
pub mod chunk;
pub mod color;
pub mod item;
pub mod light;
pub mod material;
pub mod mesh;
pub mod network;
pub mod shape;

mod blocks;
mod entities;
mod helpers;
mod lighting;
mod mesh_build;
mod mesh_jobs;
mod mesh_queue;
mod queries;
mod snapshot;
mod state;

#[cfg(test)]
mod tests;

pub(crate) use entities::is_chest_block;
pub use entities::{ChestBlockEntity, SkullBlockEntity};
pub(crate) use helpers::block_change_affects_light;
pub(crate) use state::{MeshBuildResult, MeshChunkSnapshot, MeshJobToken, SnapshotChanges};
pub use state::{SnapshotRegionRevision, World};
