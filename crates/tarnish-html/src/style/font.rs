//! cssstyle's `font` shorthand and the longhands it splits into.

use super::value::{Type, split_commas, value_type};
use super::{Style, is_js_space};

/// The longhands of `font`, in the order its value lists them.
const LONGHANDS: [&str; 6] = [
    "font-family",
    "font-size",
    "font-style",
    "font-variant",
    "font-weight",
    "line-height",
];

const SIZES: [&str; 9] = [
    "xx-small", "x-small", "small", "medium", "large", "x-large", "xx-large", "larger", "smaller",
];

pub(super) fn set_font(style: &mut Style, value: &str) {
    const SYSTEM_FONTS: [&str; 7] = [
        "caption",
        "icon",
        "menu",
        "message-box",
        "small-caption",
        "status-bar",
        "inherit",
    ];
    if let Some(parts) = parse_shorthand(value) {
        set_shorthand(style, parts);
    } else if value_type(value) == Type::Keyword
        && SYSTEM_FONTS.contains(&value.to_lowercase().as_str())
    {
        style.set("font", value);
    }
}

pub(super) fn set_font_size(style: &mut Style, value: &str) {
    let lower = value.to_lowercase();
    if SIZES.contains(&lower.as_str()) {
        style.set("font-size", &lower);
        return;
    }
    let measurement = match value_type(value) {
        Type::Calc | Type::Length | Type::Percent => value,
        _ if value == "0" => "0px",
        _ => return,
    };
    style.set("font-size", measurement);
}

/// `shorthandSetter`, given what `shorthandParser` made of the value.
fn set_shorthand(style: &mut Style, parts: Vec<(&'static str, String)>) {
    for (longhand, part) in &parts {
        match *longhand {
            "font-size" => set_font_size(style, part),
            _ => style.set(longhand, part),
        }
        let value = style.get(longhand).to_owned();
        style.remove_property(longhand);
        if !value.is_empty() {
            style.set_unlisted(longhand, &value);
        }
    }
    for longhand in LONGHANDS {
        if !parts.iter().any(|(set, _)| *set == longhand) {
            style.remove_property(longhand);
        }
    }
    style.remove_property("font");
    let values = LONGHANDS.map(|longhand| style.get(longhand));
    let calculated = values.iter().filter(|value| !value.is_empty());
    let calculated = calculated.copied().collect::<Vec<_>>().join(" ");
    if !calculated.is_empty() {
        style.set("font", &calculated);
    }
}

/// `shorthandParser`: each longhand and the last part valid for it, in the order first set.
/// `None` when a part is valid for none.
fn parse_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let mut parts: Vec<(&'static str, String)> = Vec::new();
    if value.to_lowercase() == "inherit" {
        return Some(parts);
    }
    for part in split_parts(value) {
        let mut valid = false;
        for longhand in LONGHANDS {
            if !is_valid(longhand, &part) {
                continue;
            }
            valid = true;
            match parts.iter_mut().find(|(set, _)| *set == longhand) {
                Some(entry) => entry.1 = part.clone(),
                None => parts.push((longhand, part.clone())),
            }
        }
        if !valid {
            return None;
        }
    }
    Some(parts)
}

fn is_valid(longhand: &str, part: &str) -> bool {
    let lower = part.to_lowercase();
    match longhand {
        "font-family" => split_commas(part)
            .into_iter()
            .any(|family| matches!(value_type(family), Type::String | Type::Keyword)),
        "font-size" => match value_type(&lower) {
            Type::Length | Type::Percent => true,
            Type::Keyword => SIZES.contains(&lower.as_str()),
            _ => false,
        },
        "font-style" => ["normal", "italic", "oblique", "inherit"].contains(&lower.as_str()),
        "font-variant" => ["normal", "small-caps", "inherit"].contains(&lower.as_str()),
        "font-weight" => [
            "normal", "bold", "bolder", "lighter", "100", "200", "300", "400", "500", "600", "700",
            "800", "900", "inherit",
        ]
        .contains(&lower.as_str()),
        _ => {
            let kind = value_type(part);
            (kind == Type::Keyword && lower == "normal")
                || lower == "inherit"
                || matches!(kind, Type::Number | Type::Length | Type::Percent)
        }
    }
}

/// `getParts`: the value split at white space outside quotes and parentheses.
fn split_parts(value: &str) -> Vec<String> {
    const OPENING: [char; 3] = ['"', '\'', '('];
    const CLOSING: [char; 3] = ['"', '\'', ')'];
    let mut open: Vec<usize> = Vec::new();
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if is_js_space(character) {
            if open.is_empty() {
                if !part.is_empty() {
                    parts.push(std::mem::take(&mut part));
                }
            } else {
                part.push(character);
            }
            continue;
        }
        if character == '\\' {
            // Past the end, JavaScript appends `undefined`.
            match characters.next() {
                Some(escaped) => part.push(escaped),
                None => part.push_str("undefined"),
            }
            continue;
        }
        part.push(character);
        let closing = CLOSING.iter().position(|&close| close == character);
        if closing.is_some() && closing == open.last().copied() {
            open.pop();
        } else if let Some(opening) = OPENING.iter().position(|&open| open == character) {
            open.push(opening);
        }
    }
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}
