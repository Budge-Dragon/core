//! Integer ratio arithmetic shared by every derived-magnitude service. The one
//! place a widened numerator is floor-divided by a non-zero denominator and
//! narrowed back — defined once here so profile, combat, and experience never
//! duplicate the widen/divide/narrow dance, and never sum per-term truncations
//! where a single pooled divide is correct.

use core::num::NonZeroU32;

/// A non-zero denominator from a known-positive value, built through the
/// saturating-add idiom (never `NonZeroU32::new(..).unwrap()`). A zero argument
/// folds to one — total, though every call site passes a positive constant or a
/// value a guard already proved non-zero.
#[must_use]
pub const fn nonzero(value: u32) -> NonZeroU32 {
    NonZeroU32::MIN.saturating_add(value.saturating_sub(1))
}

/// `value * num / den` as a pooled integer ratio: widen to `u64`, saturating
/// multiply, floor-divide by the non-zero denominator, narrow back. The single
/// scaling primitive — a rate multiplier, an overrate penalty, a jitter percent
/// all route through here.
#[must_use]
pub fn scale_ratio(value: u32, num: u32, den: NonZeroU32) -> u32 {
    floor_div_u64_to_u32(u64::from(value).saturating_mul(u64::from(num)), den)
}

/// Floor-divides a widened numerator by a non-zero denominator and narrows the
/// quotient into `u32`, saturating rather than truncating on overflow. A
/// multi-term derived stat builds its `u64` numerator inline and calls this
/// directly — one pooled divide, never a sum of per-term truncations.
#[must_use]
pub fn floor_div_u64_to_u32(numerator: u64, den: NonZeroU32) -> u32 {
    // The saturating narrow of a proven-integer quotient — the `u32::MAX`
    // fallback is a boundary saturation, not a masked lookup absence.
    u32::try_from(numerator / u64::from(den.get())).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonzero_maps_positive_values_and_folds_zero_to_one() {
        assert_eq!(nonzero(4).get(), 4);
        assert_eq!(nonzero(1).get(), 1);
        assert_eq!(nonzero(100).get(), 100);
        assert_eq!(nonzero(0).get(), 1);
    }

    #[test]
    fn scale_ratio_is_a_pooled_floor_divide() {
        // 6/5 of 10 is 12; 3/10 of 100 is 30.
        assert_eq!(scale_ratio(10, 6, nonzero(5)), 12);
        assert_eq!(scale_ratio(100, 3, nonzero(10)), 30);
        // Floor, not round: 7 * 1 / 2 = 3.
        assert_eq!(scale_ratio(7, 1, nonzero(2)), 3);
    }

    #[test]
    fn scale_ratio_saturates_rather_than_overflowing() {
        // u32::MAX * 2 overflows u32 but is held in u64 before the divide.
        assert_eq!(scale_ratio(u32::MAX, 2, nonzero(2)), u32::MAX);
    }

    #[test]
    fn floor_div_narrows_and_saturates() {
        assert_eq!(floor_div_u64_to_u32(3070, nonzero(100)), 30);
        assert_eq!(floor_div_u64_to_u32(0, nonzero(3)), 0);
        // A quotient beyond u32::MAX saturates.
        assert_eq!(floor_div_u64_to_u32(u64::MAX, nonzero(1)), u32::MAX);
    }
}
