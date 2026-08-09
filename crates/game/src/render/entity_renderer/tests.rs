use super::meshes::*;

fn standing_sign(metadata: u8) -> crate::render::scene::SignEntry {
    crate::render::scene::SignEntry {
        position: [0, 0, 0],
        lines: Vec::new(),
        wall_mounted: false,
        metadata,
    }
}

#[test]
fn standing_sign_text_follows_the_board_rotation() {
    let (_, north) = sign_text_plane(&standing_sign(0));
    let (_, west) = sign_text_plane(&standing_sign(4));
    let (_, south) = sign_text_plane(&standing_sign(8));

    assert_eq!(north, [0.0, 0.0, 1.0]);
    assert!((west[0] + 1.0).abs() < 0.000_01 && west[2].abs() < 0.000_01);
    assert!(south[0].abs() < 0.000_01 && (south[2] + 1.0).abs() < 0.000_01);
}

#[test]
fn fishing_line_uses_the_vanilla_seventeen_point_curve() {
    let points = fishing_line_points([1.0, 2.0, 3.0], [5.0, 10.0, 11.0]);
    assert_eq!(points.len(), 17);
    assert_eq!(points[0], [1.0, 2.0, 3.0]);
    assert_eq!(points[16], [5.0, 10.0, 11.0]);
    assert_eq!(points[8], [3.0, 5.0, 7.0]);
}

#[test]
fn item_sprite_uses_item_atlas_layers_and_is_two_sided() {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    append_item_sprite_layers(
        &mut vertices,
        &mut indices,
        crate::world::item::Item::Feather.to_id(),
        0,
        [
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
        [0.0, 0.0, 1.0],
        packed_light_for(15, 0),
    );

    assert_eq!(vertices.len(), 4);
    assert_eq!(indices.len(), 12);
    assert!(vertices.iter().all(|vertex| {
        (0.0..=1.0).contains(&vertex.uv[0]) && (0.0..=1.0).contains(&vertex.uv[1])
    }));
}
