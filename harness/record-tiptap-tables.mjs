// Records what @tiptap/extension-table's Table, TableRow, TableCell and TableHeader make with
// @tiptap/markdown's MarkdownManager and ProseMirror's DOMParser and DOMSerializer, to
// fixtures/tiptap-tables.json, for tarnish-tiptap's tests/tables.rs to expect. Its records are
// record-tiptap.mjs's, each naming in `extensions` the configuration it's of:
// - `tables`: Document, Paragraph, Text, HardBreak, Bold, Italic, Heading, an application's code
//   mark that parses marked's code spans, and the four table extensions as they come;
// - `attributes`, `attributeValues`, `wrapper`, `minWidth`, `zero` and `resizable`: the same with
//   options set, `attributeValues` with HTMLAttributes that aren't strings;
// - `global`: with an application's global `style` attribute on tables and cells, and `both`
//   with Table's `HTMLAttributes` too;
// - `kit`: TableKit, with a minimum cell width;
// - `partial`: rows and cells, with no Table.
// `schemas` records each configuration's schema, as harness/fixture.mjs's schemaRecord writes it,
// and `queries` the `width` of each element each table's `querySelectorAll("colgroup > col")`
// finds, which TableCell's colwidth reads when the cell has none of its own.
import { readFileSync } from "node:fs"
import { Extension, Mark, getSchema } from "@tiptap/core"
import { Bold } from "@tiptap/extension-bold"
import { Document } from "@tiptap/extension-document"
import { HardBreak } from "@tiptap/extension-hard-break"
import { Heading } from "@tiptap/extension-heading"
import { Italic } from "@tiptap/extension-italic"
import { Paragraph } from "@tiptap/extension-paragraph"
import { Table, TableCell, TableHeader, TableKit, TableRow } from "@tiptap/extension-table"
import { Text } from "@tiptap/extension-text"
import { MarkdownManager } from "@tiptap/markdown"
import { DOMParser, DOMSerializer, Node } from "@tiptap/pm/model"
import { parseHTML } from "linkedom"
import { Marked } from "marked"
import moreLists from "marked-more-lists"
import { otherAttributes, outcome, schemaRecord, writeFixture } from "./fixture.mjs"

const Code = Mark.create({
  name: "code",
  code: true,
  parseHTML: () => [{ tag: "code" }],
  renderHTML: ({ HTMLAttributes }) => ["code", HTMLAttributes, 0],
  markdownTokenName: "codespan",
  parseMarkdown: (token, helpers) => helpers.applyMark("code", [{ type: "text", text: token.text }]),
  renderMarkdown: (node, helpers) => "`" + helpers.renderChildren(node) + "`",
})
const inline = [Document, Paragraph, Text, HardBreak, Bold, Italic, Heading, Code]
const tables = (table = Table, row = TableRow, cell = TableCell, header = TableHeader) => [
  ...inline,
  table,
  row,
  cell,
  header,
]
const Style = Extension.create({
  name: "style",
  addGlobalAttributes: () => [
    {
      types: ["table", "tableCell", "tableHeader"],
      attributes: {
        style: {
          default: null,
          parseHTML: element => element.getAttribute("style"),
          renderHTML: attributes => (attributes.style ? { style: attributes.style } : {}),
        },
      },
    },
  ],
})
const configurations = {
  tables: tables(),
  attributes: tables(
    Table.configure({ HTMLAttributes: { class: "t", style: "color: red; min-width: 1px" } }),
    TableRow.configure({ HTMLAttributes: { class: "r" } }),
    TableCell.configure({ HTMLAttributes: { class: "c", style: "padding: 1px" } }),
    TableHeader.configure({ HTMLAttributes: { class: "h", "data-x": "y" } }),
  ),
  attributeValues: tables(
    Table.configure({ HTMLAttributes: otherAttributes }),
    TableRow.configure({ HTMLAttributes: otherAttributes }),
    TableCell.configure({ HTMLAttributes: otherAttributes }),
    TableHeader.configure({ HTMLAttributes: otherAttributes }),
  ),
  wrapper: tables(Table.configure({ renderWrapper: true })),
  minWidth: tables(Table.configure({ cellMinWidth: 12.5 })),
  zero: tables(Table.configure({ cellMinWidth: 0 })),
  resizable: tables(
    Table.configure({ resizable: true, handleWidth: 10, lastColumnResizable: false, allowTableNodeSelection: true, View: null }),
  ),
  global: [...tables(), Style],
  both: [...tables(Table.configure({ HTMLAttributes: { class: "b", style: "color: red; width: 9px" } })), Style],
  kit: [...inline, TableKit.configure({ table: { cellMinWidth: 30 } })],
  partial: [Document, Paragraph, Text, TableRow, TableCell, TableHeader],
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
const unique = values => [...new Set(values)]
const body = html => parseHTML(`<!DOCTYPE html><html><body>${html}</body></html>`).document.body

const schemas = Object.entries(converters).map(([name, { schema }]) => ({ extensions: name, ...schemaRecord(schema) }))

// A table `depth` deep in a cell of a table, around a paragraph's text.
const nestedTable = depth => {
  let node = { type: "paragraph", content: [{ type: "text", text: "x" }] }
  for (let level = 0; level < depth; level++) {
    node = { type: "table", content: [{ type: "tableRow", content: [{ type: "tableCell", content: [node] }] }] }
  }
  return { type: "doc", content: [node] }
}
// Nesting one to three deep, which tests/tables.rs folds into the conversions of any depth.
const depths = [1, 2, 3]
const deepMarkdown = depths.flatMap(depth => [
  "> ".repeat(depth) + "| a |\n" + "> ".repeat(depth) + "| - |",
  "> ".repeat(depth) + "| `a|b` |\n" + "> ".repeat(depth) + "| - |",
])
const deepHTML = depths.map(depth => "<table><tr><td>".repeat(depth) + "x" + "</td></tr></table>".repeat(depth))
const deepDocs = depths.map(nestedTable)

const markdownTables = [
  // Marked's own table forms.
  "| a | b |\n| - | - |\n| 1 | 2 |",
  "a | b\n--|--\n1 | 2",
  "| a |\n| - |",
  "|a|\n|-|\n|1|",
  "| l | c | r | n |\n| :-- | :-: | --: | --- |\n| 1 | 2 | 3 | 4 |",
  "| :- | -: |\n|:-:|---|",
  "| a | b |\n|:-|-:|\n| 1 |",
  "| a | b |\n| - | - |\n| 1 | 2 | 3 |",
  "| a | b |\n| - | - |\n| only one |",
  "| a | b |\n| - | - |\n|  |  |",
  "| | |\n|-|-|\n| | |",
  "| a | b |\n| - |",
  "| a |\n| - |\n| b |\n| - |",
  "| a |\n| \t- |\n| 1 |",
  "  | a |\n  | - |\n  | 1 |",
  "| a |\n| - |\n\n| b |\n| - |",
  "| a |\r\n| - |\r\n| 1 |",
  "| é | 😀 |\n| - | - |\n| 中文 | x |",
  // Escaped pipes, and pipes in code spans, which Table's tokenizer escapes for marked.
  "| a \\| b | c |\n| - | - |",
  "| `a|b` | c |\n| - | - |\n| 1 | `x|y|z` |",
  "| `a||b` | `c` |\n|---|---|\n| `x|y` | z |",
  "| `a | b |\n| - | - |",
  "| ``a|`b`` | c |\n|-|-|",
  "| `a\\|b` | c |\n|-|-|",
  "| \\`a|b` | c |\n|-|-|",
  "| `a|b` |\n| - |\n| 1 |\nafter",
  "| `a|b` |\n| - |\n# heading",
  "| `a|b` |\n|-|\n\nnext",
  "| `a|b` |\n| - |\n| `c|d` |\n\n| `e|f` |\n| - |",
  "| `a|b` |\n| - |\n[ref]: http://x.com\n\n[ref]",
  "`a|b`\n| - |",
  "| a |\n| - |\n| `b|c` |",
  // Inline content in cells.
  "| **bold** | *em* | `code` |\n| --- | --- | --- |\n| [link](x) | ~~del~~ | a<br>b |",
  "| a  \nb |\n| - |",
  "| &amp; &lt; | <b>x</b> |\n| - | - |",
  "| ![img](a.png) |\n| - |",
  "| \\*not em\\* | a_b_c |\n| - | - |",
  // Tables among other blocks, and where Table's start cuts a paragraph short.
  "para\n| a |\n| - |",
  "para\n\n| a |\n| - |\n\npara",
  "# heading\n| a |\n| - |",
  "a|b|c\n-|-",
  "a|b|c|d\n-|-",
  "ab|c\n-|-|-",
  "x | y | z\n--- | ---",
  "a|b\n---",
  "a | b\n:-:",
  "one\ntwo|three\n-|-",
  "> | a |\n> | - |\n> | 1 |",
  "- | a |\n  | - |\n  | 1 |",
  "- item\n\n  | `a|b` |\n  | - |",
  "1. | a | b |\n   | - | - |",
  "```\n| a |\n| - |\n```",
  "<table><tr><td>html</td></tr></table>",
  "x <td>y</td> z",
  "    | a |\n    | - |",
]
const parsesMarkdown = [
  ...unique([
    "",
    "plain",
    ...markdownTables,
    ...fixture("marked").lexed.map(record => record.input),
    ...deepMarkdown,
  ]).map(markdown => ["tables", markdown]),
  ...["| a | b |\n| - | - |\n| 1 | 2 |", "| `a|b` |\n| - |", "a|b|c\n-|-"].flatMap(markdown =>
    ["kit", "partial"].map(name => [name, markdown]),
  ),
].map(([name, markdown]) => ({
  extensions: name,
  markdown,
  ...outcome(() => ({ doc: converters[name].manager.parse(markdown) })),
}))

// The inputs html5ever builds another tree from than the fork's parse5 does, as in
// record-tiptap.mjs.
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
const row = (...cells) => `<tr>${cells.join("")}</tr>`
const table = (...rows) => `<table>${rows.join("")}</table>`
const htmlTables = [
  table(row("<td>a</td>", "<td>b</td>"), row("<td>1</td>", "<td>2</td>")),
  "<table><thead><tr><th>h</th><th>i</th></tr></thead><tbody><tr><td>a</td><td>b</td></tr></tbody><tfoot><tr><td>f</td></tr></tfoot></table>",
  table(row("<th>header</th>", "<td>cell</td>")),
  // colspan and rowspan, which read as numbers when they spell one.
  ...["2", "0", "-1", "abc", "", "2.5", " 2", "1e3", "+3", "true"].map(span =>
    table(row(`<td colspan="${span}" rowspan="${span}">x</td>`, `<th colspan="${span}">y</th>`)),
  ),
  // colwidth, read from the cell's own attribute and else from the table's colgroup.
  ...["100", "100,200", "abc", "100,", " 50", "", "1e3", "50px", "-5", "0x10", "1,,2"].map(width =>
    table(row(`<td colwidth="${width}">x</td>`, `<th colwidth="${width}">y</th>`)),
  ),
  "<table><colgroup><col width=100><col width=abc><col><col width=''></colgroup><tr><td>a</td><td>b</td><td>c</td><td>d</td><td>e</td></tr><tr><th>f</th><td>g</td></tr></table>",
  "<table><colgroup><col width=100></colgroup><tr><td colwidth=50>a</td><td>b</td></tr></table>",
  "<table><colgroup><col span=2 width=100><col width=30></colgroup><tr><td>a</td><td>b</td></tr></table>",
  "<table><colgroup><col width=10></colgroup><colgroup><col width=20></colgroup><tr><td>a</td><td>b</td></tr></table>",
  "<table><colgroup><col style='width: 40px'></colgroup><tr><td>a</td></tr></table>",
  "<table><col width=15><tr><td>a</td></tr></table>",
  "<table><tr><td>a</td><td><table><colgroup><col width=70><col width=80></colgroup><tr><td>b</td><td>c</td></tr></table></td></tr></table>",
  // Alignment, from the style and else the align attribute.
  ...[
    "style='text-align: left'",
    "style='text-align: right'",
    "style='text-align: center'",
    "style='text-align: CENTER'",
    "style='text-align: justify'",
    "style='text-align:center !important'",
    "style='TEXT-ALIGN: Right'",
    "style='text-align: '",
    "style='text-align: left; text-align: right'",
    "align=right",
    "align=' Center '",
    "align=middle",
    "style='text-align: left' align=right",
    "style='text-align: justify' align=center",
    "style='color: red'",
  ].map(attributes => table(row(`<td ${attributes}>x</td>`, `<th ${attributes}>y</th>`))),
  // Cells with nothing in them are filled, as are those with only collapsible whitespace.
  table(row("<td></td>", "<th></th>")),
  table(row("<td> </td>", "<td>\n\t</td>", "<td>&nbsp;</td>", "<td>\f</td>")),
  table(row("<td><br></td>", "<td><p></p></td>", "<td><b></b></td>", "<td><!-- c --></td>")),
  table(row("<td><p>a</p><p>b</p></td>", "<td><h1>h</h1>t</td>", "<td><ul><li>x</li></ul></td>")),
  table(row("<td><b>x</b> <i>y</i><br>z</td>", "<td><code>c</code></td>")),
  // Tables where HTML puts other things, and other things where HTML puts tables.
  "<table></table>",
  "<table><caption>cap</caption><tr><td>x</td></tr></table>",
  "<p>a<table><tr><td>b</td></tr></table>c</p>",
  "<table>foster<tr><td>x</td></tr></table>",
  "<tr><td>a</td></tr>",
  "<td>x</td>",
  "<table><tr></tr></table>",
  "<table><tr><td>a</td></tr><tr></tr><tr><td>b</td></tr></table>",
  table(row("<td>" + table(row("<td>inner</td>")) + "</td>", "<td>outer</td>")),
  '<div class="tableWrapper"><table><tr><td>x</td></tr></table></div>',
  '<table style="min-width: 50px"><colgroup><col style="min-width: 25px"><col style="min-width: 25px"></colgroup><tbody><tr><th colspan="1" rowspan="1"><p>a</p></th><td colspan="2" rowspan="1" colwidth="100,150" style="text-align: right"><p>b</p></td></tr></tbody></table>',
  "<ul><li><table><tr><td>cell in item</td></tr></table></li></ul>",
  "<blockquote><table><tr><td>quoted</td></tr></table></blockquote>",
  "<svg><td>svg cell</td></svg>",
  "<template><table><tr><td>t</td></tr></table></template><p>after</p>",
  "<figure><table><tr><td>fig</td></tr></table></figure>",
]
const parsesHTML = [
  ...unique([
    "<p>plain</p>",
    ...htmlTables,
    ...fixture("dom")
      .parses.map(record => record.html)
      .filter(html => !otherTrees.has(html)),
    ...deepHTML,
  ]).map(html => ["tables", html]),
  ...[
    table(row('<td style="color: red; text-align: center">a</td>', '<th style="padding: 1px">b</th>')),
    '<table style="width: 10px"><tr><td>a</td></tr></table>',
    table(row('<td style="">a</td>')),
  ].map(html => ["global", html]),
  ...[table(row("<td>a</td>", "<th>b</th>")), "<table></table>"].map(html => ["kit", html]),
  ...[table(row("<td>a</td>", "<th></th>")), "<tr><td>x</td></tr>", "<p>p</p>"].map(html => ["partial", html]),
].map(([name, html]) => ({
  extensions: name,
  html,
  ...outcome(() => ({ doc: converters[name].parser.parse(body(html)).toJSON() })),
}))

const queries = unique(htmlTables).map(html => {
  const tables = [...body(html).querySelectorAll("table")]
  return {
    html,
    tables: tables.map(table => [...table.querySelectorAll("colgroup > col")].map(col => col.getAttribute("width"))),
  }
})

// The documents the parses of each configuration make, each read from its JSON text, as the tests
// read it.
const parsed = name =>
  [...parsesMarkdown, ...parsesHTML].filter(record => record.extensions === name && record.doc).map(record => record.doc)
const documents = (name, docs) =>
  unique([...docs, ...parsed(name)].map(doc => JSON.stringify(doc))).map(text => [name, JSON.parse(text)])

const text = (text, ...marks) => ({ type: "text", text, ...(marks.length ? { marks: marks.map(type => ({ type })) } : {}) })
const paragraph = (...content) => ({ type: "paragraph", content })
const cellOf = type => (attrs, ...content) => ({
  type,
  ...(attrs ? { attrs } : {}),
  content: content.length ? content : [paragraph(text("x"))],
})
const cell = cellOf("tableCell")
const header = cellOf("tableHeader")
const tableRow = (...cells) => ({ type: "tableRow", content: cells })
const doc = (...content) => ({ type: "doc", content })
const tableOf = (...rows) => doc({ type: "table", content: rows })
const firstRow = (...cells) => tableOf(tableRow(...cells), tableRow(cell()))
// Every way a cell's colspan and colwidth read, JSON's every type among them.
const widths = [
  [100],
  [100, 200],
  [10],
  [0],
  [null],
  [-50],
  [1.5],
  ["50"],
  ["abc"],
  [true],
  [[100]],
  [[1, 2]],
  [{}],
  [{ toString: 1 }],
  [""],
  [],
  "100",
  "",
  100,
  true,
  { 0: 40 },
  { toString: 1 },
  null,
]
const spans = [1, 2, 0, -1, 2.5, "2", "abc", "", true, false, null, [2], [1, 2], {}, { toString: 1 }]
const htmlDocs = [
  doc({ type: "paragraph" }),
  tableOf(tableRow(header(null, paragraph(text("h"))), header()), tableRow(cell(null, paragraph(text("a"))), cell())),
  firstRow(cell({ colwidth: [100] }), cell({ colwidth: [50] })),
  firstRow(cell({ colwidth: [100] }), cell()),
  firstRow(cell({ colspan: 2, colwidth: [100, 200] }), cell({ colwidth: [30] })),
  firstRow(cell({ colspan: 2, colwidth: [100] })),
  firstRow(cell({ colspan: 3 }), header({ colspan: 2, colwidth: [10, 60] })),
  ...widths.map(colwidth => firstRow(cell({ colwidth }), cell({ colwidth: [40] }))),
  ...spans.map(colspan => firstRow(cell({ colspan, colwidth: [20, 30, 40] }), cell({ colwidth: [5] }))),
  tableOf(tableRow(cell({ colwidth: [100] })), tableRow(cell(), cell({ colwidth: [999] }))),
  ...["left", "right", "center", "justify", "", null, 1, true].map(align =>
    firstRow(cell({ align }), header({ align })),
  ),
  firstRow(cell({ rowspan: 2, colspan: 1 }), cell({ rowspan: "3" })),
  tableOf(tableRow()),
  tableOf(tableRow(), tableRow(cell())),
  doc({ type: "table" }),
  doc({ type: "table", content: [] }),
  doc({ type: "table", content: [paragraph(text("p"))] }),
  doc({ type: "table", content: [{ type: "heading", attrs: { level: 1 } }] }),
  doc({ type: "table", content: [tableRow(paragraph(text("a")), cell())] }),
  doc({ type: "table", content: [tableRow({ type: "hardBreak" })] }),
  doc(tableRow(cell()).content[0]),
  doc(paragraph(text("before")), tableOf(tableRow(cell())).content[0], paragraph(text("after"))),
  ...deepDocs,
]
// Cells holding what Markdown has to fit on one line, and tables of every shape.
const markdownDocs = [
  tableOf(tableRow(header(), header(null, paragraph(text("longer header")))), tableRow(cell(), cell())),
  tableOf(tableRow(cell(), cell(null, paragraph(text("longer")))), tableRow(cell(), cell())),
  tableOf(tableRow(cell(), header()), tableRow(header(), cell())),
  tableOf(tableRow(cell()), tableRow(header(null, paragraph(text("late header"))))),
  tableOf(tableRow(header(), header(), header()), tableRow(cell()), tableRow(cell(), cell(), cell(), cell())),
  tableOf(tableRow(header({ align: "left" }), header({ align: "center" }), header({ align: "right" }), header({ align: "justify" }))),
  tableOf(tableRow(cell(), cell({ align: "right" })), tableRow(cell({ align: "center" }), cell({ align: "left" }))),
  tableOf(tableRow(cell({ align: 1 }), cell({ align: null }), cell({ align: ["left"] }))),
  tableOf(tableRow(cell(null, paragraph(text("a")), paragraph(text("b"))))),
  tableOf(tableRow(cell(null, paragraph(text("a")), { type: "heading", attrs: { level: 2 }, content: [text("h")] }))),
  tableOf(tableRow(cell(null, { type: "heading", attrs: { level: 2 }, content: [text("h")] }))),
  tableOf(tableRow(cell(null, paragraph(text("a"), { type: "hardBreak" }, text("b"))))),
  tableOf(tableRow(cell(null, paragraph(text("bold", "bold"), text(" and "), text("em", "italic"), text("code", "code"))))),
  tableOf(tableRow(cell(null, paragraph(text("a", "bold")), paragraph(text("b", "bold"))))),
  tableOf(tableRow(cell(null, paragraph(text("a|b \\ * _ ` [ ] ~ < & >"))))),
  tableOf(tableRow(cell(null, paragraph(text("  lots   of \t space  nbsp　  "))))),
  tableOf(tableRow(cell(null, paragraph(text("line\nbreak\r\nand\n\n\nmore"))))),
  tableOf(tableRow(cell(null, paragraph(text("😀😀")), paragraph()), cell(null, paragraph(text("é"))))),
  tableOf(tableRow(cell(null, paragraph()))),
  tableOf(tableRow(cell(null, paragraph(), paragraph()))),
  tableOf(tableRow(cell({ colspan: 2, rowspan: 2, colwidth: [10, 20] }))),
  tableOf(tableRow(cell(null, tableOf(tableRow(cell())).content[0]))),
  tableOf(tableRow(cell(null, paragraph(text("a")), tableOf(tableRow(cell())).content[0]))),
  tableOf(tableRow({ type: "tableCell" }), tableRow({ type: "tableHeader", content: [] })),
  doc(paragraph(text("before")), tableOf(tableRow(cell())).content[0], paragraph(text("after"))),
  doc(tableOf(tableRow(cell())).content[0], tableOf(tableRow(cell())).content[0]),
  ...deepDocs,
]
// Tables that aren't of the schema's nodes, as the manager reads them.
const malformedDocs = [
  ...[
    undefined,
    null,
    [],
    "",
    "abc",
    0,
    5,
    true,
    {},
    { length: 0 },
    [null],
    [5],
    ["row"],
    [{}],
    [{ content: null }],
    [{ content: "abc" }],
    [{ content: {} }],
    [{ content: [] }],
    [{ content: [null] }],
    [{ content: [5] }],
    [{ content: [{}] }],
    [{ content: [{ content: "abc" }] }],
    [{ content: [{ content: { type: "text", text: "obj" } }] }],
    [{ content: [{ content: [text("t")] }] }],
    [{ content: [{ content: [{ type: "paragraph", content: [text("a")] }, text("b")] }] }],
    [{ content: [{ content: [5, null] }] }],
    [{ content: [{ attrs: "left", content: [] }] }],
    [{ content: [{ attrs: null, type: "tableHeader" }] }],
    [{ content: [{ attrs: { align: "center" }, type: "tableHeader" }] }],
  ].map(content => doc({ type: "table", ...(content === undefined ? {} : { content }) })),
]
const serializesMarkdown = [
  ...documents("tables", [...markdownDocs, ...malformedDocs, ...htmlDocs]),
  ...documents("kit", [markdownDocs[0]]),
  ...documents("partial", [markdownDocs[0]]),
].map(([name, doc]) => ({ extensions: name, doc, ...outcome(() => ({ markdown: converters[name].manager.serialize(doc) })) }))

const serializesHTML = [
  ...documents("tables", [...htmlDocs, ...markdownDocs]),
  ...["attributes", "attributeValues", "wrapper", "minWidth", "zero", "resizable", "global"].flatMap(name =>
    documents(name, htmlDocs),
  ),
  ...["global", "both"].flatMap(name =>
    documents(name, [
      firstRow(cell({ style: "color: red" }), cell({ style: "text-align: left", align: "right" })),
      firstRow(header({ style: "width: 5px; color: blue", colwidth: [80] })),
      doc({ type: "table", attrs: { style: "width: 1px" }, content: [tableRow(cell({ colwidth: [80] }))] }),
      doc({ type: "table", attrs: { style: "" }, content: [tableRow(cell({ colwidth: [80] }))] }),
      doc({ type: "table", attrs: { style: 5 }, content: [tableRow(cell())] }),
      doc({ type: "table", attrs: { style: "color: blue; min-width: 3px" }, content: [tableRow(cell())] }),
    ]),
  ),
  ...documents("kit", [htmlDocs[1], htmlDocs[2]]),
  ...documents("partial", [doc(tableRow(cell(), header()))]),
].map(([name, doc]) => ({
  extensions: name,
  doc,
  ...outcome(() => {
    const { schema, serializer } = converters[name]
    const container = document.createElement("div")
    serializer.serializeFragment(Node.fromJSON(schema, doc).content, { document }, container)
    return { html: container.innerHTML }
  }),
}))

writeFixture("tiptap-tables", {
  schemas,
  parsesMarkdown,
  serializesMarkdown,
  parsesHTML,
  serializesHTML,
  queries,
})
