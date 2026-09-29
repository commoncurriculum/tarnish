use std::fmt;

/// An error, of the class JavaScript throws it as.
#[derive(Clone, PartialEq)]
pub enum Error {
    /// `RangeError`: a number or a position out of range, or input a library refuses as such.
    Range(String),
    /// `SyntaxError`: text that doesn't parse, such as a schema's content expression.
    Syntax(String),
    /// `TypeError`: reading a property of `null` or `undefined`, calling what isn't a function,
    /// iterating what isn't iterable, as V8 words each.
    Type(String),
    /// A plain `Error`.
    Other(String),
    /// An error of a class a library defines, such as ProseMirror's `ReplaceError`.
    Of(&'static Class, String),
    /// What a hook of the host, such as a spec's `leafText`, threw. The host keeps what was
    /// thrown, to pass it along untouched.
    Host,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A class a library defines by extending `Error`.
#[derive(Debug, PartialEq, Eq)]
pub struct Class {
    pub name: &'static str,
    /// Whether `String(error)` is the message alone, as a class that overrides `toString` to
    /// give it has it, rather than `Error.prototype.toString`'s name and message.
    pub message_alone: bool,
}

impl Error {
    /// The name of the class JavaScript throws the error as.
    pub fn class(&self) -> &'static str {
        match self {
            Error::Range(_) => "RangeError",
            Error::Syntax(_) => "SyntaxError",
            Error::Type(_) => "TypeError",
            Error::Other(_) | Error::Host => "Error",
            Error::Of(class, _) => class.name,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Error::Range(message)
            | Error::Syntax(message)
            | Error::Type(message)
            | Error::Other(message)
            | Error::Of(_, message) => message,
            Error::Host => "an error thrown by a host hook",
        }
    }

    /// Whether the error is of `class`.
    pub fn is(&self, class: &Class) -> bool {
        matches!(self, Error::Of(of, _) if *of == class)
    }
}

/// `String(error)`.
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (name, message) = (self.class(), self.message());
        match self {
            Error::Of(class, _) if class.message_alone => f.write_str(message),
            _ if message.is_empty() => f.write_str(name),
            _ => write!(f, "{name}: {message}"),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for Error {}
