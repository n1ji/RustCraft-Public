#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContainerKind {
    Generic,
    CraftingTable,
    Dispenser,
    Furnace,
    BrewingStand,
    EnchantingTable,
    Anvil,
    Beacon,
    Hopper,
    Villager,
    Horse,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ContainerLayout {
    pub(crate) kind: ContainerKind,
    pub(crate) slot_count: usize,
    pub(crate) columns: usize,
    pub(crate) rows: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AtlasRegion {
    pub(crate) cell: usize,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl AtlasRegion {
    pub(crate) fn u(self) -> f32 {
        ((self.cell % 4) as f32 * 256.0) / 1024.0
    }

    pub(crate) fn v(self) -> f32 {
        ((self.cell / 4) as f32 * 256.0) / 768.0
    }
}

impl ContainerLayout {
    pub(crate) fn for_window(window_type: &str, slot_count: usize) -> Self {
        let normalized = window_type
            .strip_prefix("minecraft:")
            .unwrap_or(window_type);
        let kind = match normalized {
            "crafting_table" | "workbench" => ContainerKind::CraftingTable,
            "dispenser" | "dropper" => ContainerKind::Dispenser,
            "furnace" => ContainerKind::Furnace,
            "brewing_stand" | "brewing" => ContainerKind::BrewingStand,
            "enchanting_table" | "enchanting" => ContainerKind::EnchantingTable,
            "anvil" => ContainerKind::Anvil,
            "beacon" => ContainerKind::Beacon,
            "hopper" => ContainerKind::Hopper,
            "villager" => ContainerKind::Villager,
            "EntityHorse" => ContainerKind::Horse,
            _ => ContainerKind::Generic,
        };
        let slot_count =
            crate::client::inventory::effective_container_slot_count(window_type, slot_count);
        let (columns, rows, slot_count) = match kind {
            ContainerKind::CraftingTable => (3, 3, slot_count.max(10).min(10)),
            ContainerKind::Dispenser => (3, 3, slot_count.max(9).min(9)),
            ContainerKind::Furnace => (3, 2, slot_count.max(3).min(3)),
            ContainerKind::BrewingStand => (4, 2, slot_count.max(4).min(4)),
            ContainerKind::EnchantingTable => (2, 1, slot_count.max(2).min(2)),
            ContainerKind::Anvil => (3, 1, slot_count.max(3).min(3)),
            ContainerKind::Beacon => (1, 1, slot_count.max(1).min(1)),
            ContainerKind::Hopper => (5, 1, slot_count.max(5).min(5)),
            ContainerKind::Villager => (3, 1, slot_count.max(3).min(3)),
            ContainerKind::Horse => (5, 3, slot_count.clamp(2, 17)),
            ContainerKind::Generic => {
                let count = slot_count.min(90);
                (9, ((count + 8) / 9).max(1), count)
            }
        };
        Self {
            kind,
            slot_count,
            columns,
            rows,
        }
    }

    pub(crate) fn atlas_region(&self) -> AtlasRegion {
        match self.kind {
            ContainerKind::CraftingTable => AtlasRegion {
                cell: 1,
                w: 176.0,
                h: 166.0,
            },
            ContainerKind::Furnace => AtlasRegion {
                cell: 2,
                w: 176.0,
                h: 166.0,
            },
            ContainerKind::Dispenser => AtlasRegion {
                cell: 3,
                w: 176.0,
                h: 166.0,
            },
            ContainerKind::Hopper => AtlasRegion {
                cell: 4,
                w: 176.0,
                h: 133.0,
            },
            ContainerKind::BrewingStand => AtlasRegion {
                cell: 5,
                w: 176.0,
                h: 166.0,
            },
            ContainerKind::EnchantingTable => AtlasRegion {
                cell: 6,
                w: 176.0,
                h: 166.0,
            },
            ContainerKind::Anvil => AtlasRegion {
                cell: 7,
                w: 176.0,
                h: 166.0,
            },
            ContainerKind::Beacon => AtlasRegion {
                cell: 8,
                w: 230.0,
                h: 219.0,
            },
            ContainerKind::Villager => AtlasRegion {
                cell: 9,
                w: 176.0,
                h: 166.0,
            },
            ContainerKind::Horse => AtlasRegion {
                cell: 10,
                w: 176.0,
                h: 166.0,
            },
            ContainerKind::Generic => AtlasRegion {
                cell: 0,
                w: 176.0,
                h: 114.0 + self.rows.clamp(1, 6) as f32 * 18.0,
            },
        }
    }

    pub(crate) fn player_inventory_x(&self) -> f32 {
        match self.kind {
            ContainerKind::Beacon => 36.0,
            _ => 8.0,
        }
    }

    pub(crate) fn player_inventory_y(&self) -> f32 {
        match self.kind {
            ContainerKind::Hopper => 51.0,
            ContainerKind::Beacon => 137.0,
            _ => 84.0,
        }
    }

    pub(crate) fn slot_pos(
        &self,
        slot: usize,
        grid_x: f32,
        y: f32,
        slot_size: f32,
        slot_gap: f32,
        gs: f32,
    ) -> Option<(f32, f32)> {
        match self.kind {
            ContainerKind::CraftingTable => match slot {
                0 => Some((grid_x + 124.0 * gs, y + 35.0 * gs)),
                1..=9 => {
                    let idx = slot - 1;
                    let col = idx % 3;
                    let row = idx / 3;
                    Some((
                        grid_x + 30.0 * gs + col as f32 * (slot_size + slot_gap),
                        y + 17.0 * gs + row as f32 * (slot_size + slot_gap),
                    ))
                }
                _ => None,
            },
            ContainerKind::Dispenser => {
                let col = slot % 3;
                let row = slot / 3;
                Some((
                    grid_x + 62.0 * gs + col as f32 * (slot_size + slot_gap),
                    y + 17.0 * gs + row as f32 * (slot_size + slot_gap),
                ))
            }
            ContainerKind::Furnace => match slot {
                0 => Some((grid_x + 56.0 * gs, y + 17.0 * gs)),
                1 => Some((grid_x + 56.0 * gs, y + 53.0 * gs)),
                2 => Some((grid_x + 116.0 * gs, y + 35.0 * gs)),
                _ => None,
            },
            ContainerKind::BrewingStand => match slot {
                0 => Some((grid_x + 56.0 * gs, y + 46.0 * gs)),
                1 => Some((grid_x + 79.0 * gs, y + 53.0 * gs)),
                2 => Some((grid_x + 102.0 * gs, y + 46.0 * gs)),
                3 => Some((grid_x + 79.0 * gs, y + 17.0 * gs)),
                _ => None,
            },
            ContainerKind::EnchantingTable => match slot {
                0 => Some((grid_x + 15.0 * gs, y + 47.0 * gs)),
                1 => Some((grid_x + 35.0 * gs, y + 47.0 * gs)),
                _ => None,
            },
            ContainerKind::Anvil => match slot {
                0 => Some((grid_x + 27.0 * gs, y + 47.0 * gs)),
                1 => Some((grid_x + 76.0 * gs, y + 47.0 * gs)),
                2 => Some((grid_x + 134.0 * gs, y + 47.0 * gs)),
                _ => None,
            },
            ContainerKind::Beacon => match slot {
                0 => Some((grid_x + 136.0 * gs, y + 110.0 * gs)),
                _ => None,
            },
            ContainerKind::Hopper => {
                let col = slot % 5;
                Some((
                    grid_x + 44.0 * gs + col as f32 * (slot_size + slot_gap),
                    y + 20.0 * gs,
                ))
            }
            ContainerKind::Villager => match slot {
                0 => Some((grid_x + 36.0 * gs, y + 53.0 * gs)),
                1 => Some((grid_x + 62.0 * gs, y + 53.0 * gs)),
                2 => Some((grid_x + 120.0 * gs, y + 53.0 * gs)),
                _ => None,
            },
            ContainerKind::Horse => match slot {
                0 => Some((grid_x + 8.0 * gs, y + 18.0 * gs)),
                1 => Some((grid_x + 8.0 * gs, y + 36.0 * gs)),
                2..=16 => {
                    let idx = slot - 2;
                    Some((
                        grid_x + 80.0 * gs + (idx % 5) as f32 * (slot_size + slot_gap),
                        y + 18.0 * gs + (idx / 5) as f32 * (slot_size + slot_gap),
                    ))
                }
                _ => None,
            },
            ContainerKind::Generic => {
                let col = slot % self.columns;
                let row = slot / self.columns;
                Some((
                    grid_x + col as f32 * (slot_size + slot_gap),
                    y + row as f32 * (slot_size + slot_gap),
                ))
            }
        }
    }
}

#[cfg(test)]
mod container_layout_tests {
    use super::{ContainerKind, ContainerLayout};

    #[test]
    fn every_vanilla_special_window_uses_its_native_container() {
        let cases = [
            (
                "minecraft:crafting_table",
                0,
                ContainerKind::CraftingTable,
                10,
            ),
            ("minecraft:dispenser", 9, ContainerKind::Dispenser, 9),
            ("minecraft:dropper", 9, ContainerKind::Dispenser, 9),
            ("minecraft:furnace", 3, ContainerKind::Furnace, 3),
            ("minecraft:brewing_stand", 4, ContainerKind::BrewingStand, 4),
            (
                "minecraft:enchanting_table",
                0,
                ContainerKind::EnchantingTable,
                2,
            ),
            ("minecraft:anvil", 0, ContainerKind::Anvil, 3),
            ("minecraft:beacon", 1, ContainerKind::Beacon, 1),
            ("minecraft:hopper", 5, ContainerKind::Hopper, 5),
            ("minecraft:villager", 3, ContainerKind::Villager, 3),
            ("EntityHorse", 17, ContainerKind::Horse, 17),
        ];

        for (window_type, advertised, expected_kind, expected_slots) in cases {
            let layout = ContainerLayout::for_window(window_type, advertised);
            assert_eq!(layout.kind, expected_kind, "{window_type}");
            assert_eq!(layout.slot_count, expected_slots, "{window_type}");
        }
    }

    #[test]
    fn special_layouts_match_vanilla_dimensions_and_slot_origins() {
        let beacon = ContainerLayout::for_window("minecraft:beacon", 1);
        assert_eq!(
            (beacon.atlas_region().w, beacon.atlas_region().h),
            (230.0, 219.0)
        );
        assert_eq!(
            (beacon.player_inventory_x(), beacon.player_inventory_y()),
            (36.0, 137.0)
        );

        let brewing = ContainerLayout::for_window("minecraft:brewing_stand", 4);
        assert_eq!(
            brewing.slot_pos(0, 0.0, 0.0, 16.0, 2.0, 1.0),
            Some((56.0, 46.0))
        );
        assert_eq!(
            brewing.slot_pos(3, 0.0, 0.0, 16.0, 2.0, 1.0),
            Some((79.0, 17.0))
        );

        let horse = ContainerLayout::for_window("EntityHorse", 17);
        assert_eq!(
            horse.slot_pos(16, 0.0, 0.0, 16.0, 2.0, 1.0),
            Some((152.0, 54.0))
        );
    }
}
