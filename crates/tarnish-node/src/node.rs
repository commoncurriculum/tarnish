use napi::bindgen_prelude::{FnArgs, Function, Null, ToNapiValue, Unknown, Utf16String};
use napi::{Env, JsString, JsValue, Result, ValueType};
use napi_derive::napi;
use tarnish::{ChildAt, Marks, Node, Text};

use crate::content::Place;
use crate::fragment::{self, FragmentHandle};
use crate::js::{self, OrThrow};
use crate::mark::{self, MarkHandle};
use crate::schema::{self, MarkTypeHandle, NodeTypeHandle};
use crate::slice::{self, SliceHandle};

pub fn wrap<'env>(env: &'env Env, node: &Node<'static>) -> Result<Unknown<'env>> {
    let handle = NodeHandle { node: node.clone() };
    let (chunk, index) = node.id();
    let key = format!("{chunk}:{index}");
    js::call_registered(env, "wrapNode", FnArgs::from((handle, key)))
}

pub fn wrap_option<'env>(env: &'env Env, node: Option<&Node<'static>>) -> Result<Unknown<'env>> {
    match node {
        Some(node) => wrap(env, node),
        None => Null.into_unknown(env),
    }
}

/// A node a schema's hook is given, which may be in chunks borrowed for less than JavaScript
/// keeps a wrapper: a copy of it.
pub fn detach(node: &Node) -> Node<'static> {
    node.compact()
}

/// A JavaScript `(node, pos, parent, index)` callback as tarnish calls it: going on into a
/// node's children unless it returns `false`.
pub fn visitor(
    f: Function,
) -> impl FnMut(&Node<'static>, usize, Option<&Node<'static>>, usize) -> tarnish::Result<bool> {
    move |node, pos, parent, index| {
        js::host(|env| {
            let args = (
                wrap(env, node)?,
                pos as f64,
                wrap_option(env, parent)?,
                index as f64,
            );
            Ok(!js::is_false(&js::call(
                f.to_unknown(),
                FnArgs::from(args),
            )?)?)
        })
    }
}

/// `textBetween`'s `leafText`, a string or a function of the node, as tarnish calls it.
pub fn leaf_text(
    leaf_text: Option<Unknown>,
) -> Result<Option<impl FnMut(&Node<'static>) -> tarnish::Result<Text>>> {
    let Some(leaf_text) = leaf_text else {
        return Ok(None);
    };
    if !leaf_text.coerce_to_bool()? {
        return Ok(None);
    }
    let is_function = leaf_text.get_type()? == ValueType::Function;
    Ok(Some(move |node: &Node<'static>| {
        js::host(|env| {
            let text = match is_function {
                true => js::call(leaf_text, wrap(env, node)?)?,
                false => leaf_text,
            };
            js::text_from_js(text.coerce_to_string()?.to_unknown())
        })
    }))
}

/// A block separator as `textBetween` takes one: only a string separates.
pub fn separator(separator: Option<Unknown>) -> Result<Option<Text>> {
    match separator {
        Some(separator) if separator.get_type()? == ValueType::String => {
            js::text_from_js(separator).map(Some)
        }
        _ => Ok(None),
    }
}

#[napi(object)]
pub struct ChildInfo<'env> {
    pub node: Unknown<'env>,
    pub index: u32,
    pub offset: u32,
}

fn child_info<'env>(env: &'env Env, child: ChildAt<'static>) -> Result<ChildInfo<'env>> {
    Ok(ChildInfo {
        node: wrap_option(env, child.node.as_ref())?,
        index: child.index as u32,
        offset: child.offset as u32,
    })
}

#[napi]
pub struct NodeHandle {
    pub(crate) node: Node<'static>,
}

#[napi]
impl NodeHandle {
    #[napi]
    pub fn node_type<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        schema::wrap_node_type(env, &self.node.node_type())
    }

    #[napi]
    pub fn attrs<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        js::attrs_to_js(env, &self.node.attrs())
    }

    #[napi]
    pub fn content<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        fragment::wrap(env, self.node.content())
    }

    #[napi]
    pub fn marks<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        mark::wrap_set(env, &self.node.marks())
    }

    #[napi]
    pub fn text<'env>(&self, env: &'env Env) -> Result<Option<JsString<'env>>> {
        self.node
            .text()
            .map(|text| js::text_ref_to_js(env, text))
            .transpose()
    }

    #[napi(getter)]
    pub fn is_text(&self) -> bool {
        self.node.is_text()
    }

    #[napi(getter)]
    pub fn node_size(&self) -> u32 {
        self.node.node_size() as u32
    }

    #[napi]
    pub fn nodes_between(
        &self,
        env: &Env,
        from: f64,
        to: f64,
        f: Function,
        start_pos: f64,
    ) -> Result<()> {
        let (from, to, start_pos) = (
            js::pos(env, from)?,
            js::pos(env, to)?,
            js::pos(env, start_pos)?,
        );
        self.node
            .nodes_between(from, to, &mut visitor(f), start_pos)
            .or_throw(env)
    }

    #[napi]
    pub fn text_content<'env>(&self, env: &'env Env) -> Result<JsString<'env>> {
        let text = self.node.text_content().or_throw(env)?;
        js::text_to_js(env, &text)
    }

    #[napi]
    pub fn text_between<'env>(
        &self,
        env: &'env Env,
        from: f64,
        to: f64,
        block_separator: Option<Unknown>,
        leaf_text: Option<Unknown>,
    ) -> Result<JsString<'env>> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        let separator = separator(block_separator)?;
        let mut leaf_text = self::leaf_text(leaf_text)?;
        let leaf_text = leaf_text
            .as_mut()
            .map(|f| f as &mut tarnish::LeafTextHook<'_, 'static>);
        let text = self
            .node
            .text_between(from, to, separator.as_ref(), leaf_text)
            .or_throw(env)?;
        js::text_to_js(env, &text)
    }

    #[napi]
    pub fn eq(&self, other: &NodeHandle) -> bool {
        self.node == other.node
    }

    #[napi]
    pub fn same_markup(&self, other: &NodeHandle) -> bool {
        self.node.same_markup(&other.node)
    }

    #[napi]
    pub fn has_markup(
        &self,
        node_type: &NodeTypeHandle,
        attrs: Unknown,
        marks: Option<Vec<&MarkHandle>>,
    ) -> Result<bool> {
        let attrs = js::attrs_from_js(attrs)?;
        let marks = marks.map(mark::list);
        Ok(self
            .node
            .has_markup(&node_type.node_type(), attrs.as_ref(), marks.as_deref()))
    }

    #[napi]
    pub fn copy<'env>(&self, env: &'env Env, content: &FragmentHandle) -> Result<Unknown<'env>> {
        wrap(env, &self.node.copy(content.fragment.clone()))
    }

    /// The node with these marks, or itself when they are its own, as ProseMirror gives it
    /// back when it is passed its own array of them.
    #[napi]
    pub fn mark<'env>(&self, env: &'env Env, marks: Vec<&MarkHandle>) -> Result<Unknown<'env>> {
        let marks = mark::list(marks);
        let own = self.node.marks();
        if own.len() == marks.len() && own.iter().zip(&marks).all(|(own, mark)| own.ptr_eq(mark)) {
            return wrap(env, &self.node);
        }
        wrap(env, &self.node.mark(Marks::from_list(&marks)))
    }

    #[napi]
    pub fn with_text<'env>(&self, env: &'env Env, text: Utf16String) -> Result<Unknown<'env>> {
        let node = self.node.with_text(Text::from_units(&text)).or_throw(env)?;
        wrap(env, &node)
    }

    #[napi]
    pub fn cut<'env>(&self, env: &'env Env, from: f64, to: f64) -> Result<Unknown<'env>> {
        let node = self
            .node
            .cut(js::pos(env, from)?, js::pos(env, to)?)
            .or_throw(env)?;
        wrap(env, &node)
    }

    #[napi]
    pub fn slice<'env>(
        &self,
        env: &'env Env,
        from: f64,
        to: f64,
        include_parents: bool,
    ) -> Result<Unknown<'env>> {
        let slice = self
            .node
            .slice(js::pos(env, from)?, js::pos(env, to)?, include_parents)
            .or_throw(env)?;
        slice::wrap(env, &slice)
    }

    #[napi]
    pub fn replace<'env>(
        &self,
        env: &'env Env,
        from: f64,
        to: f64,
        slice: &SliceHandle,
    ) -> Result<Unknown<'env>> {
        let node = self
            .node
            .replace(js::pos(env, from)?, js::pos(env, to)?, &slice.slice)
            .or_throw(env)?;
        wrap(env, &node)
    }

    #[napi]
    pub fn node_at<'env>(&self, env: &'env Env, pos: f64) -> Result<Unknown<'env>> {
        let node = self.node.node_at(js::pos(env, pos)?).or_throw(env)?;
        wrap_option(env, node.as_ref())
    }

    #[napi]
    pub fn child_after<'env>(&self, env: &'env Env, pos: f64) -> Result<ChildInfo<'env>> {
        let child = self.node.child_after(js::pos(env, pos)?).or_throw(env)?;
        child_info(env, child)
    }

    #[napi]
    pub fn child_before<'env>(&self, env: &'env Env, pos: f64) -> Result<ChildInfo<'env>> {
        let child = self.node.child_before(js::pos(env, pos)?).or_throw(env)?;
        child_info(env, child)
    }

    #[napi]
    pub fn resolve<'env>(&self, env: &'env Env, pos: f64) -> Result<Unknown<'env>> {
        let resolved = self.node.resolve(js::pos(env, pos)?).or_throw(env)?;
        crate::position::wrap(env, resolved)
    }

    #[napi]
    pub fn range_has_mark(&self, env: &Env, from: f64, to: f64, mark: &MarkHandle) -> Result<bool> {
        self.node
            .range_has_mark(js::pos(env, from)?, js::pos(env, to)?, &mark.mark)
            .or_throw(env)
    }

    #[napi]
    pub fn range_has_mark_type(
        &self,
        env: &Env,
        from: f64,
        to: f64,
        mark_type: &MarkTypeHandle,
    ) -> Result<bool> {
        self.node
            .range_has_mark_type(
                js::pos(env, from)?,
                js::pos(env, to)?,
                &mark_type.mark_type(),
            )
            .or_throw(env)
    }

    #[napi]
    pub fn to_debug_string(&self, env: &Env) -> Result<String> {
        self.node.to_debug_string().or_throw(env)
    }

    #[napi]
    pub fn content_match_at<'env>(&self, env: &'env Env, index: u32) -> Result<Unknown<'env>> {
        let found = self.node.content_match_at(index as usize).or_throw(env)?;
        let place = Place::of_type(self.node.schema(), self.node.node_type().index());
        crate::content::wrap(env, &place, &found)
    }

    #[napi]
    pub fn can_replace(
        &self,
        env: &Env,
        from: u32,
        to: u32,
        replacement: &FragmentHandle,
        start: u32,
        end: u32,
    ) -> Result<bool> {
        self.node
            .can_replace(
                from as usize,
                to as usize,
                &replacement.fragment,
                start as usize,
                end as usize,
            )
            .or_throw(env)
    }

    #[napi]
    pub fn can_replace_with(
        &self,
        env: &Env,
        from: u32,
        to: u32,
        node_type: &NodeTypeHandle,
        marks: Option<Vec<&MarkHandle>>,
    ) -> Result<bool> {
        let marks = marks.map(mark::list);
        self.node
            .can_replace_with(
                from as usize,
                to as usize,
                &node_type.node_type(),
                marks.as_deref(),
            )
            .or_throw(env)
    }

    #[napi]
    pub fn can_append(&self, env: &Env, other: &NodeHandle) -> Result<bool> {
        self.node.can_append(&other.node).or_throw(env)
    }

    #[napi]
    pub fn check(&self, env: &Env) -> Result<()> {
        self.node.check().or_throw(env)
    }

    #[napi]
    pub fn to_json<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        js::value_to_js(env, &self.node.to_json())
    }
}
