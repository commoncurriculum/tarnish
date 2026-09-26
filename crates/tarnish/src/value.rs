//! Attribute values: what JSON holds, plus `undefined`, which an attribute's default may be.

use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(Arc<str>),
    Array(Arc<[Value]>),
    Object(Arc<Object>),
}

/// An object's properties, in their order.
#[derive(Clone, Debug, Default)]
pub struct Object {
    entries: Vec<(Arc<str>, Value)>,
}

impl Object {
    pub const fn new() -> Self {
        Object {
            entries: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Object {
            entries: Vec::with_capacity(capacity),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(name, _)| &**name == key)
            .map(|(_, value)| value)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.iter().any(|(name, _)| &**name == key)
    }

    /// Sets a property, where it is if the object has it, or last.
    pub fn insert(&mut self, key: impl Into<Arc<str>>, value: Value) {
        let key = key.into();
        match self.entries.iter_mut().find(|(name, _)| *name == key) {
            Some((_, slot)) => *slot = value,
            None => self.entries.push((key, value)),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Arc<str>, &Value)> {
        self.entries.iter().map(|(name, value)| (name, value))
    }
}

impl FromIterator<(Arc<str>, Value)> for Object {
    fn from_iter<I: IntoIterator<Item = (Arc<str>, Value)>>(iter: I) -> Self {
        let mut object = Object::new();
        for (key, value) in iter {
            object.insert(key, value);
        }
        object
    }
}

impl Value {
    /// `typeof value`, with `null` as `"null"`, as an attribute's
    /// [`validate`](crate::AttributeSpec) type list names it.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) | Value::Object(_) => "object",
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(string) => Some(string),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(object) => Some(object),
            _ => None,
        }
    }

    /// `value[key]`: an object's property, and `undefined` for anything else.
    pub fn get(&self, key: &str) -> &Value {
        static UNDEFINED: Value = Value::Undefined;
        self.as_object()
            .and_then(|object| object.get(key))
            .unwrap_or(&UNDEFINED)
    }

    /// Attributes as a type's `create` reads them: `null` and `undefined` are none, and a value
    /// that isn't an object has no properties.
    pub fn as_attrs(&self) -> Option<&Object> {
        static EMPTY: Object = Object::new();
        match self {
            Value::Undefined | Value::Null => None,
            Value::Object(object) => Some(object),
            _ => Some(&EMPTY),
        }
    }

    /// JavaScript's truthiness.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(value) => *value,
            Value::Number(number) => *number != 0.0 && !number.is_nan(),
            Value::String(string) => !string.is_empty(),
            Value::Array(_) | Value::Object(_) => true,
        }
    }

    /// `a === b`.
    pub fn strict_equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => Arc::ptr_eq(a, b),
            (Value::Object(a), Value::Object(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// `compareDeep`: arrays and objects compared by their contents, everything else with `===`.
pub fn compare_deep(a: &Value, b: &Value) -> bool {
    if a.strict_equals(b) {
        return true;
    }
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| compare_deep(a, b))
        }
        (Value::Object(a), Value::Object(b)) => objects_equal(a, b),
        _ => false,
    }
}

/// `compareDeep` on two objects: the same properties with deeply equal values.
pub fn objects_equal(a: &Object, b: &Object) -> bool {
    a.iter()
        .all(|(key, value)| b.get(key).is_some_and(|other| compare_deep(value, other)))
        && b.iter().all(|(key, _)| a.contains_key(key))
}

impl fmt::Display for Value {
    /// `String(value)`.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Undefined => f.write_str("undefined"),
            Value::Null => f.write_str("null"),
            Value::Bool(value) => write!(f, "{value}"),
            Value::Number(number) => f.write_str(&number_to_string(*number)),
            Value::String(string) => f.write_str(string),
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(",")?;
                    }
                    if !matches!(item, Value::Undefined | Value::Null) {
                        write!(f, "{item}")?;
                    }
                }
                Ok(())
            }
            Value::Object(_) => f.write_str("[object Object]"),
        }
    }
}

/// `String(number)`.
pub fn number_to_string(number: f64) -> String {
    let mut buffer = ryu_js::Buffer::new();
    buffer.format(number).to_owned()
}

impl Value {
    /// `JSON.stringify(value)`, byte for byte: a property that is `undefined` is left out, and
    /// in an array is `null`, as are numbers that aren't finite.
    pub fn to_json_string(&self) -> String {
        let mut out = String::new();
        write_json(self, &mut out);
        out
    }
}

fn write_json(value: &Value, out: &mut String) {
    match value {
        Value::Undefined | Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) if number.is_finite() => out.push_str(&number_to_string(*number)),
        Value::Number(_) => out.push_str("null"),
        Value::String(text) => write_json_string(text, out),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Value::Object(object) => {
            out.push('{');
            let mut first = true;
            for (key, item) in object.iter() {
                if matches!(item, Value::Undefined) {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                write_json_string(key, out);
                out.push(':');
                write_json(item, out);
            }
            out.push('}');
        }
    }
}

fn write_json_string(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32))
            }
            character => out.push(character),
        }
    }
    out.push('"');
}
