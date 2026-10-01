//! marked 17.0.6's lexer: Markdown to tokens, with the default options. Renderers and the
//! parser, which turn tokens into HTML, aren't ported.

mod delimiters;
mod helpers;
mod json;
mod lexer;
mod matchers;
mod rules;
mod tokenizer;
mod tokens;

pub use lexer::Lexer;
pub(crate) use matchers::{hr, indent, run};
pub use tokens::{Cell, Def, Destination, List, Table, Token, TokenData, Tokens};

use tarnish_js::units::Units;
use tarnish_js::{Error, random, utf16};

pub type ExtensionTokenizer =
    Box<dyn Fn(&mut Lexer, &Units, &[Token]) -> Result<Option<Token>, Error> + Send + Sync>;
pub type ListTokenizer = fn(&mut Lexer, &Units) -> Result<Option<Token>, Error>;

/// An inline tokenizer extension, as `marked.use({ extensions })` takes one. Its `start(src)`
/// is `src.indexOf(start)`, the only kind Tiptap registers for an extension, and its
/// tokenizer matches only where `src` starts with `start`, so the lexer tries it only there.
pub struct TokenizerExtension {
    pub start: &'static str,
    pub tokenizer: ExtensionTokenizer,
}

/// A marked instance, with the list tokenizer `marked.use({ tokenizer: { list } })` gave it:
/// marked's own isn't ported.
pub struct Marked {
    list: ListTokenizer,
    /// `defaults.extensions.inline`, the last added first, as marked tries them.
    inline: Vec<TokenizerExtension>,
    /// Which ASCII units the extensions' start strings begin with.
    first_units: [bool; 128],
}

impl Marked {
    pub fn new(list: ListTokenizer) -> Self {
        Marked {
            list,
            inline: Vec::new(),
            first_units: [false; 128],
        }
    }

    /// Whether an extension's start string begins with `unit`.
    fn is_first_unit(&self, unit: u16) -> bool {
        self.first_units
            .get(usize::from(unit))
            .copied()
            .unwrap_or(false)
    }

    /// `marked.use({ extensions })`: each tokenizer runs before those added earlier.
    pub fn use_extension(&mut self, extension: TokenizerExtension) {
        let first = extension
            .start
            .bytes()
            .next()
            .filter(u8::is_ascii)
            .expect("a start string that starts with ASCII");
        self.first_units[usize::from(first)] = true;
        self.inline.insert(0, extension);
    }

    /// A test of what the lexer takes on trust of each extension: that its tokenizer matches
    /// only where the source starts with its start string. Every extension is run on every
    /// suffix of random strings of `alphabet`'s pieces, and has to match on some.
    pub fn check_extension_starts(&self, alphabet: &[&str]) {
        let mut matches = vec![0; self.inline.len()];
        for units in random::strings(alphabet, 20_000) {
            let src = Units::from(units);
            for at in 0..src.len() {
                let rest = src.substring(at);
                for (extension, matches) in self.inline.iter().zip(&mut matches) {
                    let mut lexer = Lexer::new(self);
                    if let Ok(Some(_)) = (extension.tokenizer)(&mut lexer, &rest, &[]) {
                        assert!(
                            utf16::starts_with(&rest, extension.start),
                            "{:?} matched {:?}",
                            extension.start,
                            utf16::to_string(&rest)
                        );
                        *matches += 1;
                    }
                }
            }
        }
        for (extension, matches) in self.inline.iter().zip(matches) {
            assert!(matches > 0, "{:?} matched nothing", extension.start);
        }
    }
}
