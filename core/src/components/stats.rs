//! A live character's four (or five) trainable attributes. The command stat
//! exists only on the command-class shape, so an illegal
//! command-on-non-command-class combination is unrepresentable — the fifth
//! stat lives on its own variant, never as an `Option`.
//!
//! This mirrors the *shape* of the creation-time starting stats without
//! importing them: the data-side and the live-side are independent concerns,
//! and no conversion between them exists until a service needs one.

use serde::{Deserialize, Serialize};

/// A character's trainable attributes, kind-tagged. `Standard` is the four
/// classic stats; `WithCommand` adds the command-class fifth stat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stats {
    /// The four classic trainable stats.
    Standard {
        /// Strength.
        strength: u16,
        /// Agility.
        agility: u16,
        /// Vitality.
        vitality: u16,
        /// Energy.
        energy: u16,
    },
    /// The four classic stats plus command (the command-class fifth stat).
    WithCommand {
        /// Strength.
        strength: u16,
        /// Agility.
        agility: u16,
        /// Vitality.
        vitality: u16,
        /// Energy.
        energy: u16,
        /// Command.
        command: u16,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_wire_has_no_command_field() {
        let stats = Stats::Standard {
            strength: 28,
            agility: 20,
            vitality: 25,
            energy: 10,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"standard","strength":28,"agility":20,"vitality":25,"energy":10}"#
        );
        assert_eq!(serde_json::from_str::<Stats>(&json).unwrap(), stats);
    }

    #[test]
    fn with_command_wire_carries_command() {
        let stats = Stats::WithCommand {
            strength: 26,
            agility: 20,
            vitality: 20,
            energy: 15,
            command: 30,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"with_command","strength":26,"agility":20,"vitality":20,"energy":15,"command":30}"#
        );
        assert_eq!(serde_json::from_str::<Stats>(&json).unwrap(), stats);
    }
}
