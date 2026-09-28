//! Parse rules, and the order of a schema's.

use std::sync::Arc;

use crate::error::Result;
use crate::json::Map;
use crate::model::{Fragment, Mark, Schema};

/// A tag rule's `getAttrs`, given the element.
pub type AttrsHook<N> = Arc<dyn Fn(&N) -> Result<GetAttrsResult> + Send + Sync>;

/// A style rule's `getAttrs`, given the style's value.
pub type StyleAttrsHook = Arc<dyn Fn(&str) -> Result<GetAttrsResult> + Send + Sync>;

/// A `getContent`: the content a hook makes of the element.
pub type GetContentHook<N> = Arc<dyn Fn(&N, &Schema) -> Result<Fragment<'static>> + Send + Sync>;

/// A style rule's `clearMark`: whether to take the mark off the content.
pub type ClearMarkHook = Arc<dyn Fn(&Mark<'static>) -> Result<bool> + Send + Sync>;

/// A `contentElement` function: the element in a matched one that holds its content.
pub type ContentElementHook<N> = Arc<dyn Fn(&N) -> Result<N> + Send + Sync>;

/// The `ruleFromNode` parse option: a rule to use for an element instead of the parser's. As it
/// matches nothing, its `priority`, `context` and `consuming` go unread.
pub type RuleFromNode<'a, N> = &'a dyn Fn(&N) -> Result<Option<Rule<ElementRule<N>>>>;

/// Whether to keep whitespace: collapse it, keep it but turn newlines into spaces, or keep it
/// all. Each keeps more than the one before.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PreserveWhitespace {
    No,
    Yes,
    Full,
}

/// What a rule's `getAttrs` gives.
#[derive(Clone)]
pub enum GetAttrsResult {
    /// `false`: the rule doesn't match.
    Reject,
    /// `null` or `undefined`: the type's default attributes.
    Defaults,
    Attrs(Map),
}

/// The namespace a tag rule's elements must be in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Namespace {
    /// Any namespace, when the rule names none.
    Any,
    /// No namespace: JavaScript's `null`.
    Null,
    Is(String),
}

/// Which element holds a tag rule's content: the first in the matched one that matches a
/// selector, the one a hook finds, or this one.
#[derive(Clone)]
pub enum ContentElement<N> {
    Selector(String),
    Node(N),
    Hook(ContentElementHook<N>),
}

/// Where a tag rule's node takes its content from.
#[derive(Clone)]
pub enum Content<N> {
    /// The matched element's children.
    Children,
    /// `contentElement`: another element's children.
    Element(ContentElement<N>),
    /// `getContent`, which a rule's `contentElement` gives way to.
    Get(GetContentHook<N>),
}

/// A rule's `skip`: whether to parse only the element's content, or another element's.
#[derive(Clone)]
pub enum Skip<N> {
    No,
    Yes,
    Node(N),
}

/// A parse rule, as a node's or mark's `parseDOM` holds it: the fields tag and style rules
/// share, and those of its kind, `K`, a [`TagRule`] or a [`StyleRule`].
#[derive(Clone)]
pub struct Rule<K> {
    pub kind: K,
    /// Where the rule goes among a schema's rules, by [`schema_rules`]: higher first, 50 when
    /// not given. A parser tries its rules in the order it is given them.
    pub priority: Option<f64>,
    /// Whether a match keeps later rules from matching.
    pub consuming: bool,
    /// The nodes the rule may match inside, as `"a/b//c|d"`.
    pub context: Option<String>,
    /// The mark type to wrap the content in, when the rule makes no node.
    pub mark: Option<String>,
    /// Whether to leave out what the rule matches.
    pub ignore: bool,
    /// The attributes of the node or mark made, when the rule has no `get_attrs`.
    pub attrs: Option<Map>,
}

impl<K> Rule<K> {
    /// A rule of this kind that makes nothing yet.
    pub fn new(kind: K) -> Self {
        Rule {
            kind,
            priority: None,
            consuming: true,
            context: None,
            mark: None,
            ignore: false,
            attrs: None,
        }
    }
}

/// What a rule for elements matches, and what it makes of them.
#[derive(Clone)]
pub struct TagRule<N> {
    /// A CSS selector for the elements to match.
    pub tag: String,
    pub namespace: Namespace,
    /// The attributes for a matched element, in place of the rule's, or that it doesn't match.
    pub get_attrs: Option<AttrsHook<N>>,
    pub element: ElementRule<N>,
}

impl<N> TagRule<N> {
    pub fn new(tag: impl Into<String>) -> Self {
        TagRule {
            tag: tag.into(),
            namespace: Namespace::Any,
            get_attrs: None,
            element: ElementRule::default(),
        }
    }
}

/// What a rule makes of an element it matched.
#[derive(Clone)]
pub struct ElementRule<N> {
    /// The node type to make.
    pub node: Option<String>,
    pub skip: Skip<N>,
    /// Whether a match closes the node being parsed into.
    pub close_parent: bool,
    pub content: Content<N>,
    /// How to treat the whitespace in the node, when not as its parent does.
    pub preserve_whitespace: Option<PreserveWhitespace>,
}

impl<N> Default for ElementRule<N> {
    fn default() -> Self {
        ElementRule {
            node: None,
            skip: Skip::No,
            close_parent: false,
            content: Content::Children,
            preserve_whitespace: None,
        }
    }
}

/// A rule for inline styles, which makes or clears marks.
#[derive(Clone)]
pub struct StyleRule {
    /// A style property, or `property=value` to match the property only with that value.
    pub style: String,
    /// The attributes for the style's value, in place of the rule's, or that it doesn't match.
    pub get_attrs: Option<StyleAttrsHook>,
    /// Which marks to take off the content, instead of adding one.
    pub clear_mark: Option<ClearMarkHook>,
}

impl StyleRule {
    pub fn new(style: impl Into<String>) -> Self {
        StyleRule {
            style: style.into(),
            get_attrs: None,
            clear_mark: None,
        }
    }

    /// The property the rule matches.
    pub(super) fn property(&self) -> &str {
        self.style
            .split_once('=')
            .map_or(&self.style, |(property, _)| property)
    }
}

/// A rule of a node or mark type's `parseDOM`.
#[derive(Clone)]
pub enum ParseRule<N> {
    Tag(Rule<TagRule<N>>),
    Style(Rule<StyleRule>),
}

/// A field that says what a rule makes, as [`schema_rules`] reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleField {
    Node,
    Mark,
    Ignore,
    ClearMark,
}

/// A parse rule as [`schema_rules`] orders it and names its type in it.
pub trait SchemaRule {
    fn priority(&self) -> Option<f64>;
    fn has(&self, field: RuleField) -> bool;
    /// Make the rule's `node` or `mark` the type of this name.
    fn set(&mut self, field: RuleField, name: &str);
}

impl<N> SchemaRule for ParseRule<N> {
    fn priority(&self) -> Option<f64> {
        match self {
            ParseRule::Tag(rule) => rule.priority,
            ParseRule::Style(rule) => rule.priority,
        }
    }

    fn has(&self, field: RuleField) -> bool {
        match (self, field) {
            (ParseRule::Tag(rule), RuleField::Node) => rule.kind.element.node.is_some(),
            (ParseRule::Style(rule), RuleField::ClearMark) => rule.kind.clear_mark.is_some(),
            (ParseRule::Tag(rule), RuleField::Mark) => rule.mark.is_some(),
            (ParseRule::Style(rule), RuleField::Mark) => rule.mark.is_some(),
            (ParseRule::Tag(rule), RuleField::Ignore) => rule.ignore,
            (ParseRule::Style(rule), RuleField::Ignore) => rule.ignore,
            _ => false,
        }
    }

    fn set(&mut self, field: RuleField, name: &str) {
        match (self, field) {
            (ParseRule::Tag(rule), RuleField::Node) => rule.kind.element.node = Some(name.into()),
            (ParseRule::Tag(rule), RuleField::Mark) => rule.mark = Some(name.into()),
            (ParseRule::Style(rule), RuleField::Mark) => rule.mark = Some(name.into()),
            _ => {}
        }
    }
}

/// `DOMParser.schemaRules`: each mark type's rules and then each node type's, given in schema
/// order, in the order a parser tries them: higher priority first, 50 when a rule gives none,
/// and otherwise as given. A rule that doesn't say what it makes is for its own type: a mark
/// type's without a `mark`, `ignore` or `clearMark`, and a node type's without a `node`,
/// `ignore` or `mark`.
pub fn schema_rules<R: SchemaRule>(
    schema: &Schema,
    marks: Vec<Vec<R>>,
    nodes: Vec<Vec<R>>,
) -> Vec<R> {
    let mark_names = schema.mark_types().map(|mark_type| mark_type.name());
    let node_names = schema.node_types().map(|node_type| node_type.name());
    let of_marks = marks.into_iter().zip(mark_names).map(|(rules, name)| {
        let named = [RuleField::Mark, RuleField::Ignore, RuleField::ClearMark];
        (rules, name, RuleField::Mark, named)
    });
    let of_nodes = nodes.into_iter().zip(node_names).map(|(rules, name)| {
        let named = [RuleField::Node, RuleField::Ignore, RuleField::Mark];
        (rules, name, RuleField::Node, named)
    });
    let rules = of_marks
        .chain(of_nodes)
        .flat_map(|(rules, name, field, named)| {
            rules.into_iter().map(move |mut rule| {
                if !named.iter().any(|&own| rule.has(own)) {
                    rule.set(field, name);
                }
                rule
            })
        });
    by_priority(rules, R::priority)
}

fn by_priority<T>(
    rules: impl IntoIterator<Item = T>,
    priority: impl Fn(&T) -> Option<f64>,
) -> Vec<T> {
    let mut ordered: Vec<T> = Vec::new();
    for rule in rules {
        let first = priority(&rule).unwrap_or(50.0);
        let index = ordered
            .iter()
            .position(|next| priority(next).unwrap_or(50.0) < first)
            .unwrap_or(ordered.len());
        ordered.insert(index, rule);
    }
    ordered
}
