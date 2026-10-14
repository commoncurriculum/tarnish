// Records @tiptap/extension-table's Table, TableRow, TableCell and TableHeader to
// fixtures/tiptap-tables.json, as harness/tiptap.mjs records them: `tables`, as they come beside
// Document, Paragraph, Text, HardBreak, Bold, Italic, Heading and an application's code mark that
// parses marked's code spans; `attributes`, `attributeValues`, `wrapper`, `minWidth` and `zero`,
// the same with an option of their renderHTML set; `global`, with an application's global `style`
// attribute on tables and cells, and `both`, with Table's `HTMLAttributes` too; `kit`, TableKit
// with a minimum cell width; and `partial`, rows and cells with no Table. Table's other options
// reach only an editor's plugins and node view. `queries` records the `width` of each element
// each table's `querySelectorAll("colgroup > col")` finds, which TableCell's colwidth reads when
// the cell has none of its own.
import { Extension, Mark } from "@tiptap/core"
import { Bold } from "@tiptap/extension-bold"
import { Document } from "@tiptap/extension-document"
import { HardBreak } from "@tiptap/extension-hard-break"
import { Heading } from "@tiptap/extension-heading"
import { Italic } from "@tiptap/extension-italic"
import { Paragraph } from "@tiptap/extension-paragraph"
import { Table, TableCell, TableHeader, TableKit, TableRow } from "@tiptap/extension-table"
import { Text } from "@tiptap/extension-text"
import {
  body,
  doc,
  domInputs,
  markedInputs,
  otherAttributes,
  paragraph,
  record,
  text,
  unique,
  withParsed,
} from "./tiptap.mjs"

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

// A table `depth` deep in a cell of a table, around a paragraph's text.
const nestedTable = depth => {
  let node = paragraph(text("x"))
  for (let level = 0; level < depth; level++) {
    node = { type: "table", content: [{ type: "tableRow", content: [{ type: "tableCell", content: [node] }] }] }
  }
  return doc(node)
}
// Nesting one to three deep, which tests/recorded/tables.rs folds into the conversions of any
// depth.
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

const cellOf = type => (attrs, ...content) => ({
  type,
  ...(attrs ? { attrs } : {}),
  content: content.length ? content : [paragraph(text("x"))],
})
const cell = cellOf("tableCell")
const header = cellOf("tableHeader")
const tableRow = (...cells) => ({ type: "tableRow", content: cells })
const tableOf = (...rows) => doc({ type: "table", content: rows })
const firstRow = (...cells) => tableOf(tableRow(...cells), tableRow(cell()))
// A table with a header row, and one whose cells have widths.
const headed = tableOf(
  tableRow(header(null, paragraph(text("h"))), header()),
  tableRow(cell(null, paragraph(text("a"))), cell()),
)
const sized = firstRow(cell({ colwidth: [100] }), cell({ colwidth: [50] }))
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
  headed,
  sized,
  firstRow(cell({ colwidth: [100] }), cell()),
  firstRow(cell({ colspan: 2, colwidth: [100, 200] }), cell({ colwidth: [30] })),
  firstRow(cell({ colspan: 2, colwidth: [100] })),
  firstRow(cell({ colspan: 3 }), header({ colspan: 2, colwidth: [10, 60] })),
  ...widths.map(colwidth => firstRow(cell({ colwidth }), cell({ colwidth: [40] }))),
  ...spans.map(colspan => firstRow(cell({ colspan, colwidth: [20, 30, 40] }), cell({ colwidth: [5] }))),
  tableOf(tableRow(cell({ colwidth: [100] })), tableRow(cell(), cell({ colwidth: [999] }))),
  ...["left", "right", "center", "justify", "", null, 1, true].map(align => firstRow(cell({ align }), header({ align }))),
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
// A table of headers holding what Markdown has to fit on one line.
const longerHeader = tableOf(tableRow(header(), header(null, paragraph(text("longer header")))), tableRow(cell(), cell()))
// Cells holding what Markdown has to fit on one line, and tables of every shape.
const markdownDocs = [
  longerHeader,
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
].map(content => doc({ type: "table", ...(content === undefined ? {} : { content }) }))
// Tables and cells with an application's style.
const styledHTML = [
  table(row('<td style="color: red; text-align: center">a</td>', '<th style="padding: 1px">b</th>')),
  '<table style="width: 10px"><tr><td>a</td></tr></table>',
  table(row('<td style="">a</td>')),
]
const styledDocs = [
  firstRow(cell({ style: "color: red" }), cell({ style: "text-align: left", align: "right" })),
  firstRow(header({ style: "width: 5px; color: blue", colwidth: [80] })),
  doc({ type: "table", attrs: { style: "width: 1px" }, content: [tableRow(cell({ colwidth: [80] }))] }),
  doc({ type: "table", attrs: { style: "" }, content: [tableRow(cell({ colwidth: [80] }))] }),
  doc({ type: "table", attrs: { style: 5 }, content: [tableRow(cell())] }),
  doc({ type: "table", attrs: { style: "color: blue; min-width: 3px" }, content: [tableRow(cell())] }),
]

// Table's options and HTMLAttributes, which reach only renderHTML.
const rendering = extensions => ({ extensions, serializesHTML: htmlDocs })

record(
  "tiptap-tables",
  {
    tables: {
      extensions: tables(),
      parsesMarkdown: ["", "plain", ...markdownTables, ...markedInputs(), ...deepMarkdown],
      parsesHTML: ["<p>plain</p>", ...htmlTables, ...domInputs(), ...deepHTML],
      serializesMarkdown: withParsed([...markdownDocs, ...malformedDocs, ...htmlDocs]),
      serializesHTML: withParsed([...htmlDocs, ...markdownDocs]),
    },
    attributes: rendering(
      tables(
        Table.configure({ HTMLAttributes: { class: "t", style: "color: red; min-width: 1px" } }),
        TableRow.configure({ HTMLAttributes: { class: "r" } }),
        TableCell.configure({ HTMLAttributes: { class: "c", style: "padding: 1px" } }),
        TableHeader.configure({ HTMLAttributes: { class: "h", "data-x": "y" } }),
      ),
    ),
    attributeValues: rendering(
      tables(
        Table.configure({ HTMLAttributes: otherAttributes }),
        TableRow.configure({ HTMLAttributes: otherAttributes }),
        TableCell.configure({ HTMLAttributes: otherAttributes }),
        TableHeader.configure({ HTMLAttributes: otherAttributes }),
      ),
    ),
    wrapper: rendering(tables(Table.configure({ renderWrapper: true }))),
    minWidth: rendering(tables(Table.configure({ cellMinWidth: 12.5 }))),
    zero: rendering(tables(Table.configure({ cellMinWidth: 0 }))),
    // The style reaches only the tables and cells that have one.
    global: { extensions: [...tables(), Style], parsesHTML: styledHTML, serializesHTML: withParsed(styledDocs) },
    both: {
      extensions: [...tables(Table.configure({ HTMLAttributes: { class: "b", style: "color: red; width: 9px" } })), Style],
      serializesHTML: styledDocs,
    },
    // The kit's minimum cell width reaches only renderHTML.
    kit: { extensions: [...inline, TableKit.configure({ table: { cellMinWidth: 30 } })], serializesHTML: [headed, sized] },
    partial: {
      extensions: [Document, Paragraph, Text, TableRow, TableCell, TableHeader],
      parsesMarkdown: ["| a | b |\n| - | - |\n| 1 | 2 |", "| `a|b` |\n| - |", "a|b|c\n-|-"],
      parsesHTML: [table(row("<td>a</td>", "<th></th>")), "<tr><td>x</td></tr>", "<p>p</p>"],
      serializesMarkdown: withParsed([longerHeader]),
      serializesHTML: withParsed([doc(tableRow(cell(), header()))]),
    },
  },
  {
    queries: unique(htmlTables).map(html => ({
      html,
      tables: [...body(html).querySelectorAll("table")].map(table =>
        [...table.querySelectorAll("colgroup > col")].map(col => col.getAttribute("width")),
      ),
    })),
  },
)
