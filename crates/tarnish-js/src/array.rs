//! `Array.prototype.sort` as JavaScriptCore runs it, `runtime/StableSort.h` with galloping
//! merges. Which pairs it compares, and in what order, shows when a comparator throws for some
//! pairs: the output is whether it threw.
//!
//! JavaScriptCore sometimes sorts a small array another way, comparing every pair, for a stretch
//! of calls after it has compiled the caller; the order comes out the same.

use super::JsError;

/// Arrays shorter than this are sorted by binary insertion alone, and runs no longer than this
/// are extended by it.
const EXTEND_RUN_CUTOFF: usize = 8;
/// How far binary insertion extends a short run.
const FORCE_RUN_LENGTH: usize = 64;
/// How many times in a row one run wins a merge before the merge gallops.
const MIN_GALLOP_THRESHOLD: usize = 7;

/// `items.sort(compare)`, where `compare` returns the comparator's number.
pub fn sort<T: Copy>(
    items: &mut [T],
    mut compare: impl FnMut(T, T) -> Result<f64, JsError>,
) -> Result<(), JsError> {
    let less = &mut |a: T, b: T| compare(a, b).map(|order| order < 0.0);
    let n = items.len();
    if n < EXTEND_RUN_CUTOFF {
        return insertion_sort(items, 0, less);
    }
    let mut min_gallop = MIN_GALLOP_THRESHOLD;
    // Each run as its first and last index, below the run being built, with the power of the
    // boundary after it.
    let mut stack: Vec<((usize, usize), u32)> = Vec::new();
    let mut run1 = (0, next_run(items, 0, less)?);
    while run1.1 + 1 < n {
        let run2 = (run1.1 + 1, next_run(items, run1.1 + 1, less)?);
        let p = power(run1.0, run2.0, run2.1, n);
        while let Some(&(run, _)) = stack.last().filter(|(_, power)| *power > p) {
            stack.pop();
            merge(
                items,
                run.0,
                run.1 + 1,
                run1.0,
                run1.1 + 1,
                less,
                &mut min_gallop,
            )?;
            run1.0 = run.0;
        }
        stack.push((run1, p));
        run1 = run2;
    }
    while let Some((run, _)) = stack.pop() {
        merge(
            items,
            run.0,
            run.1 + 1,
            run1.0,
            run1.1 + 1,
            less,
            &mut min_gallop,
        )?;
        run1.0 = run.0;
    }
    Ok(())
}

/// The last index of the run starting at `begin`: a natural run, extended by insertion if short,
/// then by whatever ascends after it.
fn next_run<T: Copy>(
    items: &mut [T],
    begin: usize,
    less: &mut impl FnMut(T, T) -> Result<bool, JsError>,
) -> Result<usize, JsError> {
    let n = items.len();
    let mut end = extend_and_normalize_run(items, begin, less)?;
    if end - begin < EXTEND_RUN_CUTOFF {
        let size = FORCE_RUN_LENGTH.min(n - begin);
        insertion_sort(&mut items[begin..begin + size], end - begin, less)?;
        end = begin + size - 1;
    }
    while end + 1 < n && !less(items[end + 1], items[end])? {
        end += 1;
    }
    Ok(end)
}

/// Sorts `span`, whose items up to `sorted_header` are sorted, inserting each later item after
/// the items it isn't less than.
fn insertion_sort<T: Copy>(
    span: &mut [T],
    sorted_header: usize,
    less: &mut impl FnMut(T, T) -> Result<bool, JsError>,
) -> Result<(), JsError> {
    for i in sorted_header + 1..span.len() {
        let value = span[i];
        let (mut left, mut right) = (0, i);
        while left < right {
            let middle = left + (right - left) / 2;
            if less(value, span[middle])? {
                right = middle;
            } else {
                left = middle + 1;
            }
        }
        span[left..=i].rotate_right(1);
    }
    Ok(())
}

/// The last index of the ascending or strictly descending run starting at `begin`, reversing a
/// descending one.
fn extend_and_normalize_run<T: Copy>(
    items: &mut [T],
    begin: usize,
    less: &mut impl FnMut(T, T) -> Result<bool, JsError>,
) -> Result<usize, JsError> {
    let n = items.len();
    let mut end = begin;
    if end + 1 >= n {
        return Ok(end);
    }
    let descending = less(items[end + 1], items[end])?;
    end += 1;
    while end + 1 < n && less(items[end + 1], items[end])? == descending {
        end += 1;
    }
    if descending {
        items[begin..=end].reverse();
    }
    Ok(end)
}

/// The depth in the merge tree of the boundary between `[left, middle)` and `[middle, right]`.
fn power(left: usize, middle: usize, right: usize, n: usize) -> u32 {
    let (n1, n2) = ((middle - left) as u128, (right - middle + 1) as u128);
    let a = (left as u128 * 2 + n1) << 62;
    let b = (middle as u128 * 2 + n2) << 62;
    let n = n as u128;
    (((a / n) ^ (b / n)) as u64).leading_zeros()
}

/// Where `key` goes in `base`, before the items equal to it, searching out from `hint`.
fn gallop_left<T: Copy>(
    key: T,
    base: &[T],
    hint: usize,
    less: &mut impl FnMut(T, T) -> Result<bool, JsError>,
) -> Result<usize, JsError> {
    let (mut last, mut offset) = (0, 1);
    // The place is in `last..=offset` once the gallop narrows it.
    if less(base[hint], key)? {
        let max = base.len() - hint;
        while offset < max && less(base[hint + offset], key)? {
            last = offset;
            offset = (offset * 2 + 1).min(max);
        }
        (last, offset) = (hint + last + 1, hint + offset);
    } else {
        let max = hint + 1;
        while offset < max && !less(base[hint - offset], key)? {
            last = offset;
            offset = (offset * 2 + 1).min(max);
        }
        (last, offset) = (hint + 1 - offset, hint - last);
    }
    while last < offset {
        let middle = last + (offset - last) / 2;
        if less(base[middle], key)? {
            last = middle + 1;
        } else {
            offset = middle;
        }
    }
    Ok(offset)
}

/// Where `key` goes in `base`, after the items equal to it, searching out from `hint`.
fn gallop_right<T: Copy>(
    key: T,
    base: &[T],
    hint: usize,
    less: &mut impl FnMut(T, T) -> Result<bool, JsError>,
) -> Result<usize, JsError> {
    let (mut last, mut offset) = (0, 1);
    // The place is in `last..=offset` once the gallop narrows it.
    if less(key, base[hint])? {
        let max = hint + 1;
        while offset < max && less(key, base[hint - offset])? {
            last = offset;
            offset = (offset * 2 + 1).min(max);
        }
        (last, offset) = (hint + 1 - offset, hint - last);
    } else {
        let max = base.len() - hint;
        while offset < max && !less(key, base[hint + offset])? {
            last = offset;
            offset = (offset * 2 + 1).min(max);
        }
        (last, offset) = (hint + last + 1, hint + offset);
    }
    while last < offset {
        let middle = last + (offset - last) / 2;
        if less(key, base[middle])? {
            offset = middle;
        } else {
            last = middle + 1;
        }
    }
    Ok(offset)
}

/// `mergePowersortRuns`: merges the adjacent runs `[start1, end1)` and `[start2, end2)`, first
/// leaving out what is already in place at either end, then going item by item until one run
/// wins `min_gallop` times in a row, then galloping while that pays.
fn merge<T: Copy>(
    items: &mut [T],
    start1: usize,
    end1: usize,
    start2: usize,
    end2: usize,
    less: &mut impl FnMut(T, T) -> Result<bool, JsError>,
    min_gallop: &mut usize,
) -> Result<(), JsError> {
    let src = items[start1..end2].to_vec();
    let at = |index: usize| src[index - start1];
    let run = |from: usize, to: usize| &src[from - start1..to - start1];
    if start1 == end1 || start2 == end2 {
        return Ok(());
    }
    let mut left = start1 + gallop_right(at(start2), run(start1, end1), 0, less)?;
    if left == end1 {
        return Ok(());
    }
    let right_end = start2 + gallop_left(at(end1 - 1), run(start2, end2), end2 - start2 - 1, less)?;
    if right_end == start2 {
        return Ok(());
    }
    let mut right = start2;
    let mut out = left;
    'merge: loop {
        let (mut left_wins, mut right_wins) = (0, 0);
        while left_wins < *min_gallop && right_wins < *min_gallop {
            if right >= right_end || left >= end1 {
                break 'merge;
            }
            if less(at(right), at(left))? {
                items[out] = at(right);
                right += 1;
                right_wins += 1;
                left_wins = 0;
            } else {
                items[out] = at(left);
                left += 1;
                left_wins += 1;
                right_wins = 0;
            }
            out += 1;
        }
        *min_gallop += 1;
        loop {
            if *min_gallop > 1 {
                *min_gallop -= 1;
            }
            if left >= end1 || right >= right_end {
                break 'merge;
            }
            left_wins = gallop_right(at(right), run(left, end1), 0, less)?;
            items[out..out + left_wins].copy_from_slice(run(left, left + left_wins));
            out += left_wins;
            left += left_wins;
            items[out] = at(right);
            out += 1;
            right += 1;
            if left >= end1 || right >= right_end {
                break 'merge;
            }
            right_wins = gallop_left(at(left), run(right, right_end), 0, less)?;
            items[out..out + right_wins].copy_from_slice(run(right, right + right_wins));
            out += right_wins;
            right += right_wins;
            items[out] = at(left);
            out += 1;
            left += 1;
            if left >= end1 || right >= right_end {
                break 'merge;
            }
            if left_wins < MIN_GALLOP_THRESHOLD && right_wins < MIN_GALLOP_THRESHOLD {
                break;
            }
        }
        *min_gallop += 1;
    }
    let rest = run(left, end1).len();
    items[out..out + rest].copy_from_slice(run(left, end1));
    out += rest;
    items[out..out + (right_end - right)].copy_from_slice(run(right, right_end));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The comparisons sorting `items` with `(a, b) => a - b` makes, each as the indexes the
    /// two items had before sorting.
    fn comparisons(items: &[i64]) -> Vec<String> {
        let mut indexed: Vec<(usize, i64)> = items.iter().copied().enumerate().collect();
        let mut calls = Vec::new();
        sort(&mut indexed, |(i, a), (j, b)| {
            calls.push(format!("{i}:{j}"));
            Ok((a - b) as f64)
        })
        .unwrap();
        let mut sorted = items.to_vec();
        sorted.sort();
        assert_eq!(
            indexed.iter().map(|(_, item)| *item).collect::<Vec<_>>(),
            sorted
        );
        calls
    }

    #[test]
    fn sorts_strings_as_bun_collates_them() {
        let mut words = [
            "b", "a", "B", "A", "_", "-", "1", "é", "e", "ä", "z", "Z", "aa", "a b", "a-b",
        ];
        sort(&mut words, |a, b| Ok(crate::locale_compare(a, b))).unwrap();
        // `[...].sort((x, y) => x.localeCompare(y))` in Bun.
        assert_eq!(words.join(" "), "_ - 1 a A ä a b a-b aa b B e é z Z");
    }

    // Each recorded from Bun 1.4.0.
    #[test]
    fn compares_as_javascriptcore_does() {
        assert_eq!(comparisons(&[0, 3, 2, 1]).join(" "), "1:0 2:1 2:0 3:2 3:0");
        assert_eq!(
            comparisons(&[7, 6, 5, 4, 3, 2, 1, 0]).join(" "),
            "1:0 2:1 3:2 4:3 5:4 6:5 7:6"
        );
        assert_eq!(
            comparisons(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 50, 20, 30, 15, 12]).join(" "),
            "1:0 2:1 3:2 4:3 5:4 6:5 7:6 8:7 9:8 10:9 11:10 11:10 12:11 13:12 13:12 13:11 \
             14:11 14:13 14:0 14:1 14:3 14:7 14:9 14:10 12:10 14:10 13:10 11:10 12:10"
        );
        let runs: Vec<i64> = (0..46)
            .map(|i| match i {
                12 => 30,
                28 => 42,
                _ => i,
            })
            .collect();
        assert_eq!(
            comparisons(&runs).join(" "),
            "1:0 2:1 3:2 4:3 5:4 6:5 7:6 8:7 9:8 10:9 11:10 12:11 13:12 13:12 14:13 15:14 16:15 \
             17:16 18:17 19:18 20:19 21:20 22:21 23:22 24:23 25:24 26:25 27:26 28:27 29:28 \
             29:28 30:29 31:30 32:31 33:32 34:33 35:34 36:35 37:36 38:37 39:38 40:39 41:40 \
             42:41 43:42 44:43 45:44 13:0 13:1 13:3 13:7 13:10 13:12 13:11 28:12 27:12 13:12 \
             14:12 15:12 16:12 17:12 18:12 19:12 20:12 21:12 22:12 24:12 26:12 27:12 29:0 29:1 \
             29:3 29:7 29:16 29:23 29:27 29:28 29:12 45:28 44:28 42:28 38:28 40:28 41:28 29:12 \
             30:12 30:28 31:28 32:28 33:28 34:28 35:28 36:28 37:28 38:28 39:28 41:28"
        );
        let gallops: Vec<i64> = (0..10).map(|i| i * 10).chain(0..30).collect();
        assert_eq!(
            comparisons(&gallops).join(" "),
            "1:0 2:1 3:2 4:3 5:4 6:5 7:6 8:7 9:8 10:9 10:9 11:10 12:11 13:12 14:13 15:14 16:15 \
             17:16 18:17 19:18 20:19 21:20 22:21 23:22 24:23 25:24 26:25 27:26 28:27 29:28 \
             30:29 31:30 32:31 33:32 34:33 35:34 36:35 37:36 38:37 39:38 10:0 10:1 39:9 10:1 \
             11:1 12:1 13:1 14:1 15:1 16:1 17:1 18:1 19:1 21:1 20:1 20:2 21:2 22:2 23:2 24:2 \
             25:2 26:2 27:2 28:2 29:2 30:2 30:3 31:3 32:3 33:3 34:3 35:3 36:3 37:3 38:3 39:3"
        );
        let shuffled: Vec<i64> = (0..150).map(|i| i * 37 % 150).collect();
        let calls = comparisons(&shuffled);
        assert_eq!(calls.len(), 906);
        assert_eq!(
            calls[380..400].join(" "),
            "84:80 84:67 85:75 85:72 85:81 85:64 86:71 86:74 86:65 86:78 86:82 87:75 87:78 \
             87:66 87:79 87:83 88:75 88:72 88:84 88:67"
        );
        assert_eq!(
            calls[calls.len() - 12..].join(" "),
            "101:28 101:24 97:24 97:20 93:20 93:16 89:16 89:12 85:12 85:8 81:8 81:4"
        );
    }
}
