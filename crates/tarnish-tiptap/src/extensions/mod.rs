//! Tiptap's own extensions: the `@tiptap/extension-*` packages and `tiptap-extension-flat-list`,
//! each as far as the conversions read it: its schema, its HTML rules and its Markdown hooks,
//! save the flat list items' parse rules. An application extends one with the rest it needs.

pub mod bold;
pub mod document;
pub mod flat_list;
pub mod hard_break;
pub mod heading;
pub mod highlight;
pub mod image;
pub mod italic;
pub mod paragraph;
pub mod strike;
pub mod subscript;
pub mod superscript;
pub mod text;
pub mod text_style;
pub mod underline;

#[cfg(test)]
mod tests {
    use tarnish_markdown::marked::Marked;
    use tarnish_markdown::marked_more_lists::more_lists;

    use super::highlight::{HighlightOptions, highlight};
    use super::underline::underline;
    use crate::Extension;
    use crate::markdown::MarkdownManager;

    #[test]
    fn tokenizers_match_only_at_their_starts() {
        let extensions: [Extension; 2] = [
            underline().into(),
            highlight(HighlightOptions { multicolor: false }).into(),
        ];
        let manager = MarkdownManager::new(&extensions, Marked::new(more_lists()));
        let alphabet = &[
            "=", "==", "+", "++", "a", " ", "\n", "\\", "*", "`", "<", ">",
        ];
        manager.marked().check_extension_starts(alphabet);
    }
}
