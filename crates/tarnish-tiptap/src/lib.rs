//! Tiptap on servers, on tarnish's ProseMirror: extensions as `@tiptap/core` declares them,
//! `getSchema`, the HTML conversions of `@tiptap/html`, and `@tiptap/markdown`'s
//! `MarkdownManager`, with Tiptap's own extensions.
//!
//! An application declares its extensions with [`NodeExtension`], [`MarkExtension`] and
//! [`PlainExtension`], or extends Tiptap's in [`extensions`], and builds a schema from them
//! with [`get_schema`]. [`html`] parses HTML into documents of that schema and writes documents
//! as HTML, as a browser's DOM would; [`markdown::MarkdownManager`] converts to and from
//! Markdown with the extensions' Markdown hooks.
//!
//! ```
//! use tarnish_markdown::marked::Marked;
//! use tarnish_markdown::marked_more_lists::more_lists;
//! use tarnish_tiptap::extensions::{bold::bold, document::document, paragraph::paragraph, text::text};
//! use tarnish_tiptap::markdown::MarkdownManager;
//! use tarnish_tiptap::{Extension, get_schema, html};
//!
//! let extensions: Vec<Extension> =
//!     vec![document().into(), paragraph().into(), text().into(), bold().into()];
//! let schema = get_schema(&extensions)?;
//!
//! let doc = html::parse(&schema, "<p>Hello <b>world</b></p>")?;
//! assert_eq!(html::serialize(&schema, &doc)?, "<p>Hello <strong>world</strong></p>");
//!
//! let markdown = MarkdownManager::new(&extensions, Marked::new(more_lists()));
//! assert_eq!(markdown.serialize(&doc.to_json())?, "Hello **world**");
//! assert_eq!(markdown.parse("Hello **world**")?, doc.to_json());
//! # Ok::<(), tarnish::Error>(())
//! ```

#![forbid(unsafe_code)]

mod attributes;
mod extension;
pub mod extensions;
pub mod html;
pub mod markdown;
mod merge_attributes;
mod parse_html;
mod schema;
mod utilities;

pub use attributes::{
    ExtensionAttribute, GlobalAttributes, ParseAttribute, RenderAttribute, Rendered,
};
pub use extension::{
    Extension, Kind, MarkConfig, MarkExtension, NodeConfig, NodeExtension, PlainExtension,
    RenderMark, RenderNode,
};
pub use merge_attributes::merge_attributes;
pub use parse_html::{GetAttrs, ParseHtml, StyleAttrs, StyleRule, TagRule};
pub use schema::{TiptapSchema, get_schema, sort_extensions};
pub use utilities::{
    attrs_equal, decode_html_entities, get_style_property, mark_attr, marks_equal, node_attr,
};

pub use tarnish::dom::{AttrValue, SpecAttrs};
pub use tarnish_html::HtmlNode;

/// A `DOMOutputSpec` that a `renderHTML` returns, borrowing from its node or mark.
pub type DomSpec<'a> = tarnish::dom::DomSpec<'a, HtmlNode>;

/// The version of `@tiptap/core`, `@tiptap/markdown` and Tiptap's extensions this crate ports.
pub const TIPTAP: &str = "3.30.0";
