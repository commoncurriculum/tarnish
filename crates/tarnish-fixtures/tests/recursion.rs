//! Each recursion in tarnish's Rust grows the stack, or says why its depth has a bound.

use std::path::Path;

use tarnish_fixtures::recursion::unguarded_recursion;

#[test]
fn every_recursion_grows_the_stack_or_is_bounded() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let unguarded: Vec<String> = ["crates", "elixir/test/support"]
        .iter()
        .flat_map(|dir| unguarded_recursion(&root.join(dir)))
        .map(|found| found.to_string())
        .collect();
    assert!(unguarded.is_empty(), "{}", unguarded.join("\n"));
}
