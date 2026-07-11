//! A ground item's kill-locked ownership window: claimed to the kill-snapshot
//! party for a fixed span after it appears, or ownerless from the start. A
//! small serde value the world item composes — data only, plus the one window
//! predicate [`DropClaim::admits`], a total comparison like
//! [`Tick::reached`], not a service transition. The owner *identity* is host
//! state: the host resolves the picker against the persisted kill-snapshot
//! into a [`PickerStanding`] at the pickup port; core holds only the window.

use serde::{Deserialize, Serialize};

use crate::components::units::Tick;

/// A ground item's ownership window, kind-tagged. A monster/player drop is
/// `Claimed` to the kill-snapshot party until `until`; a GM-spawned or
/// item-box drop is `Unclaimed` — free-for-all the instant it lands. Zen never
/// carries a claim. The owner identity is host state resolved into
/// [`PickerStanding`] at the pickup port; core holds only the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DropClaim {
    /// Owner-locked to the kill-snapshot until `until`; after it, free-for-all.
    Claimed {
        /// The tick the ownership window ends.
        until: Tick,
    },
    /// Ownerless — free-for-all immediately, no window.
    Unclaimed,
}

/// The picker's relation to a claimed drop's kill-snapshot — a host-supplied
/// fact, resolved against the persisted owner set (authoritative server state,
/// never a client claim). A bare relation: the host never supplies a window
/// verdict, only who-relates-how. Transient pickup input (not persisted), so
/// no serde — the [`crate::services::inventory::Wearer`] idiom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerStanding {
    /// The picker is in the drop's kill-snapshot party.
    Owner,
    /// The picker is not in the snapshot.
    Stranger,
}

impl DropClaim {
    /// Whether a picker of `standing` may take this drop at `now`. The window
    /// rule, owned by core: an owner picks a `Claimed` drop any time; a
    /// stranger only once the window has elapsed (`now >= until`); an
    /// `Unclaimed` drop is free to anyone. Total over both enums — a new claim
    /// or standing breaks the build.
    #[must_use]
    pub fn admits(self, standing: PickerStanding, now: Tick) -> bool {
        match self {
            DropClaim::Unclaimed => true,
            DropClaim::Claimed { until } => match standing {
                PickerStanding::Owner => true,
                PickerStanding::Stranger => until.reached(now),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claimed_round_trips_carrying_until() {
        let claimed = DropClaim::Claimed { until: Tick(720) };
        let json = serde_json::to_string(&claimed).unwrap();
        assert_eq!(json, r#"{"kind":"claimed","until":720}"#);
        assert_eq!(serde_json::from_str::<DropClaim>(&json).unwrap(), claimed);
    }

    #[test]
    fn unclaimed_round_trips_in_its_kind_tagged_wire_form() {
        let json = serde_json::to_string(&DropClaim::Unclaimed).unwrap();
        assert_eq!(json, r#"{"kind":"unclaimed"}"#);
        assert_eq!(
            serde_json::from_str::<DropClaim>(&json).unwrap(),
            DropClaim::Unclaimed
        );
    }

    #[test]
    fn an_owner_picks_a_claimed_drop_at_any_tick() {
        let claimed = DropClaim::Claimed { until: Tick(200) };
        assert!(claimed.admits(PickerStanding::Owner, Tick(0)));
        assert!(claimed.admits(PickerStanding::Owner, Tick(199)));
        assert!(claimed.admits(PickerStanding::Owner, Tick(999)));
    }

    #[test]
    fn a_stranger_is_refused_inside_the_window_and_admitted_from_its_end() {
        let claimed = DropClaim::Claimed { until: Tick(200) };
        assert!(!claimed.admits(PickerStanding::Stranger, Tick(0)));
        assert!(!claimed.admits(PickerStanding::Stranger, Tick(199)));
        assert!(claimed.admits(PickerStanding::Stranger, Tick(200)));
        assert!(claimed.admits(PickerStanding::Stranger, Tick(201)));
    }

    #[test]
    fn an_unclaimed_drop_admits_anyone_at_any_tick() {
        assert!(DropClaim::Unclaimed.admits(PickerStanding::Owner, Tick(0)));
        assert!(DropClaim::Unclaimed.admits(PickerStanding::Stranger, Tick(0)));
        assert!(DropClaim::Unclaimed.admits(PickerStanding::Stranger, Tick(u64::MAX)));
    }
}
