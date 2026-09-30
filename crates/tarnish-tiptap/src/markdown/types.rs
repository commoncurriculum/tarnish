//! The Markdown API `@tiptap/core` declares for extensions in `src/types.ts`: what an
//! extension's `renderMarkdown`, `parseMarkdown` and `markdownTokenizer` take and return.
//! `@tiptap/markdown`'s `MarkdownManager` provides the helpers.

use tarnish::json::{Map, Value};
use tarnish_js::Error;
use tarnish_js::units::Units;
use tarnish_markdown::marked::Token;

/// An extension's `renderMarkdown(node, helpers, context)`.
pub type RenderMarkdown = fn(&Value, &dyn RenderHelpers, &RenderContext) -> Result<String, Error>;

/// `MarkdownRendererHelpers`.
pub trait RenderHelpers {
    /// `helpers.renderChildren(nodes, separator)`: an array of nodes, or a node's `content`.
    fn render_children(&self, nodes: &Value, separator: &str) -> Result<String, Error>;

    /// `helpers.renderChildren(nodes, separator)` for nodes taken out of an array.
    fn render_array(&self, nodes: &[Value], separator: &str) -> Result<String, Error>;
}

/// A renderer's `context`, as far as our renderers read it.
pub struct RenderContext<'a> {
    pub previous_node: Option<&'a Value>,
}

/// An extension's `parseMarkdown(token, helpers)`.
pub type ParseMarkdown = fn(&Token, &dyn ParseHelpers) -> Result<Parsed, Error>;

/// `MarkdownParseHelpers`.
pub trait ParseHelpers {
    /// `helpers.parseInline(tokens)`.
    fn parse_inline(&self, tokens: &[Token]) -> Result<Vec<Value>, Error>;

    /// `helpers.tokenizeInline(src)`.
    fn tokenize_inline(&self, src: &[u16]) -> Result<Vec<Token>, Error>;

    /// `helpers.parseChildren(tokens)`.
    fn parse_children(&self, tokens: &[Token]) -> Result<Vec<Value>, Error>;

    /// `helpers.createNode(type, attrs, content)`.
    fn create_node(&self, kind: &str, attrs: Option<Map>, content: Option<Vec<Value>>) -> Value;

    /// `helpers.applyMark(markType, content, attrs)`.
    fn apply_mark(&self, mark: &'static str, content: Vec<Value>, attrs: Option<Map>) -> Parsed;
}

/// What `parseMarkdown` returns.
pub enum Parsed {
    Node(Value),
    Nodes(Vec<Value>),
    /// `helpers.applyMark(mark, content, attrs)`.
    Mark {
        mark: &'static str,
        content: Vec<Value>,
        attrs: Option<Value>,
    },
}

/// An extension's `markdownTokenizer`. It is inline, the `level` Tiptap defaults to, and its
/// `start` is where the string starts in `src`.
#[derive(Clone, Copy)]
pub struct MarkdownTokenizer {
    pub start: &'static str,
    pub tokenize: Tokenize,
}

/// `tokenize(src, tokens, helpers)`.
pub type Tokenize = fn(&Units, &[Token], &mut dyn TokenizerHelpers) -> Result<Option<Token>, Error>;

/// What `tokenize` gets as `helpers`.
pub trait TokenizerHelpers {
    fn inline_tokens(&mut self, src: &Units) -> Result<Vec<Token>, Error>;
}
