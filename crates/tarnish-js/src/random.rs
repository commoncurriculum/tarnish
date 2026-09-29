//! Random inputs for the tests that hold hand-written code to the regex or parser it stands in
//! for. The generator is seeded, so a failure reproduces.

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
