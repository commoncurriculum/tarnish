//! Where two fragments start and stop differing.

use super::fragment::Fragment;

pub(crate) fn find_diff_start(a: &Fragment, b: &Fragment, mut pos: usize) -> Option<usize> {
    for index in 0.. {
        if index == a.child_count() || index == b.child_count() {
            return (a.child_count() != b.child_count()).then_some(pos);
        }
        let (child_a, child_b) = (&a.children()[index], &b.children()[index]);
        if child_a.ptr_eq(child_b) {
            pos += child_a.node_size();
            continue;
        }
        if !child_a.same_markup(child_b) {
            return Some(pos);
        }
        if let (Some(text_a), Some(text_b)) = (child_a.text(), child_b.text())
            && text_a != text_b
        {
            let (units_a, units_b) = (text_a.units(), text_b.units());
            let same = units_a
                .iter()
                .zip(units_b.iter())
                .take_while(|(a, b)| a == b)
                .count();
            pos += same;
            // Back off a split surrogate pair.
            if same > 0
                && same < units_a.len()
                && same < units_b.len()
                && is_high_surrogate(units_a[same - 1])
                && is_low_surrogate(units_a[same])
            {
                pos -= 1;
            }
            return Some(pos);
        }
        if (child_a.content().size() > 0 || child_b.content().size() > 0)
            && let Some(inner) = find_diff_start(child_a.content(), child_b.content(), pos + 1)
        {
            return Some(inner);
        }
        pos += child_a.node_size();
    }
    unreachable!()
}

pub(crate) fn find_diff_end(
    a: &Fragment,
    b: &Fragment,
    mut pos_a: usize,
    mut pos_b: usize,
) -> Option<(usize, usize)> {
    let (mut index_a, mut index_b) = (a.child_count(), b.child_count());
    loop {
        if index_a == 0 || index_b == 0 {
            return (index_a != index_b).then_some((pos_a, pos_b));
        }
        index_a -= 1;
        index_b -= 1;
        let (child_a, child_b) = (&a.children()[index_a], &b.children()[index_b]);
        let size = child_a.node_size();
        if child_a.ptr_eq(child_b) {
            pos_a -= size;
            pos_b -= size;
            continue;
        }
        if !child_a.same_markup(child_b) {
            return Some((pos_a, pos_b));
        }
        if let (Some(text_a), Some(text_b)) = (child_a.text(), child_b.text())
            && text_a != text_b
        {
            let (units_a, units_b) = (text_a.units(), text_b.units());
            let same = units_a
                .iter()
                .rev()
                .zip(units_b.iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            pos_a -= same;
            pos_b -= same;
            let (end_a, end_b) = (units_a.len() - same, units_b.len() - same);
            // Move forward off a split surrogate pair.
            if end_a > 0
                && end_b > 0
                && end_a < units_a.len()
                && is_high_surrogate(units_a[end_a - 1])
                && is_low_surrogate(units_a[end_a])
            {
                pos_a += 1;
                pos_b += 1;
            }
            return Some((pos_a, pos_b));
        }
        if (child_a.content().size() > 0 || child_b.content().size() > 0)
            && let Some(inner) =
                find_diff_end(child_a.content(), child_b.content(), pos_a - 1, pos_b - 1)
        {
            return Some(inner);
        }
        pos_a -= size;
        pos_b -= size;
    }
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..0xE000).contains(&unit)
}

fn is_high_surrogate(unit: u16) -> bool {
    (0xD800..0xDC00).contains(&unit)
}
