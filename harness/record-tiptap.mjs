// Records Tiptap's own extensions to fixtures/tiptap.json, as harness/tiptap.mjs records them:
// `own`, as they come; `colors`, with Highlight's colors and Color; `code`, an application's
// code mark and code block, and a figure whose attribute renders under a name it makes;
// `htmlAttributes` and `htmlAttributeValues`, each with HTMLAttributes and an application's
// attribute that renders some of the same ones; `levels` and `noLevels`, with Heading's levels;
// `multicolor`, with Highlight's; `inline` and `base64`, with Image's; `renderedValues`, with
// an application's attributes that render classes and styles of any value; `nodeType`, with
// attributes named `nodeType`, which renderSpec takes for a DOM node but where they're null; and
// `application`, with the lists and an application's blocks, one whose Markdown is its level and
// one whose whitespace is `pre`. The options reach no Markdown, since MarkdownManager reads the
// schema only for HTML, which it keeps as text without a `window`. `all` has every group's
// extensions, as the other recorders record them: it records fixtures/marked.json's and
// fixtures/dom.json's inputs, and what one group nests in another's.
import { Extension, Mark, Node as TiptapNode } from "@tiptap/core"
import { Audio } from "@tiptap/extension-audio"
import { Blockquote } from "@tiptap/extension-blockquote"
import { Bold } from "@tiptap/extension-bold"
import { Code } from "@tiptap/extension-code"
import { CodeBlock } from "@tiptap/extension-code-block"
import { Details, DetailsContent, DetailsSummary } from "@tiptap/extension-details"
import { Document } from "@tiptap/extension-document"
import { Emoji } from "@tiptap/extension-emoji"
import { HardBreak } from "@tiptap/extension-hard-break"
import { Heading } from "@tiptap/extension-heading"
import { Highlight } from "@tiptap/extension-highlight"
import { HorizontalRule } from "@tiptap/extension-horizontal-rule"
import { Image } from "@tiptap/extension-image"
import { Italic } from "@tiptap/extension-italic"
import { Link } from "@tiptap/extension-link"
import { BulletList, ListItem, OrderedList, TaskItem, TaskList } from "@tiptap/extension-list"
import { Mathematics } from "@tiptap/extension-mathematics"
import { Mention } from "@tiptap/extension-mention"
import { Paragraph } from "@tiptap/extension-paragraph"
import { RubyText } from "@tiptap/extension-ruby-text"
import { Strike } from "@tiptap/extension-strike"
import { Subscript } from "@tiptap/extension-subscript"
import { Superscript } from "@tiptap/extension-superscript"
import { Table, TableCell, TableHeader, TableRow } from "@tiptap/extension-table"
import { TableOfContents } from "@tiptap/extension-table-of-contents"
import { Text } from "@tiptap/extension-text"
import { TextAlign } from "@tiptap/extension-text-align"
import { BackgroundColor, Color, FontFamily, FontSize, LineHeight, TextStyle } from "@tiptap/extension-text-style"
import { Twitch } from "@tiptap/extension-twitch"
import { Underline } from "@tiptap/extension-underline"
import { Youtube } from "@tiptap/extension-youtube"
import {
  doc,
  domInputs,
  markedInputs,
  otherAttributes,
  own,
  paragraph,
  record,
  text,
  unique,
  withParsed,
} from "./tiptap.mjs"

const ApplicationCode = Mark.create({
  name: "code",
  code: true,
  renderMarkdown: (node, helpers) => "`" + helpers.renderChildren(node) + "`",
})
const ApplicationCodeBlock = TiptapNode.create({
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
// An application's block whose Markdown is the `level` its renderer is given, and one whose
// whitespace is `pre` though its element's isn't.
const Leveled = TiptapNode.create({
  name: "leveled",
  group: "block",
  content: "inline*",
  renderMarkdown: (node, helpers, context) => `${context.level}> ` + helpers.renderChildren(node),
})
const Preformatted = TiptapNode.create({
  name: "preformatted",
  group: "block",
  content: "text*",
  whitespace: "pre",
  parseHTML: () => [{ tag: "div[data-pre]" }],
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
// HTMLAttributes of strings, which the JSX of Bold leaves `children` out of.
const strings = {
  class: "option more",
  style: "color: blue; font-size: 2px",
  "data-given": "option",
  "data-null": null,
  "data-undefined": undefined,
  "data-number": 1,
  children: "child",
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
// A node whose attribute renders under the name `nodeType`.
const Typed = TiptapNode.create({
  name: "typed",
  group: "block",
  atom: true,
  addAttributes: () => ({ nodeType: { default: null } }),
  renderHTML: ({ HTMLAttributes }) => ["div", HTMLAttributes],
})
// HTMLAttributes holding a `nodeType`, on an element that holds content and one that doesn't, of
// a node and of a mark.
const nodeTyped = nodeType => ({ HTMLAttributes: { "data-x": "y", nodeType } })
// Tiptap's own extensions, with `extension` configured.
const replaced = (extension, configured) => own.map(each => (each === extension ? configured : each))

// `type` inside itself, `depth` deep, around a text node.
const nested = (type, depth) => {
  let node = text("x")
  for (let level = 0; level < depth; level++) node = { type, content: [node] }
  return doc(node)
}
// Nesting one to three deep, which tests/recorded/tiptap.rs folds into the conversions of any
// depth.
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
const paragraphs = depths.map(depth => nested("paragraph", depth))
// Tables and a mention in blockquotes one to three deep, which `all` parses.
const quotedMarkdown = depths.flatMap(depth => {
  const quotes = "> ".repeat(depth)
  return [`${quotes}| a |\n${quotes}| - |`, `${quotes}| \`a|b\` |\n${quotes}| - |`, ">".repeat(depth) + ' [@ id="x"]']
})

const ownMarkdown = [
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
]

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
const ownHTML = unique([...pickedHTML, ...deepHTML])
// The HTML inputs the options change the parses of, which Tiptap's own don't record:
// harness/record-tiptap-inline.mjs reads the documents of theirs.
const optionHTML = [
  "<h1>1</h1><h2>2</h2><h3>3</h3><h4>4</h4><h5>5</h5><h6>6</h6>",
  '<p>a<img src="a.png">b</p>',
  '<p><img src="data:image/png;base64,AA" alt="x">c</p>',
  '<img src="DATA:image/png;base64,AA"><img src=" data:x"><img src="">',
  ...marks,
]
const colorHTML = [
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
]

const markTypes = ["bold", "italic", "strike", "underline", "highlight", "subscript", "superscript", "textStyle"]
const docs = [
  doc({ type: "paragraph" }),
  doc(
    paragraph(text("plain "), ...markTypes.map(type => text(type, type)), { type: "hardBreak" }, text("both", "bold", "italic")),
    paragraph(text("a ", "bold"), text("b", "bold", "italic"), text(" c", "italic")),
    paragraph(text("*not em* <b>not bold</b> & 1. two")),
  ),
  doc(
    { type: "heading", attrs: { level: 2 }, content: [text("two")] },
    { type: "heading", attrs: { level: "3" }, content: [text("three", "bold")] },
    { type: "heading", content: [text("no level")] },
    { type: "heading", attrs: { level: 4 } },
  ),
  doc(
    { type: "image", attrs: { src: "a.png", alt: "A", title: "T", width: 10 } },
    { type: "image", attrs: { src: "b.png" } },
    { type: "image" },
  ),
]
const colors = ["red", "", null, 0, 1, "#FF0000", 'a"b']
// Documents whose rendering the options change: headings of each level and of others, images
// inline and highlights of each color, then each type Given gives its attribute to with it.
const levels = [1, 2, 3, 4, 5, 6, 0, 7, 2.5, "2", null, true]
const given = { given: "red" }
const optionDocs = [
  doc(...levels.map(level => ({ type: "heading", attrs: { level }, content: [text("h")] }))),
  doc(paragraph(text("a"), { type: "image", attrs: { src: "a.png", alt: "A" } }, text("b"))),
  ...colors.map(color => doc(paragraph(text("x", { type: "highlight", attrs: { color } })))),
  doc({ type: "paragraph", attrs: given, content: [text("p")] }),
  doc({ type: "heading", attrs: { level: 2, ...given }, content: [text("h")] }),
  doc({ type: "image", attrs: { src: "a.png", alt: "A", ...given } }),
  doc(paragraph(text("a"), { type: "hardBreak", attrs: given }, text("b"))),
  ...["bold", "italic", "strike", "underline", "subscript", "superscript", "highlight"].map(type =>
    doc(paragraph(text("x", { type, attrs: given }))),
  ),
  doc(paragraph(text("x", { type: "highlight", attrs: { color: "yellow", ...given } }))),
]
const colorDocs = [
  ...colors.map(color => doc(paragraph(text("x", { type: "highlight", attrs: { color } })))),
  ...colors.map(color => doc(paragraph(text("x", { type: "textStyle", attrs: { color } })))),
  doc(
    paragraph(text("x", { type: "highlight", attrs: { color: "yellow" } }, { type: "textStyle", attrs: { color: "blue" } })),
  ),
]
const codeDocs = [
  doc(paragraph(text("*a* <b>", "code"), text(" *c*")), { type: "codeBlock", content: [text("*d* <e>")] }),
  ...["x", 1, true, "", null].map(made => doc({ type: "figure", attrs: { made } })),
]

// Leveled outside lists, after an item's paragraph in each kind of list, and in lists nested in
// lists' items, which each indent their content.
const leveled = value => ({ type: "leveled", content: [text(value)] })
const listed = (type, ...content) => ({ type, content: [{ type: type === "taskList" ? "taskItem" : "listItem", content }] })
const levelDocs = [
  doc(leveled("a")),
  ...["bulletList", "orderedList", "taskList"].map(type => doc(listed(type, paragraph(text("a")), leveled("b")))),
  doc(listed("bulletList", paragraph(text("a")), listed("orderedList", paragraph(text("b")), leveled("c")))),
  doc(
    listed(
      "orderedList",
      paragraph(text("a")),
      listed("taskList", paragraph(text("b")), listed("bulletList", paragraph(text("c")), leveled("d"))),
      leveled("e"),
    ),
    leveled("f"),
  ),
]
// Spaces, tabs and newlines in Preformatted's element and in elements around it, beside a
// paragraph's and a list item's, which collapse theirs.
const preHTML = [
  "<div data-pre>  a  \n\tb  </div>",
  "<div data-pre>\n\na\n</div>",
  "<div data-pre> </div>",
  "<div data-pre><span> a </span>  <b>b\n</b>\n</div>",
  "<p> a  \n b </p><div data-pre> a  \n b </div><div> a  \n b </div>",
  "<ul><li><p> a  b </p><div data-pre> c \n  d </div></li></ul>",
  "<div data-pre>a\r\nb\rc&nbsp; d</div>",
]

// Documents of text whose `text` or `marks` is something other than what the manager calls a
// method of, so that they record V8's error for each.
const textOf = node => doc(paragraph({ type: "text", ...node }))
const values = [1, 0, 1.5, "x", 'a"b', "", true, false, null, {}, [], [1], { some: 1 }]
const unreadable = [
  ...values.flatMap(value => [
    textOf({ text: "a", marks: { some: value } }),
    textOf({ text: { replace: value } }),
    textOf({ text: "a", marks: [{ type: { localeCompare: value } }, { type: "unknown" }] }),
    textOf({ text: "a", marks: [{ type: "unknown" }, { type: { localeCompare: value } }] }),
  ]),
  textOf({ text: "a", marks: { other: 1 } }),
  textOf({ text: { other: 1 } }),
  textOf({ text: [1] }),
  textOf({ text: 5 }),
]

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
  return [doc({ type: "paragraph", attrs, content: [text("p")] }), doc(paragraph(text("b", { type: "bold", attrs })))]
})
// Each type of `nodeType` alone, and Typed with a `nodeType` of each kind of value.
const nodeTypeDocs = [
  doc(paragraph(text("p"))),
  doc({ type: "heading", content: [text("h")] }),
  doc(paragraph(text("a"), { type: "hardBreak" })),
  doc(paragraph(text("b", "bold"))),
  doc(paragraph(text("i", "italic"))),
  ...[1, "1", true, [1], 0, "x", {}, null].map(nodeType => doc({ type: "typed", attrs: { nodeType } })),
]

// Tiptap's own extensions with an option that reaches the parse rules and renderHTML of `type`:
// the HTML inputs that hold such an element, and the documents of that type.
const holding = element => unique([...ownHTML, ...optionHTML]).filter(html => element.test(html))
const optioned = (extensions, element, type) => ({
  extensions,
  parsesHTML: holding(element),
  serializesHTML: parsed =>
    [...docs, ...optionDocs, ...parsed].filter(doc => JSON.stringify(doc).includes(`"type":"${type}"`)),
})
// HTMLAttributes reach only renderHTML.
const attributed = extensions => ({ extensions, serializesHTML: [...docs, ...paragraphs, ...optionDocs] })

// What one group nests in another's: lists in blockquotes, details and table cells, blocks in
// list items, and links, mentions and styles in table cells.
const combinedMarkdown = [
  "> - a\n> - b\n>\n> 1. c",
  "> - [ ] task\n>   - [x] done",
  "- a\n\n  > quote\n\n  ```js\n  code\n  ```\n\n  ---",
  "1. # heading\n\n   $x$ and :smile:",
  '| [link](u) | [@ id="m"] |\n| - | - |\n| **b** `c` | $x$ |',
  ":::details\n\n:::detailsSummary\n\nsummary\n\n:::\n\n:::detailsContent\n\n- a\n- [ ] b\n\n> q\n\n:::\n\n:::",
  "> :::details\n> x\n> :::",
  ...quotedMarkdown,
]
const combinedHTML = [
  "<blockquote><ul><li>a</li></ul><ol><li><p>b</p></li></ol></blockquote>",
  '<table><tr><td><ul><li>a</li></ul><ul data-type="taskList"><li data-type="taskItem" data-checked="true">t</li></ul></td><td><ol start="3"><li>b</li></ol></td></tr></table>',
  '<table><tr><th><a href="https://x.test">link</a></th><td><span data-type="mention" data-id="m" data-label="M"></span> <span style="color: red; font-size: 2px">styled</span></td></tr></table>',
  '<details open><summary>s <b>bold</b></summary><div data-type="detailsContent"><ul><li>a</li></ul><table><tr><td>cell</td></tr></table></div></details>',
  "<ul><li><blockquote><p>q</p></blockquote><pre><code class='language-js'>c</code></pre><hr></li></ul>",
  '<table><tr><td><blockquote><p>quoted</p></blockquote><p style="text-align: right">r</p></td></tr></table>',
  '<blockquote><p style="text-align: center"><a href="u"><span style="font-size: 3px">x</span></a></p></blockquote>',
  '<ul data-type="taskList"><li data-type="taskItem" data-checked="false"><label><input type="checkbox"><span></span></label><div><p>t <code>c</code></p></div></li></ul>',
]
const item = (...content) => ({ type: "listItem", content })
const bullets = (...items) => ({ type: "bulletList", content: items.map(each => item(paragraph(text(each)))) })
const cell = (...content) => ({ type: "tableCell", content })
const table = (...cells) => ({ type: "table", content: [{ type: "tableRow", content: cells }] })
const combinedDocs = [
  doc({ type: "blockquote", content: [bullets("a", "b"), paragraph(text("after"))] }),
  doc(
    table(
      cell(bullets("a", "b")),
      cell({ type: "orderedList", attrs: { start: 3 }, content: [item(paragraph(text("c")))] }),
    ),
  ),
  doc(
    table(
      cell(paragraph(text("link", { type: "link", attrs: { href: "u" } }), { type: "mention", attrs: { id: "m" } })),
      cell(paragraph(text("styled", { type: "textStyle", attrs: { color: "red" } }))),
    ),
  ),
  doc({
    type: "details",
    content: [
      { type: "detailsSummary", content: [text("s")] },
      {
        type: "detailsContent",
        content: [
          bullets("a"),
          { type: "taskList", content: [{ type: "taskItem", attrs: { checked: true }, content: [paragraph(text("t"))] }] },
        ],
      },
    ],
  }),
  doc({
    type: "bulletList",
    content: [
      item(
        paragraph(text("a")),
        { type: "blockquote", content: [paragraph(text("q"))] },
        { type: "codeBlock", attrs: { language: "js" }, content: [text("c")] },
      ),
    ],
  }),
  doc({
    type: "blockquote",
    content: [
      { type: "heading", attrs: { level: 2, textAlign: "center" }, content: [text("h")] },
      table(cell(paragraph(text("in a quote")))),
    ],
  }),
]

record("tiptap", {
  own: {
    extensions: own,
    parsesMarkdown: [...ownMarkdown, ...deepMarkdown],
    parsesHTML: ownHTML,
    serializesMarkdown: withParsed([...docs, ...deepDocs, ...unreadable]),
    // `doc` has no toDOM, so the serializer has nothing to call for a doc inside the doc.
    serializesHTML: withParsed([...docs, ...paragraphs, doc({ type: "doc" })]),
  },
  colors: {
    extensions: [...own.filter(extension => extension !== Highlight), Highlight.configure({ multicolor: true }), Color],
    parsesMarkdown: ["==high==", "==a **b** c==", "a ==b== ++==c==++", '<span style="color: red">x</span>'],
    parsesHTML: colorHTML,
    serializesMarkdown: withParsed(colorDocs),
    serializesHTML: withParsed(colorDocs),
  },
  code: {
    extensions: [Document, Paragraph, Text, ApplicationCode, ApplicationCodeBlock, Figure],
    parsesMarkdown: ["`*a* <b>` \\*c\\*\n\n```\n*d* <e>\n```", "`code` *em*", "```\nblock\n```"],
    parsesHTML: [
      "<pre>a  \n b</pre>",
      '<figure made="x"></figure>',
      '<figure made="1"></figure>',
      '<figure made="true"></figure>',
      '<figure made=""></figure>',
      "<figure></figure><p>after</p>",
    ],
    serializesMarkdown: withParsed(codeDocs),
    serializesHTML: withParsed(codeDocs),
  },
  htmlAttributes: attributed(
    configured(strings, { image: { alt: "option", title: "option", width: 5 }, highlight: { "data-color": "option" } }),
  ),
  // A falsy class and data-given give way to Given's.
  htmlAttributeValues: attributed(
    configured(otherAttributes, { subscript: { style: 5 }, superscript: { class: 0, "data-given": null } }),
  ),
  levels: optioned(replaced(Heading, Heading.configure({ levels: [4, 2] })), /<h[1-6]/i, "heading"),
  noLevels: optioned(replaced(Heading, Heading.configure({ levels: [] })), /<h[1-6]/i, "heading"),
  // multicolor adds the color a mark parses and renders.
  multicolor: optioned(replaced(Highlight, Highlight.configure({ multicolor: true })), /<mark/i, "highlight"),
  // inline makes an image inline content.
  inline: optioned(replaced(Image, Image.configure({ inline: true })), /<img/i, "image"),
  // allowBase64 reaches only Image's parse rule.
  base64: { extensions: replaced(Image, Image.configure({ allowBase64: true })), parsesHTML: holding(/<img/i) },
  renderedValues: {
    extensions: [...replaced(Paragraph, Paragraph.configure({ HTMLAttributes: { class: "option", style: "color: blue" } })), Renders],
    serializesHTML: renderedDocs,
  },
  nodeType: {
    extensions: [
      Document,
      Paragraph.configure(nodeTyped(null)),
      Text,
      Heading.configure(nodeTyped(1)),
      HardBreak.configure(nodeTyped(0)),
      Bold.configure(nodeTyped("1")),
      Italic.configure(nodeTyped("x")),
      Typed,
    ],
    serializesHTML: nodeTypeDocs,
  },
  application: {
    extensions: [
      Document,
      Paragraph,
      Text,
      BulletList,
      OrderedList,
      ListItem,
      TaskList,
      TaskItem.configure({ nested: true }),
      Leveled,
      Preformatted,
    ],
    parsesHTML: preHTML,
    serializesMarkdown: levelDocs,
  },
  all: {
    extensions: [
      ...own,
      BulletList,
      OrderedList,
      ListItem,
      TaskList,
      TaskItem,
      Table,
      TableRow,
      TableCell,
      TableHeader,
      Blockquote,
      Code,
      CodeBlock,
      HorizontalRule,
      Details,
      DetailsSummary,
      DetailsContent,
      Mathematics,
      Emoji.configure({ emojis: [] }),
      Youtube,
      Twitch,
      Audio,
      TableOfContents,
      RubyText,
      Link,
      Mention,
      BackgroundColor,
      Color,
      FontFamily,
      FontSize,
      LineHeight,
      TextAlign.configure({ types: ["heading", "paragraph"] }),
    ],
    parsesMarkdown: [...markedInputs, ...combinedMarkdown],
    parsesHTML: [...domInputs, ...combinedHTML],
    serializesMarkdown: withParsed(combinedDocs),
    serializesHTML: withParsed(combinedDocs),
  },
})
