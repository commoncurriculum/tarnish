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

use tarnish_js::Error;
use tarnish_js::units::Units;

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
    pub inline: Vec<TokenizerExtension>,
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
}
