//! CSS whose blocks nest far deeper than a small stack could recurse through, parsed, read,
//! changed and dropped on a thread with such a stack: stylo recurses once a level, so each call
//! into it needs room for as deep as its CSS nests.

use tarnish_css::Declarations;
use tarnish_js::stack::on_dirty_scheduler_stack;

const DEPTH: usize = 100_000;

/// How deep `color-mix()` nests, which stylo parses in time that grows with the square of the
/// depth. Its value keeps the nesting, as the others' don't all.
const MIXES: usize = 1_000;

fn nested(open: &str, inside: &str, close: &str, levels: usize) -> String {
    open.repeat(levels) + inside + &close.repeat(levels)
}

/// The value of the declaration `name`, read and written as `cssText`, is `value`.
fn holds(style: &Declarations, name: &str, value: &str) {
    assert!(style.value(name) == value, "{name}: getPropertyValue");
    assert!(
        style.css_text() == format!("{name}: {value};"),
        "{name}: cssText"
    );
}

#[test]
fn declarations_nest_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let parens = nested("(", "", ")", DEPTH);
        let fallback = format!("var(--a,{parens})");
        let mix = nested("color-mix(in srgb, red, ", "blue", ")", MIXES);
        // The name, the value given, and the value as stylo keeps it.
        let declarations = [
            ("--a", "(".repeat(DEPTH), parens.clone()),
            (
                "--b",
                nested("[", "", "]", DEPTH),
                nested("[", "", "]", DEPTH),
            ),
            ("color", fallback.clone(), fallback),
            (
                "width",
                nested("calc(", "1px", ")", DEPTH),
                "calc(1px)".into(),
            ),
            ("color", mix.clone(), mix),
        ];
        for (name, given, kept) in declarations {
            let parsed = Declarations::parse(&format!("{name}: {given}"));
            holds(&parsed, name, &kept);

            let mut set = Declarations::parse("");
            assert!(set.set(name, &given, ""), "{name}: setProperty");
            holds(&set, name, &kept);
            assert!(
                set.remove(name).is_some_and(|removed| removed == kept),
                "{name}: removeProperty"
            );
            assert!(set.is_empty(), "{name}: removed");
        }
    });
}
