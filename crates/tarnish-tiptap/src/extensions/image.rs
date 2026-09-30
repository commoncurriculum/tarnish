//! `@tiptap/extension-image`.

use crate::NodeExtension;

pub struct ImageOptions {
    pub inline: bool,
}

pub fn image(options: ImageOptions) -> NodeExtension {
    NodeExtension::create("image")
        .inline(options.inline)
        .group(if options.inline { "inline" } else { "block" })
}
