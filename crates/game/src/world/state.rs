use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{mpsc, Arc, Mutex};

use super::block::Block;
use super::chunk::{Chunk, CHUNK_HEIGHT, CHUNK_SIZE};
use super::entities::{ChestBlockEntity, SkullBlockEntity};
use super::mesh::{ChunkMesh, MeshOptions};
use super::{color, light};

pub(crate) const MAX_NORMAL_BACKGROUND_MESH_JOBS: usize = 2;
pub(crate) const MAX_PRIORITY_BACKGROUND_MESH_JOBS: usize = 1;
pub(crate) const MAX_MESH_RESULTS_PER_FRAME: usize =
    MAX_NORMAL_BACKGROUND_MESH_JOBS + MAX_PRIORITY_BACKGROUND_MESH_JOBS;
pub(crate) const MAX_MESH_REMOVALS_PER_FRAME: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MeshJobToken {
    pub(crate) epoch: u64,
    pub(crate) generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRegionRevision {
    pub(crate) epoch: u64,
    pub(crate) chunks: Vec<((i32, i32), u64)>,
}

#[derive(Debug, Default)]
pub(crate) struct SnapshotChanges {
    pub all: bool,
    pub chunks: HashSet<(i32, i32)>,
    pub blocks: HashSet<(i32, i32, i32)>,
}

pub(crate) struct MeshBuildResult {
    pub(crate) token: MeshJobToken,
    pub(crate) mesh: ChunkMesh,
    pub(crate) light: Option<Arc<light::ChunkLight>>,
}

#[derive(Clone)]
pub(crate) struct MeshChunkSnapshot {
    pub(crate) chunk: Arc<Chunk>,
    pub(crate) network_light_valid: bool,
    pub(crate) local_light: Option<Arc<light::ChunkLight>>,
    /// The local light is still safe as a presentation fallback, but a worker
    /// must replace it before publishing this mesh generation.
    pub(crate) light_dirty: bool,
}

pub struct World {
    pub chunks: HashMap<(i32, i32), Arc<Chunk>>,
    pub skulls: HashMap<(i32, i32, i32), SkullBlockEntity>,
    pub chests: HashMap<(i32, i32, i32), ChestBlockEntity>,
    /// Changes only when chest topology or block metadata can invalidate the
    /// static chest render entries. Lid animation is intentionally excluded.
    pub chest_structure_generation: u64,
    pub grass_color: color::ColorMap,
    pub foliage_color: color::ColorMap,
    pub light: light::WorldLight,
    /// Monotonic generation for block, biome, and light data exposed to Lua.
    /// Render-only state deliberately does not participate in this counter.
    pub(crate) snapshot_revision: u64,
    /// Full-world generation. A reset invalidates every cached scripting region.
    pub(crate) snapshot_epoch: u64,
    /// Last content generation for each chunk, including unloaded tombstones.
    pub(crate) snapshot_chunk_revisions: HashMap<(i32, i32), u64>,
    /// Exact block changes since the last scripting snapshot publication.
    pub(crate) snapshot_dirty_blocks: HashSet<(i32, i32, i32)>,
    /// Chunk-wide changes (load/unload/lighting) since the last publication.
    pub(crate) snapshot_dirty_chunks: HashSet<(i32, i32)>,
    pub(crate) snapshot_all_dirty: bool,
    pub mesh_options: MeshOptions,
    /// When true, chunk data changes skip mesh building (used during loading).
    pub defer_mesh_build: bool,
    /// Chunks that still need their mesh built (incremental build queue).
    pub pending_mesh_queue: VecDeque<(i32, i32)>,
    /// Interactive block changes get one dedicated slot without starving
    /// normal chunk streaming work.
    pub(crate) pending_mesh_priority_queue: VecDeque<(i32, i32)>,
    /// Chunks whose GPU mesh should be removed incrementally.
    pub pending_mesh_removals: VecDeque<(i32, i32)>,
    /// Full-world generation for background mesh jobs. World resets advance
    /// this even when a new world immediately reuses the same chunk position.
    pub(crate) mesh_epoch: u64,
    /// Per-position content generation within the current mesh epoch.
    pub(crate) mesh_generations: HashMap<(i32, i32), u64>,
    pub(crate) pending_mesh_inflight: HashMap<(i32, i32), MeshJobToken>,
    pub(crate) pending_mesh_dirty: HashSet<(i32, i32)>,
    pub(crate) pending_mesh_priority_inflight: HashSet<(i32, i32)>,
    pub(crate) pending_mesh_priority_dirty: HashSet<(i32, i32)>,
    /// Chunks whose visible light keeps the last complete snapshot while a
    /// replacement is calculated off-thread. Rendering must never observe an
    /// absent/zero-filled intermediate light state.
    pub(crate) dirty_light_chunks: HashSet<(i32, i32)>,
    pub(crate) mesh_result_tx: mpsc::Sender<MeshBuildResult>,
    pub(crate) mesh_result_rx: Mutex<mpsc::Receiver<MeshBuildResult>>,
}

impl World {
    pub fn new() -> Self {
        let grass_color = color::ColorMap::load("assets/minecraft/textures/colormap/grass.png")
            .expect("Failed to load grass color map");
        let foliage_color = color::ColorMap::load("assets/minecraft/textures/colormap/foliage.png")
            .expect("Failed to load foliage color map");

        let (mesh_result_tx, mesh_result_rx) = mpsc::channel();
        World {
            chunks: HashMap::new(),
            skulls: HashMap::new(),
            chests: HashMap::new(),
            chest_structure_generation: 0,
            grass_color,
            foliage_color,
            light: light::WorldLight::new(),
            snapshot_revision: 0,
            snapshot_epoch: 0,
            snapshot_chunk_revisions: HashMap::new(),
            snapshot_dirty_blocks: HashSet::new(),
            snapshot_dirty_chunks: HashSet::new(),
            snapshot_all_dirty: false,
            mesh_options: MeshOptions::default(),
            defer_mesh_build: false,
            pending_mesh_queue: VecDeque::new(),
            pending_mesh_priority_queue: VecDeque::new(),
            pending_mesh_removals: VecDeque::new(),
            mesh_epoch: 0,
            mesh_generations: HashMap::new(),
            pending_mesh_inflight: HashMap::new(),
            pending_mesh_dirty: HashSet::new(),
            pending_mesh_priority_inflight: HashSet::new(),
            pending_mesh_priority_dirty: HashSet::new(),
            dirty_light_chunks: HashSet::new(),
            mesh_result_tx,
            mesh_result_rx: Mutex::new(mesh_result_rx),
        }
    }
}
