//! Schemas: the node and mark types a document may hold, and what each may contain.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use super::attrs::{AttrSet, AttributeSpec, Attrs};
use super::content::{Automaton, ContentMatch};
use super::fragment::Fragment;
use super::mark::{Mark, Marks};
use super::node::Node;
use crate::error::{Error, Result};
use crate::js::Given;
use crate::json::Map;
use crate::text::Text;

/// A function the host gives a node spec, such as `leafText`.
pub type NodeHook<T> = Arc<dyn Fn(&Node) -> Result<T> + Send + Sync>;

/// What [`Schema::new`] builds a schema from. Its node and mark types are in order: the order
/// decides which comes first in a group, and how marks sort in a set.
#[derive(Clone, Default)]
pub struct SchemaSpec {
    pub nodes: Vec<(String, NodeSpec)>,
    pub marks: Vec<(String, MarkSpec)>,
    /// The top node's type, `"doc"` when not given.
    pub top_node: Option<String>,
}

#[derive(Clone, Default)]
pub struct NodeSpec {
    pub content: Option<String>,
    pub marks: Option<String>,
    pub group: Option<String>,
    pub inline: bool,
    pub atom: bool,
    pub attrs: Vec<(String, AttributeSpec)>,
    pub selectable: Option<bool>,
    pub draggable: bool,
    pub code: bool,
    pub whitespace: Option<Whitespace>,
    pub defining_as_context: bool,
    pub defining_for_content: bool,
    pub defining: bool,
    pub isolating: bool,
    pub linebreak_replacement: bool,
    pub leaf_text: Option<NodeHook<Text>>,
    pub to_debug_string: Option<NodeHook<String>>,
}

#[derive(Clone, Default)]
pub struct MarkSpec {
    pub attrs: Vec<(String, AttributeSpec)>,
    /// `inclusive`, which only a `false` turns off.
    pub inclusive: Option<bool>,
    pub excludes: Option<String>,
    pub group: Option<String>,
    pub spanning: Option<bool>,
    pub code: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whitespace {
    Pre,
    Normal,
}

pub(crate) struct NodeTypeData {
    pub name: Arc<str>,
    pub spec: NodeSpec,
    pub groups: Vec<String>,
    pub attrs: AttrSet,
    pub is_block: bool,
    pub is_text: bool,
    pub content: Arc<Automaton>,
    pub inline_content: bool,
    /// The marks allowed inside, `None` for all of them.
    pub mark_set: Option<Vec<usize>>,
}

impl NodeTypeData {
    pub fn is_inline(&self) -> bool {
        !self.is_block
    }

    /// Whether nodes of the type can be made to fill content: text can't, nor can a type with
    /// attributes no defaults give.
    pub fn is_generatable(&self) -> bool {
        !(self.is_text || self.attrs.has_required())
    }
}

pub(crate) struct MarkTypeData {
    pub name: Arc<str>,
    pub spec: MarkSpec,
    pub attrs: AttrSet,
    pub excluded: Vec<usize>,
}

pub(crate) struct SchemaData {
    pub nodes: Vec<NodeTypeData>,
    pub marks: Vec<MarkTypeData>,
    pub top_node: usize,
    pub text: usize,
    pub linebreak_replacement: Option<usize>,
}

/// A document schema: the node and mark types documents in it can hold.
#[derive(Clone)]
pub struct Schema(pub(crate) Arc<SchemaData>);

impl PartialEq for Schema {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Schema {}

impl fmt::Debug for Schema {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Schema")
            .field(
                "nodes",
                &self
                    .0
                    .nodes
                    .iter()
                    .map(|node| &node.name)
                    .collect::<Vec<_>>(),
            )
            .field(
                "marks",
                &self
                    .0
                    .marks
                    .iter()
                    .map(|mark| &mark.name)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Schema {
    /// Build a schema from its spec, as `new Schema(spec)` does, raising the same errors.
    pub fn new(spec: SchemaSpec) -> Result<Schema> {
        let mut nodes: Vec<NodeTypeData> = spec
            .nodes
            .into_iter()
            .map(|(name, spec)| NodeTypeData {
                groups: match &spec.group {
                    Some(group) if !group.is_empty() => {
                        group.split(' ').map(str::to_owned).collect()
                    }
                    _ => Vec::new(),
                },
                attrs: AttrSet::new(&name, &spec.attrs),
                is_block: !(spec.inline || name == "text"),
                is_text: name == "text",
                content: Automaton::empty(),
                inline_content: false,
                mark_set: None,
                name: Arc::from(name),
                spec,
            })
            .collect();
        let top_name = spec.top_node.filter(|name| !name.is_empty());
        let top_name = top_name.as_deref().unwrap_or("doc");
        let top_node = position(&nodes, top_name).ok_or_else(|| {
            Error::Range(format!(
                "Schema is missing its top node type ('{top_name}')"
            ))
        })?;
        let text = position(&nodes, "text")
            .ok_or_else(|| Error::Range("Every schema needs a 'text' type".into()))?;
        if !nodes[text].attrs.is_empty() {
            return Err(Error::Range(
                "The text node type should not have attributes".into(),
            ));
        }

        let mut marks: Vec<MarkTypeData> = spec
            .marks
            .into_iter()
            .map(|(name, spec)| MarkTypeData {
                attrs: AttrSet::new(&name, &spec.attrs),
                excluded: Vec::new(),
                name: Arc::from(name),
                spec,
            })
            .collect();

        let mut content_cache: HashMap<String, Arc<Automaton>> = HashMap::new();
        let mut linebreak_replacement = None;
        for index in 0..nodes.len() {
            let name = nodes[index].name.clone();
            if marks.iter().any(|mark| mark.name == name) {
                return Err(Error::Range(format!(
                    "{name} can not be both a node and a mark"
                )));
            }
            let expr = nodes[index].spec.content.clone().unwrap_or_default();
            let automaton = match content_cache.get(&expr) {
                Some(automaton) => automaton.clone(),
                None => {
                    let automaton = Automaton::parse(&expr, &nodes)?;
                    content_cache.insert(expr, automaton.clone());
                    automaton
                }
            };
            nodes[index].inline_content = automaton.inline_content(&nodes);
            nodes[index].content = automaton;
            let node = &nodes[index];
            if node.spec.linebreak_replacement {
                if linebreak_replacement.is_some() {
                    return Err(Error::Range("Multiple linebreak nodes defined".into()));
                }
                if node.is_block || !node.content.is_empty_match() {
                    return Err(Error::Range(
                        "Linebreak replacement nodes must be inline leaf nodes".into(),
                    ));
                }
                linebreak_replacement = Some(index);
            }
            let mark_set = match node.spec.marks.as_deref() {
                Some("_") => None,
                Some("") => Some(Vec::new()),
                Some(expr) => Some(gather_marks(&marks, expr.split(' '))?),
                None if !node.inline_content => Some(Vec::new()),
                None => None,
            };
            nodes[index].mark_set = mark_set;
        }
        for index in 0..marks.len() {
            let excluded = match marks[index].spec.excludes.as_deref() {
                None => vec![index],
                Some("") => Vec::new(),
                Some(expr) => gather_marks(&marks, expr.split(' '))?,
            };
            marks[index].excluded = excluded;
        }

        Ok(Schema(Arc::new(SchemaData {
            nodes,
            marks,
            top_node,
            text,
            linebreak_replacement,
        })))
    }

    /// An identity for the schema, the same for clones of it.
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    pub fn node_types(&self) -> impl ExactSizeIterator<Item = NodeType> + '_ {
        (0..self.0.nodes.len()).map(|index| NodeType {
            schema: self.clone(),
            index,
        })
    }

    pub fn mark_types(&self) -> impl ExactSizeIterator<Item = MarkType> + '_ {
        (0..self.0.marks.len()).map(|index| MarkType {
            schema: self.clone(),
            index,
        })
    }

    /// The node type of this name, if the schema has one.
    pub fn node_type(&self, name: &str) -> Option<NodeType> {
        position(&self.0.nodes, name).map(|index| NodeType {
            schema: self.clone(),
            index,
        })
    }

    /// The node type of this name, raising `Unknown node type` when there is none.
    pub fn expect_node_type(&self, name: &str) -> Result<NodeType> {
        self.node_type(name)
            .ok_or_else(|| Error::Range(format!("Unknown node type: {name}")))
    }

    pub fn mark_type(&self, name: &str) -> Option<MarkType> {
        self.0
            .marks
            .iter()
            .position(|mark| &*mark.name == name)
            .map(|index| MarkType {
                schema: self.clone(),
                index,
            })
    }

    pub fn top_node_type(&self) -> NodeType {
        NodeType {
            schema: self.clone(),
            index: self.0.top_node,
        }
    }

    pub fn linebreak_replacement(&self) -> Option<NodeType> {
        self.0.linebreak_replacement.map(|index| NodeType {
            schema: self.clone(),
            index,
        })
    }

    pub(crate) fn text_type(&self) -> NodeType {
        NodeType {
            schema: self.clone(),
            index: self.0.text,
        }
    }

    /// Create a node, checking its content, as `schema.node(type, attrs, content, marks)`.
    pub fn node(
        &self,
        node_type: &NodeType,
        attrs: Option<&Map>,
        content: Fragment,
        marks: &[Mark],
    ) -> Result<Node> {
        if node_type.schema != *self {
            return Err(Error::Range(format!(
                "Node type from different schema used ({})",
                node_type.name()
            )));
        }
        node_type.create_checked(attrs, content, marks)
    }

    /// Create a text node. Empty text isn't allowed.
    pub fn text(&self, text: impl Into<Text>, marks: &[Mark]) -> Result<Node> {
        let text_type = self.text_type();
        let attrs = text_type
            .default_attrs()
            .cloned()
            .expect("text has no attributes");
        Node::new_text(text_type, attrs, text.into(), Mark::set_from(marks))
    }

    /// Create a mark, as `schema.mark(type, attrs)`.
    pub fn mark(&self, mark_type: &MarkType, attrs: Option<&Map>) -> Result<Mark> {
        mark_type.create(attrs)
    }
}

fn position(nodes: &[NodeTypeData], name: &str) -> Option<usize> {
    nodes.iter().position(|node| &*node.name == name)
}

fn gather_marks<'a>(
    marks: &[MarkTypeData],
    names: impl Iterator<Item = &'a str>,
) -> Result<Vec<usize>> {
    let mut found = Vec::new();
    for name in names {
        if let Some(index) = marks.iter().position(|mark| &*mark.name == name) {
            found.push(index);
            continue;
        }
        let mut ok = false;
        for (index, mark) in marks.iter().enumerate() {
            let in_group = mark
                .spec
                .group
                .as_deref()
                .is_some_and(|group| !group.is_empty() && group.split(' ').any(|g| g == name));
            if name == "_" || in_group {
                found.push(index);
                ok = true;
            }
        }
        if !ok {
            return Err(Error::Syntax(format!("Unknown mark type: '{name}'")));
        }
    }
    Ok(found)
}

/// A node type in a schema.
#[derive(Clone)]
pub struct NodeType {
    pub(crate) schema: Schema,
    pub(crate) index: usize,
}

impl PartialEq for NodeType {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.schema == other.schema
    }
}

impl Eq for NodeType {}

impl fmt::Debug for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "NodeType({})", self.name())
    }
}

impl NodeType {
    pub(crate) fn data(&self) -> &NodeTypeData {
        &self.schema.0.nodes[self.index]
    }

    pub fn name(&self) -> &str {
        &self.data().name
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The type's position in its schema's list of node types.
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn spec(&self) -> &NodeSpec {
        &self.data().spec
    }

    pub fn groups(&self) -> &[String] {
        &self.data().groups
    }

    pub fn is_block(&self) -> bool {
        self.data().is_block
    }

    pub fn is_text(&self) -> bool {
        self.data().is_text
    }

    pub fn is_inline(&self) -> bool {
        !self.is_block()
    }

    pub fn inline_content(&self) -> bool {
        self.data().inline_content
    }

    pub fn is_textblock(&self) -> bool {
        self.is_block() && self.inline_content()
    }

    pub fn is_leaf(&self) -> bool {
        self.data().content.is_empty_match()
    }

    pub fn is_atom(&self) -> bool {
        self.is_leaf() || self.spec().atom
    }

    pub fn is_in_group(&self, group: &str) -> bool {
        self.groups().iter().any(|g| g == group)
    }

    pub fn content_match(&self) -> ContentMatch {
        ContentMatch::start(self.schema.clone(), self.data().content.clone())
    }

    /// The marks allowed in nodes of this type, `None` when all are.
    pub fn mark_set(&self) -> Option<Vec<MarkType>> {
        self.data().mark_set.as_ref().map(|indexes| {
            indexes
                .iter()
                .map(|&index| MarkType {
                    schema: self.schema.clone(),
                    index,
                })
                .collect()
        })
    }

    pub fn whitespace(&self) -> Whitespace {
        let spec = self.spec();
        spec.whitespace.unwrap_or(if spec.code {
            Whitespace::Pre
        } else {
            Whitespace::Normal
        })
    }

    pub fn has_required_attrs(&self) -> bool {
        self.data().attrs.has_required()
    }

    /// The attributes every node of the type gets when none are given, if all have defaults.
    pub fn default_attrs(&self) -> Option<&Attrs> {
        self.data().attrs.defaults()
    }

    pub fn compatible_content(&self, other: &NodeType) -> bool {
        self == other || self.content_match().compatible(&other.content_match())
    }

    pub fn compute_attrs(&self, attrs: Option<&Map>) -> Result<Attrs> {
        self.attrs_given(&attrs.into())
    }

    /// The attributes JavaScript makes of what it's given for a node of this type.
    pub fn attrs_given(&self, given: &Given) -> Result<Attrs> {
        self.data().attrs.compute(given)
    }

    /// Create a node of this type, as `NodeType.create`.
    pub fn create(&self, attrs: Option<&Map>, content: Fragment, marks: &[Mark]) -> Result<Node> {
        if self.is_text() {
            return Err(Error::Other(
                "NodeType.create can't construct text nodes".into(),
            ));
        }
        Ok(Node::new(
            self.clone(),
            self.compute_attrs(attrs)?,
            content,
            Mark::set_from(marks),
        ))
    }

    /// [`create`](Self::create), checking the content fits the type.
    pub fn create_checked(
        &self,
        attrs: Option<&Map>,
        content: Fragment,
        marks: &[Mark],
    ) -> Result<Node> {
        self.check_content(&content)?;
        Ok(Node::new(
            self.clone(),
            self.compute_attrs(attrs)?,
            content,
            Mark::set_from(marks),
        ))
    }

    /// [`create`](Self::create), adding nodes at the start or end of the content where it needs
    /// them to fit. `None` when no such nodes make it fit.
    pub fn create_and_fill(
        &self,
        attrs: Option<&Map>,
        content: Fragment,
        marks: &[Mark],
    ) -> Result<Option<Node>> {
        let attrs = self.compute_attrs(attrs)?;
        let mut content = content;
        if content.size() > 0 {
            let Some(before) = self.content_match().fill_before(&content, false, 0)? else {
                return Ok(None);
            };
            content = before.append(&content);
        }
        let matched = self
            .content_match()
            .match_fragment(&content, 0, content.child_count());
        let after = match matched {
            Some(matched) => matched.fill_before(&Fragment::empty(), true, 0)?,
            None => None,
        };
        let Some(after) = after else {
            return Ok(None);
        };
        Ok(Some(Node::new(
            self.clone(),
            attrs,
            content.append(&after),
            Mark::set_from(marks),
        )))
    }

    pub fn valid_content(&self, content: &Fragment) -> bool {
        let result = self
            .content_match()
            .match_fragment(content, 0, content.child_count());
        if !result.is_some_and(|result| result.valid_end()) {
            return false;
        }
        content
            .children()
            .iter()
            .all(|child| self.allows_marks(child.marks()))
    }

    pub fn check_content(&self, content: &Fragment) -> Result<()> {
        if self.valid_content(content) {
            return Ok(());
        }
        let described = content.to_debug_string()?;
        Err(Error::Range(format!(
            "Invalid content for node {}: {}",
            self.name(),
            Text::from(described).slice(0, 50).to_string_lossy()
        )))
    }

    pub fn check_attrs(&self, attrs: &Map) -> Result<()> {
        self.data().attrs.check(attrs, "node", self.name())
    }

    pub fn allows_mark_type(&self, mark_type: &MarkType) -> bool {
        match &self.data().mark_set {
            None => true,
            Some(set) => mark_type.schema == self.schema && set.contains(&mark_type.index),
        }
    }

    pub fn allows_marks(&self, marks: &[Mark]) -> bool {
        self.data().mark_set.is_none()
            || marks
                .iter()
                .all(|mark| self.allows_mark_type(mark.mark_type()))
    }

    /// The given marks without those this type doesn't allow.
    pub fn allowed_marks(&self, marks: &Marks) -> Marks {
        if self.allows_marks(marks) {
            return marks.clone();
        }
        let kept: Vec<Mark> = marks
            .iter()
            .filter(|mark| self.allows_mark_type(mark.mark_type()))
            .cloned()
            .collect();
        if kept.is_empty() {
            Mark::none()
        } else {
            kept.into()
        }
    }
}

/// A mark type in a schema.
#[derive(Clone)]
pub struct MarkType {
    pub(crate) schema: Schema,
    pub(crate) index: usize,
}

impl PartialEq for MarkType {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.schema == other.schema
    }
}

impl Eq for MarkType {}

impl fmt::Debug for MarkType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MarkType({})", self.name())
    }
}

impl MarkType {
    pub(crate) fn data(&self) -> &MarkTypeData {
        &self.schema.0.marks[self.index]
    }

    pub fn name(&self) -> &str {
        &self.data().name
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Where marks of this type sort in a set.
    pub fn rank(&self) -> usize {
        self.index
    }

    pub fn spec(&self) -> &MarkSpec {
        &self.data().spec
    }

    pub fn default_attrs(&self) -> Option<&Attrs> {
        self.data().attrs.defaults()
    }

    /// Create a mark of this type, filling in attributes' defaults.
    pub fn create(&self, attrs: Option<&Map>) -> Result<Mark> {
        self.create_given(&attrs.into())
    }

    /// Create a mark of this type from what JavaScript gives it as attributes.
    pub fn create_given(&self, given: &Given) -> Result<Mark> {
        Ok(Mark::new(self.clone(), self.data().attrs.compute(given)?))
    }

    /// The set without marks of this type.
    pub fn remove_from_set(&self, set: &Marks) -> Marks {
        if !set.iter().any(|mark| mark.mark_type() == self) {
            return set.clone();
        }
        let kept: Vec<Mark> = set
            .iter()
            .filter(|mark| mark.mark_type() != self)
            .cloned()
            .collect();
        if kept.is_empty() {
            Mark::none()
        } else {
            kept.into()
        }
    }

    /// The mark of this type in the set, if there is one.
    pub fn is_in_set<'a>(&self, set: &'a [Mark]) -> Option<&'a Mark> {
        set.iter().find(|mark| mark.mark_type() == self)
    }

    pub fn check_attrs(&self, attrs: &Map) -> Result<()> {
        self.data().attrs.check(attrs, "mark", self.name())
    }

    /// The mark types this one excludes.
    pub fn excluded(&self) -> Vec<MarkType> {
        self.data()
            .excluded
            .iter()
            .map(|&index| MarkType {
                schema: self.schema.clone(),
                index,
            })
            .collect()
    }

    pub fn excludes(&self, other: &MarkType) -> bool {
        other.schema == self.schema && self.data().excluded.contains(&other.index)
    }
}
