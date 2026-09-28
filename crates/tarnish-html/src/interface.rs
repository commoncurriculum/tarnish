//! The interfaces the linkedom fork gives elements, which WebIDL names when it converts one to
//! a string.

use html5ever::{Namespace, ns};

/// The interface an element of this namespace and local name has.
pub(crate) fn interface(namespace: &Namespace, local: &str) -> &'static str {
    if *namespace == ns!(svg) {
        return "SVGElement";
    }
    if *namespace == ns!(mathml) {
        return "MathMLElement";
    }
    if *namespace != ns!(html) {
        return "Element";
    }
    match local {
        "applet" | "bgsound" | "blink" | "isindex" | "keygen" | "multicol" | "nextid"
        | "spacer" => "HTMLUnknownElement",
        "abbr" | "address" | "article" | "aside" | "b" | "bdi" | "bdo" | "cite" | "code" | "dd"
        | "dfn" | "dt" | "em" | "figcaption" | "figure" | "footer" | "header" | "hgroup" | "i"
        | "kbd" | "main" | "mark" | "nav" | "noscript" | "rp" | "rt" | "ruby" | "s" | "samp"
        | "search" | "section" | "small" | "strong" | "sub" | "summary" | "sup" | "u" | "var"
        | "wbr" | "acronym" | "basefont" | "big" | "center" | "nobr" | "noembed" | "noframes"
        | "plaintext" | "rb" | "rtc" | "strike" | "tt" => "HTMLElement",
        "a" => "HTMLAnchorElement",
        "area" => "HTMLAreaElement",
        "audio" => "HTMLAudioElement",
        "base" => "HTMLBaseElement",
        "body" => "HTMLBodyElement",
        "br" => "HTMLBRElement",
        "button" => "HTMLButtonElement",
        "canvas" => "HTMLCanvasElement",
        "data" => "HTMLDataElement",
        "datalist" => "HTMLDataListElement",
        "details" => "HTMLDetailsElement",
        "dialog" => "HTMLDialogElement",
        "dir" => "HTMLDirectoryElement",
        "div" => "HTMLDivElement",
        "dl" => "HTMLDListElement",
        "embed" => "HTMLEmbedElement",
        "fieldset" => "HTMLFieldSetElement",
        "font" => "HTMLFontElement",
        "form" => "HTMLFormElement",
        "frame" => "HTMLFrameElement",
        "frameset" => "HTMLFrameSetElement",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "HTMLHeadingElement",
        "head" => "HTMLHeadElement",
        "hr" => "HTMLHRElement",
        "html" => "HTMLHtmlElement",
        "iframe" => "HTMLIFrameElement",
        "img" => "HTMLImageElement",
        "input" => "HTMLInputElement",
        "label" => "HTMLLabelElement",
        "legend" => "HTMLLegendElement",
        "li" => "HTMLLIElement",
        "link" => "HTMLLinkElement",
        "map" => "HTMLMapElement",
        "marquee" => "HTMLMarqueeElement",
        "menu" => "HTMLMenuElement",
        "meta" => "HTMLMetaElement",
        "meter" => "HTMLMeterElement",
        "del" | "ins" => "HTMLModElement",
        "object" => "HTMLObjectElement",
        "ol" => "HTMLOListElement",
        "optgroup" => "HTMLOptGroupElement",
        "option" => "HTMLOptionElement",
        "output" => "HTMLOutputElement",
        "p" => "HTMLParagraphElement",
        "param" => "HTMLParamElement",
        "picture" => "HTMLPictureElement",
        "listing" | "pre" | "xmp" => "HTMLPreElement",
        "progress" => "HTMLProgressElement",
        "blockquote" | "q" => "HTMLQuoteElement",
        "script" => "HTMLScriptElement",
        "select" => "HTMLSelectElement",
        "slot" => "HTMLSlotElement",
        "source" => "HTMLSourceElement",
        "span" => "HTMLSpanElement",
        "style" => "HTMLStyleElement",
        "caption" => "HTMLTableCaptionElement",
        "th" | "td" => "HTMLTableCellElement",
        "col" | "colgroup" => "HTMLTableColElement",
        "table" => "HTMLTableElement",
        "time" => "HTMLTimeElement",
        "title" => "HTMLTitleElement",
        "tr" => "HTMLTableRowElement",
        "thead" | "tbody" | "tfoot" => "HTMLTableSectionElement",
        "template" => "HTMLTemplateElement",
        "textarea" => "HTMLTextAreaElement",
        "track" => "HTMLTrackElement",
        "ul" => "HTMLUListElement",
        "video" => "HTMLVideoElement",
        _ if is_custom_element_name(local) => "HTMLElement",
        _ => "HTMLUnknownElement",
    }
}

/// HTML's valid custom element name: a lower-case ASCII letter, a hyphen among its characters,
/// no upper-case ASCII letter, and not one of the names SVG and MathML already use.
fn is_custom_element_name(name: &str) -> bool {
    const RESERVED: [&str; 8] = [
        "annotation-xml",
        "color-profile",
        "font-face",
        "font-face-src",
        "font-face-uri",
        "font-face-format",
        "font-face-name",
        "missing-glyph",
    ];
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && name.contains('-')
        && characters.all(|character| {
            matches!(character,
                '-' | '.' | '0'..='9' | '_' | 'a'..='z' | '\u{b7}' | '\u{c0}'..='\u{d6}'
                | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{37d}' | '\u{37f}'..='\u{1fff}'
                | '\u{200c}'..='\u{200d}' | '\u{203f}'..='\u{2040}' | '\u{2070}'..='\u{218f}'
                | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}' | '\u{f900}'..='\u{fdcf}'
                | '\u{fdf0}'..='\u{fffd}' | '\u{10000}'..='\u{effff}')
        })
        && !RESERVED.contains(&name)
}
