//! Where the NIF's answer to a request first differs from the worker's, and how.

use std::fmt::{self, Display};
use std::mem::discriminant;

use tarnish::js::json::{stringify, write_string};
use tarnish::js::stack;
use tarnish::{Key, Map, Value};

use super::{Answered, Made, MadeUp, Span};
use crate::convert::Request;

/// A step into a JSON value: an object's key, or an array's index.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Segment {
    Key(Key),
    Index(usize),
}

/// The value at `path` in `value`.
pub fn at<'v>(value: &'v Value, path: &[Segment]) -> Option<&'v Value> {
    path.iter()
        .try_fold(value, |value, segment| match (segment, value) {
            (Segment::Key(key), Value::Object(map)) => map.get(key),
            (Segment::Index(index), Value::Array(items)) => items.get(*index),
            _ => None,
        })
}

/// How two answers differ where they first do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    /// Two values of one type that aren't equal. Numbers are equal by value, as `JSON.stringify`
    /// writes 2 and 2.0 alike; strings only when they're the same.
    Value,
    /// Two values of different types.
    Type,
    /// A key of the worker's object that the NIF's lacks.
    Missing,
    /// A key of the NIF's object that the worker's lacks.
    Extra,
    /// A key one side's object holds twice, as a JavaScript object can't, and as the NIF's term
    /// writer refuses to write.
    Duplicate(Side),
    /// Arrays of different lengths.
    Length,
    /// Values the application calls made up, which don't pair one to one with the other side's:
    /// one side has a value twice where the other has two.
    Unpaired,
    /// Both sides erred, with different messages.
    Error,
    /// The NIF erred where the worker answered.
    OursErrs,
    /// The worker erred where the NIF answered.
    TheirsErrs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Side {
    Theirs,
    Ours,
}

impl Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Side::Theirs => "js",
            Side::Ours => "ours",
        })
    }
}

impl Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Kind::Value => f.write_str("value"),
            Kind::Type => f.write_str("type"),
            Kind::Missing => f.write_str("missing key"),
            Kind::Extra => f.write_str("extra key"),
            Kind::Duplicate(side) => write!(f, "duplicate key in {side}"),
            Kind::Length => f.write_str("length"),
            Kind::Unpaired => f.write_str("made-up values not one to one"),
            Kind::Error => f.write_str("error message"),
            Kind::OursErrs => f.write_str("ours errs"),
            Kind::TheirsErrs => f.write_str("js errs"),
        }
    }
}

/// Where two answers first differ, and how.
#[derive(Debug, PartialEq)]
pub(crate) struct Difference {
    pub(crate) path: Vec<Segment>,
    pub(crate) kind: Kind,
}

impl Difference {
    /// What the difference is grouped with others by.
    pub(crate) fn group(&self, request: &Request) -> Group {
        Group {
            operation: match &request.operation {
                Value::String(operation) => operation.clone(),
                operation => stringify(operation),
            },
            pattern: self
                .path
                .iter()
                .map(|segment| match segment {
                    Segment::Key(key) => Some(key.clone()),
                    Segment::Index(_) => None,
                })
                .collect(),
            kind: self.kind,
        }
    }
}

/// Differences of one operation's answers at one place, any index standing for every index, of
/// one kind.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Group {
    operation: String,
    /// The path to the difference, with `None` for each index.
    pattern: Vec<Option<Key>>,
    kind: Kind,
}

impl Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} $", self.operation)?;
        for step in &self.pattern {
            match step {
                Some(key) => write_key(f, key)?,
                None => f.write_str("[*]")?,
            }
        }
        write!(f, " {}", self.kind)
    }
}

/// A path as `$.content[2].attrs["data-id"]`: `$` for the whole answer, then each key as
/// JavaScript would write a property access, so that no two paths are written alike.
struct Shown<'a>(&'a [Segment]);

impl Display for Shown<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("$")?;
        for segment in self.0 {
            match segment {
                Segment::Key(key) => write_key(f, key)?,
                Segment::Index(index) => write!(f, "[{index}]")?,
            }
        }
        Ok(())
    }
}

fn write_key(f: &mut fmt::Formatter, key: &str) -> fmt::Result {
    let mut chars = key.chars();
    let identifier = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || matches!(first, '_' | '$'))
        && chars.all(|rest| rest.is_ascii_alphanumeric() || matches!(rest, '_' | '$'));
    if identifier {
        return write!(f, ".{key}");
    }
    let mut quoted = String::new();
    write_string(&mut quoted, key);
    write!(f, "[{quoted}]")
}

/// Where the NIF's answer to `request` first differs from the worker's: the first difference in
/// an object's keys, then in its values in the order the worker's has them, and the first
/// difference in an array's length, then in its items. Values `made_up` accepts are the same where
/// every pair it accepts in the answers pairs one of the worker's values with one of the NIF's.
pub(crate) fn difference(
    made_up: MadeUp,
    request: &Request,
    theirs: &Answered,
    ours: &Answered,
) -> Option<Difference> {
    let kind = match (&theirs.result, &ours.result) {
        (Ok(their_answer), Ok(our_answer)) => {
            let mut comparison = Comparison {
                made_up,
                request,
                theirs: Whole {
                    answer: their_answer,
                    when: theirs.when.as_ref(),
                },
                ours: Whole {
                    answer: our_answer,
                    when: ours.when.as_ref(),
                },
                path: Vec::new(),
                pairs: Vec::new(),
            };
            let kind = comparison.values(their_answer, our_answer)?;
            return Some(Difference {
                path: comparison.path,
                kind,
            });
        }
        (Err(theirs), Err(ours)) if theirs == ours => return None,
        (Err(_), Err(_)) => Kind::Error,
        (Ok(_), Err(_)) => Kind::OursErrs,
        (Err(_), Ok(_)) => Kind::TheirsErrs,
    };
    Some(Difference {
        path: Vec::new(),
        kind,
    })
}

/// One side's whole answer, and when it was made.
struct Whole<'a> {
    answer: &'a Value,
    when: Option<&'a Span>,
}

impl<'a> Whole<'a> {
    fn made(&self, value: &'a Value) -> Made<'a> {
        Made {
            value,
            answer: self.answer,
            when: self.when,
        }
    }
}

struct Comparison<'a> {
    made_up: MadeUp,
    request: &'a Request,
    theirs: Whole<'a>,
    ours: Whole<'a>,
    /// Where the values compared are, and, once a difference is found, where it is.
    path: Vec<Segment>,
    /// The made-up values accepted so far, each of the worker's with the NIF's.
    pairs: Vec<(&'a Value, &'a Value)>,
}

impl<'a> Comparison<'a> {
    fn values(&mut self, theirs: &'a Value, ours: &'a Value) -> Option<Kind> {
        let kind = match (theirs, ours) {
            (Value::Object(theirs), Value::Object(ours)) => return self.objects(theirs, ours),
            (Value::Array(theirs), Value::Array(ours)) => return self.arrays(theirs, ours),
            (Value::Null, Value::Null) => return None,
            // The NIF writes a number that isn't finite as `null`, as the worker's JSON has it.
            (Value::Null, Value::Number(number)) if !number.as_f64().is_finite() => return None,
            (Value::Bool(a), Value::Bool(b)) if a == b => return None,
            (Value::Number(a), Value::Number(b)) if a == b => return None,
            (Value::String(a), Value::String(b)) if a == b => return None,
            _ if discriminant(theirs) == discriminant(ours) => Kind::Value,
            _ => Kind::Type,
        };
        let made_up = (self.made_up)(
            self.request,
            &self.path,
            self.theirs.made(theirs),
            self.ours.made(ours),
        );
        if !made_up {
            return Some(kind);
        }
        self.pair(theirs, ours)
    }

    /// Holds two made-up values as a pair, unless an earlier pair holds one of them with another.
    fn pair(&mut self, theirs: &'a Value, ours: &'a Value) -> Option<Kind> {
        for &(paired_theirs, paired_ours) in &self.pairs {
            match (paired_theirs == theirs, paired_ours == ours) {
                (true, true) => return None,
                (false, false) => {}
                _ => return Some(Kind::Unpaired),
            }
        }
        self.pairs.push((theirs, ours));
        None
    }

    fn objects(&mut self, theirs: &'a Map, ours: &'a Map) -> Option<Kind> {
        if let Some((kind, key)) = differing_key(theirs, ours) {
            self.path.push(Segment::Key(key.clone()));
            return Some(kind);
        }
        for (key, theirs) in theirs {
            let ours = ours.get(key).expect("a key both objects have");
            self.path.push(Segment::Key(key.clone()));
            if let Some(kind) = stack::grow(|| self.values(theirs, ours)) {
                return Some(kind);
            }
            self.path.pop();
        }
        None
    }

    fn arrays(&mut self, theirs: &'a [Value], ours: &'a [Value]) -> Option<Kind> {
        if theirs.len() != ours.len() {
            return Some(Kind::Length);
        }
        for (index, (theirs, ours)) in theirs.iter().zip(ours).enumerate() {
            self.path.push(Segment::Index(index));
            if let Some(kind) = stack::grow(|| self.values(theirs, ours)) {
                return Some(kind);
            }
            self.path.pop();
        }
        None
    }
}

/// The first key where two objects' keys differ, and how.
fn differing_key<'k>(theirs: &'k Map, ours: &'k Map) -> Option<(Kind, &'k Key)> {
    if let Some(key) = duplicate_key(|| theirs.keys()) {
        return Some((Kind::Duplicate(Side::Theirs), key));
    }
    if let Some(key) = duplicate_key(|| ours.keys()) {
        return Some((Kind::Duplicate(Side::Ours), key));
    }
    if let Some(key) = theirs.keys().find(|key| !ours.contains_key(key)) {
        return Some((Kind::Missing, key));
    }
    let key = ours.keys().find(|key| !theirs.contains_key(key))?;
    Some((Kind::Extra, key))
}

/// The first of an object's keys that an earlier one is.
fn duplicate_key<'k, K: Iterator<Item = &'k Key>>(keys: impl Fn() -> K) -> Option<&'k Key> {
    keys()
        .enumerate()
        .find(|&(index, key)| keys().take(index).any(|earlier| earlier == key))
        .map(|(_, key)| key)
}

/// A difference as replay and reduce print it: where it is, and what each side has there.
pub(crate) fn describe(
    difference: &Difference,
    theirs: &Result<Value, String>,
    ours: &Result<Value, String>,
) -> String {
    let (theirs, ours) = match (theirs, ours) {
        (Ok(theirs), Ok(ours)) => (at(theirs, &difference.path), at(ours, &difference.path)),
        (Err(theirs), Err(ours)) => return format!("errors {}", texts(theirs, ours)),
        (Ok(theirs), Err(ours)) => {
            return format!("js {} ours errs {}", brief(theirs), quoted(ours));
        }
        (Err(theirs), Ok(ours)) => {
            return format!("js errs {} ours {}", quoted(theirs), brief(ours));
        }
    };
    let shown = match (difference.kind, theirs, ours) {
        (Kind::Duplicate(side), _, _) => format!("{side} has the key twice"),
        (_, Some(Value::String(theirs)), Some(Value::String(ours))) => texts(theirs, ours),
        (Kind::Length, Some(Value::Array(theirs)), Some(Value::Array(ours))) => {
            format!("js has {} items, ours {}", theirs.len(), ours.len())
        }
        (_, Some(theirs), None) => format!("js {}, ours has no such key", brief(theirs)),
        (_, None, Some(ours)) => format!("ours {}, js has no such key", brief(ours)),
        (_, Some(theirs), Some(ours)) => format!("js {} ours {}", brief(theirs), brief(ours)),
        (_, None, None) => "neither has it".into(),
    };
    format!("{}: {shown}", Shown(&difference.path))
}

/// How many characters of a text are shown before where it first differs from another, and how
/// many from there.
const BEFORE: usize = 40;
const FROM: usize = 80;

/// Two texts at the first character where they differ, each shown around it.
fn texts(theirs: &str, ours: &str) -> String {
    let offset = theirs
        .chars()
        .zip(ours.chars())
        .take_while(|(theirs, ours)| theirs == ours)
        .count();
    format!(
        "first differ at character {offset} of js's {} and ours' {}\n            js   {}\n            ours {}",
        theirs.chars().count(),
        ours.chars().count(),
        around(theirs, offset),
        around(ours, offset),
    )
}

/// The characters of `text` from `BEFORE` before `offset` to `FROM` from it, as a JSON string,
/// with `…` where the text goes on.
fn around(text: &str, offset: usize) -> String {
    let start = offset.saturating_sub(BEFORE);
    let mut rest = text.chars().skip(start);
    let shown: String = rest.by_ref().take(offset - start + FROM).collect();
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        quoted(&shown),
        if rest.next().is_some() { "…" } else { "" },
    )
}

fn quoted(text: &str) -> String {
    let mut quoted = String::new();
    write_string(&mut quoted, text);
    quoted
}

/// A value's JSON, cut short if it's long.
fn brief(value: &Value) -> String {
    truncated(&stringify(value), 200)
}

/// The first `length` characters of `text`, with `…` if it goes on.
pub(crate) fn truncated(text: &str, length: usize) -> String {
    match text.char_indices().nth(length) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tarnish::js::json::{from_str, json};

    fn exact(_: &Request, _: &[Segment], _: Made, _: Made) -> bool {
        false
    }

    fn request(operation: &str) -> Request {
        Request {
            operation: operation.into(),
            input: Value::Null,
            options: None,
        }
    }

    fn answered(result: Result<Value, String>) -> Answered {
        Answered { result, when: None }
    }

    fn compared(made_up: MadeUp, theirs: Value, ours: Value) -> Option<Difference> {
        difference(
            made_up,
            &request("op"),
            &answered(Ok(theirs)),
            &answered(Ok(ours)),
        )
    }

    /// The difference's path and group as replay prints them.
    fn found(theirs: Value, ours: Value) -> Option<(String, String)> {
        compared(exact, theirs, ours).map(|difference| {
            (
                Shown(&difference.path).to_string(),
                difference.group(&request("op")).to_string(),
            )
        })
    }

    fn shown(path: &str, group: &str) -> Option<(String, String)> {
        Some((path.into(), group.into()))
    }

    #[test]
    fn finds_where_answers_first_differ() {
        assert_eq!(
            found(json!({"a": 1, "b": 2}), json!({"b": 2, "a": 1})),
            None
        );
        assert_eq!(
            found(json!({"a": [1, {"b": 2}]}), json!({"a": [1, {"b": 3}]})),
            shown("$.a[1].b", "op $.a[*].b value")
        );
        assert_eq!(
            found(json!({"a": 1}), json!({"a": 1, "b": 2})),
            shown("$.b", "op $.b extra key")
        );
        assert_eq!(
            found(json!({"a": 1}), json!({})),
            shown("$.a", "op $.a missing key")
        );
        assert_eq!(
            found(json!({"a": [1]}), json!({"a": [2, 1]})),
            shown("$.a", "op $.a length")
        );
        assert_eq!(
            found(json!({"a": "1"}), json!({"a": 1})),
            shown("$.a", "op $.a type")
        );
        assert_eq!(found(json!("x"), json!("y")), shown("$", "op $ value"));
    }

    #[test]
    fn groups_by_keys_whatever_their_characters() {
        assert_eq!(
            found(json!({"h1": 1, "h2": 1}), json!({"h1": 2, "h2": 1})),
            shown("$.h1", "op $.h1 value")
        );
        assert_eq!(
            found(json!({"h1": 1, "h2": 1}), json!({"h1": 1, "h2": 2})),
            shown("$.h2", "op $.h2 value")
        );
        assert_eq!(
            found(json!({"a.b": 1}), json!({"a.b": 2})),
            shown(r#"$["a.b"]"#, r#"op $["a.b"] value"#)
        );
        assert_eq!(
            found(json!({"a": {"b": 1}}), json!({"a": {"b": 2}})),
            shown("$.a.b", "op $.a.b value")
        );
        assert_eq!(
            found(json!({"0": [1]}), json!({"0": [2]})),
            shown(r#"$["0"][0]"#, r#"op $["0"][*] value"#)
        );
        let group = |operation: &str| {
            compared(exact, json!(1), json!(2))
                .unwrap()
                .group(&request(operation))
        };
        assert_ne!(group("parseHTML"), group("parseMarkdown"));
    }

    #[test]
    fn compares_numbers_by_value() {
        assert_eq!(found(json!([2]), json!([2.0])), None);
        assert_eq!(found(json!([0]), json!([-0.0])), None);
        assert_eq!(
            found(json!([2]), json!([2.5])),
            shown("$[0]", "op $[*] value")
        );
    }

    /// The worker's JSON has `null` for a number that isn't finite, as the NIF writes it.
    #[test]
    fn compares_a_number_that_isnt_finite_as_null() {
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(found(json!([null]), json!([number])), None);
        }
        assert_eq!(
            found(json!([null]), json!([0])),
            shown("$[0]", "op $[*] type")
        );
    }

    #[test]
    fn groups_errors_by_which_side_errs() {
        let differs = |theirs: Result<Value, String>, ours: Result<Value, String>| {
            difference(exact, &request("op"), &answered(theirs), &answered(ours))
                .map(|difference| difference.group(&request("op")).to_string())
        };
        assert_eq!(differs(Err("x".into()), Err("x".into())), None);
        assert_eq!(
            differs(Err("x".into()), Err("y".into())),
            Some("op $ error message".into())
        );
        assert_eq!(
            differs(Ok(Value::Null), Err("y".into())),
            Some("op $ ours errs".into())
        );
        assert_eq!(
            differs(Err("x".into()), Ok(Value::Null)),
            Some("op $ js errs".into())
        );
    }

    /// Takes two strings starting `new` as made up, as a card's `id`.
    fn new_ids(_: &Request, path: &[Segment], theirs: Made, ours: Made) -> bool {
        let Some((Segment::Key(key), node)) = path.split_last() else {
            return false;
        };
        key == "id"
            && [theirs, ours].iter().all(|made| {
                at(made.answer, node).is_some_and(|node| node["type"] == "card")
                    && made.value.as_str().is_some_and(|id| id.starts_with("new"))
            })
    }

    #[test]
    fn takes_made_up_values_paired_one_to_one_as_the_same() {
        let found = |theirs, ours| {
            compared(new_ids, theirs, ours)
                .map(|difference| (Shown(&difference.path).to_string(), difference.kind))
        };
        let cards = |ids: [&str; 3]| {
            json!([
                {"type": "card", "id": ids[0]},
                {"type": "card", "id": ids[1]},
                {"type": "card", "id": ids[2]},
            ])
        };
        assert_eq!(
            found(
                cards(["new x", "new y", "new x"]),
                cards(["new z", "new w", "new z"])
            ),
            None
        );
        assert_eq!(
            found(
                cards(["new x", "new y", "old"]),
                cards(["new z", "new z", "old"])
            ),
            Some(("$[1].id".into(), Kind::Unpaired))
        );
        assert_eq!(
            found(
                cards(["new x", "new x", "old"]),
                cards(["new z", "new w", "old"])
            ),
            Some(("$[1].id".into(), Kind::Unpaired))
        );
        assert_eq!(
            found(
                cards(["new x", "new y", "old"]),
                cards(["new z", "new w", "older"])
            ),
            Some(("$[2].id".into(), Kind::Value))
        );
        assert_eq!(
            found(
                json!({"type": "card", "id": "new x", "title": "new y"}),
                json!({"type": "card", "id": "new z", "title": "new w"}),
            ),
            Some(("$.title".into(), Kind::Value))
        );
        assert_eq!(
            found(
                json!([{"type": "paragraph", "id": "new x"}]),
                json!([{"type": "paragraph", "id": "new z"}]),
            ),
            Some(("$[0].id".into(), Kind::Value))
        );
    }

    #[test]
    fn compares_values_as_deep_as_they_nest() {
        const DEPTH: usize = 100_000;
        stack::on_dirty_scheduler_stack(|| {
            let nested = |leaf: &str| {
                from_str(&format!("{}{leaf}{}", "[".repeat(DEPTH), "]".repeat(DEPTH))).unwrap()
            };
            let (theirs, ours) = (answered(Ok(nested("1"))), answered(Ok(nested("2"))));
            let found = difference(exact, &request("op"), &theirs, &ours).unwrap();
            assert!(found.kind == Kind::Value);
            assert!(found.path == vec![Segment::Index(0); DEPTH]);
            let shown = describe(&found, &theirs.result, &ours.result);
            assert!(shown.ends_with("[0][0]: js 1 ours 2"));
        });
    }

    #[test]
    fn shows_where_texts_first_differ_on_both_sides() {
        let text =
            |differing: &str| format!("{}<p>{differing}</p>{}", "x".repeat(100), "y".repeat(100));
        let at_root = Difference {
            path: Vec::new(),
            kind: Kind::Value,
        };
        let around = |differing: &str| {
            format!(
                "…\"{}<p>{differing}</p>{}\"…",
                "x".repeat(37),
                "y".repeat(75)
            )
        };
        assert_eq!(
            describe(&at_root, &Ok(json!(text("a"))), &Ok(json!(text("b")))),
            format!(
                "$: first differ at character 103 of js's 208 and ours' 208\n            js   {}\n            ours {}",
                around("a"),
                around("b")
            )
        );
        let errors = |kind, theirs: Result<Value, String>, ours: Result<Value, String>| {
            describe(
                &Difference {
                    path: Vec::new(),
                    kind,
                },
                &theirs,
                &ours,
            )
        };
        assert_eq!(
            errors(Kind::Error, Err("bad x".into()), Err("bad y\n".into())),
            "errors first differ at character 4 of js's 5 and ours' 6\n            js   \"bad x\"\n            ours \"bad y\\n\""
        );
        assert_eq!(
            errors(Kind::TheirsErrs, Err("bad".into()), Ok(json!({"a": 1}))),
            r#"js errs "bad" ours {"a":1}"#
        );
        assert_eq!(
            errors(Kind::OursErrs, Ok(json!([1])), Err("bad".into())),
            r#"js [1] ours errs "bad""#
        );
    }

    #[test]
    fn finds_a_key_held_twice() {
        let keys = |keys: &[&str]| keys.iter().map(|&key| Key::from(key)).collect::<Vec<_>>();
        let distinct = keys(&["a", "b", "c"]);
        assert_eq!(duplicate_key(|| distinct.iter()), None);
        let twice = keys(&["a", "b", "c", "b", "a"]);
        assert_eq!(duplicate_key(|| twice.iter()), Some(&Key::from("b")));
    }

    /// A map holds a key twice only where `Map::push` doesn't check, in a build without debug
    /// assertions, as the NIF's conversions are when replay runs them.
    #[test]
    #[cfg_attr(debug_assertions, ignore = "Map::push refuses a key twice")]
    fn finds_a_key_an_answer_holds_twice() {
        let twice = |value: Value| {
            let mut map = Map::new();
            map.push("a".into(), json!(1));
            map.push("b".into(), value.clone());
            map.push("b".into(), value);
            Value::Object(map)
        };
        let found = |theirs, ours| {
            compared(exact, theirs, ours).map(|difference| {
                (
                    Shown(&difference.path).to_string(),
                    difference.kind.to_string(),
                )
            })
        };
        let at_b = |kind: &str| Some(("$.b".to_string(), kind.to_string()));
        assert_eq!(
            found(json!({"a": 1, "b": 2}), twice(json!(2))),
            at_b("duplicate key in ours")
        );
        assert_eq!(
            found(twice(json!(2)), json!({"a": 1, "b": 2})),
            at_b("duplicate key in js")
        );
        assert_eq!(
            found(json!({"a": 1, "b": 2, "c": 3}), twice(json!(2))),
            at_b("duplicate key in ours")
        );
    }
}
