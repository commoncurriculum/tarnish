//! `Lexer.ts`.

use std::borrow::Cow;
use std::collections::HashMap;

use tarnish_js::stack;

use super::Marked;
use super::matchers;
use super::rules::{INLINE, OTHER};
use super::{Token, TokenData, Tokens};
use tarnish_js::deadline::Deadline;
use tarnish_js::units::Units;
use tarnish_js::{Error, utf16};

/// A reference definition, as `tokens.links` holds it.
pub struct Link {
    pub href: Vec<u16>,
    pub title: Option<Vec<u16>>,
}

#[derive(Default)]
pub struct State {
    pub in_link: bool,
    pub top: bool,
}

/// An inline queue entry: the text to lex, and the slot its tokens go to.
struct Queued {
    src: Units,
    slot: u32,
}

pub struct Lexer<'m> {
    pub(super) marked: &'m Marked,
    /// `tokens.links`.
    pub links: HashMap<Vec<u16>, Link>,
    pub state: State,
    inline_queue: Vec<Queued>,
    slots: u32,
    deadline: Deadline,
}

impl<'m> Lexer<'m> {
    pub fn new(marked: &'m Marked) -> Self {
        Lexer {
            marked,
            links: HashMap::new(),
            state: State {
                top: true,
                ..State::default()
            },
            inline_queue: Vec::new(),
            slots: 0,
            deadline: Deadline::current(),
        }
    }

    /// `lex(src)`.
    pub fn lex(&mut self, src: &[u16]) -> Result<Vec<Token>, Error> {
        let src = Units::from(if src.contains(&utf16::unit(b'\r')) {
            OTHER.carriage_return.replace(src, "\n")
        } else {
            src.to_vec()
        });
        let mut tokens = Vec::new();
        self.block_tokens(&src, &mut tokens, false)?;
        let mut lexed = vec![None; self.slots as usize];
        for Queued { src, slot } in std::mem::take(&mut self.inline_queue) {
            lexed[slot as usize] = Some(self.inline_tokens(&src)?);
        }
        fill(&mut tokens, &mut lexed);
        Ok(tokens)
    }

    /// `blockTokens(src, tokens, lastParagraphClipped)`.
    pub fn block_tokens(
        &mut self,
        src: &Units,
        tokens: &mut Vec<Token>,
        mut last_paragraph_clipped: bool,
    ) -> Result<(), Error> {
        let mut src = src.clone();
        while !src.is_empty() {
            self.deadline.turn(src.len())?;
            if let Some(token) = self.space(&src) {
                src = src.substring(token.raw.len());
                match tokens.last_mut() {
                    // A single \n as a spacer ends the last line.
                    Some(last) if token.raw.len() == 1 => last.raw.push(utf16::unit(b'\n')),
                    _ => tokens.push(token),
                }
                continue;
            }

            if let Some(token) = self.code(&src) {
                src = src.substring(token.raw.len());
                match tokens.last_mut() {
                    // An indented code block cannot interrupt a paragraph.
                    Some(last) if matches!(last.kind, "paragraph" | "text") => {
                        self.continue_last(last, &token.raw, token.text());
                    }
                    _ => tokens.push(token),
                }
                continue;
            }

            if let Some(token) = self.fences(&src) {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.heading(&src) {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.hr(&src) {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.blockquote(&src)? {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = (self.marked.list)(self, &src)? {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.html(&src) {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.def(&src) {
                src = src.substring(token.raw.len());
                let TokenData::Def(def) = &token.data else {
                    unreachable!("a definition")
                };
                match tokens.last_mut() {
                    Some(last) if matches!(last.kind, "paragraph" | "text") => {
                        self.continue_last(last, &token.raw, &token.raw);
                    }
                    _ if !self.links.contains_key(&def.tag) => {
                        let link = Link {
                            href: def.href.clone(),
                            title: def.title.clone(),
                        };
                        self.links.insert(def.tag.clone(), link);
                        tokens.push(token);
                    }
                    _ => {}
                }
                continue;
            }

            if let Some(token) = self.table(&src) {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.lheading(&src) {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if self.state.top
                && let Some(token) = self.paragraph(&src)
            {
                src = src.substring(token.raw.len());
                match tokens.last_mut() {
                    Some(last) if last_paragraph_clipped && last.kind == "paragraph" => {
                        self.inline_queue.pop();
                        self.continue_last(last, &token.raw, token.text());
                    }
                    _ => tokens.push(token),
                }
                last_paragraph_clipped = false;
                continue;
            }

            if let Some(token) = self.text(&src) {
                src = src.substring(token.raw.len());
                match tokens.last_mut() {
                    Some(last) if last.kind == "text" => {
                        self.inline_queue.pop();
                        self.continue_last(last, &token.raw, token.text());
                    }
                    _ => tokens.push(token),
                }
                continue;
            }

            return Err(Error::Other(format!("Infinite loop on byte: {}", src[0])));
        }
        self.state.top = true;
        Ok(())
    }

    /// `inline(src)`: queues the text to be lexed once the blocks are, and returns the slot its
    /// tokens will fill.
    pub(super) fn inline(&mut self, src: Units) -> u32 {
        let slot = self.slots;
        self.slots += 1;
        self.inline_queue.push(Queued { src, slot });
        slot
    }

    /// Appends a line to the last token, which the last queue entry now lexes all of.
    fn continue_last(&mut self, last: &mut Token, raw: &[u16], text: &[u16]) {
        if last.raw.last() != Some(&utf16::unit(b'\n')) {
            last.raw.push(utf16::unit(b'\n'));
        }
        last.raw.extend_from_slice(raw);
        // The queue entry lets go of the text first, so that the text grows in place.
        if let Some(queued) = self.inline_queue.last_mut() {
            queued.src = Units::default();
        }
        let last_text = last.text.get_or_insert_with(Units::default);
        last_text.push(utf16::unit(b'\n'));
        last_text.extend_from_slice(text);
        if let Some(queued) = self.inline_queue.last_mut() {
            queued.src = last_text.clone();
        }
    }

    /// `inlineTokens(src)`, which the inline tokenizers call for what they hold.
    pub fn inline_tokens(&mut self, src: &Units) -> Result<Vec<Token>, Error> {
        stack::grow(|| self.lex_inline_tokens(src))
    }

    fn lex_inline_tokens(&mut self, src: &Units) -> Result<Vec<Token>, Error> {
        let marked = self.marked;
        let mut tokens: Vec<Token> = Vec::new();
        let mut masked: Cow<[u16]> = Cow::Borrowed(src);

        if !self.links.is_empty() {
            let mut last_index = 0;
            let reflink = |masked: &[u16], last_index| {
                let found = INLINE.reflink_search.exec_at(masked, last_index)?;
                let all = found.all();
                let open = all
                    .iter()
                    .rposition(|&unit| unit == utf16::unit(b'['))
                    .map_or(0, |open| open + 1);
                let label = utf16::slice(all, open as isize, Some(-1));
                Some((found.index(), found.end(), self.links.contains_key(label)))
            };
            while let Some((index, end, defined)) = reflink(&masked, last_index) {
                last_index = end;
                if defined {
                    mask(masked.to_mut(), index, end);
                }
            }
        }

        // Most text has no backslash, which a scan finds sooner than the regex engine does.
        if masked.contains(&utf16::BACKSLASH) {
            let mut last_index = 0;
            while let Some((index, end)) = INLINE
                .any_punctuation
                .exec_at(&masked, last_index)
                .map(|found| (found.index(), found.end()))
            {
                masked.to_mut().splice(index..end, [utf16::unit(b'+'); 2]);
                last_index = end;
            }
        }

        let mut last_index = 0;
        while let Some(found) = matchers::block_skip(&masked, last_index) {
            mask(masked.to_mut(), found.start, found.end);
            last_index = found.end;
        }

        let mut starts = Starts::new(marked, src);
        let mut src = src.clone();
        let mut keep_prev_char = false;
        let mut prev_char: Vec<u16> = Vec::new();
        'tokens: while !src.is_empty() {
            self.deadline.turn(src.len())?;
            if !keep_prev_char {
                prev_char.clear();
            }
            keep_prev_char = false;

            for extension in &marked.inline {
                if !utf16::starts_with(&src, extension.start) {
                    continue;
                }
                if let Some(token) = (extension.tokenizer)(self, &src, &tokens)? {
                    src = src.substring(token.raw.len());
                    tokens.push(token);
                    continue 'tokens;
                }
            }

            if let Some(token) = self.escape(&src) {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.tag(&src) {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.link(&src)? {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.reflink(&src)? {
                src = src.substring(token.raw.len());
                push_merging_text(&mut tokens, token);
                continue;
            }

            if let Some(token) = self.em_strong(&src, &masked, &prev_char)? {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.codespan(&src) {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.br(&src) {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.del(&src, &masked, &prev_char)? {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if let Some(token) = self.autolink(&src) {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            if !self.state.in_link
                && let Some(token) = self.url(&src)
            {
                src = src.substring(token.raw.len());
                tokens.push(token);
                continue;
            }

            // Text stops where an extension starts, so the extension gets to read it.
            let cut = match starts.index(&src) {
                Some(start) => (start + 1).min(src.len()),
                None => src.len(),
            };
            if let Some(token) = self.inline_text(&src, cut) {
                src = src.substring(token.raw.len());
                // Track the character before a run of _.
                if token.raw.last() != Some(&utf16::unit(b'_')) {
                    prev_char = token.raw.last().map(|unit| vec![*unit]).unwrap_or_default();
                }
                keep_prev_char = true;
                push_merging_text(&mut tokens, token);
                continue;
            }

            return Err(Error::Other(format!("Infinite loop on byte: {}", src[0])));
        }
        Ok(tokens)
    }
}

/// Where an extension next starts in the source `inline_tokens` lexes. The rest of the source
/// only shrinks from the front, so the place found stays the nearest until the rest starts past
/// it.
struct Starts<'m> {
    marked: &'m Marked,
    length: usize,
    /// Where the last search began (`usize::MAX` before one), and what it found, in the whole
    /// source.
    searched_from: usize,
    nearest: Option<usize>,
}

impl<'m> Starts<'m> {
    fn new(marked: &'m Marked, src: &[u16]) -> Self {
        Starts {
            marked,
            length: src.len(),
            searched_from: usize::MAX,
            nearest: None,
        }
    }

    /// The nearest place in `rest.slice(1)` an extension starts, where `rest` ends the source:
    /// the least of each start string's `indexOf`.
    fn index(&mut self, rest: &[u16]) -> Option<usize> {
        let from = self.length - rest.len() + 1;
        let stale = match self.nearest {
            Some(nearest) => nearest < from,
            None => self.searched_from > from,
        };
        if stale {
            let after = utf16::substring(rest, 1);
            self.searched_from = from;
            let marked = self.marked;
            let mut at = 0;
            self.nearest = loop {
                let Some(offset) = after[at..]
                    .iter()
                    .position(|&unit| marked.is_first_unit(unit))
                else {
                    break None;
                };
                at += offset;
                let starts = marked
                    .inline
                    .iter()
                    .any(|extension| utf16::starts_with(&after[at..], extension.start));
                if starts {
                    break Some(at + from);
                }
                at += 1;
            };
        }
        self.nearest.map(|nearest| nearest - from)
    }
}

/// Pushes `token`, or adds it to the last token when both are text.
fn push_merging_text(tokens: &mut Vec<Token>, token: Token) {
    match tokens.last_mut() {
        Some(last) if token.kind == "text" && last.kind == "text" => {
            last.raw.extend_from_slice(&token.raw);
            last.text
                .get_or_insert_with(Units::default)
                .extend_from_slice(token.text());
        }
        _ => tokens.push(token),
    }
}

/// `masked = masked.slice(0, start) + '[' + 'a'.repeat(end - start - 2) + ']' +
/// masked.slice(end)`.
fn mask(masked: &mut Vec<u16>, start: usize, end: usize) {
    let hidden = std::iter::once(utf16::unit(b'['))
        .chain(std::iter::repeat_n(
            utf16::unit(b'a'),
            (end - start).saturating_sub(2),
        ))
        .chain(std::iter::once(utf16::unit(b']')));
    masked.splice(start..end, hidden);
}

/// Gives each queued token and table cell the inline tokens its queue entry lexed to.
fn fill(tokens: &mut [Token], lexed: &mut [Option<Vec<Token>>]) {
    for token in tokens {
        match token.queued.take() {
            Some(slot) => token.tokens = lexed[slot as usize].take().map(Tokens::from),
            None => {
                if let Some(children) = token.tokens.as_mut() {
                    stack::grow(|| fill(children, lexed));
                }
            }
        }
        match &mut token.data {
            TokenData::List(list) => stack::grow(|| fill(&mut list.items, lexed)),
            TokenData::Table(table) => {
                for cell in table
                    .header
                    .iter_mut()
                    .chain(table.rows.iter_mut().flatten())
                {
                    if let Some(slot) = cell.queued.take() {
                        cell.tokens = lexed[slot as usize].take().unwrap_or_default();
                    }
                }
            }
            TokenData::None
            | TokenData::Heading { .. }
            | TokenData::Link(_)
            | TokenData::Html { .. }
            | TokenData::Def(_)
            | TokenData::ListItem { .. } => {}
        }
    }
}
