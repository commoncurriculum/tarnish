//! Writing a document as HTML as it goes: the `innerHTML` of what `DOMSerializer` would build in
//! an [`HtmlDom`], without building it. A spec of elements with ordinary names is written
//! directly; any other is rendered in a DOM of the writer's own and written from there.

use tarnish::chunk::ValueRef;
use tarnish::dom::{DomSpec, SpecAttrs, Target, is_hole, render_spec_of};
use tarnish::{Error, Result, TextRef};

use crate::dom::{HtmlDom, HtmlNode};
use crate::names::NameKind;
use crate::serialize::{escape_attribute, escape_text, is_raw_text, is_void};

/// How content written where the writer is shows in the HTML.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Content {
    /// Escaped, as in most elements.
    Text,
    /// As it is, as in a `<style>` or `<script>`.
    Raw,
    /// Not at all, as in a void element or a `<template>`, whose children aren't written.
    Dropped,
}

impl Content {
    /// How content shows inside an HTML element of this local name.
    pub(crate) fn of_html(local: &str) -> Content {
        match local {
            _ if is_void(local) || local == "template" => Content::Dropped,
            _ if is_raw_text(local) => Content::Raw,
            _ => Content::Text,
        }
    }
}

/// Where the nodes after an open element or mark go: the start of its tail on the tail stack,
/// and how content went before it.
pub struct Parent {
    tail: usize,
    content: Content,
    refused: Option<Error>,
}

/// Writes HTML as a serializer renders the specs.
pub struct HtmlWriter {
    out: String,
    /// What follows the content of each element still open, the innermost last.
    tails: String,
    content: Content,
    /// The error of appending where the writer is, when the DOM wouldn't take a child there: in
    /// a mark whose spec is text.
    refused: Option<Error>,
    /// What content that isn't written goes to.
    dropped: String,
    /// Where specs that aren't written directly are rendered.
    dom: Option<HtmlDom>,
}

impl Default for HtmlWriter {
    fn default() -> Self {
        HtmlWriter::new()
    }
}

impl HtmlWriter {
    pub fn new() -> Self {
        HtmlWriter {
            out: String::with_capacity(4096),
            tails: String::new(),
            content: Content::Text,
            refused: None,
            dropped: String::new(),
            dom: None,
        }
    }

    /// The HTML written.
    pub fn finish(self) -> String {
        self.out
    }

    fn sink(&mut self) -> &mut String {
        match self.content {
            Content::Dropped => &mut self.dropped,
            Content::Text | Content::Raw => &mut self.out,
        }
    }

    /// Opens content inside an element whose content shows as `inner`: the nodes after it go
    /// in the element until [`close`](Self::close).
    fn open(&mut self, tail: usize, inner: Content) -> Parent {
        let parent = Parent {
            tail,
            content: self.content,
            refused: self.refused.take(),
        };
        if self.content != Content::Dropped {
            self.content = inner;
        }
        parent
    }

    fn close(&mut self, parent: Parent) {
        self.content = parent.content;
        self.refused = parent.refused;
        let tail = self.tails.split_off(parent.tail);
        self.sink().push_str(&tail);
    }

    /// Runs `render`, which renders what the DOM would build before it appends it where the
    /// writer is, and fails as the DOM would append it if it takes no child there.
    fn appending<T>(&mut self, render: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let Some(refused) = self.refused.take() else {
            return render(self);
        };
        let rendered = render(self);
        self.refused = Some(refused.clone());
        rendered?;
        Err(refused)
    }

    /// Renders a spec: its HTML up to where its content goes, the rest of it on the tail stack.
    /// `Some` where the content goes: for a node, when its spec has a hole; for a mark, always,
    /// as without a hole its content goes at the end of its element.
    fn render(
        &mut self,
        spec: DomSpec<'_, HtmlNode>,
        attrs: ValueRef<'_>,
        mark: bool,
    ) -> Result<Option<Parent>> {
        // A mark whose spec is text puts its content in a text node, which the DOM refuses.
        if mark && matches!(spec, DomSpec::Text(_) | DomSpec::Attr(_)) {
            return self.render_in_dom(spec, attrs, mark);
        }
        let mut out = std::mem::take(self.sink());
        let (written, tail) = (out.len(), self.tails.len());
        let hole = write_spec(&mut out, &mut self.tails, &spec, self.content, mark);
        if let Err(Stop::InDom) = hole {
            out.truncate(written);
            self.tails.truncate(tail);
        }
        *self.sink() = out;
        match hole {
            Ok(hole) => Ok(hole.map(|inner| self.open(tail, inner))),
            Err(Stop::InDom) => self.render_in_dom(spec, attrs, mark),
            Err(Stop::Error(error)) => Err(error),
        }
    }

    fn render_in_dom(
        &mut self,
        spec: DomSpec<'_, HtmlNode>,
        attrs: ValueRef<'_>,
        mark: bool,
    ) -> Result<Option<Parent>> {
        let dom = self.dom.get_or_insert_with(HtmlDom::new);
        let rendered = render_spec_of(dom, &spec, attrs)?;
        let at = match (rendered.content_dom, mark) {
            (Some(content_dom), _) => content_dom,
            (None, true) => rendered.dom.clone(),
            (None, false) => {
                let html = rendered.dom.outer_html_in(self.content);
                self.sink().push_str(&html);
                return Ok(None);
            }
        };
        let (before, after, inner) = rendered.dom.outer_html_split(&at)?;
        self.sink().push_str(&before);
        let tail = self.tails.len();
        self.tails.push_str(&after);
        Ok(Some(match inner {
            Ok(inner) => self.open(tail, inner),
            Err(refused) => {
                let parent = self.open(tail, Content::Dropped);
                self.content = Content::Dropped;
                self.refused = Some(refused);
                parent
            }
        }))
    }
}

impl Target<HtmlNode> for HtmlWriter {
    type Parent = Parent;

    fn open_mark(&mut self, spec: DomSpec<'_, HtmlNode>, attrs: ValueRef<'_>) -> Result<Parent> {
        self.appending(|writer| writer.render(spec, attrs, true))
            .map(|parent| parent.expect("a mark's content"))
    }

    fn close_mark(&mut self, parent: Parent) -> Result<()> {
        self.close(parent);
        Ok(())
    }

    fn node(
        &mut self,
        spec: DomSpec<'_, HtmlNode>,
        attrs: ValueRef<'_>,
        content: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        self.appending(|writer| {
            if let Some(parent) = writer.render(spec, attrs, false)? {
                content(writer)?;
                writer.close(parent);
            }
            Ok(())
        })
    }

    fn text(&mut self, text: TextRef<'_>) -> Result<()> {
        if let Some(refused) = &self.refused {
            return Err(refused.clone());
        }
        let text = text.to_string_lossy();
        let content = self.content;
        let sink = self.sink();
        match content {
            Content::Raw => sink.push_str(&text),
            Content::Text | Content::Dropped => escape_text(sink, &text),
        }
        Ok(())
    }
}

/// Why [`write_spec`] stopped: the spec holds something it doesn't write as the DOM would, for
/// the spec to be rendered in a DOM instead, or rendering it failed.
enum Stop {
    InDom,
    Error(Error),
}

impl From<Error> for Stop {
    fn from(error: Error) -> Stop {
        Stop::Error(error)
    }
}

/// The bytes of a name the DOM keeps as it is, whatever their place.
const PLAIN: [bool; 256] = {
    let mut plain = [false; 256];
    let mut byte = 0;
    while byte < 256 {
        plain[byte] = matches!(byte as u8, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b':');
        byte += 1;
    }
    plain
};

/// Whether the DOM would make an element or attribute of this name with the name as it is: a
/// name of lower-case ASCII letters, digits and `-_.:`, an element's starting with a letter,
/// which is valid and which nothing lower-cases. Other names go through the DOM.
fn plain_name(name: &str, kind: NameKind) -> bool {
    let bytes = name.as_bytes();
    let starts = match kind {
        NameKind::Element => bytes.first().is_some_and(u8::is_ascii_lowercase),
        NameKind::Attribute => !bytes.is_empty(),
    };
    starts && bytes.iter().all(|&byte| PLAIN[byte as usize])
}

/// Writes a spec as `renderSpec` would build it and `innerHTML` write it: up to where its
/// content goes to `out`, and what follows the content onto `tails`. Gives how content shows
/// where it goes: where the spec has a hole, or for a mark's element without one, at its end.
/// It writes HTML elements with valid names in lower case, text and holes, and stops at
/// anything else, which goes through the DOM, in the order the DOM would make it, so that a
/// spec fails as the DOM fails it.
fn write_spec(
    out: &mut String,
    tails: &mut String,
    spec: &DomSpec<'_, HtmlNode>,
    content: Content,
    mark: bool,
) -> Result<Option<Content>, Stop> {
    let (tag, attrs, children) = match spec {
        DomSpec::Text(text) => {
            write_text(out, text, content);
            return Ok(None);
        }
        DomSpec::Attr(value) => {
            match value.as_str() {
                Some(text) => write_text(out, text, content),
                None => return Err(Stop::InDom),
            }
            return Ok(None);
        }
        DomSpec::Wrapping { tag, attrs } => {
            write_start(out, tag, attrs)?;
            write_end(tails, tag);
            return Ok(Some(Content::of_html(tag)));
        }
        DomSpec::Element {
            tag,
            attrs,
            children,
        } => (*tag, attrs, children),
        DomSpec::Hole
        | DomSpec::Node(_)
        | DomSpec::Rendered(_)
        | DomSpec::Array { .. }
        | DomSpec::Value(_) => return Err(Stop::InDom),
    };
    write_start(out, tag, attrs)?;
    let inner = Content::of_html(tag);
    // The children of a void element or a template are built but not written, nor what follows
    // a hole in one of them.
    let (mut dropped, mut dropped_tails) = (String::new(), String::new());
    let (body, body_tails) = match inner {
        Content::Dropped => (&mut dropped, &mut dropped_tails),
        Content::Text | Content::Raw => (&mut *out, &mut *tails),
    };
    let mut hole = None;
    for child in children {
        if is_hole(child) {
            if children.len() > 1 {
                return Err(Stop::Error(Error::Range(
                    "Content hole must be the only child of its parent node".into(),
                )));
            }
            hole = Some(inner);
            break;
        }
        match hole {
            // Once a child holds the content, what follows it follows the content.
            Some(_) => {
                let mut after = String::new();
                let written = tarnish::js::stack::grow(|| {
                    write_spec(&mut after, &mut String::new(), child, inner, false)
                })?;
                if written.is_some() {
                    return Err(Stop::Error(Error::Range("Multiple content holes".into())));
                }
                body_tails.push_str(&after);
            }
            None => {
                hole =
                    tarnish::js::stack::grow(|| write_spec(body, body_tails, child, inner, false))?
            }
        }
    }
    let hole = match (hole, inner) {
        (Some(_), Content::Dropped) => Some(Content::Dropped),
        (None, _) if mark => Some(inner),
        (hole, _) => hole,
    };
    match hole {
        Some(_) => write_end(tails, tag),
        None => write_end(out, tag),
    }
    Ok(hole)
}

fn write_text(out: &mut String, text: &str, content: Content) {
    match content {
        Content::Raw => out.push_str(text),
        Content::Text | Content::Dropped => escape_text(out, text),
    }
}

/// The start tag of an HTML element with the spec's attributes that aren't null, in order:
/// `style` as the CSS engine keeps its declarations, as `style.cssText` sets it.
fn write_start(out: &mut String, tag: &str, attrs: &SpecAttrs) -> Result<(), Stop> {
    if !plain_name(tag, NameKind::Element) {
        return Err(Stop::InDom);
    }
    out.push('<');
    out.push_str(tag);
    for (name, value) in attrs.iter() {
        if value.is_null() {
            continue;
        }
        if !plain_name(name, NameKind::Attribute) {
            return Err(Stop::InDom);
        }
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        let value = value.to_js_string()?;
        match name {
            "style" => escape_attribute(out, &crate::style::css_text(&value)),
            _ => escape_attribute(out, &value),
        }
        out.push('"');
    }
    out.push('>');
    Ok(())
}

fn write_end(out: &mut String, tag: &str) {
    if !is_void(tag) {
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
    }
}
