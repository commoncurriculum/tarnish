//! Checking element and attribute names as the DOM does before it creates them, and the
//! interfaces jsdom gives elements, which `String(element)` names.

use html5ever::{Namespace, Prefix, QualName, ns};
use tarnish::{Error, Result};

fn is_name_start(character: char) -> bool {
    matches!(character,
        'A'..='Z' | '_' | 'a'..='z' | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}'
        | '\u{f8}'..='\u{2ff}' | '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}'
        | '\u{200c}'..='\u{200d}' | '\u{2070}'..='\u{218f}' | '\u{2c00}'..='\u{2fef}'
        | '\u{3001}'..='\u{d7ff}' | '\u{f900}'..='\u{fdcf}' | '\u{fdf0}'..='\u{fffd}'
        | '\u{10000}'..='\u{effff}')
}

fn is_name_char(character: char) -> bool {
    is_name_start(character)
        || matches!(character,
            '-' | '.' | '0'..='9' | '\u{b7}' | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}')
}

/// XML's `NCName`: a name without a colon.
fn is_nc_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(is_name_start) && characters.all(is_name_char)
}

/// XML's `Name` production.
fn is_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first == ':' || is_name_start(first))
        && characters.all(|character| character == ':' || is_name_char(character))
}

fn invalid_character(name: &str, production: &str) -> Error {
    Error::Other(format!(
        "InvalidCharacterError: \"{name}\" did not match the {production} production"
    ))
}

fn namespace_error(message: &str) -> Error {
    Error::Other(format!("NamespaceError: {message}"))
}

/// Refuse a name that isn't XML's `Name`, as `createElement` and `setAttribute` do.
pub(crate) fn validate(name: &str) -> Result<()> {
    match is_name(name) {
        true => Ok(()),
        false => Err(invalid_character(name, "Name")),
    }
}

/// The DOM's "validate and extract": the qualified name's namespace, prefix and local name, as
/// `createElementNS` and `setAttributeNS` take them.
pub(crate) fn validate_and_extract(namespace: Option<&str>, qualified: &str) -> Result<QualName> {
    let namespace = namespace.filter(|namespace| !namespace.is_empty());
    let is_qname = match qualified.split_once(':') {
        Some((prefix, local)) => is_nc_name(prefix) && is_nc_name(local),
        None => is_nc_name(qualified),
    };
    if !is_qname {
        return Err(invalid_character(qualified, "QName"));
    }
    let (prefix, local) = match qualified.split_once(':') {
        Some((prefix, local)) => (Some(prefix), local),
        None => (None, qualified),
    };
    const XML: &str = "http://www.w3.org/XML/1998/namespace";
    const XMLNS: &str = "http://www.w3.org/2000/xmlns/";
    if prefix.is_some() && namespace.is_none() {
        return Err(namespace_error(
            "A namespace was given but a prefix was also extracted from the qualifiedName",
        ));
    }
    if prefix == Some("xml") && namespace != Some(XML) {
        return Err(namespace_error(
            "A prefix of \"xml\" was given but the namespace was not the XML namespace",
        ));
    }
    if (qualified == "xmlns" || prefix == Some("xmlns")) && namespace != Some(XMLNS) {
        return Err(namespace_error(
            "A prefix or qualifiedName of \"xmlns\" was given but the namespace was not the XMLNS namespace",
        ));
    }
    if namespace == Some(XMLNS) && qualified != "xmlns" && prefix != Some("xmlns") {
        return Err(namespace_error(
            "The XMLNS namespace was given but neither the prefix nor qualifiedName was \"xmlns\"",
        ));
    }
    Ok(QualName::new(
        prefix.map(Prefix::from),
        namespace.map_or(ns!(), Namespace::from),
        local.into(),
    ))
}

/// The interface jsdom 20 makes an element of this namespace and local name.
pub(crate) fn interface(namespace: &Namespace, local: &str) -> &'static str {
    if *namespace == ns!(svg) {
        return match local {
            "svg" => "SVGSVGElement",
            "title" => "SVGTitleElement",
            _ => "SVGElement",
        };
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
        | "section" | "small" | "strong" | "sub" | "summary" | "sup" | "u" | "var" | "wbr"
        | "acronym" | "basefont" | "big" | "center" | "nobr" | "noembed" | "noframes"
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
