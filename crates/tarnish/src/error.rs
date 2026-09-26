use std::any::Any;
use std::fmt;
use std::sync::Arc;

/// An error, of the class ProseMirror throws it as.
#[derive(Clone)]
pub enum Error {
    /// `RangeError`: an invalid position, attribute, node or mark.
    Range(String),
    /// `SyntaxError`: an invalid content or mark expression in a schema.
    Syntax(String),
    /// `ReplaceError`: a slice that doesn't fit where it is put.
    Replace(String),
    /// `TransformError`: a step that fails to apply in a transform.
    Transform(String),
    /// A plain `Error`.
    Other(String),
    /// What a hook of the host, such as a spec's `leafText`, threw, passed along untouched.
    Host(Arc<dyn Any + Send + Sync>),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn message(&self) -> &str {
        match self {
            Error::Range(message)
            | Error::Syntax(message)
            | Error::Replace(message)
            | Error::Transform(message)
            | Error::Other(message) => message,
            Error::Host(_) => "an error thrown by a host hook",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let class = match self {
            Error::Range(_) => "RangeError",
            Error::Syntax(_) => "SyntaxError",
            Error::Replace(_) => "ReplaceError",
            Error::Transform(_) => "TransformError",
            Error::Other(_) | Error::Host(_) => "Error",
        };
        write!(f, "{class}: {}", self.message())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for Error {}
