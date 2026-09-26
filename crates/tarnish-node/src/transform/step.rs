//! Steps, which JavaScript makes into a handle the first time it passes one to the bridge.

use napi::bindgen_prelude::{Either, FnArgs, Null, ToNapiValue, Unknown};
use napi::{Env, Result};
use napi_derive::napi;
use tarnish::transform::{Step, StepResult};

use super::map::{self, MappingHandle, StepMapHandle};
use crate::js::{self, OrThrow};
use crate::mark::{self, MarkHandle};
use crate::node::{self, NodeHandle};
use crate::schema::SchemaHandle;
use crate::slice::{self, SliceHandle};

/// A step as JavaScript's class of it, made from the arguments its constructor takes.
pub fn wrap<'env>(env: &'env Env, step: &Step) -> Result<Unknown<'env>> {
    let id = step.json_id();
    let make = "makeStep";
    match step {
        Step::Replace {
            from,
            to,
            slice,
            structure,
        } => {
            let args = (
                id,
                *from as f64,
                *to as f64,
                slice::wrap(env, slice)?,
                *structure,
            );
            js::call_registered(env, make, FnArgs::from(args))
        }
        Step::ReplaceAround {
            from,
            to,
            gap_from,
            gap_to,
            slice,
            insert,
            structure,
        } => {
            let args = (
                id,
                *from as f64,
                *to as f64,
                *gap_from as f64,
                *gap_to as f64,
                slice::wrap(env, slice)?,
                *insert as f64,
                *structure,
            );
            js::call_registered(env, make, FnArgs::from(args))
        }
        Step::AddMark { from, to, mark } | Step::RemoveMark { from, to, mark } => {
            let args = (id, *from as f64, *to as f64, mark::wrap(env, mark)?);
            js::call_registered(env, make, FnArgs::from(args))
        }
        Step::AddNodeMark { pos, mark } | Step::RemoveNodeMark { pos, mark } => {
            let args = (id, *pos as f64, mark::wrap(env, mark)?);
            js::call_registered(env, make, FnArgs::from(args))
        }
        Step::Attr { pos, attr, value } => {
            let value = js::optional_to_js(env, value.as_ref())?;
            js::call_registered(
                env,
                make,
                FnArgs::from((id, *pos as f64, attr.as_str(), value)),
            )
        }
        Step::DocAttr { attr, value } => {
            let value = js::optional_to_js(env, value.as_ref())?;
            js::call_registered(env, make, FnArgs::from((id, attr.as_str(), value)))
        }
    }
}

pub fn wrap_option<'env>(env: &'env Env, step: Option<Step>) -> Result<Unknown<'env>> {
    match step {
        Some(step) => wrap(env, &step),
        None => Null.into_unknown(env),
    }
}

pub fn wrap_result<'env>(env: &'env Env, result: &StepResult) -> Result<Unknown<'env>> {
    let (doc, failed) = match result {
        StepResult::Ok(doc) => (node::wrap(env, doc)?, None),
        StepResult::Failed(message) => (Null.into_unknown(env)?, Some(message.as_str())),
    };
    js::call_registered(env, "makeStepResult", FnArgs::from((doc, failed)))
}

#[napi]
pub struct StepHandle {
    pub(crate) step: Step,
}

#[napi]
impl StepHandle {
    #[napi(factory)]
    pub fn replace(
        env: &Env,
        from: f64,
        to: f64,
        slice: &SliceHandle,
        structure: bool,
    ) -> Result<Self> {
        Ok(StepHandle {
            step: Step::Replace {
                from: js::pos(env, from)?,
                to: js::pos(env, to)?,
                slice: slice.slice.clone(),
                structure,
            },
        })
    }

    #[napi(factory)]
    #[allow(clippy::too_many_arguments)]
    pub fn replace_around(
        env: &Env,
        from: f64,
        to: f64,
        gap_from: f64,
        gap_to: f64,
        slice: &SliceHandle,
        insert: f64,
        structure: bool,
    ) -> Result<Self> {
        Ok(StepHandle {
            step: Step::ReplaceAround {
                from: js::pos(env, from)?,
                to: js::pos(env, to)?,
                gap_from: js::pos(env, gap_from)?,
                gap_to: js::pos(env, gap_to)?,
                slice: slice.slice.clone(),
                insert: js::pos(env, insert)?,
                structure,
            },
        })
    }

    #[napi(factory)]
    pub fn add_mark(env: &Env, from: f64, to: f64, mark: &MarkHandle) -> Result<Self> {
        Ok(StepHandle {
            step: Step::AddMark {
                from: js::pos(env, from)?,
                to: js::pos(env, to)?,
                mark: mark.mark.clone(),
            },
        })
    }

    #[napi(factory)]
    pub fn remove_mark(env: &Env, from: f64, to: f64, mark: &MarkHandle) -> Result<Self> {
        Ok(StepHandle {
            step: Step::RemoveMark {
                from: js::pos(env, from)?,
                to: js::pos(env, to)?,
                mark: mark.mark.clone(),
            },
        })
    }

    #[napi(factory)]
    pub fn add_node_mark(env: &Env, pos: f64, mark: &MarkHandle) -> Result<Self> {
        Ok(StepHandle {
            step: Step::AddNodeMark {
                pos: js::pos(env, pos)?,
                mark: mark.mark.clone(),
            },
        })
    }

    #[napi(factory)]
    pub fn remove_node_mark(env: &Env, pos: f64, mark: &MarkHandle) -> Result<Self> {
        Ok(StepHandle {
            step: Step::RemoveNodeMark {
                pos: js::pos(env, pos)?,
                mark: mark.mark.clone(),
            },
        })
    }

    #[napi(factory)]
    pub fn attr(env: &Env, pos: f64, attr: String, value: Unknown) -> Result<Self> {
        Ok(StepHandle {
            step: Step::Attr {
                pos: js::pos(env, pos)?,
                attr,
                value: js::value_from_js(value)?,
            },
        })
    }

    #[napi(factory)]
    pub fn doc_attr(attr: String, value: Unknown) -> Result<Self> {
        Ok(StepHandle {
            step: Step::DocAttr {
                attr,
                value: js::value_from_js(value)?,
            },
        })
    }

    #[napi]
    pub fn apply<'env>(&self, env: &'env Env, doc: &NodeHandle) -> Result<Unknown<'env>> {
        let result = self.step.apply(&doc.node).or_throw(env)?;
        wrap_result(env, &result)
    }

    #[napi]
    pub fn get_map<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        map::wrap_step_map(env, &self.step.get_map())
    }

    #[napi]
    pub fn invert<'env>(&self, env: &'env Env, doc: &NodeHandle) -> Result<Unknown<'env>> {
        let inverted = self.step.invert(&doc.node).or_throw(env)?;
        wrap(env, &inverted)
    }

    #[napi]
    pub fn map<'env>(
        &self,
        env: &'env Env,
        mapping: Either<&StepMapHandle, &MappingHandle>,
    ) -> Result<Unknown<'env>> {
        let mapped = map::with_mappable(mapping, |mapping| self.step.map(mapping));
        wrap_option(env, mapped)
    }

    #[napi]
    pub fn merge<'env>(&self, env: &'env Env, other: &StepHandle) -> Result<Unknown<'env>> {
        wrap_option(env, self.step.merge(&other.step))
    }

    #[napi]
    pub fn to_json<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        js::value_to_js(env, &self.step.to_json())
    }
}

#[napi]
pub fn step_from_json<'env>(
    env: &'env Env,
    schema: &SchemaHandle,
    json: Unknown,
) -> Result<Unknown<'env>> {
    let step = Step::from_json(&schema.schema, &js::json_from_js(json)?).or_throw(env)?;
    wrap(env, &step)
}

#[napi]
pub fn step_result_from_replace<'env>(
    env: &'env Env,
    doc: &NodeHandle,
    from: f64,
    to: f64,
    slice: &SliceHandle,
) -> Result<Unknown<'env>> {
    let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
    let result = StepResult::from_replace(&doc.node, from, to, &slice.slice).or_throw(env)?;
    wrap_result(env, &result)
}
