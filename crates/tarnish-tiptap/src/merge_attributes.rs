//! `mergeAttributes`.

use indexmap::IndexMap;

use crate::{AttrValue, SpecAttrs};
use tarnish_js::{self as js, Error};

/// `mergeAttributes(html, extra)`.
pub fn merge_attributes<'a, const N: usize>(
    mut html: SpecAttrs<'a>,
    extra: [(&'a str, AttrValue<'a>); N],
) -> Result<SpecAttrs<'a>, Error> {
    for (key, value) in extra {
        merge_entry(&mut html, key, value)?;
    }
    Ok(html)
}

/// Merges an entry of an object into `merged` as `mergeAttributes` does: its value replaces a
/// falsy one, and a `class` or `style` joins the one there.
pub(super) fn merge_entry<'a>(
    merged: &mut SpecAttrs<'a>,
    key: &'a str,
    value: AttrValue<'a>,
) -> Result<(), Error> {
    match merged.get_mut(key) {
        Some(existing) if existing.truthy() => {
            *existing = joined(key, existing, value)?;
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
