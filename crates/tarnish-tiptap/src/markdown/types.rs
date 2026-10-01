//! The Markdown API `@tiptap/core` declares for extensions in `src/types.ts`: what an
//! extension's `renderMarkdown`, `parseMarkdown` and `markdownTokenizer` take and return.
//! `@tiptap/markdown`'s `MarkdownManager` provides the helpers.

use std::sync::Arc;

use super::ParseHelpers;
use tarnish_js::Error;
use tarnish_js::json::{Map, Value};
use tarnish_js::units::Units;
use tarnish_markdown::marked::{Lexer, Token};

/// An extension's `renderMarkdown(node, helpers, context)`.
pub type RenderMarkdown =
    Arc<dyn Fn(&Value, &dyn RenderHelpers, &RenderContext) -> Result<String, Error> + Send + Sync>;

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
pub type ParseMarkdown =
    Arc<dyn Fn(&Token, &ParseHelpers<'_>) -> Result<Parsed, Error> + Send + Sync>;

/// What `parseMarkdown` returns.
pub enum Parsed {
    Node(Value),
    Nodes(Vec<Value>),
    /// `helpers.applyMark(mark, content, attrs)`.
    Mark {
        mark: &'static str,
        content: Vec<Value>,
        attrs: Option<Map>,
    },
}

/// An extension's `markdownTokenizer`. It is inline, the `level` Tiptap defaults to, and its
/// `start` is where the string starts in `src`.
#[derive(Clone)]
pub(crate) struct MarkdownTokenizer {
    pub start: &'static str,
    pub tokenize: Tokenize,
}

/// `tokenize(src, tokens, lexer)`.
pub(crate) type Tokenize =
    Arc<dyn Fn(&Units, &[Token], &mut Lexer) -> Result<Option<Token>, Error> + Send + Sync>;
