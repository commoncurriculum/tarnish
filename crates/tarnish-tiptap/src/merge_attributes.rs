//! `mergeAttributes`, and the `HTMLAttributes` option it merges into an element's attributes.

use std::borrow::Cow;

use indexmap::IndexMap;

use crate::{AttrValue, DomSpec, SpecAttrs};
use tarnish_js::json::{Map, Value};
use tarnish_js::{self as js, Error, value};

/// The options of an extension whose only option is `HTMLAttributes`: attributes its element
/// gets ahead of those its node or mark renders, of any JSON, `null` standing for `undefined`.
#[derive(Clone, Default)]
pub struct HtmlAttributesOptions {
    pub html_attributes: Map,
}

/// `mergeAttributes(attrs, extra)`: each entry of `extra` in turn replaces the value of its name,
/// but a `class` or `style` joins a truthy one.
pub fn merge_attributes<'a, K: Into<Cow<'a, str>>>(
    mut attrs: SpecAttrs<'a>,
    extra: impl IntoIterator<Item = (K, AttrValue<'a>)>,
) -> Result<SpecAttrs<'a>, Error> {
    for (key, value) in extra {
        merge_entry(&mut attrs, key.into(), value)?;
    }
    Ok(attrs)
}

/// `mergeAttributes(this.options.HTMLAttributes, HTMLAttributes)`, which is `html` itself while
/// the option is empty, as it is by default.
pub(crate) fn merge_html_attributes<'a>(
    html_attributes: &Map,
    html: SpecAttrs<'a>,
) -> Result<SpecAttrs<'a>, Error> {
    if html_attributes.is_empty() {
        return Ok(html);
    }
    merge_attributes(SpecAttrs::from(html_attributes), html)
}

/// `renderHTML({ HTMLAttributes }) { return [tag, mergeAttributes(this.options.HTMLAttributes,
/// HTMLAttributes, extra), 0] }`.
pub(crate) fn render_wrapping<T, const N: usize>(
    tag: &'static str,
    html_attributes: Map,
    extra: [(&'static str, &'static str); N],
) -> impl for<'a> Fn(&'a T, SpecAttrs<'a>) -> Result<DomSpec<'a>, Error> + Send + Sync + 'static {
    render_plain(html_attributes, extra, move |attrs| {
        DomSpec::wrapping(tag, attrs)
    })
}

/// That `renderHTML` without the `0`, of an element that holds nothing, such as `br`.
pub(crate) fn render_element<T, const N: usize>(
    tag: &'static str,
    html_attributes: Map,
    extra: [(&'static str, &'static str); N],
) -> impl for<'a> Fn(&'a T, SpecAttrs<'a>) -> Result<DomSpec<'a>, Error> + Send + Sync + 'static {
    render_plain(html_attributes, extra, move |attrs| {
        DomSpec::element(tag, attrs, Vec::new())
    })
}

fn render_plain<T, const N: usize>(
    html_attributes: Map,
    extra: [(&'static str, &'static str); N],
    spec: impl for<'a> Fn(SpecAttrs<'a>) -> DomSpec<'a> + Send + Sync + 'static,
) -> impl for<'a> Fn(&'a T, SpecAttrs<'a>) -> Result<DomSpec<'a>, Error> + Send + Sync + 'static {
    move |_, html| {
        let attrs = merge_html_attributes(&html_attributes, html)?;
        let extra = extra.map(|(key, value)| (key, AttrValue::from(value)));
        Ok(spec(merge_attributes(attrs, extra)?))
    }
}

/// Merges an entry into `merged` as `mergeAttributes` does.
pub(super) fn merge_entry<'a>(
    merged: &mut SpecAttrs<'a>,
    key: Cow<'a, str>,
    value: AttrValue<'a>,
) -> Result<(), Error> {
    match merged.get_mut(&key) {
        Some(existing) if existing.truthy() => {
            *existing = match key.as_ref() {
                "class" => joined_classes(existing, &value)?.into(),
                "style" => joined_styles(existing, &value)?.into(),
                _ => value,
            };
        }
        Some(existing) => *existing = value,
        None => merged.push(key, value),
    }
    Ok(())
}

/// The classes of a truthy `existing` with those of `value` it doesn't hold after them. `value`'s
/// string is made first, and an `existing` that isn't a string has no `split` to call.
fn joined_classes(existing: &AttrValue, value: &AttrValue) -> Result<String, Error> {
    let value = match value.truthy() {
        true => Some(value.to_js_string()?),
        false => None,
    };
    let Some(existing) = existing.as_str() else {
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
fn joined_styles(existing: &AttrValue, value: &AttrValue) -> Result<String, Error> {
    let mut styles = IndexMap::new();
    let existing = parse_style_entries(&existing.to_value())?;
    for (property, style) in existing
        .into_iter()
        .chain(parse_style_entries(&value.to_value())?)
    {
        styles.insert(property, style);
    }
    let joined: Vec<String> = styles
        .iter()
        .map(|(property, style)| format!("{property}: {style}"))
        .collect();
    Ok(joined.join("; "))
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
