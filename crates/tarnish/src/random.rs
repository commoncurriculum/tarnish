//! A seeded generator for the tests that check code against a reference on random input, so a
//! failure reproduces.

/// A xorshift generator.
pub struct Random(u64);

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
}
