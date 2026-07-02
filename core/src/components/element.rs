//! The elemental vocabulary: the seven pre-S3 elements and the total
//! per-element structure every element-keyed table is read through.

use serde::{Deserialize, Serialize};

/// The seven elements of pre-S3 MU — the one element vocabulary in v2.
/// Serialized by name; any ordinal or column mapping is owned by the domain
/// doing the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    /// Ice.
    Ice,
    /// Poison.
    Poison,
    /// Lightning.
    Lightning,
    /// Fire.
    Fire,
    /// Earth.
    Earth,
    /// Wind.
    Wind,
    /// Water.
    Water,
}

/// One value per element — a total structure: every element always has a
/// value, so lookups return `T`, never `Option<T>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PerElement<T> {
    /// The ice value.
    pub ice: T,
    /// The poison value.
    pub poison: T,
    /// The lightning value.
    pub lightning: T,
    /// The fire value.
    pub fire: T,
    /// The earth value.
    pub earth: T,
    /// The wind value.
    pub wind: T,
    /// The water value.
    pub water: T,
}

impl<T> PerElement<T> {
    /// The value for one element — an exhaustive match, total by
    /// construction.
    pub fn of(&self, element: Element) -> &T {
        match element {
            Element::Ice => &self.ice,
            Element::Poison => &self.poison,
            Element::Lightning => &self.lightning,
            Element::Fire => &self.fire,
            Element::Earth => &self.earth,
            Element::Wind => &self.wind,
            Element::Water => &self.water,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVERY_ELEMENT: [Element; 7] = [
        Element::Ice,
        Element::Poison,
        Element::Lightning,
        Element::Fire,
        Element::Earth,
        Element::Wind,
        Element::Water,
    ];

    #[test]
    fn of_returns_each_named_field() {
        let table = PerElement {
            ice: 1u32,
            poison: 2,
            lightning: 3,
            fire: 4,
            earth: 5,
            wind: 6,
            water: 7,
        };
        assert_eq!(*table.of(Element::Ice), 1);
        assert_eq!(*table.of(Element::Poison), 2);
        assert_eq!(*table.of(Element::Lightning), 3);
        assert_eq!(*table.of(Element::Fire), 4);
        assert_eq!(*table.of(Element::Earth), 5);
        assert_eq!(*table.of(Element::Wind), 6);
        assert_eq!(*table.of(Element::Water), 7);
    }

    #[test]
    fn of_is_total_over_every_variant() {
        let table = PerElement {
            ice: 10u8,
            poison: 20,
            lightning: 30,
            fire: 40,
            earth: 0,
            wind: 0,
            water: 0,
        };
        for element in EVERY_ELEMENT {
            let _ = table.of(element);
        }
    }

    #[test]
    fn element_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&Element::Lightning).unwrap(),
            r#""lightning""#
        );
        assert_eq!(
            serde_json::from_str::<Element>(r#""water""#).unwrap(),
            Element::Water
        );
    }
}
