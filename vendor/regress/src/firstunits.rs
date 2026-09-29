//! (Added for tarnish-js.) The code units a match can start with, for UTF-16 and UCS-2
//! input, where the byte-based start predicates don't apply. It is computed from the parsed IR and
//! over-approximates: a unit it rejects can't start a match, and one it accepts may. It says
//! nothing about a match at the end of the input, where there is no unit to reject.

use crate::ir::{AnchorType, Node};

#[derive(Debug, Clone, Copy)]
pub struct FirstUnits {
    /// The ASCII units a match can start with.
    ascii: u128,
    /// Whether a match can start with a unit past ASCII.
    non_ascii: bool,
    /// The units, when they are one to three ASCII units, repeated to fill three, which a scan
    /// compares many units at a time with.
    few: Option<[u16; 3]>,
}

impl FirstUnits {
    fn new(ascii: u128, non_ascii: bool) -> FirstUnits {
        let count = ascii.count_ones();
        let few = (!non_ascii && (1..=3).contains(&count)).then(|| {
            let mut units = [0u16; 3];
            let mut rest = ascii;
            for slot in 0..3 {
                if rest != 0 {
                    units[slot] = rest.trailing_zeros() as u16;
                    rest &= rest - 1;
                } else {
                    units[slot] = units[0];
                }
            }
            units
        });
        FirstUnits {
            ascii,
            non_ascii,
            few,
        }
    }

    /// Whether a match can start with `unit`.
    #[inline]
    pub fn accepts(&self, unit: u32) -> bool {
        if unit < 128 {
            self.ascii & (1u128 << unit) != 0
        } else {
            self.non_ascii
        }
    }

    /// How many units at the start of `units` can't start a match.
    #[inline]
    pub fn skip(&self, units: &[u16]) -> usize {
        let Some([a, b, c]) = self.few else {
            return units
                .iter()
                .position(|&unit| self.accepts(unit.into()))
                .unwrap_or(units.len());
        };
        let found = |unit: u16| (unit == a) | (unit == b) | (unit == c);
        // Sixteen units compared with no branch between them, which the compiler vectorizes.
        let mut offset = 0;
        for chunk in units.chunks_exact(16) {
            if chunk.iter().fold(false, |hit, &unit| hit | found(unit)) {
                break;
            }
            offset += 16;
        }
        offset
            + units[offset..]
                .iter()
                .position(|&unit| found(unit))
                .unwrap_or(units.len() - offset)
    }
}

/// The first units of what a node matches, and whether it can match the empty string.
#[derive(Clone, Copy)]
struct First {
    ascii: u128,
    non_ascii: bool,
    nullable: bool,
}

impl First {
    const NOTHING: First = First {
        ascii: 0,
        non_ascii: false,
        nullable: false,
    };
    const EMPTY: First = First {
        ascii: 0,
        non_ascii: false,
        nullable: true,
    };
    const ANY: First = First {
        ascii: u128::MAX,
        non_ascii: true,
        nullable: false,
    };

    fn code_point(cp: u32) -> First {
        let mut first = First::NOTHING;
        first.add(cp);
        first
    }

    fn add(&mut self, cp: u32) {
        if cp < 128 {
            self.ascii |= 1u128 << cp;
        } else {
            self.non_ascii = true;
        }
    }

    fn union(self, other: First) -> First {
        First {
            ascii: self.ascii | other.ascii,
            non_ascii: self.non_ascii || other.non_ascii,
            nullable: self.nullable || other.nullable,
        }
    }
}

fn first(node: &Node) -> First {
    match node {
        Node::Empty | Node::Goal => First::EMPTY,
        Node::Char { c } => First::code_point(*c),
        Node::ByteSequence(bytes) => match bytes.first() {
            Some(&byte) => First::code_point(byte.into()).with_non_ascii(byte >= 128),
            None => First::EMPTY,
        },
        Node::ByteSet(bytes) => {
            let mut result = First::NOTHING;
            for &byte in bytes {
                result.add(byte.into());
            }
            result
        }
        Node::CharSet(chars) => {
            let mut result = First::NOTHING;
            for &c in chars {
                result.add(c);
            }
            result
        }
        Node::Cat(nodes) => {
            let mut result = First::EMPTY;
            for node in nodes {
                let item = first(node);
                result = First {
                    nullable: item.nullable,
                    ..result.union(item)
                };
                if !item.nullable {
                    return result;
                }
            }
            result
        }
        Node::Alt(left, right) => first(left).union(first(right)),
        Node::MatchAny | Node::MatchAnyExceptLineTerminator => First::ANY,
        // A unit follows the start, so `$` holds there only before a line terminator.
        Node::Anchor {
            anchor_type: AnchorType::EndOfLine,
            multiline,
        } => {
            if *multiline {
                First {
                    ascii: 1 << b'\n' | 1 << b'\r',
                    non_ascii: true,
                    nullable: false,
                }
            } else {
                First::NOTHING
            }
        }
        // What a lookahead matches starts with the unit the match starts with.
        Node::LookaroundAssertion {
            negate: false,
            backwards: false,
            contents,
            ..
        } => first(contents),
        Node::Anchor { .. } | Node::WordBoundary { .. } | Node::LookaroundAssertion { .. } => {
            First::EMPTY
        }
        Node::CaptureGroup { contents, .. } => first(contents),
        Node::BackRef { .. } | Node::StringSet { .. } => First {
            nullable: true,
            ..First::ANY
        },
        Node::Bracket(contents) => {
            let mut result = First::NOTHING;
            for interval in contents.cps.intervals() {
                let (low, high) = (interval.first, interval.last);
                for cp in low..=high.min(127) {
                    result.add(cp);
                }
                if high >= 128 {
                    result.non_ascii = true;
                }
            }
            if contents.invert {
                result.ascii = !result.ascii;
                result.non_ascii = true;
            }
            result
        }
        Node::Loop { loopee, quant, .. } | Node::Loop1CharBody { loopee, quant } => {
            if quant.max == Some(0) {
                return First::EMPTY;
            }
            let body = first(loopee);
            First {
                nullable: body.nullable || quant.min == 0,
                ..body
            }
        }
    }
}

impl First {
    fn with_non_ascii(mut self, non_ascii: bool) -> First {
        self.non_ascii |= non_ascii;
        self
    }
}

/// The units a match of `node` can start with, or `None` when it can match the empty string
/// before a unit.
pub fn first_units(node: &Node) -> Option<FirstUnits> {
    let result = first(node);
    (!result.nullable).then(|| FirstUnits::new(result.ascii, result.non_ascii))
}

#[cfg(all(test, feature = "utf16"))]
mod tests {
    use super::FirstUnits;
    use crate::Regex;

    /// Skipping sixteen units at a time finds what testing each unit finds.
    #[test]
    fn skip_finds_the_first_unit_accepted() {
        let sets: [(&[u8], bool); 5] = [
            (b"#", false),
            (b"[`<", false),
            (b"\n\r", false),
            (b"ab", true),
            (b"abcd", false),
        ];
        for (set, non_ascii) in sets {
            let ascii = set.iter().fold(0u128, |ascii, &byte| ascii | 1 << byte);
            let first_units = FirstUnits::new(ascii, non_ascii);
            for length in [0, 1, 15, 16, 17, 31, 32, 33, 100] {
                for at in 0..=length {
                    let mut units = vec![u16::from(b'x'); length];
                    if at < length {
                        units[at] = u16::from(set[set.len() - 1]);
                    }
                    let expected = units
                        .iter()
                        .position(|&unit| first_units.accepts(unit.into()))
                        .unwrap_or(length);
                    assert_eq!(
                        first_units.skip(&units),
                        expected,
                        "{set:?} in {length} at {at}"
                    );
                }
            }
        }
    }

    /// Every match found skipping by the first units and line starts is found without them, from
    /// every start.
    #[test]
    fn first_units_miss_no_match() {
        let patterns = [
            (r"^(?:[ \t]*(?:\n|$))+", ""),
            (r"a$|b", ""),
            (r"x*$", ""),
            (r"$", "m"),
            (r"$\n?a", "m"),
            (r"(?=\s|$)x?", ""),
            (r"(?=[ab])\w", ""),
            (r"(?=b*)c", ""),
            (r"(?!a)b|c", ""),
            (r"(?<=a)b", ""),
            (r"\bfoo|^$", "m"),
            (r"\n[^\S\n]*(?:\n[^\S\n]*)+$", ""),
            (r"#$", ""),
            (r"[😀]|é", "u"),
            (r"^ ?a", "m"),
            (r"^a|^b", "m"),
            (r"(^a)|b", "m"),
            (r"^$", "m"),
            (r"^#* *b", "m"),
            (r"^\s*", ""),
        ];
        // Every line terminator, since the line start skip stops at each.
        let alphabet: [&[u16]; 12] = [
            &[b'a' as u16],
            &[b'b' as u16],
            &[b'c' as u16],
            &[b'x' as u16],
            &[b' ' as u16],
            &[b'\n' as u16],
            &[b'\r' as u16],
            &[b'#' as u16],
            &[0xE9],
            &[0xD83D, 0xDE00],
            &[0x2028],
            &[0x2029],
        ];
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for (pattern, flags) in patterns {
            let with = Regex::with_flags(pattern, flags).unwrap();
            let mut without = Regex::with_flags(pattern, flags).unwrap();
            without.cr.first_units = None;
            without.cr.line_start = false;
            for _ in 0..20_000 {
                let mut text = Vec::new();
                for _ in 0..next() % 8 {
                    text.extend_from_slice(alphabet[(next() % alphabet.len() as u64) as usize]);
                }
                for start in 0..=text.len() {
                    let found = |regex: &Regex| {
                        let ucs2 = regex.find_from_ucs2(&text, start).next().map(|m| m.range);
                        let utf16 = regex.find_from_utf16(&text, start).next().map(|m| m.range);
                        (ucs2, utf16)
                    };
                    assert_eq!(
                        found(&with),
                        found(&without),
                        "/{pattern}/{flags} on {text:?}"
                    );
                }
            }
        }
    }
}
