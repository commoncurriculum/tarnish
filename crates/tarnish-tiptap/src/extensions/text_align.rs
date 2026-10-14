//! `@tiptap/extension-text-align`.

use tarnish_js::json::Value;

use crate::{ExtensionAttribute, GlobalAttributes, GlobalTypes, PlainExtension};

pub struct TextAlignOptions {
    /// The types the alignment is for.
    pub types: &'static [&'static str],
    /// The alignments an element's `text-align` may give.
    pub alignments: &'static [&'static str],
    pub default_alignment: Option<&'static str>,
}

impl Default for TextAlignOptions {
    fn default() -> Self {
        TextAlignOptions {
            types: &[],
            alignments: &["left", "center", "right", "justify"],
            default_alignment: None,
        }
    }
}

pub fn text_align(options: TextAlignOptions) -> PlainExtension {
    let TextAlignOptions {
        types,
        alignments,
        default_alignment,
    } = options;
    let default = default_alignment.map(|alignment| Value::String(alignment.into()));
    let parsed_default = default.clone();
    let attribute = ExtensionAttribute::new("textAlign", Value::from(default))
        .parse_html(move |element| {
            let alignment = element.style_value("text-align");
            if alignments.contains(&alignment.as_str()) {
                Some(Value::String(alignment))
            } else {
                parsed_default.clone()
            }
        })
        .render_html(|attributes, rendered| match attributes.get("textAlign") {
            Some(alignment) if alignment.truthy() => rendered.merge(
                "style",
                format!("text-align: {}", alignment.to_js_string()?),
            ),
            _ => Ok(()),
        });
    PlainExtension::create("textAlign").add_global_attributes(move |_| {
        vec![GlobalAttributes {
            types: GlobalTypes::Named(types.to_vec()),
            attributes: vec![attribute.clone()],
        }]
    })
}
