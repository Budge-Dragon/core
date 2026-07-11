//! The one place a probability unit meets the RNG. Every draw goes through
//! [`crate::rng::uniform_below`]; no `%` reduction of `next_u32` exists
//! anywhere in the crate. Denominators are constants on the unit types
//! ([`ChancePer10000::DENOMINATOR`], [`Percent::DENOMINATOR`],
//! [`Resistance::DENOMINATOR`]) — the unit and its scale are one concern.

use core::num::{NonZeroU32, NonZeroUsize};

use rand_core::RngCore;

use crate::components::collections::OneOrMore;
use crate::components::interval::Interval;
use crate::components::spatial::{Facing, TileDelta, TileOffset};
use crate::components::units::{ChancePer10000, Percent, Resistance};
use crate::rng::{uniform_below, uniform_below_usize};

/// The eight cardinal facings a drawn heading resolves to, in a fixed order so
/// a drawn index maps to the same facing bit-for-bit on every target.
const CARDINALS: [Facing; 8] = [
    Facing::POS_X,
    Facing::POS_X_POS_Y,
    Facing::POS_Y,
    Facing::NEG_X_POS_Y,
    Facing::NEG_X,
    Facing::NEG_X_NEG_Y,
    Facing::NEG_Y,
    Facing::POS_X_NEG_Y,
];

/// Draws a cardinal facing uniformly through the RNG seam — the shared heading
/// draw for spawns without an authored facing and for wander drift. Consumes
/// exactly one random word.
#[must_use]
pub fn draw_cardinal(rng: &mut impl RngCore) -> Facing {
    let bound = NonZeroUsize::MIN.saturating_add(CARDINALS.len() - 1);
    let target = uniform_below_usize(bound, rng);
    let mut position = 0usize;
    for facing in CARDINALS {
        if position == target {
            return facing;
        }
        position = position.saturating_add(1);
    }
    Facing::POS_X
}

/// The `uniform_below(3)` bound — a tile delta drawn as {0,1,2} → {−1,0,+1}.
const THREE: NonZeroU32 = NonZeroU32::MIN.saturating_add(2);

/// Draws the ±1 jiggle offset — dx then dy, each uniform in {−1, 0, +1} — for a
/// nine-outcome shove (the eight neighbours plus stay). Advances the RNG by
/// exactly two `uniform_below(3)` draws in the order dx, dy, always — even for
/// the stay(0,0) outcome — so replay is bit-identical regardless of which of the
/// nine lands (STEP-PATH / lightning-jiggle draw contract).
#[must_use]
pub fn draw_jiggle_offset(rng: &mut impl RngCore) -> TileOffset {
    let dx = draw_delta(rng);
    let dy = draw_delta(rng);
    TileOffset::new(dx, dy)
}

/// One axis delta drawn uniformly from {−1, 0, +1} — one `uniform_below(3)` draw.
fn draw_delta(rng: &mut impl RngCore) -> TileDelta {
    // The `_` covers the integer `2` (uniform_below(3) yields only {0,1,2}) —
    // an integer catch-all, not a domain-enum wildcard.
    match uniform_below(THREE, rng) {
        0 => TileDelta::Neg,
        1 => TileDelta::Zero,
        _ => TileDelta::Pos,
    }
}

/// Rolls a per-10,000 chance: `true` iff `uniform_below(10_000) < numerator`.
#[must_use]
pub fn roll_per_10000(chance: ChancePer10000, rng: &mut impl RngCore) -> bool {
    let bound = NonZeroU32::MIN.saturating_add(u32::from(ChancePer10000::DENOMINATOR) - 1);
    uniform_below(bound, rng) < u32::from(chance.numerator())
}

/// Rolls whole percent points: `true` iff `uniform_below(100) < points`.
#[must_use]
pub fn roll_percent(percent: Percent, rng: &mut impl RngCore) -> bool {
    let bound = NonZeroU32::MIN.saturating_add(u32::from(Percent::DENOMINATOR) - 1);
    uniform_below(bound, rng) < u32::from(percent.points())
}

/// Rolls a resistance byte: `true` iff `uniform_below(255) < byte` — the byte
/// read as `n/255`. `255` resists always; `0` never.
#[must_use]
pub fn roll_resistance(resistance: Resistance, rng: &mut impl RngCore) -> bool {
    let bound = NonZeroU32::MIN.saturating_add(u32::from(Resistance::DENOMINATOR) - 1);
    uniform_below(bound, rng) < u32::from(resistance.0)
}

/// Draws a value uniformly from an inclusive `[min, max]` span, reaching both
/// endpoints. The width is `max - min + 1` (at least one, since `min <= max` is
/// proven by the [`Interval`]), so a single-point span is total and consumes one
/// word. The drawn offset is proven below the width, hence at most `max - min`,
/// so the sum with `min` never exceeds `max` and narrows losslessly.
#[must_use]
pub fn uniform_in_inclusive(span: Interval<u16>, rng: &mut impl RngCore) -> u16 {
    let low = u32::from(span.min());
    let width = u32::from(span.max()) - low + 1;
    let bound = NonZeroU32::MIN.saturating_add(width - 1);
    let offset = low_u16(uniform_below(bound, rng));
    span.min().saturating_add(offset)
}

/// Rolls whether an elemental effect applies to a target: `false` when immune
/// (`resistance == 255`, short-circuiting with no draw), otherwise `true` with
/// probability `1/(resistance + 1)` — `uniform_below(resistance + 1) == 0`. The
/// faithful application curve (distinct from [`roll_resistance`], which is the
/// `n/255` resist-chance roll); a byte of `0` always applies, mid values thin it
/// hyperbolically.
#[must_use]
pub fn roll_apply_elemental(resistance: Resistance, rng: &mut impl RngCore) -> bool {
    resistance.0 < 255
        && uniform_below(NonZeroU32::MIN.saturating_add(u32::from(resistance.0)), rng) == 0
}

/// Keeps the low two bytes of a `u32`, dropping the high two. Callers pass a
/// value whose high bytes are proven zero, so this is lossless, cast-free, and
/// total — mirroring the byte-decomposition narrows in [`crate::rng`].
fn low_u16(value: u32) -> u16 {
    let [b0, b1, _, _] = value.to_le_bytes();
    u16::from_le_bytes([b0, b1])
}

/// A non-empty cumulative-weight table whose total is DERIVED from its weights
/// at construction — never an assumed 100 — and proven nonzero, so picking is
/// total thereafter. Zero-weight entries are unrepresentable: a weight is
/// `NonZeroU32`. The final entry is held apart so a pick over `0..total` always
/// resolves to a real bucket without a wildcard fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedTable<T> {
    leading: Vec<(NonZeroU32, T)>,
    last: (NonZeroU32, T),
    total: NonZeroU32,
}

impl<T> WeightedTable<T> {
    /// Builds the table, deriving `total` by summation. An empty list or a sum
    /// beyond `u32::MAX` is rejected at construction — the one place this can
    /// fail; every pick afterwards is total.
    ///
    /// # Errors
    /// Returns [`WeightError::Empty`] for no entries, or
    /// [`WeightError::TotalOverflow`] when the weights sum beyond `u32::MAX`.
    pub fn new(mut entries: Vec<(NonZeroU32, T)>) -> Result<Self, WeightError> {
        let last = entries.pop().ok_or(WeightError::Empty)?;
        let mut total = last.0;
        for (weight, _) in &entries {
            total = total
                .checked_add(weight.get())
                .ok_or(WeightError::TotalOverflow)?;
        }
        Ok(Self {
            leading: entries,
            last,
            total,
        })
    }

    /// The derived weight total — the roll bound of [`weighted_pick`].
    #[must_use]
    pub fn total(&self) -> NonZeroU32 {
        self.total
    }
}

/// Rejection of a weight table at construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightError {
    /// No entries.
    Empty,
    /// Weights sum beyond `u32::MAX`.
    TotalOverflow,
}

impl core::fmt::Display for WeightError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(f, "a weight table must have at least one entry"),
            Self::TotalOverflow => write!(f, "weight table total exceeds u32::MAX"),
        }
    }
}

impl core::error::Error for WeightError {}

/// Picks one entry by weight: rolls `uniform_below(table.total())` and walks
/// the cumulative weights. The total is derived, so editing a weight list can
/// never leave a roll with no bucket — a roll at or beyond the leading entries'
/// cumulative weight lands in the held-apart final bucket, the correct answer,
/// not a fallback.
#[must_use]
pub fn weighted_pick<'a, T>(table: &'a WeightedTable<T>, rng: &mut impl RngCore) -> &'a T {
    let roll = uniform_below(table.total, rng);
    let mut cumulative = 0u32;
    for (weight, item) in &table.leading {
        cumulative = cumulative.saturating_add(weight.get());
        if roll < cumulative {
            return item;
        }
    }
    &table.last.1
}

/// Picks one element uniformly from a non-empty list. Total over the
/// non-emptiness invariant — the first element is the answer when the roll
/// lands on position zero and the terminal case alike, never a fabricated
/// default.
#[must_use]
pub fn pick_one<'a, T>(list: &'a OneOrMore<T>, rng: &mut impl RngCore) -> &'a T {
    let target = uniform_below_usize(list.count(), rng);
    let mut position = 0usize;
    for item in list {
        if position == target {
            return item;
        }
        position = position.saturating_add(1);
    }
    list.first()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Deterministic `SplitMix64` for replayable tests; cast-free extraction of
    /// the low 32 bits keeps clippy's cast lints quiet in test code too.
    struct TestRng {
        state: u64,
    }

    impl TestRng {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }
    }

    impl RngCore for TestRng {
        fn next_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn next_u32(&mut self) -> u32 {
            let [b0, b1, b2, b3, _, _, _, _] = self.next_u64().to_le_bytes();
            u32::from_le_bytes([b0, b1, b2, b3])
        }

        fn fill_bytes(&mut self, dst: &mut [u8]) {
            for chunk in dst.chunks_mut(8) {
                let bytes = self.next_u64().to_le_bytes();
                for (slot, byte) in chunk.iter_mut().zip(bytes.iter()) {
                    *slot = *byte;
                }
            }
        }
    }

    fn nz(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    #[test]
    fn uniform_below_stays_in_range() {
        let mut rng = TestRng::new(1);
        for _ in 0..10_000 {
            assert!(uniform_below(nz(7), &mut rng) < 7);
        }
        for _ in 0..64 {
            assert_eq!(uniform_below(nz(1), &mut rng), 0);
        }
    }

    #[test]
    fn certain_and_impossible_rolls() {
        let mut rng = TestRng::new(2);
        for _ in 0..1000 {
            assert!(roll_per_10000(ChancePer10000::ALWAYS, &mut rng));
            assert!(!roll_per_10000(ChancePer10000::NEVER, &mut rng));
            assert!(!roll_resistance(Resistance(0), &mut rng));
            assert!(roll_resistance(Resistance(255), &mut rng));
            assert!(roll_percent(Percent::new(100).unwrap(), &mut rng));
            assert!(!roll_percent(Percent::new(0).unwrap(), &mut rng));
        }
    }

    #[test]
    fn weighted_pick_covers_every_bucket_and_respects_weights() {
        let table = WeightedTable::new(vec![(nz(1), 'a'), (nz(3), 'b')]).unwrap();
        assert_eq!(table.total().get(), 4);
        let mut rng = TestRng::new(3);
        let mut a = 0u32;
        let mut b = 0u32;
        for _ in 0..8000 {
            match weighted_pick(&table, &mut rng) {
                'a' => a += 1,
                'b' => b += 1,
                other => panic!("unexpected bucket {other}"),
            }
        }
        assert!(a > 0 && b > a, "b (weight 3) should dominate a (weight 1)");
    }

    #[test]
    fn weighted_pick_single_entry_is_total() {
        let table = WeightedTable::new(vec![(nz(5), 42u8)]).unwrap();
        let mut rng = TestRng::new(4);
        for _ in 0..100 {
            assert_eq!(*weighted_pick(&table, &mut rng), 42);
        }
    }

    #[test]
    fn weighted_table_rejects_empty() {
        assert_eq!(
            WeightedTable::<u8>::new(Vec::new()).unwrap_err(),
            WeightError::Empty
        );
    }

    #[test]
    fn pick_one_returns_an_element() {
        let list = OneOrMore::new(vec![10u8, 20, 30]).unwrap();
        let mut rng = TestRng::new(5);
        for _ in 0..100 {
            let picked = *pick_one(&list, &mut rng);
            assert!(picked == 10 || picked == 20 || picked == 30);
        }
    }

    #[test]
    fn uniform_in_inclusive_single_point_is_total_and_draws_one_word() {
        let point = Interval::new(7u16, 7u16).unwrap();
        let mut rng = TestRng::new(11);
        for _ in 0..64 {
            assert_eq!(uniform_in_inclusive(point, &mut rng), 7);
        }
    }

    #[test]
    fn uniform_in_inclusive_reaches_both_endpoints_and_stays_in_range() {
        let span = Interval::new(3u16, 9u16).unwrap();
        let mut rng = TestRng::new(12);
        let mut saw_min = false;
        let mut saw_max = false;
        for _ in 0..10_000 {
            let value = uniform_in_inclusive(span, &mut rng);
            assert!((3..=9).contains(&value));
            saw_min |= value == 3;
            saw_max |= value == 9;
        }
        assert!(saw_min && saw_max, "both endpoints must be reachable");
    }

    #[test]
    fn roll_apply_elemental_extremes() {
        let mut rng = TestRng::new(13);
        for _ in 0..1000 {
            // resistance 0 always applies; 255 is immune (and consumes no word).
            assert!(roll_apply_elemental(Resistance(0), &mut rng));
            assert!(!roll_apply_elemental(Resistance(255), &mut rng));
        }
    }

    #[test]
    fn roll_apply_elemental_immune_consumes_no_word() {
        let mut rng = TestRng::new(99);
        let mut probe = TestRng::new(99);
        assert!(!roll_apply_elemental(Resistance(255), &mut rng));
        assert_eq!(rng.next_u64(), probe.next_u64());
    }

    #[test]
    fn roll_apply_elemental_mid_resistance_is_bernoulli() {
        // resistance 3 applies with probability 1/4 — an integer count check.
        let mut rng = TestRng::new(14);
        let mut applied = 0u32;
        for _ in 0..8000 {
            if roll_apply_elemental(Resistance(3), &mut rng) {
                applied += 1;
            }
        }
        // Around 2000 of 8000; a wide band proves the curve without flaking.
        assert!((1500..2500).contains(&applied), "applied {applied}");
    }

    #[test]
    fn draw_jiggle_offset_reaches_all_nine_outcomes() {
        let deltas = [TileDelta::Neg, TileDelta::Zero, TileDelta::Pos];
        let mut seen = [[false; 3]; 3];
        let mut rng = TestRng::new(21);
        for _ in 0..2000 {
            let offset = draw_jiggle_offset(&mut rng);
            for (dx_index, &dx) in deltas.iter().enumerate() {
                for (dy_index, &dy) in deltas.iter().enumerate() {
                    if offset == TileOffset::new(dx, dy) {
                        seen[dx_index][dy_index] = true;
                    }
                }
            }
        }
        assert!(
            seen.iter().all(|row| row.iter().all(|&hit| hit)),
            "all nine dx,dy outcomes incl. stay and diagonals must be reachable"
        );
    }

    #[test]
    fn draw_jiggle_offset_consumes_exactly_two_words() {
        for seed in 0u64..16 {
            let mut rng = TestRng::new(seed);
            let _ = draw_jiggle_offset(&mut rng);
            let mut probe = TestRng::new(seed);
            probe.next_u32();
            probe.next_u32();
            assert_eq!(rng.next_u64(), probe.next_u64(), "seed {seed}");
        }
    }

    #[test]
    fn pick_one_covers_every_index() {
        // Over many draws every position of a small list is reached, so the
        // uniform index draw is not stuck on one element.
        let list = OneOrMore::new(vec![10u8, 20, 30, 40]).unwrap();
        let mut rng = TestRng::new(7);
        let mut seen = [false; 4];
        for _ in 0..1000 {
            match *pick_one(&list, &mut rng) {
                10 => seen[0] = true,
                20 => seen[1] = true,
                30 => seen[2] = true,
                40 => seen[3] = true,
                other => panic!("unexpected element {other}"),
            }
        }
        assert!(seen.iter().all(|&hit| hit), "not every index was reached");
    }

    proptest! {
        #[test]
        fn weighted_table_total_is_the_exact_weight_sum(
            weights in prop::collection::vec(1u32..=1_000_000, 1..12),
        ) {
            let sum: u64 = weights.iter().map(|&weight| u64::from(weight)).sum();
            let entries: Vec<(NonZeroU32, usize)> = weights
                .iter()
                .copied()
                .enumerate()
                .map(|(index, weight)| (nz(weight), index))
                .collect();
            let table = WeightedTable::new(entries).unwrap();
            prop_assert_eq!(u64::from(table.total().get()), sum);
        }

        #[test]
        fn weighted_pick_always_lands_in_a_real_bucket(
            weights in prop::collection::vec(1u32..=50, 1..8),
            seed in any::<u64>(),
        ) {
            let bucket_count = weights.len();
            let entries: Vec<(NonZeroU32, usize)> = weights
                .iter()
                .copied()
                .enumerate()
                .map(|(index, weight)| (nz(weight), index))
                .collect();
            let table = WeightedTable::new(entries).unwrap();
            let mut rng = TestRng::new(seed);
            for _ in 0..256 {
                prop_assert!(*weighted_pick(&table, &mut rng) < bucket_count);
            }
        }

        #[test]
        fn heavier_bucket_is_picked_more_over_a_large_sample(
            light in 1u32..=100,
            multiple in 2u32..=5,
            seed in any::<u64>(),
        ) {
            // The heavy bucket weighs at least twice the light one, so over a
            // large sample it must win outright — monotonicity, not exact ratio.
            let heavy = light.saturating_mul(multiple);
            let table = WeightedTable::new(vec![(nz(light), 0u8), (nz(heavy), 1u8)]).unwrap();
            let mut rng = TestRng::new(seed);
            let mut light_count = 0u32;
            let mut heavy_count = 0u32;
            for _ in 0..4000 {
                match *weighted_pick(&table, &mut rng) {
                    0 => light_count += 1,
                    1 => heavy_count += 1,
                    other => prop_assert!(false, "unexpected bucket {}", other),
                }
            }
            prop_assert!(
                heavy_count > light_count,
                "heavy {} did not exceed light {}",
                heavy_count,
                light_count
            );
        }

        #[test]
        fn pick_one_result_is_always_a_list_element(
            items in prop::collection::vec(any::<u16>(), 1..16),
            seed in any::<u64>(),
        ) {
            let list = OneOrMore::new(items.clone()).unwrap();
            let mut rng = TestRng::new(seed);
            for _ in 0..256 {
                prop_assert!(items.contains(pick_one(&list, &mut rng)));
            }
        }
    }
}
