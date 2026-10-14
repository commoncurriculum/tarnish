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
                    "class" => joined_classes(existing, value)?,
                    "style" => joined_styles(existing, value),
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

fn joined_classes(existing: &Value, value: &Value) -> Result<Value, Error> {
    let Value::String(existing) = existing else {
        return Err(value::not_a_function("mergedAttributes[key].split"));
    };
    let value = match js::truthy(Some(value)) {
        true => Some(js::to_string(value)?),
        false => None,
    };
    let mut classes: Vec<&str> = existing.split(' ').collect();
    let inserted: Vec<&str> = value
        .iter()
        .flat_map(|value| value.split(' '))
        .filter(|class| !classes.contains(class))
        .collect();
    classes.extend(inserted);
    Ok(Value::String(classes.join(" ")))
}

/// The styles of `parseStyleEntries`, which reads only a string's.
fn joined_styles(existing: &Value, value: &Value) -> Value {
    let declarations = |value: &Value| match value {
        Value::String(styles) => parse_style_entries(styles).collect(),
        _ => Vec::new(),
    };
    let mut styles = IndexMap::new();
    for (property, style) in declarations(existing)
        .into_iter()
        .chain(declarations(value))
    {
        styles.insert(property, style);
    }
    let joined: Vec<String> = styles
        .iter()
        .map(|(property, style)| format!("{property}: {style}"))
        .collect();
    Value::String(joined.join("; "))
}

/// The attributes rendered for a node or mark, as an object of their values.
pub fn rendered_object(html: &SpecAttrs) -> Map {
    html.iter()
        .map(|(key, value)| {
            let value = match value {
                AttrValue::Json(value) => value.to_value(),
                AttrValue::Text(text) => Value::String(text.to_string()),
            };
            (key.into(), value)
        })
        .collect()
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
        "class" => {
            let existing = existing.to_js_string()?;
            let value_classes = value.truthy().then(|| value.to_js_string()).transpose()?;
            let mut classes: Vec<&str> = existing.split(' ').collect();
            let inserted: Vec<&str> = value_classes
                .iter()
                .flat_map(|value| value.split(' '))
                .filter(|class| !classes.contains(class))
                .collect();
            classes.extend(inserted);
            classes.join(" ").into()
        }
        "style" => {
            let existing = existing.to_js_string()?;
            let mut styles = IndexMap::new();
            let value = value.as_str().unwrap_or_default();
            for (property, style) in
                parse_style_entries(&existing).chain(parse_style_entries(value))
            {
                styles.insert(property, style);
            }
            styles
                .iter()
                .map(|(property, style)| format!("{property}: {style}"))
                .collect::<Vec<_>>()
                .join("; ")
                .into()
        }
        _ => value,
    })
}

/// The `property: value` pairs of a style declaration list, split on the semicolons that are
/// outside quotes and parentheses.
fn parse_style_entries(styles: &str) -> impl Iterator<Item = (String, String)> + '_ {
    split_style_declarations(styles)
        .into_iter()
        .filter_map(|declaration| {
            let (property, value) = declaration.split_once(':')?;
            let (property, value) = (js::trim(property), js::trim(value));
            (!property.is_empty() && !value.is_empty())
                .then(|| (property.to_string(), value.to_string()))
        })
}

fn split_style_declarations(styles: &str) -> Vec<&str> {
    let mut declarations = Vec::new();
    let (mut single, mut double, mut depth, mut start) = (false, false, 0, 0);
    for (index, character) in styles.char_indices() {
        match character {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '(' if !single && !double => depth += 1,
            ')' if !single && !double && depth > 0 => depth -= 1,
            ';' if !single && !double && depth == 0 => {
                declarations.push(&styles[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < styles.len() {
        declarations.push(&styles[start..]);
    }
    declarations
}
