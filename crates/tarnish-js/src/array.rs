//! `Array.prototype.sort` as V8 runs it: TimSort, as `third_party/v8/builtins/array-sort.tq`
//! has it. Which pairs it compares, and in what order, shows when a comparator throws for some
//! pairs: the output is whether it threw.

use crate::Result;

/// How many times in a row one run wins a merge before the merge gallops.
const MIN_GALLOP_WINS: usize = 7;

/// `items.sort(compare)`, where `compare` returns the comparator's number. V8 sorts a copy of
/// the items and writes it back once sorted, so a comparator that throws leaves them as they
/// were.
pub fn sort<T: Copy>(items: &mut [T], compare: impl FnMut(T, T) -> Result<f64>) -> Result<()> {
    let mut state = State {
        work: items.to_vec(),
        temp: Vec::new(),
        runs: Vec::new(),
        min_gallop: MIN_GALLOP_WINS,
        compare,
    };
    state.sort()?;
    items.copy_from_slice(&state.work);
    Ok(())
}

/// Natural runs shorter than this are extended by binary insertion: `n` below 64, and otherwise
/// a length from 32 to 64 that `n` divided by it is just under a power of two.
fn min_run_length(mut n: usize) -> usize {
    let mut shifted_off = 0;
    while n >= 64 {
        shifted_off |= n & 1;
        n >>= 1;
    }
    n + shifted_off
}

/// Which array a gallop searches.
#[derive(Clone, Copy)]
enum Of {
    Work,
    Temp,
}

/// Where a merge ends.
enum End {
    /// What is left of the run in the temporary array goes where the merge stopped.
    Succeed,
    /// One item of the run in the temporary array is left, and it goes past the other run.
    CopyOne,
}

struct State<T, F> {
    work: Vec<T>,
    temp: Vec<T>,
    /// The runs waiting to be merged, each as its start and length.
    runs: Vec<(usize, usize)>,
    min_gallop: usize,
    compare: F,
}

impl<T: Copy, F: FnMut(T, T) -> Result<f64>> State<T, F> {
    /// Whether the comparator orders `a` before `b`. A `NaN` is `0`.
    fn less(&mut self, a: T, b: T) -> Result<bool> {
        Ok((self.compare)(a, b)? < 0.0)
    }

    fn at(&self, of: Of, index: usize) -> T {
        match of {
            Of::Work => self.work[index],
            Of::Temp => self.temp[index],
        }
    }

    fn sort(&mut self) -> Result<()> {
        let length = self.work.len();
        if length < 2 {
            return Ok(());
        }
        let min_run = min_run_length(length);
        let (mut low, mut remaining) = (0, length);
        while remaining != 0 {
            let mut run = self.count_and_make_run(low, low + remaining)?;
            if run < min_run {
                let forced = min_run.min(remaining);
                self.binary_insertion_sort(low, low + run, low + forced)?;
                run = forced;
            }
            self.runs.push((low, run));
            self.merge_collapse()?;
            low += run;
            remaining -= run;
        }
        self.merge_force_collapse()
    }

    /// Sorts `low..high`, whose items before `start` are sorted already.
    fn binary_insertion_sort(&mut self, low: usize, start: usize, high: usize) -> Result<()> {
        let start = if low == start { start + 1 } else { start };
        for start in start..high {
            let pivot = self.work[start];
            let (mut left, mut right) = (low, start);
            while left < right {
                let mid = left + ((right - left) >> 1);
                if self.less(pivot, self.work[mid])? {
                    right = mid;
                } else {
                    left = mid + 1;
                }
            }
            self.work.copy_within(left..start, left + 1);
            self.work[left] = pivot;
        }
        Ok(())
    }

    /// The length of the run at `low`: the longest ascending one, or the longest strictly
    /// descending one, which is reversed.
    fn count_and_make_run(&mut self, low: usize, high: usize) -> Result<usize> {
        if low + 1 == high {
            return Ok(1);
        }
        let descending = self.less(self.work[low + 1], self.work[low])?;
        let mut run = 2;
        let mut previous = self.work[low + 1];
        for index in low + 2..high {
            let current = self.work[index];
            if self.less(current, previous)? != descending {
                break;
            }
            previous = current;
            run += 1;
        }
        if descending {
            self.work[low..low + run].reverse();
        }
        Ok(run)
    }

    /// Whether the run `n` from the top is shorter than the two above it together.
    fn invariant_holds(&self, n: usize) -> bool {
        n < 2 || self.runs[n - 2].1 > self.runs[n - 1].1 + self.runs[n].1
    }

    fn merge_collapse(&mut self) -> Result<()> {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            if !self.invariant_holds(n + 1) || !self.invariant_holds(n) {
                if self.runs[n - 1].1 < self.runs[n + 1].1 {
                    n -= 1;
                }
                self.merge_at(n)?;
            } else if self.runs[n].1 <= self.runs[n + 1].1 {
                self.merge_at(n)?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn merge_force_collapse(&mut self) -> Result<()> {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            if n > 0 && self.runs[n - 1].1 < self.runs[n + 1].1 {
                n -= 1;
            }
            self.merge_at(n)?;
        }
        Ok(())
    }

    /// Merges the runs `i` and `i + 1`.
    fn merge_at(&mut self, i: usize) -> Result<()> {
        let (mut base_a, mut length_a) = self.runs[i];
        let (base_b, length_b) = self.runs[i + 1];
        self.runs[i].1 = length_a + length_b;
        self.runs.remove(i + 1);
        // What of a comes before b is in place already, and so is what of b comes after a.
        let skipped = self.gallop_right(Of::Work, self.work[base_b], base_a, length_a, 0)?;
        base_a += skipped;
        length_a -= skipped;
        if length_a == 0 {
            return Ok(());
        }
        let last_of_a = self.work[base_a + length_a - 1];
        let length_b = self.gallop_left(Of::Work, last_of_a, base_b, length_b, length_b - 1)?;
        if length_b == 0 {
            return Ok(());
        }
        if length_a <= length_b {
            self.merge_low(base_a, length_a, base_b, length_b)
        } else {
            self.merge_high(base_a, length_a, base_b, length_b)
        }
    }

    /// Where `key` goes in the sorted `length` items at `base`, before any equal to it,
    /// searching out from `hint`.
    fn gallop_left(
        &mut self,
        of: Of,
        key: T,
        base: usize,
        length: usize,
        hint: usize,
    ) -> Result<usize> {
        let (mut last, mut offset) = (0, 1);
        if self.less(self.at(of, base + hint), key)? {
            let max = length - hint;
            while offset < max {
                if !self.less(self.at(of, base + hint + offset), key)? {
                    break;
                }
                last = offset;
                offset = (offset << 1) + 1;
            }
            offset = offset.min(max);
            last += hint;
            offset += hint;
        } else {
            let max = hint + 1;
            while offset < max {
                if self.less(self.at(of, base + hint - offset), key)? {
                    break;
                }
                last = offset;
                offset = (offset << 1) + 1;
            }
            offset = offset.min(max);
            // `last` is one before where the search starts, which may be before `base`.
            (last, offset) = (hint + 1 - offset, hint - last);
            return self.bisect_left(of, key, base, last, offset);
        }
        self.bisect_left(of, key, base, last + 1, offset)
    }

    /// The binary search that ends [`State::gallop_left`], over `from..to`.
    fn bisect_left(
        &mut self,
        of: Of,
        key: T,
        base: usize,
        mut from: usize,
        mut to: usize,
    ) -> Result<usize> {
        while from < to {
            let mid = from + ((to - from) >> 1);
            if self.less(self.at(of, base + mid), key)? {
                from = mid + 1;
            } else {
                to = mid;
            }
        }
        Ok(to)
    }

    /// Where `key` goes in the sorted `length` items at `base`, after any equal to it,
    /// searching out from `hint`.
    fn gallop_right(
        &mut self,
        of: Of,
        key: T,
        base: usize,
        length: usize,
        hint: usize,
    ) -> Result<usize> {
        let (mut last, mut offset) = (0, 1);
        if self.less(key, self.at(of, base + hint))? {
            let max = hint + 1;
            while offset < max {
                if !self.less(key, self.at(of, base + hint - offset))? {
                    break;
                }
                last = offset;
                offset = (offset << 1) + 1;
            }
            offset = offset.min(max);
            (last, offset) = (hint + 1 - offset, hint - last);
            return self.bisect_right(of, key, base, last, offset);
        }
        let max = length - hint;
        while offset < max {
            if self.less(key, self.at(of, base + hint + offset))? {
                break;
            }
            last = offset;
            offset = (offset << 1) + 1;
        }
        offset = offset.min(max);
        self.bisect_right(of, key, base, last + hint + 1, offset + hint)
    }

    /// The binary search that ends [`State::gallop_right`], over `from..to`.
    fn bisect_right(
        &mut self,
        of: Of,
        key: T,
        base: usize,
        mut from: usize,
        mut to: usize,
    ) -> Result<usize> {
        while from < to {
            let mid = from + ((to - from) >> 1);
            if self.less(key, self.at(of, base + mid))? {
                to = mid;
            } else {
                from = mid + 1;
            }
        }
        Ok(to)
    }

    /// Merges the runs of `length_a` items at `base_a` and `length_b` right after, the first
    /// no longer than the second, from the front.
    fn merge_low(
        &mut self,
        base_a: usize,
        mut length_a: usize,
        base_b: usize,
        mut length_b: usize,
    ) -> Result<()> {
        self.temp.clear();
        self.temp
            .extend_from_slice(&self.work[base_a..base_a + length_a]);
        let (mut dest, mut cursor_temp, mut cursor_b) = (base_a, 0, base_b);
        self.work[dest] = self.work[cursor_b];
        dest += 1;
        cursor_b += 1;
        let end = 'merge: {
            length_b -= 1;
            if length_b == 0 {
                break 'merge End::Succeed;
            }
            if length_a == 1 {
                break 'merge End::CopyOne;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let (mut wins_a, mut wins_b) = (0, 0);
                loop {
                    if self.less(self.work[cursor_b], self.temp[cursor_temp])? {
                        self.work[dest] = self.work[cursor_b];
                        dest += 1;
                        cursor_b += 1;
                        wins_b += 1;
                        length_b -= 1;
                        wins_a = 0;
                        if length_b == 0 {
                            break 'merge End::Succeed;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    } else {
                        self.work[dest] = self.temp[cursor_temp];
                        dest += 1;
                        cursor_temp += 1;
                        wins_a += 1;
                        length_a -= 1;
                        wins_b = 0;
                        if length_a == 1 {
                            break 'merge End::CopyOne;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP_WINS || wins_b >= MIN_GALLOP_WINS || first {
                    first = false;
                    min_gallop = min_gallop.saturating_sub(1).max(1);
                    self.min_gallop = min_gallop;
                    let key = self.work[cursor_b];
                    wins_a = self.gallop_right(Of::Temp, key, cursor_temp, length_a, 0)?;
                    if wins_a > 0 {
                        self.work[dest..dest + wins_a]
                            .copy_from_slice(&self.temp[cursor_temp..cursor_temp + wins_a]);
                        dest += wins_a;
                        cursor_temp += wins_a;
                        length_a -= wins_a;
                        if length_a == 1 {
                            break 'merge End::CopyOne;
                        }
                        // Only a comparator that contradicts itself leaves none.
                        if length_a == 0 {
                            break 'merge End::Succeed;
                        }
                    }
                    self.work[dest] = self.work[cursor_b];
                    dest += 1;
                    cursor_b += 1;
                    length_b -= 1;
                    if length_b == 0 {
                        break 'merge End::Succeed;
                    }
                    let key = self.temp[cursor_temp];
                    wins_b = self.gallop_left(Of::Work, key, cursor_b, length_b, 0)?;
                    if wins_b > 0 {
                        self.work.copy_within(cursor_b..cursor_b + wins_b, dest);
                        dest += wins_b;
                        cursor_b += wins_b;
                        length_b -= wins_b;
                        if length_b == 0 {
                            break 'merge End::Succeed;
                        }
                    }
                    self.work[dest] = self.temp[cursor_temp];
                    dest += 1;
                    cursor_temp += 1;
                    length_a -= 1;
                    if length_a == 1 {
                        break 'merge End::CopyOne;
                    }
                }
                // Leaving galloping costs.
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        match end {
            End::Succeed => self.work[dest..dest + length_a]
                .copy_from_slice(&self.temp[cursor_temp..cursor_temp + length_a]),
            End::CopyOne => {
                self.work.copy_within(cursor_b..cursor_b + length_b, dest);
                self.work[dest + length_b] = self.temp[cursor_temp];
            }
        }
        Ok(())
    }

    /// Merges the runs of `length_a` items at `base_a` and `length_b` right after, the first
    /// longer than the second, from the back. Its cursors count one past the item they point
    /// at, as the merge may leave them before the first item.
    fn merge_high(
        &mut self,
        base_a: usize,
        mut length_a: usize,
        base_b: usize,
        mut length_b: usize,
    ) -> Result<()> {
        self.temp.clear();
        self.temp
            .extend_from_slice(&self.work[base_b..base_b + length_b]);
        let (mut dest, mut cursor_temp, mut cursor_a) =
            (base_b + length_b, length_b, base_a + length_a);
        self.work[dest - 1] = self.work[cursor_a - 1];
        dest -= 1;
        cursor_a -= 1;
        let end = 'merge: {
            length_a -= 1;
            if length_a == 0 {
                break 'merge End::Succeed;
            }
            if length_b == 1 {
                break 'merge End::CopyOne;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let (mut wins_a, mut wins_b) = (0, 0);
                loop {
                    if self.less(self.temp[cursor_temp - 1], self.work[cursor_a - 1])? {
                        self.work[dest - 1] = self.work[cursor_a - 1];
                        dest -= 1;
                        cursor_a -= 1;
                        wins_a += 1;
                        length_a -= 1;
                        wins_b = 0;
                        if length_a == 0 {
                            break 'merge End::Succeed;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    } else {
                        self.work[dest - 1] = self.temp[cursor_temp - 1];
                        dest -= 1;
                        cursor_temp -= 1;
                        wins_b += 1;
                        length_b -= 1;
                        wins_a = 0;
                        if length_b == 1 {
                            break 'merge End::CopyOne;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP_WINS || wins_b >= MIN_GALLOP_WINS || first {
                    first = false;
                    min_gallop = min_gallop.saturating_sub(1).max(1);
                    self.min_gallop = min_gallop;
                    let key = self.temp[cursor_temp - 1];
                    let k = self.gallop_right(Of::Work, key, base_a, length_a, length_a - 1)?;
                    wins_a = length_a - k;
                    if wins_a > 0 {
                        dest -= wins_a;
                        cursor_a -= wins_a;
                        self.work.copy_within(cursor_a..cursor_a + wins_a, dest);
                        length_a -= wins_a;
                        if length_a == 0 {
                            break 'merge End::Succeed;
                        }
                    }
                    self.work[dest - 1] = self.temp[cursor_temp - 1];
                    dest -= 1;
                    cursor_temp -= 1;
                    length_b -= 1;
                    if length_b == 1 {
                        break 'merge End::CopyOne;
                    }
                    let key = self.work[cursor_a - 1];
                    let k = self.gallop_left(Of::Temp, key, 0, length_b, length_b - 1)?;
                    wins_b = length_b - k;
                    if wins_b > 0 {
                        dest -= wins_b;
                        cursor_temp -= wins_b;
                        self.work[dest..dest + wins_b]
                            .copy_from_slice(&self.temp[cursor_temp..cursor_temp + wins_b]);
                        length_b -= wins_b;
                        if length_b == 1 {
                            break 'merge End::CopyOne;
                        }
                        // Only a comparator that contradicts itself leaves none.
                        if length_b == 0 {
                            break 'merge End::Succeed;
                        }
                    }
                    self.work[dest - 1] = self.work[cursor_a - 1];
                    dest -= 1;
                    cursor_a -= 1;
                    length_a -= 1;
                    if length_a == 0 {
                        break 'merge End::Succeed;
                    }
                }
                // Leaving galloping costs.
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        match end {
            End::Succeed => {
                self.work[dest - length_b..dest].copy_from_slice(&self.temp[..length_b])
            }
            End::CopyOne => {
                dest -= length_a;
                cursor_a -= length_a;
                self.work.copy_within(cursor_a..cursor_a + length_a, dest);
                self.work[dest - 1] = self.temp[cursor_temp - 1];
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

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

    #[cfg(feature = "collation")]
    #[test]
    fn sorts_strings_as_node_collates_them() {
        let mut words = [
            "b", "a", "B", "A", "_", "-", "1", "é", "e", "ä", "z", "Z", "aa", "a b", "a-b",
        ];
        sort(&mut words, |a, b| Ok(crate::locale_compare(a, b))).unwrap();
        // `[...].sort((x, y) => x.localeCompare(y))` in Node.
        assert_eq!(words.join(" "), "_ - 1 a A ä a b a-b aa b B e é z Z");
    }

    #[test]
    fn leaves_the_items_as_they_were_when_the_comparator_throws() {
        let mut items = [5, 4, 3, 2, 1, 0, 9, 8, 7];
        let thrown = sort(&mut items, |a, b| match a == 0 || b == 0 {
            true => Err(Error::Other("x".into())),
            false => Ok(f64::from(a - b)),
        });
        assert!(thrown.is_err());
        assert_eq!(items, [5, 4, 3, 2, 1, 0, 9, 8, 7]);
    }

    #[test]
    fn takes_nan_for_zero() {
        let mut items = [3, 1, 2];
        sort(&mut items, |_, _| Ok(f64::NAN)).unwrap();
        assert_eq!(items, [3, 1, 2]);
        let mut items: Vec<i32> = (1..=12).collect();
        sort(&mut items, |_, _| Ok(-1.0)).unwrap();
        assert_eq!(items, (1..=12).rev().collect::<Vec<_>>());
    }

    // Each recorded from Node 22.
    #[test]
    fn compares_as_v8_does() {
        assert_eq!(
            comparisons(&[0, 3, 2, 1]).join(" "),
            "1:0 2:1 2:1 2:0 3:2 3:0"
        );
        assert_eq!(
            comparisons(&[7, 6, 5, 4, 3, 2, 1, 0]).join(" "),
            "1:0 2:1 3:2 4:3 5:4 6:5 7:6"
        );
        assert_eq!(
            comparisons(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 50, 20, 30, 15, 12]).join(" "),
            "1:0 2:1 3:2 4:3 5:4 6:5 7:6 8:7 9:8 10:9 11:10 11:5 11:8 11:10 11:9 12:6 12:9 \
             12:10 12:11 13:6 13:11 13:8 13:9 14:7 14:11 14:9 14:13"
        );
        let shuffled: Vec<i64> = (0..150).map(|i| i * 37 % 150).collect();
        let calls = comparisons(&shuffled);
        assert_eq!(calls.len(), 876);
        assert_eq!(
            calls[380..400].join(" "),
            "81:77 81:76 82:76 82:79 82:78 83:80 83:78 83:79 84:80 84:83 84:79 85:84 85:81 \
             85:76 86:80 86:83 86:78 86:82 87:84 87:78"
        );
        assert_eq!(
            calls[calls.len() - 12..].join(" "),
            "122:45 122:49 126:49 126:53 130:53 130:57 134:57 134:61 138:61 138:65 142:65 \
             142:69"
        );
        // Enough runs to merge, and to gallop in both directions.
        let big: Vec<i64> = (0..1000).map(|i| i * 7919 % 1000).collect();
        let calls = comparisons(&big);
        assert_eq!(calls.len(), 8525);
        assert_eq!(
            calls[5000..5020].join(" "),
            "741:693 741:717 741:704 742:710 742:719 742:730 742:729 742:717 742:705 743:722 \
             743:694 743:705 743:718 743:706 744:722 744:694 744:708 744:732 744:719 744:707"
        );
        assert_eq!(
            calls[calls.len() - 12..].join(" "),
            "506:185 506:148 827:148 790:148 790:469 790:111 753:111 753:432 753:74 716:74 \
             716:395 716:37"
        );
        // Blocks of consecutive values, every third descending, whose merges gallop.
        let blocks: Vec<i64> = (0..28)
            .flat_map(|b| {
                let block = 11 * b % 28;
                let values = block * 37..((block + 1) * 37).min(1000);
                let values: Vec<i64> = match b % 3 {
                    0 => values.rev().collect(),
                    _ => values.collect(),
                };
                values
            })
            .collect();
        let calls = comparisons(&blocks);
        assert_eq!(calls.len(), 4225);
        assert_eq!(
            calls[600..620].join(" "),
            "223:189 224:205 224:196 224:191 224:189 224:223 225:204 225:195 225:190 225:223 \
             225:224 226:204 226:194 226:189 226:224 226:225 227:203 227:193 227:223 227:225"
        );
        assert_eq!(
            calls[calls.len() - 16..].join(" "),
            "889:104 889:108 889:110 889:334 890:334 891:334 893:334 897:334 905:334 921:334 \
             731:334 541:334 529:334 523:334 520:334 519:334"
        );
    }
}
