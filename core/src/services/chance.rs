//! The one place a probability unit meets the RNG. Every draw goes through
//! [`crate::rng::uniform_below`]; no `%` reduction of `next_u32` exists
//! anywhere in the crate. Denominators are constants on the unit types
//! ([`ChancePer10000::DENOMINATOR`], [`Percent::DENOMINATOR`],
//! [`Resistance::DENOMINATOR`]) — the unit and its scale are one concern.

use core::num::NonZeroU32;

use rand_core::RngCore;

use crate::components::collections::OneOrMore;
use crate::components::units::{ChancePer10000, Percent, Resistance};
use crate::rng::uniform_below;

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
pub fn pick_one<'a, T: Clone>(list: &'a OneOrMore<T>, rng: &mut impl RngCore) -> &'a T {
    let bound = match NonZeroU32::try_from(list.count()) {
        Ok(bound) => bound,
        Err(_) => NonZeroU32::MAX,
    };
    let target = uniform_below(bound, rng);
    let mut position = 0u32;
    for item in list.iter() {
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
}
