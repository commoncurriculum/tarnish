//! A CSS declaration block, as an element's `style` holds one: stylo, Servo's CSS engine,
//! parses, changes and writes it as Firefox does. The linkedom fork's `element.style` is this
//! crate compiled to WebAssembly, so the two agree on every declaration.
//!
//! Declarations parse as in a no-quirks document, as a `<template>`'s content is.

#![forbid(unsafe_code)]

use std::sync::{LazyLock, Once};

use style::context::QuirksMode;
use style::properties::{
    Importance, NonCustomPropertyId, PropertyDeclarationBlock, PropertyId,
    SourcePropertyDeclaration, parse_one_declaration_into, parse_style_attribute,
};
use style::servo_arc::Arc;
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style_traits::ParsingMode;

/// The document's URL, which only `url()` values would resolve against, and they keep what
/// they were written as.
static URL: LazyLock<UrlExtraData> =
    LazyLock::new(|| UrlExtraData(Arc::new(url::Url::parse("about:blank").expect("a URL"))));

/// Servo turns off the properties its layout doesn't draw yet, but stylo parses them all.
fn enable_properties() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        stylo_static_prefs::set_pref!("layout.columns.enabled", true);
        stylo_static_prefs::set_pref!("layout.container-queries.enabled", true);
        stylo_static_prefs::set_pref!("layout.grid.enabled", true);
        stylo_static_prefs::set_pref!("layout.unimplemented", true);
        stylo_static_prefs::set_pref!("layout.variable_fonts.enabled", true);
        stylo_static_prefs::set_pref!("layout.writing-mode.enabled", true);
    });
}

fn property(name: &str) -> Option<PropertyId> {
    enable_properties();
    PropertyId::parse_enabled_for_all_content(name).ok()
}

/// Every property name a declaration can have, aliases among them, but not custom
/// properties.
pub fn property_names() -> impl Iterator<Item = String> {
    enable_properties();
    NonCustomPropertyId::iter()
        .filter(|id| id.to_property_id().enabled_for_all_content())
        .map(|id| id.name().to_owned())
}

/// The declarations of a `style` attribute, with CSSOM's operations on them.
#[derive(Default)]
pub struct Declarations(PropertyDeclarationBlock);

impl Declarations {
    /// Parse a `style` attribute.
    pub fn parse(css: &str) -> Self {
        enable_properties();
        Declarations(parse_style_attribute(
            css,
            &URL,
            None,
            QuirksMode::NoQuirks,
            CssRuleType::Style,
        ))
    }

    /// `cssText`.
    pub fn css_text(&self) -> String {
        let mut text = String::new();
        self.0.to_css(&mut text).expect("writing to a string");
        text
    }

    /// `length`: how many longhands and custom properties are declared.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `item(index)`: the name of the declaration at `index`.
    pub fn item(&self, index: usize) -> Option<String> {
        let declaration = self.0.declarations().get(index)?;
        Some(declaration.id().name().into_owned())
    }

    /// `getPropertyValue(name)`.
    pub fn value(&self, name: &str) -> String {
        let mut value = String::new();
        if let Some(id) = property(name) {
            self.0
                .property_value_to_css(&id, &mut value)
                .expect("writing to a string");
        }
        value
    }

    /// `getPropertyPriority(name)`.
    pub fn priority(&self, name: &str) -> &'static str {
        match property(name).map(|id| self.0.property_priority(&id)) {
            Some(Importance::Important) => "important",
            _ => "",
        }
    }

    /// `setProperty(name, value, priority)`, which an empty value makes a removal. Whether the
    /// declarations changed.
    pub fn set(&mut self, name: &str, value: &str, priority: &str) -> bool {
        let Some(id) = property(name) else {
            return false;
        };
        if value.is_empty() {
            return self.remove(name).is_some();
        }
        let importance = match priority.eq_ignore_ascii_case("important") {
            true => Importance::Important,
            false if priority.is_empty() => Importance::Normal,
            false => return false,
        };
        let mut declarations = SourcePropertyDeclaration::default();
        let parsed = parse_one_declaration_into(
            &mut declarations,
            id,
            value,
            Origin::Author,
            &URL,
            None,
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            CssRuleType::Style,
        );
        let mut updates = Default::default();
        if parsed.is_err()
            || !self
                .0
                .prepare_for_update(&declarations, importance, &mut updates)
        {
            return false;
        }
        self.0
            .update(declarations.drain(), importance, &mut updates);
        true
    }

    /// `removeProperty(name)`: the value it had, if it was declared.
    pub fn remove(&mut self, name: &str) -> Option<String> {
        let id = property(name)?;
        let first = self.0.first_declaration_to_remove(&id)?;
        let value = self.value(name);
        self.0.remove_property(&id, first);
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorthands_expand_and_serialize_back() {
        let style = Declarations::parse("font: italic bold 12px/30px Georgia, serif; color: #f00");
        assert_eq!(style.value("font-weight"), "bold");
        assert_eq!(style.value("color"), "rgb(255, 0, 0)");
        assert_eq!(
            style.css_text(),
            "font: italic bold 12px / 30px Georgia, serif; color: rgb(255, 0, 0);"
        );
    }

    #[test]
    fn set_and_remove_change_the_declarations() {
        let mut style = Declarations::parse("color: red");
        assert!(style.set("Font-Weight", "BOLD", "IMPORTANT"));
        assert!(!style.set("font-weight", "bold", "important"));
        assert!(!style.set("font-weight", "heavy", ""));
        assert!(!style.set("font-weight", "bold", "urgent"));
        assert_eq!(style.priority("font-weight"), "important");
        assert_eq!(
            style.css_text(),
            "color: red; font-weight: bold !important;"
        );
        assert_eq!(style.remove("color").as_deref(), Some("red"));
        assert_eq!(style.remove("color"), None);
        assert!(style.set("font-weight", "", ""));
        assert!(style.is_empty());
    }

    #[test]
    fn names_are_properties_and_their_aliases() {
        let names: Vec<String> = property_names().collect();
        for name in [
            "color",
            "font",
            "grid-template-columns",
            "-webkit-transform",
        ] {
            assert!(names.iter().any(|known| known == name), "{name}");
        }
    }
}
