// Records the tokens marked's lexer makes of each input, with marked-more-lists' list tokenizer,
// to fixtures/marked.json, for tarnish-markdown's tests to expect. Each token records the fields
// `Token::to_json` writes, or the lexer's error. The version of marked is recorded too, for the
// tests to check that tarnish-markdown ports the one installed.
import { Marked } from "marked"
import moreLists from "marked-more-lists"
import { installed, outcome, writeFixture } from "./fixture.mjs"

const marked = new Marked()
marked.use(moreLists())

const inputs = [
  // Paragraphs and line breaks.
  "",
  "   ",
  "\n\n\n",
  "plain text",
  "one\ntwo",
  "one\n\ntwo",
  "one  \ntwo",
  "one\\\ntwo",
  "one\r\ntwo\rthree",
  "\ttabbed\tthrough",
  "  indented two",
  "trailing spaces   ",
  "héllo wörld 😀 中文 עברית",
  "a\u0000b",
  "a b",

  // Headings.
  "# one",
  "## two ##",
  "###### six",
  "####### seven",
  "#no space",
  "#",
  "# heading #  ",
  "#\tTab",
  "  ### indented",
  "    # code, not heading",
  "Setext one\n===",
  "Setext two\n---",
  "Setext\nover lines\n===",
  "## heading {#id}",
  "# *em* in heading",

  // Thematic breaks.
  "***",
  "---",
  "___",
  " * * *",
  "- - -",
  "_ _ _ _",
  "--",
  "text\n***\nmore",

  // Code.
  "    indented code",
  "    one\n\n    two",
  "```\nfenced\n```",
  "```js\nlet a = 1\n```",
  "~~~ruby startline=3\ncode\n~~~",
  "````\n```\nnested\n```\n````",
  "```\nunclosed",
  "``` \n  indented inside\n```",
  "  ```\n  shifted\n  ```",
  "`code`",
  "``code ` tick``",
  "` spaced `",
  "`` ` ``",
  "`unclosed",
  "a `b` c `d`",

  // Block quotes.
  "> quote",
  "> one\n> two",
  "> lazy\ncontinuation",
  "> > nested",
  ">>> deep",
  ">",
  "> # heading in quote",
  "> - list in quote\n> - two",
  "> ```\n> code in quote\n> ```",
  "> quote\n\nafter",

  // Lists.
  "- one\n- two",
  "* one\n* two",
  "+ one\n+ two",
  "- one\n\n- loose",
  "- one\n  continued",
  "- one\n\n  second paragraph",
  "- one\n  - nested\n    - deeper",
  "1. one\n2. two",
  "1) one\n2) two",
  "3. three\n4. four",
  "0. zero",
  "123456789. big",
  "1234567890. too big",
  "1. one\n1. one again",
  "7. seven\n9. skipped",
  "a. alpha\nb. beta",
  "A. upper\nB. alpha",
  "i. roman\nii. two\niii. three",
  "I. Roman\nII. Two",
  "c. starts at c",
  "iv. starts at four",
  "1. one\n   a. nested alpha\n      i. nested roman",
  "1. item 1\n2. item 2\n    a. item 2a\n        I.  sub item I\n        II. sub item II\n    e. item 2e\n7. item 7",
  "- [ ] task\n- [x] done\n- [X] upper",
  "- [ ]\n- [x]",
  "- [ ]not a task",
  "1. [ ] ordered task",
  "-\n  empty first line",
  "- ",
  "1.",
  "-one",
  "paragraph\n- interrupts",
  "paragraph\n2. doesn't interrupt",
  "- a\n+ b\n* c",
  "1. a\n2) b",
  "- a\n\n\n- b",
  "- a\n    code in item",
  "- # heading in item",
  "- > quote in item",
  "-     indented content",
  "10. ten\n    continued",

  // HTML.
  "<div>\nblock\n</div>",
  "<div>inline</div>",
  "<!-- comment -->",
  "<!-- open\ncomment -->",
  "<?php echo 1; ?>",
  "<!DOCTYPE html>",
  "<![CDATA[\nx\n]]>",
  "<script>\nlet a = '<p>'\n</script>",
  "<pre>\n  kept\n</pre>",
  "<style>p { color: red }</style>",
  "<custom-element>\nx\n</custom-element>",
  "a <b>bold</b> c",
  "a <span class=\"x\">span</span>",
  "a <br/> b",
  "<a href=\"x\">link</a>",
  "a <!-- inline comment --> b",
  "a < b > c",
  "<div\nbroken",

  // Tables.
  "| a | b |\n| - | - |\n| 1 | 2 |",
  "a | b\n--|--\n1 | 2",
  "| left | center | right |\n| :--- | :---: | ---: |\n| 1 | 2 | 3 |",
  "| a |\n| - |",
  "| a | b |\n| - | - |\n| only one |",
  "| a | b |\n| - | - |\n| 1 | 2 | 3 |",
  "| escaped \\| pipe | `code | pipe` |\n| - | - |",
  "| a | b |\n| - |",
  "| *em* | **strong** |\n| --- | --- |\n| [link](x) | ~~del~~ |",
  "table\n| a |\n| - |",

  // Emphasis.
  "*em*",
  "_em_",
  "**strong**",
  "__strong__",
  "***both***",
  "*em **strong** em*",
  "**strong *em* strong**",
  "intra*word*emphasis",
  "intra_word_underscores",
  "*unclosed",
  "**unclosed",
  "* not em *",
  "*a **b* c**",
  "**a *b** c*",
  "_a *b_ c*",
  "*a*b*c*",
  "****",
  "__a__b",
  "*(*foo*)*",
  "**foo \"*bar*\" foo**",

  // Strikethrough.
  "~one~",
  "~~two~~",
  "~~~three~~~",
  "~~a ~b~ c~~",
  "a~~b~~c",
  "~~unclosed",

  // Links and images.
  "[link](http://example.com)",
  "[link](http://example.com \"title\")",
  "[link](http://example.com 'title')",
  "[link](http://example.com (title))",
  "[link](<with spaces> \"t\")",
  "[link](a(b)c)",
  "[link]()",
  "[link](  spaced  )",
  "[link](\\(escaped\\))",
  "[a [nested] b](x)",
  "[a](x) [b](y)",
  "[*em* link](x)",
  "![image](a.png)",
  "![image](a.png \"title\")",
  "![](empty.png)",
  "[![image](a.png)](link)",
  "[full][ref]\n\n[ref]: http://x.com",
  "[collapsed][]\n\n[collapsed]: http://x.com \"T\"",
  "[shortcut]\n\n[shortcut]: <http://x.com> 'T'",
  "[missing][nope]",
  "[ref]: http://x.com\n[REF]: http://y.com\n\n[Ref]",
  "[ref]:\n  http://x.com\n  \"title on its own line\"",
  "[ref]: http://x.com \"unclosed",
  "[unclosed(x)",
  "[link](x",

  // Autolinks.
  "<http://example.com>",
  "<mailto:a@b.com>",
  "<a@b.com>",
  "http://example.com/path?q=1",
  "www.example.com",
  "https://example.com.",
  "http://example.com/a_(b)",
  "visit https://x.com/y, then",
  "a@b.com",
  "not@an@address",
  "<not an autolink>",

  // Escapes and entities.
  "\\*not em\\*",
  "\\\\ backslash",
  "\\# not heading",
  "\\- not list",
  "\\`not code\\`",
  "\\[not link\\](x)",
  "&amp; &lt; &gt; &quot;",
  "&copy; &nbsp; &#123; &#x1F600;",
  "&unknown; &#0; &#xD800;",
  "AT&T",
  "5 < 6 > 4",

  // Everything together.
  "# Title\n\nIntro with **bold**, *em*, `code` and [a link](x).\n\n- one\n- two\n  1. nested\n\n> quote\n\n```\ncode\n```\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\n---\n\nThe end.",
]

function project(token) {
  const out = { type: token.type, raw: token.raw }
  if (typeof token.text === "string") out.text = token.text
  if (Array.isArray(token.tokens)) out.tokens = token.tokens.map(project)
  if (token.type === "heading") out.depth = token.depth
  if (token.type === "link" || token.type === "image") {
    out.href = token.href
    out.title = token.title ?? null
  }
  if (token.type === "html") out.block = !!token.block
  if (token.type === "def") {
    out.tag = token.tag
    out.href = token.href
    out.title = token.title ?? null
  }
  if (token.type === "list") {
    out.ordered = token.ordered
    out.start = token.start ?? null
    out.loose = token.loose
    out.items = token.items.map(project)
  }
  if (token.type === "list_item") {
    out.task = token.task
    out.checked = token.checked ?? null
    out.loose = token.loose
  }
  if (token.type === "table") {
    const cell = cell => ({ tokens: cell.tokens.map(project) })
    out.header = token.header.map(cell)
    out.rows = token.rows.map(row => row.map(cell))
  }
  return out
}

const lexed = inputs.map(input => ({ input, ...outcome(() => ({ tokens: marked.lexer(input).map(project) })) }))
writeFixture("marked", { marked: installed("marked"), lexed })
