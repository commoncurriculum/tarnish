//! A JavaScript string's UTF-16 code units, shared with the string they were cut from as
//! JavaScriptCore shares a substring's. A lexer that keeps slices of its input at every level
//! of nesting then holds the input once, as JavaScript does, rather than once per level.

use std::fmt;
use std::ops::{Deref, Range};
use std::rc::Rc;

/// Offsets into the buffer are `u32`s, which keeps the tokens that hold these small: a request
/// holds far fewer units than one counts.
#[derive(Clone)]
pub struct Units {
    buffer: Rc<Vec<u16>>,
    start: u32,
    end: u32,
}

fn offset(index: usize) -> u32 {
    u32::try_from(index).expect("fewer than 2^32 units")
}

thread_local! {
    static EMPTY: Rc<Vec<u16>> = Rc::new(Vec::new());
}

impl Default for Units {
    fn default() -> Self {
        Units {
            buffer: EMPTY.with(Rc::clone),
            start: 0,
            end: 0,
        }
    }
}

impl Units {
    /// The units of `units` from `range`, sharing them.
    pub fn slice(&self, range: Range<usize>) -> Units {
        assert!(range.start <= range.end && range.end <= self.len());
        Units {
            buffer: self.buffer.clone(),
            start: self.start + offset(range.start),
            end: self.start + offset(range.end),
        }
    }

    /// `substring(start)`, sharing the units.
    pub fn substring(&self, start: usize) -> Units {
        self.slice(start.min(self.len())..self.len())
    }

    /// The units of `part`, a slice of these units, sharing them.
    pub fn slice_of(&self, part: &[u16]) -> Units {
        if part.is_empty() {
            return Units::default();
        }
        let offset = (part.as_ptr() as usize).wrapping_sub(self.as_ptr() as usize) / 2;
        assert!(
            part.as_ptr() >= self.as_ptr() && offset + part.len() <= self.len(),
            "a slice of these units"
        );
        self.slice(offset..offset + part.len())
    }

    /// Appends `units`: in place when nothing else shares the buffer, so that appending again
    /// and again takes linear time, as JavaScript's ropes do.
    pub fn extend_from_slice(&mut self, units: &[u16]) {
        if let Some(buffer) = Rc::get_mut(&mut self.buffer) {
            buffer.truncate(self.end as usize);
            buffer.extend_from_slice(units);
            self.end = offset(buffer.len());
            return;
        }
        let mut owned = Vec::with_capacity((self.len() + units.len()).max(2 * self.len()));
        owned.extend_from_slice(self);
        owned.extend_from_slice(units);
        *self = Units::from(owned);
    }

    pub fn push(&mut self, unit: u16) {
        self.extend_from_slice(&[unit]);
    }
}

impl From<Vec<u16>> for Units {
    fn from(units: Vec<u16>) -> Self {
        Units {
            end: offset(units.len()),
            buffer: Rc::new(units),
            start: 0,
        }
    }
}

impl From<&[u16]> for Units {
    fn from(units: &[u16]) -> Self {
        Units::from(units.to_vec())
    }
}

impl Deref for Units {
    type Target = [u16];

    fn deref(&self) -> &[u16] {
        &self.buffer[self.start as usize..self.end as usize]
    }
}

impl AsRef<[u16]> for Units {
    fn as_ref(&self) -> &[u16] {
        self
    }
}

impl PartialEq for Units {
    fn eq(&self, other: &Units) -> bool {
        **self == **other
    }
}

impl fmt::Debug for Units {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&String::from_utf16_lossy(self), formatter)
    }
}

impl Units {
    /// Whether both hold units of one buffer.
    pub fn shares(&self, other: &Units) -> bool {
        Rc::ptr_eq(&self.buffer, &other.buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::Units;
    use crate::utf16;

    #[test]
    fn slices_share_and_appends_copy_only_what_is_shared() {
        let whole = Units::from(utf16::from("abcdef"));
        let middle = whole.slice(1..4);
        assert_eq!(&*middle, &utf16::from("bcd")[..]);
        assert!(middle.shares(&whole));
        assert!(whole.slice_of(&middle[1..]).shares(&whole));

        let mut grown = middle.clone();
        grown.extend_from_slice(&utf16::from("xy"));
        assert_eq!(&*grown, &utf16::from("bcdxy")[..]);
        assert_eq!(&*whole, &utf16::from("abcdef")[..]);
        assert!(!grown.shares(&whole));

        // The copy left room to grow, and nothing else holds it.
        let pointer = grown.as_ptr();
        grown.push(utf16::unit(b'z'));
        assert_eq!(grown.as_ptr(), pointer);
        assert_eq!(&*grown, &utf16::from("bcdxyz")[..]);
    }
}
