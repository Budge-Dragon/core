//! Account-progression outcomes: the per-class unlock a level crossing earns,
//! and the authoritative verdict the creation gate returns. Data only — the
//! host delivers these; core computes and never echoes a client claim.

use serde::{Deserialize, Serialize};

use crate::components::class::CharacterClass;
use crate::components::units::Level;

/// One class an account newly earned the right to create — returned as a `Vec`,
/// one per newly-earned class, in roster order. A flat record: the unlock
/// outcome has exactly one shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassUnlocked {
    /// The class now creatable for the account.
    pub class: CharacterClass,
}

/// The authoritative answer to "may this account create this class?" — computed
/// against the account's earned-set, never trusted from a client. Three shapes:
/// an always-open class is creatable; a second-tier class is evolution-only and
/// never creatable this way; a level-gated class is creatable only once earned,
/// otherwise locked with the level still required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreationVerdict {
    /// The class may be created.
    Creatable,
    /// The class is level-gated and the account has not earned it yet.
    Locked {
        /// The level a character on the account must reach to earn it.
        required: Level,
    },
    /// A second-tier class, reachable only through a future class change.
    EvolutionOnly,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_unlocked_wire_pins() {
        let event = ClassUnlocked {
            class: CharacterClass::MagicGladiator,
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"class":"magic_gladiator"}"#
        );
        assert_eq!(
            serde_json::from_str::<ClassUnlocked>(r#"{"class":"magic_gladiator"}"#).unwrap(),
            event
        );
    }

    #[test]
    fn creation_verdict_wire_pins() {
        let creatable = CreationVerdict::Creatable;
        assert_eq!(
            serde_json::to_string(&creatable).unwrap(),
            r#"{"kind":"creatable"}"#
        );

        let locked = CreationVerdict::Locked {
            required: Level::new(220).unwrap(),
        };
        assert_eq!(
            serde_json::to_string(&locked).unwrap(),
            r#"{"kind":"locked","required":220}"#
        );

        let evolution = CreationVerdict::EvolutionOnly;
        assert_eq!(
            serde_json::to_string(&evolution).unwrap(),
            r#"{"kind":"evolution_only"}"#
        );

        for verdict in [creatable, locked, evolution] {
            let wire = serde_json::to_string(&verdict).unwrap();
            assert_eq!(
                serde_json::from_str::<CreationVerdict>(&wire).unwrap(),
                verdict
            );
        }
    }
}
