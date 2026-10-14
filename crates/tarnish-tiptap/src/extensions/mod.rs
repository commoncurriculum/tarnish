//! Tiptap's own extensions: the `@tiptap/extension-*` packages, `@tiptap/core`'s TextDirection
//! and `tiptap-extension-flat-list`, each as far as the conversions read it: its schema, its HTML
//! rules and its Markdown hooks, save the flat list items' parse rules. An application extends
//! one with the rest it needs.

pub mod audio;
pub mod blockquote;
pub mod bold;
pub mod bullet_list;
pub mod code;
pub mod code_block;
pub mod details;
pub mod document;
mod embed;
pub mod emoji;
pub mod flat_list;
pub mod hard_break;
pub mod heading;
pub mod highlight;
pub mod horizontal_rule;
pub mod image;
pub mod italic;
pub mod link;
pub mod list_item;
pub mod mathematics;
pub mod mention;
pub mod ordered_list;
pub mod paragraph;
pub mod ruby_text;
pub mod strike;
pub mod subscript;
pub mod superscript;
pub mod table;
pub mod table_of_contents;
pub mod task_item;
pub mod task_list;
pub mod text;
pub mod text_align;
pub mod text_direction;
pub mod text_style;
pub mod twitch;
pub mod underline;
pub mod unique_id;
pub mod youtube;

#[cfg(test)]
mod tests {
    use tarnish_markdown::marked::Marked;
    use tarnish_markdown::marked_more_lists::more_lists;

    use super::highlight::{HighlightOptions, highlight};
    use super::mathematics::inline_math;
    use super::mention::{MentionOptions, mention};
    use super::underline::{UnderlineOptions, underline};
    use crate::Extension;
    use crate::markdown::MarkdownManager;

    #[test]
    fn tokenizers_match_only_at_their_starts() {
        let extensions: [Extension; 4] = [
            underline(UnderlineOptions::default()).into(),
            highlight(HighlightOptions::default()).into(),
            inline_math().into(),
            mention(MentionOptions::default()).into(),
        ];
        let manager = MarkdownManager::new(&extensions, Marked::new(more_lists()));
        let alphabet = &[
            "=", "==", "+", "++", "$", "$$", "a", " ", "\n", "\\", "*", "`", "<", ">", "[", "]",
            "[@", "@", "id=\"", "'",
        ];
        manager.marked().check_extension_starts(alphabet);
    }
}
