use super::block::Block;
use super::helpers::{nbt_u8, normalize_uuid};
use super::state::World;

#[derive(Clone, Copy, Debug, Default)]
pub struct ChestBlockEntity {
    pub viewers: i32,
    pub prev_lid_angle: f32,
    pub lid_angle: f32,
}

pub(crate) fn is_chest_block(block: Block) -> bool {
    matches!(
        block,
        Block::Chest | Block::TrappedChest | Block::EnderChest
    )
}

#[derive(Clone, Debug, Default)]
pub struct SkullBlockEntity {
    pub skull_type: u8,
    pub rotation: u8,
    pub owner_uuid: Option<String>,
    pub owner_name: Option<String>,
    pub skin_property: Option<String>,
}

impl World {
    pub fn apply_block_entity_update(&mut self, x: i32, y: i32, z: i32, action: u8, nbt: &[u8]) {
        // Vanilla 1.8.9 S35 action 4 = skull tile entity.
        if action != 4 || self.get_block(x, y, z) != Block::Skull {
            return;
        }
        match crate::net::nbt::parse_root(nbt) {
            Ok(tag) => {
                if let Some(skull) = SkullBlockEntity::from_nbt(&tag) {
                    self.skulls.insert((x, y, z), skull);
                }
            }
            Err(err) => {
                log::warn!("failed to parse skull tile entity at ({x},{y},{z}): {err}")
            }
        }
    }
}

impl SkullBlockEntity {
    fn from_nbt(tag: &crate::net::nbt::NbtTag) -> Option<Self> {
        let root = tag.as_compound()?;
        let skull_type = nbt_u8(root, "SkullType").unwrap_or(0);
        let rotation = nbt_u8(root, "Rot").unwrap_or(0);

        let mut owner_uuid = None;
        let mut owner_name = None;
        let mut skin_property = None;

        if let Some(owner) = root
            .get("Owner")
            .and_then(crate::net::nbt::NbtTag::as_compound)
        {
            owner_uuid = owner
                .get("Id")
                .and_then(crate::net::nbt::NbtTag::as_str)
                .filter(|s| !s.is_empty())
                .map(normalize_uuid);
            owner_name = owner
                .get("Name")
                .and_then(crate::net::nbt::NbtTag::as_str)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned);
            skin_property = owner
                .get("Properties")
                .and_then(crate::net::nbt::NbtTag::as_compound)
                .and_then(|props| props.get("textures"))
                .and_then(crate::net::nbt::NbtTag::as_list)
                .and_then(|textures| textures.first())
                .and_then(crate::net::nbt::NbtTag::as_compound)
                .and_then(|texture| texture.get("Value"))
                .and_then(crate::net::nbt::NbtTag::as_str)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned);
        }

        if owner_name.is_none() {
            owner_name = root
                .get("ExtraType")
                .and_then(crate::net::nbt::NbtTag::as_str)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned);
        }

        Some(Self {
            skull_type,
            rotation,
            owner_uuid,
            owner_name,
            skin_property,
        })
    }
}

impl World {
    pub fn apply_chest_event(&mut self, x: i32, y: i32, z: i32, viewers: u8) {
        if is_chest_block(self.get_block(x, y, z)) {
            let position = (x, y, z);
            if !self.chests.contains_key(&position) {
                self.chests.insert(position, ChestBlockEntity::default());
                self.chest_structure_generation = self.chest_structure_generation.wrapping_add(1);
            }
            self.chests
                .get_mut(&position)
                .expect("chest entry inserted")
                .viewers = viewers as i32;
        }
    }

    /// Predict the local player's close immediately after C0D. The next
    /// server BlockAction remains authoritative. InventoryLargeChest invokes
    /// closeInventory on both halves, so mirror that for double chests.
    pub fn close_chest_for_local_viewer(&mut self, position: (i32, i32, i32)) {
        let block = self.get_block(position.0, position.1, position.2);
        if !is_chest_block(block) {
            return;
        }

        let mut positions = vec![position];
        if block != Block::EnderChest {
            for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let adjacent = (position.0 + dx, position.1, position.2 + dz);
                if self.get_block(adjacent.0, adjacent.1, adjacent.2) == block {
                    positions.push(adjacent);
                }
            }
        }
        for position in positions {
            if let Some(chest) = self.chests.get_mut(&position) {
                chest.viewers = chest.viewers.saturating_sub(1).max(0);
            }
        }
    }

    pub fn tick_chests(&mut self) {
        for chest in self.chests.values_mut() {
            chest.prev_lid_angle = chest.lid_angle;
            if chest.viewers > 0 && chest.lid_angle < 1.0 {
                chest.lid_angle = (chest.lid_angle + 0.1).min(1.0);
            } else if chest.viewers <= 0 && chest.lid_angle > 0.0 {
                chest.lid_angle = (chest.lid_angle - 0.1).max(0.0);
            }
        }
    }
}
