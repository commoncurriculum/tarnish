//! An element's inline style, as jsdom 20 reads it from the `style` attribute: CSSOM splits the
//! declarations, and cssstyle 2.3.0 keeps those of the properties it knows.
//!
//! cssstyle also parses the values of some properties, dropping those it can't, and splits
//! shorthands into their longhands. This does both for the properties mark rules read: `font`
//! and its longhands, and the colors (`color`, `background-color` and the like). Other values
//! are kept as written, where cssstyle checks lengths such as `width`'s and splits `background`,
//! `border`, `flex`, `margin` and `padding`.

mod color;
mod cssom;
mod font;
mod known;
mod value;

#[derive(Default)]
pub(crate) struct Style {
    /// The properties in the order they were set, which `length` counts.
    list: Vec<String>,
    /// Each property's value and whether it is important. A shorthand's longhands have values
    /// here without being listed.
    values: Vec<(String, String, bool)>,
}

impl Style {
    /// The style that setting the attribute to this gives.
    pub(crate) fn parse(css: &str) -> Style {
        Style::parse_css(css).unwrap_or_default()
    }

    /// The style that setting `cssText` to this gives, `None` when CSSOM throws, which leaves
    /// the attribute as it was.
    pub(crate) fn parse_css(css: &str) -> Option<Style> {
        let mut style = Style::default();
        for declaration in cssom::parse(css)? {
            style.set_property(&declaration.name, &declaration.value, declaration.important);
        }
        Some(style)
    }

    pub(crate) fn len(&self) -> usize {
        self.list.len()
    }

    /// `getPropertyValue`.
    pub(crate) fn get(&self, name: &str) -> &str {
        let value = self.values.iter().find(|(set, ..)| set == name);
        value.map_or("", |(_, value, _)| value)
    }

    pub(crate) fn css_text(&self) -> String {
        let declarations = self.list.iter().map(|name| {
            let (_, value, important) = self.entry(name).expect("a listed property's value");
            let priority = if *important { " !important" } else { "" };
            format!("{name}: {value}{priority};")
        });
        declarations.collect::<Vec<_>>().join(" ")
    }

    fn entry(&self, name: &str) -> Option<&(String, String, bool)> {
        self.values.iter().find(|(set, ..)| set == name)
    }

    fn set_property(&mut self, name: &str, value: &str, important: bool) {
        if value.is_empty() {
            self.remove_property(name);
            return;
        }
        if name.starts_with("--") {
            self.set(name, value);
        } else {
            let name = name.to_lowercase();
            if known::PROPERTIES.binary_search(&name.as_str()).is_err() {
                return;
            }
            let color = |parse: fn(&str) -> Option<String>| parse(value);
            match (name.as_str(), color::color_property(&name)) {
                ("font", _) => font::set_font(self, value),
                ("font-size", _) => font::set_font_size(self, value),
                ("background-color", _) => {
                    if let Some(color) = color(color::parse_background) {
                        self.set("background-color", &color);
                    }
                }
                (_, Some(stored)) => {
                    if let Some(color) = color(color::parse) {
                        self.set(stored, &color);
                    }
                }
                _ => self.set(&name, value),
            }
        }
        let name = match name.starts_with("--") {
            true => name.to_owned(),
            false => name.to_lowercase(),
        };
        if let Some(entry) = self.values.iter_mut().find(|(set, ..)| *set == name) {
            entry.2 = important;
        }
    }

    /// cssstyle's `_setProperty`: set the value, listing the property if it isn't.
    fn set(&mut self, name: &str, value: &str) {
        if !self.list.iter().any(|listed| listed == name) {
            self.list.push(name.to_owned());
        }
        self.set_unlisted(name, value);
    }

    fn set_unlisted(&mut self, name: &str, value: &str) {
        match self.values.iter_mut().find(|(set, ..)| set == name) {
            Some(entry) => (entry.1, entry.2) = (value.to_owned(), false),
            None => self.values.push((name.to_owned(), value.to_owned(), false)),
        }
    }

    fn remove_property(&mut self, name: &str) {
        self.values.retain(|(set, ..)| set != name);
        self.list.retain(|listed| listed != name);
    }
}

/// JavaScript's white space and line terminators, which `trim` and `\s` take.
fn is_js_space(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_space)
}

#[cfg(test)]
mod tests {
    use super::known::{NAMED_COLORS, PROPERTIES, SYSTEM_COLORS};

    #[test]
    fn known_names_are_sorted_for_search() {
        for list in [&PROPERTIES[..], &NAMED_COLORS, &SYSTEM_COLORS] {
            assert!(list.windows(2).all(|pair| pair[0] < pair[1]));
        }
    }
}
