use crate::audio::AudioBackend;
use crate::client;
use crate::client::app::App;
use crate::client::keybind::InputState;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use rand::Rng;
use winit::event_loop::ActiveEventLoop;

impl App {
    /// Reload the local player's skin from the current authenticated account.
    pub(crate) fn update_local_skin(&mut self) {
        self.local_skin_dirty = true;
        self.account_faces_dirty = true;
        self.last_skin_cache_generation = u64::MAX;
        if let Some(ref account) = self.account {
            let uuid_key = account
                .uuid
                .as_deref()
                .unwrap_or("default")
                .replace('-', "");
            let skin_info = account.skins.as_ref().and_then(|s| s.first());
            let texture_key = skin_info.and_then(|s| s.url.rsplit('/').next());
            let skin_url = skin_info.map(|s| s.url.as_str());
            let is_slim = skin_info.map_or(false, |s| s.variant == "SLIM");
            self.local_player_model.slim_arms = is_slim;

            // Load cape pixels for atlas upload
            if let Some(cape) = crate::client::app::load_local_cape_pixels(account) {
                let raw = cape;
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash_slice(&raw, &mut h);
                let hash = std::hash::Hasher::finish(&h);
                if self.local_cape_hash != hash {
                    self.local_cape_hash = hash;
                    self.local_cape_pixels = Some(raw);
                }
            }

            if let Some(tk) = texture_key {
                let path = format!("assets/skins/{}/{}.png", uuid_key, tk);
                if let Ok(skin) = crate::assets::skin::PlayerSkin::load(&path) {
                    self.local_skin = skin;
                    return;
                }
                // Fallback old format
                let old_path = format!("assets/skins/{}.png", tk);
                if let Ok(skin) = crate::assets::skin::PlayerSkin::load(&old_path) {
                    self.local_skin = skin;
                    return;
                }
                if let Some(url) = skin_url {
                    if let Ok(resp) = reqwest::blocking::get(url) {
                        if let Ok(bytes) = resp.bytes() {
                            let parent_dir = std::path::Path::new(&path)
                                .parent()
                                .unwrap_or_else(|| std::path::Path::new("assets/skins"));
                            let _ = std::fs::create_dir_all(parent_dir);
                            let _ = std::fs::write(&path, &bytes);
                            if let Ok(skin) = crate::assets::skin::PlayerSkin::load(&path) {
                                self.local_skin = skin;
                            }
                        }
                    }
                }
            }
        }
    }
}
