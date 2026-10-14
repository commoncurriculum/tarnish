//! `Node.create`, `Mark.create` and `Extension.create`, and the configs they build, which
//! `getSchema` and the `MarkdownManager` read. `.extend({...})` carries on the parent's builder:
//! a setter replaces the parent's field, and an `extend_` method adds to it, as a field that
//! spreads `this.parent()` does.

use std::borrow::Cow;
use std::sync::Arc;

use super::attributes::{ExtensionAttribute, GlobalAttributes};
use super::markdown::{
    MarkdownTokenizer, ParseHelpers, ParseMarkdown, Parsed, RenderContext, RenderHelpers,
    RenderMarkdown, Start,
};
use super::parse_html::ParseHtml;
use super::text::{TextSerializer, TextSerializerProps};
use crate::{DomSpec, SpecAttrs};
use tarnish::model::Whitespace;
use tarnish::{Mark, Node};
use tarnish_js::Error;
use tarnish_js::json::{Map, Value};
use tarnish_js::units::Units;
use tarnish_markdown::marked::{Lexer, Token};

/// A node's `renderHTML`, given the node and its rendered attributes.
pub type RenderNode = Arc<
    dyn for<'a> Fn(&'a Node<'static>, SpecAttrs<'a>) -> Result<DomSpec<'a>, Error> + Send + Sync,
>;
/// A mark's `renderHTML`, given the mark and its rendered attributes.
pub type RenderMark = Arc<
    dyn for<'a> Fn(&'a Mark<'static>, SpecAttrs<'a>) -> Result<DomSpec<'a>, Error> + Send + Sync,
>;

/// An extension's config. `K` is what its kind adds: [`NodeConfig`] for a node, [`MarkConfig`]
/// for a mark, nothing for a plain extension, and [`Kind`] once it is any of them.
pub struct Extension<K = Kind> {
    pub name: &'static str,
    pub priority: i32,
    pub kind: K,
    /// `addAttributes`.
    pub attributes: Vec<ExtensionAttribute>,
    /// `addGlobalAttributes`.
    pub global_attributes: Option<AddGlobalAttributes>,
    /// `parseHTML`.
    pub parse_html: Vec<ParseHtml>,
    /// The config's fields that a package declares for itself, such as Table's `tableRole`,
    /// which only an `extendNodeSchema` reads.
    pub fields: Map,
    /// `extendNodeSchema`.
    pub extend_node_schema: Option<ExtendNodeSchema>,
    pub(crate) markdown: Markdown,
}

/// An extension's `addGlobalAttributes()`, given `this.extensions`: the extensions of the
/// schema's node types, then those of its mark types, in its order.
pub type AddGlobalAttributes = Arc<dyn Fn(&[&Extension]) -> Vec<GlobalAttributes> + Send + Sync>;

/// An extension's `extendNodeSchema(extension)`: the fields it gives the spec of a node type,
/// given the type's extension. A field whose value is `null` is left out.
pub type ExtendNodeSchema = Arc<dyn Fn(&Extension) -> Map + Send + Sync>;

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
    pub content: Cow<'static, str>,
    pub group: &'static str,
    pub inline: bool,
    pub marks: Option<&'static str>,
    pub linebreak_replacement: bool,
    /// `code`: the node holds code, whose text Markdown keeps as it is.
    pub code: bool,
    pub atom: bool,
    /// `selectable`, which only a `false` turns off.
    pub selectable: Option<bool>,
    pub draggable: bool,
    pub isolating: bool,
    pub defining: bool,
    pub whitespace: Option<Whitespace>,
    pub render_html: Option<RenderNode>,
    /// `renderText`: the node's text, in place of its content's.
    pub render_text: Option<TextSerializer>,
}

#[derive(Default)]
pub struct MarkConfig {
    /// `code`: the mark's text is code, which Markdown keeps as it is.
    pub code: bool,
    /// `inclusive`, which only a `false` turns off.
    pub inclusive: Option<bool>,
    /// `excludes`: the marks that can't be beside this one, `_` for all of them.
    pub excludes: Option<&'static str>,
    pub render_html: Option<RenderMark>,
}

/// The fields `@tiptap/markdown` reads from an extension.
#[derive(Default)]
pub(crate) struct Markdown {
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
    /// `markdownOptions.indentsContent`: the node's children render a level deeper.
    pub indents_content: bool,
}

impl<K: Default> Extension<K> {
    pub fn create(name: &'static str) -> Self {
        Extension {
            name,
            priority: 100,
            kind: K::default(),
            attributes: Vec::new(),
            global_attributes: None,
            parse_html: Vec::new(),
            fields: Map::new(),
            extend_node_schema: None,
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
            fields: self.fields,
            extend_node_schema: self.extend_node_schema,
            markdown: self.markdown,
        }
    }

    /// Sets a field of the config that a package declares for itself.
    pub fn field(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.fields.insert(name.into(), value.into());
        self
    }

    pub fn extend_node_schema(
        mut self,
        extend: impl Fn(&Extension) -> Map + Send + Sync + 'static,
    ) -> Self {
        self.extend_node_schema = Some(Arc::new(extend));
        self
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

    /// `{ ...attributes, [name]: { ...attributes[name], ...changes } }`, of the attribute named
    /// `name`, which the extension has.
    pub fn update_attribute(
        mut self,
        name: &str,
        update: impl FnOnce(ExtensionAttribute) -> ExtensionAttribute,
    ) -> Self {
        let attribute = self
            .attributes
            .iter_mut()
            .find(|attribute| attribute.name == name)
            .unwrap_or_else(|| panic!("{} has no attribute {name}", self.name));
        *attribute = update(attribute.clone());
        self
    }

    pub fn add_global_attributes(
        mut self,
        add: impl Fn(&[&Extension]) -> Vec<GlobalAttributes> + Send + Sync + 'static,
    ) -> Self {
        self.global_attributes = Some(Arc::new(add));
        self
    }

    pub fn parse_html(mut self, rules: impl IntoIterator<Item = impl Into<ParseHtml>>) -> Self {
        self.parse_html = rules.into_iter().map(Into::into).collect();
        self
    }

    /// `parseHTML() { return [...this.parent?.(), ...rules] }`.
    pub fn extend_parse_html(
        mut self,
        rules: impl IntoIterator<Item = impl Into<ParseHtml>>,
    ) -> Self {
        self.parse_html.extend(rules.into_iter().map(Into::into));
        self
    }

    pub fn markdown_token_name(mut self, token_name: &'static str) -> Self {
        self.markdown.token_name = Some(token_name);
        self
    }

    /// `markdownTokenizer: { start: (src) => src.indexOf(start), tokenize }`, inline: `tokenize`
    /// matches only where `src` starts with `start`.
    pub fn markdown_tokenizer(
        mut self,
        start: impl Into<Cow<'static, str>>,
        tokenize: impl Fn(&Units, &[Token], &mut Lexer) -> Result<Option<Token>, Error>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.markdown.tokenizer = Some(MarkdownTokenizer {
            start: Start::Inline(start.into()),
            tokenize: Arc::new(tokenize),
        });
        self
    }

    /// `markdownTokenizer: { level: 'block', start, tokenize }`: `start(src)` is where in `src`
    /// a token may start, `None` for `-1`.
    pub fn markdown_block_tokenizer(
        mut self,
        start: impl Fn(&Units) -> Option<usize> + Send + Sync + 'static,
        tokenize: impl Fn(&Units, &[Token], &mut Lexer) -> Result<Option<Token>, Error>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.markdown.tokenizer = Some(MarkdownTokenizer {
            start: Start::Block(Arc::new(start)),
            tokenize: Arc::new(tokenize),
        });
        self
    }

    pub fn parse_markdown(
        mut self,
        parse: impl Fn(&Token, &ParseHelpers) -> Result<Parsed, Error> + Send + Sync + 'static,
    ) -> Self {
        self.markdown.parse = Some(Arc::new(parse));
        self
    }

    pub fn render_markdown(
        mut self,
        render: impl Fn(&Value, &dyn RenderHelpers, &RenderContext) -> Result<String, Error>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.markdown.render = Some(Arc::new(render));
        self
    }

    pub fn html_reopen(mut self, open: &'static str, close: &'static str) -> Self {
        self.markdown.html_reopen = Some((open, close));
        self
    }

    /// `markdownOptions: { indentsContent: true }`.
    pub fn indents_content(mut self) -> Self {
        self.markdown.indents_content = true;
        self
    }
}

impl NodeExtension {
    /// `content`, an expression that may be built from names.
    pub fn content(mut self, content: impl Into<Cow<'static, str>>) -> Self {
        self.kind.content = content.into();
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

    /// `inline: this.options.inline` and `group: this.options.inline ? 'inline' : 'block'`.
    pub fn inline_or_block(self, inline: bool) -> Self {
        self.inline(inline)
            .group(if inline { "inline" } else { "block" })
    }

    pub fn marks(mut self, marks: &'static str) -> Self {
        self.kind.marks = Some(marks);
        self
    }

    pub fn linebreak_replacement(mut self) -> Self {
        self.kind.linebreak_replacement = true;
        self
    }

    pub fn code(mut self) -> Self {
        self.kind.code = true;
        self
    }

    pub fn atom(mut self) -> Self {
        self.kind.atom = true;
        self
    }

    pub fn selectable(mut self, selectable: bool) -> Self {
        self.kind.selectable = Some(selectable);
        self
    }

    pub fn draggable(mut self) -> Self {
        self.kind.draggable = true;
        self
    }

    pub fn isolating(mut self) -> Self {
        self.kind.isolating = true;
        self
    }

    pub fn defining(mut self) -> Self {
        self.kind.defining = true;
        self
    }

    pub fn whitespace(mut self, whitespace: Whitespace) -> Self {
        self.kind.whitespace = Some(whitespace);
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

    pub fn render_text(
        mut self,
        render: impl Fn(&TextSerializerProps) -> Result<String, Error> + Send + Sync + 'static,
    ) -> Self {
        self.kind.render_text = Some(Arc::new(render));
        self
    }
}

impl MarkExtension {
    pub fn code(mut self) -> Self {
        self.kind.code = true;
        self
    }

    pub fn inclusive(mut self, inclusive: bool) -> Self {
        self.kind.inclusive = Some(inclusive);
        self
    }

    pub fn excludes(mut self, excludes: &'static str) -> Self {
        self.kind.excludes = Some(excludes);
        self
    }

    pub fn render_html(
        mut self,
        render: impl for<'a> Fn(&'a Mark<'static>, SpecAttrs<'a>) -> Result<DomSpec<'a>, Error>
        + Send
        + Sync
        + 'static,
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
