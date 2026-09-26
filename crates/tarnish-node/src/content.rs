//! Content matches.

use napi::bindgen_prelude::{FnArgs, Null, ToNapiValue, Unknown};
use napi::{Env, Result};
use napi_derive::napi;
use tarnish::ContentMatch;

use crate::fragment::{self, FragmentHandle};
use crate::js::{self, OrThrow};
use crate::schema::{self, NodeTypeHandle, SchemaHandle};

/// The wrapper of the match: one per state of an expression's automaton.
pub fn wrap<'env>(env: &'env Env, content_match: &ContentMatch) -> Result<Unknown<'env>> {
    let (automaton, state) = content_match.id();
    let handle = ContentMatchHandle {
        content_match: content_match.clone(),
    };
    let key = format!("{automaton}:{state}");
    js::call_registered(env, "wrapContentMatch", FnArgs::from((handle, key)))
}

fn wrap_option<'env>(env: &'env Env, content_match: Option<ContentMatch>) -> Result<Unknown<'env>> {
    match content_match {
        Some(content_match) => wrap(env, &content_match),
        None => Null.into_unknown(env),
    }
}

#[napi(object)]
pub struct Edge<'env> {
    #[napi(js_name = "type")]
    pub node_type: Unknown<'env>,
    pub next: Unknown<'env>,
}

#[napi]
pub struct ContentMatchHandle {
    pub(crate) content_match: ContentMatch,
}

#[napi]
impl ContentMatchHandle {
    #[napi(getter)]
    pub fn valid_end(&self) -> bool {
        self.content_match.valid_end()
    }

    #[napi(getter)]
    pub fn inline_content(&self) -> bool {
        self.content_match.inline_content()
    }

    #[napi(getter)]
    pub fn edge_count(&self) -> u32 {
        self.content_match.edge_count() as u32
    }

    #[napi]
    pub fn match_type<'env>(
        &self,
        env: &'env Env,
        node_type: &NodeTypeHandle,
    ) -> Result<Unknown<'env>> {
        wrap_option(env, self.content_match.match_type(&node_type.node_type))
    }

    #[napi]
    pub fn match_fragment<'env>(
        &self,
        env: &'env Env,
        fragment: &FragmentHandle,
        start: u32,
        end: u32,
    ) -> Result<Unknown<'env>> {
        let matched =
            self.content_match
                .match_fragment(&fragment.fragment, start as usize, end as usize);
        wrap_option(env, matched)
    }

    #[napi]
    pub fn default_type<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        match self.content_match.default_type() {
            Some(node_type) => schema::wrap_node_type(env, &node_type),
            None => Null.into_unknown(env),
        }
    }

    #[napi]
    pub fn compatible(&self, other: &ContentMatchHandle) -> bool {
        self.content_match.compatible(&other.content_match)
    }

    #[napi]
    pub fn fill_before<'env>(
        &self,
        env: &'env Env,
        after: &FragmentHandle,
        to_end: bool,
        start_index: u32,
    ) -> Result<Unknown<'env>> {
        let filled = self
            .content_match
            .fill_before(&after.fragment, to_end, start_index as usize)
            .or_throw(env)?;
        match filled {
            Some(filled) => fragment::wrap(env, &filled),
            None => Null.into_unknown(env),
        }
    }

    #[napi]
    pub fn find_wrapping<'env>(
        &self,
        env: &'env Env,
        target: &NodeTypeHandle,
    ) -> Result<Option<Vec<Unknown<'env>>>> {
        let Some(wrapping) = self.content_match.find_wrapping(&target.node_type) else {
            return Ok(None);
        };
        let wrapping = wrapping
            .iter()
            .map(|node_type| schema::wrap_node_type(env, node_type))
            .collect::<Result<_>>()?;
        Ok(Some(wrapping))
    }

    #[napi]
    pub fn edge<'env>(&self, env: &'env Env, n: u32) -> Result<Edge<'env>> {
        let (node_type, next) = self.content_match.edge(n as usize).or_throw(env)?;
        Ok(Edge {
            node_type: schema::wrap_node_type(env, &node_type)?,
            next: wrap(env, &next)?,
        })
    }

    #[napi]
    pub fn to_debug_string(&self) -> String {
        self.content_match.to_string()
    }
}

#[napi]
pub fn content_match_parse<'env>(
    env: &'env Env,
    schema: &SchemaHandle,
    expr: String,
) -> Result<Unknown<'env>> {
    let parsed = ContentMatch::parse(&schema.schema, &expr).or_throw(env)?;
    wrap(env, &parsed)
}
