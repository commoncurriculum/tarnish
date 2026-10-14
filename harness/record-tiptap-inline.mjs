// Records Tiptap's inline extensions, and the extensions that give other types attributes, to
// fixtures/tiptap-inline.json, as harness/tiptap.mjs records them. TextStyle's attributes,
// TextAlign, UniqueID and Global attributes have no Markdown hooks, and Mention's Markdown reads
// none of its options.
import { Extension, Mark, Node as TiptapNode, splitExtensions } from "@tiptap/core"
import { Bold } from "@tiptap/extension-bold"
import { Color } from "@tiptap/extension-color"
import { Document } from "@tiptap/extension-document"
import { FontFamily } from "@tiptap/extension-font-family"
import { HardBreak } from "@tiptap/extension-hard-break"
import { Heading } from "@tiptap/extension-heading"
import { Italic } from "@tiptap/extension-italic"
import { Link } from "@tiptap/extension-link"
import { Mention } from "@tiptap/extension-mention"
import { Paragraph } from "@tiptap/extension-paragraph"
import { Text } from "@tiptap/extension-text"
import { TextAlign } from "@tiptap/extension-text-align"
import * as textStyles from "@tiptap/extension-text-style"
import { BackgroundColor, FontSize, LineHeight, TextStyle, TextStyleKit } from "@tiptap/extension-text-style"
import { UniqueID } from "@tiptap/extension-unique-id"
import { doc, fixture, own, paragraph, record, text, unique, withParsed } from "./tiptap.mjs"

// Global attributes for each kind of `types`: `'*'`, `'nodes'`, `'marks'`, an array that names a
// type twice and one there isn't, and an array an extension makes of the node extensions it's given.
const Globals = Extension.create({
  name: "globals",
  addGlobalAttributes() {
    const { nodeExtensions } = splitExtensions(this.extensions)
    return [
      { types: "*", attributes: { every: { default: "*" } } },
      { types: "nodes", attributes: { node: { default: "n" }, every: { default: "node" } } },
      { types: "marks", attributes: { mark: { default: null } } },
      { types: ["paragraph", "bold", "paragraph", "missing"], attributes: { named: { default: 1 } } },
      {
        types: nodeExtensions.map(extension => extension.name).filter(name => name !== "doc" && name !== "text"),
        attributes: { made: { default: false, rendered: false } },
      },
    ]
  },
})

// A node and marks that set the fields of a spec that only an editor reads.
const Chip = TiptapNode.create({
  name: "chip",
  group: "inline",
  inline: true,
  atom: true,
  selectable: false,
  draggable: true,
  addAttributes: () => ({ label: { default: "c" } }),
  renderText: ({ node, pos, parent, index, range }) =>
    `<${node.attrs.label} ${pos} ${parent.type.name} ${index} ${range.from}-${range.to}>`,
})
const Rule = TiptapNode.create({ name: "rule", group: "block", atom: true, selectable: true })
const Box = TiptapNode.create({
  name: "box",
  group: "block",
  content: "block+",
  parseHTML: () => [{ tag: "div" }],
  renderHTML: ({ HTMLAttributes }) => ["div", HTMLAttributes, 0],
})
const Tag = Mark.create({ name: "tag", inclusive: false })
const Free = Mark.create({ name: "free", inclusive: true })

// @tiptap/extension-color and @tiptap/extension-font-family are the text style package's.
if (Color !== textStyles.Color || FontFamily !== textStyles.FontFamily) {
  throw new Error("Color and FontFamily aren't @tiptap/extension-text-style's")
}
const styles = [BackgroundColor, Color, FontFamily, FontSize, LineHeight]

// Mention warns that renderLabel is deprecated each time it renders with it.
const warn = console.warn
console.warn = (...args) => String(args[0]).startsWith("renderLabel is deprecated") || warn(...args)
const inParagraph = (...content) => doc(paragraph(...content))

const styleHTML = [
  '<p><span style="color: red; background-color: #FF0; font-family: Georgia, serif; font-size: 12px; line-height: 1.5">x</span></p>',
  `<p><span style="font-family: 'Comic Sans', &quot;A B&quot;; background-color: 'red'; color: &quot;blue&quot;">q</span></p>`,
  '<p><span style="FONT-SIZE: 9PX; Line-Height: 2">caps</span><span style="font-size: 1em; font-size: 2em">last</span></p>',
  '<p><span style="">empty</span><span style="font-size">no colon</span><span style="font-size: ">no value</span></p>',
  '<p><span style="font-size: 3px !important; line-height: normal">important</span></p>',
  '<p><span style="color: red"><span style="font-size: 4px">nested</span>out</span></p>',
  '<p><span style="font-family: a"><b><span style="line-height: 3">deeper</span></b></span></p>',
  '<h1 style="color: blue; font-size: 5px">heading</h1><p style="font-size: 6px; color: green">para</p>',
  "<p><span>no style</span><span class=x>class</span></p>",
  '<p><span style="background: blue; font: 12px serif">shorthands</span></p>',
]
const styleDocs = [
  ...["red", "", null, 0, 1, "'a'", { an: "object" }].map(value =>
    inParagraph(
      text("x", {
        type: "textStyle",
        attrs: { backgroundColor: value, color: value, fontFamily: value, fontSize: value, lineHeight: value },
      }),
    ),
  ),
  inParagraph(text("x", "textStyle")),
  inParagraph(text("x", { type: "textStyle", attrs: { fontSize: "1px" } }, "bold")),
  doc(
    { type: "heading", attrs: { level: 1, color: "red" }, content: [text("h")] },
    { type: "paragraph", attrs: { fontSize: "2px" }, content: [text("p")] },
  ),
]

const alignHTML = [
  '<p style="text-align: center">c</p><p style="text-align: RIGHT">r</p><h2 style="text-align: justify">j</h2>',
  '<p style="text-align: start">s</p><p style="text-align: left !important">i</p><p align="center">attr</p>',
  '<p style="text-align: ">e</p><p style="text-align: bogus">b</p><p style="TEXT-ALIGN: left; text-align: right">l</p>',
  '<p style="font-size: 2px; text-align: left">both</p><p style="text-align: inherit">inherit</p>',
  '<h1 style="text-align:center">h</h1><p>none</p><span style="text-align: right">loose</span>',
]
const alignDocs = [
  doc(
    ...["center", "", null, 0, 1, "bogus", { an: "object" }, [1, 2]].map(textAlign => ({
      type: "paragraph",
      attrs: { textAlign },
      content: [text("x")],
    })),
  ),
  doc(
    { type: "heading", attrs: { level: 3, textAlign: "right" }, content: [text("h")] },
    { type: "paragraph", attrs: { textAlign: "left", fontSize: "3px" } },
    { type: "paragraph" },
  ),
]

// Each href in a link of its own.
const hrefs = [
  "https://x.com/a?b=c&d#e",
  "javascript:alert(1)",
  "JavaScript:x",
  " javascript:x",
  "java&#x09;script:x",
  "jav&#x0A;ascript:x",
  "&#1;javascript:x",
  "&#xA0;javascript:x",
  "&#x2028;javascript:x",
  "data:text/html,x",
  "vbscript:x",
  "/relative",
  "#fragment",
  "?query",
  "./dot",
  "x",
  "1:x",
  "-x:y",
  "a.b:c",
  "mailto:a@b.c",
  "TEL:1",
  "callto:x",
  "sms:1",
  "cid:x",
  "xmpp:x",
  "ftps://x",
  "ftp://x",
  "git://x",
  "web+x:y",
  "web+y:z",
  "a|b:c",
  "b:c",
  "c.d:x",
  "https:1",
  "https:0",
  "https://rejected.example",
  "",
]
// Links with a class, without one and with an empty one, where the default class an option
// gives goes.
const classHTML = [
  '<p><a href="https://x" target="_self" rel="nofollow" class="c d" title="T">all</a></p>',
  '<p><a href="x" target="" rel="" class="" title="">empties</a></p>',
  '<p><a href="x"><b>bold</b> and <span style="color: red">red</span></a></p>',
]
const linkHTML = [
  `<p>${hrefs.map(href => `<a href="${href}">${href || "empty"}</a>`).join(" ")}</p>`,
  ...hrefs.map(href => `<p><a href="${href}">a</a></p>`),
  "<p><a>no href</a><a href>bare</a><a name=x>named</a></p>",
  ...classHTML,
  '<p><a href="x" target="1" rel="true" class="-2.5" title="false">strings</a></p>',
  '<p><a href="1">a<a href="2">b</a>c</a></p>',
  '<p><a href="x" HREF="y" href="z">duplicates</a></p>',
  '<h1><a href="x">heading</a></h1><a href="loose">loose</a>',
]
const linkMarkdown = [
  "[text](https://x.com)",
  '[t](u "title") [t](u \'single\') [t](u (parens)) [t](u "")',
  '[t](<u v> "t") [t]() [](u) [t](javascript:alert(1))',
  "<https://auto.link> <a@b.c> <ftp://x.y>",
  "https://bare.url www.example.com a@b.com ftp://x.y http://a.b/c?d=e&f",
  "[**bold** link](u) **[link](u)** *[a](u) b*",
  '[a][ref] [ref] [Ref][]\n\n[ref]: https://r "R"',
  "[![i](a.png)](u) [a\\]b](c\\)d) [a](b c)",
  "[a](u)[b](v) [c](u)",
  "[`code`](u) [a  \nb](u)",
  "[x](https://a.b/%20c?d=%3C) [y](<a b>)",
]
const linked = attrs => inParagraph(text("x", { type: "link", ...(attrs === undefined ? {} : { attrs }) }))
const classDocs = [
  linked({ href: "u" }),
  linked({ href: "u", target: null, rel: "me", class: "a b", title: "t" }),
  linked({ href: "u", target: 0, class: 7 }),
  linked(),
]
const linkDocs = [
  ...["u", "javascript:x", "", null, 5, true, {}, [], ["a"]].map(href => linked({ href })),
  ...["t", "", null, 0, 1, 'a"b'].map(title => linked({ href: "u", title })),
  ...classDocs,
  inParagraph(
    text("a", { type: "link", attrs: { href: "u" } }),
    text("b", "bold", { type: "link", attrs: { href: "u" } }),
    text("c", { type: "link", attrs: { href: "v" } }),
    text(" d"),
  ),
  inParagraph(text("*a* [b]", { type: "link", attrs: { href: "(u)", title: "T" } })),
]

const mentionHTML = [
  '<p><span data-type="mention" data-id="1" data-label="Ann" data-mention-suggestion-char="#">ignored</span></p>',
  '<p><span data-type="mention">bare</span> <span data-type="mention" data-id="" data-label="">empty</span></p>',
  '<p><span data-type="Mention" data-id="x">case</span><span data-type="mentions">other</span><span data-type=mention data-id=u>unquoted</span></p>',
  '<p><span data-type="mention" data-id="a" style="color: red">styled</span><b><span data-type="mention" data-id="b">bold</span></b></p>',
  '<p><span data-type="mention" data-id="1.5" data-label="true" data-mention-suggestion-char="+" class="c">x</span></p>',
  '<div data-type="mention" data-id="d">div</div><span data-type="mention" data-id="loose"></span>',
  '<p><span data-type="mention" data-id="o"><span data-type="mention" data-id="i">in</span></span></p>',
]
const mentionMarkdown = [
  '[@ id="madonna" label="Madonna"]',
  "hi [@ id=\"a\"] and [@ id='b' label='B' char='#'] there",
  '[@] [@ ] [@id="x"] [@ id=x] [@ id="a" id="b"] [@  id = "c" ]',
  '[@ __proto__="p" constructor="c" char="@"] [@ mentionSuggestionChar="%" char="&"]',
  '[@ id="a" and [@ id="a]b"] [@ id="unclosed"',
  '**[@ id="b"]** *a [@ id="c"] b* [@ id="x"](u) \\[@ id="e"] `[@ id="f"]`',
  '[@ id="x"]\n[@ id="y"]\n\n[@ label="L" id="I" char="#" extra="e"]',
  '[[@ id="x"]](u) [@ id="x" [@ id="y"]',
  '[@ id=\'one"quote\'] [@ id="two\'quote"] [@ id=""]',
]
const mention = attrs => ({ type: "mention", ...(attrs === undefined ? {} : { attrs }) })
// Mentions of each attribute, and one in bold text.
const mentioned = [
  inParagraph(
    ...[
      undefined,
      { id: "a" },
      { id: "a", label: "L" },
      { id: null, label: null },
      { label: "L" },
      { id: "h", mentionSuggestionChar: "#" },
      { id: "p", mentionSuggestionChar: "+" },
      { id: "q", mentionSuggestionChar: "++" },
      { id: "n", mentionSuggestionChar: null },
      { id: 5, label: { an: "object" } },
      { id: [1, [2, 3]], label: "" },
      { id: "", label: 0 },
      { id: 'a"b', label: "<l>" },
    ].map(mention),
  ),
  inParagraph(text("a "), { type: "mention", attrs: { id: "m" }, marks: [{ type: "bold" }] }, text(" b", "bold")),
]
const mentionDocs = [
  ...mentioned,
  // What the Markdown makes of a mention whose attributes or content JSON makes anything.
  ...["abc", 5, [1], null, {}, true].map(attrs => inParagraph({ type: "mention", attrs })),
  ...["xy", [null], [text("t"), 5], { length: 1 }, { length: "0" }, 5, ""].map(content =>
    inParagraph({ type: "mention", attrs: { id: "c" }, content }),
  ),
]

const idHTML = [
  '<p data-id="a">x</p><h1 data-id="1">h</h1><p data-id="">e</p><p>none</p><p id="plain">i</p>',
  '<p data-uid="u" data-id="d">both</p><img src="a.png" data-id="img"><p DATA-ID="caps">c</p>',
]
const idDocs = [
  doc(
    { type: "paragraph", attrs: { id: "a" }, content: [text("x")] },
    { type: "paragraph", attrs: { id: "" } },
    { type: "paragraph", attrs: { id: 0 } },
    { type: "paragraph", attrs: { id: 5 } },
    { type: "paragraph" },
    { type: "heading", attrs: { level: 2, id: { an: "object" } }, content: [text("h")] },
    { type: "heading", attrs: { level: 3 } },
    { type: "image", attrs: { src: "a.png" } },
    { type: "paragraph", attrs: { uid: "u" } },
  ),
  // Content that isn't valid, which an attribute step fails on in the node it sets or its parent.
  doc({ type: "heading", attrs: { level: 1, id: "h", uid: "u" }, content: [{ type: "paragraph", attrs: { id: "p", uid: "u" } }] }),
  doc({ type: "heading", attrs: { level: 1 }, content: [{ type: "paragraph", attrs: { id: "p", uid: "u" } }] }),
  doc({ type: "heading", attrs: { level: 1, id: "h" }, content: [{ type: "paragraph" }] }),
  doc({ type: "paragraph" }, text("loose")),
  doc({ type: "paragraph", attrs: { id: "p", uid: "u" } }, { type: "mention" }),
]
// The documents tiptap.json serializes to HTML with Tiptap's own extensions.
const tiptapDocs = unique(
  fixture("tiptap")
    .serializesHTML.filter(record => record.extensions === "own" && record.html !== undefined)
    .map(record => JSON.stringify(record.doc)),
).map(text => JSON.parse(text))
// The documents UniqueID gives ids: of each kind of attribute, those of Tiptap's own that hold a
// heading among paragraphs, and one of a type the schema hasn't.
const idsDocs = [
  ...idDocs,
  ...tiptapDocs.filter(doc => JSON.stringify(doc).includes('"type":"heading"')),
  doc({ type: "paragraph", attrs: { id: "x" } }, { type: "unknown" }),
]

// Nesting one to three deep, which tests/recorded/inline.rs folds into the conversions of any
// depth.
const depths = [1, 2, 3]
const levels = depth => Array.from({ length: depth }, (_, index) => index + 1)
const alignment = level => ["left", "center", "right"][level % 3]
const deepHTML = depths.flatMap(depth => [
  `<p><a href="u">${levels(depth)
    .map(level => `<span style="font-size: ${level}px">`)
    .join("")}x</a></p>`,
  levels(depth)
    .map(level => `<div data-id="b${level}" style="text-align: ${alignment(level)}">`)
    .join("") + "<p>x</p>",
])
// A paragraph of text and a mention inside boxes `depth` deep, with the attributes `attrs` gives
// each level.
const boxed = (depth, attrs) => {
  let node = paragraph(text("x"), mention({ id: "m" }))
  for (const level of levels(depth).reverse()) node = { type: "box", ...(attrs ? { attrs: attrs(level) } : {}), content: [node] }
  return doc(node)
}
const linkedDeep = depth => {
  let node = text("x")
  for (let level = 0; level < depth; level++) node = { type: "link", attrs: { href: "u" }, content: [node] }
  return doc(node)
}
const delimited = (open, close, depth) => open.repeat(depth) + "x" + close.repeat(depth)
const deepMarkdown = depths.map(depth => "[" + delimited("**a ", " a**", depth) + "](u)")
const deepDocs = depths.flatMap(depth => [
  boxed(depth, level => ({ id: `b${level}`, textAlign: alignment(level) })),
  linkedDeep(depth),
])
const boxes = depths.map(depth => boxed(depth))

// generateText with each of its options.
const textOptions = [{}, { blockSeparator: "\n" }, { blockSeparator: "" }, { serializers: "places" }]
const withTextOptions = docs => docs.flatMap(doc => textOptions.map(options => ({ doc, ...options })))
const ownTextDocs = [
  doc(
    paragraph(text("a"), { type: "hardBreak" }, text("b")),
    { type: "paragraph" },
    { type: "heading", attrs: { level: 2 }, content: [text("c", "bold")] },
    { type: "image", attrs: { src: "x.png" } },
    paragraph(text("d")),
  ),
  doc({ type: "unknown" }),
  inParagraph(text("")),
]
const specsTextDocs = [
  doc(
    paragraph({ type: "chip" }, text("a"), { type: "chip", attrs: { label: 5 } }),
    { type: "rule" },
    paragraph(text("b"), { type: "chip", attrs: { label: null } }),
  ),
]
const globalsHTML = [
  "<p>plain</p>",
  '<p every="x" node="1" named="true" made="1.5">a<strong every="" mark="false" named="-2">b</strong></p>',
  '<h2 every node="+.5" named="1e3">x</h2><p named="0x1"><br every="y" node="2."></p>',
]
const globalsDocs = [
  doc(
    { type: "paragraph", attrs: { every: null, node: 0, named: "", made: "not rendered" }, content: [] },
    { type: "heading", attrs: { level: 1, every: { an: "object" }, node: [1, 2] } },
  ),
  inParagraph(text("x", { type: "bold", attrs: { every: 1, mark: "m", named: true } })),
]

const typeAndPosition = ({ node, pos }) => `${node.type.name}-${pos}`
// Mention with an option that reaches its renderHTML and renderText.
const mentionWith = options => ({
  extensions: [...own, Mention.configure(options)],
  serializesHTML: mentionDocs,
  generatesText: withTextOptions(mentioned),
})
// UniqueID, whose ids reach the parse rules and renderHTML of its types.
const uniqueIds = (extensions, docs = []) => ({
  extensions,
  parsesHTML: idHTML,
  serializesHTML: withParsed(idDocs),
  generatesUniqueIds: [...idsDocs, ...docs],
})

record("tiptap-inline", {
  own: { extensions: own, generatesText: [...tiptapDocs.map(doc => ({ doc })), ...withTextOptions(ownTextDocs)] },
  globals: {
    extensions: [Document, Paragraph, Text, Heading, HardBreak, Bold, Italic, Globals],
    parsesHTML: globalsHTML,
    serializesHTML: withParsed(globalsDocs),
  },
  specs: { extensions: [Document, Paragraph, Text, Chip, Rule, Tag, Free], generatesText: withTextOptions(specsTextDocs) },
  styles: {
    extensions: [...own, ...styles],
    parsesHTML: styleHTML,
    serializesHTML: withParsed(styleDocs),
  },
  // The kit's extensions are those of `styles`.
  kit: { extensions: [Document, Paragraph, Text, Heading, Bold, TextStyleKit] },
  kitOptions: {
    extensions: [
      Document,
      Paragraph,
      Text,
      Heading,
      Bold,
      TextStyleKit.configure({
        backgroundColor: false,
        lineHeight: false,
        color: { types: ["textStyle", "heading"] },
        fontSize: { types: ["paragraph"] },
        textStyle: {
          HTMLAttributes: { class: "styled", style: "font-weight: bold; color: black", "data-x": null, "data-n": 0 },
          mergeNestedSpanStyles: false,
        },
      }),
    ],
    parsesHTML: styleHTML,
    serializesHTML: withParsed(styleDocs),
  },
  textAlignNone: { extensions: [...own, TextAlign], parsesHTML: alignHTML, serializesHTML: withParsed(alignDocs) },
  textAlign: {
    extensions: [...own, TextAlign.configure({ types: ["heading", "paragraph"] })],
    parsesHTML: alignHTML,
    serializesHTML: withParsed(alignDocs),
  },
  // An alignment's style joins a font size's, which goes first.
  textAlignOptions: {
    extensions: [
      ...own,
      FontSize.configure({ types: ["paragraph"] }),
      TextAlign.configure({ types: ["paragraph", "missing"], alignments: ["left", "right", "start"], defaultAlignment: "left" }),
    ],
    parsesHTML: alignHTML,
    serializesHTML: withParsed(alignDocs),
  },
  link: {
    extensions: [...own, Link],
    parsesMarkdown: linkMarkdown,
    parsesHTML: linkHTML,
    serializesMarkdown: withParsed(linkDocs),
    serializesHTML: withParsed(linkDocs),
  },
  // Link's options reach its parse rule and renderHTML.
  linkOptions: {
    extensions: [
      ...own,
      Link.configure({
        autolink: false,
        protocols: ["ftp", "git", { scheme: "web+x", optionalSlashes: true }, { scheme: "" }, "", "a|b", "C.D"],
        HTMLAttributes: { target: null, rel: "noopener", class: "link", "data-x": "y" },
      }),
    ],
    parsesHTML: linkHTML,
    serializesHTML: withParsed(linkDocs),
  },
  linkAllowed: {
    extensions: [
      ...own,
      Link.configure({
        defaultProtocol: "https",
        protocols: ["web+y"],
        isAllowedUri: (url, ctx) =>
          (typeof url === "string" && url.startsWith("./")) ||
          url === `${ctx.defaultProtocol}:${ctx.protocols.length}` ||
          (url !== "https://rejected.example" && ctx.defaultValidate(url)),
      }),
    ],
    parsesHTML: linkHTML,
    serializesHTML: withParsed(linkDocs),
  },
  // A class that isn't a string, which mergeAttributes can't split.
  linkClass: {
    extensions: [...own, Link.configure({ HTMLAttributes: { class: 5 } })],
    parsesHTML: classHTML,
    serializesHTML: withParsed(classDocs),
  },
  mention: {
    extensions: [...own, Mention],
    parsesMarkdown: mentionMarkdown,
    parsesHTML: mentionHTML,
    serializesMarkdown: withParsed(mentionDocs),
    serializesHTML: withParsed(mentionDocs),
    generatesText: withTextOptions(mentioned),
  },
  mentionOptions: mentionWith({
    HTMLAttributes: { class: "mention", style: "color: red", "data-x": 1 },
    suggestions: [{ char: "#" }, { char: "@" }, {}, { char: "++" }],
  }),
  mentionSuggestion: mentionWith({ suggestion: { char: "+" } }),
  mentionRender: mentionWith({
    renderText: ({ node, suggestion }) => `${suggestion.char}text:${node.attrs.id}`,
    renderHTML: ({ options, node, suggestion }) => ["b", options.HTMLAttributes, `${suggestion.char}${node.attrs.label}`],
  }),
  // A renderHTML that gives a string reaches only the HTML.
  mentionString: {
    extensions: [...own, Mention.configure({ renderHTML: ({ node }) => `<${node.attrs.id}>` })],
    serializesHTML: mentionDocs,
  },
  mentionLabel: mentionWith({
    HTMLAttributes: { class: "l" },
    renderLabel: ({ node, suggestion }) => `${suggestion.char}${node.attrs.label}!`,
  }),
  uniqueIdNone: uniqueIds([...own, UniqueID]),
  uniqueId: uniqueIds([...own, UniqueID.configure({ types: ["paragraph", "heading"], generateID: typeAndPosition })]),
  // UniqueID's id and Mention's own, of the same name.
  uniqueIdAll: {
    ...uniqueIds(
      [...own, Mention, UniqueID.configure({ types: "all", generateID: ({ node, pos }) => (node.isInline ? pos : { at: pos }) })],
      mentioned,
    ),
    parsesHTML: [...idHTML, ...mentionHTML],
    serializesHTML: withParsed([...idDocs, ...mentioned]),
  },
  uniqueIdName: uniqueIds([...own, UniqueID.configure({ types: ["paragraph"], attributeName: "uid", generateID: () => "" })]),
  // Blocks that nest, with an alignment and an id, and spans, links and mentions inside them.
  deep: {
    extensions: [
      Document,
      Paragraph,
      Text,
      Bold,
      Box,
      TextStyle,
      FontSize,
      Link,
      Mention,
      TextAlign.configure({ types: ["box", "paragraph"] }),
      UniqueID.configure({ types: ["box"], generateID: typeAndPosition }),
    ],
    parsesMarkdown: deepMarkdown,
    parsesHTML: deepHTML,
    serializesMarkdown: withParsed(deepDocs),
    serializesHTML: withParsed(deepDocs),
    generatesText: withTextOptions(boxes),
    generatesUniqueIds: boxes,
  },
})
