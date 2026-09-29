//! `helpers.ts`: what the tokenizers use of it.

use super::rules::OTHER;
use tarnish_js::utf16;

/// `splitCells(tableRow, count)`.
pub fn split_cells(table_row: &[u16], count: Option<usize>) -> Vec<Vec<u16>> {
    let row = OTHER.find_pipe.replace_with(table_row, |found| {
        let mut escaped = false;
        let mut current = found.index();
        while current > 0 && table_row[current - 1] == utf16::unit(b'\\') {
            escaped = !escaped;
            current -= 1;
        }
        utf16::from(if escaped { "|" } else { " |" })
    });
    let mut cells: Vec<&[u16]> = OTHER.split_pipe.split(&row);
    if utf16::trim(cells[0]).is_empty() {
        cells.remove(0);
    }
    if cells
        .last()
        .is_some_and(|last| utf16::trim(last).is_empty())
    {
        cells.pop();
    }
    if let Some(count) = count.filter(|count| *count > 0) {
        if cells.len() > count {
            cells.truncate(count);
        } else {
            cells.resize(count, &[]);
        }
    }
    cells
        .into_iter()
        .map(|cell| OTHER.slash_pipe.replace(utf16::trim(cell), "|"))
        .collect()
}

/// `rtrim(str, c)`: `str` without the run of `c` it ends with.
pub fn rtrim(units: &[u16], character: u8) -> &[u16] {
    let end = units
        .iter()
        .rposition(|&unit| unit != utf16::unit(character))
        .map_or(0, |index| index + 1);
    &units[..end]
}

/// `findClosingBracket(str, b)`: the index of the bracket that closes the text, `None` for -1,
/// and `Some(Err(()))` for -2, when more open than close.
pub fn find_closing_bracket(units: &[u16], open: u8, close: u8) -> Option<Result<usize, ()>> {
    let (open, close) = (utf16::unit(open), utf16::unit(close));
    utf16::find(units, close)?;
    let mut level = 0;
    let mut index = 0;
    while index < units.len() {
        if units[index] == utf16::unit(b'\\') {
            index += 1;
        } else if units[index] == open {
            level += 1;
        } else if units[index] == close {
            level -= 1;
            if level < 0 {
                return Some(Ok(index));
            }
        }
        index += 1;
    }
    (level > 0).then_some(Err(()))
}
