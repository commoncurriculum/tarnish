//! `MarkdownManager.parse`: marked's tokens to JSON, through the extensions' `parseMarkdown`.

use std::cell::RefCell;
use std::sync::LazyLock;

use tarnish_js::stack;

use super::MarkdownManager;
use super::utils::extract_absorbed_blank_lines;
use crate::markdown::Parsed;
use crate::{decode_html_entities, marks_equal};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::regexp::RegExp;
use tarnish_js::units::Units;
use tarnish_js::{self as js, Error, utf16};
use tarnish_markdown::marked::{Lexer, Token};

impl MarkdownManager {
    /// `markdownManager.parse(markdown)`.
    pub fn parse(&self, markdown: &str) -> Result<Value, Error> {
        let mut lexer = Lexer::new(&self.marked);
        let tokens = lexer.lex(&utf16::from(markdown))?;
        let helpers = ParseHelpers {
            manager: self,
            lexer: RefCell::new(lexer),
        };
        let content = helpers.parse_tokens(&tokens, true)?;
        Ok(json!({ "type": "doc", "content": content }))
    }
}

/// `MarkdownParseHelpers`, what `parseMarkdown` gets as `helpers`, holding the lexer the parse
/// started with.
pub struct ParseHelpers<'a> {
    manager: &'a MarkdownManager,
    lexer: RefCell<Lexer<'a>>,
}

static CLOSING_TAG: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^<\/[\s]*[\w-]+", "i"));
static OPENING_TAG: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(r"^<[\s]*([\w-]+)(\s|>|\/|$)", "i"));
static SELF_CLOSING: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"\/>$", ""));
static REGEX_SPECIAL: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"[.*+?^${}()|[\]\\]", "g"));

impl ParseHelpers<'_> {
    /// `helpers.tokenizeInline(src)`.
    pub fn tokenize_inline(&self, src: &[u16]) -> Result<Vec<Token>, Error> {
        self.lexer.borrow_mut().inline_tokens(&Units::from(src))
    }

    /// `helpers.parseChildren(tokens)`.
    pub fn parse_children(&self, tokens: &[Token]) -> Result<Vec<Value>, Error> {
        self.parse_tokens(tokens, false)
    }

    /// `helpers.createNode(type, attrs, content)`.
    pub fn create_node(
        &self,
        kind: &str,
        attrs: Option<Map>,
        content: Option<Vec<Value>>,
    ) -> Value {
        let mut node = Map::with_capacity(3);
        node.push("type".into(), json!(kind));
        if let Some(attrs) = attrs.filter(|attrs| !attrs.is_empty()) {
            node.push("attrs".into(), Value::Object(attrs));
        }
        if let Some(content) = content {
            node.push("content".into(), Value::Array(content));
        }
        Value::Object(node)
    }

    /// `helpers.applyMark(markType, content, attrs)`.
    pub fn apply_mark(
        &self,
        mark: &'static str,
        content: Vec<Value>,
        attrs: Option<Map>,
    ) -> Parsed {
        Parsed::Mark {
            mark,
            content,
            attrs: attrs.filter(|attrs| !attrs.is_empty()),
        }
    }

    /// `parseTokens(tokens, parseImplicitEmptyParagraphs)`.
    fn parse_tokens(
        &self,
        tokens: &[Token],
        implicit_paragraphs: bool,
    ) -> Result<Vec<Value>, Error> {
        let normalized = if implicit_paragraphs {
            extract_absorbed_blank_lines(tokens)
        } else {
            tokens.iter().map(std::borrow::Cow::Borrowed).collect()
        };
        let non_space: Vec<usize> = normalized
            .iter()
            .enumerate()
            .filter(|(_, token)| token.kind != "space")
            .map(|(index, _)| index)
            .collect();
        let mut previous_non_space: Option<usize> = None;
        let mut next_pointer = 0;
        let mut nodes = Vec::new();
        for (index, token) in normalized.iter().enumerate() {
            while next_pointer < non_space.len() && non_space[next_pointer] < index {
                previous_non_space = Some(non_space[next_pointer]);
                next_pointer += 1;
            }
            if implicit_paragraphs && token.kind == "space" {
                let next_non_space = non_space.get(next_pointer);
                let separators = count_paragraph_separators(&token.raw);
                let boundary = previous_non_space.is_none() || next_non_space.is_none();
                let count = if boundary {
                    separators
                } else {
                    separators.saturating_sub(1)
                };
                nodes.extend((0..count).map(|_| json!({ "type": "paragraph", "content": [] })));
                continue;
            }
            nodes.extend(stack::grow(|| {
                self.parse_token(token, implicit_paragraphs)
            })?);
        }
        Ok(nodes)
    }

    /// `parseToken(token, parseImplicitEmptyParagraphs)`.
    fn parse_token(&self, token: &Token, implicit_paragraphs: bool) -> Result<Vec<Value>, Error> {
        if token.kind.is_empty() {
            return Ok(Vec::new());
        }
        // `parseListToken`, whose task list grouping needs a taskList extension, which none is.
        if token.kind == "list" {
            return self.parse_token_with_handlers(token);
        }
        match self.parse_with_handlers(token)? {
            Some(nodes) => Ok(nodes),
            None => self.parse_fallback_token(token, implicit_paragraphs),
        }
    }

    /// The first result of the token's handlers that holds something.
    fn parse_with_handlers(&self, token: &Token) -> Result<Option<Vec<Value>>, Error> {
        for handler in self.manager.handlers_for_token(token.kind) {
            let Some(parse) = &handler.parse else {
                continue;
            };
            let nodes = match parse(token, self)? {
                Parsed::Node(node) => vec![node],
                Parsed::Nodes(nodes) | Parsed::Mark { content: nodes, .. } => nodes,
            };
            if !nodes.is_empty() {
                return Ok(Some(nodes));
            }
        }
        Ok(None)
    }

    /// `parseTokenWithHandlers`.
    fn parse_token_with_handlers(&self, token: &Token) -> Result<Vec<Value>, Error> {
        match self.parse_with_handlers(token)? {
            Some(nodes) => Ok(nodes),
            None => self.parse_fallback_token(token, false),
        }
    }

    /// The nodes an inline token of the type `kind` parses to: through its extension's `parse`,
    /// or else its children's.
    fn parse_inline_token(&self, token: &Token, kind: &str) -> Result<Vec<Value>, Error> {
        let parse = self
            .manager
            .handler_for_token(kind)
            .and_then(|spec| spec.parse.as_ref());
        Ok(match parse {
            Some(parse) => match parse(token, self)? {
                Parsed::Mark {
                    mark,
                    content,
                    attrs,
                } => apply_mark_to_content(mark, content, attrs.as_ref()),
                Parsed::Node(node) => vec![node],
                Parsed::Nodes(nodes) => nodes,
            },
            None => match &token.tokens {
                Some(children) => self.parse_inline(children)?,
                None => Vec::new(),
            },
        })
    }

    /// `helpers.parseInline(tokens)`, which is `parseInlineTokens`: inline tokens to text and
    /// inline nodes, with marks on the text, and adjacent text with the same marks joined.
    pub fn parse_inline(&self, tokens: &[Token]) -> Result<Vec<Value>, Error> {
        let mut result: Vec<Value> = Vec::new();
        let mut index = 0;
        while index < tokens.len() {
            let token = &tokens[index];
            match token.kind {
                "text" => result.push(text_node(decode_html_entities(utf16::to_string(
                    token.text(),
                )))),
                "escape" => result.push(text_node(utf16::to_string(token.text()))),
                "html" => {
                    let raw = &token.raw;
                    let open = OPENING_TAG.exec(raw);
                    if let Some(open) =
                        open.filter(|_| !CLOSING_TAG.test(raw) && !SELF_CLOSING.test(raw))
                    {
                        let tag_name = open.get(1).unwrap_or_default();
                        let escaped = REGEX_SPECIAL.replace(tag_name, "\\$&");
                        let closing =
                            RegExp::new(&format!(r"^<\/\s*{}\b", utf16::to_string(&escaped)), "i");
                        let mut parts: Vec<&[u16]> = vec![raw];
                        let found = tokens[index + 1..].iter().position(|next| {
                            parts.push(&next.raw);
                            next.kind == "html" && closing.test(&next.raw)
                        });
                        if let Some(offset) = found {
                            let merged = parts.concat();
                            result.extend(parse_html_token(&merged, false));
                            index += offset + 2;
                            continue;
                        }
                    }
                    result.extend(parse_html_token(html_of(token), token.block()));
                }
                kind if !kind.is_empty() => {
                    result.extend(stack::grow(|| self.parse_inline_token(token, kind))?)
                }
                _ => {}
            }
            index += 1;
        }
        let mut joined: Vec<Value> = Vec::with_capacity(result.len());
        for node in result.into_iter().rev() {
            match joined.last_mut() {
                Some(next)
                    if next["type"] == "text"
                        && node["type"] == "text"
                        && marks_equal(
                            js::array(next.get("marks")),
                            js::array(node.get("marks")),
                        ) =>
                {
                    let text = format!(
                        "{}{}",
                        node["text"].as_str().unwrap_or_default(),
                        next["text"].as_str().unwrap_or_default()
                    );
                    let mut node = node;
                    node["text"] = json!(text);
                    *next = node;
                }
                _ => joined.push(node),
            }
        }
        joined.reverse();
        Ok(joined)
    }

    /// `parseFallbackToken`.
    fn parse_fallback_token(
        &self,
        token: &Token,
        implicit_paragraphs: bool,
    ) -> Result<Vec<Value>, Error> {
        let inline = |token: &Token| match &token.tokens {
            Some(tokens) => self.parse_inline(tokens),
            None => Ok(Vec::new()),
        };
        Ok(match token.kind {
            "paragraph" => vec![json!({ "type": "paragraph", "content": inline(token)? })],
            "heading" => vec![json!({
                "type": "heading",
                "attrs": { "level": token.depth().filter(|&depth| depth != 0).unwrap_or(1) },
                "content": inline(token)?,
            })],
            "text" => vec![text_node(decode_html_entities(utf16::to_string(
                token.text(),
            )))],
            "html" => parse_html_token(html_of(token), token.block())
                .into_iter()
                .collect(),
            "escape" => vec![text_node(utf16::to_string(token.text()))],
            "space" => Vec::new(),
            _ => match &token.tokens {
                Some(tokens) => self.parse_tokens(tokens, implicit_paragraphs)?,
                None => Vec::new(),
            },
        })
    }
}

fn text_node(text: String) -> Value {
    json!({ "type": "text", "text": text })
}

/// `countParagraphSeparators(raw)`: the `\n\n`s, each counted once, once `\r\n`s are `\n`s.
fn count_paragraph_separators(raw: &[u16]) -> usize {
    let (newline, carriage_return) = (utf16::unit(b'\n'), utf16::unit(b'\r'));
    let mut count = 0;
    let mut after_newline = false;
    for (index, &unit) in raw.iter().enumerate() {
        if unit == carriage_return && raw.get(index + 1) == Some(&newline) {
            continue;
        }
        if unit != newline {
            after_newline = false;
        } else if after_newline {
            count += 1;
            after_newline = false;
        } else {
            after_newline = true;
        }
    }
    count
}

/// `token.text || token.raw`.
fn html_of(token: &Token) -> &[u16] {
    token
        .text
        .as_deref()
        .filter(|text| !text.is_empty())
        .unwrap_or(&token.raw)
}

/// `parseHTMLToken`. The worker has no `window.DOMParser`, so HTML in Markdown stays literal
/// text: a paragraph of it for a block, else a text node.
fn parse_html_token(html: &[u16], block: bool) -> Option<Value> {
    if utf16::trim(html).is_empty() {
        return None;
    }
    let text = utf16::to_string(utf16::trim_end(html));
    if text.is_empty() {
        return None;
    }
    Some(if block {
        json!({ "type": "paragraph", "content": [text_node(text)] })
    } else {
        text_node(text)
    })
}

/// `applyMarkToContent(markType, content, attrs)`.
fn apply_mark_to_content(mark: &str, content: Vec<Value>, attrs: Option<&Map>) -> Vec<Value> {
    content
        .into_iter()
        .map(|mut node| {
            let Value::Object(object) = &mut node else {
                return node;
            };
            if object.get("type").and_then(Value::as_str) == Some("text") {
                let mut new_mark = Map::new();
                new_mark.insert("type".into(), json!(mark));
                if let Some(attrs) = attrs {
                    new_mark.insert("attrs".into(), Value::Object(attrs.clone()));
                }
                // The node is this call's, so its marks grow in place: copying them, as the JS
                // does its array, would copy every mark of n nested ones n times.
                match object.get_mut("marks") {
                    Some(Value::Array(marks)) => marks.push(Value::Object(new_mark)),
                    _ => {
                        object.insert("marks".into(), json!([new_mark]));
                    }
                }
            } else {
                match object.get_mut("content") {
                    Some(Value::Array(children)) => {
                        let children = std::mem::take(children);
                        let children = stack::grow(|| apply_mark_to_content(mark, children, attrs));
                        object.insert("content".into(), Value::Array(children));
                    }
                    _ => {
                        object.shift_remove("content");
                    }
                }
            }
            node
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use tarnish_js::json::{Value, json};
    use tarnish_js::random::strings;
    use tarnish_js::regexp::RegExp;
    use tarnish_js::stack::on_dirty_scheduler_stack;

    /// A mark around inline nodes that hold content, nested far deeper than a small stack could
    /// recurse through, as an extension's may: the mark goes on the text at the bottom.
    #[test]
    fn marks_apply_to_content_as_deep_as_memory_allows() {
        on_dirty_scheduler_stack(|| {
            const DEPTH: usize = 20_000;
            let text = json!({"type": "text", "text": "x"});
            let nested =
                (0..DEPTH).fold(text, |inner, _| json!({"type": "span", "content": [inner]}));
            let mut marked = super::apply_mark_to_content("bold", vec![nested], None);
            let mut depth = 0;
            while let Some(Value::Array(content)) = marked[0].get_mut("content") {
                marked = std::mem::take(content);
                depth += 1;
            }
            assert_eq!(depth, DEPTH);
            let text = json!({"type": "text", "text": "x", "marks": [{"type": "bold"}]});
            assert_eq!(marked, vec![text]);
        });
    }

    #[test]
    fn count_paragraph_separators_matches_its_regexes() {
        let crlf = RegExp::new(r"\r\n", "g");
        let double_newline = RegExp::new(r"\n\n", "g");
        for raw in strings(&["a", "\n", "\n", "\r", "\r\n", " "], 200_000) {
            let raw_lf = crlf.replace(&raw, "\n");
            let mut expected = 0;
            let mut last_index = 0;
            while let Some(end) = double_newline
                .exec_at(&raw_lf, last_index)
                .map(|found| found.end())
            {
                expected += 1;
                last_index = end;
            }
            assert_eq!(super::count_paragraph_separators(&raw), expected, "{raw:?}");
        }
    }
}
