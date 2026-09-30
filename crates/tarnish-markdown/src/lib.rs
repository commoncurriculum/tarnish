//! Markdown to tokens as marked reads it: marked 17.0.6's lexer, and `marked-more-lists` 1.0.1's
//! list tokenizer, ported on tarnish-js so that positions and matches count UTF-16 units as they
//! do in JavaScript.

#![forbid(unsafe_code)]

pub mod marked;
pub mod marked_more_lists;

/// The version of marked this crate ports.
pub const MARKED: &str = "17.0.6";
