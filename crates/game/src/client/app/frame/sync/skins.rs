use crate::audio::AudioBackend;
use crate::client;
use crate::client::app::frame::visual::{player_skin_key, skull_skin_key};
use crate::client::app::App;
use crate::client::keybind::InputState;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use rand::Rng;
use winit::event_loop::ActiveEventLoop;

impl App {
    pub(super) fn collect_account_faces(
        &mut self,
    ) -> std::collections::HashMap<String, [[u8; 4]; 64]> {
        let active_uuid = self
            .account
            .as_ref()
            .and_then(|account| account.uuid.clone());
        let accounts = self.accounts.clone();
        let mut faces = std::collections::HashMap::with_capacity(accounts.len());
        for account in &accounts {
            let key = account
                .uuid
                .as_deref()
                .or(account.username.as_deref())
                .unwrap_or_default();
            if key.is_empty() {
                continue;
            }
            let face = if active_uuid.is_some() && active_uuid.as_deref() == account.uuid.as_deref()
            {
                self.local_skin.face_pixels()
            } else {
                self.load_account_face(account)
            };
            faces.insert(key.to_string(), face);
        }
        faces
    }

    pub(super) fn load_account_face(
        &mut self,
        account: &crate::auth::models::Account,
    ) -> [[u8; 4]; 64] {
        let uuid = account.uuid.as_deref().unwrap_or_default();
        let identity = account
            .uuid
            .as_deref()
            .or(account.username.as_deref())
            .unwrap_or("account");
        let identity_key = identity.replace('-', "");
        let skin_info = account.skins.as_ref().and_then(|skins| skins.first());
        let texture_key = skin_info
            .and_then(|skin| {
                skin.texture_key
                    .as_deref()
                    .or_else(|| skin.url.rsplit('/').next())
            })
            .filter(|key| {
                !key.is_empty()
                    && key.len() <= 128
                    && key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            });

        let mut paths = Vec::new();
        if let Some(texture_key) = texture_key {
            paths.push(std::path::PathBuf::from(format!(
                "assets/skins/{identity_key}/{texture_key}.png"
            )));
            paths.push(std::path::PathBuf::from(format!(
                "assets/skins/{texture_key}.png"
            )));
        }
        if !uuid.is_empty() {
            paths.push(std::path::PathBuf::from(format!("assets/skins/{uuid}.png")));
            paths.push(std::path::PathBuf::from(format!(
                "assets/skins/{identity_key}.png"
            )));
        }

        for path in paths {
            if let Ok(skin) = crate::assets::skin::PlayerSkin::load(path) {
                return skin.face_pixels();
            }
        }

        // A saved account may not have downloaded its skin yet.  The shared
        // cache still supplies a stable face (and can resolve legacy uuid/name
        // cache files) without blocking the render thread on HTTP.
        self.skin_cache
            .face_for(account.uuid.as_deref(), account.username.as_deref(), None)
    }

    pub(super) fn collect_pending_player_skins(
        &mut self,
        skull_hash: u64,
    ) -> Option<(Vec<crate::render::skin::PendingPlayerSkin>, u64, u64)> {
        let cache_generation = self.skin_cache.poll_content_generation();
        let roster_hash = self.compute_player_skin_roster_hash();
        let skin_generation = self.session.player_skin_generation;
        if self.last_player_skin_roster_hash == roster_hash
            && self.last_player_skin_generation == skin_generation
            && self.last_player_skin_skull_hash == skull_hash
            && self.last_skin_cache_generation == cache_generation
        {
            return None;
        }

        self.last_player_skin_roster_hash = roster_hash;
        self.last_player_skin_generation = skin_generation;
        self.last_player_skin_skull_hash = skull_hash;
        self.last_skin_cache_generation = cache_generation;

        let mut pending = Vec::new();
        self.player_skin_slims.clear();
        self.player_cape_ready.clear();
        pending.push(crate::render::skin::PendingPlayerSkin {
            key: "player/local".to_string(),
            skin: std::sync::Arc::new(self.local_skin.clone()),
            content_hash: crate::client::skin_cache::skin_content_hash(&self.local_skin),
            cape_pixels: self
                .local_cape_pixels
                .as_ref()
                .map(|pixels| std::sync::Arc::new(pixels.clone())),
            cape_content_hash: self.local_cape_hash,
        });
        let session = &self.session;
        let skin_cache = &mut self.skin_cache;
        let skin_profiles = &mut self.player_skin_profiles;
        let entities = &self.entities.entities;

        for entity in entities.values() {
            let crate::entity::EntityData::Player {
                name,
                skin_property: entity_skin_property,
                ..
            } = &entity.data
            else {
                continue;
            };
            let identity = if let Some(uuid) = entity.uuid.as_deref() {
                format!("uuid:{uuid}")
            } else {
                format!("name:{name}")
            };
            let listed_profile = entity
                .uuid
                .as_deref()
                .and_then(|uuid| session.player_list.get(uuid));
            let listed_property = listed_profile.map(|player| player.skin_property.as_deref());
            let profile = skin_profiles.entry(entity.entity_id).or_insert_with(|| {
                crate::client::app::PlayerSkinProfileCache::new(
                    identity.clone(),
                    listed_property,
                    entity_skin_property.as_deref(),
                )
            });
            profile.update(identity, listed_property, entity_skin_property.as_deref());
            let snapshot = skin_cache.snapshot_for(
                entity.uuid.as_deref(),
                Some(name),
                profile.skin_property.as_deref(),
            );
            self.player_skin_slims
                .insert(entity.entity_id, snapshot.slim);
            let cape = skin_cache.cape_for_property(profile.skin_property.as_deref());
            if cape.is_some() {
                self.player_cape_ready.insert(entity.entity_id);
            }
            pending.push(crate::render::skin::PendingPlayerSkin {
                key: player_skin_key(entity),
                skin: std::sync::Arc::clone(&snapshot.skin),
                content_hash: snapshot.content_hash,
                cape_pixels: cape
                    .as_ref()
                    .map(|cape| std::sync::Arc::clone(&cape.pixels)),
                cape_content_hash: cape.as_ref().map_or(0, |cape| cape.content_hash),
            });
        }
        skin_profiles.retain(|entity_id, _| {
            entities.get(entity_id).is_some_and(|entity| {
                matches!(&entity.data, crate::entity::EntityData::Player { .. })
            })
        });

        for skull in self
            .world
            .skulls
            .values()
            .filter(|skull| skull.skull_type == 3)
        {
            let snapshot = skin_cache.snapshot_for(
                skull.owner_uuid.as_deref(),
                skull.owner_name.as_deref(),
                skull.skin_property.as_deref(),
            );
            pending.push(crate::render::skin::PendingPlayerSkin {
                key: skull_skin_key(skull),
                skin: std::sync::Arc::clone(&snapshot.skin),
                content_hash: snapshot.content_hash,
                cape_pixels: None,
                cape_content_hash: 0,
            });
        }

        pending.sort_by(|a, b| {
            a.key
                .cmp(&b.key)
                .then_with(|| a.content_hash.cmp(&b.content_hash))
        });
        pending.dedup_by(|a, b| a.key == b.key);
        pending.truncate(crate::render::entity::atlas::PLAYER_SKIN_CAPACITY);

        use std::hash::Hasher;
        let mut layout_hasher = fnv::FnvHasher::default();
        let mut content_hasher = fnv::FnvHasher::default();
        layout_hasher.write_usize(pending.len());
        content_hasher.write_usize(pending.len());
        for skin in &pending {
            layout_hasher.write(skin.key.as_bytes());
            layout_hasher.write_u8(0);
            let (width, height) = skin.skin.dimensions();
            layout_hasher.write_u32(width);
            layout_hasher.write_u32(height);
            content_hasher.write(skin.key.as_bytes());
            content_hasher.write_u8(0);
            content_hasher.write_u64(skin.content_hash);
            content_hasher.write_u64(skin.cape_content_hash);
        }

        Some((pending, content_hasher.finish(), layout_hasher.finish()))
    }

    pub(super) fn compute_player_skin_roster_hash(&self) -> u64 {
        use std::hash::Hasher;
        let mut sum = 0u64;
        let mut xor = 0u64;
        let mut count = 0usize;

        for entity in self.entities.entities.values() {
            let crate::entity::EntityData::Player { name, .. } = &entity.data else {
                continue;
            };
            let mut entry = fnv::FnvHasher::default();
            entry.write_i32(entity.entity_id);
            entry.write_u8(entity.uuid.is_some() as u8);
            entry.write(entity.uuid.as_deref().unwrap_or(name).as_bytes());
            let value = entry.finish();
            sum = sum.wrapping_add(value);
            xor ^= value.rotate_left((value >> 58) as u32);
            count += 1;
        }

        let mut hasher = fnv::FnvHasher::default();
        hasher.write_usize(count);
        hasher.write_u64(sum);
        hasher.write_u64(xor);
        hasher.finish()
    }

    pub(super) fn compute_skull_hash(&self) -> u64 {
        use std::hash::Hasher;
        let mut sum = 0u64;
        let mut xor = 0u64;
        for (&(x, y, z), skull) in &self.world.skulls {
            let mut entry = fnv::FnvHasher::default();
            entry.write_i32(x);
            entry.write_i32(y);
            entry.write_i32(z);
            entry.write_u16(self.world.get_block_state(x, y, z));
            entry.write_u8(skull.skull_type);
            entry.write_u8(skull.rotation);
            if skull.skull_type == 3 {
                if let Some(owner) = skull.owner_uuid.as_deref().or(skull.owner_name.as_deref()) {
                    entry.write(owner.as_bytes());
                }
                entry.write_u8(0);
                if let Some(property) = skull.skin_property.as_deref() {
                    entry.write(property.as_bytes());
                }
            }
            let value = entry.finish();
            sum = sum.wrapping_add(value);
            xor ^= value.rotate_left((value >> 58) as u32);
        }

        let mut hasher = fnv::FnvHasher::default();
        hasher.write_usize(self.world.skulls.len());
        hasher.write_u64(sum);
        hasher.write_u64(xor);
        hasher.finish()
    }

    pub(super) fn compute_entity_hash(&self) -> u64 {
        use std::hash::Hasher;
        let mut h = fnv::FnvHasher::default();
        h.write_usize(self.entities.entities.len());
        h.write_u64(self.session.player_profile_generation);
        for entity in self.entities.entities.values() {
            h.write_i32(entity.entity_id);
            h.write_u32(entity.entity_type as u32);
            h.write_i64((entity.render_position.x * 20.0) as i64);
            h.write_i64((entity.render_position.y * 20.0) as i64);
            h.write_i64((entity.render_position.z * 20.0) as i64);
            h.write_i64((entity.render_chasing_position.x * 100.0) as i64);
            h.write_i64((entity.render_chasing_position.y * 100.0) as i64);
            h.write_i64((entity.render_chasing_position.z * 100.0) as i64);
            h.write_i32((entity.distance_walked_modified * 100.0) as i32);
            h.write_i32((entity.camera_yaw * 1000.0) as i32);
            h.write_i32((entity.yaw * 10.0) as i32);
            h.write_i32((entity.head_yaw * 10.0) as i32);
            h.write_i32((entity.pitch * 10.0) as i32);
            h.write_u32((entity.hurt_time * 5.0) as u32);
            h.write_u32(entity.ticks_alive);
            h.write_u32(entity.death_time.to_bits());
            h.write_u32(entity.swing_time.to_bits());
            h.write_u32(entity.critical_time.to_bits());
            h.write_u32(entity.limb_swing.to_bits());
            h.write_u32(entity.limb_swing_amount.to_bits());
            h.write_u8(entity.skin_parts);
            h.write_u8(entity.on_ground as u8);
            h.write_u8(entity.using_item as u8);
            h.write_i16(entity.current_item.unwrap_or(-1));
            h.write_i32(entity.vehicle_id.unwrap_or(-1));
            h.write_i32(entity.leash_holder.unwrap_or(-1));
            hash_entity_metadata(&mut h, &entity.metadata);
            for slot in &entity.equipment {
                hash_protocol_slot(&mut h, slot.as_ref());
            }
            hash_entity_visual(&mut h, &entity.visual);
            hash_hanging_data(&mut h, entity.hanging.as_ref());
            h.write_u32(
                self.player_skin_slims
                    .get(&entity.entity_id)
                    .copied()
                    .unwrap_or(false) as u32,
            );
            h.write_u32(self.player_cape_ready.contains(&entity.entity_id) as u32);
            if let crate::entity::EntityData::Player { name, .. } = &entity.data {
                if let Some(uuid) = entity.uuid.as_deref() {
                    h.write(uuid.as_bytes());
                }
                h.write_u8(0);
                h.write(name.as_bytes());
                h.write_u8(0);
            }
        }
        h.finish()
    }
}

fn hash_protocol_slot(hasher: &mut impl std::hash::Hasher, slot: Option<&crate::net::slot::Slot>) {
    let Some(slot) = slot else {
        hasher.write_u8(0);
        return;
    };
    hasher.write_u8(1);
    hasher.write_i16(slot.item_id);
    hasher.write_u8(slot.count);
    hasher.write_i16(slot.damage);
    if let Some(nbt) = &slot.nbt {
        hasher.write_usize(nbt.len());
        hasher.write(nbt);
    } else {
        hasher.write_usize(0);
    }
}

fn hash_entity_metadata(
    hasher: &mut impl std::hash::Hasher,
    metadata: &[crate::net::metadata::EntityMetadata],
) {
    use crate::net::metadata::MetadataValue;
    hasher.write_usize(metadata.len());
    for entry in metadata {
        hasher.write_u8(entry.index);
        match &entry.value {
            MetadataValue::Byte(value) => {
                hasher.write_u8(0);
                hasher.write_i8(*value);
            }
            MetadataValue::Short(value) => {
                hasher.write_u8(1);
                hasher.write_i16(*value);
            }
            MetadataValue::Int(value) => {
                hasher.write_u8(2);
                hasher.write_i32(*value);
            }
            MetadataValue::Float(value) => {
                hasher.write_u8(3);
                hasher.write_u32(value.to_bits());
            }
            MetadataValue::String(value) => {
                hasher.write_u8(4);
                hasher.write_usize(value.len());
                hasher.write(value.as_bytes());
            }
            MetadataValue::Slot(slot) => {
                hasher.write_u8(5);
                hash_protocol_slot(hasher, Some(slot));
            }
            MetadataValue::Position(position) => {
                hasher.write_u8(6);
                for value in position {
                    hasher.write_i32(*value);
                }
            }
            MetadataValue::Rotation(rotation) => {
                hasher.write_u8(7);
                for value in rotation {
                    hasher.write_u32(value.to_bits());
                }
            }
        }
    }
}

fn hash_entity_visual(
    hasher: &mut impl std::hash::Hasher,
    visual: &crate::entity::EntityVisualState,
) {
    hasher.write_u8(visual.is_child as u8);
    hasher.write_u8(visual.zombie_villager as u8);
    hasher.write_u8(visual.zombie_converting as u8);
    hasher.write_u8(visual.skeleton_type);
    hasher.write_u8(visual.wolf_tamed as u8);
    hasher.write_u8(visual.wolf_angry as u8);
    hasher.write_u8(visual.wolf_begging as u8);
    hasher.write_u8(visual.wolf_collar);
    hasher.write_u8(visual.ocelot_skin);
    hasher.write_u8(visual.horse_type);
    hasher.write_u8(visual.horse_saddled as u8);
    hasher.write_u32(visual.horse_variant);
    hasher.write_u8(visual.horse_armor);
    hasher.write_u8(visual.guardian_elder as u8);
    hasher.write_u8(visual.slime_size);
    hasher.write_u8(visual.bat_hanging as u8);
    hasher.write_u8(visual.villager_profession);
    hasher.write_u8(visual.armor_stand_flags);
    hasher.write_u8(visual.creeper_charged as u8);
    hasher.write_u8(visual.sheep_color);
    hasher.write_u8(visual.pig_saddled as u8);
    hasher.write_u8(visual.rabbit_type);
    for rotation in visual.armor_stand_rotations {
        for value in rotation {
            hasher.write_u32(value.to_bits());
        }
    }
}

fn hash_hanging_data(
    hasher: &mut impl std::hash::Hasher,
    hanging: Option<&crate::entity::HangingEntityData>,
) {
    use crate::entity::HangingEntityData;
    match hanging {
        None => hasher.write_u8(0),
        Some(HangingEntityData::Painting {
            title,
            anchor,
            facing,
        }) => {
            hasher.write_u8(1);
            hasher.write(title.as_bytes());
            for value in anchor {
                hasher.write_i32(*value);
            }
            hasher.write_u8(*facing);
        }
        Some(HangingEntityData::ItemFrame {
            anchor,
            facing,
            displayed_item,
            rotation,
        }) => {
            hasher.write_u8(2);
            for value in anchor {
                hasher.write_i32(*value);
            }
            hasher.write_u8(*facing);
            hasher.write_u8(*rotation);
            hash_protocol_slot(hasher, displayed_item.as_ref());
        }
        Some(HangingEntityData::LeashKnot { anchor }) => {
            hasher.write_u8(3);
            for value in anchor {
                hasher.write_i32(*value);
            }
        }
    }
}
