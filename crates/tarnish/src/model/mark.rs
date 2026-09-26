//! Marks: emphasis, links and the like, held by nodes in sets sorted by their types' rank.

use std::fmt;
use std::sync::{Arc, LazyLock};

use super::attrs::Attrs;
use super::compare_deep::objects_equal;
use super::schema::{MarkType, Schema};
use crate::error::{Error, Result};
use crate::js::{Given, Json};
use crate::json::{Map, Value};

/// A set of marks, sorted by their types' rank.
pub type Marks = Arc<[Mark]>;

static NONE: LazyLock<Marks> = LazyLock::new(|| Arc::from(Vec::new()));

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
        NONE.clone()
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

    pub fn to_json(&self) -> Value {
        let mut json = Map::with_capacity(2);
        json.push("type".into(), Value::String(self.mark_type().name().into()));
        if !self.attrs().is_empty() {
            json.push("attrs".into(), Value::Object((**self.attrs()).clone()));
        }
        Value::Object(json)
    }

    pub fn from_json<'a>(schema: &Schema, json: impl Json<'a>) -> Result<Mark> {
        if !json.truthy() {
            return Err(Error::Range("Invalid input for Mark.fromJSON".into()));
        }
        let name = json.get("type").map_or("undefined".into(), Json::string);
        let mark_type = schema
            .mark_type(&name)
            .ok_or_else(|| Error::Range(format!("There is no mark type {name} in this schema")))?;
        let attrs = json
            .get("attrs")
            .map_or(Given::Falsy(Value::Null), Json::attrs);
        let mark = mark_type.create_given(&attrs)?;
        mark_type.check_attrs(mark.attrs())?;
        Ok(mark)
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
