//! A node being parsed into, and the nodes it holds until it is written.

use super::PreserveWhitespace;
use super::html::{BLOCK_TAGS, is_tag, trailing_spaces};
use crate::Result;
use crate::chunk::{Builder, Kid, LOCAL};
use crate::dom::Dom;
use crate::js::text::Text;
use crate::json::Map;
use crate::model::{ContentMatch, Fragment, Mark, Marks, Node, NodeType, Whitespace};

/// How a context treats whitespace, and whether it is open at the start.
#[derive(Clone, Copy)]
pub(super) struct WsOptions {
    pub(super) preserve: PreserveWhitespace,
    /// Whether nothing comes before the context at the open start of an open parse, where its
    /// content needn't fit from its type's start.
    pub(super) open_left: bool,
}

impl WsOptions {
    /// The options of a node of this type opened in a context with these: the whitespace its
    /// rule sets, or all for a `pre` type, or the context's; and open at the start when first
    /// in a context that is.
    pub(super) fn child(
        self,
        node_type: &NodeType,
        preserve: Option<PreserveWhitespace>,
        first: bool,
    ) -> WsOptions {
        let preserve = match preserve {
            Some(preserve) => preserve,
            None if node_type.whitespace() == Whitespace::Pre => PreserveWhitespace::Full,
            None => self.preserve,
        };
        WsOptions {
            preserve,
            open_left: self.open_left && first,
        }
    }
}

/// A node of a context's content. The parse writes every node it makes into one chunk, each
/// once its parent is finished, so what it holds until then is what the node will be.
pub(super) enum Item<'s> {
    /// Text and its marks, joined with the text after it when that has the same marks, as
    /// `Fragment.from` joins text nodes.
    Text(Text, Vec<Mark<'static>>),
    /// A node without content that a rule made: its type, the attributes given, and its marks.
    Leaf(NodeType<'s>, Option<Map>, Vec<Mark<'static>>),
    /// A node the parse has written: its id in the parse's chunk, its type and its size.
    Written(u32, NodeType<'s>, u32),
    /// A node made elsewhere, such as the content a rule gives, that isn't text.
    Node(Node<'static>),
}

impl<'s> Item<'s> {
    pub(super) fn is_inline(&self) -> bool {
        match self {
            Item::Text(..) => true,
            Item::Leaf(node_type, ..) | Item::Written(_, node_type, _) => node_type.is_inline(),
            Item::Node(node) => node.is_inline(),
        }
    }

    pub(super) fn text(&self) -> Option<&Text> {
        match self {
            Item::Text(text, _) => Some(text),
            _ => None,
        }
    }

    pub(super) fn node_size(&self) -> usize {
        match self {
            Item::Text(text, _) => text.len(),
            Item::Leaf(..) => 1,
            Item::Written(_, _, size) => *size as usize,
            Item::Node(node) => node.node_size(),
        }
    }

    /// Writes the item into the parse's chunk, or lists the node made elsewhere: its kid.
    fn write(self, builder: &mut Builder<'static>) -> Result<Kid> {
        Ok(match self {
            Item::Text(text, marks) => {
                let text_type = builder.schema().text_type().index() as u16;
                let marks = Marks::write_list(builder, &marks);
                let id = builder.text_of(text_type, marks, &text);
                local(id, text.len())
            }
            Item::Leaf(node_type, attrs, marks) => {
                let attrs = node_type.write_attrs(builder, attrs.as_ref())?;
                let marks = Marks::write_list(builder, &marks);
                let id = builder.element(node_type.index() as u16, marks, attrs, 0, 0, 0);
                local(id, 1)
            }
            Item::Written(id, _, size) => local(id, size as usize),
            Item::Node(node) => builder.kid(node.chunk(), node.index(), node.node_size()),
        })
    }
}

fn local(id: u32, size: usize) -> Kid {
    Kid {
        slot: LOCAL,
        index: id,
        size: u32::try_from(size).expect("a node smaller than 4G positions"),
    }
}

/// Writes the items as a list of kids, joining each run of text with the same marks into one
/// text node: where the list starts, how many kids it has, and the positions they take.
fn write_list(builder: &mut Builder<'static>, items: Vec<Item>) -> Result<(u32, u32, u32)> {
    let mut kids = Vec::with_capacity(items.len());
    let mut run: Option<(Vec<Text>, Vec<Mark<'static>>)> = None;
    for item in items {
        match item {
            Item::Text(text, marks) => match &mut run {
                Some((texts, run_marks)) if *run_marks == marks => texts.push(text),
                _ => {
                    if let Some(run) = run.replace((vec![text], marks)) {
                        kids.push(write_run(builder, run)?);
                    }
                }
            },
            item => {
                if let Some(run) = run.take() {
                    kids.push(write_run(builder, run)?);
                }
                kids.push(item.write(builder)?);
            }
        }
    }
    if let Some(run) = run {
        kids.push(write_run(builder, run)?);
    }
    let size = kids.iter().map(|kid| u64::from(kid.size)).sum::<u64>();
    let size = u32::try_from(size).expect("content smaller than 4G positions");
    let count = kids.len() as u32;
    let first = match count {
        0 => 0,
        _ => builder.push_kids(kids.into_iter()),
    };
    Ok((first, count, size))
}

fn write_run(
    builder: &mut Builder<'static>,
    (texts, marks): (Vec<Text>, Vec<Mark<'static>>),
) -> Result<Kid> {
    let text = match texts.len() {
        1 => texts.into_iter().next().expect("a text"),
        _ => texts.iter().collect(),
    };
    Item::Text(text, marks).write(builder)
}

/// A node being built while parsing, of a type of the parser's schema.
pub(super) struct NodeContext<'s> {
    /// An identity, for finding the context again after the stack has changed.
    pub(super) id: usize,
    /// The node's type, which only the top of an open parse without a top node lacks.
    pub(super) node_type: Option<NodeType<'s>>,
    pub(super) attrs: Option<Map>,
    pub(super) marks: Vec<Mark<'static>>,
    pub(super) solid: bool,
    pub(super) matched: Option<ContentMatch<'s>>,
    pub(super) ws: WsOptions,
    pub(super) content: Vec<Item<'s>>,
}

impl<'s> NodeContext<'s> {
    /// A context with no content yet. Without a match given, its content starts at its type's
    /// start, unless it is open at the start, where the match is found from the content.
    pub(super) fn new(
        id: usize,
        node_type: Option<NodeType<'s>>,
        attrs: Option<Map>,
        marks: Vec<Mark<'static>>,
        solid: bool,
        matched: Option<ContentMatch<'s>>,
        ws: WsOptions,
    ) -> NodeContext<'s> {
        let matched = match (matched, &node_type) {
            (Some(matched), _) => Some(matched),
            (None, Some(node_type)) if !ws.open_left => Some(node_type.content_match()),
            (None, _) => None,
        };
        NodeContext {
            id,
            node_type,
            attrs,
            marks,
            solid,
            matched,
            ws,
            content: Vec::new(),
        }
    }

    /// `findWrapping(node)`, which reads only the node's type.
    pub(super) fn find_wrapping(
        &mut self,
        node_type: &NodeType,
    ) -> Result<Option<Vec<NodeType<'s>>>> {
        if self.matched.is_none() {
            let Some(own_type) = &self.node_type else {
                return Ok(Some(Vec::new()));
            };
            let start = own_type.content_match();
            match start.fill_before_type(node_type, false)? {
                Some(fill) => {
                    self.matched = start.match_fragment(&fill);
                }
                None => {
                    let wrap = start.find_wrapping(node_type);
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
            .and_then(|matched| matched.find_wrapping(node_type)))
    }

    /// Whether text here is inline: by the node's type, or else by its first child, or else by
    /// whether the text's DOM parent is anything but a block.
    pub(super) fn inline_context<D: Dom>(&self, dom: &D, text: Option<&D::Node>) -> Result<bool> {
        if let Some(node_type) = &self.node_type {
            return Ok(node_type.inline_content());
        }
        if let Some(first) = self.content.first() {
            return Ok(first.is_inline());
        }
        match text.map(|text| dom.parent(text)).transpose()?.flatten() {
            Some(parent) => Ok(!is_tag(BLOCK_TAGS, &dom.node_name(&parent)?)),
            None => Ok(false),
        }
    }

    /// The content, without the whitespace it ends in unless it keeps whitespace, and filled out
    /// to fit unless its end is open.
    fn take_content(&mut self, open_end: bool) -> Result<Vec<Item<'s>>> {
        if self.ws.preserve == PreserveWhitespace::No
            && let Some(Item::Text(text, _)) = self.content.last_mut()
        {
            let kept = text.len() - trailing_spaces(text);
            if kept == 0 {
                self.content.pop();
            } else if kept < text.len() {
                *text = text.slice(0, kept);
            }
        }
        let mut content = std::mem::take(&mut self.content);
        if !open_end
            && let Some(matched) = &self.matched
            && let Some(fill) = matched.fill_before(&Fragment::empty(), true, 0)?
        {
            content.extend(fill.children().map(Item::Node));
        }
        Ok(content)
    }

    /// Finish the top of an open parse, as the content parsed into it, written into the parse's
    /// chunk.
    pub(super) fn finish_content(
        mut self,
        open_end: bool,
        mut builder: Builder<'static>,
    ) -> Result<Fragment<'static>> {
        let content = self.take_content(open_end)?;
        let (first, count, size) = write_list(&mut builder, content)?;
        Ok(match count {
            0 => Fragment::empty(),
            _ => Fragment::of(builder.seal(), first, count, size, u32::MAX),
        })
    }

    /// Finish the node, writing it into the parse's chunk. Only the top of an open parse can
    /// lack a type, and it is finished with [`NodeContext::finish_content`].
    pub(super) fn finish_node(
        mut self,
        open_end: bool,
        builder: &mut Builder<'static>,
    ) -> Result<Item<'s>> {
        let content = self.take_content(open_end)?;
        let node_type = self.node_type.expect("a context with a type");
        let (first, count, size) = write_list(builder, content)?;
        let attrs = node_type.write_attrs(builder, self.attrs.as_ref())?;
        let marks = Marks::write_list(builder, &self.marks);
        let id = builder.element(node_type.index() as u16, marks, attrs, first, count, size);
        let node_size = match node_type.is_leaf() {
            true => 1,
            false => size + 2,
        };
        Ok(Item::Written(id, node_type, node_size))
    }
}

/// The node an item written into the parse's chunk is, once the chunk is sealed.
pub(super) fn written_node(builder: Builder<'static>, item: Item) -> Node<'static> {
    let Item::Written(id, ..) = item else {
        unreachable!("a finished node is written");
    };
    Node::at(builder.seal(), id)
}
