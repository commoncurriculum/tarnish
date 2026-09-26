//! Fitting a slice into a gap: `replaceStep`, and the looser `replaceRange` family.

use super::step::Step;
use super::structure::insert_point;
use super::transform::Transform;
use crate::error::{Error, Result};
use crate::model::{Attrs, ContentMatch, Fragment, Node, NodeType, ResolvedPos, Slice};
use crate::stack;

/// A step that fits `slice` in between `from` and `to`, or `None` when there's no meaningful
/// way to, or the step would change nothing.
pub fn replace_step(doc: &Node, from: usize, to: usize, slice: &Slice) -> Result<Option<Step>> {
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
/// depth it goes to, with nodes to add first or wrappers to open.
struct Fittable {
    slice_depth: usize,
    frontier_depth: usize,
    parent: Option<Node>,
    inject: Option<Fragment>,
    wrap: Option<Vec<NodeType>>,
}

struct Frontier {
    node_type: NodeType,
    matched: ContentMatch,
}

/// Places the content of a slice into the gap between two positions: `frontier` is the open
/// side of what is placed, which starts at `from`, moves forward as content is placed, and is
/// finally reconciled with `to`; `unplaced` is what is left to place, and `placed` what is,
/// open at the start as `from` is and at the end as `frontier` is.
struct Fitter {
    from: ResolvedPos,
    to: ResolvedPos,
    unplaced: Slice,
    frontier: Vec<Frontier>,
    placed: Fragment,
}

struct CloseLevel {
    depth: usize,
    fit: Fragment,
    move_to: ResolvedPos,
}

impl Fitter {
    fn new(from: ResolvedPos, to: ResolvedPos, unplaced: Slice) -> Result<Fitter> {
        let mut frontier = Vec::with_capacity(from.depth() + 1);
        for depth in 0..=from.depth() {
            let node = from.node(depth);
            frontier.push(Frontier {
                node_type: node.node_type().clone(),
                matched: node.content_match_at(from.index_after(depth))?,
            });
        }
        let mut placed = Fragment::empty();
        for depth in (1..=from.depth()).rev() {
            placed = Fragment::from_node(from.node(depth).copy(placed));
        }
        Ok(Fitter {
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

    fn fit(mut self) -> Result<Option<Step>> {
        // Place what can be placed; when nothing can, open the slice further, or drop a node.
        while self.unplaced.size() > 0 {
            match self.find_fittable()? {
                Some(fit) => self.place_nodes(fit)?,
                None => {
                    if !self.open_more() {
                        self.drop_node();
                    }
                }
            }
        }
        // When inline content comes right after the frontier and after `to`, a ReplaceAround
        // step pulls that content into the node after the frontier, so the fit reaches to
        // the end of the textblock after `to`.
        let move_inline = self.must_move_inline()?;
        let placed_size =
            self.placed.size() as isize - self.depth() as isize - self.from.depth() as isize;
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
            content = content.first_child().expect("a child").content().clone();
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
                insert: placed_size.max(0) as usize,
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
    fn find_fittable(&self) -> Result<Option<Fittable>> {
        let mut start_depth = self.unplaced.open_start();
        let mut cur = self.unplaced.content().clone();
        let mut open_end = self.unplaced.open_end();
        for d in 0..start_depth {
            let node = cur.first_child().expect("an open node").clone();
            if cur.child_count() > 1 {
                open_end = 0;
            }
            if node.node_type().spec().isolating && open_end <= d {
                start_depth = d;
                break;
            }
            cur = node.content().clone();
        }
        // Only try wrapping nodes, in pass 2, after placing them without failed.
        for pass in 1..=2 {
            let top = if pass == 1 {
                start_depth
            } else {
                self.unplaced.open_start()
            };
            for slice_depth in (0..=top).rev() {
                let (fragment, parent) = if slice_depth > 0 {
                    let parent = content_at(self.unplaced.content(), slice_depth - 1)
                        .first_child()
                        .expect("an open node")
                        .clone();
                    (parent.content().clone(), Some(parent))
                } else {
                    (self.unplaced.content().clone(), None)
                };
                let first = fragment.first_child();
                for frontier_depth in (0..=self.depth()).rev() {
                    let Frontier { node_type, matched } = &self.frontier[frontier_depth];
                    if pass == 1 {
                        // The next node fits, or there's none and the parents look compatible.
                        let (fits, inject) = match first {
                            Some(first) => match matched.match_type(first.node_type()) {
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
                                    node_type.compatible_content(parent.node_type())
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
                    } else if let Some(first) = first
                        && let Some(wrap) = matched.find_wrapping(first.node_type())
                    {
                        return Ok(Some(Fittable {
                            slice_depth,
                            frontier_depth,
                            parent,
                            inject: None,
                            wrap: Some(wrap),
                        }));
                    }
                    // Don't look further up when the parent would fit here.
                    if let Some(parent) = &parent
                        && matched.match_type(parent.node_type()).is_some()
                    {
                        break;
                    }
                }
            }
        }
        Ok(None)
    }

    fn open_more(&mut self) -> bool {
        let (content, open_start, open_end) = (
            self.unplaced.content().clone(),
            self.unplaced.open_start(),
            self.unplaced.open_end(),
        );
        let inner = content_at(&content, open_start);
        if inner.child_count() == 0 || inner.first_child().expect("a child").is_leaf() {
            return false;
        }
        let new_end = if inner.size() + open_start >= content.size().saturating_sub(open_end) {
            open_start + 1
        } else {
            0
        };
        self.unplaced = Slice::new(content, open_start + 1, open_end.max(new_end));
        true
    }

    fn drop_node(&mut self) {
        let (content, open_start, open_end) = (
            self.unplaced.content().clone(),
            self.unplaced.open_start(),
            self.unplaced.open_end(),
        );
        let inner = content_at(&content, open_start);
        if inner.child_count() <= 1 && open_start > 0 {
            let open_at_end =
                content.size().saturating_sub(open_start) <= open_start + inner.size();
            self.unplaced = Slice::new(
                drop_from_fragment(&content, open_start - 1, 1),
                open_start - 1,
                if open_at_end {
                    open_start - 1
                } else {
                    open_end
                },
            );
        } else {
            self.unplaced = Slice::new(
                drop_from_fragment(&content, open_start, 1),
                open_start,
                open_end,
            );
        }
    }

    /// Move content from the unplaced slice at `slice_depth` to the frontier node at
    /// `frontier_depth`, closing that node when it can be.
    fn place_nodes(&mut self, fit: Fittable) -> Result<()> {
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
            for node_type in wrap {
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
        let node_type = self.frontier[frontier_depth].node_type.clone();
        let mut matched = self.frontier[frontier_depth].matched.clone();
        if let Some(inject) = &inject {
            add.extend(inject.children().iter().cloned());
            matched = matched
                .match_fragment(inject, 0, inject.child_count())
                .expect("the injected nodes match");
        }
        // How many nodes are open at the end of the fragment: at 0, only its parent is;
        // below 0, none are.
        let mut open_end_count = (fragment.size() + slice_depth) as isize
            - (slice.content().size() as isize - slice.open_end() as isize);
        // Fit as many children as fit.
        while taken < fragment.child_count() {
            let next = &fragment.children()[taken];
            let Some(matches) = matched.match_type(next.node_type()) else {
                break;
            };
            taken += 1;
            // Empty open nodes are dropped.
            if taken > 1 || open_start == 0 || next.content().size() > 0 {
                matched = matches;
                let marked = next.mark(node_type.allowed_marks(next.marks()));
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
        self.placed = add_to_fragment(&self.placed, frontier_depth, &Fragment::from_array(add));
        self.frontier[frontier_depth].matched = matched;

        // When the parents' types match, and the whole node was moved and isn't open, close
        // the frontier node right away.
        if to_end
            && open_end_count < 0
            && let Some(parent) = &parent
            && *parent.node_type() == self.frontier[self.depth()].node_type
            && self.frontier.len() > 1
        {
            self.close_frontier_node()?;
        }

        // Add frontier nodes for the nodes open at the end.
        let mut cur = fragment.clone();
        for _ in 0..open_end_count.max(0) {
            let node = cur.last_child().expect("an open node").clone();
            self.frontier.push(Frontier {
                node_type: node.node_type().clone(),
                matched: node.content_match_at(node.child_count())?,
            });
            cur = node.content().clone();
        }

        // Drop what was placed from the unplaced slice: the whole node when all of it was
        // placed, the placed nodes otherwise.
        self.unplaced = if !to_end {
            Slice::new(
                drop_from_fragment(slice.content(), slice_depth, taken),
                slice.open_start(),
                slice.open_end(),
            )
        } else if slice_depth == 0 {
            Slice::empty()
        } else {
            Slice::new(
                drop_from_fragment(slice.content(), slice_depth - 1, 1),
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
        let top = &self.frontier[self.depth()];
        if !top.node_type.is_textblock()
            || content_after_fits(
                &self.to,
                self.to.depth(),
                &top.node_type,
                &top.matched,
                false,
            )?
            .is_none()
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

    fn find_close_level(&self, to: &ResolvedPos) -> Result<Option<CloseLevel>> {
        'scan: for i in (0..=self.depth().min(to.depth())).rev() {
            let Frontier { node_type, matched } = &self.frontier[i];
            let drop_inner = i < to.depth() && to.end(i + 1) == to.pos() + (to.depth() - (i + 1));
            let Some(fit) = content_after_fits(to, i, node_type, matched, drop_inner)? else {
                continue;
            };
            for d in (0..i).rev() {
                let Frontier { node_type, matched } = &self.frontier[d];
                match content_after_fits(to, d, node_type, matched, true)? {
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

    fn close(&mut self, to: ResolvedPos) -> Result<Option<ResolvedPos>> {
        let Some(close) = self.find_close_level(&to)? else {
            return Ok(None);
        };
        while self.depth() > close.depth {
            self.close_frontier_node()?;
        }
        if close.fit.child_count() > 0 {
            self.placed = add_to_fragment(&self.placed, close.depth, &close.fit);
        }
        let to = close.move_to;
        for d in close.depth + 1..=to.depth() {
            let node = to.node(d);
            let add = must(node.node_type().content_match().fill_before(
                node.content(),
                true,
                to.index(d),
            )?)?;
            self.open_frontier_node(node.node_type(), Some(node.attrs().clone()), add)?;
        }
        Ok(Some(to))
    }

    fn open_frontier_node(
        &mut self,
        node_type: &NodeType,
        attrs: Option<Attrs>,
        content: Fragment,
    ) -> Result<()> {
        let depth = self.depth();
        let top = &mut self.frontier[depth];
        // Where the node doesn't fit, which only happens while closing, ProseMirror leaves the
        // match null, and nothing reads it after.
        if let Some(matched) = top.matched.match_type(node_type) {
            top.matched = matched;
        }
        let node = node_type.create(attrs.as_deref(), content, &[])?;
        self.placed = add_to_fragment(&self.placed, depth, &Fragment::from_node(node));
        self.frontier.push(Frontier {
            node_type: node_type.clone(),
            matched: node_type.content_match(),
        });
        Ok(())
    }

    fn close_frontier_node(&mut self) -> Result<()> {
        let open = self.frontier.pop().expect("a frontier node");
        let add = open
            .matched
            .fill_before(&Fragment::empty(), true, 0)?
            .unwrap_or_else(Fragment::empty);
        if add.child_count() > 0 {
            self.placed = add_to_fragment(&self.placed, self.frontier.len(), &add);
        }
        Ok(())
    }
}

fn drop_from_fragment(fragment: &Fragment, depth: usize, count: usize) -> Fragment {
    if depth == 0 {
        return fragment.cut_by_index(count, fragment.child_count());
    }
    let first = fragment.first_child().expect("an open node");
    fragment.replace_child(
        0,
        first.copy(stack::grow(|| {
            drop_from_fragment(first.content(), depth - 1, count)
        })),
    )
}

fn add_to_fragment(fragment: &Fragment, depth: usize, content: &Fragment) -> Fragment {
    if depth == 0 {
        return fragment.append(content);
    }
    let last = fragment.last_child().expect("an open node");
    fragment.replace_child(
        fragment.child_count() - 1,
        last.copy(stack::grow(|| {
            add_to_fragment(last.content(), depth - 1, content)
        })),
    )
}

fn content_at(fragment: &Fragment, depth: usize) -> Fragment {
    let mut fragment = fragment.clone();
    for _ in 0..depth {
        fragment = fragment
            .first_child()
            .expect("an open node")
            .content()
            .clone();
    }
    fragment
}

fn close_node_start(node: &Node, open_start: isize, open_end: isize) -> Result<Node> {
    if open_start <= 0 {
        return Ok(node.clone());
    }
    let mut fragment = node.content().clone();
    if open_start > 1 {
        let first = fragment.first_child().expect("an open node");
        let inner_end = if fragment.child_count() == 1 {
            open_end - 1
        } else {
            0
        };
        fragment = fragment.replace_child(
            0,
            stack::grow(|| close_node_start(first, open_start - 1, inner_end))?,
        );
    }
    let start = node.node_type().content_match();
    fragment = must(start.fill_before(&fragment, false, 0)?)?.append(&fragment);
    if open_end <= 0 {
        let matched = must(start.match_fragment(&fragment, 0, fragment.child_count()))?;
        fragment = fragment.append(&must(matched.fill_before(&Fragment::empty(), true, 0)?)?);
    }
    Ok(node.copy(fragment))
}

/// A value ProseMirror asserts is there, an error where it isn't, as JavaScript's would be.
fn must<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(|| Error::Other("Cannot fit the content: an expected match is null".into()))
}

fn content_after_fits(
    to: &ResolvedPos,
    depth: usize,
    node_type: &NodeType,
    matched: &ContentMatch,
    open: bool,
) -> Result<Option<Fragment>> {
    let node = to.node(depth);
    let index = if open {
        to.index_after(depth)
    } else {
        to.index(depth)
    };
    if index == node.child_count() && !node_type.compatible_content(node.node_type()) {
        return Ok(None);
    }
    let fit = matched.fill_before(node.content(), true, index)?;
    Ok(fit.filter(|_| !invalid_marks(node_type, node.content(), index)))
}

fn invalid_marks(node_type: &NodeType, fragment: &Fragment, start: usize) -> bool {
    fragment.children()[start.min(fragment.child_count())..]
        .iter()
        .any(|child| !node_type.allows_marks(child.marks()))
}

fn defines_content(node_type: &NodeType) -> bool {
    let spec = node_type.spec();
    spec.defining || spec.defining_for_content
}

impl Transform {
    /// Replace a range with a slice, taking `from`, `to` and the slice's open start as hints
    /// rather than fixed points, as for a paste.
    pub fn replace_range(&mut self, from: usize, to: usize, slice: &Slice) -> Result<&mut Self> {
        if slice.size() == 0 {
            return self.delete_range(from, to);
        }
        let (mut from, mut to) = (from, to);
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

        let mut target_depths: Vec<isize> = covered_depths(&resolved_from, &resolved_to)
            .into_iter()
            .map(|depth| depth as isize)
            .collect();
        // The whole document can't be replaced.
        if target_depths.last() == Some(&0) {
            target_depths.pop();
        }
        // A negative depth -D stands for replacing from before the node at D to `to`, rather than
        // over the whole node.
        let mut preferred_target = -(resolved_from.depth() as isize + 1);
        target_depths.insert(0, preferred_target);
        // Pick a preferred target depth among the covering ones not outside a defining node, and
        // add negative depths for those `from` is at the start of, up to a defining node.
        let mut pos = resolved_from.pos() as isize - 1;
        for d in (1..=resolved_from.depth()).rev() {
            let spec = resolved_from.node(d).node_type().spec();
            if spec.defining || spec.defining_as_context || spec.isolating {
                break;
            }
            if target_depths.contains(&(d as isize)) {
                preferred_target = d as isize;
            } else if resolved_from.before(d)? as isize == pos {
                target_depths.insert(1, -(d as isize));
            }
            pos -= 1;
        }
        // Try each depth of the slice in each target depth, the preferred ones first.
        let preferred_target_index = target_depths
            .iter()
            .position(|&depth| depth == preferred_target)
            .unwrap_or(0);

        // The nodes down the slice's start, the innermost open one's first child last, which the
        // open node may not have.
        let mut left_nodes: Vec<Option<Node>> = Vec::new();
        let mut preferred_depth = slice.open_start();
        let mut content = slice.content().clone();
        for i in 0.. {
            let node = content.first_child().cloned();
            left_nodes.push(node.clone());
            if i == slice.open_start() {
                break;
            }
            content = must(node)?.content().clone();
        }

        // Back up the preferred depth to cover defining textblocks right above it, maybe skipping
        // one textblock that isn't defining.
        let preferred_parent =
            resolved_from.node((preferred_target.unsigned_abs()).saturating_sub(1));
        for d in (0..preferred_depth).rev() {
            let left_node = must(left_nodes[d].as_ref())?;
            let defines = defines_content(left_node.node_type());
            if defines && !left_node.same_markup(preferred_parent) {
                preferred_depth = d;
            } else if defines || !left_node.node_type().is_textblock() {
                break;
            }
        }

        for j in (0..=slice.open_start()).rev() {
            let open_depth = (j + preferred_depth + 1) % (slice.open_start() + 1);
            let Some(Some(insert)) = left_nodes.get(open_depth) else {
                continue;
            };
            for i in 0..target_depths.len() {
                let mut target_depth =
                    target_depths[(i + preferred_target_index) % target_depths.len()];
                let mut expand = true;
                if target_depth < 0 {
                    expand = false;
                    target_depth = -target_depth;
                }
                let target_depth = target_depth as usize;
                let parent = resolved_from.node(target_depth - 1);
                let index = resolved_from.index(target_depth - 1);
                if parent.can_replace_with(
                    index,
                    index,
                    insert.node_type(),
                    Some(insert.marks()),
                )? {
                    let closed =
                        close_fragment(slice.content(), 0, slice.open_start(), open_depth, None)?;
                    let end = if expand {
                        resolved_to.after(target_depth)?
                    } else {
                        to
                    };
                    return self.replace(
                        resolved_from.before(target_depth)?,
                        end,
                        &Slice::new(closed, open_depth, slice.open_end()),
                    );
                }
            }
        }

        let start_steps = self.steps().len();
        for i in (0..target_depths.len()).rev() {
            self.replace(from, to, slice)?;
            if self.steps().len() > start_steps {
                break;
            }
            let depth = target_depths[i];
            if depth < 0 {
                continue;
            }
            from = resolved_from.before(depth as usize)?;
            to = resolved_to.after(depth as usize)?;
        }
        Ok(self)
    }

    /// Replace a range with a node, moving the range out of a parent where the node doesn't fit.
    pub fn replace_range_with(&mut self, from: usize, to: usize, node: Node) -> Result<&mut Self> {
        let (mut from, mut to) = (from, to);
        if !node.is_inline()
            && from == to
            && self.doc().resolve(from)?.parent().content().size() > 0
            && let Some(point) = insert_point(self.doc(), from, node.node_type())?
        {
            from = point;
            to = point;
        }
        self.replace_range(from, to, &Slice::new(Fragment::from_node(node), 0, 0))
    }

    /// Delete a range, growing it over whole parents until the deletion is valid.
    pub fn delete_range(&mut self, from: usize, to: usize) -> Result<&mut Self> {
        let (mut from, mut to) = (from, to);
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

fn close_fragment(
    fragment: &Fragment,
    depth: usize,
    old_open: usize,
    new_open: usize,
    parent: Option<&Node>,
) -> Result<Fragment> {
    let mut fragment = fragment.clone();
    if depth < old_open {
        let first = fragment.first_child().expect("an open node").clone();
        let closed = stack::grow(|| {
            close_fragment(first.content(), depth + 1, old_open, new_open, Some(&first))
        })?;
        fragment = fragment.replace_child(0, first.copy(closed));
    }
    if depth > new_open {
        let matched = must(parent)?.content_match_at(0)?;
        let start = must(matched.fill_before(&fragment, false, 0)?)?.append(&fragment);
        let end = must(
            must(matched.match_fragment(&start, 0, start.child_count()))?.fill_before(
                &Fragment::empty(),
                true,
                0,
            )?,
        )?;
        fragment = start.append(&end);
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
