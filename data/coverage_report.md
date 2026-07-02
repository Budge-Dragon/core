# Coverage Report — mu-core data extraction wave

Assembled 2026-07-02 from `data/_coverage/*.json`. Spec: `docs/specs/2026-07-02-data-schemas.md`.
Baseline: full 0.95d dataset plus curated 1.0-era backports from the S6 dataset.
Version policy: `075` = record ships in a 095d-reused Version075 initializer, `095d` = 095d-specific, `s6` = 1.0-era backport (every s6 record carries a `review`).

## 1. Record counts by category and source_version

| Category | File | 075 | 095d | s6 | Total |
|---|---|---:|---:|---:|---:|
| chaos mixes | `chaos_mixes.json` | 1 | 5 | 4 | 10 |
| classes | `character_classes.json` | 4 | 1 | 4 | 9 |
| constants + exp | `game_constants.json`, `exp_tables.json` | 2 | 0 | 0 | 2 |
| drops | `drop_groups.json` | 3 | 12 | 18 | 33 |
| items | `item_definitions.json` | 196 | 14 | 33 | 243 |
| items | `item_level_bonus_tables.json` | 13 | 1 | 1 | 15 |
| maps | `map_definitions.json` | 7 | 7 | 0 | 14 |
| maps | `spawn_areas.json` | 1563 | 312 | 0 | 1875 |
| maps | `gates_warps.json` (exit 34, enter 22, warp 14) | 66 | 4 | 0 | 70 |
| maps | `terrain/` binaries | — | — | — | 14 files |
| monsters | `monster_definitions.json` | 73 | 27 | 0 | 100 |
| options | `item_options.json` | 9 | 4 | 11 | 24 |
| sets | `item_sets.json` | 45 | 0 | 36 | 81 |
| skills | `skills.json` | 30 | 5 | 17 | 52 |
| effects | `magic_effects.json` | 7 | 0 | 6 | 13 |
| stats | `stats.json` | 117 | 5 | 28 | 150 |
| **Total (JSON records)** | | **2136** | **397** | **158** | **2691** |

Plus 14 terrain binaries (not versioned JSON records).

## 2. Review-flagged records — era-review list (171 total)

### chaos_mixes (4)

- `fruits` — stat fruits are 1.0-era, data only in S6; created fruit level (stat kind) is weighted-random 0-4 in the crafting rule, not data
- `second_wings` — 2nd wings are 1.0-era, data only in S6: Summoner wings removed (Wing of Misery 12/41 input, Wings of Despair 12/42 result), level caps clamped 15->11; luck/exc result chances (20/20, max 1 exc) are S6 values
- `blood_castle_ticket` — Blood Castle is 0.97/1.0-era; recipe, prices and success rates live in the ticket rule (S6 handler values, see rules section)
- `cape_of_lord` — **era-check**: Cape of Lord is the 1.0-era Dark Lord wing, but this crafting (#24, Monarch's Crest = Loch's Feather+1 recipe) may postdate 1.0; Cape of Fighter (12/49, Rage Fighter) result and Wing of Misery (12/41) input removed, level caps clamped 15->11

### classes (8)

- `dark_wizard` — evolution to soul_master at level 150 is a curated 1.0-era backport (075/095d datasets ship no second classes); rest of the record is the shipped shared-initializer data
- `soul_master` — 1.0-era backport: second class of the dark_wizard line (class change at level 150 predates season 3); defined only in the s6 dataset; rebuilt with classic-pvp rules, shield/master/pet formulas dropped
- `dark_knight` — evolution to blade_knight at level 150 is a curated 1.0-era backport (075/095d datasets ship no second classes); rest of the record is the shipped shared-initializer data
- `blade_knight` — 1.0-era backport: second class of the dark_knight line; defined only in the s6 dataset; rebuilt with classic-pvp rules, shield/master/pet formulas dropped
- `fairy_elf` — evolution to muse_elf at level 150 is a curated 1.0-era backport; rest of the record is the shipped shared-initializer data
- `muse_elf` — 1.0-era backport: second class of the fairy_elf line; defined only in the s6 dataset; rebuilt with classic-pvp rules, shield/master/pet formulas dropped
- `magic_gladiator` — fruit_calculation curated to magic_gladiator per the domain fruit caps (default 127 / mg 100 / dl 115); the shipped initializer never assigns a strategy (all default) and fruits are 1.0-era content
- `dark_lord` — 1.0-era backport: dark lord shipped around v1.0 but is absent from the 075/095d datasets; rebuilt with classic-pvp rules; raven/horse/pet and master-tree formulas dropped (see gaps); fruit_calculation dark_lord per domain caps

### drops (18)

- `scroll_of_archangel_plus_1` … `plus_6` (6 records) — Blood Castle ticket ingredient, approved 1.0-era backport; band values are s6 data
- `scroll_of_archangel_plus_7`, `plus_8` — as above; gates 7/8 are arguably later-era (7: S1+, 8: S3)
- `blood_bone_plus_1` … `plus_6` (6 records) — Blood Castle ticket ingredient, approved 1.0-era backport; band values are s6 data
- `blood_bone_plus_7`, `plus_8` — as above; gates 7/8 are arguably later-era (7: S1+, 8: S3)
- `icarus_lochs_feather` — feeds the approved 2nd-wings mix (1.0-era); s6 Icarus map data, bound to the Icarus map only
- `icarus_crest_of_monarch` — Crest of Monarch (Loch's Feather +1) feeds Cape of Lord — Dark Lord-era content, **era doubt**; bound to the Icarus map only

### items (34)

- `7/26`, `8/26`, `9/26`, `10/26`, `11/26` — 1.0-era Dark Lord (Adamantine) pieces backported from the S6 dataset for the agnis/broy ancient sets; other DL gear is not backported this wave
- `7/40`, `8/40`, `9/40`, `10/40`, `11/40` — Red Wing is Summoner (post-pre-S3 class) gear, **not 1.0-era**; backported only as pieces of the approved chrono/semeden ancient-set backports; classes empty -> unequippable in baseline
- `8/15`, `9/15`, `10/15`, `11/15` — 1.0-era Magic Gladiator (Storm Crow) pieces backported from the S6 dataset for the gaion/muren ancient sets
- `12/3` — 1.0-era 2nd wing (muse_elf only, source class value 2); max_item_level clamped 15->11; dmg +32%/abs 25%, +1%/level (wing_damage_second)
- `12/4` — 1.0-era 2nd wing (soul_master only); clamps/values as above
- `12/5` — 1.0-era 2nd wing (blade_knight only); fast wing speed 16; clamps/values as above
- `12/6` — 1.0-era 2nd wing (magic_gladiator, source class value 1); clamps/values as above
- `13/14` — 1.0-era 2nd-wings mix ingredient (Loch's Feather) from S6 Wings.cs; source sets drops_from_monsters=false and max_item_level=1
- `13/15` — 1.0-era stat fruit (fruits chaos-mix result); item level 0-4 encodes the fruit's stat kind; consume behavior is a rule, not data
- `13/16`, `13/17`, `13/18` — Blood Castle ticket items, approved 1.0-era backport; gates 7/8 of max_item_level are arguably later-era (7: S1+, 8: S3)
- `13/21` — 1.0-era jewelry backport (ancient jewelry sets piece); fire resistance
- `13/22` — as above; earth resistance
- `13/23` — as above; wind resistance
- `13/24` — as above; no resistance; S6-only +1%/level maximum_mana option
- `13/25` — as above; ice resistance
- `13/26` — as above; wind resistance
- `13/27` — as above; water resistance
- `13/28` — as above; no resistance; S6-only +1%/level maximum_ability option
- `13/30` — 1.0-era Dark Lord wing (cape_of_lord mix result); max_item_level clamped 15->11; dmg +20%/abs 10%, +2%/level (wing_damage_first); its S6 option definitions (Cape of Lord Options) are S6-only option data and NOT backported — only luck referenced, matching the 2nd-wing option gap in options_sets
- `14/22` — 0.97d-era jewel (Version097d dataset is the oldest pre-S3 source; **tagged s6 because source_version has no 097d value**); backported as the fruits chaos-mix ingredient
- `wing_damage_second` (bonus table) — backported with 2nd wings; S6 table (1%/level) truncated to cap 11

### maps (1)

- `devias` — 0.75-era map re-derived by the 0.95d dataset with 0.95 terrain (only town map whose initializer overrides the 075_ terrain prefix); tagged 095d because the shipped record differs from 0.75 by terrain only

### monsters (2)

- `#53 Golden Titan` — ships in the upstream 095d dataset, but this golden-invasion tier is commonly dated ~0.97+; kept as 095d per dataset policy
- `#54 Golden Soldier` — same as #53; also carries no drop group in source (likely upstream omission of Box of Kundun +1)

### options (11)

- `jewelery_option_maximum_mana` — S6-only jewelry option backported with the 1.0-era ring_of_magic/pendant_of_ability items (ancient-set pieces)
- `jewelery_option_maximum_ability` — same as above
- `2nd_wing_options` — 1.0-era 2nd-wing options backported with the s6 wing items; hp/mana item-level values truncated from source cap 15 to 11
- `wings_of_spirits_options`, `wings_of_soul_options`, `wings_of_dragon_options`, `wings_of_darkness_options` — 1.0-era jewel-of-life options of backported s6 2nd wings; same value shape as the 075 first-wing options
- `ancient_bonus_total_vitality`, `ancient_bonus_total_strength`, `ancient_bonus_total_agility`, `ancient_bonus_total_energy` — s1-era ancient piece bonus (+5/+10, level rolled at drop); OpenMU ships it only in the s6 dataset

### sets (36)

All 36 ancient sets are s1-era backports from the s6 dataset:
`warrior_leather`, `anonymous_leather`, `hyperion_bronze`, `mist_bronze`, `eplete_scale`, `berserker_scale`, `garuda_brass`, `cloud_brass`, `kantata_plate`, `rave_plate`, `hyon_dragon`, `vicious_dragon`, `apollo_pad`, `barnake_pad`, `evis_bone`, `sylion_bone`, `heras_sphinx`, `minet_sphinx`, `anubis_legendary`, `enis_legendary`, `ceto_vine`, `drake_vine`, `gaia_silk`, `fase_silk`, `odin_wind`, `elvian_wind`, `argo_spirit`, `karis_spirit`, `gywen_guardian`, `aruan_guardian`, `gaion_storm_crow`, `muren_storm_crow`, `agnis_adamantine`, `broy_adamantine`, `chrono_red_wing`, `semeden_red_wing`.

`kantata_plate` additionally fixes an OpenMU data bug: `excellent_damage_chance` 10.0 -> 0.10.

### skills + effects (24)

- `skills/cometfall` — 095d source marks it a plain direct hit yet attaches target-area settings; encoded as area_automatic (the S6 dataset marks it an automatic-hits area skill)
- `skills/soul_barrier` — retail 0.97/1.0 Soul Master skill; absent from 075/095d, values from S6
- `skills/ice_storm` — retail 1.0-era Soul Master AoE; values from S6
- `skills/nova` — retail 1.0 Soul Master skill; mana 15 = 180 per full 12-stage charge; stage damage feeds nova_stage_damage
- `skills/rageful_blow` — retail 0.97/1.0 Blade Knight skill; values from S6
- `skills/death_stab` — retail 0.97/1.0 Blade Knight skill; values from S6
- `skills/swell_life` — retail 0.97/1.0 knight party buff (Greater Fortitude); values from S6
- `skills/ice_arrow` — retail 1.0 Muse Elf skill; values from S6; S6 skill_multiplier rebalance relationship not extracted (see gaps)
- `skills/penetration` — retail 1.0 elf skill; values from S6; S6 skill_multiplier rebalance relationship not extracted
- `skills/fire_slash` — retail 1.0-era Magic Gladiator skill; values from S6
- `skills/power_slash` — retail 1.0-era Magic Gladiator skill; values from S6
- `skills/nova_start` — charge-phase companion of the Nova backport (skill 40); **not on the curated list** but Nova is unusable without it
- `skills/fire_burst` — Dark Lord backport (DL is 0.97/1.0 content, only in the S6 dataset)
- `skills/earthshake` — Dark Lord backport; S6 horse_level*10 damage term dropped (dark horse pet excluded pre-S3)
- `skills/summon` — Dark Lord backport; summons party members (not a monster summon), party-summon behavior is a rules concern
- `skills/increase_critical_damage` — Dark Lord backport
- `skills/infinity_arrow` — retail 1.0 Muse Elf buff; values from S6
- `skills/generic_monster_skill` — monster-only attack skill 150: 075/095d monster initializers reference it but only the S6 skills initializer defines it — **latent upstream omission** (075/095d lookup silently resolves to null); backported so attack_skill references resolve
- `magic_effects/soul_barrier` — effect of the soul_barrier backport; S6 initializer values
- `magic_effects/critical_damage_increase` — effect of the DL increase_critical_damage backport; S6 values
- `magic_effects/infinite_arrow` — effect of the infinity_arrow backport; S6 zero-value master-skill placeholder power-up dropped
- `magic_effects/swell_life` — effect of the swell_life backport; S6 values
- `magic_effects/freeze` — created on demand by the ice_arrow backport; shares sub_type 254 with iced, so freeze replaces iced (source behavior)
- `magic_effects/defense_reduction` — effect of the fire_slash backport; S6 values

### stats (33)

- `base_leadership`, `total_leadership`, `total_leadership_requirement`, `scepter_rise`, `is_scepter_equipped`, `bonus_damage_with_scepter`, `total_energy_minus_15` — dark lord backport (~1.0)
- `final_damage_bonus`, `skill_damage_bonus`, `excellent_damage_bonus`, `two_handed_weapon_damage_increase`, `defense_ignore_chance`, `double_damage_chance` — ancient set option targets (pending decision 5); defense_ignore_chance also fed by the backported 2nd-wing option
- `critical_damage_bonus` — fed by DL critical-damage-increase skill backport and ancient sets
- `combo_bonus` — OpenMU ships the DK combo formula in its 075 dataset; **historically combo arrived ~0.98-1.0**
- `is_skill_combo_available` — dk combo backport (~0.98-1.0); unlocked by the hero-status quest
- `gain_hero_status_quest_completed` — 1.0-era hero-status quest reward flag; legacy quests deferred
- `soul_barrier_receive_decrement`, `soul_barrier_mana_toll_per_hit` — soul barrier backport (1.0-era)
- `is_stunned`, `stun_chance` — stun state/chance; pre-S3 source is the earthshake backport, **OpenMU wires stun only in S6**
- `ice_damage_bonus`, `fire_damage_bonus`, `water_damage_bonus`, `earth_damage_bonus`, `wind_damage_bonus`, `poison_damage_bonus`, `lightning_damage_bonus` — ancient jewelry damage bonuses (S1-era jewelry, pending decision 5)
- `mana_recovery_absolute`, `health_recovery_absolute` — engine regeneration terms (current += mult*max + absolute); no pre-S3 data feeds them
- `is_underwater` — spec section 11 atlans power-up; OpenMU applies underwater state at runtime only
- `is_invisible` — engine-state flag (GM hide); not wired by pre-S3 data
- `nova_stage_damage` — nova backport (0.97/1.0-era skill)

## 3. Named gaps

### chaos_mixes (14)

- `item_upgrade_12_to_15` — S6 mixes #22/#23/#49/#50: item level cap is 11 (approved decision 2)
- `illusion_temple_ticket` — S6 mix #37: Illusion Temple is Season 3
- `potion_of_bless_soul` — S6 mixes #15/#16: castle siege potions (S3+)
- `shield_potions` — S6 mixes #30/#31/#32: SD stat system is S3+ (classic PvP instead)
- `life_stone` — S6 mix #17: castle siege (S3)
- `fenrir_craftings` — S6 mixes #25-#28: Fenrir is S2 but trainable pets are excluded wholesale (decision 5)
- `dark_horse_dark_raven` — S6 mixes #13/#14 (Pet Trainer): trainable pets excluded (decision 5)
- `third_wings` — S6 mixes #38/#39: 3rd wings are Season 3
- `level_380_option` — S6 mix #36: guardian/380 options are post-S3
- `secromicon` — S6 mix #46: Season 6
- `gemstone_refinery_refine_restore` — S6 mixes #33-#35 (Elphis/Osbourne/Jerridon): Jewel of Harmony ecosystem (S4)
- `cherry_blossom_mix` — S6 mix #41: seasonal event (S4+)
- `first_wings_misery_result` — S6 adds Wings of Misery (12,41) to the 1st-wings results; Summoner (S3) — 095d result list used
- `jewel_mixes_lahap` — Lahap jewel packing (10 S6 JewelMix records): pending decision 5, era-questionable packed-item ids

### classes (26)

Dropped stat relationships, by target stat, with reason:

- Master skill tree (S4+): `bonus_damage_with_scepter_cmd_div`, `bonus_defense_rate_with_shield`, `bonus_defense_with_shield`, `master_skill_phys_bonus_dmg`, `one_handed_staff_bonus_base_damage`, `two_handed_staff_bonus_base_damage`, `weapon_mastery_attack_speed`, `scepter_pet_bonus_damage`, `master_level` (master system; pre-S3 total_level == level)
- Trainable pets excluded per spec: `bonus_defense_with_horse`, `damage_receive_horse_decrement`, `is_horse_equipped`, `fenrir_base_dmg` (all 8 classes), `raven_attack_damage_increase`, `raven_attack_rate`, `raven_attack_speed`, `raven_bonus_damage`, `raven_critical_damage_chance`, `raven_level`, `raven_maximum_damage`, `raven_minimum_damage`
- S3+ content: `innovation_def_decrement`, `temp_innovation_defense_decrement` (innovation skill), `shield_item_defense_increase` (water socket)
- Custom server feature: `resets` (base_stat 0 dropped for all 8 classes)
- Resolved: `pet_duration_increase` — intentionally excluded (trainable-pets group; Dark Horse/Raven out of scope). The dark_lord const 1.0 is dropped with it; re-add both if trainable pets ever enter scope.

### constants + exp (16; 5 of these are rules-deferred, see section 4)

- `max_letters`, `letter_send_price` — letters/inbox are host-owned social features; excluded per spec section 16
- `max_password_length` — account/password constants excluded wholesale
- `experience_rate`, `movement_speed_factor` — modeled as stats.json stats, not constants
- `basic_mount_speed` — Uniria/Dinorant mount speed (15.0) rides in item_definitions power-ups
- `cold_speed_factor` — ColdMovementSpeedFactor (0.33) wired only by S6 cold-effect initializers; skills agent owns it if the ice-arrow backport needs it
- `horse_fenrir_speeds` — trainable pets, excluded wholesale
- `master_experience` — master level/exp formula: post-S3
- `maximum_alliance_size` — guild alliances, S6-only value (decision 5 open)
- `level_dependent_damage` — dead data even in OpenMU
- (rules-deferred: `per_kill_exp_formula`, `min_damage_floor`, `dinorant_damage_factor`, `double_wield_factor`, `classic_duel_damage_factor` — section 4)

### drops (12)

- `box_item_drop_tables` — box contents are item-attached drop tables; owned by the items extractor's box_drops by design
- `legacy_quest_item_drop_groups` — quest-bound drop groups ship with quest definitions; quests.json deferred to a follow-up wave
- `blood_castle_event_reward_groups` — in-event reward groups and the saint statue's archangel-weapon drop need an event/minigame schema not in this wave
- `chaos_castle_reward_groups` — need the missing event/minigame schema; s6-era doubt on the event itself
- `s6_golden_army_boxes` — s6 golden invasion monsters 78-83; golden army is S3-era; Golden Dragon (79) is arguably 1.0-era, left to the monsters-wave curation
- `devil_square_5_to_7_ticket_bands` — arenas beyond the 095d baseline (4 arenas)
- `dark_horse_raven_spirit_drops` — trainable pets excluded wholesale
- `symbol_of_kundun_drops` — Kalima maps/items in neither baseline nor backport list
- `land_of_trials_jewel_of_guardian` — castle siege content, excluded
- `barracks_of_balgass_flame_of_condor` — 3rd-wings ingredient, post-S3
- `illusion_temple_ticket_drops` — post-S3
- `golden_soldier_no_drop_group` — observation: 095d source binds no drop group to Golden Soldier (54)

### items (9)

- Devil's Eye/Key +1..+4 monster-level drop tables are global drop groups -> drop_groups.json, not representable as box_drops
- Weapon of Archangel (13/19) is in-event Blood Castle content (saint statue drop); event/minigame schema not in this wave
- S6-only event items NOT backported: Armor of Guardsman (13/29), Illusion Temple items (13/49-51), Imperial Guardian items (14/101-109)
- Box of Luck higher kinds (+1 … +11) named in a 095d source comment but ship no drop data pre-S6; not backported
- 2nd-wings chaos mix (and its use of Loch's Feather 13/14) -> chaos_mixes.json extractor
- Cape of Lord (13/30) backported but its S6 option definitions are NOT — record references only 'luck'
- Dark lord scepters not backported this wave; DL items limited to Cape of Lord + Adamantine ancient pieces
- Summoner class not in baseline: Red Wing pieces (7-11/40) ship with empty classes lists -> unequippable
- Wings of Despair (12/42, summoner) and all 3rd wings/capes: post-S3 or excluded classes, skipped

### maps (4)

- Devil Square mini-game event config (durations, entry level ranges per square 1-4, rewards, ticket 14/19, max 10 players) — no mini-game schema this wave; the spawns, maps and entrance exit gates 58-61 ARE extracted
- Authentic 0.95d warp fees/levels unknown — OpenMU ships the 0.75 warp list verbatim with a 'todo'; adopted as-is, all 14 entries tagged 075
- 1.0-era Blood Castle maps/spawns exist only in the S6 dataset and are outside the approved 14-map scope — no map backports
- Invasion mobs (43/44/53/54) have monster definitions but no static spawn areas — invasions spawn dynamically at runtime

### monsters (4)

- Merchant store contents deferred (merchant_stores.json, follow-up wave); merchant NPCs carry only role/window
- Quests startable at NPCs (Sevina #235) deferred to quests.json (follow-up wave)
- No s6 monster backports this wave: none named in the approved source list; known 1.0-era candidates left out include Blood Castle monsters + Archangel Messenger NPC and higher golden-invasion tiers
- Golden Soldier #54 has no drop group in source — faithful to upstream 095d data, likely upstream omission

### options + sets (6)

- Wing option groups for excluded wing items unextracted: Wings of Curse 12/41 + Wings of Despair 12/42 (summoner), Cape of Fighter 12/49 (rage fighter), Cape of Lord 13/30 options incl. leadership entry, all 3rd-wing options
- S6-only normal options phys+wiz combined (0x07, MG magic swords) and curse attack (0x05, Summoner): excluded, no pre-S3 items reference them
- Excellent curse options: disabled in OpenMU until S14, excluded per spec
- Harmony/guardian/socket option groups: post-S3, excluded per spec
- Fenrir/Dark Horse option types + combination bonuses: S6 only, excluded (Dinorant options extracted from 095d)
- Ancient-set pieces that were post-095d items are backported into item_definitions.json (s6) so piece references resolve; any piece still missing gets named in its set's review flag rather than dropped

### skills + effects (7)

- Blade knight skill combo definition (3000 ms window; step lists) is attached to the character class in source and has no slot in spec sections 7/8 — class concern, not extracted
- earthshake damage_scaling: S6 horse_level*10 term dropped — dark horse excluded pre-S3
- infinite_arrow: S6 zero-value placeholder power-up on attack_damage_increase (reserved for the S4 master skill) dropped
- ice_arrow/penetration: S6 per-skill final-damage relationships (2.0 x skill_multiplier) not extracted — S6 rebalance data
- DL summon (skill 63): encoded as behavior kind 'other'; party-summon behavior is a rules concern
- Evolved-class qualification: 075/095d records keep the literal 095d class masks; second-class inheritance is a class/rules concern
- 095d dataset rewrites buff-effect client numbers to the owning skill number (client-protocol convention); canonical effect numbers kept per spec

### stats (126)

Excluded stat slots, grouped by reason:

- **Master tree (S4+), 35:** `bonus_damage_with_scepter_cmd_div`, `bonus_defense_rate_with_shield`, `bonus_defense_with_horse`, `bonus_defense_with_shield`, `book_bonus_base_damage`, `bow_str_bonus_damage`, `cross_bow_mastery_bonus_damage`, `cross_bow_str_bonus_damage`, `explosion_bonus_dmg`, `glove_weapon_bonus_damage`, `mace_bonus_damage`, `master_experience_rate`, `master_level`, `master_points_per_level_up`, `master_skill_phys_bonus_dmg`, `min_wizardry_and_curse_dmg_bonus`, `one_handed_staff_bonus_base_damage`, `one_handed_sword_bonus_damage`, `pollution_bonus_dmg`, `pollution_move_target_chance`, `raven_bonus_damage`, `requiem_bonus_dmg`, `scepter_mastery_bonus_damage`, `scepter_pet_bonus_damage`, `scepter_str_bonus_damage`, `spear_bonus_damage`, `stick_bonus_base_damage`, `stick_mastery_bonus_damage`, `two_handed_staff_bonus_base_damage`, `two_handed_staff_mastery_bonus_damage`, `two_handed_sword_mastery_bonus_damage`, `two_handed_sword_str_bonus_damage`, `weapon_mastery_attack_speed`, `wizardry_and_curse_base_dmg_bonus`, `skill_level` (master-skill level; skills do not level pre-S3)
- **Summoner class (S3), 31:** `berserker_*` (12), `bleeding_damage_multiplier`, `book_rise`, `curse_attack_damage_increase`, `curse_base_dmg`, `innovation_def_decrement`, `is_asleep`, `is_bleeding`, `is_book_equipped`, `is_stick_equipped`, `maximum_curse_base_dmg`, `minimum_curse_base_dmg`, `summoned_monster_defense_increase`, `summoned_monster_health_increase`, `temp_innovation_defense_decrement`, `stats_defense`, `stats_min_wiz_and_curse_base_dmg`, `stats_max_wiz_and_curse_base_dmg`, `min_berserker_health_decrement`, `final_berserker_health_decrement`, `weakness_phys_dmg_decrement` (also RF killing blow)
- **Shield/SD system (classic PvP instead), 12:** `current_shield`, `maximum_shield`, `maximum_shield_temp`, `shield_after_monster_kill_absolute`, `shield_after_monster_kill_multiplier`, `shield_bypass_chance`, `shield_decrease_rate_increase`, `shield_rate_increase`, `shield_recovery_absolute`, `shield_recovery_everywhere`, `shield_recovery_multiplier`, `shield_item_defense_increase` (water socket, S4; classes agent must drop the common relationship reading it)
- **Trainable pets excluded wholesale, 12:** `damage_receive_horse_decrement`, `fenrir_base_dmg`, `horse_level`, `is_horse_equipped`, `pet_duration_increase`, `raven_attack_damage_increase`, `raven_attack_rate`, `raven_attack_speed`, `raven_critical_damage_chance`, `raven_exc_damage_chance`, `raven_level`, `raven_maximum_damage`, `raven_minimum_damage`
- **Dead data (wired by no dataset), 5:** `ability_after_monster_kill_absolute`, `ability_after_monster_kill_multiplier`, `health_after_monster_kill_absolute`, `mana_after_monster_kill_absolute`, `health_loss_after_hit`
- **Post-S3 / other, remaining:** `ability_usage_reduction` (sockets S4), `bonus_experience_rate` (S6 rings/pets; exp knobs live in game_constants), `final_damage_increase_pvp` (guardian/380 options), `fully_recover_health_after_hit_chance` / `fully_recover_mana_after_hit_chance` / `fully_reflect_damage_after_hit_chance` (3rd wing options), `is_glove_weapon_equipped` / `vitality_skill_multiplier` (rage fighter S5), `is_mace_equipped` / `is_spear_equipped` / `is_two_handed_sword_equipped` (S6 weapon-class flags feeding MST), `is_mu_helper_active` (S9), `is_pet_skeleton_equipped` (S6), `is_vip` (custom), `mana_usage_reduction` / `nearby_party_member_count` (S6 skill settings), `moonstone_pendant_equipped` (kanturu S3), `points_per_reset` / `resets` (custom reset system), `required_agility/energy/leadership/strength/vitality_reduction` (harmony S4), `skill_base_damage_bonus` / `skill_base_multiplier` / `skill_final_damage_bonus` / `skill_final_multiplier` (S6 AreaSkillSettings quartet), `skill_extra_mana_cost` (infinite arrow S2+ term not in curated backports), `maximum_alliance_size` (S6, decision 5 open)

## 4. Rules, not data (deferred to Rust)

### chaos_mixes

- `ticket_devil_square` — handler recipe: 1 Devil's Eye (14,17) + 1 Devil's Key (14,18) of EQUAL item level + 1 Jewel of Chaos -> Devil's Invitation (14,19) at the input level, durability 1; success 80% for level<5 else 70%; zen by level 1-7 = 100k/200k/400k/700k/1.1m/1.6m/2m
- `ticket_blood_castle` — handler recipe: 1 Scroll of Archangel (13,16) + 1 Blood Bone (13,17) of EQUAL item level + 1 Jewel of Chaos -> Invisibility Cloak (13,18) at the input level, durability 1; success 80% flat; zen by level 1-8 = 50k/80k/150k/250k/400k/600k/850k/1.05m
- `chaos_weapon_and_first_wings_options` — result option/luck/skill are formulas of the final success rate: roll i in 0..2, item option level 3-i with chance rate/5 + 4*(i+1) percent; luck with rate/5 + 4; skill with rate/5 + 6
- `second_wings_option` — wing item-option roll (20%/10%/4% for levels 1/2/3) in the second_wings rule; luck/excellent come from result_chances; S6 max-1-excellent cap also lives in the rule
- `downgrade_chaos_weapon` — on-fail semantics: level -> random 0..level-1, 50% skill loss (if not excellent), 50% item option -1 level (removed at 1), durability rescaled
- `fruits_level` — created Fruits level (= fruit stat kind) is weighted-random 0-4 with weights 30/25/20/20/5
- `success_npc_price_divisor` — settings-level divisor REPLACES the additive success path: rate = sum(npc old-buying prices of all inputs) / divisor; per-input divisor ADDS sum(prices)/divisor percent; final zen = flat_zen + zen_per_success_percent * rate
- `max_percent_default` — OpenMU MaximumSuccessPercent 0 = uncapped; engine clamps at 100 -> emitted as max_percent 100

### constants + exp

- `per_kill_exp_formula` — (lvl+25)*lvl/3 with gap scaling, >=65 bonus, x1.25
- `min_damage_floor` — max(1, attackerLevel/10)
- `dinorant_damage_factor` — x1.3 skill-less attack multiplier
- `double_wield_factor` — halve-then-double wield rule
- `classic_duel_damage_factor` — 0.6 duel damage factor

### items

- Fruit (13/15) consume behavior (stat point add/remove) is a rule, not data

### skills

- DL summon (63): party-summon behavior is a rules concern (encoded as behavior kind 'other')

### stats

- `mana_recovery_absolute` / `health_recovery_absolute` — engine regeneration formula terms (current += mult*max + absolute)

## 5. Totals

| Metric | Count |
|---|---:|
| JSON records | **2691** (075: 2136, 095d: 397, s6: 158) |
| Terrain binaries | 14 |
| Review-flagged records | **171** |
| Named gaps | **224** |
| Rules-deferred entries | 17 |

Notable data-quality notes carried from extractors: spec section 15 exp sample (level 3 = 396) looks like a typo vs the formula (440); facts-file level-255 exp sample disagrees with the formula; kantata_plate OpenMU data bug fixed; zen caps overridden to 2,000,000,000 per decision 7; tick_duration_ms 100 is a mu-core decision, not extracted.
