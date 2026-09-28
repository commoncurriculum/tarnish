//! Fitting a slice into a gap: `replaceStep`, and the looser `replaceRange` family.

use super::step::Step;
use super::structure::insert_point;
use super::transform::Transform;
use crate::error::Result;
use crate::js;
use crate::json::Map;
use crate::model::{ContentMatch, Fragment, Node, NodeType, ResolvedPos, Schema, Slice};
use crate::stack;

/// A step that fits `slice` in between `from` and `to`, or `None` when there's no meaningful
/// way to, or the step would change nothing.
pub fn replace_step<'a>(
    doc: &Node<'a>,
    from: usize,
    to: usize,
    slice: &Slice<'a>,
) -> Result<Option<Step<'a>>> {
    if from == to && slice.size() == 0 {
        return Ok(None);
    }
    let resolved_from = doc.resolve(from)?;
    let resolved_to = doc.resolve(to)?;
    if fits_trivially(&resolved_from, &resolved_to, slice)? {
        return Ok(Some(Step::Replace {
            from,
            to,
            slice: slice.clone(),
            structure: false,
        }));
    }
    Fitter::new(resolved_from, resolved_to, slice.clone())?.fit()
}

fn fits_trivially(from: &ResolvedPos, to: &ResolvedPos, slice: &Slice) -> Result<bool> {
    Ok(slice.open_start() == 0
        && slice.open_end() == 0
        && from.start(from.depth()) == to.start(to.depth())
        && from.parent().can_replace(
            from.index(from.depth()),
            to.index(to.depth()),
            slice.content(),
            0,
            slice.content().child_count(),
        )?)
}

/// Where to place content from the unplaced slice: its depth in the slice, and the frontier
/// depth it goes to, with nodes to add first or wrappers to open, by their types' indexes.
struct Fittable<'a> {
    slice_depth: usize,
    frontier_depth: usize,
    parent: Option<Node<'a>>,
    inject: Option<Fragment<'a>>,
    wrap: Option<Vec<usize>>,
}

/// An open node on the frontier: its type's index, and the state of that type's content
/// expression after what the node holds.
#[derive(Clone, Copy)]
struct Frontier {
    node_type: usize,
    state: usize,
}

/// Places the content of a slice into the gap between two positions: `frontier` is the open
/// side of what is placed, which starts at `from`, moves forward as content is placed, and is
/// finally reconciled with `to`; `unplaced` is what is left to place, and `placed` what is,
/// open at the start as `from` is and at the end as `frontier` is.
struct Fitter<'a> {
    schema: Schema,
    from: ResolvedPos<'a>,
    to: ResolvedPos<'a>,
    unplaced: Slice<'a>,
    frontier: Vec<Frontier>,
    placed: Fragment<'a>,
}

struct CloseLevel<'a> {
    depth: usize,
    fit: Fragment<'a>,
    move_to: ResolvedPos<'a>,
}

fn frontier_of(node_type: &NodeType, matched: &ContentMatch) -> Frontier {
    Frontier {
        node_type: node_type.index(),
        state: matched.state_index(),
    }
}

/// The type and match a frontier entry names.
fn frontier_parts(schema: &Schema, entry: Frontier) -> (NodeType<'_>, ContentMatch<'_>) {
    let node_type = schema.node_type_at(entry.node_type);
    let matched = node_type
        .content_match()
        .at_state(entry.state)
        .expect("a state of the type's content expression");
    (node_type, matched)
}

impl<'a> Fitter<'a> {
    fn new(from: ResolvedPos<'a>, to: ResolvedPos<'a>, unplaced: Slice<'a>) -> Result<Self> {
        let schema = from.doc().schema().clone();
        let mut frontier = Vec::with_capacity(from.depth() + 1);
        for depth in 0..=from.depth() {
            let node = from.node(depth);
            frontier.push(frontier_of(
                &node.node_type(),
                &node.content_match_at(from.index_after(depth))?,
            ));
        }
        let mut placed = Fragment::empty();
        for depth in (1..=from.depth()).rev() {
            placed = Fragment::from_node(from.node(depth).copy(placed));
        }
        Ok(Fitter {
            schema,
            from,
            to,
            unplaced,
            frontier,
            placed,
        })
    }

    fn depth(&self) -> usize {
        self.frontier.len() - 1
    }

    fn fit(mut self) -> Result<Option<Step<'a>>> {
        // Place what can be placed; when nothing can, open the slice further, or drop a node.
        while self.unplaced.size() > 0 {
            match self.find_fittable()? {
                Some(fit) => self.place_nodes(fit)?,
                None => {
                    if !self.open_more()? {
                        self.drop_node()?;
                    }
                }
            }
        }
        // When inline content comes right after the frontier and after `to`, a ReplaceAround
        // step pulls that content into the node after the frontier, so the fit reaches to
        // the end of the textblock after `to`.
        let move_inline = self.must_move_inline()?;
        // `placed` is open `from.depth()` levels deep at its start and `depth()` at its end, a
        // token each, so this can't go below zero.
        let placed_size = self.placed.size() - self.depth() - self.from.depth();
        let target = match move_inline {
            Some(pos) => self.from.doc().resolve(pos)?,
            None => self.to.clone(),
        };
        let Some(to) = self.close(target)? else {
            return Ok(None);
        };
        let mut content = self.placed.clone();
        let (mut open_start, mut open_end) = (self.from.depth(), to.depth());
        // Drop open parent nodes that only wrap the rest.
        while open_start > 0 && open_end > 0 && content.child_count() == 1 {
            content = content
                .first_child()
                .expect("its one child")
                .content()
                .clone();
            open_start -= 1;
            open_end -= 1;
        }
        let slice = Slice::new(content, open_start, open_end);
        if let Some(move_inline) = move_inline {
            return Ok(Some(Step::ReplaceAround {
                from: self.from.pos(),
                to: move_inline,
                gap_from: self.to.pos(),
                gap_to: self.to.end(self.to.depth()),
                slice,
                insert: placed_size,
                structure: false,
            }));
        }
        if slice.size() > 0 || self.from.pos() != self.to.pos() {
            return Ok(Some(Step::Replace {
                from: self.from.pos(),
                to: to.pos(),
                slice,
                structure: false,
            }));
        }
        Ok(None)
    }

    /// A place on the unplaced slice's start spine with content that fits somewhere on the
    /// frontier.
    fn find_fittable(&self) -> Result<Option<Fittable<'a>>> {
        let mut start_depth = self.unplaced.open_start();
        let mut cur = self.unplaced.content().clone();
        let mut open_end = self.unplaced.open_end();
        for d in 0..start_depth {
            let node = js::non_null(cur.first_child(), "type")?;
            if cur.child_count() > 1 {
                open_end = 0;
            }
            if node.node_type().spec().isolating && open_end <= d {
                start_depth = d;
                break;
            }
            cur = node.content().clone();
        }
        // Only try wrapping nodes, in pass 2, after placing without wrapping failed.
        for pass in 1..=2 {
            let top = if pass == 1 {
                start_depth
            } else {
                self.unplaced.open_start()
            };
            for slice_depth in (0..=top).rev() {
                let (fragment, parent) = if slice_depth > 0 {
                    let above = content_at(self.unplaced.content(), slice_depth - 1)?;
                    let parent = js::non_null(above.first_child(), "content")?;
                    (parent.content().clone(), Some(parent))
                } else {
                    (self.unplaced.content().clone(), None)
                };
                let first = fragment.first_child();
                for frontier_depth in (0..=self.depth()).rev() {
                    let (node_type, matched) =
                        frontier_parts(&self.schema, self.frontier[frontier_depth]);
                    if pass == 1 {
                        // The next node fits, or there's none and the parents look compatible.
                        let (fits, inject) = match &first {
                            Some(first) => match matched.match_type(&first.node_type()) {
                                Some(_) => (true, None),
                                None => match matched.fill_before(
                                    &Fragment::from_node(first.clone()),
                                    false,
                                    0,
                                )? {
                                    Some(inject) => (true, Some(inject)),
                                    None => (false, None),
                                },
                            },
                            None => (
                                parent.as_ref().is_some_and(|parent| {
                                    node_type.compatible_content(&parent.node_type())
                                }),
                                None,
                            ),
                        };
                        if fits {
                            return Ok(Some(Fittable {
                                slice_depth,
                                frontier_depth,
                                parent,
                                inject,
                                wrap: None,
                            }));
                        }
                    } else if let Some(first) = &first
                        && let Some(wrap) = matched.find_wrapping(&first.node_type())
                    {
                        return Ok(Some(Fittable {
                            slice_depth,
                            frontier_depth,
                            parent,
                            inject: None,
                            wrap: Some(wrap.iter().map(NodeType::index).collect()),
                        }));
                    }
                    // Don't look further up when the parent would fit here.
                    if let Some(parent) = &parent
                        && matched.match_type(&parent.node_type()).is_some()
                    {
                        break;
                    }
                }
            }
        }
        Ok(None)
    }

    fn open_more(&mut self) -> Result<bool> {
        let (content, open_start, open_end) = (
            self.unplaced.content().clone(),
            self.unplaced.open_start(),
            self.unplaced.open_end(),
        );
        let inner = content_at(&content, open_start)?;
        match inner.first_child() {
            Some(first) if !first.is_leaf() => {}
            _ => return Ok(false),
        }
        let new_end = if inner.size() + open_start >= content.size().saturating_sub(open_end) {
            open_start + 1
        } else {
            0
        };
        self.unplaced = Slice::new(content, open_start + 1, open_end.max(new_end));
        Ok(true)
    }

    fn drop_node(&mut self) -> Result<()> {
        let (content, open_start, open_end) = (
            self.unplaced.content().clone(),
            self.unplaced.open_start(),
            self.unplaced.open_end(),
        );
        let inner = content_at(&content, open_start)?;
        self.unplaced = if inner.child_count() <= 1 && open_start > 0 {
            let open_at_end =
                content.size().saturating_sub(open_start) <= open_start + inner.size();
            Slice::new(
                drop_from_fragment(&content, open_start - 1, 1)?,
                open_start - 1,
                if open_at_end {
                    open_start - 1
                } else {
                    open_end
                },
            )
        } else {
            Slice::new(
                drop_from_fragment(&content, open_start, 1)?,
                open_start,
                open_end,
            )
        };
        Ok(())
    }

    /// Move content from the unplaced slice at `slice_depth` to the frontier node at
    /// `frontier_depth`, closing that node when it can be.
    fn place_nodes(&mut self, fit: Fittable<'a>) -> Result<()> {
        let Fittable {
            slice_depth,
            frontier_depth,
            parent,
            inject,
            wrap,
        } = fit;
        while self.depth() > frontier_depth {
            self.close_frontier_node()?;
        }
        if let Some(wrap) = &wrap {
            for &node_type in wrap {
                self.open_frontier_node(node_type, None, Fragment::empty())?;
            }
        }
        let slice = self.unplaced.clone();
        let fragment = match &parent {
            Some(parent) => parent.content().clone(),
            None => slice.content().clone(),
        };
        let open_start = slice.open_start() as isize - slice_depth as isize;
        let mut taken = 0;
        let mut add = Vec::new();
        let schema = self.schema.clone();
        let (node_type, mut matched) = frontier_parts(&schema, self.frontier[frontier_depth]);
        if let Some(inject) = &inject {
            add.extend(inject.children());
            matched = js::non_null(matched.match_fragment(inject), "matchType")?;
        }
        // How many nodes are open at the end of the fragment: at 0, only its parent is;
        // below 0, none are.
        let mut open_end_count = (fragment.size() + slice_depth) as isize
            - (slice.content().size() as isize - slice.open_end() as isize);
        while taken < fragment.child_count() {
            let next = fragment.child(taken)?;
            let Some(matches) = matched.match_type(&next.node_type()) else {
                break;
            };
            taken += 1;
            // Empty open nodes are dropped.
            if taken > 1 || open_start == 0 || next.content().size() > 0 {
                matched = matches;
                let marked = next.mark(node_type.allowed_marks(&next.marks()));
                add.push(close_node_start(
                    &marked,
                    if taken == 1 { open_start } else { 0 },
                    if taken == fragment.child_count() {
                        open_end_count
                    } else {
                        -1
                    },
                )?);
            }
        }
        let to_end = taken == fragment.child_count();
        if !to_end {
            open_end_count = -1;
        }
        self.placed = add_to_fragment(&self.placed, frontier_depth, &Fragment::from_array(add))?;
        self.frontier[frontier_depth] = frontier_of(&node_type, &matched);

        // When the parents' types match, and the whole node was moved and isn't open, close
        // the frontier node right away.
        if to_end
            && open_end_count < 0
            && let Some(parent) = &parent
            && parent.node_type().index() == self.frontier[self.depth()].node_type
            && parent.schema() == &self.schema
            && self.frontier.len() > 1
        {
            self.close_frontier_node()?;
        }

        // Add frontier nodes for the nodes open at the end.
        let mut cur = fragment.clone();
        for _ in 0..open_end_count.max(0) {
            let node = js::non_null(cur.last_child(), "type")?;
            self.frontier.push(frontier_of(
                &node.node_type(),
                &node.content_match_at(node.child_count())?,
            ));
            cur = node.content().clone();
        }

        // Drop what was placed from the unplaced slice: the whole node when all of it was
        // placed, the placed nodes otherwise.
        self.unplaced = if !to_end {
            Slice::new(
                drop_from_fragment(slice.content(), slice_depth, taken)?,
                slice.open_start(),
                slice.open_end(),
            )
        } else if slice_depth == 0 {
            Slice::empty()
        } else {
            Slice::new(
                drop_from_fragment(slice.content(), slice_depth - 1, 1)?,
                slice_depth - 1,
                if open_end_count < 0 {
                    slice.open_end()
                } else {
                    slice_depth - 1
                },
            )
        };
        Ok(())
    }

    /// The position after `to`'s textblock to move inline content up to, if it must move.
    fn must_move_inline(&self) -> Result<Option<usize>> {
        if !self.to.parent().is_textblock() {
            return Ok(None);
        }
        let (node_type, matched) = frontier_parts(&self.schema, self.frontier[self.depth()]);
        if !node_type.is_textblock()
            || content_after_fits(&self.to, self.to.depth(), &node_type, &matched, false)?.is_none()
        {
            return Ok(None);
        }
        if self.to.depth() == self.depth()
            && let Some(level) = self.find_close_level(&self.to)?
            && level.depth == self.depth()
        {
            return Ok(None);
        }
        let mut depth = self.to.depth();
        let mut after = self.to.after(depth)?;
        while depth > 1 {
            depth -= 1;
            if after != self.to.end(depth) {
                break;
            }
            after += 1;
        }
        Ok(Some(after))
    }

    fn find_close_level(&self, to: &ResolvedPos<'a>) -> Result<Option<CloseLevel<'a>>> {
        'scan: for i in (0..=self.depth().min(to.depth())).rev() {
            let (node_type, matched) = frontier_parts(&self.schema, self.frontier[i]);
            let drop_inner = i < to.depth() && to.end(i + 1) == to.pos() + (to.depth() - (i + 1));
            let Some(fit) = content_after_fits(to, i, &node_type, &matched, drop_inner)? else {
                continue;
            };
            for d in (0..i).rev() {
                let (node_type, matched) = frontier_parts(&self.schema, self.frontier[d]);
                match content_after_fits(to, d, &node_type, &matched, true)? {
                    Some(matches) if matches.child_count() == 0 => {}
                    _ => continue 'scan,
                }
            }
            let move_to = if drop_inner {
                to.doc().resolve(to.after(i + 1)?)?
            } else {
                to.clone()
            };
            return Ok(Some(CloseLevel {
                depth: i,
                fit,
                move_to,
            }));
        }
        Ok(None)
    }

    fn close(&mut self, to: ResolvedPos<'a>) -> Result<Option<ResolvedPos<'a>>> {
        let Some(close) = self.find_close_level(&to)? else {
            return Ok(None);
        };
        while self.depth() > close.depth {
            self.close_frontier_node()?;
        }
        if close.fit.child_count() > 0 {
            self.placed = add_to_fragment(&self.placed, close.depth, &close.fit)?;
        }
        let to = close.move_to;
        for d in close.depth + 1..=to.depth() {
            let node = to.node(d);
            // Where nothing fills, JavaScript passes the `null` on as the node's content, which
            // `create` takes as empty.
            let add = node
                .node_type()
                .content_match()
                .fill_before(node.content(), true, to.index(d))?
                .unwrap_or_else(Fragment::empty);
            self.open_frontier_node(node.type_index(), Some(node.attrs().to_map()), add)?;
        }
        Ok(Some(to))
    }

    fn open_frontier_node(
        &mut self,
        node_type: usize,
        attrs: Option<Map>,
        content: Fragment<'a>,
    ) -> Result<()> {
        let schema = self.schema.clone();
        let node_type = schema.node_type_at(node_type);
        let depth = self.depth();
        let (top_type, top_match) = frontier_parts(&schema, self.frontier[depth]);
        // Where the node doesn't fit, which only happens while closing, ProseMirror leaves the
        // match null, and nothing reads it after.
        if let Some(matched) = top_match.match_type(&node_type) {
            self.frontier[depth] = frontier_of(&top_type, &matched);
        }
        let node = node_type.create(attrs.as_ref(), content, &[])?;
        self.placed = add_to_fragment(&self.placed, depth, &Fragment::from_node(node))?;
        self.frontier
            .push(frontier_of(&node_type, &node_type.content_match()));
        Ok(())
    }

    fn close_frontier_node(&mut self) -> Result<()> {
        let open = self
            .frontier
            .pop()
            .expect("a frontier node below the root, as every caller checks");
        let (_, matched) = frontier_parts(&self.schema, open);
        let add = js::non_null(
            matched.fill_before(&Fragment::empty(), true, 0)?,
            "childCount",
        )?;
        if add.child_count() > 0 {
            self.placed = add_to_fragment(&self.placed, self.frontier.len(), &add)?;
        }
        Ok(())
    }
}

fn drop_from_fragment<'a>(
    fragment: &Fragment<'a>,
    depth: usize,
    count: usize,
) -> Result<Fragment<'a>> {
    if depth == 0 {
        return Ok(fragment.cut_by_index(count, fragment.child_count()));
    }
    let first = js::non_null(fragment.first_child(), "copy")?;
    let dropped = stack::grow(|| drop_from_fragment(first.content(), depth - 1, count))?;
    Ok(fragment.replace_child(0, first.copy(dropped)))
}

fn add_to_fragment<'a>(
    fragment: &Fragment<'a>,
    depth: usize,
    content: &Fragment<'a>,
) -> Result<Fragment<'a>> {
    if depth == 0 {
        return Ok(fragment.append(content));
    }
    let last = js::non_null(fragment.last_child(), "copy")?;
    let added = stack::grow(|| add_to_fragment(last.content(), depth - 1, content))?;
    Ok(fragment.replace_child(fragment.child_count() - 1, last.copy(added)))
}

fn content_at<'a>(fragment: &Fragment<'a>, depth: usize) -> Result<Fragment<'a>> {
    let mut fragment = fragment.clone();
    for _ in 0..depth {
        fragment = js::non_null(fragment.first_child(), "content")?
            .content()
            .clone();
    }
    Ok(fragment)
}

fn close_node_start<'a>(node: &Node<'a>, open_start: isize, open_end: isize) -> Result<Node<'a>> {
    if open_start <= 0 {
        return Ok(node.clone());
    }
    let mut fragment = node.content().clone();
    if open_start > 1 {
        let first = js::non_null(fragment.first_child(), "content")?;
        let inner_end = if fragment.child_count() == 1 {
            open_end - 1
        } else {
            0
        };
        fragment = fragment.replace_child(
            0,
            stack::grow(|| close_node_start(&first, open_start - 1, inner_end))?,
        );
    }
    let node_type = node.node_type();
    let start = node_type.content_match();
    fragment = js::non_null(start.fill_before(&fragment, false, 0)?, "append")?.append(&fragment);
    if open_end <= 0 {
        let matched = js::non_null(start.match_fragment(&fragment), "fillBefore")?;
        let end = js::non_null(matched.fill_before(&Fragment::empty(), true, 0)?, "size")?;
        fragment = fragment.append(&end);
    }
    Ok(node.copy(fragment))
}

fn content_after_fits<'a>(
    to: &ResolvedPos<'a>,
    depth: usize,
    node_type: &NodeType,
    matched: &ContentMatch,
    open: bool,
) -> Result<Option<Fragment<'a>>> {
    let node = to.node(depth);
    let index = if open {
        to.index_after(depth)
    } else {
        to.index(depth)
    };
    if index == node.child_count() && !node_type.compatible_content(&node.node_type()) {
        return Ok(None);
    }
    let fit = matched.fill_before(node.content(), true, index)?;
    Ok(fit.filter(|_| !invalid_marks(node_type, node.content(), index)))
}

fn invalid_marks(node_type: &NodeType, fragment: &Fragment, start: usize) -> bool {
    fragment
        .children()
        .skip(start)
        .any(|child| !node_type.allows_marks(&child.marks()))
}

/// Where `replace_range` may put a slice: over the whole node at a depth, or from before it to
/// the range's end.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Covering(usize),
    Before(usize),
}

impl Target {
    fn depth(self) -> usize {
        match self {
            Target::Covering(depth) | Target::Before(depth) => depth,
        }
    }
}

fn defines_content(node_type: &NodeType) -> bool {
    let spec = node_type.spec();
    spec.defining || spec.defining_for_content
}

impl<'a> Transform<'a> {
    /// Replace a range with a slice, taking `from`, `to` and the slice's open start as hints
    /// rather than fixed points, as for a paste.
    pub fn replace_range(
        &mut self,
        mut from: usize,
        mut to: usize,
        slice: &Slice<'a>,
    ) -> Result<&mut Self> {
        if slice.size() == 0 {
            return self.delete_range(from, to);
        }
        let resolved_from = self.doc().resolve(from)?;
        let resolved_to = self.doc().resolve(to)?;
        if fits_trivially(&resolved_from, &resolved_to, slice)? {
            return self.step(Step::Replace {
                from,
                to,
                slice: slice.clone(),
                structure: false,
            });
        }

        let mut targets: Vec<Target> = covered_depths(&resolved_from, &resolved_to)
            .into_iter()
            .map(Target::Covering)
            .collect();
        // The whole document can't be replaced.
        if targets.last() == Some(&Target::Covering(0)) {
            targets.pop();
        }
        let mut preferred_target = Target::Before(resolved_from.depth() + 1);
        targets.insert(0, preferred_target);
        // Pick a preferred target among the covering ones not outside a defining node, and add
        // targets before the nodes `from` is at the start of, up to a defining node.
        for d in (1..=resolved_from.depth()).rev() {
            let spec = resolved_from.node(d).node_type().spec();
            if spec.defining || spec.defining_as_context || spec.isolating {
                break;
            }
            if targets.contains(&Target::Covering(d)) {
                preferred_target = Target::Covering(d);
            } else if resolved_from.before(d)? + resolved_from.depth() - d + 1
                == resolved_from.pos()
            {
                targets.insert(1, Target::Before(d));
            }
        }
        let preferred_target_index = targets
            .iter()
            .position(|&target| target == preferred_target)
            .expect("the preferred target is one of the targets");

        // The nodes down the slice's start: each open one, then the innermost one's first child
        // if it has one.
        let mut left_nodes: Vec<Node<'a>> = Vec::new();
        let mut content = slice.content().clone();
        for _ in 0..slice.open_start() {
            let node = js::non_null(content.first_child(), "content")?;
            content = node.content().clone();
            left_nodes.push(node);
        }
        left_nodes.extend(content.first_child());

        // Back up the preferred depth to cover defining textblocks right above it, maybe skipping
        // one textblock that isn't defining.
        let mut preferred_depth = slice.open_start();
        let preferred_parent = resolved_from.node(preferred_target.depth() - 1);
        for d in (0..preferred_depth).rev() {
            let left_node = &left_nodes[d];
            let defines = defines_content(&left_node.node_type());
            if defines && !left_node.same_markup(preferred_parent) {
                preferred_depth = d;
            } else if defines || !left_node.node_type().is_textblock() {
                break;
            }
        }

        // Try each depth of the slice at each target, the preferred ones first.
        for j in (0..=slice.open_start()).rev() {
            let open_depth = (j + preferred_depth + 1) % (slice.open_start() + 1);
            let Some(insert) = left_nodes.get(open_depth) else {
                continue;
            };
            let insert_marks = insert.marks().to_vec();
            for i in 0..targets.len() {
                let target = targets[(i + preferred_target_index) % targets.len()];
                let depth = target.depth();
                let parent = resolved_from.node(depth - 1);
                let index = resolved_from.index(depth - 1);
                if parent.can_replace_with(
                    index,
                    index,
                    &insert.node_type(),
                    Some(&insert_marks),
                )? {
                    let closed =
                        close_fragment(slice.content(), 0, slice.open_start(), open_depth, None)?;
                    let end = match target {
                        Target::Covering(depth) => resolved_to.after(depth)?,
                        Target::Before(_) => to,
                    };
                    return self.replace(
                        resolved_from.before(depth)?,
                        end,
                        &Slice::new(closed, open_depth, slice.open_end()),
                    );
                }
            }
        }

        let start_steps = self.steps().len();
        for &target in targets.iter().rev() {
            self.replace(from, to, slice)?;
            if self.steps().len() > start_steps {
                break;
            }
            if let Target::Covering(depth) = target {
                from = resolved_from.before(depth)?;
                to = resolved_to.after(depth)?;
            }
        }
        Ok(self)
    }

    /// Replace a range with a node, moving the range out of a parent where the node doesn't fit.
    pub fn replace_range_with(
        &mut self,
        mut from: usize,
        mut to: usize,
        node: Node<'a>,
    ) -> Result<&mut Self> {
        if !node.is_inline()
            && from == to
            && self.doc().resolve(from)?.parent().content().size() > 0
            && let Some(point) = insert_point(self.doc(), from, &node.node_type())?
        {
            from = point;
            to = point;
        }
        self.replace_range(from, to, &Slice::new(Fragment::from_node(node), 0, 0))
    }

    /// Delete a range, growing it over whole parents until the deletion is valid.
    pub fn delete_range(&mut self, mut from: usize, mut to: usize) -> Result<&mut Self> {
        let mut resolved_from = self.doc().resolve(from)?;
        let mut resolved_to = self.doc().resolve(to)?;

        // When the range spans from the start of one textblock to the start of another, move out
        // of the start of both.
        if resolved_from.parent().is_textblock()
            && resolved_to.parent().is_textblock()
            && resolved_from.start(resolved_from.depth()) != resolved_to.start(resolved_to.depth())
            && resolved_from.parent_offset() == 0
            && resolved_to.parent_offset() == 0
        {
            let shared = resolved_from.shared_depth(to);
            let isolated = (shared + 1..=resolved_from.depth())
                .any(|d| resolved_from.node(d).node_type().spec().isolating)
                || (shared + 1..=resolved_to.depth())
                    .any(|d| resolved_to.node(d).node_type().spec().isolating);
            if !isolated {
                let mut d = resolved_from.depth();
                while d > 0 && from == resolved_from.start(d) {
                    from = resolved_from.before(d)?;
                    d -= 1;
                }
                let mut d = resolved_to.depth();
                while d > 0 && to == resolved_to.start(d) {
                    to = resolved_to.before(d)?;
                    d -= 1;
                }
                resolved_from = self.doc().resolve(from)?;
                resolved_to = self.doc().resolve(to)?;
            }
        }

        let covered = covered_depths(&resolved_from, &resolved_to);
        for (i, &depth) in covered.iter().enumerate() {
            let last = i == covered.len() - 1;
            if (last && depth == 0)
                || resolved_from
                    .node(depth)
                    .node_type()
                    .content_match()
                    .valid_end()
            {
                return self.delete(resolved_from.start(depth), resolved_to.end(depth));
            }
            if depth > 0
                && (last
                    || resolved_from.node(depth - 1).can_replace(
                        resolved_from.index(depth - 1),
                        resolved_to.index_after(depth - 1),
                        &Fragment::empty(),
                        0,
                        0,
                    )?)
            {
                return self.delete(resolved_from.before(depth)?, resolved_to.after(depth)?);
            }
        }
        for d in 1..=resolved_from.depth().min(resolved_to.depth()) {
            if from - resolved_from.start(d) == resolved_from.depth() - d
                && to > resolved_from.end(d)
                && resolved_to.end(d) as isize - to as isize != (resolved_to.depth() - d) as isize
                && resolved_from.start(d - 1) == resolved_to.start(d - 1)
                && resolved_from.node(d - 1).can_replace(
                    resolved_from.index(d - 1),
                    resolved_to.index(d - 1),
                    &Fragment::empty(),
                    0,
                    0,
                )?
            {
                return self.delete(resolved_from.before(d)?, to);
            }
        }
        self.delete(from, to)
    }
}

fn close_fragment<'a>(
    fragment: &Fragment<'a>,
    depth: usize,
    old_open: usize,
    new_open: usize,
    parent: Option<&Node<'a>>,
) -> Result<Fragment<'a>> {
    let mut fragment = fragment.clone();
    if depth < old_open {
        let first = js::non_null(fragment.first_child(), "copy")?;
        let closed = stack::grow(|| {
            close_fragment(first.content(), depth + 1, old_open, new_open, Some(&first))
        })?;
        fragment = fragment.replace_child(0, first.copy(closed));
    }
    if depth > new_open {
        let parent = js::defined(parent, "contentMatchAt")?;
        let matched = parent.content_match_at(0)?;
        let start =
            js::non_null(matched.fill_before(&fragment, false, 0)?, "append")?.append(&fragment);
        let end = js::non_null(matched.match_fragment(&start), "fillBefore")?.fill_before(
            &Fragment::empty(),
            true,
            0,
        )?;
        fragment = start.append(&js::non_null(end, "size")?);
    }
    Ok(fragment)
}

/// The depths at which `from` to `to` spans the whole content of the node.
fn covered_depths(from: &ResolvedPos, to: &ResolvedPos) -> Vec<usize> {
    let mut result = Vec::new();
    for d in (0..=from.depth().min(to.depth())).rev() {
        let start = from.start(d);
        if start < from.pos() - (from.depth() - d)
            || to.end(d) > to.pos() + (to.depth() - d)
            || from.node(d).node_type().spec().isolating
            || to.node(d).node_type().spec().isolating
        {
            break;
        }
        if start == to.start(d)
            || (d == from.depth()
                && d == to.depth()
                && from.parent().inline_content()
                && to.parent().inline_content()
                && d > 0
                && to.start(d - 1) == start - 1)
        {
            result.push(d);
        }
    }
    result
}
