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

    /// The recovery JavaScript's number encodes.
    pub fn from_number(value: f64) -> Recover {
        let value = value as u64;
        Recover {
            index: (value & 0xffff) as usize,
            offset: (value >> 16) as usize,
        }
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

    /// The changed ranges, each where it was before the change and where it is after.
    fn spans(&self) -> impl Iterator<Item = Span> + '_ {
        let (old_index, new_index) = if self.inverted { (2, 1) } else { (1, 2) };
        let mut diff = 0;
        self.ranges
            .chunks_exact(3)
            .enumerate()
            .map(move |(index, range)| {
                let start = range[0] as isize;
                let (old_size, new_size) = (range[old_index] as isize, range[new_index] as isize);
                let (old_start, new_start) = if self.inverted {
                    (start - diff, start)
                } else {
                    (start, start + diff)
                };
                diff += new_size - old_size;
                Span {
                    index,
                    old_start,
                    old_size,
                    new_start,
                    new_size,
                }
            })
    }

    /// The position a recovery points at in this map's output.
    ///
    /// # Panics
    ///
    /// When the map has no range at the recovery's index, as for a recovery from a map this one
    /// doesn't mirror.
    pub fn recover(&self, recover: Recover) -> usize {
        let span = self
            .spans()
            .nth(recover.index)
            .expect("a recovery of one of the map's ranges");
        position(span.new_start + recover.offset as isize)
    }

    fn map_inner(&self, pos: usize, assoc: i32) -> MapResult {
        let pos = pos as isize;
        // How far the ranges before `pos` moved it.
        let mut moved = 0;
        for span in self.spans() {
            let (start, end) = (span.old_start, span.old_start + span.old_size);
            if start > pos {
                break;
            }
            if pos <= end {
                let side = if span.old_size == 0 {
                    assoc
                } else if pos == start {
                    -1
                } else if pos == end {
                    1
                } else {
                    assoc
                };
                let result = span.new_start + if side < 0 { 0 } else { span.new_size };
                let recover = (pos != if assoc < 0 { start } else { end }).then(|| Recover {
                    index: span.index,
                    offset: (pos - start) as usize,
                });
                let mut del_info = if pos == start {
                    DEL_AFTER
                } else if pos == end {
                    DEL_BEFORE
                } else {
                    DEL_ACROSS
                };
                if if assoc < 0 { pos != start } else { pos != end } {
                    del_info |= DEL_SIDE;
                }
                return MapResult {
                    pos: position(result),
                    del_info,
                    recover,
                };
            }
            moved = span.new_start + span.new_size - end;
        }
        MapResult {
            pos: position(pos + moved),
            del_info: 0,
            recover: None,
        }
    }

    /// Whether the position is in the range the recovery points at.
    pub fn touches(&self, pos: usize, recover: Recover) -> bool {
        let pos = pos as isize;
        self.spans()
            .take_while(|span| span.old_start <= pos)
            .any(|span| span.index == recover.index && pos <= span.old_start + span.old_size)
    }

    /// The changed ranges: each one's start and end before the change, and after.
    pub fn changes(&self) -> impl Iterator<Item = (usize, usize, usize, usize)> + '_ {
        self.spans().map(|span| {
            (
                position(span.old_start),
                position(span.old_start + span.old_size),
                position(span.new_start),
                position(span.new_start + span.new_size),
            )
        })
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
        self.map_inner(pos, assoc).pos
    }

    fn map_result(&self, pos: usize, assoc: i32) -> MapResult {
        self.map_inner(pos, assoc)
    }
}

/// One of a step map's changed ranges: its start and size before the change, and after.
struct Span {
    index: usize,
    old_start: isize,
    old_size: isize,
    new_start: isize,
    new_size: isize,
}

/// A position from a map's arithmetic, which only a map with its ranges out of order takes
/// below zero, where JavaScript's would give a negative position.
fn position(n: isize) -> usize {
    n.max(0) as usize
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
    pub fn slice(&self, from: usize, to: usize) -> MappingSlice<'_> {
        MappingSlice {
            mapping: self,
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
        let other = if position % 2 == 1 {
            position - 1
        } else {
            position + 1
        };
        self.mirror.get(other).copied()
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
}

impl Mappable for Mapping {
    fn map(&self, pos: usize, assoc: i32) -> usize {
        self.slice(self.from, self.to).map(pos, assoc)
    }

    fn map_result(&self, pos: usize, assoc: i32) -> MapResult {
        self.slice(self.from, self.to).map_result(pos, assoc)
    }
}

/// A part of a mapping, from map `from` to map `to`, which borrows the mapping's maps.
#[derive(Clone, Copy, Debug)]
pub struct MappingSlice<'a> {
    mapping: &'a Mapping,
    from: usize,
    to: usize,
}

impl MappingSlice<'_> {
    /// The part as a mapping of its own, which shares the maps until either one adds to them.
    pub fn to_mapping(self) -> Mapping {
        Mapping {
            maps: self.mapping.maps.clone(),
            mirror: self.mapping.mirror.clone(),
            from: self.from,
            to: self.to,
        }
    }

    fn map_inner(self, mut pos: usize, assoc: i32) -> (usize, u8) {
        let maps = self.mapping.maps();
        let mut del_info = 0;
        let mut index = self.from;
        // JavaScript throws a TypeError reading a map past the last; mapping can't fail, so it
        // stops there.
        let to = self.to.min(maps.len());
        while index < to {
            let result = maps[index].map_result(pos, assoc);
            if let Some(recover) = result.recover
                && let Some(mirror) = self.mapping.get_mirror(index)
                && mirror > index
                && mirror < to
            {
                index = mirror + 1;
                pos = maps[mirror].recover(recover);
                continue;
            }
            del_info |= result.del_info;
            pos = result.pos;
            index += 1;
        }
        (pos, del_info)
    }
}

impl Mappable for MappingSlice<'_> {
    fn map(&self, pos: usize, assoc: i32) -> usize {
        self.map_inner(pos, assoc).0
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
