//! Erlang's external term format, from which the VM makes a whole term in one call: values and
//! nodes as the terms Jason decodes from the JSON `JSON.stringify` writes for them.

use tarnish::chunk::{Kind, ValueRef};
use tarnish::js::{self, WrittenNumber};
use tarnish::json::{Number, Value};
use tarnish::{Field, Fields, NodeRef, SetRef, stack};

const VERSION: u8 = 131;
const NEW_FLOAT: u8 = 70;
const SMALL_INTEGER: u8 = 97;
const INTEGER: u8 = 98;
const NIL: u8 = 106;
const LIST: u8 = 108;
const BINARY: u8 = 109;
const SMALL_BIG: u8 = 110;
const MAP: u8 = 116;
const SMALL_ATOM_UTF8: u8 = 119;

/// The value's term, when it takes no more than `limit` bytes.
pub fn write(value: &Value, limit: usize) -> Option<Vec<u8>> {
    let mut writer = Writer::new(limit, 64);
    writer.value(value);
    writer.finish()
}

/// [`write`] of a value a chunk holds.
pub fn write_ref(value: ValueRef, limit: usize) -> Option<Vec<u8>> {
    let mut writer = Writer::new(limit, 64);
    writer.value_ref(value);
    writer.finish()
}

/// The list of a set's marks' JSON.
pub fn write_marks(marks: SetRef, limit: usize) -> Option<Vec<u8>> {
    let mut writer = Writer::new(limit, 64);
    writer.list(marks.iter(), Writer::fields);
    writer.finish()
}

/// What [`write`] writes for the node's JSON, without making the JSON.
pub fn write_node(node: NodeRef, limit: usize) -> Option<Vec<u8>> {
    // A document's term takes about four bytes a position.
    let mut writer = Writer::new(limit, node.node_size() * 4 + 64);
    writer.fields(node);
    writer.finish()
}

struct Writer {
    out: Vec<u8>,
    limit: usize,
}

impl Writer {
    fn new(limit: usize, capacity: usize) -> Writer {
        let mut out = Vec::with_capacity(capacity.min(limit));
        out.push(VERSION);
        Writer { out, limit }
    }

    /// Whether the term is past the limit, so that the rest needn't be written.
    fn full(&self) -> bool {
        self.out.len() > self.limit
    }

    fn finish(self) -> Option<Vec<u8>> {
        (!self.full()).then_some(self.out)
    }

    fn value(&mut self, value: &Value) {
        if self.full() {
            return;
        }
        match value {
            Value::Null => self.atom("nil"),
            Value::Bool(true) => self.atom("true"),
            Value::Bool(false) => self.atom("false"),
            Value::Number(number) => self.number(number),
            Value::String(text) => self.binary(text),
            Value::Array(items) => self.list(items.iter(), |writer, item| {
                stack::grow(|| writer.value(item))
            }),
            Value::Object(object) => {
                self.map_header(object.len());
                for (key, item) in object {
                    self.binary(key);
                    stack::grow(|| self.value(item));
                }
            }
        }
    }

    fn value_ref(&mut self, value: ValueRef) {
        if self.full() {
            return;
        }
        match value.kind() {
            Kind::Null => self.atom("nil"),
            Kind::Bool(true) => self.atom("true"),
            Kind::Bool(false) => self.atom("false"),
            Kind::Number(number) => self.number(&number),
            Kind::String(text) => self.binary(text),
            Kind::Array(_) => self.list(value.items(), |writer, item| {
                stack::grow(|| writer.value_ref(item))
            }),
            Kind::Object(len) => {
                self.map_header(len as usize);
                for (key, item) in value.entries() {
                    self.binary(key);
                    stack::grow(|| self.value_ref(item));
                }
            }
        }
    }

    fn fields<'c>(&mut self, fields: impl Fields<'c>) {
        if self.full() {
            return;
        }
        self.map_header(fields.field_count());
        fields.fields(|field| {
            self.binary(field.key());
            match field {
                Field::Type(name) => self.binary(name),
                Field::Attrs(attrs) => self.value_ref(attrs),
                Field::Content(node) => self.list(node.children(), |writer, child| {
                    stack::grow(|| writer.fields(child))
                }),
                Field::Marks(marks) => self.list(marks.iter(), Writer::fields),
                // A binary holds UTF-8, which has no lone surrogate.
                Field::Text(text) => self.binary(&text.to_string_lossy()),
            }
        });
    }

    fn map_header(&mut self, arity: usize) {
        self.out.push(MAP);
        self.out.extend_from_slice(&(arity as u32).to_be_bytes());
    }

    fn list<T>(
        &mut self,
        items: impl ExactSizeIterator<Item = T>,
        mut write: impl FnMut(&mut Writer, T),
    ) {
        if items.len() > 0 {
            self.out.push(LIST);
            self.out
                .extend_from_slice(&(items.len() as u32).to_be_bytes());
            for item in items {
                write(self, item);
            }
        }
        self.out.push(NIL);
    }

    fn atom(&mut self, name: &str) {
        self.out.push(SMALL_ATOM_UTF8);
        self.out.push(name.len() as u8);
        self.out.extend_from_slice(name.as_bytes());
    }

    fn binary(&mut self, text: &str) {
        self.out.push(BINARY);
        self.out
            .extend_from_slice(&(text.len() as u32).to_be_bytes());
        self.out.extend_from_slice(text.as_bytes());
    }

    /// Jason reads the digits JavaScript writes for an integer as an integer, however large.
    fn number(&mut self, number: &Number) {
        match js::written_number(number) {
            WrittenNumber::Integer(value) => self.integer(value),
            WrittenNumber::Float(double) => {
                self.out.push(NEW_FLOAT);
                self.out.extend_from_slice(&double.to_bits().to_be_bytes());
            }
            WrittenNumber::Null => self.atom("nil"),
        }
    }

    fn integer(&mut self, value: i128) {
        if let Ok(small) = u8::try_from(value) {
            self.out.push(SMALL_INTEGER);
            self.out.push(small);
        } else if let Ok(word) = i32::try_from(value) {
            self.out.push(INTEGER);
            self.out.extend_from_slice(&word.to_be_bytes());
        } else {
            let digits = value.unsigned_abs().to_le_bytes();
            let length = digits.len() - digits.iter().rev().take_while(|&&byte| byte == 0).count();
            self.out.push(SMALL_BIG);
            self.out.push(length as u8);
            self.out.push(u8::from(value < 0));
            self.out.extend_from_slice(&digits[..length]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(json: &str) -> Vec<u8> {
        let value = tarnish::json::from_str(json).expect("JSON");
        write(&value, usize::MAX).expect("a term")[1..].to_vec()
    }

    /// Integers in the external format's smallest encoding, and floats where JavaScript writes
    /// a fraction or an exponent.
    #[test]
    fn writes_numbers_as_jason_decodes_javascripts_json() {
        assert_eq!(written("255"), [SMALL_INTEGER, 255]);
        assert_eq!(written("-1"), [INTEGER, 255, 255, 255, 255]);
        assert_eq!(written("2147483648"), [SMALL_BIG, 4, 0, 0, 0, 0, 128]);
        assert_eq!(written("-2147483649"), [SMALL_BIG, 4, 1, 1, 0, 0, 128]);
        // A double, which JavaScript writes as 123456789012345680000.
        assert_eq!(
            written("123456789012345678901"),
            [SMALL_BIG, 9, 0, 128, 112, 54, 47, 129, 159, 78, 177, 6]
        );
        let mut float = vec![NEW_FLOAT];
        float.extend_from_slice(&1e21f64.to_bits().to_be_bytes());
        assert_eq!(written("1e21"), float);
        let mut half = vec![NEW_FLOAT];
        half.extend_from_slice(&0.5f64.to_bits().to_be_bytes());
        assert_eq!(written("0.5"), half);
    }

    #[test]
    fn stops_past_the_limit() {
        let value = tarnish::json::from_str(r#"[[["deep"]], "text"]"#).expect("JSON");
        let whole = write(&value, usize::MAX).expect("a term");
        assert_eq!(write(&value, whole.len()), Some(whole.clone()));
        assert_eq!(write(&value, whole.len() - 1), None);
    }
}
