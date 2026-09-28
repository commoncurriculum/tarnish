//! Schemas: the node and mark types a document may hold, and what each may contain.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use super::attrs::{AttrSet, AttributeSpec};
use super::content::{Automaton, ContentMatch};
use super::fragment::Fragment;
use super::mark::{Mark, Marks};
use super::node::Node;
use crate::error::{Error, Result};
use crate::js::Given;
use crate::json::Map;
use crate::text::Text;

/// A function the host gives a node spec, such as `leafText`.
pub type NodeHook<T> = Arc<dyn for<'a> Fn(&Node<'a>) -> Result<T> + Send + Sync>;

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

    pub fn is_leaf(&self) -> bool {
        self.content.is_empty_match()
    }

    /// Whether nodes of the type can be made to fill content: text can't, nor can a type with
    /// attributes no defaults give.
    pub fn is_generatable(&self) -> bool {
        !(self.is_text || self.attrs.has_required())
    }

    pub fn allows_mark(&self, rank: usize) -> bool {
        self.mark_set
            .as_ref()
            .is_none_or(|allowed| allowed.contains(&rank))
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
    /// What a chunk of the schema's documents is checked against: a hash of its types' names.
    pub fingerprint: u64,
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

/// FNV-1a over the types' names, which is the same in every process.
fn fingerprint(nodes: &[NodeTypeData], marks: &[MarkTypeData]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let names = nodes
        .iter()
        .map(|node| (b'n', &node.name))
        .chain(marks.iter().map(|mark| (b'm', &mark.name)));
    for (kind, name) in names {
        for byte in [kind].iter().chain(name.as_bytes()).chain(&[0]) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
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
        if nodes.len() > usize::from(u16::MAX) {
            return Err(Error::Range(
                "A schema holds at most 65535 node types".into(),
            ));
        }
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
            fingerprint: fingerprint(&nodes, &marks),
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

    /// A hash of the schema's type names, which a document's chunks carry: a chunk loads only
    /// with a schema that has the same types in the same order.
    pub fn fingerprint(&self) -> u64 {
        self.0.fingerprint
    }

    pub(crate) fn node_data(&self, index: usize) -> &NodeTypeData {
        &self.0.nodes[index]
    }

    /// The node type at `index`.
    pub fn node_type_at(&self, index: usize) -> NodeType<'_> {
        assert!(index < self.0.nodes.len(), "a node type of the schema");
        NodeType {
            schema: self,
            index,
        }
    }

    /// The mark type of rank `rank`.
    pub fn mark_type_at(&self, rank: usize) -> MarkType<'_> {
        assert!(rank < self.0.marks.len(), "a mark type of the schema");
        MarkType {
            schema: self,
            index: rank,
        }
    }

    pub fn node_types(&self) -> impl ExactSizeIterator<Item = NodeType<'_>> + '_ {
        (0..self.0.nodes.len()).map(|index| NodeType {
            schema: self,
            index,
        })
    }

    pub fn mark_types(&self) -> impl ExactSizeIterator<Item = MarkType<'_>> + '_ {
        (0..self.0.marks.len()).map(|index| MarkType {
            schema: self,
            index,
        })
    }

    /// The node type of this name, if the schema has one.
    pub fn node_type(&self, name: &str) -> Option<NodeType<'_>> {
        position(&self.0.nodes, name).map(|index| NodeType {
            schema: self,
            index,
        })
    }

    /// The node type of this name, raising `Unknown node type` when there is none.
    pub fn expect_node_type(&self, name: &str) -> Result<NodeType<'_>> {
        self.node_type(name)
            .ok_or_else(|| Error::Range(format!("Unknown node type: {name}")))
    }

    pub fn mark_type(&self, name: &str) -> Option<MarkType<'_>> {
        self.0
            .marks
            .iter()
            .position(|mark| &*mark.name == name)
            .map(|index| MarkType {
                schema: self,
                index,
            })
    }

    pub fn top_node_type(&self) -> NodeType<'_> {
        NodeType {
            schema: self,
            index: self.0.top_node,
        }
    }

    pub fn linebreak_replacement(&self) -> Option<NodeType<'_>> {
        self.0.linebreak_replacement.map(|index| NodeType {
            schema: self,
            index,
        })
    }

    pub fn text_type(&self) -> NodeType<'_> {
        NodeType {
            schema: self,
            index: self.0.text,
        }
    }

    /// Create a node, checking its content, as `schema.node(type, attrs, content, marks)`.
    pub fn node<'a>(
        &self,
        node_type: &NodeType,
        attrs: Option<&Map>,
        content: Fragment<'a>,
        marks: &[Mark<'a>],
    ) -> Result<Node<'a>> {
        if node_type.schema != self {
            return Err(Error::Range(format!(
                "Node type from different schema used ({})",
                node_type.name()
            )));
        }
        node_type.create_checked(attrs, content, marks)
    }

    /// Create a text node. Empty text isn't allowed.
    pub fn text<'a>(&self, text: impl Into<Text>, marks: &[Mark<'a>]) -> Result<Node<'a>> {
        Node::new_text(self, &text.into(), &Mark::set_from(marks))
    }

    /// Create a mark, as `schema.mark(type, attrs)`.
    pub fn mark(&self, mark_type: &MarkType, attrs: Option<&Map>) -> Result<Mark<'static>> {
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
pub struct NodeType<'s> {
    pub(crate) schema: &'s Schema,
    pub(crate) index: usize,
}

impl PartialEq for NodeType<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.schema == other.schema
    }
}

impl Eq for NodeType<'_> {}

impl fmt::Debug for NodeType<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "NodeType({})", self.name())
    }
}

impl<'s> NodeType<'s> {
    pub(crate) fn data(&self) -> &'s NodeTypeData {
        &self.schema.0.nodes[self.index]
    }

    pub fn name(&self) -> &'s str {
        &self.data().name
    }

    pub fn schema(&self) -> &'s Schema {
        self.schema
    }

    /// The type's position in its schema's list of node types.
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn spec(&self) -> &'s NodeSpec {
        &self.data().spec
    }

    pub fn groups(&self) -> &'s [String] {
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
        self.data().is_leaf()
    }

    pub fn is_atom(&self) -> bool {
        self.is_leaf() || self.spec().atom
    }

    pub fn is_in_group(&self, group: &str) -> bool {
        self.groups().iter().any(|g| g == group)
    }

    pub fn content_match(&self) -> ContentMatch<'s> {
        ContentMatch::start(self.schema, &self.data().content)
    }

    /// The marks allowed in nodes of this type, `None` when all are.
    pub fn mark_set(&self) -> Option<Vec<MarkType<'s>>> {
        self.data().mark_set.as_ref().map(|indexes| {
            indexes
                .iter()
                .map(|&index| MarkType {
                    schema: self.schema,
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
    pub fn default_attrs(&self) -> Option<&'s Map> {
        self.data().attrs.defaults()
    }

    pub fn compatible_content(&self, other: &NodeType) -> bool {
        self == other || self.content_match().compatible(&other.content_match())
    }

    pub fn compute_attrs(&self, attrs: Option<&Map>) -> Result<Map> {
        self.attrs_given(&attrs.into())
    }

    /// The attributes JavaScript makes of what it's given for a node of this type.
    pub fn attrs_given(&self, given: &Given) -> Result<Map> {
        self.data().attrs.compute(given)
    }

    /// Create a node of this type, as `NodeType.create`.
    pub fn create<'a>(
        &self,
        attrs: Option<&Map>,
        content: Fragment<'a>,
        marks: &[Mark<'a>],
    ) -> Result<Node<'a>> {
        if self.is_text() {
            return Err(Error::Other(
                "NodeType.create can't construct text nodes".into(),
            ));
        }
        let attrs = self.compute_attrs(attrs)?;
        Ok(Node::new(self, &attrs, &content, &Mark::set_from(marks)))
    }

    /// [`create`](Self::create), checking the content fits the type.
    pub fn create_checked<'a>(
        &self,
        attrs: Option<&Map>,
        content: Fragment<'a>,
        marks: &[Mark<'a>],
    ) -> Result<Node<'a>> {
        self.check_content(&content)?;
        let attrs = self.compute_attrs(attrs)?;
        Ok(Node::new(self, &attrs, &content, &Mark::set_from(marks)))
    }

    /// [`create`](Self::create), adding nodes at the start or end of the content where it needs
    /// them to fit. `None` when no such nodes make it fit.
    pub fn create_and_fill<'a>(
        &self,
        attrs: Option<&Map>,
        content: Fragment<'a>,
        marks: &[Mark<'a>],
    ) -> Result<Option<Node<'a>>> {
        let attrs = self.compute_attrs(attrs)?;
        let mut content = content;
        if content.size() > 0 {
            let Some(before) = self.content_match().fill_before(&content, false, 0)? else {
                return Ok(None);
            };
            content = before.append(&content);
        }
        let matched = self.content_match().match_fragment(&content);
        let after = match matched {
            Some(matched) => matched.fill_before(&Fragment::empty(), true, 0)?,
            None => None,
        };
        let Some(after) = after else {
            return Ok(None);
        };
        Ok(Some(Node::new(
            self,
            &attrs,
            &content.append(&after),
            &Mark::set_from(marks),
        )))
    }

    pub fn valid_content(&self, content: &Fragment) -> bool {
        let types = content.refs().map(|child| {
            (child.node_type().schema == self.schema).then(|| usize::from(child.record.ty))
        });
        self.data().content.accepts(types)
            && (self.data().mark_set.is_none()
                || content.refs().all(|child| {
                    child
                        .marks()
                        .iter()
                        .all(|mark| self.allows_mark_type(&mark.mark_type()))
                }))
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
        self.data().attrs.check_map(attrs, "node", self.name())
    }

    pub fn allows_mark_type(&self, mark_type: &MarkType) -> bool {
        match &self.data().mark_set {
            None => true,
            Some(set) => mark_type.schema == self.schema && set.contains(&mark_type.index),
        }
    }

    pub fn allows_marks(&self, marks: &Marks) -> bool {
        self.data().mark_set.is_none()
            || marks
                .iter()
                .all(|mark| self.allows_mark_type(&mark.mark_type()))
    }

    pub fn allows_mark_list(&self, marks: &[Mark]) -> bool {
        self.data().mark_set.is_none()
            || marks
                .iter()
                .all(|mark| self.allows_mark_type(&mark.mark_type()))
    }

    /// The given marks without those this type doesn't allow.
    pub fn allowed_marks<'a>(&self, marks: &Marks<'a>) -> Marks<'a> {
        if self.allows_marks(marks) {
            return marks.clone();
        }
        let kept: Vec<Mark<'a>> = marks
            .iter()
            .filter(|mark| self.allows_mark_type(&mark.mark_type()))
            .collect();
        Mark::set_from(&kept)
    }
}

/// A mark type in a schema.
#[derive(Clone, Copy)]
pub struct MarkType<'s> {
    pub(crate) schema: &'s Schema,
    pub(crate) index: usize,
}

impl PartialEq for MarkType<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.schema == other.schema
    }
}

impl Eq for MarkType<'_> {}

impl fmt::Debug for MarkType<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MarkType({})", self.name())
    }
}

impl<'s> MarkType<'s> {
    pub(crate) fn data(&self) -> &'s MarkTypeData {
        &self.schema.0.marks[self.index]
    }

    pub fn name(&self) -> &'s str {
        &self.data().name
    }

    pub fn schema(&self) -> &'s Schema {
        self.schema
    }

    /// Where marks of this type sort in a set.
    pub fn rank(&self) -> usize {
        self.index
    }

    pub fn spec(&self) -> &'s MarkSpec {
        &self.data().spec
    }

    pub fn default_attrs(&self) -> Option<&'s Map> {
        self.data().attrs.defaults()
    }

    /// Create a mark of this type, filling in attributes' defaults.
    pub fn create(&self, attrs: Option<&Map>) -> Result<Mark<'static>> {
        self.create_given(&attrs.into())
    }

    /// Create a mark of this type from what JavaScript gives it as attributes.
    pub fn create_given(&self, given: &Given) -> Result<Mark<'static>> {
        let attrs = self.data().attrs.compute(given)?;
        Ok(Mark::new(self, &attrs))
    }

    /// The set without marks of this type.
    pub fn remove_from_set<'a>(&self, set: &Marks<'a>) -> Marks<'a> {
        if !set.iter().any(|mark| mark.mark_type() == *self) {
            return set.clone();
        }
        let kept: Vec<Mark<'a>> = set
            .iter()
            .filter(|mark| mark.mark_type() != *self)
            .collect();
        Mark::set_from(&kept)
    }

    /// The mark of this type in the set, if there is one.
    pub fn is_in_set<'a>(&self, set: &Marks<'a>) -> Option<Mark<'a>> {
        set.iter().find(|mark| mark.mark_type() == *self)
    }

    /// The mark of this type in a list of marks, if there is one.
    pub fn is_in_list<'m, 'a>(&self, marks: &'m [Mark<'a>]) -> Option<&'m Mark<'a>> {
        marks.iter().find(|mark| mark.mark_type() == *self)
    }

    pub fn check_attrs(&self, attrs: &Map) -> Result<()> {
        self.data().attrs.check_map(attrs, "mark", self.name())
    }

    /// The mark types this one excludes.
    pub fn excluded(&self) -> Vec<MarkType<'s>> {
        self.data()
            .excluded
            .iter()
            .map(|&index| MarkType {
                schema: self.schema,
                index,
            })
            .collect()
    }

    pub fn excludes(&self, other: &MarkType) -> bool {
        other.schema == self.schema && self.data().excluded.contains(&other.index)
    }
}
