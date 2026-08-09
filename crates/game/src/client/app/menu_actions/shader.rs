use crate::audio::AudioBackend;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use crate::ui::button_ids as btn;
use winit::event_loop::ActiveEventLoop;

use crate::client::app::{App, PendingShaderPackLoad, ShaderPackLoadReason};
use crate::render::scene::{ShaderOptionRow, ShaderOptionRowKind};
use crate::render::shader_pack::{
    OptifineManifest, OptifineOption, OptifineOptionKind, OptifineProfile, OptifineProfileAction,
    OptifineScreenEntry,
};
pub(crate) struct ShaderOptionsSession {
    pub pack: String,
    pub manifest: OptifineManifest,
    /// `shaders/lang/<code>.lang` labels from the pack.
    pub lang: std::collections::BTreeMap<String, String>,
    /// Nested `screen.<name>` navigation; empty means the root screen.
    pub screen_stack: Vec<String>,
    /// Overrides changed since the last executor reload.
    pub dirty: bool,
}

impl App {
    pub(super) fn open_shader_options(&mut self) {
        let Some(pack_name) = self.config.shader_pack.clone() else {
            if let Some(renderer) = &mut self.renderer {
                renderer.screens.shader_pack_status = "Select a shader pack first".to_string();
            }
            return;
        };
        match crate::render::shader_pack::load_optifine_shader_pack(&pack_name) {
            Ok(pack) => {
                let lang = pack.lang_entries(&self.config.language);
                let manifest = pack.manifest().clone();
                if manifest.options.is_empty() {
                    if let Some(renderer) = &mut self.renderer {
                        renderer.screens.shader_pack_status =
                            format!("{pack_name} exposes no shader options");
                    }
                    return;
                }
                self.shader_options = Some(ShaderOptionsSession {
                    pack: pack_name,
                    manifest,
                    lang,
                    screen_stack: Vec::new(),
                    dirty: false,
                });
                if let Some(renderer) = &mut self.renderer {
                    renderer.screens.shader_option_scroll = 0;
                    renderer.screens.shader_option_status = String::new();
                }
                self.rebuild_shader_option_rows();
                self.push_subscreen(GameState::ShaderOptions {
                    previous: Box::new(GameState::MainMenu),
                });
            }
            Err(error) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.screens.shader_pack_status = format!("Cannot read options: {error}");
                }
            }
        }
    }

    /// Back button / ESC: leave the nested screen first, then the menu.
    pub(crate) fn shader_options_back(&mut self) {
        let popped = match &mut self.shader_options {
            Some(session) if !session.screen_stack.is_empty() => {
                session.screen_stack.pop();
                true
            }
            _ => false,
        };
        if popped {
            if let Some(renderer) = &mut self.renderer {
                renderer.screens.shader_option_scroll = 0;
            }
            self.rebuild_shader_option_rows();
        } else {
            self.return_to_previous_screen();
        }
    }

    pub(super) fn rebuild_shader_option_rows(&mut self) {
        let Some(session) = &self.shader_options else {
            return;
        };
        let overrides = self.config.shader_pack_options.get(&session.pack);
        let (title, rows) = build_shader_option_rows(session, overrides);
        if let Some(renderer) = &mut self.renderer {
            renderer.screens.shader_option_title = title;
            renderer.screens.shader_option_rows = rows;
        }
    }

    /// Row/arrow click: cycle an option, enter a screen, or switch profile.
    pub(super) fn shader_option_adjust(&mut self, index: usize, direction: i32, from_row: bool) {
        let Some(row) = self
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.screens.shader_option_rows.get(index))
            .cloned()
        else {
            return;
        };
        match row.kind {
            ShaderOptionRowKind::Screen => {
                if from_row {
                    if let Some(session) = &mut self.shader_options {
                        session.screen_stack.push(row.key);
                    }
                    if let Some(renderer) = &mut self.renderer {
                        renderer.screens.shader_option_scroll = 0;
                    }
                    self.rebuild_shader_option_rows();
                }
            }
            ShaderOptionRowKind::Option => self.cycle_shader_option(&row.key, direction),
            ShaderOptionRowKind::Profile => self.cycle_shader_profile(direction),
            ShaderOptionRowKind::Spacer => {}
        }
    }

    pub(crate) fn cycle_shader_option(&mut self, name: &str, direction: i32) {
        let Some(session) = &self.shader_options else {
            return;
        };
        let Some(option) = session
            .manifest
            .options
            .iter()
            .find(|option| option.name == name)
            .cloned()
        else {
            return;
        };
        let pack = session.pack.clone();
        let current = effective_option_value(&self.config, &pack, &option);
        let next = match option.kind {
            OptifineOptionKind::Boolean => {
                if current.eq_ignore_ascii_case("true") {
                    "false".to_string()
                } else {
                    "true".to_string()
                }
            }
            OptifineOptionKind::Value | OptifineOptionKind::Constant => {
                if option.values.len() < 2 {
                    return;
                }
                let position = option
                    .values
                    .iter()
                    .position(|value| *value == current)
                    .unwrap_or(0) as i32;
                let count = option.values.len() as i32;
                let next = (position + direction).rem_euclid(count) as usize;
                option.values[next].clone()
            }
            OptifineOptionKind::Unknown => return,
        };
        self.set_shader_option(&pack, &option, next);
    }

    pub(crate) fn set_shader_option(&mut self, pack: &str, option: &OptifineOption, value: String) {
        let entry = self
            .config
            .shader_pack_options
            .entry(pack.to_string())
            .or_default();
        if value == option.default_value {
            entry.remove(&option.name);
        } else {
            entry.insert(option.name.clone(), value);
        }
        if entry.is_empty() {
            self.config.shader_pack_options.remove(pack);
        }
        self.config.save_default();
        self.mark_shader_options_dirty();
        self.rebuild_shader_option_rows();
    }

    pub(crate) fn mark_shader_options_dirty(&mut self) {
        if let Some(session) = &mut self.shader_options {
            session.dirty = true;
        }
        if let Some(renderer) = &mut self.renderer {
            renderer.screens.shader_option_status =
                "Changes apply when leaving this menu".to_string();
        }
    }

    pub(crate) fn cycle_shader_profile(&mut self, direction: i32) {
        let Some(session) = &self.shader_options else {
            return;
        };
        let profiles = session.manifest.profiles.clone();
        if profiles.is_empty() {
            return;
        }
        let pack = session.pack.clone();
        let current = profiles
            .iter()
            .position(|profile| profile_matches(&self.config, &pack, &session.manifest, profile));
        let next_index = match current {
            Some(index) => (index as i32 + direction).rem_euclid(profiles.len() as i32) as usize,
            // From "Custom", both directions start at the first profile.
            None => 0,
        };
        let mut values = Vec::new();
        collect_profile_values(&session.manifest, &profiles[next_index], &mut values, 0);
        let updates: Vec<(String, String, String)> = values
            .into_iter()
            .filter_map(|(name, value)| {
                let option = session
                    .manifest
                    .options
                    .iter()
                    .find(|option| option.name == name)?;
                session
                    .manifest
                    .is_valid_override(&name, &value)
                    .then(|| (name, value, option.default_value.clone()))
            })
            .collect();

        let entry = self
            .config
            .shader_pack_options
            .entry(pack.clone())
            .or_default();
        for (name, value, default) in updates {
            if value == default {
                entry.remove(&name);
            } else {
                entry.insert(name, value);
            }
        }
        if entry.is_empty() {
            self.config.shader_pack_options.remove(&pack);
        }
        self.config.save_default();
        self.mark_shader_options_dirty();
        self.rebuild_shader_option_rows();
    }

    pub(crate) fn reset_shader_option(&mut self, index: usize) {
        let Some(row) = self
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.screens.shader_option_rows.get(index))
            .cloned()
        else {
            return;
        };
        if row.kind != ShaderOptionRowKind::Option {
            return;
        }
        let Some(session) = &self.shader_options else {
            return;
        };
        let Some(option) = session
            .manifest
            .options
            .iter()
            .find(|option| option.name == row.key)
            .cloned()
        else {
            return;
        };
        let pack = session.pack.clone();
        let default = option.default_value.clone();
        self.set_shader_option(&pack, &option, default);
    }

    pub(crate) fn reset_all_shader_options(&mut self) {
        let Some(session) = &mut self.shader_options else {
            return;
        };
        let pack = session.pack.clone();
        self.config.shader_pack_options.remove(&pack);
        self.config.save_default();
        self.mark_shader_options_dirty();
        self.rebuild_shader_option_rows();
    }

    /// Reload the executor with the current overrides when the options
    /// screen is left after a change.
    pub(super) fn apply_shader_options_if_dirty(&mut self) {
        let Some(session) = &mut self.shader_options else {
            return;
        };
        if !session.dirty {
            return;
        }
        session.dirty = false;
        let pack = session.pack.clone();
        if self.config.shader_pack.as_deref() != Some(pack.as_str()) {
            return;
        }
        let overrides = self
            .config
            .shader_pack_options
            .get(&pack)
            .cloned()
            .unwrap_or_default();
        self.queue_shader_pack_load(pack, overrides, ShaderPackLoadReason::OptionsReload);
    }
}

fn effective_option_value(
    config: &crate::client::config::ClientConfig,
    pack: &str,
    option: &OptifineOption,
) -> String {
    config
        .shader_pack_options
        .get(pack)
        .and_then(|overrides| overrides.get(&option.name))
        .cloned()
        .unwrap_or_else(|| option.default_value.clone())
}

/// Recursively gather `SetOption` actions (following `IncludeProfile`).
fn collect_profile_values(
    manifest: &OptifineManifest,
    profile: &OptifineProfile,
    out: &mut Vec<(String, String)>,
    depth: usize,
) {
    if depth > 4 {
        return;
    }
    for action in &profile.actions {
        match action {
            OptifineProfileAction::SetOption { name, value } => {
                out.push((name.clone(), value.clone()));
            }
            OptifineProfileAction::IncludeProfile(included) => {
                if let Some(inner) = manifest
                    .profiles
                    .iter()
                    .find(|profile| profile.name == *included)
                {
                    collect_profile_values(manifest, inner, out, depth + 1);
                }
            }
            OptifineProfileAction::DisableProgram(_) => {}
        }
    }
}

fn profile_matches(
    config: &crate::client::config::ClientConfig,
    pack: &str,
    manifest: &OptifineManifest,
    profile: &OptifineProfile,
) -> bool {
    let mut values = Vec::new();
    collect_profile_values(manifest, profile, &mut values, 0);
    if values.is_empty() {
        return false;
    }
    values.iter().all(|(name, value)| {
        manifest
            .options
            .iter()
            .find(|option| option.name == *name)
            .is_some_and(|option| effective_option_value(config, pack, option) == *value)
    })
}

fn shader_lang_label(session: &ShaderOptionsSession, key: &str, fallback: &str) -> String {
    session
        .lang
        .get(key)
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

fn option_row(
    session: &ShaderOptionsSession,
    overrides_value: Option<&String>,
    name: &str,
) -> ShaderOptionRow {
    let Some(option) = session
        .manifest
        .options
        .iter()
        .find(|option| option.name == name)
    else {
        return ShaderOptionRow {
            key: name.to_string(),
            kind: ShaderOptionRowKind::Option,
            label: name.to_string(),
            description: "Not defined by this pack".to_string(),
            value: "?".to_string(),
            is_default: true,
            can_previous: false,
            can_next: false,
        };
    };
    let current = overrides_value
        .cloned()
        .unwrap_or_else(|| option.default_value.clone());
    let is_default = current == option.default_value;
    let label = shader_lang_label(session, &format!("option.{name}"), name);
    let description = session
        .lang
        .get(&format!("option.{name}.comment"))
        .cloned()
        .unwrap_or_else(|| option.description.clone());
    let (value, can_cycle) = match option.kind {
        OptifineOptionKind::Boolean => {
            let on = current.eq_ignore_ascii_case("true");
            (if on { "ON" } else { "OFF" }.to_string(), true)
        }
        OptifineOptionKind::Value | OptifineOptionKind::Constant => {
            let display = shader_lang_label(session, &format!("value.{name}.{current}"), &current);
            let prefix = session.lang.get(&format!("prefix.{name}"));
            let suffix = session.lang.get(&format!("suffix.{name}"));
            let display = format!(
                "{}{}{}",
                prefix.map(String::as_str).unwrap_or(""),
                display,
                suffix.map(String::as_str).unwrap_or("")
            );
            (display, option.values.len() > 1)
        }
        OptifineOptionKind::Unknown => ("?".to_string(), false),
    };
    ShaderOptionRow {
        key: name.to_string(),
        kind: ShaderOptionRowKind::Option,
        label,
        description,
        value,
        is_default,
        can_previous: can_cycle,
        can_next: can_cycle,
    }
}

fn build_shader_option_rows(
    session: &ShaderOptionsSession,
    overrides: Option<&std::collections::BTreeMap<String, String>>,
) -> (String, Vec<ShaderOptionRow>) {
    let manifest = &session.manifest;
    let screen_name = session.screen_stack.last().cloned().unwrap_or_default();
    let screen = manifest
        .screens
        .iter()
        .find(|screen| screen.name == screen_name);

    let title = if screen_name.is_empty() {
        session.pack.trim_end_matches(".zip").to_string()
    } else {
        shader_lang_label(session, &format!("screen.{screen_name}"), &screen_name)
    };

    let value_of = |name: &str| overrides.and_then(|map| map.get(name));

    // Current profile display: the profile whose values all match, else
    // "Custom".
    let effective = |name: &str| -> Option<String> {
        manifest
            .options
            .iter()
            .find(|option| option.name == name)
            .map(|option| {
                value_of(name)
                    .cloned()
                    .unwrap_or_else(|| option.default_value.clone())
            })
    };
    let profile_value = manifest
        .profiles
        .iter()
        .find(|profile| {
            let mut values = Vec::new();
            collect_profile_values(manifest, profile, &mut values, 0);
            !values.is_empty()
                && values
                    .iter()
                    .all(|(name, value)| effective(name).is_some_and(|current| current == *value))
        })
        .map(|profile| {
            shader_lang_label(session, &format!("profile.{}", profile.name), &profile.name)
        })
        .unwrap_or_else(|| "Custom".to_string());
    let profile_row = || ShaderOptionRow {
        key: "<profile>".to_string(),
        kind: ShaderOptionRowKind::Profile,
        label: "Profile".to_string(),
        description: shader_lang_label(session, "profile.comment", ""),
        value: profile_value.clone(),
        is_default: true,
        can_previous: true,
        can_next: true,
    };

    let mut rows = Vec::new();
    match screen {
        Some(screen) => {
            for entry in &screen.entries {
                match entry {
                    OptifineScreenEntry::Option(name) => {
                        rows.push(option_row(session, value_of(name), name));
                    }
                    OptifineScreenEntry::Screen(name) => rows.push(ShaderOptionRow {
                        key: name.clone(),
                        kind: ShaderOptionRowKind::Screen,
                        label: shader_lang_label(session, &format!("screen.{name}"), name),
                        description: String::new(),
                        value: ">".to_string(),
                        is_default: true,
                        can_previous: false,
                        can_next: false,
                    }),
                    OptifineScreenEntry::Profile => rows.push(profile_row()),
                    OptifineScreenEntry::Empty => rows.push(ShaderOptionRow {
                        kind: ShaderOptionRowKind::Spacer,
                        ..Default::default()
                    }),
                    OptifineScreenEntry::Remaining => {
                        for option in &manifest.options {
                            if option.screens.is_empty()
                                && option.kind != OptifineOptionKind::Unknown
                            {
                                rows.push(option_row(
                                    session,
                                    value_of(&option.name),
                                    &option.name,
                                ));
                            }
                        }
                    }
                }
            }
        }
        None => {
            // No screen layout: profiles first, then every option sorted.
            if !manifest.profiles.is_empty() {
                rows.push(profile_row());
            }
            let mut names: Vec<&String> = manifest
                .options
                .iter()
                .filter(|option| option.kind != OptifineOptionKind::Unknown)
                .map(|option| &option.name)
                .collect();
            names.sort();
            for name in names {
                rows.push(option_row(session, value_of(name), name));
            }
        }
    }
    (title, rows)
}
