//! The ground-entity lifecycle: stamp at birth, reap at death. The stamping
//! seam ([`stamp_item`], [`stamp_zen`]) computes a drop's appearance tick,
//! despawn tick, and ownership window from its origin and timing, so a host
//! seats the returned clocks instead of re-deriving the arithmetic. The reaper
//! ([`reap_ground`]) is the pure tick-driven transition that removes every
//! drop whose despawn has been reached and reports each removal as a
//! [`DespawnEvent`]. Every clock anchors at the drop's appearance, and both
//! seams are deterministic — no RNG, no clock reads; `now` is an input.

use crate::components::drop_claim::DropClaim;
use crate::components::units::{DurationMs, Tick, TickDuration};
use crate::entities::world_item::WorldItem;
use crate::entities::world_zen::WorldZen;
use crate::events::ground::DespawnEvent;

/// Where a ground drop came from — decides its appearance beat and whether it
/// carries an ownership window. Host-supplied classification (transient, no
/// serde).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropOrigin {
    /// A monster kill: the drop materialises one second after death, claimed
    /// to the killer's kill-snapshot for the ownership window.
    MonsterKill,
    /// A player-dropped item: appears instantly, claimed to the dropper's
    /// snapshot (strangers still wait the window out).
    PlayerDrop,
    /// A GM-spawned or item-box drop: appears instantly, never claimed. The
    /// stamping home for `Unclaimed` drops, so despawn arithmetic stays in
    /// this one seam.
    Ownerless,
}

/// The lifecycle clocks a monster/player/GM drop earns — the appearance tick
/// the host seats the item at, the despawn tick it stores, and the ownership
/// window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemStamp {
    /// The tick the drop becomes visible/pickable (`drop_tick + beat`).
    pub appearance: Tick,
    /// The tick it despawns (`appearance + item_drop_duration`).
    pub despawn: Tick,
    /// Its ownership window (`Claimed` off the appearance, or `Unclaimed`).
    pub claim: DropClaim,
}

/// The lifecycle clocks a zen pile earns — no claim, zen is never owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZenStamp {
    /// The tick the pile becomes visible/pickable.
    pub appearance: Tick,
    /// The tick it despawns (the same duration clock as items).
    pub despawn: Tick,
}

/// The corpse-to-loot beat: monster kills stage a fixed 1 second, authentic.
const MONSTER_DROP_BEAT: DurationMs = DurationMs(1000);
/// The ownership window: 10 seconds, authentic hard-coded.
const DROP_OWNER_WINDOW: DurationMs = DurationMs(10_000);

/// Stamps an item drop's lifecycle from its origin and timing. Pure, no RNG.
/// The atlas `duration` is the ground-item despawn duration; `tick` converts
/// every ms delay to whole ticks (the host cadence). The clocks anchor at
/// appearance.
#[must_use]
pub fn stamp_item(
    origin: DropOrigin,
    drop_tick: Tick,
    duration: DurationMs,
    tick: TickDuration,
) -> ItemStamp {
    let (appearance, despawn) = lifecycle(origin, drop_tick, duration, tick);
    ItemStamp {
        appearance,
        despawn,
        claim: claim_of(origin, appearance, tick),
    }
}

/// Stamps a zen pile's lifecycle — appearance + despawn, no claim. Pure, no
/// RNG.
#[must_use]
pub fn stamp_zen(
    origin: DropOrigin,
    drop_tick: Tick,
    duration: DurationMs,
    tick: TickDuration,
) -> ZenStamp {
    let (appearance, despawn) = lifecycle(origin, drop_tick, duration, tick);
    ZenStamp {
        appearance,
        despawn,
    }
}

/// The shared appearance + despawn computation: appearance is the drop tick
/// plus the origin's beat; despawn is the atlas duration off appearance.
fn lifecycle(
    origin: DropOrigin,
    drop_tick: Tick,
    duration: DurationMs,
    tick: TickDuration,
) -> (Tick, Tick) {
    let appearance = drop_tick + beat(origin).in_ticks(tick);
    let despawn = appearance + duration.in_ticks(tick);
    (appearance, despawn)
}

/// The appearance beat of an origin: monster kills stage one second,
/// everything else is instant. Total over [`DropOrigin`].
fn beat(origin: DropOrigin) -> DurationMs {
    match origin {
        DropOrigin::MonsterKill => MONSTER_DROP_BEAT,
        DropOrigin::PlayerDrop | DropOrigin::Ownerless => DurationMs(0),
    }
}

/// The claim an origin confers, off the appearance tick: kill/player drops
/// open the ownership window; ownerless drops are free immediately. Total over
/// [`DropOrigin`].
fn claim_of(origin: DropOrigin, appearance: Tick, tick: TickDuration) -> DropClaim {
    match origin {
        DropOrigin::MonsterKill | DropOrigin::PlayerDrop => DropClaim::Claimed {
            until: appearance + DROP_OWNER_WINDOW.in_ticks(tick),
        },
        DropOrigin::Ownerless => DropClaim::Unclaimed,
    }
}

/// Reaps a ground set at `now`: keeps every drop whose despawn has not been
/// reached, removes and reports each one that has. A pure `(items, zen, now)
/// -> (survivors, survivors, events)` transition — items and zen share the
/// same duration clock, one event per removed drop, survivors keep their
/// input order, draws no RNG.
#[must_use]
pub fn reap_ground(
    items: Vec<WorldItem>,
    zen: Vec<WorldZen>,
    now: Tick,
) -> (Vec<WorldItem>, Vec<WorldZen>, Vec<DespawnEvent>) {
    let mut events = Vec::new();
    let mut surviving_items = Vec::with_capacity(items.len());
    for item in items {
        if item.despawn.reached(now) {
            events.push(DespawnEvent::ItemDespawned {
                position: item.position,
                map: item.map,
                item: item.instance.item,
            });
        } else {
            surviving_items.push(item);
        }
    }
    let mut surviving_zen = Vec::with_capacity(zen.len());
    for pile in zen {
        if pile.despawn.reached(now) {
            events.push(DespawnEvent::ZenDespawned {
                position: pile.position,
                map: pile.map,
                amount: pile.amount,
            });
        } else {
            surviving_zen.push(pile);
        }
    }
    (surviving_items, surviving_zen, events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::item_instance::{
        CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
    };
    use crate::components::item_ref::ItemRef;
    use crate::components::spatial::WorldPos;
    use crate::components::units::{ItemLevel, MapNumber, Zen};

    const ATLAS_DURATION: DurationMs = DurationMs(60_000);

    fn per_50ms() -> TickDuration {
        TickDuration::new(50).unwrap()
    }

    fn ground_item(despawn: Tick, position: WorldPos) -> WorldItem {
        WorldItem {
            instance: ItemInstance {
                item: ItemRef {
                    group: 0,
                    number: 3,
                },
                level: ItemLevel::ZERO,
                roll: RarityRoll::Normal,
                normal_option: None,
                luck: LuckRoll::Plain,
                skill: SkillRoll::NoSkill,
                durability: Durability::full(30),
                augment: CraftedAugment::None,
            },
            position,
            map: MapNumber(0),
            despawn,
            claim: DropClaim::Unclaimed,
        }
    }

    fn ground_zen(despawn: Tick, position: WorldPos) -> WorldZen {
        WorldZen {
            amount: Zen(40_000),
            position,
            map: MapNumber(0),
            despawn,
        }
    }

    #[test]
    fn a_monster_drop_appears_one_second_after_the_kill() {
        let stamp = stamp_item(
            DropOrigin::MonsterKill,
            Tick(100),
            ATLAS_DURATION,
            per_50ms(),
        );
        assert_eq!(stamp.appearance, Tick(120));
    }

    #[test]
    fn a_player_drop_appears_the_instant_it_is_dropped() {
        let stamp = stamp_item(
            DropOrigin::PlayerDrop,
            Tick(100),
            ATLAS_DURATION,
            per_50ms(),
        );
        assert_eq!(stamp.appearance, Tick(100));
    }

    #[test]
    fn the_despawn_and_window_clocks_anchor_at_appearance_not_the_kill() {
        let stamp = stamp_item(
            DropOrigin::MonsterKill,
            Tick(100),
            ATLAS_DURATION,
            per_50ms(),
        );
        assert_eq!(stamp.despawn, Tick(120 + 1200));
        assert_eq!(
            stamp.claim,
            DropClaim::Claimed {
                until: Tick(120 + 200)
            }
        );
    }

    #[test]
    fn a_player_drop_anchors_both_clocks_at_the_instant_with_no_beat() {
        let stamp = stamp_item(
            DropOrigin::PlayerDrop,
            Tick(100),
            ATLAS_DURATION,
            per_50ms(),
        );
        assert_eq!(stamp.despawn, Tick(100 + 1200));
        assert_eq!(
            stamp.claim,
            DropClaim::Claimed {
                until: Tick(100 + 200)
            }
        );
    }

    #[test]
    fn an_ownerless_drop_is_stamped_unclaimed_with_the_same_clocks() {
        let stamp = stamp_item(DropOrigin::Ownerless, Tick(100), ATLAS_DURATION, per_50ms());
        assert_eq!(stamp.appearance, Tick(100));
        assert_eq!(stamp.despawn, Tick(100 + 1200));
        assert_eq!(stamp.claim, DropClaim::Unclaimed);
    }

    #[test]
    fn zen_is_stamped_with_the_same_clocks_and_no_claim() {
        let item = stamp_item(
            DropOrigin::MonsterKill,
            Tick(100),
            ATLAS_DURATION,
            per_50ms(),
        );
        let zen = stamp_zen(
            DropOrigin::MonsterKill,
            Tick(100),
            ATLAS_DURATION,
            per_50ms(),
        );
        assert_eq!(zen.appearance, item.appearance);
        assert_eq!(zen.despawn, item.despawn);
    }

    #[test]
    fn stamping_converts_ms_to_ticks_rounding_up() {
        let per = TickDuration::new(400).unwrap();
        let stamp = stamp_item(DropOrigin::MonsterKill, Tick(100), DurationMs(60_100), per);
        assert_eq!(stamp.appearance, Tick(103));
        assert_eq!(stamp.despawn, Tick(103 + 151));
        assert_eq!(
            stamp.claim,
            DropClaim::Claimed {
                until: Tick(103 + 25)
            }
        );
    }

    #[test]
    fn stamping_is_deterministic_and_draws_no_rng() {
        let a = stamp_item(DropOrigin::MonsterKill, Tick(7), ATLAS_DURATION, per_50ms());
        let b = stamp_item(DropOrigin::MonsterKill, Tick(7), ATLAS_DURATION, per_50ms());
        assert_eq!(a, b);
        let c = stamp_zen(DropOrigin::PlayerDrop, Tick(7), ATLAS_DURATION, per_50ms());
        let d = stamp_zen(DropOrigin::PlayerDrop, Tick(7), ATLAS_DURATION, per_50ms());
        assert_eq!(c, d);
    }

    #[test]
    fn a_drop_survives_one_tick_before_its_despawn_and_flips_exactly_at_it() {
        let position = WorldPos::clamped(163_840, 229_376);
        let item = ground_item(Tick(200), position);

        let (survivors, _, events) = reap_ground(vec![item.clone()], Vec::new(), Tick(199));
        assert_eq!(survivors, vec![item.clone()]);
        assert!(events.is_empty());

        let (survivors, _, events) = reap_ground(vec![item], Vec::new(), Tick(200));
        assert!(survivors.is_empty());
        assert_eq!(
            events,
            vec![DespawnEvent::ItemDespawned {
                position,
                map: MapNumber(0),
                item: ItemRef {
                    group: 0,
                    number: 3,
                },
            }]
        );
    }

    #[test]
    fn a_reached_item_is_removed_while_a_later_zen_pile_survives() {
        let position = WorldPos::clamped(163_840, 229_376);
        let item = ground_item(Tick(200), position);
        let pile = ground_zen(Tick(300), position);

        let (items, zen, events) = reap_ground(vec![item], vec![pile.clone()], Tick(250));
        assert!(items.is_empty());
        assert_eq!(zen, vec![pile]);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events.first(),
            Some(DespawnEvent::ItemDespawned { .. })
        ));
    }

    #[test]
    fn items_and_zen_reap_on_the_same_clock_each_with_its_own_event() {
        let position = WorldPos::clamped(163_840, 229_376);
        let item = ground_item(Tick(200), position);
        let pile = ground_zen(Tick(200), position);

        let (items, zen, events) = reap_ground(vec![item], vec![pile], Tick(200));
        assert!(items.is_empty());
        assert!(zen.is_empty());
        assert_eq!(
            events,
            vec![
                DespawnEvent::ItemDespawned {
                    position,
                    map: MapNumber(0),
                    item: ItemRef {
                        group: 0,
                        number: 3,
                    },
                },
                DespawnEvent::ZenDespawned {
                    position,
                    map: MapNumber(0),
                    amount: Zen(40_000),
                },
            ]
        );
    }

    #[test]
    fn survivors_keep_their_input_order() {
        let first = ground_item(Tick(500), WorldPos::clamped(163_840, 229_376));
        let second = ground_item(Tick(400), WorldPos::clamped(229_376, 163_840));
        let early_pile = ground_zen(Tick(400), WorldPos::clamped(163_840, 163_840));
        let late_pile = ground_zen(Tick(500), WorldPos::clamped(229_376, 229_376));

        let (items, zen, events) = reap_ground(
            vec![first.clone(), second.clone()],
            vec![early_pile.clone(), late_pile.clone()],
            Tick(100),
        );
        assert_eq!(items, vec![first, second]);
        assert_eq!(zen, vec![early_pile, late_pile]);
        assert!(events.is_empty());
    }

    #[test]
    fn reaping_an_empty_ground_set_returns_empty_survivors_and_no_events() {
        let (items, zen, events) = reap_ground(Vec::new(), Vec::new(), Tick(0));
        assert!(items.is_empty());
        assert!(zen.is_empty());
        assert!(events.is_empty());
    }

    #[test]
    fn the_reaper_is_deterministic_and_draws_no_rng() {
        let position = WorldPos::clamped(163_840, 229_376);
        let items = vec![
            ground_item(Tick(200), position),
            ground_item(Tick(400), position),
        ];
        let zen = vec![ground_zen(Tick(200), position)];
        let a = reap_ground(items.clone(), zen.clone(), Tick(300));
        let b = reap_ground(items, zen, Tick(300));
        assert_eq!(a, b);
    }
}
