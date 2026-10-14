//! `@tiptap/extension-list`'s `OrderedList`, which `@tiptap/extension-ordered-list` exports, with
//! its list markers of numbers, letters and roman numerals (`roman.ts`) and its Markdown
//! tokenizer (`utils.ts`).

use std::sync::LazyLock;

use super::bullet_list::render_list;
use crate::markdown::{Lines, ParseHelpers, Parsed};
use crate::merge_attributes::merge_html_attributes;
use crate::{AttrValue, DomSpec, ExtensionAttribute, HtmlNode, NodeExtension, ParseHtml};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::regexp::RegExp;
use tarnish_js::units::Units;
use tarnish_js::{self as js, Error, stack, utf16, value};
use tarnish_markdown::marked::{Lexer, List, Token, TokenData};

pub struct OrderedListOptions {
    /// The node type of the list's items.
    pub item_type_name: &'static str,
    pub html_attributes: Map,
}

impl Default for OrderedListOptions {
    fn default() -> Self {
        OrderedListOptions {
            item_type_name: "listItem",
            html_attributes: Map::new(),
        }
    }
}

pub fn ordered_list(options: OrderedListOptions) -> NodeExtension {
    let attributes = options.html_attributes;
    NodeExtension::create("orderedList")
        .group("block list")
        .content(format!("{}+", options.item_type_name))
        .add_attributes(vec![
            ExtensionAttribute::new("start", json!(1)).parse_html(|element| {
                Some(match element.attribute("start") {
                    Some(start) => js::number(js::parse_int_radix(&start, 10)),
                    None => json!(1),
                })
            }),
            ExtensionAttribute::new("type", Value::Null).parse_html(html_type),
        ])
        .parse_html([ParseHtml::tag("ol")])
        .render_html(move |_, mut html| {
            let start = html.remove("start");
            let kind = html.remove("type");
            let mut attrs = merge_html_attributes(&attributes, html)?;
            if let Some(start) = start.filter(|start| !is_one(start)) {
                attrs.set("start", start);
            }
            if let Some(kind) = kind.filter(|kind| kind.truthy() && kind.as_str() != Some("1")) {
                attrs.set("type", kind);
            }
            Ok(DomSpec::wrapping("ol", attrs))
        })
        .markdown_token_name("list")
        .parse_markdown(parse_markdown)
        .render_markdown(render_list)
        // A marker the start would find past a paragraph's first unit is mid-line, and marked
        // already ends a paragraph before a list that starts a line.
        .markdown_block_tokenizer(|_| None, tokenize)
        .indents_content()
}

/// `start === 1`: the number, which a string isn't.
fn is_one(start: &AttrValue) -> bool {
    match start {
        AttrValue::Json(start) => start.as_f64() == Some(1.0),
        AttrValue::Owned(start) => start.as_f64() == Some(1.0),
        AttrValue::Text(_) => false,
    }
}

static LIST_STYLE_TYPE: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(r"list-style-type\s*:\s*([^;]+)", "i"));

/// The `type` attribute's `parseHTML`: the list's `type`, or the HTML type its CSS
/// `list-style-type` maps to, or its first item's.
fn html_type(element: &HtmlNode) -> Option<Value> {
    if let Some(kind) = element.attribute("type").filter(|kind| !kind.is_empty()) {
        return Some(Value::String(kind));
    }
    let mapped = |element: &HtmlNode| {
        let style = element
            .attribute("style")
            .filter(|style| !style.is_empty())?;
        css_list_style_type_to_html_type(&style)
    };
    if let Some(kind) = mapped(element) {
        return Some(json!(kind));
    }
    let first_item = element.query_selector("li").ok().flatten()?;
    mapped(&first_item).map(|kind| json!(kind))
}

/// `cssListStyleTypeToHtmlType(style)`.
fn css_list_style_type_to_html_type(style: &str) -> Option<&'static str> {
    let style = utf16::from(style);
    let found = LIST_STYLE_TYPE.exec(&style)?;
    let value = utf16::to_string(utf16::trim(found.get(1).unwrap_or_default())).to_lowercase();
    match value.as_str() {
        "upper-roman" => Some("I"),
        "lower-roman" => Some("i"),
        "upper-alpha" | "upper-latin" => Some("A"),
        "lower-alpha" | "lower-latin" => Some("a"),
        _ => None,
    }
}

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let Some(list) = token
        .list()
        .filter(|list| token.kind == "list" && list.ordered)
    else {
        return Ok(Parsed::Nodes(Vec::new()));
    };
    // `token.start || 1`
    let start = list
        .start
        .filter(|start| js::number_to_boolean(*start))
        .unwrap_or(1.0);
    let content = parse_list_items(&list.items, helpers)?;
    let mut attrs = Map::new();
    if start != 1.0 {
        attrs.insert("start".into(), js::number(start));
    }
    if let Some(kind) = token
        .extra("typeMarker")
        .filter(|kind| js::truthy(Some(kind)))
    {
        attrs.insert("type".into(), kind.clone());
    }
    Ok(Parsed::Node(if attrs.is_empty() {
        json!({ "type": "orderedList", "content": content })
    } else {
        json!({ "type": "orderedList", "attrs": attrs, "content": content })
    }))
}

/// `parseListItems(items, helpers)`: each item's blocks, its text in a paragraph.
fn parse_list_items(items: &[Token], helpers: &ParseHelpers) -> Result<Vec<Value>, Error> {
    let mut parsed = Vec::with_capacity(items.len());
    for item in items {
        let alone = std::slice::from_ref(item);
        if item.kind != "list_item" {
            let first = helpers.parse_children(alone)?.into_iter().next();
            parsed.push(first.unwrap_or(Value::Null));
            continue;
        }
        let mut content = Vec::new();
        for token in item.tokens.as_deref().unwrap_or_default() {
            let alone = std::slice::from_ref(token);
            match token.kind {
                "text" if token.tokens.is_some() => content.push(json!({
                    "type": "paragraph",
                    "content": helpers.parse_children(alone)?,
                })),
                _ => content.extend(helpers.parse_children(alone)?),
            }
        }
        parsed.push(json!({ "type": "listItem", "content": content }));
    }
    Ok(parsed)
}

static ORDERED_LIST_ITEM: LazyLock<RegExp> = LazyLock::new(|| {
    RegExp::new(
        r"^(\s*)(\d+|[ivxlcdmIVXLCDM]+|[a-zA-Z]{1,2})([.)])\s+(.*)$",
        "",
    )
});
static INDENTED_LINE: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^\s", ""));
static HEADING: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^#{1,6}(?:\s|$)", ""));
static BULLET_ITEM: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^[-+*]\s+", ""));
static CODE_FENCE: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^(?:```|~~~)", ""));
static BLOCK_MATH: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^\$\$", ""));
static THEMATIC_BREAK: LazyLock<RegExp> = LazyLock::new(|| {
    RegExp::new(
        r"^(?:(?:-[ \t]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})$",
        "",
    )
});
static BLOCKQUOTE: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^>\s?", ""));

/// What `collectOrderedListItems` reads of an item: its indent, number and marker type, the
/// lines of its content, and where its lines are in the source.
struct OrderedListItem<'s> {
    indent: usize,
    number: f64,
    kind: Option<&'static str>,
    content_lines: Vec<&'s [u16]>,
    start: usize,
    end: usize,
}

/// `isBlockContentLine(line)`: whether a line starts a block, which ends an item's paragraph.
fn is_block_content_line(line: &[u16]) -> bool {
    let trimmed = utf16::trim_start(line);
    BULLET_ITEM.test(trimmed)
        || ORDERED_LIST_ITEM.test(utf16::trim_start(trimmed))
        || HEADING.test(trimmed)
        || (THEMATIC_BREAK.test(trimmed) && !utf16::starts_with(trimmed, "-"))
        || BLOCKQUOTE.test(trimmed)
        || CODE_FENCE.test(trimmed)
        || BLOCK_MATH.test(trimmed)
}

/// `interruptsLazyContinuation(line)`.
fn interrupts_lazy_continuation(line: &[u16]) -> bool {
    [
        &HEADING,
        &BULLET_ITEM,
        &CODE_FENCE,
        &BLOCK_MATH,
        &THEMATIC_BREAK,
    ]
    .iter()
    .any(|pattern| pattern.test(line))
}

/// `splitItemContent(contentLines)`: the item's paragraph lines, and the lines from the first
/// blank line or block after them.
fn split_item_content<'s>(content_lines: &[&'s [u16]]) -> (Vec<&'s [u16]>, Vec<&'s [u16]>) {
    let mut paragraph_lines = Vec::new();
    let mut block_lines = Vec::new();
    let mut reached_block_boundary = false;
    for &line in content_lines {
        if !reached_block_boundary {
            let starts_block = !paragraph_lines.is_empty() && is_block_content_line(line);
            if utf16::trim(line).is_empty() || starts_block {
                reached_block_boundary = true;
            } else {
                paragraph_lines.push(line);
                continue;
            }
        }
        block_lines.push(line);
    }
    (paragraph_lines, block_lines)
}

/// `collectOrderedListItems(lines)`: the items the lines start with, each with its lines, and
/// how many lines they take.
fn collect_ordered_list_items<'s>(lines: &mut Lines<'s>) -> (Vec<OrderedListItem<'s>>, usize) {
    let mut items = Vec::new();
    let mut current = 0;
    while let Some(line) = lines.get(current) {
        let Some(found) = ORDERED_LIST_ITEM.exec(line) else {
            break;
        };
        let indent = found.get(1).unwrap_or_default().len();
        let marker = utf16::to_string(found.get(2).unwrap_or_default());
        let number = js::parse_int_radix(&marker, 10);
        let (number, kind) = match number.is_nan() {
            true => (marker_to_start(&marker), detect_marker_type(&marker)),
            false => (number, None),
        };
        let mut content_lines = vec![found.get(4).unwrap_or_default()];
        let start = lines.start(current).unwrap_or_default();
        let mut next = current + 1;
        let mut saw_blank_line = false;
        while let Some(next_line) = lines.get(next) {
            if ORDERED_LIST_ITEM.test(next_line) {
                break;
            }
            if utf16::trim(next_line).is_empty() {
                content_lines.push(&[]);
                saw_blank_line = true;
            } else if INDENTED_LINE.test(next_line) {
                // An under-indented line keeps what isn't whitespace.
                let leading = next_line.len() - utf16::trim_start(next_line).len();
                let content_indent = indent + marker.len() + 1;
                content_lines.push(&next_line[leading.min(content_indent)..]);
            } else {
                if saw_blank_line || interrupts_lazy_continuation(next_line) {
                    break;
                }
                content_lines.push(next_line);
            }
            next += 1;
        }
        items.push(OrderedListItem {
            indent,
            number,
            kind,
            content_lines,
            start,
            end: lines.end(next - 1),
        });
        current = next;
    }
    (items, current)
}

/// `buildNestedStructure(items, baseIndent, lexer)`: the list items at `base_indent`, each
/// with its text as a paragraph, its blocks, and the items indented deeper after it as a list.
fn build_nested_structure(
    src: &Units,
    items: &[OrderedListItem],
    base_indent: usize,
    lexer: &mut Lexer,
) -> Result<Vec<Token>, Error> {
    let newline = [utf16::unit(b'\n')];
    let mut result = Vec::new();
    let mut current = 0;
    while current < items.len() {
        let item = &items[current];
        if item.indent != base_indent {
            current += 1;
            continue;
        }
        let (paragraph_lines, block_lines) = split_item_content(&item.content_lines);
        let mut tokens = Vec::new();
        let main_text = utf16::join(&paragraph_lines, &newline);
        let main_text = Units::from(utf16::trim(&main_text));
        if !main_text.is_empty() {
            let inline = lexer.inline_tokens(&main_text)?;
            tokens.push(Token {
                tokens: Some(inline.into()),
                ..Token::new("paragraph", main_text)
            });
        }
        let additional = utf16::join(&block_lines, &newline);
        let additional = Units::from(utf16::trim(&additional));
        if !additional.is_empty() {
            lexer.block_tokens(&additional, &mut tokens, false)?;
        }
        let nested_end = items[current + 1..]
            .iter()
            .position(|nested| nested.indent <= base_indent)
            .map_or(items.len(), |offset| current + 1 + offset);
        let nested = &items[current + 1..nested_end];
        if let (Some(first), Some(last)) = (nested.first(), nested.last()) {
            let next_indent = nested.iter().map(|nested| nested.indent).min();
            let next_indent = next_indent.unwrap_or_default();
            let nested_items =
                stack::grow(|| build_nested_structure(src, nested, next_indent, lexer))?;
            tokens.push(list_token(
                src.slice(first.start..last.end),
                first,
                nested_items,
                first.number,
            ));
        }
        result.push(Token {
            tokens: Some(tokens.into()),
            ..Token::new("list_item", src.slice(item.start..item.end))
        });
        current = nested_end;
    }
    Ok(result)
}

/// An ordered `list` token of the items, whose first is `first`.
fn list_token(raw: Units, first: &OrderedListItem, items: Vec<Token>, start: f64) -> Token {
    let extra = first.kind.map(|kind| {
        let mut extra = Map::new();
        extra.insert("typeMarker".into(), json!(kind));
        Box::new(extra)
    });
    Token {
        data: TokenData::List(Box::new(List {
            ordered: true,
            start: Some(start),
            loose: false,
            items,
        })),
        extra,
        ..Token::new("list", raw)
    }
}

/// The `markdownTokenizer`: the ordered list the source starts with, nested by indentation.
fn tokenize(src: &Units, _: &[Token], lexer: &mut Lexer) -> Result<Option<Token>, Error> {
    let mut lines = Lines::new(src);
    let (items, consumed) = collect_ordered_list_items(&mut lines);
    let Some(first) = items.first() else {
        return Ok(None);
    };
    let list_items = build_nested_structure(src, &items, first.indent, lexer)?;
    if list_items.is_empty() {
        return Ok(None);
    }
    // `listItems[0]?.number || 1`
    let start = match js::number_to_boolean(first.number) {
        true => first.number,
        false => 1.0,
    };
    let raw = src.slice(0..lines.end(consumed - 1));
    Ok(Some(list_token(raw, first, list_items, start)))
}

const ROMAN_NUMERALS: [(f64, &str); 13] = [
    (1000.0, "m"),
    (900.0, "cm"),
    (500.0, "d"),
    (400.0, "cd"),
    (100.0, "c"),
    (90.0, "xc"),
    (50.0, "l"),
    (40.0, "xl"),
    (10.0, "x"),
    (9.0, "ix"),
    (5.0, "v"),
    (4.0, "iv"),
    (1.0, "i"),
];

const ALPHA_NUMERALS: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";

/// `toRoman(num)`, in lowercase.
fn to_roman(num: f64) -> Result<String, Error> {
    if num == f64::INFINITY {
        return Err(js::invalid_string_length());
    }
    // The thousands at once: below 2^53, taking 1000 away again and again is exact.
    let mut remaining = num;
    let mut thousands = if remaining >= 1000.0 {
        (remaining / 1000.0).floor()
    } else {
        0.0
    };
    while thousands > 0.0 && remaining - thousands * 1000.0 < 0.0 {
        thousands -= 1.0;
    }
    while remaining - (thousands + 1.0) * 1000.0 >= 0.0 && thousands < js::MAX_SAFE_INTEGER {
        thousands += 1.0;
    }
    let mut result = js::repeat("m", thousands)?;
    remaining -= thousands * 1000.0;
    for (value, numeral) in &ROMAN_NUMERALS[1..] {
        while remaining >= *value {
            result.push_str(numeral);
            remaining -= value;
        }
    }
    Ok(result)
}

/// `fromRoman(roman)`, of a marker of roman numerals' letters: 0 where a letter isn't one.
fn from_roman(roman: &str) -> f64 {
    let lower = roman.to_ascii_lowercase();
    let mut rest = lower.as_str();
    let mut result = 0.0;
    while !rest.is_empty() {
        let Some((value, numeral)) = ROMAN_NUMERALS
            .iter()
            .find(|(_, numeral)| rest.starts_with(numeral))
        else {
            return 0.0;
        };
        result += value;
        rest = &rest[numeral.len()..];
    }
    result
}

fn is_valid_roman(marker: &str) -> bool {
    if marker.is_empty() || !marker.bytes().all(|byte| b"ivxlcdmIVXLCDM".contains(&byte)) {
        return false;
    }
    let value = from_roman(marker);
    if value <= 0.0 {
        return false;
    }
    let expected = to_roman(value).unwrap_or_default();
    match marker == marker.to_ascii_lowercase() {
        true => expected == marker,
        false => expected.to_ascii_uppercase() == marker,
    }
}

/// `fromAlpha(marker)`: a, b, ..., z, aa, ab, ..., zz from 1.
fn from_alpha(marker: &str) -> f64 {
    let lower = marker.to_ascii_lowercase();
    let letter = |byte: u8| f64::from(byte) - f64::from(b'a');
    match lower.as_bytes() {
        [only] => letter(*only) + 1.0,
        [first, second] => (letter(*first) + 1.0) * 26.0 + letter(*second) + 1.0,
        _ => 0.0,
    }
}

/// `ALPHA_NUMERALS[index]`, `None` for `undefined`.
fn alpha_numeral(index: f64) -> Option<char> {
    let valid = index.fract() == 0.0 && (0.0..26.0).contains(&index);
    valid.then(|| char::from(ALPHA_NUMERALS[index as usize]))
}

/// What `toRomanAlpha(num)` returns.
enum Alpha {
    String(String),
    Undefined,
    /// `undefined + undefined`.
    NaN,
}

/// `toRomanAlpha(num)`: the letters of a position, a to zz.
fn to_roman_alpha(num: f64) -> Alpha {
    let numeral = |index| match alpha_numeral(index) {
        Some(letter) => Alpha::String(letter.into()),
        None => Alpha::Undefined,
    };
    if num <= 26.0 {
        return numeral(num - 1.0);
    }
    let first = ((num - 1.0) / 26.0).floor() - 1.0;
    let second = (num - 1.0) % 26.0;
    if first < 0.0 {
        return numeral(second);
    }
    match (alpha_numeral(first), alpha_numeral(second)) {
        (None, None) => Alpha::NaN,
        (first, second) => {
            let part = |letter: Option<char>| letter.map_or("undefined".into(), String::from);
            Alpha::String(part(first) + &part(second))
        }
    }
}

/// `detectMarkerType(marker)`: the `type` of a marker of letters, `None` for numbers.
fn detect_marker_type(marker: &str) -> Option<&'static str> {
    if marker.is_empty() || marker.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if is_valid_roman(marker) {
        return Some(match marker == marker.to_ascii_lowercase() {
            true => "i",
            false => "I",
        });
    }
    let letters = |check: fn(&u8) -> bool| {
        (1..=2).contains(&marker.len()) && marker.bytes().all(|byte| check(&byte))
    };
    if letters(u8::is_ascii_lowercase) {
        return Some("a");
    }
    if letters(u8::is_ascii_uppercase) {
        return Some("A");
    }
    None
}

/// `markerToStart(marker)`: the position a marker of letters stands for.
fn marker_to_start(marker: &str) -> f64 {
    if !marker.is_empty() && marker.bytes().all(|byte| byte.is_ascii_digit()) {
        return js::parse_int_radix(marker, 10);
    }
    match detect_marker_type(marker) {
        Some("i" | "I") => from_roman(marker),
        Some(_) => match from_alpha(marker) {
            start if start > 0.0 => start,
            _ => 1.0,
        },
        None => match js::parse_int_radix(marker, 10) {
            parsed if parsed.is_nan() => 1.0,
            parsed => parsed,
        },
    }
}

/// `getListMarker(type, index, separator)`: the marker of the item at `index`, in the `type`'s
/// numbering.
pub(super) fn list_marker(
    kind: Option<&Value>,
    index: f64,
    separator: &str,
) -> Result<String, Error> {
    let position = index + 1.0;
    let number = || js::number_to_string(position) + separator;
    let kind = kind
        .filter(|kind| js::truthy(Some(kind)))
        .and_then(Value::as_str);
    let upper = |alpha: Alpha| match alpha {
        Alpha::String(letters) => Ok(letters.to_uppercase()),
        Alpha::Undefined => Err(value::cannot_read(value::Nullish::Undefined, "toUpperCase")),
        Alpha::NaN => Err(value::not_a_function("toRomanAlpha(...).toUpperCase")),
    };
    Ok(match kind {
        Some("a") => match to_roman_alpha(position) {
            Alpha::String(letters) => letters + separator,
            Alpha::Undefined => ["undefined", separator].concat(),
            Alpha::NaN => ["NaN", separator].concat(),
        },
        Some("A") => upper(to_roman_alpha(position))? + separator,
        Some("i") => to_roman(position)? + separator,
        Some("I") => to_roman(position)?.to_uppercase() + separator,
        _ => number(),
    })
}

#[cfg(test)]
mod tests {
    use super::{from_roman, is_valid_roman, to_roman};

    #[test]
    fn roman_numerals_read_back_as_they_are_written() {
        for number in 1..4000 {
            let roman = to_roman(f64::from(number)).expect("a numeral");
            assert_eq!(from_roman(&roman), f64::from(number));
            assert!(is_valid_roman(&roman) && is_valid_roman(&roman.to_uppercase()));
        }
        assert!(!is_valid_roman("iiii") && !is_valid_roman("ic") && !is_valid_roman("iI"));
    }
}
