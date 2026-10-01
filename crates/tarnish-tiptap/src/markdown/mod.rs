//! `@tiptap/markdown` 3.30.0: the `MarkdownManager`, which parses Markdown with marked and the
//! extensions' `parseMarkdown`, and serializes with their `renderMarkdown`, and the Markdown API
//! `@tiptap/core` declares for extensions.

mod parse;
mod serialize;
mod types;
mod utils;

pub use parse::ParseHelpers;
pub use types::*;

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::{Extension, sort_extensions};
use tarnish_js::units::Units;
use tarnish_markdown::marked::{Lexer, Marked, Token, TokenizerExtension, Tokens};

/// What `registerExtension` keeps of an extension.
#[derive(Clone)]
struct Spec {
    parse: Option<ParseMarkdown>,
    render: Option<RenderMarkdown>,
    html_reopen: Option<(&'static str, &'static str)>,
}

pub struct MarkdownManager {
    marked: Marked,
    /// Parsers by marked token type.
    registry: FxHashMap<&'static str, Vec<Spec>>,
    /// Renderers by node or mark type.
    node_type_registry: FxHashMap<&'static str, Vec<Spec>>,
    /// Registration order, which ranks marks: a lower rank opens outside a higher one.
    extension_ranks: FxHashMap<&'static str, usize>,
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
        };
        for extension in sort_extensions(extensions) {
            manager.register_extension(extension);
        }
        manager
    }

    fn register_extension(&mut self, extension: &Extension) {
        let rank = self.extension_ranks.len();
        self.extension_ranks.entry(extension.name).or_insert(rank);
        let markdown = &extension.markdown;
        let token_name = markdown.token_name.unwrap_or(extension.name);
        let spec = Spec {
            parse: markdown.parse.clone(),
            render: markdown.render.clone(),
            html_reopen: markdown.html_reopen,
        };
        if spec.parse.is_some() {
            assert_ne!(
                token_name, "taskList",
                "parseListToken's task list grouping isn't ported"
            );
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
        self.marked
            .use_extension(TokenizerExtension { start, tokenizer });
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
