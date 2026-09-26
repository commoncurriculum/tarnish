//! JSON values to the terms Jason decodes from the JSON `JSON.stringify` writes for them.

use rustler::types::atom;
use rustler::wrapper::NIF_TERM;
use rustler::{BigInt, Encoder, Env, NewBinary, Term};

use tarnish::json::{Number, Value};

/// What Jason decodes from the JSON `JSON.stringify` writes for the value.
pub fn write<'a>(env: Env<'a>, value: &Value) -> Term<'a> {
    let term = TermWriter::new(env).write(value);
    // SAFETY: the writer makes its terms in `env`.
    unsafe { Term::new(env, term) }
}

/// A map of at most this many keys is a flatmap, which keeps its keys in term order.
const FLATMAP_KEYS: usize = 32;

/// The terms of the maps and lists being built, each one's above its parent's, so that no map
/// or list needs arrays of its own.
struct TermWriter<'a, 'v> {
    env: Env<'a>,
    items: Vec<NIF_TERM>,
    /// Each entry's key, and its key's and value's terms.
    entries: Vec<(Text<'v>, NIF_TERM, NIF_TERM)>,
    /// The keys and values of the map being made.
    keys: Vec<NIF_TERM>,
    values: Vec<NIF_TERM>,
    /// The binaries made for keys and types, which a document repeats, so that one each serves
    /// them all. A text has one slot, which a later text of the same slot takes over.
    made: [Option<(Text<'v>, NIF_TERM)>; MADE_SLOTS],
}

const MADE_SLOTS: usize = 128;

impl<'a, 'v> TermWriter<'a, 'v> {
    fn new(env: Env<'a>) -> Self {
        TermWriter {
            env,
            items: Vec::new(),
            entries: Vec::new(),
            keys: Vec::new(),
            values: Vec::new(),
            made: [None; MADE_SLOTS],
        }
    }

    fn write(&mut self, value: &'v Value) -> NIF_TERM {
        let env = self.env;
        match value {
            Value::Null => atom::nil().encode(env).as_c_arg(),
            Value::Bool(boolean) => boolean.encode(env).as_c_arg(),
            Value::Number(number) => encode_number(env, number).as_c_arg(),
            Value::String(text) => binary(env, text).as_c_arg(),
            Value::Array(items) => {
                let base = self.items.len();
                for item in items {
                    let term = self.write(item);
                    self.items.push(term);
                }
                self.list(base)
            }
            Value::Object(map) => {
                let base = self.entries.len();
                for (key, item) in map {
                    let term = match item {
                        Value::String(kind) if key == "type" => self.shared_binary(Text::new(kind)),
                        _ => self.write(item),
                    };
                    let key = Text::new(key);
                    let name = self.shared_binary(key);
                    self.entries.push((key, name, term));
                }
                self.map(base)
            }
        }
    }

    fn shared_binary(&mut self, text: Text<'v>) -> NIF_TERM {
        let slot = &mut self.made[text.slot()];
        match *slot {
            Some((made, term)) if made == text => term,
            _ => {
                let term = binary(self.env, text.text).as_c_arg();
                *slot = Some((text, term));
                term
            }
        }
    }

    /// The list of the items from `base` on.
    fn list(&mut self, base: usize) -> NIF_TERM {
        let env = self.env.as_c_arg();
        // SAFETY: the items are terms made in `env`.
        let list = unsafe { rustler::wrapper::list::make_list(env, &self.items[base..]) };
        self.items.truncate(base);
        list
    }

    /// The map of the entries from `base` on.
    fn map(&mut self, base: usize) -> NIF_TERM {
        let entries = &mut self.entries[base..];
        // The VM insertion-sorts a flatmap's keys, which takes one comparison a key when they
        // come in order. Binaries' term order is their bytes'.
        if entries.len() <= FLATMAP_KEYS {
            entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        }
        self.keys.clear();
        self.values.clear();
        for &(_, key, value) in entries.iter() {
            self.keys.push(key);
            self.values.push(value);
        }
        self.entries.truncate(base);
        let env = self.env.as_c_arg();
        // SAFETY: the keys and values are terms made in `env`.
        unsafe { rustler::wrapper::map::make_map_from_arrays(env, &self.keys, &self.values) }
            .expect("keys of a map are unique")
    }
}

/// A key or a type, with its first eight bytes as a number, which settle most comparisons.
#[derive(Clone, Copy)]
struct Text<'v> {
    head: u64,
    text: &'v str,
}

impl<'v> Text<'v> {
    fn new(text: &'v str) -> Self {
        let bytes = text.as_bytes();
        let head = match bytes.first_chunk::<8>() {
            Some(first) => *first,
            None => {
                let mut head = [0; 8];
                head[..bytes.len()].copy_from_slice(bytes);
                head
            }
        };
        Text {
            head: u64::from_be_bytes(head),
            text,
        }
    }

    fn slot(self) -> usize {
        let hash = (self.head ^ self.text.len() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        (hash >> (u64::BITS - MADE_SLOTS.trailing_zeros())) as usize
    }
}

impl PartialEq for Text<'_> {
    /// The head holds a text of up to eight bytes whole, and the length tells its zero bytes
    /// from the head's padding.
    fn eq(&self, other: &Self) -> bool {
        self.head == other.head
            && self.text.len() == other.text.len()
            && self.text.get(8..) == other.text.get(8..)
    }
}

impl Eq for Text<'_> {}

/// Binaries' term order, which is their bytes'.
impl Ord for Text<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.head
            .cmp(&other.head)
            .then_with(|| self.text.cmp(other.text))
    }
}

impl PartialOrd for Text<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// A binary on the process heap, as `NewBinary` makes one. Encoding a `str` makes one off the
/// heap, which the VM copies onto it when it is short.
fn binary<'a>(env: Env<'a>, text: &str) -> Term<'a> {
    let mut binary = NewBinary::new(env, text.len());
    binary.as_mut_slice().copy_from_slice(text.as_bytes());
    binary.into()
}

/// JavaScript writes a number as an integer when it can, and Jason reads that as an integer,
/// however large.
fn encode_number<'a>(env: Env<'a>, number: &Number) -> Term<'a> {
    if let Some(integer) = number
        .as_i64()
        .filter(|integer| integer.unsigned_abs() as f64 <= tarnish::js::MAX_SAFE_INTEGER)
    {
        return integer.encode(env);
    }
    let double = number.as_f64().unwrap_or(f64::NAN);
    if !double.is_finite() {
        return atom::nil().encode(env);
    }
    let written = ryu_js::Buffer::new().format_finite(double).to_owned();
    if written.contains(['.', 'e']) {
        return double.encode(env);
    }
    match written.parse::<i64>() {
        Ok(integer) => integer.encode(env),
        Err(_) => written
            .parse::<BigInt>()
            .expect("JavaScript writes integers as digits")
            .encode(env),
    }
}

#[cfg(test)]
mod tests {
    use super::Text;

    /// Strings of up to 23 pieces of `alphabet` or U+FFFD, from a seeded generator.
    fn strings(alphabet: &[&str], count: usize) -> Vec<String> {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut below = move |bound: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % bound as u64) as usize
        };
        (0..count)
            .map(|_| {
                (0..below(24))
                    .map(|_| {
                        alphabet
                            .get(below(alphabet.len() + 1))
                            .copied()
                            .unwrap_or("\u{FFFD}")
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn texts_compare_as_their_bytes() {
        let texts = strings(&["a", "b", "\0", "é", "type"], 2_000);
        for (a, b) in texts
            .iter()
            .zip(texts.iter().rev())
            .chain(texts.iter().zip(&texts))
        {
            let (text_a, text_b) = (Text::new(a), Text::new(b));
            assert_eq!(text_a == text_b, a == b, "{a:?} {b:?}");
            assert_eq!(text_a.cmp(&text_b), a.cmp(b), "{a:?} {b:?}");
        }
    }
}
