//! The DOM's checks on the names `createElement`, `createElementNS`, `setAttribute` and
//! `setAttributeNS` take: <https://dom.spec.whatwg.org/#namespaces>.

use std::fmt;

use tarnish::js::Class;

/// Which local name a name must be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    Element,
    Attribute,
}

impl NameKind {
    pub fn is_valid(self, name: &str) -> bool {
        match self {
            NameKind::Element => is_element_local_name(name),
            NameKind::Attribute => is_attribute_local_name(name),
        }
    }

    /// Refuse a local name as `createElement` or `setAttribute` does.
    pub fn validate(self, name: &str) -> Result<(), NameError> {
        match self.is_valid(name) {
            true => Ok(()),
            false => Err(invalid_character(name, self.noun())),
        }
    }

    fn noun(self) -> &'static str {
        match self {
            NameKind::Element => "element local name",
            NameKind::Attribute => "attribute local name",
        }
    }
}

/// A name the DOM refuses: the `DOMException` it throws, by its name and its message, which are
/// the linkedom fork's. It displays as `"{name}: {message}"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameError {
    /// `InvalidCharacterError` or `NamespaceError`.
    pub name: &'static str,
    pub message: String,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.message)
    }
}

impl std::error::Error for NameError {}

/// The `DOMException`s a name the DOM refuses throws, by their names, which `String(error)`
/// gives before the message.
pub static INVALID_CHARACTER_ERROR: Class = Class {
    name: "InvalidCharacterError",
    message_alone: false,
};
pub static NAMESPACE_ERROR: Class = Class {
    name: "NamespaceError",
    message_alone: false,
};

impl From<NameError> for tarnish::Error {
    fn from(error: NameError) -> Self {
        let class = match error.name {
            "NamespaceError" => &NAMESPACE_ERROR,
            _ => &INVALID_CHARACTER_ERROR,
        };
        tarnish::Error::Of(class, error.message)
    }
}

fn invalid_character(name: &str, noun: &str) -> NameError {
    NameError {
        name: "InvalidCharacterError",
        message: format!("\"{name}\" is not a valid {noun}"),
    }
}

fn namespace_error(message: &str) -> NameError {
    NameError {
        name: "NamespaceError",
        message: message.to_owned(),
    }
}

/// Whether a name holds none of the characters no name may hold, all of which are ASCII.
fn allowed(name: &[u8]) -> bool {
    !name.iter().any(|byte| {
        matches!(
            byte,
            b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ' | b'/' | b'>'
        )
    })
}

/// <https://dom.spec.whatwg.org/#valid-element-local-name>
pub fn is_element_local_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    match bytes.first() {
        Some(first) if first.is_ascii_alphabetic() => allowed(&bytes[1..]),
        Some(first) if matches!(first, b':' | b'_') || !first.is_ascii() => {
            name.chars().skip(1).all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '-' | '.' | ':' | '_')
                    || !character.is_ascii()
            })
        }
        _ => false,
    }
}

/// <https://dom.spec.whatwg.org/#valid-attribute-local-name>
pub fn is_attribute_local_name(name: &str) -> bool {
    !name.is_empty() && allowed(name.as_bytes()) && !name.contains('=')
}

/// <https://dom.spec.whatwg.org/#valid-namespace-prefix>
fn is_namespace_prefix(prefix: &str) -> bool {
    !prefix.is_empty() && allowed(prefix.as_bytes())
}

const XML: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS: &str = "http://www.w3.org/2000/xmlns/";

/// <https://dom.spec.whatwg.org/#validate-and-extract>: the namespace, `None` for the empty
/// string, the prefix and the local name.
pub fn validate_and_extract<'a>(
    namespace: &'a str,
    qualified: &'a str,
    kind: NameKind,
) -> Result<(Option<&'a str>, Option<&'a str>, &'a str), NameError> {
    let namespace = Some(namespace).filter(|namespace| !namespace.is_empty());
    let (prefix, local) = match qualified.split_once(':') {
        Some((prefix, local)) => (Some(prefix), local),
        None => (None, qualified),
    };
    if let Some(prefix) = prefix.filter(|prefix| !is_namespace_prefix(prefix)) {
        return Err(invalid_character(prefix, "namespace prefix"));
    }
    kind.validate(local)?;
    if prefix.is_some() && namespace.is_none() {
        return Err(namespace_error(
            "A prefix was given but no namespace was provided",
        ));
    }
    if prefix == Some("xml") && namespace != Some(XML) {
        return Err(namespace_error(
            "A prefix of \"xml\" was given but the namespace was not the XML namespace",
        ));
    }
    if (qualified == "xmlns" || prefix == Some("xmlns")) && namespace != Some(XMLNS) {
        return Err(namespace_error(
            "A prefix or qualifiedName of \"xmlns\" was given but the namespace was not the XMLNS namespace",
        ));
    }
    if namespace == Some(XMLNS) && qualified != "xmlns" && prefix != Some("xmlns") {
        return Err(namespace_error(
            "The XMLNS namespace was given but neither the prefix nor qualifiedName was \"xmlns\"",
        ));
    }
    Ok((namespace, prefix, local))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked_as_the_dom_checks_them() {
        assert!(is_element_local_name("a\u{1}b"));
        assert!(is_element_local_name("_a.b"));
        assert!(!is_element_local_name("_a~b"));
        assert!(!is_element_local_name("1a"));
        assert!(!is_element_local_name("a b"));
        assert!(is_attribute_local_name("@x"));
        assert!(!is_attribute_local_name("a=b"));
        assert!(!is_attribute_local_name(""));
        let error = NameKind::Element.validate("1a").expect_err("refused");
        assert_eq!(error.name, "InvalidCharacterError");
        assert_eq!(error.message, "\"1a\" is not a valid element local name");
        assert_eq!(
            validate_and_extract("urn:x", "p:q", NameKind::Attribute),
            Ok((Some("urn:x"), Some("p"), "q"))
        );
        assert_eq!(
            validate_and_extract("", "q", NameKind::Element),
            Ok((None, None, "q"))
        );
        let error = validate_and_extract("", "p:q", NameKind::Element).expect_err("refused");
        assert_eq!(
            error.to_string(),
            "NamespaceError: A prefix was given but no namespace was provided"
        );
    }
}
