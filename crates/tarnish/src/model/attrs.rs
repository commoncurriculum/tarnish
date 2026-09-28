//! The attributes of node and mark types: their specs, their defaults, and the attributes nodes
//! and marks get from what they're given.

use std::fmt;
use std::sync::Arc;

use crate::chunk::{Builder, Chunk, ValueRef, value_equals};
use crate::error::{Error, Result};
use crate::js::{AttrKeys, Given, Keys, TypeOf};
use crate::json::{Key, Map, Value};

/// A function that raises an error for an attribute value it doesn't accept, `None` being
/// `undefined`.
pub type ValidateHook = Arc<dyn Fn(Option<&Value>) -> Result<()> + Send + Sync>;

#[derive(Clone, Default)]
pub struct AttributeSpec {
    /// The default, when the spec has one. `Some(None)` is a default of `undefined`, which a
    /// node or mark can't hold: it leaves the attribute out, where ProseMirror keeps the name
    /// with no value, so its `toJSON` writes `"attrs": {}` and `hasMarkup` sees the name.
    pub default: Option<Option<Value>>,
    pub validate: Option<Validate>,
}

#[derive(Clone)]
pub enum Validate {
    /// `|`-separated names of the types a value may have, as `typeof` gives them.
    Types(String),
    Hook(ValidateHook),
}

enum Check {
    /// The types a value may have, a bit for each [`TypeOf`], and what to say when it has
    /// another.
    Types(u8, String),
    Hook(ValidateHook),
}

fn bit(type_of: TypeOf) -> u8 {
    1 << type_of as u8
}

struct Attribute {
    default: Option<Option<Value>>,
    check: Option<Check>,
}

/// A node's or mark's attributes, in the chunk that holds them.
#[derive(Clone)]
pub struct Attrs<'a> {
    pub(crate) chunk: Arc<Chunk<'a>>,
    pub(crate) value: u32,
}

impl<'a> Attrs<'a> {
    pub fn view(&self) -> ValueRef<'_> {
        ValueRef {
            chunk: &self.chunk,
            index: self.value,
        }
    }

    pub fn get(&self, key: &str) -> Option<ValueRef<'_>> {
        self.view().get(key)
    }

    pub fn len(&self) -> usize {
        self.view().len()
    }

    pub fn is_empty(&self) -> bool {
        self.view().is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, ValueRef<'_>)> {
        self.view().entries()
    }

    pub fn to_map(&self) -> Map {
        self.view().to_map()
    }

    /// `compareDeep` with an object.
    pub fn equals_map(&self, map: &Map) -> bool {
        self.view().equals_map(map)
    }
}

impl PartialEq for Attrs<'_> {
    fn eq(&self, other: &Attrs) -> bool {
        value_equals(self.view(), other.view())
    }
}

impl fmt::Debug for Attrs<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.view(), f)
    }
}

/// An attribute's value, in a chunk or outside one.
#[derive(Clone, Copy)]
enum Found<'v> {
    Json(&'v Value),
    Chunk(ValueRef<'v>),
}

/// A node or mark type's attributes.
pub(crate) struct AttrSet {
    attrs: Vec<(Key, Attribute)>,
    /// What nodes or marks given no attributes get, when every attribute has a default.
    defaults: Option<Map>,
    /// Whether any attribute has a check.
    checked: bool,
}

/// The attributes computed from what a type is given: its defaults, or each attribute's value,
/// `None` for one whose default is `undefined`.
pub(crate) enum Computed<'g> {
    Defaults,
    Values(Vec<(&'g Key, Option<&'g Value>)>),
}

impl AttrSet {
    pub fn new(type_name: &str, specs: &[(String, AttributeSpec)]) -> AttrSet {
        let attrs: Vec<(Key, Attribute)> = specs
            .iter()
            .map(|(name, spec)| {
                let check = spec.validate.as_ref().map(|validate| match validate {
                    Validate::Hook(hook) => Check::Hook(hook.clone()),
                    Validate::Types(types) => {
                        let types: Vec<&str> = types.split('|').collect();
                        let allowed = types
                            .iter()
                            .filter_map(|name| TypeOf::named(name))
                            .fold(0, |allowed, type_of| allowed | bit(type_of));
                        let expected = format!(
                            "Expected value of type {} for attribute {name} on type {type_name}",
                            types.join(",")
                        );
                        Check::Types(allowed, expected)
                    }
                });
                let attribute = Attribute {
                    default: spec.default.clone(),
                    check,
                };
                (Key::from(name.as_str()), attribute)
            })
            .collect();
        AttrSet {
            defaults: defaults(&attrs),
            checked: attrs.iter().any(|(_, attr)| attr.check.is_some()),
            attrs,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    pub fn defaults(&self) -> Option<&Map> {
        self.defaults.as_ref()
    }

    pub fn has_required(&self) -> bool {
        self.defaults.is_none()
    }

    /// A type's `computeAttrs`, which reads each attribute as `given && given[name]`: a falsy
    /// value gives the defaults where every attribute has one, and is every attribute
    /// otherwise, required or not.
    pub fn resolve<'g>(&'g self, given: &'g Given) -> Result<Computed<'g>> {
        let given = match (given, &self.defaults) {
            (Given::Falsy(_), Some(_)) => return Ok(Computed::Defaults),
            (Given::Falsy(value), None) => {
                return Ok(Computed::Values(
                    self.attrs
                        .iter()
                        .map(|(name, _)| (name, Some(value)))
                        .collect(),
                ));
            }
            (Given::Object(given), _) => given,
        };
        let mut built = Vec::with_capacity(self.attrs.len());
        for (name, attr) in &self.attrs {
            let value = match (given.get(name), &attr.default) {
                (Some(value), _) => Some(value),
                (None, Some(default)) => default.as_ref(),
                (None, None) => {
                    return Err(Error::Range(format!(
                        "No value supplied for attribute {name}"
                    )));
                }
            };
            built.push((name, value));
        }
        Ok(Computed::Values(built))
    }

    /// [`resolve`](Self::resolve), as an object.
    pub fn compute(&self, given: &Given) -> Result<Map> {
        Ok(match self.resolve(given)? {
            Computed::Defaults => self.defaults.clone().unwrap_or_default(),
            Computed::Values(values) => values
                .into_iter()
                .filter_map(|(name, value)| Some((name.clone(), value?.clone())))
                .collect(),
        })
    }

    /// Runs the attributes' checks on what [`resolve`](Self::resolve) computed. The computed
    /// attributes are the type's own, so only their values can be refused.
    pub fn check_computed(&self, computed: &Computed) -> Result<()> {
        match computed {
            Computed::Defaults => {
                let defaults = self.defaults.as_ref().expect("defaults");
                for (name, attr) in &self.attrs {
                    check_value(attr, defaults.get(name).map(Found::Json))?;
                }
            }
            Computed::Values(values) => {
                for ((_, attr), (_, value)) in self.attrs.iter().zip(values) {
                    check_value(attr, value.map(Found::Json))?;
                }
            }
        }
        Ok(())
    }

    /// The keys of the attributes written for what [`resolve`](Self::resolve) computed.
    pub fn keys<'g>(&'g self, computed: &'g Computed<'g>) -> AttrKeys<'g> {
        AttrKeys(match computed {
            Computed::Defaults => Keys::Map(self.defaults.as_ref().expect("defaults")),
            Computed::Values(values) => Keys::Values(values),
        })
    }

    /// Writes what [`resolve`](Self::resolve) computed: a value ref.
    pub fn write(&self, builder: &mut Builder, type_index: usize, computed: &Computed) -> u32 {
        match computed {
            Computed::Defaults => {
                builder.defaults(type_index, self.defaults.as_ref().expect("defaults"))
            }
            Computed::Values(values) => {
                if values.iter().all(|(_, value)| value.is_none()) {
                    return 0;
                }
                let written: Vec<(&str, u32)> = values
                    .iter()
                    .filter_map(|(name, value)| Some((name.as_str(), builder.value((*value)?))))
                    .collect();
                builder.object_of(written)
            }
        }
    }

    /// Raise an error for attributes the type doesn't have, or values it doesn't accept.
    pub fn check_map(&self, values: &Map, kind: &str, type_name: &str) -> Result<()> {
        self.check_keys(values.keys().map(|key| key.as_str()), kind, type_name)?;
        for (name, attr) in &self.attrs {
            check_value(attr, values.get(name).map(Found::Json))?;
        }
        Ok(())
    }

    /// [`check_map`](Self::check_map) of attributes a chunk holds.
    pub fn check_ref(&self, values: ValueRef, kind: &str, type_name: &str) -> Result<()> {
        // Value 0 is the empty object, which only a check can refuse.
        if values.index == 0 && !self.checked {
            return Ok(());
        }
        // Each attribute's value, found as the keys are checked, for the checks that follow.
        let (mut few, mut many);
        let found: &mut [Option<ValueRef>] = match self.attrs.len() {
            len @ ..=8 => {
                few = [None; 8];
                &mut few[..len]
            }
            len => {
                many = vec![None; len];
                &mut many
            }
        };
        for (at, (key, value)) in values.entries_bytes().enumerate() {
            // Attributes a type computed are written in its order.
            let index = match self.attrs.get(at) {
                Some((known, _)) if known.as_bytes() == key => Some(at),
                _ => self
                    .attrs
                    .iter()
                    .position(|(known, _)| known.as_bytes() == key),
            };
            match index {
                Some(index) => found[index] = Some(value),
                None => return Err(unsupported(key, kind, type_name)),
            }
        }
        if self.checked {
            for ((_, attr), value) in self.attrs.iter().zip(found.iter()) {
                check_value(attr, value.map(Found::Chunk))?;
            }
        }
        Ok(())
    }

    fn check_keys<'k>(
        &self,
        keys: impl Iterator<Item = &'k str>,
        kind: &str,
        type_name: &str,
    ) -> Result<()> {
        for attr in keys {
            if !self.attrs.iter().any(|(known, _)| known == attr) {
                return Err(Error::Range(format!(
                    "Unsupported attribute {attr} for {kind} of type {type_name}"
                )));
            }
        }
        Ok(())
    }
}

#[inline(always)]
fn check_value(attr: &Attribute, value: Option<Found>) -> Result<()> {
    match &attr.check {
        None => Ok(()),
        Some(Check::Hook(hook)) => match value {
            None => hook(None),
            Some(Found::Json(value)) => hook(Some(value)),
            Some(Found::Chunk(value)) => hook(Some(&value.to_value())),
        },
        Some(Check::Types(allowed, expected)) => {
            let type_of = match value {
                None => TypeOf::Undefined,
                Some(Found::Json(value)) => TypeOf::of(Some(value)),
                Some(Found::Chunk(value)) => value.type_of(),
            };
            match allowed & bit(type_of) {
                0 => Err(wrong_type(expected, type_of)),
                _ => Ok(()),
            }
        }
    }
}

#[cold]
fn wrong_type(expected: &str, type_of: TypeOf) -> Error {
    Error::Range(format!("{expected}, got {}", type_of.name()))
}

#[cold]
fn unsupported(key: &[u8], kind: &str, type_name: &str) -> Error {
    let key = String::from_utf8_lossy(key);
    Error::Range(format!(
        "Unsupported attribute {key} for {kind} of type {type_name}"
    ))
}

/// The attributes' defaults, when every attribute has one.
fn defaults(attrs: &[(Key, Attribute)]) -> Option<Map> {
    let mut defaults = Map::with_capacity(attrs.len());
    for (name, attr) in attrs {
        if let Some(value) = attr.default.clone()? {
            defaults.push(name.clone(), value);
        }
    }
    Some(defaults)
}
