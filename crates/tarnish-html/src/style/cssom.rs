//! The declarations of an inline style as CSSOM 0.5, which cssstyle hands them to, reads them:
//! as the one rule of `#bogus{…}`.

use super::js_trim;

/// A declaration as CSSOM keeps it: the name as written, and its value and priority.
pub(super) struct Declaration {
    pub(super) name: String,
    pub(super) value: String,
    pub(super) important: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    BeforeSelector,
    Selector,
    AtRule,
    BeforeName,
    Name,
    BeforeValue,
    Value,
    ValueParenthesis,
}

const AT_RULES: [&str; 6] = [
    "@-moz-document",
    "@media",
    "@supports",
    "@host",
    "@import",
    "@font-face",
];

/// The first rule's declarations, in the order CSSOM lists them: a name where it was first set,
/// and again where it was set after being set to nothing, each with its last value. `None`
/// where CSSOM throws, and for at-rules, which CSSOM nests where no inline style has them.
pub(super) fn parse(css: &str) -> Option<Vec<Declaration>> {
    let token = format!("#bogus{{{css}}}");
    let bytes = token.as_bytes();
    let mut state = State::BeforeSelector;
    let mut buffer = String::new();
    let mut name = String::new();
    let mut important = false;
    let mut depth = 0usize;
    let mut closed_rules = 0usize;
    let mut list: Vec<String> = Vec::new();
    let mut values: Vec<(String, String, bool)> = Vec::new();
    let mut i = 0;
    while let Some(character) = token[i..].chars().next() {
        let mut declare = |value: &str, important: bool| {
            if closed_rules > 0 {
                return;
            }
            match values.iter_mut().find(|(set, ..)| *set == name) {
                Some(entry) => {
                    if entry.1.is_empty() {
                        list.push(name.clone());
                    }
                    (entry.1, entry.2) = (value.to_owned(), important);
                }
                None => {
                    list.push(name.clone());
                    values.push((name.clone(), value.to_owned(), important));
                }
            }
        };
        match (character, state) {
            (' ' | '\t' | '\r' | '\n' | '\u{c}', _) => {
                if matches!(
                    state,
                    State::Selector | State::Value | State::ValueParenthesis | State::AtRule
                ) {
                    buffer.push(character);
                }
            }
            ('"' | '\'', _) => {
                let mut end = i + 1;
                loop {
                    end += token[end..].find(character)? + 1;
                    if bytes[end - 2] != b'\\' {
                        break;
                    }
                }
                buffer.push_str(&token[i..end]);
                i = end - 1;
                if state == State::BeforeValue {
                    state = State::Value;
                }
            }
            ('/', _) if bytes.get(i + 1) == Some(&b'*') => {
                i += 2 + token[i + 2..].find("*/")? + 1;
            }
            ('@', _) => {
                if AT_RULES.iter().any(|rule| token[i..].starts_with(rule))
                    || is_keyframes(&token[i..])
                {
                    return None;
                }
                if state == State::Selector {
                    state = State::AtRule;
                }
                buffer.push('@');
            }
            ('{', State::Selector | State::AtRule) => {
                buffer.clear();
                state = State::BeforeName;
            }
            ('{', _) => {}
            (':', State::Name) => {
                name = js_trim(&buffer).to_owned();
                buffer.clear();
                state = State::BeforeValue;
            }
            ('(', State::Value) => {
                state = State::ValueParenthesis;
                depth = 1;
                buffer.push('(');
            }
            ('(', State::ValueParenthesis) => {
                depth += 1;
                buffer.push('(');
            }
            (')', _) => {
                if state == State::ValueParenthesis {
                    depth -= 1;
                    if depth == 0 {
                        state = State::Value;
                    }
                }
                buffer.push(')');
            }
            ('!', State::Value) if token[i..].starts_with("!important") => {
                important = true;
                i += "important".len();
            }
            (';', State::Value) => {
                declare(js_trim(&buffer), important);
                important = false;
                buffer.clear();
                state = State::BeforeName;
            }
            (';', State::AtRule) => {
                buffer.clear();
                state = State::BeforeSelector;
            }
            ('}', State::Value | State::BeforeName | State::Name) => {
                if state == State::Value {
                    declare(js_trim(&buffer), important);
                    important = false;
                }
                closed_rules += 1;
                buffer.clear();
                state = State::BeforeSelector;
            }
            // "Unexpected }", outside any rule.
            ('}', State::BeforeSelector | State::Selector) => return None,
            ('}', _) => {}
            ('/' | ':' | '(' | '!' | ';', _) => buffer.push(character),
            _ => {
                state = match state {
                    State::BeforeSelector => State::Selector,
                    State::BeforeName => State::Name,
                    State::BeforeValue => State::Value,
                    state => state,
                };
                buffer.push(character);
            }
        }
        i += token[i..].chars().next().map_or(1, char::len_utf8);
    }
    if closed_rules == 0 {
        return None;
    }
    let declarations = list.into_iter().map(|name| {
        let (_, value, important) = values.iter().find(|(set, ..)| *set == name)?;
        Some(Declaration {
            value: value.clone(),
            important: *important,
            name,
        })
    });
    declarations.collect()
}

/// Whether CSSOM's `/@(-(?:\w+-)+)?keyframes/` matches at the start.
fn is_keyframes(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('@') else {
        return false;
    };
    if rest.starts_with("keyframes") {
        return true;
    }
    let Some(mut after) = rest.strip_prefix('-') else {
        return false;
    };
    loop {
        let word = after
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count();
        if word == 0 || after.as_bytes().get(word) != Some(&b'-') {
            return false;
        }
        after = &after[word + 1..];
        if after.starts_with("keyframes") {
            return true;
        }
    }
}
