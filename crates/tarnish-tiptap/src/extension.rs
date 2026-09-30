//! `Node.create`, `Mark.create` and `Extension.create`, and the configs they build, which
//! `getSchema` and the `MarkdownManager` read. `.extend({...})` carries on the parent's builder:
//! a setter replaces the parent's field, and an `extend_` method adds to it, as a field that
//! spreads `this.parent()` does.

use std::sync::Arc;

use super::attributes::{ExtensionAttribute, GlobalAttributes};
use super::markdown::{MarkdownTokenizer, ParseMarkdown, RenderMarkdown};
use super::parse_html::ParseHtml;
use crate::{DomSpec, SpecAttrs};
use tarnish::{Error, Mark, Node};

/// A node's `renderHTML`, given the node and its rendered attributes.
pub type RenderNode = Arc<
    dyn for<'a> Fn(&'a Node<'static>, SpecAttrs<'a>) -> Result<DomSpec<'a>, Error> + Send + Sync,
>;
/// A mark's `renderHTML`, given the mark and its rendered attributes.
pub type RenderMark =
    Arc<dyn for<'a> Fn(&'a Mark<'static>, SpecAttrs<'a>) -> DomSpec<'a> + Send + Sync>;

/// An extension's config. `K` is what its kind adds: [`NodeConfig`] for a node, [`MarkConfig`]
/// for a mark, nothing for a plain extension, and [`Kind`] once it is any of them.
pub struct Extension<K = Kind> {
    pub name: &'static str,
    pub priority: i32,
    pub kind: K,
    /// `addAttributes`.
    pub attributes: Vec<ExtensionAttribute>,
    /// `addGlobalAttributes`.
    pub global_attributes: Vec<GlobalAttributes>,
    /// `parseHTML`.
    pub parse_html: Vec<ParseHtml>,
    pub markdown: Markdown,
}

/// `Node.create(config)`.
pub type NodeExtension = Extension<NodeConfig>;
/// `Mark.create(config)`.
pub type MarkExtension = Extension<MarkConfig>;
/// `Extension.create(config)`.
pub type PlainExtension = Extension<()>;

pub enum Kind {
    Node(NodeConfig),
    Mark(MarkConfig),
    Extension,
}

#[derive(Default)]
pub struct NodeConfig {
    pub content: &'static str,
    pub group: &'static str,
    pub inline: bool,
    pub marks: Option<&'static str>,
    pub linebreak_replacement: bool,
    pub render_html: Option<RenderNode>,
}

#[derive(Default)]
pub struct MarkConfig {
    pub render_html: Option<RenderMark>,
}

/// The fields `@tiptap/markdown` reads from an extension.
#[derive(Default)]
pub struct Markdown {
    /// `markdownTokenName`: the marked token type `parseMarkdown` handles, when not the name.
    pub token_name: Option<&'static str>,
    /// `markdownTokenizer`.
    pub tokenizer: Option<MarkdownTokenizer>,
    /// `parseMarkdown`.
    pub parse: Option<ParseMarkdown>,
    /// `renderMarkdown`.
    pub render: Option<RenderMarkdown>,
    /// `markdownOptions.htmlReopen`: the HTML that reopens the mark where Markdown can't.
    pub html_reopen: Option<(&'static str, &'static str)>,
}

impl<K: Default> Extension<K> {
    pub fn create(name: &'static str) -> Self {
        Extension {
            name,
            priority: 100,
            kind: K::default(),
            attributes: Vec::new(),
            global_attributes: Vec::new(),
            parse_html: Vec::new(),
            markdown: Markdown::default(),
        }
    }
}

impl<K> Extension<K> {
    fn map_kind(self, kind: impl FnOnce(K) -> Kind) -> Extension {
        Extension {
            name: self.name,
            priority: self.priority,
            kind: kind(self.kind),
            attributes: self.attributes,
            global_attributes: self.global_attributes,
            parse_html: self.parse_html,
            markdown: self.markdown,
        }
    }

    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn add_attributes(mut self, attributes: Vec<ExtensionAttribute>) -> Self {
        self.attributes = attributes;
        self
    }

    /// `addAttributes() { return { ...this.parent?.(), ...attributes } }`: an attribute replaces
    /// the parent's of the same name where that one was.
    pub fn extend_attributes(mut self, attributes: Vec<ExtensionAttribute>) -> Self {
        for attribute in attributes {
            match self
                .attributes
                .iter_mut()
                .find(|parent| parent.name == attribute.name)
            {
                Some(parent) => *parent = attribute,
                None => self.attributes.push(attribute),
            }
        }
        self
    }

    pub fn add_global_attributes(mut self, global_attributes: Vec<GlobalAttributes>) -> Self {
        self.global_attributes = global_attributes;
        self
    }

    pub fn parse_html(mut self, rules: Vec<ParseHtml>) -> Self {
        self.parse_html = rules;
        self
    }

    /// `parseHTML() { return [...this.parent?.(), ...rules] }`.
    pub fn extend_parse_html(mut self, rules: Vec<ParseHtml>) -> Self {
        self.parse_html.extend(rules);
        self
    }

    pub fn markdown_token_name(mut self, token_name: &'static str) -> Self {
        self.markdown.token_name = Some(token_name);
        self
    }

    pub fn markdown_tokenizer(mut self, tokenizer: MarkdownTokenizer) -> Self {
        self.markdown.tokenizer = Some(tokenizer);
        self
    }

    pub fn parse_markdown(mut self, parse: ParseMarkdown) -> Self {
        self.markdown.parse = Some(parse);
        self
    }

    pub fn render_markdown(mut self, render: RenderMarkdown) -> Self {
        self.markdown.render = Some(render);
        self
    }

    pub fn html_reopen(mut self, open: &'static str, close: &'static str) -> Self {
        self.markdown.html_reopen = Some((open, close));
        self
    }
}

impl NodeExtension {
    pub fn content(mut self, content: &'static str) -> Self {
        self.kind.content = content;
        self
    }

    pub fn group(mut self, group: &'static str) -> Self {
        self.kind.group = group;
        self
    }

    pub fn inline(mut self, inline: bool) -> Self {
        self.kind.inline = inline;
        self
    }

    pub fn marks(mut self, marks: &'static str) -> Self {
        self.kind.marks = Some(marks);
        self
    }

    pub fn linebreak_replacement(mut self) -> Self {
        self.kind.linebreak_replacement = true;
        self
    }

    pub fn render_html(
        mut self,
        render: impl for<'a> Fn(&'a Node<'static>, SpecAttrs<'a>) -> Result<DomSpec<'a>, Error>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.kind.render_html = Some(Arc::new(render));
        self
    }
}

impl MarkExtension {
    pub fn render_html(
        mut self,
        render: impl for<'a> Fn(&'a Mark<'static>, SpecAttrs<'a>) -> DomSpec<'a> + Send + Sync + 'static,
    ) -> Self {
        self.kind.render_html = Some(Arc::new(render));
        self
    }
}

impl From<NodeExtension> for Extension {
    fn from(extension: NodeExtension) -> Self {
        extension.map_kind(Kind::Node)
    }
}

impl From<MarkExtension> for Extension {
    fn from(extension: MarkExtension) -> Self {
        extension.map_kind(Kind::Mark)
    }
}

impl From<PlainExtension> for Extension {
    fn from(extension: PlainExtension) -> Self {
        extension.map_kind(|()| Kind::Extension)
    }
}
