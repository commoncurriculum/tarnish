//! Cloning, comparing, dropping and walking values, which nest as deeply as the JSON they came
//! from.

use super::{Key, Map, Value};
use crate::stack;

impl Clone for Value {
    fn clone(&self) -> Value {
        match self {
            Value::Null => Value::Null,
            Value::Bool(boolean) => Value::Bool(*boolean),
            Value::Number(number) => Value::Number(*number),
            Value::String(string) => Value::String(string.clone()),
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|item| stack::grow(|| item.clone()))
                    .collect(),
            ),
            Value::Object(map) => Value::Object(Map {
                entries: map
                    .entries
                    .iter()
                    .map(|(key, item)| (key.clone(), stack::grow(|| item.clone())))
                    .collect(),
            }),
        }
    }
}

impl PartialEq for Value {
    /// Equal JSON: arrays equal item by item, objects with equal keys and values, in any order,
    /// and numbers equal as `===` has them.
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| stack::grow(|| a == b))
            }
            (Value::Object(a), Value::Object(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(key, a)| b.get(key).is_some_and(|b| stack::grow(|| a == b)))
            }
            _ => false,
        }
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        match self {
            Value::Array(items) => stack::drop_nested(items),
            Value::Object(map) => stack::drop_nested(&mut map.entries),
            _ => {}
        }
    }
}

/// A part of a value, as JSON writes it.
#[derive(Clone, Copy)]
pub enum Event<'a> {
    /// Null, a boolean, a number or a string.
    Scalar(&'a Value),
    /// An array or object, whose items, or keys each followed by its value, come next.
    Open(&'a Value),
    /// The key of the entry whose value comes next.
    Key(&'a Key),
    /// The end of the innermost array or object open.
    Close(&'a Value),
}

/// The parts of `value` in the order JSON writes them, however deeply it nests.
pub fn events(value: &Value) -> Events<'_> {
    Events {
        next: Some(value),
        open: Vec::new(),
    }
}

pub struct Events<'a> {
    next: Option<&'a Value>,
    open: Vec<(&'a Value, Children<'a>)>,
}

enum Children<'a> {
    Items(std::slice::Iter<'a, Value>),
    Entries(std::slice::Iter<'a, (Key, Value)>),
}

impl<'a> Iterator for Events<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Event<'a>> {
        let value = match self.next.take() {
            Some(value) => value,
            None => {
                let (open, children) = self.open.last_mut()?;
                let item = match children {
                    Children::Items(items) => items.next(),
                    Children::Entries(entries) => {
                        if let Some((key, item)) = entries.next() {
                            self.next = Some(item);
                            return Some(Event::Key(key));
                        }
                        None
                    }
                };
                match item {
                    Some(item) => item,
                    None => {
                        let closed = *open;
                        self.open.pop();
                        return Some(Event::Close(closed));
                    }
                }
            }
        };
        let children = match value {
            Value::Array(items) => Children::Items(items.iter()),
            Value::Object(map) => Children::Entries(map.entries.iter()),
            _ => return Some(Event::Scalar(value)),
        };
        self.open.push((value, children));
        Some(Event::Open(value))
    }
}
