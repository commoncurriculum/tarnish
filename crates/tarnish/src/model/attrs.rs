//! The attributes of node and mark types: their specs, their defaults, and the attributes nodes
//! and marks get from what they're given.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use super::compare_deep::deep_equal;
use super::read::{AttrKeys, Given, Keys, Property};
use crate::chunk::{Builder, Chunk, EMPTY_OBJECT, JsonView, Kind, ValueRef};
use crate::js::TypeOf;
use crate::json::{EMPTY, Key, Map, Value};
use crate::{Error, Result};

/// A function that raises an error for an attribute value it doesn't accept, `None` being
/// `undefined`.
pub type ValidateHook = Arc<dyn Fn(Option<&Value>) -> Result<()> + Send + Sync>;

#[derive(Clone, Default)]
pub struct AttributeSpec {
    pub default: AttributeDefault,
    pub validate: Option<Validate>,
}

#[derive(Clone, Default)]
pub enum AttributeDefault {
    /// No default: nodes and marks must be given the attribute.
    #[default]
    Required,
    /// `undefined`, which a node or mark can't hold: it leaves the attribute out, where
    /// ProseMirror keeps the name with no value, so its `toJSON` writes `"attrs": {}` and
    /// `hasMarkup` sees the name.
    Undefined,
    Value(Value),
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
    default: AttributeDefault,
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
        deep_equal(self.view(), other.view())
    }
}

impl fmt::Debug for Attrs<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.view(), f)
    }
}

/// A node or mark type's attributes.
pub(crate) struct AttrSet {
    names: Vec<Key>,
    /// Each attribute, in the place of its name.
    attrs: Vec<Attribute>,
    /// What nodes or marks given no attributes get, when every attribute has a default.
    defaults: Option<Map>,
    /// Whether any attribute has a check.
    checked: bool,
    /// The type, as an error names it: `node of type paragraph`.
    owner: String,
    /// Where the type's attributes are among the schema's, for a builder to keep their keys by.
    slot: usize,
}

/// The attributes computed from what a type is given: its defaults, or each attribute's value,
/// `None` for one whose default is `undefined`.
pub(crate) enum Computed<'g> {
    Defaults,
    Values(Vec<(&'g Key, Option<PropertyRef<'g>>)>),
}

/// A [`Property`] or a [`Value`], borrowed, as an attribute's value.
#[derive(Clone, Copy)]
pub(crate) enum PropertyRef<'v> {
    Text(&'v str),
    Value(&'v Value),
}

impl<'v> JsonView<'v> for PropertyRef<'v> {
    fn kind(self) -> Kind<'v> {
        match self {
            PropertyRef::Text(text) => Kind::String(text),
            PropertyRef::Value(value) => value.kind(),
        }
    }

    fn items(self) -> impl ExactSizeIterator<Item = Self> {
        let items: &[Value] = match self {
            PropertyRef::Value(Value::Array(items)) => items,
            _ => &[],
        };
        items.iter().map(PropertyRef::Value)
    }

    fn entries(self) -> impl ExactSizeIterator<Item = (&'v str, Self)> {
        let object = match self {
            PropertyRef::Value(Value::Object(object)) => object,
            _ => &EMPTY,
        };
        object
            .iter()
            .map(|(key, value)| (key.as_str(), PropertyRef::Value(value)))
    }

    fn to_value(self) -> Cow<'v, Value> {
        match self {
            PropertyRef::Text(text) => Cow::Owned(Value::String(text.into())),
            PropertyRef::Value(value) => Cow::Borrowed(value),
        }
    }
}

impl Property<'_> {
    fn as_ref(&self) -> PropertyRef<'_> {
        match self {
            Property::Text(text) => PropertyRef::Text(text),
            Property::Value(value) => PropertyRef::Value(value),
        }
    }
}

impl AttrSet {
    /// A type's attributes, `kind` being `node` or `mark`.
    pub fn new(
        kind: &str,
        type_name: &str,
        specs: &[(String, AttributeSpec)],
        slot: usize,
    ) -> AttrSet {
        let (names, attrs): (Vec<Key>, Vec<Attribute>) = specs
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
            .unzip();
        AttrSet {
            defaults: defaults(&names, &attrs),
            checked: attrs.iter().any(|attr| attr.check.is_some()),
            names,
            attrs,
            owner: format!("{kind} of type {type_name}"),
            slot,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    /// The attributes' names, in order.
    pub fn names(&self) -> &[Key] {
        &self.names
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
        match (given, &self.defaults) {
            (Given::Falsy(_), Some(_)) => Ok(Computed::Defaults),
            (Given::Falsy(value), None) => Ok(Computed::Values(
                self.names
                    .iter()
                    .map(|name| (name, Some(PropertyRef::Value(value))))
                    .collect(),
            )),
            (Given::Object(given), _) => {
                let mut next = 0;
                self.values(|name, _| given.get_from(name, &mut next).map(PropertyRef::Value))
            }
            (Given::Named(given), _) => {
                self.values(|_, index| given.get(index)?.as_ref().map(Property::as_ref))
            }
        }
    }

    /// Each attribute's value: the one `given` gives for its name and place, or its default.
    fn values<'g>(
        &'g self,
        mut given: impl FnMut(&Key, usize) -> Option<PropertyRef<'g>>,
    ) -> Result<Computed<'g>> {
        let mut built = Vec::with_capacity(self.attrs.len());
        for (index, (name, attr)) in self.names.iter().zip(&self.attrs).enumerate() {
            let value = match (given(name, index), &attr.default) {
                (Some(value), _) => Some(value),
                (None, AttributeDefault::Value(value)) => Some(PropertyRef::Value(value)),
                (None, AttributeDefault::Undefined) => None,
                (None, AttributeDefault::Required) => return Err(no_value(name)),
            };
            built.push((name, value));
        }
        Ok(Computed::Values(built))
    }

    /// Fail as [`resolve`](Self::resolve) would, without computing the attributes.
    pub fn check_given(&self, given: &Given) -> Result<()> {
        if !self.has_required() {
            return Ok(());
        }
        // A falsy value is every attribute's.
        let has = |index: usize, name: &Key| match given {
            Given::Falsy(_) => true,
            Given::Object(given) => given.contains_key(name),
            Given::Named(given) => given.get(index).is_some_and(Option::is_some),
        };
        let missing =
            self.names
                .iter()
                .zip(&self.attrs)
                .enumerate()
                .find(|&(index, (name, attr))| {
                    matches!(attr.default, AttributeDefault::Required) && !has(index, name)
                });
        match missing {
            Some((_, (name, _))) => Err(no_value(name)),
            None => Ok(()),
        }
    }

    /// [`resolve`](Self::resolve), as an object.
    pub fn compute(&self, given: &Given) -> Result<Map> {
        Ok(match self.resolve(given)? {
            Computed::Defaults => self.defaults.clone().unwrap_or_default(),
            Computed::Values(values) => values
                .into_iter()
                .filter_map(|(name, value)| Some((name.clone(), value?.to_value().into_owned())))
                .collect(),
        })
    }

    /// Runs the attributes' checks on what [`resolve`](Self::resolve) computed. The computed
    /// attributes are the type's own, so only their values can be refused.
    pub fn check_computed(&self, computed: &Computed) -> Result<()> {
        match computed {
            Computed::Defaults => {
                let defaults = self.defaults.as_ref().expect("defaults");
                for (name, attr) in self.names.iter().zip(&self.attrs) {
                    check_value(attr, defaults.get(name))?;
                }
            }
            Computed::Values(values) => {
                for (attr, (_, value)) in self.attrs.iter().zip(values) {
                    check_value(attr, *value)?;
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
                    return EMPTY_OBJECT;
                }
                let names = self.names.iter().map(Key::as_str);
                builder.attrs(self.slot, names, values.iter().map(|&(_, value)| value))
            }
        }
    }

    /// Raise an error for attributes the type doesn't have, or values it doesn't accept.
    pub fn check_map(&self, values: &Map) -> Result<()> {
        self.check(values.iter().map(|(key, value)| (key.as_bytes(), value)))
    }

    /// [`check_map`](Self::check_map) of attributes a chunk holds.
    pub fn check_ref(&self, values: ValueRef) -> Result<()> {
        // Only a check can refuse the empty object.
        if values.index == EMPTY_OBJECT && !self.checked {
            return Ok(());
        }
        self.check(values.entries_bytes())
    }

    fn check<'v, V: JsonView<'v>>(
        &self,
        values: impl Iterator<Item = (&'v [u8], V)>,
    ) -> Result<()> {
        // Each attribute's value, found as the keys are checked, for the checks that follow.
        let (mut few, mut many);
        let found: &mut [Option<V>] = match self.attrs.len() {
            len @ ..=8 => {
                few = [None; 8];
                &mut few[..len]
            }
            len => {
                many = vec![None; len];
                &mut many
            }
        };
        for (at, (key, value)) in values.enumerate() {
            // Attributes a type computed are written in its order.
            let index = match self.names.get(at) {
                Some(known) if known.as_bytes() == key => Some(at),
                _ => self.names.iter().position(|known| known.as_bytes() == key),
            };
            match index {
                Some(index) => found[index] = Some(value),
                None => return Err(self.unsupported(key)),
            }
        }
        if self.checked {
            for (attr, value) in self.attrs.iter().zip(found.iter()) {
                check_value(attr, *value)?;
            }
        }
        Ok(())
    }

    #[cold]
    fn unsupported(&self, key: &[u8]) -> Error {
        let key = String::from_utf8_lossy(key);
        Error::Range(format!("Unsupported attribute {key} for {}", self.owner))
    }
}

#[inline(always)]
fn check_value<'v>(attr: &Attribute, value: Option<impl JsonView<'v>>) -> Result<()> {
    match &attr.check {
        None => Ok(()),
        Some(Check::Hook(hook)) => hook(value.map(JsonView::to_value).as_deref()),
        Some(Check::Types(allowed, expected)) => {
            let type_of = value.map_or(TypeOf::Undefined, JsonView::type_of);
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
fn no_value(name: &str) -> Error {
    Error::Range(format!("No value supplied for attribute {name}"))
}

/// The attributes' defaults, when every attribute has one.
fn defaults(names: &[Key], attrs: &[Attribute]) -> Option<Map> {
    let mut defaults = Map::with_capacity(attrs.len());
    for (name, attr) in names.iter().zip(attrs) {
        match &attr.default {
            AttributeDefault::Required => return None,
            AttributeDefault::Undefined => {}
            AttributeDefault::Value(value) => defaults.push(name.clone(), value.clone()),
        }
    }
    Some(defaults)
}
