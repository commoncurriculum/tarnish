//! Changing a document's structure: lifting, wrapping, splitting, joining, and changing types.

use super::map::Mappable;
use super::mark::{clear_incompatible, line_breaks};
use super::step::Step;
use super::transform::{BlockAttrs, Transform};
use crate::error::{Error, Result};
use crate::json::Map;
use crate::model::{
    Attrs, ContentMatch, Fragment, Mark, Node, NodeRange, NodeType, Slice, Whitespace,
};

/// A node type to wrap content in, and its attributes.
#[derive(Clone, Debug)]
pub struct Wrapper {
    pub node_type: NodeType,
    pub attrs: Option<Attrs>,
}

impl Wrapper {
    fn create(&self, content: Fragment) -> Result<Node> {
        self.node_type.create(self.attrs.as_deref(), content, &[])
    }
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

pub(crate) fn lift(tr: &mut Transform, range: &NodeRange, target: usize) -> Result<()> {
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
    tr.step(Step::ReplaceAround {
        from: start,
        to: end,
        gap_from: gap_start,
        gap_to: gap_end,
        slice: Slice::new(before.append(&after), open_start, open_end),
        insert,
        structure: true,
    })?;
    Ok(())
}

/// A way to wrap the range's content in a node of this type: the wrappers around it and inside
/// it, with the node itself. With `inner_range`, that range's content is what must fit.
pub fn find_wrapping(
    range: &NodeRange,
    node_type: &NodeType,
    attrs: Option<Attrs>,
    inner_range: Option<&NodeRange>,
) -> Result<Option<Vec<Wrapper>>> {
    let Some(around) = find_wrapping_outside(range, node_type)? else {
        return Ok(None);
    };
    let Some(inner) = find_wrapping_inside(inner_range.unwrap_or(range), node_type)? else {
        return Ok(None);
    };
    let with_attrs = |node_type: NodeType| Wrapper {
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

fn find_wrapping_outside(range: &NodeRange, node_type: &NodeType) -> Result<Option<Vec<NodeType>>> {
    let (parent, start, end) = (range.parent(), range.start_index(), range.end_index());
    let Some(around) = parent.content_match_at(start)?.find_wrapping(node_type) else {
        return Ok(None);
    };
    let outer = around.first().unwrap_or(node_type);
    Ok(parent
        .can_replace_with(start, end, outer, None)?
        .then_some(around))
}

fn find_wrapping_inside(range: &NodeRange, node_type: &NodeType) -> Result<Option<Vec<NodeType>>> {
    let (parent, start, end) = (range.parent(), range.start_index(), range.end_index());
    let inner = parent.child(start)?;
    let Some(inside) = node_type.content_match().find_wrapping(inner.node_type()) else {
        return Ok(None);
    };
    let last = inside.last().unwrap_or(node_type);
    let mut inner_match = Some(last.content_match());
    for index in start..end {
        let Some(current) = inner_match else { break };
        inner_match = current.match_type(parent.child(index)?.node_type());
    }
    Ok(inner_match
        .is_some_and(|found| found.valid_end())
        .then_some(inside))
}

pub(crate) fn wrap(tr: &mut Transform, range: &NodeRange, wrappers: &[Wrapper]) -> Result<()> {
    let mut content = Fragment::empty();
    for wrapper in wrappers.iter().rev() {
        if content.size() > 0 {
            let matched = wrapper.node_type.content_match().match_fragment(
                &content,
                0,
                content.child_count(),
            );
            if !matched.is_some_and(|matched| matched.valid_end()) {
                return Err(Error::Range(
                    "Wrapper type given to Transform.wrap does not form valid content of its parent wrapper".into(),
                ));
            }
        }
        content = Fragment::from_node(wrapper.create(content)?);
    }
    let (start, end) = (range.start(), range.end());
    tr.step(Step::ReplaceAround {
        from: start,
        to: end,
        gap_from: start,
        gap_to: end,
        slice: Slice::new(content, 0, 0),
        insert: wrappers.len(),
        structure: true,
    })?;
    Ok(())
}

pub(crate) fn set_block_type(
    tr: &mut Transform,
    from: usize,
    to: usize,
    node_type: &NodeType,
    mut attrs: BlockAttrs,
) -> Result<()> {
    if !node_type.is_textblock() {
        return Err(Error::Range(
            "Type given to setBlockType should be a textblock".into(),
        ));
    }
    let map_from = tr.steps().len();
    let schema = node_type.schema().clone();
    tr.doc().clone().nodes_between(
        from,
        to,
        &mut |node, pos, _, _| {
            let attrs_here: Option<Attrs> = match &mut attrs {
                BlockAttrs::Fixed(attrs) => attrs.map(|attrs| std::sync::Arc::new(attrs.clone())),
                BlockAttrs::Hook(hook) => hook(node)?,
            };
            if !(node.is_textblock()
                && !node.has_markup(node_type, attrs_here.as_deref(), None)
                && can_change_type(tr.doc(), tr.map_from(map_from, pos, 1), node_type)?)
            {
                return Ok(true);
            }
            let mut convert_newlines = None;
            if let Some(linebreak) = schema.linebreak_replacement() {
                let pre = node_type.whitespace() == Whitespace::Pre;
                let supports_linebreak = node_type.content_match().match_type(&linebreak).is_some();
                if pre && !supports_linebreak {
                    convert_newlines = Some(false);
                } else if !pre && supports_linebreak {
                    convert_newlines = Some(true);
                }
            }
            // Clear the markup the new type doesn't allow.
            if convert_newlines == Some(false) {
                replace_linebreaks(tr, node, pos, map_from)?;
            }
            let at = tr.map_from(map_from, pos, 1);
            clear_incompatible(tr, at, node_type, None, convert_newlines.is_none())?;
            let mapping = tr.mapping_from(map_from);
            let start = mapping.map(pos, 1);
            let end = mapping.map(pos + node.node_size(), 1);
            let block = node_type.create(attrs_here.as_deref(), Fragment::empty(), node.marks())?;
            tr.step(Step::ReplaceAround {
                from: start,
                to: end,
                gap_from: start + 1,
                gap_to: end - 1,
                slice: Slice::new(Fragment::from_node(block), 0, 0),
                insert: 1,
                structure: true,
            })?;
            if convert_newlines == Some(true) {
                replace_newlines(tr, node, pos, map_from)?;
            }
            Ok(false)
        },
        0,
    )
}

/// Replace the line breaks in the node's text with its schema's linebreak replacement.
fn replace_newlines(tr: &mut Transform, node: &Node, pos: usize, map_from: usize) -> Result<()> {
    let schema = node.node_type().schema().clone();
    for (offset, child) in node.content().children_with_offsets() {
        let Some(text) = child.text() else { continue };
        for (index, _) in line_breaks(text.units()) {
            let start = tr.map_from(map_from, pos + 1 + offset + index, 1);
            let linebreak = schema
                .linebreak_replacement()
                .expect("a linebreak replacement")
                .create(None, Fragment::empty(), &[])?;
            tr.replace_with(start, start + 1, Fragment::from_node(linebreak))?;
        }
    }
    Ok(())
}

/// Replace the node's linebreak replacements with newlines.
fn replace_linebreaks(tr: &mut Transform, node: &Node, pos: usize, map_from: usize) -> Result<()> {
    let schema = node.node_type().schema().clone();
    for (offset, child) in node.content().children_with_offsets() {
        if Some(child.node_type().clone()) == schema.linebreak_replacement() {
            let start = tr.map_from(map_from, pos + 1 + offset, 1);
            let newline = schema.text("\n", &[])?;
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

pub(crate) fn set_node_markup(
    tr: &mut Transform,
    pos: usize,
    node_type: Option<&NodeType>,
    attrs: Option<&Map>,
    marks: Option<&[Mark]>,
) -> Result<()> {
    let node = tr
        .doc()
        .node_at(pos)?
        .cloned()
        .ok_or_else(|| Error::Range("No node at given position".into()))?;
    let node_type = node_type.unwrap_or(node.node_type());
    let new_node = node_type.create(attrs, Fragment::empty(), marks.unwrap_or(node.marks()))?;
    if node.is_leaf() {
        tr.replace_with(pos, pos + node.node_size(), Fragment::from_node(new_node))?;
        return Ok(());
    }
    if !node_type.valid_content(node.content()) {
        return Err(Error::Range(format!(
            "Invalid content for node type {}",
            node_type.name()
        )));
    }
    tr.step(Step::ReplaceAround {
        from: pos,
        to: pos + node.node_size(),
        gap_from: pos + 1,
        gap_to: pos + node.node_size() - 1,
        slice: Slice::new(Fragment::from_node(new_node), 0, 0),
        insert: 1,
        structure: true,
    })?;
    Ok(())
}

/// A type in `types_after`, which may be shorter than asked for, or have no type at a depth.
fn type_after(types_after: Option<&[Option<Wrapper>]>, index: isize) -> Option<&Wrapper> {
    let types = types_after?;
    if index < 0 {
        return None;
    }
    types.get(index as usize)?.as_ref()
}

/// Whether the node at `pos` can be split, with `depth - 1` of its ancestors.
pub fn can_split(
    doc: &Node,
    pos: usize,
    depth: usize,
    types_after: Option<&[Option<Wrapper>]>,
) -> Result<bool> {
    let resolved = doc.resolve(pos)?;
    let Some(base) = resolved.depth().checked_sub(depth) else {
        return Ok(false);
    };
    let parent = resolved.parent();
    let index = resolved.index(resolved.depth());
    let inner_type = match types_after
        .and_then(|types| types.last())
        .and_then(Option::as_ref)
    {
        Some(wrapper) => wrapper.node_type.clone(),
        None => parent.node_type().clone(),
    };
    if parent.node_type().spec().isolating
        || !parent.can_replace(index, parent.child_count(), &Fragment::empty(), 0, 0)?
        || !inner_type.valid_content(&parent.content().cut_by_index(index, parent.child_count()))
    {
        return Ok(false);
    }
    let mut i = depth as isize - 2;
    for d in (base + 1..resolved.depth()).rev() {
        let node = resolved.node(d);
        let index = resolved.index(d);
        if node.node_type().spec().isolating {
            return Ok(false);
        }
        let mut rest = node.content().cut_by_index(index, node.child_count());
        if let Some(over) = type_after(types_after, i + 1) {
            rest = rest.replace_child(0, over.create(Fragment::empty())?);
        }
        let after = match type_after(types_after, i) {
            Some(wrapper) => wrapper.node_type.clone(),
            None => node.node_type().clone(),
        };
        if !node.can_replace(index + 1, node.child_count(), &Fragment::empty(), 0, 0)?
            || !after.valid_content(&rest)
        {
            return Ok(false);
        }
        i -= 1;
    }
    let index = resolved.index_after(base);
    let base_type = match type_after(types_after, 0) {
        Some(wrapper) => wrapper.node_type.clone(),
        None => resolved.node(base + 1).node_type().clone(),
    };
    resolved
        .node(base)
        .can_replace_with(index, index, &base_type, None)
}

pub(crate) fn split(
    tr: &mut Transform,
    pos: usize,
    depth: usize,
    types_after: Option<&[Option<Wrapper>]>,
) -> Result<()> {
    let resolved = tr.doc().resolve(pos)?;
    let mut before = Fragment::empty();
    let mut after = Fragment::empty();
    let mut i = depth as isize - 1;
    for d in (resolved.depth().saturating_sub(depth) + 1..=resolved.depth()).rev() {
        before = Fragment::from_node(resolved.node(d).copy(before));
        after = Fragment::from_node(match type_after(types_after, i) {
            Some(wrapper) => wrapper.create(after)?,
            None => resolved.node(d).copy(after),
        });
        i -= 1;
    }
    tr.step(Step::Replace {
        from: pos,
        to: pos,
        slice: Slice::new(before.append(&after), depth, depth),
        structure: true,
    })?;
    Ok(())
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
        let node_type = if Some(child.node_type()) == linebreak.as_ref() {
            schema.node_type("text").expect("a text type")
        } else {
            child.node_type().clone()
        };
        let Some(next) = matched.match_type(&node_type) else {
            return Ok(false);
        };
        matched = next;
        if !a.node_type().allows_marks(child.marks()) {
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
                resolved.node(d).maybe_child(index).cloned(),
            )
        } else {
            let before = index
                .checked_sub(1)
                .and_then(|i| resolved.node(d).maybe_child(i))
                .cloned();
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

pub(crate) fn join(tr: &mut Transform, pos: usize, depth: usize) -> Result<()> {
    let mut convert_newlines = None;
    let schema = tr.doc().node_type().schema().clone();
    let before = tr.doc().resolve(pos - depth)?;
    let before_type = before.parent().node_type().clone();
    if let Some(linebreak) = schema.linebreak_replacement()
        && before_type.inline_content()
    {
        let pre = before_type.whitespace() == Whitespace::Pre;
        let supports_linebreak = before_type.content_match().match_type(&linebreak).is_some();
        if pre && !supports_linebreak {
            convert_newlines = Some(false);
        } else if !pre && supports_linebreak {
            convert_newlines = Some(true);
        }
    }
    let map_from = tr.steps().len();
    if convert_newlines == Some(false) {
        let after = tr.doc().resolve(pos + depth)?;
        let (node, at) = (after.parent().clone(), after.before(after.depth())?);
        replace_linebreaks(tr, &node, at, map_from)?;
    }
    if before_type.inline_content() {
        let start: ContentMatch = before
            .parent()
            .content_match_at(before.index(before.depth()))?;
        clear_incompatible(
            tr,
            pos + depth - 1,
            &before_type,
            Some(start),
            convert_newlines.is_none(),
        )?;
    }
    let mapping = tr.mapping_from(map_from);
    let start = mapping.map(pos - depth, 1);
    tr.step(Step::Replace {
        from: start,
        to: mapping.map(pos + depth, -1),
        slice: Slice::empty(),
        structure: true,
    })?;
    if convert_newlines == Some(true) {
        let full = tr.doc().resolve(start)?;
        let (node, at) = (full.parent().clone(), full.before(full.depth())?);
        let steps = tr.steps().len();
        replace_newlines(tr, &node, at, steps)?;
    }
    Ok(())
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
        content = content
            .first_child()
            .expect("an open node")
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
                let first = content.first_child().expect("content");
                match parent
                    .content_match_at(insert_pos)?
                    .find_wrapping(first.node_type())
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
