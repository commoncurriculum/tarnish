//! `src/table/utilities/markdown.ts`: a table as Markdown, and the pipes in code spans that
//! Table's `markdownTokenizer` escapes for marked.

use tarnish_js::json::Value;
use tarnish_js::units::Units;
use tarnish_js::{self as js, Error, stack, utf16, value};
use tarnish_markdown::marked::{Lexer, Token};

use super::utils::normalize_table_cell_align;
use crate::markdown::RenderHelpers;

/// `DEFAULT_CELL_LINE_SEPARATOR`, which joins the Markdown of a cell's blocks.
const DEFAULT_CELL_LINE_SEPARATOR: &str = "\u{1F}";

const NEWLINE: u16 = b'\n' as u16;
const PIPE: u16 = b'|' as u16;
const BACKTICK: u16 = b'`' as u16;
const BACKSLASH: u16 = b'\\' as u16;

/// `escapeTableCellPipes(line)`: the `|`s inside code spans escaped, so that marked's
/// `splitCells` doesn't split there. A backslash outside a code span escapes what follows it,
/// and a run of backticks with no run as long after it is left as it is.
fn escape_table_cell_pipes(line: &[u16]) -> Vec<u16> {
    let mut result = Vec::with_capacity(line.len());
    let run = |at: usize| {
        line[at..]
            .iter()
            .take_while(|&&unit| unit == BACKTICK)
            .count()
    };
    let mut i = 0;
    while i < line.len() {
        if line[i] == BACKSLASH && i + 1 < line.len() {
            result.extend_from_slice(&line[i..i + 2]);
            i += 2;
            continue;
        }
        if line[i] != BACKTICK {
            result.push(line[i]);
            i += 1;
            continue;
        }
        let run_len = run(i);
        let mut j = i + run_len;
        let mut found = false;
        while j < line.len() {
            if line[j] != BACKTICK {
                j += 1;
                continue;
            }
            let close_len = run(j);
            if close_len == run_len {
                result.extend_from_slice(&line[i..i + run_len]);
                // `spanContent.replace(/(?<!\\)\|/g, '\\|')`
                let span = &line[i + run_len..j];
                for (at, &unit) in span.iter().enumerate() {
                    if unit == PIPE && (at == 0 || span[at - 1] != BACKSLASH) {
                        result.push(BACKSLASH);
                    }
                    result.push(unit);
                }
                result.extend_from_slice(&line[j..j + run_len]);
                i = j + run_len;
                found = true;
                break;
            }
            j += close_len;
        }
        if !found {
            result.extend_from_slice(&line[i..i + run_len]);
            i += run_len;
        }
    }
    result
}

/// `preprocessTablePipes(src)`, `None` where it is `src` as it was.
fn preprocess_table_pipes(src: &[u16]) -> Option<Vec<u16>> {
    let mut changed = false;
    let lines: Vec<Vec<u16>> = src
        .split(|&unit| unit == NEWLINE)
        .map(|line| {
            if !line.contains(&PIPE) || !line.contains(&BACKTICK) {
                return line.to_vec();
            }
            let escaped = escape_table_cell_pipes(line);
            changed |= escaped != line;
            escaped
        })
        .collect();
    changed.then(|| lines.join(&NEWLINE))
}

/// `/^[ \t|:]*-[ \t|:-]*$/.test(line) && line.includes('|')`: a GFM delimiter row, and not the
/// `---` under a setext heading.
fn is_delimiter_row(line: &[u16]) -> bool {
    let of = |byte: u8| line.contains(&u16::from(byte));
    line.iter().all(|&unit| {
        u8::try_from(unit).is_ok_and(|byte| matches!(byte, b' ' | b'\t' | b'|' | b':' | b'-'))
    }) && of(b'-')
        && of(b'|')
}

/// The tokenizer's `start(src)`: at the start, when the first line has a pipe and the second is
/// a delimiter row.
pub fn tokenizer_start(src: &Units) -> Option<usize> {
    let mut lines = src.split(|&unit| unit == NEWLINE);
    let first = lines.next()?;
    let separator = lines.next()?;
    (is_delimiter_row(separator) && first.contains(&PIPE)).then_some(0)
}

/// The tokenizer's `tokenize(src, tokens, helper)`: a table up to the first blank line, lexed
/// again with the pipes in its code spans escaped, or nothing where none are, for marked's own
/// table tokenizer to lex.
pub fn tokenize(src: &Units, _: &[Token], lexer: &mut Lexer) -> Result<Option<Token>, Error> {
    // The candidate, `src` up to its first blank line, has `src`'s second line or none.
    let separator = src.split(|&unit| unit == NEWLINE).nth(1);
    if !separator.is_some_and(is_delimiter_row) {
        return Ok(None);
    }
    let candidate = match utf16::index_of_str(src, "\n\n", 0) {
        Some(end) => &src[..end],
        None => &src[..],
    };
    let Some(preprocessed) = preprocess_table_pipes(candidate) else {
        return Ok(None);
    };
    let mut block = Vec::new();
    let preprocessed = Units::from(preprocessed);
    stack::grow(|| lexer.block_tokens(&preprocessed, &mut block, false))?;
    let table = match block.into_iter().next() {
        Some(table) if table.kind == "table" && !table.raw.is_empty() => table,
        _ => return Ok(None),
    };
    // The escapes add no lines, so the table's lines of `src` are its raw.
    let line_count = table.raw.iter().filter(|&&unit| unit == NEWLINE).count() + 1;
    let newlines = src.iter().enumerate().filter(|(_, unit)| **unit == NEWLINE);
    let end = newlines.map(|(at, _)| at).nth(line_count - 1);
    Ok(Some(Token {
        raw: src.slice(0..end.unwrap_or(src.len())),
        ..table
    }))
}

/// `raw.replace(/[ \t]*\r?\n[ \t]*/g, '<br>')`: each line break, and the spaces and tabs around
/// it, as a `<br>`.
fn line_breaks_to_br(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut result = String::with_capacity(raw.len());
    let (mut copied, mut i) = (0, 0);
    while i < bytes.len() {
        let blank = bytes[i..]
            .iter()
            .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
            .count();
        let at = i + blank;
        let newline = match bytes.get(at..at + 2) {
            Some(b"\r\n") => Some(at + 2),
            _ if bytes.get(at) == Some(&b'\n') => Some(at + 1),
            _ => None,
        };
        // No match starts in the blanks before anything but a line break.
        let Some(after) = newline else {
            i = at + 1;
            continue;
        };
        let end = after
            + bytes[after..]
                .iter()
                .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
                .count();
        result.push_str(&raw[copied..i]);
        result.push_str("<br>");
        copied = end;
        i = end;
    }
    result.push_str(&raw[copied..]);
    result
}

/// `collapseWhitespace(s)`: each run of whitespace a space, and none at either end.
fn collapse_whitespace(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    let mut in_run = false;
    for character in text.chars() {
        if js::is_whitespace(character) {
            if !in_run {
                collapsed.push(' ');
            }
            in_run = true;
        } else {
            collapsed.push(character);
            in_run = false;
        }
    }
    js::trim(&collapsed).to_owned()
}

/// A cell as `renderTableToMarkdown` reads it.
struct Cell {
    text: String,
    is_header: bool,
    align: Option<&'static str>,
}

/// `(r[i] && r[i].text) || ''`.
fn text(row: &[Cell], column: usize) -> &str {
    row.get(column).map_or("", |cell| &cell.text)
}

/// The text of a cell, its blocks' Markdown on one line.
fn cell_text(cell: &Value, helpers: &dyn RenderHelpers) -> Result<String, Error> {
    let raw = match value::get(Some(cell), "content")? {
        Some(Value::Array(children)) if children.len() > 1 => {
            let parts = children
                .iter()
                .map(|child| helpers.render_children(child, ""))
                .collect::<Result<Vec<_>, Error>>()?;
            parts.join(DEFAULT_CELL_LINE_SEPARATOR)
        }
        Some(content) if js::truthy(Some(content)) => helpers.render_children(content, "")?,
        _ => String::new(),
    };
    let lines = raw.replace(DEFAULT_CELL_LINE_SEPARATOR, "\n");
    Ok(collapse_whitespace(&line_breaks_to_br(&lines)))
}

/// `renderTableToMarkdown(node, h)`: a GFM table, its columns padded to their widest cell and
/// aligned as their first aligned cell, under the first row when it has a header cell, or else
/// under an empty header.
pub fn render_table_to_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
) -> Result<String, Error> {
    let content = value::optional(Some(node), "content");
    let Some(content) = content.filter(|content| js::truthy(Some(content))) else {
        return Ok(String::new());
    };
    if value::length(Some(content))?.as_deref() == Some(&js::number(0.0)) {
        return Ok(String::new());
    }
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    for row_node in value::array_method(Some(content), "node.content.forEach")? {
        let mut cells = Vec::new();
        let row_content = value::get(Some(row_node), "content")?;
        if js::truthy(row_content) {
            for cell_node in value::array_method(row_content, "rowNode.content.forEach")? {
                let text = cell_text(cell_node, helpers)?;
                let kind = value::optional(Some(cell_node), "type");
                let attrs = value::optional(Some(cell_node), "attrs");
                let align = value::optional(attrs, "align").and_then(Value::as_str);
                cells.push(Cell {
                    text,
                    is_header: kind.is_some_and(|kind| kind == "tableHeader"),
                    align: normalize_table_cell_align(align),
                });
            }
        }
        rows.push(cells);
    }
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if column_count == 0 {
        return Ok(String::new());
    }
    let col_widths: Vec<usize> = (0..column_count)
        .map(|column| {
            let widest = rows.iter().map(|row| js::utf16_len(text(row, column)));
            widest.max().unwrap_or(0).max(3)
        })
        .collect();
    let pad = |text: &str, width: usize| {
        text.to_owned() + &" ".repeat(width.saturating_sub(js::utf16_len(text)))
    };
    let line = |texts: Vec<String>| format!("| {} |\n", texts.join(" | "));
    let has_header = rows[0].iter().any(|cell| cell.is_header);
    let col_alignments = (0..column_count).map(|column| {
        rows.iter()
            .find_map(|row| row.get(column).and_then(|cell| cell.align))
    });

    let row_line = |row: &[Cell]| {
        let cells = col_widths.iter().enumerate();
        line(
            cells
                .map(|(column, &width)| pad(text(row, column), width))
                .collect(),
        )
    };

    let mut out = String::from("\n");
    out += &row_line(if has_header { &rows[0] } else { &[] });
    let separator = col_widths
        .iter()
        .zip(col_alignments)
        .map(|(&width, align)| {
            let dashes = "-".repeat(width.max(3));
            match align {
                Some("left") => format!(":{dashes}"),
                Some("right") => format!("{dashes}:"),
                Some("center") => format!(":{dashes}:"),
                _ => dashes,
            }
        });
    out += &line(separator.collect());
    for row in &rows[usize::from(has_header)..] {
        out += &row_line(row);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use tarnish_js::random::strings;
    use tarnish_js::regexp::RegExp;
    use tarnish_js::utf16;

    #[test]
    fn line_breaks_become_brs_where_the_regex_finds_them() {
        let regex = RegExp::new(r"[ \t]*\r?\n[ \t]*", "g");
        for raw in strings(&[" ", "\t", "\r", "\n", "\r\n", "a", "é"], 200_000) {
            let expected = utf16::to_string(&regex.replace(&raw, "<br>"));
            let raw = utf16::to_string(&raw);
            assert_eq!(super::line_breaks_to_br(&raw), expected, "{raw:?}");
        }
    }

    #[test]
    fn whitespace_collapses_where_the_regex_finds_it() {
        let regex = RegExp::new(r"\s+", "g");
        let alphabet = [
            " ", "\t", "\n", "\u{A0}", "\u{3000}", "\u{FEFF}", "\u{85}", "a",
        ];
        for text in strings(&alphabet, 200_000) {
            let replaced = utf16::to_string(&regex.replace(&text, " "));
            let text = utf16::to_string(&text);
            assert_eq!(
                super::collapse_whitespace(&text),
                tarnish_js::trim(&replaced),
                "{text:?}"
            );
        }
    }
}
