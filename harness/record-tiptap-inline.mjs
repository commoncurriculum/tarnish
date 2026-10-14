// Records what Tiptap's inline extensions, and the extensions that give other types attributes,
// make of documents, to fixtures/tiptap-inline.json, for tarnish-tiptap's tests/inline.rs to
// expect. Each record names the configuration of extensions it's of in `extensions`:
// - `schemas` records each configuration's node and mark types as ProseMirror reads their specs.
// - `generatesText` records the text `generateText` makes of documents, with a `blockSeparator`
//   and the text serializers it names in `serializers`, where the record has them.
// - `parsesMarkdown`, `serializesMarkdown`, `parsesHTML` and `serializesHTML` record what
//   harness/record-tiptap.mjs records, of each configuration.
import { readFileSync } from "node:fs"
import { Extension, Mark, Node as TiptapNode, generateText, getSchema, splitExtensions } from "@tiptap/core"
import { Bold } from "@tiptap/extension-bold"
import { Document } from "@tiptap/extension-document"
import { HardBreak } from "@tiptap/extension-hard-break"
import { Heading } from "@tiptap/extension-heading"
import { Highlight } from "@tiptap/extension-highlight"
import { Image } from "@tiptap/extension-image"
import { Italic } from "@tiptap/extension-italic"
import { Link } from "@tiptap/extension-link"
import { Paragraph } from "@tiptap/extension-paragraph"
import { Strike } from "@tiptap/extension-strike"
import { Subscript } from "@tiptap/extension-subscript"
import { Superscript } from "@tiptap/extension-superscript"
import { Text } from "@tiptap/extension-text"
import { Color } from "@tiptap/extension-color"
import { FontFamily } from "@tiptap/extension-font-family"
import * as textStyles from "@tiptap/extension-text-style"
import { BackgroundColor, FontSize, LineHeight, TextStyle, TextStyleKit } from "@tiptap/extension-text-style"
import { TextAlign } from "@tiptap/extension-text-align"
import { Underline } from "@tiptap/extension-underline"
import { MarkdownManager } from "@tiptap/markdown"
import { DOMParser, DOMSerializer, Node } from "@tiptap/pm/model"
import { parseHTML } from "linkedom"
import { Marked } from "marked"
import moreLists from "marked-more-lists"
import { outcome, writeFixture } from "./fixture.mjs"

// Tiptap's own extensions, as harness/record-tiptap.mjs gives them.
const own = [
  Document,
  Paragraph,
  Text,
  Heading,
  Image,
  HardBreak,
  Bold,
  Italic,
  Strike,
  Underline,
  Subscript,
  Superscript,
  Highlight,
  TextStyle,
]

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
const Tag = Mark.create({ name: "tag", inclusive: false })
const Free = Mark.create({ name: "free", inclusive: true })

// @tiptap/extension-color and @tiptap/extension-font-family are the text style package's.
if (Color !== textStyles.Color || FontFamily !== textStyles.FontFamily) {
  throw new Error("Color and FontFamily aren't @tiptap/extension-text-style's")
}
const styles = [BackgroundColor, Color, FontFamily, FontSize, LineHeight]

const configurations = {
  own,
  globals: [Document, Paragraph, Text, Heading, HardBreak, Bold, Italic, Globals],
  specs: [Document, Paragraph, Text, Chip, Rule, Tag, Free],
  styles: [...own, ...styles],
  kit: [Document, Paragraph, Text, Heading, Bold, TextStyleKit],
  kitOptions: [
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
  textAlignNone: [...own, TextAlign],
  textAlign: [...own, TextAlign.configure({ types: ["heading", "paragraph"] })],
  // An alignment's style joins a font size's, which goes first.
  textAlignOptions: [
    ...own,
    FontSize.configure({ types: ["paragraph"] }),
    TextAlign.configure({
      types: ["paragraph", "missing"],
      alignments: ["left", "right", "start"],
      defaultAlignment: "left",
    }),
  ],
  link: [...own, Link],
  linkOptions: [
    ...own,
    Link.configure({
      autolink: false,
      protocols: ["ftp", "git", { scheme: "web+x", optionalSlashes: true }, { scheme: "" }, "", "a|b", "C.D"],
      HTMLAttributes: { target: null, rel: "noopener", class: "link", "data-x": "y" },
    }),
  ],
  linkAllowed: [
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
  // A class that isn't a string, which mergeAttributes can't split.
  linkClass: [...own, Link.configure({ HTMLAttributes: { class: 5 } })],
}

const converters = Object.fromEntries(
  Object.entries(configurations).map(([name, extensions]) => {
    const marked = new Marked()
    marked.use(moreLists())
    const schema = getSchema(extensions)
    const parser = DOMParser.fromSchema(schema)
    const serializer = DOMSerializer.fromSchema(schema)
    return [name, { extensions, manager: new MarkdownManager({ extensions, marked }), schema, parser, serializer }]
  }),
)
const { document } = parseHTML("<!DOCTYPE html><html><body></body></html>")
const fixture = name => JSON.parse(readFileSync(new URL(`../fixtures/${name}.json`, import.meta.url), "utf8"))
const unique = values => [...new Set(values)]
// The inputs of each configuration in `inputs`, each once.
const each = inputs => Object.entries(inputs).flatMap(([name, list]) => unique(list).map(input => [name, input]))
const paragraph = (...content) => ({ type: "doc", content: [{ type: "paragraph", content }] })

const schemas = Object.entries(converters).map(([name, { schema }]) => ({
  extensions: name,
  nodes: Object.values(schema.nodes).map(type => [
    type.name,
    {
      atom: type.isAtom,
      inline: type.isInline,
      selectable: type.spec.selectable !== false,
      draggable: !!type.spec.draggable,
      groups: type.groups,
      attrs: type.defaultAttrs,
    },
  ]),
  marks: Object.values(schema.marks).map(type => [
    type.name,
    { inclusive: type.spec.inclusive !== false, attrs: type.instance ? type.instance.attrs : null },
  ]),
}))

// The inputs html5ever builds another tree from than the fork's parse5 does, which tarnish-html's
// tests/recorded.rs holds to html5ever's tree, as harness/record-tiptap.mjs leaves them out.
const otherTrees = new Set([
  "<select><option>a<b>x</b></option></select>",
  "<select><option>a<p>b</p></select>",
  "<p><span>a<isindex>b</span>c</p>",
  "<math><mi><![CDATA[x<y]]></mi></math>",
  "<math><mi><![CDATA[x]]></mi></math>",
  "<svg><desc><![CDATA[x]]></desc></svg>",
  "<svg><foreignObject><![CDATA[x]]></foreignObject></svg>",
  "<math><annotation-xml encoding=text/html><![CDATA[x]]></annotation-xml></math>",
])
// fixtures/dom.json's HTML that `pattern` matches.
const domHTML = pattern =>
  fixture("dom")
    .parses.map(record => record.html)
    .filter(html => !otherTrees.has(html) && pattern.test(html))
// fixtures/marked.json's Markdown that `pattern` matches.
const markedMarkdown = pattern =>
  fixture("marked")
    .lexed.map(record => record.input)
    .filter(markdown => pattern.test(markdown))

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
    paragraph({
      type: "text",
      text: "x",
      marks: [
        {
          type: "textStyle",
          attrs: { backgroundColor: value, color: value, fontFamily: value, fontSize: value, lineHeight: value },
        },
      ],
    }),
  ),
  paragraph({ type: "text", text: "x", marks: [{ type: "textStyle" }] }),
  paragraph({ type: "text", text: "x", marks: [{ type: "textStyle", attrs: { fontSize: "1px" } }, { type: "bold" }] }),
  {
    type: "doc",
    content: [
      { type: "heading", attrs: { level: 1, color: "red" }, content: [{ type: "text", text: "h" }] },
      { type: "paragraph", attrs: { fontSize: "2px" }, content: [{ type: "text", text: "p" }] },
    ],
  },
]

const alignHTML = [
  '<p style="text-align: center">c</p><p style="text-align: RIGHT">r</p><h2 style="text-align: justify">j</h2>',
  '<p style="text-align: start">s</p><p style="text-align: left !important">i</p><p align="center">attr</p>',
  '<p style="text-align: ">e</p><p style="text-align: bogus">b</p><p style="TEXT-ALIGN: left; text-align: right">l</p>',
  '<p style="font-size: 2px; text-align: left">both</p><p style="text-align: inherit">inherit</p>',
  '<h1 style="text-align:center">h</h1><p>none</p><span style="text-align: right">loose</span>',
]
const alignDocs = [
  {
    type: "doc",
    content: ["center", "", null, 0, 1, "bogus", { an: "object" }, [1, 2]].map(textAlign => ({
      type: "paragraph",
      attrs: { textAlign },
      content: [{ type: "text", text: "x" }],
    })),
  },
  {
    type: "doc",
    content: [
      { type: "heading", attrs: { level: 3, textAlign: "right" }, content: [{ type: "text", text: "h" }] },
      { type: "paragraph", attrs: { textAlign: "left", fontSize: "3px" } },
      { type: "paragraph" },
    ],
  },
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
const linkHTML = [
  `<p>${hrefs.map(href => `<a href="${href}">${href || "empty"}</a>`).join(" ")}</p>`,
  ...hrefs.map(href => `<p><a href="${href}">a</a></p>`),
  "<p><a>no href</a><a href>bare</a><a name=x>named</a></p>",
  '<p><a href="https://x" target="_self" rel="nofollow" class="c d" title="T">all</a></p>',
  '<p><a href="x" target="1" rel="true" class="-2.5" title="false">strings</a></p>',
  '<p><a href="x" target="" rel="" class="" title="">empties</a></p>',
  '<p><a href="x"><b>bold</b> and <span style="color: red">red</span></a></p>',
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
const linkDocs = [
  ...["u", "javascript:x", "", null, 5, true, {}, [], ["a"]].map(href =>
    paragraph({ type: "text", text: "x", marks: [{ type: "link", attrs: { href } }] }),
  ),
  ...["t", "", null, 0, 1, 'a"b'].map(title =>
    paragraph({ type: "text", text: "x", marks: [{ type: "link", attrs: { href: "u", title } }] }),
  ),
  paragraph({
    type: "text",
    text: "x",
    marks: [{ type: "link", attrs: { href: "u", target: null, rel: "me", class: "a b", title: "t" } }],
  }),
  paragraph({ type: "text", text: "x", marks: [{ type: "link", attrs: { href: "u", target: 0, class: 7 } }] }),
  paragraph({ type: "text", text: "x", marks: [{ type: "link" }] }),
  paragraph(
    { type: "text", text: "a", marks: [{ type: "link", attrs: { href: "u" } }] },
    { type: "text", text: "b", marks: [{ type: "bold" }, { type: "link", attrs: { href: "u" } }] },
    { type: "text", text: "c", marks: [{ type: "link", attrs: { href: "v" } }] },
    { type: "text", text: " d" },
  ),
  paragraph({ type: "text", text: "*a* [b]", marks: [{ type: "link", attrs: { href: "(u)", title: "T" } }] }),
]

const parsesMarkdown = each({
  globals: ["plain **bold**"],
  styles: ['<span style="color: red">x</span>', "plain *em*"],
  link: [...linkMarkdown, ...markedMarkdown(/\[|<|https?:|www\.|@|ftp:/)],
  linkOptions: linkMarkdown,
}).map(([name, markdown]) => ({
  extensions: name,
  markdown,
  ...outcome(() => ({ doc: converters[name].manager.parse(markdown) })),
}))

const parsesHTML = each({
  globals: [
    "<p>plain</p>",
    '<p every="x" node="1" named="true" made="1.5">a<strong every="" mark="false" named="-2">b</strong></p>',
    '<h2 every node="+.5" named="1e3">x</h2><p named="0x1"><br every="y" node="2."></p>',
  ],
  styles: [...styleHTML, ...domHTML(/style|<span/i)],
  kit: styleHTML,
  kitOptions: styleHTML,
  textAlignNone: alignHTML,
  textAlign: [...alignHTML, ...domHTML(/style|align/i)],
  textAlignOptions: alignHTML,
  link: [...linkHTML, ...domHTML(/<a\b/i)],
  linkOptions: linkHTML,
  linkAllowed: linkHTML,
  linkClass: linkHTML.slice(0, 3),
}).map(([name, html]) => ({
  extensions: name,
  html,
  ...outcome(() => {
    const { document } = parseHTML(`<!DOCTYPE html><html><body>${html}</body></html>`)
    return { doc: converters[name].parser.parse(document.body).toJSON() }
  }),
}))

// The documents the parses of each configuration make, each read from its JSON text, as the tests
// read it, and those `docs` gives.
const documents = docs =>
  Object.keys(converters).flatMap(name => {
    const parsed = [...parsesMarkdown, ...parsesHTML]
      .filter(record => record.extensions === name && record.doc)
      .map(record => record.doc)
    return unique([...(docs[name] ?? []), ...parsed].map(doc => JSON.stringify(doc))).map(text => [
      name,
      JSON.parse(text),
    ])
  })

const docs = {
  globals: [
    {
      type: "doc",
      content: [
        { type: "paragraph", attrs: { every: null, node: 0, named: "", made: "not rendered" }, content: [] },
        { type: "heading", attrs: { level: 1, every: { an: "object" }, node: [1, 2] } },
      ],
    },
    paragraph({ type: "text", text: "x", marks: [{ type: "bold", attrs: { every: 1, mark: "m", named: true } }] }),
  ],
  styles: styleDocs,
  kit: styleDocs,
  kitOptions: styleDocs,
  textAlignNone: alignDocs,
  textAlign: alignDocs,
  textAlignOptions: alignDocs,
  link: linkDocs,
  linkOptions: linkDocs,
  linkAllowed: linkDocs,
  linkClass: linkDocs.slice(0, 3),
}

const serializesMarkdown = documents(docs).map(([name, doc]) => ({
  extensions: name,
  doc,
  ...outcome(() => ({ markdown: converters[name].manager.serialize(doc) })),
}))

const serializesHTML = documents(docs).map(([name, doc]) => ({
  extensions: name,
  doc,
  ...outcome(() => {
    const { schema, serializer } = converters[name]
    const container = document.createElement("div")
    serializer.serializeFragment(Node.fromJSON(schema, doc).content, { document }, container)
    return { html: container.innerHTML }
  }),
}))

// Text serializers that name the nodes they're given and where.
const serializers = {
  places: {
    paragraph: ({ node, pos, index }) => `[${node.type.name} ${pos} ${index}]`,
    chip: ({ node }) => `(${node.attrs.label})`,
  },
}
const textDocs = {
  own: [
    {
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [{ type: "text", text: "a" }, { type: "hardBreak" }, { type: "text", text: "b" }],
        },
        { type: "paragraph" },
        { type: "heading", attrs: { level: 2 }, content: [{ type: "text", text: "c", marks: [{ type: "bold" }] }] },
        { type: "image", attrs: { src: "x.png" } },
        { type: "paragraph", content: [{ type: "text", text: "d" }] },
      ],
    },
    { type: "doc", content: [{ type: "unknown" }] },
    { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: "" }] }] },
  ],
  specs: [
    {
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [{ type: "chip" }, { type: "text", text: "a" }, { type: "chip", attrs: { label: 5 } }],
        },
        { type: "rule" },
        {
          type: "paragraph",
          content: [
            { type: "text", text: "b" },
            { type: "chip", attrs: { label: null } },
          ],
        },
      ],
    },
  ],
}
// The documents tiptap.json serializes to HTML with Tiptap's own extensions, with the default
// options, and those above with each.
const tiptapDocs = fixture("tiptap")
  .serializesHTML.filter(record => !record.extensions && record.html !== undefined)
  .map(record => JSON.stringify(record.doc))
const textOptions = [{}, { blockSeparator: "\n" }, { blockSeparator: "" }, { serializers: "places" }]
const generatesText = [
  ...unique(tiptapDocs).map(text => ["own", JSON.parse(text), {}]),
  ...Object.entries(textDocs).flatMap(([name, docs]) =>
    docs.flatMap(doc => textOptions.map(options => [name, doc, options])),
  ),
].map(([name, doc, { blockSeparator, serializers: named }]) => ({
  extensions: name,
  doc,
  ...(blockSeparator === undefined ? {} : { blockSeparator }),
  ...(named === undefined ? {} : { serializers: named }),
  ...outcome(() => ({
    text: generateText(doc, converters[name].extensions, {
      blockSeparator,
      textSerializers: named && serializers[named],
    }),
  })),
}))

writeFixture("tiptap-inline", {
  schemas,
  generatesText,
  parsesMarkdown,
  serializesMarkdown,
  parsesHTML,
  serializesHTML,
})
