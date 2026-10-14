//! The Markdown API `@tiptap/core` declares for extensions in `src/types.ts`: what an
//! extension's `renderMarkdown`, `parseMarkdown` and `markdownTokenizer` take and return.
//! `@tiptap/markdown`'s `MarkdownManager` provides the helpers.

use std::sync::Arc;

use super::ParseHelpers;
use tarnish_js::Error;
use tarnish_js::json::{Map, Value};
use tarnish_js::units::Units;
use tarnish_markdown::marked::{BlockStart, Lexer, Token};

/// An extension's `renderMarkdown(node, helpers, context)`.
pub type RenderMarkdown =
    Arc<dyn Fn(&Value, &dyn RenderHelpers, &RenderContext) -> Result<String, Error> + Send + Sync>;

/// `MarkdownRendererHelpers`.
pub trait RenderHelpers {
    /// `helpers.renderChildren(nodes, separator)`: an array of nodes, or a node's `content`.
    fn render_children(&self, nodes: &Value, separator: &str) -> Result<String, Error>;

    /// `helpers.renderChildren(nodes, separator)` for nodes taken out of an array.
    fn render_array(&self, nodes: &[Value], separator: &str) -> Result<String, Error>;

    /// `helpers.renderChild(node, index)`: a node as the child at `index`.
    fn render_child(&self, node: &Value, index: usize) -> Result<String, Error>;

    /// `helpers.indent(content)`.
    fn indent(&self, content: &str) -> String;
}

/// A renderer's `context`.
pub struct RenderContext<'a> {
    /// The node's index among the nodes it was rendered with.
    pub index: usize,
    /// How many of the node's ancestors indent their content.
    pub level: usize,
    /// `parentType`, the parent's `type`.
    pub parent_type: Option<&'a Value>,
    pub previous_node: Option<&'a Value>,
    /// `meta.parentAttrs`, the parent's `attrs`.
    pub parent_attrs: Option<&'a Value>,
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

/// An extension's `markdownTokenizer`.
#[derive(Clone)]
pub(crate) struct MarkdownTokenizer {
    pub start: Start,
    pub tokenize: Tokenize,
}

/// A tokenizer's `level` and `start`.
#[derive(Clone)]
pub(crate) enum Start {
    /// Inline, the level Tiptap defaults to, starting where the string starts in `src`.
    Inline(&'static str),
    /// Block, starting where `start(src)` says.
    Block(BlockStart),
}

/// `tokenize(src, tokens, lexer)`.
pub(crate) type Tokenize =
    Arc<dyn Fn(&Units, &[Token], &mut Lexer) -> Result<Option<Token>, Error> + Send + Sync>;
