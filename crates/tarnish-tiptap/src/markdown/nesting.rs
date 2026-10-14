//! `@tiptap/core`'s utilities for Markdown whose blocks nest by indentation:
//! `renderNestedMarkdownContent` and `parseIndentedBlocks`.

use tarnish_js::json::Value;
use tarnish_js::units::Units;
use tarnish_js::value::{self, Nullish};
use tarnish_js::{self as js, Error, stack, utf16};
use tarnish_markdown::marked::{Lexer, Token};

use super::RenderHelpers;

/// `renderNestedMarkdownContent(node, h, prefix)`: the node's first child after the prefix, and
/// each other child on the lines after it, indented, a paragraph after a blank line. `prefix`
/// is called only for a node whose `content` is an array.
pub fn render_nested_markdown_content(
    node: &Value,
    helpers: &dyn RenderHelpers,
    prefix: impl FnOnce() -> Result<String, Error>,
) -> Result<String, Error> {
    let Some(Value::Array(content)) = value::optional(Some(node), "content") else {
        return Ok(String::new());
    };
    let prefix = prefix()?;
    let Some((first, children)) = content.split_first() else {
        // `h.renderChildren([undefined])` reads the type of `undefined`.
        return Err(value::cannot_read(Nullish::Undefined, "type"));
    };
    let mut output = prefix + &helpers.render_array(std::slice::from_ref(first), "")?;
    for (index, child) in children.iter().enumerate() {
        let child_content = helpers.render_child(child, index + 1)?;
        let indented: Vec<String> = child_content
            .split('\n')
            .map(|line| helpers.indent(line))
            .collect();
        let paragraph = value::get(Some(child), "type")?.is_some_and(|kind| kind == "paragraph");
        output.push_str(if paragraph { "\n\n" } else { "\n" });
        output.push_str(&indented.join("\n"));
    }
    Ok(output)
}

/// `parseIndentedBlocks`'s `config`: how a kind of block's items read.
pub trait IndentedBlockConfig {
    /// What `extractItemData` takes from an item's line.
    type Item;

    /// `config.itemPattern` and `config.extractItemData`: the item a line is, and its
    /// `indentLevel`.
    fn item(&self, line: &[u16]) -> Option<(usize, Self::Item)>;

    /// `config.createToken(data, nestedTokens)`.
    fn create_token(
        &self,
        item: Self::Item,
        nested_tokens: Option<Vec<Token>>,
        lexer: &mut Lexer,
    ) -> Result<Token, Error>;

    /// `config.baseIndentSize || 2`.
    fn base_indent_size(&self) -> usize {
        2
    }

    /// `config.customNestedParser(dedented)`, or else `lexer.blockTokens(dedented)`.
    fn nested(&self, dedented: Units, lexer: &mut Lexer) -> Result<Option<Vec<Token>>, Error> {
        let mut tokens = Vec::new();
        lexer.block_tokens(&dedented, &mut tokens, false)?;
        Ok(Some(tokens))
    }
}

/// What `parseIndentedBlocks` gives: the items, and the source they take, each line of it with
/// a newline after it.
pub struct IndentedBlocks {
    pub items: Vec<Token>,
    pub raw: Units,
}

/// `parseIndentedBlocks(src, config, lexer)`: the items at the start of `src`, after any blank
/// lines, each with the lines indented more than it as its nested tokens.
pub fn parse_indented_blocks<C: IndentedBlockConfig>(
    src: &Units,
    config: &C,
    lexer: &mut Lexer,
) -> Result<Option<IndentedBlocks>, Error> {
    let mut lines = Lines::new(src);
    let mut items = Vec::new();
    // Where the lines taken so far end.
    let mut taken = 0;
    let mut i = 0;
    while let Some(line) = lines.get(i) {
        let Some((indent_level, item)) = config.item(line) else {
            if !items.is_empty() {
                break;
            }
            if is_blank(line) {
                i += 1;
                taken = lines.end(i - 1);
                continue;
            }
            return Ok(None);
        };
        taken = lines.end(i);
        let mut nested_lines: Vec<&[u16]> = Vec::new();
        i += 1;
        while let Some(next_line) = lines.get(i) {
            let continues = if is_blank(next_line) {
                let next_non_empty = (i + 1..)
                    .map_while(|j| lines.get(j))
                    .find(|line| !is_blank(line));
                match next_non_empty {
                    None => break,
                    Some(next_non_empty) => leading_whitespace(next_non_empty) > indent_level,
                }
            } else {
                leading_whitespace(next_line) > indent_level
            };
            if !continues {
                break;
            }
            nested_lines.push(next_line);
            taken = lines.end(i);
            i += 1;
        }
        let mut nested_tokens = None;
        if !nested_lines.is_empty() {
            let dedent = indent_level + config.base_indent_size();
            let dedented: Vec<&[u16]> = nested_lines
                .iter()
                .map(|line| utf16::substring(line, dedent))
                .collect();
            let dedented = utf16::join(&dedented, &[utf16::unit(b'\n')]);
            if !utf16::trim(&dedented).is_empty() {
                nested_tokens = stack::grow(|| config.nested(Units::from(dedented), lexer))?;
            }
        }
        items.push(config.create_token(item, nested_tokens, lexer)?);
    }
    if items.is_empty() {
        return Ok(None);
    }
    Ok(Some(IndentedBlocks {
        items,
        raw: with_newline(src, taken),
    }))
}

/// `src`'s lines up to `end`, a line's end, with a newline after the last.
fn with_newline(src: &Units, end: usize) -> Units {
    match src.get(end) {
        Some(&unit) if unit == utf16::unit(b'\n') => src.slice(0..end + 1),
        _ => {
            let mut raw = src[..end].to_vec();
            raw.push(utf16::unit(b'\n'));
            Units::from(raw)
        }
    }
}

/// `line.trim() === ''`.
fn is_blank(line: &[u16]) -> bool {
    line.iter().all(|&unit| js::is_whitespace_unit(unit))
}

/// `line.match(/^(\s*)/)[1].length`.
fn leading_whitespace(line: &[u16]) -> usize {
    line.iter()
        .take_while(|&&unit| js::is_whitespace_unit(unit))
        .count()
}

/// `src.split('\n')`, split as far as it is read.
pub(crate) struct Lines<'s> {
    src: &'s [u16],
    /// Where each line split so far ends.
    ends: Vec<usize>,
}

impl<'s> Lines<'s> {
    pub(crate) fn new(src: &'s [u16]) -> Self {
        Lines {
            src,
            ends: Vec::new(),
        }
    }

    /// Line `i`, if `src` has that many.
    pub(crate) fn get(&mut self, i: usize) -> Option<&'s [u16]> {
        while self.ends.len() <= i {
            let start = self.start(self.ends.len())?;
            let end = utf16::find(&self.src[start..], utf16::unit(b'\n'))
                .map_or(self.src.len(), |offset| start + offset);
            self.ends.push(end);
        }
        let start = self.start(i)?;
        Some(&self.src[start..self.ends[i]])
    }

    /// Where line `i` starts, if `src` has it: after the newline that ends the line before.
    pub(crate) fn start(&self, i: usize) -> Option<usize> {
        match i.checked_sub(1) {
            None => Some(0),
            Some(before) => {
                let end = *self.ends.get(before)?;
                (end < self.src.len()).then_some(end + 1)
            }
        }
    }

    /// Where line `i`, which has been read, ends.
    pub(crate) fn end(&self, i: usize) -> usize {
        self.ends[i]
    }
}

#[cfg(test)]
mod tests {
    use super::Lines;
    use tarnish_js::random::strings;
    use tarnish_js::utf16;

    #[test]
    fn lines_are_what_split_makes() {
        for src in strings(&["a", "\n", "\n\n", " ", "é"], 20_000) {
            let mut lines = Lines::new(&src);
            let split = utf16::split(&src, utf16::unit(b'\n'));
            let read: Vec<&[u16]> = (0..).map_while(|i| lines.get(i)).collect();
            assert_eq!(read, split, "{:?}", String::from_utf16_lossy(&src));
        }
    }
}
