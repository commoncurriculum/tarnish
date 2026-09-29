//! Where two fragments start and stop differing.

use super::fragment::Fragment;
use super::view::{NodeRef, TextRef};
use crate::js::stack;

impl Fragment<'_> {
    /// The first position at which this fragment and `other` differ, counting from `pos`, or
    /// `None` when they are the same.
    pub fn find_diff_start(&self, other: &Fragment, pos: usize) -> Option<usize> {
        diff_start(
            self.refs(),
            other.refs(),
            self.child_count() != other.child_count(),
            pos,
        )
    }

    /// The first position, searching from the ends, at which this fragment and `other` differ,
    /// as a position in each, or `None` when they are the same.
    pub fn find_diff_end(
        &self,
        other: &Fragment,
        pos_a: usize,
        pos_b: usize,
    ) -> Option<(usize, usize)> {
        diff_end(
            self.refs().rev(),
            other.refs().rev(),
            self.child_count() != other.child_count(),
            pos_a,
            pos_b,
        )
    }
}

fn diff_start<'x, 'y>(
    a: impl Iterator<Item = NodeRef<'x>>,
    b: impl Iterator<Item = NodeRef<'y>>,
    lengths_differ: bool,
    mut pos: usize,
) -> Option<usize> {
    for (a, b) in a.zip(b) {
        if a.ptr_eq(b) {
            pos += a.node_size();
            continue;
        }
        if !a.same_markup(b) {
            return Some(pos);
        }
        if let (Some(text_a), Some(text_b)) = (a.text(), b.text())
            && !text_a.same(text_b)
        {
            return Some(pos + common_prefix(text_a, text_b));
        }
        if (a.content_size() > 0 || b.content_size() > 0)
            && let Some(inner) = stack::grow(|| {
                diff_start(
                    a.children(),
                    b.children(),
                    a.child_count() != b.child_count(),
                    pos + 1,
                )
            })
        {
            return Some(inner);
        }
        pos += a.node_size();
    }
    lengths_differ.then_some(pos)
}

fn diff_end<'x, 'y>(
    a: impl Iterator<Item = NodeRef<'x>>,
    b: impl Iterator<Item = NodeRef<'y>>,
    lengths_differ: bool,
    mut pos_a: usize,
    mut pos_b: usize,
) -> Option<(usize, usize)> {
    for (a, b) in a.zip(b) {
        let size = a.node_size();
        if a.ptr_eq(b) {
            pos_a -= size;
            pos_b -= size;
            continue;
        }
        if !a.same_markup(b) {
            return Some((pos_a, pos_b));
        }
        if let (Some(text_a), Some(text_b)) = (a.text(), b.text())
            && !text_a.same(text_b)
        {
            let same = common_suffix(text_a, text_b);
            return Some((pos_a - same, pos_b - same));
        }
        if (a.content_size() > 0 || b.content_size() > 0)
            && let Some(inner) = stack::grow(|| {
                diff_end(
                    a.children().rev(),
                    b.children().rev(),
                    a.child_count() != b.child_count(),
                    pos_a - 1,
                    pos_b - 1,
                )
            })
        {
            return Some(inner);
        }
        pos_a -= size;
        pos_b -= size;
    }
    lengths_differ.then_some((pos_a, pos_b))
}

/// The UTF-16 units the texts start with alike, short of a surrogate pair they split.
fn common_prefix(a: TextRef, b: TextRef) -> usize {
    if let (Some(a), Some(b)) = (a.as_str(), b.as_str()) {
        // Texts without lone surrogates first differ at a whole character.
        return a
            .chars()
            .zip(b.chars())
            .take_while(|(a, b)| a == b)
            .map(|(c, _)| c.len_utf16())
            .sum();
    }
    let (a, b) = (a.units(), b.units());
    let same = a.iter().zip(b.iter()).take_while(|(a, b)| a == b).count();
    let splits_pair = same > 0
        && same < a.len()
        && same < b.len()
        && is_high_surrogate(a[same - 1])
        && is_low_surrogate(a[same]);
    same - usize::from(splits_pair)
}

/// The UTF-16 units the texts end with alike, short of a surrogate pair they split.
fn common_suffix(a: TextRef, b: TextRef) -> usize {
    if let (Some(a), Some(b)) = (a.as_str(), b.as_str()) {
        return a
            .chars()
            .rev()
            .zip(b.chars().rev())
            .take_while(|(a, b)| a == b)
            .map(|(c, _)| c.len_utf16())
            .sum();
    }
    let (a, b) = (a.units(), b.units());
    let same = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (end_a, end_b) = (a.len() - same, b.len() - same);
    let splits_pair = same > 0
        && end_a > 0
        && end_b > 0
        && is_high_surrogate(a[end_a - 1])
        && is_low_surrogate(a[end_a]);
    same - usize::from(splits_pair)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..0xE000).contains(&unit)
}

fn is_high_surrogate(unit: u16) -> bool {
    (0xD800..0xDC00).contains(&unit)
}
