//! `@tiptap/extension-heading`.

use crate::DomSpec;
use crate::{ExtensionAttribute, NodeExtension, ParseHtml};
use tarnish::json::{json, object};

pub struct HeadingOptions {
    pub levels: &'static [u8],
}

pub fn heading(options: HeadingOptions) -> NodeExtension {
    let levels = options.levels;
    NodeExtension::create("heading")
        .content("inline*")
        .group("block")
        .add_attributes(vec![
            ExtensionAttribute::new("level", json!(1)).not_rendered(),
        ])
        .parse_html(
            levels
                .iter()
                .map(|level| {
                    ParseHtml::tag(TAGS[usize::from(*level) - 1])
                        .attrs(object!({ "level": level }))
                        .into()
                })
                .collect(),
        )
        .render_html(move |node, html| {
            let level = node
                .attrs_view()
                .get("level")
                .and_then(|level| level.as_f64())
                .filter(|level| levels.iter().any(|allowed| f64::from(*allowed) == *level))
                .map_or(levels[0], |level| level as u8);
            Ok(DomSpec::wrapping(TAGS[usize::from(level) - 1], html))
        })
        .markdown_token_name("heading")
}

const TAGS: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];
