//! Changing a document's structure: lifting, wrapping, splitting, joining, and changing types.

use super::map::Mappable;
use super::mark::{clear_incompatible, line_breaks};
use super::step::Step;
use super::transform::Transform;
use crate::error::{Error, Result};
use crate::js;
use crate::json::Map;
use crate::model::{Fragment, Mark, Node, NodeRange, NodeType, Slice, Whitespace};

/// A node type to wrap content in, and its attributes.
#[derive(Clone, Debug)]
pub struct Wrapper<'s> {
    pub node_type: NodeType<'s>,
    pub attrs: Option<Map>,
}

impl Wrapper<'_> {
    fn create<'a>(&self, content: Fragment<'a>) -> Result<Node<'a>> {
        self.node_type.create(self.attrs.as_ref(), content, &[])
    }
}

/// The attributes `set_block_type` gives each textblock: the same for all, or from a function
/// of the old block.
pub enum BlockAttrs<'f, 'a> {
    Fixed(Option<&'f Map>),
    Hook(&'f mut dyn FnMut(&Node<'a>) -> Result<Option<Map>>),
}

fn can_cut(node: &Node, start: usize, end: usize) -> Result<bool> {
    Ok(
        (start == 0 || node.can_replace(start, node.child_count(), &Fragment::empty(), 0, 0)?)
            && (end == node.child_count() || node.can_replace(0, end, &Fragment::empty(), 0, 0)?),
    )
}

/// The depth the range's content can be lifted to, not across isolating nodes.
pub fn lift_target(range: &NodeRange) -> Result<Option<usize>> {
    let parent = range.parent();
    let content = parent
        .content()
        .cut_by_index(range.start_index(), range.end_index());
    let (from, to) = (range.resolved_from(), range.resolved_to());
    let (mut content_before, mut content_after) = (0, 0);
    let mut depth = range.depth();
    loop {
        let node = from.node(depth);
        let index = from.index(depth) + content_before;
        let end_index = to.index_after(depth) - content_after;
        if depth < range.depth()
            && node.can_replace(index, end_index, &content, 0, content.child_count())?
        {
            return Ok(Some(depth));
        }
        if depth == 0 || node.node_type().spec().isolating || !can_cut(node, index, end_index)? {
            return Ok(None);
        }
        if index > 0 {
            content_before = 1;
        }
        if end_index < node.child_count() {
            content_after = 1;
        }
        depth -= 1;
    }
}

/// A way to wrap the range's content in a node of this type: the wrappers around it and inside
/// it, with the node itself. With `inner_range`, that range's content is what must fit.
pub fn find_wrapping<'s>(
    range: &'s NodeRange,
    node_type: &NodeType<'s>,
    attrs: Option<Map>,
    inner_range: Option<&'s NodeRange>,
) -> Result<Option<Vec<Wrapper<'s>>>> {
    let Some(around) = find_wrapping_outside(range, node_type)? else {
        return Ok(None);
    };
    let Some(inner) = find_wrapping_inside(inner_range.unwrap_or(range), node_type)? else {
        return Ok(None);
    };
    let with_attrs = |node_type: NodeType<'s>| Wrapper {
        node_type,
        attrs: None,
    };
    let mut result: Vec<Wrapper> = around.into_iter().map(with_attrs).collect();
    result.push(Wrapper {
        node_type: node_type.clone(),
        attrs,
    });
    result.extend(inner.into_iter().map(with_attrs));
    Ok(Some(result))
}

fn find_wrapping_outside<'s>(
    range: &'s NodeRange,
    node_type: &NodeType<'s>,
) -> Result<Option<Vec<NodeType<'s>>>> {
    let (parent, start, end) = (range.parent(), range.start_index(), range.end_index());
    let Some(around) = parent.content_match_at(start)?.find_wrapping(node_type) else {
        return Ok(None);
    };
    let outer = around.first().unwrap_or(node_type);
    Ok(parent
        .can_replace_with(start, end, outer, None)?
        .then_some(around))
}

fn find_wrapping_inside<'s>(
    range: &'s NodeRange,
    node_type: &NodeType<'s>,
) -> Result<Option<Vec<NodeType<'s>>>> {
    let (parent, start, end) = (range.parent(), range.start_index(), range.end_index());
    let inner = parent.child(start)?;
    let Some(inside) = node_type.content_match().find_wrapping(&inner.node_type()) else {
        return Ok(None);
    };
    let last = inside.last().unwrap_or(node_type);
    let mut inner_match = Some(last.content_match());
    for index in start..end {
        let Some(current) = inner_match else { break };
        inner_match = current.match_type(&parent.child(index)?.node_type());
    }
    Ok(inner_match
        .is_some_and(|found| found.valid_end())
        .then_some(inside))
}

/// What becomes of line breaks in text moved into a textblock type, in a schema with a
/// linebreak replacement node.
enum NewlineConversion<'s> {
    /// The type keeps newlines, being `pre`, and doesn't allow the replacement node, which
    /// becomes a newline.
    ToNewlines(NodeType<'s>),
    /// The type allows the replacement node, which newlines become, and doesn't keep them.
    ToLinebreaks(NodeType<'s>),
}

fn newline_conversion<'s>(node_type: &NodeType<'s>) -> Option<NewlineConversion<'s>> {
    let linebreak = node_type.schema().linebreak_replacement()?;
    let pre = node_type.whitespace() == Whitespace::Pre;
    let allowed = node_type.content_match().match_type(&linebreak).is_some();
    match (pre, allowed) {
        (true, false) => Some(NewlineConversion::ToNewlines(linebreak)),
        (false, true) => Some(NewlineConversion::ToLinebreaks(linebreak)),
        _ => None,
    }
}

/// Replace the line breaks in the node's text with `linebreak` nodes.
fn replace_newlines<'a>(
    tr: &mut Transform<'a>,
    node: &Node<'a>,
    pos: usize,
    map_from: usize,
    linebreak: &NodeType,
) -> Result<()> {
    for (offset, child) in node.content().children_with_offsets() {
        let Some(text) = child.text() else { continue };
        for (index, _) in line_breaks(&text.units()) {
            let start = tr.mapping_from(map_from).map(pos + 1 + offset + index, 1);
            let replacement = linebreak.create(None, Fragment::empty(), &[])?;
            tr.replace_with(start, start + 1, Fragment::from_node(replacement))?;
        }
    }
    Ok(())
}

/// Replace the node's `linebreak` nodes with newlines.
fn replace_linebreaks<'a>(
    tr: &mut Transform<'a>,
    node: &Node<'a>,
    pos: usize,
    map_from: usize,
    linebreak: &NodeType,
) -> Result<()> {
    for (offset, child) in node.content().children_with_offsets() {
        if child.node_type() == *linebreak {
            let start = tr.mapping_from(map_from).map(pos + 1 + offset, 1);
            let newline = linebreak.schema().text("\n", &[])?;
            tr.replace_with(start, start + 1, Fragment::from_node(newline))?;
        }
    }
    Ok(())
}

fn can_change_type(doc: &Node, pos: usize, node_type: &NodeType) -> Result<bool> {
    let resolved = doc.resolve(pos)?;
    let index = resolved.index(resolved.depth());
    resolved
        .parent()
        .can_replace_with(index, index + 1, node_type, None)
}

/// A step that gives the node from `start` to `end` the type, attributes and marks of `node`,
/// keeping its content.
fn retype(start: usize, end: usize, node: Node) -> Step {
    Step::ReplaceAround {
        from: start,
        to: end,
        gap_from: start + 1,
        gap_to: end - 1,
        slice: Slice::new(Fragment::from_node(node), 0, 0),
        insert: 1,
        structure: true,
    }
}

/// Whether the node at `pos` can be split, with `depth - 1` of its ancestors, each part split
/// off getting the matching type of `types_after`, outermost first, when it has one.
pub fn can_split(
    doc: &Node,
    pos: usize,
    depth: usize,
    types_after: &[Option<Wrapper>],
) -> Result<bool> {
    let type_after = |i: usize| types_after.get(i).and_then(Option::as_ref);
    let resolved = doc.resolve(pos)?;
    let Some(base) = resolved.depth().checked_sub(depth) else {
        return Ok(false);
    };
    let parent = resolved.parent();
    let index = resolved.index(resolved.depth());
    let inner_type = match types_after.last().and_then(Option::as_ref) {
        Some(wrapper) => wrapper.node_type.clone(),
        None => parent.node_type(),
    };
    if parent.node_type().spec().isolating
        || !parent.can_replace(index, parent.child_count(), &Fragment::empty(), 0, 0)?
        || !inner_type.valid_content(&parent.content().cut_by_index(index, parent.child_count()))
    {
        return Ok(false);
    }
    for d in (base + 1..resolved.depth()).rev() {
        let node = resolved.node(d);
        let index = resolved.index(d);
        if node.node_type().spec().isolating {
            return Ok(false);
        }
        let i = d - base - 1;
        let mut rest = node.content().cut_by_index(index, node.child_count());
        if let Some(over) = type_after(i + 1) {
            rest = rest.replace_child(0, over.create(Fragment::empty())?);
        }
        let after = match type_after(i) {
            Some(wrapper) => wrapper.node_type.clone(),
            None => node.node_type(),
        };
        if !node.can_replace(index + 1, node.child_count(), &Fragment::empty(), 0, 0)?
            || !after.valid_content(&rest)
        {
            return Ok(false);
        }
    }
    let index = resolved.index_after(base);
    let base_type = match type_after(0) {
        Some(wrapper) => wrapper.node_type.clone(),
        // A `depth` of 0 leaves no node below `base`, whose type JavaScript reads.
        None if base == resolved.depth() => {
            return Err(js::type_error(js::Nullish::Undefined, "type"));
        }
        None => resolved.node(base + 1).node_type(),
    };
    resolved
        .node(base)
        .can_replace_with(index, index, &base_type, None)
}

/// Whether the blocks before and after `pos` can be joined.
pub fn can_join(doc: &Node, pos: usize) -> Result<bool> {
    let resolved = doc.resolve(pos)?;
    let index = resolved.index(resolved.depth());
    Ok(joinable(
        resolved.node_before().as_ref(),
        resolved.node_after().as_ref(),
    )? && resolved
        .parent()
        .can_replace(index, index + 1, &Fragment::empty(), 0, 0)?)
}

fn can_append_with_substituted_linebreaks(a: &Node, b: &Node) -> Result<bool> {
    let schema = a.node_type().schema();
    let linebreak = schema.linebreak_replacement();
    let mut matched = a.content_match_at(a.child_count())?;
    for child in b.children() {
        let child_type = child.node_type();
        let node_type = if linebreak.as_ref() == Some(&child_type) {
            schema.text_type()
        } else {
            child_type
        };
        let Some(next) = matched.match_type(&node_type) else {
            return Ok(false);
        };
        matched = next;
        if !a.node_type().allows_marks(&child.marks()) {
            return Ok(false);
        }
    }
    Ok(matched.valid_end())
}

fn joinable(a: Option<&Node>, b: Option<&Node>) -> Result<bool> {
    match (a, b) {
        (Some(a), Some(b)) if !a.is_leaf() => can_append_with_substituted_linebreaks(a, b),
        _ => Ok(false),
    }
}

/// An ancestor of `pos` that can be joined to the block before it, or with a positive `dir`,
/// after it: the position to join at.
pub fn join_point(doc: &Node, pos: usize, dir: i32) -> Result<Option<usize>> {
    let resolved = doc.resolve(pos)?;
    let mut pos = pos;
    let mut d = resolved.depth();
    loop {
        let mut index = resolved.index(d);
        let (before, after) = if d == resolved.depth() {
            (resolved.node_before(), resolved.node_after())
        } else if dir > 0 {
            index += 1;
            (
                Some(resolved.node(d + 1).clone()),
                resolved.node(d).maybe_child(index),
            )
        } else {
            let before = index
                .checked_sub(1)
                .and_then(|i| resolved.node(d).maybe_child(i));
            (before, Some(resolved.node(d + 1).clone()))
        };
        if let Some(before) = &before
            && !before.is_textblock()
            && joinable(Some(before), after.as_ref())?
            && resolved
                .node(d)
                .can_replace(index, index + 1, &Fragment::empty(), 0, 0)?
        {
            return Ok(Some(pos));
        }
        if d == 0 {
            return Ok(None);
        }
        pos = if dir < 0 {
            resolved.before(d)?
        } else {
            resolved.after(d)?
        };
        d -= 1;
    }
}

/// A position at or around `pos` where a node of this type can be inserted, looking up at
/// parents when `pos` is at the start or end of one.
pub fn insert_point(doc: &Node, pos: usize, node_type: &NodeType) -> Result<Option<usize>> {
    let resolved = doc.resolve(pos)?;
    let index = resolved.index(resolved.depth());
    if resolved
        .parent()
        .can_replace_with(index, index, node_type, None)?
    {
        return Ok(Some(pos));
    }
    if resolved.parent_offset() == 0 {
        for d in (0..resolved.depth()).rev() {
            let index = resolved.index(d);
            if resolved
                .node(d)
                .can_replace_with(index, index, node_type, None)?
            {
                return Ok(Some(resolved.before(d + 1)?));
            }
            if index > 0 {
                return Ok(None);
            }
        }
    }
    if resolved.parent_offset() == resolved.parent().content().size() {
        for d in (0..resolved.depth()).rev() {
            let index = resolved.index_after(d);
            if resolved
                .node(d)
                .can_replace_with(index, index, node_type, None)?
            {
                return Ok(Some(resolved.after(d + 1)?));
            }
            if index < resolved.node(d).child_count() {
                return Ok(None);
            }
        }
    }
    Ok(None)
}

/// A position at or around `pos` where the slice can be inserted, trying parents' nearest
/// boundaries.
pub fn drop_point(doc: &Node, pos: usize, slice: &Slice) -> Result<Option<usize>> {
    let resolved = doc.resolve(pos)?;
    if slice.content().size() == 0 {
        return Ok(Some(pos));
    }
    let mut content = slice.content().clone();
    for _ in 0..slice.open_start() {
        content = js::non_null(content.first_child(), "content")?
            .content()
            .clone();
    }
    let passes = if slice.open_start() == 0 && slice.size() > 0 {
        2
    } else {
        1
    };
    for pass in 1..=passes {
        for d in (0..=resolved.depth()).rev() {
            let bias: i32 = if d == resolved.depth() {
                0
            } else if resolved.pos() as f64
                <= (resolved.start(d + 1) + resolved.end(d + 1)) as f64 / 2.0
            {
                -1
            } else {
                1
            };
            let insert_pos = resolved.index(d) + usize::from(bias > 0);
            let parent = resolved.node(d);
            let fits = if pass == 1 {
                parent.can_replace(insert_pos, insert_pos, &content, 0, content.child_count())?
            } else {
                let first = content
                    .first_child()
                    .expect("a closed slice with a size has a first child");
                match parent
                    .content_match_at(insert_pos)?
                    .find_wrapping(&first.node_type())
                {
                    Some(wrapping) if !wrapping.is_empty() => {
                        parent.can_replace_with(insert_pos, insert_pos, &wrapping[0], None)?
                    }
                    _ => false,
                }
            };
            if fits {
                return Ok(Some(match bias {
                    0 => resolved.pos(),
                    bias if bias < 0 => resolved.before(d + 1)?,
                    _ => resolved.after(d + 1)?,
                }));
            }
        }
    }
    Ok(None)
}

impl<'a> Transform<'a> {
    /// Lift the range's content out of its parent to `target` depth.
    pub fn lift(&mut self, range: &NodeRange<'a>, target: usize) -> Result<&mut Self> {
        let (from, to, depth) = (range.resolved_from(), range.resolved_to(), range.depth());
        let gap_start = from.before(depth + 1)?;
        let gap_end = to.after(depth + 1)?;
        let (mut start, mut end) = (gap_start, gap_end);

        let mut before = Fragment::empty();
        let mut open_start = 0;
        let mut splitting = false;
        for d in (target + 1..=depth).rev() {
            if splitting || from.index(d) > 0 {
                splitting = true;
                before = Fragment::from_node(from.node(d).copy(before));
                open_start += 1;
            } else {
                start -= 1;
            }
        }
        let mut after = Fragment::empty();
        let mut open_end = 0;
        let mut splitting = false;
        for d in (target + 1..=depth).rev() {
            if splitting || to.after(d + 1)? < to.end(d) {
                splitting = true;
                after = Fragment::from_node(to.node(d).copy(after));
                open_end += 1;
            } else {
                end += 1;
            }
        }
        let insert = before.size() - open_start;
        self.step(Step::ReplaceAround {
            from: start,
            to: end,
            gap_from: gap_start,
            gap_to: gap_end,
            slice: Slice::new(before.append(&after), open_start, open_end),
            insert,
            structure: true,
        })
    }

    /// Wrap the range in these nodes, outermost first.
    pub fn wrap(&mut self, range: &NodeRange<'a>, wrappers: &[Wrapper]) -> Result<&mut Self> {
        let mut content = Fragment::empty();
        for wrapper in wrappers.iter().rev() {
            if content.size() > 0 {
                let matched = wrapper.node_type.content_match().match_fragment(&content);
                if !matched.is_some_and(|matched| matched.valid_end()) {
                    return Err(Error::Range(
                        "Wrapper type given to Transform.wrap does not form valid content of its parent wrapper".into(),
                    ));
                }
            }
            content = Fragment::from_node(wrapper.create(content)?);
        }
        let (start, end) = (range.start(), range.end());
        self.step(Step::ReplaceAround {
            from: start,
            to: end,
            gap_from: start,
            gap_to: end,
            slice: Slice::new(content, 0, 0),
            insert: wrappers.len(),
            structure: true,
        })
    }

    /// Give the textblocks between `from` and `to` this type, and these attributes.
    pub fn set_block_type(
        &mut self,
        from: usize,
        to: usize,
        node_type: &NodeType,
        mut attrs: BlockAttrs<'_, 'a>,
    ) -> Result<&mut Self> {
        if !node_type.is_textblock() {
            return Err(Error::Range(
                "Type given to setBlockType should be a textblock".into(),
            ));
        }
        let map_from = self.steps().len();
        let conversion = newline_conversion(node_type);
        self.doc().clone().nodes_between(
            from,
            to,
            &mut |node, pos, _, _| {
                let hooked;
                let attrs_here = match &mut attrs {
                    BlockAttrs::Fixed(attrs) => *attrs,
                    BlockAttrs::Hook(hook) => {
                        hooked = hook(node)?;
                        hooked.as_ref()
                    }
                };
                if !(node.is_textblock()
                    && !node.has_markup(node_type, attrs_here, None)
                    && can_change_type(
                        self.doc(),
                        self.mapping_from(map_from).map(pos, 1),
                        node_type,
                    )?)
                {
                    return Ok(true);
                }
                if let Some(NewlineConversion::ToNewlines(linebreak)) = &conversion {
                    replace_linebreaks(self, node, pos, map_from, linebreak)?;
                }
                // Clear the markup the new type doesn't allow.
                let at = self.mapping_from(map_from).map(pos, 1);
                clear_incompatible(self, at, node_type, None, conversion.is_none())?;
                let mapping = self.mapping_from(map_from);
                let (start, end) = (mapping.map(pos, 1), mapping.map(pos + node.node_size(), 1));
                let block =
                    node_type.create(attrs_here, Fragment::empty(), &node.marks().to_vec())?;
                self.step(retype(start, end, block))?;
                if let Some(NewlineConversion::ToLinebreaks(linebreak)) = &conversion {
                    replace_newlines(self, node, pos, map_from, linebreak)?;
                }
                Ok(false)
            },
            0,
        )?;
        Ok(self)
    }

    /// Change the type, attributes or marks of the node at `pos`, keeping its type when none
    /// is given and its marks when none are.
    pub fn set_node_markup(
        &mut self,
        pos: usize,
        node_type: Option<&NodeType>,
        attrs: Option<&Map>,
        marks: Option<&[Mark<'a>]>,
    ) -> Result<&mut Self> {
        let node = self
            .doc()
            .node_at(pos)?
            .ok_or_else(|| Error::Range("No node at given position".into()))?;
        let node_type = node_type.cloned().unwrap_or_else(|| node.node_type());
        let own = node.marks().to_vec();
        let new_node = node_type.create(attrs, Fragment::empty(), marks.unwrap_or(&own))?;
        if node.is_leaf() {
            return self.replace_with(pos, pos + node.node_size(), Fragment::from_node(new_node));
        }
        if !node_type.valid_content(node.content()) {
            return Err(Error::Range(format!(
                "Invalid content for node type {}",
                node_type.name()
            )));
        }
        self.step(retype(pos, pos + node.node_size(), new_node))
    }

    /// Split the node at `pos`, and `depth - 1` of its ancestors. Each part split off gets the
    /// matching type of `types_after`, outermost first, when it has one, or its original's.
    pub fn split(
        &mut self,
        pos: usize,
        depth: usize,
        types_after: &[Option<Wrapper>],
    ) -> Result<&mut Self> {
        let resolved = self.doc().resolve(pos)?;
        let mut before = Fragment::empty();
        let mut after = Fragment::empty();
        // A `depth` past the top can't split: the slice opens deeper than `pos` is, and the step
        // fails. JavaScript builds it on through ancestors at negative depths, which count back
        // from `pos`'s own, failing the same way, or with a TypeError when `depth` runs past
        // those too.
        for (d, i) in (1..=resolved.depth()).rev().zip((0..depth).rev()) {
            before = Fragment::from_node(resolved.node(d).copy(before));
            after = Fragment::from_node(match types_after.get(i).and_then(Option::as_ref) {
                Some(wrapper) => wrapper.create(after)?,
                None => resolved.node(d).copy(after),
            });
        }
        self.step(Step::Replace {
            from: pos,
            to: pos,
            slice: Slice::new(before.append(&after), depth, depth),
            structure: true,
        })
    }

    /// Join the blocks around `pos`, and their last and first descendants down `depth` levels.
    pub fn join(&mut self, pos: usize, depth: usize) -> Result<&mut Self> {
        let Some(before_pos) = pos.checked_sub(depth) else {
            let before_pos = pos as i128 - depth as i128;
            return Err(Error::Range(format!("Position {before_pos} out of range")));
        };
        let before = self.doc().resolve(before_pos)?;
        let before_parent = before.parent().clone();
        let before_type = before_parent.node_type();
        let conversion = if before_type.inline_content() {
            newline_conversion(&before_type)
        } else {
            None
        };
        let map_from = self.steps().len();
        if let Some(NewlineConversion::ToNewlines(linebreak)) = &conversion {
            let after = self.doc().resolve(pos + depth)?;
            let (node, at) = (after.parent().clone(), after.before(after.depth())?);
            replace_linebreaks(self, &node, at, map_from, linebreak)?;
        }
        if before_type.inline_content() {
            let start = before_parent.content_match_at(before.index(before.depth()))?;
            clear_incompatible(
                self,
                pos + depth - 1,
                &before_type,
                Some(start),
                conversion.is_none(),
            )?;
        }
        let mapping = self.mapping_from(map_from);
        let start = mapping.map(before_pos, 1);
        self.step(Step::Replace {
            from: start,
            to: mapping.map(pos + depth, -1),
            slice: Slice::empty(),
            structure: true,
        })?;
        if let Some(NewlineConversion::ToLinebreaks(linebreak)) = &conversion {
            let full = self.doc().resolve(start)?;
            let (node, at) = (full.parent().clone(), full.before(full.depth())?);
            let steps = self.steps().len();
            replace_newlines(self, &node, at, steps, linebreak)?;
        }
        Ok(self)
    }
}
