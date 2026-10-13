//! Random inputs for the tests that hold hand-written code to the regex or parser it stands in
//! for. The generator is seeded, so a failure reproduces.

#[cfg(feature = "regexp")]
use crate::regexp::RegExp;
use crate::utf16;

/// A xorshift generator.
pub struct Random(u64);

#[expect(
    clippy::new_without_default,
    clippy::should_implement_trait,
    reason = "a test's generator, always of the one seed, whose numbers never run out"
)]
impl Random {
    pub fn new() -> Self {
        Random(0x9E37_79B9_7F4A_7C15)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number below `bound`.
    pub fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// `count` strings of up to 23 pieces each: pieces of `alphabet`, and now and then a lone
/// surrogate, which JavaScript strings can hold.
pub fn strings(alphabet: &[&str], count: usize) -> impl Iterator<Item = Vec<u16>> {
    let pieces: Vec<Vec<u16>> = alphabet.iter().map(|piece| utf16::from(piece)).collect();
    let mut random = Random::new();
    (0..count).map(move |_| {
        let mut units = Vec::new();
        for _ in 0..random.below(24) {
            match pieces.get(random.below(pieces.len() + 1)) {
                Some(piece) => units.extend_from_slice(piece),
                None => units.push(0xD83D),
            }
        }
        units
    })
}

/// JSON texts, as `harness/record-js.mjs` draws them too: values nested up to five deep of
/// numbers at the edges of a double and past it, strings with escapes, a lone surrogate's among
/// them, and keys that repeat, are array indices or are `__proto__`, with a space now and then.
pub fn json_texts() -> impl Iterator<Item = String> {
    let mut random = Random::new();
    std::iter::repeat_with(move || {
        let mut text = String::new();
        json_value(&mut random, 0, &mut text);
        text
    })
}

const NUMBERS: &[&str] = &[
    "0",
    "-0",
    "1",
    "-1",
    "0.1",
    "2.50",
    "1e5",
    "1.5E3",
    "1e-7",
    "-1e21",
    "1e308",
    "4.9e-324",
    "0.30000000000000004",
    "9007199254740993",
    "-9007199254740993",
    "18446744073709551616",
    "123456789012345678901234567890",
    "1e400",
    "2.2250738585072011e-308",
    "1.00000000000000011102230246251565404236316680908203125",
    "9007199254740993.0",
    "1.7976931348623158e308",
];
const STRINGS: &[&str] = &[
    r#""a""#,
    r#""é""#,
    r#""😀""#,
    r#""\ud800""#,
    r#""é""#,
    r#""\n""#,
    r#""\/""#,
    r#""""#,
];
const KEYS: &[&str] = &[
    r#""a""#,
    r#""b""#,
    r#""a""#,
    r#""1""#,
    r#""0""#,
    r#""01""#,
    r#""4294967295""#,
    r#""4294967294""#,
    r#""__proto__""#,
];

// bounded: past depth 3 a value is a scalar
fn json_value(random: &mut Random, depth: usize, out: &mut String) {
    let pick = |random: &mut Random, list: &[&'static str]| list[random.below(list.len())];
    let space = |random: &mut Random| if random.below(4) == 0 { " " } else { "" };
    match random.below(if depth > 3 { 5 } else { 7 }) {
        0 | 1 => out.push_str(pick(random, NUMBERS)),
        2 => out.push_str(pick(random, STRINGS)),
        3 => out.push_str(["true", "false"][random.below(2)]),
        4 => out.push_str("null"),
        5 => {
            out.push('[');
            for index in 0..random.below(4) {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(space(random));
                json_value(random, depth + 1, out);
            }
            out.push(']');
        }
        _ => {
            out.push('{');
            for index in 0..random.below(5) {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(pick(random, KEYS));
                out.push(':');
                out.push_str(space(random));
                json_value(random, depth + 1, out);
            }
            out.push('}');
        }
    }
}

/// Checks that `ours` gives what `theirs` gives on `count` strings of `alphabet`.
pub fn check_same<T: PartialEq + std::fmt::Debug>(
    alphabet: &[&str],
    count: usize,
    ours: impl Fn(&[u16]) -> T,
    theirs: impl Fn(&[u16]) -> T,
) {
    for src in strings(alphabet, count) {
        assert_eq!(
            ours(&src),
            theirs(&src),
            "{:?}",
            String::from_utf16_lossy(&src)
        );
    }
}

/// Checks that the guard `may` says yes wherever `regex` matches, and that both the matches
/// and its noes happen often enough for the check to mean something.
#[cfg(feature = "regexp")]
pub fn check_may(regex: &RegExp, may: fn(&[u16]) -> bool, alphabet: &[&str]) {
    let (mut matches, mut noes) = (0, 0);
    for src in strings(alphabet, 500_000) {
        let may_match = may(&src);
        if regex.exec(&src).is_some() {
            assert!(may_match, "{:?}", String::from_utf16_lossy(&src));
            matches += 1;
        }
        noes += usize::from(!may_match);
    }
    assert!(
        matches > 100 && noes > 100,
        "{matches} matches, {noes} noes"
    );
}
