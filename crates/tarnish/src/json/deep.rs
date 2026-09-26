//! Cloning, comparing, dropping and walking values, which nest as deeply as the JSON they came
//! from: each recurses a few levels, then keeps its place in a vector instead of on the stack.

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
pub fn nested(value: &Value) -> impl Iterator<Item = (&Value, usize)> {
    let mut depth = 0;
    events(value).filter_map(move |event| match event {
        Event::Scalar(value) => Some((value, depth)),
        Event::Open(value) => {
            depth += 1;
            Some((value, depth - 1))
        }
        Event::Close(_) => {
            depth -= 1;
            None
        }
        Event::Key(_) => None,
    })
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
