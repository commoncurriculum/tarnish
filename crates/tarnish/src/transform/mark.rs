//! Adding and removing marks, and clearing what a new parent type doesn't allow.

use super::step::{MarkOp, Step};
use super::transform::Transform;
use crate::error::{Error, Result};
use crate::model::{ContentMatch, Fragment, Mark, MarkType, NodeType, Slice, Whitespace};

/// A mark, or all marks of a type.
#[derive(Clone, Copy)]
pub enum MarkMatch<'a> {
    Mark(&'a Mark),
    Type(&'a MarkType),
}

impl Transform {
    pub fn add_mark(&mut self, from: usize, to: usize, mark: &Mark) -> Result<&mut Self> {
        // Ranges the steps cover, extended while the next node continues them.
        let mut removed: Vec<(usize, usize, Mark)> = Vec::new();
        let mut added: Vec<(usize, usize)> = Vec::new();
        let mut removing: Option<usize> = None;
        self.doc().clone().nodes_between(
            from,
            to,
            &mut |node, pos, parent, _| {
                if !node.is_inline() {
                    return Ok(true);
                }
                let marks = node.marks();
                let parent = parent.expect("an inline node's parent");
                if !mark.is_in_set(marks) && parent.node_type().allows_mark_type(mark.mark_type()) {
                    let start = pos.max(from);
                    let end = (pos + node.node_size()).min(to);
                    let new_set = mark.add_to_set(marks);
                    for old in marks.iter() {
                        if !old.is_in_set(&new_set) {
                            match removing {
                                Some(index)
                                    if removed[index].1 == start && removed[index].2 == *old =>
                                {
                                    removed[index].1 = end;
                                }
                                _ => {
                                    removed.push((start, end, old.clone()));
                                    removing = Some(removed.len() - 1);
                                }
                            }
                        }
                    }
                    match added.last_mut() {
                        Some(adding) if adding.1 == start => adding.1 = end,
                        _ => added.push((start, end)),
                    }
                }
                Ok(true)
            },
            0,
        )?;
        for (from, to, mark) in removed {
            self.step(Step::Mark {
                op: MarkOp::Remove,
                from,
                to,
                mark,
            })?;
        }
        for (from, to) in added {
            self.step(Step::Mark {
                op: MarkOp::Add,
                from,
                to,
                mark: mark.clone(),
            })?;
        }
        Ok(self)
    }

    /// Remove the mark, the marks of the type, or with `None` all marks, from the inline
    /// content between `from` and `to`.
    pub fn remove_mark(
        &mut self,
        from: usize,
        to: usize,
        mark: Option<MarkMatch>,
    ) -> Result<&mut Self> {
        struct Matched {
            style: Mark,
            from: usize,
            to: usize,
            step: usize,
        }
        let mut matched: Vec<Matched> = Vec::new();
        let mut step = 0;
        self.doc().clone().nodes_between(
            from,
            to,
            &mut |node, pos, _, _| {
                if !node.is_inline() {
                    return Ok(true);
                }
                step += 1;
                let to_remove: Vec<Mark> = match mark {
                    Some(MarkMatch::Type(mark_type)) => {
                        let mut set = node.marks().clone();
                        let mut found = Vec::new();
                        while let Some(mark) = mark_type.is_in_set(&set).cloned() {
                            set = mark.remove_from_set(&set);
                            found.push(mark);
                        }
                        found
                    }
                    Some(MarkMatch::Mark(mark)) if mark.is_in_set(node.marks()) => {
                        vec![mark.clone()]
                    }
                    Some(MarkMatch::Mark(_)) => Vec::new(),
                    None => node.marks().to_vec(),
                };
                let end = (pos + node.node_size()).min(to);
                for style in to_remove {
                    let found = matched
                        .iter_mut()
                        .rev()
                        .find(|m| m.step == step - 1 && m.style == style);
                    match found {
                        Some(found) => {
                            found.to = end;
                            found.step = step;
                        }
                        None => matched.push(Matched {
                            style,
                            from: pos.max(from),
                            to: end,
                            step,
                        }),
                    }
                }
                Ok(true)
            },
            0,
        )?;
        for m in matched {
            self.step(Step::Mark {
                op: MarkOp::Remove,
                from: m.from,
                to: m.to,
                mark: m.style,
            })?;
        }
        Ok(self)
    }

    /// Remove the content and marks of the node at `pos` that a node of `parent_type` wouldn't
    /// allow, matching from `start` when given.
    pub fn clear_incompatible(
        &mut self,
        pos: usize,
        parent_type: &NodeType,
        start: Option<ContentMatch>,
    ) -> Result<&mut Self> {
        clear_incompatible(self, pos, parent_type, start, true)?;
        Ok(self)
    }
}

/// The ends of the line breaks in a text: `\r\n`, `\r` or `\n`, as start and length.
pub(crate) fn line_breaks(units: &[u16]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < units.len() {
        match units[index] {
            0x0d if units.get(index + 1) == Some(&0x0a) => {
                found.push((index, 2));
                index += 2;
                continue;
            }
            0x0d | 0x0a => found.push((index, 1)),
            _ => {}
        }
        index += 1;
    }
    found
}

/// [`Transform::clear_incompatible`], which with `clear_newlines` false keeps the newlines in
/// text rather than making them spaces.
pub(super) fn clear_incompatible(
    tr: &mut Transform,
    pos: usize,
    parent_type: &NodeType,
    start: Option<ContentMatch>,
    clear_newlines: bool,
) -> Result<()> {
    let node = tr.doc().node_at(pos)?.cloned().ok_or_else(|| {
        Error::Other(format!(
            "No node at {pos} to clear incompatible content from"
        ))
    })?;
    let mut matched = start.unwrap_or_else(|| parent_type.content_match());
    let mut replace_steps = Vec::new();
    let mut cur = pos + 1;
    for child in node.children() {
        let end = cur + child.node_size();
        match matched.match_type(child.node_type()) {
            None => replace_steps.push(Step::Replace {
                from: cur,
                to: end,
                slice: Slice::empty(),
                structure: false,
            }),
            Some(allowed) => {
                matched = allowed;
                for mark in child.marks().iter() {
                    if !parent_type.allows_mark_type(mark.mark_type()) {
                        tr.step(Step::Mark {
                            op: MarkOp::Remove,
                            from: cur,
                            to: end,
                            mark: mark.clone(),
                        })?;
                    }
                }
                if clear_newlines
                    && let Some(text) = child.text()
                    && parent_type.whitespace() != Whitespace::Pre
                {
                    let mut slice = None;
                    for (index, length) in line_breaks(&text.units()) {
                        if slice.is_none() {
                            let marks = parent_type.allowed_marks(child.marks());
                            let space = parent_type.schema().text(" ", &marks)?;
                            slice = Some(Slice::new(Fragment::from_node(space), 0, 0));
                        }
                        replace_steps.push(Step::Replace {
                            from: cur + index,
                            to: cur + index + length,
                            slice: slice.clone().expect("a slice"),
                            structure: false,
                        });
                    }
                }
            }
        }
        cur = end;
    }
    if !matched.valid_end() {
        let fill = matched
            .fill_before(&Fragment::empty(), true, 0)?
            .unwrap_or_else(Fragment::empty);
        tr.replace(cur, cur, &Slice::new(fill, 0, 0))?;
    }
    for step in replace_steps.into_iter().rev() {
        tr.step(step)?;
    }
    Ok(())
}
