//! Marks: emphasis, links and the like, held by nodes in sets sorted by their types' rank.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use super::attrs::Attrs;
use super::compare_deep::objects_equal;
use super::read::Reader;
use super::schema::{MarkType, Schema};
use crate::error::Result;
use crate::js::Json;

/// A set of marks, sorted by their types' rank. The empty set holds nothing, so that a node
/// without marks, as most are, touches no count shared between threads to make or drop it.
#[derive(Clone, Default, PartialEq)]
pub struct Marks(Option<Arc<[Mark]>>);

impl Marks {
    /// Whether these are the very same set, every empty set being `Mark.none`.
    pub fn ptr_eq(&self, other: &Marks) -> bool {
        match (&self.0, &other.0) {
            (Some(marks), Some(others)) => Arc::ptr_eq(marks, others),
            (None, None) => true,
            _ => false,
        }
    }
}

impl Deref for Marks {
    type Target = [Mark];

    fn deref(&self) -> &[Mark] {
        self.0.as_deref().unwrap_or(&[])
    }
}

impl FromIterator<Mark> for Marks {
    fn from_iter<I: IntoIterator<Item = Mark>>(marks: I) -> Marks {
        let marks: Arc<[Mark]> = marks.into_iter().collect();
        Marks((!marks.is_empty()).then_some(marks))
    }
}

impl From<Vec<Mark>> for Marks {
    fn from(marks: Vec<Mark>) -> Marks {
        marks.into_iter().collect()
    }
}

impl From<&[Mark]> for Marks {
    fn from(marks: &[Mark]) -> Marks {
        marks.iter().cloned().collect()
    }
}

impl<const N: usize> From<[Mark; N]> for Marks {
    fn from(marks: [Mark; N]) -> Marks {
        marks.into_iter().collect()
    }
}

impl fmt::Debug for Marks {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

#[derive(Clone)]
pub struct Mark(Arc<MarkData>);

struct MarkData {
    mark_type: MarkType,
    attrs: Attrs,
}

impl Mark {
    pub(crate) fn new(mark_type: MarkType, attrs: Attrs) -> Mark {
        Mark(Arc::new(MarkData { mark_type, attrs }))
    }

    /// The empty set.
    pub fn none() -> Marks {
        Marks::default()
    }

    pub fn mark_type(&self) -> &MarkType {
        &self.0.mark_type
    }

    pub fn attrs(&self) -> &Attrs {
        &self.0.attrs
    }

    /// An identity for the mark, the same for clones of it.
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    /// Whether this is the very same mark as `other`, not just an equal one.
    pub fn ptr_eq(&self, other: &Mark) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// The set with this mark added in its place, replacing marks it excludes. The set itself
    /// when it has the mark, or a mark that excludes it.
    pub fn add_to_set(&self, set: &Marks) -> Marks {
        let mut copy: Option<Vec<Mark>> = None;
        let mut placed = false;
        for (index, other) in set.iter().enumerate() {
            if self == other {
                return set.clone();
            }
            if self.mark_type().excludes(other.mark_type()) {
                copy.get_or_insert_with(|| set[..index].to_vec());
            } else if other.mark_type().excludes(self.mark_type()) {
                return set.clone();
            } else {
                if !placed && other.mark_type().rank() > self.mark_type().rank() {
                    copy.get_or_insert_with(|| set[..index].to_vec())
                        .push(self.clone());
                    placed = true;
                }
                if let Some(copy) = &mut copy {
                    copy.push(other.clone());
                }
            }
        }
        let mut copy = copy.unwrap_or_else(|| set.to_vec());
        if !placed {
            copy.push(self.clone());
        }
        copy.into()
    }

    /// The set without this mark, or the set itself when it doesn't have it.
    pub fn remove_from_set(&self, set: &Marks) -> Marks {
        match set.iter().position(|other| self == other) {
            Some(index) => set[..index]
                .iter()
                .chain(&set[index + 1..])
                .cloned()
                .collect(),
            None => set.clone(),
        }
    }

    pub fn is_in_set(&self, set: &[Mark]) -> bool {
        set.iter().any(|other| self == other)
    }

    pub fn same_set(a: &[Mark], b: &[Mark]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a == b)
    }

    /// The marks as a set, sorted by rank.
    pub fn set_from(marks: &[Mark]) -> Marks {
        if marks.is_empty() {
            return Mark::none();
        }
        if marks.is_sorted_by_key(|mark| mark.mark_type().rank()) {
            return marks.into();
        }
        let mut copy = marks.to_vec();
        copy.sort_by_key(|mark| mark.mark_type().rank());
        copy.into()
    }

    /// [`set_from`](Self::set_from), taking the marks out of `marks`.
    pub(crate) fn set_from_vec(marks: &mut Vec<Mark>) -> Marks {
        if marks.is_empty() {
            return Mark::none();
        }
        marks.sort_by_key(|mark| mark.mark_type().rank());
        marks.drain(..).collect()
    }

    pub fn from_json<'a>(schema: &Schema, json: impl Json<'a>) -> Result<Mark> {
        Reader::new(schema).mark(json)
    }
}

impl PartialEq for Mark {
    fn eq(&self, other: &Mark) -> bool {
        self.ptr_eq(other)
            || (self.mark_type() == other.mark_type() && objects_equal(self.attrs(), other.attrs()))
    }
}

impl fmt::Debug for Mark {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{:?}", self.mark_type().name(), self.attrs())
    }
}
