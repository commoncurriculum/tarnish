//! Room on the stack for stylo, whose parsers, serializers and destructors recurse once for each
//! level that blocks nest in the CSS they read (`calc((((…))))`, `--a: [[[…]]]`), with no limit.
//! It can't grow the stack as it goes, so each call into it runs where there is room for as deep
//! as its CSS nests: a caller's stack may be as small as a BEAM dirty scheduler's.

/// The room stylo needs for some CSS: how deep its blocks nest.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Room(usize);

/// WebAssembly's call stack is the engine's, which no new segment can extend.
#[cfg(target_family = "wasm")]
impl Room {
    pub(crate) fn of(_: &str) -> Room {
        Room(0)
    }

    pub(crate) fn run<R>(self, f: impl FnOnce() -> R) -> R {
        f()
    }
}

#[cfg(not(target_family = "wasm"))]
mod native {
    use cssparser::{ParseError, Parser, ParserInput, Token};
    use tarnish_js::stack;

    use super::Room;

    /// The stack stylo takes for CSS that doesn't nest, and for each level that it does, three
    /// times what the deepest found takes (`min(1px, min(…))`: 2.7 KiB a level optimized, 22 KiB
    /// unoptimized).
    const BASE: usize = if cfg!(debug_assertions) {
        256 << 10
    } else {
        64 << 10
    };
    const PER_LEVEL: usize = if cfg!(debug_assertions) {
        64 << 10
    } else {
        8 << 10
    };

    /// The most blocks CSS may open for their count to stand in for how deep they nest.
    const FEW: usize = 8;

    impl Room {
        pub(crate) fn of(css: &str) -> Room {
            // Every block opens with one of these, so their count bounds how deep blocks nest.
            let openers = css.bytes().filter(|byte| b"([{".contains(byte)).count();
            if openers <= FEW {
                return Room(openers);
            }
            Room(nesting(&mut Parser::new(&mut ParserInput::new(css))))
        }

        /// Runs `f`, which hands CSS this deep to stylo, where there is room for it.
        pub(crate) fn run<R>(self, f: impl FnOnce() -> R) -> R {
            stack::with_room(BASE.saturating_add(self.0.saturating_mul(PER_LEVEL)), f)
        }
    }

    /// How deep blocks nest in what is left of `input`, as cssparser reads them for stylo.
    fn nesting(input: &mut Parser) -> usize {
        let mut deepest = 0;
        loop {
            match input.next_including_whitespace_and_comments() {
                Ok(
                    Token::Function(_)
                    | Token::ParenthesisBlock
                    | Token::SquareBracketBlock
                    | Token::CurlyBracketBlock,
                ) => {}
                Ok(_) => continue,
                Err(_) => return deepest,
            }
            let mut inner = 0;
            let _ = input.parse_nested_block(|block| {
                inner = stack::grow(|| nesting(block));
                Ok::<_, ParseError<()>>(())
            });
            deepest = deepest.max(inner + 1);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn nesting_is_counted_as_cssparser_reads_blocks() {
            for (css, deep) in [
                ("color: red", 0),
                ("a(b) c[d] {e}", 1),
                ("--a: f(([{x}]))", 4),
                // Brackets in strings, comments, escapes and URLs open and close nothing.
                (
                    r#"--a: "((((((((((" /* (((((((((( */ \(\(\(\(\(\(\(\(\(\( url(()"#,
                    0,
                ),
                (r#"--a: (((((")))))))))))"((((()"#, 10),
                (r"--a: (((((/*)))))))))))*/((((()", 10),
                (r"--a: (((((\)\)\)\)\)\)\)\)\)\)((((()", 10),
                // A closer of another kind closes nothing.
                ("--a: ((((([)))))))))))]((((()", 10),
                // Blocks left open close at the end.
                ("--a: ((((((((((", 10),
            ] {
                assert_eq!(
                    nesting(&mut Parser::new(&mut ParserInput::new(css))),
                    deep,
                    "{css}"
                );
                assert!(Room::of(css).0 >= deep, "{css}");
            }
        }
    }
}
