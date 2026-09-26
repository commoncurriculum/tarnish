//! Parse rules, and the order of a schema's.

use std::sync::Arc;

use crate::error::Result;
use crate::model::{Attrs, Fragment, Mark, Schema};

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

    pub(super) fn tag(&self) -> Option<&TagRule<N>> {
        match &self.kind {
            RuleKind::Tag(tag) => Some(tag),
            RuleKind::Style(_) => None,
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
