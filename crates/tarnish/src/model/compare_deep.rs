//! prosemirror-model's `compareDeep`: arrays and objects compared by their contents, everything
//! else with `===`.

use crate::chunk::JsonView;
use crate::chunk::Kind;
use crate::js;
use crate::json::{Map, Value};
use crate::stack;

pub fn compare_deep(a: &Value, b: &Value) -> bool {
    deep_equal(a, b)
}

pub fn objects_equal(a: &Map, b: &Map) -> bool {
    let entries = a.iter().map(|(key, value)| (key.as_str(), value));
    entries_equal(entries, b.len(), |key| b.get(key))
}

/// `compareDeep` of two values, each held however it is.
pub(crate) fn deep_equal<'a, 'b>(a: impl JsonView<'a>, b: impl JsonView<'b>) -> bool {
    if let (Some(a), Some(b)) = (a.place(), b.place())
        && a == b
    {
        return true;
    }
    match (a.kind(), b.kind()) {
        (Kind::Null, Kind::Null) => true,
        (Kind::Bool(a), Kind::Bool(b)) => a == b,
        (Kind::Number(a), Kind::Number(b)) => js::same_number(&a, &b),
        (Kind::String(a), Kind::String(b)) => a == b,
        (Kind::Array(_), Kind::Array(_)) => {
            let (a, b) = (a.items(), b.items());
            a.len() == b.len() && a.zip(b).all(|(a, b)| stack::grow(|| deep_equal(a, b)))
        }
        (Kind::Object(_), Kind::Object(len)) => {
            entries_equal(a.entries(), len as usize, |key| b.get(key))
        }
        _ => false,
    }
}

/// `compareDeep` of two objects: `a`'s entries, and the other's length and values by key.
pub(crate) fn entries_equal<'a, 'b, A: JsonView<'a>, B: JsonView<'b>>(
    mut a: impl ExactSizeIterator<Item = (&'a str, A)>,
    len: usize,
    get: impl Fn(&str) -> Option<B>,
) -> bool {
    a.len() == len
        && a.all(|(key, value)| {
            get(key).is_some_and(|other| stack::grow(|| deep_equal(value, other)))
        })
}
