//! Slices, and replacing a range of a document with one.

use super::fragment::Fragment;
use super::node::Node;
use super::resolved_pos::ResolvedPos;
use super::schema::Schema;
use crate::error::{Error, Result};
use crate::js;
use crate::json::{Map, NULL, Value};
use crate::stack;

/// A piece cut out of a document: its content, and how deep it is cut open at each end.
#[derive(Clone, Debug, PartialEq)]
pub struct Slice {
    content: Fragment,
    open_start: usize,
    open_end: usize,
}

impl Slice {
    /// A slice of `content` open to these depths, which its first and last nodes must reach.
    pub fn new(content: Fragment, open_start: usize, open_end: usize) -> Slice {
        Slice {
            content,
            open_start,
            open_end,
        }
    }

    pub fn empty() -> Slice {
        Slice::new(Fragment::empty(), 0, 0)
    }

    pub fn content(&self) -> &Fragment {
        &self.content
    }

    pub fn open_start(&self) -> usize {
        self.open_start
    }

    pub fn open_end(&self) -> usize {
        self.open_end
    }

    /// The size the slice adds to a document it goes into.
    pub fn size(&self) -> usize {
        self.content
            .size()
            .saturating_sub(self.open_start + self.open_end)
    }

    /// The slice with `fragment` put in at `pos`, or `None` where it doesn't fit.
    pub fn insert_at(&self, pos: usize, fragment: &Fragment) -> Result<Option<Slice>> {
        let content = insert_into(
            &self.content,
            pos + self.open_start,
            fragment,
            self.open_start + 1,
            self.open_end + 1,
            None,
        )?;
        Ok(content.map(|content| Slice::new(content, self.open_start, self.open_end)))
    }

    /// The slice without its content from `from` to `to`, which must be flat.
    pub fn remove_between(&self, from: usize, to: usize) -> Result<Slice> {
        let content = remove_range(&self.content, from + self.open_start, to + self.open_start)?;
        Ok(Slice::new(content, self.open_start, self.open_end))
    }

    /// `toString`.
    pub fn to_debug_string(&self) -> Result<String> {
        Ok(format!(
            "{}({},{})",
            self.content.to_debug_string()?,
            self.open_start,
            self.open_end
        ))
    }

    /// The slice as JSON, `null` when it is empty.
    pub fn to_json(&self) -> Value {
        if self.content.size() == 0 {
            return Value::Null;
        }
        let mut json = Map::with_capacity(3);
        json.push("content".into(), self.content.to_json());
        if self.open_start > 0 {
            json.push("openStart".into(), self.open_start.into());
        }
        if self.open_end > 0 {
            json.push("openEnd".into(), self.open_end.into());
        }
        Value::Object(json)
    }

    pub fn from_json(schema: &Schema, json: &Value) -> Result<Slice> {
        if !js::truthy(Some(json)) {
            return Ok(Slice::empty());
        }
        let depth = |value: Option<&Value>| match value {
            value if !js::truthy(value) => Some(0),
            Some(Value::Number(number)) => number
                .as_f64()
                .filter(|number| number.fract() == 0.0 && *number >= 0.0)
                .map(|number| number as usize),
            _ => None,
        };
        let (Some(open_start), Some(open_end)) =
            (depth(json.get("openStart")), depth(json.get("openEnd")))
        else {
            return Err(Error::Range("Invalid input for Slice.fromJSON".into()));
        };
        Ok(Slice::new(
            Fragment::from_json(schema, json.get("content").unwrap_or(&NULL))?,
            open_start,
            open_end,
        ))
    }

    /// A slice of `fragment` open as deep as it goes at both ends, but not into isolating
    /// nodes unless `open_isolating`.
    pub fn max_open(fragment: Fragment, open_isolating: bool) -> Slice {
        let open_start = open_depth(
            &fragment,
            open_isolating,
            Fragment::first_child,
            Node::first_child,
        );
        let open_end = open_depth(
            &fragment,
            open_isolating,
            Fragment::last_child,
            Node::last_child,
        );
        Slice::new(fragment, open_start, open_end)
    }
}

fn open_depth(
    fragment: &Fragment,
    open_isolating: bool,
    edge: fn(&Fragment) -> Option<&Node>,
    next: fn(&Node) -> Option<&Node>,
) -> usize {
    let mut depth = 0;
    let mut node = edge(fragment);
    while let Some(current) = node
        && !current.is_leaf()
        && (open_isolating || !current.node_type().spec().isolating)
    {
        depth += 1;
        node = next(current);
    }
    depth
}

fn remove_range(content: &Fragment, from: usize, to: usize) -> Result<Fragment> {
    let (index, offset) = content.find_index(from)?;
    let child = content.maybe_child(index);
    let (index_to, offset_to) = content.find_index(to)?;
    if offset == from || child.is_some_and(Node::is_text) {
        if offset_to != to && !content.child(index_to)?.is_text() {
            return Err(Error::Range("Removing non-flat range".into()));
        }
        return Ok(content
            .cut(0, from)
            .append(&content.cut(to, content.size())));
    }
    if index != index_to {
        return Err(Error::Range("Removing non-flat range".into()));
    }
    let child = child.expect("a child around the position");
    let inner = stack::grow(|| remove_range(child.content(), from - offset - 1, to - offset - 1))?;
    Ok(content.replace_child(index, child.copy(inner)))
}

fn insert_into(
    content: &Fragment,
    dist: usize,
    insert: &Fragment,
    open_start: usize,
    open_end: usize,
    parent: Option<&Node>,
) -> Result<Option<Fragment>> {
    let (index, offset) = content.find_index(dist)?;
    let child = content.maybe_child(index);
    if offset == dist || child.is_some_and(Node::is_text) {
        if let Some(parent) = parent
            && open_start == 0
            && open_end == 0
            && !parent.can_replace(index, index, insert, 0, insert.child_count())?
        {
            return Ok(None);
        }
        return Ok(Some(
            content
                .cut(0, dist)
                .append(insert)
                .append(&content.cut(dist, content.size())),
        ));
    }
    let child = child.expect("a child around the position");
    let inner = stack::grow(|| {
        insert_into(
            child.content(),
            dist - offset - 1,
            insert,
            if index == 0 {
                open_start.saturating_sub(1)
            } else {
                0
            },
            if index + 1 == content.child_count() {
                open_end.saturating_sub(1)
            } else {
                0
            },
            Some(child),
        )
    })?;
    Ok(inner.map(|inner| content.replace_child(index, child.copy(inner))))
}

/// The document `from` is in, with `from` to `to` replaced by `slice`.
pub(crate) fn replace(from: &ResolvedPos, to: &ResolvedPos, slice: &Slice) -> Result<Node> {
    if slice.open_start > from.depth() {
        return Err(Error::Replace(
            "Inserted content deeper than insertion position".into(),
        ));
    }
    if from.depth() + slice.open_end != to.depth() + slice.open_start {
        return Err(Error::Replace("Inconsistent open depths".into()));
    }
    replace_outer(from, to, slice, 0)
}

fn replace_outer(
    from: &ResolvedPos,
    to: &ResolvedPos,
    slice: &Slice,
    depth: usize,
) -> Result<Node> {
    let index = from.index(depth);
    let node = from.node(depth);
    if index == to.index(depth) && depth < from.depth() - slice.open_start {
        let inner = stack::grow(|| replace_outer(from, to, slice, depth + 1))?;
        Ok(node.copy(node.content().replace_child(index, inner)))
    } else if slice.content.size() == 0 {
        close(node, replace_two_way(from, to, depth)?)
    } else if slice.open_start == 0
        && slice.open_end == 0
        && from.depth() == depth
        && to.depth() == depth
    {
        let parent = from.parent();
        let content = parent.content();
        close(
            parent,
            content
                .cut(0, from.parent_offset())
                .append(&slice.content)
                .append(&content.cut(to.parent_offset(), content.size())),
        )
    } else {
        let (start, end) = prepare_slice_for_replace(slice, from)?;
        close(node, replace_three_way(from, &start, &end, to, depth)?)
    }
}

fn check_join(main: &Node, sub: &Node) -> Result<()> {
    if !sub.node_type().compatible_content(main.node_type()) {
        return Err(Error::Replace(format!(
            "Cannot join {} onto {}",
            sub.node_type().name(),
            main.node_type().name()
        )));
    }
    Ok(())
}

fn joinable<'a>(before: &'a ResolvedPos, after: &ResolvedPos, depth: usize) -> Result<&'a Node> {
    let node = before.node(depth);
    check_join(node, after.node(depth))?;
    Ok(node)
}

fn add_node(child: Node, target: &mut Vec<Node>) {
    if let Some(last) = target.last_mut()
        && let Some(joined) = last.join_text(&child)
    {
        *last = joined;
    } else {
        target.push(child);
    }
}

fn add_range(
    start: Option<&ResolvedPos>,
    end: Option<&ResolvedPos>,
    depth: usize,
    target: &mut Vec<Node>,
) {
    let node = end.or(start).expect("a position").node(depth);
    let mut start_index = 0;
    let end_index = end.map_or(node.child_count(), |end| end.index(depth));
    if let Some(start) = start {
        start_index = start.index(depth);
        if start.depth() > depth {
            start_index += 1;
        } else if start.text_offset() > 0 {
            add_node(start.node_after().expect("text after"), target);
            start_index += 1;
        }
    }
    for child in &node.children()[start_index.min(end_index)..end_index] {
        add_node(child.clone(), target);
    }
    if let Some(end) = end
        && end.depth() == depth
        && end.text_offset() > 0
    {
        add_node(end.node_before().expect("text before"), target);
    }
}

fn close(node: &Node, content: Fragment) -> Result<Node> {
    if !node.node_type().valid_content(&content) {
        return Err(Error::Replace(format!(
            "Invalid content for node {}",
            node.node_type().name()
        )));
    }
    Ok(node.copy(content))
}

fn replace_three_way(
    from: &ResolvedPos,
    start: &ResolvedPos,
    end: &ResolvedPos,
    to: &ResolvedPos,
    depth: usize,
) -> Result<Fragment> {
    let open_start = if from.depth() > depth {
        Some(joinable(from, start, depth + 1)?)
    } else {
        None
    };
    let open_end = if to.depth() > depth {
        Some(joinable(end, to, depth + 1)?)
    } else {
        None
    };
    let mut content = Vec::new();
    add_range(None, Some(from), depth, &mut content);
    match (open_start, open_end) {
        (Some(open_start), Some(open_end)) if start.index(depth) == end.index(depth) => {
            check_join(open_start, open_end)?;
            let inner = stack::grow(|| replace_three_way(from, start, end, to, depth + 1))?;
            add_node(close(open_start, inner)?, &mut content);
        }
        _ => {
            if let Some(open_start) = open_start {
                add_node(
                    close(
                        open_start,
                        stack::grow(|| replace_two_way(from, start, depth + 1))?,
                    )?,
                    &mut content,
                );
            }
            add_range(Some(start), Some(end), depth, &mut content);
            if let Some(open_end) = open_end {
                add_node(
                    close(
                        open_end,
                        stack::grow(|| replace_two_way(end, to, depth + 1))?,
                    )?,
                    &mut content,
                );
            }
        }
    }
    add_range(Some(to), None, depth, &mut content);
    Ok(Fragment::new(content))
}

fn replace_two_way(from: &ResolvedPos, to: &ResolvedPos, depth: usize) -> Result<Fragment> {
    let mut content = Vec::new();
    add_range(None, Some(from), depth, &mut content);
    if from.depth() > depth {
        let node = joinable(from, to, depth + 1)?;
        add_node(
            close(node, stack::grow(|| replace_two_way(from, to, depth + 1))?)?,
            &mut content,
        );
    }
    add_range(Some(to), None, depth, &mut content);
    Ok(Fragment::new(content))
}

/// The slice placed in copies of `along`'s ancestors, and resolved where it starts and ends.
fn prepare_slice_for_replace(
    slice: &Slice,
    along: &ResolvedPos,
) -> Result<(ResolvedPos, ResolvedPos)> {
    let extra = along.depth() - slice.open_start;
    let mut node = along.node(extra).copy(slice.content.clone());
    for depth in (0..extra).rev() {
        node = along.node(depth).copy(Fragment::from_node(node));
    }
    let start = node.resolve(slice.open_start + extra)?;
    let size = node.content().size();
    let Some(end) = size.checked_sub(slice.open_end + extra) else {
        let end = size as isize - (slice.open_end + extra) as isize;
        return Err(Error::Range(format!("Position {end} out of range")));
    };
    Ok((start, node.resolve(end)?))
}
