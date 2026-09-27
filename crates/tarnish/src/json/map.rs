//! An object's entries in a vector, searched in place of a hashed map.

use super::{Key, Value};

/// An object's entries in insertion order, keys unique.
#[derive(Clone, Debug, Default)]
pub struct Map {
    pub(super) entries: Vec<(Key, Value)>,
}

impl Map {
    #[inline]
    pub const fn new() -> Self {
        Map {
            entries: Vec::new(),
        }
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Map {
            entries: Vec::with_capacity(capacity),
        }
    }

    #[inline]
    fn position(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|(entry, _)| entry == key)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[inline]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(entry, _)| entry == key)
            .map(|(_, value)| value)
    }

    /// `get`, looking first at the entry `next` points to, and leaving `next` after the entry
    /// found: keys looked up in the order the entries have take one comparison each.
    #[inline]
    pub fn get_from(&self, key: &str, next: &mut usize) -> Option<&Value> {
        let index = match self.entries.get(*next) {
            Some((entry, _)) if entry == key => *next,
            _ => self.position(key)?,
        };
        *next = index + 1;
        Some(&self.entries[index].1)
    }

    #[inline]
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.entries
            .iter_mut()
            .find(|(entry, _)| entry == key)
            .map(|(_, value)| value)
    }

    #[inline]
    pub fn contains_key(&self, key: &str) -> bool {
        self.position(key).is_some()
    }

    /// Replaces the value of a key already there, where it is, or adds the key at the end.
    #[inline]
    pub fn insert(&mut self, key: Key, value: Value) -> Option<Value> {
        match self.position(&key) {
            Some(index) => Some(std::mem::replace(&mut self.entries[index].1, value)),
            None => {
                self.entries.push((key, value));
                None
            }
        }
    }

    /// Adds a key known not to be there yet.
    #[inline]
    pub fn push(&mut self, key: Key, value: Value) {
        debug_assert!(!self.contains_key(&key), "{key:?} is already a key");
        self.entries.push((key, value));
    }

    /// Removes a key, moving the last entry into its place, as serde_json's `remove` does.
    #[inline]
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.swap_remove(key)
    }

    #[inline]
    pub fn swap_remove(&mut self, key: &str) -> Option<Value> {
        let index = self.position(key)?;
        Some(self.entries.swap_remove(index).1)
    }

    /// Removes a key, keeping the order of the others.
    #[inline]
    pub fn shift_remove(&mut self, key: &str) -> Option<Value> {
        let index = self.position(key)?;
        Some(self.entries.remove(index).1)
    }

    #[inline]
    pub fn retain(&mut self, mut keep: impl FnMut(&Key, &mut Value) -> bool) {
        self.entries.retain_mut(|(key, value)| keep(key, value));
    }

    #[inline]
    pub fn entry(&mut self, key: impl Into<Key>) -> Entry<'_> {
        let key = key.into();
        let index = match self.position(&key) {
            Some(index) => index,
            None => {
                self.entries.push((key, Value::Null));
                return Entry {
                    value: &mut self.entries.last_mut().expect("just pushed").1,
                    vacant: true,
                };
            }
        };
        Entry {
            value: &mut self.entries[index].1,
            vacant: false,
        }
    }

    #[inline]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&Key, &Value)> + ExactSizeIterator {
        self.entries.iter().map(|(key, value)| (key, value))
    }

    #[inline]
    pub fn iter_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = (&Key, &mut Value)> + ExactSizeIterator {
        self.entries.iter_mut().map(|(key, value)| (&*key, value))
    }

    #[inline]
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &Key> + ExactSizeIterator {
        self.entries.iter().map(|(key, _)| key)
    }

    #[inline]
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &Value> + ExactSizeIterator {
        self.entries.iter().map(|(_, value)| value)
    }

    #[inline]
    pub fn values_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = &mut Value> + ExactSizeIterator {
        self.entries.iter_mut().map(|(_, value)| value)
    }
}

/// A key's place in a map, which `entry` has filled with `null` if it was empty.
pub struct Entry<'a> {
    value: &'a mut Value,
    vacant: bool,
}

impl<'a> Entry<'a> {
    pub fn or_insert(self, default: Value) -> &'a mut Value {
        if self.vacant {
            *self.value = default;
        }
        self.value
    }

    pub fn or_insert_with(self, default: impl FnOnce() -> Value) -> &'a mut Value {
        if self.vacant {
            *self.value = default();
        }
        self.value
    }
}

impl PartialEq for Map {
    /// Equal keys with equal values, in any order.
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .all(|(key, value)| other.get(key).is_some_and(|other| value == other))
    }
}

impl<'a> IntoIterator for &'a Map {
    type Item = (&'a Key, &'a Value);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (Key, Value)>,
        fn(&'a (Key, Value)) -> (&'a Key, &'a Value),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(key, value)| (key, value))
    }
}

impl<'a> IntoIterator for &'a mut Map {
    type Item = (&'a Key, &'a mut Value);
    type IntoIter = std::iter::Map<
        std::slice::IterMut<'a, (Key, Value)>,
        fn(&'a mut (Key, Value)) -> (&'a Key, &'a mut Value),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter_mut().map(|(key, value)| (&*key, value))
    }
}

impl IntoIterator for Map {
    type Item = (Key, Value);
    type IntoIter = std::vec::IntoIter<(Key, Value)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl Map {
    /// The map of entries whose keys are distinct, as another object's are, without looking
    /// for each key among those before it.
    pub fn from_distinct(entries: impl IntoIterator<Item = (Key, Value)>) -> Map {
        let entries = entries.into_iter();
        let mut map = Map::with_capacity(entries.size_hint().0);
        for (key, value) in entries {
            map.push(key, value);
        }
        map
    }
}

impl FromIterator<(Key, Value)> for Map {
    fn from_iter<I: IntoIterator<Item = (Key, Value)>>(iter: I) -> Self {
        let mut map = Map::new();
        map.extend(iter);
        map
    }
}

impl Extend<(Key, Value)> for Map {
    fn extend<I: IntoIterator<Item = (Key, Value)>>(&mut self, iter: I) {
        for (key, value) in iter {
            self.insert(key, value);
        }
    }
}

impl std::ops::Index<&str> for Map {
    type Output = Value;

    fn index(&self, key: &str) -> &Value {
        self.get(key).expect("no entry found for key")
    }
}
