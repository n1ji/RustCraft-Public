use crate::client::inventory::ItemStackView;
use crate::net::nbt::NbtTag;
use crate::render::gui::widgets::{draw_button, MenuMetrics};
use crate::render::hooks::ScriptDrawCommand;
use crate::ui::format::format_text;
use std::collections::HashMap;

use crate::render::hud::status::hud_metrics;
use crate::render::hud::{HudContext, HudRenderer, TooltipLine};
pub(super) fn item_display_name(context: &HudContext<'_>, item_id: u16, damage: u16) -> String {
    if item_id < 256 {
        for key in block_translation_keys(item_id, damage) {
            let value = context.settings.ui_text.dynamic(key);
            if value != *key {
                return value;
            }
        }
    }

    for key in item_translation_keys(item_id, damage) {
        let value = context.settings.ui_text.dynamic(key);
        if value != *key {
            return value;
        }
    }

    context
        .settings
        .ui_text
        .get("rustcraft.item.unknown")
        .to_string()
}

pub(super) fn append_extra_stack_details(
    context: &HudContext<'_>,
    stack: &ItemStackView,
    compound: Option<&HashMap<String, NbtTag>>,
    lines: &mut Vec<TooltipLine>,
) {
    if stack.item_id == 373 {
        let mut potion_lines = potion_tooltip_lines(context, stack.damage, compound);
        if lines
            .first()
            .map(|line| line.text == context.settings.ui_text.dynamic("item.potion.name"))
            .unwrap_or(false)
        {
            if let Some(first) = potion_lines.first() {
                lines[0].text = first.text.clone();
                potion_lines.remove(0);
            }
        }
        lines.extend(potion_lines);
    }

    if stack.item_id == 401 {
        if let Some(flight) = compound
            .and_then(|root| root.get("Fireworks"))
            .and_then(NbtTag::as_compound)
            .and_then(|fireworks| fireworks.get("Flight"))
            .and_then(NbtTag::as_i32)
        {
            lines.push(TooltipLine {
                text: format!(
                    "{} {}",
                    context.settings.ui_text.dynamic("item.fireworks.flight"),
                    flight
                ),
                color: TOOLTIP_GRAY,
            });
        }
    }

    if context.settings.advanced_tooltips {
        if let Some(max_damage) = max_damage_for_item(stack.item_id) {
            let remaining = max_damage.saturating_sub(stack.damage as u32);
            lines.push(TooltipLine {
                text: format_text(
                    &context
                        .settings
                        .ui_text
                        .dynamic("rustcraft.tooltip.durability"),
                    &[&remaining.to_string(), &max_damage.to_string()],
                ),
                color: TOOLTIP_GRAY,
            });
        }
    }
}

pub(super) fn append_block_list(
    context: &HudContext<'_>,
    compound: Option<&HashMap<String, NbtTag>>,
    tag_key: &str,
    title_key: &str,
    lines: &mut Vec<TooltipLine>,
) {
    let Some(blocks) = compound
        .and_then(|root| root.get(tag_key))
        .and_then(NbtTag::as_list)
    else {
        return;
    };
    let mut names = blocks
        .iter()
        .filter_map(NbtTag::as_str)
        .map(|name| block_name_from_nbt_id(context, name))
        .collect::<Vec<_>>();
    names.dedup();
    if names.is_empty() {
        return;
    }

    lines.push(TooltipLine {
        text: String::new(),
        color: TOOLTIP_GRAY,
    });
    lines.push(TooltipLine {
        text: context.settings.ui_text.dynamic(title_key),
        color: TOOLTIP_GRAY,
    });
    for name in names {
        lines.push(TooltipLine {
            text: name,
            color: TOOLTIP_DARK_GRAY,
        });
    }
}

pub(super) fn append_attribute_modifiers(
    context: &HudContext<'_>,
    compound: Option<&HashMap<String, NbtTag>>,
    lines: &mut Vec<TooltipLine>,
) {
    let Some(modifiers) = compound
        .and_then(|root| root.get("AttributeModifiers"))
        .and_then(NbtTag::as_list)
    else {
        return;
    };
    let mut added_header = false;
    for modifier in modifiers {
        let Some(modifier) = modifier.as_compound() else {
            continue;
        };
        let attr = modifier
            .get("AttributeName")
            .or_else(|| modifier.get("Name"))
            .and_then(NbtTag::as_str)
            .unwrap_or("");
        let amount = modifier
            .get("Amount")
            .and_then(NbtTag::as_f64)
            .unwrap_or(0.0);
        let operation = modifier
            .get("Operation")
            .and_then(NbtTag::as_i32)
            .unwrap_or(0)
            .clamp(0, 2);
        if attr.is_empty() || amount == 0.0 {
            continue;
        }
        if !added_header {
            lines.push(TooltipLine {
                text: String::new(),
                color: TOOLTIP_GRAY,
            });
            lines.push(TooltipLine {
                text: context
                    .settings
                    .ui_text
                    .dynamic("rustcraft.tooltip.whenEquipped"),
                color: TOOLTIP_GRAY,
            });
            added_header = true;
        }
        let attr_name = attribute_name(context, attr);
        lines.push(TooltipLine {
            text: attribute_modifier_text(context, amount, operation, &attr_name),
            color: if amount > 0.0 {
                [0.33, 0.78, 0.33, 1.0]
            } else {
                [0.78, 0.33, 0.33, 1.0]
            },
        });
    }
}

pub(super) fn potion_tooltip_lines(
    context: &HudContext<'_>,
    damage: u16,
    compound: Option<&HashMap<String, NbtTag>>,
) -> Vec<TooltipLine> {
    let mut lines = Vec::new();
    let potion = potion_effect_from_damage(damage);
    let mut potion_name = context.settings.ui_text.dynamic(
        potion
            .map(|effect| effect.postfix_key)
            .unwrap_or("item.emptyPotion.name"),
    );
    if damage & 0x4000 != 0 && potion.is_some() {
        let splash = context.settings.ui_text.dynamic("potion.prefix.grenade");
        potion_name = format!("{} {}", splash, potion_name);
    }
    lines.push(TooltipLine {
        text: potion_name,
        color: [1.0, 1.0, 1.0, 1.0],
    });

    let custom_effects = compound
        .and_then(|root| root.get("CustomPotionEffects"))
        .and_then(NbtTag::as_list);
    if let Some(custom_effects) = custom_effects {
        for effect in custom_effects {
            let Some(effect) = effect.as_compound() else {
                continue;
            };
            let id = effect.get("Id").and_then(NbtTag::as_i32).unwrap_or(0);
            let amplifier = effect
                .get("Amplifier")
                .and_then(NbtTag::as_i32)
                .unwrap_or(0);
            let duration = effect.get("Duration").and_then(NbtTag::as_i32).unwrap_or(0);
            if let Some(effect) = potion_effect_by_id(id) {
                lines.push(TooltipLine {
                    text: potion_effect_line(context, effect.name_key, amplifier, duration),
                    color: TOOLTIP_BLUE,
                });
            }
        }
    } else if let Some(effect) = potion {
        lines.push(TooltipLine {
            text: potion_effect_line(context, effect.name_key, effect.amplifier, effect.duration),
            color: TOOLTIP_BLUE,
        });
    } else {
        lines.push(TooltipLine {
            text: context.settings.ui_text.dynamic("potion.empty"),
            color: TOOLTIP_GRAY,
        });
    }

    lines
}

pub(super) fn potion_effect_line(
    context: &HudContext<'_>,
    name_key: &str,
    amplifier: i32,
    duration_ticks: i32,
) -> String {
    let potency_key = format!("potion.potency.{}", amplifier.clamp(0, 3));
    let potency = context.settings.ui_text.dynamic(&potency_key);
    let duration = format_duration(duration_ticks);
    let name = context.settings.ui_text.dynamic(name_key);
    if potency.is_empty() {
        format!("{} ({})", name, duration)
    } else {
        format!("{} {} ({})", name, potency, duration)
    }
}

pub(super) fn block_name_from_nbt_id(context: &HudContext<'_>, id: &str) -> String {
    let normalized = id.strip_prefix("minecraft:").unwrap_or(id);
    for key in block_id_translation_keys(normalized) {
        let value = context.settings.ui_text.dynamic(key);
        if value != *key {
            return value;
        }
    }
    normalized.to_string()
}

pub(super) fn attribute_name(context: &HudContext<'_>, key: &str) -> String {
    let lang_key = format!("attribute.name.{}", key);
    let value = context.settings.ui_text.dynamic(&lang_key);
    if value == lang_key {
        key.rsplit('.').next().unwrap_or(key).to_string()
    } else {
        value
    }
}

pub(super) fn attribute_modifier_text(
    context: &HudContext<'_>,
    amount: f64,
    operation: i32,
    attr_name: &str,
) -> String {
    let percent = operation == 1 || operation == 2;
    let value = if percent { amount * 100.0 } else { amount };
    let key = if value >= 0.0 {
        format!("attribute.modifier.plus.{}", operation)
    } else {
        format!("attribute.modifier.take.{}", operation)
    };
    format_text(
        &context.settings.ui_text.dynamic(&key),
        &[&format_number(value.abs()), attr_name],
    )
}

pub(super) fn enchantment_display_name(context: &HudContext<'_>, id: i16, level: i16) -> String {
    let key = enchantment_key(id);
    let name = key
        .map(|key| context.settings.ui_text.dynamic(key))
        .filter(|value| !value.starts_with("enchantment."))
        .unwrap_or_else(|| {
            context
                .settings
                .ui_text
                .get("rustcraft.enchant.unknown")
                .to_string()
        });
    let level_key = format!("enchantment.level.{}", level.max(1));
    let level_text = context.settings.ui_text.dynamic(&level_key);
    if level_text == level_key {
        format!("{} {}", name, level)
    } else {
        format!("{} {}", name, level_text)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Enchantment {
    pub(crate) id: i16,
    pub(crate) level: i16,
}

pub(super) fn enchantments_from(
    compound: Option<&HashMap<String, NbtTag>>,
    key: &str,
) -> Vec<Enchantment> {
    compound
        .and_then(|root| root.get(key))
        .and_then(NbtTag::as_list)
        .map(|list| {
            list.iter()
                .filter_map(|tag| {
                    let compound = tag.as_compound()?;
                    Some(Enchantment {
                        id: compound.get("id").and_then(NbtTag::as_i16).unwrap_or(0),
                        level: compound.get("lvl").and_then(NbtTag::as_i16).unwrap_or(1),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn enchantment_key(id: i16) -> Option<&'static str> {
    Some(match id {
        0 => "enchantment.protect.all",
        1 => "enchantment.protect.fire",
        2 => "enchantment.protect.fall",
        3 => "enchantment.protect.explosion",
        4 => "enchantment.protect.projectile",
        5 => "enchantment.oxygen",
        6 => "enchantment.waterWorker",
        7 => "enchantment.thorns",
        8 => "enchantment.waterWalker",
        16 => "enchantment.damage.all",
        17 => "enchantment.damage.undead",
        18 => "enchantment.damage.arthropods",
        19 => "enchantment.knockback",
        20 => "enchantment.fire",
        21 => "enchantment.lootBonus",
        32 => "enchantment.digging",
        33 => "enchantment.untouching",
        34 => "enchantment.durability",
        35 => "enchantment.lootBonusDigger",
        48 => "enchantment.arrowDamage",
        49 => "enchantment.arrowKnockback",
        50 => "enchantment.arrowFire",
        51 => "enchantment.arrowInfinite",
        61 => "enchantment.lootBonusFishing",
        62 => "enchantment.fishingSpeed",
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PotionEffect {
    name_key: &'static str,
    postfix_key: &'static str,
    amplifier: i32,
    duration: i32,
}

pub(super) fn potion_effect_from_damage(damage: u16) -> Option<PotionEffect> {
    let base = damage & 0x3f;
    let amplified = damage & 0x20 != 0;
    let extended = damage & 0x40 != 0;
    let (name_key, postfix_key, default_duration) = match base {
        1 => ("potion.regeneration", "potion.regeneration.postfix", 900),
        2 => ("potion.moveSpeed", "potion.moveSpeed.postfix", 3600),
        3 => (
            "potion.fireResistance",
            "potion.fireResistance.postfix",
            3600,
        ),
        4 => ("potion.poison", "potion.poison.postfix", 900),
        5 => ("potion.heal", "potion.heal.postfix", 1),
        6 => ("potion.nightVision", "potion.nightVision.postfix", 3600),
        8 => ("potion.weakness", "potion.weakness.postfix", 1800),
        9 => ("potion.damageBoost", "potion.damageBoost.postfix", 3600),
        10 => ("potion.moveSlowdown", "potion.moveSlowdown.postfix", 1800),
        11 => ("potion.jump", "potion.jump.postfix", 3600),
        12 => ("potion.harm", "potion.harm.postfix", 1),
        13 => (
            "potion.waterBreathing",
            "potion.waterBreathing.postfix",
            3600,
        ),
        14 => ("potion.invisibility", "potion.invisibility.postfix", 3600),
        _ => return None,
    };
    let mut duration = default_duration;
    if extended && duration > 1 {
        duration = (duration as f32 * 8.0 / 3.0).round() as i32;
    }
    if amplified && duration > 1 {
        duration /= 2;
    }
    Some(PotionEffect {
        name_key,
        postfix_key,
        amplifier: if amplified { 1 } else { 0 },
        duration,
    })
}

pub(super) fn potion_effect_by_id(id: i32) -> Option<PotionEffect> {
    let (name_key, postfix_key) = match id {
        1 => ("potion.moveSpeed", "potion.moveSpeed.postfix"),
        2 => ("potion.moveSlowdown", "potion.moveSlowdown.postfix"),
        3 => ("potion.digSpeed", "potion.digSpeed.postfix"),
        4 => ("potion.digSlowDown", "potion.digSlowDown.postfix"),
        5 => ("potion.damageBoost", "potion.damageBoost.postfix"),
        6 => ("potion.heal", "potion.heal.postfix"),
        7 => ("potion.harm", "potion.harm.postfix"),
        8 => ("potion.jump", "potion.jump.postfix"),
        9 => ("potion.confusion", "potion.confusion.postfix"),
        10 => ("potion.regeneration", "potion.regeneration.postfix"),
        11 => ("potion.resistance", "potion.resistance.postfix"),
        12 => ("potion.fireResistance", "potion.fireResistance.postfix"),
        13 => ("potion.waterBreathing", "potion.waterBreathing.postfix"),
        14 => ("potion.invisibility", "potion.invisibility.postfix"),
        15 => ("potion.blindness", "potion.blindness.postfix"),
        16 => ("potion.nightVision", "potion.nightVision.postfix"),
        17 => ("potion.hunger", "potion.hunger.postfix"),
        18 => ("potion.weakness", "potion.weakness.postfix"),
        19 => ("potion.poison", "potion.poison.postfix"),
        20 => ("potion.wither", "potion.wither.postfix"),
        21 => ("potion.healthBoost", "potion.healthBoost.postfix"),
        22 => ("potion.absorption", "potion.absorption.postfix"),
        23 => ("potion.saturation", "potion.saturation.postfix"),
        _ => return None,
    };
    Some(PotionEffect {
        name_key,
        postfix_key,
        amplifier: 0,
        duration: 0,
    })
}
use super::render::*;
use super::translations::*;
