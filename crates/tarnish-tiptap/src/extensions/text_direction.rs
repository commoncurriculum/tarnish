//! `@tiptap/core`'s `TextDirection`: a `dir` attribute on every node type but text, once a
//! direction is set. Tiptap's `Editor` adds it from its `textDirection` option; with no editor
//! here, an application adds it. Its plugin, which sets the editor's own element's `dir`, is an
//! editor's.

use tarnish_js::json::Value;

use crate::{ExtensionAttribute, GlobalAttributes, GlobalTypes, PlainExtension};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Ltr,
    Rtl,
    Auto,
}

impl Direction {
    pub fn name(self) -> &'static str {
        match self {
            Direction::Ltr => "ltr",
            Direction::Rtl => "rtl",
            Direction::Auto => "auto",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TextDirectionOptions {
    /// `None` for `undefined`, which gives no type the attribute.
    pub direction: Option<Direction>,
}

pub fn text_direction(options: TextDirectionOptions) -> PlainExtension {
    let extension = PlainExtension::create("textDirection");
    let Some(direction) = options.direction.map(Direction::name) else {
        return extension;
    };
    let dir = ExtensionAttribute::new("dir", Value::String(direction.into()))
        .parse_html(move |element| {
            let dir = element.attribute("dir");
            let dir = dir.filter(|dir| matches!(dir.as_str(), "ltr" | "rtl" | "auto"));
            Some(Value::String(
                dir.map_or_else(|| direction.into(), Into::into),
            ))
        })
        .render_html(|attributes, rendered| match attributes.get("dir") {
            Some(dir) if dir.truthy() => rendered.merge("dir", dir),
            _ => Ok(()),
        });
    extension.add_global_attributes(move |_| {
        vec![GlobalAttributes {
            types: GlobalTypes::Nodes,
            attributes: vec![dir.clone()],
        }]
    })
}
