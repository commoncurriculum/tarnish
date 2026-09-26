//! Indexing values by key and position, and comparing them with plain Rust values.

use super::{Map, NULL, Value};

/// What a value can be indexed by: a key of an object, or a position in an array.
pub trait JsonIndex {
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value>;
    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value>;
    /// `value[index] = …`: an object's entry, added if missing, or an array's item.
    fn index_or_insert<'v>(&self, value: &'v mut Value) -> &'v mut Value;
}

impl JsonIndex for str {
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        value.as_object()?.get(self)
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        value.as_object_mut()?.get_mut(self)
    }

    fn index_or_insert<'v>(&self, value: &'v mut Value) -> &'v mut Value {
        if value.is_null() {
            *value = Value::Object(Map::new());
        }
        match value {
            Value::Object(map) => map.entry(self).or_insert(Value::Null),
            other => panic!("cannot access key {self:?} in JSON {other:?}"),
        }
    }
}

impl JsonIndex for String {
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        self.as_str().index_into(value)
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        self.as_str().index_into_mut(value)
    }

    fn index_or_insert<'v>(&self, value: &'v mut Value) -> &'v mut Value {
        self.as_str().index_or_insert(value)
    }
}

impl JsonIndex for usize {
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        value.as_array()?.get(*self)
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        value.as_array_mut()?.get_mut(*self)
    }

    fn index_or_insert<'v>(&self, value: &'v mut Value) -> &'v mut Value {
        match value {
            Value::Array(items) => {
                let len = items.len();
                items
                    .get_mut(*self)
                    .unwrap_or_else(|| panic!("index {self} out of bounds for an array of {len}"))
            }
            other => panic!("cannot access index {self} of JSON {other:?}"),
        }
    }
}

impl<T: JsonIndex + ?Sized> JsonIndex for &T {
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        (**self).index_into(value)
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        (**self).index_into_mut(value)
    }

    fn index_or_insert<'v>(&self, value: &'v mut Value) -> &'v mut Value {
        (**self).index_or_insert(value)
    }
}

impl<I: JsonIndex> std::ops::Index<I> for Value {
    type Output = Value;

    fn index(&self, index: I) -> &Value {
        index.index_into(self).unwrap_or(&NULL)
    }
}

impl<I: JsonIndex> std::ops::IndexMut<I> for Value {
    fn index_mut(&mut self, index: I) -> &mut Value {
        index.index_or_insert(self)
    }
}

impl PartialEq<str> for Value {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == Some(other)
    }
}

impl PartialEq<&str> for Value {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}

impl PartialEq<String> for Value {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == Some(other.as_str())
    }
}

impl PartialEq<Value> for str {
    fn eq(&self, other: &Value) -> bool {
        other == self
    }
}

impl PartialEq<Value> for &str {
    fn eq(&self, other: &Value) -> bool {
        other == *self
    }
}

impl PartialEq<Value> for String {
    fn eq(&self, other: &Value) -> bool {
        other == self
    }
}

impl PartialEq<bool> for Value {
    fn eq(&self, other: &bool) -> bool {
        self.as_bool() == Some(*other)
    }
}

macro_rules! number_equality {
    ($($integer:ty => $as:ident as $target:ty),*) => {
        $(
            impl PartialEq<$integer> for Value {
                fn eq(&self, other: &$integer) -> bool {
                    self.$as().is_some_and(|value| value == *other as $target)
                }
            }
        )*
    };
}

number_equality!(i32 => as_i64 as i64, i64 => as_i64 as i64, u32 => as_u64 as u64, u64 => as_u64 as u64, usize => as_u64 as u64, f64 => as_f64 as f64);
