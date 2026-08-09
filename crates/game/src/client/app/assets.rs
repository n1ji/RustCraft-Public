use std::io::Read;
use std::path::Path;

use crate::assets;
use crate::assets::resource_pack::ResourcePack;
use crate::audio;
use crate::audio::AudioBackend;
use crate::client;

pub(crate) fn apply_audio_config(
    audio: &mut audio::AudioBackendImpl,
    config: &client::config::ClientConfig,
) {
    audio.set_volume(audio::SoundCategory::Master, config.master_volume);
    audio.set_volume(audio::SoundCategory::Music, config.music_volume);
    audio.set_volume(audio::SoundCategory::Blocks, config.blocks_volume);
    audio.set_volume(audio::SoundCategory::Hostile, config.hostile_volume);
    audio.set_volume(audio::SoundCategory::Friendly, config.friendly_volume);
    audio.set_volume(audio::SoundCategory::Players, config.players_volume);
    audio.set_volume(audio::SoundCategory::Ambient, config.ambient_volume);
    audio.set_volume(audio::SoundCategory::Weather, config.weather_volume);
    audio.set_volume(audio::SoundCategory::Ui, config.ui_volume);
}

pub(crate) fn load_local_skin(
    account: Option<&crate::auth::models::Account>,
    model: &mut client::player_model::PlayerModel,
) -> assets::skin::PlayerSkin {
    let mut skin = if let Some(acct) = account {
        let uuid_key = acct.uuid.as_deref().unwrap_or("default").replace('-', "");
        let skin_info = acct.skins.as_ref().and_then(|s| s.first());
        model.slim_arms = skin_info.map_or(false, |s| s.variant == "SLIM");
        let texture_key = skin_info.and_then(|s| s.url.rsplit('/').next());
        let skin_url = skin_info.map(|s| s.url.as_str());
        // Try local cached file first (new UUID-directory format)
        Some(if let Some(tk) = texture_key {
            let path = format!("assets/skins/{}/{}.png", uuid_key, tk);
            if let Ok(skin) = assets::skin::PlayerSkin::load(&path) {
                skin
            } else {
                // Fallback: old flat format
                let old_path = format!("assets/skins/{}.png", tk);
                if let Ok(skin) = assets::skin::PlayerSkin::load(&old_path) {
                    skin
                } else {
                    download_and_save_skin(skin_url, texture_key, &uuid_key)
                        .unwrap_or_else(|| fallback_skin())
                }
            }
        } else {
            download_and_save_skin(skin_url, texture_key, &uuid_key)
                .unwrap_or_else(|| fallback_skin())
        })
    } else {
        None
    }
    .unwrap_or_else(fallback_skin);

    skin
}

fn download_and_save_skin(
    skin_url: Option<&str>,
    texture_key: Option<&str>,
    uuid_key: &str,
) -> Option<assets::skin::PlayerSkin> {
    let url = skin_url?;
    let resp = reqwest::blocking::get(url).ok()?;
    let bytes = resp.bytes().ok()?;
    let tk = texture_key.unwrap_or("local");
    let path = format!("assets/skins/{}/{}.png", uuid_key, tk);
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &bytes);
    assets::skin::PlayerSkin::load(&path).ok()
}

fn fallback_skin() -> assets::skin::PlayerSkin {
    if let Ok(path) = std::env::var("RUSTCRAFT_SKIN") {
        if let Ok(skin) = assets::skin::PlayerSkin::load(path) {
            return skin;
        }
    }
    assets::skin::PlayerSkin::load("assets/skin.png")
        .or_else(|_| assets::skin::PlayerSkin::load("assets/minecraft/textures/entity/steve.png"))
        .unwrap_or_else(|_| assets::skin::PlayerSkin::default_steve())
}

pub(crate) fn load_local_cape_pixels(account: &crate::auth::models::Account) -> Option<Vec<u8>> {
    let cape = account
        .capes
        .as_ref()?
        .iter()
        .find(|c| c.state.eq_ignore_ascii_case("active"))?;
    let texture_key = cape.url.rsplit('/').next()?.trim();
    if texture_key.is_empty()
        || texture_key.len() > 128
        || !texture_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    let path = format!("assets/capes/{texture_key}.png");
    if let Ok(bytes) = std::fs::read(&path) {
        if bytes.len() <= 4 * 1024 * 1024 {
            if let Ok(img) = crate::assets::skin::decode_skin_auxiliary_image(&bytes) {
                return Some(crate::client::skin_cache::normalize_cape_image(img));
            }
        }
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    crate::client::skin_cache::download_cape_texture(
        &client,
        &cape.url,
        std::path::Path::new(&path),
    )
    .ok()
}

#[derive(Clone, Debug)]
pub struct ResourcePackInfo {
    pub name: String,
    pub enabled: bool,
    pub is_default: bool,
    pub description: String,
    pub pack_format: i32,
    /// 32×32 RGBA pack icon pixels, or None if no icon
    pub icon: Option<[u8; 4096]>,
}

/// Expected pack format for MC 1.8.9
pub(crate) const MC_PACK_FORMAT: i32 = 1;

pub(crate) fn scan_resource_packs(enabled: &[String]) -> Vec<ResourcePackInfo> {
    let rp_dir = Path::new("resourcepacks");
    if !rp_dir.is_dir() {
        return Vec::new();
    }
    let mut packs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(rp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_zip = path.extension().and_then(|s| s.to_str()) == Some("zip");
            if is_zip || (path.is_dir() && path.file_stem().map_or(false, |s| s != "resourcepacks"))
            {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    let enabled_flag = enabled.iter().any(|e| e == name);
                    let meta_path = if is_zip {
                        path.clone()
                    } else {
                        path.join("pack.mcmeta")
                    };
                    let (desc, pack_format) = read_pack_meta(&meta_path, is_zip);
                    let icon = read_pack_icon(&path, is_zip);
                    packs.push(ResourcePackInfo {
                        name: name.to_string(),
                        enabled: enabled_flag,
                        is_default: false,
                        description: desc,
                        pack_format,
                        icon,
                    });
                }
            }
        }
    }
    packs
}

fn read_pack_icon(path: &std::path::Path, is_zip: bool) -> Option<[u8; 4096]> {
    let data = if is_zip {
        if let Ok(file) = std::fs::File::open(path) {
            if let Ok(mut zip) = zip::ZipArchive::new(file) {
                if let Ok(mut entry) = zip.by_name("pack.png") {
                    if entry.size() > 8 * 1024 * 1024 {
                        return None;
                    }
                    let mut buf = Vec::new();
                    std::io::Read::take(&mut entry, 8 * 1024 * 1024 + 1)
                        .read_to_end(&mut buf)
                        .ok();
                    if buf.len() > 8 * 1024 * 1024 {
                        return None;
                    }
                    buf
                } else {
                    return None;
                }
            } else {
                return None;
            }
        } else {
            return None;
        }
    } else {
        let icon_path = path.join("pack.png");
        if icon_path.exists() {
            if std::fs::metadata(&icon_path).ok()?.len() > 8 * 1024 * 1024 {
                return None;
            }
            std::fs::read(&icon_path).ok()?
        } else {
            return None;
        }
    };
    if let Ok(img) =
        crate::assets::texture::decode_image_with_limits(&data, 1_024, 16 * 1024 * 1024)
    {
        let resized = img.resize_exact(32, 32, image::imageops::FilterType::Nearest);
        let rgba = resized.to_rgba8();
        let mut pixels = [0u8; 4096];
        let src = rgba.into_raw();
        let copy_len = src.len().min(4096);
        pixels[..copy_len].copy_from_slice(&src[..copy_len]);
        Some(pixels)
    } else {
        None
    }
}

fn read_pack_meta(path: &std::path::Path, is_zip: bool) -> (String, i32) {
    let text = if is_zip {
        if let Ok(file) = std::fs::File::open(path) {
            if let Ok(mut zip) = zip::ZipArchive::new(file) {
                if let Ok(mut entry) = zip.by_name("pack.mcmeta") {
                    let mut t = String::new();
                    std::io::Read::read_to_string(&mut entry, &mut t).ok();
                    t
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else if path.exists() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };
    if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&text) {
        let desc = meta["pack"]["description"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let fmt = meta["pack"]["pack_format"].as_i64().unwrap_or(0) as i32;
        (desc, fmt)
    } else {
        (String::new(), 0)
    }
}
