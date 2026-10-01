//! The ProseMirror tarnish ports is the one `harness/record-versions.mjs` finds installed.

use tarnish_fixtures::version;

#[test]
fn ports_the_installed_prosemirror() {
    let model = version("@tiptap/pm > prosemirror-model");
    assert_eq!(model, tarnish::PROSEMIRROR_MODEL);
    let transform = version("@tiptap/pm > prosemirror-transform");
    assert_eq!(transform, tarnish::PROSEMIRROR_TRANSFORM);
}
