use super::mesh_jobs::build_mesh_from_snapshot;
use super::*;
use crate::world::mesh::{ChunkMesh, MeshOptions};
use crate::world::{block::Block, chunk::Chunk};
use std::sync::Arc;

fn begin_tracked_mesh_job(world: &mut World, position: (i32, i32)) -> MeshJobToken {
    world.enqueue_chunk_mesh(position.0, position.1);
    assert_eq!(world.pending_mesh_queue.pop_front(), Some(position));
    let token = world.mesh_job_token(position);
    assert!(world
        .pending_mesh_inflight
        .insert(position, token)
        .is_none());
    token
}

fn begin_tracked_priority_mesh_job(world: &mut World, position: (i32, i32)) -> MeshJobToken {
    world.enqueue_chunk_mesh_priority(position.0, position.1);
    assert_eq!(
        world.pending_mesh_priority_queue.pop_front(),
        Some(position)
    );
    let token = world.mesh_job_token(position);
    assert!(world
        .pending_mesh_inflight
        .insert(position, token)
        .is_none());
    assert!(world.pending_mesh_priority_inflight.insert(position));
    token
}

fn send_test_mesh_result(world: &World, position: (i32, i32), token: MeshJobToken) {
    world
        .mesh_result_tx
        .send(MeshBuildResult {
            token,
            mesh: ChunkMesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                cx: position.0,
                cz: position.1,
                transparent_start: 0,
                aabb_min: [f32::MAX; 3],
                aabb_max: [f32::MIN; 3],
            },
            light: None,
        })
        .unwrap();
}

#[test]
fn resource_reload_rejects_meshes_with_the_previous_atlas_uvs() {
    let mut world = World::new();
    let position = (0, 0);
    world.chunks.insert(position, Arc::new(Chunk::new(0, 0)));
    let old_token = begin_tracked_mesh_job(&mut world, position);
    world.pending_mesh_dirty.insert(position);
    world.pending_mesh_priority_queue.push_back(position);
    world.pending_mesh_priority_inflight.insert(position);
    world.pending_mesh_priority_dirty.insert(position);
    world.pending_mesh_removals.push_back(position);

    world.invalidate_mesh_jobs_for_resource_reload();
    send_test_mesh_result(&world, position, old_token);

    assert!(world.pending_mesh_queue.is_empty());
    assert!(world.pending_mesh_priority_queue.is_empty());
    assert!(world.pending_mesh_removals.is_empty());
    assert!(world.pending_mesh_inflight.is_empty());
    assert!(world.pending_mesh_dirty.is_empty());
    assert!(world.pending_mesh_priority_inflight.is_empty());
    assert!(world.pending_mesh_priority_dirty.is_empty());
    assert!(world.poll_finished_meshes().is_empty());
}

#[test]
fn scripting_snapshot_revision_tracks_world_content_changes() {
    let mut world = World::new();
    world.chunks.insert((0, 0), Arc::new(Chunk::new(0, 0)));

    let initial = world.snapshot_revision();
    world.set_block_state(0, 64, 0, 1 << 4);
    assert_ne!(world.snapshot_revision(), initial);

    let after_block = world.snapshot_revision();
    world.recompute_chunk_light(0, 0);
    assert_ne!(world.snapshot_revision(), after_block);

    let after_light = world.snapshot_revision();
    world.unload_chunk(0, 0);
    assert_ne!(world.snapshot_revision(), after_light);
}

#[test]
fn identical_server_block_state_is_a_complete_no_op() {
    let mut chunk = Chunk::new(0, 0);
    chunk.set_state(1, 64, 1, Block::Stone.to_id() << 4);
    let mut world = World::new();
    world.chunks.insert((0, 0), Arc::new(chunk));
    let shared_snapshot = world.chunks[&(0, 0)].clone();
    world.take_snapshot_changes();
    let revision = world.snapshot_revision();

    assert!(!world.set_block_state(1, 64, 1, Block::Stone.to_id() << 4));
    assert!(Arc::ptr_eq(
        &shared_snapshot,
        world.chunks.get(&(0, 0)).unwrap()
    ));
    assert_eq!(world.snapshot_revision(), revision);
    let changes = world.take_snapshot_changes();
    assert!(!changes.all && changes.blocks.is_empty() && changes.chunks.is_empty());

    world.apply_block_change(1, 64, 1, Block::Stone.to_id() << 4);
    assert!(world.pending_mesh_queue.is_empty());
}

#[test]
fn nearby_snapshot_revision_ignores_distant_chunk_changes() {
    let mut world = World::new();
    world.chunks.insert((0, 0), Arc::new(Chunk::new(0, 0)));
    world.chunks.insert((20, 20), Arc::new(Chunk::new(20, 20)));
    let nearby = world.snapshot_revision_around((0, 64, 0), 15);

    assert!(world.set_block_state(320, 64, 320, Block::Stone.to_id() << 4));
    assert_eq!(world.snapshot_revision_around((0, 64, 0), 15), nearby);

    assert!(world.set_block_state(0, 64, 0, Block::Stone.to_id() << 4));
    assert_ne!(world.snapshot_revision_around((0, 64, 0), 15), nearby);
}

#[test]
fn equal_light_properties_preserve_server_light_nibbles() {
    let mut chunk = Chunk::new(0, 0);
    chunk.set_state(1, 64, 1, Block::Stone.to_id() << 4);
    chunk.finish_network_light(true, true);
    let mut world = World::new();
    world.chunks.insert((0, 0), Arc::new(chunk));

    assert!(world.set_block_state(1, 64, 1, Block::Dirt.to_id() << 4));
    assert!(world.chunks[&(0, 0)].has_valid_network_light());

    assert!(world.set_block_state(1, 64, 1, Block::Torch.to_id() << 4));
    assert!(!world.chunks[&(0, 0)].has_valid_network_light());
}

#[test]
fn light_changes_recompute_and_invalidate_every_reachable_chunk() {
    let mut world = World::new();
    for cx in -1..=1 {
        let mut chunk = Chunk::new(cx, 0);
        chunk.finish_network_light(true, true);
        world.chunks.insert((cx, 0), Arc::new(chunk));
    }
    let neighbor_revision = world.snapshot_revision_around((16, 64, 8), 0);

    assert!(world.set_block_state_and_relight(8, 64, 8, Block::Torch.to_id() << 4));
    assert!(!world.chunks[&(1, 0)].has_valid_network_light());
    assert_ne!(
        world.snapshot_revision_around((16, 64, 8), 0),
        neighbor_revision
    );
    assert!(world.light.chunk(1, 0).unwrap().get(0, 64, 8).block > 0);

    assert!(world.set_block_state_and_relight(8, 64, 8, Block::Air.to_id() << 4));
    assert_eq!(world.light.chunk(1, 0).unwrap().get(0, 64, 8).block, 0);
}

#[test]
fn network_block_change_keeps_visible_light_until_the_mesh_job_replaces_it() {
    let mut world = World::new();
    for cx in -1..=1 {
        let mut chunk = Chunk::new(cx, 0);
        chunk.finish_network_light(true, true);
        world.chunks.insert((cx, 0), Arc::new(chunk));
    }

    world.apply_block_change(8, 64, 8, Block::Torch.to_id() << 4);
    assert!(world.light.chunk(0, 0).is_some());
    assert!(world.dirty_light_chunks.contains(&(0, 0)));
    assert_eq!(world.light_at_world(8, 64, 8).sky, 15);
    assert!(!world.chunks[&(0, 0)].has_valid_network_light());
    assert!(world.pending_mesh_priority_queue.contains(&(0, 0)));
    assert!(!world.pending_mesh_queue.contains(&(0, 0)));

    let result = build_mesh_from_snapshot(
        0,
        0,
        world.mesh_snapshot(0, 0),
        MeshOptions::default(),
        world.mesh_job_token((0, 0)),
    );
    assert!(result
        .light
        .as_ref()
        .is_some_and(|light| light.get(8, 64, 8).block > 0));
}

#[test]
fn placing_an_opaque_block_does_not_black_out_neighboring_daylight() {
    let mut world = World::new();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut chunk = Chunk::new(cx, cz);
            chunk.finish_network_light(true, true);
            world.chunks.insert((cx, cz), Arc::new(chunk));
        }
    }

    world.apply_block_change(8, 64, 8, Block::Stone.to_id() << 4);
    let rebuilt = build_mesh_from_snapshot(
        0,
        0,
        world.mesh_snapshot(0, 0),
        MeshOptions::default(),
        world.mesh_job_token((0, 0)),
    )
    .light
    .expect("the dirty chunk must publish replacement light");

    assert_eq!(rebuilt.get(9, 64, 8).sky, 15);
    assert_eq!(rebuilt.get(8, 65, 8).sky, 15);
}

#[test]
fn background_light_crosses_chunk_edges_in_both_directions() {
    let mut world = World::new();
    for cx in 0..=1 {
        let mut chunk = Chunk::new(cx, 0);
        chunk.finish_network_light(true, true);
        world.chunks.insert((cx, 0), Arc::new(chunk));
    }

    world.apply_block_change(15, 64, 8, Block::Torch.to_id() << 4);
    let lit = build_mesh_from_snapshot(
        1,
        0,
        world.mesh_snapshot(1, 0),
        MeshOptions::default(),
        world.mesh_job_token((1, 0)),
    )
    .light
    .expect("the invalid neighboring chunk must return computed light");
    assert!(lit.get(0, 64, 8).block > 0);

    world.apply_block_change(15, 64, 8, Block::Air.to_id() << 4);
    let dark = build_mesh_from_snapshot(
        1,
        0,
        world.mesh_snapshot(1, 0),
        MeshOptions::default(),
        world.mesh_job_token((1, 0)),
    )
    .light
    .expect("removing the source must recompute neighboring light");
    assert_eq!(dark.get(0, 64, 8).block, 0);
}

#[test]
fn installed_local_light_is_reused_by_later_mesh_jobs() {
    let mut chunk = Chunk::new(0, 0);
    chunk.set_state(1, 64, 1, Block::Torch.to_id() << 4);
    let mut world = World::new();
    world.chunks.insert((0, 0), Arc::new(chunk));
    world.recompute_chunk_light(0, 0);

    let result = build_mesh_from_snapshot(
        0,
        0,
        world.mesh_snapshot(0, 0),
        MeshOptions::default(),
        world.mesh_job_token((0, 0)),
    );

    assert!(result.light.is_none());
}

#[test]
fn mesh_job_uses_the_captured_network_light_validity() {
    let mut chunk = Chunk::new(0, 0);
    chunk.set_block_light(1, 64, 1, 11);
    chunk.finish_network_light(true, true);
    let mut world = World::new();
    world.chunks.insert((0, 0), Arc::new(chunk));
    let snapshot = world.mesh_snapshot(0, 0);

    world.chunks[&(0, 0)].invalidate_network_light();
    let result = build_mesh_from_snapshot(
        0,
        0,
        snapshot,
        MeshOptions::default(),
        world.mesh_job_token((0, 0)),
    );

    assert!(result.light.is_none());
}

#[test]
fn snapshot_revision_handles_extreme_coordinates_and_radius() {
    let world = World::new();
    let revision = world.snapshot_revision_around((i32::MAX, 0, i32::MIN), i32::MAX);
    assert!(revision.chunks.is_empty());
    assert_eq!(revision.epoch, world.snapshot_revision());
}

#[test]
fn unload_reload_rejects_the_old_mesh_job_without_clearing_the_new_one() {
    let position = (0, 0);
    let mut world = World::new();
    world
        .chunks
        .insert(position, Arc::new(Chunk::new(position.0, position.1)));
    let old_token = begin_tracked_mesh_job(&mut world, position);

    world.unload_chunk(position.0, position.1);
    world
        .chunks
        .insert(position, Arc::new(Chunk::new(position.0, position.1)));
    let new_token = begin_tracked_mesh_job(&mut world, position);
    assert_ne!(old_token, new_token);

    send_test_mesh_result(&world, position, old_token);
    assert!(world.poll_finished_meshes().is_empty());
    assert_eq!(world.pending_mesh_inflight.get(&position), Some(&new_token));

    send_test_mesh_result(&world, position, new_token);
    assert_eq!(world.poll_finished_meshes().len(), 1);
    assert!(!world.pending_mesh_inflight.contains_key(&position));
}

#[test]
fn synchronous_mutation_and_rebuild_reject_an_older_mesh_result() {
    let position = (0, 0);
    let mut world = World::new();
    world
        .chunks
        .insert(position, Arc::new(Chunk::new(position.0, position.1)));
    let old_token = begin_tracked_mesh_job(&mut world, position);

    assert!(world.set_block_state(1, 64, 1, Block::Stone.to_id() << 4));
    assert!(!world.rebuild_chunk_at(1, 64, 1).is_empty());
    assert_ne!(old_token, world.mesh_job_token(position));

    send_test_mesh_result(&world, position, old_token);
    assert!(world.poll_finished_meshes().is_empty());
    assert!(!world.pending_mesh_inflight.contains_key(&position));
    assert!(!world.pending_mesh_queue.contains(&position));
}

#[test]
fn world_epoch_rejects_a_result_sent_after_world_reset() {
    let position = (0, 0);
    let mut world = World::new();
    world
        .chunks
        .insert(position, Arc::new(Chunk::new(position.0, position.1)));
    let old_token = begin_tracked_mesh_job(&mut world, position);

    world.clear_server_world();
    assert!(world.pending_mesh_queue.is_empty());
    assert!(world.pending_mesh_priority_queue.is_empty());
    assert!(world.pending_mesh_inflight.is_empty());
    assert!(world.pending_mesh_priority_inflight.is_empty());
    assert!(world.pending_mesh_dirty.is_empty());
    assert!(world.pending_mesh_priority_dirty.is_empty());
    world
        .chunks
        .insert(position, Arc::new(Chunk::new(position.0, position.1)));
    let new_token = begin_tracked_mesh_job(&mut world, position);
    assert_ne!(old_token.epoch, new_token.epoch);

    // Models an old rayon job that completes only after clear_server_world
    // has drained the results which were already available.
    send_test_mesh_result(&world, position, old_token);
    assert!(world.poll_finished_meshes().is_empty());
    assert_eq!(world.pending_mesh_inflight.get(&position), Some(&new_token));
}

#[test]
fn dirty_inflight_mesh_result_requeues_the_latest_generation() {
    let position = (0, 0);
    let backlog = (1, 0);
    let mut world = World::new();
    world
        .chunks
        .insert(position, Arc::new(Chunk::new(position.0, position.1)));
    world
        .chunks
        .insert(backlog, Arc::new(Chunk::new(backlog.0, backlog.1)));
    let old_token = begin_tracked_mesh_job(&mut world, position);
    world.enqueue_chunk_mesh(backlog.0, backlog.1);

    world.enqueue_chunk_mesh(position.0, position.1);
    assert!(world.pending_mesh_dirty.contains(&position));
    assert_ne!(old_token, world.mesh_job_token(position));

    send_test_mesh_result(&world, position, old_token);
    assert!(world.poll_finished_meshes().is_empty());
    assert!(!world.pending_mesh_inflight.contains_key(&position));
    assert!(!world.pending_mesh_dirty.contains(&position));
    assert_eq!(world.pending_mesh_queue.front(), Some(&backlog));
    assert_eq!(world.pending_mesh_queue.back(), Some(&position));
}

#[test]
fn interactive_block_change_overtakes_streaming_mesh_backlog() {
    let changed = (0, 0);
    let backlog = (5, 5);
    let mut world = World::new();
    for position in [changed, backlog] {
        world
            .chunks
            .insert(position, Arc::new(Chunk::new(position.0, position.1)));
    }
    world.enqueue_chunk_mesh(backlog.0, backlog.1);

    world.enqueue_chunk_mesh_at_block(8, 8);

    assert_eq!(world.pending_mesh_priority_queue.front(), Some(&changed));
    assert!(!world.pending_mesh_queue.contains(&changed));
    assert!(world.pending_mesh_queue.contains(&backlog));
}

#[test]
fn priority_mesh_job_does_not_consume_normal_streaming_slots() {
    let priority = (0, 0);
    let normal_a = (1, 0);
    let normal_b = (2, 0);
    let mut world = World::new();
    for position in [priority, normal_a, normal_b] {
        world
            .chunks
            .insert(position, Arc::new(Chunk::new(position.0, position.1)));
    }
    world.enqueue_chunk_mesh(normal_a.0, normal_a.1);
    world.enqueue_chunk_mesh(normal_b.0, normal_b.1);
    world.enqueue_chunk_mesh_priority(priority.0, priority.1);

    world.schedule_background_meshes();

    assert_eq!(world.pending_mesh_inflight.len(), 3);
    assert_eq!(world.pending_mesh_priority_inflight.len(), 1);
    assert!(world.pending_mesh_priority_inflight.contains(&priority));
    assert!(world.pending_mesh_inflight.contains_key(&normal_a));
    assert!(world.pending_mesh_inflight.contains_key(&normal_b));
}

#[test]
fn multi_block_change_stays_on_the_normal_mesh_lane() {
    let position = (0, 0);
    let mut chunk = Chunk::new(position.0, position.1);
    chunk.set_state(8, 64, 8, Block::Stone.to_id() << 4);
    let mut world = World::new();
    world.chunks.insert(position, Arc::new(chunk));
    let raw_position = (8 << 12) | (8 << 8) | 64;

    world.apply_multi_block_change(
        position.0,
        position.1,
        &[(raw_position, Block::Dirt.to_id() << 4)],
    );

    assert!(world.pending_mesh_priority_queue.is_empty());
    assert!(world.pending_mesh_queue.contains(&position));
}

#[test]
fn dirty_priority_mesh_job_requeues_on_the_priority_lane() {
    let position = (0, 0);
    let mut world = World::new();
    world
        .chunks
        .insert(position, Arc::new(Chunk::new(position.0, position.1)));
    let old_token = begin_tracked_priority_mesh_job(&mut world, position);

    world.enqueue_chunk_mesh(position.0, position.1);
    assert!(world.pending_mesh_dirty.contains(&position));

    send_test_mesh_result(&world, position, old_token);
    assert!(world.poll_finished_meshes().is_empty());
    assert_eq!(world.pending_mesh_priority_queue.front(), Some(&position));
    assert!(!world.pending_mesh_queue.contains(&position));
}

#[test]
fn interactive_update_upgrades_an_inflight_normal_mesh_job() {
    let position = (0, 0);
    let mut world = World::new();
    world
        .chunks
        .insert(position, Arc::new(Chunk::new(position.0, position.1)));
    let old_token = begin_tracked_mesh_job(&mut world, position);

    world.enqueue_chunk_mesh_priority(position.0, position.1);
    assert!(world.pending_mesh_priority_dirty.contains(&position));

    send_test_mesh_result(&world, position, old_token);
    assert!(world.poll_finished_meshes().is_empty());
    assert_eq!(world.pending_mesh_priority_queue.front(), Some(&position));
    assert!(!world.pending_mesh_queue.contains(&position));
}
