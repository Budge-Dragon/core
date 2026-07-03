//! Shared container vocabulary. Data only: these types hold elements and
//! prove structural invariants; they carry no sampling or domain behavior.

use core::num::NonZeroUsize;

use serde::{Deserialize, Serialize};

/// A non-empty list. Serialized as a plain JSON array; an empty array is a
/// parse error. It proves non-emptiness by construction — the first element
/// always exists, so `first` returns `T`, never `Option<T>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "Vec<T>",
    into = "Vec<T>",
    bound(
        serialize = "T: Clone + Serialize",
        deserialize = "T: Deserialize<'de>"
    )
)]
pub struct OneOrMore<T> {
    head: T,
    tail: Vec<T>,
}

impl<T> OneOrMore<T> {
    /// Builds a non-empty list from a vector.
    ///
    /// # Errors
    /// Returns [`EmptyCollection`] when `items` is empty.
    pub fn new(items: Vec<T>) -> Result<Self, EmptyCollection> {
        let mut iter = items.into_iter();
        match iter.next() {
            None => Err(EmptyCollection),
            Some(head) => Ok(Self {
                head,
                tail: iter.collect(),
            }),
        }
    }

    /// The first element — always present.
    pub fn first(&self) -> &T {
        &self.head
    }

    /// Borrows every element, head first.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.into_iter()
    }

    /// The number of elements — always at least one.
    #[must_use]
    pub fn count(&self) -> NonZeroUsize {
        NonZeroUsize::MIN.saturating_add(self.tail.len())
    }
}

impl<'a, T> IntoIterator for &'a OneOrMore<T> {
    type Item = &'a T;
    type IntoIter = core::iter::Chain<core::iter::Once<&'a T>, core::slice::Iter<'a, T>>;

    fn into_iter(self) -> Self::IntoIter {
        core::iter::once(&self.head).chain(self.tail.iter())
    }
}

impl<T> IntoIterator for OneOrMore<T> {
    type Item = T;
    type IntoIter = core::iter::Chain<core::iter::Once<T>, std::vec::IntoIter<T>>;

    fn into_iter(self) -> Self::IntoIter {
        core::iter::once(self.head).chain(self.tail)
    }
}

impl<T> TryFrom<Vec<T>> for OneOrMore<T> {
    type Error = EmptyCollection;

    fn try_from(items: Vec<T>) -> Result<Self, Self::Error> {
        Self::new(items)
    }
}

impl<T> From<OneOrMore<T>> for Vec<T> {
    fn from(list: OneOrMore<T>) -> Self {
        let mut items = Self::with_capacity(1 + list.tail.len());
        items.push(list.head);
        items.extend(list.tail);
        items
    }
}

/// Parse failure: a non-empty list was given no elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyCollection;

impl core::fmt::Display for EmptyCollection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "expected at least one element, found none")
    }
}

impl core::error::Error for EmptyCollection {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(OneOrMore::<u8>::new(Vec::new()), Err(EmptyCollection));
    }

    #[test]
    fn single_element_list_has_count_one() {
        let list = OneOrMore::new(vec![7u8]).unwrap();
        assert_eq!(*list.first(), 7);
        assert_eq!(list.count().get(), 1);
        assert_eq!(list.iter().copied().collect::<Vec<_>>(), vec![7]);
    }

    #[test]
    fn multi_element_list_preserves_order_and_count() {
        let list = OneOrMore::new(vec![1u16, 2, 3, 4]).unwrap();
        assert_eq!(*list.first(), 1);
        assert_eq!(list.count().get(), 4);
        assert_eq!(list.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn serde_round_trips_as_plain_array() {
        let list = OneOrMore::new(vec![10u32, 20, 30]).unwrap();
        let json = serde_json::to_string(&list).unwrap();
        assert_eq!(json, "[10,20,30]");
        assert_eq!(serde_json::from_str::<OneOrMore<u32>>(&json).unwrap(), list);
    }

    #[test]
    fn serde_rejects_empty_array() {
        assert!(serde_json::from_str::<OneOrMore<u32>>("[]").is_err());
    }

    #[test]
    fn into_vec_round_trips() {
        let list = OneOrMore::new(vec![5u8, 6]).unwrap();
        let items: Vec<u8> = list.into();
        assert_eq!(items, vec![5, 6]);
    }

    /// A deliberately non-`Clone` payload: the type must still be
    /// representable now the `Clone` bound sits only on the serialize path.
    #[derive(Debug, PartialEq, Eq)]
    struct NonClone(u8);

    #[test]
    fn non_clone_element_is_representable() {
        let list = OneOrMore::new(vec![NonClone(1), NonClone(2)]).unwrap();
        assert_eq!(list.first(), &NonClone(1));
        assert_eq!(list.count().get(), 2);
    }

    #[test]
    fn borrowing_into_iterator_yields_refs_in_order() {
        let list = OneOrMore::new(vec![NonClone(1), NonClone(2), NonClone(3)]).unwrap();
        let seen: Vec<&NonClone> = (&list).into_iter().collect();
        assert_eq!(seen, vec![&NonClone(1), &NonClone(2), &NonClone(3)]);
        // `for x in &list` composes through the same impl.
        let mut total = 0u8;
        for item in &list {
            total += item.0;
        }
        assert_eq!(total, 6);
    }

    #[test]
    fn owning_into_iterator_yields_owned_in_order() {
        let list = OneOrMore::new(vec![NonClone(4), NonClone(5)]).unwrap();
        let owned: Vec<NonClone> = list.into_iter().collect();
        assert_eq!(owned, vec![NonClone(4), NonClone(5)]);
    }
}
