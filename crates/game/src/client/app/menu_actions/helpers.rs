use crate::audio::AudioBackend;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use crate::ui::button_ids as btn;
use winit::event_loop::ActiveEventLoop;

use crate::client::app::{App, PendingShaderPackLoad, ShaderPackLoadReason};
pub(crate) fn open_folder(path: &std::path::Path) {
    let _ = std::fs::create_dir_all(path);
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

pub(crate) fn config_value_label(
    kind: &crate::scripting::ConfigEntryKind,
    value: &crate::scripting::ConfigValue,
) -> String {
    use crate::scripting::{ConfigEntryKind, ConfigValue};

    match (kind, value) {
        (ConfigEntryKind::Boolean, ConfigValue::Boolean(value)) => {
            if *value { "On" } else { "Off" }.to_string()
        }
        (ConfigEntryKind::Number { .. }, ConfigValue::Number(value)) => {
            let formatted = format!("{value:.4}");
            formatted
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        }
        (ConfigEntryKind::Choice { options }, ConfigValue::Choice(value)) => options
            .iter()
            .find(|option| option.value == *value)
            .map(|option| option.label.clone())
            .unwrap_or_else(|| value.clone()),
        _ => "Invalid value".to_string(),
    }
}

pub(crate) fn adjusted_config_value(
    kind: &crate::scripting::ConfigEntryKind,
    value: &crate::scripting::ConfigValue,
    direction: i32,
) -> Option<crate::scripting::ConfigValue> {
    use crate::scripting::{ConfigEntryKind, ConfigValue};

    match (kind, value) {
        (ConfigEntryKind::Boolean, ConfigValue::Boolean(value)) => {
            Some(ConfigValue::Boolean(!value))
        }
        (ConfigEntryKind::Number { min, max, step }, ConfigValue::Number(value)) => {
            let raw = if direction < 0 {
                value - step
            } else {
                value + step
            }
            .clamp(*min, *max);
            let snapped = (min + ((raw - min) / step).round() * step).clamp(*min, *max);
            Some(ConfigValue::Number(snapped))
        }
        (ConfigEntryKind::Choice { options }, ConfigValue::Choice(value))
            if !options.is_empty() =>
        {
            let current = options
                .iter()
                .position(|option| option.value == *value)
                .unwrap_or(0);
            let next = if direction < 0 {
                current.checked_sub(1).unwrap_or(options.len() - 1)
            } else {
                (current + 1) % options.len()
            };
            Some(ConfigValue::Choice(options[next].value.clone()))
        }
        _ => None,
    }
}

/// Matches GuiOptionSlider's track: the 8 px thumb travels between the two
/// 4 px insets of the 20 px-tall button, scaled with the GUI.
pub(crate) fn slider_value_from_mouse(mouse_x: f32, x: f32, width: f32, height: f32) -> f32 {
    let inset = 4.0 * (height / 20.0);
    ((mouse_x - (x + inset)) / (width - inset * 2.0)).clamp(0.0, 1.0)
}

pub(crate) fn next_framerate(current: u32) -> u32 {
    match current {
        0..=29 => 30,
        30..=59 => 60,
        60..=119 => 120,
        120..=239 => 240,
        240..=499 => 500,
        500..=999 => crate::client::config::UNLIMITED_FRAMERATE,
        _ => crate::client::config::UNLIMITED_FRAMERATE,
    }
}

pub(crate) fn previous_framerate(current: u32) -> u32 {
    match current {
        0..=30 => 30,
        31..=60 => 30,
        61..=120 => 60,
        121..=240 => 120,
        241..=500 => 240,
        _ => 500,
    }
}

pub(crate) fn slider_to_framerate(value: f32) -> u32 {
    if value >= 0.995 {
        crate::client::config::UNLIMITED_FRAMERATE
    } else {
        (30.0 + value.clamp(0.0, 1.0) * (crate::client::config::UNLIMITED_FRAMERATE - 30) as f32)
            .round() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::adjusted_config_value;
    use crate::scripting::{ConfigChoice, ConfigEntryKind, ConfigValue};

    #[test]
    fn mod_config_controls_toggle_step_clamp_and_cycle() {
        assert_eq!(
            adjusted_config_value(&ConfigEntryKind::Boolean, &ConfigValue::Boolean(true), 1,),
            Some(ConfigValue::Boolean(false))
        );

        let number = ConfigEntryKind::Number {
            min: 0.0,
            max: 2.0,
            step: 0.1,
        };
        assert_eq!(
            adjusted_config_value(&number, &ConfigValue::Number(1.0), 1),
            Some(ConfigValue::Number(1.1))
        );
        assert_eq!(
            adjusted_config_value(&number, &ConfigValue::Number(2.0), 1),
            Some(ConfigValue::Number(2.0))
        );

        let choice = ConfigEntryKind::Choice {
            options: vec![
                ConfigChoice {
                    value: "classic".into(),
                    label: "Classic".into(),
                },
                ConfigChoice {
                    value: "subtle".into(),
                    label: "Subtle".into(),
                },
            ],
        };
        assert_eq!(
            adjusted_config_value(&choice, &ConfigValue::Choice("classic".into()), -1),
            Some(ConfigValue::Choice("subtle".into()))
        );
        assert_eq!(
            adjusted_config_value(&choice, &ConfigValue::Choice("subtle".into()), 1),
            Some(ConfigValue::Choice("classic".into()))
        );
    }
}
