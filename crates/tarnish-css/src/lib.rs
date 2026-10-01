//! A CSS declaration block, as an element's `style` holds one: stylo, Servo's CSS engine,
//! parses, changes and writes it as Firefox does.

#![forbid(unsafe_code)]

use std::sync::LazyLock;

use style::context::QuirksMode;
use style::properties::{
    Importance, NonCustomPropertyId, PropertyDeclarationBlock, PropertyId,
    SourcePropertyDeclaration, parse_one_declaration_into, parse_style_attribute,
};
use style::servo_arc::Arc;
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style_traits::ParsingMode;

mod room;

use room::Room;

/// This crate's version and the stylo it's built on. The fork's WebAssembly gives it as
/// `engine()`, so a test can check that JavaScript and Rust run the same engine.
pub const ENGINE: &str = concat!(env!("CARGO_PKG_VERSION"), "+stylo-0.21.0");

const QUIRKS: QuirksMode = QuirksMode::NoQuirks;

struct Settings {
    /// `about:blank`: only `url()` values would resolve against it, and they keep what they
    /// were written as.
    url: UrlExtraData,
}

static SETTINGS: LazyLock<Settings> = LazyLock::new(|| {
    // Servo turns off the properties its layout doesn't draw yet, but stylo parses them all.
    stylo_static_prefs::set_pref!("layout.columns.enabled", true);
    stylo_static_prefs::set_pref!("layout.container-queries.enabled", true);
    stylo_static_prefs::set_pref!("layout.grid.enabled", true);
    stylo_static_prefs::set_pref!("layout.unimplemented", true);
    stylo_static_prefs::set_pref!("layout.variable_fonts.enabled", true);
    stylo_static_prefs::set_pref!("layout.writing-mode.enabled", true);
    Settings {
        url: UrlExtraData(Arc::new(url::Url::parse("about:blank").expect("a URL"))),
    }
});

fn property(name: &str) -> Option<PropertyId> {
    LazyLock::force(&SETTINGS);
    PropertyId::parse_enabled_for_all_content(name).ok()
}

/// Every property name a declaration can have, aliases among them, but not custom
/// properties.
pub fn property_names() -> impl Iterator<Item = String> {
    LazyLock::force(&SETTINGS);
    NonCustomPropertyId::iter()
        .filter(|id| id.to_property_id().enabled_for_all_content())
        .map(|id| id.name().to_owned())
}

/// The declarations of a `style` attribute, with CSSOM's operations on them.
pub struct Declarations {
    block: PropertyDeclarationBlock,
    /// As deep as any CSS they were parsed from nests, which stylo walks them as deep as.
    room: Room,
}

impl Declarations {
    pub fn parse(css: &str) -> Self {
        let room = Room::of(css);
        let block = room
            .run(|| parse_style_attribute(css, &SETTINGS.url, None, QUIRKS, CssRuleType::Style));
        Declarations { block, room }
    }

    /// `cssText`.
    pub fn css_text(&self) -> String {
        let mut text = String::new();
        self.room
            .run(|| self.block.to_css(&mut text))
            .expect("writing to a string");
        text
    }

    /// `length`: how many longhands and custom properties are declared.
    pub fn len(&self) -> usize {
        self.block.len()
    }

    pub fn is_empty(&self) -> bool {
        self.block.is_empty()
    }

    /// `item(index)`: the name of the declaration at `index`.
    pub fn item(&self, index: usize) -> Option<String> {
        let declaration = self.block.declarations().get(index)?;
        Some(declaration.id().name().into_owned())
    }

    /// Every `item(index)`, in order.
    pub fn names(&self) -> Vec<String> {
        let declarations = self.block.declarations().iter();
        declarations
            .map(|declaration| declaration.id().name().into_owned())
            .collect()
    }

    /// `getPropertyValue(name)`.
    pub fn value(&self, name: &str) -> String {
        property(name).map_or_else(String::new, |id| self.room.run(|| self.value_of(&id)))
    }

    fn value_of(&self, id: &PropertyId) -> String {
        let mut value = String::new();
        self.block
            .property_value_to_css(id, &mut value)
            .expect("writing to a string");
        value
    }

    /// `getPropertyPriority(name)`.
    pub fn priority(&self, name: &str) -> &'static str {
        match property(name).map(|id| self.block.property_priority(&id)) {
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
            return self.remove_id(&id).is_some();
        }
        let importance = match priority.eq_ignore_ascii_case("important") {
            true => Importance::Important,
            false if priority.is_empty() => Importance::Normal,
            false => return false,
        };
        let room = self.room.max(Room::of(value));
        let block = &mut self.block;
        let changed = room.run(|| {
            let mut declarations = SourcePropertyDeclaration::default();
            let parsed = parse_one_declaration_into(
                &mut declarations,
                id,
                value,
                Origin::Author,
                &SETTINGS.url,
                None,
                ParsingMode::DEFAULT,
                QUIRKS,
                CssRuleType::Style,
            );
            let mut updates = Default::default();
            if parsed.is_err() || !block.prepare_for_update(&declarations, importance, &mut updates)
            {
                return false;
            }
            block.update(declarations.drain(), importance, &mut updates);
            true
        });
        if changed {
            self.room = room;
        }
        changed
    }

    /// `removeProperty(name)`: the value it had, if it was declared.
    pub fn remove(&mut self, name: &str) -> Option<String> {
        self.remove_id(&property(name)?)
    }

    fn remove_id(&mut self, id: &PropertyId) -> Option<String> {
        let first = self.block.first_declaration_to_remove(id)?;
        self.room.run(|| {
            let value = self.value_of(id);
            self.block.remove_property(id, first);
            Some(value)
        })
    }
}

impl Drop for Declarations {
    fn drop(&mut self) {
        let block = std::mem::take(&mut self.block);
        self.room.run(|| drop(block));
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
        let names = style.names();
        assert_eq!(names.len(), style.len());
        assert_eq!(names[0], "font-style");
        assert_eq!(names.last().map(String::as_str), Some("color"));
        let items: Vec<String> = (0..style.len())
            .filter_map(|index| style.item(index))
            .collect();
        assert_eq!(names, items);
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
    fn styles_parse_as_in_a_no_quirks_document() {
        assert!(Declarations::parse("width: 10; color: f00").is_empty());
    }

    #[test]
    fn the_engine_names_the_stylo_it_is_built_on() {
        let (_, stylo) = ENGINE.split_once("+stylo-").expect("stylo's version");
        let pin = format!("stylo = \"={stylo}\"");
        assert!(include_str!("../Cargo.toml").contains(&pin), "{pin}");
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
