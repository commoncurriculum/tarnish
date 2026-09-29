//! marked's tokens (`Tokens.ts`). `kind` is the token's `type`, marked's own or one an
//! extension's tokenizer makes, and `data` holds the fields only some types have. Strings are
//! UTF-16, as JavaScript's are, and `raw` and `text` share the source they were cut from.

use std::ops::{Deref, DerefMut};

use tarnish_js::stack;

use tarnish_js::json::{Map, Value};
use tarnish_js::units::Units;

#[derive(Clone, Debug, Default)]
pub struct Token {
    pub kind: &'static str,
    pub raw: Units,
    pub text: Option<Units>,
    pub tokens: Option<Tokens>,
    pub data: TokenData,
    /// The fields an extension's tokenizer puts on its token beyond marked's, which few do.
    pub extra: Option<Box<Map>>,
    /// The entry of the lexer's inline queue that fills `tokens`.
    pub queued: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub enum TokenData {
    #[default]
    None,
    Heading {
        depth: usize,
    },
    /// A link's or an image's.
    Link(Box<Destination>),
    Html {
        block: bool,
    },
    Def(Box<Def>),
    List(Box<List>),
    ListItem {
        task: bool,
        checked: Option<bool>,
        loose: bool,
    },
    Table(Box<Table>),
}

#[derive(Clone, Debug)]
pub struct Destination {
    pub href: Vec<u16>,
    pub title: Option<Vec<u16>>,
}

#[derive(Clone, Debug)]
pub struct Def {
    pub tag: Vec<u16>,
    pub href: Vec<u16>,
    pub title: Option<Vec<u16>>,
}

#[derive(Clone, Debug, Default)]
pub struct List {
    pub ordered: bool,
    pub start: Option<f64>,
    pub loose: bool,
    pub items: Vec<Token>,
}

#[derive(Clone, Debug)]
pub struct Table {
    pub header: Vec<Cell>,
    pub rows: Vec<Vec<Cell>>,
}

/// `Tokens.TableCell`, without the `text` its tokens are lexed from.
#[derive(Clone, Debug)]
pub struct Cell {
    pub tokens: Vec<Token>,
    pub(crate) queued: Option<u32>,
}

/// A token's `tokens`, which nest as deeply as the Markdown does, so each level clones and
/// drops on the stack segments `stack::grow` adds. Tokens nest only through these.
#[derive(Debug, Default)]
pub struct Tokens(Vec<Token>);

impl From<Vec<Token>> for Tokens {
    fn from(tokens: Vec<Token>) -> Tokens {
        Tokens(tokens)
    }
}

impl Deref for Tokens {
    type Target = [Token];

    fn deref(&self) -> &[Token] {
        &self.0
    }
}

impl DerefMut for Tokens {
    fn deref_mut(&mut self) -> &mut [Token] {
        &mut self.0
    }
}

impl Clone for Tokens {
    fn clone(&self) -> Tokens {
        Tokens(stack::grow(|| self.0.clone()))
    }
}

impl Drop for Tokens {
    fn drop(&mut self) {
        stack::drop_nested(&mut self.0);
    }
}

impl Token {
    pub fn new(kind: &'static str, raw: impl Into<Units>) -> Self {
        Token {
            kind,
            raw: raw.into(),
            ..Token::default()
        }
    }

    pub fn text(&self) -> &[u16] {
        self.text.as_deref().unwrap_or_default()
    }

    /// `token.href`, which links, images and definitions have.
    pub fn href(&self) -> Option<&[u16]> {
        match &self.data {
            TokenData::Link(link) => Some(&link.href),
            TokenData::Def(def) => Some(&def.href),
            _ => None,
        }
    }

    /// `token.title`.
    pub fn title(&self) -> Option<&[u16]> {
        match &self.data {
            TokenData::Link(link) => link.title.as_deref(),
            TokenData::Def(def) => def.title.as_deref(),
            _ => None,
        }
    }

    /// A field an extension's tokenizer put on the token.
    pub fn extra(&self, key: &str) -> Option<&Value> {
        self.extra.as_deref().and_then(|extra| extra.get(key))
    }

    /// `token.depth`.
    pub fn depth(&self) -> Option<usize> {
        match self.data {
            TokenData::Heading { depth } => Some(depth),
            _ => None,
        }
    }

    /// `token.block`.
    pub fn block(&self) -> bool {
        matches!(self.data, TokenData::Html { block: true })
    }

    /// A `list` token's list.
    pub fn list(&self) -> Option<&List> {
        match &self.data {
            TokenData::List(list) => Some(list),
            _ => None,
        }
    }
}
