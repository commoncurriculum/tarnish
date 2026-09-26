//! The attributes of node and mark types: their specs, their defaults, and the attributes nodes
//! and marks get from what they're given.

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::js::{self, Given};
use crate::json::{Key, Map, Value};

/// A node's or mark's attributes, shared by the nodes and marks that have the same ones.
pub type Attrs = Arc<Map>;

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

struct Attribute {
    default: Option<Option<Value>>,
    validate: Option<ValidateHook>,
}

/// A node or mark type's attributes.
pub(crate) struct AttrSet {
    attrs: Vec<(Key, Attribute)>,
    /// What nodes or marks given no attributes get, when every attribute has a default.
    defaults: Option<Attrs>,
}

impl AttrSet {
    pub fn new(type_name: &str, specs: &[(String, AttributeSpec)]) -> AttrSet {
        let attrs: Vec<(Key, Attribute)> = specs
            .iter()
            .map(|(name, spec)| {
                let validate = spec.validate.as_ref().map(|validate| match validate {
                    Validate::Hook(hook) => hook.clone(),
                    Validate::Types(types) => validate_type(type_name, name, types),
                });
                let attribute = Attribute {
                    default: spec.default.clone(),
                    validate,
                };
                (Key::from(name.as_str()), attribute)
            })
            .collect();
        AttrSet {
            defaults: defaults(&attrs),
            attrs,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    pub fn defaults(&self) -> Option<&Attrs> {
        self.defaults.as_ref()
    }

    pub fn has_required(&self) -> bool {
        self.defaults.is_none()
    }

    /// A type's `computeAttrs`, which reads each attribute as `given && given[name]`: a falsy
    /// value gives the defaults where every attribute has one, and is every attribute
    /// otherwise, required or not.
    pub fn compute(&self, given: &Given) -> Result<Attrs> {
        let given = match (given, &self.defaults) {
            (Given::Falsy(_), Some(defaults)) => return Ok(defaults.clone()),
            (Given::Falsy(value), None) => {
                let built = self
                    .attrs
                    .iter()
                    .map(|(name, _)| (name.clone(), value.clone()));
                return Ok(Arc::new(built.collect()));
            }
            (Given::Object(given), _) => given,
        };
        let mut built = Map::with_capacity(self.attrs.len());
        for (name, attr) in &self.attrs {
            let value = match (given.get(name), &attr.default) {
                (Some(value), _) => Some(value.clone()),
                (None, Some(default)) => default.clone(),
                (None, None) => {
                    return Err(Error::Range(format!(
                        "No value supplied for attribute {name}"
                    )));
                }
            };
            if let Some(value) = value {
                built.push(name.clone(), value);
            }
        }
        Ok(Arc::new(built))
    }

    /// Raise an error for attributes the type doesn't have, or values it doesn't accept.
    pub fn check(&self, values: &Map, kind: &str, type_name: &str) -> Result<()> {
        for attr in values.keys() {
            if !self.attrs.iter().any(|(known, _)| known == attr) {
                return Err(Error::Range(format!(
                    "Unsupported attribute {attr} for {kind} of type {type_name}"
                )));
            }
        }
        for (name, attr) in &self.attrs {
            if let Some(validate) = &attr.validate {
                validate(values.get(name))?;
            }
        }
        Ok(())
    }
}

/// The attributes' defaults, when every attribute has one.
fn defaults(attrs: &[(Key, Attribute)]) -> Option<Attrs> {
    let mut defaults = Map::with_capacity(attrs.len());
    for (name, attr) in attrs {
        if let Some(value) = attr.default.clone()? {
            defaults.push(name.clone(), value);
        }
    }
    Some(Arc::new(defaults))
}

fn validate_type(type_name: &str, attr_name: &str, types: &str) -> ValidateHook {
    let types: Vec<String> = types.split('|').map(str::to_owned).collect();
    let (type_name, attr_name) = (type_name.to_owned(), attr_name.to_owned());
    Arc::new(move |value: Option<&Value>| {
        let name = js::type_of(value);
        if types.iter().any(|allowed| allowed == name) {
            return Ok(());
        }
        Err(Error::Range(format!(
            "Expected value of type {} for attribute {attr_name} on type {type_name}, got {name}",
            types.join(",")
        )))
    })
}
