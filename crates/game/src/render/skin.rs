//! Game-facing player skin upload descriptions.

#[derive(Clone, Debug)]
pub(crate) struct PendingPlayerSkin {
    pub key: String,
    pub skin: std::sync::Arc<crate::assets::skin::PlayerSkin>,
    pub content_hash: u64,
    pub cape_pixels: Option<std::sync::Arc<Vec<u8>>>,
    pub cape_content_hash: u64,
}
