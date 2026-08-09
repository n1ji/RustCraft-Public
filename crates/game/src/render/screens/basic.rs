use crate::ui::button_ids as btn;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::ui::format::format_text;

use super::{ScreenContext, ScreenRenderer};

impl ScreenRenderer {
    pub(super) fn draw_main_menu(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        context.text_center(
            metrics.sh * 0.25 - 30.0 * scale,
            context.key("rustcraft.brand"),
            24.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );
        context.text_center(
            metrics.sh * 0.25 - 2.0 * scale,
            context.key("rustcraft.version"),
            12.0 * scale,
            [0.75, 0.75, 0.75, 1.0],
        );
        context.text(
            2.0 * scale,
            metrics.sh - 11.0 * scale,
            context.key("rustcraft.version"),
            8.0 * scale,
            [1.0, 1.0, 1.0, 0.8],
        );
        let mut y = metrics.sh * 0.25 + 48.0 * scale;
        context.button_enabled(
            btn::SINGLEPLAYER,
            [metrics.btn_x, y, metrics.btn_w, metrics.btn_h],
            context.key("menu.singleplayer"),
            false,
        );
        y += metrics.btn_h + metrics.btn_gap;
        context.button(
            btn::MULTIPLAYER,
            [metrics.btn_x, y, metrics.btn_w, metrics.btn_h],
            context.key("menu.multiplayer"),
        );
        y += metrics.btn_h + metrics.btn_gap;
        context.button(
            btn::ALT_MANAGER,
            [metrics.btn_x, y, metrics.btn_w, metrics.btn_h],
            context.key("rustcraft.altmanager.title"),
        );
        y += metrics.btn_h + metrics.btn_gap;
        context.button(
            btn::MODDING,
            [metrics.btn_x, y, metrics.btn_w, metrics.btn_h],
            context.key("rustcraft.modding.title"),
        );
        y += metrics.btn_h + metrics.btn_gap;
        let half = (metrics.btn_w - metrics.btn_gap) * 0.5;
        context.button(
            btn::OPTIONS,
            [metrics.btn_x, y, half, metrics.btn_h],
            context.key("menu.options"),
        );
        context.button(
            btn::QUIT,
            [
                metrics.btn_x + half + metrics.btn_gap,
                y,
                half,
                metrics.btn_h,
            ],
            context.key("menu.quit"),
        );
    }

    pub(super) fn draw_pause_menu(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        context.text_center(
            metrics.sh * 0.25 - 30.0 * scale,
            context.key("menu.game"),
            18.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );

        let y = metrics.sh * 0.25 + 8.0 * scale;
        let half = (metrics.btn_w - 4.0 * scale) * 0.5;
        context.button(
            btn::BACK_TO_GAME,
            [metrics.btn_x, y, metrics.btn_w, metrics.btn_h],
            context.key("menu.returnToGame"),
        );
        context.button_enabled(
            0,
            [metrics.btn_x, y + 24.0 * scale, half, metrics.btn_h],
            context.key("gui.achievements"),
            false,
        );
        context.button_enabled(
            0,
            [
                metrics.btn_x + half + 4.0 * scale,
                y + 24.0 * scale,
                half,
                metrics.btn_h,
            ],
            context.key("gui.stats"),
            false,
        );
        context.button(
            btn::PAUSE_OPTIONS,
            [metrics.btn_x, y + 48.0 * scale, half, metrics.btn_h],
            context.key("menu.options"),
        );
        context.button(
            btn::MODDING,
            [
                metrics.btn_x + half + 4.0 * scale,
                y + 48.0 * scale,
                half,
                metrics.btn_h,
            ],
            "Modding",
        );
        context.button(
            btn::DISCONNECT,
            [
                metrics.btn_x,
                y + 72.0 * scale,
                metrics.btn_w,
                metrics.btn_h,
            ],
            context.key("menu.disconnect"),
        );
    }

    pub(super) fn draw_options_menu(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        context.text_center(
            metrics.sh * 0.25 - 30.0 * scale,
            text.get("options.title"),
            24.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );

        let left = metrics.sw * 0.5 - metrics.btn_w - 4.0 * scale;
        let right = metrics.sw * 0.5 + 4.0 * scale;
        let y = metrics.sh * 0.25 + 20.0 * scale;
        let row = metrics.btn_h + metrics.btn_gap;
        context.button(
            btn::VIDEO_SETTINGS,
            [left, y, metrics.btn_w, metrics.btn_h],
            context.key("options.video"),
        );
        context.button(
            btn::CONTROLS,
            [right, y, metrics.btn_w, metrics.btn_h],
            context.key("options.controls"),
        );
        context.button(
            btn::LANGUAGE,
            [left, y + row, metrics.btn_w, metrics.btn_h],
            context.key("options.language"),
        );
        context.button(
            btn::AUDIO_SETTINGS,
            [right, y + row, metrics.btn_w, metrics.btn_h],
            context.key("options.sounds"),
        );
        context.button(
            btn::CHAT_SETTINGS,
            [left, y + 2.0 * row, metrics.btn_w, metrics.btn_h],
            context.key("options.chat.title"),
        );
        context.button(
            btn::SKIN_CUSTOMIZATION,
            [left, y + 3.0 * row, metrics.btn_w, metrics.btn_h],
            context.key("options.skinCustomisation"),
        );
        context.button(
            btn::RESOURCE_PACKS,
            [left, y + 4.0 * row, metrics.btn_w, metrics.btn_h],
            context.key("resourcePack.title"),
        );
        if context.in_world {
            let difficulty = format!(
                "{}: {}",
                text.get("options.difficulty"),
                difficulty_name(&text, context.settings.difficulty)
            );
            context.button_enabled(
                0,
                [right, y + 3.0 * row, metrics.btn_w, metrics.btn_h],
                difficulty,
                false,
            );
        }
        context.button(
            btn::DONE,
            [
                left,
                y + 5.0 * row + 26.0 * scale,
                metrics.btn_w,
                metrics.btn_h,
            ],
            context.key("gui.done"),
        );
    }

    pub(super) fn draw_video_settings(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        let left = metrics.sw * 0.5 - metrics.btn_w - 4.0 * scale;
        let right = metrics.sw * 0.5 + 4.0 * scale;
        let row = metrics.btn_h + metrics.btn_gap;
        let mut y = 60.0 * scale;
        let gui_scale = context.settings.gui_scale;
        let fov = context.settings.fov;
        let render_distance = context.settings.render_distance;
        let max_framerate = context.settings.max_framerate;
        let smooth_lighting = context.settings.smooth_lighting;
        let particles = context.settings.particles_label.clone();
        let fsr3_enabled = context.settings.fsr3_enabled;
        let fsr3_quality = context.settings.fsr3_quality;
        let fsr3_sharpness = context.settings.fsr3_sharpness;
        let fsr3_frame_generation = context.settings.fsr3_frame_generation;
        let fsr3_available = context.settings.fsr3_available;
        let clouds = context.settings.clouds;
        let entity_shadows = context.settings.entity_shadows;
        let weather_effects = context.settings.weather_effects;
        let view_bobbing = context.settings.view_bobbing;
        let advanced_tooltips = context.settings.advanced_tooltips;
        let better_grass = context.settings.better_grass;
        let connected_textures = context.settings.connected_textures;

        context.text_center(
            24.0 * scale,
            text.get("options.videoTitle"),
            18.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );
        context.slider(
            btn::GUI_SCALE_DOWN,
            [left, y, metrics.btn_w, metrics.btn_h],
            format!("{}: {}", text.get("options.guiScale"), gui_scale),
            gui_scale_slider_value(gui_scale),
        );
        context.slider(
            btn::FOV_DOWN,
            [right, y, metrics.btn_w, metrics.btn_h],
            format!("{}: {}", text.get("options.fov"), fov_label(&text, fov)),
            ((fov - 30.0) / 80.0).clamp(0.0, 1.0),
        );
        y += row;

        context.slider(
            btn::RENDER_DISTANCE_DOWN,
            [left, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.renderDistance"),
                render_distance
            ),
            render_distance_slider_value(render_distance),
        );
        context.slider(
            btn::FRAMERATE_DOWN,
            [right, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.framerateLimit"),
                framerate_label(&text, max_framerate)
            ),
            framerate_slider_value(max_framerate),
        );
        y += row;

        context.button(
            btn::SMOOTH_LIGHTING_TOGGLE,
            [left, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.ao"),
                if smooth_lighting {
                    text.get("options.ao.max")
                } else {
                    text.get("options.ao.off")
                }
            ),
        );
        context.button(
            btn::PARTICLES_TOGGLE,
            [right, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.particles"),
                particle_label(&text, &particles)
            ),
        );
        y += row;

        context.button(
            btn::SHADER_PACKS,
            [left, y, metrics.btn_w, metrics.btn_h],
            "Shader Packs...",
        );
        context.button(
            btn::FSR3_TOGGLE,
            [right, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.fsr3"),
                text.bool_label(fsr3_enabled)
            ),
        );
        y += row;

        context.button(
            btn::FSR3_QUALITY_CYCLE,
            [left, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.fsr3.quality"),
                text.get(fsr3_quality.translation_key())
            ),
        );
        context.slider(
            btn::FSR3_SHARPNESS,
            [right, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}%",
                text.get("options.fsr3.sharpness"),
                (fsr3_sharpness.clamp(0.0, 1.0) * 100.0).round() as u32
            ),
            fsr3_sharpness,
        );
        y += row;

        context.button(
            btn::FSR3_FRAME_GENERATION_TOGGLE,
            [left, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.fsr3.frameGeneration"),
                text.bool_label(fsr3_frame_generation)
            ),
        );
        context.button_enabled(
            0,
            [right, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.fsr3.runtime"),
                text.get(if fsr3_available {
                    "options.fsr3.runtime.available"
                } else {
                    "options.fsr3.runtime.unavailable"
                })
            ),
            false,
        );
        y += row;

        context.button(
            btn::CLOUDS_TOGGLE,
            [left, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.renderClouds"),
                text.bool_label(clouds)
            ),
        );
        context.button(
            btn::ENTITY_SHADOWS_TOGGLE,
            [right, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.entityShadows"),
                text.bool_label(entity_shadows)
            ),
        );
        y += row;

        context.button(
            btn::WEATHER_EFFECTS_TOGGLE,
            [left, y, metrics.btn_w, metrics.btn_h],
            format!("Weather Effects: {}", text.bool_label(weather_effects)),
        );
        context.button_enabled(
            0,
            [right, y, metrics.btn_w, metrics.btn_h],
            String::new(),
            false,
        );
        y += row;

        context.button(
            btn::VIEW_BOBBING_TOGGLE,
            [left, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.viewBobbing"),
                text.bool_label(view_bobbing)
            ),
        );
        context.button(
            btn::ADVANCED_TOOLTIPS_TOGGLE,
            [right, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.advancedTooltips"),
                text.bool_label(advanced_tooltips)
            ),
        );
        y += row;

        context.button(
            btn::BETTER_GRASS_TOGGLE,
            [left, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.betterGrass"),
                text.bool_label(better_grass)
            ),
        );
        context.button(
            btn::CONNECTED_TEXTURES_TOGGLE,
            [right, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.connectedTextures"),
                text.bool_label(connected_textures)
            ),
        );
        context.button(
            btn::DONE,
            [
                left,
                metrics.sh - 46.0 * scale,
                metrics.btn_w,
                metrics.btn_h,
            ],
            context.key("gui.done"),
        );
    }

    pub(super) fn draw_language(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        let language_name = context.settings.language_name.clone();
        let language_code = context.settings.language_code.clone();
        context.text_center(
            24.0 * scale,
            text.get("options.language"),
            18.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );
        let current = format_text(
            text.get("rustcraft.language.current"),
            &[&language_name, &language_code],
        );
        context.text_center(64.0 * scale, current, metrics.font_sz, [0.9, 0.9, 0.9, 1.0]);
        context.button(
            btn::LANGUAGE_TOGGLE,
            [metrics.btn_x, 96.0 * scale, metrics.btn_w, metrics.btn_h],
            if language_code == "zh_CN" {
                "English (US)"
            } else {
                "简体中文"
            },
        );
        context.text_center(
            130.0 * scale,
            text.get("rustcraft.language.reloadNote"),
            metrics.font_sz * 0.65,
            [0.65, 0.65, 0.65, 1.0],
        );
        context.button(
            btn::DONE,
            [
                metrics.btn_x,
                metrics.sh - 46.0 * scale,
                metrics.btn_w,
                metrics.btn_h,
            ],
            text.get("gui.done"),
        );
    }

    pub(super) fn draw_audio_settings(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        let master_volume = context.settings.master_volume;
        let music_volume = context.settings.music_volume;
        let blocks_volume = context.settings.blocks_volume;
        let hostile_volume = context.settings.hostile_volume;
        let friendly_volume = context.settings.friendly_volume;
        let players_volume = context.settings.players_volume;
        let ambient_volume = context.settings.ambient_volume;
        let weather_volume = context.settings.weather_volume;
        let ui_volume = context.settings.ui_volume;
        let audio_device = context.settings.audio_device.clone();
        context.text_center(
            24.0 * scale,
            text.get("options.sounds.title"),
            18.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );
        let rows = [
            (
                text.get("soundCategory.master").to_string(),
                master_volume,
                btn::MASTER_VOLUME_DOWN,
            ),
            (
                text.get("soundCategory.music").to_string(),
                music_volume,
                btn::MUSIC_VOLUME_DOWN,
            ),
            (
                text.get("soundCategory.block").to_string(),
                blocks_volume,
                btn::BLOCKS_VOLUME_DOWN,
            ),
            (
                text.get("soundCategory.hostile").to_string(),
                hostile_volume,
                btn::HOSTILE_VOLUME_DOWN,
            ),
            (
                text.get("soundCategory.neutral").to_string(),
                friendly_volume,
                btn::FRIENDLY_VOLUME_DOWN,
            ),
            (
                text.get("soundCategory.player").to_string(),
                players_volume,
                btn::PLAYERS_VOLUME_DOWN,
            ),
            (
                text.get("soundCategory.ambient").to_string(),
                ambient_volume,
                btn::AMBIENT_VOLUME_DOWN,
            ),
            (
                text.get("soundCategory.weather").to_string(),
                weather_volume,
                btn::WEATHER_VOLUME_DOWN,
            ),
            (
                text.get("soundCategory.ui").to_string(),
                ui_volume,
                btn::UI_VOLUME_DOWN,
            ),
        ];
        for (index, (label, value, id)) in rows.iter().enumerate() {
            let column = index % 2;
            let row = index / 2;
            let x = metrics.sw * 0.5 - metrics.btn_w - 4.0 * scale
                + column as f32 * (metrics.btn_w + 8.0 * scale);
            let y = 58.0 * scale + row as f32 * (metrics.btn_h + metrics.btn_gap);
            context.slider(
                *id,
                [x, y, metrics.btn_w, metrics.btn_h],
                format!("{}: {}%", label, (*value * 100.0).round()),
                *value,
            );
        }
        let device_y = 58.0 * scale + 5.0 * (metrics.btn_h + metrics.btn_gap);
        context.button(
            btn::AUDIO_DEVICE_CYCLE,
            [metrics.btn_x, device_y, metrics.btn_w, metrics.btn_h],
            format!("{}: {}", text.get("soundCategory.master"), audio_device),
        );
        context.text_center(
            device_y + metrics.btn_h + 4.0 * scale,
            if audio_device == "null" {
                text.get("rustcraft.audio.nullBackend")
            } else {
                ""
            },
            metrics.font_sz * 0.62,
            [0.65, 0.65, 0.65, 1.0],
        );
        context.button(
            btn::DONE,
            [
                metrics.btn_x,
                metrics.sh - 46.0 * scale,
                metrics.btn_w,
                metrics.btn_h,
            ],
            text.get("gui.done"),
        );
    }

    pub(super) fn draw_skin_customization(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        let skin_parts = context.settings.skin_parts;
        context.text_center(
            24.0 * scale,
            text.get("options.skinCustomisation.title"),
            18.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );
        let left = metrics.sw * 0.5 - metrics.btn_w - 4.0 * scale;
        let right = metrics.sw * 0.5 + 4.0 * scale;
        let labels = [
            (
                text.get("options.modelPart.cape"),
                0x01,
                btn::SKIN_CAPE_TOGGLE,
            ),
            (
                text.get("options.modelPart.jacket"),
                0x02,
                btn::SKIN_JACKET_TOGGLE,
            ),
            (
                text.get("options.modelPart.left_sleeve"),
                0x04,
                btn::SKIN_LEFT_SLEEVE_TOGGLE,
            ),
            (
                text.get("options.modelPart.right_sleeve"),
                0x08,
                btn::SKIN_RIGHT_SLEEVE_TOGGLE,
            ),
            (
                text.get("options.modelPart.left_pants_leg"),
                0x10,
                btn::SKIN_LEFT_PANTS_TOGGLE,
            ),
            (
                text.get("options.modelPart.right_pants_leg"),
                0x20,
                btn::SKIN_RIGHT_PANTS_TOGGLE,
            ),
            (
                text.get("options.modelPart.hat"),
                0x40,
                btn::SKIN_HAT_TOGGLE,
            ),
        ];
        for (index, (label, mask, id)) in labels.iter().enumerate() {
            let x = if index % 2 == 0 { left } else { right };
            let y = 58.0 * scale + (index / 2) as f32 * (metrics.btn_h + metrics.btn_gap);
            context.button(
                *id,
                [x, y, metrics.btn_w, metrics.btn_h],
                format!("{}: {}", label, text.bool_label(skin_parts & *mask != 0)),
            );
        }
        context.button(
            btn::SKIN_ALL_TOGGLE,
            [
                metrics.btn_x,
                58.0 * scale + 4.0 * (metrics.btn_h + metrics.btn_gap),
                metrics.btn_w,
                metrics.btn_h,
            ],
            if skin_parts == 0x7f {
                text.get("gui.all")
            } else {
                text.get("rustcraft.skin.allCustom")
            },
        );
        context.text_center(
            58.0 * scale + 5.0 * (metrics.btn_h + metrics.btn_gap) + 6.0 * scale,
            text.get("rustcraft.skin.syncNote"),
            metrics.font_sz * 0.62,
            [0.65, 0.65, 0.65, 1.0],
        );
        context.button(
            btn::DONE,
            [
                metrics.btn_x,
                metrics.sh - 46.0 * scale,
                metrics.btn_w,
                metrics.btn_h,
            ],
            text.get("gui.done"),
        );
    }

    pub(super) fn draw_chat_settings(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        let text = context.settings.ui_text.clone();
        let chat_width = context.settings.chat_width;
        let chat_height = context.settings.chat_height;
        let chat_background = context.settings.chat_background;
        let chat_overlay = context.settings.chat_overlay;
        let chat_player_avatars = context.settings.chat_player_avatars;
        let tab_player_avatars = context.settings.tab_player_avatars;
        let left = metrics.sw * 0.5 - metrics.btn_w - 4.0 * scale;
        let right = metrics.sw * 0.5 + 4.0 * scale;
        let y = 52.0 * scale;
        let row = metrics.btn_h + metrics.btn_gap;
        context.text_center(
            20.0 * scale,
            text.get("options.chat.title"),
            18.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );
        context.slider(
            btn::CHAT_WIDTH_DOWN,
            [left, y, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {:.0}%",
                text.get("options.chat.width"),
                chat_width * 100.0
            ),
            chat_width.clamp(0.1, 1.0),
        );
        context.slider(
            btn::CHAT_HEIGHT_DOWN,
            [right, y, metrics.btn_w, metrics.btn_h],
            format!("{}: {}", text.get("options.chat.height"), chat_height),
            (chat_height as f32 - 1.0) / 29.0,
        );
        context.button(
            btn::CHAT_BACKGROUND_TOGGLE,
            [left, y + row, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.chat.background"),
                text.bool_label(chat_background)
            ),
        );
        context.button(
            btn::CHAT_OVERLAY_TOGGLE,
            [right, y + row, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.chat.overlay"),
                text.bool_label(chat_overlay)
            ),
        );
        context.button(
            btn::CHAT_AVATARS_TOGGLE,
            [left, y + 2.0 * row, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.chat avatars"),
                text.bool_label(chat_player_avatars)
            ),
        );
        context.button(
            btn::TAB_AVATARS_TOGGLE,
            [right, y + 2.0 * row, metrics.btn_w, metrics.btn_h],
            format!(
                "{}: {}",
                text.get("options.chat.tabAvatars"),
                text.bool_label(tab_player_avatars)
            ),
        );
        context.button(
            btn::DONE,
            [
                metrics.btn_x,
                y + 3.0 * row + 10.0 * scale,
                metrics.btn_w,
                metrics.btn_h,
            ],
            text.get("gui.done"),
        );
    }

    pub(super) fn draw_connecting(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        context.text_center(
            metrics.sh * 0.5 - 20.0 * scale,
            context.key("multiplayer.connecting"),
            14.0 * scale,
            [1.0, 1.0, 1.0, 1.0],
        );
        context.text_center(
            metrics.sh * 0.5 + 10.0 * scale,
            &self.server_address,
            metrics.font_sz * 0.75,
            [0.65, 0.65, 0.65, 1.0],
        );
        let dots = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .div_euclid(500)
            .rem_euclid(4) as usize;
        context.text_center(
            metrics.sh * 0.5 + 30.0 * scale,
            ".".repeat(dots),
            metrics.font_sz * 0.65,
            [0.55, 0.55, 0.55, 1.0],
        );
    }

    pub(super) fn draw_loading_world(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        context.text_center(
            metrics.sh * 0.5 - 50.0 * metrics.gs,
            context.key("multiplayer.loadingWorld"),
            14.0 * metrics.gs,
            [1.0, 1.0, 1.0, 1.0],
        );
    }

    pub(super) fn draw_disconnected(&mut self, context: &mut ScreenContext<'_>) {
        let metrics = context.metrics;
        let scale = metrics.gs;
        context.text_center(
            metrics.sh * 0.25 - 10.0 * scale,
            context.key("rustcraft.connection.disconnected"),
            18.0 * scale,
            [1.0, 0.3, 0.3, 1.0],
        );
        if !self.connection_status.is_empty() {
            context.text_center(
                metrics.sh * 0.25 + 20.0 * scale,
                &self.connection_status,
                metrics.font_sz * 0.85,
                [0.8, 0.8, 0.8, 1.0],
            );
        }
        context.button(
            btn::BACK_TO_GAME,
            [
                metrics.btn_x,
                metrics.sh * 0.5 + 10.0 * scale,
                metrics.btn_w,
                metrics.btn_h,
            ],
            context.key("menu.returnToMenu"),
        );
    }
}

fn difficulty_name(text: &crate::ui::text::UiText, value: u8) -> &str {
    match value {
        0 => text.get("options.difficulty.peaceful"),
        1 => text.get("options.difficulty.easy"),
        2 => text.get("options.difficulty.normal"),
        _ => text.get("options.difficulty.hard"),
    }
}

fn framerate_label(text: &crate::ui::text::UiText, value: u32) -> String {
    if value >= crate::client::config::UNLIMITED_FRAMERATE {
        text.get("options.framerateLimit.max").to_string()
    } else {
        format!("{value} fps")
    }
}

fn framerate_slider_value(value: u32) -> f32 {
    if value >= crate::client::config::UNLIMITED_FRAMERATE {
        1.0
    } else {
        ((value.saturating_sub(30)) as f32
            / (crate::client::config::UNLIMITED_FRAMERATE - 30) as f32)
            .clamp(0.0, 1.0)
    }
}

fn gui_scale_slider_value(value: u32) -> f32 {
    (value.saturating_sub(1).min(3) as f32) / 3.0
}

fn render_distance_slider_value(value: u8) -> f32 {
    (value.saturating_sub(2).min(14) as f32) / 14.0
}

fn fov_label(text: &crate::ui::text::UiText, value: f32) -> String {
    if value <= 30.0 {
        text.get("options.fov.min").to_string()
    } else if value >= 110.0 {
        text.get("options.fov.max").to_string()
    } else {
        format!("{value:.0}")
    }
}

fn particle_label(text: &crate::ui::text::UiText, value: &str) -> String {
    match value {
        "Decreased" => text.get("options.particles.decreased").to_string(),
        "Minimal" => text.get("options.particles.minimal").to_string(),
        _ => text.get("options.particles.all").to_string(),
    }
}

fn draw_stepper(
    context: &mut ScreenContext<'_>,
    down: u32,
    up: u32,
    rect: [f32; 4],
    label: impl AsRef<str>,
) {
    let [x, y, width, height] = rect;
    let arrow_width = 24.0 * context.metrics.gs;
    context.button(down, [x, y, arrow_width, height], "<");
    context.button(up, [x + width - arrow_width, y, arrow_width, height], ">");
    context.panel(
        [x + arrow_width, y, width - arrow_width * 2.0, height],
        [0.05, 0.05, 0.05, 0.85],
    );
    context.text_center(
        y + (height - context.metrics.font_sz) * 0.5,
        label,
        context.metrics.font_sz,
        [0.9, 0.9, 0.9, 1.0],
    );
}
