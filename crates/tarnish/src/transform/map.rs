//! Mapping positions through changes: step maps, and mappings of many.

use std::fmt;
use std::sync::{Arc, LazyLock};

/// Something positions can be mapped through.
pub trait Mappable {
    /// Map a position. `assoc`'s sign says which side the position sticks to when content is
    /// inserted at it: negative for before, positive for after.
    fn map(&self, pos: usize, assoc: i32) -> usize;

    /// Map a position, and tell what was deleted around it.
    fn map_result(&self, pos: usize, assoc: i32) -> MapResult;
}

const DEL_BEFORE: u8 = 1;
const DEL_AFTER: u8 = 2;
const DEL_ACROSS: u8 = 4;
const DEL_SIDE: u8 = 8;

/// Where a position inside a replaced range was: the range's index in its map, and the offset
/// into it. A mirrored map puts the position back there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recover {
    pub index: usize,
    pub offset: usize,
}

impl Recover {
    /// The single number JavaScript encodes a recovery as: the index in the low 16 bits.
    pub fn to_number(self) -> f64 {
        self.index as f64 + self.offset as f64 * 65536.0
    }
}

/// A mapped position, with what the mapping deleted around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapResult {
    pub pos: usize,
    pub del_info: u8,
    pub recover: Option<Recover>,
}

impl MapResult {
    /// Whether the token on the side `assoc` points to was deleted.
    pub fn deleted(&self) -> bool {
        self.del_info & DEL_SIDE > 0
    }

    pub fn deleted_before(&self) -> bool {
        self.del_info & (DEL_BEFORE | DEL_ACROSS) > 0
    }

    pub fn deleted_after(&self) -> bool {
        self.del_info & (DEL_AFTER | DEL_ACROSS) > 0
    }

    /// Whether a deletion spanned the position, the tokens on both its sides.
    pub fn deleted_across(&self) -> bool {
        self.del_info & DEL_ACROSS > 0
    }
}

static EMPTY_RANGES: LazyLock<Arc<[usize]>> = LazyLock::new(|| Arc::from(Vec::new()));

/// The changes one step made: for each changed range, its start, its old size and its new
/// size.
#[derive(Clone, PartialEq, Eq)]
pub struct StepMap {
    ranges: Arc<[usize]>,
    inverted: bool,
}

impl StepMap {
    /// A map of changed ranges, three numbers each: start, old size, new size.
    pub fn new(ranges: Vec<usize>, inverted: bool) -> StepMap {
        if ranges.is_empty() {
            return StepMap::empty();
        }
        StepMap {
            ranges: ranges.into(),
            inverted,
        }
    }

    pub fn empty() -> StepMap {
        StepMap {
            ranges: EMPTY_RANGES.clone(),
            inverted: false,
        }
    }

    /// A map moving every position by `n`.
    pub fn offset(n: isize) -> StepMap {
        match n {
            0 => StepMap::empty(),
            n if n < 0 => StepMap::new(vec![0, n.unsigned_abs(), 0], false),
            n => StepMap::new(vec![0, 0, n as usize], false),
        }
    }

    pub fn ranges(&self) -> &[usize] {
        &self.ranges
    }

    pub fn inverted(&self) -> bool {
        self.inverted
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    fn sizes(&self) -> (usize, usize) {
        if self.inverted { (2, 1) } else { (1, 2) }
    }

    /// The position a recovery points at in this map's output.
    pub fn recover(&self, recover: Recover) -> usize {
        let mut diff: isize = 0;
        if !self.inverted {
            for index in 0..recover.index {
                diff += self.ranges[index * 3 + 2] as isize - self.ranges[index * 3 + 1] as isize;
            }
        }
        (self.ranges[recover.index * 3] as isize + diff) as usize + recover.offset
    }

    /// Map a position that may lie outside any document: the transform's changed range starts
    /// out beyond both ends.
    pub(crate) fn map_signed(&self, pos: i64, assoc: i32) -> i64 {
        self.map_inner(pos, assoc).0
    }

    fn map_inner(&self, pos: i64, assoc: i32) -> (i64, u8, Option<Recover>) {
        let (old_index, new_index) = self.sizes();
        let mut diff: i64 = 0;
        for i in (0..self.ranges.len()).step_by(3) {
            let start = self.ranges[i] as i64 - if self.inverted { diff } else { 0 };
            if start > pos {
                break;
            }
            let old_size = self.ranges[i + old_index] as i64;
            let new_size = self.ranges[i + new_index] as i64;
            let end = start + old_size;
            if pos <= end {
                let side = if old_size == 0 {
                    assoc
                } else if pos == start {
                    -1
                } else if pos == end {
                    1
                } else {
                    assoc
                };
                let result = start + diff + if side < 0 { 0 } else { new_size };
                let recover = if pos == if assoc < 0 { start } else { end } {
                    None
                } else {
                    Some(Recover {
                        index: i / 3,
                        offset: (pos - start) as usize,
                    })
                };
                let mut del = if pos == start {
                    DEL_AFTER
                } else if pos == end {
                    DEL_BEFORE
                } else {
                    DEL_ACROSS
                };
                if if assoc < 0 { pos != start } else { pos != end } {
                    del |= DEL_SIDE;
                }
                return (result, del, recover);
            }
            diff += new_size - old_size;
        }
        (pos + diff, 0, None)
    }

    /// Whether the position is in the range the recovery points at.
    pub fn touches(&self, pos: usize, recover: Recover) -> bool {
        let (old_index, new_index) = self.sizes();
        let pos = pos as i64;
        let mut diff: i64 = 0;
        for i in (0..self.ranges.len()).step_by(3) {
            let start = self.ranges[i] as i64 - if self.inverted { diff } else { 0 };
            if start > pos {
                break;
            }
            let old_size = self.ranges[i + old_index] as i64;
            let end = start + old_size;
            if pos <= end && i == recover.index * 3 {
                return true;
            }
            diff += self.ranges[i + new_index] as i64 - old_size;
        }
        false
    }

    /// The changed ranges: each one's start and end before the change, and after.
    pub fn changes(&self) -> Vec<(usize, usize, usize, usize)> {
        let (old_index, new_index) = self.sizes();
        let mut result = Vec::with_capacity(self.ranges.len() / 3);
        let mut diff: isize = 0;
        for i in (0..self.ranges.len()).step_by(3) {
            let start = self.ranges[i] as isize;
            let old_start = start - if self.inverted { diff } else { 0 };
            let new_start = start + if self.inverted { 0 } else { diff };
            let (old_size, new_size) = (
                self.ranges[i + old_index] as isize,
                self.ranges[i + new_index] as isize,
            );
            result.push((
                old_start as usize,
                (old_start + old_size) as usize,
                new_start as usize,
                (new_start + new_size) as usize,
            ));
            diff += new_size - old_size;
        }
        result
    }

    /// The map of the change undone: from positions after it to positions before.
    pub fn invert(&self) -> StepMap {
        StepMap {
            ranges: self.ranges.clone(),
            inverted: !self.inverted,
        }
    }
}

impl Mappable for StepMap {
    fn map(&self, pos: usize, assoc: i32) -> usize {
        self.map_inner(pos as i64, assoc).0.max(0) as usize
    }

    fn map_result(&self, pos: usize, assoc: i32) -> MapResult {
        let (pos, del_info, recover) = self.map_inner(pos as i64, assoc);
        MapResult {
            pos: pos.max(0) as usize,
            del_info,
            recover,
        }
    }
}

impl fmt::Display for StepMap {
    /// `toString`: `-` for an inverted map, then the ranges as JSON.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let ranges: Vec<String> = self.ranges.iter().map(usize::to_string).collect();
        write!(
            f,
            "{}[{}]",
            if self.inverted { "-" } else { "" },
            ranges.join(",")
        )
    }
}

impl fmt::Debug for StepMap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// A pipeline of step maps, which knows which maps mirror others, so that mapping through a
/// change and its undoing gives back the position it started from.
#[derive(Clone, Debug, Default)]
pub struct Mapping {
    maps: Arc<Vec<StepMap>>,
    mirror: Arc<Vec<usize>>,
    from: usize,
    to: usize,
}

impl Mapping {
    pub fn new() -> Mapping {
        Mapping::default()
    }

    /// A mapping through these maps, with mirror pairs, from map `from` to map `to`.
    pub fn with_maps(maps: Vec<StepMap>, mirror: Vec<usize>, from: usize, to: usize) -> Mapping {
        Mapping {
            maps: Arc::new(maps),
            mirror: Arc::new(mirror),
            from,
            to,
        }
    }

    pub fn maps(&self) -> &[StepMap] {
        &self.maps
    }

    /// Pairs of maps that mirror each other, flattened.
    pub fn mirror(&self) -> &[usize] {
        &self.mirror
    }

    pub fn from(&self) -> usize {
        self.from
    }

    pub fn to(&self) -> usize {
        self.to
    }

    /// The part of the mapping from map `from` to map `to`.
    pub fn slice(&self, from: usize, to: usize) -> Mapping {
        Mapping {
            maps: self.maps.clone(),
            mirror: self.mirror.clone(),
            from,
            to,
        }
    }

    /// Add a map at the end, the mirror of map `mirrors` when given.
    pub fn append_map(&mut self, map: StepMap, mirrors: Option<usize>) {
        let maps = Arc::make_mut(&mut self.maps);
        maps.push(map);
        self.to = maps.len();
        if let Some(mirrors) = mirrors {
            self.set_mirror(self.to - 1, mirrors);
        }
    }

    /// Add all of `mapping`'s maps, keeping which mirror which.
    pub fn append_mapping(&mut self, mapping: &Mapping) {
        let start = self.maps.len();
        for (index, map) in mapping.maps.iter().enumerate() {
            let mirror = mapping.get_mirror(index).filter(|&mirror| mirror < index);
            self.append_map(map.clone(), mirror.map(|mirror| start + mirror));
        }
    }

    /// The map that mirrors map `n`.
    pub fn get_mirror(&self, n: usize) -> Option<usize> {
        let position = self.mirror.iter().position(|&index| index == n)?;
        Some(
            self.mirror[if position % 2 == 1 {
                position - 1
            } else {
                position + 1
            }],
        )
    }

    /// Record that maps `n` and `m` mirror each other.
    pub fn set_mirror(&mut self, n: usize, m: usize) {
        let mirror = Arc::make_mut(&mut self.mirror);
        mirror.push(n);
        mirror.push(m);
    }

    /// Add `mapping`'s maps inverted, last first.
    pub fn append_mapping_inverted(&mut self, mapping: &Mapping) {
        let total = self.maps.len() + mapping.maps.len();
        for index in (0..mapping.maps.len()).rev() {
            let mirror = mapping.get_mirror(index).filter(|&mirror| mirror > index);
            self.append_map(
                mapping.maps[index].invert(),
                mirror.map(|mirror| total - mirror - 1),
            );
        }
    }

    pub fn invert(&self) -> Mapping {
        let mut inverse = Mapping::new();
        inverse.append_mapping_inverted(self);
        inverse
    }

    fn map_inner(&self, mut pos: usize, assoc: i32) -> (usize, u8) {
        let mut del_info = 0;
        let mut index = self.from;
        let to = self.to.min(self.maps.len());
        while index < to {
            let map = &self.maps[index];
            let result = map.map_result(pos, assoc);
            if let Some(recover) = result.recover
                && let Some(mirror) = self.get_mirror(index)
                && mirror > index
                && mirror < to
            {
                index = mirror + 1;
                pos = self.maps[mirror].recover(recover);
                continue;
            }
            del_info |= result.del_info;
            pos = result.pos;
            index += 1;
        }
        (pos, del_info)
    }
}

impl Mappable for Mapping {
    fn map(&self, pos: usize, assoc: i32) -> usize {
        if !self.mirror.is_empty() {
            return self.map_inner(pos, assoc).0;
        }
        let to = self.to.min(self.maps.len());
        self.maps[self.from.min(to)..to]
            .iter()
            .fold(pos, |pos, map| map.map(pos, assoc))
    }

    fn map_result(&self, pos: usize, assoc: i32) -> MapResult {
        let (pos, del_info) = self.map_inner(pos, assoc);
        MapResult {
            pos,
            del_info,
            recover: None,
        }
    }
}
