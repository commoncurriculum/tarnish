//! Parsing documents from a DOM: `DOMParser`.

mod context;
mod find;
mod html;
mod node_context;
mod rule;
mod rule_context;
mod walk;

use super::Dom;
use crate::error::Result;
use crate::model::{Attrs, ContentMatch, Node, ResolvedPos, Schema, Slice};
use context::ParseContext;
use node_context::Finished;

pub use rule::{
    AttrsHook, ClearMarkHook, ContentElement, ContentElementHook, GetAttrs, GetContentHook,
    ParseRule, PreserveWhitespace, RuleFromNode, RuleKind, SchemaRule, Skip, StyleAttrsHook,
    TagRule, schema_rules,
};

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
