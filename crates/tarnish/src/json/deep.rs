//! Cloning, comparing, dropping and measuring values, which nest as deeply as the JSON they
//! came from: each recurses a few levels, then keeps its place in a vector instead of on the
//! stack.

use std::cell::{Cell, RefCell};

use super::{Key, Map, Value};

/// How many levels these recurse before they go on without recursing.
const RECURSION: usize = 64;

impl Clone for Value {
    fn clone(&self) -> Value {
        clone_within(self, 0)
    }
}

fn clone_within(value: &Value, depth: usize) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Bool(boolean) => Value::Bool(*boolean),
        Value::Number(number) => Value::Number(number.clone()),
        Value::String(string) => Value::String(string.clone()),
        _ if depth == RECURSION => clone_deep(value),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| clone_within(item, depth + 1))
                .collect(),
        ),
        Value::Object(map) => Value::Object(Map {
            entries: map
                .entries
                .iter()
                .map(|(key, item)| (key.clone(), clone_within(item, depth + 1)))
                .collect(),
        }),
    }
}

fn clone_deep(root: &Value) -> Value {
    enum Copying<'a> {
        Array(std::slice::Iter<'a, Value>, Vec<Value>),
        Object(std::slice::Iter<'a, (Key, Value)>, Vec<(Key, Value)>),
    }
    let mut open: Vec<Copying> = Vec::new();
    let mut next = root;
    loop {
        let mut copied = match next {
            Value::Array(items) => {
                open.push(Copying::Array(
                    items.iter(),
                    Vec::with_capacity(items.len()),
                ));
                None
            }
            Value::Object(map) => {
                open.push(Copying::Object(
                    map.entries.iter(),
                    Vec::with_capacity(map.len()),
                ));
                None
            }
            scalar => Some(clone_within(scalar, 0)),
        };
        loop {
            let Some(copying) = open.last_mut() else {
                return copied.expect("the root's copy");
            };
            let following = match copying {
                Copying::Array(items, copies) => {
                    copies.extend(copied.take());
                    items.next()
                }
                Copying::Object(entries, copies) => {
                    if let Some(copy) = copied.take() {
                        copies.last_mut().expect("the key being copied").1 = copy;
                    }
                    entries.next().map(|(key, item)| {
                        copies.push((key.clone(), Value::Null));
                        item
                    })
                }
            };
            match following {
                Some(item) => {
                    next = item;
                    break;
                }
                None => {
                    copied = Some(match open.pop().expect("the value being copied") {
                        Copying::Array(_, copies) => Value::Array(copies),
                        Copying::Object(_, copies) => Value::Object(Map { entries: copies }),
                    })
                }
            }
        }
    }
}

impl PartialEq for Value {
    /// Equal JSON: arrays equal item by item, and objects with equal keys and values, in any
    /// order.
    fn eq(&self, other: &Value) -> bool {
        equal_within(self, other, 0)
    }
}

fn equal_within(a: &Value, b: &Value, depth: usize) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        _ if depth == RECURSION => equal_deep(a, b),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equal_within(a, b, depth + 1))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| equal_within(a, b, depth + 1)))
        }
        _ => false,
    }
}

fn equal_deep(a: &Value, b: &Value) -> bool {
    let mut pending = vec![(a, b)];
    while let Some(pair) = pending.pop() {
        match pair {
            (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
                pending.extend(a.iter().zip(b))
            }
            (Value::Object(a), Value::Object(b)) if a.len() == b.len() => {
                for (key, a) in a {
                    match b.get(key) {
                        Some(b) => pending.push((a, b)),
                        None => return false,
                    }
                }
            }
            (Value::Array(_) | Value::Object(_), _) => return false,
            (a, b) => {
                if !equal_within(a, b, 0) {
                    return false;
                }
            }
        }
    }
    true
}

thread_local! {
    /// How many values are dropping, one inside another.
    static DROPPING: Cell<usize> = const { Cell::new(0) };
    /// What values dropping [`RECURSION`] levels down held, left for the outermost to drop.
    static LEFT: RefCell<Vec<Contents>> = const { RefCell::new(Vec::new()) };
}

enum Contents {
    Items(Vec<Value>),
    Entries(Vec<(Key, Value)>),
}

impl Contents {
    fn drop(self) {
        match self {
            Contents::Items(items) => drop(items),
            Contents::Entries(entries) => drop(entries),
        }
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        let contents = match self {
            Value::Array(items) if !items.is_empty() => Contents::Items(std::mem::take(items)),
            Value::Object(map) if !map.is_empty() => {
                Contents::Entries(std::mem::take(&mut map.entries))
            }
            _ => return,
        };
        let depth = DROPPING.get();
        if depth == RECURSION {
            LEFT.with_borrow_mut(|left| left.push(contents));
            return;
        }
        DROPPING.set(depth + 1);
        contents.drop();
        if depth == 0 {
            while let Some(contents) = LEFT.with_borrow_mut(Vec::pop) {
                contents.drop();
            }
        }
        DROPPING.set(depth);
    }
}

/// Every value inside `value`, and `value` itself first, with how many arrays and objects hold
/// it, in the order JSON writes them.
pub fn nested(value: &Value) -> Nested<'_> {
    Nested {
        next: Some((value, 0)),
        open: Vec::new(),
    }
}

pub struct Nested<'a> {
    next: Option<(&'a Value, usize)>,
    open: Vec<(Children<'a>, usize)>,
}

enum Children<'a> {
    Items(std::slice::Iter<'a, Value>),
    Entries(std::slice::Iter<'a, (Key, Value)>),
}

impl<'a> Iterator for Nested<'a> {
    type Item = (&'a Value, usize);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((value, depth)) = self.next.take() {
                match value {
                    Value::Array(items) => {
                        self.open.push((Children::Items(items.iter()), depth + 1))
                    }
                    Value::Object(map) => self
                        .open
                        .push((Children::Entries(map.entries.iter()), depth + 1)),
                    _ => {}
                }
                return Some((value, depth));
            }
            let (children, depth) = self.open.last_mut()?;
            let child = match children {
                Children::Items(items) => items.next(),
                Children::Entries(entries) => entries.next().map(|(_, item)| item),
            };
            match child {
                Some(child) => self.next = Some((child, *depth)),
                None => {
                    self.open.pop();
                }
            }
        }
    }
}
