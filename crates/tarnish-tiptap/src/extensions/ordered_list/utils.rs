//! `utils.ts`: the Markdown tokenizer's reading of an ordered list's lines into nested items, and
//! `parseListItems`.

use std::sync::LazyLock;

use super::roman::{detect_marker_type, marker_to_start};
use crate::markdown::{Lines, ParseHelpers};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::regexp::RegExp;
use tarnish_js::units::Units;
use tarnish_js::{self as js, Error, stack, utf16};
use tarnish_markdown::marked::{Lexer, List, Token, TokenData};

/// `parseListItems(items, helpers)`: each item's blocks, its text in a paragraph.
pub(super) fn parse_list_items(
    items: &[Token],
    helpers: &ParseHelpers,
) -> Result<Vec<Value>, Error> {
    let mut parsed = Vec::with_capacity(items.len());
    for item in items {
        let alone = std::slice::from_ref(item);
        if item.kind != "list_item" {
            let first = helpers.parse_children(alone)?.into_iter().next();
            parsed.push(Value::from(first));
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
pub(super) fn tokenize(
    src: &Units,
    _: &[Token],
    lexer: &mut Lexer,
) -> Result<Option<Token>, Error> {
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
