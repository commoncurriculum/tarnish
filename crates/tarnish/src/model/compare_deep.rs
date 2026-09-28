//! prosemirror-model's `compareDeep`: arrays and objects compared by their contents, everything
//! else with `===`.

use crate::js;
use crate::json::{Map, Value};
use crate::stack;

pub fn compare_deep(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => js::same_number(a, b),
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(a, b)| stack::grow(|| compare_deep(a, b)))
        }
        (Value::Object(a), Value::Object(b)) => objects_equal(a, b),
        _ => false,
    }
}

/// `compareDeep` on two objects: the same keys, with deeply equal values.
pub fn objects_equal(a: &Map, b: &Map) -> bool {
    a.len() == b.len()
        && a.iter().all(|(key, value)| {
            b.get(key)
                .is_some_and(|other| stack::grow(|| compare_deep(value, other)))
        })
}
