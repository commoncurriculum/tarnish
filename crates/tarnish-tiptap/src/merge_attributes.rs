//! `mergeAttributes`.

use std::borrow::Cow;

use indexmap::IndexMap;

use crate::{AttrValue, SpecAttrs};
use tarnish_js::json::{Map, Value};
use tarnish_js::{self as js, Error, value};

/// `mergeAttributes(html, extra)`.
pub fn merge_attributes<'a, const N: usize>(
    mut html: SpecAttrs<'a>,
    extra: [(&'a str, AttrValue<'a>); N],
) -> Result<SpecAttrs<'a>, Error> {
    for (key, value) in extra {
        merge_entry(&mut html, Cow::Borrowed(key), value)?;
    }
    Ok(html)
}

/// `mergeAttributes(...objects)` of objects whose values are any JSON, such as the options an
/// embed gives its iframe ahead of what its node renders. `null` stands for `undefined` too,
/// which an element is given no more than `null`.
pub fn merge_objects<'m>(objects: impl IntoIterator<Item = &'m Map>) -> Result<Map, Error> {
    let mut merged = Map::new();
    for object in objects {
        for (key, value) in object.iter() {
            let joined = match merged.get(key) {
                Some(existing) if js::truthy(Some(existing)) => match key.as_ref() {
                    "class" => Value::String(joined_classes(existing, value)?),
                    "style" => Value::String(joined_styles(existing, value)?),
                    _ => value.clone(),
                },
                _ => value.clone(),
            };
            merged.insert(key.clone(), joined);
        }
    }
    Ok(merged)
}

/// `mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, extra)` of an `HTMLAttributes`
/// option whose values are any JSON, as [`merge_objects`] merges them.
pub(crate) fn merge_configured<'a, const N: usize>(
    configured: &Map,
    html: SpecAttrs<'a>,
    extra: [(&'a str, AttrValue<'a>); N],
) -> Result<SpecAttrs<'a>, Error> {
    if configured.is_empty() {
        return merge_attributes(html, extra);
    }
    let extra = rendered_object(&SpecAttrs::from(extra));
    element_attrs(merge_objects([
        configured,
        &rendered_object(&html),
        &extra,
    ])?)
}

/// The classes of a truthy `existing` with those of `value` it doesn't hold after them. `value`'s
/// string is made first, and an `existing` that isn't a string has no `split` to call.
fn joined_classes(existing: &Value, value: &Value) -> Result<String, Error> {
    let value = match js::truthy(Some(value)) {
        true => Some(js::to_string(value)?),
        false => None,
    };
    let Value::String(existing) = existing else {
        return Err(value::not_a_function("mergedAttributes[key].split"));
    };
    let mut classes: Vec<&str> = existing.split(' ').collect();
    let inserted: Vec<&str> = value
        .iter()
        .flat_map(|value| value.split(' '))
        .filter(|class| !classes.contains(class))
        .collect();
    classes.extend(inserted);
    Ok(classes.join(" "))
}

/// Each property of `existing`'s styles and then `value`'s, where it first comes, with the value
/// it last has.
fn joined_styles(existing: &Value, value: &Value) -> Result<String, Error> {
    let mut styles = IndexMap::new();
    let existing = parse_style_entries(existing)?;
    for (property, style) in existing.into_iter().chain(parse_style_entries(value)?) {
        styles.insert(property, style);
    }
    let joined: Vec<String> = styles
        .iter()
        .map(|(property, style)| format!("{property}: {style}"))
        .collect();
    Ok(joined.join("; "))
}

/// The attributes rendered for a node or mark, as an object of their values.
pub fn rendered_object(html: &SpecAttrs) -> Map {
    html.iter()
        .map(|(key, value)| (key.into(), attr_value(value)))
        .collect()
}

fn attr_value(value: &AttrValue) -> Value {
    match value {
        AttrValue::Json(value) => value.to_value(),
        AttrValue::Text(text) => Value::String(text.to_string()),
    }
}

/// An object of attributes as an element's, which leaves out those `null` or `undefined`.
pub fn element_attrs(object: Map) -> Result<SpecAttrs<'static>, Error> {
    let mut attrs = SpecAttrs::new();
    for (key, value) in object.iter() {
        if !value.is_null() {
            attrs.push(key.to_string(), AttrValue::from(js::to_string(value)?));
        }
    }
    Ok(attrs)
}

/// Merges an entry of an object into `merged` as `mergeAttributes` does: its value replaces a
/// falsy one, and a `class` or `style` joins the one there.
pub(super) fn merge_entry<'a>(
    merged: &mut SpecAttrs<'a>,
    key: Cow<'a, str>,
    value: AttrValue<'a>,
) -> Result<(), Error> {
    match merged.get_mut(&key) {
        Some(existing) if existing.truthy() => {
            *existing = joined(&key, existing, value)?;
        }
        Some(existing) => *existing = value,
        None => merged.push(key, value),
    }
    Ok(())
}

/// What an entry's `value` makes of a truthy value its key already has.
fn joined<'a>(
    key: &str,
    existing: &AttrValue<'a>,
    value: AttrValue<'a>,
) -> Result<AttrValue<'a>, Error> {
    Ok(match key {
        "class" => joined_classes(&attr_value(existing), &attr_value(&value))?.into(),
        "style" => joined_styles(&attr_value(existing), &attr_value(&value))?.into(),
        _ => value,
    })
}

/// `parseStyleEntries(styles)`: the `property: value` pairs of `styles || ''`'s declarations.
fn parse_style_entries(styles: &Value) -> Result<Vec<(String, String)>, Error> {
    let declarations = match js::truthy(Some(styles)) {
        true => split_style_declarations(styles)?,
        false => Vec::new(),
    };
    Ok(declarations
        .iter()
        .filter_map(|declaration| {
            let (property, value) = declaration.split_once(':')?;
            let (property, value) = (js::trim(property), js::trim(value));
            (!property.is_empty() && !value.is_empty())
                .then(|| (property.to_string(), value.to_string()))
        })
        .collect())
}

/// `splitStyleDeclarations(styles)`, which walks `styles[0]` to `styles[styles.length - 1]`: a
/// string's characters, an array's items, an object's as an array-like's, and nothing of a
/// number or boolean, which has no `length`.
fn split_style_declarations(styles: &Value) -> Result<Vec<String>, Error> {
    let mut split = Split::default();
    match styles {
        Value::String(styles) => styles
            .chars()
            .for_each(|character| split.character(character)),
        Value::Array(items) => {
            for item in items {
                split.item(item)?;
            }
        }
        Value::Object(object) => {
            // `i < styles.length` holds for each integer from 0 below it.
            let length = js::to_number(object.get("length"))?;
            let count = if length > 0.0 { length.ceil() } else { 0.0 };
            let mut indexed: Vec<(f64, &Value)> = object
                .iter()
                .filter_map(|(key, item)| Some((array_index(key)?, item)))
                .filter(|(index, _)| *index < count)
                .collect();
            indexed.sort_by(|(one, _), (other, _)| one.total_cmp(other));
            let mut next = 0.0;
            for (index, item) in indexed {
                split.undefined(index - next)?;
                split.item(item)?;
                next = index + 1.0;
            }
            split.undefined(count - next)?;
        }
        _ => {}
    }
    if !split.current.is_empty() {
        split.declarations.push(split.current);
    }
    Ok(split.declarations)
}

/// The integer `i` whose `String(i)` is `key`.
fn array_index(key: &str) -> Option<f64> {
    let canonical = key == "0"
        || (key.starts_with(['1', '2', '3', '4', '5', '6', '7', '8', '9'])
            && key.bytes().all(|byte| byte.is_ascii_digit()));
    if !canonical {
        return None;
    }
    key.parse::<u64>().ok().map(|index| index as f64)
}

/// The declarations `splitStyleDeclarations` has split off, and the one it's in.
#[derive(Default)]
struct Split {
    declarations: Vec<String>,
    current: String,
    single: bool,
    double: bool,
    depth: usize,
}

impl Split {
    /// A character of a string, split on the semicolons outside quotes and parentheses.
    fn character(&mut self, character: char) {
        let quoted = self.single || self.double;
        match character {
            '\'' if !self.double => self.single = !self.single,
            '"' if !self.single => self.double = !self.double,
            '(' if !quoted => self.depth += 1,
            ')' if !quoted && self.depth > 0 => self.depth -= 1,
            ';' if !quoted && self.depth == 0 => {
                self.declarations.push(std::mem::take(&mut self.current));
                return;
            }
            _ => {}
        }
        self.current.push(character);
    }

    /// An item that is a quote, a parenthesis or a semicolon is read as that character is, and
    /// any other joins the declaration as `current += item` makes it.
    fn item(&mut self, item: &Value) -> Result<(), Error> {
        match item {
            Value::String(text) => match text.as_str() {
                "'" | "\"" | "(" | ")" | ";" => self.character(text.chars().next().expect("one")),
                text => self.current.push_str(text),
            },
            item => self.current.push_str(&js::to_string(item)?),
        }
        Ok(())
    }

    /// `count` items of `undefined`, each `"undefined"` in the declaration, as long as memory
    /// allows.
    fn undefined(&mut self, count: f64) -> Result<(), Error> {
        const UNDEFINED: &str = "undefined";
        if count <= 0.0 {
            return Ok(());
        }
        let additional = count * UNDEFINED.len() as f64;
        if additional > isize::MAX as f64 {
            return Err(js::invalid_string_length());
        }
        js::reserve(&mut self.current, additional as usize)?;
        for _ in 0..count as u64 {
            self.current.push_str(UNDEFINED);
        }
        Ok(())
    }
}
