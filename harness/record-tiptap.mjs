// Records what @tiptap/markdown's MarkdownManager and ProseMirror's DOMParser and DOMSerializer do
// with Tiptap's extensions, to fixtures/tiptap.json, for tarnish-tiptap's tests to expect:
// - Each configuration records its schema, as harness/fixture.mjs's schemaRecord writes it.
// - Each Markdown input records the document the manager parses it to: fixtures/marked.json's,
//   and Tiptap's own syntax.
// - Each document records the Markdown the manager serializes it to: the documents the parses
//   make, and some that hold text whose `text` or `marks` is something other than what the manager
//   calls a method of, so that they record V8's error for each.
// - Each HTML input records the document DOMParser parses the body of a linkedom document holding
//   it to: fixtures/dom.json's, and Tiptap's own elements and styles.
// - Each document records the innerHTML of an element DOMSerializer fills in linkedom.
// A record names the extensions it's of in `extensions`: `own`, Tiptap's own; `colors`, with
// Highlight's colors and Color's, or `code`, an application's code mark and code block, and
// a figure whose attribute renders under a name it makes. Tiptap's own extensions with an option
// set record their schemas and the HTML conversions of the inputs that hold what the option
// changes: `htmlAttributes` and `htmlAttributeValues`, each with HTMLAttributes and an
// application's attribute that renders some of the same ones; `levels` and `noLevels`, with
// Heading's levels; `multicolor`, with Highlight's; and `inline` and `base64`, with Image's.
// `renderedValues`, with an application's attributes that render classes and styles of any value,
// records its schema and the HTML its documents serialize to.
import { readFileSync } from "node:fs"
import { Extension, Mark, Node as TiptapNode, getSchema } from "@tiptap/core"
import { Bold } from "@tiptap/extension-bold"
import { Document } from "@tiptap/extension-document"
import { HardBreak } from "@tiptap/extension-hard-break"
import { Heading } from "@tiptap/extension-heading"
import { Highlight } from "@tiptap/extension-highlight"
import { Image } from "@tiptap/extension-image"
import { Italic } from "@tiptap/extension-italic"
import { Paragraph } from "@tiptap/extension-paragraph"
import { Strike } from "@tiptap/extension-strike"
import { Subscript } from "@tiptap/extension-subscript"
import { Superscript } from "@tiptap/extension-superscript"
import { Text } from "@tiptap/extension-text"
import { Color, TextStyle } from "@tiptap/extension-text-style"
import { Underline } from "@tiptap/extension-underline"
import { MarkdownManager } from "@tiptap/markdown"
import { DOMParser, DOMSerializer, Node } from "@tiptap/pm/model"
import { parseHTML } from "linkedom"
import { Marked } from "marked"
import moreLists from "marked-more-lists"
import { outcome, schemaRecord, writeFixture } from "./fixture.mjs"

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
const Code = Mark.create({
  name: "code",
  code: true,
  renderMarkdown: (node, helpers) => "`" + helpers.renderChildren(node) + "`",
})
const CodeBlock = TiptapNode.create({
  name: "codeBlock",
  code: true,
  group: "block",
  content: "text*",
  parseHTML: () => [{ tag: "pre" }],
  renderMarkdown: (node, helpers) => "```\n" + helpers.renderChildren(node) + "\n```",
})
const Figure = TiptapNode.create({
  name: "figure",
  group: "block",
  parseHTML: () => [{ tag: "figure" }],
  renderHTML: ({ HTMLAttributes }) => ["figure", HTMLAttributes],
  addAttributes: () => ({ made: { default: null, renderHTML: attributes => ({ "data-made": attributes.made }) } }),
})
// An application's attribute on each type with HTMLAttributes, which renders a class, a style and
// an attribute of the options'.
const Given = Extension.create({
  name: "given",
  addGlobalAttributes: () => [
    {
      types: [
        "paragraph",
        "heading",
        "image",
        "hardBreak",
        "bold",
        "italic",
        "strike",
        "underline",
        "subscript",
        "superscript",
        "highlight",
      ],
      attributes: {
        given: {
          default: null,
          renderHTML: ({ given }) => (given ? { class: given, style: `color: ${given}`, "data-given": given } : {}),
        },
      },
    },
  ],
})
// HTMLAttributes of strings, which the JSX of Bold leaves `children` out of, and of other values,
// each with what some extensions add or replace.
const strings = {
  class: "option more",
  style: "color: blue; font-size: 2px",
  "data-given": "option",
  "data-null": null,
  "data-undefined": undefined,
  "data-number": 1,
  children: "child",
}
const others = {
  class: 0,
  style: undefined,
  "data-given": null,
  "data-true": true,
  "data-object": {},
  "data-array": [1, "a", null],
  "data-empty": "",
}
// Tiptap's own extensions, each with `attributes` and what `extra` adds for it, Highlight with
// its colors, and Given.
const configured = (attributes, extra) => {
  const html = name => ({ HTMLAttributes: { ...attributes, ...extra[name] } })
  return [
    Document,
    Paragraph.configure(html("paragraph")),
    Text,
    Heading.configure(html("heading")),
    Image.configure(html("image")),
    HardBreak.configure(html("hardBreak")),
    Bold.configure(html("bold")),
    Italic.configure(html("italic")),
    Strike.configure(html("strike")),
    Underline.configure(html("underline")),
    Subscript.configure(html("subscript")),
    Superscript.configure(html("superscript")),
    Highlight.configure({ multicolor: true, ...html("highlight") }),
    TextStyle,
    Given,
  ]
}
// An application's two attributes on paragraphs and bold, each rendering the object a document
// gives it, so that the second's class and style join the first's whatever they hold.
const Renders = Extension.create({
  name: "renders",
  addGlobalAttributes: () => [
    {
      types: ["paragraph", "bold"],
      attributes: {
        first: { default: null, renderHTML: ({ first }) => first },
        second: { default: null, renderHTML: ({ second }) => second },
      },
    },
  ],
})
// Tiptap's own extensions, with `extension` configured.
const replaced = (extension, configured) => own.map(each => (each === extension ? configured : each))
const configurations = {
  own,
  colors: [...own.filter(extension => extension !== Highlight), Highlight.configure({ multicolor: true }), Color],
  code: [Document, Paragraph, Text, Code, CodeBlock, Figure],
  htmlAttributes: configured(strings, {
    image: { alt: "option", title: "option", width: 5 },
    highlight: { "data-color": "option" },
  }),
  htmlAttributeValues: configured(others, { subscript: { style: 5 }, superscript: { class: 5 } }),
  levels: replaced(Heading, Heading.configure({ levels: [4, 2] })),
  noLevels: replaced(Heading, Heading.configure({ levels: [] })),
  multicolor: replaced(Highlight, Highlight.configure({ multicolor: true })),
  inline: replaced(Image, Image.configure({ inline: true })),
  base64: replaced(Image, Image.configure({ allowBase64: true })),
  renderedValues: [
    ...replaced(Paragraph, Paragraph.configure({ HTMLAttributes: { class: "option", style: "color: blue" } })),
    Renders,
  ],
}
const converters = Object.fromEntries(
  Object.entries(configurations).map(([name, extensions]) => {
    const marked = new Marked()
    marked.use(moreLists())
    const schema = getSchema(extensions)
    const parser = DOMParser.fromSchema(schema)
    const serializer = DOMSerializer.fromSchema(schema)
    return [name, { manager: new MarkdownManager({ extensions, marked }), schema, parser, serializer }]
  }),
)
const { document } = parseHTML("<!DOCTYPE html><html><body></body></html>")
const fixture = name => JSON.parse(readFileSync(new URL(`../fixtures/${name}.json`, import.meta.url), "utf8"))
const named = name => ({ extensions: name })
const unique = values => [...new Set(values)]

const schemas = Object.entries(converters).map(([name, { schema }]) => ({ ...named(name), ...schemaRecord(schema) }))

// `type` inside itself, `depth` deep, around a text node.
const nested = (type, depth) => {
  let node = { type: "text", text: "x" }
  for (let level = 0; level < depth; level++) node = { type, content: [node] }
  return { type: "doc", content: [node] }
}
// Nesting one to three deep, which tests/deep.rs folds into the conversions of any depth.
const depths = [1, 2, 3]
const delimited = (open, close, depth) => open.repeat(depth) + "x" + close.repeat(depth)
const deepMarkdown = depths.flatMap(depth => [
  ">".repeat(4 * depth) + " x",
  "> ".repeat(depth) + "x\ny",
  "1. ".repeat(depth) + "x",
  ...[["**a ", " a**"], ["*a ", " a*"], ["~~a ", " a~~"]].map(
    ([open, close]) => ">".repeat(4 * depth) + " " + delimited(open, close, depth),
  ),
  "# " + delimited("*a ", " a*", depth) + "\n\ny",
])
const deepHTML = depths.flatMap(depth => ["div", "p", "b", "span style=color:red"].map(tag => `<${tag}>`.repeat(depth) + "x"))
const deepDocs = depths.flatMap(depth => ["doc", "paragraph", "bold", "italic", "strike"].map(type => nested(type, depth)))

// The configurations with an option set, each with the HTML inputs and the type of the documents it
// records: Tiptap's own hand-picked ones where `picked`. None records Markdown, since
// MarkdownManager reads the schema only for HTML, which it keeps as text without a `window`.
const optioned = {
  // HTMLAttributes reach only renderHTML, of each type the hand-picked inputs hold.
  htmlAttributes: { picked: true },
  htmlAttributeValues: { picked: true },
  // Levels reach Heading's parse rules and renderHTML; its Markdown hooks read a heading's level.
  levels: { html: /<h[1-6]/i, type: "heading" },
  noLevels: { html: /<h[1-6]/i, type: "heading" },
  // multicolor adds the color a mark parses and renders, which Highlight's Markdown hooks never read.
  multicolor: { html: /<mark/i, type: "highlight" },
  // inline makes an image inline content; Image's and Paragraph's Markdown hooks read no schema.
  inline: { html: /<img/i, type: "image" },
  // allowBase64 reaches only Image's parse rule.
  base64: { html: /<img/i, type: "image" },
}
const parsesMarkdown = [
  ...unique([
    "",
    "plain",
    "**bold** *italic* ~~strike~~ `code`",
    "***both*** **a *b* c**",
    "one  \ntwo\n\nthree",
    "&nbsp;",
    "# heading\n\n- list\n\n> quote",
    "### three **bold**\n\nSetext\n===",
    "++under++ ==high== ++==both==++",
    '![alt](a.png "title")\n\ntext ![](b.png) more',
    ...fixture("marked").lexed.map(record => record.input),
    ...deepMarkdown,
  ]).map(markdown => ["own", markdown]),
  ...["==high==", "==a **b** c==", "a ==b== ++==c==++", '<span style="color: red">x</span>'].map(markdown => [
    "colors",
    markdown,
  ]),
  ...["`*a* <b>` \\*c\\*\n\n```\n*d* <e>\n```", "`code` *em*", "```\nblock\n```"].map(markdown => ["code", markdown]),
].map(([name, markdown]) => ({
  ...named(name),
  markdown,
  ...outcome(() => ({ doc: converters[name].manager.parse(markdown) })),
}))

// The inputs html5ever builds another tree from than the fork's parse5 does, which tarnish-html's
// tests/recorded.rs holds to html5ever's tree.
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
// A span without a color, `depth` deep in elements that aren't spans inside a span with one.
const uncolored = depth =>
  `<p><span style="color: red">${"<b>".repeat(depth)}<span style="font-size: 9px">x</span>${"</b>".repeat(depth)}</span></p>`
const marks = [
  "<p><mark>plain</mark></p>",
  '<p><mark data-color="red">x</mark></p>',
  '<p><mark style="background-color: #FF0000">x</mark></p>',
  '<p><mark style="background-color: rgb(1, 2, 3); color: inherit">x</mark></p>',
  '<p><mark data-color="red" style="background-color: blue">x</mark></p>',
  '<p><mark data-color="" style="background-color: blue">x</mark></p>',
  '<p><mark style="background: yellow">x</mark></p>',
  '<p><mark style="BACKGROUND-COLOR: Blue">x</mark></p>',
  '<p><mark style="background-color: ">x</mark></p>',
  '<p><mark style="background-color: red; background-color: blue">x</mark></p>',
  '<p><mark data-color="red"><span style="color: blue">x</span></mark></p>',
]
const pickedHTML = [
  "<p>plain</p>",
  "<p><strong>b</strong><b>b</b><em>i</em><i>i</i><s>s</s><del>d</del><u>u</u><sub>x</sub><sup>y</sup><mark>h</mark></p>",
  "<p><span style='color: red'>c</span><span>none</span><br>z</p>",
  "<p><b style='font-weight: normal'>not bold</b><i style='font-style: normal'>not italic</i></p>",
  "<p><span style='font-weight: bold; font-style: italic; text-decoration: underline line-through'>styled</span></p>",
  "<h1>heading</h1><ul><li>item</li></ul>",
  "<h3>three</h3><h6>six</h6>",
  "<img src='a.png' alt='A' title='T' width='10' height='5'><img alt='no src'><img src='data:image/png;base64,AA'>",
  "loose text",
]
const ownHTML = unique([
  ...pickedHTML,
  ...fixture("dom")
    .parses.map(record => record.html)
    .filter(html => !otherTrees.has(html)),
  ...deepHTML,
])
// The HTML inputs the options change the parses of, which Tiptap's own don't record:
// harness/record-tiptap-inline.mjs reads the documents of theirs.
const optionHTML = [
  "<h1>1</h1><h2>2</h2><h3>3</h3><h4>4</h4><h5>5</h5><h6>6</h6>",
  '<p>a<img src="a.png">b</p>',
  '<p><img src="data:image/png;base64,AA" alt="x">c</p>',
  '<img src="DATA:image/png;base64,AA"><img src=" data:x"><img src="">',
  ...marks,
]
const optionedHTML = Object.entries(optioned).flatMap(([name, { picked, html }]) => {
  const inputs = picked
    ? unique([...pickedHTML, ...deepHTML, ...optionHTML])
    : unique([...ownHTML, ...optionHTML]).filter(input => html.test(input))
  return inputs.map(input => [name, input])
})
const parsesHTML = [
  ...ownHTML.map(html => ["own", html]),
  ...[
    ...marks,
    '<p><span style="color: red">x</span></p>',
    `<p><span style="color: 'red'">x</span></p>`,
    '<p><span style="color: #00F; color: ">x</span></p>',
    '<p><span style="font-weight: bold">x</span></p>',
    '<p><span style="color: red"><span>x</span></span></p>',
    '<p><span style="color: red"><span style="font-weight: bold">x</span><span style="color: blue">y</span></span></p>',
    '<p><span style="color: red">a<span style="color: blue">b<span style="color: green">c</span></span>d</span></p>',
    '<p><span style="color: red"><span style="color: blue"><span style="color: green">x</span></span></span></p>',
    // Each span gets the style of the one around it ahead of its own, through elements that aren't
    // spans as deep as 20.
    ...[0, 1, 2, 3, 19, 20, 21, 22].map(uncolored),
    `<p>${[...Array(22).keys()].map(level => `<span style="color: #${level}">`).join("")}x${"</span>".repeat(22)}</p>`,
  ].map(html => ["colors", html]),
  ...[
    "<pre>a  \n b</pre>",
    '<figure made="x"></figure>',
    '<figure made="1"></figure>',
    '<figure made="true"></figure>',
    '<figure made=""></figure>',
    "<figure></figure><p>after</p>",
  ].map(html => ["code", html]),
  ...optionedHTML,
].map(([name, html]) => ({
  ...named(name),
  html,
  ...outcome(() => {
    const { document } = parseHTML(`<!DOCTYPE html><html><body>${html}</body></html>`)
    return { doc: converters[name].parser.parse(document.body).toJSON() }
  }),
}))

// The documents the parses of each configuration make, each read from its JSON text, as the tests
// read it.
const parsed = name =>
  [...parsesMarkdown, ...parsesHTML]
    .filter(record => record.extensions === name && record.doc)
    .map(record => record.doc)
const documents = (name, docs) =>
  unique([...docs, ...parsed(name)].map(doc => JSON.stringify(doc))).map(text => [name, JSON.parse(text)])

const withMarks = (text, ...marks) => ({ type: "text", text, marks: marks.map(type => ({ type })) })
const markTypes = ["bold", "italic", "strike", "underline", "highlight", "subscript", "superscript", "textStyle"]
const docs = [
  { type: "doc", content: [{ type: "paragraph" }] },
  {
    type: "doc",
    content: [
      {
        type: "paragraph",
        content: [
          { type: "text", text: "plain " },
          ...markTypes.map(type => withMarks(type, type)),
          { type: "hardBreak" },
          withMarks("both", "bold", "italic"),
        ],
      },
      { type: "paragraph", content: [withMarks("a ", "bold"), withMarks("b", "bold", "italic"), withMarks(" c", "italic")] },
      { type: "paragraph", content: [{ type: "text", text: "*not em* <b>not bold</b> & 1. two" }] },
    ],
  },
  {
    type: "doc",
    content: [
      { type: "heading", attrs: { level: 2 }, content: [{ type: "text", text: "two" }] },
      { type: "heading", attrs: { level: "3" }, content: [withMarks("three", "bold")] },
      { type: "heading", content: [{ type: "text", text: "no level" }] },
      { type: "heading", attrs: { level: 4 } },
    ],
  },
  {
    type: "doc",
    content: [
      { type: "image", attrs: { src: "a.png", alt: "A", title: "T", width: 10 } },
      { type: "image", attrs: { src: "b.png" } },
      { type: "image" },
    ],
  },
]
const paragraph = (...content) => ({ type: "doc", content: [{ type: "paragraph", content }] })
const colors = ["red", "", null, 0, 1, "#FF0000", 'a"b']
// Documents whose rendering the options change: headings of each level and of others, images
// inline and highlights of each color, then each type Given gives its attribute to with it.
const levels = [1, 2, 3, 4, 5, 6, 0, 7, 2.5, "2", null, true]
const given = { given: "red" }
const optionDocs = [
  {
    type: "doc",
    content: levels.map(level => ({ type: "heading", attrs: { level }, content: [{ type: "text", text: "h" }] })),
  },
  paragraph({ type: "text", text: "a" }, { type: "image", attrs: { src: "a.png", alt: "A" } }, { type: "text", text: "b" }),
  ...colors.map(color => paragraph({ type: "text", text: "x", marks: [{ type: "highlight", attrs: { color } }] })),
  { type: "doc", content: [{ type: "paragraph", attrs: given, content: [{ type: "text", text: "p" }] }] },
  { type: "doc", content: [{ type: "heading", attrs: { level: 2, ...given }, content: [{ type: "text", text: "h" }] }] },
  { type: "doc", content: [{ type: "image", attrs: { src: "a.png", alt: "A", ...given } }] },
  paragraph({ type: "text", text: "a" }, { type: "hardBreak", attrs: given }, { type: "text", text: "b" }),
  ...["bold", "italic", "strike", "underline", "subscript", "superscript", "highlight"].map(type =>
    paragraph({ type: "text", text: "x", marks: [{ type, attrs: given }] }),
  ),
  paragraph({ type: "text", text: "x", marks: [{ type: "highlight", attrs: { color: "yellow", ...given } }] }),
]
const colorDocs = [
  ...colors.map(color => paragraph({ type: "text", text: "x", marks: [{ type: "highlight", attrs: { color } }] })),
  ...colors.map(color => paragraph({ type: "text", text: "x", marks: [{ type: "textStyle", attrs: { color } }] })),
  paragraph({
    type: "text",
    text: "x",
    marks: [
      { type: "highlight", attrs: { color: "yellow" } },
      { type: "textStyle", attrs: { color: "blue" } },
    ],
  }),
]
const codeDocs = [
  {
    type: "doc",
    content: [
      {
        type: "paragraph",
        content: [
          { type: "text", text: "*a* <b>", marks: [{ type: "code" }] },
          { type: "text", text: " *c*" },
        ],
      },
      { type: "codeBlock", content: [{ type: "text", text: "*d* <e>" }] },
    ],
  },
  ...["x", 1, true, "", null].map(made => ({ type: "doc", content: [{ type: "figure", attrs: { made } }] })),
]

const text = node => ({ type: "doc", content: [{ type: "paragraph", content: [{ type: "text", ...node }] }] })
const values = [1, 0, 1.5, "x", 'a"b', "", true, false, null, {}, [], [1], { some: 1 }]
const serializesMarkdown = [
  ...documents("own", [...docs, ...deepDocs]),
  ...values
    .flatMap(value => [
      text({ text: "a", marks: { some: value } }),
      text({ text: { replace: value } }),
      text({ text: "a", marks: [{ type: { localeCompare: value } }, { type: "unknown" }] }),
      text({ text: "a", marks: [{ type: "unknown" }, { type: { localeCompare: value } }] }),
    ])
    .map(doc => ["own", doc]),
  ...[text({ text: "a", marks: { other: 1 } }), text({ text: { other: 1 } }), text({ text: [1] }), text({ text: 5 })].map(
    doc => ["own", doc],
  ),
  ...documents("colors", colorDocs),
  ...documents("code", codeDocs),
].map(([name, doc]) => ({ ...named(name), doc, ...outcome(() => ({ markdown: converters[name].manager.serialize(doc) })) }))

// What Renders' two attributes render: classes and styles that aren't strings, which only HTML
// reads, joined onto strings and joined onto. Each pair renders on a paragraph, whose
// HTMLAttributes they join after, and on bold text. The styles are custom properties, which an
// element's style keeps whatever their values.
const rendered = [
  [{ class: 5 }, { class: "x" }],
  [{ class: [1, "a"] }, { class: "x" }],
  [{ class: [] }, { class: "x" }],
  [{ class: {} }, { class: "x" }],
  [{ class: true }, { class: "x" }],
  [{ class: 5 }, { class: { toString: 1 } }],
  [{ class: "a" }, { class: 5 }],
  [{ class: "a" }, { class: [1, "b", null] }],
  [{ class: "a" }, { class: {} }],
  [{ class: "a" }, { class: { toString: 1 } }],
  [{ class: "a" }, { class: 0 }],
  [{ class: 0 }, { class: "x" }],
  [{ class: 5 }, null],
  [{ style: ["--a: red"] }, { style: "--b: 1px" }],
  [{ style: "--a: red" }, { style: ["--b: 1px", 5] }],
  [{ style: ["--a: red", ";", "--b: 1px"] }, { style: "--c: 0" }],
  [{ style: ["--a: red; --b: 1px"] }, { style: ["--c: ", null, [1, [2, null]], { d: 1 }, true, 5] }],
  [{ style: ["--a: x'", ";", "--b: y"] }, { style: "--c: z" }],
  [{ style: ["--a: ", "'", ";", "'", ";", "--b: y"] }, { style: "--c: z" }],
  [{ style: ["--a: f", "(", ";", ")", ";", "--b: y"] }, { style: "--c: z" }],
  [{ style: 5 }, { style: "--b: 1px" }],
  [{ style: true }, { style: [] }],
  [{ style: {} }, { style: "--b: 1px" }],
  [{ style: { length: 2, 0: "--a: red" } }, { style: "--b: 1px" }],
  [{ style: { length: "1.5", 1: ";", 0: "--a: red" } }, { style: "--b: 1px" }],
  [{ style: "--a: red" }, { style: [{ toString: 1 }] }],
  [{ style: ["--a: red"] }, null],
]
const renderedDocs = rendered.flatMap(([first, second]) => {
  const attrs = { first, second }
  return [
    { type: "doc", content: [{ type: "paragraph", attrs, content: [{ type: "text", text: "p" }] }] },
    paragraph({ type: "text", text: "b", marks: [{ type: "bold", attrs }] }),
  ]
})

const paragraphs = depths.map(depth => nested("paragraph", depth))
// The documents of a configuration with an option set that hold its type.
const optionedDocs = ([name, { type }]) =>
  documents(name, [...docs, ...paragraphs, ...optionDocs]).filter(
    ([, doc]) => !type || JSON.stringify(doc).includes(`"type":"${type}"`),
  )
const serializesHTML = [
  ...documents("own", [...docs, ...paragraphs]),
  // `doc` has no toDOM, so the serializer has nothing to call for a doc inside the doc.
  ["own", { type: "doc", content: [{ type: "doc" }] }],
  ...documents("colors", colorDocs),
  ...documents("code", codeDocs),
  ...Object.entries(optioned).flatMap(optionedDocs),
  ...renderedDocs.map(doc => ["renderedValues", doc]),
].map(([name, doc]) => ({
  ...named(name),
  doc,
  ...outcome(() => {
    const { schema, serializer } = converters[name]
    const container = document.createElement("div")
    serializer.serializeFragment(Node.fromJSON(schema, doc).content, { document }, container)
    return { html: container.innerHTML }
  }),
}))

writeFixture("tiptap", {
  schemas,
  parsesMarkdown,
  serializesMarkdown,
  parsesHTML,
  serializesHTML,
})
