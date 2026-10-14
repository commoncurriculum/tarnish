//! `@tiptap/core`'s Markdown specs in `utilities/markdown`: `createBlockMarkdownSpec` and
//! `createAtomBlockMarkdownSpec`, which give a node Pandoc's `:::name {attributes}` blocks, and
//! the `parseAttributes` and `serializeAttributes` of their attributes.

use std::sync::LazyLock;

use tarnish_js::json::{Map, Value, json};
use tarnish_js::regexp::{Match, RegExp};
use tarnish_js::units::Units;
use tarnish_js::{self as js, Error, stack, utf16};
use tarnish_markdown::marked::{Lexer, Token};

use super::{Parsed, RenderContext, RenderHelpers};
use crate::NodeExtension;

/// What a block's content is made of.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlockContent {
    /// `content: 'block'`.
    Block,
    /// `content: 'inline'`.
    Inline,
}

impl NodeExtension {
    /// `...createBlockMarkdownSpec({ nodeName, content })`: the node is a `:::nodeName` block
    /// that ends at the `:::` its nested blocks leave open.
    pub fn block_markdown_spec(self, node_name: &'static str, content: BlockContent) -> Self {
        let opening = RegExp::new(&format!(r"^:::{node_name}(?:\s+\{{([^}}]*)\}})?\s*\n"), "");
        let starts = RegExp::new(&format!("^:::{node_name}"), "m");
        self.parse_markdown(move |token, helpers| {
            let tokens = token.tokens.as_deref().unwrap_or_default();
            let node_content = match content {
                BlockContent::Block => helpers.parse_children(tokens)?,
                BlockContent::Inline => helpers.parse_inline(tokens)?,
            };
            let attrs = token_attributes(token);
            Ok(Parsed::Node(helpers.create_node(
                node_name,
                Some(attrs),
                Some(node_content),
            )))
        })
        .markdown_block_tokenizer(
            move |src| starts.exec(src).map(|found| found.index()),
            move |src, _, lexer| tokenize_block(node_name, content, &opening, src, lexer),
        )
        .render_markdown(move |node, helpers, _| {
            let attrs = js::or(node.get("attrs"), &js::EMPTY_OBJECT);
            let attrs = serialize_attributes(attrs)?;
            let content = js::or(node.get("content"), &js::EMPTY_ARRAY);
            let rendered = helpers.render_children(content, "\n\n")?;
            Ok(format!(
                ":::{node_name}{}\n\n{rendered}\n\n:::",
                braced(&attrs)
            ))
        })
    }

    /// `...createAtomBlockMarkdownSpec({ nodeName, allowedAttributes })`: the node is a
    /// `:::nodeName {attributes} :::` line, which renders the allowed attributes in their order.
    pub fn atom_block_markdown_spec(
        self,
        node_name: &'static str,
        allowed_attributes: &'static [&'static str],
    ) -> Self {
        let atom = RegExp::new(
            &format!(r"^:::{node_name}(?:\s+\{{([^}}]*)\}})?\s*:::(?:\n|$)"),
            "",
        );
        let starts = RegExp::new(&format!(r"^:::{node_name}(?:\s|$)"), "m");
        self.parse_markdown(move |token, helpers| {
            let attrs = token_attributes(token);
            Ok(Parsed::Node(helpers.create_node(
                node_name,
                Some(attrs),
                Some(Vec::new()),
            )))
        })
        .markdown_block_tokenizer(
            move |src| starts.exec(src).map(|found| found.index()),
            move |src, _, _| {
                let Some(found) = atom.exec(src) else {
                    return Ok(None);
                };
                let attributes = parse_attributes(found.get(1).unwrap_or_default());
                Ok(Some(Token {
                    extra: Some(Box::new(Map::from_iter([(
                        "attributes".into(),
                        Value::Object(attributes),
                    )]))),
                    ..Token::new(node_name, src.slice_of(found.all()))
                }))
            },
        )
        .render_markdown(move |node, _: &dyn RenderHelpers, _: &RenderContext| {
            let attrs = js::or(node.get("attrs"), &js::EMPTY_OBJECT);
            let filtered = filter_attributes(attrs, allowed_attributes)?;
            let attrs = serialize_attributes(&Value::Object(filtered))?;
            Ok(format!(":::{node_name}{} :::", braced(&attrs)))
        })
    }
}

/// `attrs ? ` {${attrs}}` : ''`.
fn braced(attrs: &str) -> String {
    match attrs.is_empty() {
        true => String::new(),
        false => format!(" {{{attrs}}}"),
    }
}

/// `{ ...defaultAttributes, ...token.attributes }`, with no default attributes.
fn token_attributes(token: &Token) -> Map {
    match token.extra("attributes") {
        Some(Value::Object(attributes)) => attributes.clone(),
        _ => Map::new(),
    }
}

/// A match of `/^:::([\w-]*)(\s.*)?/gm`: an opening `:::name` or a closing `:::` at the start of
/// a line, with what follows it on the line, or on the next line for a `:::` that ends its own.
#[derive(Debug, PartialEq)]
struct BlockLine {
    index: usize,
    end: usize,
    /// Whether `[\w-]*` matched a name.
    named: bool,
    /// Whether `(\s.*)` matched, ending with `:::`.
    atom: bool,
}

/// The next [`BlockLine`] in `text` from `last_index`.
fn next_block_line(text: &[u16], last_index: usize) -> Option<BlockLine> {
    let colon = utf16::unit(b':');
    let index = (last_index..text.len().saturating_sub(2)).find(|&at| {
        (at == 0 || utf16::is_line_terminator(text[at - 1])) && text[at..at + 3] == [colon; 3]
    })?;
    let is_name = |unit: u16| {
        u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    };
    let name_end = (index + 3..text.len())
        .find(|&at| !is_name(text[at]))
        .unwrap_or(text.len());
    let rest_end = match text.get(name_end) {
        Some(&unit) if js::is_whitespace_unit(unit) => (name_end + 1..text.len())
            .find(|&at| utf16::is_line_terminator(text[at]))
            .unwrap_or(text.len()),
        _ => name_end,
    };
    let rest = &text[name_end..rest_end];
    Some(BlockLine {
        index,
        end: rest_end,
        named: name_end > index + 3,
        atom: rest.len() >= 3 && rest[rest.len() - 3..] == [colon; 3],
    })
}

/// The block spec's `tokenize(src, tokens, lexer)`.
fn tokenize_block(
    node_name: &'static str,
    content: BlockContent,
    opening: &RegExp,
    src: &Units,
    lexer: &mut Lexer,
) -> Result<Option<Token>, Error> {
    let Some(opening_match) = opening.exec(src) else {
        return Ok(None);
    };
    let attributes = parse_attributes(opening_match.get(1).unwrap_or_default());
    let position = opening_match.all().len();
    let remaining = src.substring(position);
    let mut level = 1;
    let mut last_index = 0;
    while let Some(found) = next_block_line(&remaining, last_index) {
        last_index = found.end;
        // An atom `:::` block, which this one holds.
        if found.atom {
            continue;
        }
        if found.named {
            level += 1;
            continue;
        }
        level -= 1;
        if level > 0 {
            continue;
        }
        let raw_content = remaining.slice(0..found.index);
        let matched_content = utf16::trim(&raw_content);
        let full_match = src.slice(0..position + found.end);
        let tokens = match matched_content.is_empty() {
            true => Vec::new(),
            false => match content {
                BlockContent::Block => block_tokens(&raw_content, lexer)?,
                BlockContent::Inline => lexer.inline_tokens(&src.slice_of(matched_content))?,
            },
        };
        let extra = Map::from_iter([
            ("attributes".into(), Value::Object(attributes)),
            ("content".into(), json!(utf16::to_string(matched_content))),
        ]);
        return Ok(Some(Token {
            tokens: Some(tokens.into()),
            extra: Some(Box::new(extra)),
            ..Token::new(node_name, full_match)
        }));
    }
    Ok(None)
}

/// Where a `g` regex's next search starts after `found`: past an empty match by one unit.
fn advance(found: &Match, text: &[u16]) -> usize {
    match found.end() == found.index() {
        true => (found.end() + 1).min(text.len() + 1),
        false => found.end(),
    }
}

/// `lexer.blockTokens(rawContent)`, each token with text and no tokens given its inline tokens
/// now, without the empty paragraphs it ends with.
fn block_tokens(raw_content: &Units, lexer: &mut Lexer) -> Result<Vec<Token>, Error> {
    let mut tokens = Vec::new();
    stack::grow(|| lexer.block_tokens(raw_content, &mut tokens, false))?;
    for token in &mut tokens {
        let text = token.text.clone().filter(|text| !text.is_empty());
        let has_tokens = token
            .tokens
            .as_ref()
            .is_some_and(|tokens| !tokens.is_empty());
        if let Some(text) = text.filter(|_| !has_tokens) {
            token.tokens = Some(lexer.inline_tokens(&text)?.into());
        }
    }
    while let Some(last) = tokens.last() {
        let blank = last.kind == "paragraph" && utf16::trim(last.text()).is_empty();
        if !blank {
            break;
        }
        tokens.pop();
    }
    Ok(tokens)
}

/// `filterAttributes(attrs)`: the allowed attributes `attrs` has, in the order they're allowed.
fn filter_attributes(attrs: &Value, allowed: &[&str]) -> Result<Map, Error> {
    let mut filtered = Map::new();
    for key in allowed {
        // `key in attrs`, which only an object can be asked.
        let value = match attrs {
            Value::Object(map) => map.get(key),
            Value::Array(_) => None,
            other => {
                return Err(Error::Type(format!(
                    "Cannot use 'in' operator to search for '{key}' in {}",
                    js::to_string(other)?
                )));
            }
        };
        if let Some(value) = value {
            filtered.insert((*key).into(), value.clone());
        }
    }
    Ok(filtered)
}

static QUOTED: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r#"["']([^"']*)["']"#, "g"));
static CLASS: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"(?:^|\s)\.([\w-]+)", "g"));
static ID: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"(?:^|\s)#([\w-]+)", ""));
static IDS: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"(?:^|\s)#([\w-]+)", "g"));
static KEY_VALUE: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(r"([a-zA-Z][\w-]*)\s*=\s*(__QUOTED_\d+__)", "g"));
static KEY_VALUE_REMOVED: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(r"([a-zA-Z][\w-]*)\s*=\s*__QUOTED_\d+__", "g"));
static QUOTED_INDEX: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"__QUOTED_(\d+)__", ""));
static WHITESPACE: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"\s+", ""));
static NAME: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^[a-zA-Z][\w-]*$", ""));

/// Every match of a `g` regex in `text`, as `matchAll` finds them.
fn all<'t>(regex: &RegExp, text: &'t [u16]) -> Vec<Match<'t>> {
    let mut matches = Vec::new();
    let mut last_index = 0;
    while let Some(found) = regex.exec_at(text, last_index) {
        last_index = advance(&found, text);
        matches.push(found);
    }
    matches
}

/// `parseAttributes(attrString)`: Pandoc's `.class #id key="value" flag`.
pub fn parse_attributes(attr_string: &[u16]) -> Map {
    let mut attributes = Map::new();
    if utf16::trim(attr_string).is_empty() {
        return attributes;
    }
    let mut quoted_strings: Vec<Vec<u16>> = Vec::new();
    let temp = QUOTED.replace_with(attr_string, |found| {
        quoted_strings.push(found.all().to_vec());
        utf16::from(&format!("__QUOTED_{}__", quoted_strings.len() - 1))
    });
    let classes = all(&CLASS, &temp);
    if !classes.is_empty() {
        let classes: Vec<&[u16]> = classes
            .iter()
            .map(|found| utf16::slice(utf16::trim(found.all()), 1, None))
            .collect();
        let classes = utf16::join(&classes, &utf16::from(" "));
        attributes.insert("class".into(), json!(utf16::to_string(&classes)));
    }
    if let Some(id) = ID.exec(&temp) {
        let id = id.get(1).unwrap_or_default();
        attributes.insert("id".into(), json!(utf16::to_string(id)));
    }
    for found in all(&KEY_VALUE, &temp) {
        let quoted_ref = found.get(2).unwrap_or_default();
        let digits = QUOTED_INDEX
            .exec(quoted_ref)
            .and_then(|index| index.get(1).map(utf16::to_string));
        let quoted_index = js::parse_int_radix(digits.as_deref().unwrap_or("0"), 10);
        let quoted_value = quoted_strings
            .get(quoted_index as usize)
            .filter(|value| !value.is_empty());
        if let Some(quoted_value) = quoted_value {
            let key = utf16::to_string(found.get(1).unwrap_or_default());
            let value = utf16::slice(quoted_value, 1, Some(-1));
            attributes.insert(key.into(), json!(utf16::to_string(value)));
        }
    }
    let clean = CLASS.replace(&temp, "");
    let clean = IDS.replace(&clean, "");
    let clean = KEY_VALUE_REMOVED.replace(&clean, "");
    let clean = utf16::trim(&clean);
    if !clean.is_empty() {
        let split = WHITESPACE.split(clean);
        for attr in split.into_iter().filter(|attr| !attr.is_empty()) {
            if NAME.test(attr) {
                attributes.insert(utf16::to_string(attr).into(), Value::Bool(true));
            }
        }
    }
    attributes
}

/// `serializeAttributes(attributes)`: the class's names, the id, and then each other attribute
/// as a flag for `true` or `key="value"` for anything but `false`, `null` and `undefined`.
pub fn serialize_attributes(attributes: &Value) -> Result<String, Error> {
    let entries = entries(attributes);
    if !js::truthy(Some(attributes)) || entries.is_empty() {
        return Ok(String::new());
    }
    let mut parts: Vec<String> = Vec::new();
    let property = |key: &str| match attributes {
        Value::Object(map) => map.get(key),
        _ => None,
    };
    if let Some(class) = property("class").filter(|class| js::truthy(Some(class))) {
        let class = utf16::from(&js::to_string(class)?);
        for name in WHITESPACE.split(&class) {
            if !name.is_empty() {
                parts.push(format!(".{}", utf16::to_string(name)));
            }
        }
    }
    if let Some(id) = property("id").filter(|id| js::truthy(Some(id))) {
        parts.push(format!("#{}", js::to_string(id)?));
    }
    for (key, value) in entries {
        if key == "class" || key == "id" {
            continue;
        }
        match value {
            Value::Bool(true) => parts.push(key),
            Value::Bool(false) | Value::Null => {}
            value => parts.push(format!("{key}=\"{}\"", js::to_string(&value)?)),
        }
    }
    Ok(parts.join(" "))
}

/// `Object.entries(value)`: an object's own entries, an array's items and a string's code units
/// by index, and nothing of another value.
fn entries(value: &Value) -> Vec<(String, Value)> {
    match value {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| (index.to_string(), item.clone()))
            .collect(),
        Value::String(string) => utf16::from(string)
            .into_iter()
            .enumerate()
            .map(|(index, unit)| (index.to_string(), json!(utf16::to_string(&[unit]))))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use tarnish_js::random::strings;
    use tarnish_js::regexp::RegExp;

    use super::{BlockLine, next_block_line};

    #[test]
    fn block_lines_match_their_regex() {
        let regex = RegExp::new(r"^:::([\w-]*)(\s.*)?", "gm");
        let alphabet = &[
            ":::", ":", "::", "\n", "\r", "\u{2028}", " ", "\t", "\u{A0}", "a", "-", "_", "é",
            "{x}", ":::\n",
        ];
        for text in strings(alphabet, 100_000) {
            let mut last_index = 0;
            loop {
                let expected = regex.exec_at(&text, last_index).map(|found| BlockLine {
                    index: found.index(),
                    end: found.end(),
                    named: found.get(1).is_some_and(|name| !name.is_empty()),
                    atom: found
                        .get(2)
                        .is_some_and(|rest| rest.ends_with(&[58, 58, 58])),
                });
                let actual = next_block_line(&text, last_index);
                assert_eq!(actual, expected, "{:?}", String::from_utf16_lossy(&text));
                match actual {
                    Some(found) => last_index = found.end,
                    None => break,
                }
            }
        }
    }
}
