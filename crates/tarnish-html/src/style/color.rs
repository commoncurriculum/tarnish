//! cssstyle's `parseColor`, which writes a hex, `rgb()` or `rgba()` color as `rgb()` or
//! `rgba()`, keeps a named color, and refuses the rest. An `hsl()` color, which cssstyle turns
//! into `rgb()` as well, is kept as written.

use tarnish::js::number_to_string;

use super::value::{
    Type, function_argument, hex_digits, is_integer, is_percent, parse_float, split_commas,
    value_type,
};

/// The properties cssstyle sets to the color `parseColor` makes of the value, and the names it
/// sets them under.
pub(super) fn color_property(name: &str) -> Option<&'static str> {
    Some(match name {
        "color" => "color",
        "flood-color" => "flood-color",
        "lighting-color" => "lighting-color",
        "outline-color" => "outline-color",
        "stop-color" => "stop-color",
        "text-line-through-color" => "text-line-through-color",
        "text-overline-color" => "text-overline-color",
        "text-underline-color" => "text-underline-color",
        "webkit-border-after-color" => "-webkit-border-after-color",
        "webkit-border-before-color" => "-webkit-border-before-color",
        "webkit-border-end-color" => "-webkit-border-end-color",
        "webkit-border-start-color" => "-webkit-border-start-color",
        "webkit-column-rule-color" => "-webkit-column-rule-color",
        "webkit-match-nearest-mail-blockquote-color" => {
            "-webkit-match-nearest-mail-blockquote-color"
        }
        "webkit-tap-highlight-color" => "-webkit-tap-highlight-color",
        "webkit-text-emphasis-color" => "-webkit-text-emphasis-color",
        "webkit-text-fill-color" => "-webkit-text-fill-color",
        "webkit-text-stroke-color" => "-webkit-text-stroke-color",
        _ => return None,
    })
}

/// `background-color`'s parser: a color, or `transparent` or `inherit`.
pub(super) fn parse_background(value: &str) -> Option<String> {
    parse(value).or_else(|| {
        let keyword = value_type(value) == Type::Keyword
            && matches!(value.to_lowercase().as_str(), "transparent" | "inherit");
        keyword.then(|| value.to_owned())
    })
}

pub(super) fn parse(value: &str) -> Option<String> {
    if let Some(hex) = hex_digits(value) {
        return Some(parse_hex(hex));
    }
    if let Some(arguments) = function_argument(value, "rgb") {
        let parts = split_commas(arguments);
        if parts.len() != 3 {
            return None;
        }
        let [red, green, blue] = channels(&parts)?;
        return Some(format!("rgb({red}, {green}, {blue})"));
    }
    if let Some(arguments) = function_argument(value, "rgba") {
        let parts = split_commas(arguments);
        if parts.len() != 4 {
            return None;
        }
        let [red, green, blue] = channels(&parts[..3])?;
        let alpha = parse_float(parts[3]);
        let alpha = if alpha.is_nan() {
            1.0
        } else {
            alpha.clamp(0.0, 1.0)
        };
        return Some(match alpha == 1.0 {
            true => format!("rgb({red}, {green}, {blue})"),
            false => format!("rgba({red}, {green}, {blue}, {})", number_to_string(alpha)),
        });
    }
    (value_type(value) == Type::Color).then(|| value.to_owned())
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`, or seven digits, the seventh unread.
fn parse_hex(hex: &str) -> String {
    let digits: String = match hex.len() {
        3 | 4 => hex[..3].chars().flat_map(|digit| [digit, digit]).collect(),
        _ => hex.to_owned(),
    };
    let digits = match hex.len() {
        4 => format!("{digits}{0}{0}", &hex[3..]),
        _ => digits,
    };
    let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).expect("hex digits");
    let (red, green, blue) = (channel(0), channel(2), channel(4));
    match digits.len() {
        8 => {
            let alpha = format!("{:.3}", f64::from(channel(6)) / 255.0);
            let alpha: f64 = alpha.parse().expect("a number");
            format!("rgba({red}, {green}, {blue}, {})", number_to_string(alpha))
        }
        _ => format!("rgb({red}, {green}, {blue})"),
    }
}

/// Three channels, all percentages or all integers, each from 0 to 255.
fn channels(parts: &[&str]) -> Option<[i64; 3]> {
    let values: Vec<f64> = if parts.iter().all(|part| is_percent(part)) {
        let percent = |part: &str| (parse_float(&part[..part.len() - 1]) * 255.0 / 100.0).floor();
        parts.iter().map(|part| percent(part)).collect()
    } else if parts.iter().all(|part| is_integer(part)) {
        parts.iter().map(|part| parse_float(part)).collect()
    } else {
        return None;
    };
    Some([0, 1, 2].map(|index| values[index].clamp(0.0, 255.0) as i64))
}
