//! `Tokenizer.ts`, with `this.lexer` the lexer these methods are on. The `list` tokenizer is
//! whatever `marked.use` gave, such as `marked-more-lists`'s: marked's own is not ported.

use tarnish_js::stack;

use super::delimiters::{self, Flank};
use super::helpers::{find_closing_bracket, rtrim, split_cells};
use super::lexer::{Lexer, Link};
use super::matchers;
use super::rules::{BLOCK, INLINE, OTHER};
use super::{Cell, Def, Destination, Table, Token, TokenData, Tokens};
use tarnish_js::Error;
use tarnish_js::units::Units;
use tarnish_js::utf16;

/// `outputLink(cap, link, raw, lexer)`.
fn output_link(
    lexer: &mut Lexer,
    cap0: &[u16],
    cap1: &Units,
    href: Vec<u16>,
    title: Option<Vec<u16>>,
    raw: Units,
) -> Result<Token, Error> {
    let text = OTHER.output_link_replace.replace_units(cap1, "$1");
    lexer.state.in_link = true;
    let tokens = lexer.inline_tokens(&text)?;
    lexer.state.in_link = false;
    let kind = if cap0.first() == Some(&utf16::unit(b'!')) {
        "image"
    } else {
        "link"
    };
    Ok(Token {
        data: TokenData::Link(Box::new(Destination {
            href,
            title: title.filter(|title| !title.is_empty()),
        })),
        text: Some(text),
        tokens: Some(tokens.into()),
        ..Token::new(kind, raw)
    })
}

/// `indentCodeCompensation(raw, text)`.
fn indent_code_compensation(raw: &[u16], text: &Units) -> Units {
    let Some(found) = OTHER.indent_code_compensation.exec(raw) else {
        return text.clone();
    };
    let indent_to_code = found.get(1).map_or(0, <[u16]>::len);
    let lines: Vec<&[u16]> = utf16::split(text, utf16::unit(b'\n'))
        .into_iter()
        .map(|node| match OTHER.beginning_space.exec(node) {
            Some(indent) if indent.all().len() >= indent_to_code => &node[indent_to_code..],
            _ => node,
        })
        .collect();
    Units::from(utf16::join(&lines, &[utf16::unit(b'\n')]))
}

/// `str.replace(regex, "$1")` when `str` is truthy, else `str`.
fn unescape(text: &[u16]) -> Vec<u16> {
    INLINE.any_punctuation.replace(text, "$1")
}

impl Lexer<'_> {
    pub(super) fn space(&self, src: &Units) -> Option<Token> {
        let length = matchers::newline(src)?;
        (length > 0).then(|| Token::new("space", src.slice(0..length)))
    }

    pub(super) fn code(&self, src: &Units) -> Option<Token> {
        let cap = BLOCK.code.exec(src)?;
        let raw = src.slice_of(cap.all());
        let text = OTHER.code_remove_indent.replace_units(&raw, "");
        Some(Token {
            text: Some(text.slice_of(rtrim(&text, b'\n'))),
            ..Token::new("code", raw)
        })
    }

    pub(super) fn fences(&self, src: &Units) -> Option<Token> {
        let cap = BLOCK.fences.exec(src)?;
        let raw = cap.all();
        let text = indent_code_compensation(raw, &src.slice_of(cap.get(3).unwrap_or_default()));
        Some(Token {
            text: Some(text),
            ..Token::new("code", src.slice_of(raw))
        })
    }

    pub(super) fn heading(&mut self, src: &Units) -> Option<Token> {
        let cap = matchers::heading(src)?;
        let mut text = utf16::trim(&src[cap.text]);
        // `/#$/` and `/ $/`.
        if text.last() == Some(&utf16::unit(b'#')) {
            let trimmed = rtrim(text, b'#');
            // CommonMark requires space before trailing #s.
            if trimmed.is_empty() || trimmed.last() == Some(&utf16::unit(b' ')) {
                text = utf16::trim(trimmed);
            }
        }
        let text = src.slice_of(text);
        let depth = cap.depth;
        let raw = src.slice(0..cap.end);
        let queued = self.inline(text.clone());
        Some(Token {
            data: TokenData::Heading { depth },
            text: Some(text),
            tokens: Some(Tokens::default()),
            queued: Some(queued),
            ..Token::new("heading", raw)
        })
    }

    pub(super) fn hr(&self, src: &Units) -> Option<Token> {
        let length = matchers::hr(src)?;
        Some(Token::new("hr", src.slice_of(rtrim(&src[..length], b'\n'))))
    }

    pub(super) fn blockquote(&mut self, src: &Units) -> Result<Option<Token>, Error> {
        let Some(cap) = BLOCK.blockquote.exec(src) else {
            return Ok(None);
        };
        let whole = src.slice_of(rtrim(cap.all(), b'\n'));
        let mut lines: Vec<Units> = utf16::split(&whole, utf16::unit(b'\n'))
            .into_iter()
            .map(|line| whole.slice_of(line))
            .collect();
        let mut raw = Units::default();
        let mut text = Units::default();
        let mut tokens: Vec<Token> = Vec::new();
        let newline = [utf16::unit(b'\n')];

        while !lines.is_empty() {
            let mut in_blockquote = false;
            let mut i = 0;
            while i < lines.len() {
                if OTHER.blockquote_start.test(&lines[i]) {
                    in_blockquote = true;
                } else if in_blockquote {
                    break;
                }
                i += 1;
            }
            let current_raw = join(&lines[..i]);
            lines.drain(..i);

            // Precede a setext continuation with 4 spaces so it isn't a setext.
            let current_text = OTHER
                .blockquote_setext_replace
                .replace_units(&current_raw, "\n    $1");
            let current_text = OTHER
                .blockquote_setext_replace2
                .replace_units(&current_text, "");
            raw = if raw.is_empty() {
                current_raw.clone()
            } else {
                Units::from(utf16::concat(&[&raw, &newline, &current_raw]))
            };
            text = if text.is_empty() {
                current_text.clone()
            } else {
                Units::from(utf16::concat(&[&text, &newline, &current_text]))
            };

            // Parse blockquote lines as top level tokens, merging paragraphs if this is a
            // continuation.
            let top = self.state.top;
            self.state.top = true;
            stack::grow(|| self.block_tokens(&current_text, &mut tokens, true))?;
            self.state.top = top;

            if lines.is_empty() {
                break;
            }

            let last_kind = tokens.last().map(|token| token.kind);
            if last_kind == Some("code") {
                // A blockquote continuation cannot be preceded by a code block.
                break;
            } else if last_kind == Some("blockquote") {
                // Include the continuation in the nested blockquote.
                let old = tokens.pop().expect("the last token");
                let new_text = join_rest(&old.raw, &lines);
                let new = self.blockquote(&new_text)?.expect("a blockquote");
                raw = Units::from(utf16::concat(&[
                    &raw[..raw.len().saturating_sub(old.raw.len())],
                    &new.raw,
                ]));
                text = Units::from(utf16::concat(&[
                    &text[..text.len().saturating_sub(old.text().len())],
                    new.text(),
                ]));
                tokens.push(new);
                break;
            } else if last_kind == Some("list") {
                // Include the continuation in the nested list.
                let old = tokens.pop().expect("the last token");
                let new_text = join_rest(&old.raw, &lines);
                let new = self.list(&new_text)?.expect("a list");
                raw = Units::from(utf16::concat(&[
                    &raw[..raw.len().saturating_sub(old.raw.len())],
                    &new.raw,
                ]));
                text = Units::from(utf16::concat(&[
                    &text[..text.len().saturating_sub(old.raw.len())],
                    &new.raw,
                ]));
                let rest = new_text.substring(new.raw.len());
                lines = utf16::split(&rest, utf16::unit(b'\n'))
                    .into_iter()
                    .map(|line| rest.slice_of(line))
                    .collect();
                tokens.push(new);
                continue;
            }
        }

        Ok(Some(Token {
            text: Some(text),
            tokens: Some(tokens.into()),
            ..Token::new("blockquote", raw)
        }))
    }

    /// `this.list(src)`, the tokenizer `marked.use` put in place.
    pub(super) fn list(&mut self, src: &Units) -> Result<Option<Token>, Error> {
        (self.marked.list)(self, src)
    }

    pub(super) fn html(&self, src: &Units) -> Option<Token> {
        let cap = BLOCK.html.exec(src)?;
        let raw = src.slice_of(cap.all());
        Some(Token {
            data: TokenData::Html { block: true },
            text: Some(raw.clone()),
            ..Token::new("html", raw)
        })
    }

    pub(super) fn def(&self, src: &Units) -> Option<Token> {
        let cap = matchers::def(src)?;
        let tag = OTHER
            .multiple_space_global
            .replace(&utf16::to_lower_case(cap.get(1).unwrap_or_default()), " ");
        let href = match cap.truthy(2) {
            Some(href) => unescape(&OTHER.href_brackets.replace(href, "$1")),
            None => Vec::new(),
        };
        let title = match cap.truthy(3) {
            Some(title) => Some(unescape(&title[1..title.len() - 1])),
            None => cap.get(3).map(<[u16]>::to_vec),
        };
        Some(Token {
            data: TokenData::Def(Box::new(Def { tag, href, title })),
            ..Token::new("def", src.slice_of(cap.all()))
        })
    }

    pub(super) fn table(&mut self, src: &Units) -> Option<Token> {
        let cap = matchers::table(src)?;
        let delimiter = cap.get(2).unwrap_or_default();
        if !OTHER.table_delimiter.test(delimiter) {
            // A delimiter row must have a pipe or colon, or it is a setext heading.
            return None;
        }
        let headers = split_cells(cap.get(1).unwrap_or_default(), None);
        let aligned = OTHER.table_align_chars.replace(delimiter, "");
        let aligns = utf16::split(&aligned, utf16::unit(b'|'));
        let rows: Vec<Vec<u16>> = match cap.get(3) {
            Some(rows) if !utf16::trim(rows).is_empty() => utf16::split(
                &OTHER.table_row_blank_line.replace(rows, ""),
                utf16::unit(b'\n'),
            )
            .into_iter()
            .map(<[u16]>::to_vec)
            .collect(),
            _ => Vec::new(),
        };
        if headers.len() != aligns.len() {
            // Header and align columns must be equal; rows can differ.
            return None;
        }
        let raw = src.slice_of(cap.all());
        let header: Vec<Cell> = headers.into_iter().map(|text| self.cell(text)).collect();
        let columns = header.len();
        let rows = rows
            .iter()
            .map(|row| {
                split_cells(row, Some(columns))
                    .into_iter()
                    .map(|text| self.cell(text))
                    .collect()
            })
            .collect();
        Some(Token {
            data: TokenData::Table(Box::new(Table { header, rows })),
            ..Token::new("table", raw)
        })
    }

    fn cell(&mut self, text: Vec<u16>) -> Cell {
        let queued = self.inline(Units::from(text));
        Cell {
            tokens: Vec::new(),
            queued: Some(queued),
        }
    }

    pub(super) fn lheading(&mut self, src: &Units) -> Option<Token> {
        let cap = matchers::lheading(src)?;
        let text = src.slice_of(utf16::trim(cap.get(1).unwrap_or_default()));
        let depth = if cap.get(2).and_then(<[u16]>::first) == Some(&utf16::unit(b'=')) {
            1
        } else {
            2
        };
        let raw = src.slice_of(cap.all());
        let queued = self.inline(text.clone());
        Some(Token {
            data: TokenData::Heading { depth },
            text: Some(text),
            tokens: Some(Tokens::default()),
            queued: Some(queued),
            ..Token::new("heading", raw)
        })
    }

    pub(super) fn paragraph(&mut self, src: &Units) -> Option<Token> {
        // `cap[1]` is the whole match.
        let length = matchers::paragraph(src)?;
        let first = &src[..length];
        let text = src.slice_of(match first.last() {
            Some(&last) if last == utf16::unit(b'\n') => &first[..first.len() - 1],
            _ => first,
        });
        let raw = src.slice(0..length);
        let queued = self.inline(text.clone());
        Some(Token {
            text: Some(text),
            tokens: Some(Tokens::default()),
            queued: Some(queued),
            ..Token::new("paragraph", raw)
        })
    }

    pub(super) fn text(&mut self, src: &Units) -> Option<Token> {
        let cap = BLOCK.text.exec(src)?;
        let text = src.slice_of(cap.all());
        let queued = self.inline(text.clone());
        Some(Token {
            text: Some(text.clone()),
            tokens: Some(Tokens::default()),
            queued: Some(queued),
            ..Token::new("text", text)
        })
    }

    pub(super) fn escape(&self, src: &Units) -> Option<Token> {
        let cap = INLINE.escape.exec(src)?;
        Some(Token {
            text: cap.get(1).map(|text| src.slice_of(text)),
            ..Token::new("escape", src.slice_of(cap.all()))
        })
    }

    pub(super) fn tag(&mut self, src: &Units) -> Option<Token> {
        let cap = INLINE.tag.exec(src)?;
        let raw = cap.all();
        if !self.state.in_link && OTHER.start_a_tag.test(raw) {
            self.state.in_link = true;
        } else if self.state.in_link && OTHER.end_a_tag.test(raw) {
            self.state.in_link = false;
        }
        let raw = src.slice_of(raw);
        Some(Token {
            data: TokenData::Html { block: false },
            text: Some(raw.clone()),
            ..Token::new("html", raw)
        })
    }

    pub(super) fn link(&mut self, src: &Units) -> Result<Option<Token>, Error> {
        let Some(cap) = INLINE.link.exec(src) else {
            return Ok(None);
        };
        let mut cap0 = cap.all();
        let cap1 = cap.get(1).unwrap_or_default();
        let mut cap2 = cap.get(2).unwrap_or_default();
        let mut cap3 = cap.get(3);
        let trimmed_url = utf16::trim(cap2);
        if OTHER.start_angle_bracket.test(trimmed_url) {
            // CommonMark requires matching angle brackets.
            if !OTHER.end_angle_bracket.test(trimmed_url) {
                return Ok(None);
            }
            // The ending angle bracket cannot be escaped.
            let rtrim_slash = rtrim(&trimmed_url[..trimmed_url.len() - 1], b'\\');
            if (trimmed_url.len() - rtrim_slash.len()).is_multiple_of(2) {
                return Ok(None);
            }
        } else {
            match find_closing_bracket(cap2, b'(', b')') {
                Some(Err(())) => return Ok(None),
                Some(Ok(last_paren_index)) => {
                    let start = if cap0.first() == Some(&utf16::unit(b'!')) {
                        5
                    } else {
                        4
                    };
                    let link_length = start + cap1.len() + last_paren_index;
                    cap2 = &cap2[..last_paren_index];
                    cap0 = utf16::trim(&cap0[..link_length.min(cap0.len())]);
                    cap3 = Some(&[]);
                }
                None => {}
            }
        }
        let title = match cap3 {
            Some(title) if !title.is_empty() => utf16::slice(title, 1, Some(-1)),
            _ => &[],
        };
        let mut href = utf16::trim(cap2);
        if OTHER.start_angle_bracket.test(href) {
            href = utf16::slice(href, 1, Some(-1));
        }
        let href = if href.is_empty() {
            Vec::new()
        } else {
            unescape(href)
        };
        let title = if title.is_empty() {
            Vec::new()
        } else {
            unescape(title)
        };
        let raw = src.slice_of(cap0);
        output_link(self, cap0, &src.slice_of(cap1), href, Some(title), raw).map(Some)
    }

    pub(super) fn reflink(&mut self, src: &Units) -> Result<Option<Token>, Error> {
        let Some(cap) = INLINE.reflink.exec(src).or_else(|| INLINE.nolink.exec(src)) else {
            return Ok(None);
        };
        let label = cap.truthy(2).or(cap.get(1)).unwrap_or_default();
        let link_string = OTHER.multiple_space_global.replace(label, " ");
        let Some(Link { href, title }) = self.links.get(&utf16::to_lower_case(&link_string)) else {
            let text = src.slice_of(&cap.all()[..1]);
            return Ok(Some(Token {
                text: Some(text.clone()),
                ..Token::new("text", text)
            }));
        };
        let (href, title) = (href.clone(), title.clone());
        let raw = src.slice_of(cap.all());
        let cap1 = src.slice_of(cap.get(1).unwrap_or_default());
        output_link(self, cap.all(), &cap1, href, title, raw).map(Some)
    }

    pub(super) fn em_strong(
        &mut self,
        src: &Units,
        masked_src: &[u16],
        prev_char: &[u16],
    ) -> Result<Option<Token>, Error> {
        let Some(found) = INLINE.em_strong_l_delim.exec(src) else {
            return Ok(None);
        };
        if (1..=4).all(|group| found.truthy(group).is_none()) {
            return Ok(None);
        }
        // _ can't be between two alphanumerics.
        if found.truthy(4).is_some() && OTHER.unicode_alpha_numeric.exec(prev_char).is_some() {
            return Ok(None);
        }
        let next_char = found.truthy(1).or(found.truthy(3));
        if !(next_char.is_none()
            || prev_char.is_empty()
            || INLINE.punctuation.exec(prev_char).is_some())
        {
            return Ok(None);
        }
        let left_length = utf16::code_points(found.all()) - 1;
        let mut delim_total = left_length as isize;
        let mut mid_delim_total = 0isize;
        let delimiter = found.all()[0];
        // Clip maskedSrc to the same section of the string as src.
        let masked = utf16::slice(
            masked_src,
            -(src.len() as isize) + left_length as isize,
            None,
        );
        let mut last_index = 0;
        while let Some(right) = delimiters::em_strong(masked, last_index, delimiter) {
            last_index = right.end;
            let Some((right_delim, flank)) = right.run else {
                continue;
            };
            let mut right_length = right_delim.len() as isize;
            if flank == Flank::Open {
                delim_total += right_length;
                continue;
            } else if flank == Flank::Either
                && !left_length.is_multiple_of(3)
                && (left_length as isize + right_length) % 3 == 0
            {
                // CommonMark emphasis rules 9 and 10.
                mid_delim_total += right_length;
                continue;
            }
            delim_total -= right_length;
            if delim_total > 0 {
                continue;
            }
            // Remove extra characters: *a*** becomes *a*.
            right_length = right_length.min(right_length + delim_total + mid_delim_total);
            let raw = src.slice_of(utf16::slice(
                src,
                0,
                Some(left_length as isize + right_delim.start as isize + right_length),
            ));
            // An odd smallest delimiter makes emphasis, an even one strong.
            let (kind, trim) = if (left_length as isize).min(right_length) % 2 != 0 {
                ("em", 1)
            } else {
                ("strong", 2)
            };
            let text = raw.slice_of(utf16::slice(&raw, trim, Some(-trim)));
            let tokens = self.inline_tokens(&text)?;
            return Ok(Some(Token {
                text: Some(text),
                tokens: Some(tokens.into()),
                ..Token::new(kind, raw)
            }));
        }
        Ok(None)
    }

    pub(super) fn codespan(&self, src: &Units) -> Option<Token> {
        let cap = INLINE.code.exec(src)?;
        let mut text = OTHER
            .new_line_char_global
            .replace_units(&src.slice_of(cap.get(2).unwrap_or_default()), " ");
        let has_non_space_chars = OTHER.non_space_char.test(&text);
        let has_space_chars_on_both_ends =
            OTHER.starting_space_char.test(&text) && OTHER.ending_space_char.test(&text);
        if has_non_space_chars && has_space_chars_on_both_ends {
            text = text.slice(1..text.len() - 1);
        }
        Some(Token {
            text: Some(text),
            ..Token::new("codespan", src.slice_of(cap.all()))
        })
    }

    pub(super) fn br(&self, src: &Units) -> Option<Token> {
        let cap = INLINE.br.exec(src)?;
        Some(Token::new("br", src.slice_of(cap.all())))
    }

    pub(super) fn del(
        &mut self,
        src: &Units,
        masked_src: &[u16],
        prev_char: &[u16],
    ) -> Result<Option<Token>, Error> {
        let Some(found) = INLINE.del_l_delim.exec(src) else {
            return Ok(None);
        };
        let next_char = found.truthy(1);
        if !(next_char.is_none()
            || prev_char.is_empty()
            || INLINE.punctuation.exec(prev_char).is_some())
        {
            return Ok(None);
        }
        let left_length = utf16::code_points(found.all()) - 1;
        let mut delim_total = left_length as isize;
        // Clip maskedSrc to the same section of the string as src.
        let masked = utf16::slice(
            masked_src,
            -(src.len() as isize) + left_length as isize,
            None,
        );
        let mut last_index = 0;
        while let Some(right) = delimiters::del(masked, last_index) {
            last_index = right.end;
            let Some((right_delim, flank)) = right.run else {
                continue;
            };
            let mut right_length = right_delim.len() as isize;
            if right_length != left_length as isize {
                continue;
            }
            if flank == Flank::Open {
                delim_total += right_length;
                continue;
            }
            delim_total -= right_length;
            if delim_total > 0 {
                continue;
            }
            right_length = right_length.min(right_length + delim_total);
            let raw = src.slice_of(utf16::slice(
                src,
                0,
                Some(left_length as isize + right_delim.start as isize + right_length),
            ));
            let text = raw.slice_of(utf16::slice(
                &raw,
                left_length as isize,
                Some(-(left_length as isize)),
            ));
            let tokens = self.inline_tokens(&text)?;
            return Ok(Some(Token {
                text: Some(text),
                tokens: Some(tokens.into()),
                ..Token::new("del", raw)
            }));
        }
        Ok(None)
    }

    pub(super) fn autolink(&self, src: &Units) -> Option<Token> {
        let cap = INLINE.autolink.exec(src)?;
        let text = src.slice_of(cap.get(1).unwrap_or_default());
        let href = if cap.get(2).is_some_and(|at| utf16::is(at, "@")) {
            utf16::concat(&[&utf16::from("mailto:"), &text])
        } else {
            text.to_vec()
        };
        Some(link_token(src.slice_of(cap.all()), text, href))
    }

    pub(super) fn url(&self, src: &Units) -> Option<Token> {
        let cap = matchers::url(src)?;
        let (text, href);
        if cap.get(2).is_some_and(|at| utf16::is(at, "@")) {
            text = src.slice_of(cap.all());
            href = utf16::concat(&[&utf16::from("mailto:"), &text]);
        } else {
            let mut cap0 = cap.all();
            loop {
                let previous = cap0;
                cap0 = INLINE
                    .backpedal
                    .exec(cap0)
                    .map_or(&[][..], |found| found.all());
                if previous == cap0 {
                    break;
                }
            }
            text = src.slice_of(cap0);
            href = if cap.get(1).is_some_and(|scheme| utf16::is(scheme, "www.")) {
                utf16::concat(&[&utf16::from("http://"), &text])
            } else {
                text.to_vec()
            };
        }
        Some(link_token(text.clone(), text, href))
    }

    pub(super) fn inline_text(&self, src: &Units, cut: usize) -> Option<Token> {
        let text = src.slice(0..matchers::inline_text(&src[..cut])?);
        Some(Token {
            text: Some(text.clone()),
            ..Token::new("text", text)
        })
    }
}

/// An autolink's or URL's token, whose one child is its text.
fn link_token(raw: Units, text: Units, href: Vec<u16>) -> Token {
    Token {
        data: TokenData::Link(Box::new(Destination { href, title: None })),
        tokens: Some(
            vec![Token {
                text: Some(text.clone()),
                ..Token::new("text", text.clone())
            }]
            .into(),
        ),
        text: Some(text),
        ..Token::new("link", raw)
    }
}

/// `lines.join('\n')`, which is the one line itself when there is one.
fn join(lines: &[Units]) -> Units {
    match lines {
        [line] => line.clone(),
        _ => {
            let parts: Vec<&[u16]> = lines.iter().map(|line| &line[..]).collect();
            Units::from(utf16::join(&parts, &[utf16::unit(b'\n')]))
        }
    }
}

/// `oldRaw + '\n' + lines.join('\n')`.
fn join_rest(old_raw: &[u16], lines: &[Units]) -> Units {
    let newline = [utf16::unit(b'\n')];
    Units::from(utf16::concat(&[old_raw, &newline, &join(lines)]))
}
