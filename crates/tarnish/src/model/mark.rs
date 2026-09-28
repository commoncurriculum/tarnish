//! Marks: emphasis, links and the like, held by nodes in sets sorted by their types' rank.

use std::fmt;
use std::sync::Arc;

use super::attrs::Attrs;
use super::read::Reader;
use super::schema::{MarkType, Schema};
use super::view::{MarkRef, SetRef};
use crate::chunk::{Builder, Chunk, EMPTY_SET, Holder, ValueRef};
use crate::error::Result;
use crate::js::Json;
use crate::json::Map;

/// A mark, in the chunk that holds it.
#[derive(Clone)]
pub struct Mark<'a> {
    pub(crate) chunk: Arc<Chunk<'a>>,
    pub(crate) index: u32,
}

/// A set of marks, sorted by their types' rank, in the chunk that holds it. The empty set is in
/// no chunk, so that a node without marks, as most are, touches no count shared between
/// threads.
#[derive(Clone, Default)]
pub struct Marks<'a> {
    pub(crate) chunk: Option<Arc<Chunk<'a>>>,
    pub(crate) set: u32,
}

impl<'a> Marks<'a> {
    /// The set a ref in `chunk` names.
    pub(crate) fn at(chunk: &Arc<Chunk<'a>>, reference: u32) -> Marks<'a> {
        let (chunk, set) = chunk.resolve(reference);
        match set {
            EMPTY_SET => Marks::default(),
            set => Marks {
                chunk: Some(chunk.clone()),
                set,
            },
        }
    }

    pub(crate) fn view(&self) -> Option<SetRef<'_>> {
        self.chunk.as_ref().map(|chunk| SetRef {
            chunk,
            set: self.set,
        })
    }

    pub fn len(&self) -> usize {
        self.view().map_or(0, SetRef::len)
    }

    pub fn is_empty(&self) -> bool {
        self.chunk.is_none()
    }

    /// The marks, each as its own handle.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Mark<'a>> + ExactSizeIterator + '_ {
        let (start, len) = match &self.chunk {
            Some(chunk) => chunk.set(self.set),
            None => (0, 0),
        };
        (start..start + len).map(move |member| {
            let chunk = self
                .chunk
                .as_ref()
                .expect("a set with members is in a chunk");
            let (chunk, index) = chunk.resolve(chunk.member(member));
            Mark {
                chunk: chunk.clone(),
                index,
            }
        })
    }

    pub fn get(&self, index: usize) -> Option<Mark<'a>> {
        self.iter().nth(index)
    }

    pub fn to_vec(&self) -> Vec<Mark<'a>> {
        self.iter().collect()
    }

    /// Whether these are the very same set, every empty set being `Mark.none`.
    pub fn ptr_eq(&self, other: &Marks) -> bool {
        match (&self.chunk, &other.chunk) {
            (Some(chunk), Some(others)) => self.set == other.set && chunk.ptr_eq(others),
            (None, None) => true,
            _ => false,
        }
    }

    /// An identity for the set, the same for clones of it.
    pub fn id(&self) -> (usize, u32) {
        match &self.chunk {
            Some(chunk) => (Arc::as_ptr(chunk) as *const u8 as usize, self.set),
            None => (0, 0),
        }
    }

    /// A set of these marks, in this order, which for a set is by rank.
    pub fn from_list(marks: &[Mark<'a>]) -> Marks<'a> {
        let Some(first) = marks.first() else {
            return Marks::default();
        };
        let mut builder = Builder::new(first.chunk.schema());
        let members: Vec<u32> = marks
            .iter()
            .map(|mark| builder.external(&mark.chunk, mark.index))
            .collect();
        let set = builder.set(&members);
        Marks {
            chunk: Some(builder.seal()),
            set,
        }
    }

    /// Writes a ref to this set into a chunk being built.
    pub(crate) fn write(&self, builder: &mut Builder<'a>) -> u32 {
        match &self.chunk {
            Some(chunk) => builder.external(chunk, self.set),
            None => EMPTY_SET,
        }
    }
}

impl PartialEq for Marks<'_> {
    fn eq(&self, other: &Marks) -> bool {
        match (self.view(), other.view()) {
            (Some(a), Some(b)) => a.same(b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl fmt::Debug for Marks<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<'a> Mark<'a> {
    /// A mark of this type with these attributes, in a chunk of its own.
    pub(crate) fn new(mark_type: &MarkType, attrs: &Map) -> Mark<'static> {
        let mut builder = Builder::new(mark_type.schema());
        let attrs = builder.map(attrs);
        let index = builder.mark(mark_type.rank() as u32, attrs);
        Mark {
            chunk: builder.seal(),
            index,
        }
    }

    pub(crate) fn view(&self) -> MarkRef<'_> {
        MarkRef {
            chunk: &self.chunk,
            index: self.index,
        }
    }

    /// The empty set.
    pub fn none() -> Marks<'a> {
        Marks::default()
    }

    pub fn mark_type(&self) -> MarkType<'_> {
        self.view().mark_type()
    }

    pub fn attrs(&self) -> Attrs<'a> {
        let (chunk, value) = self.chunk.resolve(self.chunk.mark(self.index).1);
        Attrs {
            chunk: chunk.clone(),
            value,
        }
    }

    pub fn attrs_view(&self) -> ValueRef<'_> {
        self.view().attrs()
    }

    /// An identity for the mark, the same for clones of it.
    pub fn id(&self) -> (usize, u32) {
        (Arc::as_ptr(&self.chunk) as *const u8 as usize, self.index)
    }

    /// Whether this is the very same mark as `other`, not just an equal one.
    pub fn ptr_eq(&self, other: &Mark) -> bool {
        self.view().ptr_eq(other.view())
    }

    /// The set with this mark added in its place, replacing marks it excludes. The set itself
    /// when it has the mark, or a mark that excludes it.
    pub fn add_to_set(&self, set: &Marks<'a>) -> Marks<'a> {
        let marks = set.to_vec();
        match self.added_to(&marks) {
            Some(added) => Marks::from_list(&added),
            None => set.clone(),
        }
    }

    /// [`add_to_set`](Self::add_to_set) on a list of marks, `None` when it leaves them as they
    /// are.
    pub fn added_to(&self, marks: &[Mark<'a>]) -> Option<Vec<Mark<'a>>> {
        added_to(self, marks)
    }

    /// The set without this mark, or the set itself when it doesn't have it.
    pub fn remove_from_set(&self, set: &Marks<'a>) -> Marks<'a> {
        match self.removed_from(&set.to_vec()) {
            Some(kept) => Marks::from_list(&kept),
            None => set.clone(),
        }
    }

    /// [`remove_from_set`](Self::remove_from_set) on a list of marks, `None` when it doesn't
    /// have this mark.
    pub fn removed_from(&self, marks: &[Mark<'a>]) -> Option<Vec<Mark<'a>>> {
        let index = marks.iter().position(|other| self == other)?;
        Some(
            marks[..index]
                .iter()
                .chain(&marks[index + 1..])
                .cloned()
                .collect(),
        )
    }

    pub fn is_in_set(&self, set: &Marks) -> bool {
        let mine = self.view();
        set.view()
            .is_some_and(|set| set.iter().any(|other| mine.equals(other)))
    }

    pub fn is_in_list(&self, marks: &[Mark]) -> bool {
        marks.iter().any(|other| self == other)
    }

    pub fn same_set(a: &Marks, b: &Marks) -> bool {
        a == b
    }

    pub fn same_list(a: &[Mark], b: &[Mark]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a == b)
    }

    /// The marks as a set, sorted by rank.
    pub fn set_from(marks: &[Mark<'a>]) -> Marks<'a> {
        if marks.is_empty() {
            return Mark::none();
        }
        if marks.is_sorted_by_key(|mark| mark.view().rank()) {
            return Marks::from_list(marks);
        }
        Marks::from_list(&Mark::sorted(marks))
    }

    /// The marks sorted by rank, as `Mark.setFrom` sorts an array.
    pub fn sorted(marks: &[Mark<'a>]) -> Vec<Mark<'a>> {
        let mut copy = marks.to_vec();
        copy.sort_by_key(|mark| mark.view().rank());
        copy
    }

    pub fn from_json<'j>(schema: &Schema, json: impl Json<'j>) -> Result<Mark<'static>> {
        let mut reader = Reader::new(schema);
        let index = reader.mark(json)?;
        Ok(Mark {
            chunk: reader.finish(),
            index,
        })
    }
}

impl PartialEq for Mark<'_> {
    fn eq(&self, other: &Mark) -> bool {
        self.view().equals(other.view())
    }
}

/// A mark as `addToSet` reads it: a handle, or a mark borrowed from its chunk.
pub(crate) trait SetMember: Clone {
    fn mark_type(&self) -> MarkType<'_>;

    /// `Mark.eq`.
    fn same(&self, other: &Self) -> bool;
}

impl SetMember for Mark<'_> {
    fn mark_type(&self) -> MarkType<'_> {
        Mark::mark_type(self)
    }

    fn same(&self, other: &Self) -> bool {
        self == other
    }
}

impl SetMember for MarkRef<'_> {
    fn mark_type(&self) -> MarkType<'_> {
        MarkRef::mark_type(*self)
    }

    fn same(&self, other: &Self) -> bool {
        self.equals(*other)
    }
}

/// `mark.addToSet(marks)` on a list, `None` when it leaves them as they are.
pub(crate) fn added_to<M: SetMember>(mark: &M, marks: &[M]) -> Option<Vec<M>> {
    let mut copy: Option<Vec<M>> = None;
    let mut placed = false;
    let my_type = mark.mark_type();
    for (index, other) in marks.iter().enumerate() {
        if mark.same(other) {
            return None;
        }
        let other_type = other.mark_type();
        if my_type.excludes(&other_type) {
            copy.get_or_insert_with(|| marks[..index].to_vec());
        } else if other_type.excludes(&my_type) {
            return None;
        } else {
            if !placed && other_type.rank() > my_type.rank() {
                copy.get_or_insert_with(|| marks[..index].to_vec())
                    .push(mark.clone());
                placed = true;
            }
            if let Some(copy) = &mut copy {
                copy.push(other.clone());
            }
        }
    }
    let mut copy = copy.unwrap_or_else(|| marks.to_vec());
    if !placed {
        copy.push(mark.clone());
    }
    Some(copy)
}

impl fmt::Debug for Mark<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{:?}", self.mark_type().name(), self.attrs_view())
    }
}
