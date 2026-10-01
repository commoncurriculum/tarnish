//! Tiptap's own extensions, as `harness/record-tiptap.mjs` gives them to the `MarkdownManager`
//! and to `getSchema`.

use tarnish_markdown::marked::Marked;
use tarnish_markdown::marked_more_lists::more_lists;
use tarnish_tiptap::extensions::bold::bold;
use tarnish_tiptap::extensions::document::document;
use tarnish_tiptap::extensions::hard_break::hard_break;
use tarnish_tiptap::extensions::heading::{HeadingOptions, heading};
use tarnish_tiptap::extensions::highlight::{HighlightOptions, highlight};
use tarnish_tiptap::extensions::image::{ImageOptions, image};
use tarnish_tiptap::extensions::italic::italic;
use tarnish_tiptap::extensions::paragraph::paragraph;
use tarnish_tiptap::extensions::strike::strike;
use tarnish_tiptap::extensions::subscript::subscript;
use tarnish_tiptap::extensions::superscript::superscript;
use tarnish_tiptap::extensions::text::text;
use tarnish_tiptap::extensions::text_style::text_style;
use tarnish_tiptap::extensions::underline::underline;
use tarnish_tiptap::markdown::MarkdownManager;
use tarnish_tiptap::{Extension, TiptapSchema, get_schema};

pub fn extensions() -> Vec<Extension> {
    vec![
        document().into(),
        paragraph().into(),
        text().into(),
        heading(HeadingOptions::default()).into(),
        image(ImageOptions::default()).into(),
        hard_break().into(),
        bold().into(),
        italic().into(),
        strike().into(),
        underline().into(),
        subscript().into(),
        superscript().into(),
        highlight(HighlightOptions { multicolor: false }).into(),
        text_style().into(),
    ]
}

pub fn schema() -> TiptapSchema {
    get_schema(&extensions()).expect("the schema")
}

/// `new MarkdownManager({ extensions, marked })`, with a marked that uses marked-more-lists.
pub fn markdown() -> MarkdownManager {
    MarkdownManager::new(&extensions(), Marked::new(more_lists()))
}
