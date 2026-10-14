//! `@tiptap/markdown` 3.30.0: the `MarkdownManager`, which parses Markdown with marked and the
//! extensions' `parseMarkdown`, and serializes with their `renderMarkdown`, and the Markdown API
//! `@tiptap/core` declares for extensions, with its utilities for Markdown that nests.

mod nesting;
mod parse;
mod serialize;
mod types;
mod utils;

pub use nesting::{
    IndentedBlockConfig, IndentedBlocks, parse_indented_blocks, render_nested_markdown_content,
};
pub use parse::ParseHelpers;
pub use types::*;

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{Extension, Kind, sort_extensions};
use tarnish_js::units::Units;
use tarnish_markdown::marked::{BlockExtension, Lexer, Marked, Token, TokenizerExtension, Tokens};

/// What `registerExtension` keeps of an extension.
#[derive(Clone)]
struct Spec {
    parse: Option<ParseMarkdown>,
    render: Option<RenderMarkdown>,
    html_reopen: Option<(&'static str, &'static str)>,
    /// `isIndenting`.
    indents_content: bool,
}

/// The manager's `indentation` option: what `helpers.indent` puts before content.
#[derive(Clone, Copy, Debug)]
pub struct Indentation {
    pub style: IndentStyle,
    pub size: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndentStyle {
    Space,
    Tab,
}

impl Default for Indentation {
    fn default() -> Self {
        Indentation {
            style: IndentStyle::Space,
            size: 2,
        }
    }
}

pub struct MarkdownManager {
    marked: Marked,
    /// Parsers by marked token type.
    registry: FxHashMap<&'static str, Vec<Spec>>,
    /// Renderers by node or mark type.
    node_type_registry: FxHashMap<&'static str, Vec<Spec>>,
    /// Registration order, which ranks marks: a lower rank opens outside a higher one.
    extension_ranks: FxHashMap<&'static str, usize>,
    /// The node and mark types that are code.
    code_types: FxHashSet<&'static str>,
    /// `indentString`.
    indent_string: String,
}

impl MarkdownManager {
    /// `new MarkdownManager({ extensions, marked })`: registers the extensions in the order
    /// `sortExtensions` puts them, adding their tokenizers to `marked`.
    pub fn new(extensions: &[Extension], marked: Marked) -> Self {
        let mut manager = MarkdownManager {
            marked,
            registry: FxHashMap::default(),
            node_type_registry: FxHashMap::default(),
            extension_ranks: FxHashMap::default(),
            code_types: FxHashSet::default(),
            indent_string: String::new(),
        };
        manager.set_indentation(Indentation::default());
        for extension in sort_extensions(extensions) {
            manager.register_extension(extension);
        }
        manager
    }

    /// `new MarkdownManager({ extensions, marked, indentation })`.
    pub fn with_indentation(mut self, indentation: Indentation) -> Self {
        self.set_indentation(indentation);
        self
    }

    fn set_indentation(&mut self, Indentation { style, size }: Indentation) {
        let character = match style {
            IndentStyle::Space => " ",
            IndentStyle::Tab => "\t",
        };
        self.indent_string = character.repeat(size);
    }

    fn register_extension(&mut self, extension: &Extension) {
        let code = match &extension.kind {
            Kind::Node(node) => node.code,
            Kind::Mark(mark) => mark.code,
            Kind::Extension => false,
        };
        if code {
            self.code_types.insert(extension.name);
        }
        let rank = self.extension_ranks.len();
        self.extension_ranks.entry(extension.name).or_insert(rank);
        let markdown = &extension.markdown;
        let token_name = markdown.token_name.unwrap_or(extension.name);
        let spec = Spec {
            parse: markdown.parse.clone(),
            render: markdown.render.clone(),
            html_reopen: markdown.html_reopen,
            indents_content: markdown.indents_content,
        };
        if spec.parse.is_some() {
            self.registry
                .entry(token_name)
                .or_default()
                .push(spec.clone());
        }
        if spec.render.is_some() {
            self.node_type_registry
                .entry(extension.name)
                .or_default()
                .push(spec);
        }
        if let Some(tokenizer) = &markdown.tokenizer {
            self.register_tokenizer(tokenizer);
        }
    }

    /// `registerTokenizer`: `tokenize` as a marked extension.
    fn register_tokenizer(&mut self, MarkdownTokenizer { start, tokenize }: &MarkdownTokenizer) {
        let tokenize = Arc::clone(tokenize);
        let tokenizer = Box::new(move |lexer: &mut Lexer, src: &Units, tokens: &[Token]| {
            Ok(tokenize(src, tokens, lexer)?.map(|mut token| {
                token.tokens.get_or_insert_with(Tokens::default);
                token
            }))
        });
        match start {
            Start::Inline(start) => self
                .marked
                .use_extension(TokenizerExtension { start, tokenizer }),
            Start::Block(start) => {
                let start = Arc::clone(start);
                self.marked.use_block_extension(BlockExtension {
                    start: Some(Box::new(move |src: &Units| start(src))),
                    tokenizer,
                });
            }
        }
    }

    /// The marked the manager parses with, which holds the extensions' tokenizers.
    pub fn marked(&self) -> &Marked {
        &self.marked
    }

    /// `getHandlersForToken(type)`.
    fn handlers_for_token(&self, kind: &str) -> &[Spec] {
        self.registry.get(kind).map_or(&[], Vec::as_slice)
    }

    /// `getHandlerForToken(type)`: the first parser of the token type, or else the first
    /// renderer of the node type.
    fn handler_for_token(&self, kind: &str) -> Option<&Spec> {
        self.handlers_for_token(kind)
            .first()
            .or_else(|| self.handler_for_node_type(kind))
    }

    /// `getHandlersForNodeType(type)[0]`.
    fn handler_for_node_type(&self, kind: &str) -> Option<&Spec> {
        self.node_type_registry
            .get(kind)
            .and_then(|specs| specs.first())
    }
}
