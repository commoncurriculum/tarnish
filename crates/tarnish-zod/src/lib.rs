//! The parts of zod 4 that attribute schemas use, with zod's output (unknown object
//! keys dropped, keys in the schema's order) and zod's errors (every issue, as `ZodError`'s
//! message writes them).

use std::borrow::Cow;

use tarnish_js::json::{self, Map, Value, array_index, json};
use tarnish_js::{Class, Error, MAX_SAFE_INTEGER};

/// The version of zod this crate ports.
pub const ZOD: &str = "4.5.4";

#[derive(Clone, Debug, PartialEq)]
pub enum Schema {
    String,
    /// `z.number()`, which rejects non-finite numbers.
    Number,
    /// `z.int()`: a safe integer.
    Int,
    Boolean,
    Enum(&'static [&'static str]),
    Array(Box<Schema>),
    Object(Vec<(&'static str, Schema)>),
    /// `z.record(z.string(), value)`.
    Record(Box<Schema>),
    Nullable(Box<Schema>),
    Optional(Box<Schema>),
    Default(Box<Schema>, Value),
    /// `.catch(value)`; `None` catches to `undefined`.
    Catch(Box<Schema>, Option<Value>),
    /// `z.strictObject(shape)`, for which a key the shape lacks is an issue.
    StrictObject(Vec<(&'static str, Schema)>),
}

/// A zod issue: its fields before `path` and `message`, in zod's order.
#[derive(Debug)]
pub struct Issue {
    fields: Vec<(&'static str, Value)>,
    path: Vec<Value>,
    message: String,
}

/// Every issue of a failed parse. Its `Display` is `ZodError.message`.
#[derive(Debug)]
pub struct Issues(Vec<Issue>);

impl Issues {
    /// `error.issues[0].message`.
    pub fn first_message(&self) -> &str {
        &self.0[0].message
    }
}

impl std::fmt::Display for Issues {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        let issues = self
            .0
            .iter()
            .map(|issue| {
                let mut object = Map::new();
                for (key, value) in &issue.fields {
                    object.insert((*key).into(), value.clone());
                }
                object.insert("path".into(), Value::Array(issue.path.clone()));
                object.insert("message".into(), Value::String(issue.message.clone()));
                Value::Object(object)
            })
            .collect();
        formatter.write_str(&json::stringify_pretty(&Value::Array(issues)))
    }
}

pub type Parsed = Result<Option<Value>, Issues>;

/// zod's `ZodError`, whose `toString` gives its message alone.
pub static ZOD_ERROR: Class = Class {
    name: "ZodError",
    message_alone: true,
};

/// The `ZodError` a throwing parse throws.
impl From<Issues> for Error {
    fn from(issues: Issues) -> Self {
        Error::Of(&ZOD_ERROR, issues.to_string())
    }
}

impl Schema {
    pub fn array(item: Schema) -> Schema {
        Schema::Array(Box::new(item))
    }

    pub fn record(value: Schema) -> Schema {
        Schema::Record(Box::new(value))
    }

    pub fn nullable(self) -> Schema {
        Schema::Nullable(Box::new(self))
    }

    pub fn optional(self) -> Schema {
        Schema::Optional(Box::new(self))
    }

    pub fn nullish(self) -> Schema {
        self.nullable().optional()
    }

    pub fn default(self, value: Value) -> Schema {
        Schema::Default(Box::new(self), value)
    }

    pub fn catch(self, value: Option<Value>) -> Schema {
        Schema::Catch(Box::new(self), value)
    }

    /// `schema.strict()`, for an object schema. Only `parse` reads it: the other parses read the
    /// object by key.
    pub fn strict(self) -> Schema {
        match self {
            Schema::Object(shape) => Schema::StrictObject(shape),
            _ => panic!("only an object schema is strict"),
        }
    }

    /// `schema.shape[key]`, for an object schema.
    pub fn field(&self, key: &str) -> &Schema {
        &self.entry(key).1
    }

    /// `schema.pick(mask)`, for an object schema: the mask's fields, in the mask's order.
    pub fn pick(&self, mask: &[&str]) -> Schema {
        self.reshaped(mask.iter().map(|key| self.entry(key).clone()).collect())
    }

    /// `schema.omit(mask)`, for an object schema: the other fields, in the shape's order.
    pub fn omit(&self, mask: &[&str]) -> Schema {
        // Only for the panic: zod throws for a key the shape lacks.
        for key in mask {
            self.entry(key);
        }
        let kept = self.shape().iter().filter(|(key, _)| !mask.contains(key));
        self.reshaped(kept.cloned().collect())
    }

    /// `schema.partial()`, for an object schema: each field `.optional()`.
    pub fn partial(&self) -> Schema {
        let fields = self.shape().iter();
        self.reshaped(
            fields
                .map(|(key, field)| (*key, field.clone().optional()))
                .collect(),
        )
    }

    /// `schema.extend(shape)`, for an object schema: `{ ...schema.shape, ...shape }`, where a
    /// key of both keeps its place with `shape`'s field.
    pub fn extend(&self, shape: impl IntoIterator<Item = (&'static str, Schema)>) -> Schema {
        let mut fields = self.shape().to_vec();
        for (key, field) in shape {
            match fields.iter_mut().find(|(name, _)| *name == key) {
                Some((_, replaced)) => *replaced = field,
                None => fields.push((key, field)),
            }
        }
        self.reshaped(fields)
    }

    /// An object schema with `shape`, strict where this one is, as zod's object methods keep
    /// the rest of the schema they're called on.
    fn reshaped(&self, shape: Vec<(&'static str, Schema)>) -> Schema {
        match self {
            Schema::StrictObject(_) => Schema::StrictObject(shape),
            _ => Schema::Object(shape),
        }
    }

    /// Whether the schema puts a value in place of `undefined`, as `.default()` does, which zod
    /// calls an `optin` of `"defaulted"`: `.optional()` and `.catch()` keep it from the schema
    /// they wrap.
    fn defaults(&self) -> bool {
        match self {
            Schema::Default(..) => true,
            Schema::Optional(inner) | Schema::Nullable(inner) | Schema::Catch(inner, _) => {
                inner.defaults()
            }
            _ => false,
        }
    }

    /// `schema.unwrap()` of the schemas that wrap one: optional, nullable, default and catch.
    pub fn unwrap(&self) -> Option<&Schema> {
        match self {
            Schema::Optional(inner)
            | Schema::Nullable(inner)
            | Schema::Default(inner, _)
            | Schema::Catch(inner, _) => Some(inner),
            _ => None,
        }
    }

    /// `schema.shape`, for an object schema: its fields in order.
    pub fn shape(&self) -> &[(&'static str, Schema)] {
        match self {
            Schema::Object(shape) | Schema::StrictObject(shape) => shape,
            _ => panic!("only an object schema has a shape"),
        }
    }

    /// The shape's entry for `key`, with zod's error where the shape has none.
    fn entry(&self, key: &str) -> &(&'static str, Schema) {
        self.shape()
            .iter()
            .find(|(name, _)| *name == key)
            .unwrap_or_else(|| panic!("Unrecognized key: \"{key}\""))
    }

    /// `schema.parse(value)` where `None` is `undefined`. `Ok(None)` is an `undefined` result.
    pub fn parse(&self, value: Option<&Value>) -> Parsed {
        let parsed = collecting(|issues| self.run(value.into(), &Path::Root, issues))?;
        Ok(parsed.map(Cow::into_owned))
    }

    /// `schema.parse(value)` for an object schema.
    pub fn parse_fields<'v>(&'v self, value: Option<&'v Value>) -> Result<Fields<'v>, Issues> {
        collecting(|issues| self.fields(value, issues))
    }

    /// `schema.parse(object)` for an object schema, the object's properties read by key: an
    /// object made to be parsed, as `{ ...data, key: value }` is, needn't be made.
    pub fn parse_properties<'v>(
        &'v self,
        property: impl FnMut(&str) -> Option<&'v Value>,
    ) -> Result<Fields<'v>, Issues> {
        collecting(|issues| self.properties(property, issues))
    }

    /// `schema.safeParse(object).success ? data : undefined`, as `parse_properties` reads it.
    pub fn safe_parse_properties<'v>(
        &'v self,
        property: impl FnMut(&str) -> Option<&'v Value>,
    ) -> Option<Fields<'v>> {
        let mut issues = Sink::Count(0);
        let fields = self.properties(property, &mut issues);
        issues.is_empty().then_some(fields)
    }

    /// `const result = schema.safeParse(object); result.success ? result.data :
    /// schema.parse(fallback)`, both objects' properties read by key.
    pub fn parse_properties_or<'v>(
        &'v self,
        property: impl FnMut(&str) -> Option<&'v Value>,
        fallback: impl FnMut(&str) -> Option<&'v Value>,
    ) -> Result<Fields<'v>, Issues> {
        match self.safe_parse_properties(property) {
            Some(fields) => Ok(fields),
            None => self.parse_properties(fallback),
        }
    }

    /// An object schema's fields parsed from `value`, which mean nothing if it raised an issue.
    fn fields<'v>(&'v self, value: Option<&'v Value>, issues: &mut Sink) -> Fields<'v> {
        match value {
            Some(Value::Object(entries)) => self.properties(map_properties(entries), issues),
            _ => {
                type_issue(value.into(), "object", &Path::Root, issues);
                Fields(Vec::new())
            }
        }
    }

    /// An object schema's fields parsed from the object `property` reads.
    fn properties<'v>(
        &'v self,
        property: impl FnMut(&str) -> Option<&'v Value>,
        issues: &mut Sink,
    ) -> Fields<'v> {
        let Schema::Object(shape) = self else {
            panic!("fields are parsed by an object schema");
        };
        let mut fields = Vec::with_capacity(shape.len());
        object_entries(shape, property, &Path::Root, issues, |key, value| {
            fields.push((key, value))
        });
        Fields(fields)
    }

    /// The value parsed from `input`: the value itself wherever it passes as it is.
    fn run<'v>(
        &'v self,
        input: Input<'v>,
        path: &Path,
        issues: &mut Sink,
    ) -> Option<Cow<'v, Value>> {
        match self {
            Schema::Optional(inner) => match input {
                // `undefined` passes as it is, unless the schema wrapped puts a value in its
                // place, which it does here as alone, where an issue it raises is dropped.
                Input::Undefined if inner.defaults() => {
                    let mut inner_issues = Sink::Count(0);
                    let parsed = inner.run(input, path, &mut inner_issues);
                    parsed.filter(|_| inner_issues.is_empty())
                }
                Input::Undefined => None,
                _ => inner.run(input, path, issues),
            },
            Schema::Nullable(inner) => match input {
                Input::Value(null @ Value::Null) => Some(Cow::Borrowed(null)),
                _ => inner.run(input, path, issues),
            },
            // The default stands for `undefined`, given or parsed.
            Schema::Default(inner, default) => match input {
                Input::Undefined => Some(Cow::Borrowed(default)),
                _ => inner
                    .run(input, path, issues)
                    .or(Some(Cow::Borrowed(default))),
            },
            Schema::Catch(inner, caught) => {
                let mut inner_issues = Sink::Count(0);
                let parsed = inner.run(input, path, &mut inner_issues);
                if inner_issues.is_empty() {
                    parsed
                } else {
                    caught.as_ref().map(Cow::Borrowed)
                }
            }
            Schema::String => check_type(input, "string", Value::is_string, path, issues),
            Schema::Boolean => check_type(input, "boolean", Value::is_boolean, path, issues),
            Schema::Number => match input.value() {
                Some(value @ Value::Number(_)) => Some(Cow::Borrowed(value)),
                _ => type_issue(input, "number", path, issues),
            },
            Schema::Int => match input.value().and_then(Value::as_f64) {
                None => type_issue(input, "number", path, issues),
                Some(number) if number.fract() != 0.0 => {
                    issues.add(|| Issue {
                        fields: vec![
                            ("expected", json!("int")),
                            ("format", json!("safeint")),
                            ("code", json!("invalid_type")),
                        ],
                        path: path_values(path),
                        message: "Invalid input: expected int, received number".into(),
                    });
                    None
                }
                Some(number) if number > MAX_SAFE_INTEGER => {
                    bound_issue("too_big", "maximum", "<=", 1.0, path, issues)
                }
                Some(number) if number < -MAX_SAFE_INTEGER => {
                    bound_issue("too_small", "minimum", ">=", -1.0, path, issues)
                }
                Some(_) => input.value().map(Cow::Borrowed),
            },
            Schema::Enum(options) => match input.value() {
                Some(value @ Value::String(string)) if options.contains(&string.as_str()) => {
                    Some(Cow::Borrowed(value))
                }
                _ => {
                    issues.add(|| enum_issue(options, path));
                    None
                }
            },
            Schema::Array(item) => {
                let Some(array @ Value::Array(items)) = input.value() else {
                    return type_issue(input, "array", path, issues);
                };
                // A copy of the items, made from the first one that parses to something else.
                let mut parsed: Option<Vec<Value>> = None;
                for (index, entry) in items.iter().enumerate() {
                    let path = path.join(Segment::Index(index));
                    let result = item.run(Input::Value(entry), &path, issues);
                    if parsed.is_none() && is_itself(&result, entry) {
                        continue;
                    }
                    parsed
                        .get_or_insert_with(|| items[..index].to_vec())
                        .push(result.map_or(Value::Null, Cow::into_owned));
                }
                Some(parsed.map_or(Cow::Borrowed(array), |parsed| {
                    Cow::Owned(Value::Array(parsed))
                }))
            }
            Schema::Object(shape) | Schema::StrictObject(shape) => {
                let Some(Value::Object(entries)) = input.value() else {
                    return type_issue(input, "object", path, issues);
                };
                let mut object = Map::with_capacity(shape.len());
                // The shape's keys are distinct.
                object_entries(
                    shape,
                    map_properties(entries),
                    path,
                    issues,
                    |key, value| object.push(key.into(), value.into_owned()),
                );
                if let Schema::StrictObject(_) = self {
                    let unrecognized: Vec<&str> = entries
                        .keys()
                        .map(|key| key.as_ref())
                        .filter(|key| shape.iter().all(|(name, _)| name != key))
                        .collect();
                    if !unrecognized.is_empty() {
                        issues.add(|| unrecognized_keys(&unrecognized, path));
                    }
                }
                Some(Cow::Owned(Value::Object(object)))
            }
            Schema::Record(item) => {
                let Some(record @ Value::Object(entries)) = input.value() else {
                    return type_issue(input, "record", path, issues);
                };
                // A copy of the entries, made from the first one that parses to something else
                // or that a record leaves out.
                let mut parsed: Option<Map> = None;
                for (index, (key, entry)) in entries.iter().enumerate() {
                    let result = match key.as_str() {
                        PROTO => None,
                        _ => item.run(Input::Value(entry), &path.join(Segment::Key(key)), issues),
                    };
                    if parsed.is_none() && key != PROTO && is_itself(&result, entry) {
                        continue;
                    }
                    let parsed = parsed.get_or_insert_with(|| {
                        let before = entries.iter().take(index);
                        before
                            .map(|(key, entry)| (key.clone(), entry.clone()))
                            .collect()
                    });
                    if let Some(result) = result {
                        parsed.push(key.clone(), result.into_owned());
                    }
                }
                Some(parsed.map_or(Cow::Borrowed(record), |parsed| {
                    Cow::Owned(Value::Object(parsed))
                }))
            }
        }
    }
}

/// What JavaScript reads where a parse reads a value: `undefined`, a JSON value, or, for a
/// property an object doesn't have of its own, the function every object inherits from
/// `Object.prototype` under that name.
#[derive(Clone, Copy)]
enum Input<'v> {
    Undefined,
    Value(&'v Value),
    Inherited,
}

impl<'v> Input<'v> {
    fn value(self) -> Option<&'v Value> {
        match self {
            Input::Value(value) => Some(value),
            Input::Undefined | Input::Inherited => None,
        }
    }

    /// The property `key` of an object, which has `own` of its own under that name.
    fn property(key: &str, own: Option<&'v Value>) -> Input<'v> {
        match own {
            Some(value) => Input::Value(value),
            None if INHERITED.contains(&key) => Input::Inherited,
            None => Input::Undefined,
        }
    }
}

impl<'v> From<Option<&'v Value>> for Input<'v> {
    fn from(value: Option<&'v Value>) -> Input<'v> {
        value.map_or(Input::Undefined, Input::Value)
    }
}

/// The key under which a JavaScript object's prototype is, which neither an object schema nor a
/// record parses: zod leaves it out of what it makes.
const PROTO: &str = "__proto__";

/// The functions every JavaScript object inherits from `Object.prototype`, by name.
const INHERITED: &[&str] = &[
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// An object schema's fields parsed from the object `property` reads, in the order a JavaScript
/// object holds the shape's keys, without the keys it doesn't have.
fn object_entries<'v>(
    shape: &'v [(&'static str, Schema)],
    mut property: impl FnMut(&str) -> Option<&'v Value>,
    path: &Path,
    issues: &mut Sink,
    mut field_parsed: impl FnMut(&'static str, Cow<'v, Value>),
) {
    for (key, field) in in_js_order(shape) {
        if *key == PROTO {
            continue;
        }
        let input = Input::property(key, property(key));
        if let Some(result) = field.run(input, &path.join(Segment::Key(key)), issues) {
            field_parsed(key, result);
        }
    }
}

/// The shape's entries in the order a JavaScript object holds keys: array indices first,
/// ascending, then the rest in their order.
fn in_js_order<'s>(
    shape: &'s [(&'static str, Schema)],
) -> impl Iterator<Item = &'s (&'static str, Schema)> {
    let mut indices: Vec<_> = shape
        .iter()
        .filter_map(|entry| Some((array_index(entry.0)?, entry)))
        .collect();
    indices.sort_by_key(|(index, _)| *index);
    let rest = shape.iter().filter(|(key, _)| array_index(key).is_none());
    indices.into_iter().map(|(_, entry)| entry).chain(rest)
}

/// The properties of an object's `entries`, as a shape reads them. The shape's keys are
/// usually in the order the entries are, so each key is looked for where the last one was
/// found first. Most shapes have optional keys an object lacks, which a lookup would compare
/// with every entry: a key none of the entries shares a length and first byte with isn't
/// looked up.
fn map_properties<'v>(entries: &'v Map) -> impl FnMut(&str) -> Option<&'v Value> + 'v {
    fn sketch(key: &str) -> u64 {
        let first = key.as_bytes().first().copied().unwrap_or(0);
        1 << ((key.len() as u32 * 7 + u32::from(first)) % u64::BITS)
    }
    let sketches = entries
        .keys()
        .fold(0, |sketches, key| sketches | sketch(key));
    let mut next = 0;
    move |key| {
        if sketches & sketch(key) == 0 {
            None
        } else {
            entries.get_from(key, &mut next)
        }
    }
}

/// Whether a child parsed to `entry` itself, as one that passes as it is does.
fn is_itself(parsed: &Option<Cow<Value>>, entry: &Value) -> bool {
    matches!(parsed, Some(Cow::Borrowed(same)) if std::ptr::eq(*same, entry))
}

/// An object schema's parse: its fields in the shape's order, each borrowed from the value
/// parsed where it passes as it is.
pub struct Fields<'v>(Vec<(&'static str, Cow<'v, Value>)>);

impl<'v> Fields<'v> {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(field, _)| *field == key)
            .map(|(_, value)| value.as_ref())
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = (&'static str, &'a Value)> + 'a {
        self.0.iter().map(|(key, value)| (*key, value.as_ref()))
    }

    pub fn into_map(self) -> Map {
        let mut map = Map::with_capacity(self.0.len());
        for (key, value) in self.0 {
            // The shape's keys are distinct.
            map.push(key.into(), value.into_owned());
        }
        map
    }
}

impl std::ops::Index<&str> for Fields<'_> {
    type Output = Value;

    fn index(&self, key: &str) -> &Value {
        self.get(key).expect("no field for key")
    }
}

/// `run`'s result, or the issues it raised.
fn collecting<T>(run: impl FnOnce(&mut Sink) -> T) -> Result<T, Issues> {
    let mut sink = Sink::Collect(Vec::new());
    let result = run(&mut sink);
    match sink {
        Sink::Collect(issues) if !issues.is_empty() => Err(Issues(issues)),
        _ => Ok(result),
    }
}

/// Where a parse's issues go: all of them, for the `ZodError` a throwing parse throws, or only
/// how many, where whether it failed is all that matters.
enum Sink {
    Collect(Vec<Issue>),
    Count(usize),
}

impl Sink {
    fn add(&mut self, issue: impl FnOnce() -> Issue) {
        match self {
            Sink::Collect(issues) => issues.push(issue()),
            Sink::Count(count) => *count += 1,
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Sink::Collect(issues) => issues.is_empty(),
            Sink::Count(count) => *count == 0,
        }
    }
}

fn check_type<'v>(
    input: Input<'v>,
    expected: &str,
    matches: fn(&Value) -> bool,
    path: &Path,
    issues: &mut Sink,
) -> Option<Cow<'v, Value>> {
    match input {
        Input::Value(value) if matches(value) => Some(Cow::Borrowed(value)),
        _ => type_issue(input, expected, path, issues),
    }
}

/// The issue of a value an enum doesn't list. zod lists the options as the keys of an object
/// holding them: each once, array indices first.
fn enum_issue(options: &[&str], path: &Path) -> Issue {
    let mut values: Vec<&str> = Vec::with_capacity(options.len());
    for option in options {
        if !values.contains(option) {
            values.push(option);
        }
    }
    values.sort_by_key(|value| array_index(value).map_or((1, 0), |index| (0, index)));
    let message = match values[..] {
        [only] => format!("Invalid input: expected \"{only}\""),
        _ => {
            let listed = values.iter().map(|value| format!("\"{value}\""));
            let listed: Vec<_> = listed.collect();
            format!("Invalid option: expected one of {}", listed.join("|"))
        }
    };
    Issue {
        fields: vec![("code", json!("invalid_value")), ("values", json!(values))],
        path: path_values(path),
        message,
    }
}

fn unrecognized_keys(keys: &[&str], path: &Path) -> Issue {
    let listed = keys
        .iter()
        .map(|key| format!("\"{key}\""))
        .collect::<Vec<_>>();
    let plural = if keys.len() > 1 { "s" } else { "" };
    Issue {
        fields: vec![("code", json!("unrecognized_keys")), ("keys", json!(keys))],
        path: path_values(path),
        message: format!("Unrecognized key{plural}: {}", listed.join(", ")),
    }
}

fn type_issue<'v>(
    input: Input,
    expected: &str,
    path: &Path,
    issues: &mut Sink,
) -> Option<Cow<'v, Value>> {
    issues.add(|| {
        let received = match input {
            Input::Undefined => "undefined",
            Input::Inherited => "function",
            Input::Value(Value::Null) => "null",
            Input::Value(Value::Bool(_)) => "boolean",
            Input::Value(Value::Number(_)) => "number",
            Input::Value(Value::String(_)) => "string",
            Input::Value(Value::Array(_)) => "array",
            Input::Value(Value::Object(_)) => "object",
        };
        Issue {
            fields: vec![
                ("expected", json!(expected)),
                ("code", json!("invalid_type")),
            ],
            path: path_values(path),
            message: format!("Invalid input: expected {expected}, received {received}"),
        }
    });
    None
}

fn bound_issue<'v>(
    code: &str,
    key: &'static str,
    comparison: &str,
    sign: f64,
    path: &Path,
    issues: &mut Sink,
) -> Option<Cow<'v, Value>> {
    issues.add(|| {
        let bound = sign * MAX_SAFE_INTEGER;
        Issue {
            fields: vec![
                ("code", json!(code)),
                (key, tarnish_js::number(bound)),
                (
                    "note",
                    json!("Integers must be within the safe integer range."),
                ),
                ("origin", json!("int")),
                ("inclusive", json!(true)),
            ],
            path: path_values(path),
            message: format!(
                "Too {}: expected int to be {comparison}{}",
                if code == "too_big" { "big" } else { "small" },
                tarnish_js::number_to_string(bound)
            ),
        }
    });
    None
}

/// Where the value being parsed is, each step on the stack of the parse that took it, until an
/// issue needs its path.
enum Path<'a> {
    Root,
    Step(&'a Path<'a>, Segment<'a>),
}

#[derive(Clone, Copy)]
enum Segment<'a> {
    Key(&'a str),
    Index(usize),
}

impl<'a> Path<'a> {
    fn join(&'a self, segment: Segment<'a>) -> Path<'a> {
        Path::Step(self, segment)
    }
}

fn path_values(path: &Path) -> Vec<Value> {
    let Path::Step(parent, segment) = *path else {
        return Vec::new();
    };
    let mut values = path_values(parent);
    values.push(match segment {
        Segment::Key(key) => json!(key),
        Segment::Index(index) => json!(index),
    });
    values
}

#[cfg(test)]
mod tests {
    use super::Schema;
    use tarnish_js::json::{Value, json};

    #[test]
    fn parses_the_fallback_where_the_object_fails() {
        let schema = Schema::Object(vec![
            ("a", Schema::String),
            ("b", Schema::Number.default(json!(0))),
        ]);
        let (good, bad) = (json!({ "a": "x", "b": 1 }), json!({ "a": 1 }));
        let a = json!("y");
        let fallback = |key: &str| (key == "a").then_some(&a);
        let parsed = schema.parse_properties_or(|key| good.get(key), fallback);
        assert_eq!(parsed.unwrap().into_map(), *good.as_object().unwrap());
        let parsed = schema.parse_properties_or(|key| bad.get(key), fallback);
        assert_eq!(
            parsed.unwrap().into_map(),
            *json!({ "a": "y", "b": 0 }).as_object().unwrap()
        );
        let failed = schema.parse_properties_or(|key| bad.get(key), |_| None);
        assert_eq!(
            failed
                .err()
                .map(|issues| issues.first_message().to_string()),
            Some("Invalid input: expected string, received undefined".into())
        );
    }

    #[test]
    fn copies_an_array_from_the_first_item_that_changes() {
        let schema = Schema::array(Schema::String.catch(Some(json!("x"))));
        let parsed = schema.parse(Some(&json!(["a", 1, "b"]))).unwrap();
        assert_eq!(parsed, Some(json!(["a", "x", "b"])));
        let unchanged = json!(["a", "b"]);
        assert_eq!(schema.parse(Some(&unchanged)).unwrap(), Some(unchanged));
    }

    fn object() -> Schema {
        Schema::Object(vec![
            ("a", Schema::String),
            ("b", Schema::Number.nullable()),
            ("c", Schema::Boolean.default(json!(false))),
        ])
    }

    #[test]
    fn reads_a_field_of_the_shape() {
        assert_eq!(object().field("b"), &Schema::Number.nullable());
    }

    #[test]
    fn picks_fields_in_the_order_of_the_mask() {
        let picked = object().pick(&["c", "a"]);
        let expected = Schema::Object(vec![
            ("c", Schema::Boolean.default(json!(false))),
            ("a", Schema::String),
        ]);
        assert_eq!(picked, expected);
    }

    #[test]
    fn omits_fields_keeping_the_order_of_the_shape() {
        let omitted = object().omit(&["b"]);
        let expected = Schema::Object(vec![
            ("a", Schema::String),
            ("c", Schema::Boolean.default(json!(false))),
        ]);
        assert_eq!(omitted, expected);
    }

    #[test]
    fn makes_every_field_optional() {
        let expected = Schema::Object(vec![
            ("a", Schema::String.optional()),
            ("b", Schema::Number.nullable().optional()),
            ("c", Schema::Boolean.default(json!(false)).optional()),
        ]);
        assert_eq!(object().partial(), expected);
    }

    #[test]
    fn extends_replacing_a_field_in_place_and_adding_the_rest_after() {
        let extended = object().extend([("b", Schema::String), ("d", Schema::Int)]);
        let expected = Schema::Object(vec![
            ("a", Schema::String),
            ("b", Schema::String),
            ("c", Schema::Boolean.default(json!(false))),
            ("d", Schema::Int),
        ]);
        assert_eq!(extended, expected);
    }

    #[test]
    fn a_strict_object_refuses_the_keys_its_shape_lacks_after_its_fields() {
        let schema = Schema::Object(vec![("flag", Schema::Boolean.optional())]).strict();
        let first = |value: Value| {
            schema
                .parse(Some(&value))
                .unwrap_err()
                .first_message()
                .to_string()
        };
        assert_eq!(
            schema.parse(Some(&json!({"flag": true}))).unwrap(),
            Some(json!({"flag": true}))
        );
        assert_eq!(
            first(json!({"flag": true, "a": 1})),
            "Unrecognized key: \"a\""
        );
        assert_eq!(
            first(json!({"a": 1, "b": 2})),
            "Unrecognized keys: \"a\", \"b\""
        );
        assert_eq!(
            first(json!({"flag": 1, "a": 1})),
            "Invalid input: expected boolean, received number"
        );
        assert_eq!(
            first(json!(null)),
            "Invalid input: expected object, received null"
        );
    }

    #[test]
    #[should_panic(expected = "only an object schema is strict")]
    fn makes_only_an_object_schema_strict() {
        Schema::Object(Vec::new()).optional().strict();
    }

    #[test]
    fn unwraps_only_the_schemas_that_wrap_one() {
        let wrapped = Schema::String.nullable().optional().catch(None);
        let inner = wrapped.unwrap().and_then(Schema::unwrap);
        assert_eq!(inner, Some(&Schema::String.nullable()));
        assert_eq!(Schema::array(Schema::String).unwrap(), None);
    }

    #[test]
    #[should_panic(expected = "Unrecognized key: \"z\"")]
    fn picks_no_key_the_shape_lacks() {
        object().pick(&["a", "z"]);
    }

    #[test]
    #[should_panic(expected = "Unrecognized key: \"z\"")]
    fn omits_no_key_the_shape_lacks() {
        object().omit(&["z"]);
    }

    #[test]
    fn copies_a_record_from_the_first_entry_that_changes() {
        let schema = Schema::record(Schema::String.optional().catch(None));
        let parsed = schema.parse(Some(&json!({ "a": "1", "b": 2, "c": "3" })));
        assert_eq!(parsed.unwrap(), Some(json!({ "a": "1", "c": "3" })));
        let unchanged = json!({ "a": "1", "c": "3" });
        assert_eq!(schema.parse(Some(&unchanged)).unwrap(), Some(unchanged));
    }
}
