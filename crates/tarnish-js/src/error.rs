//! JavaScript's errors, as far as the output shows them: the bridge reports an error's message,
//! and a message that quotes another error quotes `String(error)`, class name and all.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ErrorKind {
    Error,
    TypeError,
    RangeError,
    /// zod's `ZodError`, whose `String(error)` is its message alone.
    ZodError,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JsError {
    pub kind: ErrorKind,
    pub message: String,
}

impl JsError {
    pub fn error(message: impl Into<String>) -> Self {
        JsError {
            kind: ErrorKind::Error,
            message: message.into(),
        }
    }

    pub fn type_error(message: impl Into<String>) -> Self {
        JsError {
            kind: ErrorKind::TypeError,
            message: message.into(),
        }
    }

    pub fn range_error(message: impl Into<String>) -> Self {
        JsError {
            kind: ErrorKind::RangeError,
            message: message.into(),
        }
    }
}

/// `String(error)`.
impl fmt::Display for JsError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let name = match self.kind {
            ErrorKind::Error => "Error",
            ErrorKind::TypeError => "TypeError",
            ErrorKind::RangeError => "RangeError",
            ErrorKind::ZodError => return formatter.write_str(&self.message),
        };
        write!(formatter, "{name}: {}", self.message)
    }
}
