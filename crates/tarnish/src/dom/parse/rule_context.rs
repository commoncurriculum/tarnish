//! Rules' `context`: the nodes a rule may match inside, as `"a/b//c|d"`.

use super::context::ParseContext;
use crate::dom::Dom;
use crate::error::Result;
use crate::text::is_js_space;

impl<'p, D: Dom> ParseContext<'p, D> {
    /// Whether the context string matches the nodes being parsed into.
    pub(super) fn matches_context(&self, context: &str) -> Result<bool> {
        if context.contains('|') {
            for part in split_alternatives(context) {
                if self.matches_context(part)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        let parts: Vec<&str> = context.split('/').collect();
        let option = self.options.context.as_ref();
        let use_root = !self.is_open
            && option.is_none_or(|option| {
                self.nodes[0]
                    .node_type
                    .is_some_and(|top| top == option.parent().node_type())
            });
        let min_depth = -(option.map_or(0, |option| option.depth() as isize + 1))
            + if use_root { 0 } else { 1 };
        self.match_parts(
            &parts,
            parts.len() as isize - 1,
            self.open as isize,
            min_depth,
            use_root,
        )
    }

    fn match_parts(
        &self,
        parts: &[&str],
        mut index: isize,
        mut depth: isize,
        min_depth: isize,
        use_root: bool,
    ) -> Result<bool> {
        let option = self.options.context.as_ref();
        while index >= 0 {
            let part = parts[index as usize];
            if part.is_empty() {
                if index == parts.len() as isize - 1 || index == 0 {
                    index -= 1;
                    continue;
                }
                while depth >= min_depth {
                    if self.match_parts(parts, index - 1, depth, min_depth, use_root)? {
                        return Ok(true);
                    }
                    depth -= 1;
                }
                return Ok(false);
            }
            let next = if depth > 0 || (depth == 0 && use_root) {
                self.nodes[depth as usize].node_type
            } else if let Some(option) = option
                && depth >= min_depth
            {
                Some(option.node((depth - min_depth) as usize).node_type())
            } else {
                None
            };
            match next {
                Some(next) if next.name() == part || next.is_in_group(part) => {}
                _ => return Ok(false),
            }
            depth -= 1;
            index -= 1;
        }
        Ok(true)
    }
}

/// `context.split(/\s*\|\s*/)`: the space around each `|` goes with it.
fn split_alternatives(context: &str) -> Vec<&str> {
    let space = |c: char| c.len_utf16() == 1 && is_js_space(c as u16);
    let parts: Vec<&str> = context.split('|').collect();
    let last = parts.len() - 1;
    parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| {
            let part = if index > 0 {
                part.trim_start_matches(space)
            } else {
                part
            };
            if index < last {
                part.trim_end_matches(space)
            } else {
                part
            }
        })
        .collect()
}
