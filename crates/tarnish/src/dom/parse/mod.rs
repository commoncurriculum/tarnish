//! Parsing documents from a DOM: `DOMParser`.

mod context;
mod find;
mod html;
mod node_context;
mod rule;
mod rule_context;
mod walk;

use std::collections::BTreeMap;

use super::Dom;
use crate::Result;
use crate::json::Map;
use crate::model::{ContentMatch, Node, ResolvedPos, Schema, Slice};
use context::ParseContext;

pub use rule::{
    AttrsHook, ClearMarkHook, Content, ContentElement, ContentElementHook, ElementRule,
    GetAttrsResult, GetContentHook, Namespace, NodeRule, ParseRule, PreserveWhitespace, Rule,
    RuleField, RuleFromNode, SchemaRule, StyleAttrsHook, StyleRule, TagRule, schema_rules,
};

/// A DOM position to find the document position of: the offset into `node`. Parsing sets `pos`
/// when the position is in the parsed content.
#[derive(Clone)]
pub struct FindPosition<N> {
    pub node: N,
    pub offset: usize,
    /// The position found. In a text node, it counts back from where the text parsed from it
    /// ends, so collapsing the whitespace before the offset moves it back, as far as below zero,
    /// as in JavaScript.
    pub pos: Option<isize>,
}

/// Options for [`DomParser::parse`] and [`DomParser::parse_slice`].
pub struct ParseOptions<'a, N> {
    pub preserve_whitespace: Option<PreserveWhitespace>,
    pub find_positions: Option<&'a mut [FindPosition<N>]>,
    /// The index of the child to start parsing at.
    pub from: Option<usize>,
    /// The index of the child to stop parsing at.
    pub to: Option<usize>,
    /// The node whose type and attributes to parse into, instead of the schema's top node's.
    pub top_node: Option<Node<'a>>,
    pub top_match: Option<ContentMatch<'a>>,
    /// Nodes to count as the context above the top node.
    pub context: Option<ResolvedPos<'a>>,
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
    tags: Vec<Rule<TagRule<N>>>,
    /// For each element name a tag rule's selector requires, in lower case and sorted by it, the
    /// indices of the tag rules an element of that name can match, in order: those that require
    /// its name, and those that require none.
    tags_by_element: Vec<(String, Vec<usize>)>,
    /// The indices of the tag rules that require no element name.
    tags_for_any_element: Vec<usize>,
    styles: Vec<Rule<StyleRule>>,
    /// The properties the style rules match, each once.
    matched_styles: Vec<String>,
    normalize_lists: bool,
}

impl<N: Clone> DomParser<N> {
    /// A parser of `schema` with these rules, each kind tried in order.
    pub fn new(
        schema: Schema,
        tags: Vec<Rule<TagRule<N>>>,
        styles: Vec<Rule<StyleRule>>,
    ) -> Result<Self> {
        let mut matched_styles: Vec<String> = Vec::new();
        for rule in &styles {
            let property = rule.kind.property();
            if !matched_styles.iter().any(|matched| matched == property) {
                matched_styles.push(property.to_owned());
            }
        }
        // Lists are only normalized when a list in the schema can't directly hold one.
        let mut normalize_lists = true;
        for rule in &tags {
            let tag = &rule.kind.tag;
            let is_list = (tag.starts_with("ul") || tag.starts_with("ol"))
                && !tag[2..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
            if let Some(node) = &rule.kind.element.node
                && is_list
            {
                let node_type = schema.expect_node_type(node)?;
                if node_type.content_match().match_type(&node_type).is_some() {
                    normalize_lists = false;
                    break;
                }
            }
        }
        let mut by_element: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut tags_for_any_element = Vec::new();
        for (index, rule) in tags.iter().enumerate() {
            match required_element(&rule.kind.tag) {
                Some(name) => by_element
                    .entry(name.to_ascii_lowercase())
                    .or_default()
                    .push(index),
                None => tags_for_any_element.push(index),
            }
        }
        let tags_by_element = by_element
            .into_iter()
            .map(|(name, mut indices)| {
                indices.extend(&tags_for_any_element);
                indices.sort_unstable();
                (name, indices)
            })
            .collect();
        Ok(DomParser {
            schema,
            tags,
            tags_by_element,
            tags_for_any_element,
            styles,
            matched_styles,
            normalize_lists,
        })
    }

    /// `DOMParser.fromSchema`: a parser of each mark type's rules and each node type's, given in
    /// schema order, ordered and named by [`schema_rules`].
    pub fn from_schema(
        schema: Schema,
        marks: Vec<Vec<ParseRule<N>>>,
        nodes: Vec<Vec<ParseRule<N>>>,
    ) -> Result<Self> {
        let (mut tags, mut styles) = (Vec::new(), Vec::new());
        for rule in schema_rules(&schema, marks, nodes) {
            match rule {
                ParseRule::Tag(rule) => tags.push(rule),
                ParseRule::Style(rule) => styles.push(rule),
            }
        }
        DomParser::new(schema, tags, styles)
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The tag rules and the style rules, each kind in the order it is tried.
    pub fn rules(&self) -> (&[Rule<TagRule<N>>], &[Rule<StyleRule>]) {
        (&self.tags, &self.styles)
    }

    /// Parse a document from the content of a DOM node.
    pub fn parse<D: Dom<Node = N>>(
        &self,
        dom: &D,
        node: &N,
        options: ParseOptions<'_, N>,
    ) -> Result<Node<'static>> {
        let top_open = options.top_open;
        self.run(dom, node, options, false)?.finish_node(top_open)
    }

    /// Parse the content of a DOM node as a slice, open at its sides. With a top node, the
    /// slice holds the content parsed into it.
    pub fn parse_slice<D: Dom<Node = N>>(
        &self,
        dom: &D,
        node: &N,
        options: ParseOptions<'_, N>,
    ) -> Result<Slice<'static>> {
        let content = self.run(dom, node, options, true)?.finish_content(true)?;
        Ok(Slice::max_open(content, true))
    }

    /// Parse the node's children, and give back the parse, for its top to be finished.
    fn run<'p, D: Dom<Node = N>>(
        &'p self,
        dom: &'p D,
        node: &N,
        options: ParseOptions<'p, N>,
        is_open: bool,
    ) -> Result<ParseContext<'p, D>> {
        let (from, to) = (options.from, options.to);
        let mut context = ParseContext::new(self, dom, options, is_open);
        context.add_all(node, &[], from, to)?;
        Ok(context)
    }

    /// The first tag rule after the one at `after` that matches the element.
    fn match_tag<D: Dom<Node = N>>(
        &self,
        dom: &D,
        node: &N,
        context: &ParseContext<'_, D>,
        after: Option<usize>,
    ) -> Result<Option<Matched<'_, N>>> {
        let start = after.map_or(0, |after| after + 1);
        let mut name = dom.local_name(node)?.unwrap_or_default();
        name.make_ascii_lowercase();
        let candidates = match self
            .tags_by_element
            .binary_search_by(|(element, _)| element.as_str().cmp(&name))
        {
            Ok(at) => &self.tags_by_element[at].1,
            Err(_) => &self.tags_for_any_element,
        };
        for &index in candidates.iter().filter(|&&index| index >= start) {
            let rule = &self.tags[index];
            let tag = &rule.kind;
            if !dom.matches(node, &tag.tag)? {
                continue;
            }
            let in_namespace = match &tag.namespace {
                Namespace::Any => true,
                Namespace::Null => dom.namespace(node)?.is_none(),
                Namespace::Is(namespace) => dom.namespace(node)?.as_ref() == Some(namespace),
            };
            if !in_namespace {
                continue;
            }
            if let Some(rule_context) = &rule.context
                && !context.matches_context(rule_context)?
            {
                continue;
            }
            // JavaScript's `matchTag` and `matchStyle` write what `getAttrs` gives onto the
            // rule, where it stays after the parse. Here it travels with the match instead.
            let attrs = match &tag.get_attrs {
                Some(get_attrs) => match get_attrs(node)? {
                    GetAttrsResult::Reject => continue,
                    GetAttrsResult::Defaults => None,
                    GetAttrsResult::Attrs(attrs) => Some(attrs),
                },
                None => rule.attrs.clone(),
            };
            return Ok(Some(Matched {
                element: &tag.element,
                mark: rule.mark.as_deref(),
                ignore: rule.ignore,
                attrs,
                continue_after: (!rule.consuming).then_some(index),
                skip_to: None,
                content_element: None,
            }));
        }
        Ok(None)
    }

    /// The index of the first style rule after the one at `after` that matches the style, and
    /// the attributes it gives.
    fn match_style<D: Dom<Node = N>>(
        &self,
        property: &str,
        value: &str,
        context: &ParseContext<'_, D>,
        after: Option<usize>,
    ) -> Result<Option<(usize, Option<Map>)>> {
        let start = after.map_or(0, |after| after + 1);
        for (index, rule) in self.styles.iter().enumerate().skip(start) {
            let style = &rule.kind.style;
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
            let attrs = match &rule.kind.get_attrs {
                Some(get_attrs) => match get_attrs(value)? {
                    GetAttrsResult::Reject => continue,
                    GetAttrsResult::Defaults => None,
                    GetAttrsResult::Attrs(attrs) => Some(attrs),
                },
                None => rule.attrs.clone(),
            };
            return Ok(Some((index, attrs)));
        }
        Ok(None)
    }
}

/// A rule that matched an element: one of the parser's, or the one `ruleFromNode` gave.
struct Matched<'r, N> {
    element: &'r ElementRule<N>,
    mark: Option<&'r str>,
    ignore: bool,
    attrs: Option<Map>,
    /// The index of the parser's rule, when it isn't consuming, for the rules after it to match
    /// the element too.
    continue_after: Option<usize>,
    /// The nodes a rule from `ruleFromNode` names, as [`NodeRule`] has them.
    skip_to: Option<&'r N>,
    content_element: Option<&'r N>,
}

impl<'r, N> Matched<'r, N> {
    fn from_node(from_node: &'r NodeRule<N>) -> Self {
        let rule = &from_node.rule;
        Matched {
            element: &rule.kind,
            mark: rule.mark.as_deref(),
            ignore: rule.ignore,
            attrs: rule.attrs.clone(),
            continue_after: None,
            skip_to: from_node.skip_to.as_ref(),
            content_element: from_node.content_element.as_ref(),
        }
    }

    /// Whether the rule makes nothing of the element, parsing only its content or another
    /// node's.
    fn skips(&self) -> bool {
        self.element.skip || self.skip_to.is_some()
    }
}

/// The element name a tag rule's selector requires, when the selector is a name followed only by
/// attribute selectors, `[name]` or `[name=value]`. Such a selector is valid CSS, so an element
/// of another name neither matches it nor makes matching it throw, and the rule goes untried.
fn required_element(selector: &str) -> Option<&str> {
    /// The length of the identifier at the start, which starts with a letter.
    fn identifier(text: &str) -> Option<usize> {
        if !text.as_bytes().first()?.is_ascii_alphabetic() {
            return None;
        }
        let end = text
            .bytes()
            .position(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')));
        Some(end.unwrap_or(text.len()))
    }
    let name = identifier(selector)?;
    let mut rest = &selector[name..];
    while !rest.is_empty() {
        rest = rest.strip_prefix('[')?;
        rest = &rest[identifier(rest)?..];
        if let Some(value) = rest.strip_prefix('=') {
            rest = match value.as_bytes().first()? {
                quote @ (b'"' | b'\'') => {
                    let close = value[1..].find(|c| {
                        c == char::from(*quote) || matches!(c, '\\' | '\n' | '\r' | '\x0c')
                    })?;
                    value[1..][close..].strip_prefix(char::from(*quote))?
                }
                _ => &value[identifier(value)?..],
            };
        }
        rest = rest.strip_prefix(']')?;
    }
    Some(&selector[..name])
}

#[cfg(test)]
mod tests {
    use super::required_element;

    #[test]
    fn requires_the_name_of_a_selector_of_a_name_and_attributes() {
        for (selector, name) in [
            ("p", Some("p")),
            ("H1", Some("H1")),
            ("my-element", Some("my-element")),
            (r#"div[data-content-type="standard"]"#, Some("div")),
            ("img[src][alt='a b']", Some("img")),
            ("span[data-type=mention]", Some("span")),
            ("*", None),
            ("b, strong", None),
            ("p.lead", None),
            ("ul > li", None),
            ("a[href^=http]", None),
            ("p[", None),
            ("p[x", None),
            ("p[data-x=1]", None),
            (r#"p[x="a\"b"]"#, None),
            ("1p", None),
            ("svg|rect", None),
        ] {
            assert_eq!(required_element(selector), name, "{selector}");
        }
    }
}
