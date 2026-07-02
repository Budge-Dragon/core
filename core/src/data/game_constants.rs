//! Record shape of `game_constants.json` — global scalar knobs.
//! Single-record file.

use serde::{Deserialize, Serialize};

use super::common::SourceVersion;

/// The global scalar constants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameConstants {
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Real-time length of one simulation tick.
    pub tick_duration_ms: u32,
    /// Interval between health/mana recovery pulses.
    pub recovery_interval_ms: u32,
    /// Radius in tiles within which entities observe each other.
    pub info_range: u8,
    /// How long a dropped item stays on the ground.
    pub item_drop_duration_ms: u32,
    /// Highest option level a drop can roll.
    pub max_item_option_level_drop: u8,
    /// Drop-level surcharge applied to excellent items.
    pub excellent_drop_level_delta: u8,
    /// Highest option level reachable by jewels.
    pub max_option_level: u8,
    /// Whether monsters drop money at all.
    pub should_drop_money: bool,
    /// Multiplier on dropped money amounts.
    pub money_amount_rate: f64,
    /// Flat money added to every money drop.
    pub base_money_drop: u32,
    /// Widest drop-level window an item pool spans.
    pub drop_level_max_gap: u8,
    /// Probability a dropped weapon rolls +Skill, `0.0..=1.0`.
    pub skill_drop_chance: f64,
    /// Money cap of the inventory.
    pub max_inventory_money: u32,
    /// Money cap of the vault.
    pub max_vault_money: u32,
    /// Whether picking up money clamps at the cap instead of failing.
    pub clamp_money_on_pickup: bool,
    /// Maximum members in one party.
    pub maximum_party_size: u8,
    /// Maximum characters on one account.
    pub max_characters_per_account: u8,
    /// Regex a new character name must match.
    pub character_name_regex: String,
    /// Whether experience gain stops at the table's end.
    pub prevent_experience_overflow: bool,
    /// Whether area skills hit players.
    pub area_skill_hits_player: bool,
    /// Inclusive `[min, max]` random multiplier on per-kill experience.
    pub random_exp_multiplier_range: [f64; 2],
    /// Damage dealt per point of item durability lost.
    pub damage_per_one_item_durability: u32,
    /// Damage dealt per point of pet durability lost.
    pub damage_per_one_pet_durability: u32,
    /// Hits taken per point of item durability lost.
    pub hits_per_one_item_durability: u32,
    /// Floor on any hit chance, `0.0..=1.0`.
    pub minimum_hit_chance: f64,
    /// Damage factor applied when attack rate overshoots defense rate.
    pub overrate_damage_factor: f64,
    /// Inventory geometry.
    pub inventory: InventoryConstants,
    /// Movement speed knobs.
    pub movement: MovementConstants,
    /// Era-doubt note; absent = uncontested.
    pub review: Option<String>,
}

/// Inventory geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InventoryConstants {
    /// Equipment slots.
    pub equipped_slots: u8,
    /// Rows of the main inventory grid.
    pub main_rows: u8,
    /// Columns of the main inventory grid.
    pub main_columns: u8,
    /// Personal store slots.
    pub store_slots: u8,
    /// Vault slots.
    pub vault_slots: u8,
    /// Temporary storage slots (trade, crafting).
    pub temp_storage_slots: u8,
}

/// Movement speed knobs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MovementConstants {
    /// Item level at which gear enables running.
    pub running_gear_min_level: u8,
    /// Speed with running gear.
    pub running_gear_speed: f64,
    /// Speed with wings.
    pub wing_speed: f64,
    /// Speed with fast wings.
    pub fast_wing_speed: f64,
    /// Speed multiplier while iced.
    pub iced_speed_factor: f64,
}
