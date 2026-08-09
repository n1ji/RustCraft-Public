//! Backend-neutral scene and UI data consumed by game render systems.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerListRow {
    pub name: String,
    pub address: String,
    pub online: bool,
    pub ping_ms: Option<u32>,
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
    pub version_name: Option<String>,
    pub description: Option<String>,
    pub error: Option<String>,
    pub favicon_pixels: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Default)]
pub struct ModManagerRow {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub protocol_translator: bool,
    pub config_entries: usize,
    pub granted_permissions: Vec<String>,
    pub denied_permissions: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ModConfigRow {
    pub key: String,
    pub label: String,
    pub description: String,
    pub value: String,
    pub is_default: bool,
    pub can_previous: bool,
    pub can_next: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShaderOptionRowKind {
    #[default]
    Option,
    Screen,
    Profile,
    Spacer,
}

#[derive(Clone, Debug, Default)]
pub struct ShaderOptionRow {
    pub key: String,
    pub kind: ShaderOptionRowKind,
    pub label: String,
    pub description: String,
    pub value: String,
    pub is_default: bool,
    pub can_previous: bool,
    pub can_next: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SidebarLine {
    pub display: String,
    pub score: i32,
}

#[derive(Clone, Debug)]
pub struct EntityBillboard {
    pub entity_id: i32,
    pub position: [f32; 3],
    pub sky_light: u8,
    pub block_light: u8,
    pub height: f32,
    pub width: f32,
    pub name: Option<String>,
    pub kind: EntityBillboardKind,
    pub entity_type: crate::entity::EntityType,
    pub health: Option<(f32, f32)>,
    pub held_item: Option<u16>,
    pub equipment: [Option<(u16, u16)>; 5],
    pub item_id: Option<u16>,
    pub item_damage: Option<u16>,
    pub item_nbt: Option<Vec<u8>>,
    pub swing_progress: f32,
    pub skin_key: Option<String>,
    pub slim: bool,
    pub skin_parts_mask: u8,
    pub has_cape: bool,
    pub cape_rotation: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub head_yaw: f32,
    pub limb_swing: f32,
    pub limb_swing_amount: f32,
    pub sneaking: bool,
    pub blocking: bool,
    pub invisible: bool,
    pub riding: bool,
    pub name_visible: bool,
    pub age_ticks: f32,
    pub hover_start: f32,
    pub velocity: [f32; 3],
    pub hurt_alpha: f32,
    pub death_alpha: f32,
    pub swing_alpha: f32,
    pub critical_alpha: f32,
    pub visual: crate::entity::EntityVisualState,
    pub hanging: Option<crate::entity::HangingEntityData>,
    pub fishing_line_start: Option<[f32; 3]>,
    pub painting_lights: Vec<[u8; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityBillboardKind {
    Player,
    Hostile,
    Passive,
    Item,
    XpOrb,
    Projectile,
    Other,
}

#[derive(Clone, Debug)]
pub struct SkullRenderEntry {
    pub position: [i32; 3],
    pub block_metadata: u8,
    pub skull_type: u8,
    pub rotation: u8,
    pub skin_key: String,
    pub sky_light: u8,
    pub block_light: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct ChestRenderEntry {
    pub position: [i32; 3],
    pub block: crate::world::block::Block,
    pub metadata: u8,
    pub lid_angle: f32,
    pub double_x: bool,
    pub double_z: bool,
    pub sky_light: u8,
    pub block_light: u8,
}

#[derive(Clone, Debug)]
pub struct ControlBindingRow {
    pub action: crate::client::keybind::Action,
    pub label: String,
    pub binding: String,
    pub category: String,
    pub conflict: bool,
    pub listening: bool,
    pub is_default: bool,
}

#[derive(Clone, Debug)]
pub struct SignEntry {
    pub position: [i32; 3],
    pub lines: Vec<String>,
    pub wall_mounted: bool,
    pub metadata: u8,
}
