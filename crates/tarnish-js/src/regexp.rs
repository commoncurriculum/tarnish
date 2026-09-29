//! A JavaScript `RegExp` on UTF-16 strings, run by regress. A regex with the `u` flag reads
//! surrogate pairs as one code point, and one without reads each code unit alone, as
//! JavaScript does. A `g` regex's `lastIndex` is the caller's, passed to [`RegExp::exec_at`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use regress::{Flags, Regex};

use crate::units::Units;
use crate::utf16;

pub struct RegExp {
    regex: Arc<Regex>,
    unicode: bool,
    global: bool,
    anchored: bool,
}

pub struct Match<'t> {
    text: &'t [u16],
    groups: Vec<Option<Range<usize>>>,
}

impl<'t> Match<'t> {
    /// `match.index`.
    pub fn index(&self) -> usize {
        self.range(0).start
    }

    /// `lastIndex` after the match.
    pub fn end(&self) -> usize {
        self.range(0).end
    }

    fn range(&self, group: usize) -> Range<usize> {
        self.groups[group].clone().expect("the match")
    }

    /// `match[0]`.
    pub fn all(&self) -> &'t [u16] {
        &self.text[self.range(0)]
    }

    /// `match[group]`, `None` for `undefined`.
    pub fn get(&self, group: usize) -> Option<&'t [u16]> {
        self.groups
            .get(group)?
            .clone()
            .map(|range| &self.text[range])
    }

    /// `match[group]` as JavaScript tests it: an unmatched or empty group is falsy.
    pub fn truthy(&self, group: usize) -> Option<&'t [u16]> {
        self.get(group).filter(|text| !text.is_empty())
    }
}

thread_local! {
    /// The group vectors of dropped matches, for the next ones.
    static SPARE_GROUPS: RefCell<Vec<Vec<Option<Range<usize>>>>> = const { RefCell::new(Vec::new()) };
}

impl Drop for Match<'_> {
    fn drop(&mut self) {
        let groups = std::mem::take(&mut self.groups);
        SPARE_GROUPS.with_borrow_mut(|spare| spare.push(groups));
    }
}

/// How many compiled regexes a thread keeps for the regexes marked builds from the input, as
/// JavaScriptCore keeps them by source and flags.
const CACHED_REGEXES: usize = 256;

thread_local! {
    static COMPILED: RefCell<HashMap<(String, String), Arc<Regex>>> = RefCell::new(HashMap::new());
}

impl RegExp {
    /// `new RegExp(source, flags)`.
    pub fn new(source: &str, flags: &str) -> Self {
        let flags_compiled = flags.replace(['g', 'y', 'd'], "");
        let key = (source.to_owned(), flags_compiled);
        let regex = COMPILED.with_borrow_mut(|compiled| {
            if let Some(regex) = compiled.get(&key) {
                return regex.clone();
            }
            let regex = Arc::new(
                Regex::with_flags(source, Flags::from(key.1.as_str()))
                    .unwrap_or_else(|error| panic!("/{source}/{flags}: {error}")),
            );
            if compiled.len() == CACHED_REGEXES {
                compiled.clear();
            }
            compiled.insert(key, regex.clone());
            regex
        });
        RegExp {
            anchored: regex.is_start_anchored(),
            regex,
            unicode: flags.contains('u'),
            global: flags.contains('g'),
        }
    }

    /// `exec` from `lastIndex` 0.
    pub fn exec<'t>(&self, text: &'t [u16]) -> Option<Match<'t>> {
        self.exec_at(text, 0)
    }

    /// `exec` from `lastIndex`.
    #[inline]
    pub fn exec_at<'t>(&self, text: &'t [u16], last_index: usize) -> Option<Match<'t>> {
        if last_index > text.len() {
            return None;
        }
        // Most tokenizers try a rule that can't start with the next character, which this
        // settles before the engine is set up.
        if self.anchored
            && text
                .get(last_index)
                .is_some_and(|&unit| !self.regex.can_start_with(unit))
        {
            return None;
        }
        self.run(text, last_index)
    }

    #[inline(never)]
    fn run<'t>(&self, text: &'t [u16], last_index: usize) -> Option<Match<'t>> {
        let mut groups = SPARE_GROUPS.with_borrow_mut(Vec::pop).unwrap_or_default();
        if self
            .regex
            .find_utf16_into(text, last_index, self.unicode, &mut groups)
        {
            Some(Match { text, groups })
        } else {
            SPARE_GROUPS.with_borrow_mut(|spare| spare.push(groups));
            None
        }
    }

    /// `test`, for a regex without the `g` flag.
    pub fn test(&self, text: &[u16]) -> bool {
        debug_assert!(!self.global, "test on a g regex depends on its lastIndex");
        self.exec(text).is_some()
    }

    /// `replace(regex, replacement)`, where the replacement may hold `$1` to `$9`, `$&` and `$$`.
    pub fn replace(&self, text: &[u16], replacement: &str) -> Vec<u16> {
        let replacement = utf16::from(replacement);
        self.replace_with(text, |found| expand(&replacement, found))
    }

    /// [`RegExp::replace`] on shared units. When what is left is one stretch of them, the result
    /// shares it, as JavaScriptCore's does.
    pub fn replace_units(&self, text: &Units, replacement: &str) -> Units {
        let replacement = utf16::from(replacement);
        // Stretches of `text` kept, and the units put between them.
        let mut pieces: Vec<Result<Range<usize>, Vec<u16>>> = Vec::new();
        let mut copied = 0;
        let mut last_index = 0;
        while let Some(found) = self.exec_at(text, last_index) {
            pieces.push(Ok(copied..found.index()));
            pieces.push(Err(expand(&replacement, &found)));
            copied = found.end();
            if !self.global {
                break;
            }
            last_index = if found.end() == found.index() {
                self.advance(text, found.end())
            } else {
                found.end()
            };
        }
        pieces.push(Ok(copied.min(text.len())..text.len()));
        pieces.retain(|piece| match piece {
            Ok(range) => !range.is_empty(),
            Err(units) => !units.is_empty(),
        });
        match pieces.as_slice() {
            [] => Units::default(),
            [Ok(range)] => text.slice(range.clone()),
            _ => Units::from(
                pieces
                    .iter()
                    .flat_map(|piece| match piece {
                        Ok(range) => &text[range.clone()],
                        Err(units) => units.as_slice(),
                    })
                    .copied()
                    .collect::<Vec<u16>>(),
            ),
        }
    }

    /// `replace(regex, (match, ...) => ...)`: every match with the `g` flag, or the first.
    pub fn replace_with(
        &self,
        text: &[u16],
        mut replace: impl FnMut(&Match) -> Vec<u16>,
    ) -> Vec<u16> {
        let mut result = Vec::with_capacity(text.len());
        let mut copied = 0;
        let mut last_index = 0;
        while let Some(found) = self.exec_at(text, last_index) {
            result.extend_from_slice(&text[copied..found.index()]);
            result.extend(replace(&found));
            copied = found.end();
            if !self.global {
                break;
            }
            last_index = if found.end() == found.index() {
                self.advance(text, found.end())
            } else {
                found.end()
            };
        }
        result.extend_from_slice(&text[copied.min(text.len())..]);
        result
    }

    /// `split(regex)`, for a regex without capturing groups.
    pub fn split<'t>(&self, text: &'t [u16]) -> Vec<&'t [u16]> {
        let size = text.len();
        if size == 0 {
            return if self.exec(text).is_some() {
                Vec::new()
            } else {
                vec![text]
            };
        }
        let mut parts = Vec::new();
        let (mut p, mut q) = (0, 0);
        while q < size {
            let Some(found) = self.exec_at(text, q).filter(|found| found.index() < size) else {
                break;
            };
            let end = found.end().min(size);
            if end == p {
                q = self.advance(text, found.index());
                continue;
            }
            parts.push(&text[p..found.index()]);
            p = end;
            q = p;
        }
        parts.push(&text[p..]);
        parts
    }

    /// `AdvanceStringIndex`: past one code point with the `u` flag, else one code unit.
    fn advance(&self, text: &[u16], index: usize) -> usize {
        if self.unicode && index + 1 < text.len() {
            let pair = char::decode_utf16(text[index..index + 2].iter().copied()).next();
            if matches!(pair, Some(Ok(character)) if character.len_utf16() == 2) {
                return index + 2;
            }
        }
        index + 1
    }
}

/// `GetSubstitution` for the replacement patterns marked uses.
fn expand(replacement: &[u16], found: &Match) -> Vec<u16> {
    let mut result = Vec::with_capacity(replacement.len());
    let mut units = replacement.iter().copied().peekable();
    while let Some(unit) = units.next() {
        if unit != u16::from(b'$') {
            result.push(unit);
            continue;
        }
        match units.peek().copied() {
            Some(next) if next == u16::from(b'$') => {
                units.next();
                result.push(next);
            }
            Some(next) if next == u16::from(b'&') => {
                units.next();
                result.extend_from_slice(found.all());
            }
            Some(next) if (u16::from(b'1')..=u16::from(b'9')).contains(&next) => {
                let group = usize::from(next - u16::from(b'0'));
                if group < found.groups.len() {
                    units.next();
                    result.extend_from_slice(found.get(group).unwrap_or_default());
                } else {
                    result.push(unit);
                }
            }
            _ => result.push(unit),
        }
    }
    result
}
