//! Attributes as `addAttributes` and `addGlobalAttributes` declare them, and
//! `getRenderedAttributes`.

use std::borrow::Cow;
use std::sync::Arc;

use tarnish::chunk::ValueRef;
use tarnish::{Error, Value};
use tarnish_html::HtmlNode;

use super::merge_attributes::merge_entry;
use crate::{AttrValue, SpecAttrs};

/// An attribute's `renderHTML`, given the node's or mark's attributes: it hands each entry of the
/// object it returns to [`Rendered::merge`], in the object's order.
pub type RenderAttribute =
    Arc<dyn for<'a> Fn(ValueRef<'a>, &mut Rendered<'a>) -> Result<(), Error> + Send + Sync>;
/// An attribute's `parseHTML`, given the element: `None` for `null` or `undefined`.
pub type ParseAttribute = Arc<dyn Fn(&HtmlNode) -> Option<Value> + Send + Sync>;

/// The HTML attributes `getRenderedAttributes` has merged so far.
pub struct Rendered<'a>(SpecAttrs<'a>);

impl<'a> Rendered<'a> {
    /// Merges an entry of an attribute's rendering as `mergeAttributes` does. An attribute's
    /// rendering is an object, whose keys are distinct.
    pub fn merge(
        &mut self,
        key: impl Into<Cow<'a, str>>,
        value: impl Into<AttrValue<'a>>,
    ) -> Result<(), Error> {
        merge_entry(&mut self.0, key.into(), value.into())
    }
}

#[derive(Clone)]
pub struct ExtensionAttribute {
    pub name: &'static str,
    /// `None` for a required attribute.
    pub default: Option<Value>,
    pub rendered: bool,
    pub render_html: Option<RenderAttribute>,
    pub parse_html: Option<ParseAttribute>,
}

impl ExtensionAttribute {
    pub fn new(name: &'static str, default: Value) -> Self {
        ExtensionAttribute {
            name,
            default: Some(default),
            rendered: true,
            render_html: None,
            parse_html: None,
        }
    }

    pub fn default(mut self, default: Value) -> Self {
        self.default = Some(default);
        self
    }

    pub fn not_rendered(mut self) -> Self {
        self.rendered = false;
        self
    }

    pub fn render_html(
        mut self,
        render: impl for<'a> Fn(ValueRef<'a>, &mut Rendered<'a>) -> Result<(), Error>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.render_html = Some(Arc::new(render));
        self
    }

    pub fn parse_html(
        mut self,
        parse: impl Fn(&HtmlNode) -> Option<Value> + Send + Sync + 'static,
    ) -> Self {
        self.parse_html = Some(Arc::new(parse));
        self
    }
}

/// One entry of `addGlobalAttributes`: attributes for the types it names.
pub struct GlobalAttributes {
    pub types: &'static [&'static str],
    pub attributes: Vec<ExtensionAttribute>,
}

/// `getRenderedAttributes(nodeOrMark, extensionAttributes)`, given the attributes of a node or
/// mark, which hold every one of its type's.
pub fn get_rendered_attributes<'a>(
    attrs: ValueRef<'a>,
    attributes: &[ExtensionAttribute],
) -> Result<SpecAttrs<'a>, Error> {
    let mut rendered = Rendered(SpecAttrs::new());
    for attribute in attributes.iter().filter(|attribute| attribute.rendered) {
        match &attribute.render_html {
            Some(render) => render(attrs, &mut rendered)?,
            None => rendered.merge(
                attribute.name,
                attrs.get(attribute.name).expect("its type's attribute"),
            )?,
        }
    }
    Ok(rendered.0)
}
