//! Parsing documents from a DOM: `DOMParser`.

use std::sync::Arc;

use super::{Dom, NodeKind};
use crate::error::{Error, Result};
use crate::model::{
    Attrs, ContentMatch, Fragment, Mark, MarkType, Node, NodeType, ResolvedPos, Schema, Slice,
    Whitespace,
};
use crate::stack;
use crate::text::{Text, is_js_space};

/// What a rule's `getAttrs` gives: `false` to not match, or the attributes, where `None` is
/// JavaScript's `null` for the defaults.
pub type AttrsHook<N> = Arc<dyn Fn(&N) -> Result<Option<Option<Attrs>>> + Send + Sync>;

/// A style rule's `getAttrs`, given the style's value.
pub type StyleAttrsHook = Arc<dyn Fn(&str) -> Result<Option<Option<Attrs>>> + Send + Sync>;

pub type GetContentHook<N> = Arc<dyn Fn(&N, &Schema) -> Result<Fragment> + Send + Sync>;

pub type ClearMarkHook = Arc<dyn Fn(&Mark) -> Result<bool> + Send + Sync>;

/// A `contentElement` function: the element in a matched one that holds its content.
pub type ContentElementHook<N> = Arc<dyn Fn(&N) -> Result<N> + Send + Sync>;

/// The `ruleFromNode` parse option: a rule to use for a DOM node instead of the parser's.
pub type RuleFromNode<'a, N> = &'a dyn Fn(&N) -> Result<Option<ParseRule<N>>>;

/// Whether to keep whitespace: collapse it, keep it but turn newlines into spaces, or keep it
/// all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreserveWhitespace {
    No,
    Yes,
    Full,
}

/// A rule's `getAttrs`, of an element or of a style's value.
#[derive(Clone)]
pub enum GetAttrs<N> {
    Tag(AttrsHook<N>),
    Style(StyleAttrsHook),
}

/// Where a tag rule's node finds its content.
#[derive(Clone)]
pub enum ContentElement<N> {
    Selector(String),
    Node(N),
    Hook(ContentElementHook<N>),
}

/// A rule's `skip`: whether to parse only the element's content, or another element's.
#[derive(Clone)]
pub enum Skip<N> {
    No,
    Yes,
    Node(N),
}

/// What a rule matches.
#[derive(Clone)]
pub enum RuleKind<N> {
    /// Elements that match a CSS selector, in a namespace when `namespace` is given, where
    /// `Some(None)` is no namespace.
    Tag(TagRule<N>),
    /// Inline styles that set a property, or with `property=value`, set it to a value.
    Style(String),
}

#[derive(Clone)]
pub struct TagRule<N> {
    pub selector: String,
    pub namespace: Option<Option<String>>,
    pub content_element: Option<ContentElement<N>>,
    pub get_content: Option<GetContentHook<N>>,
    pub preserve_whitespace: Option<PreserveWhitespace>,
}

/// A parse rule, as a node's or mark's `parseDOM` holds it.
#[derive(Clone)]
pub struct ParseRule<N> {
    pub kind: RuleKind<N>,
    /// Where the rule goes among a schema's rules: higher first, 50 when not given.
    pub priority: Option<f64>,
    /// Whether a match keeps later rules from matching.
    pub consuming: bool,
    pub context: Option<String>,
    pub node: Option<String>,
    pub mark: Option<String>,
    pub ignore: bool,
    pub close_parent: bool,
    pub skip: Skip<N>,
    pub attrs: Option<Attrs>,
    pub get_attrs: Option<GetAttrs<N>>,
    pub clear_mark: Option<ClearMarkHook>,
}

impl<N> ParseRule<N> {
    /// A rule of this kind that does nothing yet.
    pub fn new(kind: RuleKind<N>) -> Self {
        ParseRule {
            kind,
            priority: None,
            consuming: true,
            context: None,
            node: None,
            mark: None,
            ignore: false,
            close_parent: false,
            skip: Skip::No,
            attrs: None,
            get_attrs: None,
            clear_mark: None,
        }
    }

    fn tag(&self) -> Option<&TagRule<N>> {
        match &self.kind {
            RuleKind::Tag(tag) => Some(tag),
            RuleKind::Style(_) => None,
        }
    }
}

/// A DOM position to find the document position of: the offset into `node`. Parsing sets `pos`
/// when the position is in the parsed content.
#[derive(Clone)]
pub struct FindPosition<N> {
    pub node: N,
    pub offset: usize,
    pub pos: Option<usize>,
}

/// Options for [`DomParser::parse`] and [`DomParser::parse_slice`].
pub struct ParseOptions<'a, N> {
    pub preserve_whitespace: Option<PreserveWhitespace>,
    pub find_positions: Option<&'a mut Vec<FindPosition<N>>>,
    /// The index of the child to start parsing at.
    pub from: Option<usize>,
    /// The index of the child to stop parsing at.
    pub to: Option<usize>,
    /// The node whose type and attributes to parse into, instead of the schema's top node's.
    pub top_node: Option<Node>,
    pub top_match: Option<ContentMatch>,
    /// Nodes to count as the context above the top node.
    pub context: Option<ResolvedPos>,
    pub rule_from_node: Option<RuleFromNode<'a, N>>,
    pub top_open: bool,
}

impl<N> Default for ParseOptions<'_, N> {
    fn default() -> Self {
        ParseOptions {
            preserve_whitespace: None,
            find_positions: None,
            from: None,
            to: None,
            top_node: None,
            top_match: None,
            context: None,
            rule_from_node: None,
            top_open: false,
        }
    }
}

/// A rule of a schema's type, as [`schema_rules`] orders it.
pub struct SchemaRule<N> {
    pub rule: ParseRule<N>,
    /// Whether the rule is a mark type's, rather than a node type's.
    pub of_mark: bool,
    /// The type's index in the schema's mark or node types.
    pub type_index: usize,
    /// The rule's index in its type's rules.
    pub rule_index: usize,
    /// Whether the rule had no node or mark, and got its type's.
    pub named: bool,
}

/// A schema's rules, each type's given in schema order, in the order its parser tries them:
/// higher priority first, then marks' rules before nodes', in schema order. A rule without a
/// node, mark or `ignore` gets its type's name.
pub fn schema_rules<N>(
    marks: Vec<(String, Vec<ParseRule<N>>)>,
    nodes: Vec<(String, Vec<ParseRule<N>>)>,
) -> Vec<SchemaRule<N>> {
    let mut result: Vec<SchemaRule<N>> = Vec::new();
    let mut insert = |rule: SchemaRule<N>| {
        let priority = rule.rule.priority.unwrap_or(50.0);
        let index = result
            .iter()
            .position(|next| next.rule.priority.unwrap_or(50.0) < priority)
            .unwrap_or(result.len());
        result.insert(index, rule);
    };
    for (of_mark, types) in [(true, marks), (false, nodes)] {
        for (type_index, (name, rules)) in types.into_iter().enumerate() {
            for (rule_index, mut rule) in rules.into_iter().enumerate() {
                let named = if of_mark {
                    !(rule.mark.is_some() || rule.ignore || rule.clear_mark.is_some())
                } else {
                    !(rule.node.is_some() || rule.ignore || rule.mark.is_some())
                };
                if named && of_mark {
                    rule.mark = Some(name.clone());
                } else if named {
                    rule.node = Some(name.clone());
                }
                insert(SchemaRule {
                    rule,
                    of_mark,
                    type_index,
                    rule_index,
                    named,
                });
            }
        }
    }
    result
}

const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "canvas",
    "dd",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "li",
    "noscript",
    "ol",
    "output",
    "p",
    "pre",
    "section",
    "table",
    "tfoot",
    "ul",
];

const IGNORE_TAGS: &[&str] = &["head", "noscript", "object", "script", "style", "title"];

fn is_list_tag(name: &str) -> bool {
    name == "ol" || name == "ul"
}

/// Parses DOM content into documents of a schema by its rules.
pub struct DomParser<N> {
    schema: Schema,
    tags: Vec<ParseRule<N>>,
    styles: Vec<ParseRule<N>>,
    matched_styles: Vec<String>,
    normalize_lists: bool,
}

impl<N: Clone> DomParser<N> {
    /// A parser of `schema` with these rules, tried in order.
    pub fn new(schema: Schema, rules: Vec<ParseRule<N>>) -> Result<Self> {
        let mut tags = Vec::new();
        let mut styles = Vec::new();
        let mut matched_styles: Vec<String> = Vec::new();
        for rule in rules {
            match &rule.kind {
                RuleKind::Tag(_) => tags.push(rule),
                RuleKind::Style(style) => {
                    let property = style.split('=').next().unwrap_or_default().to_owned();
                    if !matched_styles.contains(&property) {
                        matched_styles.push(property);
                    }
                    styles.push(rule);
                }
            }
        }
        // Lists are only normalized when a list in the schema can't directly hold one.
        let mut normalize_lists = true;
        for rule in &tags {
            let Some(tag) = rule.tag() else { continue };
            let is_list = (tag.selector.starts_with("ul") || tag.selector.starts_with("ol"))
                && !tag.selector[2..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
            let Some(node) = &rule.node else { continue };
            if !is_list {
                continue;
            }
            let node_type = schema.expect_node_type(node)?;
            if node_type.content_match().match_type(&node_type).is_some() {
                normalize_lists = false;
                break;
            }
        }
        Ok(DomParser {
            schema,
            tags,
            styles,
            matched_styles,
            normalize_lists,
        })
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Parse a document from the content of a DOM node.
    pub fn parse<D: Dom<Node = N>>(
        &self,
        dom: &D,
        node: &N,
        options: ParseOptions<'_, N>,
    ) -> Result<Node> {
        let (from, to) = (options.from, options.to);
        let mut context = ParseContext::new(self, dom, options, false);
        context.add_all(node, &[], from, to)?;
        match context.finish()? {
            Finished::Node(node) => Ok(node),
            Finished::Fragment(_) => unreachable!("a closed parse has a top node"),
        }
    }

    /// Parse the content of a DOM node as a slice, open at its sides.
    pub fn parse_slice<D: Dom<Node = N>>(
        &self,
        dom: &D,
        node: &N,
        options: ParseOptions<'_, N>,
    ) -> Result<Slice> {
        let (from, to) = (options.from, options.to);
        let mut context = ParseContext::new(self, dom, options, true);
        context.add_all(node, &[], from, to)?;
        match context.finish()? {
            Finished::Fragment(fragment) => Ok(Slice::max_open(fragment, true)),
            Finished::Node(_) => unreachable!("an open parse has no top node"),
        }
    }

    /// The first tag rule after the rule at `after` that matches the element, and the attributes
    /// it gives.
    fn match_tag<D: Dom<Node = N>>(
        &self,
        dom: &D,
        node: &N,
        context: &ParseContext<'_, '_, D>,
        after: Option<usize>,
    ) -> Result<Option<Matched<N>>> {
        let start = after.map_or(0, |after| after + 1);
        for (index, rule) in self.tags.iter().enumerate().skip(start) {
            let tag = rule.tag().expect("a tag rule");
            if !dom.matches(node, &tag.selector)? {
                continue;
            }
            if let Some(namespace) = &tag.namespace
                && dom.namespace(node)? != *namespace
            {
                continue;
            }
            if let Some(rule_context) = &rule.context
                && !context.matches_context(rule_context)?
            {
                continue;
            }
            let attrs = match &rule.get_attrs {
                Some(GetAttrs::Tag(get_attrs)) => match get_attrs(node)? {
                    None => continue,
                    Some(attrs) => attrs,
                },
                _ => rule.attrs.clone(),
            };
            return Ok(Some(Matched {
                rule: RuleRef::Listed(index),
                attrs,
            }));
        }
        Ok(None)
    }

    /// The first style rule after the rule at `after` that matches the style, and the
    /// attributes it gives.
    fn match_style<D: Dom<Node = N>>(
        &self,
        property: &str,
        value: &str,
        context: &ParseContext<'_, '_, D>,
        after: Option<usize>,
    ) -> Result<Option<(usize, Option<Attrs>)>> {
        let start = after.map_or(0, |after| after + 1);
        for (index, rule) in self.styles.iter().enumerate().skip(start) {
            let RuleKind::Style(style) = &rule.kind else {
                continue;
            };
            if !style.starts_with(property) {
                continue;
            }
            if let Some(rule_context) = &rule.context
                && !context.matches_context(rule_context)?
            {
                continue;
            }
            // The style is the property itself, or `property=value` with this value.
            if style.len() > property.len()
                && (style.as_bytes()[property.len()] != b'='
                    || &style[property.len() + 1..] != value)
            {
                continue;
            }
            let attrs = match &rule.get_attrs {
                Some(GetAttrs::Style(get_attrs)) => match get_attrs(value)? {
                    None => continue,
                    Some(attrs) => attrs,
                },
                _ => rule.attrs.clone(),
            };
            return Ok(Some((index, attrs)));
        }
        Ok(None)
    }
}

/// A matched rule: one of the parser's, or one `ruleFromNode` gave.
enum RuleRef<N> {
    Listed(usize),
    FromNode(Box<ParseRule<N>>),
}

struct Matched<N> {
    rule: RuleRef<N>,
    attrs: Option<Attrs>,
}

const OPT_PRESERVE_WS: u8 = 1;
const OPT_PRESERVE_WS_FULL: u8 = 2;
const OPT_OPEN_LEFT: u8 = 4;

fn ws_options_for(
    node_type: Option<&NodeType>,
    preserve: Option<PreserveWhitespace>,
    base: u8,
) -> u8 {
    if let Some(preserve) = preserve {
        return match preserve {
            PreserveWhitespace::No => 0,
            PreserveWhitespace::Yes => OPT_PRESERVE_WS,
            PreserveWhitespace::Full => OPT_PRESERVE_WS | OPT_PRESERVE_WS_FULL,
        };
    }
    match node_type {
        Some(node_type) if node_type.whitespace() == Whitespace::Pre => {
            OPT_PRESERVE_WS | OPT_PRESERVE_WS_FULL
        }
        _ => base & !OPT_OPEN_LEFT,
    }
}

/// HTML's whitespace: space, tab, newline, carriage return and form feed.
fn is_html_space(unit: u16) -> bool {
    matches!(unit, 0x20 | 0x09 | 0x0a | 0x0d | 0x0c)
}

enum Finished {
    Node(Node),
    Fragment(Fragment),
}

/// A node being built while parsing.
struct NodeContext {
    /// An identity, for finding the context again after the stack has changed.
    id: usize,
    node_type: Option<NodeType>,
    attrs: Option<Attrs>,
    marks: Vec<Mark>,
    solid: bool,
    matched: Option<ContentMatch>,
    options: u8,
    content: Vec<Node>,
}

impl NodeContext {
    fn find_wrapping(&mut self, node: &Node) -> Result<Option<Vec<NodeType>>> {
        if self.matched.is_none() {
            let Some(node_type) = &self.node_type else {
                return Ok(Some(Vec::new()));
            };
            let start = node_type.content_match();
            match start.fill_before(&Fragment::from_node(node.clone()), false, 0)? {
                Some(fill) => {
                    self.matched = start.match_fragment(&fill, 0, fill.child_count());
                }
                None => {
                    let wrap = start.find_wrapping(node.node_type());
                    if wrap.is_some() {
                        self.matched = Some(start);
                    }
                    return Ok(wrap);
                }
            }
        }
        Ok(self
            .matched
            .as_ref()
            .and_then(|matched| matched.find_wrapping(node.node_type())))
    }

    fn finish(mut self, open_end: bool) -> Result<Finished> {
        if self.options & OPT_PRESERVE_WS == 0
            && let Some(last) = self.content.last()
            && let Some(text) = last.text()
        {
            let units = text.units();
            let kept = units.len()
                - units
                    .iter()
                    .rev()
                    .take_while(|&&unit| is_html_space(unit))
                    .count();
            if kept < units.len() {
                if kept == 0 {
                    self.content.pop();
                } else {
                    let cut = last.cut(0, kept)?;
                    *self.content.last_mut().expect("a last node") = cut;
                }
            }
        }
        let mut content = Fragment::from_array(self.content);
        if !open_end
            && let Some(matched) = &self.matched
            && let Some(fill) = matched.fill_before(&Fragment::empty(), true, 0)?
        {
            content = content.append(&fill);
        }
        Ok(match &self.node_type {
            Some(node_type) => {
                Finished::Node(node_type.create(self.attrs.as_deref(), content, &self.marks)?)
            }
            None => Finished::Fragment(content),
        })
    }
}

/// A text to add: a DOM text node's, or the newline a `<br>` stands for.
struct TextSource<'n, N> {
    dom: Option<&'n N>,
    value: Text,
}

struct ParseContext<'p, 'o, D: Dom> {
    parser: &'p DomParser<D::Node>,
    dom: &'p D,
    options: ParseOptions<'o, D::Node>,
    is_open: bool,
    open: usize,
    needs_block: bool,
    nodes: Vec<NodeContext>,
    local_preserve_ws: bool,
    next_id: usize,
}

impl<'p, 'o, D: Dom> ParseContext<'p, 'o, D> {
    fn new(
        parser: &'p DomParser<D::Node>,
        dom: &'p D,
        options: ParseOptions<'o, D::Node>,
        is_open: bool,
    ) -> Self {
        let top_options = ws_options_for(None, options.preserve_whitespace, 0)
            | if is_open { OPT_OPEN_LEFT } else { 0 };
        let top = match &options.top_node {
            Some(top_node) => NodeContext {
                id: 0,
                node_type: Some(top_node.node_type().clone()),
                attrs: Some(top_node.attrs().clone()),
                marks: Vec::new(),
                solid: true,
                matched: Some(
                    options
                        .top_match
                        .clone()
                        .unwrap_or_else(|| top_node.node_type().content_match()),
                ),
                options: top_options,
                content: Vec::new(),
            },
            None if is_open => NodeContext {
                id: 0,
                node_type: None,
                attrs: None,
                marks: Vec::new(),
                solid: true,
                matched: None,
                options: top_options,
                content: Vec::new(),
            },
            None => {
                let node_type = parser.schema.top_node_type();
                NodeContext {
                    id: 0,
                    matched: (top_options & OPT_OPEN_LEFT == 0).then(|| node_type.content_match()),
                    node_type: Some(node_type),
                    attrs: None,
                    marks: Vec::new(),
                    solid: true,
                    options: top_options,
                    content: Vec::new(),
                }
            }
        };
        ParseContext {
            parser,
            dom,
            options,
            is_open,
            open: 0,
            needs_block: false,
            nodes: vec![top],
            local_preserve_ws: false,
            next_id: 1,
        }
    }

    fn top(&self) -> &NodeContext {
        &self.nodes[self.open]
    }

    fn top_mut(&mut self) -> &mut NodeContext {
        &mut self.nodes[self.open]
    }

    /// The context with this identity, which is on the stack.
    fn context(&self, id: usize) -> &NodeContext {
        self.nodes
            .iter()
            .find(|context| context.id == id)
            .expect("a context on the stack")
    }

    fn schema(&self) -> &'p Schema {
        &self.parser.schema
    }

    fn add_dom(&mut self, node: &D::Node, marks: &[Mark]) -> Result<()> {
        match self.dom.kind(node)? {
            NodeKind::Text => {
                let value = self.dom.text(node)?;
                self.add_text_node(
                    TextSource {
                        dom: Some(node),
                        value,
                    },
                    marks,
                )
            }
            NodeKind::Element => self.add_element(node, marks, None),
            NodeKind::Other => Ok(()),
        }
    }

    fn inline_context(&self, source: &TextSource<'_, D::Node>) -> Result<bool> {
        let top = self.top();
        if let Some(node_type) = &top.node_type {
            return Ok(node_type.inline_content());
        }
        if let Some(first) = top.content.first() {
            return Ok(first.is_inline());
        }
        let Some(dom) = source.dom else {
            return Ok(false);
        };
        match self.dom.parent(dom)? {
            Some(parent) => {
                let name = self.dom.node_name(&parent)?.to_lowercase();
                Ok(!BLOCK_TAGS.contains(&name.as_str()))
            }
            None => Ok(false),
        }
    }

    fn add_text_node(&mut self, source: TextSource<'_, D::Node>, marks: &[Mark]) -> Result<()> {
        let top_options = self.top().options;
        let preserve = if top_options & OPT_PRESERVE_WS_FULL != 0 {
            PreserveWhitespace::Full
        } else if self.local_preserve_ws || top_options & OPT_PRESERVE_WS != 0 {
            PreserveWhitespace::Yes
        } else {
            PreserveWhitespace::No
        };
        let mut value: Vec<u16> = source.value.units().to_vec();
        let schema = self.schema();
        if preserve == PreserveWhitespace::Full
            || self.inline_context(&source)?
            || value.iter().any(|&unit| !is_html_space(unit))
        {
            if preserve == PreserveWhitespace::No {
                value = collapse_spaces(&value);
                // Leading space goes when nothing comes before it, or a hard break, or text
                // that ends in space.
                if value.first().is_some_and(|&unit| is_html_space(unit))
                    && self.open == self.nodes.len() - 1
                {
                    let node_before = self.top().content.last();
                    let dom_before = match source.dom {
                        Some(dom) => self.dom.previous_sibling(dom)?,
                        None => None,
                    };
                    let after_break = match &dom_before {
                        Some(before) => self.dom.node_name(before)? == "BR",
                        None => false,
                    };
                    let after_space = node_before.is_some_and(|before| {
                        before
                            .text()
                            .and_then(|text| text.units().last().copied())
                            .is_some_and(is_html_space)
                    });
                    if node_before.is_none() || after_break || after_space {
                        value.remove(0);
                    }
                }
            } else if preserve == PreserveWhitespace::Full {
                value = normalize_newlines(&value, &[0x0a]);
            } else if let Some(linebreak) = schema.linebreak_replacement()
                && value.iter().any(|&unit| unit == 0x0a || unit == 0x0d)
                && self
                    .top_mut()
                    .find_wrapping(&linebreak.create(None, Fragment::empty(), &[])?)?
                    .is_some()
            {
                for (index, line) in split_lines(&value).into_iter().enumerate() {
                    if index > 0 {
                        self.insert_node(
                            linebreak.create(None, Fragment::empty(), &[])?,
                            marks,
                            true,
                        )?;
                    }
                    if !line.is_empty() {
                        let blank = !line.iter().any(|&unit| !is_js_space(unit));
                        self.insert_node(schema.text(line, &[])?, marks, blank)?;
                    }
                }
                value.clear();
            } else {
                value = normalize_newlines(&value, &[0x20]);
            }
            if !value.is_empty() {
                let blank = !value.iter().any(|&unit| !is_js_space(unit));
                self.insert_node(schema.text(value, &[])?, marks, blank)?;
            }
            if let Some(dom) = source.dom {
                self.find_in_text(dom, &source.value)?;
            }
        } else if let Some(dom) = source.dom {
            self.find_inside(dom)?;
        }
        Ok(())
    }

    /// Parse an element by the first rule that matches it, or its content when none does.
    fn add_element(
        &mut self,
        node: &D::Node,
        marks: &[Mark],
        match_after: Option<usize>,
    ) -> Result<()> {
        let dom = self.dom;
        let outer_ws = self.local_preserve_ws;
        let mut top = self.top().id;
        let name = dom.node_name(node)?;
        if name == "PRE" || dom.style_value(node, "white-space")?.contains("pre") {
            self.local_preserve_ws = true;
        }
        let name = name.to_lowercase();
        if is_list_tag(&name) && self.parser.normalize_lists {
            normalize_list(dom, node)?;
        }
        let from_node = match self.options.rule_from_node {
            Some(rule_from_node) => rule_from_node(node)?,
            None => None,
        };
        let matched = match from_node {
            Some(rule) => Some(Matched {
                attrs: rule.attrs.clone(),
                rule: RuleRef::FromNode(Box::new(rule)),
            }),
            None => self.parser.match_tag(dom, node, self, match_after)?,
        };
        let rule = matched.as_ref().map(|matched| match &matched.rule {
            RuleRef::Listed(index) => &self.parser.tags[*index],
            RuleRef::FromNode(rule) => &**rule,
        });
        let ignore = match rule {
            Some(rule) => rule.ignore,
            None => IGNORE_TAGS.contains(&name.as_str()),
        };
        if ignore {
            self.find_inside(node)?;
            self.ignore_fallback(node, marks)?;
        } else if rule.is_none_or(|rule| !matches!(rule.skip, Skip::No) || rule.close_parent) {
            let mut node = node.clone();
            if let Some(rule) = rule {
                if rule.close_parent {
                    self.open = self.open.saturating_sub(1);
                } else if let Skip::Node(skip) = &rule.skip {
                    node = skip.clone();
                }
            }
            let skip = rule.is_some_and(|rule| !matches!(rule.skip, Skip::No));
            let mut sync = false;
            let old_needs_block = self.needs_block;
            if BLOCK_TAGS.contains(&name.as_str()) {
                // `top` is the context that was open when the element came, which closing its
                // parent leaves on the stack.
                if self
                    .context(top)
                    .content
                    .first()
                    .is_some_and(Node::is_inline)
                    && self.open > 0
                {
                    self.open -= 1;
                    top = self.top().id;
                }
                sync = true;
                if self.context(top).node_type.is_none() {
                    self.needs_block = true;
                }
            } else if dom.first_child(&node)?.is_none() {
                self.leaf_fallback(&node, marks)?;
                self.local_preserve_ws = outer_ws;
                return Ok(());
            }
            let inner_marks = if skip {
                Some(marks.to_vec())
            } else {
                self.read_styles(&node, marks)?
            };
            if let Some(inner_marks) = inner_marks {
                self.add_all(&node, &inner_marks, None, None)?;
            }
            if sync {
                self.sync(top);
            }
            self.needs_block = old_needs_block;
        } else if let Some(inner_marks) = self.read_styles(node, marks)? {
            let matched = matched.expect("a matched rule");
            let continue_after = match &matched.rule {
                RuleRef::Listed(index) if !self.parser.tags[*index].consuming => Some(*index),
                _ => None,
            };
            self.add_element_by_rule(node, &matched, &inner_marks, continue_after)?;
        }
        self.local_preserve_ws = outer_ws;
        Ok(())
    }

    /// Called for a leaf DOM node that would otherwise be ignored.
    fn leaf_fallback(&mut self, node: &D::Node, marks: &[Mark]) -> Result<()> {
        if self.dom.node_name(node)? == "BR"
            && self
                .top()
                .node_type
                .as_ref()
                .is_some_and(NodeType::inline_content)
        {
            let source = TextSource {
                dom: None,
                value: Text::from("\n"),
            };
            self.add_text_node(source, marks)?;
        }
        Ok(())
    }

    /// Called for an ignored node.
    fn ignore_fallback(&mut self, node: &D::Node, marks: &[Mark]) -> Result<()> {
        // An ignored <br> still makes an inline context.
        if self.dom.node_name(node)? == "BR"
            && !self
                .top()
                .node_type
                .as_ref()
                .is_some_and(NodeType::inline_content)
        {
            let dash = self.schema().text("-", &[])?;
            self.find_place(&dash, marks.to_vec(), true)?;
        }
        Ok(())
    }

    /// The marks with those the element's styles add or clear, `None` when a style's rule
    /// ignores the element.
    fn read_styles(&mut self, node: &D::Node, marks: &[Mark]) -> Result<Option<Vec<Mark>>> {
        let mut marks = marks.to_vec();
        if self.dom.style_count(node)? == 0 {
            return Ok(Some(marks));
        }
        let parser = self.parser;
        for property in &parser.matched_styles {
            let value = self.dom.style_value(node, property)?;
            if value.is_empty() {
                continue;
            }
            let mut after = None;
            while let Some((index, attrs)) = parser.match_style(property, &value, self, after)? {
                let rule = &parser.styles[index];
                if rule.ignore {
                    return Ok(None);
                }
                if let Some(clear_mark) = &rule.clear_mark {
                    let mut kept = Vec::with_capacity(marks.len());
                    for mark in marks {
                        if !clear_mark(&mark)? {
                            kept.push(mark);
                        }
                    }
                    marks = kept;
                } else {
                    marks.push(self.rule_mark_type(rule)?.create(attrs.as_deref())?);
                }
                if rule.consuming {
                    break;
                }
                after = Some(index);
            }
        }
        Ok(Some(marks))
    }

    fn rule_mark_type(&self, rule: &ParseRule<D::Node>) -> Result<MarkType> {
        let name = rule.mark.as_deref().unwrap_or("undefined");
        self.schema()
            .mark_type(name)
            .ok_or_else(|| Error::Other(format!("No mark type {name} in the schema")))
    }

    fn add_element_by_rule(
        &mut self,
        node: &D::Node,
        matched: &Matched<D::Node>,
        marks: &[Mark],
        continue_after: Option<usize>,
    ) -> Result<()> {
        let rule = match &matched.rule {
            RuleRef::Listed(index) => &self.parser.tags[*index],
            RuleRef::FromNode(rule) => &**rule,
        };
        let mut marks = marks.to_vec();
        let mut sync = false;
        let mut leaf = false;
        if let Some(name) = &rule.node {
            let node_type = self.schema().expect_node_type(name)?;
            if !node_type.is_leaf() {
                let preserve = rule.tag().and_then(|tag| tag.preserve_whitespace);
                if let Some(inner) =
                    self.enter(&node_type, matched.attrs.clone(), marks.clone(), preserve)?
                {
                    sync = true;
                    marks = inner;
                }
            } else {
                leaf = true;
                let is_break = self.dom.node_name(node)? == "BR";
                let created = node_type.create(matched.attrs.as_deref(), Fragment::empty(), &[])?;
                if !self.insert_node(created, &marks, is_break)? {
                    self.leaf_fallback(node, &marks)?;
                }
            }
        } else {
            marks.push(
                self.rule_mark_type(rule)?
                    .create(matched.attrs.as_deref())?,
            );
        }
        let start_in = self.top().id;
        let tag = rule.tag();
        if leaf {
            self.find_inside(node)?;
        } else if let Some(after) = continue_after {
            self.add_element(node, &marks, Some(after))?;
        } else if let Some(get_content) = tag.and_then(|tag| tag.get_content.as_ref()) {
            self.find_inside(node)?;
            for child in get_content(node, self.schema())?.children() {
                self.insert_node(child.clone(), &marks, false)?;
            }
        } else {
            let content_dom = match tag.and_then(|tag| tag.content_element.as_ref()) {
                None => node.clone(),
                Some(ContentElement::Selector(selector)) => self
                    .dom
                    .query_selector(node, selector)?
                    .ok_or_else(|| Error::Other(format!("No element matches {selector}")))?,
                Some(ContentElement::Hook(hook)) => hook(node)?,
                Some(ContentElement::Node(content)) => content.clone(),
            };
            self.find_around(node, &content_dom, true)?;
            self.add_all(&content_dom, &marks, None, None)?;
            self.find_around(node, &content_dom, false)?;
        }
        if sync && self.sync(start_in) {
            self.open -= 1;
        }
        Ok(())
    }

    /// Add the parent's children from index `start` to `end`, or all of them.
    fn add_all(
        &mut self,
        parent: &D::Node,
        marks: &[Mark],
        start: Option<usize>,
        end: Option<usize>,
    ) -> Result<()> {
        let dom = self.dom;
        let mut index = start.unwrap_or(0);
        let mut child = match start {
            Some(start) if start > 0 => dom.child(parent, start)?,
            _ => dom.first_child(parent)?,
        };
        let end = match end {
            Some(end) => dom.child(parent, end)?,
            None => None,
        };
        loop {
            let at_end = match (&child, &end) {
                (Some(child), Some(end)) => dom.same(child, end)?,
                (None, None) => true,
                (None, Some(_)) => true,
                (Some(_), None) => false,
            };
            if at_end {
                break;
            }
            let current = child.expect("a child before the end");
            self.find_at_point(parent, index)?;
            stack::grow(|| self.add_dom(&current, marks))?;
            child = dom.next_sibling(&current)?;
            index += 1;
        }
        self.find_at_point(parent, index)
    }

    /// Find a place for a node: the context to put it in, leaving contexts that aren't solid
    /// and opening wrappers where needed. The marks the node gets, or `None` when it fits
    /// nowhere.
    fn find_place(
        &mut self,
        node: &Node,
        marks: Vec<Mark>,
        cautious: bool,
    ) -> Result<Option<Vec<Mark>>> {
        let mut route: Option<Vec<NodeType>> = None;
        let mut sync = None;
        let mut penalty = 0;
        for depth in (0..=self.open).rev() {
            let context = &mut self.nodes[depth];
            let found = context.find_wrapping(node)?;
            if let Some(found) = found
                && route
                    .as_ref()
                    .is_none_or(|route| route.len() > found.len() + penalty)
            {
                let done = found.is_empty();
                route = Some(found);
                sync = Some(context.id);
                if done {
                    break;
                }
            }
            if context.solid {
                if cautious {
                    break;
                }
                penalty += 2;
            }
        }
        let Some(route) = route else { return Ok(None) };
        self.sync(sync.expect("a context for the route"));
        let mut marks = marks;
        for node_type in route {
            marks = self.enter_inner(&node_type, None, marks, false, None)?;
        }
        Ok(Some(marks))
    }

    /// Insert a node, adjusting the context where it needs to. Whether it fit.
    fn insert_node(&mut self, node: Node, marks: &[Mark], cautious: bool) -> Result<bool> {
        let mut marks = marks.to_vec();
        if node.is_inline()
            && self.needs_block
            && self.top().node_type.is_none()
            && let Some(block) = self.textblock_from_context()?
        {
            marks = self.enter_inner(&block, None, marks, false, None)?;
        }
        let Some(inner_marks) = self.find_place(&node, marks, cautious)? else {
            return Ok(false);
        };
        self.close_extra(false)?;
        let schema = self.schema().clone();
        let top = self.top_mut();
        if let Some(matched) = &top.matched {
            top.matched = matched.match_type(node.node_type());
        }
        let mut node_marks = Mark::none();
        for mark in inner_marks.iter().chain(node.marks().iter()) {
            let applies = match &top.node_type {
                Some(node_type) => node_type.allows_mark_type(mark.mark_type()),
                None => mark_may_apply(&schema, mark.mark_type(), node.node_type()),
            };
            if applies {
                node_marks = mark.add_to_set(&node_marks);
            }
        }
        top.content.push(node.mark(node_marks));
        Ok(true)
    }

    /// Start a node of this type, adjusting the context where it needs to. The marks left for
    /// its content, or `None` when it fits nowhere.
    fn enter(
        &mut self,
        node_type: &NodeType,
        attrs: Option<Attrs>,
        marks: Vec<Mark>,
        preserve: Option<PreserveWhitespace>,
    ) -> Result<Option<Vec<Mark>>> {
        let created = node_type.create(attrs.as_deref(), Fragment::empty(), &[])?;
        if self.find_place(&created, marks.clone(), false)?.is_none() {
            return Ok(None);
        }
        self.enter_inner(node_type, attrs, marks, true, preserve)
            .map(Some)
    }

    /// Open a node of this type, giving it the marks it allows and leaving the rest.
    fn enter_inner(
        &mut self,
        node_type: &NodeType,
        attrs: Option<Attrs>,
        marks: Vec<Mark>,
        solid: bool,
        preserve: Option<PreserveWhitespace>,
    ) -> Result<Vec<Mark>> {
        self.close_extra(false)?;
        let schema = self.schema().clone();
        let top = self.top_mut();
        top.matched = top
            .matched
            .as_ref()
            .and_then(|matched| matched.match_type(node_type));
        let mut options = ws_options_for(Some(node_type), preserve, top.options);
        if top.options & OPT_OPEN_LEFT != 0 && top.content.is_empty() {
            options |= OPT_OPEN_LEFT;
        }
        let mut apply = Mark::none();
        let mut rest = Vec::new();
        for mark in marks {
            let applies = match &top.node_type {
                Some(parent) => parent.allows_mark_type(mark.mark_type()),
                None => mark_may_apply(&schema, mark.mark_type(), node_type),
            };
            if applies {
                apply = mark.add_to_set(&apply);
            } else {
                rest.push(mark);
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(NodeContext {
            id,
            node_type: Some(node_type.clone()),
            attrs,
            marks: apply.to_vec(),
            solid,
            matched: (options & OPT_OPEN_LEFT == 0).then(|| node_type.content_match()),
            options,
            content: Vec::new(),
        });
        self.open += 1;
        Ok(rest)
    }

    /// Finish the nodes above the open one and add them to their parents.
    fn close_extra(&mut self, open_end: bool) -> Result<()> {
        while self.nodes.len() - 1 > self.open {
            let context = self.nodes.pop().expect("a node above the open one");
            let Finished::Node(node) = context.finish(open_end)? else {
                unreachable!("an inner context has a type")
            };
            self.nodes.last_mut().expect("a parent").content.push(node);
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Finished> {
        self.open = 0;
        self.close_extra(self.is_open)?;
        let top_open = self.is_open || self.options.top_open;
        let top = self.nodes.pop().expect("the top context");
        top.finish(top_open)
    }

    /// Make the context with this identity the open one, if it is still on the stack.
    fn sync(&mut self, to: usize) -> bool {
        for index in (0..=self.open).rev() {
            if self.nodes[index].id == to {
                self.open = index;
                return true;
            } else if self.local_preserve_ws {
                self.nodes[index].options |= OPT_PRESERVE_WS;
            }
        }
        false
    }

    fn current_pos(&mut self) -> Result<usize> {
        self.close_extra(false)?;
        let mut pos = 0;
        for index in (0..=self.open).rev() {
            pos += self.nodes[index]
                .content
                .iter()
                .map(Node::node_size)
                .sum::<usize>();
            if index > 0 {
                pos += 1;
            }
        }
        Ok(pos)
    }

    /// The indexes of the positions to find that are still unfound.
    fn unfound(&self) -> Vec<usize> {
        match &self.options.find_positions {
            Some(find) => (0..find.len())
                .filter(|&index| find[index].pos.is_none())
                .collect(),
            None => Vec::new(),
        }
    }

    fn set_found(&mut self, index: usize, pos: usize) {
        if let Some(find) = &mut self.options.find_positions {
            find[index].pos = Some(pos);
        }
    }

    fn find_at_point(&mut self, parent: &D::Node, offset: usize) -> Result<()> {
        let count = self
            .options
            .find_positions
            .as_ref()
            .map_or(0, |find| find.len());
        for index in 0..count {
            let target = &self.options.find_positions.as_ref().expect("positions")[index];
            if target.offset == offset && self.dom.same(&target.node, parent)? {
                let pos = self.current_pos()?;
                self.set_found(index, pos);
            }
        }
        Ok(())
    }

    fn find_inside(&mut self, parent: &D::Node) -> Result<()> {
        for index in self.unfound() {
            let target = self.options.find_positions.as_ref().expect("positions")[index]
                .node
                .clone();
            if self.dom.kind(parent)? == NodeKind::Element && self.dom.contains(parent, &target)? {
                let pos = self.current_pos()?;
                self.set_found(index, pos);
            }
        }
        Ok(())
    }

    fn find_around(&mut self, parent: &D::Node, content: &D::Node, before: bool) -> Result<()> {
        if self.options.find_positions.is_none() || self.dom.same(parent, content)? {
            return Ok(());
        }
        for index in self.unfound() {
            let target = self.options.find_positions.as_ref().expect("positions")[index]
                .node
                .clone();
            if self.dom.kind(parent)? == NodeKind::Element && self.dom.contains(parent, &target)? {
                let position = self.dom.compare_document_position(content, &target)?;
                if position & if before { 2 } else { 4 } != 0 {
                    let pos = self.current_pos()?;
                    self.set_found(index, pos);
                }
            }
        }
        Ok(())
    }

    fn find_in_text(&mut self, node: &D::Node, text: &Text) -> Result<()> {
        let count = self
            .options
            .find_positions
            .as_ref()
            .map_or(0, |find| find.len());
        for index in 0..count {
            let target = &self.options.find_positions.as_ref().expect("positions")[index];
            if self.dom.same(&target.node, node)? {
                let offset = target.offset;
                let pos = (self.current_pos()? + offset).saturating_sub(text.len());
                self.set_found(index, pos);
            }
        }
        Ok(())
    }

    /// Whether the context string matches the nodes being parsed into.
    fn matches_context(&self, context: &str) -> Result<bool> {
        if context.contains('|') {
            for part in split_alternatives(context) {
                if self.matches_context(part)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        let parts: Vec<&str> = context.split('/').collect();
        let option = self.options.context.as_ref();
        let use_root = !self.is_open
            && option.is_none_or(|option| {
                Some(option.parent().node_type()) == self.nodes[0].node_type.as_ref()
            });
        let min_depth = -(option.map_or(0, |option| option.depth() as isize + 1))
            + if use_root { 0 } else { 1 };
        self.match_parts(
            &parts,
            parts.len() as isize - 1,
            self.open as isize,
            min_depth,
            use_root,
        )
    }

    fn match_parts(
        &self,
        parts: &[&str],
        mut index: isize,
        mut depth: isize,
        min_depth: isize,
        use_root: bool,
    ) -> Result<bool> {
        let option = self.options.context.as_ref();
        while index >= 0 {
            let part = parts[index as usize];
            if part.is_empty() {
                if index == parts.len() as isize - 1 || index == 0 {
                    index -= 1;
                    continue;
                }
                while depth >= min_depth {
                    if self.match_parts(parts, index - 1, depth, min_depth, use_root)? {
                        return Ok(true);
                    }
                    depth -= 1;
                }
                return Ok(false);
            }
            let next = if depth > 0 || (depth == 0 && use_root) {
                self.nodes[depth as usize].node_type.clone()
            } else if let Some(option) = option
                && depth >= min_depth
            {
                Some(
                    option
                        .node((depth - min_depth) as usize)
                        .node_type()
                        .clone(),
                )
            } else {
                None
            };
            match next {
                Some(next) if next.name() == part || next.is_in_group(part) => {}
                _ => return Ok(false),
            }
            depth -= 1;
            index -= 1;
        }
        Ok(true)
    }

    fn textblock_from_context(&self) -> Result<Option<NodeType>> {
        if let Some(context) = &self.options.context {
            for depth in (0..=context.depth()).rev() {
                let found = context
                    .node(depth)
                    .content_match_at(context.index_after(depth))?
                    .default_type();
                if let Some(found) = found
                    && found.is_textblock()
                    && found.default_attrs().is_some()
                {
                    return Ok(Some(found));
                }
            }
        }
        Ok(self
            .schema()
            .node_types()
            .find(|node_type| node_type.is_textblock() && node_type.default_attrs().is_some()))
    }
}

/// Move lists that are directly inside lists into the item before them, as browsers take them.
fn normalize_list<D: Dom>(dom: &D, list: &D::Node) -> Result<()> {
    let mut previous_item: Option<D::Node> = None;
    let mut child = dom.first_child(list)?;
    while let Some(current) = child {
        let name = match dom.kind(&current)? {
            NodeKind::Element => Some(dom.node_name(&current)?.to_lowercase()),
            _ => None,
        };
        let mut current = current;
        match name.as_deref() {
            Some(name) if is_list_tag(name) && previous_item.is_some() => {
                let item = previous_item.clone().expect("an item");
                dom.append_child(&item, &current)?;
                current = item;
            }
            Some("li") => previous_item = Some(current.clone()),
            Some(_) => previous_item = None,
            None => {}
        }
        child = dom.next_sibling(&current)?;
    }
    Ok(())
}

/// Whether a mark of this type could apply to a node of this type anywhere in the schema.
fn mark_may_apply(schema: &Schema, mark_type: &MarkType, node_type: &NodeType) -> bool {
    fn scan(matched: &ContentMatch, node_type: &NodeType, seen: &mut Vec<ContentMatch>) -> bool {
        seen.push(matched.clone());
        for index in 0..matched.edge_count() {
            let (edge_type, next) = matched.edge(index).expect("an edge in range");
            if edge_type == *node_type {
                return true;
            }
            if !seen.contains(&next) && scan(&next, node_type, seen) {
                return true;
            }
        }
        false
    }
    schema.node_types().any(|parent| {
        parent.allows_mark_type(mark_type)
            && scan(&parent.content_match(), node_type, &mut Vec::new())
    })
}

/// `value.replace(/[ \t\r\n\u000c]+/g, " ")`.
fn collapse_spaces(value: &[u16]) -> Vec<u16> {
    let mut result = Vec::with_capacity(value.len());
    let mut in_space = false;
    for &unit in value {
        let space = is_html_space(unit);
        if !(space && in_space) {
            result.push(if space { 0x20 } else { unit });
        }
        in_space = space;
    }
    result
}

/// Line ends replaced with `with`: `\r\n`, `\r` and `\n` each.
fn normalize_newlines(value: &[u16], with: &[u16]) -> Vec<u16> {
    let mut result = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        match value[index] {
            0x0d => {
                result.extend_from_slice(with);
                if value.get(index + 1) == Some(&0x0a) {
                    index += 1;
                }
            }
            0x0a => result.extend_from_slice(with),
            unit => result.push(unit),
        }
        index += 1;
    }
    result
}

/// `value.split(/\r?\n|\r/)`.
fn split_lines(value: &[u16]) -> Vec<Vec<u16>> {
    let mut lines = vec![Vec::new()];
    let mut index = 0;
    while index < value.len() {
        match value[index] {
            0x0d => {
                if value.get(index + 1) == Some(&0x0a) {
                    index += 1;
                }
                lines.push(Vec::new());
            }
            0x0a => lines.push(Vec::new()),
            unit => lines.last_mut().expect("a line").push(unit),
        }
        index += 1;
    }
    lines
}

/// `context.split(/\s*\|\s*/)`: the space around each `|` goes with it.
fn split_alternatives(context: &str) -> Vec<&str> {
    let space = |c: char| c.len_utf16() == 1 && is_js_space(c as u16);
    let parts: Vec<&str> = context.split('|').collect();
    let last = parts.len() - 1;
    parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| {
            let part = if index > 0 {
                part.trim_start_matches(space)
            } else {
                part
            };
            if index < last {
                part.trim_end_matches(space)
            } else {
                part
            }
        })
        .collect()
}
