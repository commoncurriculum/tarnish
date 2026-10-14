//! What Tiptap's extensions make with @tiptap/markdown's `MarkdownManager`, ProseMirror's
//! `DOMParser` and `DOMSerializer`, `generateText` and `generateUniqueIds`, as
//! `harness/record-tiptap*.mjs` record them in `fixtures/tiptap*.json`, one module here for each:
//! tarnish-tiptap's must make the same schemas, documents, Markdown, HTML and text, or fail with
//! the same error, and convert what nests far deeper than a small stack could recurse through as
//! the recorded conversions fold to.

mod blocks;
mod inline;
mod lists;
mod own;
mod tables;
mod tiptap;
