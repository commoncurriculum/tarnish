use super::fits_trivially;
use crate::Result;
use crate::js;
use crate::js::stack;
use crate::model::{Fragment, Node, NodeType, ResolvedPos, Slice};
use crate::transform::step::Step;
use crate::transform::structure::insert_point;
use crate::transform::transform::Transform;

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
            let node = js::value::non_null(content.first_child(), "content")?;
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
        let first = js::value::non_null(fragment.first_child(), "copy")?;
        let closed = stack::grow(|| {
            close_fragment(first.content(), depth + 1, old_open, new_open, Some(&first))
        })?;
        fragment = fragment.replace_child(0, first.copy(closed));
    }
    if depth > new_open {
        let parent = js::value::defined(parent, "contentMatchAt")?;
        let matched = parent.content_match_at(0)?;
        let start = js::value::non_null(matched.fill_before(&fragment, false, 0)?, "append")?
            .append(&fragment);
        let end = js::value::non_null(matched.match_fragment(&start), "fillBefore")?.fill_before(
            &Fragment::empty(),
            true,
            0,
        )?;
        fragment = start.append(&js::value::non_null(end, "size")?);
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
